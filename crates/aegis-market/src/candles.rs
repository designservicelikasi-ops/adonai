//! Tick-to-candle aggregation and bounded candle series.

use aegis_types::{Candle, Tick, Timeframe};
use std::collections::VecDeque;

/// Builds candles of one timeframe from the bid price of incoming ticks.
#[derive(Debug, Clone)]
pub struct CandleBuilder {
    tf: Timeframe,
    point: f64,
    current: Option<Candle>,
    spread_sum: f64,
}

impl CandleBuilder {
    pub fn new(tf: Timeframe, point: f64) -> Self {
        CandleBuilder { tf, point, current: None, spread_sum: 0.0 }
    }

    pub fn current(&self) -> Option<&Candle> {
        self.current.as_ref()
    }

    /// Feed a tick. Returns the bar that this tick just completed, if any.
    /// Ticks older than the bar in progress are ignored (returns `None`).
    pub fn on_tick(&mut self, tick: &Tick) -> Option<Candle> {
        let bucket = self.tf.bucket(tick.ts_ms);
        let spread_pts = if self.point > 0.0 { tick.spread() / self.point } else { 0.0 };
        match &mut self.current {
            Some(c) if bucket == c.open_ts => {
                c.high = c.high.max(tick.bid);
                c.low = c.low.min(tick.bid);
                c.close = tick.bid;
                c.tick_volume += 1;
                self.spread_sum += spread_pts;
                c.spread_points = self.spread_sum / c.tick_volume as f64;
                None
            }
            Some(c) if bucket < c.open_ts => None,
            _ => {
                let done = self.current.take();
                self.current = Some(Candle {
                    open_ts: bucket,
                    open: tick.bid,
                    high: tick.bid,
                    low: tick.bid,
                    close: tick.bid,
                    tick_volume: 1,
                    spread_points: spread_pts,
                });
                self.spread_sum = spread_pts;
                done
            }
        }
    }
}

/// Completed candles, oldest first, capped at `capacity`.
#[derive(Debug, Clone)]
pub struct CandleSeries {
    tf: Timeframe,
    capacity: usize,
    bars: VecDeque<Candle>,
}

impl CandleSeries {
    pub fn new(tf: Timeframe, capacity: usize) -> Self {
        CandleSeries { tf, capacity, bars: VecDeque::new() }
    }

    pub fn len(&self) -> usize {
        self.bars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bars.is_empty()
    }

    pub fn last(&self) -> Option<&Candle> {
        self.bars.back()
    }

    pub fn to_vec(&self) -> Vec<Candle> {
        self.bars.iter().copied().collect()
    }

    pub fn closes(&self) -> Vec<f64> {
        self.bars.iter().map(|c| c.close).collect()
    }

    /// Insert a completed bar keeping the series ordered and free of duplicates.
    /// Returns false when the bar is invalid or misaligned (it is then dropped).
    pub fn push(&mut self, c: Candle) -> bool {
        if !c.is_valid() || c.open_ts % self.tf.millis() != 0 {
            return false;
        }
        match self.bars.back() {
            None => self.bars.push_back(c),
            Some(last) if c.open_ts > last.open_ts => self.bars.push_back(c),
            Some(last) if c.open_ts == last.open_ts => *self.bars.back_mut().expect("non-empty") = c,
            Some(_) => {
                // Late (historical) bar: place it at its ordered position.
                match self.bars.binary_search_by_key(&c.open_ts, |b| b.open_ts) {
                    Ok(i) => self.bars[i] = c,
                    Err(i) => self.bars.insert(i, c),
                }
            }
        }
        while self.bars.len() > self.capacity {
            self.bars.pop_front();
        }
        true
    }

    /// Number of bars missing between the first and last bar of the series,
    /// ignoring gaps of two days or more (week-ends / market closures).
    pub fn missing_bars(&self) -> usize {
        let step = self.tf.millis();
        self.bars
            .iter()
            .zip(self.bars.iter().skip(1))
            .map(|(a, b)| b.open_ts - a.open_ts)
            .filter(|gap| *gap > step && *gap < 2 * 86_400_000)
            .map(|gap| (gap / step - 1) as usize)
            .sum()
    }
}

