//! Daily accounting.
//!
//! Convention (documented in docs/RISK.md): the accounting day is the UTC day.
//! The day's NET result is `equity_now - equity_at_day_start - net_deposits`,
//! so it contains realized P&L, floating P&L, commissions and swaps exactly as
//! the broker applies them. Deals are also summed to show the realized part.

use aegis_types::{utc_day, utc_month, Deal, Position, TimestampMs};
use serde::{Deserialize, Serialize};

/// Accounting of one UTC day. Amounts in account currency.
///
/// GROSS = the broker's profit of each closing deal (price move, spread already
/// inside), before commission, swap and fees. NET = gross + commission + swap +
/// fees (those three are negative when they are costs). Spread and slippage are
/// shown separately as ESTIMATED / MEASURED components already inside gross.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DayStats {
    pub day: i64,
    pub start_equity: f64,
    pub start_balance: f64,
    /// Sum of the raw profit of today's closed deals (before commission/swap).
    pub realized_gross: f64,
    /// Sum of the positive deal profits.
    pub gross_profit: f64,
    /// Sum of the negative deal profits (negative number).
    pub gross_loss: f64,
    pub commissions: f64,
    pub swaps: f64,
    /// Other broker fees (negative = cost).
    pub fees: f64,
    /// Spread paid on today's entries, estimated from the quote at decision time (positive = cost).
    pub spread_cost_est: f64,
    /// Slippage measured on today's fills: fill price vs. requested price (positive = cost).
    pub slippage_cost: f64,
    pub closed_trades: u32,
    pub winning_trades: u32,
    /// Entry instructions sent today.
    pub entries_sent: u32,
    /// Net deposits (+) / withdrawals (-) seen today.
    pub deposits: f64,
    /// Best realized gross / net result reached today (used by the goal policy).
    pub goal_best_gross: Option<f64>,
    pub goal_best_net: Option<f64>,
    /// Losing trades in a row (carried across days).
    pub consecutive_losses: u32,
    pub last_loss_ms: TimestampMs,
    /// Calendar month (UTC, `utc_month`) of the monthly totals below (carried across days).
    #[serde(default)]
    pub month: i64,
    /// Realized gross / net result of the month's closed deals.
    #[serde(default)]
    pub month_gross: f64,
    #[serde(default)]
    pub month_net: f64,
    /// Best realized gross / net result of the month (used by a monthly goal).
    #[serde(default)]
    pub month_best_gross: Option<f64>,
    #[serde(default)]
    pub month_best_net: Option<f64>,
}

impl Default for DayStats {
    fn default() -> Self {
        DayStats::new(0, 0.0, 0.0)
    }
}

impl DayStats {
    pub fn new(ts_ms: TimestampMs, equity: f64, balance: f64) -> Self {
        DayStats {
            day: utc_day(ts_ms),
            start_equity: equity,
            start_balance: balance,
            realized_gross: 0.0,
            gross_profit: 0.0,
            gross_loss: 0.0,
            commissions: 0.0,
            swaps: 0.0,
            fees: 0.0,
            spread_cost_est: 0.0,
            slippage_cost: 0.0,
            closed_trades: 0,
            winning_trades: 0,
            entries_sent: 0,
            deposits: 0.0,
            goal_best_gross: None,
            goal_best_net: None,
            consecutive_losses: 0,
            last_loss_ms: 0,
            month: utc_month(ts_ms),
            month_gross: 0.0,
            month_net: 0.0,
            month_best_gross: None,
            month_best_net: None,
        }
    }

    /// Start the monthly totals over when `ts_ms` is in a later month.
    fn sync_month(&mut self, ts_ms: TimestampMs) {
        let m = utc_month(ts_ms);
        if m > self.month {
            self.month = m;
            self.month_gross = 0.0;
            self.month_net = 0.0;
            self.month_best_gross = None;
            self.month_best_net = None;
        }
    }

