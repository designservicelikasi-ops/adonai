//! Performance analytics from the trade journal (closed AEGIS trades only).
//!
//! Gross = the broker's profit of the closing deals; net = gross + commission +
//! swap + fees. Spread and slippage are inside gross already: they are reported
//! as cost components, never subtracted twice.

use aegis_data::JournalEntry;
use aegis_types::TimestampMs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const DAY: i64 = 86_400_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Stats {
    pub key: String,
    pub trades: u32,
    pub wins: u32,
    pub gross_profit: f64,
    /// Negative number.
    pub gross_loss: f64,
    pub gross: f64,
    pub net: f64,
    pub commission: f64,
    pub swap: f64,
    pub fees: f64,
    pub est_spread_cost: f64,
    pub slippage_cost: f64,
    pub win_rate: Option<f64>,
    /// Sum of net wins / |sum of net losses|; `None` without losses.
    pub profit_factor: Option<f64>,
    /// Average net result per trade.
    pub expectancy: Option<f64>,
    pub avg_win: Option<f64>,
    pub avg_loss: Option<f64>,
    /// Average cost per trade (commission + swap + fees + spread + slippage), positive.
    pub avg_cost: Option<f64>,
    /// Costs as a share of gross profit.
    pub cost_share: Option<f64>,
    /// Average share of the best price move that was kept at exit (0..1).
    pub exit_efficiency: Option<f64>,
}

impl Stats {
    fn new(key: impl Into<String>) -> Stats {
        Stats { key: key.into(), ..Default::default() }
    }

    fn add(&mut self, e: &JournalEntry) {
        self.trades += 1;
        if e.net > 0.0 {
            self.wins += 1;
        }
        if e.gross >= 0.0 {
            self.gross_profit += e.gross;
        } else {
            self.gross_loss += e.gross;
        }
        self.gross += e.gross;
        self.net += e.net;
        self.commission += e.commission;
        self.swap += e.swap;
        self.fees += e.fee;
        self.est_spread_cost += e.est_spread_cost;
        self.slippage_cost += e.slippage_cost;
    }

    fn finish(mut self, entries: &[&JournalEntry]) -> Stats {
        let n = self.trades as f64;
        if self.trades > 0 {
            self.win_rate = Some(self.wins as f64 / n);
            self.expectancy = Some(self.net / n);
            let costs = -(self.commission + self.swap + self.fees) + self.est_spread_cost + self.slippage_cost.max(0.0);
            self.avg_cost = Some(costs / n);
            self.cost_share = (self.gross_profit > 0.0).then(|| costs / self.gross_profit);
        }
        let wins: Vec<f64> = entries.iter().map(|e| e.net).filter(|v| *v > 0.0).collect();
        let losses: Vec<f64> = entries.iter().map(|e| e.net).filter(|v| *v <= 0.0).collect();
        let (sw, sl) = (wins.iter().sum::<f64>(), losses.iter().sum::<f64>());
        self.avg_win = (!wins.is_empty()).then(|| sw / wins.len() as f64);
        self.avg_loss = (!losses.is_empty()).then(|| sl / losses.len() as f64);
        self.profit_factor = (sl < 0.0).then(|| sw / -sl);
        let eff: Vec<f64> = entries
            .iter()
            .filter_map(|e| {
                let side = e.side?;
                let exit = e.exit_price?;
                (e.mfe_price > 0.0 && e.entry_filled > 0.0).then(|| ((exit - e.entry_filled) * side.sign() / e.mfe_price).clamp(-5.0, 1.0))
            })
            .collect();
        self.exit_efficiency = (!eff.is_empty()).then(|| eff.iter().sum::<f64>() / eff.len() as f64);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PerformanceReport {
    pub computed_ms: TimestampMs,
    pub all_time: Stats,
    pub today: Stats,
    pub last_7_days: Stats,
    pub last_30_days: Stats,
    /// Last 48 hours, one bucket per UTC hour ("YYYY-MM-DD HH").
    pub hourly: Vec<Stats>,
    /// One bucket per UTC day ("YYYY-MM-DD"), last 120 days.
    pub daily: Vec<Stats>,
    /// ISO-like weeks starting Monday ("YYYY-MM-DD" of the Monday).
    pub weekly: Vec<Stats>,
    /// "YYYY-MM".
    pub monthly: Vec<Stats>,
    pub by_strategy: Vec<Stats>,
    pub by_symbol: Vec<Stats>,
    /// Hour of day (UTC) at which the trade was opened, "00".."23".
    pub by_hour: Vec<Stats>,
    pub by_regime: Vec<Stats>,
    /// Cumulative net result after each closed trade.
    pub equity_curve: Vec<(TimestampMs, f64)>,
    /// Largest fall of the cumulative net curve (positive number).
    pub max_drawdown: f64,
    /// Mean / standard deviation of daily net results × sqrt(252). `None` under 10 days.
    pub daily_sharpe: Option<f64>,
    pub avg_trades_per_day: Option<f64>,
    /// Strategies whose last 7 days are clearly worse than their previous 30.
    pub degradation: Vec<String>,
}

fn civil(ts: TimestampMs) -> (i64, u32, u32, u32) {
    // Days since 1970-01-01 to (year, month, day) — Howard Hinnant's algorithm.
    let days = ts.div_euclid(DAY);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let hour = (ts.rem_euclid(DAY) / 3_600_000) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d, hour)
}

fn day_key(ts: TimestampMs) -> String {
    let (y, m, d, _) = civil(ts);
    format!("{y:04}-{m:02}-{d:02}")
}

fn group<'a>(entries: &[&'a JournalEntry], key: impl Fn(&JournalEntry) -> String) -> Vec<Stats> {
    let mut m: BTreeMap<String, Vec<&'a JournalEntry>> = BTreeMap::new();
    for e in entries {
        m.entry(key(e)).or_default().push(e);
    }
    m.into_iter()
        .map(|(k, v)| {
            let mut s = Stats::new(k);
            for e in &v {
                s.add(e);
            }
            s.finish(&v)
        })
        .collect()
}

