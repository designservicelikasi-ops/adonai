//! Shared domain types for every AEGIS module.
//!
//! Prices and money are `f64`. Volumes are always normalised to the broker
//! volume step before being used in an order (see `SymbolSpec::normalize_volume`).

use serde::{Deserialize, Serialize};
use std::fmt;

pub type TimestampMs = i64;

/// Current wall-clock time in milliseconds since the Unix epoch (UTC).
pub fn now_ms() -> TimestampMs {
    chrono::Utc::now().timestamp_millis()
}

/// UTC day index (days since epoch). The AEGIS accounting day is the UTC day.
pub fn utc_day(ts_ms: TimestampMs) -> i64 {
    ts_ms.div_euclid(86_400_000)
}

/// Calendar month (UTC) as `year * 12 + month - 1`: consecutive months differ by 1.
pub fn utc_month(ts_ms: TimestampMs) -> i64 {
    use chrono::Datelike;
    let d = chrono::DateTime::from_timestamp_millis(ts_ms).unwrap_or_default();
    d.year() as i64 * 12 + d.month0() as i64
}

pub fn format_ts(ts_ms: TimestampMs) -> String {
    chrono::DateTime::from_timestamp_millis(ts_ms)
        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "-".to_string())
}

/// Random identifier with a readable prefix, e.g. `dec-9f2c...`.
pub fn new_id(prefix: &str) -> String {
    let bytes: [u8; 10] = rand::random();
    format!("{prefix}-{}", hex::encode(bytes))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn sign(self) -> f64 {
        match self {
            Side::Buy => 1.0,
            Side::Sell => -1.0,
        }
    }
    pub fn opposite(self) -> Side {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Buy => "buy",
            Side::Sell => "sell",
        }
    }
    pub fn parse(s: &str) -> Option<Side> {
        match s {
            "buy" => Some(Side::Buy),
            "sell" => Some(Side::Sell),
            _ => None,
        }
    }
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Timeframe {
    M1,
    M5,
    M15,
    M30,
    H1,
    H4,
    D1,
}

impl Timeframe {
    pub const ALL: [Timeframe; 7] = [
        Timeframe::M1,
        Timeframe::M5,
        Timeframe::M15,
        Timeframe::M30,
        Timeframe::H1,
        Timeframe::H4,
        Timeframe::D1,
    ];
    pub fn millis(self) -> i64 {
        60_000
            * match self {
                Timeframe::M1 => 1,
                Timeframe::M5 => 5,
                Timeframe::M15 => 15,
                Timeframe::M30 => 30,
                Timeframe::H1 => 60,
                Timeframe::H4 => 240,
                Timeframe::D1 => 1440,
            }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Timeframe::M1 => "M1",
            Timeframe::M5 => "M5",
            Timeframe::M15 => "M15",
            Timeframe::M30 => "M30",
            Timeframe::H1 => "H1",
            Timeframe::H4 => "H4",
            Timeframe::D1 => "D1",
        }
    }
    pub fn parse(s: &str) -> Option<Timeframe> {
        Timeframe::ALL.into_iter().find(|t| t.as_str() == s)
    }
    /// Open timestamp of the bar containing `ts_ms`.
    pub fn bucket(self, ts_ms: TimestampMs) -> TimestampMs {
        ts_ms - ts_ms.rem_euclid(self.millis())
    }
}

impl fmt::Display for Timeframe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tick {
    pub ts_ms: TimestampMs,
    pub bid: f64,
    pub ask: f64,
}