    /// Start a new accounting day when the UTC date changed. Returns true on rollover.
    pub fn roll_if_new_day(&mut self, ts_ms: TimestampMs, equity: f64, balance: f64) -> bool {
        if utc_day(ts_ms) != self.day {
            // The losing streak is about the strategy's recent behaviour, not the calendar.
            let (streak, last_loss) = (self.consecutive_losses, self.last_loss_ms);
            let month = (self.month, self.month_gross, self.month_net, self.month_best_gross, self.month_best_net);
            *self = DayStats::new(ts_ms, equity, balance);
            self.consecutive_losses = streak;
            self.last_loss_ms = last_loss;
            // The month carries over the day change; it restarts only in a new month.
            (self.month, self.month_gross, self.month_net, self.month_best_gross, self.month_best_net) = month;
            self.sync_month(ts_ms);
            true
        } else {
            false
        }
    }

    pub fn on_deal(&mut self, d: &Deal) {
        if utc_day(d.ts_ms) != self.day {
            return;
        }
        self.sync_month(d.ts_ms);
        self.month_gross += d.profit;
        self.month_net += d.net();
        let (mg, mn) = (self.month_gross, self.month_net);
        self.month_best_gross = Some(self.month_best_gross.map_or(mg, |b| b.max(mg)));
        self.month_best_net = Some(self.month_best_net.map_or(mn, |b| b.max(mn)));
        self.realized_gross += d.profit;
        if d.profit >= 0.0 {
            self.gross_profit += d.profit;
        } else {
            self.gross_loss += d.profit;
        }
        self.commissions += d.commission;
        self.swaps += d.swap;
        self.fees += d.fee;
        self.closed_trades += 1;
        if d.net() > 0.0 {
            self.winning_trades += 1;
            self.consecutive_losses = 0;
        } else {
            self.consecutive_losses += 1;
            self.last_loss_ms = d.ts_ms;
        }
        let (g, n) = (self.realized_gross, self.realized_net());
        self.goal_best_gross = Some(self.goal_best_gross.map_or(g, |b| b.max(g)));
        self.goal_best_net = Some(self.goal_best_net.map_or(n, |b| b.max(n)));
    }

    /// An entry instruction left the engine; `spread_cost` is the spread estimated at decision time.
    pub fn on_entry_sent(&mut self, spread_cost: f64) {
        self.entries_sent += 1;
        if spread_cost.is_finite() && spread_cost > 0.0 {
            self.spread_cost_est += spread_cost;
        }
    }

    /// A fill was reported; `cost` is the measured slippage in money (negative = price improvement).
    pub fn on_slippage(&mut self, cost: f64) {
        if cost.is_finite() {
            self.slippage_cost += cost;
        }
    }

    /// All trading costs of the day as positive numbers: commission + swap cost + fees + spread + slippage.
    pub fn total_costs(&self) -> f64 {
        -(self.commissions + self.swaps + self.fees) + self.spread_cost_est + self.slippage_cost.max(0.0)
    }

    pub fn on_balance_operation(&mut self, amount: f64) {
        self.deposits += amount;
    }

    /// Realized net result of the day (profit + commissions + swaps + fees of closed deals).
    pub fn realized_net(&self) -> f64 {
        self.realized_gross + self.commissions + self.swaps + self.fees
    }

    /// Net result of the day including floating P&L.
    pub fn net_pnl(&self, equity_now: f64) -> f64 {
        equity_now - self.start_equity - self.deposits
    }

    pub fn net_pnl_pct(&self, equity_now: f64) -> f64 {
        if self.start_equity > 0.0 {
            self.net_pnl(equity_now) / self.start_equity * 100.0
        } else {
            0.0
        }
    }
}

pub fn floating_pnl(positions: &[Position]) -> f64 {
    positions.iter().map(Position::floating).sum()
}

