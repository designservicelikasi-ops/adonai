use aegis_backtest::synthetic::{generate, SyntheticParams};
use aegis_backtest::*;
use aegis_strategy::{build, OpenView, SignalIntent, Strategy, StrategyAction, StrategyMeta};
use aegis_types::*;
use serde_json::Value;

const BAR: i64 = 900_000;

fn spec() -> SymbolSpec {
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
        stops_level_points: 0.0,
        swap_long: -6.5,
        swap_short: 1.2,
        trade_mode: SymbolTradeMode::Full,
        currency_profit: "USD".into(), currency_base: "EUR".into(), freeze_level_points: 0.0,
    }
}

/// Test strategy: enters at the close of one given bar, optionally exits at another.
struct Scripted {
    meta: StrategyMeta,
    side: Side,
    enter_at: i64,
    exit_at: Option<i64>,
    sl: f64,
    tp: f64,
}

impl Scripted {
    fn new(side: Side, enter_bar: i64, exit_bar: Option<i64>) -> Self {
        Scripted {
            meta: StrategyMeta {
                id: "scripted".into(),
                name: "scripted".into(),
                description: String::new(),
                timeframes: vec![Timeframe::M15],
                symbols: vec![],
                regimes: vec![],
                min_bars: 1,
                entry_rule: String::new(),
                exit_rule: String::new(),
                stop_rule: String::new(),
                target_rule: String::new(),
            },
            side,
            enter_at: enter_bar * BAR,
            exit_at: exit_bar.map(|b| b * BAR),
            sl: 0.0020,
            tp: 0.0040,
        }
    }
}

impl Strategy for Scripted {
    fn meta(&self) -> &StrategyMeta {
        &self.meta
    }
    fn config(&self) -> Value {
        Value::Null
    }
    fn evaluate(&self, bars: &[Candle], open: Option<&OpenView>) -> StrategyAction {
        let ts = bars.last().map(|b| b.open_ts).unwrap_or(-1);
        match open {
            None if ts == self.enter_at => StrategyAction::Enter(SignalIntent {
                side: self.side,
                sl_distance: self.sl,
                tp_distance: self.tp,
                reason: "scripted".into(),
                evidence: vec![],
            }),
            Some(_) if Some(ts) == self.exit_at => StrategyAction::Exit { reason: "scripted".into() },
            _ => StrategyAction::None,
        }
    }
}

fn flat(n: usize) -> Vec<Candle> {
    (0..n)
        .map(|i| Candle { open_ts: i as i64 * BAR, open: 1.1000, high: 1.1002, low: 1.0998, close: 1.1000, tick_volume: 10, spread_points: 10.0 })
        .collect()
}

/// No commission, no slippage, fixed 10-point spread: easy to verify by hand.
fn clean() -> BacktestParams {
    BacktestParams { commission_per_lot_round_turn: 0.0, slippage_points: 0.0, spread_points: Some(10.0), apply_swaps: false, ..Default::default() }
}

