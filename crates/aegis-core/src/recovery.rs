//! Recovery & Hedge Intelligence and Micro-Profit 2.0 inside the engine.

use crate::engine::Engine;
use aegis_data::JournalEntry;
use aegis_risk::recovery::{assess, ClosedTrade, RecoveryAssessment, RecoveryContext};
use aegis_risk::TargetFeasibility;
use aegis_types::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MicroSection {
    /// Net targets studied for every decision (account currency).
    pub targets: Vec<f64>,
}

impl Default for MicroSection {
    fn default() -> Self {
        MicroSection { targets: vec![0.05, 0.08, 0.10, 0.15, 0.20, 0.30, 0.50] }
    }
}

impl MicroSection {
    pub fn validate(&self) -> Result<(), String> {
        if self.targets.is_empty() || self.targets.len() > 30 || self.targets.iter().any(|t| !(t.is_finite() && *t > 0.0 && *t <= 1e6)) {
            return Err("micro.targets: 1..30 positive amounts".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MicroTable {
    pub ts_ms: TimestampMs,
    pub symbol: String,
    pub timeframe: Timeframe,
    pub side: Side,
    pub volume: f64,
    pub entry: f64,
    pub sl: f64,
    pub rows: Vec<TargetFeasibility>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct KindStats {
    pub trades: usize,
    pub wins: usize,
    pub net: f64,
    /// Commission + swap + fees paid by these trades (negative = cost).
    pub costs: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HedgeOutcome {
    pub hedge_ticket: i64,
    pub covered_ticket: i64,
    pub symbol: String,
    pub hedge_net: f64,
    pub covered_net: Option<f64>,
    /// Covered + hedge.
    pub combined_net: Option<f64>,
    /// The hedge improved the final result (combined better than the covered position alone).
    pub effective: Option<bool>,
    pub hedge_costs: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReentryOutcome {
    pub ticket: i64,
    pub after_ticket: i64,
    pub symbol: String,
    pub loss_net: Option<f64>,
    pub reentry_net: Option<f64>,
    /// Loss + re-entry: what the sequence finally cost or earned.
    pub sequence_net: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RecoveryReport {
    pub normal: KindStats,
    pub reentries: KindStats,
    pub hedges: KindStats,
    pub hedge_outcomes: Vec<HedgeOutcome>,
    pub reentry_outcomes: Vec<ReentryOutcome>,
    /// Strategies with at least 3 losses in a row (most recent streak), with the streak.
    pub repeated_losses: Vec<(String, u32)>,
    pub note: String,
}

pub fn recovery_report(journal: &[JournalEntry]) -> RecoveryReport {
    let closed: Vec<&JournalEntry> = journal.iter().filter(|e| e.closed_ms.is_some()).collect();
    let by_ticket: HashMap<i64, &JournalEntry> = journal.iter().map(|e| (e.ticket, e)).collect();
    let stats = |kind: &str| {
        let v: Vec<&&JournalEntry> = closed.iter().filter(|e| (if e.entry_kind.is_empty() { "normal" } else { e.entry_kind.as_str() }) == kind).collect();
        KindStats { trades: v.len(), wins: v.iter().filter(|e| e.net > 0.0).count(), net: v.iter().map(|e| e.net).sum(), costs: v.iter().map(|e| e.commission + e.swap + e.fee).sum() }
    };
    let net_of = |t: i64| by_ticket.get(&t).filter(|e| e.closed_ms.is_some()).map(|e| e.net);
    let hedge_outcomes = journal
        .iter()
        .filter(|e| e.entry_kind == "hedge" && e.closed_ms.is_some())
        .filter_map(|e| {
            let covered = e.related_ticket?;
            let cn = net_of(covered);
            Some(HedgeOutcome {
                hedge_ticket: e.ticket,
                covered_ticket: covered,
                symbol: e.symbol.clone(),
                hedge_net: e.net,
                covered_net: cn,
                combined_net: cn.map(|c| c + e.net),
                effective: cn.map(|_| e.net > 0.0),
                hedge_costs: e.commission + e.swap + e.fee,
            })
        })
        .collect();
    let reentry_outcomes = journal
        .iter()
        .filter(|e| e.entry_kind == "reentry")
        .filter_map(|e| {
            let after = e.related_ticket?;
            let l = net_of(after);
            let r = e.closed_ms.map(|_| e.net);
            Some(ReentryOutcome { ticket: e.ticket, after_ticket: after, symbol: e.symbol.clone(), loss_net: l, reentry_net: r, sequence_net: l.zip(r).map(|(a, b)| a + b) })
        })
        .collect();
    let mut sorted = closed.clone();
    sorted.sort_by_key(|e| e.closed_ms);
    let mut streaks: HashMap<String, u32> = HashMap::new();
    for e in &sorted {
        let s = streaks.entry(e.strategy_id.clone()).or_default();
        *s = if e.net < 0.0 { *s + 1 } else { 0 };
    }
    let mut repeated: Vec<(String, u32)> = streaks.into_iter().filter(|(_, n)| *n >= 3).collect();
    repeated.sort_by(|a, b| b.1.cmp(&a.1));
    RecoveryReport {
        normal: stats("normal"),
        reentries: stats("reentry"),
        hedges: stats("hedge"),
        hedge_outcomes,
        reentry_outcomes,
        repeated_losses: repeated,
        note: if closed.len() < 30 { format!("{} transaction(s) clôturée(s): trop peu pour conclure sur l'utilité des secondes positions", closed.len()) } else { String::new() },
    }
}

impl Engine {
    pub(crate) async fn recovery_assessment(&mut self, symbol: &str, tf: Timeframe, side: Side, bar_ts: TimestampMs, volume: f64, risk: f64, now: TimestampMs) -> RecoveryAssessment {
        let magic = self.cfg.execution.magic;
        let open: Vec<Position> = self.positions.iter().filter(|p| p.magic == magic).cloned().collect();
        let journal = self.store.journal_recent(300).await.unwrap_or_default();
        let today = utc_day(now);
        let closed: Vec<ClosedTrade> = journal
            .iter()
            .filter(|e| e.closed_ms.is_some_and(|c| now - c <= 7 * 86_400_000))
            .filter_map(|e| {
                let spec = self.specs.get(&e.symbol)?;
                Some(ClosedTrade {
                    ticket: e.ticket,
                    symbol: e.symbol.clone(),
                    side: e.side?,
                    strategy_id: e.strategy_id.clone(),
                    closed_ms: e.closed_ms?,
                    net: e.net,
                    volume: e.volume,
                    risk_amount: spec.price_distance_value((e.entry_filled - e.sl_initial).abs(), e.volume),
                })
            })
            .collect();
        let reentries_today = journal.iter().filter(|e| e.symbol == symbol && e.entry_kind == "reentry" && utc_day(e.opened_ms) == today).count() as u32;
        let mut hedges: HashMap<i64, u32> = HashMap::new();
        for e in journal.iter().filter(|e| e.entry_kind == "hedge") {
            if let Some(t) = e.related_ticket {
                *hedges.entry(t).or_default() += 1;
            }
        }
        let bars = self.market.candles(symbol, tf);
        let last_swing_ts = aegis_market::structure::swings(aegis_strategy::window(&bars), 3).last().map(|s| s.ts_ms);
        let ctx = RecoveryContext {
            now_ms: now,
            symbol,
            side,
            signal_bar_ts: bar_ts,
            signal_bar_ms: tf.millis(),
            proposed_volume: volume,
            proposed_risk: risk,
            open_positions: &open,
            recent_closed: &closed,
            reentries_today,
            hedges_by_ticket: &hedges,
            last_swing_ts,
        };
        assess(&ctx, &self.risk.config().recovery)
    }

    /// Feasibility of the configured net targets for this decision (shown on the Micro-Profit page).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn note_micro_targets(&mut self, symbol: &str, tf: Timeframe, side: Side, volume: f64, entry: f64, sl: f64, now: TimestampMs) {
        let (Some(spec), Some((tick, _))) = (self.specs.get(symbol).cloned(), self.market.last_tick(symbol)) else { return };
        let mut vol = volume;
        if vol <= 0.0 {
            vol = spec.volume_min;
        }
        let rc = self.risk.config().clone();
        let sl_dist = (entry - sl).abs();
        let prob = |tp: f64| self.scenario_probability(symbol, tf, side, tp, sl_dist);
        let rows = aegis_risk::target_feasibility(&spec, &rc, side, vol, entry, sl, tick.spread(), &self.cfg.micro.targets, &prob);
        self.micro_tables.insert(symbol.to_string(), MicroTable { ts_ms: now, symbol: symbol.into(), timeframe: tf, side, volume: vol, entry, sl, rows });
    }
}
