//! Simulated broker account (TEST ADAPTER — not a real broker).
//!
//! It applies the same rules the MQL5 connector applies before calling the
//! broker: expiry, idempotency by `cid`, volume and stop validation, maximum
//! deviation. Faults can be injected to exercise the engine's failure handling.

use aegis_protocol::{Body, OrderAction};
use aegis_types::{
    AccountSnapshot, AccountTradeMode, Deal, OrderKind, PendingOrder, Position, Side, SymbolSpec,
    SymbolTradeMode, TimestampMs,
};
use std::collections::HashMap;

pub const RET_DONE: i64 = 10009;
pub const RET_PLACED: i64 = 10008;
pub const RET_DONE_PARTIAL: i64 = 10010;
pub const RET_REQUOTE: i64 = 10004;
pub const RET_REJECT: i64 = 10006;
pub const RET_INVALID: i64 = 10013;
pub const RET_INVALID_VOLUME: i64 = 10014;
pub const RET_INVALID_STOPS: i64 = 10016;
pub const RET_MARKET_CLOSED: i64 = 10018;
/// AEGIS connector codes (outside the broker range).
pub const RET_AEGIS_EXPIRED: i64 = 90001;
pub const RET_AEGIS_NOT_FOUND: i64 = 90002;

/// Fault applied to the next instruction received.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fault {
    None,
    /// Refuse with this broker return code.
    Reject(i64),
    /// Execute but never report the result (lost acknowledgement).
    ExecuteSilently,
    /// Drop the instruction entirely (lost request).
    Drop,
    /// Fill only this fraction of the requested volume.
    Partial(f64),
    /// Report success with an empty ticket/price (malformed broker answer).
    EmptySuccess,
}

#[derive(Debug, Clone)]
pub struct SimBrokerConfig {
    pub login: i64,
    pub server: String,
    pub currency: String,
    pub balance: f64,
    pub leverage: i64,
    pub trade_mode: AccountTradeMode,
    pub commission_per_lot_round_turn: f64,
    /// Adverse slippage applied to market fills, in points.
    pub slippage_points: f64,
    pub specs: Vec<SymbolSpec>,
}

pub const SIM_SYMBOL: &str = "EURUSD.sim";

pub fn sim_spec() -> SymbolSpec {
    SymbolSpec {
        symbol: SIM_SYMBOL.into(),
        digits: 5,
        point: 0.00001,
        contract_size: 100_000.0,
        tick_size: 0.00001,
        tick_value: 1.0,
        volume_min: 0.01,
        volume_max: 50.0,
        volume_step: 0.01,
        stops_level_points: 10.0,
        swap_long: -6.5,
        swap_short: 1.2,
        trade_mode: SymbolTradeMode::Full,
        currency_profit: "USD".into(),
    }
}

impl Default for SimBrokerConfig {
    fn default() -> Self {
        SimBrokerConfig {
            login: 900_001,
            server: "AEGIS-Simulator".into(),
            currency: "USD".into(),
            balance: 10_000.0,
            leverage: 100,
            trade_mode: AccountTradeMode::Demo,
            commission_per_lot_round_turn: 7.0,
            slippage_points: 0.0,
            specs: vec![sim_spec()],
        }
    }
}

/// Fields of a `result` message.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub cid: String,
    pub ok: bool,
    pub retcode: i64,
    pub ticket: i64,
    pub price: f64,
    pub volume: f64,
    pub msg: String,
}

impl Outcome {
    pub fn to_body(&self) -> Body {
        Body::Result {
            cid: self.cid.clone(),
            ok: self.ok,
            retcode: self.retcode,
            ticket: self.ticket,
            price: self.price,
            volume: self.volume,
            msg: self.msg.clone(),
        }
    }
}

pub struct SimBroker {
    pub cfg: SimBrokerConfig,
    balance: f64,
    positions: Vec<Position>,
    pending: Vec<PendingOrder>,
    prices: HashMap<String, (f64, f64)>,
    next_ticket: i64,
    processed: HashMap<String, Outcome>,
    pub fault: Fault,
    /// Number of `open` instructions that actually reached the "broker".
    pub executions: u64,
}

