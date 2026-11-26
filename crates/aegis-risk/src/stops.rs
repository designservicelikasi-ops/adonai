//! Advanced stop management: choosing the INITIAL stop of a new position.
//!
//! The stop is not forced to be short. Three candidates are measured on completed candles
//! (no look-ahead):
//! - **strategy**: the distance the strategy asked for;
//! - **structural**: behind the nearest confirmed swing low (BUY) / swing high (SELL), plus a
//!   buffer in ATR and the spread;
//! - **volatility**: a multiple of ATR.
//!
//! A candidate is valid only inside `[min, max]`: at least `min_atr` ATR, the broker's stops
//! level and a few spreads; at most `max_atr` ATR. The money at risk does NOT grow with a
//! wider stop: the risk engine sizes the volume on the stop distance (a wider stop = a smaller
//! volume), and refuses the trade if even the broker minimum volume exceeds the budget.
//!
//! The six stop roles AEGIS distinguishes are listed by [`StopRole`]; this module chooses the
//! first three, Position Intelligence handles break-even and trailing, the protection guard
//! handles the emergency stop.

use aegis_market::indicators::{atr, swing_levels};
use aegis_types::{Candle, Side, SymbolSpec};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopRole {
    /// Distance asked by the strategy.
    Initial,
    /// Behind a market structure (swing).
    Structural,
    /// Multiple of the recent volatility (ATR).
    Volatility,
    /// Moved to entry + costs (Position Intelligence SECURE).
    BreakEven,
    /// Following the price (Position Intelligence ADJUST SL).
    Trailing,
    /// Re-installed by the protection guard after the broker lost it.
    Emergency,
}

