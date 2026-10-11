use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

string_id!(MarketId);
string_id!(StrategyId);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candle {
    pub open_time_ms: i64,
    pub close_time_ms: i64,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    /// Horário local em que o candle foi recebido; `None` para histórico sem metadado.
    pub received_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataQuality {
    Valid,
    Delayed,
    Invalid,
    Inconsistent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Buy,
    Sell,
    Hold,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeSignal {
    pub strategy_id: StrategyId,
    pub market_id: MarketId,
    pub direction: Direction,
    pub estimated_probability: Option<Decimal>,
    pub expected_net_edge_bps: i64,
    pub max_slippage_bps: u32,
    pub signal_timestamp_ms: i64,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SignalValidationError {
    #[error("strategy_id não pode ser vazio")]
    EmptyStrategyId,
    #[error("market_id não pode ser vazio")]
    EmptyMarketId,
    #[error("probabilidade estimada deve estar entre 0 e 1")]
    InvalidProbability,
    #[error("slippage máximo do sinal excede 10000 bps")]
    InvalidSlippage,
    #[error("timestamps do sinal não podem ser negativos")]
    InvalidTimestamp,
    #[error("expiração do sinal deve ser igual ou posterior ao timestamp do sinal")]
    InvalidExpiry,
}

impl TradeSignal {
    /// Valida apenas a forma do contrato; idade e expiração relativa ao relógio
    /// são gates do motor de risco, que possui o contexto de execução.
    pub fn validate_shape(&self) -> Result<(), SignalValidationError> {
        if self.strategy_id.0.trim().is_empty() {
            return Err(SignalValidationError::EmptyStrategyId);
        }
        if self.market_id.0.trim().is_empty() {
            return Err(SignalValidationError::EmptyMarketId);
        }
        if self
            .estimated_probability
            .is_some_and(|probability| !(Decimal::ZERO..=Decimal::ONE).contains(&probability))
        {
            return Err(SignalValidationError::InvalidProbability);
        }
        if self.max_slippage_bps > 10_000 {
            return Err(SignalValidationError::InvalidSlippage);
        }
        if self.signal_timestamp_ms < 0 || self.expires_at_ms < 0 {
            return Err(SignalValidationError::InvalidTimestamp);
        }
        if self.expires_at_ms < self.signal_timestamp_ms {
            return Err(SignalValidationError::InvalidExpiry);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarketSnapshot {
    pub market_id: MarketId,
    pub price: Decimal,
    pub observed_at_ms: i64,
    pub quality: DataQuality,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PortfolioSnapshot {
    pub equity: Decimal,
    pub cash: Decimal,
    pub daily_start_equity: Decimal,
    pub open_exposure: Decimal,
    pub has_open_position: bool,
}

/// Snapshot versionado do estado paper necessário para retomar sem duplicar candles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaperPortfolioState {
    pub schema_version: u16,
    pub account_key: String,
    pub symbol: String,
    pub interval: String,
    pub starting_balance: Decimal,
    pub cash: Decimal,
    pub quantity: Decimal,
    pub entry_price: Option<Decimal>,
    pub stop_price: Option<Decimal>,
    pub entry_fee: Decimal,
    pub entry_fee_bps: u32,
    pub position_slippage_bps: u32,
    pub session_start_equity: Decimal,
    pub session_date_utc: String,
    pub last_processed_close_ms: Option<i64>,
    pub last_mark_price: Option<Decimal>,
    pub last_signal: String,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    Pending,
    Unknown,
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
}

impl OrderStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Filled | Self::Cancelled | Self::Rejected)
    }

    /// `Unknown` é deliberadamente tratado como estado que exige reconciliação.
    pub fn requires_reconciliation(self) -> bool {
        matches!(self, Self::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal() -> TradeSignal {
        TradeSignal {
            strategy_id: StrategyId::from("ema_crossover"),
            market_id: MarketId::from("BTCUSDT"),
            direction: Direction::Buy,
            estimated_probability: Some(Decimal::new(3, 1)),
            expected_net_edge_bps: 10,
            max_slippage_bps: 5,
            signal_timestamp_ms: 100,
            expires_at_ms: 200,
        }
    }

    #[test]
    fn signal_shape_accepts_bounded_probability_and_order_status_helpers_are_explicit() {
        assert!(signal().validate_shape().is_ok());
        assert!(OrderStatus::Filled.is_terminal());
        assert!(OrderStatus::Unknown.requires_reconciliation());
        assert!(!OrderStatus::Open.is_terminal());
    }

    #[test]
    fn signal_shape_rejects_invalid_probability_and_expiry() {
        let mut invalid_probability = signal();
        invalid_probability.estimated_probability = Some(Decimal::new(11, 1));
        assert_eq!(
            invalid_probability.validate_shape(),
            Err(SignalValidationError::InvalidProbability)
        );

        let mut invalid_expiry = signal();
        invalid_expiry.expires_at_ms = 99;
        assert_eq!(
            invalid_expiry.validate_shape(),
            Err(SignalValidationError::InvalidExpiry)
        );
    }
}