impl SimBroker {
    pub fn new(cfg: SimBrokerConfig) -> Self {
        SimBroker {
            balance: cfg.balance,
            cfg,
            positions: Vec::new(),
            pending: Vec::new(),
            prices: HashMap::new(),
            next_ticket: 1_000,
            processed: HashMap::new(),
            fault: Fault::None,
            executions: 0,
        }
    }

    fn spec(&self, symbol: &str) -> Option<&SymbolSpec> {
        self.cfg.specs.iter().find(|s| s.symbol == symbol)
    }

    fn ticket(&mut self) -> i64 {
        self.next_ticket += 1;
        self.next_ticket
    }

    pub fn positions(&self) -> &[Position] {
        &self.positions
    }

    pub fn pending(&self) -> &[PendingOrder] {
        &self.pending
    }

    pub fn balance(&self) -> f64 {
        self.balance
    }

    fn refresh_profit(&mut self) {
        let specs = self.cfg.specs.clone();
        for p in &mut self.positions {
            if let (Some(spec), Some((bid, ask))) = (specs.iter().find(|s| s.symbol == p.symbol), self.prices.get(&p.symbol)) {
                let exit = if p.side == Side::Buy { *bid } else { *ask };
                p.profit = (exit - p.price_open) * p.side.sign() / spec.tick_size * spec.tick_value * p.volume;
            }
        }
    }

    pub fn account(&self, now: TimestampMs) -> AccountSnapshot {
        let floating: f64 = self.positions.iter().map(|p| p.profit + p.swap + p.commission).sum();
        let margin: f64 = self
            .positions
            .iter()
            .filter_map(|p| self.spec(&p.symbol).map(|s| p.price_open / s.tick_size * s.tick_value * p.volume / self.cfg.leverage as f64))
            .sum();
        let equity = self.balance + floating;
        AccountSnapshot {
            ts_ms: now,
            login: self.cfg.login,
            server: self.cfg.server.clone(),
            currency: self.cfg.currency.clone(),
            balance: self.balance,
            equity,
            margin,
            margin_free: equity - margin,
            leverage: self.cfg.leverage,
            trade_mode: self.cfg.trade_mode,
            trade_allowed: true,
        }
    }

    fn close_at(&mut self, index: usize, volume: f64, price: f64, now: TimestampMs, comment: &str) -> Deal {
        let spec = self.spec(&self.positions[index].symbol).cloned().expect("position has a known symbol");
        let p = &mut self.positions[index];
        let profit = (price - p.price_open) * p.side.sign() / spec.tick_size * spec.tick_value * volume;
        let commission = -self.cfg.commission_per_lot_round_turn * volume;
        let deal = Deal {
            deal_ticket: 0,
            position_ticket: p.ticket,
            symbol: p.symbol.clone(),
            side: p.side,
            volume,
            price,
            profit,
            swap: 0.0,
            commission,
            ts_ms: now,
            comment: comment.to_string(),
            origin: p.comment.clone(),
        };
        p.volume = aegis_types::round_to(p.volume - volume, 8);
        if p.volume < spec.volume_min - 1e-9 {
            self.positions.remove(index);
        }
        self.balance += profit + commission;
        let ticket = self.ticket();
        Deal { deal_ticket: ticket, ..deal }
    }

