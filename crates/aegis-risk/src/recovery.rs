//! Recovery & Hedge rules: when may AEGIS take a SECOND position after a failure?
//!
//! Only when a new, independent opportunity exists (re-entry) or a real need to cover an
//! exposure exists (hedge, on a hedging account with the policy enabled). Every second
//! position is a new risk, evaluated by the Risk Engine like any other. ABSOLUTELY FORBIDDEN,
//! and checked here explicitly:
//! - martingale: a re-entry never risks more money, nor more volume, than the losing trade;
//! - averaging down / grid: no new position on the side of an AEGIS position that is losing;
//! - repeated entries without a new signal: a re-entry needs a bar formed entirely after the
//!   loss and (same side) a structure that changed since;
//! - multiplying hedges: at most `max_hedges_per_position`, each at most `max_hedge_ratio` of
//!   the covered volume.
//! A previous loss is NEVER a reason to enter: these rules can only refuse.

use crate::engine::Check;
use aegis_types::{Position, Side, TimestampMs};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RecoveryPolicy {
    /// A trade is a re-entry when a losing trade on the same instrument closed this recently.
    pub reentry_window_minutes: u32,
    /// Same-side re-entry needs a swing confirmed after the loss (the structure changed).
    pub reentry_requires_new_structure: bool,
    pub max_reentries_per_symbol_per_day: u32,
    pub max_hedges_per_position: u32,
    /// Hedge volume / covered volume.
    pub max_hedge_ratio: f64,
}

impl Default for RecoveryPolicy {
    fn default() -> Self {
        RecoveryPolicy { reentry_window_minutes: 60, reentry_requires_new_structure: true, max_reentries_per_symbol_per_day: 2, max_hedges_per_position: 1, max_hedge_ratio: 1.0 }
    }
}

impl RecoveryPolicy {
    pub fn validate(&self) -> Result<(), crate::ConfigError> {
        if self.reentry_window_minutes > 10_080 || self.max_reentries_per_symbol_per_day > 50 || self.max_hedges_per_position > 3 || !(0.1..=1.0).contains(&self.max_hedge_ratio) {
            return Err(crate::ConfigError { field: "recovery", problem: "window within 0..10080 min, re-entries within 0..50, hedges per position within 0..3, hedge ratio within 0.1..1" });
        }
        Ok(())
    }
}

/// A closed AEGIS trade, as the engine knows it from the journal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClosedTrade {
    pub ticket: i64,
    pub symbol: String,
    pub side: Side,
    pub strategy_id: String,
    pub closed_ms: TimestampMs,
    pub net: f64,
    pub volume: f64,
    /// Money it risked at its initial stop.
    pub risk_amount: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntryKind {
    Normal,
    Reentry { after: i64 },
    Hedge { covers: i64 },
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EntryKind::Normal => "normal",
            EntryKind::Reentry { .. } => "reentry",
            EntryKind::Hedge { .. } => "hedge",
        }
    }
    pub fn related(self) -> Option<i64> {
        match self {
            EntryKind::Normal => None,
            EntryKind::Reentry { after } => Some(after),
            EntryKind::Hedge { covers } => Some(covers),
        }
    }
}

pub struct RecoveryContext<'a> {
    pub now_ms: TimestampMs,
    pub symbol: &'a str,
    pub side: Side,
    /// Open time of the closed bar that produced the signal.
    pub signal_bar_ts: TimestampMs,
    pub signal_bar_ms: i64,
    pub proposed_volume: f64,
    pub proposed_risk: f64,
    /// AEGIS positions only.
    pub open_positions: &'a [Position],
    /// AEGIS trades closed recently, any order.
    pub recent_closed: &'a [ClosedTrade],
    pub reentries_today: u32,
    /// Covered ticket -> hedges already opened for it.
    pub hedges_by_ticket: &'a HashMap<i64, u32>,
    /// Time of the most recent confirmed swing on the signal timeframe.
    pub last_swing_ts: Option<TimestampMs>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoveryAssessment {
    pub kind: EntryKind,
    pub checks: Vec<Check>,
}

fn check(name: &str, passed: bool, detail: impl Into<String>) -> Check {
    Check { name: name.into(), passed, detail: detail.into() }
}

