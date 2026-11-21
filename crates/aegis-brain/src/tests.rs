use super::*;
use aegis_probability::SideScore;
use aegis_types::SymbolTradeMode;

fn eurusd() -> SymbolSpec {
    SymbolSpec {
        symbol: "EURUSD".into(),
        digits: 5,
        point: 0.00001,
        contract_size: 100_000.0,
        tick_size: 0.00001,
        tick_value: 1.0,
        volume_min: 0.01,
        volume_max: 100.0,
        volume_step: 0.01,
        stops_level_points: 10.0,
        swap_long: -6.5,
        swap_short: 1.2,
        trade_mode: SymbolTradeMode::Full,
        currency_profit: "USD".into(),
        currency_base: "EUR".into(),
        freeze_level_points: 0.0,
    }
}

/// Risk thresholds low enough for the brain's own logic to be what is tested.
fn risk() -> RiskConfig {
    RiskConfig { buy_min_probability: 0.5, sell_min_probability: 0.5, min_net_expected_profit: 0.0, ..Default::default() }
}

fn cfg() -> BrainConfig {
    BrainConfig { enabled: true, ..Default::default() }
}

/// BUY 0.01 lot at 1.10010 (spread 1 pip), stop 20 pips below, ATR 10 pips.
fn request<'a>(spec: &'a SymbolSpec, risk: &'a RiskConfig, volume: f64) -> TargetRequest<'a> {
    TargetRequest {
        spec,
        risk,
        side: Side::Buy,
        entry: 1.10010,
        sl: 1.09810,
        spread_price: 0.00010,
        risk_volume: volume,
        atr: Some(0.00100),
        strategy_tp_distance: 0.00300,
        extra_cost_per_lot: 0.0,
        haircut: 1.0,
        fixed_tp_distance: None,
        max_tp_distance: None,
    }
}

#[test]
fn the_users_tp_in_pips_is_respected() {
    let (spec, risk) = (eurusd(), self::risk());
    assert!((pip_size(&spec) - 0.0001).abs() < 1e-12);
    // Very likely below 5 pips (a 20-pip stop must be rare to pay for it), a coin flip beyond.
    let p = |d: f64| (Some(if d <= 0.000_51 { 0.97 } else { 0.5 }), "test".into());
    // Maximum 5 pips: every candidate stays within it.
    let mut r = request(&spec, &risk, 0.01);
    r.max_tp_distance = Some(5.0 * pip_size(&spec));
    let c = choose_target(&cfg(), &r, p);
    let b = c.chosen.unwrap_or_else(|| panic!("{}", c.reason));
    assert!(b.tp_distance <= 0.0005 + 1e-9, "{b:?}");
    // Fixed 4 pips: that target only (at the volume that keeps the gain in range).
    let mut r = request(&spec, &risk, 0.01);
    r.fixed_tp_distance = Some(4.0 * pip_size(&spec));
    let c = choose_target(&cfg(), &r, p);
    let b = c.chosen.unwrap_or_else(|| panic!("{}", c.reason));
    assert!((b.tp_distance - 0.0004).abs() < 1e-9 && c.tried == 1, "{b:?}");
    // A fixed TP whose gain is outside the range at every volume: no trade.
    r.fixed_tp_distance = Some(300.0 * pip_size(&spec));
    assert!(choose_target(&cfg(), &r, p).chosen.is_none());
    assert!(BrainConfig { tp_pips: 10.0, max_tp_pips: 5.0, ..cfg() }.validate().is_err());
}

/// A random-walk-like probability: the farther the target, the less likely (stop 20 pips).
fn walk(d: f64) -> (Option<f64>, String) {
    (Some(0.00200 / (0.00200 + d)), "test".into())
}

