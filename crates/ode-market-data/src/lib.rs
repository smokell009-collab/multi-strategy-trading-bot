use ode_domain::Candle;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::{io::Read, str::FromStr};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MarketDataError {
    #[error("erro ao ler CSV: {0}")]
    Csv(#[from] csv::Error),
    #[error("histórico vazio")]
    Empty,
    #[error("intervalo esperado deve ser positivo")]
    InvalidInterval,
    #[error("linha {line}: campo {field} inválido ({value})")]
    InvalidField {
        line: usize,
        field: &'static str,
        value: String,
    },
    #[error("histórico inválido na linha {line}: {reason}")]
    InvalidHistory { line: usize, reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataQualityReport {
    pub bars: usize,
    pub gap_count: usize,
    pub latency_samples: usize,
    pub latency_p50_ms: Option<i64>,
    pub latency_p95_ms: Option<i64>,
    pub latency_max_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RawCandle {
    open_time_ms: String,
    close_time_ms: String,
    open: String,
    high: String,
    low: String,
    close: String,
    volume: String,
    received_at_ms: Option<String>,
}

/// CSV obrigatório: open_time_ms,close_time_ms,open,high,low,close,volume;
/// `received_at_ms` é opcional e só deve ser preenchido se o instante original de recebimento for conhecido.
pub fn read_csv<R: Read>(
    reader: R,
    interval_ms: i64,
) -> Result<(Vec<Candle>, DataQualityReport), MarketDataError> {
    if interval_ms <= 0 {
        return Err(MarketDataError::InvalidInterval);
    }
    let mut csv = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(reader);
    let mut candles = Vec::new();
    for (zero_index, row) in csv.deserialize::<RawCandle>().enumerate() {
        let line = zero_index + 2;
        let raw = row?;
        candles.push(Candle {
            open_time_ms: parse_i64(line, "open_time_ms", raw.open_time_ms)?,
            close_time_ms: parse_i64(line, "close_time_ms", raw.close_time_ms)?,
            open: parse_decimal(line, "open", raw.open)?,
            high: parse_decimal(line, "high", raw.high)?,
            low: parse_decimal(line, "low", raw.low)?,
            close: parse_decimal(line, "close", raw.close)?,
            volume: parse_decimal(line, "volume", raw.volume)?,
            received_at_ms: raw
                .received_at_ms
                .filter(|v| !v.is_empty())
                .map(|v| parse_i64(line, "received_at_ms", v))
                .transpose()?,
        });
    }
    let report = validate_candles(&candles, interval_ms)?;
    Ok((candles, report))
}

fn parse_i64(line: usize, field: &'static str, value: String) -> Result<i64, MarketDataError> {
    value
        .parse()
        .map_err(|_| MarketDataError::InvalidField { line, field, value })
}

fn parse_decimal(
    line: usize,
    field: &'static str,
    value: String,
) -> Result<Decimal, MarketDataError> {
    Decimal::from_str(&value).map_err(|_| MarketDataError::InvalidField { line, field, value })
}

pub fn validate_candles(
    candles: &[Candle],
    interval_ms: i64,
) -> Result<DataQualityReport, MarketDataError> {
    if interval_ms <= 0 {
        return Err(MarketDataError::InvalidInterval);
    }
    if candles.is_empty() {
        return Err(MarketDataError::Empty);
    }
    let mut latencies = Vec::new();
    for (index, candle) in candles.iter().enumerate() {
        let line = index + 1;
        if candle.open_time_ms < 0 || candle.close_time_ms < candle.open_time_ms {
            return Err(MarketDataError::InvalidHistory {
                line,
                reason: "timestamps negativos ou close anterior ao open".into(),
            });
        }
        if candle.open <= Decimal::ZERO
            || candle.high <= Decimal::ZERO
            || candle.low <= Decimal::ZERO
            || candle.close <= Decimal::ZERO
        {
            return Err(MarketDataError::InvalidHistory {
                line,
                reason: "preços OHLC devem ser positivos".into(),
            });
        }
        if candle.high < candle.open.max(candle.close)
            || candle.low > candle.open.min(candle.close)
            || candle.high < candle.low
        {
            return Err(MarketDataError::InvalidHistory {
                line,
                reason: "OHLC inconsistente".into(),
            });
        }
        if candle.volume < Decimal::ZERO {
            return Err(MarketDataError::InvalidHistory {
                line,
                reason: "volume negativo".into(),
            });
        }
        if let Some(received_at_ms) = candle.received_at_ms {
            let latency = received_at_ms.saturating_sub(candle.close_time_ms);
            if latency < 0 {
                return Err(MarketDataError::InvalidHistory {
                    line,
                    reason: "recebimento anterior ao fechamento do candle".into(),
                });
            }
            latencies.push(latency);
        }
        if let Some(previous) = index.checked_sub(1).and_then(|i| candles.get(i)) {
            let delta = candle.open_time_ms.saturating_sub(previous.open_time_ms);
            if delta <= 0 {
                return Err(MarketDataError::InvalidHistory {
                    line,
                    reason: "candles duplicados ou fora de ordem".into(),
                });
            }
            if delta != interval_ms {
                return Err(MarketDataError::InvalidHistory {
                    line,
                    reason: format!(
                        "cadência irregular (intervalo {delta} ms, esperado {interval_ms} ms)"
                    ),
                });
            }
        }
    }
    latencies.sort_unstable();
    let percentile = |percent: usize| -> Option<i64> {
        (!latencies.is_empty()).then(|| {
            let rank = (latencies.len() * percent).div_ceil(100).max(1);
            latencies[rank.saturating_sub(1).min(latencies.len() - 1)]
        })
    };
    Ok(DataQualityReport {
        bars: candles.len(),
        gap_count: 0,
        latency_samples: latencies.len(),
        latency_p50_ms: percentile(50),
        latency_p95_ms: percentile(95),
        latency_max_ms: latencies.last().copied(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const HEADER: &str = "open_time_ms,close_time_ms,open,high,low,close,volume,received_at_ms\n";

    #[test]
    fn imports_ohlcv_and_reports_latency_percentiles() {
        let csv = format!(
            "{HEADER}0,59999,10,12,9,11,100,60010\n60000,119999,11,13,10,12,120,120020\n120000,179999,12,14,11,13,90,180030\n"
        );
        let (candles, report) = read_csv(Cursor::new(csv), 60_000).unwrap();
        assert_eq!(candles.len(), 3);
        assert_eq!(candles[1].volume, Decimal::from(120));
        assert_eq!(report.latency_samples, 3);
        assert_eq!(report.latency_p50_ms, Some(21));
        assert_eq!(report.latency_p95_ms, Some(31));
    }

    #[test]
    fn rejects_gaps_bad_ohlc_and_negative_volume() {
        let gap =
            format!("{HEADER}0,59999,10,12,9,11,100,60000\n120000,179999,11,13,10,12,120,180000\n");
        assert!(
            read_csv(Cursor::new(gap), 60_000)
                .unwrap_err()
                .to_string()
                .contains("cadência irregular")
        );
        let bad_ohlc = format!("{HEADER}0,59999,10,9,8,11,100,60000\n");
        assert!(
            read_csv(Cursor::new(bad_ohlc), 60_000)
                .unwrap_err()
                .to_string()
                .contains("OHLC inconsistente")
        );
        let bad_volume = format!("{HEADER}0,59999,10,12,9,11,-1,60000\n");
        assert!(
            read_csv(Cursor::new(bad_volume), 60_000)
                .unwrap_err()
                .to_string()
                .contains("volume negativo")
        );
    }

    #[test]
    fn historical_csv_can_omit_unknown_receive_times() {
        let csv = "open_time_ms,close_time_ms,open,high,low,close,volume\n0,59999,10,12,9,11,100\n";
        let (candles, report) = read_csv(Cursor::new(csv), 60_000).unwrap();
        assert_eq!(candles[0].received_at_ms, None);
        assert_eq!(report.latency_samples, 0);
    }
}
