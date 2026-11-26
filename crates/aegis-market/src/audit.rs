//! Data quality audit of a stored candle history (Fast Data Pipeline).
//!
//! - duplicates / out of order: same or decreasing open time;
//! - invalid: OHLC inconsistent (`Candle::is_valid`);
//! - gaps: missing bars, NOT counting the weekly FX closure (Friday 22:00 → Sunday 22:00 UTC);
//! - outliers: a true range or a close-to-close jump above 8 x the median true range;
//! - verdict: `good` (< 0.5 % issues), `usable` (< 3 %), `poor` otherwise. A poor history must
//!   not be used to train or validate a model.

use crate::indicators::{median, true_range};
use crate::regime::{session_at, Session};
use aegis_types::{Candle, Timeframe, TimestampMs};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DataAudit {
    pub bars: usize,
    pub from_ts: TimestampMs,
    pub to_ts: TimestampMs,
    pub duplicates: usize,
    pub out_of_order: usize,
    pub invalid: usize,
    pub missing_bars: usize,
    /// Largest gaps: (start of the gap, missing bars).
    pub gaps: Vec<(TimestampMs, usize)>,
    pub outliers: Vec<(TimestampMs, String)>,
    pub issue_share: f64,
    pub verdict: String,
}

fn market_closed(ts: TimestampMs) -> bool {
    session_at(ts) == Session::Closed
}

pub fn audit(candles: &[Candle], tf: Timeframe) -> DataAudit {
    let mut a = DataAudit { bars: candles.len(), from_ts: candles.first().map(|c| c.open_ts).unwrap_or(0), to_ts: candles.last().map(|c| c.open_ts).unwrap_or(0), ..Default::default() };
    if candles.is_empty() {
        a.verdict = "aucune donnée".into();
        return a;
    }
    let step = tf.millis();
    let trs: Vec<f64> = (1..candles.len()).map(|i| true_range(candles[i - 1].close, &candles[i])).collect();
    let med = median(&trs).unwrap_or(0.0);
    for i in 0..candles.len() {
        let c = &candles[i];
        if !c.is_valid() {
            a.invalid += 1;
        }
        if i == 0 {
            continue;
        }
        let p = &candles[i - 1];
        if c.open_ts == p.open_ts {
            a.duplicates += 1;
            continue;
        }
        if c.open_ts < p.open_ts {
            a.out_of_order += 1;
            continue;
        }
        let missing: usize = ((p.open_ts + step)..c.open_ts).step_by(step as usize).filter(|t| !market_closed(*t)).count();
        if missing > 0 {
            a.missing_bars += missing;
            a.gaps.push((p.open_ts + step, missing));
        }
        if med > 0.0 {
            let tr = true_range(p.close, c);
            let jump = (c.close - p.close).abs();
            if tr > 8.0 * med || jump > 8.0 * med {
                a.outliers.push((c.open_ts, format!("variation de {:.1} x la normale", tr.max(jump) / med)));
            }
        }
    }
    a.gaps.sort_by(|x, y| y.1.cmp(&x.1));
    a.gaps.truncate(20);
    let issues = a.duplicates + a.out_of_order + a.invalid + a.missing_bars + a.outliers.len();
    a.issue_share = issues as f64 / (a.bars + a.missing_bars).max(1) as f64;
    a.verdict = if a.issue_share < 0.005 { "good" } else if a.issue_share < 0.03 { "usable" } else { "poor" }.into();
    a.outliers.truncate(50);
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    // Monday 2027-01-18 00:00 UTC.
    const MON: i64 = 1_800_230_400_000;

    fn bar(ts: i64, close: f64) -> Candle {
        Candle { open_ts: ts, open: close, high: close + 0.0002, low: close - 0.0002, close, tick_volume: 1, spread_points: 0.0 }
    }

    #[test]
    fn gaps_duplicates_outliers_and_the_weekend_are_told_apart() {
        let h = Timeframe::H1.millis();
        let mut v: Vec<Candle> = (0..100).map(|i| bar(MON + i * h, 1.1 + (i % 3) as f64 * 0.0001)).collect();
        assert_eq!(audit(&v, Timeframe::H1).verdict, "good");
        v.remove(50); // one missing hour
        v.insert(10, v[10]); // a duplicate
        let spike = v[70].open_ts;
        v[70] = bar(spike, 1.1500); // a spike
        let r = audit(&v, Timeframe::H1);
        assert_eq!((r.missing_bars, r.duplicates), (1, 1));
        assert!(r.outliers.iter().any(|(t, _)| *t == spike));
        // Friday 21:00 then Sunday 22:00: the closure is not a gap.
        let fri = MON + 4 * 86_400_000 + 21 * h;
        let sun = MON + 6 * 86_400_000 + 22 * h;
        let w = vec![bar(fri, 1.1), bar(sun, 1.1)];
        assert_eq!(audit(&w, Timeframe::H1).missing_bars, 0);
    }
}
