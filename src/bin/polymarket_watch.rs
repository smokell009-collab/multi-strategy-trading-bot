use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use clap::{Parser, ValueEnum};
use ode_domain::{
    Candle, DataQuality, Direction, MarketId, MarketSnapshot, PaperPortfolioState,
    PortfolioSnapshot, StrategyId, TradeSignal,
};
use ode_events::AuditEvent;
use ode_observability::AuditStore;
use ode_polymarket::{
    FeeSchedule, Market, MarketFeedEvent, OrderBook, Outcome, PolymarketClient, PriceHistoryPoint,
    TradeTick, run_market_stream,
};
use ode_risk::{KillSwitch, RiskEngine, RiskInput, RiskLimits, RiskPolicy};
use ode_strategy_technical::{EmaCrossover, PriceRsiDivergence, StrategiesConfig, Strategy};
use rust_decimal::{Decimal, RoundingStrategy, prelude::ToPrimitive};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    time::Duration,
};
use tokio::sync::{mpsc, watch};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutcomeArg {
    Yes,
    No,
}

impl From<OutcomeArg> for Outcome {
    fn from(value: OutcomeArg) -> Self {
        match value {
            OutcomeArg::Yes => Self::Yes,
            OutcomeArg::No => Self::No,
        }
    }
}

fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Buy => "Buy",
        Direction::Sell => "Sell",
        Direction::Hold => "Hold",
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum StrategyArg {
    #[value(name = "both")]
    Both,
    #[value(name = "ema-crossover")]
    EmaCrossover,
    #[value(name = "price-rsi-divergence")]
    PriceRsiDivergence,
}

#[derive(Debug, Parser)]
#[command(
    name = "polymarket-watch",
    about = "Monitor público Polymarket + simulação paper pUSD; nunca envia ordens"
)]
struct Args {
    /// Slug de um mercado binário Polymarket; não faz busca nem escolhe mercado automaticamente.
    #[arg(long)]
    slug: String,
    /// Qual outcome binário observar e simular.
    #[arg(long, value_enum, default_value_t = OutcomeArg::Yes)]
    outcome: OutcomeArg,
    /// Estratégia(s) a comparar em contas paper independentes.
    #[arg(long, value_enum, default_value_t = StrategyArg::Both)]
    strategy: StrategyArg,
    /// Cadência dos candles do outcome: 1m, 5m, 15m, 30m, 1h ou 4h.
    #[arg(long, default_value = "1m")]
    interval: String,
    #[arg(long, default_value = "configs/polymarket-strategies.example.toml")]
    strategy_config: PathBuf,
    #[arg(long, default_value = "configs/risk-overrides.example.toml")]
    risk_overrides: PathBuf,
    #[arg(long, default_value = "data/polymarket-paper.sqlite3")]
    audit_db: PathBuf,
    #[arg(long, default_value = "data/KILL_SWITCH.json")]
    kill_switch_file: PathBuf,
    /// Saldo inicial de cada carteira virtual independente, em pUSD.
    #[arg(long, default_value = "1000")]
    starting_balance_pusd: Decimal,
    #[arg(long, default_value = "0.01")]
    risk_per_trade: Decimal,
    #[arg(long, default_value = "0.05")]
    max_daily_loss: Decimal,
    #[arg(long, default_value = "0.25")]
    max_allocation: Decimal,
    #[arg(long, default_value = "0.05")]
    stop_loss: Decimal,
    #[arg(long, default_value_t = 50)]
    max_slippage_bps: u32,
    #[arg(long, default_value_t = 10_000)]
    max_data_age_ms: i64,
    /// Consulta mercado+book e encerra; não conecta ao WebSocket e não simula fills.
    #[arg(long, default_value_t = false)]
    once: bool,
    /// Encerra após N segundos (útil para testes); zero mantém execução até Ctrl+C.
    #[arg(long, default_value_t = 0)]
    max_runtime_seconds: u64,
}

#[derive(Debug, Clone, Copy)]
enum FeeModel {
    Free,
    Curve { rate: Decimal },
    Unsupported,
}

impl FeeModel {
    fn from_schedule(schedule: &FeeSchedule) -> Self {
        match schedule.enabled {
            Some(false) => Self::Free,
            Some(true) => {
                let Some(rate) = schedule.rate else {
                    return Self::Unsupported;
                };
                if rate == Decimal::ZERO {
                    return Self::Free;
                }
                if rate < Decimal::ZERO
                    || rate > Decimal::new(10, 2)
                    || schedule.exponent != Some(1)
                {
                    return Self::Unsupported;
                }
                Self::Curve { rate }
            }
            None => Self::Unsupported,
        }
    }

    /// Documented current Polymarket fee curve for exponent=1: C × rate × p × (1-p).
    fn fee(self, shares: Decimal, price: Decimal) -> Option<Decimal> {
        match self {
            Self::Free => Some(Decimal::ZERO),
            Self::Curve { rate } => Some(
                (shares * rate * price * (Decimal::ONE - price))
                    .round_dp_with_strategy(5, RoundingStrategy::MidpointAwayFromZero),
            ),
            Self::Unsupported => None,
        }
    }

    fn conservative_fee_bps(self) -> u32 {
        match self {
            Self::Free => 0,
            Self::Curve { rate } => (rate * Decimal::from(10_000))
                .ceil()
                .to_u32()
                .unwrap_or(1_000)
                .min(1_000),
            Self::Unsupported => 1_000,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Free => "sem taxa segundo Gamma",
            Self::Curve { .. } => "taxa taker estimada pela feeSchedule do Gamma (expoente 1)",
            Self::Unsupported => "feeSchedule ausente/incompatível: paper fills bloqueados",
        }
    }
}

#[derive(Debug, Clone)]
struct Fill {
    shares: Decimal,
    notional: Decimal,
    average_price: Decimal,
    fee: Decimal,
    slippage_bps: u32,
    levels: usize,
}

