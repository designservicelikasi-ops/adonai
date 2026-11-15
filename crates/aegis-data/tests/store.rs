use aegis_data::*;
use aegis_types::*;
use serde_json::json;

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
        stops_level_points: 10.0,
        swap_long: -6.0,
        swap_short: 1.0,
        trade_mode: SymbolTradeMode::Full,
        currency_profit: "USD".into(),
    }
}

fn candle(i: i64) -> Candle {
    Candle {
        open_ts: i * 60_000,
        open: 1.1,
        high: 1.1010,
        low: 1.0990,
        close: 1.1005,
        tick_volume: 10,
        spread_points: 12.0,
    }
}

async fn seed_strategy(s: &Store) {
    s.upsert_strategy(&StrategyRow {
        strategy_id: "ema_cross".into(),
        name: "EMA".into(),
        stage: "research".into(),
        enabled: true,
        config: json!({}),
    })
    .await
    .unwrap();
}

fn decision(id: &str) -> DecisionRecord {
    DecisionRecord {
        decision_id: id.into(),
        ts_ms: 1000,
        symbol: "EURUSD".into(),
        strategy_id: "ema_cross".into(),
        side: Side::Buy,
        mode: "monitor".into(),
        signal: json!({"why": "test"}),
        outcome: "pending".into(),
        verdict: None,
    }
}

fn order(cid: &str, decision_id: &str, auth: &str) -> OrderRecord {
    OrderRecord {
        cid: cid.into(),
        decision_id: decision_id.into(),
        auth_id: auth.into(),
        symbol: "EURUSD".into(),
        side: Side::Buy,
        kind: OrderKind::Market,
        volume: 0.1,
        price: 0.0,
        sl: 1.09,
        tp: 1.12,
        status: "authorized".into(),
        ticket: None,
        fill_price: None,
        fill_volume: None,
        retcode: None,
        message: None,
        created_ms: 1,
        updated_ms: 1,
    }
}

fn temp_db(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aegis-test-{}-{}", name, new_id("t")));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("aegis.db")
}

#[tokio::test]
async fn migrations_apply_and_integrity_ok() {
    let s = Store::open_in_memory().await.unwrap();
    assert!(s.integrity_check().await.unwrap());
    s.ping().await.unwrap();
}

#[tokio::test]
async fn candles_are_validated_deduplicated_and_ordered() {
    let s = Store::open_in_memory().await.unwrap();
    let batch: Vec<Candle> = (0..50).map(candle).collect();
    s.insert_candles("EURUSD", Timeframe::M1, DataOrigin::Broker, &batch).await.unwrap();
    // Re-inserting the same bars must not create duplicates.
    s.insert_candles("EURUSD", Timeframe::M1, DataOrigin::Broker, &batch).await.unwrap();
    assert_eq!(s.candle_count("EURUSD", Timeframe::M1, DataOrigin::Broker).await.unwrap(), 50);
    // Synthetic data is kept strictly separate from broker data.
    assert_eq!(s.candle_count("EURUSD", Timeframe::M1, DataOrigin::Synthetic).await.unwrap(), 0);
    let last = s.load_candles("EURUSD", Timeframe::M1, DataOrigin::Broker, 10).await.unwrap();
    assert_eq!(last.len(), 10);
    assert!(last.windows(2).all(|w| w[0].open_ts < w[1].open_ts));
    assert_eq!(last.last().unwrap().open_ts, 49 * 60_000);

    // An invalid candle aborts the whole batch (nothing is written).
    let mut bad = vec![candle(100), candle(101)];
    bad[1].high = 0.5;
    assert!(matches!(
        s.insert_candles("EURUSD", Timeframe::M1, DataOrigin::Broker, &bad).await,
        Err(DataError::Invalid(_))
    ));
    assert_eq!(s.candle_count("EURUSD", Timeframe::M1, DataOrigin::Broker).await.unwrap(), 50);
    // Misaligned timestamp is rejected.
    let mut off = candle(200);
    off.open_ts += 1;
    assert!(s.insert_candles("EURUSD", Timeframe::M1, DataOrigin::Broker, &[off]).await.is_err());
}

#[tokio::test]
async fn duplicate_decisions_and_orders_are_rejected() {
    let s = Store::open_in_memory().await.unwrap();
    seed_strategy(&s).await;
    s.insert_decision(&decision("dec-1")).await.unwrap();
    assert!(matches!(s.insert_decision(&decision("dec-1")).await, Err(DataError::Duplicate(_))));

    s.insert_order(&order("cid-1", "dec-1", "auth-1")).await.unwrap();
    // Same client id.
    assert!(matches!(
        s.insert_order(&order("cid-1", "dec-1", "auth-2")).await,
        Err(DataError::Duplicate(_))
    ));
    // Same authorization reused for another order.
    assert!(matches!(
        s.insert_order(&order("cid-2", "dec-1", "auth-1")).await,
        Err(DataError::Duplicate(_))
    ));
}

