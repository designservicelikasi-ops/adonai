use super::lab::Lab;
use super::*;

fn spec() -> SymbolSpec {
    SymbolSpec {
        symbol: "EURUSD.sim".into(),
        digits: 5,
        point: 0.00001,
        contract_size: 100_000.0,
        tick_size: 0.00001,
        tick_value: 1.0,
        volume_min: 0.01,
        volume_max: 100.0,
        volume_step: 0.01,
        stops_level_points: 0.0,
        swap_long: 0.0,
        swap_short: 0.0,
        trade_mode: aegis_types::SymbolTradeMode::Full,
        currency_profit: "USD".into(),
        currency_base: "EUR".into(),
        freeze_level_points: 0.0,
    }
}

fn bar(i: usize, o: f64, h: f64, l: f64, c: f64) -> Candle {
    Candle { open_ts: i as i64 * 300_000, open: o, high: h, low: l, close: c, tick_volume: 10, spread_points: 10.0 }
}

/// Quiet channel, then a breakout and a rally.
fn breakout_path() -> Vec<Candle> {
    let mut v: Vec<Candle> = (0..60).map(|i| if i % 2 == 0 { bar(i, 1.0998, 1.1004, 1.0996, 1.1002) } else { bar(i, 1.1002, 1.1004, 1.0996, 1.0998) }).collect();
    let mut p = 1.1002;
    for i in 60..120 {
        let o = p;
        p += 0.0004;
        v.push(bar(i, o, p + 0.0001, o - 0.0001, p));
    }
    v
}

fn feed(lab: &mut Lab, bars: &[Candle]) {
    for i in 1..=bars.len() {
        lab.handle(ShadowInput::Bar { symbol: "EURUSD.sim".into(), timeframe: Timeframe::M5, window: Arc::new(bars[..i].to_vec()) });
    }
}

fn lab() -> Lab {
    let mut l = Lab::new(ShadowConfig::default(), 0);
    l.handle(ShadowInput::Spec(spec()));
    l
}

#[test]
fn champions_and_challengers_paper_trade_the_same_bars() {
    let mut l = lab();
    feed(&mut l, &breakout_path());
    let r = l.report();
    let champ = r.candidates.iter().find(|c| c.id == "donchian_breakout").unwrap();
    assert_eq!(champ.role, Role::Champion);
    assert!(champ.forward.trades >= 1, "{champ:?}");
    assert!(champ.forward.net > 0.0, "the rally reaches the target: {:?}", champ.forward);
    let challengers: Vec<&CandidateView> = r.candidates.iter().filter(|c| c.strategy_id == "donchian_breakout" && c.role == Role::Challenger).collect();
    assert_eq!(challengers.len(), 4, "tp x0.6/x1.5, sl x0.75/x1.33");
    assert!(challengers.iter().all(|c| c.config != champ.config));
    assert_eq!(r.bars_processed, 120);
    // Without a backtest, nothing is ever "ready".
    assert!(r.candidates.iter().all(|c| c.stage == PipelineStage::Candidate || c.stage == PipelineStage::Backtest));
}

#[test]
fn refused_signals_are_followed_without_look_ahead() {
    let mut l = lab();
    let bars = breakout_path();
    feed(&mut l, &bars[..61]);
    let d = ShadowDecision {
        decision_id: "d1".into(),
        ts_ms: bars[61].open_ts, // decided when bar 61 opens
        symbol: "EURUSD.sim".into(),
        timeframe: Timeframe::M5,
        strategy_id: "donchian_breakout".into(),
        side: Side::Buy,
        entry: 1.1010,
        sl: 1.0990,
        tp: 1.1030,
        outcome: "rejected".into(),
        failed_checks: vec!["actualites".into()],
    };
    l.handle(ShadowInput::Decision(d.clone()));
    // A bar from BEFORE the decision with a huge range must not resolve it.
    l.handle(ShadowInput::Bar { symbol: "EURUSD.sim".into(), timeframe: Timeframe::M5, window: Arc::new(vec![bar(0, 1.1, 1.2, 1.0, 1.1)]) });
    assert_eq!(l.report().pending_hypotheticals, 1);
    feed(&mut l, &bars[..]);
    let r = l.report();
    let m = r.missed.iter().find(|m| m.check == "actualites").unwrap();
    assert_eq!((m.would_have_won, m.would_have_lost), (1, 0));
    // Approved decisions are real trades, not hypotheticals.
    l.handle(ShadowInput::Decision(ShadowDecision { outcome: "approved".into(), ..d }));
    assert_eq!(l.report().pending_hypotheticals, 0);
}