#[derive(Debug)]
struct PaperAccount {
    account_key: String,
    symbol: String,
    interval: String,
    starting_balance: Decimal,
    cash: Decimal,
    quantity: Decimal,
    entry_price: Option<Decimal>,
    stop_price: Option<Decimal>,
    entry_fee: Decimal,
    entry_fee_bps: u32,
    position_slippage_bps: u32,
    session_start_equity: Decimal,
    session_date_utc: String,
    last_processed_close_ms: Option<i64>,
    last_mark_price: Option<Decimal>,
    last_signal: String,
    store: AuditStore,
}

impl PaperAccount {
    fn load_or_create(
        store: AuditStore,
        account_key: String,
        symbol: String,
        interval: String,
        starting_balance: Decimal,
    ) -> Result<Self> {
        if let Some(state) = store.load_paper_state(&account_key)? {
            if state.symbol != symbol || state.interval != interval {
                bail!("snapshot paper existente não corresponde ao mercado/intervalo selecionado");
            }
            if state.starting_balance != starting_balance {
                eprintln!(
                    "WARN saldo inicial CLI difere do snapshot persistido; saldo existente será mantido para {}",
                    account_key
                );
            }
            return Ok(Self::from_state(store, state));
        }
        let now_ms = unix_ms();
        let date = utc_date(now_ms);
        let state = PaperPortfolioState {
            schema_version: 1,
            account_key: account_key.clone(),
            symbol: symbol.clone(),
            interval: interval.clone(),
            starting_balance,
            cash: starting_balance,
            quantity: Decimal::ZERO,
            entry_price: None,
            stop_price: None,
            entry_fee: Decimal::ZERO,
            entry_fee_bps: 0,
            position_slippage_bps: 0,
            session_start_equity: starting_balance,
            session_date_utc: date.clone(),
            last_processed_close_ms: None,
            last_mark_price: None,
            last_signal: "Hold".to_owned(),
            updated_at_ms: now_ms,
        };
        store.save_paper_state(&state)?;
        Ok(Self::from_state(store, state))
    }

    fn from_state(store: AuditStore, state: PaperPortfolioState) -> Self {
        Self {
            account_key: state.account_key,
            symbol: state.symbol,
            interval: state.interval,
            starting_balance: state.starting_balance,
            cash: state.cash,
            quantity: state.quantity,
            entry_price: state.entry_price,
            stop_price: state.stop_price,
            entry_fee: state.entry_fee,
            entry_fee_bps: state.entry_fee_bps,
            position_slippage_bps: state.position_slippage_bps,
            session_start_equity: state.session_start_equity,
            session_date_utc: state.session_date_utc,
            last_processed_close_ms: state.last_processed_close_ms,
            last_mark_price: state.last_mark_price,
            last_signal: state.last_signal,
            store,
        }
    }

    fn to_state(&self, now_ms: i64) -> PaperPortfolioState {
        PaperPortfolioState {
            schema_version: 1,
            account_key: self.account_key.clone(),
            symbol: self.symbol.clone(),
            interval: self.interval.clone(),
            starting_balance: self.starting_balance,
            cash: self.cash,
            quantity: self.quantity,
            entry_price: self.entry_price,
            stop_price: self.stop_price,
            entry_fee: self.entry_fee,
            entry_fee_bps: self.entry_fee_bps,
            position_slippage_bps: self.position_slippage_bps,
            session_start_equity: self.session_start_equity,
            session_date_utc: self.session_date_utc.clone(),
            last_processed_close_ms: self.last_processed_close_ms,
            last_mark_price: self.last_mark_price,
            last_signal: self.last_signal.clone(),
            updated_at_ms: now_ms,
        }
    }

    fn equity(&self, mark_price: Decimal) -> Decimal {
        self.cash + self.quantity * mark_price
    }

    fn portfolio_snapshot(&self, mark_price: Decimal, now_ms: i64) -> PortfolioSnapshot {
        let current_equity = self.equity(mark_price);
        let daily_start_equity = if self.session_date_utc == utc_date(now_ms) {
            self.session_start_equity
        } else {
            current_equity
        };
        PortfolioSnapshot {
            equity: current_equity,
            cash: self.cash,
            daily_start_equity,
            open_exposure: self.quantity * mark_price,
            has_open_position: self.quantity > Decimal::ZERO,
        }
    }

    fn roll_session(&mut self, mark_price: Decimal, now_ms: i64) {
        let today = utc_date(now_ms);
        if self.session_date_utc != today {
            self.session_start_equity = self.equity(mark_price);
            self.session_date_utc = today;
        }
        self.last_mark_price = Some(mark_price);
    }

    fn persist_event(
        &mut self,
        event_type: &str,
        details: Value,
        now_ms: i64,
        atomic_fill: bool,
    ) -> Result<()> {
        let event = AuditEvent::new(event_type, MarketId::from(self.symbol.clone()), details);
        let state = self.to_state(now_ms);
        if atomic_fill {
            self.store.commit_paper_event(&event, &state)?;
        } else {
            self.store.append_event(&event)?;
            self.store.save_paper_state(&state)?;
        }
        Ok(())
    }

    fn status_json(&self, mark: Option<Decimal>) -> Value {
        let mark = mark.or(self.last_mark_price).unwrap_or(Decimal::ZERO);
        json!({
            "strategy_account": self.account_key,
            "quote_currency": "pUSD",
            "cash_pusd": self.cash.to_string(),
            "position_shares": self.quantity.to_string(),
            "entry_price_pusd": self.entry_price.map(|value| value.to_string()),
            "mark_bid_pusd": mark.to_string(),
            "equity_pusd": self.equity(mark).to_string(),
            "last_signal": self.last_signal,
        })
    }
}

struct StrategyRuntime {
    strategy: Box<dyn Strategy>,
    risk: RiskEngine,
    account: PaperAccount,
}

struct CandleAggregator {
    interval_ms: i64,
    history: VecDeque<Candle>,
    current: Option<Candle>,
}

impl CandleAggregator {
    fn new(interval_ms: i64, history: Vec<Candle>) -> Self {
        Self {
            interval_ms,
            history: history.into_iter().collect(),
            current: None,
        }
    }

