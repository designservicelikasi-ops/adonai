use aegis_strategy::*;
use aegis_types::{Candle, Side};
use serde_json::{json, Value};

/// Candles from a close-price path; each bar spans its move plus a small wick.
fn bars(closes: &[f64]) -> Vec<Candle> {
    closes
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let o = if i == 0 { c } else { closes[i - 1] };
            Candle {
                open_ts: i as i64 * 900_000,
                open: o,
                high: o.max(c) + 0.0002,
                low: o.min(c) - 0.0002,
                close: c,
                tick_volume: 10,
                spread_points: 10.0,
            }
        })
        .collect()
}

/// Run a strategy bar by bar (flat, no position) and collect the entries.
fn entries(s: &dyn Strategy, b: &[Candle]) -> Vec<(usize, SignalIntent)> {
    let mut out = Vec::new();
    for i in 1..=b.len() {
        if let StrategyAction::Enter(sig) = s.evaluate(window(&b[..i]), None) {
            out.push((i - 1, sig));
        }
    }
    out
}

fn down_then_up() -> Vec<f64> {
    let mut v: Vec<f64> = (0..100).map(|i| 1.2000 - i as f64 * 0.0005).collect();
    v.extend((1..=80).map(|i| 1.1505 + i as f64 * 0.0008));
    v
}

#[test]
fn ema_trend_enters_once_on_the_cross_and_exits_on_the_reverse_cross() {
    let s = build("ema_trend", &Value::Null).unwrap();
    let b = bars(&down_then_up());
    let e = entries(s.as_ref(), &b);
    assert_eq!(e.len(), 1, "exactly one cross in a V-shaped path: {e:?}");
    let (idx, sig) = &e[0];
    assert!(*idx > 100, "the cross happens after the turn");
    assert_eq!(sig.side, Side::Buy);
    // Stops are ATR multiples (2x and 3x by default).
    assert!((sig.tp_distance / sig.sl_distance - 1.5).abs() < 1e-9);
    assert!(sig.sl_distance > 0.0);
    assert!(sig.reason.contains("EMA20"));
    assert!(sig.evidence.iter().any(|(k, _)| k == "atr"));

    // Holding a short when the fast EMA crosses up => exit; holding a long => stay.
    let at_cross = window(&b[..=*idx]);
    let short = OpenView { side: Side::Sell, entry_price: 1.16, bars_held: 5 };
    assert!(matches!(s.evaluate(at_cross, Some(&short)), StrategyAction::Exit { .. }));
    let long = OpenView { side: Side::Buy, entry_price: 1.16, bars_held: 5 };
    assert_eq!(s.evaluate(at_cross, Some(&long)), StrategyAction::None);
    // While a position is open the strategy never proposes a second entry.
    for i in 60..b.len() {
        assert!(!matches!(s.evaluate(window(&b[..i]), Some(&long)), StrategyAction::Enter(_)));
    }
}

#[test]
fn ema_trend_sells_on_a_down_cross() {
    let s = build("ema_trend", &Value::Null).unwrap();
    let mirrored: Vec<f64> = down_then_up().iter().map(|c| 2.4 - c).collect();
    let e = entries(s.as_ref(), &bars(&mirrored));
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].1.side, Side::Sell);
}

#[test]
fn rsi_reversion_buys_when_rsi_leaves_the_oversold_zone() {
    let s = build("rsi_reversion", &Value::Null).unwrap();
    // Sideways noise, a sharp sell-off, then a bounce.
    let mut v: Vec<f64> = (0..40).map(|i| 1.1000 + if i % 2 == 0 { 0.0004 } else { -0.0004 }).collect();
    v.extend((1..=12).map(|i| 1.0996 - i as f64 * 0.0010));
    v.extend((1..=6).map(|i| 1.0876 + i as f64 * 0.0012));
    let b = bars(&v);
    let e = entries(s.as_ref(), &b);
    assert_eq!(e.len(), 1, "{e:?}");
    assert_eq!(e[0].1.side, Side::Buy);
    assert!(e[0].0 >= 52, "entry comes with the bounce, not during the fall");
    assert!((e[0].1.tp_distance - e[0].1.sl_distance).abs() < 1e-12);

    // A long is closed once the RSI is back above 50.
    let long = OpenView { side: Side::Buy, entry_price: 1.09, bars_held: 3 };
    let rally: Vec<f64> = v.iter().copied().chain((1..=15).map(|i| 1.0948 + i as f64 * 0.0015)).collect();
    assert!(matches!(s.evaluate(window(&bars(&rally)), Some(&long)), StrategyAction::Exit { .. }));
}

