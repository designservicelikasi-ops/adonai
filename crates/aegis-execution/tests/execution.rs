use aegis_data::{DecisionRecord, Store, StrategyRow};
use aegis_execution::*;
use aegis_protocol::{Body, OrderAction};
use aegis_risk::*;
use aegis_types::*;
use serde_json::json;
use std::collections::HashMap;

const NOW: i64 = 1_800_000_000_000;

fn spec() -> SymbolSpec {
    SymbolSpec {
        symbol: "EURUSD".into(), digits: 5, point: 0.00001, contract_size: 100_000.0, tick_size: 0.00001, tick_value: 1.0,
        volume_min: 0.01, volume_max: 100.0, volume_step: 0.01, stops_level_points: 10.0, swap_long: -6.5, swap_short: 1.2,
        trade_mode: SymbolTradeMode::Full, currency_profit: "USD".into(),
    }
}

struct Rig {
    store: Store,
    risk: RiskEngine,
    exec: Executor,
    n: u32,
}

impl Rig {
    async fn new() -> Rig {
        let store = Store::open_in_memory().await.unwrap();
        store
            .upsert_strategy(&StrategyRow { strategy_id: "s".into(), name: "s".into(), stage: "demo".into(), enabled: true, config: json!({}) })
            .await
            .unwrap();
        Rig { exec: Executor::new(store.clone(), ExecConfig::default()), store, risk: RiskEngine::new(RiskConfig::default()).unwrap(), n: 0 }
    }

    /// A genuine authorization from the risk engine, with its decision persisted.
    async fn authorize(&mut self, kind: OrderKind) -> Authorization {
        self.n += 1;
        let decision_id = format!("dec-{}", self.n);
        self.store
            .insert_decision(&DecisionRecord {
                decision_id: decision_id.clone(), ts_ms: NOW, symbol: "EURUSD".into(), strategy_id: "s".into(), side: Side::Buy,
                mode: "demo_auto".into(), signal: json!({}), outcome: "pending".into(), verdict: None,
            })
            .await
            .unwrap();
        let mut specs = HashMap::new();
        specs.insert("EURUSD".to_string(), spec());
        let account = AccountSnapshot {
            ts_ms: NOW, login: 1, server: "D".into(), currency: "USD".into(), balance: 10_000.0, equity: 10_000.0, margin: 0.0,
            margin_free: 10_000.0, leverage: 100, trade_mode: AccountTradeMode::Demo, trade_allowed: true,
        };
        let tick = Tick { ts_ms: NOW, bid: 1.10000, ask: 1.10010 };
        let day = DayStats::new(NOW, 10_000.0, 10_000.0);
        let (entry, sl, tp) = match kind {
            OrderKind::Market => (0.0, 1.09810, 1.10410),
            _ => (1.09900, 1.09700, 1.10300),
        };
        let proposal = TradeProposal {
            decision_id, strategy_id: "s".into(), symbol: "EURUSD".into(), side: Side::Buy, kind, entry_price: entry, sl, tp,
            win_probability: Some(0.6), probability_source: "test".into(), claimed_risk_amount: None, requested_volume: None,
        };
        let ctx = RiskContext {
            now_ms: NOW, mode: Mode::DemoAuto, connected: true, terminal: Some(TerminalKind::Mt5), account: Some(&account), specs: &specs,
            tick: Some(&tick), positions: &[], pending: &[], day: Some(&day), peak_equity: 10_000.0, live_armed: false,
        };
        let a = self.risk.evaluate(&proposal, &ctx);
        assert!(a.approved, "{}", a.rejection_summary());
        a.authorization.unwrap()
    }

    async fn status(&self, cid: &str) -> String {
        self.store.get_order(cid).await.unwrap().unwrap().status
    }
}

fn cid_of(b: &Body) -> String {
    match b {
        Body::Order { cid, .. } => cid.clone(),
        _ => panic!("not an order"),
    }
}

