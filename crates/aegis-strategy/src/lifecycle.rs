//! Strategy lifecycle. Promotion is always a manual, evidence-gated action:
//! good historical results alone never move a strategy forward.

use aegis_types::DataOrigin;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyStage {
    /// Signals are computed and displayed only.
    Research,
    /// May trade automatically on a demo account.
    Demo,
    /// May trade automatically on a real account (when LIVE AUTO is armed).
    LiveApproved,
    Disabled,
}

impl StrategyStage {
    pub fn as_str(self) -> &'static str {
        match self {
            StrategyStage::Research => "research",
            StrategyStage::Demo => "demo",
            StrategyStage::LiveApproved => "live_approved",
            StrategyStage::Disabled => "disabled",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "research" => Some(StrategyStage::Research),
            "demo" => Some(StrategyStage::Demo),
            "live_approved" => Some(StrategyStage::LiveApproved),
            "disabled" => Some(StrategyStage::Disabled),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            StrategyStage::Research => "Recherche",
            StrategyStage::Demo => "Démo",
            StrategyStage::LiveApproved => "Approuvée réel",
            StrategyStage::Disabled => "Désactivée",
        }
    }
}

/// Evidence available when a promotion is requested.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    /// Out-of-sample backtest.
    pub oos_trades: u32,
    pub oos_net: f64,
    pub oos_origin: DataOrigin,
    /// Forward (demo) trades actually executed through the broker.
    pub forward_trades: u32,
    pub forward_net: f64,
}

pub const MIN_OOS_TRADES: u32 = 30;
pub const MIN_FORWARD_TRADES: u32 = 50;

/// Check whether a manual stage change is permitted. `simulator` relaxes the
/// data-origin rule so the software itself can be tested against the test adapter.
pub fn can_promote(from: StrategyStage, to: StrategyStage, ev: &Evidence, simulator: bool) -> Result<(), String> {
    use StrategyStage::*;
    match (from, to) {
        (a, b) if a == b => Err("la stratégie est déjà à ce stade".into()),
        // Stepping back or disabling is always allowed.
        (_, Disabled) | (_, Research) | (LiveApproved, Demo) => Ok(()),
        (Disabled, Demo) | (Research, Demo) => {
            if ev.oos_trades < MIN_OOS_TRADES {
                return Err(format!("{} transactions hors échantillon (minimum {MIN_OOS_TRADES})", ev.oos_trades));
            }
            if ev.oos_net <= 0.0 {
                return Err("résultat net hors échantillon négatif ou nul".into());
            }
            if ev.oos_origin != DataOrigin::Broker && !simulator {
                return Err("le backtest repose sur des données synthétiques: données broker requises".into());
            }
            Ok(())
        }
        (Demo, LiveApproved) => {
            if simulator {
                return Err("promotion vers le réel impossible avec le simulateur".into());
            }
            if ev.forward_trades < MIN_FORWARD_TRADES {
                return Err(format!("{} transactions en démo (minimum {MIN_FORWARD_TRADES})", ev.forward_trades));
            }
            if ev.forward_net <= 0.0 {
                return Err("résultat net en démo négatif ou nul".into());
            }
            Ok(())
        }
        _ => Err("une stratégie doit passer par le stade Démo avant le réel".into()),
    }
}

/// Lower bound of the Wilson score interval for a win rate — a conservative
/// probability estimate that shrinks towards zero when the sample is small.
pub fn wilson_lower_bound(wins: u32, n: u32, z: f64) -> Option<f64> {
    if n == 0 || wins > n {
        return None;
    }
    let n = n as f64;
    let p = wins as f64 / n;
    let z2 = z * z;
    let centre = p + z2 / (2.0 * n);
    let margin = z * ((p * (1.0 - p) + z2 / (4.0 * n)) / n).sqrt();
    Some(((centre - margin) / (1.0 + z2 / n)).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use StrategyStage::*;

    fn ev(oos_trades: u32, oos_net: f64, origin: DataOrigin, fwd: u32, fwd_net: f64) -> Evidence {
        Evidence { oos_trades, oos_net, oos_origin: origin, forward_trades: fwd, forward_net: fwd_net }
    }

    #[test]
    fn a_good_backtest_alone_never_reaches_live() {
        let great = ev(500, 9_999.0, DataOrigin::Broker, 0, 0.0);
        assert!(can_promote(Research, LiveApproved, &great, false).is_err());
        assert!(can_promote(Research, Demo, &great, false).is_ok());
        assert!(can_promote(Demo, LiveApproved, &great, false).is_err(), "no forward evidence");
    }

    #[test]
    fn research_to_demo_requires_real_oos_evidence() {
        assert!(can_promote(Research, Demo, &ev(10, 50.0, DataOrigin::Broker, 0, 0.0), false).is_err());
        assert!(can_promote(Research, Demo, &ev(40, -5.0, DataOrigin::Broker, 0, 0.0), false).is_err());
        assert!(can_promote(Research, Demo, &ev(40, 50.0, DataOrigin::Synthetic, 0, 0.0), false).is_err());
        assert!(can_promote(Research, Demo, &ev(40, 50.0, DataOrigin::Synthetic, 0, 0.0), true).is_ok());
        assert!(can_promote(Research, Demo, &ev(40, 50.0, DataOrigin::Broker, 0, 0.0), false).is_ok());
    }

    #[test]
    fn demo_to_live_requires_forward_trades_and_never_the_simulator() {
        let good = ev(40, 50.0, DataOrigin::Broker, 60, 120.0);
        assert!(can_promote(Demo, LiveApproved, &good, false).is_ok());
        assert!(can_promote(Demo, LiveApproved, &good, true).is_err());
        assert!(can_promote(Demo, LiveApproved, &ev(40, 50.0, DataOrigin::Broker, 49, 120.0), false).is_err());
        assert!(can_promote(Demo, LiveApproved, &ev(40, 50.0, DataOrigin::Broker, 60, -1.0), false).is_err());
    }

    #[test]
    fn demotion_is_always_possible() {
        let none = ev(0, 0.0, DataOrigin::Synthetic, 0, 0.0);
        assert!(can_promote(LiveApproved, Demo, &none, false).is_ok());
        assert!(can_promote(LiveApproved, Disabled, &none, false).is_ok());
        assert!(can_promote(Demo, Research, &none, false).is_ok());
        assert!(can_promote(Demo, Demo, &none, false).is_err());
    }

    #[test]
    fn wilson_bound_is_conservative() {
        assert_eq!(wilson_lower_bound(0, 0, 1.0), None);
        assert_eq!(wilson_lower_bound(5, 3, 1.0), None);
        let small = wilson_lower_bound(6, 10, 1.96).unwrap();
        let large = wilson_lower_bound(600, 1000, 1.96).unwrap();
        assert!(small < large && large < 0.6);
        assert!((large - 0.5693).abs() < 0.001, "{large}");
        assert!(wilson_lower_bound(10, 10, 1.96).unwrap() < 1.0);
        assert_eq!(wilson_lower_bound(0, 10, 1.96).unwrap(), 0.0);
    }

    #[test]
    fn stage_string_roundtrip() {
        for s in [Research, Demo, LiveApproved, Disabled] {
            assert_eq!(StrategyStage::parse(s.as_str()), Some(s));
        }
        assert_eq!(StrategyStage::parse("live"), None);
    }
}
