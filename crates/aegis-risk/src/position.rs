//! Position Intelligence: what to do with an open position.
//!
//! HOLD, SECURE (stop moved to break-even plus costs), ADJUST SL (trailing),
//! ADJUST TP (target brought closer when the move stalls) or CLOSE (time stop,
//! net target reached). Rules that can never be broken:
//! - a stop-loss only moves in the protective direction (never widened);
//! - levels respect the broker's stops level and freeze level;
//! - "profit" means the NET result realizable now, after closing costs.

use crate::config::ConfigError;
use aegis_types::Side;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PositionPolicy {
    pub enabled: bool,
    /// SECURE once the favourable move reaches this multiple of the initial risk R.
    pub secure_at_r: f64,
    /// Points added beyond break-even + costs when securing.
    pub secure_buffer_points: f64,
    /// Trailing starts at this multiple of R. 0 = no trailing.
    pub trail_from_r: f64,
    /// Trailing distance in ATR of the signal timeframe.
    pub trail_atr: f64,
    /// After this many bars without reaching the target, bring the target to
    /// `tp_decay_atr` ATR from the price (only closer). 0 = off.
    pub tp_decay_bars: usize,
    pub tp_decay_atr: f64,
    /// Close after this many bars. 0 = off.
    pub max_bars_held: usize,
    /// Close when the net result realizable now reaches this amount (account currency). 0 = off.
    pub close_at_net: f64,
    /// Smallest stop/target move worth sending, in points.
    pub min_step_points: f64,
    /// CLOSE PARTIAL once the favourable move reaches this multiple of R (once per position).
    /// 0 = off. Both the closed part and the rest must respect the broker's volume rules.
    #[serde(default)]
    pub partial_close_at_r: f64,
    /// Share of the volume closed by CLOSE PARTIAL (rounded DOWN to the volume step).
    #[serde(default = "default_partial_fraction")]
    pub partial_close_fraction: f64,
}

fn default_partial_fraction() -> f64 {
    0.5
}

impl Default for PositionPolicy {
    fn default() -> Self {
        PositionPolicy {
            enabled: true,
            secure_at_r: 0.6,
            secure_buffer_points: 1.0,
            trail_from_r: 1.0,
            trail_atr: 1.0,
            tp_decay_bars: 0,
            tp_decay_atr: 0.3,
            max_bars_held: 0,
            close_at_net: 0.0,
            min_step_points: 3.0,
            partial_close_at_r: 0.0,
            partial_close_fraction: 0.5,
        }
    }
}

impl PositionPolicy {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let bad = |field, problem| Err(ConfigError { field, problem });
        if !(0.1..=10.0).contains(&self.secure_at_r) {
            return bad("position.secure_at_r", "must be within 0.1..10");
        }
        if !(0.0..=10_000.0).contains(&self.secure_buffer_points) || !(0.0..=10_000.0).contains(&self.min_step_points) {
            return bad("position.secure_buffer_points", "points must be within 0..10000");
        }
        if !(0.0..=20.0).contains(&self.trail_from_r) || !(0.1..=20.0).contains(&self.trail_atr) {
            return bad("position.trail_atr", "trail_from_r within 0..20, trail_atr within 0.1..20");
        }
        if self.tp_decay_bars > 10_000 || !(0.05..=20.0).contains(&self.tp_decay_atr) || self.max_bars_held > 100_000 {
            return bad("position.tp_decay_bars", "bars out of range or tp_decay_atr outside 0.05..20");
        }
        if !(self.close_at_net.is_finite() && self.close_at_net >= 0.0) {
            return bad("position.close_at_net", "must be >= 0");
        }
        if !(0.0..=20.0).contains(&self.partial_close_at_r) || !(0.05..=0.95).contains(&self.partial_close_fraction) {
            return bad("position.partial_close_at_r", "partial_close_at_r within 0..20, partial_close_fraction within 0.05..0.95");
        }
        Ok(())
    }
}

