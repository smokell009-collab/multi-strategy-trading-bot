use ode_domain::{
    DataQuality, Direction, MarketId, MarketSnapshot, PortfolioSnapshot, TradeSignal,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use thiserror::Error;
use uuid::Uuid;

const BPS_DENOMINATOR: i64 = 10_000;
const MAX_TRADE_RISK_BPS: u32 = 1_000;
const MAX_DAILY_LOSS_BPS: u32 = 5_000;
const MAX_ALLOCATION_BPS: u32 = 10_000;
const MAX_STOP_LOSS_BPS: u32 = 5_000;
const MAX_SLIPPAGE_BPS: u32 = 1_000;
const MAX_DATA_AGE_MS: i64 = 86_400_000;

#[derive(Debug, Error)]
pub enum RiskError {
    #[error("arquivo de política de risco não pôde ser lido: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML de política inválido: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("JSON do kill-switch inválido: {0}")]
    Json(#[from] serde_json::Error),
    #[error("política de risco inválida: {0}")]
    InvalidPolicy(String),
    #[error("motivo do kill-switch vazio ou maior que 500 caracteres")]
    InvalidKillSwitchReason,
}

#[derive(Debug, Clone)]
pub struct RiskLimits {
    pub risk_per_trade: Decimal,
    pub max_daily_loss: Decimal,
    pub max_allocation: Decimal,
    pub stop_loss: Decimal,
    pub fee_bps: u32,
    pub max_slippage_bps: u32,
    pub min_expected_edge_bps: i64,
    pub max_market_data_age_ms: i64,
}

impl RiskLimits {
    pub fn validate(&self) -> Result<(), RiskError> {
        if self.risk_per_trade <= Decimal::ZERO || self.risk_per_trade > ratio(MAX_TRADE_RISK_BPS) {
            return Err(RiskError::InvalidPolicy(format!(
                "risk_per_trade deve estar entre 0 e {MAX_TRADE_RISK_BPS} bps"
            )));
        }
        if self.max_daily_loss <= Decimal::ZERO || self.max_daily_loss > ratio(MAX_DAILY_LOSS_BPS) {
            return Err(RiskError::InvalidPolicy(format!(
                "max_daily_loss deve estar entre 0 e {MAX_DAILY_LOSS_BPS} bps"
            )));
        }
        if self.max_allocation <= Decimal::ZERO || self.max_allocation > ratio(MAX_ALLOCATION_BPS) {
            return Err(RiskError::InvalidPolicy(
                "max_allocation deve estar entre 0 e 10000 bps".into(),
            ));
        }
        if self.stop_loss <= Decimal::ZERO || self.stop_loss > ratio(MAX_STOP_LOSS_BPS) {
            return Err(RiskError::InvalidPolicy(format!(
                "stop_loss deve estar entre 0 e {MAX_STOP_LOSS_BPS} bps"
            )));
        }
        if self.fee_bps > MAX_SLIPPAGE_BPS || self.max_slippage_bps > MAX_SLIPPAGE_BPS {
            return Err(RiskError::InvalidPolicy(format!(
                "taxas e slippage não podem exceder {MAX_SLIPPAGE_BPS} bps"
            )));
        }
        if self.min_expected_edge_bps < 0 {
            return Err(RiskError::InvalidPolicy(
                "edge líquido mínimo não pode ser negativo".into(),
            ));
        }
        if !(1..=MAX_DATA_AGE_MS).contains(&self.max_market_data_age_ms) {
            return Err(RiskError::InvalidPolicy(
                "idade máxima de dados fora do limite permitido".into(),
            ));
        }
        Ok(())
    }
}

fn ratio(bps: u32) -> Decimal {
    Decimal::from(bps) / Decimal::from(BPS_DENOMINATOR)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskPolicy {
    #[serde(default)]
    pub strategies: BTreeMap<String, RiskOverrides>,
    #[serde(default)]
    pub markets: BTreeMap<String, RiskOverrides>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskOverrides {
    pub risk_per_trade_bps: Option<u32>,
    pub max_daily_loss_bps: Option<u32>,
    pub max_allocation_bps: Option<u32>,
    pub stop_loss_bps: Option<u32>,
    pub fee_bps: Option<u32>,
    pub max_slippage_bps: Option<u32>,
    pub min_expected_edge_bps: Option<i64>,
    pub max_market_data_age_ms: Option<i64>,
}

impl RiskPolicy {
    pub fn from_toml(input: &str) -> Result<Self, RiskError> {
        let mut policy: Self = toml::from_str(input)?;
        policy.canonicalize_market_keys()?;
        Ok(policy)
    }

    fn canonicalize_market_keys(&mut self) -> Result<(), RiskError> {
        let mut normalized_markets = BTreeMap::new();
        for (market_id, overrides) in std::mem::take(&mut self.markets) {
            let canonical_id = MarketId::from(market_id)
                .canonicalized()
                .map_err(|error| RiskError::InvalidPolicy(error.to_string()))?
                .0;
            if normalized_markets
                .insert(canonical_id.clone(), overrides)
                .is_some()
            {
                return Err(RiskError::InvalidPolicy(format!(
                    "chaves de mercado duplicadas após canonicalização: {canonical_id}"
                )));
            }
        }
        self.markets = normalized_markets;
        Ok(())
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, RiskError> {
        let contents = fs::read_to_string(path)?;
        Self::from_toml(&contents)
    }

    fn limits_for(
        &self,
        base: &RiskLimits,
        strategy_id: &str,
        market_id: &str,
    ) -> Result<RiskLimits, RiskError> {
        let mut effective = base.clone();
        if let Some(overrides) = self.strategies.get(strategy_id) {
            overrides.apply(&mut effective);
        }
        if let Some(overrides) = self.markets.get(market_id) {
            overrides.apply(&mut effective);
        }
        effective.validate()?;
        Ok(effective)
    }
}

impl RiskOverrides {
    fn apply(&self, limits: &mut RiskLimits) {
        if let Some(value) = self.risk_per_trade_bps {
            limits.risk_per_trade = limits.risk_per_trade.min(ratio(value));
        }
        if let Some(value) = self.max_daily_loss_bps {
            limits.max_daily_loss = limits.max_daily_loss.min(ratio(value));
        }
        if let Some(value) = self.max_allocation_bps {
            limits.max_allocation = limits.max_allocation.min(ratio(value));
        }
        if let Some(value) = self.stop_loss_bps {
            limits.stop_loss = limits.stop_loss.min(ratio(value));
        }
        if let Some(value) = self.fee_bps {
            limits.fee_bps = limits.fee_bps.max(value);
        }
        if let Some(value) = self.max_slippage_bps {
            limits.max_slippage_bps = limits.max_slippage_bps.min(value);
        }
        if let Some(value) = self.min_expected_edge_bps {
            limits.min_expected_edge_bps = limits.min_expected_edge_bps.max(value);
        }
        if let Some(value) = self.max_market_data_age_ms {
            limits.max_market_data_age_ms = limits.max_market_data_age_ms.min(value);
        }
    }
}

#[derive(Debug, Clone)]
pub struct RiskEngine {
    base_limits: RiskLimits,
    policy: RiskPolicy,
}

impl RiskEngine {
    pub fn new(base_limits: RiskLimits, mut policy: RiskPolicy) -> Result<Self, RiskError> {
        base_limits.validate()?;
        policy.canonicalize_market_keys()?;
        Ok(Self {
            base_limits,
            policy,
        })
    }

    pub fn authorize_entry(&self, input: RiskInput<'_>) -> Result<EntryTicket, RiskRejection> {
        if input.kill_switch_active {
            return Err(RiskRejection::KillSwitchActive);
        }
        if input.signal.validate_shape().is_err() {
            return Err(RiskRejection::InvalidSignal);
        }
        if input.signal.direction != Direction::Buy {
            return Err(RiskRejection::NotAnEntrySignal);
        }
        let signal_market_id = input
            .signal
            .market_id
            .canonicalized()
            .map_err(|_| RiskRejection::InvalidSignal)?;
        let snapshot_market_id = input
            .market
            .market_id
            .canonicalized()
            .map_err(|_| RiskRejection::InvalidMarketData)?;
        if signal_market_id != snapshot_market_id {
            return Err(RiskRejection::MarketMismatch);
        }
        if input.market.quality != DataQuality::Valid {
            return Err(RiskRejection::InvalidMarketData);
        }
        let limits = self
            .policy
            .limits_for(
                &self.base_limits,
                &input.signal.strategy_id.0,
                &signal_market_id.0,
            )
            .map_err(|_| RiskRejection::InvalidPolicy)?;
        if input.market.observed_at_ms > input.now_ms
            || input.now_ms.saturating_sub(input.market.observed_at_ms)
                > limits.max_market_data_age_ms
        {
            return Err(RiskRejection::StaleMarketData);
        }
        if input.signal.signal_timestamp_ms > input.now_ms
            || input.now_ms > input.signal.expires_at_ms
            || input.signal.expires_at_ms < input.signal.signal_timestamp_ms
        {
            return Err(RiskRejection::ExpiredSignal);
        }
        if input.signal.expected_net_edge_bps < limits.min_expected_edge_bps {
            return Err(RiskRejection::InsufficientExpectedEdge);
        }
        if input.execution_slippage_bps > input.signal.max_slippage_bps
            || input.execution_slippage_bps > limits.max_slippage_bps
        {
            return Err(RiskRejection::SlippageLimit);
        }
        if input.portfolio.has_open_position {
            return Err(RiskRejection::PositionAlreadyOpen);
        }
        if input.market.price <= Decimal::ZERO || input.portfolio.equity <= Decimal::ZERO {
            return Err(RiskRejection::InvalidPriceOrEquity);
        }
        if input.portfolio.daily_start_equity <= Decimal::ZERO
            || input.portfolio.cash < Decimal::ZERO
        {
            return Err(RiskRejection::InvalidPortfolio);
        }
        let daily_floor =
            input.portfolio.daily_start_equity * (Decimal::ONE - limits.max_daily_loss);
        if input.portfolio.equity <= daily_floor {
            return Err(RiskRejection::DailyLossLimit);
        }

        let slippage = ratio(input.execution_slippage_bps);
        let fee_rate = ratio(limits.fee_bps);
        let fill_price = input.market.price * (Decimal::ONE + slippage);
        let stop_market_price = fill_price * (Decimal::ONE - limits.stop_loss);
        let stop_fill_price = stop_market_price * (Decimal::ONE - slippage);
        let risk_per_unit =
            (fill_price - stop_fill_price) + (fill_price * fee_rate) + (stop_fill_price * fee_rate);
        let risk_budget = input.portfolio.equity * limits.risk_per_trade;
        if risk_per_unit <= Decimal::ZERO || risk_budget <= Decimal::ZERO {
            return Err(RiskRejection::InvalidRiskParameters);
        }
        let risk_sized_quantity = risk_budget / risk_per_unit;
        let allocation_quantity = input.portfolio.equity * limits.max_allocation / fill_price;
        let affordable_quantity = input.portfolio.cash / (fill_price * (Decimal::ONE + fee_rate));
        let quantity = risk_sized_quantity
            .min(allocation_quantity)
            .min(affordable_quantity);
        if quantity <= Decimal::ZERO {
            return Err(RiskRejection::InsufficientFunds);
        }
        let entry_fee = quantity * fill_price * fee_rate;
        Ok(EntryTicket {
            quantity,
            fill_price,
            stop_price: stop_market_price,
            risk_budget,
            entry_fee,
            fee_bps: limits.fee_bps,
            slippage_bps: input.execution_slippage_bps,
        })
    }
}

pub struct RiskInput<'a> {
    pub signal: &'a TradeSignal,
    pub market: &'a MarketSnapshot,
    pub portfolio: &'a PortfolioSnapshot,
    pub now_ms: i64,
    pub kill_switch_active: bool,
    pub execution_slippage_bps: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntryTicket {
    pub quantity: Decimal,
    pub fill_price: Decimal,
    pub stop_price: Decimal,
    pub risk_budget: Decimal,
    pub entry_fee: Decimal,
    pub fee_bps: u32,
    pub slippage_bps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RiskRejection {
    #[error("kill-switch ativo")]
    KillSwitchActive,
    #[error("sinal não é uma entrada")]
    NotAnEntrySignal,
    #[error("mercado do sinal não corresponde ao snapshot")]
    MarketMismatch,
    #[error("qualidade de dados inválida")]
    InvalidMarketData,
    #[error("dados de mercado atrasados ou futuros")]
    StaleMarketData,
    #[error("sinal expirado ou com timestamps inválidos")]
    ExpiredSignal,
    #[error("contrato do sinal inválido")]
    InvalidSignal,
    #[error("edge esperado inferior ao mínimo configurado")]
    InsufficientExpectedEdge,
    #[error("slippage acima do limite do sinal ou da política")]
    SlippageLimit,
    #[error("já existe posição aberta")]
    PositionAlreadyOpen,
    #[error("preço ou patrimônio inválido")]
    InvalidPriceOrEquity,
    #[error("snapshot de carteira inconsistente")]
    InvalidPortfolio,
    #[error("limite de perda diária atingido")]
    DailyLossLimit,
    #[error("política efetiva inválida; operação bloqueada")]
    InvalidPolicy,
    #[error("parâmetros de risco inválidos")]
    InvalidRiskParameters,
    #[error("saldo insuficiente para quantidade positiva")]
    InsufficientFunds,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KillSwitchState {
    pub activated_at_ms: i64,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct KillSwitch {
    path: PathBuf,
}

impl KillSwitch {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn state(&self) -> Result<Option<KillSwitchState>, RiskError> {
        match fs::read_to_string(&self.path) {
            Ok(contents) => Ok(Some(serde_json::from_str(&contents)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn is_active(&self) -> Result<bool, RiskError> {
        self.state().map(|state| state.is_some())
    }

    /// Ativa o bloqueio de forma atômica e durável. A biblioteca deliberadamente não oferece desativação.
    pub fn activate(&self, reason: &str, now_ms: i64) -> Result<KillSwitchState, RiskError> {
        let reason = reason.trim();
        if reason.is_empty() || reason.chars().count() > 500 {
            return Err(RiskError::InvalidKillSwitchReason);
        }
        if let Some(state) = self.state()? {
            return Ok(state);
        }
        let state = KillSwitchState {
            activated_at_ms: now_ms,
            reason: reason.to_owned(),
        };
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let temp_path = self.path.with_extension(format!("{}.tmp", Uuid::new_v4()));
        let bytes = serde_json::to_vec(&state)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temp_path, &self.path)?;
        File::open(parent)?.sync_all()?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ode_domain::{MarketId, StrategyId};
    use proptest::prelude::*;

    fn base_limits() -> RiskLimits {
        RiskLimits {
            risk_per_trade: Decimal::new(1, 2),
            max_daily_loss: Decimal::new(2, 2),
            max_allocation: Decimal::new(25, 2),
            stop_loss: Decimal::new(2, 2),
            fee_bps: 10,
            max_slippage_bps: 10,
            min_expected_edge_bps: 0,
            max_market_data_age_ms: 120_000,
        }
    }

    fn fixtures(
        price: Decimal,
        equity: Decimal,
        now: i64,
    ) -> (TradeSignal, MarketSnapshot, PortfolioSnapshot) {
        (
            TradeSignal {
                strategy_id: StrategyId::from("ema_crossover"),
                market_id: MarketId::from("BTCUSDT"),
                direction: Direction::Buy,
                estimated_probability: None,
                expected_net_edge_bps: 10,
                max_slippage_bps: 5,
                signal_timestamp_ms: now - 1,
                expires_at_ms: now + 10_000,
            },
            MarketSnapshot {
                market_id: MarketId::from("BTCUSDT"),
                price,
                observed_at_ms: now - 1,
                quality: DataQuality::Valid,
            },
            PortfolioSnapshot {
                equity,
                cash: equity,
                daily_start_equity: equity,
                open_exposure: Decimal::ZERO,
                has_open_position: false,
            },
        )
    }

    fn engine(policy: RiskPolicy) -> RiskEngine {
        RiskEngine::new(base_limits(), policy).unwrap()
    }

    #[test]
    fn kill_switch_blocks_entry_before_any_other_check() {
        let engine = engine(RiskPolicy::default());
        let (signal, market, portfolio) =
            fixtures(Decimal::from(100), Decimal::from(10_000), 1_000);
        let result = engine.authorize_entry(RiskInput {
            signal: &signal,
            market: &market,
            portfolio: &portfolio,
            now_ms: 1_000,
            kill_switch_active: true,
            execution_slippage_bps: 5,
        });
        assert_eq!(result, Err(RiskRejection::KillSwitchActive));
    }

    #[test]
    fn stale_or_low_quality_market_data_fails_closed() {
        let engine = engine(RiskPolicy::default());
        let (signal, mut market, portfolio) =
            fixtures(Decimal::from(100), Decimal::from(10_000), 1_000);
        market.quality = DataQuality::Delayed;
        assert_eq!(
            engine.authorize_entry(RiskInput {
                signal: &signal,
                market: &market,
                portfolio: &portfolio,
                now_ms: 1_000,
                kill_switch_active: false,
                execution_slippage_bps: 5,
            }),
            Err(RiskRejection::InvalidMarketData)
        );
        market.quality = DataQuality::Valid;
        market.observed_at_ms = -200_000;
        assert_eq!(
            engine.authorize_entry(RiskInput {
                signal: &signal,
                market: &market,
                portfolio: &portfolio,
                now_ms: 1_000,
                kill_switch_active: false,
                execution_slippage_bps: 5,
            }),
            Err(RiskRejection::StaleMarketData)
        );
    }

    #[test]
    fn market_matching_is_canonical_but_never_crosses_venues() {
        let engine = engine(RiskPolicy::default());
        let (mut signal, mut market, portfolio) =
            fixtures(Decimal::from(100), Decimal::from(10_000), 1_000);
        signal.market_id = MarketId::from(" binance:btcusdt ");
        market.market_id = MarketId::from("BINANCE:BTCUSDT");
        assert!(
            engine
                .authorize_entry(RiskInput {
                    signal: &signal,
                    market: &market,
                    portfolio: &portfolio,
                    now_ms: 1_000,
                    kill_switch_active: false,
                    execution_slippage_bps: 5,
                })
                .is_ok()
        );

        market.market_id = MarketId::from("COINBASE:BTCUSDT");
        assert_eq!(
            engine.authorize_entry(RiskInput {
                signal: &signal,
                market: &market,
                portfolio: &portfolio,
                now_ms: 1_000,
                kill_switch_active: false,
                execution_slippage_bps: 5,
            }),
            Err(RiskRejection::MarketMismatch)
        );
    }

    #[test]
    fn canonical_market_id_selects_the_matching_market_risk_override() {
        let policy =
            RiskPolicy::from_toml("[markets.\" binance:btcusdt \" ]\nmax_slippage_bps = 3\n")
                .unwrap();
        let engine = engine(policy);
        let (mut signal, mut market, portfolio) =
            fixtures(Decimal::from(100), Decimal::from(10_000), 1_000);
        signal.market_id = MarketId::from(" binance:btcusdt ");
        market.market_id = MarketId::from("BINANCE:BTCUSDT");
        assert_eq!(
            engine.authorize_entry(RiskInput {
                signal: &signal,
                market: &market,
                portfolio: &portfolio,
                now_ms: 1_000,
                kill_switch_active: false,
                execution_slippage_bps: 5,
            }),
            Err(RiskRejection::SlippageLimit)
        );
    }

    #[test]
    fn duplicate_market_override_keys_after_canonicalization_are_rejected() {
        let policy = RiskPolicy::from_toml(
            "[markets.BTCUSDT]\nmax_slippage_bps = 3\n\n[markets.\" btcusdt \"]\nmax_slippage_bps = 4\n",
        );
        assert!(matches!(policy, Err(RiskError::InvalidPolicy(_))));
    }

    #[test]
    fn empty_market_id_fails_closed_before_sizing() {
        let engine = engine(RiskPolicy::default());
        let (mut signal, market, portfolio) =
            fixtures(Decimal::from(100), Decimal::from(10_000), 1_000);
        signal.market_id = MarketId::from(" \t ");
        assert_eq!(
            engine.authorize_entry(RiskInput {
                signal: &signal,
                market: &market,
                portfolio: &portfolio,
                now_ms: 1_000,
                kill_switch_active: false,
                execution_slippage_bps: 5,
            }),
            Err(RiskRejection::InvalidSignal)
        );

        let (signal, mut market, portfolio) =
            fixtures(Decimal::from(100), Decimal::from(10_000), 1_000);
        market.market_id = MarketId::from(" ");
        assert_eq!(
            engine.authorize_entry(RiskInput {
                signal: &signal,
                market: &market,
                portfolio: &portfolio,
                now_ms: 1_000,
                kill_switch_active: false,
                execution_slippage_bps: 5,
            }),
            Err(RiskRejection::InvalidMarketData)
        );
    }

    #[test]
    fn malformed_signal_is_rejected_before_sizing() {
        let engine = engine(RiskPolicy::default());
        let (mut signal, market, portfolio) =
            fixtures(Decimal::from(100), Decimal::from(10_000), 1_000);
        signal.estimated_probability = Some(Decimal::from(2));
        assert_eq!(
            engine.authorize_entry(RiskInput {
                signal: &signal,
                market: &market,
                portfolio: &portfolio,
                now_ms: 1_000,
                kill_switch_active: false,
                execution_slippage_bps: 5,
            }),
            Err(RiskRejection::InvalidSignal)
        );
    }

    #[test]
    fn policy_applies_strategy_then_market_overrides() {
        let policy = RiskPolicy::from_toml(
            "\
            [strategies.ema_crossover]\nmax_allocation_bps = 2000\n\
            [markets.BTCUSDT]\nmax_slippage_bps = 8\n",
        )
        .unwrap();
        let limits = policy
            .limits_for(&base_limits(), "ema_crossover", "BTCUSDT")
            .unwrap();
        assert_eq!(limits.max_allocation, Decimal::new(20, 2));
        assert_eq!(limits.max_slippage_bps, 8);
    }

    #[test]
    fn policy_cannot_disable_mandatory_risk_limits() {
        let policy =
            RiskPolicy::from_toml("[strategies.ema_crossover]\nrisk_per_trade_bps = 0\n").unwrap();
        assert!(
            policy
                .limits_for(&base_limits(), "ema_crossover", "BTCUSDT")
                .is_err()
        );
    }

    #[test]
    fn overrides_cannot_relax_base_limits() {
        let policy = RiskPolicy::from_toml(
            "[strategies.ema_crossover]\nrisk_per_trade_bps = 500\nmax_allocation_bps = 9000\nmax_slippage_bps = 900\nmax_market_data_age_ms = 80000000\n",
        )
        .unwrap();
        let base = base_limits();
        let effective = policy
            .limits_for(&base, "ema_crossover", "BTCUSDT")
            .unwrap();
        assert_eq!(effective.risk_per_trade, base.risk_per_trade);
        assert_eq!(effective.max_allocation, base.max_allocation);
        assert_eq!(effective.max_slippage_bps, base.max_slippage_bps);
        assert_eq!(
            effective.max_market_data_age_ms,
            base.max_market_data_age_ms
        );
    }

    #[test]
    fn persisted_kill_switch_survives_reopening_and_corruption_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("KILL_SWITCH.json");
        let first = KillSwitch::new(&path);
        assert!(!first.is_active().unwrap());
        let state = first.activate("operator halt", 42).unwrap();
        let reopened = KillSwitch::new(&path);
        assert!(reopened.is_active().unwrap());
        assert_eq!(reopened.state().unwrap(), Some(state));
        fs::write(&path, b"not-json").unwrap();
        assert!(reopened.is_active().is_err());
    }

    proptest! {
        #[test]
        fn authorized_size_never_exceeds_risk_or_allocation_caps(price_cents in 1_i64..100_000, equity_cents in 1_i64..1_000_000_000) {
            let price = Decimal::new(price_cents, 2);
            let equity = Decimal::new(equity_cents, 2);
            let now = 1_000;
            let (signal, market, portfolio) = fixtures(price, equity, now);
            let engine = engine(RiskPolicy::default());
            if let Ok(ticket) = engine.authorize_entry(RiskInput {
                signal: &signal, market: &market, portfolio: &portfolio, now_ms: now,
                kill_switch_active: false, execution_slippage_bps: 5,
            }) {
                let limits = base_limits();
                let max_notional = equity * limits.max_allocation;
                prop_assert!(ticket.quantity * ticket.fill_price <= max_notional + Decimal::new(1, 4));
                let stop_fill = ticket.fill_price * (Decimal::ONE - limits.stop_loss)
                    * (Decimal::ONE - Decimal::new(5, 4));
                let unit_loss = ticket.fill_price - stop_fill
                    + ticket.fill_price * ratio(limits.fee_bps)
                    + stop_fill * ratio(limits.fee_bps);
                prop_assert!(ticket.quantity * unit_loss <= equity * limits.risk_per_trade + Decimal::new(1, 4));
            }
        }
    }
}