impl Tick {
    pub fn mid(&self) -> f64 {
        (self.bid + self.ask) / 2.0
    }
    pub fn spread(&self) -> f64 {
        self.ask - self.bid
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Candle {
    pub open_ts: TimestampMs,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub tick_volume: i64,
    /// Average spread during the bar, in points. 0 when unknown.
    pub spread_points: f64,
}

impl Candle {
    pub fn is_valid(&self) -> bool {
        self.open.is_finite()
            && self.high.is_finite()
            && self.low.is_finite()
            && self.close.is_finite()
            && self.low > 0.0
            && self.high >= self.low
            && self.high >= self.open.max(self.close)
            && self.low <= self.open.min(self.close)
    }
}

/// Where a dataset came from. Synthetic data may be used to test the software
/// but can never justify real execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataOrigin {
    Broker,
    Synthetic,
}

impl DataOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            DataOrigin::Broker => "broker",
            DataOrigin::Synthetic => "synthetic",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "broker" => Some(DataOrigin::Broker),
            "synthetic" => Some(DataOrigin::Synthetic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolTradeMode {
    Full,
    CloseOnly,
    Disabled,
}

/// Broker contract specification for one instrument, as reported by the terminal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolSpec {
    pub symbol: String,
    pub digits: u32,
    pub point: f64,
    pub contract_size: f64,
    pub tick_size: f64,
    /// Value of one `tick_size` move for 1.0 lot, in account currency.
    pub tick_value: f64,
    pub volume_min: f64,
    pub volume_max: f64,
    pub volume_step: f64,
    /// Minimum distance between price and SL/TP, in points.
    pub stops_level_points: f64,
    /// Swap per lot per night in account currency (negative = cost).
    pub swap_long: f64,
    pub swap_short: f64,
    pub trade_mode: SymbolTradeMode,
    pub currency_profit: String,
    /// Base currency (EUR for EURUSD). Empty when the terminal did not report it.
    #[serde(default)]
    pub currency_base: String,
    /// Distance from the price inside which the broker freezes SL/TP changes, in points.
    #[serde(default)]
    pub freeze_level_points: f64,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SpecError {
    #[error("symbol spec field `{0}` is invalid")]
    Invalid(&'static str),
}

impl SymbolSpec {
    pub fn validate(&self) -> Result<(), SpecError> {
        let pos = |v: f64| v.is_finite() && v > 0.0;
        if self.symbol.is_empty() {
            return Err(SpecError::Invalid("symbol"));
        }
        if !pos(self.point) {
            return Err(SpecError::Invalid("point"));
        }
        if !pos(self.contract_size) {
            return Err(SpecError::Invalid("contract_size"));
        }
        if !pos(self.tick_size) {
            return Err(SpecError::Invalid("tick_size"));
        }
        if !pos(self.tick_value) {
            return Err(SpecError::Invalid("tick_value"));
        }
        if !pos(self.volume_min) || !pos(self.volume_step) || !pos(self.volume_max) {
            return Err(SpecError::Invalid("volume"));
        }
        if self.volume_max < self.volume_min {
            return Err(SpecError::Invalid("volume_max"));
        }
        if !self.stops_level_points.is_finite() || self.stops_level_points < 0.0 {
            return Err(SpecError::Invalid("stops_level_points"));
        }
        if !self.swap_long.is_finite() || !self.swap_short.is_finite() {
            return Err(SpecError::Invalid("swap"));
        }
        Ok(())
    }

    /// Money value (account currency) of a price distance for a given volume.
    pub fn price_distance_value(&self, distance: f64, volume: f64) -> f64 {
        distance.abs() / self.tick_size * self.tick_value * volume
    }

    /// Round a volume DOWN to the broker step. Returns 0.0 if below the minimum.
    pub fn normalize_volume(&self, volume: f64) -> f64 {
        if !volume.is_finite() || volume <= 0.0 {
            return 0.0;
        }
        let steps = ((volume + 1e-9) / self.volume_step).floor();
        let v = (steps * self.volume_step).min(self.volume_max);
        if v + 1e-9 < self.volume_min {
            0.0
        } else {
            round_to(v, 8)
        }
    }

    /// True when `volume` is a legal order volume for this instrument.
    pub fn is_volume_valid(&self, volume: f64) -> bool {
        if !volume.is_finite() || volume + 1e-9 < self.volume_min || volume > self.volume_max + 1e-9
        {
            return false;
        }
        let steps = volume / self.volume_step;
        (steps - steps.round()).abs() < 1e-6
    }

    pub fn round_price(&self, price: f64) -> f64 {
        round_to(price, self.digits)
    }

    pub fn swap_per_lot(&self, side: Side) -> f64 {
        match side {
            Side::Buy => self.swap_long,
            Side::Sell => self.swap_short,
        }
    }
}

pub fn round_to(v: f64, digits: u32) -> f64 {
    let m = 10f64.powi(digits as i32);
    (v * m).round() / m
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountTradeMode {
    Demo,
    Contest,
    Real,
}

impl AccountTradeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            AccountTradeMode::Demo => "demo",
            AccountTradeMode::Contest => "contest",
            AccountTradeMode::Real => "real",
        }
    }
}

/// How the account holds positions (MT5 `ACCOUNT_MARGIN_MODE`).
///
/// On a **netting** account there is at most one position per symbol: an opposite order
/// reduces, closes or reverses it instead of opening a separate hedge, and a same-side
/// order is merged into it (the merged position takes the new order's SL/TP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccountMarginMode {
    /// Several independent positions per symbol, both sides allowed.
    Hedging,
    /// One position per symbol (retail netting).
    Netting,
    /// Exchange netting (one position per symbol, exchange rules).
    Exchange,
    /// Not reported (connector older than protocol 1.2). Treated like netting.
    #[default]
    Unknown,
}

