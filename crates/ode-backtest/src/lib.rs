use ode_domain::{
    Candle, DataQuality, Direction, MarketId, MarketSnapshot, PortfolioSnapshot, StrategyId,
    TradeSignal,
};
use ode_market_data::{DataQualityReport, MarketDataError, validate_candles};
use ode_risk::{RiskEngine, RiskInput, RiskLimits, RiskPolicy};
use ode_strategy_technical::Strategy;
use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub starting_balance: Decimal,
    pub risk_limits: RiskLimits,
    pub risk_policy: RiskPolicy,
    pub in_sample_ratio: Decimal,
    pub interval_ms: i64,
    pub market_id: String,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            starting_balance: Decimal::from(10_000),
            risk_limits: RiskLimits {
                risk_per_trade: Decimal::new(1, 2),
                max_daily_loss: Decimal::new(2, 2),
                max_allocation: Decimal::new(25, 2),
                stop_loss: Decimal::new(2, 2),
                fee_bps: 10,
                max_slippage_bps: 5,
                min_expected_edge_bps: 0,
                max_market_data_age_ms: 120_000,
            },
            risk_policy: RiskPolicy::default(),
            in_sample_ratio: Decimal::new(7, 1),
            interval_ms: 60_000,
            market_id: "BTCUSDT".to_owned(),
        }
    }
}