#[tokio::test]
async fn foreign_keys_are_enforced() {
    let s = Store::open_in_memory().await.unwrap();
    // No strategy row => decision insert must fail.
    assert!(s.insert_decision(&decision("dec-x")).await.is_err());
    seed_strategy(&s).await;
    // No decision row => order insert must fail.
    assert!(s.insert_order(&order("cid-9", "missing", "auth-9")).await.is_err());
    assert!(s.get_order("cid-9").await.unwrap().is_none());
}

#[tokio::test]
async fn order_lifecycle_is_traced() {
    let s = Store::open_in_memory().await.unwrap();
    seed_strategy(&s).await;
    s.insert_decision(&decision("dec-1")).await.unwrap();
    s.insert_order(&order("cid-1", "dec-1", "auth-1")).await.unwrap();
    s.update_order(&OrderUpdate { cid: "cid-1".into(), status: "sent".into(), ..Default::default() })
        .await
        .unwrap();
    s.update_order(&OrderUpdate {
        cid: "cid-1".into(),
        status: "filled".into(),
        ticket: Some(42),
        fill_price: Some(1.1002),
        fill_volume: Some(0.1),
        retcode: Some(10009),
        message: Some("done".into()),
    })
    .await
    .unwrap();
    let o = s.get_order("cid-1").await.unwrap().unwrap();
    assert_eq!(o.status, "filled");
    assert_eq!(o.ticket, Some(42));
    let ev = s.order_events("cid-1").await.unwrap();
    assert_eq!(ev.iter().map(|e| e.1.as_str()).collect::<Vec<_>>(), vec!["authorized", "sent", "filled"]);
    assert!(matches!(
        s.update_order(&OrderUpdate { cid: "nope".into(), status: "x".into(), ..Default::default() }).await,
        Err(DataError::NotFound(_))
    ));
    assert_eq!(s.orders_with_status(&["filled"]).await.unwrap().len(), 1);
    assert_eq!(s.orders_with_status(&["sent"]).await.unwrap().len(), 0);
}

