//! End-to-end tests: the real engine, the real protocol over TCP, and the
//! simulated terminal (TEST ADAPTER) on the other side.

use aegis_core::view::EngineView;
use aegis_core::{resolve_shared_key, start, Command, EngineConfig, EngineHandle};
use aegis_data::{BacktestRunRow, Store};
use aegis_simterminal::{spawn, Fault, PriceFeed, SimControl, SimExit, SimHandle, SimTerminalConfig, SIM_SYMBOL};
use aegis_strategy::StrategyStage;
use aegis_types::*;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aegis-e2e-{name}-{}", new_id("t")));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn config(dir: &PathBuf) -> EngineConfig {
    let mut cfg = EngineConfig::default();
    cfg.engine.data_dir = dir.clone();
    cfg.engine.signal_timeframes = vec![Timeframe::M5];
    cfg.engine.history_bars = 300;
    cfg.bridge.listen = "127.0.0.1:0".into();
    cfg.bridge.shared_key_env = "AEGIS_E2E_KEY_NOT_SET".into();
    // Long enough for slow CI machines (Windows runners) to answer before the timeout.
    cfg.execution.result_timeout_ms = 4_000;
    cfg.execution.reconcile_grace_ms = 1_000;
    // The fixtures' out-of-sample evidence gives Wilson bounds around 0.5-0.6:
    // the probability thresholds are lowered to the minimum the configuration allows.
    // No network in tests: the news module is exercised by its own tests and `news_*` below.
    cfg.news.enabled = false;
    cfg.risk.buy_min_probability = 0.5;
    cfg.risk.sell_min_probability = 0.5;
    cfg
}

/// Out-of-sample evidence fixture for the breakout strategy on the simulator's instrument.
async fn seed_evidence(dir: &PathBuf, wins: u32, trades: u32) {
    let store = Store::open(&dir.join("aegis.db")).await.unwrap();
    store
        .save_backtest(
            &BacktestRunRow {
                run_id: new_id("bt-fixture"),
                ts_ms: now_ms(),
                strategy_id: "donchian_breakout".into(),
                symbol: SIM_SYMBOL.into(),
                timeframe: "M5".into(),
                origin: DataOrigin::Synthetic,
                params: json!({ "fixture": true }),
                metrics: json!({ "segments": [{ "segment": "oos", "metrics": { "trades": trades, "wins": wins, "net_profit": 500.0 } }] }),
            },
            &[],
        )
        .await
        .unwrap();
    store.close().await;
}

/// Quiet M5 history ending right before "now": a tight channel around 1.1000.
fn quiet_history(bars: usize) -> Vec<Candle> {
    let tf = Timeframe::M5;
    let end = tf.bucket(now_ms());
    (0..bars)
        .map(|i| {
            let c = if i % 2 == 0 { 1.1002 } else { 1.0998 };
            let o = if i % 2 == 0 { 1.0998 } else { 1.1002 };
            Candle { open_ts: end - (bars - i) as i64 * tf.millis(), open: o, high: 1.1004, low: 1.0996, close: c, tick_volume: 20, spread_points: 10.0 }
        })
        .collect()
}

/// `flat` quiet ticks, a breakout, then a rally far enough to reach any 4-ATR target.
fn breakout_script(flat: usize) -> Vec<f64> {
    let mut v: Vec<f64> = (0..flat).map(|i| if i % 2 == 0 { 1.1000 } else { 1.1001 }).collect();
    v.extend([1.1010, 1.1016, 1.1020, 1.1023, 1.1025]);
    v.extend((1..=80).map(|i| 1.1025 + i as f64 * 0.0001));
    v
}

fn sim_config(h: &EngineHandle, cfg: &EngineConfig, script: Vec<f64>) -> SimTerminalConfig {
    let (psk, _) = resolve_shared_key(cfg).unwrap();
    let mut s = SimTerminalConfig::new(&h.bridge_addr.to_string(), &psk);
    s.history = quiet_history(300);
    s.feed = PriceFeed::Script(script);
    // The scripted rally moves one pip per tick. On slower machines (Windows CI runners)
    // a 12 ms tick lets the price run past the target before the order is processed,
    // and the simulated broker rightly refuses it ("invalid stops").
    s.tick_interval_ms = if cfg!(windows) { 60 } else { 12 };
    s.sim_ms_per_tick = 60_000; // five ticks per M5 bar
    s
}