/// Drawdown from the equity peak, in % (0 when at or above the peak).
pub fn drawdown_pct(peak_equity: f64, equity: f64) -> f64 {
    if peak_equity > 0.0 && equity < peak_equity {
        (peak_equity - equity) / peak_equity * 100.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aegis_types::Side;

    fn deal(ts: i64, profit: f64, commission: f64, swap: f64) -> Deal {
        Deal {
            deal_ticket: 1,
            position_ticket: 1,
            symbol: "EURUSD".into(),
            side: Side::Buy,
            volume: 0.1,
            price: 1.1,
            profit,
            swap,
            commission,
            fee: 0.0,
            ts_ms: ts,
            comment: String::new(),
            origin: String::new(),
        }
    }

    #[test]
    fn realized_and_net_are_separated() {
        let day0 = 86_400_000 * 100;
        let mut d = DayStats::new(day0, 10_000.0, 10_000.0);
        d.on_deal(&deal(day0 + 10, 50.0, -3.5, -1.0));
        d.on_deal(&deal(day0 + 20, -20.0, -3.5, 0.0));
        assert!((d.realized_gross - 30.0).abs() < 1e-9);
        assert!((d.realized_net() - 22.0).abs() < 1e-9);
        assert_eq!(d.closed_trades, 2);
        assert_eq!(d.winning_trades, 1);
        assert!((d.gross_profit - 50.0).abs() < 1e-9 && (d.gross_loss + 20.0).abs() < 1e-9);
        assert_eq!(d.consecutive_losses, 1);
        assert_eq!(d.goal_best_gross, Some(50.0), "best gross was reached after the first deal");
        // Equity also carries -15 of floating loss.
        assert!((d.net_pnl(10_007.0) - 7.0).abs() < 1e-9);
        assert!((d.net_pnl_pct(9_900.0) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn losing_streak_survives_the_day_change_and_resets_on_a_win() {
        let mut d = DayStats::new(0, 1_000.0, 1_000.0);
        d.on_deal(&deal(10, -5.0, 0.0, 0.0));
        d.on_deal(&deal(20, -5.0, 0.0, 0.0));
        assert_eq!(d.consecutive_losses, 2);
        assert!(d.roll_if_new_day(86_400_000, 990.0, 990.0));
        assert_eq!(d.consecutive_losses, 2);
        assert_eq!(d.last_loss_ms, 20);
        d.on_deal(&deal(86_400_000 + 5, 3.0, 0.0, 0.0));
        assert_eq!(d.consecutive_losses, 0);
    }

    #[test]
    fn costs_are_counted_once_and_as_positive_numbers() {
        let mut d = DayStats::new(0, 1_000.0, 1_000.0);
        d.on_entry_sent(0.12);
        d.on_slippage(0.03);
        let mut x = deal(10, 0.5, -0.07, -0.01);
        x.fee = -0.02;
        d.on_deal(&x);
        assert_eq!(d.entries_sent, 1);
        assert!((d.total_costs() - (0.07 + 0.01 + 0.02 + 0.12 + 0.03)).abs() < 1e-9);
        assert!((d.realized_net() - 0.40).abs() < 1e-9);
    }

    #[test]
    fn old_stored_records_still_load() {
        // Accounting saved by v0.1 lacks the new fields.
        let old = r#"{"day":3,"start_equity":1000.0,"start_balance":1000.0,"realized_gross":1.0,"commissions":0.0,"swaps":0.0,"closed_trades":1,"winning_trades":1,"deposits":0.0}"#;
        let d: DayStats = serde_json::from_str(old).unwrap();
        assert_eq!(d.day, 3);
        assert_eq!(d.entries_sent, 0);
    }

    #[test]
    fn deposits_do_not_count_as_profit() {
        let mut d = DayStats::new(0, 1_000.0, 1_000.0);
        d.on_balance_operation(500.0);
        assert!((d.net_pnl(1_500.0)).abs() < 1e-9);
    }

    #[test]
    fn rollover_and_stale_deals() {
        let mut d = DayStats::new(0, 1_000.0, 1_000.0);
        d.on_deal(&deal(86_400_000 * 2, 99.0, 0.0, 0.0)); // not today: ignored
        assert_eq!(d.closed_trades, 0);
        assert!(!d.roll_if_new_day(86_399_999, 1_010.0, 1_005.0));
        assert!(d.roll_if_new_day(86_400_000, 1_010.0, 1_005.0));
        assert_eq!(d.start_equity, 1_010.0);
        assert_eq!(d.day, 1);
    }

    #[test]
    fn drawdown() {
        assert!((drawdown_pct(10_000.0, 9_000.0) - 10.0).abs() < 1e-9);
        assert_eq!(drawdown_pct(10_000.0, 10_500.0), 0.0);
        assert_eq!(drawdown_pct(0.0, 100.0), 0.0);
    }
}
