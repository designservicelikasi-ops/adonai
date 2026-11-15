//! Market regime and trading-session detection (rule-based, explainable).

use crate::indicators::{atr, efficiency_ratio, ema, median, true_range};
use aegis_types::{Candle, Regime, TimestampMs};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RegimeReading {
    pub regime: Regime,
    /// Kaufman efficiency ratio over 20 bars (0 = noise, 1 = straight trend).
    pub efficiency: f64,
    /// ATR(14) divided by the median true range of the last 100 bars.
    pub volatility_ratio: f64,
}

pub const REGIME_MIN_BARS: usize = 60;
const TREND_EFFICIENCY: f64 = 0.30;
const HIGH_VOL_RATIO: f64 = 1.8;

/// Classify the regime from completed candles (oldest first).
pub fn detect_regime(candles: &[Candle]) -> RegimeReading {
    let unknown = RegimeReading { regime: Regime::Unknown, efficiency: 0.0, volatility_ratio: 0.0 };
    if candles.len() < REGIME_MIN_BARS {
        return unknown;
    }
    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
    let (Some(er), Some(a), Some(fast), Some(slow)) =
        (efficiency_ratio(&closes, 20), atr(candles, 14), ema(&closes, 20), ema(&closes, 50))
    else {
        return unknown;
    };
    let window = &candles[candles.len().saturating_sub(101)..];
    let trs: Vec<f64> = (1..window.len()).map(|i| true_range(window[i - 1].close, &window[i])).collect();
    let med = median(&trs).unwrap_or(0.0);
    let vol_ratio = if med > 0.0 { a / med } else { 0.0 };
    let regime = if vol_ratio >= HIGH_VOL_RATIO {
        Regime::HighVolatility
    } else if er >= TREND_EFFICIENCY && fast > slow {
        Regime::TrendUp
    } else if er >= TREND_EFFICIENCY && fast < slow {
        Regime::TrendDown
    } else {
        Regime::Range
    };
    RegimeReading { regime, efficiency: er, volatility_ratio: vol_ratio }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Session {
    Closed,
    Asia,
    London,
    LondonNewYork,
    NewYork,
    OffHours,
}

impl Session {
    pub fn label(self) -> &'static str {
        match self {
            Session::Closed => "Marché fermé (week-end)",
            Session::Asia => "Asie",
            Session::London => "Londres",
            Session::LondonNewYork => "Londres + New York",
            Session::NewYork => "New York",
            Session::OffHours => "Hors session",
        }
    }
}

/// FX session from a UTC timestamp (fixed UTC hours; daylight-saving shifts are ignored).
pub fn session_at(ts_ms: TimestampMs) -> Session {
    let day = ts_ms.div_euclid(86_400_000);
    let hour = ts_ms.rem_euclid(86_400_000) / 3_600_000;
    // 1970-01-01 was a Thursday: 0=Thu 1=Fri 2=Sat 3=Sun 4=Mon ...
    let weekday = day.rem_euclid(7);
    let closed = weekday == 2 || (weekday == 1 && hour >= 22) || (weekday == 3 && hour < 22);
    if closed {
        return Session::Closed;
    }
    match hour {
        0..=6 => Session::Asia,
        7..=11 => Session::London,
        12..=15 => Session::LondonNewYork,
        16..=20 => Session::NewYork,
        _ => Session::OffHours,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(f: impl Fn(usize) -> f64, n: usize, range: f64) -> Vec<Candle> {
        (0..n)
            .map(|i| {
                let c = f(i);
                let o = if i == 0 { c } else { f(i - 1) };
                Candle {
                    open_ts: i as i64 * 60_000,
                    open: o,
                    high: o.max(c) + range,
                    low: o.min(c) - range,
                    close: c,
                    tick_volume: 1,
                    spread_points: 0.0,
                }
            })
            .collect()
    }

    #[test]
    fn detects_trends_range_and_unknown() {
        let up = series(|i| 1.0 + i as f64 * 0.001, 120, 0.0002);
        assert_eq!(detect_regime(&up).regime, Regime::TrendUp);
        let down = series(|i| 2.0 - i as f64 * 0.001, 120, 0.0002);
        assert_eq!(detect_regime(&down).regime, Regime::TrendDown);
        let chop = series(|i| 1.0 + if i % 2 == 0 { 0.001 } else { -0.001 }, 120, 0.0002);
        let r = detect_regime(&chop);
        assert_eq!(r.regime, Regime::Range);
        assert!(r.efficiency < 0.1);
        assert_eq!(detect_regime(&up[..30]).regime, Regime::Unknown);
    }

    #[test]
    fn detects_volatility_expansion() {
        // Quiet for 100 bars then bars five times larger.
        let mut bars = series(|i| 1.0 + if i % 2 == 0 { 0.0005 } else { -0.0005 }, 100, 0.0001);
        let burst = series(|i| 1.0 + if i % 2 == 0 { 0.006 } else { -0.006 }, 30, 0.001);
        for (k, mut c) in burst.into_iter().enumerate() {
            c.open_ts = (100 + k as i64) * 60_000;
            bars.push(c);
        }
        let r = detect_regime(&bars);
        assert_eq!(r.regime, Regime::HighVolatility, "{r:?}");
    }

    #[test]
    fn sessions() {
        let h = 3_600_000;
        let monday = 4 * 86_400_000; // 1970-01-05
        assert_eq!(session_at(monday + 3 * h), Session::Asia);
        assert_eq!(session_at(monday + 8 * h), Session::London);
        assert_eq!(session_at(monday + 13 * h), Session::LondonNewYork);
        assert_eq!(session_at(monday + 18 * h), Session::NewYork);
        assert_eq!(session_at(monday + 22 * h), Session::OffHours);
        let saturday = 2 * 86_400_000;
        assert_eq!(session_at(saturday + 12 * h), Session::Closed);
        let friday = 86_400_000;
        assert_eq!(session_at(friday + 23 * h), Session::Closed);
        assert_eq!(session_at(friday + 10 * h), Session::London);
        let sunday = 3 * 86_400_000;
        assert_eq!(session_at(sunday + 10 * h), Session::Closed);
        assert_eq!(session_at(sunday + 23 * h), Session::OffHours);
    }
}
