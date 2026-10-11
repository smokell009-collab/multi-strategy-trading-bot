use anyhow::{Context, Result, bail};
use chrono::Utc;
use clap::Parser;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    str::FromStr,
    time::Duration,
};
use tokio::time::sleep;
use tracing::{error, info, warn};

const BINANCE_KLINES_URL: &str = "https://data-api.binance.vision/api/v3/klines";
const FAST_EMA: usize = 9;
const SLOW_EMA: usize = 21;
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
    /// Faz uma consulta e termina; útil para smoke tests.
    #[arg(long, default_value_t = false)]
    once: bool,
}

#[derive(Debug, Clone)]
struct Candle {
    open_time_ms: i64,
    close_time_ms: i64,
    close: Decimal,
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
    entry_fee: Decimal,
    start_equity: Decimal,
    audit_path: String,
}

#[derive(Serialize)]
struct AuditRecord {
    timestamp: String,
    event: String,
    symbol: String,
    details: Value,
}

impl PaperBroker {
    fn new(balance: Decimal, audit_path: String) -> Self {
        Self {
            cash: balance,
            quantity: Decimal::ZERO,
            entry_price: None,
            entry_fee: Decimal::ZERO,
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

    fn record(&self, event: &str, symbol: &str, details: Value) -> Result<()> {
        let record = AuditRecord {
            timestamp: Utc::now().to_rfc3339(),
            event: event.to_owned(),
            symbol: symbol.to_owned(),
            details,
        };
        if let Some(parent) = std::path::Path::new(&self.audit_path).parent() {
            fs::create_dir_all(parent).context("não foi possível criar a pasta de auditoria")?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.audit_path)
            .with_context(|| format!("não foi possível abrir {}", self.audit_path))?;
        serde_json::to_writer(&mut file, &record)?;
        file.write_all(b"\n")?;
        Ok(())
    }

    fn on_candle(&mut self, args: &Args, signal: Signal, candle: &Candle) -> Result<()> {
        let price = candle.close;
        let equity = self.equity(price);
        let daily_floor = self.start_equity * (Decimal::ONE - args.max_daily_loss);
        let daily_loss_locked = equity <= daily_floor;
        let stop_price = self
            .entry_price
            .map(|entry| entry * (Decimal::ONE - args.stop_loss));
        let stop_hit = stop_price.is_some_and(|stop| price <= stop);

        self.record(
            "signal",
            &args.symbol,
            json!({
                "signal": format!("{signal:?}"),
                "close": price.to_string(),
                "candle_close_time_ms": candle.close_time_ms,
                "equity": equity.to_string(),
                "daily_loss_locked": daily_loss_locked,
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

        if signal == Signal::Buy {
            if self.has_position() {
                self.record(
                    "entry_rejected",
                    &args.symbol,
                    json!({"reason":"position_already_open"}),
                )?;
                return Ok(());
            }
            if daily_loss_locked {
                self.record(
                    "entry_rejected",
                    &args.symbol,
                    json!({"reason":"daily_loss_limit"}),
                )?;
                warn!("entrada bloqueada: limite de perda diária atingido");
                return Ok(());
            }
            self.open_position(args, price, equity)?;
        }
        Ok(())
    }

    fn open_position(&mut self, args: &Args, market_price: Decimal, equity: Decimal) -> Result<()> {
        if market_price <= Decimal::ZERO {
            self.record(
                "entry_rejected",
                &args.symbol,
                json!({"reason":"invalid_price"}),
            )?;
            return Ok(());
        }
        let slippage = Decimal::from(args.slippage_bps) / Decimal::from(10_000);
        let fee_rate = Decimal::from(args.fee_bps) / Decimal::from(10_000);
        let fill_price = market_price * (Decimal::ONE + slippage);
        let risk_budget = equity * args.risk_per_trade;
        let stop_market_price = fill_price * (Decimal::ONE - args.stop_loss);
        let stop_fill_price = stop_market_price * (Decimal::ONE - slippage);
        let risk_per_unit =
            (fill_price - stop_fill_price) + (fill_price * fee_rate) + (stop_fill_price * fee_rate);
        if risk_budget <= Decimal::ZERO || risk_per_unit <= Decimal::ZERO {
            self.record(
                "entry_rejected",
                &args.symbol,
                json!({"reason":"invalid_risk_parameters"}),
            )?;
            return Ok(());
        }
        let risk_sized_qty = risk_budget / risk_per_unit;
        let allocation_qty = equity * args.max_allocation / fill_price;
        let affordable_qty = self.cash / (fill_price * (Decimal::ONE + fee_rate));
        let quantity = risk_sized_qty.min(allocation_qty).min(affordable_qty);
        if quantity <= Decimal::ZERO {
            self.record(
                "entry_rejected",
                &args.symbol,
                json!({"reason":"insufficient_cash_or_zero_size"}),
            )?;
            return Ok(());
        }
        let notional = quantity * fill_price;
        let fee = notional * fee_rate;
        self.cash -= notional + fee;
        self.quantity = quantity;
        self.entry_price = Some(fill_price);
        self.entry_fee = fee;
        self.record(
            "paper_order_filled",
            &args.symbol,
            json!({
                "side":"buy", "quantity":quantity.to_string(),
                "market_price":market_price.to_string(), "simulated_fill_price":fill_price.to_string(),
                "fee":fee.to_string(), "risk_budget":risk_budget.to_string(),
                "stop_price":(fill_price * (Decimal::ONE - args.stop_loss)).to_string(),
                "execution_mode":"paper"
            }),
        )?;
        info!(quantity=%quantity, price=%fill_price, "compra simulada aberta; nenhuma ordem real foi enviada");
        Ok(())
    }

    fn close_position(&mut self, args: &Args, market_price: Decimal, reason: &str) -> Result<()> {
        if !self.has_position() || market_price <= Decimal::ZERO {
            return Ok(());
        }
        let slippage = Decimal::from(args.slippage_bps) / Decimal::from(10_000);
        let fee_rate = Decimal::from(args.fee_bps) / Decimal::from(10_000);
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
        self.entry_fee = Decimal::ZERO;
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
    for (name, value) in [
        ("risk-per-trade", args.risk_per_trade),
        ("max-daily-loss", args.max_daily_loss),
        ("max-allocation", args.max_allocation),
        ("stop-loss", args.stop_loss),
    ] {
        if value <= Decimal::ZERO || value >= Decimal::ONE {
            bail!("{name} deve estar entre 0 e 1 (exclusivo)");
        }
    }
    if args.fee_bps > 1_000 || args.slippage_bps > 1_000 {
        bail!("fee-bps e slippage-bps não podem exceder 1000");
    }
    Ok(())
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

async fn fetch_candles(client: &Client, args: &Args) -> Result<Vec<Candle>> {
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
        let close = Decimal::from_str(row[4].as_str().context("close inválido")?)
            .context("preço de fechamento inválido")?;
        let close_time_ms = row[6].as_i64().context("close time inválido")?;
        if close > Decimal::ZERO && close_time_ms < now_ms {
            candles.push(Candle {
                open_time_ms,
                close_time_ms,
                close,
            });
        }
    }
    candles.sort_by_key(|c| c.open_time_ms);
    if candles.len() < SLOW_EMA + 1 {
        bail!(
            "dados insuficientes: {} candles fechados; mínimo {}",
            candles.len(),
            SLOW_EMA + 1
        );
    }
    Ok(candles)
}

fn ema(values: &[Decimal], period: usize) -> Option<Decimal> {
    if period == 0 || values.len() < period {
        return None;
    }
    let alpha = Decimal::from(2) / Decimal::from(period + 1);
    let mut current = values[0];
    for value in &values[1..] {
        current = *value * alpha + current * (Decimal::ONE - alpha);
    }
    Some(current)
}

fn compute_signal(candles: &[Candle]) -> Signal {
    if candles.len() < SLOW_EMA + 1 {
        return Signal::Hold;
    }
    let closes: Vec<Decimal> = candles.iter().map(|c| c.close).collect();
    let previous = &closes[..closes.len() - 1];
    let (Some(prev_fast), Some(prev_slow), Some(curr_fast), Some(curr_slow)) = (
        ema(previous, FAST_EMA),
        ema(previous, SLOW_EMA),
        ema(&closes, FAST_EMA),
        ema(&closes, SLOW_EMA),
    ) else {
        return Signal::Hold;
    };
    if prev_fast <= prev_slow && curr_fast > curr_slow {
        Signal::Buy
    } else if prev_fast >= prev_slow && curr_fast < curr_slow {
        Signal::Sell
    } else {
        Signal::Hold
    }
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
        match fetch_candles(&client, &args).await {
            Ok(candles) => {
                let latest = candles.last().expect("fetch valida candles não vazios");
                if last_processed_close != Some(latest.close_time_ms) {
                    last_processed_close = Some(latest.close_time_ms);
                    if !is_fresh(latest, &args.interval) {
                        warn!(
                            candle_close_time_ms = latest.close_time_ms,
                            "dados atrasados; nenhuma decisão de entrada será autorizada"
                        );
                        broker.record(
                            "stale_market_data",
                            &args.symbol,
                            json!({"close_time_ms":latest.close_time_ms}),
                        )?;
                    } else {
                        let signal = compute_signal(&candles);
                        broker.on_candle(&args, signal, latest)?;
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
        Candle {
            open_time_ms: i * 60_000,
            close_time_ms: i * 60_000 + 59_999,
            close,
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
            once: true,
        }
    }

    #[test]
    fn crossover_buy_is_detected_without_lookahead() {
        let mut candles: Vec<Candle> = (0..40).map(|i| candle(i, Decimal::from(100 - i))).collect();
        candles.push(candle(40, Decimal::from(200)));
        assert_eq!(compute_signal(&candles), Signal::Buy);
    }

    #[test]
    fn insufficient_history_never_signals() {
        let candles: Vec<Candle> = (0..10).map(|i| candle(i, Decimal::from(100 + i))).collect();
        assert_eq!(compute_signal(&candles), Signal::Hold);
    }

    #[test]
    fn paper_risk_sizing_respects_allocation_cap() {
        let args = test_args();
        let mut broker =
            PaperBroker::new(Decimal::from(10_000), "/tmp/ode-test-audit.jsonl".into());
        let candle = candle(1, Decimal::from(100));
        broker.on_candle(&args, Signal::Buy, &candle).unwrap();
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
        let mut broker =
            PaperBroker::new(Decimal::from(10_000), "/tmp/ode-test-audit.jsonl".into());
        // A 3% drawdown on the mark-to-market equity trips the 2% daily guard.
        broker.cash = Decimal::from(9_700);
        broker
            .on_candle(&args, Signal::Buy, &candle(2, Decimal::from(100)))
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
