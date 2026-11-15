//! AEGIS EXECUTION — order lifecycle, idempotency and reconciliation.
//!
//! Rules enforced here:
//! * An entry instruction can only be built from an `Authorization` that the
//!   risk engine accepts (signature, expiry, single use).
//! * "Sent" never means "executed": an order is `filled` only when the broker
//!   says so, or when reconciliation finds its position on the account.
//! * Every state change is persisted before the caller sends anything.

use aegis_data::{CommandRecord, DataError, OrderRecord, OrderUpdate, Store};
use aegis_protocol::{Body, OrderAction};
use aegis_risk::{AuthError, Authorization, RiskEngine};
use aegis_types::{OrderKind, PendingOrder, Position, Side, TimestampMs};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    /// Persisted, not yet handed to the connector.
    Authorized,
    /// Handed to the connector; outcome not known yet.
    Sent,
    /// Pending order accepted by the broker.
    Placed,
    Filled,
    PartiallyFilled,
    Rejected,
    /// No answer in time: the truth will come from reconciliation.
    Unknown,
    /// Reconciliation established that nothing was executed.
    NotExecuted,
}

impl OrderStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            OrderStatus::Authorized => "authorized",
            OrderStatus::Sent => "sent",
            OrderStatus::Placed => "placed",
            OrderStatus::Filled => "filled",
            OrderStatus::PartiallyFilled => "partially_filled",
            OrderStatus::Rejected => "rejected",
            OrderStatus::Unknown => "unknown",
            OrderStatus::NotExecuted => "not_executed",
        }
    }
    pub fn is_final(self) -> bool {
        !matches!(self, OrderStatus::Authorized | OrderStatus::Sent | OrderStatus::Unknown)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("authorization refused: {0}")]
    Auth(#[from] AuthError),
    #[error("storage error: {0}")]
    Data(#[from] DataError),
    #[error("refused: {0}")]
    Refused(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExecConfig {
    /// Magic number stamped on every AEGIS order.
    pub magic: i64,
    /// Time allowed for the connector to report an outcome.
    pub result_timeout_ms: i64,
    /// A snapshot must be this much newer than the send time to prove non-execution.
    pub reconcile_grace_ms: i64,
}

impl Default for ExecConfig {
    fn default() -> Self {
        ExecConfig { magic: 20_260_115, result_timeout_ms: 5_000, reconcile_grace_ms: 3_000 }
    }
}

/// What happened, for the audit trail and the user interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ExecEvent {
    Filled { cid: String, ticket: i64, price: f64, volume: f64, requested_volume: f64, slippage_points: Option<f64> },
    Placed { cid: String, ticket: i64 },
    Rejected { cid: String, retcode: i64, message: String },
    TimedOut { cid: String },
    ReconciledFilled { cid: String, ticket: i64 },
    ReconciledNotExecuted { cid: String },
    CommandDone { cid: String, action: String, ticket: i64 },
    CommandFailed { cid: String, action: String, ticket: i64, message: String },
    /// A result arrived for an instruction this engine never issued or already settled.
    UnexpectedResult { cid: String },
}

struct InFlight {
    sent_ms: TimestampMs,
    requested_volume: f64,
    price: f64,
    kind: OrderKind,
    side: Side,
    point: f64,
    timed_out: bool,
}

struct CommandInFlight {
    action: &'static str,
    ticket: i64,
}

pub struct Executor {
    store: Store,
    cfg: ExecConfig,
    orders: HashMap<String, InFlight>,
    commands: HashMap<String, CommandInFlight>,
}

/// Short unique client id; fits in an MT5 order comment (31 chars max).
pub fn new_cid() -> String {
    format!("A{}", hex::encode(rand::random::<[u8; 8]>()))
}

impl Executor {
    pub fn new(store: Store, cfg: ExecConfig) -> Self {
        Executor { store, cfg, orders: HashMap::new(), commands: HashMap::new() }
    }

    pub fn config(&self) -> &ExecConfig {
        &self.cfg
    }

    pub fn in_flight(&self) -> usize {
        self.orders.len()
    }

    /// After a restart: orders that were authorized or sent have an unknown fate.
    /// They are flagged so the next account snapshot settles them.
    pub async fn recover(&mut self, now_ms: TimestampMs) -> Result<usize, ExecError> {
        let stale = self.store.orders_with_status(&["authorized", "sent", "unknown"]).await?;
        for o in &stale {
            if o.status != "unknown" {
                self.store
                    .update_order(&OrderUpdate {
                        cid: o.cid.clone(),
                        status: OrderStatus::Unknown.as_str().into(),
                        message: Some("engine restarted before the outcome was known".into()),
                        ..Default::default()
                    })
                    .await?;
            }
            self.orders.insert(
                o.cid.clone(),
                InFlight { sent_ms: now_ms, requested_volume: o.volume, price: o.price, kind: o.kind, side: o.side, point: 0.0, timed_out: true },
            );
        }
        Ok(stale.len())
    }

    /// Turn a risk authorization into an order instruction.
    /// The authorization is consumed here, so it can never produce two orders.
    pub async fn submit_entry(
        &mut self,
        risk: &mut RiskEngine,
        auth: &Authorization,
        point: f64,
        now_ms: TimestampMs,
    ) -> Result<Body, ExecError> {
        risk.consume_authorization(auth, now_ms)?;
        let cid = new_cid();
        let record = OrderRecord {
            cid: cid.clone(),
            decision_id: auth.decision_id.clone(),
            auth_id: auth.auth_id.clone(),
            symbol: auth.symbol.clone(),
            side: auth.side,
            kind: auth.kind,
            volume: auth.volume,
            price: auth.price,
            sl: auth.sl,
            tp: auth.tp,
            status: OrderStatus::Authorized.as_str().into(),
            ticket: None,
            fill_price: None,
            fill_volume: None,
            retcode: None,
            message: None,
            created_ms: now_ms,
            updated_ms: now_ms,
        };
        // The unique constraints on cid and auth_id make this the idempotency barrier.
        self.store.insert_order(&record).await?;
        let body = Body::Order {
            cid: cid.clone(),
            auth_id: auth.auth_id.clone(),
            action: OrderAction::Open,
            symbol: auth.symbol.clone(),
            side: auth.side,
            kind: auth.kind,
            volume: auth.volume,
            price: auth.price,
            sl: auth.sl,
            tp: auth.tp,
            deviation: auth.max_slippage_points,
            ticket: 0,
            expires: auth.expires_ms,
            magic: self.cfg.magic,
        };
        body.validate().map_err(ExecError::Refused)?;
        self.orders.insert(
            cid,
            InFlight { sent_ms: now_ms, requested_volume: auth.volume, price: auth.price, kind: auth.kind, side: auth.side, point, timed_out: false },
        );
        Ok(body)
    }

    /// Call once the frame has actually been handed to the connector.
    pub async fn mark_sent(&mut self, cid: &str) -> Result<(), ExecError> {
        if self.orders.contains_key(cid) {
            self.store
                .update_order(&OrderUpdate { cid: cid.into(), status: OrderStatus::Sent.as_str().into(), ..Default::default() })
                .await?;
        } else if self.commands.contains_key(cid) {
            self.store.update_command(cid, "sent", None, None).await?;
        }
        Ok(())
    }

    /// The frame could not be delivered: nothing reached the broker.
    pub async fn mark_send_failed(&mut self, cid: &str, why: &str) -> Result<(), ExecError> {
        if self.orders.remove(cid).is_some() {
            self.store
                .update_order(&OrderUpdate {
                    cid: cid.into(),
                    status: OrderStatus::NotExecuted.as_str().into(),
                    message: Some(format!("not sent: {why}")),
                    ..Default::default()
                })
                .await?;
        } else if self.commands.remove(cid).is_some() {
            self.store.update_command(cid, "failed", None, Some(&format!("not sent: {why}"))).await?;
        }
        Ok(())
    }

    async fn submit_command(
        &mut self,
        action: OrderAction,
        ticket: i64,
        symbol: &str,
        side: Side,
        volume: f64,
        sl: f64,
        tp: f64,
        reason: &str,
        now_ms: TimestampMs,
    ) -> Result<Body, ExecError> {
        let name = match action {
            OrderAction::Close => "close",
            OrderAction::Modify => "modify",
            OrderAction::Cancel => "cancel",
            OrderAction::Open => return Err(ExecError::Refused("entries require an authorization".into())),
        };
        let cid = new_cid();
        let body = Body::Order {
            cid: cid.clone(),
            auth_id: format!("mgmt-{name}"),
            action,
            symbol: symbol.to_string(),
            side,
            kind: OrderKind::Market,
            volume,
            price: 0.0,
            sl,
            tp,
            deviation: 0.0,
            ticket,
            expires: now_ms + self.cfg.result_timeout_ms,
            magic: self.cfg.magic,
        };
        body.validate().map_err(ExecError::Refused)?;
        self.store
            .insert_command(&CommandRecord {
                cid: cid.clone(),
                action: name.into(),
                ticket,
                symbol: symbol.to_string(),
                volume,
                sl,
                tp,
                reason: reason.to_string(),
                status: "created".into(),
                retcode: None,
                message: None,
                created_ms: now_ms,
                updated_ms: now_ms,
            })
            .await?;
        self.commands.insert(cid, CommandInFlight { action: name, ticket });
        Ok(body)
    }

    /// Close a position fully (`volume = None`) or partially.
    pub async fn submit_close(&mut self, pos: &Position, volume: Option<f64>, reason: &str, now_ms: TimestampMs) -> Result<Body, ExecError> {
        let v = volume.unwrap_or(pos.volume);
        if !(v > 0.0 && v <= pos.volume + 1e-9) {
            return Err(ExecError::Refused(format!("close volume {v} outside (0, {}]", pos.volume)));
        }
        self.submit_command(OrderAction::Close, pos.ticket, &pos.symbol, pos.side, v, 0.0, 0.0, reason, now_ms).await
    }

    /// Change SL/TP. The stop can be tightened or kept, never removed or loosened:
    /// a modification must not increase the risk that was authorized.
    pub async fn submit_modify(&mut self, pos: &Position, sl: f64, tp: f64, reason: &str, now_ms: TimestampMs) -> Result<Body, ExecError> {
        if !(sl.is_finite() && sl > 0.0 && tp.is_finite() && tp >= 0.0) {
            return Err(ExecError::Refused("a stop-loss is mandatory".into()));
        }
        let dir = pos.side.sign();
        if pos.sl > 0.0 && (sl - pos.sl) * dir < -1e-12 {
            return Err(ExecError::Refused("the stop-loss may only be moved in the protective direction".into()));
        }
        if tp > 0.0 && (tp - pos.price_open) * dir <= 0.0 {
            return Err(ExecError::Refused("take-profit on the wrong side of the entry".into()));
        }
        self.submit_command(OrderAction::Modify, pos.ticket, &pos.symbol, pos.side, pos.volume, sl, tp, reason, now_ms).await
    }

    pub async fn submit_cancel(&mut self, ord: &PendingOrder, reason: &str, now_ms: TimestampMs) -> Result<Body, ExecError> {
        self.submit_command(OrderAction::Cancel, ord.ticket, &ord.symbol, ord.side, ord.volume, 0.0, 0.0, reason, now_ms).await
    }

    /// Process the connector's report for an instruction.
    #[allow(clippy::too_many_arguments)]
    pub async fn on_result(
        &mut self,
        cid: &str,
        ok: bool,
        retcode: i64,
        ticket: i64,
        price: f64,
        volume: f64,
        msg: &str,
    ) -> Result<ExecEvent, ExecError> {
        if let Some(cmd) = self.commands.remove(cid) {
            let status = if ok { "done" } else { "failed" };
            self.store.update_command(cid, status, Some(retcode), Some(msg)).await?;
            return Ok(if ok {
                ExecEvent::CommandDone { cid: cid.into(), action: cmd.action.into(), ticket: cmd.ticket }
            } else {
                ExecEvent::CommandFailed { cid: cid.into(), action: cmd.action.into(), ticket: cmd.ticket, message: msg.into() }
            });
        }
        let Some(o) = self.orders.remove(cid) else {
            return Ok(ExecEvent::UnexpectedResult { cid: cid.into() });
        };
        if !ok {
            self.store
                .update_order(&OrderUpdate {
                    cid: cid.into(),
                    status: OrderStatus::Rejected.as_str().into(),
                    retcode: Some(retcode),
                    message: Some(msg.into()),
                    ..Default::default()
                })
                .await?;
            return Ok(ExecEvent::Rejected { cid: cid.into(), retcode, message: msg.into() });
        }
        if o.kind != OrderKind::Market {
            self.store
                .update_order(&OrderUpdate {
                    cid: cid.into(),
                    status: OrderStatus::Placed.as_str().into(),
                    ticket: Some(ticket),
                    retcode: Some(retcode),
                    message: Some(msg.into()),
                    ..Default::default()
                })
                .await?;
            return Ok(ExecEvent::Placed { cid: cid.into(), ticket });
        }
        // A "success" without a ticket, price or volume is not proof of execution.
        if ticket <= 0 || !(price > 0.0) || !(volume > 0.0) {
            self.orders.insert(cid.to_string(), InFlight { timed_out: true, ..o });
            self.store
                .update_order(&OrderUpdate {
                    cid: cid.into(),
                    status: OrderStatus::Unknown.as_str().into(),
                    retcode: Some(retcode),
                    message: Some(format!("incomplete broker answer: {msg}")),
                    ..Default::default()
                })
                .await?;
            return Ok(ExecEvent::TimedOut { cid: cid.into() });
        }
        let partial = volume + 1e-9 < o.requested_volume;
        let status = if partial { OrderStatus::PartiallyFilled } else { OrderStatus::Filled };
        self.store
            .update_order(&OrderUpdate {
                cid: cid.into(),
                status: status.as_str().into(),
                ticket: Some(ticket),
                fill_price: Some(price),
                fill_volume: Some(volume),
                retcode: Some(retcode),
                message: Some(msg.into()),
            })
            .await?;
        let slippage_points = (o.point > 0.0).then(|| (price - o.price) * o.side.sign() / o.point);
        Ok(ExecEvent::Filled { cid: cid.into(), ticket, price, volume, requested_volume: o.requested_volume, slippage_points })
    }

    /// Flag instructions whose outcome did not arrive in time.
    pub async fn check_timeouts(&mut self, now_ms: TimestampMs) -> Result<Vec<ExecEvent>, ExecError> {
        let mut events = Vec::new();
        let late: Vec<String> = self
            .orders
            .iter()
            .filter(|(_, o)| !o.timed_out && now_ms - o.sent_ms > self.cfg.result_timeout_ms)
            .map(|(cid, _)| cid.clone())
            .collect();
        for cid in late {
            if let Some(o) = self.orders.get_mut(&cid) {
                o.timed_out = true;
            }
            self.store
                .update_order(&OrderUpdate {
                    cid: cid.clone(),
                    status: OrderStatus::Unknown.as_str().into(),
                    message: Some("no answer from the connector".into()),
                    ..Default::default()
                })
                .await?;
            events.push(ExecEvent::TimedOut { cid });
        }
        Ok(events)
    }

    /// Settle unknown orders against the broker's own picture of the account.
    /// `snapshot_ms` is when the snapshot was received.
    pub async fn reconcile(
        &mut self,
        positions: &[Position],
        pending: &[PendingOrder],
        snapshot_ms: TimestampMs,
    ) -> Result<Vec<ExecEvent>, ExecError> {
        let mut events = Vec::new();
        let unknown: Vec<String> = self.orders.iter().filter(|(_, o)| o.timed_out).map(|(c, _)| c.clone()).collect();
        for cid in unknown {
            let found_pos = positions.iter().find(|p| p.comment == cid && p.magic == self.cfg.magic);
            let found_ord = pending.iter().find(|p| p.comment == cid && p.magic == self.cfg.magic);
            if let Some(p) = found_pos {
                self.orders.remove(&cid);
                self.store
                    .update_order(&OrderUpdate {
                        cid: cid.clone(),
                        status: OrderStatus::Filled.as_str().into(),
                        ticket: Some(p.ticket),
                        fill_price: Some(p.price_open),
                        fill_volume: Some(p.volume),
                        message: Some("found on the account during reconciliation".into()),
                        ..Default::default()
                    })
                    .await?;
                events.push(ExecEvent::ReconciledFilled { cid, ticket: p.ticket });
            } else if let Some(p) = found_ord {
                self.orders.remove(&cid);
                self.store
                    .update_order(&OrderUpdate {
                        cid: cid.clone(),
                        status: OrderStatus::Placed.as_str().into(),
                        ticket: Some(p.ticket),
                        message: Some("pending order found during reconciliation".into()),
                        ..Default::default()
                    })
                    .await?;
                events.push(ExecEvent::Placed { cid, ticket: p.ticket });
            } else {
                let sent = self.orders.get(&cid).map(|o| o.sent_ms).unwrap_or(0);
                if snapshot_ms - sent >= self.cfg.reconcile_grace_ms {
                    self.orders.remove(&cid);
                    self.store
                        .update_order(&OrderUpdate {
                            cid: cid.clone(),
                            status: OrderStatus::NotExecuted.as_str().into(),
                            message: Some("absent from the account after reconciliation".into()),
                            ..Default::default()
                        })
                        .await?;
                    events.push(ExecEvent::ReconciledNotExecuted { cid });
                }
            }
        }
        Ok(events)
    }

    /// Positions on the account that AEGIS did not open (other magic number).
    pub fn foreign_positions<'a>(&self, positions: &'a [Position]) -> Vec<&'a Position> {
        positions.iter().filter(|p| p.magic != self.cfg.magic).collect()
    }
}
