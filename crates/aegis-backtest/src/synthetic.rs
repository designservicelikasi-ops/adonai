//! Deterministic SYNTHETIC candle generator.
//!
//! Its only purpose is to exercise the software (tests, demonstrations with the
//! simulator). Synthetic candles are always stored with `DataOrigin::Synthetic`
//! and can never justify real execution.

use aegis_types::{Candle, Timeframe, TimestampMs};

/// Small deterministic generator (SplitMix64) so results do not depend on any
/// external crate version.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Standard normal (Box–Muller).
    pub fn normal(&mut self) -> f64 {
        let u1 = self.uniform().max(1e-12);
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyntheticParams {
    pub seed: u64,
    pub bars: usize,
    pub timeframe: Timeframe,
    pub start_ts: TimestampMs,
    pub start_price: f64,
    /// Standard deviation of one bar's return.
    pub bar_volatility: f64,
    pub point: f64,
    pub spread_points: f64,
}

impl Default for SyntheticParams {
    fn default() -> Self {
        SyntheticParams {
            seed: 42,
            bars: 6_000,
            timeframe: Timeframe::M15,
            start_ts: 1_704_067_200_000, // 2024-01-01 00:00 UTC
            start_price: 1.1000,
            bar_volatility: 0.0006,
            point: 0.00001,
            spread_points: 12.0,
        }
    }
}

/// Regime-switching random walk: alternating trending, ranging and volatile phases.
pub fn generate(p: &SyntheticParams) -> Vec<Candle> {
    let mut rng = Rng::new(p.seed);
    let mut out = Vec::with_capacity(p.bars);
    let mut price = p.start_price;
    let anchor = p.start_price;
    let (mut drift, mut vol_mult, mut left) = (0.0, 1.0, 0usize);
    let start = p.timeframe.bucket(p.start_ts);
    for i in 0..p.bars {
        if left == 0 {
            left = 80 + (rng.uniform() * 320.0) as usize;
            let kind = rng.uniform();
            (drift, vol_mult) = if kind < 0.3 {
                (p.bar_volatility * 0.25, 1.0)
            } else if kind < 0.6 {
                (-p.bar_volatility * 0.25, 1.0)
            } else if kind < 0.9 {
                (0.0, 0.7)
            } else {
                (0.0, 2.2)
            };
        }
        left -= 1;
        let open = price;
        // Mild pull towards the anchor keeps the series in a realistic band.
        let pull = (anchor - price) / anchor * 0.002;
        let mut hi = open;
        let mut lo = open;
        for _ in 0..4 {
            price *= 1.0 + drift / 4.0 + pull / 4.0 + rng.normal() * p.bar_volatility * vol_mult / 2.0;
            hi = hi.max(price);
            lo = lo.min(price);
        }
        let round = |v: f64| (v / p.point).round() * p.point;
        let (open, close) = (round(open), round(price));
        out.push(Candle {
            open_ts: start + i as i64 * p.timeframe.millis(),
            open,
            high: round(hi).max(open).max(close),
            low: round(lo).min(open).min(close),
            close,
            tick_volume: 40 + (rng.uniform() * 200.0) as i64,
            spread_points: p.spread_points * (0.8 + rng.uniform() * 0.6),
        });
        price = close;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic_and_valid() {
        let p = SyntheticParams { bars: 2_000, ..Default::default() };
        let a = generate(&p);
        let b = generate(&p);
        assert_eq!(a, b, "same seed, same series");
        assert_eq!(a.len(), 2_000);
        assert!(a.iter().all(Candle::is_valid));
        assert!(a.windows(2).all(|w| w[1].open_ts - w[0].open_ts == Timeframe::M15.millis()));
        assert!(a.iter().all(|c| c.open_ts % Timeframe::M15.millis() == 0));
        let other = generate(&SyntheticParams { seed: 7, bars: 2_000, ..Default::default() });
        assert_ne!(a, other);
        // Prices stay in a plausible band.
        assert!(a.iter().all(|c| c.low > 0.8 && c.high < 1.5));
    }

    #[test]
    fn rng_is_roughly_standard_normal() {
        let mut r = Rng::new(1);
        let xs: Vec<f64> = (0..20_000).map(|_| r.normal()).collect();
        let mean = xs.iter().sum::<f64>() / xs.len() as f64;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64;
        assert!(mean.abs() < 0.03, "mean {mean}");
        assert!((var - 1.0).abs() < 0.05, "var {var}");
    }
}
