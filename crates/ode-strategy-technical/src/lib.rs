use ode_domain::{Candle, Direction};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use thiserror::Error;

mod divergence;
pub use divergence::{
    DivergenceAnalysis, DivergenceConfig, DivergenceDirection, DivergenceSignal, PriceRsiDivergence,
};

#[derive(Debug, Error)]
pub enum StrategyConfigError {
    #[error("não foi possível ler configuração: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML de estratégia inválido: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("parâmetros EMA inválidos; exija 0 < fast_period < slow_period <= 500")]
    InvalidPeriods,
    #[error("parâmetros de divergência inválidos; RSI deve estar em 2..=500 e pivôs em 1..=100")]
    InvalidDivergenceParameters,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategiesConfig {
    #[serde(default)]
    pub ema_crossover: EmaCrossoverConfig,
    #[serde(default)]
    pub price_rsi_divergence: DivergenceConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EmaCrossoverConfig {
    pub enabled: bool,
    pub fast_period: usize,
    pub slow_period: usize,
}

impl Default for EmaCrossoverConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            fast_period: 9,
            slow_period: 21,
        }
    }
}

impl StrategiesConfig {
    pub fn from_toml(input: &str) -> Result<Self, StrategyConfigError> {
        let config: Self = toml::from_str(input)?;
        config.ema_crossover.validate()?;
        config.price_rsi_divergence.validate()?;
        Ok(config)
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, StrategyConfigError> {
        Self::from_toml(&fs::read_to_string(path)?)
    }
}

impl EmaCrossoverConfig {
    pub fn validate(&self) -> Result<(), StrategyConfigError> {
        if self.fast_period == 0 || self.fast_period >= self.slow_period || self.slow_period > 500 {
            return Err(StrategyConfigError::InvalidPeriods);
        }
        Ok(())
    }
}

/// Contrato puro: avalia somente o histórico fechado recebido e não acessa feed, risco ou execução.
pub trait Strategy: Send + Sync {
    fn id(&self) -> &'static str;
    fn version(&self) -> &'static str;
    fn parameters(&self) -> serde_json::Value;
    fn enabled(&self) -> bool;
    fn min_history_len(&self) -> usize;
    fn evaluate(&self, closed_history: &[Candle]) -> Direction;
}

#[derive(Debug, Clone)]
pub struct EmaCrossover {
    config: EmaCrossoverConfig,
}

impl EmaCrossover {
    pub fn new(config: EmaCrossoverConfig) -> Result<Self, StrategyConfigError> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &EmaCrossoverConfig {
        &self.config
    }
}

impl Default for EmaCrossover {
    fn default() -> Self {
        Self::new(EmaCrossoverConfig::default()).expect("defaults are valid")
    }
}

impl Strategy for EmaCrossover {
    fn id(&self) -> &'static str {
        "ema_crossover"
    }

    fn version(&self) -> &'static str {
        "1.0.0"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::to_value(&self.config).expect("EMA config is serializable")
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    fn min_history_len(&self) -> usize {
        self.config.slow_period + 1
    }

    fn evaluate(&self, closed_history: &[Candle]) -> Direction {
        if !self.enabled() || closed_history.len() < self.min_history_len() {
            return Direction::Hold;
        }
        let closes: Vec<Decimal> = closed_history.iter().map(|candle| candle.close).collect();
        let previous = &closes[..closes.len() - 1];
        let Some(previous_fast) = ema(previous, self.config.fast_period) else {
            return Direction::Hold;
        };
        let Some(previous_slow) = ema(previous, self.config.slow_period) else {
            return Direction::Hold;
        };
        let Some(current_fast) = ema(&closes, self.config.fast_period) else {
            return Direction::Hold;
        };
        let Some(current_slow) = ema(&closes, self.config.slow_period) else {
            return Direction::Hold;
        };
        if previous_fast <= previous_slow && current_fast > current_slow {
            Direction::Buy
        } else if previous_fast >= previous_slow && current_fast < current_slow {
            Direction::Sell
        } else {
            Direction::Hold
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn candle(index: i64, close: i64) -> Candle {
        let close = Decimal::from(close);
        Candle {
            open_time_ms: index * 60_000,
            close_time_ms: (index + 1) * 60_000 - 1,
            open: close,
            high: close,
            low: close,
            close,
            volume: Decimal::ONE,
            received_at_ms: None,
        }
    }

    #[test]
    fn crossover_uses_only_supplied_closed_history() {
        let strategy = EmaCrossover::new(EmaCrossoverConfig {
            enabled: true,
            fast_period: 2,
            slow_period: 3,
        })
        .unwrap();
        let mut history: Vec<_> = (0..12).map(|i| candle(i, 120 - i * 2)).collect();
        history.push(candle(12, 150));
        assert_eq!(strategy.evaluate(&history), Direction::Buy);
        let prefix = &history[..history.len() - 1];
        assert_ne!(strategy.evaluate(prefix), Direction::Buy);
    }

    #[test]
    fn disabled_strategy_never_emits_entry_or_exit() {
        let strategy = EmaCrossover::new(EmaCrossoverConfig {
            enabled: false,
            fast_period: 2,
            slow_period: 3,
        })
        .unwrap();
        let history: Vec<_> = (0..20).map(|i| candle(i, 100 + i)).collect();
        assert_eq!(strategy.evaluate(&history), Direction::Hold);
    }

    #[test]
    fn invalid_periods_are_rejected() {
        assert!(
            EmaCrossover::new(EmaCrossoverConfig {
                enabled: true,
                fast_period: 9,
                slow_period: 9
            })
            .is_err()
        );
    }

    #[test]
    fn missing_divergence_section_defaults_to_disabled_and_invalid_parameters_fail_closed() {
        let config = StrategiesConfig::from_toml(
            "[ema_crossover]\nenabled = true\nfast_period = 9\nslow_period = 21\n",
        )
        .unwrap();
        assert!(!config.price_rsi_divergence.enabled);
        assert!(
            StrategiesConfig::from_toml("[price_rsi_divergence]\nenabled = true\nrsi_period = 1\n")
                .is_err()
        );
    }
}
