//! Capacity test: 2 000 round trips (entry + exit) in one trading day through the
//! real software chain — risk engine, single-use signed authorization, orders and
//! commands persisted in SQLite (WAL, synchronous=FULL: one fsync per write),
//! HMAC-SHA256 frames sealed and verified on both sides, broker (SIMULATOR) fill,
//! confirmation back into the executor, daily accounting.
//!
//! What it measures: the cost of AEGIS's own processing per trade on this machine.
//! What it does NOT measure: network latency to MT5 and the broker's execution
//! time (only a real demo account can measure those — see TESTING.md).
//!
//! Run: `cargo test --release -p aegis-core --test capacity -- --ignored --nocapture`

use aegis_data::Store;
use aegis_execution::{ExecConfig, ExecEvent, Executor};
use aegis_protocol::{Body, Session};
use aegis_risk::{DayStats, RiskConfig, RiskContext, RiskEngine, TradeProposal};
use aegis_simterminal::{sim_spec, SimBroker, SimBrokerConfig, SIM_SYMBOL};
use aegis_types::*;
use std::collections::HashMap;
use std::time::Instant;

const TRADES: usize = 2_000;
const DAY_START: i64 = 1_800_000_000_000 - (1_800_000_000_000 % 86_400_000);

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn two_thousand_round_trips_in_one_day() {
    let dir = std::env::temp_dir().join(format!("aegis-capacity-{}", new_id("t")));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir.join("aegis.db")).await.unwrap();
    let mut exec = Executor::new(store.clone(), ExecConfig::default());
    let cfg = RiskConfig {
        max_trades_per_day: 5_000,
        max_orders_per_minute: 120,
        max_consecutive_losses: 0,
        buy_min_probability: 0.6,
        sell_min_probability: 0.6,
        max_cost_to_gross_ratio: 0.0,
        min_net_expected_profit: 0.01,
        daily_goal: aegis_risk::DailyGoal { enabled: false, ..Default::default() },
        ..Default::default()
    };
    let mut risk = RiskEngine::new(cfg).unwrap();
    let spec = sim_spec();
    let mut specs = HashMap::new();
    specs.insert(SIM_SYMBOL.to_string(), spec.clone());
    let mut broker = SimBroker::new(SimBrokerConfig { commission_per_lot_round_turn: 7.0, ..Default::default() });
    let key = b"capacity-test-key-0123456789abcdef".to_vec();
    let (mut engine_side, mut terminal_side) = (Session::new("cap".into(), key.clone(), 30_000), Session::new("cap".into(), key, 30_000));
    let mut day = DayStats::new(DAY_START, 10_000.0, 10_000.0);
    // Decision ids must be unique: the risk engine refuses duplicates.
    let mut latencies = Vec::with_capacity(TRADES);
    let mut decide_us = Vec::with_capacity(TRADES);
    let started = Instant::now();
    let mut entries = 0;
    let mut closes = 0;

    for i in 0..TRADES {
        // 2 000 trades spread over the day: one every 43 seconds of market time.
        let now = DAY_START + 60_000 + i as i64 * 43_000;
        let mid = 1.10000 + 0.0005 * ((i as f64) * 0.1).sin();
        let (bid, ask) = (round_to(mid, 5), round_to(mid + 0.00008, 5));
        let deals = broker.on_price(SIM_SYMBOL, bid, ask, now);
        for d in &deals {
            day.on_deal(d);
        }
        let account = broker.account(now);
        let tick = Tick { ts_ms: now, bid, ask };
        let t0 = Instant::now();

        // 1. decision: risk evaluation with costs and net expectation
        let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
        let entry = if side == Side::Buy { ask } else { bid };
        let p = TradeProposal {
            decision_id: format!("cap-{i}"),
            strategy_id: "capacity".into(),
            symbol: SIM_SYMBOL.into(),
            side,
            kind: OrderKind::Market,
            entry_price: 0.0,
            sl: round_to(entry - side.sign() * 0.00100, 5),
            tp: round_to(entry + side.sign() * 0.00060, 5),
            win_probability: Some(0.72),
            probability_source: "capacity test".into(),
            claimed_risk_amount: None,
            requested_volume: Some(0.10),
        };
        let positions = broker.positions().to_vec();
        let ctx = RiskContext {
            now_ms: now,
            mode: Mode::DemoAuto,
            connected: true,
            terminal: Some(TerminalKind::Simulator),
            account: Some(&account),
            specs: &specs,
            tick: Some(&tick),
            positions: &positions,
            pending: &[],
            day: Some(&day),
            peak_equity: 10_000.0,
            live_armed: false,
        };
        let a = risk.evaluate(&p, &ctx);
        decide_us.push(t0.elapsed().as_secs_f64() * 1e6);
        assert!(a.approved, "trade {i}: {}", a.rejection_summary());

        // 2-6. authorization consumed, order persisted, frame sealed, verified, executed, confirmed
        let body = exec.submit_entry(&mut risk, a.authorization.as_ref().unwrap(), spec.point, now).await.unwrap();
        let cid = match &body { Body::Order { cid, .. } => cid.clone(), _ => unreachable!() };
        exec.mark_sent(&cid).await.unwrap();
        day.on_entry_sent(a.micro.as_ref().map(|m| m.costs.spread).unwrap_or(0.0));
        let line = engine_side.seal(body, now).unwrap();
        let received = terminal_side.open(&line, now).unwrap().body;
        let (outcome, _) = broker.execute(&received, now);
        let back = terminal_side.seal(outcome.unwrap().to_body(), now).unwrap();
        let Body::Result { cid: rc, ok, retcode, ticket, price, volume, msg } = engine_side.open(&back, now).unwrap().body else { panic!("no result") };
        match exec.on_result(&rc, ok, retcode, ticket, price, volume, &msg).await.unwrap() {
            ExecEvent::Filled { .. } => entries += 1,
            other => panic!("entry {i} not filled: {other:?}"),
        }

        // 7. exit of the same position (Position Intelligence / strategy exit path)
        let pos = broker.positions().iter().find(|x| x.ticket == ticket).cloned().expect("position open");
        let body = exec.submit_close(&pos, None, "capacity", now).await.unwrap();
        let ccid = match &body { Body::Order { cid, .. } => cid.clone(), _ => unreachable!() };
        exec.mark_sent(&ccid).await.unwrap();
        let line = engine_side.seal(body, now).unwrap();
        let received = terminal_side.open(&line, now).unwrap().body;
        let (outcome, deals) = broker.execute(&received, now);
        for d in &deals {
            day.on_deal(d);
            store.insert_deal(d).await.unwrap();
        }
        let back = terminal_side.seal(outcome.unwrap().to_body(), now).unwrap();
        let Body::Result { cid: rc, ok, retcode, ticket, price, volume, msg } = engine_side.open(&back, now).unwrap().body else { panic!("no result") };
        if let ExecEvent::CommandDone { .. } = exec.on_result(&rc, ok, retcode, ticket, price, volume, &msg).await.unwrap() {
            closes += 1;
        }
        latencies.push(t0.elapsed().as_secs_f64() * 1e3);
    }
    let total = started.elapsed().as_secs_f64();
    let (p50, p99, max) = (pct(&mut latencies.clone(), 0.5), pct(&mut latencies.clone(), 0.99), pct(&mut latencies.clone(), 1.0));
    let d50 = pct(&mut decide_us, 0.5);
    let throughput = TRADES as f64 / total;
    let report = serde_json::json!({
        "round_trips": TRADES, "entries_filled": entries, "exits_done": closes,
        "wall_time_s": total, "round_trips_per_s": throughput,
        "round_trip_ms": { "p50": p50, "p99": p99, "max": max },
        "risk_decision_us_p50": d50,
        "day": { "entries_sent": day.entries_sent, "closed_trades": day.closed_trades, "gross": day.realized_gross, "net": day.realized_net(), "commissions": day.commissions, "spread_est": day.spread_cost_est },
        "orders_in_db": store.recent_orders(100_000).await.unwrap().len(),
        "machine": format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    });
    println!("CAPACITY REPORT {}", serde_json::to_string_pretty(&report).unwrap());
    assert_eq!((entries, closes), (TRADES, TRADES));
    assert_eq!(day.entries_sent as usize, TRADES);
    assert_eq!(day.closed_trades as usize, TRADES);
    // 2 000 trades a day is one every 43 s; AEGIS's own processing must stay far below that.
    assert!(p99 < 1_000.0, "p99 round trip {p99} ms");
    store.close().await;
    let _ = std::fs::remove_dir_all(dir);
}