    fn history(&self) -> Vec<Candle> {
        self.history.iter().cloned().collect()
    }

    fn ingest_trade(&mut self, tick: &TradeTick) -> Vec<Candle> {
        let mut closed = Vec::new();
        let open_time = tick.timestamp_ms.div_euclid(self.interval_ms) * self.interval_ms;
        if tick.size <= Decimal::ZERO || tick.timestamp_ms <= 0 {
            return closed;
        }
        if self.current.is_none()
            && self
                .history
                .back()
                .is_some_and(|last| open_time <= last.open_time_ms)
        {
            return closed;
        }
        match self.current.as_mut() {
            None => self.current = Some(new_candle(open_time, tick)),
            Some(current) if open_time == current.open_time_ms => {
                current.high = current.high.max(tick.price);
                current.low = current.low.min(tick.price);
                current.close = tick.price;
                current.volume += tick.size;
                current.received_at_ms = Some(unix_ms());
            }
            Some(current) if open_time > current.open_time_ms => {
                let previous = self.current.take().expect("current candle checked above");
                let had_gap = open_time != previous.open_time_ms + self.interval_ms;
                if had_gap {
                    self.history.clear();
                }
                self.push_closed(previous.clone());
                closed.push(previous);
                self.current = Some(new_candle(open_time, tick));
            }
            Some(_) => {}
        }
        closed
    }

    fn flush_due(&mut self, now_ms: i64) -> Vec<Candle> {
        let Some(current) = self.current.as_ref() else {
            return Vec::new();
        };
        if now_ms < current.open_time_ms + self.interval_ms {
            return Vec::new();
        }
        let candle = self.current.take().expect("current candle checked above");
        self.push_closed(candle.clone());
        vec![candle]
    }

    fn push_closed(&mut self, mut candle: Candle) {
        candle.close_time_ms = candle.open_time_ms + self.interval_ms - 1;
        if self
            .history
            .back()
            .is_some_and(|last| last.open_time_ms + self.interval_ms != candle.open_time_ms)
        {
            self.history.clear();
        }
        if self
            .history
            .back()
            .is_some_and(|last| last.open_time_ms == candle.open_time_ms)
        {
            self.history.pop_back();
        }
        self.history.push_back(candle);
        while self.history.len() > 2_000 {
            self.history.pop_front();
        }
    }
}

fn new_candle(open_time_ms: i64, tick: &TradeTick) -> Candle {
    Candle {
        open_time_ms,
        close_time_ms: open_time_ms,
        open: tick.price,
        high: tick.price,
        low: tick.price,
        close: tick.price,
        volume: tick.size,
        received_at_ms: Some(unix_ms()),
    }
}

fn interval_millis(interval: &str) -> Result<(i64, u64)> {
    let seconds = match interval {
        "1m" => 60,
        "5m" => 300,
        "15m" => 900,
        "30m" => 1_800,
        "1h" => 3_600,
        "4h" => 14_400,
        _ => bail!("interval precisa ser 1m, 5m, 15m, 30m, 1h ou 4h"),
    };
    Ok((seconds * 1_000, seconds as u64))
}

fn history_to_candles(
    points: Vec<PriceHistoryPoint>,
    interval_ms: i64,
    interval_seconds: u64,
    now_ms: i64,
) -> Vec<Candle> {
    let completed_boundary = now_ms.div_euclid(interval_ms) * interval_ms;
    let mut by_bucket = BTreeMap::new();
    for point in points {
        if point.resolution_seconds != interval_seconds
            || point.timestamp_ms <= 0
            || point.timestamp_ms >= completed_boundary
        {
            continue;
        }
        let open_time_ms = point.timestamp_ms.div_euclid(interval_ms) * interval_ms;
        if open_time_ms + interval_ms > completed_boundary {
            continue;
        }
        by_bucket.insert(
            open_time_ms,
            Candle {
                open_time_ms,
                close_time_ms: open_time_ms + interval_ms - 1,
                open: point.price,
                high: point.price,
                low: point.price,
                close: point.price,
                volume: Decimal::ZERO,
                received_at_ms: None,
            },
        );
    }
    let candles: Vec<_> = by_bucket.into_values().collect();
    let mut suffix_reversed = Vec::new();
    let mut next_open = None;
    for candle in candles.into_iter().rev() {
        if next_open.is_some_and(|expected| candle.open_time_ms + interval_ms != expected) {
            break;
        }
        next_open = Some(candle.open_time_ms);
        suffix_reversed.push(candle);
    }
    suffix_reversed.reverse();
    if suffix_reversed.len() > 2_000 {
        suffix_reversed.drain(..suffix_reversed.len() - 2_000);
    }
    suffix_reversed
}

fn build_strategies(
    config: &StrategiesConfig,
    selection: StrategyArg,
) -> Result<Vec<Box<dyn Strategy>>> {
    let mut strategies: Vec<Box<dyn Strategy>> = Vec::new();
    if matches!(selection, StrategyArg::Both | StrategyArg::EmaCrossover) {
        let strategy = EmaCrossover::new(config.ema_crossover.clone())?;
        if !strategy.enabled() {
            bail!("EMA está desabilitada no arquivo de estratégia");
        }
        strategies.push(Box::new(strategy));
    }
    if matches!(
        selection,
        StrategyArg::Both | StrategyArg::PriceRsiDivergence
    ) {
        let strategy = PriceRsiDivergence::new(config.price_rsi_divergence.clone())?;
        if !strategy.enabled() {
            bail!(
                "price_rsi_divergence está desabilitada; habilite-a no arquivo Polymarket de estratégias"
            );
        }
        strategies.push(Box::new(strategy));
    }
    if strategies.is_empty() {
        bail!("nenhuma estratégia habilitada para simulação");
    }
    Ok(strategies)
}

struct RuntimeSetup<'a> {
    market: &'a Market,
    outcome: Outcome,
    interval: &'a str,
    account_symbol: &'a str,
    starting_balance: Decimal,
    args: &'a Args,
    fee_model: FeeModel,
    store: &'a AuditStore,
    policy: &'a RiskPolicy,
}

