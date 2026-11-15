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
    cfg.execution.result_timeout_ms = 1_000;
    cfg.execution.reconcile_grace_ms = 300;
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
    s.tick_interval_ms = 12;
    s.sim_ms_per_tick = 60_000; // five ticks per M5 bar
    s
}

async fn wait_for(h: &EngineHandle, what: &str, f: impl Fn(&EngineView) -> bool) -> Arc<EngineView> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let v = h.snapshot();
        if f(&v) {
            return v;
        }
        if tokio::time::Instant::now() > deadline {
            let logs: Vec<String> = v.logs.iter().take(25).map(|l| format!("[{}] {}", l.source, l.text)).collect();
            panic!("timeout waiting for: {what}\nrecent engine log:\n{}", logs.join("\n"));
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
    // A low daily target: after the first winning trade, new entries must stop.
    let r = rig_with("full", 60, true, |c| c.risk.daily_profit_target_pct = 0.3).await;
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
        assert_eq!(p.sl, d.sl);
        assert_eq!(p.tp, d.tp);
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
    // The daily target is reached: the rally continues but nothing more is opened.
    assert!(v.daily_halt.as_ref().is_some_and(|h| h.contains("objectif")), "{:?}", v.daily_halt);
    let v = wait_for(h, "a later signal refused because of the daily target", |v| {
        v.decisions.iter().any(|x| x.decision_id != d.decision_id && x.failures().iter().any(|f| f.starts_with("objectif_journalier")))
    })
    .await;
    assert_eq!(v.orders.len(), 1, "the profit target stops new entries instead of chasing more");
    let s = v.strategies.iter().find(|s| s.meta.id == "donchian_breakout").unwrap();
    assert_eq!((s.forward.trades, s.forward.wins), (1, 1));

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
