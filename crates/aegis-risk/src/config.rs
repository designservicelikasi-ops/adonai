use crate::goal::DailyGoal;
use crate::position::PositionPolicy;
use serde::{Deserialize, Serialize};

/// What the emergency stop does to positions that are already open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KillPositionPolicy {
    /// Leave open positions with their broker-side SL/TP (default).
    Keep,
    /// Request a market close of every open position.
    Close,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RiskConfig {
    /// Maximum loss per trade if the stop-loss is hit, in % of equity.
    pub risk_per_trade_pct: f64,
    /// New entries stop when the day's net result reaches this loss, in % of day-start equity.
    pub max_daily_loss_pct: f64,
    /// The emergency stop engages when equity falls this far below its peak, in %.
    pub max_drawdown_pct: f64,
    /// Hard cap on the size of one trade, in lots (micro-trading). 0 = only the risk-based size.
    /// It can only REDUCE the size computed from `risk_per_trade_pct`, never increase it.
    pub max_lots_per_trade: f64,
    pub max_open_positions: usize,
    pub max_positions_per_symbol: usize,
    /// Total open notional (account currency) divided by equity.
    pub max_notional_leverage: f64,
    pub max_spread_points: f64,
    /// Maximum price deviation the broker may apply on a market order.
    pub max_slippage_points: f64,
    /// Minimum net expected result per trade, in account currency.
    pub min_net_expected_profit: f64,
    /// Daily profit goal in account currency (gross or net). Never a promise and never a reason to trade more.
    pub daily_goal: DailyGoal,
    /// Minimum calibrated probability that a BUY reaches its take-profit before its stop-loss.
    pub buy_min_probability: f64,
    /// Minimum calibrated probability that a SELL reaches its take-profit before its stop-loss.
    pub sell_min_probability: f64,
    /// Maximum entries sent per accounting day.
    pub max_trades_per_day: u32,
    /// Consecutive losing trades that trigger a cooldown. 0 = off.
    pub max_consecutive_losses: u32,
    /// Pause after `max_consecutive_losses`, in minutes.
    pub loss_cooldown_minutes: u32,
    /// Maximum instructions (entries) authorized per rolling minute.
    pub max_orders_per_minute: u32,
    /// Notional exposure in any single currency, divided by equity.
    pub max_currency_exposure: f64,
    /// Entries are refused when the day's trading costs exceed this share of the day's gross profit
    /// (checked after `cost_check_min_trades` closed trades). 0 = off.
    pub max_cost_to_gross_ratio: f64,
    pub cost_check_min_trades: u32,
    /// Trading windows in UTC ("HH:MM"). Empty = no time restriction.
    pub trading_windows: Vec<TradingWindow>,
    /// Position Intelligence: securing, trailing, target decay, exits.
    pub position: PositionPolicy,
    pub max_tick_age_ms: i64,
    pub max_account_age_ms: i64,
    pub authorization_ttl_ms: i64,
    /// Round-turn commission per lot in account currency (not exposed by the MT5 API).
    pub commission_per_lot_round_turn: f64,
    /// Slippage assumed when estimating costs, in points (entry + exit combined).
    pub expected_slippage_points: f64,
    /// Currency conversion fee applied to the gross result, in %.
    pub conversion_fee_pct: f64,
    /// Nights a position is expected to be held (swap estimate).
    pub expected_holding_nights: f64,
    pub kill_cancel_pending: bool,
    pub kill_position_policy: KillPositionPolicy,
}

