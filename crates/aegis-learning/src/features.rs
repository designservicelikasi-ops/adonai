//! Feature engineering — the single source of truth.
//!
//! Features are computed here, in Rust, both for the training dataset exported
//! to Python and for live inference, so training and serving can never diverge.
//! Every feature uses only bars up to and including the current closed bar.

use aegis_market::indicators::{atr, donchian, efficiency_ratio, ema, rsi};
use aegis_market::regime::detect_regime;
use aegis_types::{Candle, TimestampMs};

pub const FEATURE_NAMES: [&str; 11] = [
    "ret_1", "ret_5", "ret_20", "ema_gap_atr", "rsi_14", "atr_rel", "efficiency_20", "vol_ratio",
    "channel_pos_20", "hour_sin", "hour_cos",
];

/// Bars required before the first feature vector can be computed.
pub const MIN_BARS: usize = 101;

/// Feature vector for the last bar of `bars`. `None` when history is too short
/// or an indicator is undefined.
pub fn compute(bars: &[Candle]) -> Option<[f64; 11]> {
    if bars.len() < MIN_BARS {
        return None;
    }
    let closes: Vec<f64> = bars.iter().map(|c| c.close).collect();
    let n = closes.len();
    let last = closes[n - 1];
    let a = atr(bars, 14).filter(|a| *a > 0.0)?;
    let (hi, lo) = donchian(bars, 20)?;
    let hour = (bars[n - 1].open_ts.rem_euclid(86_400_000)) as f64 / 3_600_000.0;
    let angle = hour / 24.0 * 2.0 * std::f64::consts::PI;
    let f = [
        (last / closes[n - 2]).ln(),
        (last / closes[n - 6]).ln(),
        (last / closes[n - 21]).ln(),
        (ema(&closes, 20)? - ema(&closes, 50)?) / a,
        rsi(&closes, 14)? / 100.0,
        a / last,
        efficiency_ratio(&closes, 20)?,
        detect_regime(bars).volatility_ratio,
        if hi > lo { (last - lo) / (hi - lo) } else { 0.5 },
        angle.sin(),
        angle.cos(),
    ];
    f.iter().all(|v| v.is_finite()).then_some(f)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub ts: TimestampMs,
    pub features: [f64; 11],
    /// 1 when the close `horizon` bars later is above the current close.
    pub label: u8,
    /// Forward log return over the horizon (for analysis only, never a feature).
    pub forward_return: f64,
}

/// Supervised dataset: one row per bar that has both enough history and a known future.
/// The last `horizon` bars are excluded because their label is not known yet.
pub fn build_dataset(bars: &[Candle], horizon: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    if horizon == 0 || bars.len() < MIN_BARS + horizon {
        return rows;
    }
    // A sliding window keeps the cost linear and matches what live inference sees.
    const WINDOW: usize = 300;
    for i in (MIN_BARS - 1)..(bars.len() - horizon) {
        let start = (i + 1).saturating_sub(WINDOW);
        if let Some(features) = compute(&bars[start..=i]) {
            let fwd = (bars[i + horizon].close / bars[i].close).ln();
            rows.push(Row { ts: bars[i].open_ts, features, label: (fwd > 0.0) as u8, forward_return: fwd });
        }
    }
    rows
}

pub fn dataset_to_csv(rows: &[Row]) -> String {
    let mut s = String::from("ts,");
    s.push_str(&FEATURE_NAMES.join(","));
    s.push_str(",label,forward_return\n");
    for r in rows {
        s.push_str(&r.ts.to_string());
        for v in r.features {
            s.push_str(&format!(",{v:.10}"));
        }
        s.push_str(&format!(",{},{:.10}\n", r.label, r.forward_return));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn wave(n: usize) -> Vec<Candle> {
        (0..n)
            .map(|i| {
                let c = 1.1 + 0.01 * (i as f64 / 15.0).sin() + i as f64 * 0.00001;
                let o = if i == 0 { c } else { 1.1 + 0.01 * ((i - 1) as f64 / 15.0).sin() + (i - 1) as f64 * 0.00001 };
                Candle { open_ts: i as i64 * 900_000, open: o, high: o.max(c) + 0.0003, low: o.min(c) - 0.0003, close: c, tick_volume: 10, spread_points: 10.0 }
            })
            .collect()
    }

    #[test]
    fn needs_history_and_produces_finite_bounded_values() {
        let bars = wave(400);
        assert!(compute(&bars[..MIN_BARS - 1]).is_none());
        let f = compute(&bars).unwrap();
        assert_eq!(f.len(), FEATURE_NAMES.len());
        assert!(f.iter().all(|v| v.is_finite()));
        assert!((0.0..=1.0).contains(&f[4]), "rsi scaled to 0..1");
        assert!((0.0..=1.0).contains(&f[6]), "efficiency ratio");
        assert!((0.0..=1.0).contains(&f[8]), "channel position");
        assert!((f[9].powi(2) + f[10].powi(2) - 1.0).abs() < 1e-9, "hour encoding is on the unit circle");
    }

    #[test]
    fn features_do_not_depend_on_future_bars() {
        let bars = wave(400);
        let rows = build_dataset(&bars, 4);
        // Recompute the same rows from a truncated history: identical features.
        let truncated = build_dataset(&bars[..300], 4);
        assert!(!truncated.is_empty());
        for (a, b) in truncated.iter().zip(&rows) {
            assert_eq!(a.ts, b.ts);
            assert_eq!(a.features, b.features);
        }
    }

    #[test]
    fn labels_use_the_horizon_and_last_bars_are_excluded() {
        let bars = wave(400);
        let h = 4;
        let rows = build_dataset(&bars, h);
        assert_eq!(rows.len(), 400 - (MIN_BARS - 1) - h);
        assert_eq!(rows.last().unwrap().ts, bars[400 - h - 1].open_ts);
        for r in rows.iter().take(50) {
            let i = (r.ts / 900_000) as usize;
            let up = bars[i + h].close > bars[i].close;
            assert_eq!(r.label == 1, up);
        }
        assert!(build_dataset(&bars[..50], h).is_empty());
        assert!(build_dataset(&bars, 0).is_empty());
    }

    #[test]
    fn csv_has_header_and_one_line_per_row() {
        let rows = build_dataset(&wave(200), 4);
        let csv = dataset_to_csv(&rows);
        let mut lines = csv.lines();
        assert_eq!(lines.next().unwrap(), format!("ts,{},label,forward_return", FEATURE_NAMES.join(",")));
        assert_eq!(lines.count(), rows.len());
    }
}