#[derive(Debug, Error)]
pub enum BacktestError {
    #[error("dados de mercado inválidos: {0}")]
    Data(#[from] MarketDataError),
    #[error("configuração de backtest inválida: {0}")]
    InvalidConfig(String),
    #[error("política de risco inválida: {0}")]
    Risk(#[from] ode_risk::RiskError),
    #[error("histórico insuficiente para warm-up e split temporal")]
    InsufficientData,
}

#[derive(Debug, Clone, Serialize)]
pub struct BacktestMetrics {
    pub starting_balance: Decimal,
    pub ending_equity: Decimal,
    /// Retornos em percentuais (ex.: 2.5 significa +2,5%).
    pub total_return_pct: Decimal,
    pub trade_count: usize,
    pub risk_rejected_entries: usize,
    pub win_rate_pct: Decimal,
    pub expectancy_per_trade: Decimal,
    pub profit_factor: Option<Decimal>,
    pub max_drawdown_pct: Decimal,
    pub buy_and_hold_return_pct: Decimal,
    pub relative_to_buy_and_hold_pct: Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletedTrade {
    pub entry_time_ms: i64,
    pub exit_time_ms: i64,
    pub entry_fill: Decimal,
    pub exit_fill: Decimal,
    pub quantity: Decimal,
    pub net_pnl: Decimal,
    pub exit_reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BacktestReport {
    pub strategy_id: String,
    pub strategy_version: String,
    pub strategy_parameters: serde_json::Value,
    pub bars: usize,
    pub in_sample_start_ms: i64,
    pub in_sample_end_ms: i64,
    pub out_of_sample_start_ms: i64,
    pub out_of_sample_end_ms: i64,
    pub split_index: usize,
    pub data_quality: DataQualityReport,
    pub in_sample: BacktestMetrics,
    pub out_of_sample: BacktestMetrics,
    pub in_sample_trades: Vec<CompletedTrade>,
    pub out_of_sample_trades: Vec<CompletedTrade>,
    pub interpretation: String,
}

struct Position {
    quantity: Decimal,
    entry_fill: Decimal,
    entry_fee: Decimal,
    fee_bps: u32,
    slippage_bps: u32,
    stop_price: Decimal,
    entry_time_ms: i64,
}

struct Simulation {
    cash: Decimal,
    position: Option<Position>,
    trades: Vec<CompletedTrade>,
    risk_rejected_entries: usize,
    peak_equity: Decimal,
    max_drawdown_fraction: Decimal,
}

pub fn run_out_of_sample(
    candles: &[Candle],
    strategy: &dyn Strategy,
    config: &BacktestConfig,
) -> Result<BacktestReport, BacktestError> {
    validate_config(config)?;
    let quality = validate_candles(candles, config.interval_ms)?;
    let split_index = (Decimal::from(candles.len() as u64) * config.in_sample_ratio)
        .round()
        .to_usize()
        .ok_or(BacktestError::InsufficientData)?;
    let minimum_segment = strategy.min_history_len().saturating_add(2);
    if split_index < minimum_segment || candles.len().saturating_sub(split_index) < minimum_segment
    {
        return Err(BacktestError::InsufficientData);
    }
    let risk_engine = RiskEngine::new(config.risk_limits.clone(), config.risk_policy.clone())?;

    // Cada período começa flat; no OOS, a estratégia ainda pode usar o prefixo como warm-up.
    let in_sample_run = simulate(candles, 0, split_index, strategy, config, &risk_engine)?;
    let out_of_sample = simulate(
        candles,
        split_index,
        candles.len(),
        strategy,
        config,
        &risk_engine,
    )?;
    let in_sample_metrics = metrics(
        config.starting_balance,
        candles,
        0,
        split_index,
        &in_sample_run,
        config,
    );
    let oos_metrics = metrics(
        config.starting_balance,
        candles,
        split_index,
        candles.len(),
        &out_of_sample,
        config,
    );
    let interpretation = if oos_metrics.relative_to_buy_and_hold_pct > Decimal::ZERO {
        "A estratégia superou o benchmark buy-and-hold neste recorte OOS; isso não prova rentabilidade futura.".to_owned()
    } else {
        "A estratégia não superou o benchmark buy-and-hold neste recorte OOS; não ajustar parâmetros com base no resultado de teste.".to_owned()
    };

    Ok(BacktestReport {
        strategy_id: strategy.id().to_owned(),
        strategy_version: strategy.version().to_owned(),
        strategy_parameters: strategy.parameters(),
        bars: candles.len(),
        in_sample_start_ms: candles[0].open_time_ms,
        in_sample_end_ms: candles[split_index - 1].close_time_ms,
        out_of_sample_start_ms: candles[split_index].open_time_ms,
        out_of_sample_end_ms: candles[candles.len() - 1].close_time_ms,
        split_index,
        data_quality: quality,
        in_sample: in_sample_metrics,
        out_of_sample: oos_metrics,
        in_sample_trades: in_sample_run.trades,
        out_of_sample_trades: out_of_sample.trades,
        interpretation,
    })
}

fn validate_config(config: &BacktestConfig) -> Result<(), BacktestError> {
    if config.starting_balance <= Decimal::ZERO
        || config.interval_ms <= 0
        || config.market_id.trim().is_empty()
        || config.in_sample_ratio < Decimal::new(5, 1)
        || config.in_sample_ratio > Decimal::new(9, 1)
    {
        return Err(BacktestError::InvalidConfig(
            "saldo, mercado, intervalo ou in_sample_ratio fora dos limites".into(),
        ));
    }
    config.risk_limits.validate()?;
    Ok(())
}

fn simulate(
    candles: &[Candle],
    start: usize,
    end: usize,
    strategy: &dyn Strategy,
    config: &BacktestConfig,
    risk_engine: &RiskEngine,
) -> Result<Simulation, BacktestError> {
    let mut sim = Simulation {
        cash: config.starting_balance,
        position: None,
        trades: Vec::new(),
        risk_rejected_entries: 0,
        peak_equity: config.starting_balance,
        max_drawdown_fraction: Decimal::ZERO,
    };
    let first_decision = start.max(strategy.min_history_len().saturating_sub(1));
    if first_decision >= end {
        return Err(BacktestError::InsufficientData);
    }
    for index in first_decision..end {
        let candle = &candles[index];
        if sim
            .position
            .as_ref()
            .is_some_and(|position| candle.low <= position.stop_price)
        {
            let position = sim.position.take().expect("position checked above");
            let stop_market = if candle.open <= position.stop_price {
                candle.open
            } else {
                position.stop_price
            };
            close_position(
                &mut sim,
                position,
                stop_market,
                candle.close_time_ms,
                "stop_loss",
            );
        }
        mark_to_market(&mut sim, candle.close);
        let Some(next) = candles.get(index + 1).filter(|_| index + 1 < end) else {
            continue;
        };
        let signal = strategy.evaluate(&candles[..=index]);
        match signal {
            Direction::Buy if sim.position.is_none() => {
                if !open_position(&mut sim, candle, next, strategy, config, risk_engine) {
                    sim.risk_rejected_entries += 1;
                }
            }
            Direction::Sell if sim.position.is_some() => {
                let position = sim.position.take().expect("position checked above");
                close_position(
                    &mut sim,
                    position,
                    next.open,
                    next.open_time_ms,
                    "strategy_exit",
                );
            }
            Direction::Buy | Direction::Sell | Direction::Hold => {}
        }
    }
    let last = &candles[end - 1];
    if let Some(position) = sim.position.take() {
        close_position(
            &mut sim,
            position,
            last.close,
            last.close_time_ms,
            "period_end",
        );
    }
    mark_to_market(&mut sim, last.close);
    Ok(sim)
}

fn open_position(
    sim: &mut Simulation,
    signal_candle: &Candle,
    fill_candle: &Candle,
    strategy: &dyn Strategy,
    config: &BacktestConfig,
    risk_engine: &RiskEngine,
) -> bool {
    let expires_at_ms = signal_candle
        .close_time_ms
        .saturating_add(config.interval_ms.saturating_mul(2));
    let signal = TradeSignal {
        strategy_id: StrategyId::from(strategy.id()),
        market_id: MarketId::from(config.market_id.as_str()),
        direction: Direction::Buy,
        estimated_probability: None,
        expected_net_edge_bps: 0,
        max_slippage_bps: config.risk_limits.max_slippage_bps,
        signal_timestamp_ms: signal_candle.close_time_ms,
        expires_at_ms,
    };
    // A autorização de sizing acontece na abertura seguinte: preço e liquidez do fill não usam close futuro.
    let market = MarketSnapshot {
        market_id: MarketId::from(config.market_id.as_str()),
        price: fill_candle.open,
        observed_at_ms: fill_candle.open_time_ms,
        quality: DataQuality::Valid,
    };
    let portfolio = PortfolioSnapshot {
        equity: sim.cash,
        cash: sim.cash,
        daily_start_equity: config.starting_balance,
        open_exposure: Decimal::ZERO,
        has_open_position: false,
    };
    let ticket = match risk_engine.authorize_entry(RiskInput {
        signal: &signal,
        market: &market,
        portfolio: &portfolio,
        now_ms: fill_candle.open_time_ms,
        kill_switch_active: false,
        execution_slippage_bps: config.risk_limits.max_slippage_bps,
    }) {
        Ok(ticket) => ticket,
        Err(_) => return false,
    };
    sim.cash -= ticket.quantity * ticket.fill_price + ticket.entry_fee;
    sim.position = Some(Position {
        quantity: ticket.quantity,
        entry_fill: ticket.fill_price,
        entry_fee: ticket.entry_fee,
        fee_bps: ticket.fee_bps,
        slippage_bps: ticket.slippage_bps,
        stop_price: ticket.stop_price,
        entry_time_ms: fill_candle.open_time_ms,
    });
    true
}

fn close_position(
    sim: &mut Simulation,
    position: Position,
    market_price: Decimal,
    exit_time_ms: i64,
    reason: &str,
) {
    let exit_fill = market_price * (Decimal::ONE - ratio(position.slippage_bps));
    let proceeds = position.quantity * exit_fill;
    let exit_fee = proceeds * ratio(position.fee_bps);
    let net_pnl =
        (exit_fill - position.entry_fill) * position.quantity - position.entry_fee - exit_fee;
    sim.cash += proceeds - exit_fee;
    sim.trades.push(CompletedTrade {
        entry_time_ms: position.entry_time_ms,
        exit_time_ms,
        entry_fill: position.entry_fill,
        exit_fill,
        quantity: position.quantity,
        net_pnl,
        exit_reason: reason.to_owned(),
    });
}

fn mark_to_market(sim: &mut Simulation, price: Decimal) {
    let equity = sim.cash
        + sim
            .position
            .as_ref()
            .map_or(Decimal::ZERO, |p| p.quantity * price);
    sim.peak_equity = sim.peak_equity.max(equity);
    if sim.peak_equity > Decimal::ZERO {
        sim.max_drawdown_fraction = sim
            .max_drawdown_fraction
            .max((sim.peak_equity - equity) / sim.peak_equity);
    }
}

fn metrics(
    starting_balance: Decimal,
    candles: &[Candle],
    start: usize,
    end: usize,
    sim: &Simulation,
    config: &BacktestConfig,
) -> BacktestMetrics {
    let ending_equity = sim.cash;
    let total_return_pct = pct((ending_equity - starting_balance) / starting_balance);
    let wins = sim
        .trades
        .iter()
        .filter(|trade| trade.net_pnl > Decimal::ZERO)
        .count();
    let gross_profit: Decimal = sim
        .trades
        .iter()
        .filter(|trade| trade.net_pnl > Decimal::ZERO)
        .map(|t| t.net_pnl)
        .sum();
    let gross_loss: Decimal = sim
        .trades
        .iter()
        .filter(|trade| trade.net_pnl < Decimal::ZERO)
        .map(|t| -t.net_pnl)
        .sum();
    let count = Decimal::from(sim.trades.len() as u64);
    let expectancy = if sim.trades.is_empty() {
        Decimal::ZERO
    } else {
        sim.trades.iter().map(|t| t.net_pnl).sum::<Decimal>() / count
    };
    let profit_factor = if gross_loss > Decimal::ZERO {
        Some(gross_profit / gross_loss)
    } else {
        None
    };
    let buy_hold_return_pct = buy_and_hold_return(starting_balance, &candles[start..end], config);
    BacktestMetrics {
        starting_balance,
        ending_equity,
        total_return_pct,
        trade_count: sim.trades.len(),
        risk_rejected_entries: sim.risk_rejected_entries,
        win_rate_pct: if sim.trades.is_empty() {
            Decimal::ZERO
        } else {
            pct(Decimal::from(wins as u64) / count)
        },
        expectancy_per_trade: expectancy,
        profit_factor,
        max_drawdown_pct: pct(sim.max_drawdown_fraction),
        buy_and_hold_return_pct: buy_hold_return_pct,
        relative_to_buy_and_hold_pct: total_return_pct - buy_hold_return_pct,
    }
}

fn buy_and_hold_return(
    starting_balance: Decimal,
    candles: &[Candle],
    config: &BacktestConfig,
) -> Decimal {
    let Some(first) = candles.first() else {
        return Decimal::ZERO;
    };
    let Some(last) = candles.last() else {
        return Decimal::ZERO;
    };
    if first.open <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    let fee = ratio(config.risk_limits.fee_bps);
    let slippage = ratio(config.risk_limits.max_slippage_bps);
    let entry = first.open * (Decimal::ONE + slippage);
    let quantity = starting_balance / (entry * (Decimal::ONE + fee));
    let exit = last.close * (Decimal::ONE - slippage);
    let ending = quantity * exit * (Decimal::ONE - fee);
    pct((ending - starting_balance) / starting_balance)
}

fn ratio(bps: u32) -> Decimal {
    Decimal::from(bps) / Decimal::from(10_000)
}

fn pct(fraction: Decimal) -> Decimal {
    fraction * Decimal::from(100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ode_strategy_technical::{
        DivergenceConfig, EmaCrossover, EmaCrossoverConfig, PriceRsiDivergence,
    };

    fn candles() -> Vec<Candle> {
        let closes = [
            100, 99, 98, 97, 96, 97, 99, 101, 104, 107, 108, 107, 104, 101, 98, 95, 94, 96, 99,
            103, 107, 110, 112, 111, 108, 104, 100, 97, 95, 98, 103, 108, 114, 118, 116, 112, 108,
            104, 101, 98,
        ];
        closes
            .iter()
            .enumerate()
            .map(|(i, value)| {
                let close = Decimal::from(*value);
                let open = if i == 0 {
                    close
                } else {
                    Decimal::from(closes[i - 1])
                };
                Candle {
                    open_time_ms: i as i64 * 60_000,
                    close_time_ms: (i as i64 + 1) * 60_000 - 1,
                    open,
                    high: open.max(close) + Decimal::ONE,
                    low: open.min(close) - Decimal::ONE,
                    close,
                    volume: Decimal::from(100),
                    received_at_ms: Some((i as i64 + 1) * 60_000 + 10),
                }
            })
            .collect()
    }

    fn test_strategy() -> EmaCrossover {
        EmaCrossover::new(EmaCrossoverConfig {
            enabled: true,
            fast_period: 2,
            slow_period: 4,
        })
        .unwrap()
    }

    fn test_config() -> BacktestConfig {
        BacktestConfig {
            in_sample_ratio: Decimal::new(6, 1),
            ..BacktestConfig::default()
        }
    }

    #[test]
    fn holdout_report_separates_in_sample_and_oos_metrics() {
        let report = run_out_of_sample(&candles(), &test_strategy(), &test_config()).unwrap();
        assert_eq!(report.bars, 40);
        assert_eq!(report.split_index, 24);
        assert!(report.in_sample.trade_count > 0);
        assert!(report.out_of_sample.trade_count > 0);
        assert_eq!(report.data_quality.latency_samples, 40);
        assert!(report.in_sample.max_drawdown_pct >= Decimal::ZERO);
    }

    #[test]
    fn next_open_fills_and_costs_are_included() {
        let bars = candles();
        let mut config = test_config();
        config.risk_limits.fee_bps = 50;
        config.risk_limits.max_slippage_bps = 20;
        let report = run_out_of_sample(&bars, &test_strategy(), &config).unwrap();
        let trade = &report.in_sample_trades[0];
        let entry_index = (trade.entry_time_ms / 60_000) as usize;
        assert!(trade.entry_fill > bars[entry_index].open);
        assert!(report.in_sample.buy_and_hold_return_pct < Decimal::from(100));
    }

    #[test]
    fn invalid_history_and_relaxed_risk_config_fail_closed() {
        let mut bars = candles();
        bars[4].close = Decimal::from(-1);
        assert!(run_out_of_sample(&bars, &test_strategy(), &test_config()).is_err());
        let mut config = test_config();
        config.risk_limits.risk_per_trade = Decimal::new(2, 1);
        assert!(run_out_of_sample(&candles(), &test_strategy(), &config).is_err());
    }

    #[test]
    fn price_rsi_divergence_runs_only_when_explicitly_selected_and_records_parameters() {
        let strategy = PriceRsiDivergence::new(DivergenceConfig {
            enabled: true,
            rsi_period: 3,
            pivot_left_bars: 1,
            pivot_right_bars: 1,
        })
        .unwrap();
        let report = run_out_of_sample(&candles(), &strategy, &test_config()).unwrap();
        assert_eq!(report.strategy_id, "price_rsi_divergence");
        assert_eq!(report.strategy_version, "1.0.0");
        assert_eq!(report.strategy_parameters["enabled"], true);
        assert_eq!(report.strategy_parameters["rsi_period"], 3);
        assert_eq!(report.bars, 40);
    }
}