fn create_runtimes(
    strategies: Vec<Box<dyn Strategy>>,
    setup: RuntimeSetup<'_>,
) -> Result<Vec<StrategyRuntime>> {
    let RuntimeSetup {
        market,
        outcome,
        interval,
        account_symbol,
        starting_balance,
        args,
        fee_model,
        store,
        policy,
    } = setup;
    let mut runtimes = Vec::new();
    for strategy in strategies {
        let account_key = format!(
            "polymarket:paper:pUSD:{}:{}:{}:{}",
            market.slug,
            outcome.as_str().to_ascii_lowercase(),
            interval,
            strategy.id()
        );
        let account = PaperAccount::load_or_create(
            store.clone(),
            account_key,
            account_symbol.to_owned(),
            interval.to_owned(),
            starting_balance,
        )?;
        let limits = RiskLimits {
            risk_per_trade: args.risk_per_trade,
            max_daily_loss: args.max_daily_loss,
            max_allocation: args.max_allocation,
            stop_loss: args.stop_loss,
            fee_bps: fee_model.conservative_fee_bps(),
            max_slippage_bps: args.max_slippage_bps,
            min_expected_edge_bps: 0,
            max_market_data_age_ms: args.max_data_age_ms,
        };
        let risk = RiskEngine::new(limits, policy.clone())?;
        runtimes.push(StrategyRuntime {
            strategy,
            risk,
            account,
        });
    }
    Ok(runtimes)
}

fn market_id_text(market: &Market, outcome: Outcome, asset_id: &str) -> String {
    format!(
        "POLYMARKET:{}:{}:{}:{}",
        market.condition_id,
        market.slug,
        outcome.as_str(),
        asset_id
    )
}

fn fee_model_from_market(market: &Market) -> FeeModel {
    FeeModel::from_schedule(&market.fees)
}

fn simulate_buy(
    book: &OrderBook,
    wanted_shares: Decimal,
    cash: Decimal,
    max_slippage_bps: u32,
    fee_model: FeeModel,
) -> Option<Fill> {
    let best_ask = book.best_ask()?.price;
    let max_price =
        best_ask * (Decimal::ONE + Decimal::from(max_slippage_bps) / Decimal::from(10_000));
    let mut remaining = wanted_shares;
    let mut shares = Decimal::ZERO;
    let mut notional = Decimal::ZERO;
    let mut levels = 0;
    for level in &book.asks {
        if level.price > max_price || remaining <= Decimal::ZERO {
            break;
        }
        let fee_per_share = fee_model.fee(Decimal::ONE, level.price)?;
        let affordable = cash / (level.price + fee_per_share);
        let fill_shares = remaining.min(level.size).min(affordable);
        if fill_shares <= Decimal::ZERO {
            break;
        }
        remaining -= fill_shares;
        shares += fill_shares;
        notional += fill_shares * level.price;
        levels += 1;
    }
    if shares <= Decimal::ZERO {
        return None;
    }
    let average_price = notional / shares;
    let fee = fee_model.fee(shares, average_price)?;
    if notional + fee > cash {
        return None;
    }
    let slippage = ((average_price - best_ask).max(Decimal::ZERO) / best_ask
        * Decimal::from(10_000))
    .ceil()
    .to_u32()
    .unwrap_or(u32::MAX);
    Some(Fill {
        shares,
        notional,
        average_price,
        fee,
        slippage_bps: slippage,
        levels,
    })
}

fn simulate_sell(
    book: &OrderBook,
    wanted_shares: Decimal,
    max_slippage_bps: u32,
    fee_model: FeeModel,
) -> Option<Fill> {
    let best_bid = book.best_bid()?.price;
    let min_price =
        best_bid * (Decimal::ONE - Decimal::from(max_slippage_bps) / Decimal::from(10_000));
    let mut remaining = wanted_shares;
    let mut shares = Decimal::ZERO;
    let mut notional = Decimal::ZERO;
    let mut levels = 0;
    for level in book.bids.iter().rev() {
        if level.price < min_price || remaining <= Decimal::ZERO {
            break;
        }
        let fill_shares = remaining.min(level.size);
        if fill_shares <= Decimal::ZERO {
            continue;
        }
        remaining -= fill_shares;
        shares += fill_shares;
        notional += fill_shares * level.price;
        levels += 1;
    }
    if shares <= Decimal::ZERO {
        return None;
    }
    let average_price = notional / shares;
    let fee = fee_model.fee(shares, average_price)?;
    let slippage = ((best_bid - average_price).max(Decimal::ZERO) / best_bid
        * Decimal::from(10_000))
    .ceil()
    .to_u32()
    .unwrap_or(u32::MAX);
    Some(Fill {
        shares,
        notional,
        average_price,
        fee,
        slippage_bps: slippage,
        levels,
    })
}

struct CandleProcessContext<'a> {
    candle: &'a Candle,
    history: &'a [Candle],
    book: Option<&'a OrderBook>,
    book_received_at_ms: Option<i64>,
    market: &'a Market,
    outcome: Outcome,
    args: &'a Args,
    fee_model: FeeModel,
    kill_switch: &'a KillSwitch,
}

