//! Protection guard: every AEGIS position must carry a stop-loss AT THE BROKER, and every
//! stop modification the broker accepted must be visible in its next snapshot.
//!
//! This module is pure (no I/O): the engine feeds it the broker's positions and applies the
//! actions it returns. It never widens a stop and never sends anything in MONITOR (the engine
//! enforces that by only alerting).

use crate::config::MissingStopPolicy;
use aegis_types::{Position, Side, SymbolSpec, TimestampMs};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Failed re-installations before the guard falls back to closing the position.
pub const MAX_RESTORE_ATTEMPTS: u32 = 2;
/// Time the broker has to show an accepted modification in its snapshot.
pub const MODIFY_VERIFY_MS: TimestampMs = 5_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProtectionAction {
    Alert { ticket: i64, message: String },
    /// Re-install `sl` (and keep `tp`).
    Restore { ticket: i64, sl: f64, tp: f64, reason: String },
    Close { ticket: i64, reason: String },
}

/// A modification accepted by the broker, waiting to be seen in a snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpectedLevels {
    pub ticket: i64,
    pub sl: f64,
    pub tp: f64,
    pub accepted_ms: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProtectionIncident {
    pub ts_ms: TimestampMs,
    pub ticket: i64,
    pub symbol: String,
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProtectionGuard {
    /// Ticket -> first time it was seen without a stop.
    unprotected_since: HashMap<i64, TimestampMs>,
    /// Ticket -> failed or pending re-installations.
    attempts: HashMap<i64, u32>,
    /// Ticket -> a close was already requested by the guard.
    closing: HashMap<i64, TimestampMs>,
    expected: HashMap<i64, ExpectedLevels>,
    /// Most recent first, bounded.
    pub incidents: Vec<ProtectionIncident>,
    pub verified_modifications: u64,
    pub unverified_modifications: u64,
}

impl ProtectionGuard {
    fn incident(&mut self, now: TimestampMs, p: &Position, kind: &str, detail: String) {
        self.incidents.insert(0, ProtectionIncident { ts_ms: now, ticket: p.ticket, symbol: p.symbol.clone(), kind: kind.into(), detail });
        self.incidents.truncate(200);
    }

    /// The broker accepted a modification of `ticket` to these levels.
    pub fn on_modification_accepted(&mut self, ticket: i64, sl: f64, tp: f64, now: TimestampMs) {
        self.expected.insert(ticket, ExpectedLevels { ticket, sl, tp, accepted_ms: now });
    }

    /// A re-installation the guard asked for was refused by the broker.
    pub fn on_restore_failed(&mut self, ticket: i64) {
        *self.attempts.entry(ticket).or_default() += 1;
    }

    pub fn attempts(&self, ticket: i64) -> u32 {
        self.attempts.get(&ticket).copied().unwrap_or(0)
    }

    pub fn pending_verifications(&self) -> usize {
        self.expected.len()
    }

    pub fn unprotected(&self) -> Vec<i64> {
        let mut v: Vec<i64> = self.unprotected_since.keys().copied().collect();
        v.sort();
        v
    }

    /// Review AEGIS positions (`mine`) against their planned stops.
    /// `prices`: symbol -> (bid, ask). `can_send`: false in MONITOR or without a terminal.
    #[allow(clippy::too_many_arguments)]
    pub fn review(
        &mut self,
        mine: &[Position],
        planned_sl: &HashMap<i64, f64>,
        specs: &HashMap<String, SymbolSpec>,
        prices: &HashMap<String, (f64, f64)>,
        policy: MissingStopPolicy,
        grace_ms: TimestampMs,
        can_send: bool,
        now: TimestampMs,
    ) -> Vec<ProtectionAction> {
        let mut out = Vec::new();
        let open: Vec<i64> = mine.iter().map(|p| p.ticket).collect();
        self.unprotected_since.retain(|t, _| open.contains(t));
        self.attempts.retain(|t, _| open.contains(t));
        self.closing.retain(|t, _| open.contains(t));
        self.expected.retain(|t, _| open.contains(t));

        for p in mine {
            // ---- accepted modifications must show in the snapshot
            if let Some(exp) = self.expected.get(&p.ticket).cloned() {
                let tol = specs.get(&p.symbol).map(|s| s.point * 0.5).unwrap_or(1e-9);
                if (p.sl - exp.sl).abs() <= tol && (exp.tp <= 0.0 || (p.tp - exp.tp).abs() <= tol) {
                    self.expected.remove(&p.ticket);
                    self.verified_modifications += 1;
                } else if now - exp.accepted_ms > MODIFY_VERIFY_MS {
                    self.expected.remove(&p.ticket);
                    self.unverified_modifications += 1;
                    let msg = format!("modification acceptée (SL {} / TP {}) mais le broker montre SL {} / TP {}", exp.sl, exp.tp, p.sl, p.tp);
                    self.incident(now, p, "modification_non_constatee", msg.clone());
                    out.push(ProtectionAction::Alert { ticket: p.ticket, message: msg });
                }
            }

            // ---- a stop must exist at the broker
            if p.sl > 0.0 {
                if self.unprotected_since.remove(&p.ticket).is_some() {
                    self.incident(now, p, "stop_retabli", format!("stop {} constaté chez le broker", p.sl));
                }
                self.attempts.remove(&p.ticket);
                continue;
            }
            let since = *self.unprotected_since.entry(p.ticket).or_insert(now);
            if now - since < grace_ms || self.closing.contains_key(&p.ticket) {
                continue;
            }
            let first_report = !self.incidents.iter().any(|i| i.ticket == p.ticket && i.kind == "stop_manquant");
            if first_report {
                self.incident(now, p, "stop_manquant", format!("position {} {} {} lot(s) SANS stop-loss chez le broker", p.symbol, p.side, p.volume));
            }
            if !can_send || policy == MissingStopPolicy::Alert {
                if first_report {
                    let why = if can_send { "politique « alerte »" } else { "aucun ordre ne peut partir (MONITOR ou terminal déconnecté)" };
                    out.push(ProtectionAction::Alert { ticket: p.ticket, message: format!("position {} sans stop-loss — {why}", p.ticket) });
                }
                continue;
            }
            let close = |reason: String| ProtectionAction::Close { ticket: p.ticket, reason };
            if policy == MissingStopPolicy::Close {
                self.closing.insert(p.ticket, now);
                out.push(close("position sans stop-loss: clôture (politique « clôturer »)".into()));
                continue;
            }
            // Restore: only a stop on the protective side of the current price, beyond the stops level.
            let restore = planned_sl.get(&p.ticket).copied().filter(|sl| *sl > 0.0).and_then(|sl| {
                let spec = specs.get(&p.symbol)?;
                let (bid, ask) = prices.get(&p.symbol).copied()?;
                let exit = if p.side == Side::Buy { bid } else { ask };
                let room = (exit - sl) * p.side.sign();
                (room >= spec.stops_level_points * spec.point && room > 0.0).then_some(sl)
            });
            match restore {
                Some(sl) if self.attempts(p.ticket) < MAX_RESTORE_ATTEMPTS => {
                    // Attempts are counted when the broker refuses (`on_restore_failed`).
                    out.push(ProtectionAction::Restore { ticket: p.ticket, sl, tp: p.tp, reason: format!("réinstallation du stop prévu {sl}") });
                }
                Some(_) => {
                    self.closing.insert(p.ticket, now);
                    out.push(close(format!("stop refusé {} fois: clôture de protection", self.attempts(p.ticket))));
                }
                None => {
                    self.closing.insert(p.ticket, now);
                    out.push(close("stop prévu inconnu ou déjà dépassé par le prix: clôture de protection".into()));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::eurusd;

    const NOW: i64 = 1_800_000_000_000;

    fn pos(ticket: i64, sl: f64) -> Position {
        Position {
            ticket, symbol: "EURUSD".into(), side: Side::Buy, volume: 0.1, price_open: 1.1, sl, tp: 1.102,
            profit: 0.0, swap: 0.0, commission: 0.0, magic: 1, comment: String::new(), open_ts: NOW,
        }
    }

    fn world(bid: f64) -> (HashMap<String, SymbolSpec>, HashMap<String, (f64, f64)>, HashMap<i64, f64>) {
        let specs = HashMap::from([("EURUSD".to_string(), eurusd())]);
        let prices = HashMap::from([("EURUSD".to_string(), (bid, bid + 0.0001))]);
        (specs, prices, HashMap::from([(1, 1.099)]))
    }

    #[test]
    fn a_missing_stop_is_restored_after_the_grace_period_then_closed_if_refused() {
        let (specs, prices, planned) = world(1.1);
        let mut g = ProtectionGuard::default();
        let p = [pos(1, 0.0)];
        let run = |g: &mut ProtectionGuard, t| g.review(&p, &planned, &specs, &prices, MissingStopPolicy::Restore, 5_000, true, t);
        assert!(run(&mut g, NOW).is_empty(), "grace period: the broker may not have shown the stop yet");
        assert!(matches!(run(&mut g, NOW + 5_000).as_slice(), [ProtectionAction::Restore { sl, .. }] if *sl == 1.099));
        g.on_restore_failed(1);
        g.on_restore_failed(1);
        assert!(matches!(run(&mut g, NOW + 6_000).as_slice(), [ProtectionAction::Close { .. }]));
        assert!(run(&mut g, NOW + 7_000).is_empty(), "the close is requested once");
        assert!(g.incidents.iter().any(|i| i.kind == "stop_manquant"));
    }

    #[test]
    fn price_beyond_the_planned_stop_closes_and_monitor_only_alerts() {
        let (specs, prices, planned) = world(1.0985); // already below the planned stop 1.099
        let p = [pos(1, 0.0)];
        let mut g = ProtectionGuard::default();
        assert!(g.review(&p, &planned, &specs, &prices, MissingStopPolicy::Restore, 1_000, true, NOW).is_empty());
        let a = g.review(&p, &planned, &specs, &prices, MissingStopPolicy::Restore, 1_000, true, NOW + 2_000);
        assert!(matches!(a.as_slice(), [ProtectionAction::Close { .. }]), "{a:?}");

        let mut g = ProtectionGuard::default();
        g.review(&p, &planned, &specs, &prices, MissingStopPolicy::Close, 1_000, false, NOW);
        let a = g.review(&p, &planned, &specs, &prices, MissingStopPolicy::Close, 1_000, false, NOW + 2_000);
        assert!(matches!(a.as_slice(), [ProtectionAction::Alert { .. }]), "MONITOR never sends: {a:?}");
        assert!(g.review(&p, &planned, &specs, &prices, MissingStopPolicy::Close, 1_000, false, NOW + 3_000).is_empty(), "alerted once");
    }

    #[test]
    fn accepted_modifications_are_verified_against_the_snapshot() {
        let (specs, prices, planned) = world(1.1);
        let mut g = ProtectionGuard::default();
        g.on_modification_accepted(1, 1.0995, 1.102, NOW);
        g.review(&[pos(1, 1.0995)], &planned, &specs, &prices, MissingStopPolicy::Restore, 5_000, true, NOW + 100);
        assert_eq!((g.verified_modifications, g.pending_verifications()), (1, 0));
        g.on_modification_accepted(1, 1.0997, 1.102, NOW);
        assert!(g.review(&[pos(1, 1.0995)], &planned, &specs, &prices, MissingStopPolicy::Restore, 5_000, true, NOW + 1_000).is_empty());
        let a = g.review(&[pos(1, 1.0995)], &planned, &specs, &prices, MissingStopPolicy::Restore, 5_000, true, NOW + 6_000);
        assert!(matches!(a.as_slice(), [ProtectionAction::Alert { .. }]));
        assert_eq!(g.unverified_modifications, 1);
    }

    #[test]
    fn protected_positions_produce_nothing_and_closed_tickets_are_forgotten() {
        let (specs, prices, planned) = world(1.1);
        let mut g = ProtectionGuard::default();
        assert!(g.review(&[pos(1, 1.099)], &planned, &specs, &prices, MissingStopPolicy::Restore, 1_000, true, NOW).is_empty());
        g.review(&[pos(2, 0.0)], &planned, &specs, &prices, MissingStopPolicy::Restore, 1_000, true, NOW);
        assert_eq!(g.unprotected(), vec![2]);
        g.review(&[], &planned, &specs, &prices, MissingStopPolicy::Restore, 1_000, true, NOW);
        assert!(g.unprotected().is_empty());
    }
}