impl Default for RiskConfig {
    fn default() -> Self {
        RiskConfig {
            risk_per_trade_pct: 0.5,
            max_daily_loss_pct: 2.0,
            max_drawdown_pct: 10.0,
            max_lots_per_trade: 0.0,
            max_open_positions: 3,
            max_positions_per_symbol: 1,
            max_notional_leverage: 5.0,
            max_spread_points: 25.0,
            max_slippage_points: 10.0,
            min_net_expected_profit: 0.50,
            daily_goal: DailyGoal::default(),
            buy_min_probability: 0.75,
            sell_min_probability: 0.75,
            max_trades_per_day: 500,
            max_consecutive_losses: 5,
            loss_cooldown_minutes: 30,
            max_orders_per_minute: 60,
            max_currency_exposure: 5.0,
            max_cost_to_gross_ratio: 0.6,
            cost_check_min_trades: 20,
            trading_windows: Vec::new(),
            position: PositionPolicy::default(),
            max_tick_age_ms: 5_000,
            max_account_age_ms: 15_000,
            authorization_ttl_ms: 3_000,
            commission_per_lot_round_turn: 7.0,
            expected_slippage_points: 2.0,
            conversion_fee_pct: 0.0,
            expected_holding_nights: 0.0,
            kill_cancel_pending: true,
            kill_position_policy: KillPositionPolicy::Keep,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
#[error("risk configuration rejected: `{field}` {problem}")]
pub struct ConfigError {
    pub field: &'static str,
    pub problem: &'static str,
}

impl RiskConfig {
    /// Hard safety bounds. A configuration outside them is refused, not clamped.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let range = |field, v: f64, lo: f64, hi: f64, problem| {
            if v.is_finite() && v >= lo && v <= hi {
                Ok(())
            } else {
                Err(ConfigError { field, problem })
            }
        };
        range("risk_per_trade_pct", self.risk_per_trade_pct, 0.01, 5.0, "must be within 0.01..5 %")?;
        range("max_daily_loss_pct", self.max_daily_loss_pct, 0.1, 20.0, "must be within 0.1..20 %")?;
        range("max_drawdown_pct", self.max_drawdown_pct, 0.5, 50.0, "must be within 0.5..50 %")?;
        range("max_notional_leverage", self.max_notional_leverage, 0.1, 100.0, "must be within 0.1..100")?;
        range("max_spread_points", self.max_spread_points, 0.0, 100_000.0, "must be >= 0")?;
        range("max_slippage_points", self.max_slippage_points, 0.0, 10_000.0, "must be >= 0")?;
        range("min_net_expected_profit", self.min_net_expected_profit, 0.0, 1e9, "must be >= 0")?;
        range("max_lots_per_trade", self.max_lots_per_trade, 0.0, 1_000.0, "must be within 0..1000 lots")?;
        range("buy_min_probability", self.buy_min_probability, 0.5, 0.99, "must be within 0.50..0.99")?;
        range("sell_min_probability", self.sell_min_probability, 0.5, 0.99, "must be within 0.50..0.99")?;
        range("max_currency_exposure", self.max_currency_exposure, 0.1, 100.0, "must be within 0.1..100")?;
        range("max_cost_to_gross_ratio", self.max_cost_to_gross_ratio, 0.0, 1.0, "must be within 0..1")?;
        if !(1..=10_000).contains(&self.max_trades_per_day) {
            return Err(ConfigError { field: "max_trades_per_day", problem: "must be within 1..10000" });
        }
        if self.max_consecutive_losses > 100 || self.loss_cooldown_minutes > 1_440 {
            return Err(ConfigError { field: "max_consecutive_losses", problem: "streak within 0..100, cooldown within 0..1440 min" });
        }
        if !(1..=600).contains(&self.max_orders_per_minute) {
            return Err(ConfigError { field: "max_orders_per_minute", problem: "must be within 1..600" });
        }
        if self.cost_check_min_trades == 0 || self.cost_check_min_trades > 10_000 {
            return Err(ConfigError { field: "cost_check_min_trades", problem: "must be within 1..10000" });
        }
        for w in &self.trading_windows {
            if w.minutes().is_none() {
                return Err(ConfigError { field: "trading_windows", problem: "each window needs start and end as HH:MM (UTC)" });
            }
        }
        self.daily_goal.validate()?;
        self.position.validate()?;
        range("commission_per_lot_round_turn", self.commission_per_lot_round_turn, 0.0, 1e6, "must be >= 0")?;
        range("expected_slippage_points", self.expected_slippage_points, 0.0, 10_000.0, "must be >= 0")?;
        range("conversion_fee_pct", self.conversion_fee_pct, 0.0, 10.0, "must be within 0..10 %")?;
        range("expected_holding_nights", self.expected_holding_nights, 0.0, 365.0, "must be within 0..365")?;
        if self.risk_per_trade_pct > self.max_daily_loss_pct {
            return Err(ConfigError { field: "risk_per_trade_pct", problem: "cannot exceed max_daily_loss_pct" });
        }
        if self.max_open_positions == 0 || self.max_open_positions > 100 {
            return Err(ConfigError { field: "max_open_positions", problem: "must be within 1..100" });
        }
        if self.max_positions_per_symbol == 0 {
            return Err(ConfigError { field: "max_positions_per_symbol", problem: "must be >= 1" });
        }
        if !(500..=600_000).contains(&self.max_tick_age_ms) {
            return Err(ConfigError { field: "max_tick_age_ms", problem: "must be within 500..600000 ms" });
        }
        if !(1_000..=600_000).contains(&self.max_account_age_ms) {
            return Err(ConfigError { field: "max_account_age_ms", problem: "must be within 1000..600000 ms" });
        }
        if !(500..=60_000).contains(&self.authorization_ttl_ms) {
            return Err(ConfigError { field: "authorization_ttl_ms", problem: "must be within 500..60000 ms" });
        }
        Ok(())
    }
}

/// A trading window in UTC. `end` before `start` wraps past midnight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TradingWindow {
    pub start: String,
    pub end: String,
}

fn hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

impl TradingWindow {
    pub fn minutes(&self) -> Option<(u32, u32)> {
        let (a, b) = (hhmm(&self.start)?, hhmm(&self.end)?);
        (a != b).then_some((a, b))
    }
    pub fn contains(&self, minute_of_day: u32) -> bool {
        match self.minutes() {
            Some((a, b)) if a < b => (a..b).contains(&minute_of_day),
            Some((a, b)) => minute_of_day >= a || minute_of_day < b,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_validate_and_bad_values_are_refused() {
        RiskConfig::default().validate().unwrap();
        let bad = [
            RiskConfig { buy_min_probability: 0.3, ..Default::default() },
            RiskConfig { sell_min_probability: 1.0, ..Default::default() },
            RiskConfig { max_trades_per_day: 0, ..Default::default() },
            RiskConfig { max_orders_per_minute: 0, ..Default::default() },
            RiskConfig { trading_windows: vec![TradingWindow { start: "25:00".into(), end: "10:00".into() }], ..Default::default() },
            RiskConfig { trading_windows: vec![TradingWindow { start: "10:00".into(), end: "10:00".into() }], ..Default::default() },
        ];
        for c in bad {
            assert!(c.validate().is_err(), "{c:?}");
        }
        // A high-volume day (2 000 entries) is a valid configuration, not an error.
        RiskConfig { max_trades_per_day: 2_000, max_orders_per_minute: 120, ..Default::default() }.validate().unwrap();
    }

    #[test]
    fn windows_including_overnight() {
        let day = TradingWindow { start: "07:00".into(), end: "20:30".into() };
        assert!(day.contains(7 * 60) && day.contains(20 * 60 + 29));
        assert!(!day.contains(20 * 60 + 30) && !day.contains(6 * 60 + 59));
        let night = TradingWindow { start: "22:00".into(), end: "02:00".into() };
        assert!(night.contains(23 * 60) && night.contains(60) && !night.contains(12 * 60));
    }
}