fn process_closed_candle(
    runtime: &mut StrategyRuntime,
    context: CandleProcessContext<'_>,
) -> Result<()> {
    let CandleProcessContext {
        candle,
        history,
        book,
        book_received_at_ms,
        market,
        outcome,
        args,
        fee_model,
        kill_switch,
    } = context;
    let now_ms = unix_ms();
    if runtime
        .account
        .last_processed_close_ms
        .is_some_and(|last| candle.close_time_ms <= last)
    {
        return Ok(());
    }
    let direction = runtime.strategy.evaluate(history);
    let Some(book) = book.filter(|book| book.is_valid()) else {
        runtime.account.last_processed_close_ms = Some(candle.close_time_ms);
        runtime.account.last_signal = direction_name(direction).to_owned();
        runtime.account.persist_event("polymarket_signal", json!({
            "mode":"paper","decision":format!("{:?}", direction),"reason":"sem book válido",
            "strategy_id":runtime.strategy.id(),"strategy_version":runtime.strategy.version(),
            "quote_currency":"pUSD","synthetic_candle":true,"candle_close_ms":candle.close_time_ms
        }), now_ms, false)?;
        return Ok(());
    };
    let received_at = book_received_at_ms.unwrap_or_default();
    let book_fresh =
        received_at > 0 && now_ms >= received_at && now_ms - received_at <= args.max_data_age_ms;
    let Some(best_bid) = book.best_bid().map(|level| level.price) else {
        return Ok(());
    };
    let Some(best_ask) = book.best_ask().map(|level| level.price) else {
        return Ok(());
    };
    runtime.account.roll_session(best_bid, now_ms);
    let market_id = MarketId::from(runtime.account.symbol.clone());
    let should_stop = runtime.account.quantity > Decimal::ZERO
        && runtime
            .account
            .stop_price
            .is_some_and(|stop| best_bid <= stop);
    let should_exit =
        runtime.account.quantity > Decimal::ZERO && (direction == Direction::Sell || should_stop);
    let reason = if should_stop {
        "stop_loss"
    } else {
        "strategy_sell"
    };

    if should_exit {
        if !book_fresh || matches!(fee_model, FeeModel::Unsupported) {
            runtime.account.last_processed_close_ms = Some(candle.close_time_ms);
            runtime.account.last_signal = "Sell".to_owned();
            runtime.account.persist_event(
                "polymarket_paper_exit_blocked",
                json!({
                    "reason":reason,"book_fresh":book_fresh,"fee_model":fee_model.description(),
                    "strategy_id":runtime.strategy.id(),"quote_currency":"pUSD"
                }),
                now_ms,
                false,
            )?;
            return Ok(());
        }
        if let Some(fill) = simulate_sell(
            book,
            runtime.account.quantity,
            args.max_slippage_bps,
            fee_model,
        ) {
            let min_order_size = book
                .min_order_size
                .or(market.min_order_size)
                .unwrap_or(Decimal::ZERO);
            if fill.notional < min_order_size {
                runtime.account.last_processed_close_ms = Some(candle.close_time_ms);
                runtime.account.last_signal = "Sell".to_owned();
                runtime.account.persist_event(
                    "polymarket_paper_exit_blocked",
                    json!({
                        "reason":"fill abaixo do mínimo do mercado","notional_pusd":fill.notional.to_string(),
                        "minimum_order_size_pusd":min_order_size.to_string(),"strategy_id":runtime.strategy.id()
                    }),
                    now_ms,
                    false,
                )?;
                return Ok(());
            }
            let old_quantity = runtime.account.quantity;
            let sold_quantity = fill.shares.min(old_quantity);
            let entry_fee_allocated = if old_quantity > Decimal::ZERO {
                runtime.account.entry_fee * sold_quantity / old_quantity
            } else {
                Decimal::ZERO
            };
            let entry_cost = runtime.account.entry_price.unwrap_or_default() * sold_quantity
                + entry_fee_allocated;
            let realized_pnl = fill.notional - fill.fee - entry_cost;
            runtime.account.cash += fill.notional - fill.fee;
            runtime.account.quantity -= sold_quantity;
            runtime.account.entry_fee -= entry_fee_allocated;
            if runtime.account.quantity <= Decimal::ZERO {
                runtime.account.quantity = Decimal::ZERO;
                runtime.account.entry_price = None;
                runtime.account.stop_price = None;
                runtime.account.entry_fee = Decimal::ZERO;
                runtime.account.entry_fee_bps = 0;
                runtime.account.position_slippage_bps = 0;
            }
            runtime.account.last_processed_close_ms = Some(candle.close_time_ms);
            runtime.account.last_signal = "Sell".to_owned();
            let details = json!({
                "mode":"paper","side":"sell","reason":reason,"strategy_id":runtime.strategy.id(),
                "strategy_version":runtime.strategy.version(),"outcome":outcome.as_str(),
                "shares":sold_quantity.to_string(),"average_fill_price_pusd":fill.average_price.to_string(),
                "gross_proceeds_pusd":fill.notional.to_string(),"fee_pusd":fill.fee.to_string(),
                "realized_pnl_pusd":realized_pnl.to_string(),"slippage_bps":fill.slippage_bps,
                "book_levels_consumed":fill.levels,"quote_currency":"pUSD","synthetic_candle":true,
                "market_question":market.question
            });
            runtime.account.persist_event(
                "polymarket_paper_fill",
                details.clone(),
                now_ms,
                true,
            )?;
            println!(
                "{}",
                json!({"event":"paper_fill","details":details,"account":runtime.account.status_json(Some(best_bid))})
            );
        } else {
            runtime.account.last_processed_close_ms = Some(candle.close_time_ms);
            runtime.account.last_signal = "Sell".to_owned();
            runtime.account.persist_event(
                "polymarket_paper_exit_unfilled",
                json!({"reason":reason,"strategy_id":runtime.strategy.id()}),
                now_ms,
                false,
            )?;
        }
        return Ok(());
    }

    if direction == Direction::Buy && runtime.account.quantity == Decimal::ZERO {
        if !book_fresh
            || matches!(fee_model, FeeModel::Unsupported)
            || market.accepting_orders == Some(false)
        {
            runtime.account.last_processed_close_ms = Some(candle.close_time_ms);
            runtime.account.last_signal = "Buy".to_owned();
            runtime.account.persist_event(
                "polymarket_paper_entry_blocked",
                json!({
                    "book_fresh":book_fresh,"fee_model":fee_model.description(),
                    "accepting_orders":market.accepting_orders,"strategy_id":runtime.strategy.id(),
                    "quote_currency":"pUSD"
                }),
                now_ms,
                false,
            )?;
            return Ok(());
        }
        let signal = TradeSignal {
            strategy_id: StrategyId::from(runtime.strategy.id()),
            market_id: market_id.clone(),
            direction: Direction::Buy,
            estimated_probability: None,
            expected_net_edge_bps: 0,
            max_slippage_bps: args.max_slippage_bps,
            signal_timestamp_ms: candle.close_time_ms.max(0),
            expires_at_ms: now_ms + args.max_data_age_ms.max(1_000),
        };
        let market_snapshot = MarketSnapshot {
            market_id,
            price: best_ask,
            observed_at_ms: received_at,
            quality: if book_fresh {
                DataQuality::Valid
            } else {
                DataQuality::Delayed
            },
        };
        let portfolio_snapshot = runtime.account.portfolio_snapshot(best_bid, now_ms);
        let kill_active = kill_switch
            .is_active()
            .context("estado do kill-switch inválido; entrada paper bloqueada")?;
        let ticket = runtime.risk.authorize_entry(RiskInput {
            signal: &signal,
            market: &market_snapshot,
            portfolio: &portfolio_snapshot,
            now_ms,
            kill_switch_active: kill_active,
            execution_slippage_bps: args.max_slippage_bps,
        });
        match ticket {
            Ok(ticket) => {
                if let Some(fill) = simulate_buy(
                    book,
                    ticket.quantity,
                    runtime.account.cash,
                    args.max_slippage_bps,
                    fee_model,
                ) {
                    let min_order_size = book
                        .min_order_size
                        .or(market.min_order_size)
                        .unwrap_or(Decimal::ZERO);
                    if fill.notional < min_order_size {
                        runtime.account.last_processed_close_ms = Some(candle.close_time_ms);
                        runtime.account.last_signal = "Buy".to_owned();
                        runtime.account.persist_event("polymarket_paper_entry_rejected", json!({
                            "reason":"fill abaixo do mínimo do mercado","notional_pusd":fill.notional.to_string(),
                            "minimum_order_size_pusd":min_order_size.to_string(),"strategy_id":runtime.strategy.id()
                        }), now_ms, false)?;
                        return Ok(());
                    }
                    runtime.account.cash -= fill.notional + fill.fee;
                    runtime.account.quantity = fill.shares;
                    runtime.account.entry_price = Some(fill.average_price);
                    runtime.account.stop_price =
                        Some(fill.average_price * (Decimal::ONE - args.stop_loss));
                    runtime.account.entry_fee = fill.fee;
                    runtime.account.entry_fee_bps = fee_model.conservative_fee_bps();
                    runtime.account.position_slippage_bps = fill.slippage_bps;
                    runtime.account.last_processed_close_ms = Some(candle.close_time_ms);
                    runtime.account.last_signal = "Buy".to_owned();
                    let details = json!({
                        "mode":"paper","side":"buy","reason":"strategy_buy","strategy_id":runtime.strategy.id(),
                        "strategy_version":runtime.strategy.version(),"strategy_parameters":runtime.strategy.parameters(),
                        "outcome":outcome.as_str(),"shares":fill.shares.to_string(),
                        "average_fill_price_pusd":fill.average_price.to_string(),"gross_cost_pusd":fill.notional.to_string(),
                        "fee_pusd":fill.fee.to_string(),"stop_price_pusd":runtime.account.stop_price.map(|value| value.to_string()),
                        "slippage_bps":fill.slippage_bps,"book_levels_consumed":fill.levels,
                        "quote_currency":"pUSD","synthetic_candle":true,"market_question":market.question,
                        "live_order_sent":false
                    });
                    runtime.account.persist_event(
                        "polymarket_paper_fill",
                        details.clone(),
                        now_ms,
                        true,
                    )?;
                    println!(
                        "{}",
                        json!({"event":"paper_fill","details":details,"account":runtime.account.status_json(Some(best_bid))})
                    );
                    return Ok(());
                }
                runtime.account.last_signal = "Buy".to_owned();
            }
            Err(rejection) => {
                eprintln!(
                    "paper entry rejeitada para {}: {rejection}",
                    runtime.strategy.id()
                );
                runtime.account.last_signal = "Buy".to_owned();
            }
        }
    } else {
        runtime.account.last_signal = direction_name(direction).to_owned();
    }
    runtime.account.last_processed_close_ms = Some(candle.close_time_ms);
    runtime.account.persist_event("polymarket_signal", json!({
        "mode":"paper","decision":format!("{:?}", direction),"strategy_id":runtime.strategy.id(),
        "strategy_version":runtime.strategy.version(),"strategy_parameters":runtime.strategy.parameters(),
        "outcome":outcome.as_str(),"candle_close_ms":candle.close_time_ms,
        "price_close_pusd":candle.close.to_string(),"quote_currency":"pUSD","synthetic_candle":true,
        "book_fresh":book_fresh,"best_bid_pusd":best_bid.to_string(),"best_ask_pusd":best_ask.to_string(),
        "account":runtime.account.status_json(Some(best_bid))
    }), now_ms, false)?;
    Ok(())
}

