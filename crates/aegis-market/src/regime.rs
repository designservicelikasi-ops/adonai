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

// ------------------------------------------------------------------ detailed regimes (0.3)

/// Main character of the market (Part 2.2 of the directive). Rules, on completed bars:
/// - `TrendUp` / `TrendDown`: efficiency ratio ER(20) >= 0.30 and EMA20 above / below EMA50;
/// - `Transition`: a trend by the rule above, but EMA20 and EMA50 crossed in the last 10 bars,
///   or the last 10 bars move efficiently (ER(10) >= 0.4) AGAINST the trend;
/// - `Chaotic`: high volatility (ATR >= 1.8 x normal) with ER(20) < 0.20 (violent and directionless);
/// - `Consolidation`: ER(20) < 0.30 with low or compressing volatility;
/// - `Range`: ER(20) < 0.30 otherwise;
/// - `Unknown`: fewer than 60 bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegimeKind {
    TrendUp,
    TrendDown,
    Transition,
    Consolidation,
    Range,
    Chaotic,
    Unknown,
}

impl RegimeKind {
    pub fn label(self) -> &'static str {
        match self {
            RegimeKind::TrendUp => "tendance haussière",
            RegimeKind::TrendDown => "tendance baissière",
            RegimeKind::Transition => "transition de tendance",
            RegimeKind::Consolidation => "consolidation",
            RegimeKind::Range => "marché latéral",
            RegimeKind::Chaotic => "marché désordonné",
            RegimeKind::Unknown => "indéterminé",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            RegimeKind::TrendUp => "trend_up",
            RegimeKind::TrendDown => "trend_down",
            RegimeKind::Transition => "transition",
            RegimeKind::Consolidation => "consolidation",
            RegimeKind::Range => "range",
            RegimeKind::Chaotic => "chaotic",
            RegimeKind::Unknown => "unknown",
        }
    }
}

/// Volatility level: ATR(14) / median true range of 100 bars (>= 1.8 high, <= 0.6 low).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolatilityLevel {
    High,
    Normal,
    Low,
}

/// Volatility dynamics: ATR(5) / ATR(20) (> 1.3 expanding, < 0.75 compressing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolatilityDynamics {
    Expanding,
    Compressing,
    Stable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegimeDetail {
    pub kind: RegimeKind,
    pub volatility: VolatilityLevel,
    pub dynamics: VolatilityDynamics,
    pub efficiency: f64,
    pub volatility_ratio: f64,
    /// Short-term / long-term ATR.
    pub dynamics_ratio: f64,
    /// Conditions added by the engine: "conditions_anormales", "annonce_proche".
    pub flags: Vec<String>,
}

impl RegimeDetail {
    pub fn unknown() -> RegimeDetail {
        RegimeDetail { kind: RegimeKind::Unknown, volatility: VolatilityLevel::Normal, dynamics: VolatilityDynamics::Stable, efficiency: 0.0, volatility_ratio: 0.0, dynamics_ratio: 0.0, flags: Vec::new() }
    }
    pub fn describe(&self) -> String {
        let vol = match self.volatility {
            VolatilityLevel::High => "forte volatilité",
            VolatilityLevel::Normal => "volatilité normale",
            VolatilityLevel::Low => "faible volatilité",
        };
        let dynm = match self.dynamics {
            VolatilityDynamics::Expanding => ", en expansion",
            VolatilityDynamics::Compressing => ", en compression",
            VolatilityDynamics::Stable => "",
        };
        let flags = if self.flags.is_empty() { String::new() } else { format!(" [{}]", self.flags.join(", ")) };
        format!("{} — {vol}{dynm}{flags}", self.kind.label())
    }
}