#[test]
fn donchian_breakout_fires_on_a_close_outside_the_previous_channel() {
    let s = build("donchian_breakout", &Value::Null).unwrap();
    let mut v: Vec<f64> = (0..60).map(|i| 1.1000 + if i % 2 == 0 { 0.0003 } else { -0.0003 }).collect();
    let b = bars(&v);
    assert!(entries(s.as_ref(), &b).is_empty(), "no breakout inside the channel");
    v.push(1.1030);
    let b = bars(&v);
    let e = entries(s.as_ref(), &b);
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].0, 60);
    assert_eq!(e[0].1.side, Side::Buy);
    assert!((e[0].1.tp_distance / e[0].1.sl_distance - 2.0).abs() < 1e-9);
    // Downside break.
    v.pop();
    v.push(1.0960);
    let e = entries(s.as_ref(), &bars(&v));
    assert_eq!(e[0].1.side, Side::Sell);
    // Falling back to the middle of the channel closes a long.
    let long = OpenView { side: Side::Buy, entry_price: 1.1030, bars_held: 2 };
    assert!(matches!(s.evaluate(window(&bars(&v)), Some(&long)), StrategyAction::Exit { .. }));
}

#[test]
fn strategies_stay_silent_without_enough_history() {
    for s in default_strategies() {
        let short = bars(&down_then_up()[..s.meta().min_bars - 1]);
        assert_eq!(s.evaluate(&short, None), StrategyAction::None, "{}", s.meta().id);
        assert_eq!(s.evaluate(&[], None), StrategyAction::None);
    }
}

#[test]
fn decisions_depend_only_on_past_bars() {
    // Appending future bars must not change what was decided at an earlier bar.
    let full = bars(&down_then_up());
    for s in default_strategies() {
        for cut in [70, 110, 130, 150] {
            let then = s.evaluate(window(&full[..cut]), None);
            let mut extended = full[..cut].to_vec();
            extended.extend_from_slice(&full[cut..]);
            assert_eq!(s.evaluate(window(&extended[..cut]), None), then, "{} @ {cut}", s.meta().id);
        }
    }
}

#[test]
fn registry_metadata_and_configuration_validation() {
    let all = default_strategies();
    assert_eq!(all.len(), 3);
    let mut ids: Vec<&str> = all.iter().map(|s| s.meta().id.as_str()).collect();
    ids.dedup();
    assert_eq!(ids, STRATEGY_IDS.to_vec(), "ids are unique and stable");
    for s in &all {
        let m = s.meta();
        assert!(!m.entry_rule.is_empty() && !m.exit_rule.is_empty() && !m.stop_rule.is_empty() && !m.target_rule.is_empty());
        assert!(!m.timeframes.is_empty() && !m.regimes.is_empty());
        assert!(m.supports_symbol("EURUSD"));
        // The reported configuration rebuilds the same strategy.
        assert!(build(&m.id, &s.config()).is_ok());
    }
    assert_eq!(build("martingale", &Value::Null).err(), Some(StrategyError::Unknown("martingale".into())));
    assert!(build("ema_trend", &json!({"fast": 50, "slow": 20})).is_err());
    assert!(build("ema_trend", &json!({"sl_atr": 0.0})).is_err());
    assert!(build("ema_trend", &json!({"typo": 1})).is_err());
    assert!(build("rsi_reversion", &json!({"oversold": 60.0})).is_err());
    assert!(build("donchian_breakout", &json!({"period": 1})).is_err());
    let custom = build("ema_trend", &json!({"fast": 10, "slow": 30})).unwrap();
    assert_eq!(custom.config()["fast"], 10);
}
