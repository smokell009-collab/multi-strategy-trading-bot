use anyhow::{Context, Result, bail};
use chrono::Utc;
use clap::Parser;
use ode_backtest::{BacktestConfig, run_out_of_sample};
use ode_domain::{
    Candle, DataQuality, Direction, MarketId, MarketSnapshot, PortfolioSnapshot, StrategyId,
    TradeSignal,
};
use ode_events::AuditEvent;
use ode_market_data::{DataQualityReport, read_csv, validate_candles};
use ode_risk::{KillSwitch, RiskEngine, RiskInput, RiskLimits, RiskPolicy};
use ode_strategy_technical::{EmaCrossover, StrategiesConfig, Strategy};
use reqwest::Client;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
    str::FromStr,
    time::Duration,
};
use tokio::time::sleep;
use tracing::{error, info, warn};

const BINANCE_KLINES_URL: &str = "https://data-api.binance.vision/api/v3/klines";
const KLINE_LIMIT: usize = 100;

#[derive(Debug, Parser)]
#[command(
    name = "olho-de-deus",
    version,
    about = "Bot de trading em paper mode; não envia ordens reais"
)]
struct Args {
    /// Par Spot da Binance, por exemplo BTCUSDT.
    #[arg(long, default_value = "BTCUSDT")]
    symbol: String,
    /// Intervalo de candles aceito pela Binance.
    #[arg(long, default_value = "1m")]
    interval: String,
    /// Frequência de consulta em segundos.
    #[arg(long, default_value_t = 15)]
    poll_seconds: u64,
    /// Saldo inicial fictício, denominado em USDT.
    #[arg(long, default_value = "10000")]
    starting_balance: Decimal,
    /// Fração do patrimônio arriscada por operação (0.01 = 1%).
    #[arg(long, default_value = "0.01")]
    risk_per_trade: Decimal,
    /// Perda máxima diária em relação ao patrimônio no início do processo (0.02 = 2%).
    #[arg(long, default_value = "0.02")]
    max_daily_loss: Decimal,
    /// Teto de alocação por posição (0.25 = 25%).
    #[arg(long, default_value = "0.25")]
    max_allocation: Decimal,
    /// Stop de proteção simulado (0.02 = 2%).
    #[arg(long, default_value = "0.02")]
    stop_loss: Decimal,
    /// Taxa simulada em basis points; 10 = 0,10% por lado.
    #[arg(long, default_value_t = 10)]
    fee_bps: u32,
    /// Slippage simulado em basis points; 5 = 0,05%.
    #[arg(long, default_value_t = 5)]
    slippage_bps: u32,
    /// Arquivo TOML com parâmetros e interruptor da estratégia.
    #[arg(long, default_value = "configs/strategies.example.toml")]
    strategy_config: PathBuf,
    /// CSV OHLCV histórico: executa holdout cronológico e não consulta a Binance.
    #[arg(long)]
    backtest_csv: Option<PathBuf>,
    /// Fração inicial in-sample (0.70 = primeiros 70%; último trecho é OOS; sem ajuste automático).
    #[arg(long = "in-sample-ratio", default_value = "0.7")]
    in_sample_ratio: Decimal,
    /// Caminho opcional para gravar o relatório JSON além de imprimi-lo no stdout.
    #[arg(long)]
    backtest_report: Option<PathBuf>,
    /// Arquivo TOML com overrides por estratégia e por ativo.
    #[arg(long, default_value = "configs/risk-overrides.example.toml")]
    risk_overrides: PathBuf,
    /// Arquivo persistente do kill-switch. Se existir e for válido, bloqueia novas entradas.
    #[arg(long, default_value = "data/KILL_SWITCH.json")]
    kill_switch_file: PathBuf,
    /// Ativa o kill-switch com o motivo informado e encerra; não há flag de desativação.
    #[arg(long)]
    activate_kill_switch: Option<String>,
    /// Faz uma consulta e termina; útil para smoke tests.
    #[arg(long, default_value_t = false)]
    once: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Signal {
    Buy,
    Sell,
    Hold,
}

#[derive(Debug)]
struct PaperBroker {
    cash: Decimal,
    quantity: Decimal,
    entry_price: Option<Decimal>,
    stop_price: Option<Decimal>,
    entry_fee: Decimal,
    entry_fee_bps: u32,
    position_slippage_bps: u32,
    start_equity: Decimal,
    audit_path: String,
}

impl PaperBroker {
    fn new(balance: Decimal, audit_path: String) -> Self {
        Self {
            cash: balance,
            quantity: Decimal::ZERO,
            entry_price: None,
            stop_price: None,
            entry_fee: Decimal::ZERO,
            entry_fee_bps: 0,
            position_slippage_bps: 0,
            start_equity: balance,
            audit_path,
        }
    }

