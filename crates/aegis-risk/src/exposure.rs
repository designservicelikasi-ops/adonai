//! Global Exposure Manager: positions on correlated instruments are one exposure.
//!
//! Correlation: Pearson correlation of close-to-close returns on common bar times (at least
//! 50 common returns). Two instruments with |rho| >= threshold move together: a BUY on one and a
//! BUY on the other (rho > 0) — or a SELL (rho < 0) — add up to the same bet. The money at risk
//! of that cluster (positions' current stops + the new trade) is limited.

use aegis_types::TimestampMs;
use std::collections::HashMap;

/// Correlation of returns of two close series given as (bar time, close), oldest first.
pub fn correlation(a: &[(TimestampMs, f64)], b: &[(TimestampMs, f64)]) -> Option<(f64, usize)> {
    let bm: HashMap<TimestampMs, f64> = b.iter().copied().collect();
    let common: Vec<(f64, f64)> = a.iter().filter_map(|(t, x)| bm.get(t).map(|y| (*x, *y))).collect();
    let rets: Vec<(f64, f64)> = common.windows(2).filter(|w| w[0].0 > 0.0 && w[0].1 > 0.0).map(|w| ((w[1].0 / w[0].0).ln(), (w[1].1 / w[0].1).ln())).collect();
    let n = rets.len();
    if n < 50 {
        return None;
    }
    let (mx, my) = (rets.iter().map(|r| r.0).sum::<f64>() / n as f64, rets.iter().map(|r| r.1).sum::<f64>() / n as f64);
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (x, y) in &rets {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx).powi(2);
        syy += (y - my).powi(2);
    }
    (sxx > 0.0 && syy > 0.0).then(|| (sxy / (sxx * syy).sqrt(), n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correlation_of_returns() {
        let a: Vec<(i64, f64)> = (0..200).map(|i| (i, 1.0 + ((i * 7919) % 97) as f64 * 0.001)).collect();
        let same: Vec<(i64, f64)> = a.iter().map(|(t, x)| (*t, x * 2.0)).collect();
        let inverse: Vec<(i64, f64)> = a.iter().map(|(t, x)| (*t, 1.0 / x)).collect();
        assert!(correlation(&a, &same).unwrap().0 > 0.99);
        assert!(correlation(&a, &inverse).unwrap().0 < -0.99);
        assert!(correlation(&a[..30], &same[..30]).is_none(), "too few common bars");
    }
}