async fn wait_for(h: &EngineHandle, what: &str, f: impl Fn(&EngineView) -> bool) -> Arc<EngineView> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let v = h.snapshot();
        if f(&v) {
            return v;
        }
        if tokio::time::Instant::now() > deadline {
            // Strategy signals flood the log: show the rest first.
            let logs: Vec<String> = v.logs.iter().filter(|l| l.source != "strategy").take(25).map(|l| format!("[{}] {}", l.source, l.text)).collect();
            panic!("timeout waiting for: {what}\nrecent engine log (without strategy signals):\n{}\nalerts: {:?}", logs.join("\n"), v.alerts.iter().take(5).map(|a| &a.text).collect::<Vec<_>>());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Run a command that must succeed; on failure show the engine log.
async fn must(h: &EngineHandle, c: Command) -> String {
    match h.command(c.clone()).await {
        Ok(m) => m,
        Err(e) => {
            let v = h.snapshot();
            let logs: Vec<String> = v.logs.iter().take(30).map(|l| format!("[{}] {}", l.source, l.text)).collect();
            panic!("command {c:?} failed: {e}\nconnection: {:?}\nrecent engine log:\n{}", v.connection.is_some(), logs.join("\n"));
        }
    }
}

struct Rig {
    dir: PathBuf,
    cfg: EngineConfig,
    h: EngineHandle,
    sim: SimHandle,
}

/// Engine + connected simulator, breakout strategy promoted to Demo.
async fn rig(name: &str, flat_ticks: usize, auto: bool) -> Rig {
    rig_with(name, flat_ticks, auto, |_| {}).await
}

async fn rig_with(name: &str, flat_ticks: usize, auto: bool, tune: impl FnOnce(&mut EngineConfig)) -> Rig {
    let dir = temp_dir(name);
    let mut cfg = config(&dir);
    tune(&mut cfg);
    seed_evidence(&dir, 60, 100).await;
    let h = start(cfg.clone()).await.unwrap();
    let sim = spawn(sim_config(&h, &cfg, breakout_script(flat_ticks)));
    wait_for(&h, "simulator connected with account", |v| v.connection.is_some() && v.account.is_some() && !v.specs.is_empty()).await;
    must(&h, Command::SetStrategyStage { id: "donchian_breakout".into(), stage: StrategyStage::Demo }).await;
    if auto {
        must(&h, Command::SetMode { mode: Mode::DemoAuto, confirmation: String::new() }).await;
    }
    Rig { dir, cfg, h, sim }
}

fn breakout_decision(v: &EngineView) -> Option<&aegis_core::view::DecisionView> {
    v.decisions.iter().find(|d| d.strategy_id == "donchian_breakout")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_cycle_signal_risk_order_fill_close_reconcile() {
    // A low daily goal (net): after the first winning trade, new entries must stop.
    let r = rig_with("full", 60, true, |c| c.risk.daily_goal.amount = 0.01).await;
    let h = &r.h;

    // 1-4: engine started, simulator connected (clearly identified), market data flowing.
    let v = h.snapshot();
    assert!(v.is_simulator(), "the test adapter is never presented as a broker");
    assert_eq!(v.connection.as_ref().unwrap().terminal, TerminalKind::Simulator);
    assert_eq!(v.mode, Mode::DemoAuto);

    // 5-8: signal generated, costs and risk evaluated, authorized.
    let v = wait_for(h, "approved breakout decision", |v| breakout_decision(v).is_some_and(|d| d.outcome == "approved")).await;
    let d = breakout_decision(&v).unwrap().clone();
    assert_eq!(d.side, Side::Buy);
    assert!(d.reason.contains("canal"), "the signal explains itself: {}", d.reason);
    assert!(d.engine_checks.iter().all(|c| c.passed));
    assert!(d.assessment.checks.iter().all(|c| c.passed));
    let micro = d.micro().unwrap();
    assert!(micro.costs.total() > 0.0);
    assert!(micro.expected_net.unwrap() > 0.0);
    assert!(micro.probability_source.contains("hors échantillon"));
    assert!(d.assessment.risk_amount <= d.assessment.risk_budget);
    let cid = d.cid.clone().expect("an order was created for the approved decision");

    // 9-11: executed on the (simulated) demo account, confirmed, position monitored.
    let v = wait_for(h, "order confirmed as filled", |v| v.orders.iter().any(|o| o.cid == cid && o.status == "filled")).await;
    let order = v.orders.iter().find(|o| o.cid == cid).unwrap().clone();
    assert_eq!(order.decision_id, d.decision_id);
    assert!((order.volume - d.assessment.volume).abs() < 1e-9, "the broker received the volume the risk engine computed");
    assert!(order.fill_price.unwrap() > 0.0 && order.ticket.unwrap() > 0);
    let ticket = order.ticket.unwrap();
    let v = wait_for(h, "position visible", |v| v.positions.iter().any(|p| p.ticket == ticket) || v.deals.iter().any(|x| x.position_ticket == ticket)).await;
    if let Some(p) = v.positions.iter().find(|p| p.ticket == ticket) {
        assert_eq!(p.comment, cid);
        // Position Intelligence may already have secured/trailed the stop:
        // it can only have moved in the protective direction (BUY: upward).
        assert!(p.sl >= d.sl - 1e-9, "stop widened: {} < {}", p.sl, d.sl);
        assert!(p.tp <= d.tp + 1e-9 && p.tp > p.price_open);
        assert!(v.foreign_tickets.is_empty());
    }

    // 13-15: closed at the target, reconciled, reported.
    let v = wait_for(h, "closing deal recorded", |v| v.deals.iter().any(|x| x.position_ticket == ticket) && v.positions.is_empty()).await;
    let deal = v.deals.iter().find(|x| x.position_ticket == ticket).unwrap();
    assert_eq!(deal.comment, "tp");
    assert!(deal.profit > 0.0 && deal.commission < 0.0);
    let day = v.day.as_ref().unwrap();
    assert_eq!(day.closed_trades, 1);
    assert!((day.realized_net() - deal.net()).abs() < 1e-6);
    let v = wait_for(h, "equity reflects the result", |v| v.daily_net.is_some_and(|n| (n - deal.net()).abs() < 0.05)).await;
    assert!(v.daily_net.unwrap() > 0.0);
    // The daily goal is reached: the rally continues but nothing more is opened.
    assert!(v.daily_halt.as_ref().is_some_and(|h| h.contains("objectif")), "{:?}", v.daily_halt);
    let goal = v.goal.as_ref().unwrap();
    assert!(goal.reached && goal.blocks_entries && goal.remaining == 0.0);
    assert!((goal.achieved - deal.net()).abs() < 1e-6, "a NET goal counts the deal after commission");
    assert_eq!(day.entries_sent, 1);
    assert!(day.spread_cost_est > 0.0, "the spread paid on entry is accounted for");
    let v = wait_for(h, "a later signal refused because of the daily target", |v| {
        v.decisions.iter().any(|x| x.decision_id != d.decision_id && x.failures().iter().any(|f| f.starts_with("objectif_journalier")))
    })
    .await;
    assert_eq!(v.orders.len(), 1, "the profit target stops new entries instead of chasing more");
    let s = v.strategies.iter().find(|s| s.meta.id == "donchian_breakout").unwrap();
    assert_eq!((s.forward.trades, s.forward.wins), (1, 1));

    // The journal ties the decision to its real outcome.
    let v = wait_for(h, "journal closed", |v| v.journal.iter().any(|e| e.ticket == ticket && e.closed_ms.is_some())).await;
    let j = v.journal.iter().find(|e| e.ticket == ticket).unwrap();
    assert_eq!((j.decision_id.as_str(), j.cid.as_str(), j.strategy_id.as_str()), (d.decision_id.as_str(), cid.as_str(), "donchian_breakout"));
    assert_eq!(j.terminal, "simulator");
    assert_eq!(j.probability, micro.win_probability);
    assert!((j.net - deal.net()).abs() < 1e-9 && (j.gross - deal.profit).abs() < 1e-9);
    assert!(j.est_spread_cost > 0.0 && j.mfe_price > 0.0);
    assert_eq!(v.probability_live.as_ref().map(|r| r.n), Some(1), "live calibration counts the real trade");
    assert_eq!(v.performance.today.trades, 1);
    assert!((v.performance.all_time.net - deal.net()).abs() < 1e-9 && v.performance.by_strategy[0].key == "donchian_breakout");
    // Entry Intelligence shows the decision of the bar.
    assert!(v.entry_states.iter().any(|e| e.symbol == SIM_SYMBOL && e.timeframe == Timeframe::M5));

    // 12: everything is in the database and the audit trail is intact.
    let msg = h.command(Command::VerifyAudit).await.unwrap();
    assert!(msg.contains("intègre"));
    r.sim.stop().await;
    r.h.shutdown().await;
    let store = Store::open(&r.dir.join("aegis.db")).await.unwrap();
    let trail: Vec<String> = store.order_events(&cid).await.unwrap().into_iter().map(|e| e.1).collect();
    assert_eq!(trail, vec!["authorized", "sent", "filled"]);
    let decisions = store.recent_decisions(100).await.unwrap();
    let rec = decisions.iter().find(|x| x.decision_id == d.decision_id).unwrap();
    assert_eq!(rec.outcome, "approved");
    assert!(rec.verdict.as_ref().unwrap()["risk"]["micro"]["costs"]["commission"].as_f64().unwrap() > 0.0);
    assert_eq!(store.deals_since(0, 10).await.unwrap().len(), 1);
    // Simulator candles are stored as synthetic, never as broker data.
    assert!(store.candle_count(SIM_SYMBOL, Timeframe::M5, DataOrigin::Synthetic).await.unwrap() >= 300);
    assert_eq!(store.candle_count(SIM_SYMBOL, Timeframe::M5, DataOrigin::Broker).await.unwrap(), 0);
    let audit: Vec<String> = store.audit_tail(500).await.unwrap().into_iter().map(|a| a.event).collect();
    for expected in ["engine_start", "terminal_connected", "strategy_stage", "mode_change", "order_event", "deal", "engine_stop"] {
        assert!(audit.iter().any(|e| e == expected), "audit trail lacks `{expected}`");
    }
    assert!(store.verify_audit_chain().await.unwrap() > 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monitor_mode_observes_but_never_trades() {
    let r = rig("monitor", 40, false).await;
    let h = &r.h;
    let v = wait_for(h, "breakout observed", |v| breakout_decision(v).is_some()).await;
    let d = breakout_decision(&v).unwrap();
    assert_eq!(d.outcome, "observed");
    assert!(d.cid.is_none());
    assert!(d.assessment.authorization.is_none());
    assert_eq!(d.assessment.failed_checks().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["mode"]);
    assert!(d.micro().is_some(), "costs are still shown in MONITOR");
    // Let the whole rally play out: nothing must ever be sent or opened.
    tokio::time::sleep(Duration::from_millis(900)).await;
    let v = h.snapshot();
    assert!(v.orders.is_empty() && v.positions.is_empty() && v.deals.is_empty() && v.commands.is_empty());
    assert_eq!(v.mode, Mode::Monitor);
    // Manual actions are refused too.
    assert!(h.command(Command::ClosePosition { ticket: 1, volume: None }).await.is_err());
    r.sim.stop().await;
    r.h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn research_stage_and_missing_evidence_block_execution() {
    // No promotion: the strategy stays in Research.
    let dir = temp_dir("research");
    let cfg = config(&dir);
    seed_evidence(&dir, 60, 100).await;
    let h = start(cfg.clone()).await.unwrap();
    let sim = spawn(sim_config(&h, &cfg, breakout_script(40)));
    wait_for(&h, "connected", |v| v.account.is_some()).await;
    h.command(Command::SetMode { mode: Mode::DemoAuto, confirmation: String::new() }).await.unwrap();
    let v = wait_for(&h, "decision", |v| breakout_decision(v).is_some()).await;
    let d = breakout_decision(&v).unwrap();
    assert_eq!(d.outcome, "rejected");
    assert!(d.failures().iter().any(|f| f.starts_with("stade_strategie")), "{:?}", d.failures());
    assert!(d.cid.is_none());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(h.snapshot().orders.is_empty());
    sim.stop().await;
    h.shutdown().await;

    // Promotion is refused without out-of-sample evidence, so execution stays impossible.
    let dir = temp_dir("noevidence");
    let cfg = config(&dir);
    let h = start(cfg.clone()).await.unwrap();
    let sim = spawn(sim_config(&h, &cfg, breakout_script(40)));
    wait_for(&h, "connected", |v| v.account.is_some()).await;
    let refused = h.command(Command::SetStrategyStage { id: "donchian_breakout".into(), stage: StrategyStage::Demo }).await;
    assert!(refused.unwrap_err().contains("hors échantillon"));
    let direct = h.command(Command::SetStrategyStage { id: "donchian_breakout".into(), stage: StrategyStage::LiveApproved }).await;
    assert!(direct.is_err(), "no shortcut to live");
    h.command(Command::SetMode { mode: Mode::DemoAuto, confirmation: String::new() }).await.unwrap();
    let v = wait_for(&h, "decision", |v| breakout_decision(v).is_some()).await;
    let d = breakout_decision(&v).unwrap();
    assert_eq!(d.outcome, "rejected");
    assert!(d.failures().iter().any(|f| f.starts_with("esperance_nette")), "{:?}", d.failures());
    sim.stop().await;
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn broker_rejection_and_partial_fill_are_reported_as_they_happened() {
    let r = rig("reject", 60, true).await;
    r.sim.control.send(SimControl::Fault(Fault::Reject(10019))).await.unwrap();
    let v = wait_for(&r.h, "rejected order", |v| v.orders.iter().any(|o| o.status == "rejected")).await;
    let o = v.orders.iter().find(|o| o.status == "rejected").unwrap();
    assert_eq!(o.retcode, Some(10019));
    assert!(o.ticket.is_none() && o.fill_price.is_none());
    assert!(!v.positions.iter().any(|p| p.comment == o.cid), "a rejected order never appears as a position");
    assert!(v.logs.iter().any(|l| l.text.contains("REJETÉ par le broker")));
    r.sim.stop().await;
    r.h.shutdown().await;

    let r = rig("partial", 60, true).await;
    r.sim.control.send(SimControl::Fault(Fault::Partial(0.5))).await.unwrap();
    let v = wait_for(&r.h, "partial fill", |v| v.orders.iter().any(|o| o.status == "partially_filled")).await;
    let o = v.orders.iter().find(|o| o.status == "partially_filled").unwrap();
    assert!(o.fill_volume.unwrap() < o.volume);
    r.sim.stop().await;
    r.h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lost_acknowledgement_is_unknown_until_reconciliation_finds_the_position() {
    let r = rig("lostack", 60, true).await;
    r.sim.control.send(SimControl::Fault(Fault::ExecuteSilently)).await.unwrap();
    // The order is never reported as filled on the strength of having been sent.
    let v = wait_for(&r.h, "order sent", |v| !v.orders.is_empty()).await;
    assert!(matches!(v.orders[0].status.as_str(), "sent" | "authorized" | "unknown" | "filled"));
    let cid = v.orders[0].cid.clone();
    let v = wait_for(&r.h, "reconciled", |v| v.orders.iter().any(|o| o.cid == cid && o.status == "filled")).await;
    assert!(v.logs.iter().any(|l| l.text.contains("réconciliation")));
    r.sim.stop().await;
    r.h.shutdown().await;
    let store = Store::open(&r.dir.join("aegis.db")).await.unwrap();
    let trail: Vec<String> = store.order_events(&cid).await.unwrap().into_iter().map(|e| e.1).collect();
    assert_eq!(&trail[..2], ["authorized", "sent"], "{trail:?}");
    assert_eq!(trail.last().unwrap(), "filled", "{trail:?}");
    assert!(!trail.contains(&"not_executed".to_string()), "an executed order must never be recorded as not executed");
    // The strategy is credited with the trade even though the fill was never acknowledged.
    assert!(store.strategy_of_order(&cid).await.unwrap().is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lost_request_ends_as_not_executed() {
    let r = rig("lostreq", 60, true).await;
    r.sim.control.send(SimControl::Fault(Fault::Drop)).await.unwrap();
    let v = wait_for(&r.h, "not executed", |v| v.orders.iter().any(|o| o.status == "not_executed")).await;
    assert!(v.positions.is_empty());
    let cid = v.orders.iter().find(|o| o.status == "not_executed").unwrap().cid.clone();
    r.sim.stop().await;
    r.h.shutdown().await;
    let store = Store::open(&r.dir.join("aegis.db")).await.unwrap();
    let trail: Vec<String> = store.order_events(&cid).await.unwrap().into_iter().map(|e| e.1).collect();
    assert_eq!(trail, vec!["authorized", "sent", "unknown", "not_executed"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unauthenticated_terminals_and_replayed_frames_are_rejected() {
    let dir = temp_dir("auth");
    let cfg = config(&dir);
    let h = start(cfg.clone()).await.unwrap();

    // Wrong shared key: no session, nothing reaches the engine.
    let mut bad = sim_config(&h, &cfg, breakout_script(10));
    bad.psk = b"this-is-not-the-right-shared-key".to_vec();
    let exit = spawn(bad).task.await.unwrap();
    assert!(matches!(exit, SimExit::HandshakeFailed(_)), "{exit:?}");
    let v = wait_for(&h, "refusal recorded", |v| v.bridge.handshakes_refused == 1).await;
    assert!(v.connection.is_none() && v.account.is_none() && v.markets.is_empty());

    // Raw garbage on the socket.
    use tokio::io::AsyncWriteExt;
    let mut raw = tokio::net::TcpStream::connect(h.bridge_addr).await.unwrap();
    raw.write_all(b"{\"t\":\"order\",\"symbol\":\"EURUSD\",\"volume\":100}\n").await.unwrap();
    wait_for(&h, "garbage refused", |v| v.bridge.handshakes_refused == 2).await;
    drop(raw);

    // Genuine terminal, then a frame replayed on the same session.
    let sim = spawn(sim_config(&h, &cfg, breakout_script(500)));
    wait_for(&h, "connected", |v| v.connection.is_some() && v.account.is_some()).await;
    // A second terminal cannot hijack the session.
    let second = spawn(sim_config(&h, &cfg, breakout_script(10)));
    let exit = second.task.await.unwrap();
    assert!(!matches!(exit, SimExit::Stopped), "{exit:?}");
    wait_for(&h, "second terminal refused", |v| v.bridge.handshakes_refused == 3 && v.connection.is_some()).await;

    sim.control.send(SimControl::ReplayLastFrame).await.unwrap();
    let v = wait_for(&h, "replay detected", |v| v.bridge.protocol_violations == 1).await;
    assert!(v.connection.is_none(), "the session is closed after a replay");
    assert!(v.bridge.last_disconnect.contains("séquence"), "{}", v.bridge.last_disconnect);
    let exit = sim.task.await.unwrap();
    assert!(matches!(exit, SimExit::Disconnected(_)));
    h.shutdown().await;

    let store = Store::open(&dir.join("aegis.db")).await.unwrap();
    let audit: Vec<String> = store.audit_tail(100).await.unwrap().into_iter().map(|a| a.event).collect();
    assert_eq!(audit.iter().filter(|e| *e == "connection_refused").count(), 3);
    assert!(audit.iter().any(|e| e == "terminal_disconnected"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disconnection_and_stale_data_are_detected_and_block_entries() {
    let r = rig("disconnect", 100_000, true).await;
    let h = &r.h;
    wait_for(h, "live prices", |v| v.market(SIM_SYMBOL).is_some_and(|m| m.tradable())).await;

    // Stale market data: ticks stop while the connection stays up.
    r.sim.control.send(SimControl::PauseTicks(true)).await.unwrap();
    let v = wait_for(h, "stale price flagged", |v| v.market(SIM_SYMBOL).is_some_and(|m| !m.tradable())).await;
    let m = v.market(SIM_SYMBOL).unwrap();
    assert!(m.issues.iter().any(|i| i.describe().contains("obsolète")), "{:?}", m.issues);
    assert!(v.health.iter().any(|x| x.name == "Marché" && x.status != aegis_core::view::HealthStatus::Ok));

    // Network failure.
    r.sim.stop().await;
    let v = wait_for(h, "disconnection detected", |v| v.connection.is_none()).await;
    assert!(v.health.iter().any(|x| x.name == "Connexion terminal" && x.status == aegis_core::view::HealthStatus::Down));
    assert!(h.command(Command::SetMode { mode: Mode::LiveAuto, confirmation: String::new() }).await.is_err());

    // Reconnection works and data flows again.
    let sim = spawn(sim_config(h, &r.cfg, breakout_script(100_000)));
    wait_for(h, "reconnected with fresh prices", |v| v.connection.is_some() && v.market(SIM_SYMBOL).is_some_and(|m| m.tradable())).await;
    assert_eq!(h.snapshot().bridge.connections_accepted, 2);
    sim.stop().await;
    r.h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kill_switch_blocks_entries_survives_restart_and_needs_manual_reset() {
    let r = rig("kill", 60, true).await;
    r.h.command(Command::Kill { reason: "test d'arrêt".into() }).await.unwrap();
    let v = wait_for(&r.h, "decision under kill switch", |v| breakout_decision(v).is_some()).await;
    let d = breakout_decision(&v).unwrap();
    assert_eq!(d.outcome, "rejected");
    assert!(d.failures().iter().any(|f| f.starts_with("arret_urgence")));
    assert!(v.kill.active && v.orders.is_empty());
    assert!(r.h.command(Command::SetMode { mode: Mode::DemoAuto, confirmation: String::new() }).await.is_ok(), "already in that mode");
    r.sim.stop().await;
    r.h.shutdown().await;

    // Restart on the same data directory: still stopped, and back in MONITOR.
    let h = start(r.cfg.clone()).await.unwrap();
    let v = h.snapshot();
    assert!(v.kill.active, "the emergency stop survives a restart");
    assert_eq!(v.kill.reason, "test d'arrêt");
    assert_eq!(v.mode, Mode::Monitor, "automation is never restored automatically");
    let sim = spawn(sim_config(&h, &r.cfg, breakout_script(100_000)));
    wait_for(&h, "connected", |v| v.account.is_some()).await;
    assert!(h.command(Command::SetMode { mode: Mode::DemoAuto, confirmation: String::new() }).await.unwrap_err().contains("arrêt d'urgence"));
    assert!(h.command(Command::ResetKill { phrase: "ok".into() }).await.is_err());
    assert!(h.snapshot().kill.active);
    h.command(Command::ResetKill { phrase: "RESET AEGIS".into() }).await.unwrap();
    let v = h.snapshot();
    assert!(!v.kill.active);
    assert_eq!(v.mode, Mode::Monitor);
    sim.stop().await;
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn drawdown_triggers_the_emergency_stop_automatically() {
    let dir = temp_dir("drawdown");
    let mut cfg = config(&dir);
    cfg.risk.max_drawdown_pct = 0.5;
    cfg.risk.max_daily_loss_pct = 0.6;
    seed_evidence(&dir, 60, 100).await;
    let h = start(cfg.clone()).await.unwrap();
    // Breakout up (entry), then a collapse through the stop: a 0.5 % loss per trade = max drawdown.
    let mut script: Vec<f64> = breakout_script(60).into_iter().take(66).collect();
    script.extend((1..=60).map(|i| 1.1026 - i as f64 * 0.0002));
    let sim = spawn(sim_config(&h, &cfg, script));
    wait_for(&h, "connected", |v| v.account.is_some()).await;
    must(&h, Command::SetStrategyStage { id: "donchian_breakout".into(), stage: StrategyStage::Demo }).await;
    must(&h, Command::SetMode { mode: Mode::DemoAuto, confirmation: String::new() }).await;
    let v = wait_for(&h, "automatic emergency stop", |v| v.kill.active).await;
    assert_eq!(v.kill.actor, "system");
    assert!(v.kill.reason.contains("drawdown"));
    assert!(v.drawdown_pct.unwrap() >= 0.5);
    assert!(v.daily_net.unwrap() < 0.0);
    sim.stop().await;
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_auto_is_off_by_default_and_cannot_be_enabled_casually() {
    let r = rig("live", 100_000, false).await;
    let v = r.h.snapshot();
    assert_eq!(v.mode, Mode::Monitor);
    assert!(!v.live_armed && !v.live_trading_enabled_in_config);
    let e = r.h.command(Command::SetMode { mode: Mode::LiveAuto, confirmation: "900001 J'ACCEPTE LE RISQUE REEL".into() }).await.unwrap_err();
    assert!(e.contains("désactivé dans la configuration"), "{e}");
    r.sim.stop().await;
    r.h.shutdown().await;

    // Even when enabled in the configuration, the simulator can never go live.
    let dir = temp_dir("live2");
    let mut cfg = config(&dir);
    cfg.safety.live_trading_enabled = true;
    let h = start(cfg.clone()).await.unwrap();
    assert_eq!(h.snapshot().mode, Mode::Monitor, "enabled in config still boots in MONITOR");
    let sim = spawn(sim_config(&h, &cfg, breakout_script(100_000)));
    wait_for(&h, "connected", |v| v.account.is_some()).await;
    let e = h.command(Command::SetMode { mode: Mode::LiveAuto, confirmation: "900001 J'ACCEPTE LE RISQUE REEL".into() }).await.unwrap_err();
    assert!(e.contains("simulateur"), "{e}");
    assert!(!h.snapshot().live_armed);
    sim.stop().await;
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn risk_configuration_is_validated_persisted_and_restored() {
    let dir = temp_dir("riskcfg");
    let cfg = config(&dir);
    let h = start(cfg.clone()).await.unwrap();
    let mut rc = h.snapshot().risk_config.clone();
    rc.risk_per_trade_pct = 40.0;
    assert!(h.command(Command::SetRiskConfig(rc.clone())).await.is_err());
    assert_eq!(h.snapshot().risk_config.risk_per_trade_pct, 0.5);
    rc.risk_per_trade_pct = 0.25;
    rc.max_open_positions = 2;
    h.command(Command::SetRiskConfig(rc)).await.unwrap();
    h.shutdown().await;
    let h = start(cfg).await.unwrap();
    let v = h.snapshot();
    assert_eq!((v.risk_config.risk_per_trade_pct, v.risk_config.max_open_positions), (0.25, 2));
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backtests_are_reproducible_stored_and_feed_the_evidence() {
    let dir = temp_dir("backtest");
    let cfg = config(&dir);
    let h = start(cfg.clone()).await.unwrap();
    // Real instruments cannot receive synthetic data.
    assert!(h.command(Command::GenerateSynthetic { symbol: "EURUSD".into(), timeframe: Timeframe::M15, bars: 4000, seed: 1 }).await.is_err());
    h.command(Command::GenerateSynthetic { symbol: SIM_SYMBOL.into(), timeframe: Timeframe::M15, bars: 6000, seed: 42 }).await.unwrap();
    let bt = || Command::RunBacktest {
        strategy_id: "ema_trend".into(),
        symbol: SIM_SYMBOL.into(),
        timeframe: Timeframe::M15,
        origin: DataOrigin::Synthetic,
        params: Default::default(),
    };
    let msg = h.command(bt()).await.unwrap();
    assert!(msg.contains("SYNTHÉTIQUES"), "{msg}");
    let first = h.snapshot().last_backtest.clone().unwrap();
    h.command(bt()).await.unwrap();
    let second = h.snapshot().last_backtest.clone().unwrap();
    assert_eq!(first.full, second.full, "same data, same parameters, same result");
    assert_eq!(first.trades, second.trades);
    assert!(first.full.trades > 0);
    assert_eq!(first.segments.iter().map(|s| s.segment.as_str()).collect::<Vec<_>>(), vec!["train", "validation", "oos"]);
    assert_eq!(first.walk_forward.len(), 5);
    let v = h.snapshot();
    assert_eq!(v.backtests.len(), 2);
    let s = v.strategies.iter().find(|s| s.meta.id == "ema_trend").unwrap();
    assert_eq!(s.evidence.len(), 1);
    assert_eq!(s.evidence[0].origin, DataOrigin::Synthetic);
    assert_eq!(s.stage, "research", "a backtest never promotes a strategy by itself");
    // Missing data and unknown strategies are reported, not papered over.
    assert!(h.command(Command::RunBacktest { strategy_id: "ema_trend".into(), symbol: SIM_SYMBOL.into(), timeframe: Timeframe::H1, origin: DataOrigin::Broker, params: Default::default() }).await.is_err());
    assert!(h.command(Command::RunBacktest { strategy_id: "nope".into(), symbol: SIM_SYMBOL.into(), timeframe: Timeframe::M15, origin: DataOrigin::Synthetic, params: Default::default() }).await.is_err());

    // Dataset export for the research environment, then a backup.
    let out = dir.join("research").join("dataset.csv");
    h.command(Command::ExportDataset { symbol: SIM_SYMBOL.into(), timeframe: Timeframe::M15, origin: DataOrigin::Synthetic, horizon: 4, path: out.clone() }).await.unwrap();
    let csv = std::fs::read_to_string(&out).unwrap();
    assert!(csv.lines().count() > 5_000 && csv.starts_with("ts,ret_1"));
    assert!(out.with_extension("meta.json").exists());
    let msg = h.command(Command::Backup).await.unwrap();
    assert!(msg.contains("sauvegarde créée"));
    assert!(h.snapshot().data.last_backup.is_some());
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn database_failure_blocks_entries_and_is_reported() {
    use sqlx::Connection;
    let r = rig("dbfail", 400, true).await;
    let h = &r.h;
    wait_for(h, "live prices", |v| v.market(SIM_SYMBOL).is_some_and(|m| m.tradable())).await;

    // Another process takes the write lock: the engine can no longer record anything.
    let url = format!("sqlite://{}", r.dir.join("aegis.db").display());
    let mut locker = sqlx::SqliteConnection::connect(&url).await.unwrap();
    sqlx::query("BEGIN EXCLUSIVE").execute(&mut locker).await.unwrap();

    let v = wait_for(h, "database reported as down", |v| !v.data.db_ok).await;
    assert!(v.health.iter().any(|x| x.name == "Données" && x.status == aegis_core::view::HealthStatus::Down));
    assert!(v.logs.iter().any(|l| l.text.contains("base de données indisponible")));

    // The breakout happens while the database is down: it must not be traded.
    let v = wait_for(h, "breakout decision while the database is down", |v| breakout_decision(v).is_some()).await;
    let d = breakout_decision(&v).unwrap();
    assert_eq!(d.outcome, "rejected");
    assert!(d.cid.is_none());
    assert!(d.failures().iter().any(|f| f.starts_with("base_de_donnees")), "{:?}", d.failures());
    assert!(v.orders.is_empty() && v.positions.is_empty(), "an order that cannot be recorded is never sent");

    // The lock is released: the engine recovers by itself.
    sqlx::query("ROLLBACK").execute(&mut locker).await.unwrap();
    locker.close().await.unwrap();
    wait_for(h, "database back", |v| v.data.db_ok).await;
    r.sim.stop().await;
    r.h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probability_models_are_trained_versioned_restricted_and_rolled_back() {
    let dir = temp_dir("probability");
    let mut cfg = config(&dir);
    cfg.probability = aegis_probability::ProbabilityConfig {
        tp_grid: vec![0.5, 1.0],
        sl_grid: vec![1.0],
        horizon_bars: 20,
        stride: 4,
        folds: 3,
        min_samples: 200,
        ..Default::default()
    };
    let h = start(cfg.clone()).await.unwrap();
    must(&h, Command::GenerateSynthetic { symbol: SIM_SYMBOL.into(), timeframe: Timeframe::M5, bars: 2_500, seed: 9 }).await;
    let train = || Command::TrainProbability { symbol: SIM_SYMBOL.into(), timeframe: Timeframe::M5, origin: DataOrigin::Synthetic };
    let msg = must(&h, train()).await;
    assert!(msg.contains("SYNTHÉTIQUES"), "{msg}");
    let v = h.snapshot();
    assert_eq!(v.probability_models.len(), 1);
    let first = v.probability_models[0].clone();
    assert!(!first.usable, "a synthetic model is not usable while no simulator is connected");
    assert!(first.buy_report.n > 0 && first.buy_report.brier.is_finite(), "real out-of-sample metrics");
    must(&h, train()).await;
    let second = h.snapshot().probability_models[0].model_id.clone();
    assert_ne!(second, first.model_id, "each training is a new version");
    must(&h, Command::ActivateProbabilityModel { model_id: first.model_id.clone() }).await;
    assert_eq!(h.snapshot().probability_models[0].model_id, first.model_id, "rollback");
    assert!(h.command(Command::ActivateProbabilityModel { model_id: "missing".into() }).await.is_err());
    assert!(h.command(Command::TrainProbability { symbol: "UNKNOWN".into(), timeframe: Timeframe::M5, origin: DataOrigin::Broker }).await.is_err());
    h.shutdown().await;
    // The active version survives a restart, and the dataset is registered once.
    let h = start(cfg.clone()).await.unwrap();
    assert_eq!(h.snapshot().probability_models[0].model_id, first.model_id);
    h.shutdown().await;
    let store = Store::open(&dir.join("aegis.db")).await.unwrap();
    assert_eq!(store.datasets().await.unwrap().len(), 1, "same candles, same dataset version");
    assert_eq!(store.probability_models(None).await.unwrap().len(), 2);
    store.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn high_impact_news_suspends_entries_on_the_exposed_currencies() {
    // A local calendar: US high-impact event in 10 minutes (the simulator trades EUR/USD).
    let cal_dir = temp_dir("calendar");
    let when = chrono::DateTime::from_timestamp_millis(now_ms() + 10 * 60_000).unwrap().to_rfc3339();
    let cal = cal_dir.join("calendar.json");
    std::fs::write(&cal, format!(r#"[{{"title":"CPI m/m","country":"USD","date":"{when}","impact":"High","forecast":"0.3%","previous":"0.2%"}}]"#)).unwrap();
    let url = format!("file://{}", cal.display());
    let r = rig_with("news", 60, true, move |c| {
        c.news.enabled = true;
        c.news.sources = vec![aegis_news::NewsSource { name: "local".into(), url }];
    })
    .await;
    let h = &r.h;
    let v = wait_for(h, "calendar loaded", |v| v.news.feed_ok && !v.news.upcoming.is_empty()).await;
    let (_, a) = v.news.by_symbol.iter().find(|(s, _)| s == SIM_SYMBOL).expect("assessment for the instrument");
    assert_eq!(a.recommendation, aegis_news::Recommendation::Suspended, "{:?}", a.reasons);
    assert_eq!(v.news.upcoming[0].actual, None, "no published value invented");
    let v = wait_for(h, "breakout decision", |v| breakout_decision(v).is_some()).await;
    let d = breakout_decision(&v).unwrap();
    assert_eq!(d.outcome, "rejected");
    assert!(d.failures().iter().any(|f| f.starts_with("actualites") && f.contains("CPI")), "{:?}", d.failures());
    assert!(v.orders.is_empty(), "no order around a high-impact event");
    let e = v.entry_states.iter().find(|e| e.symbol == SIM_SYMBOL).unwrap();
    assert_eq!(e.decision, "WAIT");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shadow_observes_independently_and_promotion_needs_evidence() {
    let r = rig("shadow", 60, true).await;
    let h = &r.h;
    let v = wait_for(h, "shadow sees live bars and decisions", |v| v.shadow.as_ref().is_some_and(|s| s.bars_processed > 0 && s.decisions_seen > 0)).await;
    let s = v.shadow.as_ref().unwrap();
    assert!(s.candidates.iter().any(|c| c.id == "donchian_breakout" && c.role == aegis_shadow::Role::Champion && c.live_enabled));
    assert!(s.candidates.len() > 9, "challengers exist next to the champions");
    assert_eq!(v.shadow_dropped, 0);
    // A challenger without backtest/out-of-sample/forward evidence cannot be promoted.
    let challenger = s.candidates.iter().find(|c| c.role == aegis_shadow::Role::Challenger).unwrap().id.clone();
    let err = h.command(Command::PromoteChallenger { candidate_id: challenger }).await.unwrap_err();
    assert!(err.contains("non validé"), "{err}");
    assert!(h.command(Command::PromoteChallenger { candidate_id: "donchian_breakout".into() }).await.is_err(), "a champion is not promoted");

    // New parameters: validated, persisted, strategy back to research.
    assert!(h.command(Command::SetStrategyConfig { id: "donchian_breakout".into(), config: json!({ "period": 1 }) }).await.is_err());
    must(h, Command::SetStrategyConfig { id: "donchian_breakout".into(), config: json!({ "period": 30, "tp_atr": 1.0 }) }).await;
    let v = h.snapshot();
    let st = v.strategies.iter().find(|s| s.meta.id == "donchian_breakout").unwrap();
    assert_eq!(st.stage, "research");
    assert_eq!(st.config["period"], 30);
    assert!(st.evidence.is_empty(), "evidence earned with other parameters no longer applies");
    let Rig { dir, cfg, h, sim } = r;
    sim.stop().await;
    h.shutdown().await;
    let h = start(cfg).await.unwrap();
    let v = h.snapshot();
    let st = v.strategies.iter().find(|s| s.meta.id == "donchian_breakout").unwrap();
    assert_eq!((st.config["period"].as_u64(), st.config["tp_atr"].as_f64()), (Some(30), Some(1.0)), "parameters survive a restart");
    h.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unattended_operation_heartbeat_backups_and_alerts() {
    let dir = temp_dir("ops");
    let mut cfg = config(&dir);
    cfg.ops.backup_keep = 1;
    // Pretend the last automatic backup is old: one is due at the first timer tick.
    let store = Store::open(&dir.join("aegis.db")).await.unwrap();
    store.kv_set("last_auto_backup_ms", &1i64).await.unwrap();
    std::fs::create_dir_all(dir.join("backups")).unwrap();
    std::fs::write(dir.join("backups").join("aegis-auto-1970-01-01-00-00-00.db"), b"old").unwrap();
    std::fs::write(dir.join("backups").join("aegis-manual.db"), b"kept").unwrap();
    store.close().await;
    let h = start(cfg).await.unwrap();
    wait_for(&h, "automatic backup", |v| v.data.last_backup.as_ref().is_some_and(|b| b.contains("aegis-auto-"))).await;
    let files: Vec<String> = std::fs::read_dir(dir.join("backups")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(files.iter().filter(|f| f.starts_with("aegis-auto-")).count(), 1, "rotation keeps 1: {files:?}");
    assert!(files.contains(&"aegis-manual.db".to_string()), "manual backups are never deleted");
    let beat = std::fs::read_to_string(dir.join("heartbeat")).unwrap();
    assert!(beat.contains("MONITOR") || beat.to_lowercase().contains("monitor"), "{beat}");
    must(&h, Command::Kill { reason: "test d'alerte".into() }).await;
    let v = wait_for(&h, "alert recorded", |v| !v.alerts.is_empty()).await;
    assert_eq!(v.alerts[0].level, "critical");
    assert!(v.alerts[0].text.contains("test d'alerte"));
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn brain_takes_a_micro_profit_inside_the_net_range_and_learns_from_it() {
    let r = rig_with("brain", 60, true, |c| {
        c.brain.enabled = true;
        // Micro-gains have micro-expectations: the default minimum ($0.50) would refuse them all.
        c.risk.min_net_expected_profit = 0.01;
        // One trade is enough for this test.
        c.risk.daily_goal.amount = 0.01;
    })
    .await;
    let h = &r.h;
    let v = wait_for(h, "approved brain decision", |v| breakout_decision(v).is_some_and(|d| d.outcome == "approved")).await;
    let d = breakout_decision(&v).unwrap().clone();
    let brain = d.engine_checks.iter().find(|c| c.name == "cerveau").expect("the brain explains its choice");
    assert!(brain.passed && brain.detail.contains("espérance"), "{}", brain.detail);
    // The risk engine's own, independent computation lands in the 0.08 - 2.00 net range.
    let micro = d.micro().unwrap();
    assert!(micro.net_if_win >= 0.08 - 1e-6 && micro.net_if_win <= 2.0 + 1e-6, "{micro:?}");
    assert!(micro.expected_net.unwrap() > 0.0);
    assert!(micro.probability_source.starts_with("cerveau"), "{}", micro.probability_source);
    let choice = v.brain.choices.iter().find(|c| c.decision_id == d.decision_id).expect("choice recorded");
    let chosen = choice.chosen.clone().unwrap();
    assert!(choice.traded);
    assert!((chosen.volume - d.assessment.volume).abs() < 1e-9, "the risk engine used the brain's volume");
    assert!((d.tp - chosen.tp).abs() < 1e-9);
    // Without a probability model, only the strategy's backtest evidence exists: it is valid
    // for a CLOSER target only, so the brain never aims farther than the strategy.
    assert!(chosen.tp_distance <= choice.strategy_tp_distance + 1e-9);
    assert!(!v.brain.warnings.iter().any(|w| w.contains("aucune transaction ne peut passer")));

    // The trade closes; the journal marks it as a brain trade with its raw probability.
    let cid = d.cid.clone().unwrap();
    let v = wait_for(h, "brain trade closed", |v| v.journal.iter().any(|e| e.cid == cid && e.closed_ms.is_some())).await;
    let j = v.journal.iter().find(|e| e.cid == cid).unwrap().clone();
    assert!(j.brain && j.probability_raw == Some(chosen.probability_raw));
    assert!(j.net > 0.0 && j.net <= 2.0 + 0.05, "a micro-profit: {}", j.net);
    // The lessons now include the real costs of that trade.
    let v = wait_for(h, "lessons from the closed trade", |v| v.brain.lessons.iter().any(|l| l.trades >= 1)).await;
    let l = v.brain.lessons.iter().find(|l| l.symbol == SIM_SYMBOL).unwrap();
    assert!(l.cost.observed_per_lot.is_some() && l.contexts.len() == 1);
    // 300 bars of history are not enough to retrain: the brain says so and proposes nothing.
    let msg = must(h, Command::BrainRetrain).await;
    assert!(msg.contains("insuffisant"), "{msg}");
    assert!(h.snapshot().brain.candidates.is_empty());
    r.sim.stop().await;
    r.h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn brain_proposes_a_better_version_and_only_the_user_activates_it() {
    let r = rig_with("brain-retrain", 100_000, false, |c| {
        c.brain.enabled = true;
        c.probability = aegis_probability::ProbabilityConfig {
            tp_grid: vec![0.5, 1.0],
            sl_grid: vec![1.0],
            horizon_bars: 20,
            stride: 4,
            folds: 3,
            min_samples: 200,
            ..Default::default()
        };
    })
    .await;
    let h = &r.h;
    assert!(h.command(Command::BrainAcceptCandidate { model_id: "nothing".into() }).await.is_err());
    must(h, Command::GenerateSynthetic { symbol: SIM_SYMBOL.into(), timeframe: Timeframe::M5, bars: 3_000, seed: 4 }).await;
    let msg = must(h, Command::BrainRetrain).await;
    assert!(msg.contains("proposée"), "no active model: a calibrated version is an improvement: {msg}");
    let v = h.snapshot();
    let c = v.brain.candidates.first().expect("candidate listed").clone();
    assert!(c.active_model_id.is_none() && c.score.buy.rows > 0 && c.bars_trained < 3_300);
    assert!(v.probability_models.is_empty(), "a proposed version is NOT used before the user accepts it");
    assert!(v.alerts.iter().any(|a| a.source == "brain" && a.text.contains("prête")));
    assert_eq!(v.brain.retrain_log.first().map(|x| x.proposed), Some(true));

    must(h, Command::BrainAcceptCandidate { model_id: c.model_id.clone() }).await;
    let v = h.snapshot();
    assert_eq!(v.probability_models.first().map(|m| m.model_id.clone()), Some(c.model_id.clone()));
    assert!(v.brain.candidates.is_empty());

    // Next retraining: same data, compared with the now-active version on unseen bars.
    let msg = must(h, Command::BrainRetrain).await;
    assert!(msg.contains("écartée") || msg.contains("proposée"), "{msg}");
    if let Some(next) = h.snapshot().brain.candidates.first().cloned() {
        assert_eq!(next.active_model_id.as_deref(), Some(c.model_id.as_str()));
        must(h, Command::BrainDismissCandidate { model_id: next.model_id.clone() }).await;
        assert_eq!(h.snapshot().probability_models[0].model_id, c.model_id, "dismissing changes nothing");
    }
    r.sim.stop().await;
    r.h.shutdown().await;
    // Proposals survive a restart (stored, inactive).
    let store = Store::open(&r.dir.join("aegis.db")).await.unwrap();
    let rows = store.probability_models(None).await.unwrap();
    assert_eq!(rows.iter().filter(|m| m.active).count(), 1);
    store.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_sl_tp_in_pips_and_a_monthly_goal_are_applied_and_kept() {
    let r = rig_with("brain-pips", 60, false, |c| c.brain.enabled = true).await;
    let h = &r.h;
    // SL 12 pips, TP chosen by the brain but never beyond 8 pips.
    let mut b = h.snapshot().brain.config.clone();
    b.sl_pips = 12.0;
    b.max_tp_pips = 8.0;
    let msg = must(h, Command::SetBrainConfig(b.clone())).await;
    assert!(msg.contains("SL 12 pips") && msg.contains("au plus 8 pips"), "{msg}");
    let mut bad = b.clone();
    bad.tp_pips = 20.0;
    assert!(h.command(Command::SetBrainConfig(bad)).await.is_err(), "fixed TP above the maximum is refused");
    // Monthly goal, protecting the gain once reached.
    let mut rc = h.snapshot().risk_config.clone();
    rc.daily_goal.period = aegis_risk::GoalPeriod::Month;
    rc.daily_goal.policy = aegis_risk::GoalPolicy::ProtectGains;
    rc.daily_goal.amount = 300.0;
    must(h, Command::SetRiskConfig(rc)).await;
    let v = wait_for(h, "monthly goal shown", |v| v.goal.as_ref().is_some_and(|g| g.period == aegis_risk::GoalPeriod::Month)).await;
    assert!(v.goal.as_ref().unwrap().detail.contains("mensuel"));

    // The next signal (observed in MONITOR) uses the user's stop exactly.
    let v = wait_for(h, "breakout decision with the brain", |v| breakout_decision(v).is_some_and(|d| d.engine_checks.iter().any(|c| c.name == "cerveau"))).await;
    let d = breakout_decision(&v).unwrap().clone();
    assert!(((d.entry - d.sl) - 0.0012).abs() < 1e-9, "SL 12 pips below the entry: entry {} sl {}", d.entry, d.sl);
    let c = v.brain.choices.iter().find(|c| c.decision_id == d.decision_id).unwrap();
    assert!((c.sl_distance - 0.0012).abs() < 1e-12);
    for x in c.chosen.iter().chain(c.best.iter()) {
        assert!(x.tp_distance <= 0.0008 + 1e-9, "TP never beyond 8 pips: {x:?}");
    }
    r.sim.stop().await;
    r.h.shutdown().await;
    // Both settings survive a restart (the application's version wins over the file).
    let h = start(r.cfg.clone()).await.unwrap();
    let v = h.snapshot();
    assert_eq!((v.brain.config.sl_pips, v.brain.config.max_tp_pips), (12.0, 8.0));
    assert_eq!(v.risk_config.daily_goal.period, aegis_risk::GoalPeriod::Month);
    h.shutdown().await;
}

// ------------------------------------------------------------------ Risk Guardian 2.0 (0.3)

/// Breakout, then the price holds just above the entry: the position stays open.
fn breakout_then_hold() -> Vec<f64> {
    let mut v = breakout_script(60);
    v.truncate(60 + 5 + 3);
    v.extend((0..3_000).map(|i| if i % 2 == 0 { 1.1028 } else { 1.1027 }));
    v
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_position_that_loses_its_stop_at_the_broker_is_protected_again() {
    let dir = temp_dir("guard");
    let mut cfg = config(&dir);
    cfg.risk.missing_stop_grace_ms = 1_000;
    seed_evidence(&dir, 60, 100).await;
    let h = start(cfg.clone()).await.unwrap();
    let sim = spawn(sim_config(&h, &cfg, breakout_then_hold()));
    wait_for(&h, "simulator connected", |v| v.connection.is_some() && v.account.is_some() && !v.specs.is_empty()).await;
    must(&h, Command::SetStrategyStage { id: "donchian_breakout".into(), stage: StrategyStage::Demo }).await;
    must(&h, Command::SetMode { mode: Mode::DemoAuto, confirmation: String::new() }).await;
    let v = wait_for(&h, "AEGIS position open with its stop", |v| v.positions.iter().any(|p| p.magic == cfg.execution.magic && p.sl > 0.0)).await;
    assert_eq!(v.guardian.account_margin_mode, "hedging", "the account mode travels with the account frame");
    let p = v.positions.iter().find(|p| p.magic == cfg.execution.magic).unwrap().clone();
    let planned = v.decisions.iter().find(|d| d.cid.as_deref() == Some(p.comment.as_str())).map(|d| d.sl).unwrap();
    // Entry latency is split into engine time and terminal/broker time.
    assert!(v.guardian.execution.internal_us.count >= 1 && v.guardian.execution.broker_ms.count >= 1, "{:?}", v.guardian.execution);
    assert!(v.guardian.portfolio_risk.is_some_and(|r| r > 0.0));

    // The stop disappears at the broker.
    sim.control.send(SimControl::StripStops).await.unwrap();
    wait_for(&h, "missing stop detected", |v| v.guardian.incidents.iter().any(|i| i.ticket == p.ticket && i.kind == "stop_manquant")).await;
    // (The strategy may close the position right after: the guard's record is what is checked.)
    let v = wait_for(&h, "stop re-installed and seen at the broker", |v| {
        v.guardian.incidents.iter().any(|i| i.ticket == p.ticket && i.kind == "stop_retabli") && v.guardian.verified_modifications >= 1
    })
    .await;
    let restore = v.commands.iter().find(|c| c.ticket == p.ticket && c.action == "modify" && c.reason.contains("réinstallation")).expect("restoration command");
    // The guard re-installs the PLANNED stop (it never invents a different one).
    assert!((restore.sl - planned).abs() < 1e-9, "restored {} vs planned {planned}", restore.sl);
    assert_eq!(restore.status, "done");
    assert!(v.alerts.iter().any(|a| a.text.contains("sans stop-loss")));

    sim.stop().await;
    h.shutdown().await;
    let store = Store::open(&dir.join("aegis.db")).await.unwrap();
    let audit: Vec<String> = store.audit_tail(1_000).await.unwrap().into_iter().map(|a| a.event).collect();
    assert!(audit.iter().any(|e| e == "stop_restore"), "the restoration is in the audit trail");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_netting_account_is_recognised_and_never_stacks_positions() {
    let dir = temp_dir("netting");
    let cfg = {
        let mut c = config(&dir);
        c.risk.max_positions_per_symbol = 3;
        c.risk.max_open_positions = 5;
        c
    };
    seed_evidence(&dir, 60, 100).await;
    let h = start(cfg.clone()).await.unwrap();
    let mut s = sim_config(&h, &cfg, breakout_then_hold());
    s.broker.margin_mode = AccountMarginMode::Netting;
    let sim = spawn(s);
    let v = wait_for(&h, "netting account", |v| v.account.as_ref().is_some_and(|a| a.margin_mode == AccountMarginMode::Netting)).await;
    assert_eq!(v.guardian.account_margin_mode, "netting");
    must(&h, Command::SetStrategyStage { id: "donchian_breakout".into(), stage: StrategyStage::Demo }).await;
    must(&h, Command::SetMode { mode: Mode::DemoAuto, confirmation: String::new() }).await;
    wait_for(&h, "first position", |v| !v.positions.is_empty()).await;
    // Any later entry on the same symbol would merge into / offset that position: refused.
    let v = wait_for(&h, "a later decision refused because of the netting account", |v| {
        v.decisions.iter().any(|d| d.failures().iter().any(|f| f.starts_with("mode_marge")))
    })
    .await;
    assert_eq!(v.positions.len(), 1);
    sim.stop().await;
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_adapted_stop_never_borrows_the_strategy_backtest_probability() {
    let r = rig_with("adaptive-stop", 60, true, |c| c.risk.stops.mode = aegis_risk::StopMode::Adaptive).await;
    let v = wait_for(&r.h, "breakout decision", |v| breakout_decision(v).is_some()).await;
    let d = breakout_decision(&v).unwrap();
    let plan = d.engine_checks.iter().find(|c| c.name == "stop_initial").expect("the stop choice is explained");
    // On the quiet fixture history the planner picks a volatility (or structural) stop.
    assert!(plan.detail.contains("structurel") || plan.detail.contains("volatilité"), "{}", plan.detail);
    // The only evidence is a backtest measured with the strategy's own stops: not applicable.
    let micro = d.micro();
    assert!(micro.is_none_or(|m| m.win_probability.is_none()), "{:?}", micro.map(|m| &m.probability_source));
    assert_ne!(d.outcome, "approved");
    assert!(d.failures().iter().any(|f| f.starts_with("seuil_probabilite") || f.starts_with("esperance_nette")));
    r.sim.stop().await;
    r.h.shutdown().await;
}

// ------------------------------------------------------------------ Autonomy Commander (0.3)

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn autonomy_grants_are_bound_audited_revocable_and_only_persistent_demo_grants_resume() {
    use aegis_autonomy::Profile;
    let dir = temp_dir("autonomy");
    let cfg = config(&dir);
    seed_evidence(&dir, 60, 100).await;
    let h = start(cfg.clone()).await.unwrap();
    let v = h.snapshot();
    assert_eq!((v.autonomy.profile.as_str(), v.mode), ("research_autonomy", Mode::Monitor), "no grant at start");
    assert!(v.autonomy.capabilities.iter().all(|c| !c.trading || !c.allowed));
    // No account: a grant is always bound to an account.
    let e = h.command(Command::GrantAutonomy { profile: Profile::DemoFullAutonomy, duration_minutes: None, persist_across_restart: true, confirmation: String::new(), note: String::new() }).await.unwrap_err();
    assert!(e.contains("terminal"), "{e}");
    let sim = spawn(sim_config(&h, &cfg, breakout_then_hold()));
    wait_for(&h, "connected", |v| v.account.is_some()).await;

    // Grant DEMO FULL AUTONOMY, persistent across restarts.
    let msg = must(&h, Command::GrantAutonomy { profile: Profile::DemoFullAutonomy, duration_minutes: Some(120), persist_across_restart: true, confirmation: String::new(), note: "test".into() }).await;
    assert!(msg.contains("900001"), "{msg}");
    let v = wait_for(&h, "demo mode under the grant", |v| v.mode == Mode::DemoAuto && v.autonomy.active_grant.is_some()).await;
    let g = v.autonomy.active_grant.clone().unwrap();
    assert_eq!((g.account_login, g.granted_by.as_str(), g.persist_across_restart), (Some(900_001), "user", true));
    assert!(g.expires_ms.is_some_and(|e| e > now_ms()));
    assert!(v.autonomy.capabilities.iter().any(|c| c.capability == aegis_autonomy::Capability::OpenPositions && c.allowed));
    // A LIVE grant on the simulator is refused whatever the phrase.
    assert!(h.command(Command::GrantAutonomy { profile: Profile::LiveAuthorizedAutonomy, duration_minutes: Some(60), persist_across_restart: false, confirmation: "900001 J'ACCEPTE LE RISQUE REEL".into(), note: String::new() }).await.is_err());
    // Trading profiles cannot be the default.
    assert!(h.command(Command::SetDefaultProfile { profile: Profile::DemoFullAutonomy }).await.is_err());

    // Restart: the persistent demo grant comes back and DEMO AUTO resumes once the account reconnects.
    sim.stop().await;
    h.shutdown().await;
    let h = start(cfg.clone()).await.unwrap();
    assert_eq!(h.snapshot().mode, Mode::Monitor, "always boots in MONITOR");
    let sim = spawn(sim_config(&h, &cfg, breakout_then_hold()));
    let v = wait_for(&h, "demo grant resumed after restart", |v| v.mode == Mode::DemoAuto).await;
    assert_eq!(v.autonomy.active_grant.as_ref().map(|x| x.grant_id.clone()), Some(g.grant_id.clone()));

    // Immediate revocation.
    let msg = must(&h, Command::RevokeAutonomy { reason: "fin du test".into() }).await;
    assert!(msg.contains("révoquée"), "{msg}");
    wait_for(&h, "monitor after revocation", |v| v.mode == Mode::Monitor && v.autonomy.active_grant.is_none()).await;
    let v = wait_for(&h, "permission trail visible", |v| v.autonomy.events.iter().any(|e| e.event == "revoke")).await;
    for ev in ["grant", "resume", "revoke"] {
        assert!(v.autonomy.events.iter().any(|e| e.event == ev), "permission trail lacks `{ev}`: {:?}", v.autonomy.events.iter().map(|e| &e.event).collect::<Vec<_>>());
    }
    // Observer as default: no automatic research.
    must(&h, Command::SetDefaultProfile { profile: Profile::Observer }).await;
    let v = wait_for(&h, "observer", |v| v.autonomy.profile == "observer").await;
    assert!(v.autonomy.capabilities.iter().any(|c| c.capability == aegis_autonomy::Capability::TrainModels && !c.allowed));
    sim.stop().await;
    h.shutdown().await;

    let store = Store::open(&dir.join("aegis.db")).await.unwrap();
    let grants = store.grants(10).await.unwrap();
    assert!(grants.iter().any(|x| x.grant_id == g.grant_id && x.revoked_by.as_deref() == Some("user") && x.revoke_reason.as_deref() == Some("fin du test")));
    let audit: Vec<String> = store.audit_tail(2_000).await.unwrap().into_iter().map(|a| a.event).collect();
    assert!(audit.iter().any(|e| e == "autonomy_grant") && audit.iter().any(|e| e == "autonomy_revoke"));
}

// ------------------------------------------------------------------ Market intelligence (0.3)

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_scanner_reads_every_timeframe_scores_separately_from_probability_and_explains_decisions() {
    let r = rig("scanner", 60, true).await;
    let v = wait_for(&r.h, "breakout decision", |v| breakout_decision(v).is_some() && v.scanner.scans > 0).await;
    let d = breakout_decision(&v).unwrap();
    for name in ["regime_detaille", "multi_unites", "stop_initial", "autonomie"] {
        assert!(d.engine_checks.iter().any(|c| c.name == name), "decision lacks `{name}`: {:?}", d.engine_checks.iter().map(|c| &c.name).collect::<Vec<_>>());
    }
    let (_, reads) = v.scanner.mtf.iter().find(|(s, _)| s == SIM_SYMBOL).expect("multi-timeframe reading of the instrument");
    let tfs: Vec<Timeframe> = reads.iter().map(|r| r.timeframe).collect();
    for tf in [Timeframe::D1, Timeframe::H4, Timeframe::H1, Timeframe::M30, Timeframe::M15, Timeframe::M5, Timeframe::M1] {
        assert!(tfs.contains(&tf), "{tf} missing from {tfs:?}");
    }
    // The fixture has 300 M5 bars: D1 cannot be available — and is said so, not assumed.
    assert!(!reads.iter().find(|r| r.timeframe == Timeframe::D1).unwrap().available);
    assert!(reads.iter().find(|r| r.timeframe == Timeframe::M5).unwrap().available);
    assert!(v.scanner.regimes.iter().any(|(s, tf, _)| s == SIM_SYMBOL && tf == "M5"));
    let v = wait_for(&r.h, "an opportunity scored", |v| !v.scanner.opportunities.is_empty()).await;
    for o in &v.scanner.opportunities {
        assert!(o.opportunity.score <= 100);
        assert!(!o.opportunity.components.is_empty());
        // No probability model was trained in this test: the probability is absent and explained.
        assert!(o.probability.is_none() && !o.admissible);
        assert!(o.reasons.iter().any(|x| x.contains("probabilité")), "{:?}", o.reasons);
    }
    r.sim.stop().await;
    r.h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_decision_studies_the_net_targets_and_the_recovery_rules() {
    let r = rig("micro-recovery", 60, true).await;
    let v = wait_for(&r.h, "breakout decision with its target table", |v| breakout_decision(v).is_some() && !v.micro_targets.is_empty()).await;
    let d = breakout_decision(&v).unwrap();
    assert!(d.engine_checks.iter().any(|c| c.name == "anti_moyenne"), "recovery rules are applied to every entry");
    let t = v.micro_targets.iter().find(|t| t.symbol == SIM_SYMBOL).unwrap();
    let targets: Vec<f64> = t.rows.iter().map(|r| r.target_net).collect();
    assert_eq!(targets, vec![0.05, 0.08, 0.10, 0.15, 0.20, 0.30, 0.50]);
    for row in &t.rows {
        assert!(row.costs.commission > 0.0 && row.tp_distance >= 0.0);
        // No calibrated scenario model in this test: never feasible, and the reason says why.
        assert!(!row.feasible && !row.reason.is_empty());
    }
    // A first trade is a normal entry; the journal records its kind.
    let v = wait_for(&r.h, "journal entry", |v| !v.journal.is_empty()).await;
    assert_eq!(v.journal[0].entry_kind, "normal");
    assert!(v.recovery.note.contains("trop peu"), "{}", v.recovery.note);
    r.sim.stop().await;
    r.h.shutdown().await;
}

// ------------------------------------------------------------------ Fast Learning (0.3)

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn parameter_research_runs_in_parallel_proposes_only_and_data_is_audited() {
    let r = rig("sweep", 100_000, false).await;
    let h = &r.h;
    must(h, Command::GenerateSynthetic { symbol: SIM_SYMBOL.into(), timeframe: Timeframe::M15, bars: 4_000, seed: 3 }).await;
    let audit = must(h, Command::AuditData { symbol: SIM_SYMBOL.into(), timeframe: Timeframe::M15, origin: DataOrigin::Synthetic }).await;
    assert!(audit.contains("4000 bougies") || audit.contains("bougies"), "{audit}");
    let before = h.snapshot().strategies.iter().find(|s| s.meta.id == "donchian_breakout").unwrap().config.clone();
    let mut grid = std::collections::BTreeMap::new();
    grid.insert("period".to_string(), vec![json!(10), json!(20), json!(30)]);
    grid.insert("sl_atr".to_string(), vec![json!(1.5), json!(2.5)]);
    let msg = must(h, Command::RunParameterSweep { strategy_id: "donchian_breakout".into(), symbol: SIM_SYMBOL.into(), timeframe: Timeframe::M15, origin: DataOrigin::Synthetic, grid, folds: 4, threads: 3 }).await;
    assert!(msg.contains("6 combinaison"), "{msg}");
    let v = wait_for(h, "sweep and audit visible", |v| !v.learning.sweeps.is_empty() && !v.learning.audits.is_empty()).await;
    let s = &v.learning.sweeps[0];
    assert!(s.threads >= 1 && s.threads <= 3);
    // The running simulator may have closed a few more bars: the sweep and the audit see the same history.
    assert!(s.in_sample_bars + s.holdout_bars >= 4_000);
    assert!(v.learning.audits[0].3.bars >= 4_000);
    // A sweep proposes; it never changes the strategy.
    let after = v.strategies.iter().find(|s| s.meta.id == "donchian_breakout").unwrap().config.clone();
    assert_eq!(before, after);
    // Synthetic data is refused for a real instrument name.
    assert!(h.command(Command::RunParameterSweep { strategy_id: "donchian_breakout".into(), symbol: "EURUSD".into(), timeframe: Timeframe::M15, origin: DataOrigin::Synthetic, grid: Default::default(), folds: 4, threads: 1 }).await.is_err());
    r.sim.stop().await;
    r.h.shutdown().await;
}
