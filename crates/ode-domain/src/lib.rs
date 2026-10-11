use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

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
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
}