#[test]
fn premature_entries_and_exits_are_flagged() {
    let mut l = lab();
    // Nearly stopped (MAE 90 % of risk) and then a win.
    l.handle(ShadowInput::TradeClosed(ShadowTrade { ticket: 1, strategy_id: "s".into(), side: Side::Buy, entry: 1.1, exit: 1.102, sl_initial: 1.099, tp_initial: 1.102, mfe_price: 0.002, mae_price: 0.0009, net: 1.0 }));
    // Reached 90 % of the target then exited near entry.
    l.handle(ShadowInput::TradeClosed(ShadowTrade { ticket: 2, strategy_id: "s".into(), side: Side::Sell, entry: 1.1, exit: 1.0999, sl_initial: 1.101, tp_initial: 1.098, mfe_price: 0.0018, mae_price: 0.0001, net: 0.01 }));
    let a = l.report().trade_analysis;
    assert_eq!((a.trades, a.premature_entries, a.premature_exits), (2, 1, 1));
    assert!(a.avg_exit_efficiency.unwrap() < 1.0);
    assert_eq!(a.notes.len(), 2);
}

#[test]
fn history_evaluation_feeds_the_pipeline_and_restore_keeps_forward_results() {
    let mut l = lab();
    let mut bars = Vec::new();
    for k in 0..12 {
        bars.extend(breakout_path().into_iter().map(|b| Candle { open_ts: b.open_ts + k as i64 * 120 * 300_000, ..b }));
    }
    l.handle(ShadowInput::History { symbol: "EURUSD.sim".into(), timeframe: Timeframe::M5, origin: "synthetic".into(), bars: Arc::new(bars) });
    let r = l.report();
    assert!(r.candidates.iter().all(|c| c.oos.len() == 1), "every candidate evaluated once");
    feed(&mut l, &breakout_path());
    let before = l.report();
    let mut fresh = lab();
    fresh.handle(ShadowInput::Restore(Box::new(before.clone())));
    let after = fresh.report();
    let f = |r: &ShadowReport| r.candidates.iter().find(|c| c.id == "donchian_breakout").unwrap().forward.clone();
    assert_eq!(f(&after), f(&before));
    // Changing a champion's parameters starts its forward record from zero.
    fresh.handle(ShadowInput::Champions(vec![("donchian_breakout".into(), serde_json::json!({"period": 30}), true)]));
    let c = fresh.report().candidates.into_iter().find(|c| c.id == "donchian_breakout").unwrap();
    assert_eq!(c.forward.trades, 0);
    assert!(c.live_enabled);
}

#[tokio::test]
async fn runs_as_an_independent_task() {
    let h = spawn(ShadowConfig::default(), 0);
    h.tx.send(ShadowInput::Spec(spec())).await.unwrap();
    let bars = breakout_path();
    for i in 1..=bars.len() {
        h.tx.send(ShadowInput::Bar { symbol: "EURUSD.sim".into(), timeframe: Timeframe::M5, window: Arc::new(bars[..i].to_vec()) }).await.unwrap();
    }
    let mut rx = h.report.clone();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if rx.borrow().bars_processed == 120 {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "shadow report never reached 120 bars");
        let _ = tokio::time::timeout(std::time::Duration::from_millis(200), rx.changed()).await;
    }
}