#[tokio::test]
async fn positions_sync_and_deals_dedup() {
    let s = Store::open_in_memory().await.unwrap();
    let p = |ticket| Position {
        ticket,
        symbol: "EURUSD".into(),
        side: Side::Buy,
        volume: 0.1,
        price_open: 1.1,
        sl: 1.09,
        tp: 1.12,
        profit: 1.0,
        swap: 0.0,
        commission: 0.0,
        magic: 1,
        comment: "".into(),
        open_ts: 5,
    };
    s.sync_positions(&[p(1), p(2)]).await.unwrap();
    assert_eq!(s.open_positions().await.unwrap().len(), 2);
    s.sync_positions(&[p(2)]).await.unwrap();
    let open = s.open_positions().await.unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].ticket, 2);

    let d = Deal {
        deal_ticket: 77,
        position_ticket: 1,
        symbol: "EURUSD".into(),
        side: Side::Buy,
        volume: 0.1,
        price: 1.101,
        profit: 10.0,
        swap: -0.5,
        commission: -0.7,
        ts_ms: 9_000,
        comment: "tp".into(),
        origin: "Acid".into(),
    };
    assert!(s.insert_deal(&d).await.unwrap());
    assert!(!s.insert_deal(&d).await.unwrap(), "replayed deal must be ignored");
    assert_eq!(s.deals_since(0, 10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn audit_log_is_append_only_and_hash_chained() {
    let s = Store::open_in_memory().await.unwrap();
    for i in 0..5 {
        s.audit("test", "event", &json!({"i": i})).await.unwrap();
    }
    assert_eq!(s.verify_audit_chain().await.unwrap(), 5);
    assert_eq!(s.audit_tail(2).await.unwrap().len(), 2);
}

#[tokio::test]
async fn audit_tampering_is_blocked_and_detected() {
    let path = temp_db("audit");
    let s = Store::open(&path).await.unwrap();
    s.audit("test", "a", &json!({})).await.unwrap();
    s.audit("test", "b", &json!({})).await.unwrap();
    s.close().await;

    // Tamper with the file through a raw connection.
    use sqlx::Connection;
    let url = format!("sqlite://{}", path.display());
    let mut raw = sqlx::SqliteConnection::connect(&url).await.unwrap();
    // The triggers refuse UPDATE and DELETE.
    assert!(sqlx::query("UPDATE audit_log SET event = 'x' WHERE id = 1").execute(&mut raw).await.is_err());
    assert!(sqlx::query("DELETE FROM audit_log WHERE id = 1").execute(&mut raw).await.is_err());
    // Even if someone drops the trigger and edits a row, the chain exposes it.
    sqlx::query("DROP TRIGGER audit_log_no_update").execute(&mut raw).await.unwrap();
    sqlx::query("UPDATE audit_log SET detail = '{\"forged\":true}' WHERE id = 1")
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();

    let s = Store::open(&path).await.unwrap();
    assert!(matches!(s.verify_audit_chain().await, Err(DataError::AuditChainBroken(1))));
}

#[tokio::test]
async fn state_survives_restart() {
    let path = temp_db("restart");
    {
        let s = Store::open(&path).await.unwrap();
        seed_strategy(&s).await;
        s.insert_decision(&decision("dec-1")).await.unwrap();
        s.insert_order(&order("cid-1", "dec-1", "auth-1")).await.unwrap();
        s.kv_set("kill_switch", &json!({"active": true, "reason": "test"})).await.unwrap();
        s.upsert_symbol(&spec()).await.unwrap();
        s.audit("engine", "start", &json!({})).await.unwrap();
        // No explicit close: simulate an abrupt stop by dropping the pool.
    }
    let s = Store::open(&path).await.unwrap();
    let kill: serde_json::Value = s.kv_get("kill_switch").await.unwrap().unwrap();
    assert_eq!(kill["active"], true);
    assert_eq!(s.get_order("cid-1").await.unwrap().unwrap().status, "authorized");
    assert_eq!(s.load_symbols().await.unwrap(), vec![spec()]);
    assert_eq!(s.verify_audit_chain().await.unwrap(), 1);
    assert!(s.integrity_check().await.unwrap());
    assert_eq!(s.kv_get::<serde_json::Value>("absent").await.unwrap(), None);
}

#[tokio::test]
async fn backup_and_restore_roundtrip() {
    let path = temp_db("backup");
    let s = Store::open(&path).await.unwrap();
    s.audit("engine", "before-backup", &json!({})).await.unwrap();
    s.kv_set("marker", &1).await.unwrap();
    let backup = path.parent().unwrap().join("backup.db");
    s.backup_to(&backup).await.unwrap();
    // Refuses to overwrite an existing backup.
    assert!(s.backup_to(&backup).await.is_err());
    s.kv_set("marker", &2).await.unwrap();
    s.close().await;

    Store::restore_from(&backup, &path).await.unwrap();
    let s = Store::open(&path).await.unwrap();
    assert_eq!(s.kv_get::<i64>("marker").await.unwrap(), Some(1));
    assert!(path.parent().unwrap().join("aegis.db.before-restore").exists());

    // A corrupt backup is refused.
    let junk = path.parent().unwrap().join("junk.db");
    std::fs::write(&junk, b"this is not a database").unwrap();
    assert!(Store::restore_from(&junk, &path).await.is_err());
}

#[tokio::test]
async fn retention_keeps_trading_records() {
    let s = Store::open_in_memory().await.unwrap();
    seed_strategy(&s).await;
    s.insert_decision(&decision("dec-1")).await.unwrap();
    s.insert_candles("EURUSD", Timeframe::M1, DataOrigin::Broker, &[candle(1), candle(2)]).await.unwrap();
    let (c, _) = s.apply_retention(30, 30).await.unwrap();
    assert_eq!(c, 2, "candles from 1970 are older than 30 days");
    assert_eq!(s.recent_decisions(10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn risk_config_models_and_backtests_roundtrip() {
    let s = Store::open_in_memory().await.unwrap();
    assert!(s.latest_risk_config().await.unwrap().is_none());
    s.save_risk_config("user", &json!({"risk": 0.5})).await.unwrap();
    s.save_risk_config("user", &json!({"risk": 0.25})).await.unwrap();
    assert_eq!(s.latest_risk_config().await.unwrap().unwrap()["risk"], 0.25);

    s.register_model(&ModelRow {
        model_id: "m1".into(),
        created_ms: 1,
        status: "candidate".into(),
        origin: DataOrigin::Synthetic,
        path: "models/m1.json".into(),
        metrics: json!({"auc": 0.5}),
    })
    .await
    .unwrap();
    assert_eq!(s.list_models().await.unwrap()[0].origin, DataOrigin::Synthetic);

    let run = BacktestRunRow {
        run_id: "bt-1".into(),
        ts_ms: 1,
        strategy_id: "ema_cross".into(),
        symbol: "EURUSD".into(),
        timeframe: "M15".into(),
        origin: DataOrigin::Synthetic,
        params: json!({}),
        metrics: json!({"net": 1.0}),
    };
    s.save_backtest(&run, &[("oos".into(), json!({"pnl": 1.0}))]).await.unwrap();
    assert!(matches!(s.save_backtest(&run, &[]).await, Err(DataError::Duplicate(_))));
    assert_eq!(s.backtest_trade_count("bt-1").await.unwrap(), 1);
    assert_eq!(s.list_backtests(5).await.unwrap().len(), 1);
}