    fn has_position(&self) -> bool {
        self.quantity > Decimal::ZERO
    }

    fn equity(&self, mark_price: Decimal) -> Decimal {
        self.cash + self.quantity * mark_price
    }

    fn record(&self, event_type: &str, market_id: &str, details: Value) -> Result<()> {
        let event = AuditEvent::new(event_type, MarketId::from(market_id), details);
        if let Some(parent) = std::path::Path::new(&self.audit_path).parent() {
            fs::create_dir_all(parent).context("não foi possível criar a pasta de auditoria")?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.audit_path)
            .with_context(|| format!("não foi possível abrir {}", self.audit_path))?;
        serde_json::to_writer(&mut file, &event)?;
        file.write_all(b"\n")?;
        Ok(())
    }

    fn on_candle(
        &mut self,
        args: &Args,
        signal: Signal,
        candle: &Candle,
        risk_engine: &RiskEngine,
        kill_switch_active: bool,
    ) -> Result<()> {
        let price = candle.close;
        let equity = self.equity(price);
        let stop_hit = self.stop_price.is_some_and(|stop| price <= stop);
        self.record(
            "signal_observed",
            &args.symbol,
            json!({
                "signal": format!("{signal:?}"),
                "close": price.to_string(),
                "candle_close_time_ms": candle.close_time_ms,
                "equity": equity.to_string(),
                "kill_switch_active": kill_switch_active,
            }),
        )?;

        if self.has_position() && (stop_hit || signal == Signal::Sell) {
            let reason = if stop_hit {
                "stop_loss"
            } else {
                "ema_cross_down"
            };
            return self.close_position(args, price, reason);
        }
        if signal != Signal::Buy {
            return Ok(());
        }

        let now_ms = Utc::now().timestamp_millis();
        let period_ms = interval_millis(&args.interval).context("intervalo inválido")?;
        let trade_signal = TradeSignal {
            strategy_id: StrategyId::from("ema_crossover"),
            market_id: MarketId::from(args.symbol.as_str()),
            direction: Direction::Buy,
            estimated_probability: None,
            // A estratégia EMA do MVP não estima edge líquido; a política-base exige apenas >= 0.
            expected_net_edge_bps: 0,
            max_slippage_bps: args.slippage_bps,
            signal_timestamp_ms: candle.close_time_ms,
            expires_at_ms: candle
                .close_time_ms
                .saturating_add(period_ms.saturating_mul(2)),
        };
        let market = MarketSnapshot {
            market_id: MarketId::from(args.symbol.as_str()),
            price,
            observed_at_ms: candle.close_time_ms,
            quality: DataQuality::Valid,
        };
        let portfolio = PortfolioSnapshot {
            equity,
            cash: self.cash,
            daily_start_equity: self.start_equity,
            open_exposure: self.quantity * price,
            has_open_position: self.has_position(),
        };
        match risk_engine.authorize_entry(RiskInput {
            signal: &trade_signal,
            market: &market,
            portfolio: &portfolio,
            now_ms,
            kill_switch_active,
            execution_slippage_bps: args.slippage_bps,
        }) {
            Ok(ticket) => self.open_position(args, ticket),
            Err(reason) => {
                self.record(
                    "risk_rejected",
                    &args.symbol,
                    json!({"reason":reason.to_string(), "close":price.to_string()}),
                )?;
                warn!(reason=%reason, "entrada bloqueada pelo motor de risco");
                Ok(())
            }
        }
    }