#[test]
fn chosen_target_is_inside_the_net_range_and_has_the_best_expectation() {
    let (spec, risk) = (eurusd(), risk());
    let c = choose_target(&cfg(), &request(&spec, &risk, 0.01), |d| {
        // Slight edge on small targets only.
        let (p, s) = walk(d);
        (p.map(|p| (p + if d < 0.0006 { 0.08 } else { 0.0 }).min(0.99)), s)
    });
    let best = c.chosen.clone().unwrap_or_else(|| panic!("{}", c.reason));
    assert!(best.net_if_win >= 0.08 - 1e-9 && best.net_if_win <= 2.0 + 1e-9, "{best:?}");
    assert!(best.expected_net > 0.0);
    assert!(c.in_range >= 5 && c.with_probability == c.in_range);
    // The win at 0.01 lot pays the spread ($0.10), commission ($0.07) and slippage ($0.02).
    let gross = spec.price_distance_value(best.tp_distance + 0.00010, 0.01);
    assert!((gross - best.net_if_win - 0.19).abs() < 0.011, "gross {gross}, net {}", best.net_if_win);
    assert!(c.reason.contains("espérance"), "{}", c.reason);
}

#[test]
fn without_an_edge_nothing_is_traded_and_the_reason_says_why() {
    let (spec, risk) = (eurusd(), risk());
    let c = choose_target(&cfg(), &request(&spec, &risk, 0.01), walk);
    assert!(c.chosen.is_none(), "a random walk minus costs has a negative expectation: {:?}", c.chosen);
    assert!(c.best.as_ref().unwrap().expected_net <= 0.0);
    assert!(c.reason.contains("aucune cible rentable"), "{}", c.reason);
}

#[test]
fn costs_that_eat_every_micro_gain_leave_no_possible_target() {
    let spec = eurusd();
    let risk = RiskConfig { commission_per_lot_round_turn: 900.0, ..risk() };
    // $9 of commission at 0.01 lot: targets in range exist (90+ pips away) but none pays
    // for a 20-pip stop that also costs $9 more.
    let c = choose_target(&BrainConfig { max_net_win: 0.5, ..cfg() }, &request(&spec, &risk, 0.01), walk);
    assert!(c.in_range > 0 && c.chosen.is_none(), "{}", c.reason);
    // With a target forced under the noise level, nothing is even tried.
    let c = choose_target(&BrainConfig { min_tp_atr: 5.0, max_net_win: 0.1, min_net_win: 0.08, ..cfg() }, &request(&spec, &self::risk(), 0.01), walk);
    assert_eq!(c.in_range, 0);
    assert!(c.reason.contains("aucune cible possible"), "{}", c.reason);
}

#[test]
fn a_large_risk_volume_is_reduced_to_keep_the_gain_under_the_maximum() {
    let (spec, risk) = (eurusd(), risk());
    // 5 lots: one pip is $50, far above $2. Only small volumes can stay in range.
    let c = choose_target(&cfg(), &request(&spec, &risk, 5.0), |d| (Some(if d < 0.0008 { 0.9 } else { 0.5 }), "test".into()));
    let best = c.chosen.unwrap_or_else(|| panic!("{}", c.reason));
    assert!(best.volume < 5.0, "{best:?}");
    assert!(best.net_if_win <= 2.0 + 1e-9);
    assert!(spec.is_volume_valid(best.volume));
}

#[test]
fn learned_costs_and_haircut_make_the_brain_more_prudent() {
    let (spec, risk) = (eurusd(), risk());
    let edge = |d: f64| (Some(if d < 0.0006 { 0.93 } else { 0.4 }), "test".into());
    let base = choose_target(&cfg(), &request(&spec, &risk, 0.01), edge).chosen.unwrap();
    let mut careful = request(&spec, &risk, 0.01);
    careful.extra_cost_per_lot = 5.0; // $0.05 more per trade at 0.01 lot
    careful.haircut = 0.9;
    let c = choose_target(&cfg(), &careful, edge);
    if let Some(b) = &c.chosen {
        assert!(b.expected_net < base.expected_net);
        assert!((b.probability - b.probability_raw * 0.9).abs() < 1e-12);
    }
    // A strong haircut removes every positive expectation.
    careful.haircut = 0.5;
    assert!(choose_target(&cfg(), &careful, edge).chosen.is_none());
}

