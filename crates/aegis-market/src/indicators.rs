//! Technical indicators. Every function only looks at the slice it is given
//! (bars up to and including the current one), so there is no look-ahead.

use aegis_types::Candle;

/// Exponential moving average over the whole slice, seeded with the SMA of the
/// first `period` values. `None` when there are fewer than `period` values.
pub fn ema(values: &[f64], period: usize) -> Option<f64> {
    ema_series(values, period).last().copied().flatten()
}

/// EMA value for every index (`None` until `period` values are available).
pub fn ema_series(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 || values.len() < period {
        return out;
    }
    let k = 2.0 / (period as f64 + 1.0);
    let mut e = values[..period].iter().sum::<f64>() / period as f64;
    out[period - 1] = Some(e);
    for i in period..values.len() {
        e = values[i] * k + e * (1.0 - k);
        out[i] = Some(e);
    }
    out
}

pub fn sma(values: &[f64], period: usize) -> Option<f64> {
    if period == 0 || values.len() < period {
        return None;
    }
    Some(values[values.len() - period..].iter().sum::<f64>() / period as f64)
}

/// Wilder's RSI. `None` until `period + 1` closes are available.
pub fn rsi(closes: &[f64], period: usize) -> Option<f64> {
    if period == 0 || closes.len() < period + 1 {
        return None;
    }
    let (mut gain, mut loss) = (0.0, 0.0);
    for i in 1..=period {
        let d = closes[i] - closes[i - 1];
        if d >= 0.0 { gain += d } else { loss -= d }
    }
    gain /= period as f64;
    loss /= period as f64;
    for i in period + 1..closes.len() {
        let d = closes[i] - closes[i - 1];
        let (g, l) = if d >= 0.0 { (d, 0.0) } else { (0.0, -d) };
        gain = (gain * (period as f64 - 1.0) + g) / period as f64;
        loss = (loss * (period as f64 - 1.0) + l) / period as f64;
    }
    if loss == 0.0 {
        return Some(if gain == 0.0 { 50.0 } else { 100.0 });
    }
    Some(100.0 - 100.0 / (1.0 + gain / loss))
}

pub fn true_range(prev_close: f64, c: &Candle) -> f64 {
    (c.high - c.low).max((c.high - prev_close).abs()).max((c.low - prev_close).abs())
}

/// Wilder's ATR. `None` until `period + 1` candles are available.
pub fn atr(candles: &[Candle], period: usize) -> Option<f64> {
    if period == 0 || candles.len() < period + 1 {
        return None;
    }
    let trs: Vec<f64> = (1..candles.len()).map(|i| true_range(candles[i - 1].close, &candles[i])).collect();
    let mut a = trs[..period].iter().sum::<f64>() / period as f64;
    for tr in &trs[period..] {
        a = (a * (period as f64 - 1.0) + tr) / period as f64;
    }
    Some(a)
}

/// Highest high and lowest low of the last `period` candles.
pub fn donchian(candles: &[Candle], period: usize) -> Option<(f64, f64)> {
    if period == 0 || candles.len() < period {
        return None;
    }
    let w = &candles[candles.len() - period..];
    Some((
        w.iter().map(|c| c.high).fold(f64::MIN, f64::max),
        w.iter().map(|c| c.low).fold(f64::MAX, f64::min),
    ))
}

/// Kaufman efficiency ratio over the last `period` moves: 1 = straight line, 0 = noise.
pub fn efficiency_ratio(closes: &[f64], period: usize) -> Option<f64> {
    if period == 0 || closes.len() < period + 1 {
        return None;
    }
    let w = &closes[closes.len() - period - 1..];
    let net = (w[w.len() - 1] - w[0]).abs();
    let path: f64 = w.windows(2).map(|p| (p[1] - p[0]).abs()).sum();
    Some(if path > 0.0 { net / path } else { 0.0 })
}

/// Standard deviation of log returns over the last `period` moves (per bar).
pub fn realized_volatility(closes: &[f64], period: usize) -> Option<f64> {
    if period < 2 || closes.len() < period + 1 {
        return None;
    }
    let w = &closes[closes.len() - period - 1..];
    let r: Vec<f64> = w.windows(2).map(|p| (p[1] / p[0]).ln()).collect();
    let mean = r.iter().sum::<f64>() / r.len() as f64;
    let var = r.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (r.len() as f64 - 1.0);
    Some(var.sqrt())
}

/// Population standard deviation of the last `period` values.
pub fn stddev(values: &[f64], period: usize) -> Option<f64> {
    if period < 2 || values.len() < period {
        return None;
    }
    let w = &values[values.len() - period..];
    let mean = w.iter().sum::<f64>() / period as f64;
    Some((w.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / period as f64).sqrt())
}

/// Bollinger bands of the last `period` closes: (middle, upper, lower).
pub fn bollinger(closes: &[f64], period: usize, k: f64) -> Option<(f64, f64, f64)> {
    let mid = sma(closes, period)?;
    let sd = stddev(closes, period)?;
    Some((mid, mid + k * sd, mid - k * sd))
}

