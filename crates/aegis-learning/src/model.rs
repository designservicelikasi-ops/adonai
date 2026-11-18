//! Model artifacts, inference and the validation gate.
//!
//! Models are trained in Python (research/train.py) and exported as a JSON
//! artifact. The gate is re-evaluated HERE from the recorded validation metrics:
//! the engine never trusts a status written in the file.

use crate::features::FEATURE_NAMES;
use aegis_types::{DataOrigin, TimestampMs};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FoldMetrics {
    pub n: usize,
    pub auc: f64,
    pub brier: f64,
    pub log_loss: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub method: String,
    pub n_samples: usize,
    pub folds: Vec<FoldMetrics>,
    pub auc_mean: f64,
    pub brier_mean: f64,
    /// Brier score of always predicting the base rate, on the same folds.
    pub baseline_brier: f64,
    /// Expected calibration error of the out-of-sample predictions.
    pub ece: f64,
    pub positive_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParityVector {
    pub features: Vec<f64>,
    pub probability: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriftReference {
    /// Interior bin edges (deciles of the training distribution).
    pub edges: Vec<f64>,
    /// Fraction of training samples in each bin (`edges.len() + 1` values).
    pub fractions: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelArtifact {
    pub model_id: String,
    pub kind: String,
    pub created_ms: TimestampMs,
    pub origin: DataOrigin,
    pub symbol: String,
    pub timeframe: String,
    pub horizon_bars: usize,
    /// What the output probability means.
    pub target: String,
    pub feature_names: Vec<String>,
    pub mean: Vec<f64>,
    pub scale: Vec<f64>,
    pub coef: Vec<f64>,
    pub intercept: f64,
    /// Platt calibration applied to the raw logit: p = sigmoid(a * z + b).
    pub calibration_a: f64,
    pub calibration_b: f64,
    pub validation: ValidationReport,
    pub parity: Vec<ParityVector>,
    pub drift_reference: Vec<DriftReference>,
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("cannot read model: {0}")]
    Io(#[from] std::io::Error),
    #[error("model file is not valid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("model is inconsistent: {0}")]
    Inconsistent(String),
}

/// Criteria a model must meet before its probabilities may influence execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GateCriteria {
    pub min_samples: usize,
    pub min_folds: usize,
    pub min_auc_mean: f64,
    pub min_auc_each_fold: f64,
    /// The model's Brier score must beat the base-rate baseline by this margin.
    pub min_brier_improvement: f64,
    pub max_ece: f64,
}

impl Default for GateCriteria {
    fn default() -> Self {
        GateCriteria { min_samples: 2_000, min_folds: 4, min_auc_mean: 0.55, min_auc_each_fold: 0.50, min_brier_improvement: 0.002, max_ece: 0.05 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateResult {
    pub passed: bool,
    pub failures: Vec<String>,
}

fn sigmoid(z: f64) -> f64 {
    if z >= 0.0 {
        1.0 / (1.0 + (-z).exp())
    } else {
        let e = z.exp();
        e / (1.0 + e)
    }
}

impl ModelArtifact {
    pub fn load(path: &Path) -> Result<ModelArtifact, ModelError> {
        let m: ModelArtifact = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        m.check_consistency()?;
        Ok(m)
    }

    /// Structural checks: dimensions, feature contract with this build, finite numbers, parity.
    pub fn check_consistency(&self) -> Result<(), ModelError> {
        let bad = |m: &str| Err(ModelError::Inconsistent(m.to_string()));
        if self.kind != "logistic_regression" {
            return bad("unsupported model kind");
        }
        if self.feature_names.iter().map(String::as_str).ne(FEATURE_NAMES.iter().copied()) {
            return bad("feature list does not match this version of AEGIS");
        }
        let n = FEATURE_NAMES.len();
        if self.mean.len() != n || self.scale.len() != n || self.coef.len() != n {
            return bad("parameter dimensions do not match the feature list");
        }
        let all = self.mean.iter().chain(&self.scale).chain(&self.coef).chain([&self.intercept, &self.calibration_a, &self.calibration_b]);
        if !all.clone().all(|v| v.is_finite()) || self.scale.iter().any(|s| *s <= 0.0) {
            return bad("non-finite parameter or non-positive scale");
        }
        if self.horizon_bars == 0 {
            return bad("horizon must be >= 1 bar");
        }
        if !self.drift_reference.is_empty() {
            if self.drift_reference.len() != n {
                return bad("drift reference does not cover every feature");
            }
            if self.drift_reference.iter().any(|d| d.fractions.len() != d.edges.len() + 1) {
                return bad("drift reference bins are malformed");
            }
        }
        if self.parity.is_empty() {
            return bad("no parity vector: inference cannot be verified");
        }
        let worst = self.parity_error();
        if !(worst <= 1e-9) {
            return Err(ModelError::Inconsistent(format!("inference differs from the training code by {worst:e}")));
        }
        Ok(())
    }

    /// Largest difference between this implementation and the probabilities the
    /// training code recorded for the same inputs.
    pub fn parity_error(&self) -> f64 {
        self.parity
            .iter()
            .map(|p| match self.predict(&p.features) {
                Some(v) => (v - p.probability).abs(),
                None => f64::INFINITY,
            })
            .fold(0.0, f64::max)
    }

    /// Raw decision value z for a feature vector.
    pub fn logit(&self, features: &[f64]) -> Option<f64> {
        if features.len() != self.coef.len() || features.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let z: f64 = features
            .iter()
            .zip(&self.mean)
            .zip(&self.scale)
            .zip(&self.coef)
            .map(|(((x, m), s), c)| (x - m) / s * c)
            .sum::<f64>()
            + self.intercept;
        Some(z)
    }

    /// Calibrated probability of the target event.
    pub fn predict(&self, features: &[f64]) -> Option<f64> {
        self.logit(features).map(|z| sigmoid(self.calibration_a * z + self.calibration_b))
    }

    /// Evaluate the validation gate from the recorded metrics.
    pub fn gate(&self, c: &GateCriteria) -> GateResult {
        let v = &self.validation;
        let mut failures = Vec::new();
        if self.origin != DataOrigin::Broker {
            failures.push("entraîné sur des données synthétiques: inutilisable pour l'exécution".to_string());
        }
        if v.n_samples < c.min_samples {
            failures.push(format!("{} échantillons (minimum {})", v.n_samples, c.min_samples));
        }
        if v.folds.len() < c.min_folds {
            failures.push(format!("{} plis de validation (minimum {})", v.folds.len(), c.min_folds));
        }
        if !(v.auc_mean >= c.min_auc_mean) {
            failures.push(format!("AUC moyenne {:.3} (minimum {:.3})", v.auc_mean, c.min_auc_mean));
        }
        if let Some(worst) = v.folds.iter().map(|f| f.auc).fold(None, |a: Option<f64>, b| Some(a.map_or(b, |x| x.min(b)))) {
            if !(worst >= c.min_auc_each_fold) {
                failures.push(format!("un pli a une AUC de {worst:.3} (minimum {:.3})", c.min_auc_each_fold));
            }
        }
        if !(v.baseline_brier - v.brier_mean >= c.min_brier_improvement) {
            failures.push(format!("score de Brier {:.4} n'améliore pas assez la référence {:.4}", v.brier_mean, v.baseline_brier));
        }
        if !(v.ece <= c.max_ece) {
            failures.push(format!("erreur de calibration {:.3} (maximum {:.3})", v.ece, c.max_ece));
        }
        GateResult { passed: failures.is_empty(), failures }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn artifact() -> ModelArtifact {
        let n = FEATURE_NAMES.len();
        let mut m = ModelArtifact {
            model_id: "test-model".into(),
            kind: "logistic_regression".into(),
            created_ms: 1,
            origin: DataOrigin::Broker,
            symbol: "EURUSD".into(),
            timeframe: "M15".into(),
            horizon_bars: 4,
            target: "P(close[t+4] > close[t])".into(),
            feature_names: FEATURE_NAMES.iter().map(|s| s.to_string()).collect(),
            mean: vec![0.0; n],
            scale: vec![1.0; n],
            coef: (0..n).map(|i| 0.1 * (i as f64 - 5.0)).collect(),
            intercept: 0.2,
            calibration_a: 0.8,
            calibration_b: -0.05,
            validation: ValidationReport {
                method: "walk-forward".into(),
                n_samples: 5_000,
                folds: (0..5).map(|_| FoldMetrics { n: 800, auc: 0.58, brier: 0.244, log_loss: 0.68 }).collect(),
                auc_mean: 0.58,
                brier_mean: 0.244,
                baseline_brier: 0.250,
                ece: 0.02,
                positive_rate: 0.5,
            },
            parity: vec![],
            drift_reference: vec![],
        };
        let x: Vec<f64> = (0..n).map(|i| i as f64 * 0.1).collect();
        let p = m.predict(&x).unwrap();
        m.parity = vec![ParityVector { features: x, probability: p }];
        m
    }

    #[test]
    fn prediction_matches_hand_computation() {
        let mut m = artifact();
        m.coef = vec![0.0; 11];
        m.coef[0] = 2.0;
        m.mean[0] = 1.0;
        m.scale[0] = 0.5;
        m.intercept = -1.0;
        m.calibration_a = 1.0;
        m.calibration_b = 0.0;
        let mut x = vec![0.0; 11];
        x[0] = 1.5; // (1.5 - 1) / 0.5 * 2 - 1 = 1
        let p = m.predict(&x).unwrap();
        assert!((p - 1.0 / (1.0 + (-1.0f64).exp())).abs() < 1e-12);
        // Platt scaling.
        m.calibration_a = 0.5;
        m.calibration_b = 0.5;
        assert!((m.predict(&x).unwrap() - 1.0 / (1.0 + (-1.0f64).exp())).abs() < 1e-12);
        assert!(m.predict(&x[..5]).is_none());
        x[3] = f64::NAN;
        assert!(m.predict(&x).is_none());
        // Extreme logits stay finite.
        m.intercept = 1e6;
        x[3] = 0.0;
        assert_eq!(m.predict(&x), Some(1.0));
    }

    #[test]
    fn consistency_checks_catch_bad_artifacts() {
        assert!(artifact().check_consistency().is_ok());
        let mut m = artifact();
        m.feature_names[2] = "something_else".into();
        assert!(m.check_consistency().is_err(), "feature contract");
        let mut m = artifact();
        m.coef.pop();
        assert!(m.check_consistency().is_err(), "dimensions");
        let mut m = artifact();
        m.scale[0] = 0.0;
        assert!(m.check_consistency().is_err(), "scale");
        let mut m = artifact();
        m.intercept = f64::NAN;
        assert!(m.check_consistency().is_err(), "nan");
        let mut m = artifact();
        m.parity[0].probability += 1e-4;
        assert!(m.check_consistency().is_err(), "parity mismatch");
        let mut m = artifact();
        m.parity.clear();
        assert!(m.check_consistency().is_err(), "parity mandatory");
        let mut m = artifact();
        m.kind = "llm".into();
        assert!(m.check_consistency().is_err());
    }

    #[test]
    fn gate_is_recomputed_from_metrics() {
        let c = GateCriteria::default();
        assert!(artifact().gate(&c).passed);

        let mut m = artifact();
        m.origin = DataOrigin::Synthetic;
        assert!(!m.gate(&c).passed, "synthetic models never pass");

        let mut m = artifact();
        m.validation.auc_mean = 0.51;
        assert!(!m.gate(&c).passed);

        let mut m = artifact();
        m.validation.folds[2].auc = 0.47;
        assert!(!m.gate(&c).passed, "one bad fold is enough");

        let mut m = artifact();
        m.validation.brier_mean = 0.2499;
        assert!(!m.gate(&c).passed, "must beat the base rate");

        let mut m = artifact();
        m.validation.ece = 0.2;
        assert!(!m.gate(&c).passed);

        let mut m = artifact();
        m.validation.n_samples = 100;
        m.validation.folds.truncate(2);
        let g = m.gate(&c);
        assert_eq!(g.failures.len(), 2);

        // NaN metrics can never pass.
        let mut m = artifact();
        m.validation.auc_mean = f64::NAN;
        m.validation.ece = f64::NAN;
        assert!(!m.gate(&c).passed);
    }

    /// The artifact committed in models/ was produced by research/train.py (Python).
    /// Loading it here proves the two implementations agree.
    #[test]
    fn python_trained_model_loads_with_matching_inference_and_is_gated() {
        // Embedded at compile time: no runtime file access (see TEST_REPORT.md, macOS note).
        let text = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../models/demo-synthetic-model.json"));
        let m: ModelArtifact = serde_json::from_str(text).expect("the demo model must parse");
        m.check_consistency().expect("the demo model must be consistent");
        assert_eq!(m.kind, "logistic_regression");
        assert!(m.parity.len() >= 5);
        assert!(m.parity_error() < 1e-12, "Rust and Python disagree by {}", m.parity_error());
        assert_eq!(m.drift_reference.len(), FEATURE_NAMES.len());
        assert!(m.validation.folds.len() >= 4 && m.validation.n_samples > 10_000);
        // Whatever its score, a model trained on synthetic data is refused.
        assert_eq!(m.origin, DataOrigin::Synthetic);
        let gate = m.gate(&GateCriteria::default());
        assert!(!gate.passed);
        assert!(gate.failures.iter().any(|f| f.contains("synthétiques")));
    }

    #[test]
    fn load_roundtrip_and_errors() {
        let dir = std::env::temp_dir().join(format!("aegis-model-{}", aegis_types::new_id("t")));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.json");
        std::fs::write(&path, serde_json::to_string(&artifact()).unwrap()).unwrap();
        assert_eq!(ModelArtifact::load(&path).unwrap(), artifact());
        std::fs::write(&path, "{not json").unwrap();
        assert!(matches!(ModelArtifact::load(&path), Err(ModelError::Json(_))));
        assert!(matches!(ModelArtifact::load(&dir.join("missing.json")), Err(ModelError::Io(_))));
    }
}
