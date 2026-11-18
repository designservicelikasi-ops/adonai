//! Probability quality: Brier score, log loss, reliability diagram, ECE.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ReliabilityBin {
    pub lower: f64,
    pub upper: f64,
    pub count: usize,
    pub mean_predicted: f64,
    pub observed_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CalibrationReport {
    pub n: usize,
    pub brier: f64,
    pub log_loss: f64,
    /// Expected calibration error over 10 equal-width bins.
    pub ece: f64,
    pub mean_predicted: f64,
    pub observed_rate: f64,
    pub bins: Vec<ReliabilityBin>,
    /// Brier score of each walk-forward fold, in order.
    pub fold_brier: Vec<f64>,
}

pub fn brier(preds: &[(f64, f64)]) -> f64 {
    if preds.is_empty() {
        return f64::NAN;
    }
    preds.iter().map(|(p, y)| (p - y).powi(2)).sum::<f64>() / preds.len() as f64
}

pub fn log_loss(preds: &[(f64, f64)]) -> f64 {
    if preds.is_empty() {
        return f64::NAN;
    }
    preds
        .iter()
        .map(|(p, y)| {
            let p = p.clamp(1e-12, 1.0 - 1e-12);
            -(y * p.ln() + (1.0 - y) * (1.0 - p).ln())
        })
        .sum::<f64>()
        / preds.len() as f64
}

/// `preds` are (predicted probability, outcome 0/1); `folds` gives each prediction's fold.
pub fn reliability(preds: &[(f64, f64)], folds: &[usize]) -> CalibrationReport {
    let n = preds.len();
    let mut bins: Vec<ReliabilityBin> = (0..10).map(|b| ReliabilityBin { lower: b as f64 / 10.0, upper: (b + 1) as f64 / 10.0, ..Default::default() }).collect();
    for (p, y) in preds {
        let b = ((p * 10.0).floor() as usize).min(9);
        bins[b].count += 1;
        bins[b].mean_predicted += p;
        bins[b].observed_rate += y;
    }
    let mut ece = 0.0;
    for b in &mut bins {
        if b.count > 0 {
            b.mean_predicted /= b.count as f64;
            b.observed_rate /= b.count as f64;
            ece += b.count as f64 / n.max(1) as f64 * (b.mean_predicted - b.observed_rate).abs();
        }
    }
    let mut fold_ids: Vec<usize> = folds.to_vec();
    fold_ids.sort_unstable();
    fold_ids.dedup();
    let fold_brier = fold_ids
        .iter()
        .map(|f| brier(&preds.iter().zip(folds).filter(|(_, g)| *g == f).map(|(p, _)| *p).collect::<Vec<_>>()))
        .collect();
    CalibrationReport {
        n,
        brier: brier(preds),
        log_loss: log_loss(preds),
        ece: if n > 0 { ece } else { f64::NAN },
        mean_predicted: preds.iter().map(|x| x.0).sum::<f64>() / n.max(1) as f64,
        observed_rate: preds.iter().map(|x| x.1).sum::<f64>() / n.max(1) as f64,
        bins,
        fold_brier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_values() {
        let p = [(0.8, 1.0), (0.2, 0.0), (0.6, 0.0)];
        assert!((brier(&p) - (0.04 + 0.04 + 0.36) / 3.0).abs() < 1e-12);
        let ll = -((0.8f64).ln() + (0.8f64).ln() + (0.4f64).ln()) / 3.0;
        assert!((log_loss(&p) - ll).abs() < 1e-12);
        let r = reliability(&p, &[0, 0, 1]);
        assert_eq!(r.n, 3);
        assert_eq!(r.fold_brier.len(), 2);
        assert_eq!(r.bins[8].count, 1);
        // ECE: bins 2 (0.2 vs 0), 6 (0.6 vs 0), 8 (0.8 vs 1) -> (0.2 + 0.6 + 0.2) / 3
        assert!((r.ece - 1.0 / 3.0).abs() < 1e-12, "{}", r.ece);
        assert!(brier(&[]).is_nan());
    }

    #[test]
    fn a_perfectly_calibrated_forecaster_has_low_ece() {
        let mut preds = Vec::new();
        for b in 0..10 {
            let p = b as f64 / 10.0 + 0.05;
            for k in 0..1000 {
                preds.push((p, if (k as f64) < p * 1000.0 { 1.0 } else { 0.0 }));
            }
        }
        assert!(reliability(&preds, &vec![0; preds.len()]).ece < 0.001);
    }
}