/// Everything the manager needs to know about one position, prepared by the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManagedPosition {
    pub ticket: i64,
    pub side: Side,
    pub entry: f64,
    pub sl: f64,
    pub tp: f64,
    /// Stop-loss at opening: defines the initial risk R.
    pub initial_sl: f64,
    pub bars_held: usize,
    pub bid: f64,
    pub ask: f64,
    pub atr: Option<f64>,
    pub point: f64,
    pub digits: u32,
    pub stops_level_points: f64,
    pub freeze_level_points: f64,
    /// Price distance that pays the round-trip commission and slippage of this volume.
    pub cost_cover_price: f64,
    /// Net result if closed now, after estimated closing costs (account currency).
    pub net_realizable: f64,
    /// Volume and broker volume rules (CLOSE PARTIAL).
    #[serde(default)]
    pub volume: f64,
    #[serde(default)]
    pub volume_min: f64,
    #[serde(default)]
    pub volume_step: f64,
    /// A partial close was already done on this position.
    #[serde(default)]
    pub partial_done: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionActionKind {
    Hold,
    Secure,
    AdjustSl,
    AdjustTp,
    /// Close part of the volume (`PositionAction::volume`).
    ClosePartial,
    Close,
}

impl PositionActionKind {
    pub fn label(self) -> &'static str {
        match self {
            PositionActionKind::Hold => "HOLD",
            PositionActionKind::Secure => "SECURE",
            PositionActionKind::AdjustSl => "ADJUST SL",
            PositionActionKind::AdjustTp => "ADJUST TP",
            PositionActionKind::ClosePartial => "CLOSE PARTIAL",
            PositionActionKind::Close => "CLOSE",
        }
    }
}

/// The decision, with the complete target levels (a modification sends both).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PositionAction {
    pub kind: PositionActionKind,
    pub sl: f64,
    pub tp: f64,
    pub reason: String,
    /// Volume to close (CLOSE PARTIAL only).
    #[serde(default)]
    pub volume: Option<f64>,
}

/// Volume of a partial close: `fraction` of `volume` rounded DOWN to the step, such that both the
/// closed part and the rest are at least the broker minimum. `None` when impossible.
pub fn partial_volume(volume: f64, fraction: f64, volume_min: f64, volume_step: f64) -> Option<f64> {
    if !(volume > 0.0 && volume_min > 0.0 && volume_step > 0.0 && fraction > 0.0 && fraction < 1.0) {
        return None;
    }
    let part = ((volume * fraction + 1e-9) / volume_step).floor() * volume_step;
    let part = aegis_types::round_to(part, 8);
    let rest = aegis_types::round_to(volume - part, 8);
    let aligned = |v: f64| ((v / volume_step) - (v / volume_step).round()).abs() < 1e-6;
    (part + 1e-9 >= volume_min && rest + 1e-9 >= volume_min && aligned(rest)).then_some(part)
}

fn round(v: f64, digits: u32) -> f64 {
    aegis_types::round_to(v, digits)
}