fn close_to(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn entry_fills_at_next_open_plus_spread_and_take_profit_pays_exactly() {
    let mut bars = flat(12);
    bars[8].high = 1.1050; // reaches the target
    let r = run(&Scripted::new(Side::Buy, 5, None), &spec(), &bars, &clean());
    assert_eq!(r.trades.len(), 1);
    let t = &r.trades[0];
    assert_eq!(t.entry_ts, 6 * BAR, "signal on bar 5 is filled on bar 6");
    assert!(close_to(t.entry, 1.1001), "bid open + 10 points of spread");
    assert!(close_to(t.sl, 1.0981) && close_to(t.tp, 1.1041));
    // Budget 50 / (200 points x 1 per lot) = 0.25 lot ; target = 400 points => +100.
    assert_eq!(t.volume, 0.25);
    assert_eq!(t.exit_reason, ExitReason::TakeProfit);
    assert!(close_to(t.net, 100.0), "{t:?}");
    assert_eq!(t.bars_held, 2);
    assert!(close_to(r.metrics.final_equity, 10_100.0));
    assert_eq!(r.equity_curve.last().unwrap().1, r.metrics.final_equity);
}

#[test]
fn stop_is_assumed_first_when_stop_and_target_share_a_bar() {
    let mut bars = flat(12);
    bars[7].high = 1.1050;
    bars[7].low = 1.0970;
    let r = run(&Scripted::new(Side::Buy, 5, None), &spec(), &bars, &clean());
    let t = &r.trades[0];
    assert_eq!(t.exit_reason, ExitReason::StopLoss);
    assert!(close_to(t.net, -50.0), "{t:?}");
}

#[test]
fn a_gap_through_the_stop_costs_more_than_the_stop() {
    let mut bars = flat(12);
    bars[7].open = 1.0950;
    bars[7].high = 1.0955;
    bars[7].low = 1.0945;
    bars[7].close = 1.0950;
    let r = run(&Scripted::new(Side::Buy, 5, None), &spec(), &bars, &clean());
    let t = &r.trades[0];
    assert_eq!(t.exit_reason, ExitReason::StopLoss);
    assert!(close_to(t.exit, 1.0950));
    // 1.1001 -> 1.0950 = 510 points x 0.25 lot = -127.5 (the stop alone would have been -50).
    assert!(close_to(t.net, -127.5), "{t:?}");
}

#[test]
fn shorts_are_stopped_on_the_ask() {
    let mut bars = flat(12);
    bars[7].high = 1.10195; // bid high 1.10195 + 10 points spread = ask 1.10205 >= stop 1.1020
    let r = run(&Scripted::new(Side::Sell, 5, None), &spec(), &bars, &clean());
    let t = &r.trades[0];
    assert!(close_to(t.entry, 1.1000), "sell fills at the bid");
    assert!(close_to(t.sl, 1.1020));
    assert_eq!(t.exit_reason, ExitReason::StopLoss);
    assert!(close_to(t.net, -50.0));

    // Without that spike the bid alone never reaches the stop.
    let mut calm = flat(12);
    calm[7].high = 1.1018; // ask 1.1019 stays just below the stop
    let r = run(&Scripted::new(Side::Sell, 5, None), &spec(), &calm, &clean());
    assert_eq!(r.trades[0].exit_reason, ExitReason::EndOfData);
}

#[test]
fn commission_slippage_and_swap_are_charged() {
    let mut bars = flat(200);
    bars[150].high = 1.1060;
    let params = BacktestParams { spread_points: Some(10.0), ..Default::default() }; // 7/lot, 2 points slippage, swaps on
    let r = run(&Scripted::new(Side::Buy, 5, None), &spec(), &bars, &params);
    let t = &r.trades[0];
    // Per-lot loss = 200 + 7 + 1 (half slippage) = 208 -> 50 / 208 = 0.2403 -> 0.24 lot.
    assert_eq!(t.volume, 0.24);
    assert!(close_to(t.entry, 1.10011), "open + spread + half slippage");
    assert!(close_to(t.commission, -1.68));
    // Entry bar 6 (01:30 UTC day 0), exit bar 150 (13:30 UTC day 1) => one night.
    assert!(close_to(t.swap, -6.5 * 0.24), "{t:?}");
    assert!(close_to(t.gross, 400.0 * 0.24));
    assert!(close_to(t.net, t.gross + t.commission + t.swap));
    assert!(close_to(r.metrics.total_commission, -1.68));
    assert!(r.metrics.net_profit < t.gross, "costs reduce the result");
}

#[test]
fn signal_exit_is_filled_at_the_following_open() {
    let mut bars = flat(12);
    bars[9].open = 1.1010;
    bars[9].high = 1.1012;
    bars[9].low = 1.1008;
    bars[9].close = 1.1010;
    let r = run(&Scripted::new(Side::Buy, 5, Some(8)), &spec(), &bars, &clean());
    let t = &r.trades[0];
    assert_eq!(t.exit_reason, ExitReason::Signal);
    assert_eq!(t.exit_ts, 9 * BAR);
    assert!(close_to(t.exit, 1.1010));
    assert!(close_to(t.net, 90.0 * 0.25), "1.1001 -> 1.1010 = 90 points");
}

#[test]
fn undersized_accounts_skip_the_trade_instead_of_rounding_up() {
    let params = BacktestParams { initial_equity: 100.0, ..clean() };
    let r = run(&Scripted::new(Side::Buy, 5, None), &spec(), &flat(12), &params);
    assert!(r.trades.is_empty());
    assert_eq!(r.metrics.skipped_signals, 1);
    assert_eq!(r.metrics.final_equity, 100.0);
}

#[test]
fn metrics_match_hand_computation() {
    let trade = |net: f64, bars_held: usize| Trade {
        entry_ts: 0,
        exit_ts: bars_held as i64 * BAR,
        side: Side::Buy,
        volume: 0.1,
        entry: 1.0,
        exit: 1.0,
        sl: 0.9,
        tp: 1.1,
        gross: net,
        commission: 0.0,
        swap: 0.0,
        net,
        exit_reason: ExitReason::TakeProfit,
        bars_held,
        reason: String::new(),
    };
    let trades = vec![trade(100.0, 2), trade(-50.0, 4), trade(60.0, 6), trade(-30.0, 8)];
    let curve = vec![(0, 1000.0), (1, 1100.0), (2, 1050.0), (3, 1110.0), (4, 1080.0)];
    let m = compute_metrics(&trades, &curve, 1000.0, 0);
    assert_eq!((m.trades, m.wins), (4, 2));
    assert!(close_to(m.net_profit, 80.0));
    assert!(close_to(m.gross_profit, 160.0) && close_to(m.gross_loss, 80.0));
    assert!(close_to(m.win_rate, 0.5));
    assert!(close_to(m.profit_factor.unwrap(), 2.0));
    assert!(close_to(m.max_drawdown, 50.0));
    assert!(close_to(m.max_drawdown_pct, 50.0 / 1100.0 * 100.0));
    assert!(close_to(m.avg_trade, 20.0));
    assert!(close_to(m.avg_win, 80.0) && close_to(m.avg_loss, 40.0));
    assert!(close_to(m.expectancy, 0.5 * 80.0 - 0.5 * 40.0));
    assert!(close_to(m.avg_bars_held, 5.0));
    assert!(close_to(m.avg_duration_ms, 5.0 * BAR as f64));
    assert!(close_to(m.final_equity, 1080.0));

    let none = compute_metrics(&[], &[(0, 1000.0)], 1000.0, 0);
    assert_eq!(none.trades, 0);
    assert_eq!(none.profit_factor, None);
    let only_wins = compute_metrics(&trades[..1], &curve[..2], 1000.0, 0);
    assert_eq!(only_wins.profit_factor, None, "undefined without a losing trade");
}

fn real_strategies() -> Vec<Box<dyn Strategy>> {
    aegis_strategy::default_strategies()
}

#[test]
fn results_are_reproducible() {
    let bars = generate(&SyntheticParams { bars: 3_000, ..Default::default() });
    for s in real_strategies() {
        let a = run(s.as_ref(), &spec(), &bars, &BacktestParams::default());
        let b = run(s.as_ref(), &spec(), &bars, &BacktestParams::default());
        assert_eq!(a, b, "{}", s.meta().id);
        assert!(a.metrics.trades > 0, "{} should trade on 3000 bars", s.meta().id);
        // Internal consistency of every trade.
        for t in &a.trades {
            assert!(t.exit_ts >= t.entry_ts);
            assert!(spec().is_volume_valid(t.volume));
            assert!(close_to(t.net, t.gross + t.commission + t.swap));
            assert!((t.entry - t.sl) * t.side.sign() > 0.0 && (t.tp - t.entry) * t.side.sign() > 0.0);
        }
        let sum: f64 = a.trades.iter().map(|t| t.net).sum();
        assert!(close_to(a.metrics.net_profit, sum));
        assert!(close_to(a.metrics.final_equity, 10_000.0 + sum));
    }
}

#[test]
fn no_look_ahead_trades_closed_in_the_past_do_not_change_when_data_is_added() {
    let bars = generate(&SyntheticParams { bars: 3_000, ..Default::default() });
    let cut = 1_800;
    for s in real_strategies() {
        let full = run(s.as_ref(), &spec(), &bars, &BacktestParams::default());
        let partial = run(s.as_ref(), &spec(), &bars[..cut], &BacktestParams::default());
        let settled: Vec<&Trade> = partial.trades.iter().filter(|t| t.exit_reason != ExitReason::EndOfData).collect();
        assert!(!settled.is_empty());
        for (i, t) in settled.iter().enumerate() {
            assert_eq!(*t, &full.trades[i], "{} trade {i} changed after adding future bars", s.meta().id);
        }
    }
}

#[test]
fn costs_always_hurt() {
    let bars = generate(&SyntheticParams { bars: 3_000, ..Default::default() });
    let s = build("donchian_breakout", &Value::Null).unwrap();
    let free = BacktestParams { commission_per_lot_round_turn: 0.0, slippage_points: 0.0, spread_points: Some(0.0), apply_swaps: false, ..Default::default() };
    let costly = BacktestParams { commission_per_lot_round_turn: 14.0, slippage_points: 6.0, spread_points: Some(25.0), ..Default::default() };
    let a = run(s.as_ref(), &spec(), &bars, &free);
    let b = run(s.as_ref(), &spec(), &bars, &costly);
    assert!(b.metrics.net_profit < a.metrics.net_profit, "free {} vs costly {}", a.metrics.net_profit, b.metrics.net_profit);
}

#[test]
fn segments_are_chronological_and_disjoint() {
    let bars = generate(&SyntheticParams { bars: 2_000, ..Default::default() });
    let [(n1, a), (n2, b), (n3, c)] = split(&bars, 0.6, 0.2).unwrap();
    assert_eq!((n1, n2, n3), ("train", "validation", "oos"));
    assert_eq!((a.len(), b.len(), c.len()), (1200, 400, 400));
    assert!(a.last().unwrap().open_ts < b[0].open_ts && b.last().unwrap().open_ts < c[0].open_ts);
    assert!(split(&bars, 0.7, 0.4).is_none());
    assert!(split(&bars[..10], 0.6, 0.2).is_none());

    let s = build("ema_trend", &Value::Null).unwrap();
    let seg = run_segmented(s.as_ref(), &spec(), &bars, &BacktestParams::default(), 0.6, 0.2);
    assert_eq!(seg.iter().map(|x| x.segment.as_str()).collect::<Vec<_>>(), vec!["train", "validation", "oos"]);
    assert!(seg[0].to_ts < seg[1].from_ts && seg[1].to_ts < seg[2].from_ts);
    // Each segment starts from the initial equity: results are independent.
    assert!(seg.iter().all(|x| x.result.equity_curve[0].1 == 10_000.0));

    let folds = walk_forward(s.as_ref(), &spec(), &bars, &BacktestParams::default(), 4);
    assert_eq!(folds.len(), 4);
    assert_eq!(folds.iter().map(|f| f.bars).sum::<usize>(), 2_000);
    assert!(folds.windows(2).all(|w| w[0].to_ts < w[1].from_ts));
    assert!(walk_forward(s.as_ref(), &spec(), &bars[..50], &BacktestParams::default(), 4).is_empty());
}