/// Swing levels: a high (low) that is the extreme of `strength` bars on each side.
/// Only pivots fully confirmed inside the slice are returned (no look-ahead),
/// most recent first: (resistances, supports).
pub fn swing_levels(candles: &[Candle], strength: usize, max_levels: usize) -> (Vec<f64>, Vec<f64>) {
    let (mut res, mut sup) = (Vec::new(), Vec::new());
    if strength == 0 || candles.len() < 2 * strength + 1 {
        return (res, sup);
    }
    for i in (strength..candles.len() - strength).rev() {
        let w = &candles[i - strength..=i + strength];
        let c = &candles[i];
        if res.len() < max_levels && w.iter().all(|x| x.high <= c.high) {
            res.push(c.high);
        }
        if sup.len() < max_levels && w.iter().all(|x| x.low >= c.low) {
            sup.push(c.low);
        }
        if res.len() >= max_levels && sup.len() >= max_levels {
            break;
        }
    }
    (res, sup)
}

pub fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(o: f64, h: f64, l: f64, cl: f64) -> Candle {
        Candle { open_ts: 0, open: o, high: h, low: l, close: cl, tick_volume: 1, spread_points: 0.0 }
    }

    #[test]
    fn ema_matches_hand_computation() {
        // period 3, k = 0.5: seed = (1+2+3)/3 = 2 ; then 4 -> 3 ; 5 -> 4
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(ema_series(&v, 3), vec![None, None, Some(2.0), Some(3.0), Some(4.0)]);
        assert_eq!(ema(&v, 3), Some(4.0));
        assert_eq!(ema(&v, 6), None);
        assert_eq!(ema(&v, 0), None);
    }

    #[test]
    fn rsi_extremes_and_reference_value() {
        let up: Vec<f64> = (0..20).map(|i| 1.0 + i as f64 * 0.01).collect();
        assert_eq!(rsi(&up, 14), Some(100.0));
        let down: Vec<f64> = (0..20).map(|i| 2.0 - i as f64 * 0.01).collect();
        assert!(rsi(&down, 14).unwrap() < 1e-9);
        let flat = [1.0; 20];
        assert_eq!(rsi(&flat, 14), Some(50.0));
        assert_eq!(rsi(&up[..14], 14), None);
        // Classic textbook series (Wilder): expected RSI(14) ≈ 70.46 after the first 15 closes.
        let closes = [
            44.34, 44.09, 44.15, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84, 46.08, 45.89, 46.03, 45.61,
            46.28, 46.28,
        ];
        let r = rsi(&closes, 14).unwrap();
        assert!((r - 70.46).abs() < 0.1, "rsi = {r}");
    }

    #[test]
    fn atr_and_true_range() {
        assert_eq!(true_range(10.0, &c(10.0, 12.0, 9.0, 11.0)), 3.0);
        assert_eq!(true_range(15.0, &c(10.0, 12.0, 9.0, 11.0)), 6.0); // gap down
        let bars = vec![c(10.0, 11.0, 9.0, 10.0); 5];
        assert!((atr(&bars, 3).unwrap() - 2.0).abs() < 1e-12);
        assert_eq!(atr(&bars[..3], 3), None);
    }

    #[test]
    fn bollinger_and_swings() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        let (m, u, l) = bollinger(&v, 5, 2.0).unwrap();
        assert!((m - 3.0).abs() < 1e-12);
        assert!((u - (3.0 + 2.0 * 2f64.sqrt())).abs() < 1e-12 && (l - (3.0 - 2.0 * 2f64.sqrt())).abs() < 1e-12);
        assert_eq!(bollinger(&v, 6, 2.0), None);
        // A peak at index 2 and a trough at index 5, each with 2 lower/higher bars on both sides.
        let hl = [(1.0, 0.5), (2.0, 1.5), (3.0, 2.5), (2.0, 1.5), (1.5, 1.0), (1.2, 0.2), (1.6, 0.9), (1.8, 1.1)];
        let bars: Vec<Candle> = hl.iter().map(|(h, l)| c((h + l) / 2.0, *h, *l, (h + l) / 2.0)).collect();
        let (res, sup) = swing_levels(&bars, 2, 3);
        assert_eq!(res, vec![3.0]);
        assert_eq!(sup, vec![0.2]);
        // The last `strength` bars can never confirm a pivot.
        let (res, _) = swing_levels(&bars[..4], 2, 3);
        assert!(res.is_empty());
    }

    #[test]
    fn donchian_efficiency_volatility_median() {
        let bars = vec![c(1.0, 5.0, 0.5, 2.0), c(2.0, 3.0, 1.0, 2.5), c(2.5, 4.0, 2.0, 3.0)];
        assert_eq!(donchian(&bars, 2), Some((4.0, 1.0)));
        assert_eq!(donchian(&bars, 4), None);
        let straight = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(efficiency_ratio(&straight, 3), Some(1.0));
        let zigzag = [1.0, 2.0, 1.0, 2.0, 1.0];
        assert_eq!(efficiency_ratio(&zigzag, 4), Some(0.0));
        assert_eq!(realized_volatility(&[1.0, 1.0, 1.0, 1.0], 3), Some(0.0));
        assert!(realized_volatility(&[1.0, 1.1, 0.9, 1.2], 3).unwrap() > 0.0);
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(median(&[]), None);
    }
}
