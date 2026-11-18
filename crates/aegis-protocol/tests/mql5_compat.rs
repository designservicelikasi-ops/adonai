//! Cross-checks between the Rust engine and the MQL5 connector (mt5/*.mq5).
//!
//! The connector cannot be compiled on the development machine, so these tests
//! pin down, on the Rust side, exactly what the MQL5 code is written to produce
//! and to consume: field names, value formats and reference cryptographic values.

use aegis_protocol::*;
use aegis_types::*;
use hmac::{Hmac, Mac};
use sha2::Sha256;

const PSK: &[u8] = b"0123456789abcdef0123456789abcdef";
const NOW: i64 = 1_800_000_000_000;

fn hmac_hex(key: &[u8], msg: &[u8]) -> String {
    let mut m = Hmac::<Sha256>::new_from_slice(key).unwrap();
    m.update(msg);
    hex::encode(m.finalize().into_bytes())
}

/// Reference values hard-coded in mt5/AegisSelfTest.mq5.
#[test]
fn self_test_reference_values() {
    assert_eq!(hmac_hex(&[0x0b; 20], b"Hi There"), "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");
    assert_eq!(hmac_hex(b"Jefe", b"what do ya want for nothing?"), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    assert_eq!(
        hmac_hex(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First"),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
    let key = derive_session_key(PSK, "client-nonce-0001", "server-nonce-0001");
    assert_eq!(key.len(), 32);
    // Embedded at compile time: no runtime file access (see TEST_REPORT.md, macOS note).
    let mq5 = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../mt5/AegisSelfTest.mq5"));
    assert!(mq5.contains(&hex::encode(&key)), "AegisSelfTest.mq5 must contain the engine's session key {}", hex::encode(&key));
}

/// Build a frame the way `AegisSealFrame` does in MQL5: string concatenation,
/// envelope fields first, numbers printed with a fixed number of decimals.
fn mql5_frame(key: &[u8], seq: u64, sid: &str, body_fields: &str) -> String {
    let json = format!("{{\"v\":1,\"id\":\"{sid}-c{seq}\",\"ts\":{NOW},\"seq\":{seq},\"sid\":\"{sid}\",{body_fields}}}");
    format!("{} {}", hmac_hex(key, json.as_bytes()), json)
}

#[test]
fn every_connector_message_is_accepted_by_the_engine() {
    // Field lists copied from mt5/AegisConnector.mq5 (SendSpec, SendAccount, SendSnapshot, ...).
    let bodies: Vec<(&str, &str)> = vec![
        ("spec", r#""t":"spec","symbol":"EURUSD","digits":5,"point":0.0000100000,"contract_size":100000.00000000,"tick_size":0.0000100000,"tick_value":1.0000000000,"volume_min":0.01000000,"volume_max":100.00000000,"volume_step":0.01000000,"stops_level":10.0,"swap_long":-6.50000000,"swap_short":1.20000000,"trade_mode":"full","currency_profit":"USD","currency_base":"EUR","freeze_level":5.0"#),
        // A connector of protocol 1.0 (no currency_base / freeze_level / fee) is still accepted.
        ("spec", r#""t":"spec","symbol":"GBPUSD","digits":5,"point":0.0000100000,"contract_size":100000.00000000,"tick_size":0.0000100000,"tick_value":1.0000000000,"volume_min":0.01000000,"volume_max":100.00000000,"volume_step":0.01000000,"stops_level":10.0,"swap_long":-6.50000000,"swap_short":1.20000000,"trade_mode":"full","currency_profit":"USD""#),
        ("account", r#""t":"account","login":5001234,"server":"Broker-Demo","currency":"USD","balance":10000.00,"equity":10012.50,"margin":110.00,"margin_free":9902.50,"leverage":100,"trade_mode":"demo","trade_allowed":true"#),
        ("pos", r#""t":"pos","snap":3,"ticket":123456789,"symbol":"EURUSD","side":"buy","volume":0.23000000,"price_open":1.10012000,"sl":1.09810000,"tp":1.10410000,"profit":-2.30,"swap":0.00,"commission":0.00,"magic":20260115,"comment":"A0123456789abcdef","open_time":1799999990000"#),
        ("ord", r#""t":"ord","snap":3,"ticket":123456790,"symbol":"EURUSD","side":"sell","kind":"limit","volume":0.10000000,"price":1.10500000,"sl":1.10700000,"tp":1.10100000,"magic":20260115,"comment":"A0123456789abcdee","setup_time":1799999980000"#),
        ("snap_end", r#""t":"snap_end","snap":3,"positions":1,"orders":1"#),
        ("tick", r#""t":"tick","symbol":"EURUSD","time":1799999999500,"bid":1.10000,"ask":1.10010"#),
        ("bar", r#""t":"bar","symbol":"EURUSD","tf":"M15","time":1799999100000,"open":1.10000,"high":1.10050,"low":1.09980,"close":1.10020,"volume":345,"spread":9.0,"last":false"#),
        ("result", r#""t":"result","cid":"A0123456789abcdef","ok":true,"retcode":10009,"ticket":123456789,"price":1.10012000,"volume":0.23000000,"msg":"Request executed""#),
        ("deal", r#""t":"deal","deal":987654321,"position":123456789,"symbol":"EURUSD","side":"buy","volume":0.23000000,"price":1.10410000,"profit":91.54,"swap":0.00,"commission":-1.61,"time":1799999999900,"comment":"[tp 1.10410]","origin":"A0123456789abcdef","fee":-0.05"#),
        ("balance_op", r#""t":"balance_op","deal":987654322,"amount":500.00,"time":1799999999950"#),
        ("heartbeat", r#""t":"heartbeat""#),
        ("bye", r#""t":"bye","reason":"connector stopped""#),
    ];
    let key = derive_session_key(PSK, "client-nonce-0001", "server-nonce-0001");
    let mut session = Session::new("sid-abc".into(), key.clone(), 30_000);
    for (i, (kind, fields)) in bodies.iter().enumerate() {
        let line = mql5_frame(&key, i as u64 + 1, "sid-abc", fields);
        let env = session.open(&line, NOW).unwrap_or_else(|e| panic!("`{kind}` as produced by the connector is rejected: {e}"));
        assert_eq!(env.body.kind(), *kind);
    }
    // Spot-check that values survive the fixed-decimal formatting.
    let line = mql5_frame(&key, 1, "sid-x", bodies[0].1);
    let mut s2 = Session::new("sid-x".into(), key.clone(), 30_000);
    let spec = s2.open(&line, NOW).unwrap().body.to_symbol_spec().unwrap();
    assert_eq!((spec.digits, spec.point, spec.tick_value, spec.swap_long), (5, 0.00001, 1.0, -6.5));
    assert_eq!(spec.trade_mode, SymbolTradeMode::Full);
    assert_eq!((spec.currency_base.as_str(), spec.freeze_level_points), ("EUR", 5.0), "protocol 1.1 fields");
}

#[test]
fn the_connector_hello_opens_a_session() {
    let hello = r#""t":"hello","nonce":"0123456789abcdef0123456789abcdef","terminal":"mt5","login":5001234,"server":"Broker-Demo","trade_mode":"demo","build":"AegisConnector 1.00 / MT5 build 4410""#;
    let json = format!("{{\"v\":1,\"id\":\"hello-0123456789abcdef0123456789abcdef\",\"ts\":{NOW},\"seq\":0,\"sid\":\"\",{hello}}}");
    let line = format!("{} {}", hmac_hex(PSK, json.as_bytes()), json);
    let mut nonces = NonceCache::new(30_000);
    let (welcome, _session, info) = server_accept(PSK, &line, &mut nonces, 30_000, NOW).unwrap();
    assert_eq!((info.terminal, info.login, info.trade_mode), (TerminalKind::Mt5, 5_001_234, AccountTradeMode::Demo));
    // The welcome carries what the connector reads: "t", "nonce" and "sid", as flat strings.
    let v: serde_json::Value = serde_json::from_str(&welcome[65..]).unwrap();
    assert_eq!(v["t"], "welcome");
    assert!(v["nonce"].is_string() && v["sid"].as_str().is_some_and(|s| !s.is_empty()));
}

#[test]
fn engine_instructions_carry_every_field_the_connector_reads() {
    let key = derive_session_key(PSK, "a-client-nonce-01", "a-server-nonce-01");
    let mut s = Session::new("sid-1".into(), key, 30_000);
    let order = Body::Order {
        cid: "A0123456789abcdef".into(), auth_id: "auth-1".into(), action: OrderAction::Open, symbol: "EURUSD".into(), side: Side::Buy,
        kind: OrderKind::Market, volume: 0.23, price: 1.1001, sl: 1.0981, tp: 1.1041, deviation: 10.0, ticket: 0, expires: NOW + 3_000, magic: 20_260_115,
    };
    let line = s.seal(order, NOW).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line[65..]).unwrap();
    // HandleOrder() in AegisConnector.mq5 reads exactly these keys.
    for (k, string) in [("cid", true), ("action", true), ("symbol", true), ("side", true), ("kind", true), ("volume", false), ("price", false),
        ("sl", false), ("tp", false), ("deviation", false), ("ticket", false), ("expires", false), ("magic", false)] {
        assert!(v.get(k).is_some(), "order lacks `{k}`");
        assert_eq!(v[k].is_string(), string, "`{k}` has the wrong JSON type");
    }
    assert_eq!((v["action"].as_str(), v["side"].as_str(), v["kind"].as_str()), (Some("open"), Some("buy"), Some("market")));
    for (action, name) in [(OrderAction::Close, "close"), (OrderAction::Modify, "modify"), (OrderAction::Cancel, "cancel")] {
        let b = Body::Order { cid: "c".into(), auth_id: "a".into(), action, symbol: "EURUSD".into(), side: Side::Buy, kind: OrderKind::Market, volume: 0.1, price: 0.0, sl: 1.0, tp: 0.0, deviation: 0.0, ticket: 5, expires: NOW, magic: 1 };
        let line = s.seal(b, NOW).unwrap();
        assert!(line.contains(&format!("\"action\":\"{name}\"")));
    }
    let line = s.seal(Body::HistoryReq { symbol: "EURUSD".into(), tf: "M15".into(), count: 500 }, NOW).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line[65..]).unwrap();
    assert_eq!((v["t"].as_str(), v["symbol"].as_str(), v["tf"].as_str(), v["count"].as_u64()), (Some("history_req"), Some("EURUSD"), Some("M15"), Some(500)));
    // Envelope fields read by HandleFrame().
    for k in ["v", "sid", "seq", "ts", "t"] {
        assert!(v.get(k).is_some());
    }
    // Everything the engine sends is flat, as the connector's reader requires.
    assert!(v.as_object().unwrap().values().all(|x| !x.is_object() && !x.is_array()));
}
