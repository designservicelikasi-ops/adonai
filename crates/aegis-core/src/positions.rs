//! Position Intelligence inside the engine: on every closed bar of a signal
//! timeframe, each AEGIS position on that instrument is reviewed and the
//! decision (HOLD / SECURE / ADJUST SL / ADJUST TP / CLOSE) is recorded and,
//! in an execution mode, carried out. Positions opened outside AEGIS are never
//! touched.

use crate::engine::Engine;
use crate::view::PositionDecisionView;
use aegis_market::indicators::atr;
use aegis_risk::{manage_position, ManagedPosition, PositionActionKind};
use aegis_types::*;
use serde_json::json;

/// A modification still waiting for the broker's answer blocks the next one for this long.
const MODIFY_PENDING_MS: i64 = 10_000;

impl Engine {
    /// Remember the opening stop of a position: it defines the initial risk R.
    pub(crate) async fn note_initial_stop(&mut self, p: &Position) {
        if p.magic != self.cfg.execution.magic || p.sl <= 0.0 || self.initial_sl.contains_key(&p.ticket) {
            return;
        }
        self.initial_sl.insert(p.ticket, p.sl);
        if let Err(e) = self.store.kv_set("position_initial_sl", &self.initial_sl).await {
            self.db_failed(&e.to_string());
        }
    }

    pub(crate) async fn forget_closed_positions(&mut self) {
        let before = self.initial_sl.len();
        let open: Vec<i64> = self.positions.iter().map(|p| p.ticket).collect();
        self.initial_sl.retain(|t, _| open.contains(t));
        let planned = self.planned_sl.len();
        self.planned_sl.retain(|t, _| open.contains(t));
        if self.planned_sl.len() != planned {
            let _ = self.store.kv_set("position_planned_sl", &self.planned_sl).await;
        }
        self.modifying.retain(|t, _| open.contains(t));
        self.partial_done.retain(|t| open.contains(t));
        if self.initial_sl.len() != before {
            let _ = self.store.kv_set("position_initial_sl", &self.initial_sl).await;
        }
    }

    pub(crate) async fn manage_positions(&mut self, symbol: &str, tf: Timeframe) {
        let magic = self.cfg.execution.magic;
        let mine: Vec<Position> = self.positions.iter().filter(|p| p.symbol == symbol && p.magic == magic).cloned().collect();
        if mine.is_empty() {
            return;
        }
        let (Some(spec), Some((tick, _))) = (self.specs.get(symbol).cloned(), self.market.last_tick(symbol)) else { return };
        let bars = self.market.candles(symbol, tf);
        let a = atr(&bars, 14);
        let policy = self.risk.config().position.clone();
        let rc = self.risk.config().clone();
        let now = now_ms();
        for p in mine {
            let Some(initial_sl) = self.initial_sl.get(&p.ticket).copied() else { continue };
            let cost_money = rc.commission_per_lot_round_turn * p.volume + spec.price_distance_value(rc.expected_slippage_points * spec.point, p.volume);
            let per_price = spec.price_distance_value(1.0, p.volume);
            // Commission already booked on the position is part of `p.commission`;
            // what is left to pay is estimated from the round-turn figure.
            let remaining_commission = (rc.commission_per_lot_round_turn * p.volume - p.commission.abs()).max(0.0);
            let net_realizable = p.profit + p.swap + p.commission - remaining_commission - spec.price_distance_value(rc.expected_slippage_points / 2.0 * spec.point, p.volume);
            let mp = ManagedPosition {
                ticket: p.ticket,
                side: p.side,
                entry: p.price_open,
                sl: p.sl,
                tp: p.tp,
                initial_sl,
                bars_held: ((now - p.open_ts).max(0) / tf.millis()) as usize,
                bid: tick.bid,
                ask: tick.ask,
                atr: a,
                point: spec.point,
                digits: spec.digits,
                stops_level_points: spec.stops_level_points,
                freeze_level_points: spec.freeze_level_points,
                cost_cover_price: if per_price > 0.0 { cost_money / per_price } else { f64::INFINITY },
                net_realizable,
                volume: p.volume,
                volume_min: spec.volume_min,
                volume_step: spec.volume_step,
                partial_done: self.partial_done.contains(&p.ticket),
            };
            let action = manage_position(&mp, &policy);
            let mut executed = false;
            let mut note = String::new();
            if action.kind != PositionActionKind::Hold {
                let cap = match action.kind {
                    PositionActionKind::Close | PositionActionKind::ClosePartial => aegis_autonomy::Capability::ClosePositions,
                    _ => aegis_autonomy::Capability::ModifyProtections,
                };
                let permission = self.permit(cap, aegis_autonomy::Requester::PositionIntelligence);
                if !self.mode.can_execute() {
                    note = "MONITOR: non exécuté".into();
                } else if !permission.allowed {
                    note = format!("permission refusée: {}", permission.reason);
                    self.permission_event("denied", "commander", Some(&permission), format!("{} position {}", action.kind.label(), p.ticket)).await;
                } else if self.modifying.get(&p.ticket).is_some_and(|t| now - *t < MODIFY_PENDING_MS) {
                    note = "modification précédente en attente de réponse".into();
                } else {
                    let r = if action.kind == PositionActionKind::Close {
                        self.close_position(&p, None, &format!("{}: {}", action.kind.label(), action.reason)).await
                    } else if action.kind == PositionActionKind::ClosePartial {
                        let r = self.close_position(&p, action.volume, &format!("{}: {}", action.kind.label(), action.reason)).await;
                        if r.is_ok() {
                            self.partial_done.insert(p.ticket);
                            let _ = self.store.kv_set("position_partial_done", &self.partial_done).await;
                        }
                        r
                    } else {
                        match self.executor.submit_modify(&p, action.sl, action.tp, &action.reason, now).await {
                            Ok(body) => self.dispatch_command(body, &format!("{} position {}", action.kind.label(), p.ticket), &action.reason).await,
                            Err(e) => Err(e.to_string()),
                        }
                    };
                    match r {
                        Ok(_) => {
                            executed = true;
                            self.modifying.insert(p.ticket, now);
                            self.permission_event("action", "engine", Some(&permission), format!("{} position {}: {}", action.kind.label(), p.ticket, action.reason)).await;
                        }
                        Err(e) => note = format!("refusé: {e}"),
                    }
                }
                self.audit("position", "position_action", json!({ "ticket": p.ticket, "action": action.kind, "sl": action.sl, "tp": action.tp, "reason": action.reason, "executed": executed, "note": note })).await;
                self.log("info", "position", format!("{} {} {}: {}{}", action.kind.label(), p.symbol, p.ticket, action.reason, if note.is_empty() { String::new() } else { format!(" ({note})") }));
            }
            self.position_decisions.retain(|d| d.ticket != p.ticket);
            self.position_decisions.push(PositionDecisionView { ts_ms: now, ticket: p.ticket, symbol: p.symbol.clone(), action, executed, note, net_realizable, initial_sl });
        }
        self.cache.dirty = true;
    }
}