async fn run() -> Result<()> {
    let args = Args::parse();
    let (interval_ms, interval_seconds) = interval_millis(&args.interval)?;
    if args.starting_balance_pusd <= Decimal::ZERO {
        bail!("starting balance pUSD deve ser positivo");
    }
    if args.max_data_age_ms <= 0 || args.max_data_age_ms > 86_400_000 {
        bail!("max-data-age-ms fora do intervalo seguro");
    }
    if args.max_slippage_bps > 1_000 {
        bail!("max-slippage-bps não pode exceder 1000");
    }

    let client = PolymarketClient::new()?;
    let market = client.market_by_slug(&args.slug).await?;
    let outcome: Outcome = args.outcome.into();
    let asset_id = market.asset_id(outcome).to_owned();
    let book = client.order_book(&asset_id).await?;
    let fee_model = fee_model_from_market(&market);
    if args.once {
        println!(
            "{}",
            json!({
                "event":"read_only_snapshot","mode":"read-only","question":market.question,
                "slug":market.slug,"condition_id":market.condition_id,"version":market.version,
                "outcome":outcome.as_str(),"asset_id":asset_id,"quote_currency":"pUSD",
                "active":market.active,"closed":market.closed,"accepting_orders":market.accepting_orders,
                "fee_model":fee_model.description(),"fee_rate":market.fees.rate.map(|value| value.to_string()),
                "fee_exponent":market.fees.exponent,
                "best_bid_pusd":book.best_bid().map(|level| level.price.to_string()),
                "best_ask_pusd":book.best_ask().map(|level| level.price.to_string()),
                "bid_depth_levels":book.bids.len(),"ask_depth_levels":book.asks.len(),
                "live_order_sent":false
            })
        );
        return Ok(());
    }
    if !market.can_read_book() {
        bail!("mercado fechado/inativo ou sem order book; modo contínuo bloqueado");
    }
    if !book.is_valid() {
        bail!("book inicial inválido/crossed; monitor paper não iniciado");
    }

    let config = StrategiesConfig::from_path(&args.strategy_config).with_context(|| {
        format!(
            "não foi possível carregar {}",
            args.strategy_config.display()
        )
    })?;
    let strategies = build_strategies(&config, args.strategy)?;
    let policy = RiskPolicy::from_path(&args.risk_overrides).with_context(|| {
        format!(
            "não foi possível carregar {}",
            args.risk_overrides.display()
        )
    })?;
    let store = AuditStore::open(&args.audit_db)?;
    let market_id = market_id_text(&market, outcome, &asset_id);
    let mut runtimes = create_runtimes(
        strategies,
        RuntimeSetup {
            market: &market,
            outcome,
            interval: &args.interval,
            account_symbol: &market_id,
            starting_balance: args.starting_balance_pusd,
            args: &args,
            fee_model,
            store: &store,
            policy: &policy,
        },
    )?;

    let now_ms = unix_ms();
    let (history, history_warning) = match client.price_history(&asset_id, interval_seconds).await {
        Ok(points) => (
            history_to_candles(points, interval_ms, interval_seconds, now_ms),
            None,
        ),
        Err(error) => (
            Vec::new(),
            Some(format!(
                "histórico indisponível; estratégias aguardam candles live: {error:#}"
            )),
        ),
    };
    if let Some(warning) = history_warning.as_deref() {
        eprintln!("WARN {warning}");
    }
    let mut aggregator = CandleAggregator::new(interval_ms, history);
    let strategy_summary: Vec<_> = runtimes.iter().map(|runtime| json!({
        "id":runtime.strategy.id(),"version":runtime.strategy.version(),"parameters":runtime.strategy.parameters(),
        "paper_account":runtime.account.account_key
    })).collect();
    println!(
        "{}",
        json!({
            "event":"monitor_started","mode":"read-only+paper","question":market.question,
            "slug":market.slug,"outcome":outcome.as_str(),"asset_id":asset_id,"quote_currency":"pUSD",
            "interval":args.interval,"synthetic_history_bars":aggregator.history.len(),"synthetic_ohlc":true,
            "fee_model":fee_model.description(),"strategies":strategy_summary,
            "independent_paper_accounts":true,"paper_accounts_are_not_an_ensemble":true,
            "database":args.audit_db,"kill_switch_file":args.kill_switch_file,
            "live_order_sent":false,"geographic_trading":false
        })
    );

    let kill_switch = KillSwitch::new(&args.kill_switch_file);
    let (feed_tx, mut feed_rx) = mpsc::channel(1024);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let feed_task = tokio::spawn(run_market_stream(
        asset_id.clone(),
        market.min_order_size,
        market.min_tick_size,
        feed_tx,
        shutdown_rx,
    ));
    let mut latest_book: Option<OrderBook> = None;
    let mut latest_book_received_at_ms: Option<i64> = None;
    let mut flush_clock = tokio::time::interval(Duration::from_millis(250));
    let deadline = if args.max_runtime_seconds > 0 {
        Some(tokio::time::Instant::now() + Duration::from_secs(args.max_runtime_seconds))
    } else {
        None
    };
    let mut last_top: Option<(Decimal, Decimal)> = None;

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("{}", json!({"event":"shutdown","reason":"ctrl_c","live_order_sent":false}));
                break;
            }
            _ = async { if let Some(deadline) = deadline { tokio::time::sleep_until(deadline).await; } else { std::future::pending::<()>().await; } } => {
                println!("{}", json!({"event":"shutdown","reason":"max_runtime","live_order_sent":false}));
                break;
            }
            _ = flush_clock.tick() => {
                let closed = aggregator.flush_due(unix_ms());
                for candle in closed {
                    let history = aggregator.history();
                    for runtime in &mut runtimes {
                        process_closed_candle(runtime, CandleProcessContext {
                            candle: &candle,
                            history: &history,
                            book: latest_book.as_ref(),
                            book_received_at_ms: latest_book_received_at_ms,
                            market: &market,
                            outcome,
                            args: &args,
                            fee_model,
                            kill_switch: &kill_switch,
                        })?;
                    }
                }
            }
            event = feed_rx.recv() => {
                let Some(event) = event else { bail!("feed Polymarket encerrou o canal"); };
                match event {
                    MarketFeedEvent::Connected => {
                        latest_book = None;
                        latest_book_received_at_ms = None;
                        println!("{}", json!({"event":"feed_connected","channel":"public_market","asset_id":asset_id}));
                    }
                    MarketFeedEvent::Disconnected { reason } => {
                        latest_book = None;
                        latest_book_received_at_ms = None;
                        println!("{}", json!({"event":"feed_disconnected","reason":reason,"retry":"exponential_backoff"}));
                    }
                    MarketFeedEvent::Book { book, received_at_ms, .. } => {
                        if !book.is_valid() { continue; }
                        let top = (book.best_bid().unwrap().price, book.best_ask().unwrap().price);
                        if last_top != Some(top) {
                            println!("{}", json!({"event":"top_of_book","best_bid_pusd":top.0.to_string(),
                                "best_ask_pusd":top.1.to_string(),"spread_pusd":(top.1-top.0).to_string(),
                                "timestamp_ms":book.timestamp_ms,"received_at_ms":received_at_ms}));
                            last_top = Some(top);
                        }
                        latest_book = Some(book);
                        latest_book_received_at_ms = Some(received_at_ms);
                    }
                    MarketFeedEvent::Trade { tick, received_at_ms } => {
                        if tick.asset_id != asset_id || tick.timestamp_ms > received_at_ms
                            || received_at_ms - tick.timestamp_ms > args.max_data_age_ms {
                            println!("{}", json!({"event":"trade_ignored","reason":"wrong_asset_or_stale","timestamp_ms":tick.timestamp_ms}));
                            continue;
                        }
                        for candle in aggregator.ingest_trade(&tick) {
                            let history = aggregator.history();
                            for runtime in &mut runtimes {
                                process_closed_candle(runtime, CandleProcessContext {
                                    candle: &candle,
                                    history: &history,
                                    book: latest_book.as_ref(),
                                    book_received_at_ms: latest_book_received_at_ms,
                                    market: &market,
                                    outcome,
                                    args: &args,
                                    fee_model,
                                    kill_switch: &kill_switch,
                                })?;
                            }
                        }
                    }
                }
            }
        }
    }
    let _ = shutdown_tx.send(true);
    feed_task.abort();
    Ok(())
}

