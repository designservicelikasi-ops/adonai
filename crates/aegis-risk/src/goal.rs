//! Daily Profit Goal Manager.
//!
//! The goal is an amount in account currency, measured on CLOSED trades of the
//! accounting day, either GROSS (broker profit of the deals, before commission,
//! swap and fees) or NET (after them). It is a target the engine pursues, never
//! a promise: it never increases the position size, never lowers a probability
//! threshold and never creates a trade. Its only power is to stop new entries.

use crate::config::ConfigError;
use crate::day::DayStats;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalBasis {
    /// Sum of the deals' profit, before commission, swap and fees.
    Gross,
    /// Profit + commission + swap + fees.
    Net,
}

impl GoalBasis {
    pub fn label(self) -> &'static str {
        match self {
            GoalBasis::Gross => "brut",
            GoalBasis::Net => "net",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalPolicy {
    /// Once the goal is reached, no new entry for the rest of the day.
    StopEntries,
    /// Once reached, entries continue while the day's result stays above
    /// `protect_fraction` of its best level since the goal was reached.
    ProtectGains,
    /// The goal is only tracked and displayed.
    TrackOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DailyGoal {
    pub enabled: bool,
    /// Amount in account currency.
    pub amount: f64,
    pub basis: GoalBasis,
    pub policy: GoalPolicy,
    /// For `ProtectGains`: share of the best result that must be kept (0.5..1).
    pub protect_fraction: f64,
}

impl Default for DailyGoal {
    fn default() -> Self {
        DailyGoal { enabled: true, amount: 15.0, basis: GoalBasis::Net, policy: GoalPolicy::StopEntries, protect_fraction: 0.8 }
    }
}

impl DailyGoal {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !(self.amount.is_finite() && self.amount > 0.0 && self.amount <= 1e9) {
            return Err(ConfigError { field: "daily_goal.amount", problem: "must be a positive amount" });
        }
        if !(0.5..=1.0).contains(&self.protect_fraction) {
            return Err(ConfigError { field: "daily_goal.protect_fraction", problem: "must be within 0.5..1" });
        }
        Ok(())
    }
}

/// Where the day stands against its goal. Every amount is in account currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalStatus {
    pub enabled: bool,
    pub basis: GoalBasis,
    pub policy: GoalPolicy,
    pub amount: f64,
    /// Realized result of the day on the goal's basis (closed trades only).
    pub achieved: f64,
    /// Amount still to reach the goal (0 once reached).
    pub remaining: f64,
    /// 0..100+, may exceed 100.
    pub progress_pct: f64,
    pub reached: bool,
    /// Best realized result on the goal's basis since the goal was first reached.
    pub best_since_reached: Option<f64>,
    /// New entries are blocked by the goal policy.
    pub blocks_entries: bool,
    pub detail: String,
}

pub fn goal_status(goal: &DailyGoal, day: &DayStats) -> GoalStatus {
    let achieved = match goal.basis {
        GoalBasis::Gross => day.realized_gross,
        GoalBasis::Net => day.realized_net(),
    };
    let best = match goal.basis {
        GoalBasis::Gross => day.goal_best_gross,
        GoalBasis::Net => day.goal_best_net,
    };
    let reached = goal.enabled && (achieved >= goal.amount || best.is_some_and(|b| b >= goal.amount));
    let best_since_reached = if reached { Some(best.unwrap_or(achieved).max(achieved)) } else { None };
    let (blocks, detail) = if !goal.enabled {
        (false, "objectif désactivé".to_string())
    } else if !reached {
        (false, format!("{:.2} / {:.2} ({}): reste {:.2}", achieved, goal.amount, goal.basis.label(), goal.amount - achieved))
    } else {
        match goal.policy {
            GoalPolicy::StopEntries => (true, format!("objectif {} de {:.2} atteint ({achieved:.2}): nouvelles entrées arrêtées pour la journée", goal.basis.label(), goal.amount)),
            GoalPolicy::TrackOnly => (false, format!("objectif {} de {:.2} atteint ({achieved:.2}): suivi seulement", goal.basis.label(), goal.amount)),
            GoalPolicy::ProtectGains => {
                let floor = best_since_reached.unwrap_or(achieved) * goal.protect_fraction;
                if achieved < floor {
                    (true, format!("objectif atteint puis gain redescendu à {achieved:.2} sous le plancher protégé {floor:.2}: entrées arrêtées"))
                } else {
                    (false, format!("objectif atteint ({achieved:.2}); entrées permises tant que le résultat reste ≥ {floor:.2}"))
                }
            }
        }
    };
    GoalStatus {
        enabled: goal.enabled,
        basis: goal.basis,
        policy: goal.policy,
        amount: goal.amount,
        achieved,
        remaining: if goal.enabled { (goal.amount - achieved).max(0.0) } else { 0.0 },
        progress_pct: if goal.amount > 0.0 { achieved / goal.amount * 100.0 } else { 0.0 },
        reached,
        best_since_reached,
        blocks_entries: blocks,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aegis_types::{Deal, Side};

    fn deal(profit: f64, commission: f64) -> Deal {
        Deal {
            deal_ticket: 1,
            position_ticket: 1,
            symbol: "EURUSD".into(),
            side: Side::Buy,
            volume: 0.1,
            price: 1.1,
            profit,
            swap: 0.0,
            commission,
            fee: 0.0,
            ts_ms: 10,
            comment: String::new(),
            origin: String::new(),
        }
    }

    #[test]
    fn brief_example_remaining_amount() {
        // Goal 15 gross, 8.40 achieved -> 6.60 remaining.
        let goal = DailyGoal { basis: GoalBasis::Gross, ..Default::default() };
        let mut d = DayStats::new(0, 1_000.0, 1_000.0);
        d.on_deal(&deal(5.0, -0.5));
        d.on_deal(&deal(3.4, -0.5));
        let s = goal_status(&goal, &d);
        assert!((s.achieved - 8.4).abs() < 1e-9);
        assert!((s.remaining - 6.6).abs() < 1e-9);
        assert!(!s.reached && !s.blocks_entries);
        // The same day on a NET basis is further from the goal: commissions are paid.
        let net = goal_status(&DailyGoal { basis: GoalBasis::Net, ..goal.clone() }, &d);
        assert!((net.achieved - 7.4).abs() < 1e-9);
    }

    #[test]
    fn gross_goal_reached_is_not_reported_as_net() {
        let mut d = DayStats::new(0, 1_000.0, 1_000.0);
        d.on_deal(&deal(15.0, -4.0));
        let gross = goal_status(&DailyGoal { basis: GoalBasis::Gross, ..Default::default() }, &d);
        let net = goal_status(&DailyGoal { basis: GoalBasis::Net, ..Default::default() }, &d);
        assert!(gross.reached && gross.blocks_entries);
        assert!(!net.reached, "15 gross minus 4 of costs is 11 net, not 15");
        assert!((net.remaining - 4.0).abs() < 1e-9);
    }

    #[test]
    fn policies() {
        let mut d = DayStats::new(0, 1_000.0, 1_000.0);
        d.on_deal(&deal(20.0, 0.0));
        let protect = DailyGoal { policy: GoalPolicy::ProtectGains, protect_fraction: 0.8, ..Default::default() };
        assert!(!goal_status(&protect, &d).blocks_entries, "20 >= 0.8 x 20");
        d.on_deal(&deal(-5.0, 0.0)); // 15 < 16
        let s = goal_status(&protect, &d);
        assert!(s.reached && s.blocks_entries, "{s:?}");
        let track = DailyGoal { policy: GoalPolicy::TrackOnly, ..Default::default() };
        assert!(!goal_status(&track, &d).blocks_entries);
        // Once reached, the goal stays reached for the day even after a give-back.
        let stop = DailyGoal::default();
        assert!(goal_status(&stop, &d).reached && goal_status(&stop, &d).blocks_entries);
        let off = DailyGoal { enabled: false, ..Default::default() };
        let s = goal_status(&off, &d);
        assert!(!s.blocks_entries && s.remaining == 0.0);
    }

    #[test]
    fn invalid_goals() {
        assert!(DailyGoal { amount: 0.0, ..Default::default() }.validate().is_err());
        assert!(DailyGoal { amount: f64::NAN, ..Default::default() }.validate().is_err());
        assert!(DailyGoal { protect_fraction: 0.2, ..Default::default() }.validate().is_err());
    }
}