    fn open_position(&mut self, args: &Args, ticket: ode_risk::EntryTicket) -> Result<()> {
        let notional = ticket.quantity * ticket.fill_price;
        self.cash -= notional + ticket.entry_fee;
        self.quantity = ticket.quantity;
        self.entry_price = Some(ticket.fill_price);
        self.stop_price = Some(ticket.stop_price);
        self.entry_fee = ticket.entry_fee;
        self.entry_fee_bps = ticket.fee_bps;
        self.position_slippage_bps = ticket.slippage_bps;
        self.record(
            "paper_order_filled",
            &args.symbol,
            json!({
                "side":"buy", "quantity":ticket.quantity.to_string(),
                "simulated_fill_price":ticket.fill_price.to_string(),
                "fee":ticket.entry_fee.to_string(), "risk_budget":ticket.risk_budget.to_string(),
                "stop_price":ticket.stop_price.to_string(), "execution_mode":"paper"
            }),
        )?;
        info!(quantity=%ticket.quantity, price=%ticket.fill_price, "compra simulada aberta; nenhuma ordem real foi enviada");
        Ok(())
    }

    fn close_position(&mut self, args: &Args, market_price: Decimal, reason: &str) -> Result<()> {
        if !self.has_position() || market_price <= Decimal::ZERO {
            return Ok(());
        }
        let slippage = Decimal::from(self.position_slippage_bps) / Decimal::from(10_000);
        let fee_rate = Decimal::from(self.entry_fee_bps) / Decimal::from(10_000);
        let fill_price = market_price * (Decimal::ONE - slippage);
        let proceeds = self.quantity * fill_price;
        let exit_fee = proceeds * fee_rate;
        let pnl = (fill_price - self.entry_price.unwrap_or(market_price)) * self.quantity
            - self.entry_fee
            - exit_fee;
        self.cash += proceeds - exit_fee;
        let closed_qty = self.quantity;
        self.quantity = Decimal::ZERO;
        self.entry_price = None;
        self.stop_price = None;
        self.entry_fee = Decimal::ZERO;
        self.entry_fee_bps = 0;
        self.position_slippage_bps = 0;
        self.record(
            "paper_order_filled",
            &args.symbol,
            json!({
                "side":"sell", "reason":reason, "quantity":closed_qty.to_string(),
                "market_price":market_price.to_string(), "simulated_fill_price":fill_price.to_string(),
                "fee":exit_fee.to_string(), "net_trade_pnl":pnl.to_string(),
                "cash":self.cash.to_string(), "execution_mode":"paper"
            }),
        )?;
        info!(reason, net_pnl=%pnl, cash=%self.cash, "posição simulada encerrada");
        Ok(())
    }
}

fn validate_args(args: &Args) -> Result<()> {
    if !args.symbol.chars().all(|c| c.is_ascii_alphanumeric()) || args.symbol.is_empty() {
        bail!("symbol deve conter apenas letras e números, por exemplo BTCUSDT");
    }
    if interval_millis(&args.interval).is_none() {
        bail!("intervalo não suportado; use 1m, 3m, 5m, 15m, 30m, 1h, 2h, 4h, 6h, 8h, 12h ou 1d");
    }
    if args.poll_seconds == 0 || args.starting_balance <= Decimal::ZERO {
        bail!("poll-seconds e starting-balance devem ser maiores que zero");
    }
    if args.risk_per_trade <= Decimal::ZERO
        || args.risk_per_trade > Decimal::new(1, 1)
        || args.max_daily_loss <= Decimal::ZERO
        || args.max_daily_loss > Decimal::new(5, 1)
        || args.max_allocation <= Decimal::ZERO
        || args.max_allocation > Decimal::ONE
        || args.stop_loss <= Decimal::ZERO
        || args.stop_loss > Decimal::new(5, 1)
    {
        bail!(
            "parâmetros de risco fora das barreiras obrigatórias do motor (risco/operação <=10%, perda diária/stop <=50%, alocação <=100%)"
        );
    }
    if args.fee_bps > 1_000 || args.slippage_bps > 1_000 {
        bail!("fee-bps e slippage-bps não podem exceder 1000");
    }
    if args.in_sample_ratio < Decimal::new(5, 1) || args.in_sample_ratio > Decimal::new(9, 1) {
        bail!("in-sample-ratio deve estar entre 0.5 e 0.9");
    }
    Ok(())
}

fn risk_limits(args: &Args) -> Result<RiskLimits> {
    let max_age = interval_millis(&args.interval)
        .context("intervalo inválido")?
        .saturating_mul(2);
    Ok(RiskLimits {
        risk_per_trade: args.risk_per_trade,
        max_daily_loss: args.max_daily_loss,
        max_allocation: args.max_allocation,
        stop_loss: args.stop_loss,
        fee_bps: args.fee_bps,
        max_slippage_bps: args.slippage_bps,
        min_expected_edge_bps: 0,
        max_market_data_age_ms: max_age,
    })
}

fn risk_engine(args: &Args, policy: RiskPolicy) -> Result<RiskEngine> {
    RiskEngine::new(risk_limits(args)?, policy).context("política de risco inválida; fail-closed")
}

fn interval_millis(interval: &str) -> Option<i64> {
    match interval {
        "1m" => Some(60_000),
        "3m" => Some(180_000),
        "5m" => Some(300_000),
        "15m" => Some(900_000),
        "30m" => Some(1_800_000),
        "1h" => Some(3_600_000),
        "2h" => Some(7_200_000),
        "4h" => Some(14_400_000),
        "6h" => Some(21_600_000),
        "8h" => Some(28_800_000),
        "12h" => Some(43_200_000),
        "1d" => Some(86_400_000),
        _ => None,
    }
}

async fn fetch_candles(
    client: &Client,
    args: &Args,
    min_history_len: usize,
) -> Result<(Vec<Candle>, DataQualityReport)> {
    let limit = KLINE_LIMIT.to_string();
    let response = client
        .get(BINANCE_KLINES_URL)
        .query(&[
            ("symbol", args.symbol.as_str()),
            ("interval", args.interval.as_str()),
            ("limit", limit.as_str()),
        ])
        .send()
        .await
        .context("falha ao consultar dados públicos de mercado Binance")?
        .error_for_status()
        .context("Binance retornou erro HTTP para klines")?;
    let rows: Vec<Vec<Value>> = response
        .json()
        .await
        .context("resposta de klines inválida")?;
    let now_ms = Utc::now().timestamp_millis();
    let mut candles = Vec::with_capacity(rows.len());
    for row in rows {
        if row.len() < 7 {
            bail!("candle recebido com menos de 7 campos");
        }
        let open_time_ms = row[0].as_i64().context("open time inválido")?;
        let open = Decimal::from_str(row[1].as_str().context("open inválido")?)
            .context("preço open inválido")?;
        let high = Decimal::from_str(row[2].as_str().context("high inválido")?)
            .context("preço high inválido")?;
        let low = Decimal::from_str(row[3].as_str().context("low inválido")?)
            .context("preço low inválido")?;
        let close = Decimal::from_str(row[4].as_str().context("close inválido")?)
            .context("preço de fechamento inválido")?;
        let volume = Decimal::from_str(row[5].as_str().context("volume inválido")?)
            .context("volume inválido")?;
        let close_time_ms = row[6].as_i64().context("close time inválido")?;
        if close > Decimal::ZERO && close_time_ms < now_ms {
            candles.push(Candle {
                open_time_ms,
                close_time_ms,
                open,
                high,
                low,
                close,
                volume,
                received_at_ms: None,
            });
        }
    }
    candles.sort_by_key(|c| c.open_time_ms);
    if candles.len() < min_history_len {
        bail!(
            "dados insuficientes: {} candles fechados; mínimo {}",
            candles.len(),
            min_history_len
        );
    }
    if let Some(latest) = candles.last_mut() {
        latest.received_at_ms = Some(now_ms);
    }
    let period_ms = interval_millis(&args.interval).context("intervalo inválido")?;
    let quality = validate_candles(&candles, period_ms)
        .context("qualidade/cadência de mercado inválida; nenhuma decisão permitida")?;
    Ok((candles, quality))
}

fn is_fresh(candle: &Candle, interval: &str) -> bool {
    let Some(period_ms) = interval_millis(interval) else {
        return false;
    };
    Utc::now()
        .timestamp_millis()
        .saturating_sub(candle.close_time_ms)
        <= period_ms.saturating_mul(2)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_owned()))
        .init();
    let args = Args::parse();
    validate_args(&args)?;
    let kill_switch = KillSwitch::new(args.kill_switch_file.clone());
    if let Some(reason) = args.activate_kill_switch.as_deref() {
        let state = kill_switch.activate(reason, Utc::now().timestamp_millis())?;
        info!(path=%kill_switch.path().display(), reason=%state.reason, "kill-switch persistido; novas entradas bloqueadas");
        return Ok(());
    }
    let strategy_config =
        StrategiesConfig::from_path(&args.strategy_config).with_context(|| {
            format!(
                "não foi possível carregar estratégia em {}",
                args.strategy_config.display()
            )
        })?;
    let strategy = EmaCrossover::new(strategy_config.ema_crossover)
        .context("configuração da estratégia EMA inválida")?;
    let policy = RiskPolicy::from_path(&args.risk_overrides).with_context(|| {
        format!(
            "não foi possível carregar overrides em {}",
            args.risk_overrides.display()
        )
    })?;
    if let Some(csv_path) = &args.backtest_csv {
        let period_ms = interval_millis(&args.interval).context("intervalo inválido")?;
        let (candles, _) = read_csv(
            File::open(csv_path).with_context(|| {
                format!("não foi possível abrir histórico {}", csv_path.display())
            })?,
            period_ms,
        )
        .with_context(|| format!("histórico CSV inválido: {}", csv_path.display()))?;
        let report = run_out_of_sample(
            &candles,
            &strategy,
            &BacktestConfig {
                starting_balance: args.starting_balance,
                risk_limits: risk_limits(&args)?,
                risk_policy: policy,
                in_sample_ratio: args.in_sample_ratio,
                interval_ms: period_ms,
                market_id: args.symbol.clone(),
            },
        )
        .context("holdout fora da amostra não pôde ser calculado")?;
        let report_json = serde_json::to_string_pretty(&report)?;
        if let Some(report_path) = &args.backtest_report {
            if let Some(parent) = report_path
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
            {
                fs::create_dir_all(parent)?;
            }
            fs::write(report_path, &report_json).with_context(|| {
                format!(
                    "não foi possível gravar relatório {}",
                    report_path.display()
                )
            })?;
        }
        println!("{report_json}");
        return Ok(());
    }
    let risk_engine = risk_engine(&args, policy)?;
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()?;
    let mut broker = PaperBroker::new(args.starting_balance, "data/audit.jsonl".to_owned());
    let mut last_processed_close = None;
    info!(symbol=%args.symbol, interval=%args.interval, balance=%args.starting_balance,
        "Olho de Deus iniciado em PAPER TRADING. Execução real não está implementada.");

    loop {
        match fetch_candles(&client, &args, strategy.min_history_len()).await {
            Ok((candles, quality)) => {
                let latest = candles.last().expect("fetch valida candles não vazios");
                info!(bars=quality.bars, p50_latency_ms=?quality.latency_p50_ms,
                    p95_latency_ms=?quality.latency_p95_ms, "qualidade do feed consultada");
                // Consultar o latch em todo ciclo, mesmo quando ainda não há candle novo.
                // Um arquivo ilegível/corrompido interrompe o processo: fail-closed.
                let kill_active = kill_switch
                    .is_active()
                    .context("estado do kill-switch incerto; fail-closed")?;
                if kill_active {
                    warn!(path=%kill_switch.path().display(), "kill-switch persistente ativo; entradas bloqueadas");
                }
                if last_processed_close != Some(latest.close_time_ms) {
                    last_processed_close = Some(latest.close_time_ms);
                    if !is_fresh(latest, &args.interval) {
                        warn!(
                            candle_close_time_ms = latest.close_time_ms,
                            "dados atrasados; nenhuma decisão será autorizada"
                        );
                        broker.record(
                            "stale_market_data",
                            &args.symbol,
                            json!({"close_time_ms":latest.close_time_ms}),
                        )?;
                    } else {
                        let signal = match strategy.evaluate(&candles) {
                            Direction::Buy => Signal::Buy,
                            Direction::Sell => Signal::Sell,
                            Direction::Hold => Signal::Hold,
                        };
                        broker.on_candle(&args, signal, latest, &risk_engine, kill_active)?;
                    }
                }
                info!(close=%latest.close, equity=%broker.equity(latest.close), has_position=broker.has_position(),
                    "estado paper atualizado");
            }
            Err(err) => {
                error!(error=%err, "falha de dados; nenhuma nova ordem simulada será criada neste ciclo");
                broker.record(
                    "market_data_error",
                    &args.symbol,
                    json!({"error":err.to_string()}),
                )?;
            }
        }
        if args.once {
            break;
        }
        tokio::select! {
            _ = sleep(Duration::from_secs(args.poll_seconds)) => {},
            result = tokio::signal::ctrl_c() => {
                if result.is_ok() {
                    info!("encerramento solicitado; nenhuma ordem real estava habilitada");
                    break;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candle(i: i64, close: Decimal) -> Candle {
        let now = Utc::now().timestamp_millis();
        Candle {
            open_time_ms: now - 60_000 + i,
            close_time_ms: now,
            open: close,
            high: close,
            low: close,
            close,
            volume: Decimal::ONE,
            received_at_ms: Some(now),
        }
    }

    fn test_args() -> Args {
        Args {
            symbol: "BTCUSDT".into(),
            interval: "1m".into(),
            poll_seconds: 15,
            starting_balance: Decimal::from(10_000),
            risk_per_trade: Decimal::new(1, 2),
            max_daily_loss: Decimal::new(2, 2),
            max_allocation: Decimal::new(25, 2),
            stop_loss: Decimal::new(2, 2),
            fee_bps: 10,
            slippage_bps: 5,
            strategy_config: PathBuf::from("configs/strategies.example.toml"),
            backtest_csv: None,
            in_sample_ratio: Decimal::new(7, 1),
            backtest_report: None,
            risk_overrides: PathBuf::from("configs/risk-overrides.example.toml"),
            kill_switch_file: PathBuf::from("data/KILL_SWITCH.json"),
            activate_kill_switch: None,
            once: true,
        }
    }

    fn test_engine(args: &Args) -> RiskEngine {
        risk_engine(args, RiskPolicy::default()).unwrap()
    }

    #[test]
    fn crossover_buy_is_detected_without_lookahead() {
        let mut candles: Vec<Candle> = (0..40).map(|i| candle(i, Decimal::from(100 - i))).collect();
        candles.push(candle(40, Decimal::from(200)));
        let strategy = EmaCrossover::default();
        assert_eq!(strategy.evaluate(&candles), Direction::Buy);
        assert_ne!(
            strategy.evaluate(&candles[..candles.len() - 1]),
            Direction::Buy
        );
    }

    #[test]
    fn insufficient_history_never_signals() {
        let candles: Vec<Candle> = (0..10).map(|i| candle(i, Decimal::from(100 + i))).collect();
        let strategy = EmaCrossover::default();
        assert_eq!(strategy.evaluate(&candles), Direction::Hold);
    }

    #[test]
    fn paper_risk_sizing_respects_allocation_cap() {
        let args = test_args();
        let engine = test_engine(&args);
        let mut broker =
            PaperBroker::new(Decimal::from(10_000), "/tmp/ode-test-audit.jsonl".into());
        let candle = candle(1, Decimal::from(100));
        broker
            .on_candle(&args, Signal::Buy, &candle, &engine, false)
            .unwrap();
        assert!(broker.has_position());
        let entry_price = broker.entry_price.unwrap();
        let exposure = broker.quantity * entry_price;
        assert!(exposure <= Decimal::from(2_500));
        let slippage = Decimal::from(args.slippage_bps) / Decimal::from(10_000);
        let fee_rate = Decimal::from(args.fee_bps) / Decimal::from(10_000);
        let stop_fill = entry_price * (Decimal::ONE - args.stop_loss) * (Decimal::ONE - slippage);
        let estimated_loss = broker.quantity
            * (entry_price - stop_fill + entry_price * fee_rate + stop_fill * fee_rate);
        assert!(estimated_loss <= Decimal::from(100));
        assert!(broker.cash >= Decimal::ZERO);
    }

    #[test]
    fn daily_loss_gate_blocks_new_entries() {
        let args = test_args();
        let engine = test_engine(&args);
        let mut broker =
            PaperBroker::new(Decimal::from(10_000), "/tmp/ode-test-audit.jsonl".into());
        broker.cash = Decimal::from(9_700);
        broker
            .on_candle(
                &args,
                Signal::Buy,
                &candle(2, Decimal::from(100)),
                &engine,
                false,
            )
            .unwrap();
        assert!(!broker.has_position());
    }

    #[test]
    fn persistent_kill_switch_blocks_new_entries() {
        let args = test_args();
        let engine = test_engine(&args);
        let mut broker =
            PaperBroker::new(Decimal::from(10_000), "/tmp/ode-test-audit.jsonl".into());
        broker
            .on_candle(
                &args,
                Signal::Buy,
                &candle(3, Decimal::from(100)),
                &engine,
                true,
            )
            .unwrap();
        assert!(!broker.has_position());
    }

    #[test]
    fn unsupported_interval_is_rejected() {
        let mut args = test_args();
        args.interval = "9m".into();
        assert!(validate_args(&args).is_err());
    }
}