fn stats(key: &str, entries: &[&JournalEntry]) -> Stats {
    let mut s = Stats::new(key);
    for e in entries {
        s.add(e);
    }
    s.finish(entries)
}

pub fn compute(journal: &[JournalEntry], now: TimestampMs) -> PerformanceReport {
    let mut closed: Vec<&JournalEntry> = journal.iter().filter(|e| e.closed_ms.is_some()).collect();
    closed.sort_by_key(|e| e.closed_ms);
    let at = |e: &JournalEntry| e.closed_ms.unwrap_or(0);
    let since = |ms: i64| closed.iter().copied().filter(|e| at(e) >= ms).collect::<Vec<_>>();
    let today_start = now.div_euclid(DAY) * DAY;

    let mut curve = Vec::with_capacity(closed.len());
    let (mut cum, mut peak, mut dd) = (0.0f64, 0.0f64, 0.0f64);
    for e in &closed {
        cum += e.net;
        peak = peak.max(cum);
        dd = dd.max(peak - cum);
        curve.push((at(e), cum));
    }

    let daily = group(&since(now - 120 * DAY), |e| day_key(e.closed_ms.unwrap_or(0)));
    let daily_sharpe = (daily.len() >= 10).then(|| {
        let x: Vec<f64> = daily.iter().map(|d| d.net).collect();
        let mean = x.iter().sum::<f64>() / x.len() as f64;
        let sd = (x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (x.len() as f64 - 1.0)).sqrt();
        if sd > 0.0 { mean / sd * 252f64.sqrt() } else { 0.0 }
    });
    let active_days = group(&closed, |e| day_key(e.closed_ms.unwrap_or(0))).len();

    let by_strategy = group(&closed, |e| if e.strategy_id.is_empty() { "?".into() } else { e.strategy_id.clone() });
    let mut degradation = Vec::new();
    for s in &by_strategy {
        let recent: Vec<&JournalEntry> = closed.iter().copied().filter(|e| e.strategy_id == s.key && at(e) >= now - 7 * DAY).collect();
        let before: Vec<&JournalEntry> = closed.iter().copied().filter(|e| e.strategy_id == s.key && at(e) < now - 7 * DAY && at(e) >= now - 37 * DAY).collect();
        if recent.len() >= 20 && before.len() >= 20 {
            let (r, b) = (stats("", &recent), stats("", &before));
            if let (Some(re), Some(be)) = (r.expectancy, b.expectancy) {
                if be > 0.0 && re < 0.0 {
                    degradation.push(format!("{}: espérance nette {re:+.3} sur 7 jours contre {be:+.3} sur les 30 jours précédents", s.key));
                }
            }
        }
    }

    PerformanceReport {
        computed_ms: now,
        all_time: stats("total", &closed),
        today: stats("aujourd'hui", &since(today_start)),
        last_7_days: stats("7 jours", &since(now - 7 * DAY)),
        last_30_days: stats("30 jours", &since(now - 30 * DAY)),
        hourly: group(&since(now - 2 * DAY), |e| {
            let (y, m, d, h) = civil(e.closed_ms.unwrap_or(0));
            format!("{y:04}-{m:02}-{d:02} {h:02}")
        }),
        daily,
        weekly: group(&closed, |e| {
            let ts = e.closed_ms.unwrap_or(0);
            // 1970-01-01 was a Thursday: shift so weeks start on Monday.
            let monday = (ts.div_euclid(DAY) + 3).div_euclid(7) * 7 - 3;
            day_key(monday * DAY)
        }),
        monthly: group(&closed, |e| {
            let (y, m, _, _) = civil(e.closed_ms.unwrap_or(0));
            format!("{y:04}-{m:02}")
        }),
        by_strategy,
        by_symbol: group(&closed, |e| e.symbol.clone()),
        by_hour: group(&closed, |e| format!("{:02}", civil(e.opened_ms).3)),
        by_regime: group(&closed, |e| if e.regime.is_empty() { "?".into() } else { e.regime.clone() }),
        equity_curve: curve,
        max_drawdown: dd,
        daily_sharpe,
        avg_trades_per_day: (active_days > 0).then(|| closed.len() as f64 / active_days as f64),
        degradation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aegis_types::Side;

    fn trade(closed: i64, gross: f64, commission: f64, strategy: &str) -> JournalEntry {
        JournalEntry {
            ticket: closed,
            strategy_id: strategy.into(),
            symbol: "EURUSD".into(),
            side: Some(Side::Buy),
            regime: "range".into(),
            opened_ms: closed - 60_000,
            closed_ms: Some(closed),
            volume: 0.1,
            entry_filled: 1.1000,
            exit_price: Some(1.1000 + gross / 10_000.0),
            gross,
            commission,
            net: gross + commission,
            est_spread_cost: 0.01,
            mfe_price: 0.0010,
            ..Default::default()
        }
    }

    #[test]
    fn calendar_keys() {
        assert_eq!(day_key(0), "1970-01-01");
        // 2026-11-18 12:34 UTC
        let ts = 1_795_005_240_000;
        assert_eq!(civil(ts), (2026, 11, 18, 12));
        assert_eq!(day_key(951_782_400_000), "2000-02-29");
    }

    #[test]
    fn gross_net_costs_and_ratios() {
        let now = 1_795_005_240_000;
        let j = vec![trade(now - 3_000, 0.50, -0.07, "a"), trade(now - 2_000, -0.30, -0.07, "a"), trade(now - 1_000, 0.20, -0.07, "b"), JournalEntry { closed_ms: None, ..trade(now, 9.0, 0.0, "a") }];
        let r = compute(&j, now);
        let t = &r.all_time;
        assert_eq!(t.trades, 3, "open trades are not counted");
        assert!((t.gross - 0.40).abs() < 1e-12 && (t.net - 0.19).abs() < 1e-12);
        assert!((t.gross_profit - 0.70).abs() < 1e-12 && (t.gross_loss + 0.30).abs() < 1e-12);
        assert!((t.commission + 0.21).abs() < 1e-12);
        assert_eq!(t.wins, 2);
        // Net wins 0.43 + 0.13, net loss -0.37.
        assert!((t.profit_factor.unwrap() - 0.56 / 0.37).abs() < 1e-9);
        assert!((t.expectancy.unwrap() - 0.19 / 3.0).abs() < 1e-12);
        assert!((t.avg_cost.unwrap() - (0.21 + 0.03) / 3.0).abs() < 1e-12);
        assert!(t.exit_efficiency.is_some());
        assert_eq!(r.by_strategy.len(), 2);
        assert_eq!(r.equity_curve.len(), 3);
        assert!((r.max_drawdown - 0.37).abs() < 1e-12);
        assert_eq!(r.today.trades, 3);
        assert_eq!(r.monthly[0].key, "2026-11");
        assert_eq!(r.weekly[0].key, "2026-11-16", "the Monday of the week");
        assert!(r.daily_sharpe.is_none(), "not enough days");
    }

    #[test]
    fn a_good_win_rate_can_still_lose_money() {
        let now = 10 * DAY;
        let mut j: Vec<JournalEntry> = (0..9).map(|i| trade(now - 100 + i, 0.10, -0.02, "s")).collect();
        j.push(trade(now - 1, -2.0, -0.02, "s"));
        let t = compute(&j, now).all_time;
        assert!(t.win_rate.unwrap() >= 0.9);
        assert!(t.net < 0.0 && t.profit_factor.unwrap() < 1.0);
    }

    #[test]
    fn degradation_is_flagged() {
        let now = 60 * DAY;
        let mut j = Vec::new();
        for i in 0..25 {
            j.push(trade(now - 20 * DAY + i, 0.30, -0.05, "s"));
            j.push(trade(now - DAY + i, -0.30, -0.05, "s"));
        }
        let r = compute(&j, now);
        assert_eq!(r.degradation.len(), 1, "{:?}", r.degradation);
    }
}