fn unix_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn utc_date(timestamp_ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(timestamp_ms)
        .unwrap_or_else(Utc::now)
        .date_naive()
        .to_string()
}

#[tokio::main]
async fn main() -> Result<()> {
    run().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn tick(timestamp_ms: i64, price: &str, size: &str) -> TradeTick {
        TradeTick {
            asset_id: "asset".to_owned(),
            timestamp_ms,
            price: Decimal::from_str(price).unwrap(),
            size: Decimal::from_str(size).unwrap(),
            side: Some("BUY".to_owned()),
        }
    }

    #[test]
    fn history_uses_only_matching_closed_resolution_and_contiguous_suffix() {
        let points = vec![
            PriceHistoryPoint {
                timestamp_ms: 60_000,
                price: Decimal::new(4, 1),
                resolution_seconds: 60,
            },
            PriceHistoryPoint {
                timestamp_ms: 120_000,
                price: Decimal::new(5, 1),
                resolution_seconds: 60,
            },
            PriceHistoryPoint {
                timestamp_ms: 300_000,
                price: Decimal::new(6, 1),
                resolution_seconds: 60,
            },
            PriceHistoryPoint {
                timestamp_ms: 360_000,
                price: Decimal::new(7, 1),
                resolution_seconds: 300,
            },
            PriceHistoryPoint {
                timestamp_ms: 420_000,
                price: Decimal::new(8, 1),
                resolution_seconds: 60,
            },
        ];
        let candles = history_to_candles(points, 60_000, 60, 480_000);
        assert_eq!(candles.len(), 1);
        assert_eq!(candles[0].open_time_ms, 420_000);
        assert_eq!(candles[0].open, candles[0].high);
        assert_eq!(candles[0].volume, Decimal::ZERO);
    }

    #[test]
    fn live_candles_use_trades_and_never_fill_empty_time_buckets() {
        let mut aggregator = CandleAggregator::new(60_000, vec![]);
        assert!(aggregator.ingest_trade(&tick(1_000, "0.4", "2")).is_empty());
        assert!(
            aggregator
                .ingest_trade(&tick(30_000, "0.5", "3"))
                .is_empty()
        );
        let closed = aggregator.ingest_trade(&tick(61_000, "0.6", "1"));
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].open, Decimal::new(4, 1));
        assert_eq!(closed[0].high, Decimal::new(5, 1));
        assert_eq!(closed[0].close, Decimal::new(5, 1));
        assert_eq!(closed[0].volume, Decimal::from(5));
        let gap = aggregator.ingest_trade(&tick(181_000, "0.7", "1"));
        assert_eq!(gap.len(), 1);
        assert_eq!(aggregator.history.len(), 1);
        assert_eq!(aggregator.history[0].open_time_ms, 60_000);
    }

    #[test]
    fn paper_fee_curve_matches_documented_rate_and_zero_fee_markets() {
        let rate = Decimal::new(4, 2);
        assert_eq!(
            FeeModel::Curve { rate }.fee(Decimal::from(100), Decimal::new(5, 1)),
            Some(Decimal::ONE)
        );
        assert_eq!(
            FeeModel::Free.fee(Decimal::from(100), Decimal::new(5, 1)),
            Some(Decimal::ZERO)
        );
        assert!(
            FeeModel::Unsupported
                .fee(Decimal::ONE, Decimal::new(5, 1))
                .is_none()
        );
    }

    #[test]
    fn paper_sweep_consumes_depth_and_applies_slippage_cap() {
        let book = OrderBook {
            asset_id: "asset".to_owned(),
            timestamp_ms: 1,
            hash: None,
            bids: vec![ode_polymarket::BookLevel {
                price: Decimal::new(49, 2),
                size: Decimal::from(10),
            }],
            asks: vec![
                ode_polymarket::BookLevel {
                    price: Decimal::new(50, 2),
                    size: Decimal::from(3),
                },
                ode_polymarket::BookLevel {
                    price: Decimal::new(51, 2),
                    size: Decimal::from(4),
                },
            ],
            min_order_size: Some(Decimal::from(5)),
            tick_size: None,
            last_trade_price: None,
        };
        let fill = simulate_buy(
            &book,
            Decimal::from(5),
            Decimal::from(100),
            300,
            FeeModel::Free,
        )
        .unwrap();
        assert_eq!(fill.shares, Decimal::from(5));
        assert_eq!(fill.levels, 2);
        assert!(fill.average_price > Decimal::new(50, 2));
        let capped = simulate_buy(
            &book,
            Decimal::from(5),
            Decimal::from(100),
            100,
            FeeModel::Free,
        )
        .unwrap();
        assert_eq!(capped.shares, Decimal::from(3));
    }
}
