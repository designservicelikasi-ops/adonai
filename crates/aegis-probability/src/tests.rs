use super::*;

const POINT: f64 = 0.00001;

fn c(i: usize, o: f64, h: f64, l: f64, cl: f64) -> Candle {
    Candle { open_ts: i as i64 * 60_000, open: o, high: h, low: l, close: cl, tick_volume: 10, spread_points: 10.0 }
}

fn lcg(seed: &mut u64) -> f64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (*seed >> 11) as f64 / (1u64 << 53) as f64
}

fn normal(seed: &mut u64) -> f64 {
    let (u, v) = (lcg(seed).max(1e-12), lcg(seed));
    (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
}

/// Bars whose returns follow r_t = phi * r_{t-1} + noise (phi = 0: random walk).
fn series(n: usize, phi: f64, seed: u64) -> Vec<Candle> {
    let mut s = seed;
    let (mut p, mut r) = (1.1, 0.0);
    (0..n)
        .map(|i| {
            r = phi * r + 0.0004 * normal(&mut s);
            let o = p;
            p += r;
            let wick = 0.0001 * lcg(&mut s);
            c(i, o, o.max(p) + wick, o.min(p) - wick, p)
        })
        .collect()
}

fn small_cfg() -> ProbabilityConfig {
    ProbabilityConfig { tp_grid: vec![0.5, 1.0], sl_grid: vec![0.5, 1.0], horizon_bars: 20, stride: 3, folds: 4, min_samples: 500, ..Default::default() }
}

#[test]
fn triple_barrier_is_pessimistic_and_respects_the_spread() {
    // Close 1.1000; spread 10 points (0.0001) -> a BUY enters at 1.1001.
    let mut bars = vec![c(0, 1.1, 1.1, 1.1, 1.1000)];
    bars.push(c(1, 1.1000, 1.1005, 1.0999, 1.1004)); // high 1.1005 < 1.1001 + 0.0005
    bars.push(c(2, 1.1004, 1.1006, 1.1003, 1.1005)); // high 1.1006 = TP
    bars.push(c(3, 1.1005, 1.1005, 1.1005, 1.1005));
    assert_eq!(triple_barrier(&bars, 0, Side::Buy, 0.0005, 0.0010, 0.0001, 3), Some(true));
    // Same path for a SELL (enters at the bid 1.1000): the ask high 1.1006 + 0.0001 reaches SL 1.1005.
    assert_eq!(triple_barrier(&bars, 0, Side::Sell, 0.0010, 0.0005, 0.0001, 3), Some(false));
    // TP and SL inside the same bar: order unknown -> counted as a loss.
    let both = vec![c(0, 1.1, 1.1, 1.1, 1.1), c(1, 1.1, 1.1020, 1.0980, 1.1), c(2, 1.1, 1.1, 1.1, 1.1)];
    assert_eq!(triple_barrier(&both, 0, Side::Buy, 0.0005, 0.0005, 0.0, 2), Some(false));
    // Neither reached within the horizon -> failure; unknown future -> None.
    let flat = vec![c(0, 1.1, 1.1, 1.1, 1.1); 5];
    assert_eq!(triple_barrier(&flat, 0, Side::Buy, 0.001, 0.001, 0.0, 3), Some(false));
    assert_eq!(triple_barrier(&flat, 3, Side::Buy, 0.001, 0.001, 0.0, 3), None);
}

#[test]
fn random_walk_gets_a_calibrated_scenario_probability_and_no_market_edge() {
    let bars = series(3_000, 0.0, 11);
    let m = train("p-test", "TEST", "M1", DataOrigin::Synthetic, &bars, POINT, &small_cfg(), 0).unwrap();
    for side in [&m.buy, &m.sell] {
        assert_ne!(side.selected, Selected::Full, "no market feature can predict a random walk: {}", side.selection_reason);
        assert!(side.full_report.n >= 500);
        // Out-of-sample metrics are real numbers, not placeholders.
        assert!(side.baseline_report.brier > 0.05 && side.baseline_report.brier < 0.3);
        assert!(side.baseline_report.ece.is_finite());
        // 4 folds, the first one (trained on 20 % of the history) is not evaluated.
        assert_eq!(side.full_report.fold_brier.len(), 3);
    }
    // Wider target than stop on a random walk: well below 50 %.
    if m.buy.selected == Selected::ScenarioOnly {
        let p = m.estimate(&bars[..2_000], Side::Buy, 0.0, 0.0);
        assert!(p.probability.is_none(), "zero distances are outside the grid");
        let a = aegis_market::indicators::atr(&bars[1_700..2_000], 14).unwrap();
        let wide = m.estimate(&bars[1_700..2_000], Side::Buy, 1.0 * a, 0.5 * a).probability.unwrap();
        let tight = m.estimate(&bars[1_700..2_000], Side::Buy, 0.5 * a, 1.0 * a).probability.unwrap();
        assert!(wide < 0.5 && tight > wide, "wide {wide} tight {tight}");
    }
    assert_eq!(m.dataset_hash, dataset_hash(&bars));
    assert_ne!(m.dataset_hash, dataset_hash(&bars[1..]), "the dataset version changes with the data");
}

#[test]
fn planted_momentum_is_found_out_of_sample() {
    let bars = series(3_000, 0.6, 5);
    let m = train("p-test", "TEST", "M1", DataOrigin::Synthetic, &bars, POINT, &small_cfg(), 0).unwrap();
    assert_eq!(m.buy.selected, Selected::Full, "{}", m.buy.selection_reason);
    assert!(m.buy.full_report.brier < m.buy.baseline_report.brier);
    assert!(m.buy.full_report.ece <= 0.05, "calibrated: {}", m.buy.full_report.ece);
}

#[test]
fn training_is_reproducible_and_refuses_bad_inputs() {
    let bars = series(1_500, 0.0, 2);
    let cfg = ProbabilityConfig { min_samples: 100, ..small_cfg() };
    let a = train("x", "TEST", "M1", DataOrigin::Synthetic, &bars, POINT, &cfg, 0).unwrap();
    let b = train("x", "TEST", "M1", DataOrigin::Synthetic, &bars, POINT, &cfg, 0).unwrap();
    assert_eq!(a, b, "same data, same configuration, same model");
    assert!(matches!(train("x", "T", "M1", DataOrigin::Synthetic, &bars[..200], POINT, &cfg, 0), Err(TrainError::NotEnoughData(200))));
    assert!(matches!(train("x", "T", "M1", DataOrigin::Synthetic, &bars, 0.0, &cfg, 0), Err(TrainError::Config(_))));
    let bad = ProbabilityConfig { tp_grid: vec![], ..cfg };
    assert!(bad.validate().is_err());
    // Serialization round trip (stored in the database as JSON).
    let back: ProbabilityModel = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
    assert_eq!(back.buy.selected, a.buy.selected);
}