#[test]
fn the_risk_thresholds_are_respected_before_choosing() {
    let spec = eurusd();
    // Small targets: very likely but tiny; far targets: better expectation but p < 75 %.
    let p = |d: f64| (Some(if d < 0.0006 { 0.92 } else { 0.70 }), "test".into());
    let loose = risk();
    let free = choose_target(&cfg(), &request(&spec, &loose, 0.01), p).chosen.unwrap();
    assert!(free.probability < 0.75, "without thresholds the far target wins: {free:?}");
    let strict = RiskConfig { buy_min_probability: 0.75, ..risk() };
    let c = choose_target(&cfg(), &request(&spec, &strict, 0.01), p);
    let b = c.chosen.unwrap_or_else(|| panic!("{}", c.reason));
    assert!(b.probability >= 0.75 && b.expected_net > 0.0, "{b:?}");
    // A minimum expectation above every candidate: nothing chosen, and the reason names the threshold.
    let picky = RiskConfig { min_net_expected_profit: 5.0, ..risk() };
    let c = choose_target(&cfg(), &request(&spec, &picky, 0.01), p);
    assert!(c.chosen.is_none() && c.best.is_some());
    assert!(c.reason.contains("seuils de risque"), "{}", c.reason);
}

#[test]
fn missing_probability_blocks_the_trade() {
    let (spec, risk) = (eurusd(), risk());
    let c = choose_target(&cfg(), &request(&spec, &risk, 0.01), |_| (None, "aucun modèle".into()));
    assert!(c.chosen.is_none() && c.with_probability == 0);
    assert!(c.reason.contains("aucun modèle"), "{}", c.reason);
}

fn trade(i: i64, strategy: &str, net: f64, p: Option<f64>, commission: f64) -> JournalEntry {
    // Every trade opens at 08:00 UTC (London session) on a different day.
    let opened = i * 86_400_000 + 8 * 3_600_000;
    JournalEntry {
        ticket: i,
        strategy_id: strategy.into(),
        symbol: "EURUSD".into(),
        side: Some(Side::Buy),
        opened_ms: opened,
        closed_ms: Some(opened + 600_000),
        volume: 0.01,
        probability: p,
        commission,
        net,
        ..Default::default()
    }
}

#[test]
fn lessons_learn_real_costs_calibration_and_losing_contexts() {
    let c = cfg();
    // Configured: $9/lot. Paid: $0.12 per 0.01 lot = $12/lot.
    let mut trades: Vec<JournalEntry> = (0..40).map(|i| trade(i, "a", if i % 4 == 0 { 0.10 } else { -0.20 }, Some(0.7), -0.12)).collect();
    trades.extend((40..45).map(|i| trade(i, "b", -0.5, Some(0.7), -0.07)));
    let l = learn(&c, "EURUSD", &trades, 9.0, 1);
    assert_eq!(l.trades, 45);
    let observed = l.cost.observed_per_lot.unwrap();
    assert!(observed > 9.0 && l.cost.extra_per_lot > 0.0 && l.cost.extra_per_lot < observed - 9.0, "shrunk: {:?}", l.cost);
    // 70 % promised, 10 wins out of 45 obtained: probabilities are cut.
    assert_eq!(l.calibration.actual_wins, 10);
    assert!(l.haircut() < 0.6 && l.haircut() > 0.2, "{:?}", l.calibration);
    let a = l.context("a", Side::Buy, "londres").unwrap();
    assert_eq!(a.trades, 40);
    assert!(a.blocked, "40 trades losing on average: {}", a.verdict);
    let b = l.context("b", Side::Buy, "londres").unwrap();
    assert!(!b.blocked, "5 trades are not enough to conclude: {}", b.verdict);
    assert!(l.context("a", Side::Sell, "londres").is_none());

    // A broker cheaper than configured never makes the brain bolder; a good model keeps its probabilities.
    let good: Vec<JournalEntry> = (0..50).map(|i| trade(i, "a", if i % 10 < 7 { 0.1 } else { -0.2 }, Some(0.7), -0.01)).collect();
    let l = learn(&c, "EURUSD", &good, 9.0, 1);
    assert_eq!(l.cost.extra_per_lot, 0.0);
    assert!((l.haircut() - 1.0).abs() < 1e-9);
    // No trade yet: neutral lessons.
    let l = learn(&c, "EURUSD", &[], 9.0, 1);
    assert_eq!((l.haircut(), l.extra_cost_per_lot(), l.contexts.len()), (1.0, 0.0, 0));
}