fn position(ticket: i64, comment: &str, magic: i64) -> Position {
    Position {
        ticket, symbol: "EURUSD".into(), side: Side::Buy, volume: 0.23, price_open: 1.10012, sl: 1.09810, tp: 1.10410, profit: 0.0,
        swap: 0.0, commission: 0.0, magic, comment: comment.into(), open_ts: NOW,
    }
}

#[tokio::test]
async fn an_authorization_produces_exactly_one_order() {
    let mut r = Rig::new().await;
    let auth = r.authorize(OrderKind::Market).await;
    let body = r.exec.submit_entry(&mut r.risk, &auth, 0.00001, NOW + 10).await.unwrap();
    match &body {
        Body::Order { action, volume, sl, tp, auth_id, magic, expires, deviation, .. } => {
            assert_eq!(*action, OrderAction::Open);
            assert_eq!((*volume, *sl, *tp), (auth.volume, auth.sl, auth.tp));
            assert_eq!(auth_id, &auth.auth_id);
            assert_eq!(*magic, ExecConfig::default().magic);
            assert_eq!(*expires, auth.expires_ms);
            assert_eq!(*deviation, 10.0);
        }
        _ => panic!(),
    }
    let cid = cid_of(&body);
    assert!(cid.len() <= 31, "must fit an MT5 comment");
    assert_eq!(r.status(&cid).await, "authorized");
    r.exec.mark_sent(&cid).await.unwrap();
    assert_eq!(r.status(&cid).await, "sent");

    // Replaying the same authorization is refused and creates nothing.
    let again = r.exec.submit_entry(&mut r.risk, &auth, 0.00001, NOW + 20).await;
    assert!(matches!(again, Err(ExecError::Auth(AuthError::AlreadyUsed))));
    assert_eq!(r.store.recent_orders(10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn forged_expired_or_killed_authorizations_never_become_orders() {
    let mut r = Rig::new().await;
    let auth = r.authorize(OrderKind::Market).await;

    let mut forged = auth.clone();
    forged.volume = 10.0;
    assert!(matches!(r.exec.submit_entry(&mut r.risk, &forged, 0.00001, NOW).await, Err(ExecError::Auth(AuthError::BadSignature))));
    assert!(matches!(r.exec.submit_entry(&mut r.risk, &auth, 0.00001, NOW + 60_000).await, Err(ExecError::Auth(AuthError::Expired))));

    let auth2 = r.authorize(OrderKind::Market).await;
    r.risk.engage_kill("test", "user", NOW);
    assert!(matches!(r.exec.submit_entry(&mut r.risk, &auth2, 0.00001, NOW).await, Err(ExecError::Auth(AuthError::KillSwitchActive))));

    assert!(r.store.recent_orders(10).await.unwrap().is_empty(), "no order row for refused authorizations");
    assert_eq!(r.exec.in_flight(), 0);
}

#[tokio::test]
async fn broker_confirmation_drives_the_status() {
    let mut r = Rig::new().await;

    // Filled, with 2 points of adverse slippage.
    let auth = r.authorize(OrderKind::Market).await;
    let cid = cid_of(&r.exec.submit_entry(&mut r.risk, &auth, 0.00001, NOW).await.unwrap());
    r.exec.mark_sent(&cid).await.unwrap();
    let ev = r.exec.on_result(&cid, true, 10009, 555, 1.10012, 0.23, "done").await.unwrap();
    match ev {
        ExecEvent::Filled { ticket, slippage_points, volume, .. } => {
            assert_eq!(ticket, 555);
            assert_eq!(volume, 0.23);
            assert!((slippage_points.unwrap() - 2.0).abs() < 1e-6);
        }
        other => panic!("{other:?}"),
    }
    let o = r.store.get_order(&cid).await.unwrap().unwrap();
    assert_eq!((o.status.as_str(), o.ticket, o.fill_price), ("filled", Some(555), Some(1.10012)));
    // A duplicated / replayed result changes nothing.
    assert_eq!(r.exec.on_result(&cid, true, 10009, 999, 9.9, 9.9, "dup").await.unwrap(), ExecEvent::UnexpectedResult { cid: cid.clone() });
    assert_eq!(r.store.get_order(&cid).await.unwrap().unwrap().ticket, Some(555));

    // Rejected by the broker.
    let auth = r.authorize(OrderKind::Market).await;
    let cid = cid_of(&r.exec.submit_entry(&mut r.risk, &auth, 0.00001, NOW).await.unwrap());
    let ev = r.exec.on_result(&cid, false, 10019, 0, 0.0, 0.0, "not enough money").await.unwrap();
    assert_eq!(ev, ExecEvent::Rejected { cid: cid.clone(), retcode: 10019, message: "not enough money".into() });
    assert_eq!(r.status(&cid).await, "rejected");

    // Partial fill.
    let auth = r.authorize(OrderKind::Market).await;
    let cid = cid_of(&r.exec.submit_entry(&mut r.risk, &auth, 0.00001, NOW).await.unwrap());
    r.exec.on_result(&cid, true, 10010, 556, 1.1001, 0.10, "partial").await.unwrap();
    let o = r.store.get_order(&cid).await.unwrap().unwrap();
    assert_eq!((o.status.as_str(), o.fill_volume), ("partially_filled", Some(0.10)));

    // Pending order accepted.
    let auth = r.authorize(OrderKind::Limit).await;
    let cid = cid_of(&r.exec.submit_entry(&mut r.risk, &auth, 0.00001, NOW).await.unwrap());
    assert_eq!(r.exec.on_result(&cid, true, 10008, 777, 0.0, 0.0, "placed").await.unwrap(), ExecEvent::Placed { cid: cid.clone(), ticket: 777 });
    assert_eq!(r.status(&cid).await, "placed");

    // A result nobody asked for.
    assert_eq!(r.exec.on_result("Aunknown", true, 10009, 1, 1.0, 1.0, "").await.unwrap(), ExecEvent::UnexpectedResult { cid: "Aunknown".into() });
    assert_eq!(r.exec.in_flight(), 0);
}

#[tokio::test]
async fn a_success_without_ticket_is_not_treated_as_executed() {
    let mut r = Rig::new().await;
    let auth = r.authorize(OrderKind::Market).await;
    let cid = cid_of(&r.exec.submit_entry(&mut r.risk, &auth, 0.00001, NOW).await.unwrap());
    let ev = r.exec.on_result(&cid, true, 10009, 0, 0.0, 0.0, "weird").await.unwrap();
    assert_eq!(ev, ExecEvent::TimedOut { cid: cid.clone() });
    assert_eq!(r.status(&cid).await, "unknown");
    assert_eq!(r.exec.in_flight(), 1, "still waiting for reconciliation");
}

#[tokio::test]
async fn silence_leads_to_unknown_then_reconciliation_decides() {
    let mut r = Rig::new().await;
    let magic = ExecConfig::default().magic;

    // Case 1: the order was in fact executed (the answer was lost).
    let auth = r.authorize(OrderKind::Market).await;
    let cid = cid_of(&r.exec.submit_entry(&mut r.risk, &auth, 0.00001, NOW).await.unwrap());
    r.exec.mark_sent(&cid).await.unwrap();
    assert!(r.exec.check_timeouts(NOW + 4_000).await.unwrap().is_empty());
    assert_eq!(r.exec.check_timeouts(NOW + 6_000).await.unwrap(), vec![ExecEvent::TimedOut { cid: cid.clone() }]);
    assert_eq!(r.status(&cid).await, "unknown");
    assert!(r.exec.check_timeouts(NOW + 7_000).await.unwrap().is_empty(), "reported once");
    let ev = r.exec.reconcile(&[position(900, &cid, magic)], &[], NOW + 8_000).await.unwrap();
    assert_eq!(ev, vec![ExecEvent::ReconciledFilled { cid: cid.clone(), ticket: 900 }]);
    let o = r.store.get_order(&cid).await.unwrap().unwrap();
    assert_eq!((o.status.as_str(), o.ticket), ("filled", Some(900)));

    // Case 2: nothing was executed.
    let auth = r.authorize(OrderKind::Market).await;
    let cid = cid_of(&r.exec.submit_entry(&mut r.risk, &auth, 0.00001, NOW).await.unwrap());
    r.exec.check_timeouts(NOW + 6_000).await.unwrap();
    // A snapshot taken too soon after sending proves nothing.
    assert!(r.exec.reconcile(&[], &[], NOW + 1_000).await.unwrap().is_empty());
    assert_eq!(r.status(&cid).await, "unknown");
    // A position with the same comment but a foreign magic number is not ours.
    assert!(r.exec.reconcile(&[position(901, &cid, 42)], &[], NOW + 1_500).await.unwrap().is_empty());
    let ev = r.exec.reconcile(&[], &[], NOW + 9_000).await.unwrap();
    assert_eq!(ev, vec![ExecEvent::ReconciledNotExecuted { cid: cid.clone() }]);
    assert_eq!(r.status(&cid).await, "not_executed");
    // A late answer for a settled order is flagged, not applied.
    assert_eq!(r.exec.on_result(&cid, true, 10009, 5, 1.1, 0.23, "late").await.unwrap(), ExecEvent::UnexpectedResult { cid });
}

#[tokio::test]
async fn orders_in_flight_survive_a_restart_as_unknown() {
    let mut r = Rig::new().await;
    let auth = r.authorize(OrderKind::Market).await;
    let cid = cid_of(&r.exec.submit_entry(&mut r.risk, &auth, 0.00001, NOW).await.unwrap());
    r.exec.mark_sent(&cid).await.unwrap();

    // New executor on the same database = engine restart.
    let mut fresh = Executor::new(r.store.clone(), ExecConfig::default());
    assert_eq!(fresh.recover(NOW + 60_000).await.unwrap(), 1);
    assert_eq!(r.status(&cid).await, "unknown");
    let ev = fresh.reconcile(&[position(910, &cid, ExecConfig::default().magic)], &[], NOW + 61_000).await.unwrap();
    assert_eq!(ev, vec![ExecEvent::ReconciledFilled { cid: cid.clone(), ticket: 910 }]);
    let trail: Vec<String> = r.store.order_events(&cid).await.unwrap().into_iter().map(|e| e.1).collect();
    assert_eq!(trail, vec!["authorized", "sent", "unknown", "filled"]);
}

#[tokio::test]
async fn undelivered_frames_are_recorded_as_not_executed() {
    let mut r = Rig::new().await;
    let auth = r.authorize(OrderKind::Market).await;
    let cid = cid_of(&r.exec.submit_entry(&mut r.risk, &auth, 0.00001, NOW).await.unwrap());
    r.exec.mark_send_failed(&cid, "terminal disconnected").await.unwrap();
    let o = r.store.get_order(&cid).await.unwrap().unwrap();
    assert_eq!(o.status, "not_executed");
    assert!(o.message.unwrap().contains("terminal disconnected"));
    assert_eq!(r.exec.in_flight(), 0);
}

#[tokio::test]
async fn management_commands_only_reduce_risk() {
    let mut r = Rig::new().await;
    let magic = ExecConfig::default().magic;
    let pos = position(42, "Aabc", magic);

    // Close: full and partial; never more than the position.
    let full = r.exec.submit_close(&pos, None, "strategy exit", NOW).await.unwrap();
    assert!(matches!(&full, Body::Order { action: OrderAction::Close, volume, ticket: 42, .. } if *volume == 0.23));
    assert!(r.exec.submit_close(&pos, Some(0.10), "partial", NOW).await.is_ok());
    assert!(matches!(r.exec.submit_close(&pos, Some(5.0), "too much", NOW).await, Err(ExecError::Refused(_))));
    assert!(matches!(r.exec.submit_close(&pos, Some(0.0), "zero", NOW).await, Err(ExecError::Refused(_))));

    // Modify: the stop may be tightened, never loosened or removed.
    assert!(r.exec.submit_modify(&pos, 1.09900, 1.10410, "trail", NOW).await.is_ok());
    assert!(matches!(r.exec.submit_modify(&pos, 1.09700, 1.10410, "loosen", NOW).await, Err(ExecError::Refused(_))));
    assert!(matches!(r.exec.submit_modify(&pos, 0.0, 1.10410, "remove", NOW).await, Err(ExecError::Refused(_))));
    assert!(matches!(r.exec.submit_modify(&pos, 1.09900, 1.09000, "tp wrong side", NOW).await, Err(ExecError::Refused(_))));
    // Mirror rule for a short.
    let short = Position { side: Side::Sell, sl: 1.10200, tp: 1.09600, ..pos.clone() };
    assert!(r.exec.submit_modify(&short, 1.10100, 1.09600, "tighten", NOW).await.is_ok());
    assert!(r.exec.submit_modify(&short, 1.10300, 1.09600, "loosen", NOW).await.is_err());

    // Results are traced.
    let cid = cid_of(&full);
    r.exec.mark_sent(&cid).await.unwrap();
    assert_eq!(r.exec.on_result(&cid, true, 10009, 42, 1.1, 0.23, "closed").await.unwrap(), ExecEvent::CommandDone { cid: cid.clone(), action: "close".into(), ticket: 42 });
    let cmds = r.store.recent_commands(20).await.unwrap();
    assert_eq!(cmds.len(), 4, "refused commands are not stored");
    assert_eq!(cmds.iter().find(|c| c.cid == cid).unwrap().status, "done");

    let ord = PendingOrder { ticket: 7, symbol: "EURUSD".into(), side: Side::Buy, kind: OrderKind::Limit, volume: 0.1, price: 1.09, sl: 1.08, tp: 1.1, magic, comment: String::new(), setup_ts: NOW };
    let cancel = r.exec.submit_cancel(&ord, "kill switch", NOW).await.unwrap();
    let ccid = cid_of(&cancel);
    let ev = r.exec.on_result(&ccid, false, 10013, 7, 0.0, 0.0, "invalid request").await.unwrap();
    assert!(matches!(ev, ExecEvent::CommandFailed { ticket: 7, .. }));

    assert_eq!(r.exec.foreign_positions(&[pos.clone(), position(43, "manual", 0)]).len(), 1);
}

#[tokio::test]
async fn a_position_opened_and_closed_before_any_confirmation_is_still_recognised() {
    let mut r = Rig::new().await;
    let auth = r.authorize(OrderKind::Market).await;
    let cid = cid_of(&r.exec.submit_entry(&mut r.risk, &auth, 0.00001, NOW).await.unwrap());
    r.exec.mark_sent(&cid).await.unwrap();
    // The acknowledgement is lost; the position hits its target before the timeout.
    assert_eq!(r.exec.note_deal("someone-else", 1), None);
    assert_eq!(r.exec.note_deal("", 1), None);
    assert_eq!(r.exec.note_deal(&cid, 920), Some(cid.clone()));
    // The account is flat again, yet the order must NOT be declared "not executed".
    let ev = r.exec.reconcile(&[], &[], NOW + 60_000).await.unwrap();
    assert_eq!(ev, vec![ExecEvent::ReconciledFilled { cid: cid.clone(), ticket: 920 }]);
    assert_eq!(r.status(&cid).await, "filled");
    assert_eq!(r.exec.in_flight(), 0);
}

#[tokio::test]
async fn a_snapshot_showing_the_position_settles_the_order_without_waiting_for_the_timeout() {
    let mut r = Rig::new().await;
    let auth = r.authorize(OrderKind::Market).await;
    let cid = cid_of(&r.exec.submit_entry(&mut r.risk, &auth, 0.00001, NOW).await.unwrap());
    // Not timed out yet, but an empty snapshot proves nothing...
    assert!(r.exec.reconcile(&[], &[], NOW + 10_000).await.unwrap().is_empty());
    assert_eq!(r.exec.in_flight(), 1);
    // ...whereas a snapshot containing the position does.
    let ev = r.exec.reconcile(&[position(930, &cid, ExecConfig::default().magic)], &[], NOW + 200).await.unwrap();
    assert_eq!(ev, vec![ExecEvent::ReconciledFilled { cid, ticket: 930 }]);
}