pub fn manage(p: &ManagedPosition, policy: &PositionPolicy) -> PositionAction {
    let hold = |reason: String| PositionAction { kind: PositionActionKind::Hold, sl: p.sl, tp: p.tp, reason, volume: None };
    if !policy.enabled {
        return hold("gestion automatique désactivée".into());
    }
    let valid = [p.entry, p.bid, p.ask, p.point].iter().all(|v| v.is_finite() && *v > 0.0) && p.ask >= p.bid;
    if !valid {
        return hold("données de prix invalides: aucune action".into());
    }
    if policy.max_bars_held > 0 && p.bars_held >= policy.max_bars_held {
        return PositionAction { kind: PositionActionKind::Close, sl: p.sl, tp: p.tp, reason: format!("durée maximale atteinte ({} bougies)", p.bars_held), volume: None };
    }
    if policy.close_at_net > 0.0 && p.net_realizable >= policy.close_at_net {
        return PositionAction {
            kind: PositionActionKind::Close,
            sl: p.sl,
            tp: p.tp,
            reason: format!("résultat net réalisable {:.2} ≥ objectif de sortie {:.2}", p.net_realizable, policy.close_at_net),
            volume: None,
        };
    }

    let dir = p.side.sign();
    // The price at which the position would be closed now.
    let price = match p.side {
        Side::Buy => p.bid,
        Side::Sell => p.ask,
    };
    let freeze = p.freeze_level_points * p.point;
    let frozen = freeze > 0.0 && ((p.sl > 0.0 && (price - p.sl).abs() <= freeze) || (p.tp > 0.0 && (p.tp - price).abs() <= freeze));
    if frozen {
        return hold("prix dans la zone de gel du broker: modification impossible".into());
    }
    let r = (p.entry - p.initial_sl) * dir;
    if !(r > 0.0) {
        return hold("risque initial inconnu: aucune modification automatique".into());
    }
    let moved = (price - p.entry) * dir;
    let in_r = moved / r;
    let min_dist = p.stops_level_points * p.point;
    let step = policy.min_step_points * p.point;
    // Most protective stop the broker accepts right now.
    let limit = price - dir * min_dist;

    let mut candidate: Option<(f64, PositionActionKind, String)> = None;
    if in_r >= policy.secure_at_r {
        let be = p.entry + dir * (p.cost_cover_price + policy.secure_buffer_points * p.point);
        candidate = Some((be, PositionActionKind::Secure, format!("gain de {in_r:.2} R: stop au point mort + coûts ({be:.*})", p.digits as usize)));
    }
    if policy.trail_from_r > 0.0 && in_r >= policy.trail_from_r {
        if let Some(a) = p.atr.filter(|a| a.is_finite() && *a > 0.0) {
            let trail = price - dir * policy.trail_atr * a;
            let better = candidate.as_ref().is_none_or(|(c, _, _)| (trail - c) * dir > 0.0);
            if better {
                candidate = Some((trail, PositionActionKind::AdjustSl, format!("stop suiveur à {} ATR ({trail:.*}), gain de {in_r:.2} R", policy.trail_atr, p.digits as usize)));
            }
        }
    }
    if let Some((mut sl, mut kind, mut reason)) = candidate {
        // Respect the broker's minimum distance.
        if (sl - limit) * dir > 0.0 {
            sl = limit;
            reason.push_str(" — limité par la distance minimale du broker");
        }
        let sl = round(sl, p.digits);
        let improves = p.sl <= 0.0 || (sl - p.sl) * dir >= step;
        if improves {
            // SECURE only when the stop crosses from the loss side to the protected side.
            let was_at_risk = p.sl <= 0.0 || (p.sl - p.entry) * dir < 0.0;
            if kind == PositionActionKind::Secure && !was_at_risk {
                kind = PositionActionKind::AdjustSl;
            } else if kind == PositionActionKind::AdjustSl && was_at_risk && (sl - p.entry) * dir >= 0.0 {
                kind = PositionActionKind::Secure;
                reason = format!("{reason} (sécurisation)");
            }
            return PositionAction { kind, sl, tp: p.tp, reason, volume: None };
        }
    }

    // CLOSE PARTIAL: take part of the gain once, keep the rest running behind its stop.
    if policy.partial_close_at_r > 0.0 && !p.partial_done && in_r >= policy.partial_close_at_r && p.net_realizable > 0.0 {
        if let Some(v) = partial_volume(p.volume, policy.partial_close_fraction, p.volume_min, p.volume_step) {
            return PositionAction {
                kind: PositionActionKind::ClosePartial,
                sl: p.sl,
                tp: p.tp,
                reason: format!("gain de {in_r:.2} R: clôture partielle de {v} lot(s) sur {}", p.volume),
                volume: Some(v),
            };
        }
    }

    if policy.tp_decay_bars > 0 && p.bars_held >= policy.tp_decay_bars && p.tp > 0.0 {
        if let Some(a) = p.atr.filter(|a| a.is_finite() && *a > 0.0) {
            let target = round(price + dir * (policy.tp_decay_atr * a).max(min_dist), p.digits);
            let closer = (p.tp - target) * dir >= step;
            // A target below the costs would turn a "win" into a net loss: never set it there.
            let covers_costs = (target - p.entry) * dir > p.cost_cover_price;
            if closer && covers_costs {
                return PositionAction {
                    kind: PositionActionKind::AdjustTp,
                    sl: p.sl,
                    tp: target,
                    reason: format!("{} bougies sans atteindre la cible: TP rapproché à {target:.*}", p.bars_held, p.digits as usize),
                    volume: None,
                };
            }
        }
    }
    hold(format!("position suivie: {in_r:+.2} R, net réalisable {:+.2}", p.net_realizable))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BUY at 1.10000, initial SL 1.09900 (R = 100 points), TP 1.10200, ATR 30 points.
    fn buy(bid: f64) -> ManagedPosition {
        ManagedPosition {
            ticket: 1,
            side: Side::Buy,
            entry: 1.10000,
            sl: 1.09900,
            tp: 1.10200,
            initial_sl: 1.09900,
            bars_held: 3,
            bid,
            ask: bid + 0.00010,
            atr: Some(0.00030),
            point: 0.00001,
            digits: 5,
            stops_level_points: 10.0,
            freeze_level_points: 0.0,
            cost_cover_price: 0.00009,
            net_realizable: (bid - 1.10000) * 10_000.0,
            volume: 0.10,
            volume_min: 0.01,
            volume_step: 0.01,
            partial_done: false,
        }
    }

    fn sell_mirror(p: &ManagedPosition) -> ManagedPosition {
        // Reflect every price around the entry.
        let m = |x: f64| 2.0 * p.entry - x;
        ManagedPosition { side: Side::Sell, sl: m(p.sl), tp: m(p.tp), initial_sl: m(p.initial_sl), bid: m(p.ask), ask: m(p.bid), ..p.clone() }
    }

    #[test]
    fn hold_until_the_move_is_worth_securing() {
        let a = manage(&buy(1.10040), &PositionPolicy::default()); // 0.4 R
        assert_eq!(a.kind, PositionActionKind::Hold);
        assert_eq!((a.sl, a.tp), (1.09900, 1.10200));
    }

    #[test]
    fn secure_moves_the_stop_to_break_even_plus_costs() {
        let a = manage(&buy(1.10070), &PositionPolicy::default()); // 0.7 R
        assert_eq!(a.kind, PositionActionKind::Secure, "{a:?}");
        // entry + 9 points of costs + 1 point of buffer
        assert!((a.sl - 1.10010).abs() < 1e-9, "{a:?}");
        assert_eq!(a.tp, 1.10200);
        let s = manage(&sell_mirror(&buy(1.10070)), &PositionPolicy::default());
        assert_eq!(s.kind, PositionActionKind::Secure);
        assert!((s.sl - 1.09990).abs() < 1e-9, "{s:?}");
    }

    #[test]
    fn trailing_follows_but_never_widens() {
        let mut p = buy(1.10150); // 1.5 R
        p.sl = 1.10010; // already secured
        let a = manage(&p, &PositionPolicy::default());
        assert_eq!(a.kind, PositionActionKind::AdjustSl);
        assert!((a.sl - 1.10120).abs() < 1e-9, "1 ATR behind the bid: {a:?}");
        // Price falls back: the trailing level would be lower than the current stop -> hold.
        p.sl = 1.10120;
        p.bid = 1.10130;
        p.ask = 1.10140;
        let a = manage(&p, &PositionPolicy::default());
        assert_eq!(a.kind, PositionActionKind::Hold, "{a:?}");
        assert_eq!(a.sl, 1.10120, "the stop is never moved back");
    }

    #[test]
    fn broker_distances_are_respected() {
        let mut p = buy(1.10150);
        p.sl = 1.10010;
        p.stops_level_points = 50.0; // the stop may not be closer than 50 points to the bid
        let a = manage(&p, &PositionPolicy::default());
        assert!((a.sl - 1.10100).abs() < 1e-9, "{a:?}");
        assert!(a.reason.contains("distance minimale"));
        p.freeze_level_points = 30.0;
        p.tp = 1.10170; // within 20 points of the bid: frozen
        assert_eq!(manage(&p, &PositionPolicy::default()).kind, PositionActionKind::Hold);
    }

    #[test]
    fn stalled_targets_come_closer_but_stay_beyond_costs() {
        let policy = PositionPolicy { tp_decay_bars: 10, tp_decay_atr: 0.3, secure_at_r: 5.0, trail_from_r: 0.0, ..Default::default() };
        let mut p = buy(1.10030);
        p.bars_held = 12;
        let a = manage(&p, &policy);
        assert_eq!(a.kind, PositionActionKind::AdjustTp, "{a:?}");
        // bid + max(0.3 ATR = 9 points, stops level 10 points)
        assert!((a.tp - 1.10040).abs() < 1e-9, "{a:?}");
        assert_eq!(a.sl, p.sl);
        // At a loss, a target that would not even pay the costs is not set.
        let mut p = buy(1.09960);
        p.bars_held = 12;
        assert_eq!(manage(&p, &policy).kind, PositionActionKind::Hold);
    }

    #[test]
    fn time_stop_and_net_exit() {
        let policy = PositionPolicy { max_bars_held: 5, ..Default::default() };
        let mut p = buy(1.10010);
        p.bars_held = 5;
        assert_eq!(manage(&p, &policy).kind, PositionActionKind::Close);
        let policy = PositionPolicy { close_at_net: 0.20, ..Default::default() };
        let mut p = buy(1.10010);
        p.net_realizable = 0.19;
        assert_eq!(manage(&p, &policy).kind, PositionActionKind::Hold);
        p.net_realizable = 0.21;
        let a = manage(&p, &policy);
        assert_eq!(a.kind, PositionActionKind::Close);
        assert!(a.reason.contains("net réalisable"));
    }

    #[test]
    fn unknown_risk_or_bad_prices_never_trigger_an_action() {
        let mut p = buy(1.10150);
        p.initial_sl = 0.0;
        assert_eq!(manage(&p, &PositionPolicy::default()).kind, PositionActionKind::Hold);
        let mut p = buy(1.10150);
        p.bid = f64::NAN;
        assert_eq!(manage(&p, &PositionPolicy::default()).kind, PositionActionKind::Hold);
        assert_eq!(manage(&buy(1.10150), &PositionPolicy { enabled: false, ..Default::default() }).kind, PositionActionKind::Hold);
        assert!(PositionPolicy { trail_atr: 0.0, ..Default::default() }.validate().is_err());
        PositionPolicy::default().validate().unwrap();
    }

    #[test]
    fn partial_close_respects_the_broker_volume_rules_and_happens_once() {
        assert_eq!(partial_volume(0.10, 0.5, 0.01, 0.01), Some(0.05));
        assert_eq!(partial_volume(0.03, 0.5, 0.01, 0.01), Some(0.01), "rounded down to the step");
        assert_eq!(partial_volume(0.01, 0.5, 0.01, 0.01), None, "the minimum volume cannot be split");
        assert_eq!(partial_volume(0.15, 0.5, 0.1, 0.1), None, "0.1 closed would leave 0.05 < minimum");
        assert_eq!(partial_volume(0.30, 0.5, 0.1, 0.1), Some(0.1), "0.15 rounded down to 0.1, 0.2 left");
        assert_eq!(partial_volume(0.25, 0.5, 0.1, 0.1), None, "a remainder off the volume step is refused");
    }

    #[test]
    fn close_partial_once_at_the_configured_r() {
        let policy = PositionPolicy { partial_close_at_r: 1.0, secure_at_r: 5.0, trail_from_r: 0.0, ..Default::default() };
        let a = manage(&buy(1.10110), &policy);
        assert_eq!(a.kind, PositionActionKind::ClosePartial, "{}", a.reason);
        assert_eq!(a.volume, Some(0.05));
        let done = ManagedPosition { partial_done: true, ..buy(1.10110) };
        assert_ne!(manage(&done, &policy).kind, PositionActionKind::ClosePartial);
        // Below the trigger: nothing.
        assert_eq!(manage(&buy(1.10050), &policy).kind, PositionActionKind::Hold);
    }
}
