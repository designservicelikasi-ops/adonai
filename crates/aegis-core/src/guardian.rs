//! Risk Guardian 2.0 inside the engine: protection of AEGIS positions at the broker
//! (missing stop, unverified modification) and the guardian's published view.

use crate::engine::Engine;
use crate::view::GuardianView;
use aegis_risk::ProtectionAction;
use aegis_types::*;
use serde_json::json;
use std::collections::HashMap;

impl Engine {
    /// Run after every broker snapshot.
    pub(crate) async fn guard_positions(&mut self, now: TimestampMs) {
        let magic = self.cfg.execution.magic;
        let mine: Vec<Position> = self.positions.iter().filter(|p| p.magic == magic).cloned().collect();
        let prices: HashMap<String, (f64, f64)> = mine
            .iter()
            .filter_map(|p| self.market.last_tick(&p.symbol).map(|(t, _)| (p.symbol.clone(), (t.bid, t.ask))))
            .collect();
        let rc = self.risk.config().clone();
        let can_send = self.mode.can_execute() && self.conn.is_some();
        let actions = self.protection.review(&mine, &self.planned_sl, &self.specs, &prices, rc.missing_stop_policy, rc.missing_stop_grace_ms, can_send, now);
        for action in actions {
            // Protective actions are never blocked by a permission, but they are recorded.
            let cap = if matches!(action, ProtectionAction::Close { .. }) { aegis_autonomy::Capability::ClosePositions } else { aegis_autonomy::Capability::ModifyProtections };
            let d = self.permit(cap, aegis_autonomy::Requester::RiskGuardian);
            self.permission_event("action", "risk_guardian", Some(&d), format!("{action:?}")).await;
            match action {
                ProtectionAction::Alert { ticket, message } => {
                    self.audit("guardian", "protection_alert", json!({ "ticket": ticket, "message": message })).await;
                    self.alert("critical", "risk", message);
                }
                ProtectionAction::Restore { ticket, sl, tp, reason } => {
                    let Some(p) = mine.iter().find(|p| p.ticket == ticket).cloned() else { continue };
                    let r = match self.executor.submit_modify(&p, sl, tp, &reason, now).await {
                        Ok(body) => {
                            let cid = match &body {
                                aegis_protocol::Body::Order { cid, .. } => cid.clone(),
                                _ => String::new(),
                            };
                            let r = self.dispatch_command(body, &format!("réinstallation du stop de la position {ticket}"), &reason).await;
                            if r.is_ok() {
                                // Marked as a guard restoration: a broker refusal counts as an attempt.
                                if let Some(entry) = self.modify_requests.get_mut(&cid) {
                                    entry.3 = true;
                                }
                            }
                            r
                        }
                        Err(e) => Err(e.to_string()),
                    };
                    if let Err(e) = &r {
                        self.protection.on_restore_failed(ticket);
                        self.log("warn", "risk", format!("réinstallation du stop {ticket} impossible: {e}"));
                    }
                    self.audit("guardian", "stop_restore", json!({ "ticket": ticket, "sl": sl, "sent": r.is_ok(), "reason": reason })).await;
                    self.alert("critical", "risk", format!("position {ticket} sans stop-loss chez le broker: {reason}"));
                }
                ProtectionAction::Close { ticket, reason } => {
                    let Some(p) = mine.iter().find(|p| p.ticket == ticket).cloned() else { continue };
                    let r = self.close_position(&p, None, &format!("protection: {reason}")).await;
                    self.audit("guardian", "protective_close", json!({ "ticket": ticket, "sent": r.is_ok(), "reason": reason, "error": r.as_ref().err() })).await;
                    self.alert("critical", "risk", format!("position {ticket}: {reason}{}", r.err().map(|e| format!(" — ÉCHEC: {e}")).unwrap_or_default()));
                }
            }
        }
    }

    pub(crate) fn guardian_view(&self) -> GuardianView {
        let rc = self.risk.config();
        let equity = self.account.as_ref().map(|a| a.equity);
        // Same measure as the risk engine: loss at the CURRENT stops, unbounded without a stop.
        let mut risk = Some(0.0);
        for p in &self.positions {
            match (p.sl > 0.0, self.specs.get(&p.symbol)) {
                (true, Some(s)) => {
                    if let Some(r) = risk.as_mut() {
                        *r += s.price_distance_value(((p.price_open - p.sl) * p.side.sign()).max(0.0), p.volume) + rc.commission_per_lot_round_turn * p.volume;
                    }
                }
                _ => risk = None,
            }
        }
        let day = self.accounting.as_ref().map(|a| &a.day);
        GuardianView {
            account_margin_mode: self.account.as_ref().map(|a| a.margin_mode.as_str().to_string()).unwrap_or_default(),
            unprotected_tickets: self.protection.unprotected(),
            incidents: self.protection.incidents.iter().take(50).cloned().collect(),
            verified_modifications: self.protection.verified_modifications,
            unverified_modifications: self.protection.unverified_modifications,
            pending_verifications: self.protection.pending_verifications(),
            portfolio_risk: risk,
            portfolio_risk_pct: match (risk, equity) {
                (Some(r), Some(e)) if e > 0.0 => Some(r / e * 100.0),
                _ => None,
            },
            portfolio_risk_limit_pct: rc.effective_portfolio_risk_pct(),
            daily_risk_sent: day.map(|d| d.risk_sent),
            daily_risk_budget: day.filter(|_| rc.daily_risk_budget_pct > 0.0).map(|d| d.start_equity * rc.daily_risk_budget_pct / 100.0),
            loss_streak_scale: rc.loss_streak_scale(day.map(|d| d.consecutive_losses).unwrap_or(0)),
            execution: self.latency.report(),
        }
    }
}
