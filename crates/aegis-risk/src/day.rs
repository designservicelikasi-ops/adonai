//! Daily accounting.
//!
//! Convention (documented in docs/RISK.md): the accounting day is the UTC day.
//! The day's NET result is `equity_now - equity_at_day_start - net_deposits`,
//! so it contains realized P&L, floating P&L, commissions and swaps exactly as
//! the broker applies them. Deals are also summed to show the realized part.

use aegis_types::{utc_day, Deal, Position, TimestampMs};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayStats {
    pub day: i64,
    pub start_equity: f64,
    pub start_balance: f64,
    /// Sum of the raw profit of today's closed deals (before commission/swap).
    pub realized_gross: f64,
    pub commissions: f64,
    pub swaps: f64,
    pub closed_trades: u32,
    pub winning_trades: u32,
    /// Net deposits (+) / withdrawals (-) seen today.
    pub deposits: f64,
}

impl DayStats {
    pub fn new(ts_ms: TimestampMs, equity: f64, balance: f64) -> Self {
        DayStats {
            day: utc_day(ts_ms),
            start_equity: equity,
            start_balance: balance,
            realized_gross: 0.0,
            commissions: 0.0,
            swaps: 0.0,
            closed_trades: 0,
            winning_trades: 0,
            deposits: 0.0,
        }
    }

    /// Start a new accounting day when the UTC date changed. Returns true on rollover.
    pub fn roll_if_new_day(&mut self, ts_ms: TimestampMs, equity: f64, balance: f64) -> bool {
        if utc_day(ts_ms) != self.day {
            *self = DayStats::new(ts_ms, equity, balance);
            true
        } else {
            false
        }
    }

    pub fn on_deal(&mut self, d: &Deal) {
        if utc_day(d.ts_ms) != self.day {
            return;
        }
        self.realized_gross += d.profit;
        self.commissions += d.commission;
        self.swaps += d.swap;
        self.closed_trades += 1;
        if d.net() > 0.0 {
            self.winning_trades += 1;
        }
    }

    pub fn on_balance_operation(&mut self, amount: f64) {
        self.deposits += amount;
    }

    /// Realized net result of the day (profit + commissions + swaps of closed deals).
    pub fn realized_net(&self) -> f64 {
        self.realized_gross + self.commissions + self.swaps
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
        // Equity also carries -15 of floating loss.
        assert!((d.net_pnl(10_007.0) - 7.0).abs() < 1e-9);
        assert!((d.net_pnl_pct(9_900.0) + 1.0).abs() < 1e-9);
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