impl StopRole {
    pub fn label(self) -> &'static str {
        match self {
            StopRole::Initial => "stop initial (stratégie)",
            StopRole::Structural => "stop structurel",
            StopRole::Volatility => "stop de volatilité",
            StopRole::BreakEven => "break-even",
            StopRole::Trailing => "stop suiveur",
            StopRole::Emergency => "stop de protection d'urgence",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopMode {
    /// Keep the strategy's stop (behaviour of 0.2, default).
    Strategy,
    /// Prefer a structural stop, else a volatility stop, else the strategy's, within bounds.
    Adaptive,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StopPolicy {
    pub mode: StopMode,
    pub atr_period: usize,
    /// Volatility stop = `volatility_atr` x ATR.
    pub volatility_atr: f64,
    /// Swing confirmation: bars on each side of a pivot.
    pub swing_strength: usize,
    /// Bars searched for a swing.
    pub swing_lookback: usize,
    /// Buffer beyond the swing, in ATR.
    pub structure_buffer_atr: f64,
    pub min_atr: f64,
    pub max_atr: f64,
    /// The stop must be at least this many spreads away.
    pub min_spreads: f64,
    /// When the stop changes, scale the target to keep the strategy's reward:risk.
    pub keep_reward_ratio: bool,
}

impl Default for StopPolicy {
    fn default() -> Self {
        StopPolicy {
            mode: StopMode::Strategy,
            atr_period: 14,
            volatility_atr: 1.5,
            swing_strength: 3,
            swing_lookback: 60,
            structure_buffer_atr: 0.2,
            min_atr: 0.5,
            max_atr: 4.0,
            min_spreads: 3.0,
            keep_reward_ratio: true,
        }
    }
}

impl StopPolicy {
    pub fn validate(&self) -> Result<(), crate::ConfigError> {
        let bad = |problem| Err(crate::ConfigError { field: "stops", problem });
        if !(2..=200).contains(&self.atr_period) || !(1..=20).contains(&self.swing_strength) || !(10..=1_000).contains(&self.swing_lookback) {
            return bad("atr_period within 2..200, swing_strength within 1..20, swing_lookback within 10..1000");
        }
        let ok = |v: f64, lo: f64, hi: f64| v.is_finite() && v >= lo && v <= hi;
        if !ok(self.volatility_atr, 0.2, 20.0) || !ok(self.structure_buffer_atr, 0.0, 5.0) || !ok(self.min_spreads, 0.0, 100.0) {
            return bad("volatility_atr within 0.2..20, structure_buffer_atr within 0..5, min_spreads within 0..100");
        }
        if !ok(self.min_atr, 0.05, 20.0) || !ok(self.max_atr, 0.1, 50.0) || self.min_atr >= self.max_atr {
            return bad("min_atr within 0.05..20, max_atr within 0.1..50 and min_atr < max_atr");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StopCandidate {
    pub role: StopRole,
    /// Price distance from the entry; `None` when it cannot be measured.
    pub distance: Option<f64>,
    pub valid: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StopPlan {
    pub role: StopRole,
    pub sl_distance: f64,
    pub tp_distance: f64,
    /// True when the plan differs from what the strategy asked for.
    pub changed: bool,
    pub atr: Option<f64>,
    pub bounds: Option<(f64, f64)>,
    pub candidates: Vec<StopCandidate>,
    pub reason: String,
}

/// Choose the initial stop. `candles`: completed bars of the signal timeframe, oldest first.
pub fn plan_stop(
    candles: &[Candle],
    side: Side,
    entry: f64,
    spread: f64,
    spec: &SymbolSpec,
    strategy_sl: f64,
    strategy_tp: f64,
    policy: &StopPolicy,
) -> StopPlan {
    let keep = |reason: String, candidates: Vec<StopCandidate>, a: Option<f64>, bounds| StopPlan {
        role: StopRole::Initial,
        sl_distance: strategy_sl,
        tp_distance: strategy_tp,
        changed: false,
        atr: a,
        bounds,
        candidates,
        reason,
    };
    if policy.mode == StopMode::Strategy {
        return keep("stop de la stratégie (mode « stratégie »)".into(), Vec::new(), None, None);
    }
    let Some(a) = atr(candles, policy.atr_period).filter(|a| a.is_finite() && *a > 0.0) else {
        return keep("ATR indisponible: stop de la stratégie".into(), Vec::new(), None, None);
    };
    let min = (policy.min_atr * a).max(spec.stops_level_points * spec.point + spread).max(policy.min_spreads * spread);
    let max = policy.max_atr * a;
    if min >= max {
        return keep(format!("bornes incohérentes (min {min:.6} >= max {max:.6}): stop de la stratégie"), Vec::new(), Some(a), None);
    }
    let inside = |d: f64| d.is_finite() && d >= min - 1e-12 && d <= max + 1e-12;
    let dir = side.sign();

    // Structural: nearest confirmed swing on the protective side of the entry.
    let window = &candles[candles.len().saturating_sub(policy.swing_lookback)..];
    let (highs, lows) = swing_levels(window, policy.swing_strength, 12);
    // Most recent confirmed swing on the protective side that leaves at least the minimum room
    // (a minor pivot right next to the price is not a structure worth hiding behind).
    let levels = match side {
        Side::Buy => lows,
        Side::Sell => highs,
    };
    let structural = levels
        .into_iter()
        .filter(|lvl| (entry - lvl) * dir > 0.0)
        .map(|lvl| (entry - lvl) * dir + policy.structure_buffer_atr * a + spread)
        .find(|d| *d >= min - 1e-12);
    let volatility = policy.volatility_atr * a;
    let describe = |d: Option<f64>| match d {
        Some(d) if inside(d) => (true, format!("{:.1} ATR, dans les bornes", d / a)),
        Some(d) if d < min => (false, format!("{:.1} ATR: trop proche (min {:.1} ATR)", d / a, min / a)),
        Some(d) => (false, format!("{:.1} ATR: trop loin (max {:.1} ATR)", d / a, max / a)),
        None => (false, "aucun swing confirmé du bon côté du prix".into()),
    };
    let mut candidates = Vec::new();
    for (role, d) in [(StopRole::Structural, structural), (StopRole::Volatility, Some(volatility)), (StopRole::Initial, Some(strategy_sl))] {
        let (valid, reason) = describe(d);
        candidates.push(StopCandidate { role, distance: d, valid, reason });
    }
    let Some(pick) = candidates.iter().find(|c| c.valid).cloned() else {
        return keep("aucun stop dans les bornes: stop de la stratégie (le moteur de risque décidera)".into(), candidates, Some(a), Some((min, max)));
    };
    let sl = pick.distance.expect("valid candidates have a distance");
    let tp = if policy.keep_reward_ratio && strategy_sl > 0.0 { strategy_tp * sl / strategy_sl } else { strategy_tp };
    let changed = (sl - strategy_sl).abs() > spec.point * 0.5 || (tp - strategy_tp).abs() > spec.point * 0.5;
    StopPlan {
        role: pick.role,
        sl_distance: sl,
        tp_distance: tp,
        changed,
        atr: Some(a),
        bounds: Some((min, max)),
        reason: format!("{} à {:.1} ATR ({}){}", pick.role.label(), sl / a, pick.reason, if policy.keep_reward_ratio { "; cible ajustée au même rapport gain/risque" } else { "" }),
        candidates,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::eurusd;

    /// Flat closes at 1.1000, highs 1.1005, lows rising by a hair (so no bar is a pivot) except
    /// one clear swing low `swing_low` ten bars ago. ATR is about 15 points.
    fn bars(swing_low: f64) -> Vec<Candle> {
        (0..80)
            .map(|i| {
                let low = if i == 69 { swing_low } else { 1.0990 + i as f64 * 0.000001 };
                Candle { open_ts: i * 60_000, open: 1.1000, high: 1.1005, low, close: 1.1000, tick_volume: 1, spread_points: 10.0 }
            })
            .collect()
    }

    fn adaptive() -> StopPolicy {
        StopPolicy { mode: StopMode::Adaptive, ..Default::default() }
    }

    #[test]
    fn strategy_mode_changes_nothing() {
        let p = plan_stop(&bars(1.0980), Side::Buy, 1.1001, 0.0001, &eurusd(), 0.0010, 0.0020, &StopPolicy::default());
        assert!(!p.changed && p.sl_distance == 0.0010 && p.role == StopRole::Initial);
    }

    #[test]
    fn a_stop_is_placed_behind_the_structure_when_it_fits() {
        // Structural = 21 points to the swing + 0.2 ATR + spread: about 1.6 ATR.
        let p = plan_stop(&bars(1.0980), Side::Buy, 1.1001, 0.0001, &eurusd(), 0.0010, 0.0020, &adaptive());
        assert_eq!(p.role, StopRole::Structural, "{}", p.reason);
        assert!(p.sl_distance > 1.1001 - 1.0980, "beyond the swing low");
        assert!(p.changed);
        // Same reward:risk as the strategy (2:1).
        assert!((p.tp_distance / p.sl_distance - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_swing_too_far_falls_back_to_volatility_and_a_sell_looks_above() {
        let p = plan_stop(&bars(1.0900), Side::Buy, 1.1001, 0.0001, &eurusd(), 0.0010, 0.0020, &adaptive());
        assert_eq!(p.role, StopRole::Volatility, "{}", p.reason);
        assert!(p.candidates.iter().any(|c| c.role == StopRole::Structural && !c.valid && c.reason.contains("trop loin")));
        // SELL above every high of the series: no swing above the entry, the volatility stop is used.
        let p = plan_stop(&bars(1.0980), Side::Sell, 1.1010, 0.0001, &eurusd(), 0.0010, 0.0020, &adaptive());
        assert_eq!(p.role, StopRole::Volatility);
    }

    #[test]
    fn bounds_respect_the_broker_and_the_spread() {
        // Spread of 40 points: the minimum becomes 3 spreads = 120 points, above 4 ATR (~ 60 points): no candidate fits.
        let p = plan_stop(&bars(1.0980), Side::Buy, 1.1001, 0.0040, &eurusd(), 0.0010, 0.0020, &adaptive());
        assert!(!p.changed && p.role == StopRole::Initial, "{}", p.reason);
        assert!(StopPolicy { min_atr: 5.0, max_atr: 4.0, ..adaptive() }.validate().is_err());
        adaptive().validate().unwrap();
    }
}