    /// Feed a new quote. Triggers stops, targets and pending orders. Returns closing deals.
    pub fn on_price(&mut self, symbol: &str, bid: f64, ask: f64, now: TimestampMs) -> Vec<Deal> {
        self.prices.insert(symbol.to_string(), (bid, ask));
        let mut deals = Vec::new();
        // Pending orders first: a triggered order becomes a position at the trigger price.
        let mut i = 0;
        while i < self.pending.len() {
            let o = &self.pending[i];
            let hit = o.symbol == symbol
                && match (o.kind, o.side) {
                    (OrderKind::Limit, Side::Buy) => ask <= o.price,
                    (OrderKind::Stop, Side::Buy) => ask >= o.price,
                    (OrderKind::Limit, Side::Sell) => bid >= o.price,
                    (OrderKind::Stop, Side::Sell) => bid <= o.price,
                    _ => false,
                };
            if hit {
                let o = self.pending.remove(i);
                self.positions.push(Position {
                    ticket: o.ticket,
                    symbol: o.symbol,
                    side: o.side,
                    volume: o.volume,
                    price_open: o.price,
                    sl: o.sl,
                    tp: o.tp,
                    profit: 0.0,
                    swap: 0.0,
                    commission: 0.0,
                    magic: o.magic,
                    comment: o.comment,
                    open_ts: now,
                });
            } else {
                i += 1;
            }
        }
        let mut i = 0;
        while i < self.positions.len() {
            let p = &self.positions[i];
            let exit = if p.side == Side::Buy { bid } else { ask };
            let stopped = p.sl > 0.0 && (exit - p.sl) * p.side.sign() <= 0.0;
            let target = p.tp > 0.0 && (exit - p.tp) * p.side.sign() >= 0.0;
            if p.symbol == symbol && (stopped || target) {
                let volume = p.volume;
                let before = self.positions.len();
                deals.push(self.close_at(i, volume, exit, now, if stopped { "sl" } else { "tp" }));
                if self.positions.len() == before {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
        self.refresh_profit();
        deals
    }

    fn reject(cid: &str, retcode: i64, msg: &str) -> Outcome {
        Outcome { cid: cid.to_string(), ok: false, retcode, ticket: 0, price: 0.0, volume: 0.0, msg: msg.to_string() }
    }

    /// Handle an `order` instruction. Returns `(outcome to report, closing deals)`.
    /// `None` means nothing is reported (injected fault).
    pub fn execute(&mut self, body: &Body, now: TimestampMs) -> (Option<Outcome>, Vec<Deal>) {
        let Body::Order { cid, action, symbol, side, kind, volume, price, sl, tp, deviation, ticket, expires, magic, .. } = body else {
            return (None, Vec::new());
        };
        // Idempotency: an instruction already processed is answered again, not executed again.
        if let Some(prev) = self.processed.get(cid) {
            return (Some(prev.clone()), Vec::new());
        }
        let fault = std::mem::replace(&mut self.fault, Fault::None);
        if fault == Fault::Drop {
            return (None, Vec::new());
        }
        let mut deals = Vec::new();
        let outcome = if now > *expires {
            Self::reject(cid, RET_AEGIS_EXPIRED, "instruction expired")
        } else if let Fault::Reject(code) = fault {
            Self::reject(cid, code, "rejected by broker (injected)")
        } else {
            match action {
                OrderAction::Open => self.open(cid, symbol, *side, *kind, *volume, *price, *sl, *tp, *deviation, *magic, fault, now),
                OrderAction::Close => match self.positions.iter().position(|p| p.ticket == *ticket) {
                    Some(i) if *volume <= self.positions[i].volume + 1e-9 => {
                        let (bid, ask) = self.prices.get(symbol).copied().unwrap_or((0.0, 0.0));
                        let exit = if self.positions[i].side == Side::Buy { bid } else { ask };
                        if exit <= 0.0 {
                            Self::reject(cid, RET_MARKET_CLOSED, "no price")
                        } else {
                            let d = self.close_at(i, *volume, exit, now, "close");
                            let out = Outcome { cid: cid.clone(), ok: true, retcode: RET_DONE, ticket: *ticket, price: exit, volume: *volume, msg: "closed".into() };
                            deals.push(d);
                            out
                        }
                    }
                    Some(_) => Self::reject(cid, RET_INVALID_VOLUME, "close volume exceeds position"),
                    None => Self::reject(cid, RET_AEGIS_NOT_FOUND, "position not found"),
                },
                OrderAction::Modify => match self.positions.iter_mut().find(|p| p.ticket == *ticket) {
                    Some(p) => {
                        let dir = p.side.sign();
                        let (bid, ask) = self.prices.get(symbol).copied().unwrap_or((p.price_open, p.price_open));
                        let exit = if p.side == Side::Buy { bid } else { ask };
                        if (exit - *sl) * dir <= 0.0 || (*tp > 0.0 && (*tp - exit) * dir <= 0.0) {
                            Self::reject(cid, RET_INVALID_STOPS, "invalid stops")
                        } else {
                            p.sl = *sl;
                            p.tp = *tp;
                            Outcome { cid: cid.clone(), ok: true, retcode: RET_DONE, ticket: *ticket, price: 0.0, volume: 0.0, msg: "modified".into() }
                        }
                    }
                    None => Self::reject(cid, RET_AEGIS_NOT_FOUND, "position not found"),
                },
                OrderAction::Cancel => match self.pending.iter().position(|o| o.ticket == *ticket) {
                    Some(i) => {
                        self.pending.remove(i);
                        Outcome { cid: cid.clone(), ok: true, retcode: RET_DONE, ticket: *ticket, price: 0.0, volume: 0.0, msg: "cancelled".into() }
                    }
                    None => Self::reject(cid, RET_AEGIS_NOT_FOUND, "order not found"),
                },
            }
        };
        self.refresh_profit();
        self.processed.insert(cid.clone(), outcome.clone());
        match fault {
            Fault::ExecuteSilently => (None, deals),
            Fault::EmptySuccess if outcome.ok => {
                (Some(Outcome { ticket: 0, price: 0.0, volume: 0.0, msg: "empty answer (injected)".into(), ..outcome }), deals)
            }
            _ => (Some(outcome), deals),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn open(
        &mut self,
        cid: &str,
        symbol: &str,
        side: Side,
        kind: OrderKind,
        volume: f64,
        price: f64,
        sl: f64,
        tp: f64,
        deviation: f64,
        magic: i64,
        fault: Fault,
        now: TimestampMs,
    ) -> Outcome {
        let Some(spec) = self.spec(symbol).cloned() else {
            return Self::reject(cid, RET_INVALID, "unknown symbol");
        };
        let Some((bid, ask)) = self.prices.get(symbol).copied() else {
            return Self::reject(cid, RET_MARKET_CLOSED, "no price");
        };
        if !spec.is_volume_valid(volume) {
            return Self::reject(cid, RET_INVALID_VOLUME, "invalid volume");
        }
        let dir = side.sign();
        let market = if side == Side::Buy { ask } else { bid };
        let reference = if kind == OrderKind::Market { market } else { price };
        let min = spec.stops_level_points * spec.point;
        if (reference - sl) * dir < min || (tp - reference) * dir < min {
            return Self::reject(cid, RET_INVALID_STOPS, "invalid stops");
        }
        self.executions += 1;
        if kind != OrderKind::Market {
            let ticket = self.ticket();
            self.pending.push(PendingOrder { ticket, symbol: symbol.into(), side, kind, volume, price, sl, tp, magic, comment: cid.into(), setup_ts: now });
            return Outcome { cid: cid.into(), ok: true, retcode: RET_PLACED, ticket, price: 0.0, volume: 0.0, msg: "placed".into() };
        }
        let fill = spec.round_price(market + dir * self.cfg.slippage_points * spec.point);
        if (fill - price).abs() / spec.point > deviation + 1e-6 {
            self.executions -= 1;
            return Self::reject(cid, RET_REQUOTE, "price moved beyond the allowed deviation");
        }
        let filled = match fault {
            Fault::Partial(ratio) => spec.normalize_volume(volume * ratio).max(spec.volume_min),
            _ => volume,
        };
        let ticket = self.ticket();
        self.positions.push(Position {
            ticket,
            symbol: symbol.into(),
            side,
            volume: filled,
            price_open: fill,
            sl,
            tp,
            profit: 0.0,
            swap: 0.0,
            commission: 0.0,
            magic,
            comment: cid.into(),
            open_ts: now,
        });
        let partial = filled + 1e-9 < volume;
        Outcome {
            cid: cid.into(),
            ok: true,
            retcode: if partial { RET_DONE_PARTIAL } else { RET_DONE },
            ticket,
            price: fill,
            volume: filled,
            msg: if partial { "partially filled".into() } else { "done".into() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    fn order(cid: &str, action: OrderAction, ticket: i64, volume: f64) -> Body {
        Body::Order {
            cid: cid.into(),
            auth_id: "auth".into(),
            action,
            symbol: SIM_SYMBOL.into(),
            side: Side::Buy,
            kind: OrderKind::Market,
            volume,
            price: 1.10010,
            sl: 1.09810,
            tp: 1.10410,
            deviation: 10.0,
            ticket,
            expires: NOW + 3_000,
            magic: 7,
        }
    }

    fn broker() -> SimBroker {
        let mut b = SimBroker::new(SimBrokerConfig::default());
        b.on_price(SIM_SYMBOL, 1.10000, 1.10010, NOW);
        b
    }

    #[test]
    fn market_order_fills_and_take_profit_closes_with_exact_accounting() {
        let mut b = broker();
        let (out, _) = b.execute(&order("A1", OrderAction::Open, 0, 0.20), NOW);
        let out = out.unwrap();
        assert!(out.ok);
        assert_eq!((out.price, out.volume), (1.10010, 0.20));
        assert_eq!(b.positions()[0].comment, "A1");
        // Spread is paid immediately: floating = -10 points x 0.2 lot = -2.
        assert!((b.account(NOW).equity - 9_998.0).abs() < 1e-6);
        assert!(b.on_price(SIM_SYMBOL, 1.10200, 1.10210, NOW + 1).is_empty());
        let deals = b.on_price(SIM_SYMBOL, 1.10410, 1.10420, NOW + 2);
        assert_eq!(deals.len(), 1);
        // 400 points x 0.2 lot = 80 ; commission 7 x 0.2 = 1.4
        assert!((deals[0].profit - 80.0).abs() < 1e-6);
        assert!((deals[0].commission + 1.4).abs() < 1e-9);
        assert_eq!(deals[0].comment, "tp");
        assert!(b.positions().is_empty());
        assert!((b.balance() - 10_078.6).abs() < 1e-6);
    }

    #[test]
    fn stop_loss_closes_on_the_bid() {
        let mut b = broker();
        b.execute(&order("A1", OrderAction::Open, 0, 0.10), NOW);
        let deals = b.on_price(SIM_SYMBOL, 1.09800, 1.09810, NOW + 1);
        assert_eq!(deals[0].comment, "sl");
        assert!((deals[0].profit + 21.0).abs() < 1e-6, "1.10010 -> 1.09800 = 210 points x 0.1");
    }

    #[test]
    fn duplicate_instructions_are_answered_but_not_executed_twice() {
        let mut b = broker();
        let (first, _) = b.execute(&order("A1", OrderAction::Open, 0, 0.10), NOW);
        let (second, _) = b.execute(&order("A1", OrderAction::Open, 0, 0.10), NOW + 1);
        assert_eq!(first, second);
        assert_eq!(b.positions().len(), 1);
        assert_eq!(b.executions, 1);
    }

    #[test]
    fn expired_invalid_and_far_priced_instructions_are_refused() {
        let mut b = broker();
        let (out, _) = b.execute(&order("A1", OrderAction::Open, 0, 0.10), NOW + 10_000);
        assert_eq!(out.unwrap().retcode, RET_AEGIS_EXPIRED);
        let (out, _) = b.execute(&order("A2", OrderAction::Open, 0, 0.015), NOW);
        assert_eq!(out.unwrap().retcode, RET_INVALID_VOLUME);
        // Market ran away by 50 points; the allowed deviation is 10.
        b.on_price(SIM_SYMBOL, 1.10050, 1.10060, NOW);
        let (out, _) = b.execute(&order("A3", OrderAction::Open, 0, 0.10), NOW);
        assert_eq!(out.unwrap().retcode, RET_REQUOTE);
        // Stop too close.
        let mut bad = order("A4", OrderAction::Open, 0, 0.10);
        if let Body::Order { sl, price, .. } = &mut bad {
            *sl = 1.10058;
            *price = 1.10060;
        }
        let (out, _) = b.execute(&bad, NOW);
        assert_eq!(out.unwrap().retcode, RET_INVALID_STOPS);
        assert!(b.positions().is_empty());
        assert_eq!(b.executions, 0);
    }

    #[test]
    fn faults_are_injected_once() {
        let mut b = broker();
        b.fault = Fault::Reject(RET_REJECT);
        assert_eq!(b.execute(&order("A1", OrderAction::Open, 0, 0.10), NOW).0.unwrap().retcode, RET_REJECT);
        b.fault = Fault::Drop;
        assert!(b.execute(&order("A2", OrderAction::Open, 0, 0.10), NOW).0.is_none());
        assert!(b.positions().is_empty());
        b.fault = Fault::ExecuteSilently;
        assert!(b.execute(&order("A3", OrderAction::Open, 0, 0.10), NOW).0.is_none());
        assert_eq!(b.positions().len(), 1, "executed although nothing was reported");
        b.fault = Fault::Partial(0.5);
        let out = b.execute(&order("A4", OrderAction::Open, 0, 0.20), NOW).0.unwrap();
        assert_eq!((out.retcode, out.volume), (RET_DONE_PARTIAL, 0.10));
        b.fault = Fault::EmptySuccess;
        let out = b.execute(&order("A5", OrderAction::Open, 0, 0.10), NOW).0.unwrap();
        assert!(out.ok && out.ticket == 0);
        // Back to normal afterwards.
        assert_eq!(b.execute(&order("A6", OrderAction::Open, 0, 0.10), NOW).0.unwrap().retcode, RET_DONE);
    }

    #[test]
    fn close_modify_cancel_and_pending_orders() {
        let mut b = broker();
        let t = b.execute(&order("A1", OrderAction::Open, 0, 0.20), NOW).0.unwrap().ticket;
        // Partial close.
        let (out, deals) = b.execute(&order("C1", OrderAction::Close, t, 0.05), NOW);
        assert!(out.unwrap().ok);
        assert_eq!(deals.len(), 1);
        assert!((b.positions()[0].volume - 0.15).abs() < 1e-9);
        assert_eq!(b.execute(&order("C2", OrderAction::Close, t, 5.0), NOW).0.unwrap().retcode, RET_INVALID_VOLUME);
        assert_eq!(b.execute(&order("C3", OrderAction::Close, 999, 0.05), NOW).0.unwrap().retcode, RET_AEGIS_NOT_FOUND);
        // Modify.
        let mut m = order("M1", OrderAction::Modify, t, 0.0);
        if let Body::Order { sl, .. } = &mut m {
            *sl = 1.09900;
        }
        assert!(b.execute(&m, NOW).0.unwrap().ok);
        assert_eq!(b.positions()[0].sl, 1.09900);
        // Pending buy limit below the market, then triggered, then cancelled case.
        let mut lim = order("P1", OrderAction::Open, 0, 0.10);
        if let Body::Order { kind, price, sl, tp, .. } = &mut lim {
            *kind = OrderKind::Limit;
            *price = 1.09950;
            *sl = 1.09750;
            *tp = 1.10350;
        }
        let out = b.execute(&lim, NOW).0.unwrap();
        assert_eq!(out.retcode, RET_PLACED);
        assert_eq!(b.pending().len(), 1);
        b.on_price(SIM_SYMBOL, 1.09940, 1.09950, NOW + 5);
        assert!(b.pending().is_empty());
        assert_eq!(b.positions().len(), 2);
        let mut lim2 = lim.clone();
        if let Body::Order { cid, .. } = &mut lim2 {
            *cid = "P2".into();
        }
        if let Body::Order { price, sl, tp, .. } = &mut lim2 {
            *price = 1.09800;
            *sl = 1.09600;
            *tp = 1.10200;
        }
        let ticket = b.execute(&lim2, NOW).0.unwrap().ticket;
        assert!(b.execute(&order("X1", OrderAction::Cancel, ticket, 0.0), NOW).0.unwrap().ok);
        assert!(b.pending().is_empty());
        assert_eq!(b.execute(&order("X2", OrderAction::Cancel, ticket, 0.0), NOW).0.unwrap().retcode, RET_AEGIS_NOT_FOUND);
    }
}