#[test]
fn the_haircut_is_learned_from_the_raw_probability() {
    // The brain recorded raw 0.8 but used 0.4: learning must start from 0.8.
    let trades: Vec<JournalEntry> = (0..100).map(|i| JournalEntry { probability_raw: Some(0.8), ..trade(i, "a", if i % 2 == 0 { 0.1 } else { -0.1 }, Some(0.4), 0.0) }).collect();
    let l = learn(&cfg(), "EURUSD", &trades, 9.0, 1);
    assert!((l.calibration.promised_wins - 80.0).abs() < 1e-9);
    assert!(l.haircut() < 0.7);
}

#[test]
fn sessions_cover_the_day() {
    let h = |x: i64| x * 3_600_000;
    assert_eq!(session_of(h(3)), "asie");
    assert_eq!(session_of(h(8)), "londres");
    assert_eq!(session_of(h(13)), "londres_new_york");
    assert_eq!(session_of(h(18)), "new_york");
    assert_eq!(session_of(h(22)), "fermeture");
    assert_eq!(session_of(h(24 * 400 + 8)), "londres");
}

fn score(buy: (usize, usize, Option<f64>), sell: (usize, usize, Option<f64>)) -> HoldoutScore {
    HoldoutScore {
        buy: SideScore { rows: buy.0, predicted: buy.1, brier: buy.2 },
        sell: SideScore { rows: sell.0, predicted: sell.1, brier: sell.2 },
        ..Default::default()
    }
}

#[test]
fn a_new_version_is_proposed_only_when_it_is_better_on_unseen_data() {
    let c = cfg();
    let active = score((100, 100, Some(0.200)), (100, 100, Some(0.210)));
    let better = score((100, 100, Some(0.198)), (100, 100, Some(0.208)));
    let same = score((100, 100, Some(0.2), ), (100, 100, Some(0.21)));
    let narrower = score((100, 50, Some(0.150)), (100, 100, Some(0.150)));
    assert!(candidate_is_better(&c, &better, Some(&active)).0);
    let (ok, why) = candidate_is_better(&c, &same, Some(&active));
    assert!(!ok && why.contains("pas assez"), "{why}");
    let (ok, why) = candidate_is_better(&c, &narrower, Some(&active));
    assert!(!ok && why.contains("couvre moins"), "{why}");
    let lost = score((100, 0, None), (100, 100, Some(0.1)));
    assert!(!candidate_is_better(&c, &lost, Some(&active)).0);
    // No usable active model: any version that predicts something is an improvement.
    assert!(candidate_is_better(&c, &better, None).0);
    assert!(!candidate_is_better(&c, &score((100, 0, None), (100, 0, None)), None).0);
}

#[test]
fn configuration_is_validated_and_matches_broker_variants() {
    let c = cfg();
    assert!(c.validate().is_ok());
    assert!(c.applies_to("EURUSD") && c.applies_to("EURUSDm") && c.applies_to("eurusd.sim"));
    assert!(!c.applies_to("GBPUSD") && !BrainConfig::default().applies_to("EURUSD"), "off by default");
    assert!(BrainConfig { min_net_win: 3.0, ..cfg() }.validate().is_err());
    assert!(BrainConfig { holdout_pct: 90.0, ..cfg() }.validate().is_err());
    let json = serde_json::to_string(&c).unwrap();
    assert_eq!(serde_json::from_str::<BrainConfig>(&json).unwrap(), c);
}
