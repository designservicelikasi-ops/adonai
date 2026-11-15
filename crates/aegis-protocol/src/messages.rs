use aegis_types::{
    AccountSnapshot, AccountTradeMode, Candle, Deal, OrderKind, PendingOrder, Position, Side,
    SymbolSpec, SymbolTradeMode, TerminalKind, Timeframe, TimestampMs,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    pub id: String,
    pub ts: TimestampMs,
    pub seq: u64,
    pub sid: String,
    #[serde(flatten)]
    pub body: Body,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderAction {
    /// Open a market position or place a pending order.
    Open,
    /// Modify SL/TP of a position (`ticket`).
    Modify,
    /// Close a position, fully or partially (`volume`).
    Close,
    /// Cancel a pending order (`ticket`).
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Body {
    // ---- handshake
    Hello { nonce: String, terminal: TerminalKind, login: i64, server: String, trade_mode: AccountTradeMode, build: String },
    Welcome { nonce: String },
    Heartbeat {},
    Bye { reason: String },
    Error { code: String, msg: String },

    // ---- connector -> engine
    Spec {
        symbol: String, digits: u32, point: f64, contract_size: f64, tick_size: f64, tick_value: f64,
        volume_min: f64, volume_max: f64, volume_step: f64, stops_level: f64,
        swap_long: f64, swap_short: f64, trade_mode: SymbolTradeMode, currency_profit: String,
    },
    Tick { symbol: String, time: TimestampMs, bid: f64, ask: f64 },
    Account {
        login: i64, server: String, currency: String, balance: f64, equity: f64, margin: f64,
        margin_free: f64, leverage: i64, trade_mode: AccountTradeMode, trade_allowed: bool,
    },
    Pos {
        snap: i64, ticket: i64, symbol: String, side: Side, volume: f64, price_open: f64, sl: f64, tp: f64,
        profit: f64, swap: f64, commission: f64, magic: i64, comment: String, open_time: TimestampMs,
    },
    Ord {
        snap: i64, ticket: i64, symbol: String, side: Side, kind: OrderKind, volume: f64, price: f64,
        sl: f64, tp: f64, magic: i64, comment: String, setup_time: TimestampMs,
    },
    /// Closes a position/order snapshot; counts let the engine detect lost frames.
    SnapEnd { snap: i64, positions: u32, orders: u32 },
    Deal {
        deal: i64, position: i64, symbol: String, side: Side, volume: f64, price: f64,
        profit: f64, swap: f64, commission: f64, time: TimestampMs, comment: String,
        /// Comment of the order that opened the position (the AEGIS `cid` for AEGIS orders).
        origin: String,
    },
    BalanceOp { deal: i64, amount: f64, time: TimestampMs },
    /// Outcome of an `order` instruction, as reported by the broker.
    Result { cid: String, ok: bool, retcode: i64, ticket: i64, price: f64, volume: f64, msg: String },
    Bar {
        symbol: String, tf: String, time: TimestampMs, open: f64, high: f64, low: f64, close: f64,
        volume: i64, spread: f64, last: bool,
    },

    // ---- engine -> connector
    Order {
        cid: String, auth_id: String, action: OrderAction, symbol: String, side: Side, kind: OrderKind,
        volume: f64, price: f64, sl: f64, tp: f64, deviation: f64, ticket: i64,
        /// The connector must refuse the instruction after this time (ms, engine clock).
        expires: TimestampMs, magic: i64,
    },
    HistoryReq { symbol: String, tf: String, count: u32 },
}

fn ok_symbol(s: &str) -> Result<(), String> {
    let valid = !s.is_empty()
        && s.len() <= 32
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '#' | '+' | '!'));
    if valid { Ok(()) } else { Err(format!("invalid symbol `{}`", s.chars().take(40).collect::<String>())) }
}

fn ok_text(name: &str, s: &str, max: usize) -> Result<(), String> {
    if s.len() <= max && !s.chars().any(|c| c.is_control()) {
        Ok(())
    } else {
        Err(format!("field `{name}` too long or contains control characters"))
    }
}

fn finite(name: &str, vals: &[f64]) -> Result<(), String> {
    if vals.iter().all(|v| v.is_finite()) { Ok(()) } else { Err(format!("non-finite number in `{name}`")) }
}

