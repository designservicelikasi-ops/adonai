//! Live monitoring of a model: feature drift (PSI) and realised performance.

use crate::model::{DriftReference, ModelArtifact};
use aegis_types::TimestampMs;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Population Stability Index between the training distribution and live values.
/// < 0.1 stable, 0.1–0.25 moderate shift, > 0.25 significant drift.
pub fn psi(reference: &DriftReference, live: &[f64]) -> Option<f64> {
    if live.is_empty() || reference.fractions.len() != reference.edges.len() + 1 {
        return None;
    }
    let mut counts = vec![0usize; reference.fractions.len()];
    for v in live {
        let bin = reference.edges.iter().take_while(|e| v > *e).count();
        counts[bin] += 1;
    }
    let eps = 1e-4;
    Some(
        counts
            .iter()
            .zip(&reference.fractions)
            .map(|(c, expected)| {
                let actual = (*c as f64 / live.len() as f64).max(eps);
                let expected = expected.max(eps);
                (actual - expected) * (actual / expected).ln()
            })
            .sum(),
    )
}

pub const PSI_DRIFT_THRESHOLD: f64 = 0.25;
const DRIFT_WINDOW: usize = 500;
const MIN_DRIFT_SAMPLES: usize = 100;
const SCORE_WINDOW: usize = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriftReport {
    pub samples: usize,
    /// (feature name, PSI) for every feature, when enough samples exist.
    pub psi: Vec<(String, f64)>,
    pub drifted: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LivePerformance {
    pub resolved: usize,
    pub pending: usize,
    /// Brier score of resolved live predictions.
    pub brier: Option<f64>,
    pub accuracy: Option<f64>,
}

struct Pending {
    bar_index: u64,
    probability: f64,
    close: f64,
}

/// Tracks one model on one instrument/timeframe.
pub struct ModelMonitor {
    horizon: u64,
    bar_index: u64,
    features: VecDeque<Vec<f64>>,
    pending: VecDeque<Pending>,
    scores: VecDeque<(f64, bool)>,
    pub last_prediction: Option<(TimestampMs, f64)>,
}

impl ModelMonitor {
    pub fn new(model: &ModelArtifact) -> Self {
        ModelMonitor {
            horizon: model.horizon_bars as u64,
            bar_index: 0,
            features: VecDeque::new(),
            pending: VecDeque::new(),
            scores: VecDeque::new(),
            last_prediction: None,
        }
    }

    /// Record the prediction made at the close of a bar and resolve older ones
    /// whose horizon has now elapsed.
    pub fn on_bar(&mut self, ts: TimestampMs, close: f64, features: &[f64], probability: f64) {
        self.bar_index += 1;
        while let Some(p) = self.pending.front() {
            if self.bar_index - p.bar_index < self.horizon {
                break;
            }
            let p = self.pending.pop_front().expect("front exists");
            let up = close > p.close;
            self.scores.push_back((p.probability, up));
            if self.scores.len() > SCORE_WINDOW {
                self.scores.pop_front();
            }
        }
        self.pending.push_back(Pending { bar_index: self.bar_index, probability, close });
        self.features.push_back(features.to_vec());
        if self.features.len() > DRIFT_WINDOW {
            self.features.pop_front();
        }
        self.last_prediction = Some((ts, probability));
    }

    pub fn performance(&self) -> LivePerformance {
        let n = self.scores.len();
        if n == 0 {
            return LivePerformance { resolved: 0, pending: self.pending.len(), brier: None, accuracy: None };
        }
        let brier = self.scores.iter().map(|(p, up)| (p - *up as u8 as f64).powi(2)).sum::<f64>() / n as f64;
        let hits = self.scores.iter().filter(|(p, up)| (*p > 0.5) == *up).count();
        LivePerformance { resolved: n, pending: self.pending.len(), brier: Some(brier), accuracy: Some(hits as f64 / n as f64) }
    }

    pub fn drift(&self, model: &ModelArtifact) -> DriftReport {
        let samples = self.features.len();
        let mut report = DriftReport { samples, psi: Vec::new(), drifted: Vec::new() };
        if samples < MIN_DRIFT_SAMPLES || model.drift_reference.len() != model.feature_names.len() {
            return report;
        }
        for (i, name) in model.feature_names.iter().enumerate() {
            let live: Vec<f64> = self.features.iter().map(|f| f[i]).collect();
            if let Some(v) = psi(&model.drift_reference[i], &live) {
                if v > PSI_DRIFT_THRESHOLD {
                    report.drifted.push(name.clone());
                }
                report.psi.push((name.clone(), v));
            }
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::artifact;

    fn uniform_reference() -> DriftReference {
        // Deciles of a uniform(0,1) training distribution.
        DriftReference { edges: (1..10).map(|i| i as f64 / 10.0).collect(), fractions: vec![0.1; 10] }
    }

    #[test]
    fn psi_is_near_zero_for_the_same_distribution_and_large_after_a_shift() {
        let r = uniform_reference();
        let same: Vec<f64> = (0..1000).map(|i| (i as f64 + 0.5) / 1000.0).collect();
        assert!(psi(&r, &same).unwrap() < 0.001);
        let shifted: Vec<f64> = (0..1000).map(|i| 0.7 + (i as f64) / 1000.0 * 0.3).collect();
        assert!(psi(&r, &shifted).unwrap() > PSI_DRIFT_THRESHOLD);
        assert!(psi(&r, &[]).is_none());
        let broken = DriftReference { edges: vec![0.5], fractions: vec![1.0] };
        assert!(psi(&broken, &same).is_none());
    }

    #[test]
    fn predictions_are_scored_only_after_their_horizon() {
        let model = artifact(); // horizon 4 bars
        let mut m = ModelMonitor::new(&model);
        let f = vec![0.0; 11];
        // Price rises every bar; the model says 0.8 (up) each time.
        for i in 0..4 {
            m.on_bar(i, 1.0 + i as f64 * 0.01, &f, 0.8);
        }
        let p = m.performance();
        assert_eq!((p.resolved, p.pending), (0, 4));
        assert!(p.brier.is_none(), "nothing can be scored before the horizon elapses");
        m.on_bar(4, 1.04, &f, 0.8);
        let p = m.performance();
        assert_eq!((p.resolved, p.pending), (1, 4));
        assert!((p.brier.unwrap() - 0.04).abs() < 1e-12, "(0.8 - 1)^2");
        assert_eq!(p.accuracy, Some(1.0));
        // Now the price falls: the same confident predictions are wrong.
        for i in 5..20 {
            m.on_bar(i, 1.04 - (i - 4) as f64 * 0.01, &f, 0.8);
        }
        let p = m.performance();
        assert!(p.accuracy.unwrap() < 0.5);
        assert!(p.brier.unwrap() > 0.3);
        assert_eq!(m.last_prediction, Some((19, 0.8)));
    }

    #[test]
    fn drift_report_flags_shifted_features() {
        let mut model = artifact();
        model.drift_reference = vec![uniform_reference(); 11];
        let mut m = ModelMonitor::new(&model);
        assert!(m.drift(&model).psi.is_empty(), "not enough samples yet");
        for i in 0..300 {
            let u = (i as f64 + 0.5) / 300.0;
            let mut f = vec![u; 11];
            f[3] = 0.95; // this feature is stuck in the top decile
            m.on_bar(i, 1.0, &f, 0.5);
        }
        let d = m.drift(&model);
        assert_eq!(d.samples, 300);
        assert_eq!(d.drifted, vec!["ema_gap_atr".to_string()]);
        assert_eq!(d.psi.len(), 11);
        // A model without a reference cannot be monitored for drift (and says so by being empty).
        model.drift_reference.clear();
        assert!(m.drift(&model).psi.is_empty());
    }
}
