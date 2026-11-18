//! Ridge logistic regression by iteratively reweighted least squares.

use crate::linalg::solve;
use crate::FittedModel;
use serde::{Deserialize, Serialize};

pub fn sigmoid(z: f64) -> f64 {
    if z >= 0.0 {
        1.0 / (1.0 + (-z).exp())
    } else {
        let e = z.exp();
        e / (1.0 + e)
    }
}

/// Per-feature mean and standard deviation, learnt on the training rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Standardizer {
    pub mean: Vec<f64>,
    pub std: Vec<f64>,
}

impl Standardizer {
    pub fn fit(x: &[Vec<f64>]) -> Standardizer {
        let d = x.first().map_or(0, Vec::len);
        let n = x.len().max(1) as f64;
        let mean: Vec<f64> = (0..d).map(|j| x.iter().map(|r| r[j]).sum::<f64>() / n).collect();
        let std = (0..d)
            .map(|j| {
                let v = x.iter().map(|r| (r[j] - mean[j]).powi(2)).sum::<f64>() / n;
                // A constant column carries no information: keep it at zero.
                if v.sqrt() > 1e-12 { v.sqrt() } else { 1.0 }
            })
            .collect();
        Standardizer { mean, std }
    }
    pub fn apply(&self, x: &[f64]) -> Vec<f64> {
        x.iter().enumerate().map(|(j, v)| (v - self.mean[j]) / self.std[j]).collect()
    }
}

/// IRLS on standardized features; the intercept is not penalized.
fn irls(z: &[Vec<f64>], y: &[f64], ridge: f64) -> Vec<f64> {
    let d = z.first().map_or(0, Vec::len) + 1;
    let mut w = vec![0.0; d];
    let mean_y = y.iter().sum::<f64>() / y.len().max(1) as f64;
    w[0] = (mean_y.clamp(1e-3, 1.0 - 1e-3) / (1.0 - mean_y.clamp(1e-3, 1.0 - 1e-3))).ln();
    for _ in 0..30 {
        let mut h = vec![vec![0.0; d]; d];
        let mut g = vec![0.0; d];
        for (row, &t) in z.iter().zip(y) {
            let mut xi = Vec::with_capacity(d);
            xi.push(1.0);
            xi.extend_from_slice(row);
            let p = sigmoid(xi.iter().zip(&w).map(|(a, b)| a * b).sum());
            let s = (p * (1.0 - p)).max(1e-9);
            for a in 0..d {
                g[a] += (t - p) * xi[a];
                let sa = s * xi[a];
                for b in a..d {
                    h[a][b] += sa * xi[b];
                }
            }
        }
        for a in 0..d {
            for b in 0..a {
                h[a][b] = h[b][a];
            }
        }
        for a in 1..d {
            h[a][a] += ridge;
            g[a] -= ridge * w[a];
        }
        let Some(step) = solve(h, g) else { break };
        let size: f64 = step.iter().map(|v| v.abs()).fold(0.0, f64::max);
        for (wi, si) in w.iter_mut().zip(&step) {
            *wi += si;
        }
        if size < 1e-8 {
            break;
        }
    }
    w
}

pub fn fit_logistic(x: &[Vec<f64>], y: &[f64], ridge: f64) -> FittedModel {
    let standardizer = Standardizer::fit(x);
    let z: Vec<Vec<f64>> = x.iter().map(|r| standardizer.apply(r)).collect();
    let weights = irls(&z, y, ridge);
    FittedModel { feature_names: Vec::new(), standardizer, weights, platt: (1.0, 0.0) }
}

/// Platt scaling: p = sigmoid(a * logit + b), fitted by unpenalized IRLS.
pub fn fit_platt(logits: &[f64], y: &[f64]) -> (f64, f64) {
    let x: Vec<Vec<f64>> = logits.iter().map(|l| vec![*l]).collect();
    let w = irls(&x, y, 0.0);
    if w.iter().all(|v| v.is_finite()) && w[1] > 0.0 {
        (w[1], w[0])
    } else {
        (1.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random numbers in [0, 1).
    pub fn lcg(seed: &mut u64) -> f64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*seed >> 11) as f64 / (1u64 << 53) as f64
    }

    #[test]
    fn recovers_known_coefficients() {
        let mut s = 7u64;
        let (mut x, mut y) = (Vec::new(), Vec::new());
        for _ in 0..20_000 {
            let a = lcg(&mut s) * 4.0 - 2.0;
            let b = lcg(&mut s) * 10.0;
            let p = sigmoid(0.5 + 1.5 * a - 0.2 * (b - 5.0));
            x.push(vec![a, b]);
            y.push(if lcg(&mut s) < p { 1.0 } else { 0.0 });
        }
        let m = fit_logistic(&x, &y, 0.0);
        // Back to the original scale.
        let wa = m.weights[1] / m.standardizer.std[0];
        let wb = m.weights[2] / m.standardizer.std[1];
        assert!((wa - 1.5).abs() < 0.1, "{wa}");
        assert!((wb + 0.2).abs() < 0.03, "{wb}");
        let p = m.predict(&[0.0, 5.0]).unwrap();
        assert!((p - sigmoid(0.5)).abs() < 0.03, "{p}");
    }

    #[test]
    fn platt_fixes_an_overconfident_score() {
        let mut s = 3u64;
        let (mut l, mut y) = (Vec::new(), Vec::new());
        for _ in 0..10_000 {
            let true_logit = lcg(&mut s) * 4.0 - 2.0;
            l.push(true_logit * 3.0); // overconfident by a factor 3
            y.push(if lcg(&mut s) < sigmoid(true_logit) { 1.0 } else { 0.0 });
        }
        let (a, b) = fit_platt(&l, &y);
        assert!((a - 1.0 / 3.0).abs() < 0.05 && b.abs() < 0.1, "a={a} b={b}");
    }

    #[test]
    fn constant_columns_and_extreme_inputs_stay_finite() {
        let x: Vec<Vec<f64>> = (0..200).map(|i| vec![1.0, i as f64]).collect();
        let y: Vec<f64> = (0..200).map(|i| if i > 100 { 1.0 } else { 0.0 }).collect();
        let m = fit_logistic(&x, &y, 1.0);
        assert!(m.weights.iter().all(|w| w.is_finite()));
        assert!(m.predict(&[1.0, 1e6]).unwrap() > 0.99);
        assert!(m.predict(&[1.0, f64::NAN]).is_none());
        assert_eq!(sigmoid(-1000.0), 0.0);
        assert_eq!(sigmoid(1000.0), 1.0);
    }
}