fn positive(name: &str, vals: &[f64]) -> Result<(), String> {
    if vals.iter().all(|v| v.is_finite() && *v > 0.0) { Ok(()) } else { Err(format!("`{name}` must be > 0")) }
}

fn non_negative(name: &str, vals: &[f64]) -> Result<(), String> {
    if vals.iter().all(|v| v.is_finite() && *v >= 0.0) { Ok(()) } else { Err(format!("`{name}` must be >= 0")) }
}

impl Body {
    pub fn kind(&self) -> &'static str {
        match self {
            Body::Hello { .. } => "hello",
            Body::Welcome { .. } => "welcome",
            Body::Heartbeat {} => "heartbeat",
            Body::Bye { .. } => "bye",
            Body::Error { .. } => "error",
            Body::Spec { .. } => "spec",
            Body::Tick { .. } => "tick",
            Body::Account { .. } => "account",
            Body::Pos { .. } => "pos",
            Body::Ord { .. } => "ord",
            Body::SnapEnd { .. } => "snap_end",
            Body::Deal { .. } => "deal",
            Body::BalanceOp { .. } => "balance_op",
            Body::Result { .. } => "result",
            Body::Bar { .. } => "bar",
            Body::Order { .. } => "order",
            Body::HistoryReq { .. } => "history_req",
        }
    }

    /// Structural validation applied to every decoded frame.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Body::Hello { nonce, server, build, login, .. } => {
                ok_text("nonce", nonce, 64)?;
                ok_text("server", server, 64)?;
                ok_text("build", build, 64)?;
                if *login <= 0 { return Err("login must be > 0".into()); }
                Ok(())
            }
            Body::Welcome { nonce } => ok_text("nonce", nonce, 64),
            Body::Heartbeat {} => Ok(()),
            Body::Bye { reason } => ok_text("reason", reason, 256),
            Body::Error { code, msg } => ok_text("code", code, 64).and(ok_text("msg", msg, 512)),
            Body::Spec { symbol, point, contract_size, tick_size, tick_value, volume_min, volume_max, volume_step, stops_level, swap_long, swap_short, currency_profit, .. } => {
                ok_symbol(symbol)?;
                ok_text("currency_profit", currency_profit, 8)?;
                positive("spec", &[*point, *contract_size, *tick_size, *tick_value, *volume_min, *volume_max, *volume_step])?;
                non_negative("stops_level", &[*stops_level])?;
                finite("swap", &[*swap_long, *swap_short])
            }
            Body::Tick { symbol, time, bid, ask } => {
                ok_symbol(symbol)?;
                positive("bid/ask", &[*bid, *ask])?;
                if ask < bid { return Err("ask < bid".into()); }
                if *time <= 0 { return Err("tick time must be > 0".into()); }
                Ok(())
            }
            Body::Account { login, server, currency, balance, equity, margin, margin_free, leverage, .. } => {
                ok_text("server", server, 64)?;
                ok_text("currency", currency, 8)?;
                if currency.is_empty() || *login <= 0 || *leverage < 0 { return Err("invalid account identity".into()); }
                non_negative("balance/equity/margin", &[*balance, *equity, *margin])?;
                finite("margin_free", &[*margin_free])
            }
            Body::Pos { symbol, volume, price_open, sl, tp, profit, swap, commission, comment, ticket, .. } => {
                ok_symbol(symbol)?;
                ok_text("comment", comment, 64)?;
                if *ticket <= 0 { return Err("ticket must be > 0".into()); }
                positive("volume/price_open", &[*volume, *price_open])?;
                non_negative("sl/tp", &[*sl, *tp])?;
                finite("profit/swap/commission", &[*profit, *swap, *commission])
            }
            Body::Ord { symbol, volume, price, sl, tp, comment, ticket, .. } => {
                ok_symbol(symbol)?;
                ok_text("comment", comment, 64)?;
                if *ticket <= 0 { return Err("ticket must be > 0".into()); }
                positive("volume/price", &[*volume, *price])?;
                non_negative("sl/tp", &[*sl, *tp])
            }
            Body::SnapEnd { .. } => Ok(()),
            Body::Deal { symbol, volume, price, profit, swap, commission, comment, deal, origin, .. } => {
                ok_symbol(symbol)?;
                ok_text("comment", comment, 64)?;
                ok_text("origin", origin, 64)?;
                if *deal <= 0 { return Err("deal ticket must be > 0".into()); }
                positive("volume/price", &[*volume, *price])?;
                finite("profit/swap/commission", &[*profit, *swap, *commission])
            }
            Body::BalanceOp { amount, .. } => finite("amount", &[*amount]),
            Body::Result { cid, price, volume, msg, .. } => {
                ok_text("cid", cid, 64)?;
                ok_text("msg", msg, 256)?;
                if cid.is_empty() { return Err("cid is empty".into()); }
                non_negative("price/volume", &[*price, *volume])
            }
            Body::Bar { symbol, tf, open, high, low, close, spread, volume, .. } => {
                ok_symbol(symbol)?;
                if Timeframe::parse(tf).is_none() { return Err(format!("unknown timeframe `{tf}`")); }
                positive("ohlc", &[*open, *high, *low, *close])?;
                non_negative("spread", &[*spread])?;
                if high < low || *volume < 0 { return Err("inconsistent bar".into()); }
                Ok(())
            }
            Body::Order { cid, auth_id, action, symbol, volume, price, sl, tp, deviation, ticket, .. } => {
                ok_symbol(symbol)?;
                ok_text("cid", cid, 64)?;
                ok_text("auth_id", auth_id, 64)?;
                if cid.is_empty() || auth_id.is_empty() { return Err("cid and auth_id are mandatory".into()); }
                non_negative("price/sl/tp/deviation", &[*price, *sl, *tp, *deviation])?;
                match action {
                    OrderAction::Open => {
                        positive("volume", &[*volume])?;
                        positive("sl/tp", &[*sl, *tp]).map_err(|_| "an entry must carry SL and TP".to_string())
                    }
                    OrderAction::Close => {
                        positive("volume", &[*volume])?;
                        if *ticket <= 0 { return Err("ticket required".into()); }
                        Ok(())
                    }
                    OrderAction::Modify | OrderAction::Cancel => {
                        if *ticket <= 0 { return Err("ticket required".into()); }
                        Ok(())
                    }
                }
            }
            Body::HistoryReq { symbol, tf, count } => {
                ok_symbol(symbol)?;
                if Timeframe::parse(tf).is_none() { return Err(format!("unknown timeframe `{tf}`")); }
                if *count == 0 || *count > 100_000 { return Err("count out of range".into()); }
                Ok(())
            }
        }
    }

    // ---- conversions to domain types (None when the variant does not match)

    pub fn to_symbol_spec(&self) -> Option<SymbolSpec> {
        match self {
            Body::Spec { symbol, digits, point, contract_size, tick_size, tick_value, volume_min, volume_max, volume_step, stops_level, swap_long, swap_short, trade_mode, currency_profit } => Some(SymbolSpec {
                symbol: symbol.clone(),
                digits: *digits,
                point: *point,
                contract_size: *contract_size,
                tick_size: *tick_size,
                tick_value: *tick_value,
                volume_min: *volume_min,
                volume_max: *volume_max,
                volume_step: *volume_step,
                stops_level_points: *stops_level,
                swap_long: *swap_long,
                swap_short: *swap_short,
                trade_mode: *trade_mode,
                currency_profit: currency_profit.clone(),
            }),
            _ => None,
        }
    }

    pub fn from_symbol_spec(s: &SymbolSpec) -> Body {
        Body::Spec {
            symbol: s.symbol.clone(),
            digits: s.digits,
            point: s.point,
            contract_size: s.contract_size,
            tick_size: s.tick_size,
            tick_value: s.tick_value,
            volume_min: s.volume_min,
            volume_max: s.volume_max,
            volume_step: s.volume_step,
            stops_level: s.stops_level_points,
            swap_long: s.swap_long,
            swap_short: s.swap_short,
            trade_mode: s.trade_mode,
            currency_profit: s.currency_profit.clone(),
        }
    }

    /// `received_ms` is the engine's receive time: freshness is judged on the engine clock.
    pub fn to_account(&self, received_ms: TimestampMs) -> Option<AccountSnapshot> {
        match self {
            Body::Account { login, server, currency, balance, equity, margin, margin_free, leverage, trade_mode, trade_allowed } => Some(AccountSnapshot {
                ts_ms: received_ms,
                login: *login,
                server: server.clone(),
                currency: currency.clone(),
                balance: *balance,
                equity: *equity,
                margin: *margin,
                margin_free: *margin_free,
                leverage: *leverage,
                trade_mode: *trade_mode,
                trade_allowed: *trade_allowed,
            }),
            _ => None,
        }
    }

    pub fn from_account(a: &AccountSnapshot) -> Body {
        Body::Account {
            login: a.login,
            server: a.server.clone(),
            currency: a.currency.clone(),
            balance: a.balance,
            equity: a.equity,
            margin: a.margin,
            margin_free: a.margin_free,
            leverage: a.leverage,
            trade_mode: a.trade_mode,
            trade_allowed: a.trade_allowed,
        }
    }

    pub fn to_position(&self) -> Option<Position> {
        match self {
            Body::Pos { ticket, symbol, side, volume, price_open, sl, tp, profit, swap, commission, magic, comment, open_time, .. } => Some(Position {
                ticket: *ticket,
                symbol: symbol.clone(),
                side: *side,
                volume: *volume,
                price_open: *price_open,
                sl: *sl,
                tp: *tp,
                profit: *profit,
                swap: *swap,
                commission: *commission,
                magic: *magic,
                comment: comment.clone(),
                open_ts: *open_time,
            }),
            _ => None,
        }
    }

    pub fn from_position(snap: i64, p: &Position) -> Body {
        Body::Pos {
            snap,
            ticket: p.ticket,
            symbol: p.symbol.clone(),
            side: p.side,
            volume: p.volume,
            price_open: p.price_open,
            sl: p.sl,
            tp: p.tp,
            profit: p.profit,
            swap: p.swap,
            commission: p.commission,
            magic: p.magic,
            comment: p.comment.clone(),
            open_time: p.open_ts,
        }
    }

    pub fn to_pending(&self) -> Option<PendingOrder> {
        match self {
            Body::Ord { ticket, symbol, side, kind, volume, price, sl, tp, magic, comment, setup_time, .. } => Some(PendingOrder {
                ticket: *ticket,
                symbol: symbol.clone(),
                side: *side,
                kind: *kind,
                volume: *volume,
                price: *price,
                sl: *sl,
                tp: *tp,
                magic: *magic,
                comment: comment.clone(),
                setup_ts: *setup_time,
            }),
            _ => None,
        }
    }

    pub fn from_pending(snap: i64, o: &PendingOrder) -> Body {
        Body::Ord {
            snap,
            ticket: o.ticket,
            symbol: o.symbol.clone(),
            side: o.side,
            kind: o.kind,
            volume: o.volume,
            price: o.price,
            sl: o.sl,
            tp: o.tp,
            magic: o.magic,
            comment: o.comment.clone(),
            setup_time: o.setup_ts,
        }
    }

    pub fn to_deal(&self) -> Option<Deal> {
        match self {
            Body::Deal { deal, position, symbol, side, volume, price, profit, swap, commission, time, comment, origin } => Some(Deal {
                deal_ticket: *deal,
                position_ticket: *position,
                symbol: symbol.clone(),
                side: *side,
                volume: *volume,
                price: *price,
                profit: *profit,
                swap: *swap,
                commission: *commission,
                ts_ms: *time,
                comment: comment.clone(),
                origin: origin.clone(),
            }),
            _ => None,
        }
    }

    pub fn from_deal(d: &Deal) -> Body {
        Body::Deal {
            deal: d.deal_ticket,
            position: d.position_ticket,
            symbol: d.symbol.clone(),
            side: d.side,
            volume: d.volume,
            price: d.price,
            profit: d.profit,
            swap: d.swap,
            commission: d.commission,
            time: d.ts_ms,
            comment: d.comment.clone(),
            origin: d.origin.clone(),
        }
    }

    pub fn to_candle(&self) -> Option<(String, Timeframe, Candle, bool)> {
        match self {
            Body::Bar { symbol, tf, time, open, high, low, close, volume, spread, last } => Some((
                symbol.clone(),
                Timeframe::parse(tf)?,
                Candle { open_ts: *time, open: *open, high: *high, low: *low, close: *close, tick_volume: *volume, spread_points: *spread },
                *last,
            )),
            _ => None,
        }
    }
}