/// Aggregate ordered lower-timeframe bars into a higher timeframe (UTC aligned).
/// Only groups whose first source bar starts exactly on the target boundary and
/// that are followed by a later group are emitted, so partial bars are never produced.
pub fn aggregate(source: &[Candle], source_tf: Timeframe, target: Timeframe) -> Vec<Candle> {
    let mut out: Vec<Candle> = Vec::new();
    if target.millis() <= source_tf.millis() || target.millis() % source_tf.millis() != 0 {
        return out;
    }
    let mut current: Option<Candle> = None;
    let mut spread_sum = 0.0;
    let mut n = 0.0;
    for c in source {
        let bucket = target.bucket(c.open_ts);
        match &mut current {
            Some(agg) if agg.open_ts == bucket => {
                agg.high = agg.high.max(c.high);
                agg.low = agg.low.min(c.low);
                agg.close = c.close;
                agg.tick_volume += c.tick_volume;
                spread_sum += c.spread_points;
                n += 1.0;
                agg.spread_points = spread_sum / n;
            }
            _ => {
                if let Some(done) = current.take() {
                    out.push(done);
                }
                if c.open_ts == bucket {
                    current = Some(Candle { open_ts: bucket, ..*c });
                    spread_sum = c.spread_points;
                    n = 1.0;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(ts: i64, bid: f64) -> Tick {
        Tick { ts_ms: ts, bid, ask: bid + 0.0002 }
    }

    #[test]
    fn builder_aggregates_and_emits_on_new_bucket() {
        let mut b = CandleBuilder::new(Timeframe::M1, 0.0001);
        assert!(b.on_tick(&t(60_000, 1.10)).is_none());
        assert!(b.on_tick(&t(70_000, 1.12)).is_none());
        assert!(b.on_tick(&t(80_000, 1.09)).is_none());
        assert!(b.on_tick(&t(119_999, 1.11)).is_none());
        // An out-of-order tick from the previous minute is ignored.
        assert!(b.on_tick(&t(59_000, 9.99)).is_none());
        let done = b.on_tick(&t(120_000, 1.115)).expect("bar completed");
        assert_eq!(done.open_ts, 60_000);
        assert_eq!((done.open, done.high, done.low, done.close), (1.10, 1.12, 1.09, 1.11));
        assert_eq!(done.tick_volume, 4);
        assert!((done.spread_points - 2.0).abs() < 1e-6);
        assert!(done.is_valid());
        assert_eq!(b.current().unwrap().open_ts, 120_000);
    }

    fn bar(i: i64, tf: Timeframe) -> Candle {
        Candle { open_ts: i * tf.millis(), open: 1.0, high: 1.2, low: 0.9, close: 1.1, tick_volume: 5, spread_points: 10.0 }
    }

    #[test]
    fn series_is_ordered_deduplicated_and_bounded() {
        let mut s = CandleSeries::new(Timeframe::M1, 5);
        for i in [1, 2, 4] {
            assert!(s.push(bar(i, Timeframe::M1)));
        }
        assert!(s.push(bar(3, Timeframe::M1)), "late bar is inserted in order");
        assert!(s.push(Candle { close: 1.15, ..bar(4, Timeframe::M1) }), "same bar refreshes");
        assert_eq!(s.len(), 4);
        assert_eq!(s.last().unwrap().close, 1.15);
        assert!(s.to_vec().windows(2).all(|w| w[0].open_ts < w[1].open_ts));
        // Invalid and misaligned bars are refused.
        assert!(!s.push(Candle { high: 0.5, ..bar(9, Timeframe::M1) }));
        assert!(!s.push(Candle { open_ts: 61_234, ..bar(9, Timeframe::M1) }));
        for i in 5..10 {
            s.push(bar(i, Timeframe::M1));
        }
        assert_eq!(s.len(), 5);
        assert_eq!(s.to_vec()[0].open_ts, 5 * 60_000);
    }

    #[test]
    fn missing_bars_are_counted_but_weekends_ignored() {
        let mut s = CandleSeries::new(Timeframe::M1, 100);
        for i in [0, 1, 2, 5, 6] {
            s.push(bar(i, Timeframe::M1));
        }
        assert_eq!(s.missing_bars(), 2);
        s.push(bar(6 + 3 * 1440, Timeframe::M1)); // three days later
        assert_eq!(s.missing_bars(), 2);
    }

    #[test]
    fn aggregation_to_higher_timeframe() {
        // 9 M5 bars starting at a quarter-hour boundary: two complete M15 bars + 3 trailing bars
        // (the last group is not emitted because it is not known to be complete).
        let src: Vec<Candle> = (0..9)
            .map(|i| Candle {
                open_ts: i * Timeframe::M5.millis(),
                open: 1.0 + i as f64,
                high: 2.0 + i as f64,
                low: 0.5 + i as f64,
                close: 1.5 + i as f64,
                tick_volume: 10,
                spread_points: i as f64,
            })
            .collect();
        let out = aggregate(&src, Timeframe::M5, Timeframe::M15);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].open_ts, 0);
        assert_eq!((out[0].open, out[0].high, out[0].low, out[0].close), (1.0, 4.0, 0.5, 3.5));
        assert_eq!(out[0].tick_volume, 30);
        assert!((out[0].spread_points - 1.0).abs() < 1e-12);
        assert_eq!(out[1].open_ts, Timeframe::M15.millis());
        // A source starting mid-bucket does not create a partial bar.
        let out = aggregate(&src[1..], Timeframe::M5, Timeframe::M15);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].open_ts, Timeframe::M15.millis());
        assert!(aggregate(&src, Timeframe::M15, Timeframe::M5).is_empty());
    }
}
