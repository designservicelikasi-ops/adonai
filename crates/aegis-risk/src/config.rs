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
    pub max_open_positions: usize,
    pub max_positions_per_symbol: usize,
    /// Total open notional (account currency) divided by equity.
    pub max_notional_leverage: f64,
    pub max_spread_points: f64,
    /// Maximum price deviation the broker may apply on a market order.
    pub max_slippage_points: f64,
    /// Minimum net expected result per trade, in account currency.
    pub min_net_expected_profit: f64,
    /// New entries stop when the day's net result reaches this gain, in % of day-start equity. 0 = off.
    /// This is a stop condition, never a promise and never a reason to trade more.
    pub daily_profit_target_pct: f64,
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
            max_open_positions: 3,
            max_positions_per_symbol: 1,
            max_notional_leverage: 5.0,
            max_spread_points: 25.0,
            max_slippage_points: 10.0,
            min_net_expected_profit: 0.50,
            daily_profit_target_pct: 1.0,
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
        range("daily_profit_target_pct", self.daily_profit_target_pct, 0.0, 100.0, "must be within 0..100 %")?;
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