pub fn assess(ctx: &RecoveryContext<'_>, p: &RecoveryPolicy) -> RecoveryAssessment {
    let mut checks = Vec::new();
    let on_symbol: Vec<&Position> = ctx.open_positions.iter().filter(|x| x.symbol == ctx.symbol).collect();

    // ---- never add to a losing position (averaging down, grid)
    let losing_same: Vec<&&Position> = on_symbol.iter().filter(|x| x.side == ctx.side && x.floating() < 0.0).collect();
    checks.push(check(
        "anti_moyenne",
        losing_same.is_empty(),
        if losing_same.is_empty() { "aucune position perdante du même sens sur l'instrument".to_string() } else { format!("position {} du même sens en perte: ajouter serait une moyenne à la baisse (interdit)", losing_same[0].ticket) },
    ));

    // ---- hedge: an open AEGIS position on the other side
    if let Some(covered) = on_symbol.iter().find(|x| x.side != ctx.side) {
        let n = ctx.hedges_by_ticket.get(&covered.ticket).copied().unwrap_or(0);
        checks.push(check("couverture_unique", n < p.max_hedges_per_position, format!("{n} couverture(s) déjà ouverte(s) pour la position {} (max {})", covered.ticket, p.max_hedges_per_position)));
        let max_vol = covered.volume * p.max_hedge_ratio;
        checks.push(check("couverture_volume", ctx.proposed_volume <= max_vol + 1e-9, format!("couverture {} lot(s) pour {} lot(s) couverts (max {:.2})", ctx.proposed_volume, covered.volume, max_vol)));
        return RecoveryAssessment { kind: EntryKind::Hedge { covers: covered.ticket }, checks };
    }

    // ---- re-entry: a recent loss on the same instrument
    let window = p.reentry_window_minutes as i64 * 60_000;
    let loss = ctx
        .recent_closed
        .iter()
        .filter(|t| t.symbol == ctx.symbol && t.net < 0.0 && ctx.now_ms - t.closed_ms <= window)
        .max_by_key(|t| t.closed_ms);
    let Some(loss) = loss else {
        return RecoveryAssessment { kind: EntryKind::Normal, checks };
    };
    let fresh = ctx.signal_bar_ts >= loss.closed_ms;
    checks.push(check("reentry_nouveau_signal", fresh, if fresh {
        format!("signal d'une bougie formée après la perte de la position {}", loss.ticket)
    } else {
        format!("la bougie du signal a commencé avant la clôture en perte de {}: pas un nouveau signal", loss.ticket)
    }));
    if loss.side == ctx.side && p.reentry_requires_new_structure {
        let changed = ctx.last_swing_ts.is_some_and(|t| t + ctx.signal_bar_ms > loss.closed_ms);
        checks.push(check("reentry_structure", changed, if changed {
            "un swing s'est confirmé depuis la perte: la structure a évolué".to_string()
        } else {
            "même sens, structure inchangée depuis la perte: le signal initial n'est pas renouvelé".to_string()
        }));
    }
    checks.push(check(
        "reentry_max",
        ctx.reentries_today < p.max_reentries_per_symbol_per_day,
        format!("{} re-entrée(s) aujourd'hui sur {} (max {})", ctx.reentries_today, ctx.symbol, p.max_reentries_per_symbol_per_day),
    ));
    let risk_ok = ctx.proposed_risk <= loss.risk_amount * (1.0 + 1e-6) + 1e-9;
    let vol_ok = ctx.proposed_volume <= loss.volume + 1e-9;
    checks.push(check("anti_martingale", risk_ok && vol_ok, format!(
        "re-entrée: risque {:.2} et {} lot(s) pour {:.2} et {} lot(s) sur la perte — jamais plus après une perte",
        ctx.proposed_risk, ctx.proposed_volume, loss.risk_amount, loss.volume
    )));
    RecoveryAssessment { kind: EntryKind::Reentry { after: loss.ticket }, checks }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    fn pos(ticket: i64, side: Side, profit: f64, volume: f64) -> Position {
        Position { ticket, symbol: "EURUSD".into(), side, volume, price_open: 1.1, sl: 1.09, tp: 1.12, profit, swap: 0.0, commission: 0.0, magic: 1, comment: String::new(), open_ts: NOW - 60_000 }
    }
    fn loss(ticket: i64, side: Side, closed_ms: i64) -> ClosedTrade {
        ClosedTrade { ticket, symbol: "EURUSD".into(), side, strategy_id: "s".into(), closed_ms, net: -5.0, volume: 0.10, risk_amount: 20.0 }
    }

    fn ctx<'a>(open: &'a [Position], closed: &'a [ClosedTrade], hedges: &'a HashMap<i64, u32>) -> RecoveryContext<'a> {
        RecoveryContext {
            now_ms: NOW,
            symbol: "EURUSD",
            side: Side::Buy,
            signal_bar_ts: NOW - 300_000,
            signal_bar_ms: 300_000,
            proposed_volume: 0.10,
            proposed_risk: 20.0,
            open_positions: open,
            recent_closed: closed,
            reentries_today: 0,
            hedges_by_ticket: hedges,
            last_swing_ts: Some(NOW - 300_000),
        }
    }

    fn failed(a: &RecoveryAssessment) -> Vec<&str> {
        a.checks.iter().filter(|c| !c.passed).map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn a_plain_entry_is_normal_and_averaging_down_is_refused() {
        let h = HashMap::new();
        let a = assess(&ctx(&[], &[], &h), &RecoveryPolicy::default());
        assert_eq!(a.kind, EntryKind::Normal);
        assert!(failed(&a).is_empty());
        let losing = [pos(1, Side::Buy, -3.0, 0.1)];
        assert_eq!(failed(&assess(&ctx(&losing, &[], &h), &RecoveryPolicy::default())), vec!["anti_moyenne"]);
        let winning = [pos(1, Side::Buy, 3.0, 0.1)];
        assert!(failed(&assess(&ctx(&winning, &[], &h), &RecoveryPolicy::default())).is_empty());
    }

    #[test]
    fn reentry_needs_a_new_bar_a_new_structure_and_never_more_risk() {
        let h = HashMap::new();
        let closed = [loss(7, Side::Buy, NOW - 400_000)];
        let a = assess(&ctx(&[], &closed, &h), &RecoveryPolicy::default());
        assert_eq!(a.kind, EntryKind::Reentry { after: 7 });
        assert!(failed(&a).is_empty(), "{:?}", a.checks);
        // Martingale: doubling the lot (or the risk) after the loss is refused.
        let mut c = ctx(&[], &closed, &h);
        c.proposed_volume = 0.20;
        assert!(failed(&assess(&c, &RecoveryPolicy::default())).contains(&"anti_martingale"));
        let mut c = ctx(&[], &closed, &h);
        c.proposed_risk = 21.0;
        assert!(failed(&assess(&c, &RecoveryPolicy::default())).contains(&"anti_martingale"));
        // The signal bar started before the loss closed: not a new signal.
        let mut c = ctx(&[], &closed, &h);
        c.signal_bar_ts = NOW - 500_000;
        assert!(failed(&assess(&c, &RecoveryPolicy::default())).contains(&"reentry_nouveau_signal"));
        // Same side, no swing since the loss: structure unchanged.
        let mut c = ctx(&[], &closed, &h);
        c.last_swing_ts = Some(NOW - 3_600_000);
        assert!(failed(&assess(&c, &RecoveryPolicy::default())).contains(&"reentry_structure"));
        // ...but a reversal (other side) does not need it.
        let closed_sell = [loss(7, Side::Sell, NOW - 400_000)];
        let mut c = ctx(&[], &closed_sell, &h);
        c.last_swing_ts = None;
        assert!(failed(&assess(&c, &RecoveryPolicy::default())).is_empty());
        // Daily cap.
        let mut c = ctx(&[], &closed, &h);
        c.reentries_today = 2;
        assert!(failed(&assess(&c, &RecoveryPolicy::default())).contains(&"reentry_max"));
        // An old loss is outside the window: a normal entry.
        let old = [loss(7, Side::Buy, NOW - 2 * 3_600_000)];
        assert_eq!(assess(&ctx(&[], &old, &h), &RecoveryPolicy::default()).kind, EntryKind::Normal);
    }

    #[test]
    fn hedges_are_single_and_never_bigger_than_the_covered_position() {
        let open = [pos(3, Side::Sell, -4.0, 0.10)];
        let h = HashMap::new();
        let a = assess(&ctx(&open, &[], &h), &RecoveryPolicy::default());
        assert_eq!(a.kind, EntryKind::Hedge { covers: 3 });
        assert!(failed(&a).is_empty());
        let mut c = ctx(&open, &[], &h);
        c.proposed_volume = 0.15;
        assert!(failed(&assess(&c, &RecoveryPolicy::default())).contains(&"couverture_volume"));
        let one = HashMap::from([(3, 1)]);
        assert!(failed(&assess(&ctx(&open, &[], &one), &RecoveryPolicy::default())).contains(&"couverture_unique"), "no grid of hedges");
        assert!(RecoveryPolicy { max_hedge_ratio: 2.0, ..Default::default() }.validate().is_err(), "an over-hedge is not configurable");
    }
}
