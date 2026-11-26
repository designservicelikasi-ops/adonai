//! AEGIS RISK — centralised risk management, cost model and order authorization.

mod auth;
mod config;
mod costs;
mod day;
mod engine;
pub mod exposure;
mod goal;
mod position;
mod protection;
pub mod recovery;
mod stops;

pub use auth::{AuthError, Authorization};
pub use config::{utc_weekday, weekday_index, ConfigError, KillPositionPolicy, MissingStopPolicy, RiskConfig, TradingWindow};
pub use position::{manage as manage_position, ManagedPosition, PositionAction, PositionActionKind, PositionPolicy};
pub use stops::{plan_stop, StopCandidate, StopMode, StopPlan, StopPolicy, StopRole};
pub use protection::{ExpectedLevels, ProtectionAction, ProtectionGuard, ProtectionIncident, MAX_RESTORE_ATTEMPTS, MODIFY_VERIFY_MS};
pub use goal::{goal_status, DailyGoal, GoalBasis, GoalPeriod, GoalPolicy, GoalStatus};
pub use costs::{estimate_costs, evaluate_micro_profit, target_feasibility, CostBreakdown, MicroProfitEvaluation, TargetFeasibility};
pub use day::{drawdown_pct, floating_pnl, DayStats};
pub use engine::{
    currencies_of, per_lot_loss, size_volume, size_volume_scaled, Check, KillPlan, KillSwitchState, RiskAssessment, RiskContext, RiskEngine, TradeProposal,
    KILL_RESET_PHRASE,
};

#[cfg(test)]
pub(crate) mod testutil {
    use aegis_types::{SymbolSpec, SymbolTradeMode};

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
            stops_level_points: 10.0,
            swap_long: -6.5,
            swap_short: 1.2,
            trade_mode: SymbolTradeMode::Full,
            currency_profit: "USD".into(), currency_base: "EUR".into(), freeze_level_points: 0.0,
        }
    }
}