impl AccountMarginMode {
    pub fn as_str(self) -> &'static str {
        match self {
            AccountMarginMode::Hedging => "hedging",
            AccountMarginMode::Netting => "netting",
            AccountMarginMode::Exchange => "exchange",
            AccountMarginMode::Unknown => "unknown",
        }
    }
    /// True when several positions can coexist on one symbol (separate hedges possible).
    pub fn allows_hedging(self) -> bool {
        self == AccountMarginMode::Hedging
    }
}

/// What is on the other side of the bridge. A simulator is never presented as a broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalKind {
    Mt5,
    Simulator,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountSnapshot {
    pub ts_ms: TimestampMs,
    pub login: i64,
    pub server: String,
    pub currency: String,
    pub balance: f64,
    pub equity: f64,
    pub margin: f64,
    pub margin_free: f64,
    pub leverage: i64,
    pub trade_mode: AccountTradeMode,
    pub trade_allowed: bool,
    /// Added in 0.3 (protocol 1.2). `Unknown` for older connectors.
    #[serde(default)]
    pub margin_mode: AccountMarginMode,
}

impl AccountSnapshot {
    pub fn is_sane(&self) -> bool {
        self.balance.is_finite()
            && self.equity.is_finite()
            && self.margin.is_finite()
            && self.margin_free.is_finite()
            && self.balance >= 0.0
            && self.equity >= 0.0
            && !self.currency.is_empty()
            && self.login > 0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub ticket: i64,
    pub symbol: String,
    pub side: Side,
    pub volume: f64,
    pub price_open: f64,
    pub sl: f64,
    pub tp: f64,
    pub profit: f64,
    pub swap: f64,
    pub commission: f64,
    pub magic: i64,
    pub comment: String,
    pub open_ts: TimestampMs,
}

impl Position {
    pub fn floating(&self) -> f64 {
        self.profit + self.swap + self.commission
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderKind {
    Market,
    Limit,
    Stop,
}

impl OrderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            OrderKind::Market => "market",
            OrderKind::Limit => "limit",
            OrderKind::Stop => "stop",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "market" => Some(OrderKind::Market),
            "limit" => Some(OrderKind::Limit),
            "stop" => Some(OrderKind::Stop),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingOrder {
    pub ticket: i64,
    pub symbol: String,
    pub side: Side,
    pub kind: OrderKind,
    pub volume: f64,
    pub price: f64,
    pub sl: f64,
    pub tp: f64,
    pub magic: i64,
    pub comment: String,
    pub setup_ts: TimestampMs,
}

/// A closing deal reported by the broker (one closed trade leg).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deal {
    pub deal_ticket: i64,
    pub position_ticket: i64,
    pub symbol: String,
    /// Side of the position that was closed (not of the closing deal).
    pub side: Side,
    pub volume: f64,
    pub price: f64,
    pub profit: f64,
    pub swap: f64,
    pub commission: f64,
    /// Other broker fees on the deal (MT5 `DEAL_FEE`), negative = cost.
    #[serde(default)]
    pub fee: f64,
    pub ts_ms: TimestampMs,
    pub comment: String,
    /// Comment of the order that opened the position (the AEGIS client id for AEGIS orders).
    pub origin: String,
}

impl Deal {
    /// Result after commission, swap and fees.
    pub fn net(&self) -> f64 {
        self.profit + self.swap + self.commission + self.fee
    }
}

/// Operating mode. The engine always boots in `Monitor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Monitor,
    DemoAuto,
    LiveAuto,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Monitor => "MONITOR",
            Mode::DemoAuto => "DEMO AUTO",
            Mode::LiveAuto => "LIVE AUTO",
        }
    }
    pub fn can_execute(self) -> bool {
        !matches!(self, Mode::Monitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Regime {
    TrendUp,
    TrendDown,
    Range,
    HighVolatility,
    Unknown,
}

impl Regime {
    pub fn label(self) -> &'static str {
        match self {
            Regime::TrendUp => "Tendance haussière",
            Regime::TrendDown => "Tendance baissière",
            Regime::Range => "Range",
            Regime::HighVolatility => "Volatilité élevée",
            Regime::Unknown => "Indéterminé",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Regime::TrendUp => "trend_up",
            Regime::TrendDown => "trend_down",
            Regime::Range => "range",
            Regime::HighVolatility => "high_volatility",
            Regime::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn eurusd() -> SymbolSpec {
        SymbolSpec {
            symbol: "EURUSD".into(),
            digits: 5,
            point: 0.00001,
            contract_size: 100_000.0,
            tick_size: 0.00001,
            tick_value: 1.0,
            volume_min: 0.01,
            volume_max: 100.0,
            volume_step: 0.01,
            stops_level_points: 0.0,
            swap_long: -6.5,
            swap_short: 1.2,
            trade_mode: SymbolTradeMode::Full,
            currency_profit: "USD".into(), currency_base: "EUR".into(), freeze_level_points: 0.0,
        }
    }

    #[test]
    fn volume_normalisation_rounds_down_and_respects_bounds() {
        let s = eurusd();
        assert_eq!(s.normalize_volume(0.127), 0.12);
        assert_eq!(s.normalize_volume(0.009), 0.0);
        assert_eq!(s.normalize_volume(500.0), 100.0);
        assert_eq!(s.normalize_volume(f64::NAN), 0.0);
        assert_eq!(s.normalize_volume(-1.0), 0.0);
        assert_eq!(s.normalize_volume(0.3), 0.3);
    }

    #[test]
    fn volume_validity() {
        let s = eurusd();
        assert!(s.is_volume_valid(0.01));
        assert!(s.is_volume_valid(1.23));
        assert!(!s.is_volume_valid(0.005));
        assert!(!s.is_volume_valid(0.015));
        assert!(!s.is_volume_valid(100.01));
    }

    #[test]
    fn distance_value_uses_tick_value() {
        let s = eurusd();
        // 20 pips = 0.0020 = 200 ticks, 0.5 lot, $1/tick/lot => $100
        assert!((s.price_distance_value(0.0020, 0.5) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn spec_validation_rejects_zero_tick_value() {
        let mut s = eurusd();
        s.tick_value = 0.0;
        assert_eq!(s.validate(), Err(SpecError::Invalid("tick_value")));
    }

    #[test]
    fn timeframe_bucket() {
        assert_eq!(Timeframe::M5.bucket(301_000), 300_000);
        assert_eq!(Timeframe::M1.bucket(59_999), 0);
        assert_eq!(utc_day(86_400_000 * 3 + 5), 3);
    }

    #[test]
    fn candle_validity() {
        let c = Candle { open_ts: 0, open: 1.1, high: 1.2, low: 1.0, close: 1.15, tick_volume: 1, spread_points: 0.0 };
        assert!(c.is_valid());
        let bad = Candle { high: 1.05, ..c };
        assert!(!bad.is_valid());
    }
}
