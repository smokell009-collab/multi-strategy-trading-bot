use crate::{Strategy, StrategyConfigError};
use ode_domain::{Candle, Direction};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Configuração da divergência preço/RSI. O módulo começa desligado e não faz parte do loop paper.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct DivergenceConfig {
    pub enabled: bool,
    pub rsi_period: usize,
    pub pivot_left_bars: usize,
    pub pivot_right_bars: usize,
}

impl Default for DivergenceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            rsi_period: 14,
            pivot_left_bars: 2,
            pivot_right_bars: 2,
        }
    }
}

impl DivergenceConfig {
    pub fn validate(&self) -> Result<(), StrategyConfigError> {
        if !(2..=500).contains(&self.rsi_period)
            || !(1..=100).contains(&self.pivot_left_bars)
            || !(1..=100).contains(&self.pivot_right_bars)
        {
            return Err(StrategyConfigError::InvalidDivergenceParameters);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DivergenceDirection {
    Bullish,
    Bearish,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DivergenceSignal {
    pub direction: DivergenceDirection,
    pub first_pivot_index: usize,
    pub second_pivot_index: usize,
    pub confirmation_index: usize,
    pub confirmation_time_ms: i64,
    pub first_price: Decimal,
    pub second_price: Decimal,
    pub first_rsi: Decimal,
    pub second_rsi: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DivergenceAnalysis {
    Disabled,
    InvalidData,
    InsufficientHistory,
    NoDivergence,
    Confirmed(DivergenceSignal),
    Invalidated {
        signal: DivergenceSignal,
        invalidated_at_ms: i64,
    },
}

/// Detector determinístico de divergência regular entre pivôs estritos de preço e RSI Wilder.
/// A resposta só pode gerar uma direção na barra que confirma o segundo pivô.
#[derive(Debug, Clone)]
pub struct PriceRsiDivergence {
    config: DivergenceConfig,
}

impl PriceRsiDivergence {
    pub fn new(config: DivergenceConfig) -> Result<Self, StrategyConfigError> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &DivergenceConfig {
        &self.config
    }

    pub fn analyze(&self, closed_history: &[Candle]) -> DivergenceAnalysis {
        if !self.config.enabled {
            return DivergenceAnalysis::Disabled;
        }
        if !valid_history(closed_history) {
            return DivergenceAnalysis::InvalidData;
        }
        if closed_history.len() < self.min_history_len() {
            return DivergenceAnalysis::InsufficientHistory;
        }

        let lows = confirmed_pivots(closed_history, &self.config, DivergenceDirection::Bullish);
        let highs = confirmed_pivots(closed_history, &self.config, DivergenceDirection::Bearish);
        let rsi_values = rsi_wilder_series(closed_history, self.config.rsi_period);
        let mut candidates = Vec::new();
        collect_divergences(
            closed_history,
            &self.config,
            &lows,
            DivergenceDirection::Bullish,
            &rsi_values,
            &mut candidates,
        );
        collect_divergences(
            closed_history,
            &self.config,
            &highs,
            DivergenceDirection::Bearish,
            &rsi_values,
            &mut candidates,
        );

        let Some(signal) = candidates.into_iter().max_by_key(|signal| {
            (
                signal.confirmation_index,
                match signal.direction {
                    DivergenceDirection::Bullish => 0_u8,
                    DivergenceDirection::Bearish => 1_u8,
                },
            )
        }) else {
            return DivergenceAnalysis::NoDivergence;
        };

        let invalidated_at_ms = closed_history[signal.confirmation_index + 1..]
            .iter()
            .find(|candle| match signal.direction {
                DivergenceDirection::Bullish => candle.low < signal.second_price,
                DivergenceDirection::Bearish => candle.high > signal.second_price,
            })
            .map(|candle| candle.close_time_ms);
        match invalidated_at_ms {
            Some(invalidated_at_ms) => DivergenceAnalysis::Invalidated {
                signal,
                invalidated_at_ms,
            },
            None => DivergenceAnalysis::Confirmed(signal),
        }
    }
}

impl Strategy for PriceRsiDivergence {
    fn id(&self) -> &'static str {
        "price_rsi_divergence"
    }

    fn version(&self) -> &'static str {
        "1.0.0"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::to_value(&self.config).expect("divergence config is serializable")
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    fn min_history_len(&self) -> usize {
        self.config.rsi_period
            + 2 * (self.config.pivot_left_bars + self.config.pivot_right_bars)
            + 1
    }

    fn evaluate(&self, closed_history: &[Candle]) -> Direction {
        match self.analyze(closed_history) {
            DivergenceAnalysis::Confirmed(signal)
                if signal.confirmation_index + 1 == closed_history.len() =>
            {
                match signal.direction {
                    DivergenceDirection::Bullish => Direction::Buy,
                    DivergenceDirection::Bearish => Direction::Sell,
                }
            }
            _ => Direction::Hold,
        }
    }
}

fn valid_history(candles: &[Candle]) -> bool {
    let structurally_valid = candles.iter().enumerate().all(|(index, candle)| {
        let valid_candle = candle.open_time_ms >= 0
            && candle.close_time_ms >= candle.open_time_ms
            && candle.open > Decimal::ZERO
            && candle.high > Decimal::ZERO
            && candle.low > Decimal::ZERO
            && candle.close > Decimal::ZERO
            && candle.volume >= Decimal::ZERO
            && candle.high >= candle.open.max(candle.close)
            && candle.low <= candle.open.min(candle.close)
            && candle.low <= candle.high;
        let ordered = index == 0
            || (candles[index - 1].open_time_ms < candle.open_time_ms
                && candles[index - 1].close_time_ms < candle.close_time_ms);
        valid_candle && ordered
    });
    if !structurally_valid || candles.len() < 2 {
        return structurally_valid;
    }
    let expected_open_delta = candles[1].open_time_ms - candles[0].open_time_ms;
    let expected_candle_duration = candles[0].close_time_ms - candles[0].open_time_ms;
    expected_open_delta > 0
        && candles
            .windows(2)
            .all(|pair| pair[1].open_time_ms - pair[0].open_time_ms == expected_open_delta)
        && candles
            .iter()
            .all(|candle| candle.close_time_ms - candle.open_time_ms == expected_candle_duration)
}

fn confirmed_pivots(
    candles: &[Candle],
    config: &DivergenceConfig,
    direction: DivergenceDirection,
) -> Vec<usize> {
    let last_confirmable = candles.len().saturating_sub(config.pivot_right_bars + 1);
    (config.pivot_left_bars..=last_confirmable)
        .filter(|&index| is_pivot(candles, index, config, direction))
        .collect()
}

fn is_pivot(
    candles: &[Candle],
    index: usize,
    config: &DivergenceConfig,
    direction: DivergenceDirection,
) -> bool {
    if index < config.pivot_left_bars
        || index.saturating_add(config.pivot_right_bars) >= candles.len()
    {
        return false;
    }
    let value = pivot_value(&candles[index], direction);
    let strictly_beyond = |other: &Candle| match direction {
        DivergenceDirection::Bullish => value < other.low,
        DivergenceDirection::Bearish => value > other.high,
    };
    candles[index - config.pivot_left_bars..index]
        .iter()
        .all(strictly_beyond)
        && candles[index + 1..=index + config.pivot_right_bars]
            .iter()
            .all(strictly_beyond)
}

fn pivot_value(candle: &Candle, direction: DivergenceDirection) -> Decimal {
    match direction {
        DivergenceDirection::Bullish => candle.low,
        DivergenceDirection::Bearish => candle.high,
    }
}

fn collect_divergences(
    candles: &[Candle],
    config: &DivergenceConfig,
    pivots: &[usize],
    direction: DivergenceDirection,
    rsi_values: &[Option<Decimal>],
    output: &mut Vec<DivergenceSignal>,
) {
    for pair in pivots.windows(2) {
        let first_index = pair[0];
        let second_index = pair[1];
        let Some(first_rsi) = rsi_values.get(first_index).copied().flatten() else {
            continue;
        };
        let Some(second_rsi) = rsi_values.get(second_index).copied().flatten() else {
            continue;
        };
        let first_price = pivot_value(&candles[first_index], direction);
        let second_price = pivot_value(&candles[second_index], direction);
        let diverges = match direction {
            DivergenceDirection::Bullish => second_price < first_price && second_rsi > first_rsi,
            DivergenceDirection::Bearish => second_price > first_price && second_rsi < first_rsi,
        };
        if !diverges {
            continue;
        }

        let confirmation_index = second_index + config.pivot_right_bars;
        if confirmation_index >= candles.len() {
            continue;
        }
        let signal = DivergenceSignal {
            direction,
            first_pivot_index: first_index,
            second_pivot_index: second_index,
            confirmation_index,
            confirmation_time_ms: candles[confirmation_index].close_time_ms,
            first_price,
            second_price,
            first_rsi,
            second_rsi,
        };
        output.push(signal);
    }
}

/// Série Wilder causal: o valor em cada índice só usa fechamentos anteriores ou iguais.
fn rsi_wilder_series(candles: &[Candle], period: usize) -> Vec<Option<Decimal>> {
    let mut values = vec![None; candles.len()];
    if period == 0 || candles.len() <= period {
        return values;
    }
    let period_decimal = Decimal::from(period as u64);
    let mut gain_sum = Decimal::ZERO;
    let mut loss_sum = Decimal::ZERO;
    for cursor in 1..=period {
        let delta = candles[cursor].close - candles[cursor - 1].close;
        if delta > Decimal::ZERO {
            gain_sum += delta;
        } else {
            loss_sum -= delta;
        }
    }
    let mut average_gain = gain_sum / period_decimal;
    let mut average_loss = loss_sum / period_decimal;
    values[period] = Some(rsi_value(average_gain, average_loss));
    for cursor in period + 1..candles.len() {
        let delta = candles[cursor].close - candles[cursor - 1].close;
        let gain = delta.max(Decimal::ZERO);
        let loss = (-delta).max(Decimal::ZERO);
        average_gain = (average_gain * Decimal::from((period - 1) as u64) + gain) / period_decimal;
        average_loss = (average_loss * Decimal::from((period - 1) as u64) + loss) / period_decimal;
        values[cursor] = Some(rsi_value(average_gain, average_loss));
    }
    values
}

fn rsi_value(average_gain: Decimal, average_loss: Decimal) -> Decimal {
    if average_loss == Decimal::ZERO {
        return if average_gain == Decimal::ZERO {
            Decimal::from(50)
        } else {
            Decimal::from(100)
        };
    }
    let relative_strength = average_gain / average_loss;
    Decimal::from(100) - Decimal::from(100) / (Decimal::ONE + relative_strength)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candle(index: usize, open: i64, high: i64, low: i64, close: i64) -> Candle {
        Candle {
            open_time_ms: index as i64 * 60_000,
            close_time_ms: (index as i64 + 1) * 60_000 - 1,
            open: Decimal::from(open),
            high: Decimal::from(high),
            low: Decimal::from(low),
            close: Decimal::from(close),
            volume: Decimal::from(10),
            received_at_ms: None,
        }
    }

    fn bullish_history() -> Vec<Candle> {
        let closes = [100, 105, 95, 101, 98, 103, 99, 106, 102];
        closes
            .iter()
            .enumerate()
            .map(|(index, close)| {
                let open = if index == 0 {
                    *close
                } else {
                    closes[index - 1]
                };
                let mut low = open.min(*close) - 1;
                if index == 2 {
                    low = 90;
                } else if index == 5 {
                    low = 85;
                }
                candle(index, open, open.max(*close) + 1, low, *close)
            })
            .collect()
    }

    fn bearish_history() -> Vec<Candle> {
        let closes = [200, 195, 205, 199, 202, 197, 201, 194, 198];
        closes
            .iter()
            .enumerate()
            .map(|(index, close)| {
                let open = if index == 0 {
                    *close
                } else {
                    closes[index - 1]
                };
                let mut high = open.max(*close) + 1;
                if index == 2 {
                    high = 210;
                } else if index == 5 {
                    high = 215;
                }
                candle(index, open, high, open.min(*close) - 1, *close)
            })
            .collect()
    }

    fn strategy() -> PriceRsiDivergence {
        PriceRsiDivergence::new(DivergenceConfig {
            enabled: true,
            rsi_period: 2,
            pivot_left_bars: 1,
            pivot_right_bars: 1,
        })
        .unwrap()
    }

    #[test]
    fn divergence_is_disabled_by_default_and_parameters_are_validated() {
        let config = DivergenceConfig::default();
        assert!(!config.enabled);
        assert_eq!(
            PriceRsiDivergence::new(config)
                .unwrap()
                .evaluate(&bullish_history()),
            Direction::Hold
        );
        assert!(
            PriceRsiDivergence::new(DivergenceConfig {
                rsi_period: 1,
                ..DivergenceConfig::default()
            })
            .is_err()
        );
    }

    #[test]
    fn insufficient_history_invalid_candles_and_tied_pivots_fail_closed() {
        let detector = strategy();
        assert_eq!(
            detector.analyze(&bullish_history()[..3]),
            DivergenceAnalysis::InsufficientHistory
        );

        let mut invalid = bullish_history();
        invalid[4].volume = Decimal::from(-1);
        assert_eq!(detector.analyze(&invalid), DivergenceAnalysis::InvalidData);

        let mut irregular = bullish_history();
        for candle in &mut irregular[4..] {
            candle.open_time_ms += 60_000;
            candle.close_time_ms += 60_000;
        }
        assert_eq!(
            detector.analyze(&irregular),
            DivergenceAnalysis::InvalidData
        );

        let mut tied = bullish_history();
        tied[5].low = Decimal::from(90);
        assert_eq!(detector.evaluate(&tied[..7]), Direction::Hold);
        assert_eq!(
            detector.analyze(&tied[..7]),
            DivergenceAnalysis::NoDivergence
        );
    }

    #[test]
    fn bullish_and_bearish_divergences_wait_for_right_bars_and_use_only_closed_prefixes() {
        let detector = strategy();
        let bullish = bullish_history();
        assert_eq!(detector.evaluate(&bullish[..6]), Direction::Hold);
        assert_eq!(detector.evaluate(&bullish[..7]), Direction::Buy);
        let DivergenceAnalysis::Confirmed(signal) = detector.analyze(&bullish[..7]) else {
            panic!("second pivot must be confirmed by the right-hand closed bar");
        };
        assert_eq!(signal.direction, DivergenceDirection::Bullish);
        assert_eq!(signal.first_pivot_index, 2);
        assert_eq!(signal.second_pivot_index, 5);
        assert_eq!(signal.confirmation_index, 6);
        assert!(signal.first_rsi < signal.second_rsi);

        let bearish = bearish_history();
        assert_eq!(detector.evaluate(&bearish[..6]), Direction::Hold);
        assert_eq!(detector.evaluate(&bearish[..7]), Direction::Sell);
        let DivergenceAnalysis::Confirmed(signal) = detector.analyze(&bearish[..7]) else {
            panic!("bearish divergence should be confirmed");
        };
        assert_eq!(signal.direction, DivergenceDirection::Bearish);
        assert!(signal.first_rsi > signal.second_rsi);
    }

    #[test]
    fn confirmed_divergence_is_not_repeated_and_break_of_second_pivot_invalidates_it() {
        let detector = strategy();
        let history = bullish_history();
        assert_eq!(detector.evaluate(&history[..7]), Direction::Buy);
        assert_eq!(detector.evaluate(&history), Direction::Hold);
        assert_eq!(detector.evaluate(&history), Direction::Hold);

        let mut broken = history[..7].to_vec();
        broken.push(candle(7, 99, 101, 84, 100));
        broken.push(candle(8, 100, 102, 98, 101));
        let analysis = detector.analyze(&broken);
        assert!(matches!(
            analysis,
            DivergenceAnalysis::Invalidated {
                signal: DivergenceSignal {
                    direction: DivergenceDirection::Bullish,
                    ..
                },
                ..
            }
        ));
        assert_eq!(detector.evaluate(&broken), Direction::Hold);
    }
}