pub fn detect_regime_detail(candles: &[Candle]) -> RegimeDetail {
    if candles.len() < REGIME_MIN_BARS {
        return RegimeDetail::unknown();
    }
    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
    let (Some(er), Some(a14), Some(a5), Some(a20)) = (efficiency_ratio(&closes, 20), atr(candles, 14), atr(candles, 5), atr(candles, 20)) else {
        return RegimeDetail::unknown();
    };
    let window = &candles[candles.len().saturating_sub(101)..];
    let trs: Vec<f64> = (1..window.len()).map(|i| true_range(window[i - 1].close, &window[i])).collect();
    let med = median(&trs).unwrap_or(0.0);
    let vr = if med > 0.0 { a14 / med } else { 0.0 };
    let dr = if a20 > 0.0 { a5 / a20 } else { 1.0 };
    let volatility = if vr >= HIGH_VOL_RATIO { VolatilityLevel::High } else if vr <= 0.6 { VolatilityLevel::Low } else { VolatilityLevel::Normal };
    let dynamics = if dr > 1.3 { VolatilityDynamics::Expanding } else if dr < 0.75 { VolatilityDynamics::Compressing } else { VolatilityDynamics::Stable };
    let e20 = crate::indicators::ema_series(&closes, 20);
    let e50 = crate::indicators::ema_series(&closes, 50);
    let rel = |i: usize| match (e20[i], e50[i]) {
        (Some(a), Some(b)) => (a - b).signum(),
        _ => 0.0,
    };
    let n = closes.len();
    let now_rel = rel(n - 1);
    let crossed_recently = (n.saturating_sub(10)..n - 1).any(|i| rel(i) != 0.0 && rel(i) != now_rel);
    let er10 = efficiency_ratio(&closes, 10).unwrap_or(0.0);
    let recent_dir = (closes[n - 1] - closes[n.saturating_sub(11)]).signum();
    let kind = if volatility == VolatilityLevel::High && er < 0.20 {
        RegimeKind::Chaotic
    } else if er >= TREND_EFFICIENCY && now_rel != 0.0 {
        if crossed_recently || (er10 >= 0.4 && recent_dir != 0.0 && recent_dir != now_rel) {
            RegimeKind::Transition
        } else if now_rel > 0.0 {
            RegimeKind::TrendUp
        } else {
            RegimeKind::TrendDown
        }
    } else if volatility == VolatilityLevel::Low || dynamics == VolatilityDynamics::Compressing {
        RegimeKind::Consolidation
    } else {
        RegimeKind::Range
    };
    RegimeDetail { kind, volatility, dynamics, efficiency: er, volatility_ratio: vr, dynamics_ratio: dr, flags: Vec::new() }
}

#[cfg(test)]
mod detail_tests {
    use super::*;

    fn series(f: impl Fn(usize) -> f64, n: usize, range: impl Fn(usize) -> f64) -> Vec<Candle> {
        (0..n)
            .map(|i| {
                let c = f(i);
                let o = if i == 0 { c } else { f(i - 1) };
                let r = range(i);
                Candle { open_ts: i as i64 * 60_000, open: o, high: o.max(c) + r, low: o.min(c) - r, close: c, tick_volume: 1, spread_points: 0.0 }
            })
            .collect()
    }

    #[test]
    fn the_detailed_regimes_follow_their_rules() {
        let up = detect_regime_detail(&series(|i| 1.0 + i as f64 * 0.001, 150, |_| 0.0002));
        assert_eq!(up.kind, RegimeKind::TrendUp);
        let down = detect_regime_detail(&series(|i| 2.0 - i as f64 * 0.001, 150, |_| 0.0002));
        assert_eq!(down.kind, RegimeKind::TrendDown);
        // Trend, then a sharp efficient move against it: transition.
        let turn = detect_regime_detail(&series(|i| if i < 140 { 1.0 + i as f64 * 0.001 } else { 1.139 - (i - 139) as f64 * 0.003 }, 150, |_| 0.0002));
        assert_eq!(turn.kind, RegimeKind::Transition, "{turn:?}");
        // Quiet oscillation getting quieter: consolidation, compressing.
        let amp = |i: usize| if i > 120 { 0.00004 } else { 0.0005 };
        let quiet = detect_regime_detail(&series(|i| 1.0 + if i % 2 == 0 { amp(i) } else { -amp(i) }, 150, |i| amp(i) / 2.0));
        assert_eq!(quiet.kind, RegimeKind::Consolidation, "{quiet:?}");
        // Violent and directionless at the end: chaotic, high volatility, expanding.
        let wild = detect_regime_detail(&series(|i| 1.0 + if i % 2 == 0 { 0.0002 } else { -0.0002 } * if i > 135 { 20.0 } else { 1.0 }, 150, |_| 0.0001));
        assert_eq!((wild.kind, wild.volatility), (RegimeKind::Chaotic, VolatilityLevel::High), "{wild:?}");
        assert_eq!(wild.dynamics, VolatilityDynamics::Expanding);
        // Range: directionless oscillation of normal size.
        let range = detect_regime_detail(&series(|i| 1.0 + if i % 2 == 0 { 0.001 } else { -0.001 }, 150, |_| 0.0002));
        assert_eq!(range.kind, RegimeKind::Range, "{range:?}");
        assert_eq!(detect_regime_detail(&series(|i| i as f64, 10, |_| 0.1)).kind, RegimeKind::Unknown);
    }
}
