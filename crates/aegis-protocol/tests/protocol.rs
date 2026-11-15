use aegis_protocol::*;
use aegis_types::*;

const PSK: &[u8] = b"0123456789abcdef0123456789abcdef";
const NOW: i64 = 1_800_000_000_000;

fn tick() -> Body {
    Body::Tick { symbol: "EURUSD".into(), time: NOW, bid: 1.1, ask: 1.1001 }
}

fn order() -> Body {
    Body::Order {
        cid: "cid-1".into(),
        auth_id: "auth-1".into(),
        action: OrderAction::Open,
        symbol: "EURUSD".into(),
        side: Side::Buy,
        kind: OrderKind::Market,
        volume: 0.1,
        price: 1.1001,
        sl: 1.09,
        tp: 1.12,
        deviation: 10.0,
        ticket: 0,
        expires: NOW + 3_000,
        magic: 20260001,
    }
}

fn pair() -> (Session, Session) {
    let key = derive_session_key(PSK, "client-nonce-0001", "server-nonce-0001");
    (Session::new("sid-1".into(), key.clone(), 30_000), Session::new("sid-1".into(), key, 30_000))
}

#[test]
fn roundtrip_and_wire_shape() {
    let (mut a, mut b) = pair();
    let line = a.seal(tick(), NOW).unwrap();
    assert_eq!(line.as_bytes()[64], b' ');
    assert!(line[65..].starts_with('{') && !line.contains('\n'));
    // The payload is a flat JSON object: no nested object or array.
    let v: serde_json::Value = serde_json::from_str(&line[65..]).unwrap();
    assert!(v.as_object().unwrap().values().all(|x| !x.is_object() && !x.is_array()));
    assert_eq!(v["t"], "tick");
    let env = b.open(&line, NOW + 5).unwrap();
    assert_eq!(env.body, tick());
    assert_eq!(env.seq, 1);
}

#[test]
fn every_message_type_roundtrips() {
    let bodies = vec![
        Body::Hello { nonce: new_nonce(), terminal: TerminalKind::Mt5, login: 1, server: "S".into(), trade_mode: AccountTradeMode::Demo, build: "1".into() },
        Body::Welcome { nonce: new_nonce() },
        Body::Heartbeat {},
        Body::Bye { reason: "x".into() },
        Body::Error { code: "c".into(), msg: "m".into() },
        Body::Spec { symbol: "EURUSD".into(), digits: 5, point: 0.00001, contract_size: 100000.0, tick_size: 0.00001, tick_value: 1.0, volume_min: 0.01, volume_max: 100.0, volume_step: 0.01, stops_level: 0.0, swap_long: -1.0, swap_short: 0.5, trade_mode: SymbolTradeMode::Full, currency_profit: "USD".into() },
        tick(),
        Body::Account { login: 1, server: "S".into(), currency: "USD".into(), balance: 1.0, equity: 1.0, margin: 0.0, margin_free: 1.0, leverage: 100, trade_mode: AccountTradeMode::Demo, trade_allowed: true },
        Body::Pos { snap: 1, ticket: 5, symbol: "EURUSD".into(), side: Side::Sell, volume: 0.1, price_open: 1.1, sl: 0.0, tp: 0.0, profit: -1.0, swap: 0.0, commission: -0.3, magic: 7, comment: "c".into(), open_time: NOW },
        Body::Ord { snap: 1, ticket: 6, symbol: "EURUSD".into(), side: Side::Buy, kind: OrderKind::Limit, volume: 0.1, price: 1.09, sl: 1.08, tp: 1.1, magic: 7, comment: "".into(), setup_time: NOW },
        Body::SnapEnd { snap: 1, positions: 1, orders: 1 },
        Body::Deal { deal: 9, position: 5, symbol: "EURUSD".into(), side: Side::Sell, volume: 0.1, price: 1.1, profit: 3.0, swap: 0.0, commission: -0.3, time: NOW, comment: "tp".into(), origin: "Acid".into() },
        Body::BalanceOp { deal: 10, amount: 100.0, time: NOW },
        Body::Result { cid: "cid-1".into(), ok: true, retcode: 10009, ticket: 5, price: 1.1, volume: 0.1, msg: "done".into() },
        Body::Bar { symbol: "EURUSD".into(), tf: "M15".into(), time: NOW, open: 1.1, high: 1.2, low: 1.0, close: 1.15, volume: 3, spread: 12.0, last: true },
        order(),
        Body::HistoryReq { symbol: "EURUSD".into(), tf: "M15".into(), count: 500 },
    ];
    let (mut a, mut b) = pair();
    for body in bodies {
        let line = a.seal(body.clone(), NOW).unwrap();
        assert_eq!(b.open(&line, NOW).unwrap().body, body, "{}", body.kind());
    }
}

#[test]
fn unauthenticated_and_tampered_frames_are_rejected() {
    let (mut a, mut b) = pair();
    let line = a.seal(order(), NOW).unwrap();

    // Flip the volume inside the payload: the MAC no longer matches.
    let tampered = line.replace("\"volume\":0.1", "\"volume\":9.9");
    assert_ne!(tampered, line);
    assert_eq!(b.open(&tampered, NOW), Err(ProtoError::BadMac));

    // Wrong key (an attacker without the secret).
    let mut evil = Session::new("sid-1".into(), b"another-key-another-key-another!".to_vec(), 30_000);
    let forged = evil.seal(order(), NOW).unwrap();
    assert_eq!(b.open(&forged, NOW), Err(ProtoError::BadMac));

    // No MAC at all / garbage.
    assert_eq!(b.open(&line[65..], NOW), Err(ProtoError::Malformed));
    assert_eq!(b.open("hello", NOW), Err(ProtoError::Malformed));
    assert_eq!(b.open("", NOW), Err(ProtoError::Malformed));
    let not_hex = format!("{} {}", "z".repeat(64), &line[65..]);
    assert_eq!(b.open(&not_hex, NOW), Err(ProtoError::Malformed));

    // After all these attacks the genuine frame is still accepted exactly once.
    assert!(b.open(&line, NOW).is_ok());
}

#[test]
fn replayed_duplicated_and_reordered_frames_are_rejected() {
    let (mut a, mut b) = pair();
    let first = a.seal(order(), NOW).unwrap();
    let second = a.seal(tick(), NOW).unwrap();
    let third = a.seal(tick(), NOW).unwrap();

    assert!(b.open(&first, NOW).is_ok());
    // Replay of the same order instruction.
    assert_eq!(b.open(&first, NOW), Err(ProtoError::Sequence { expected: 2, got: 1 }));
    // A gap (frame lost or reordered).
    assert_eq!(b.open(&third, NOW), Err(ProtoError::Sequence { expected: 2, got: 3 }));
    assert!(b.open(&second, NOW).is_ok());
    assert!(b.open(&third, NOW).is_ok());
}

#[test]
fn frames_from_another_session_cannot_be_replayed() {
    let (mut a, _) = pair();
    let captured = a.seal(order(), NOW).unwrap();

    // New session after a reconnect: fresh nonces => fresh key.
    let key2 = derive_session_key(PSK, "client-nonce-0002", "server-nonce-0002");
    let mut b2 = Session::new("sid-2".into(), key2, 30_000);
    assert_eq!(b2.open(&captured, NOW), Err(ProtoError::BadMac));

    // Same key but another session id is refused as well.
    let key = derive_session_key(PSK, "client-nonce-0001", "server-nonce-0001");
    let mut b3 = Session::new("sid-3".into(), key, 30_000);
    assert_eq!(b3.open(&captured, NOW), Err(ProtoError::WrongSession));
}

#[test]
fn stale_or_future_frames_are_rejected() {
    let (mut a, mut b) = pair();
    let line = a.seal(tick(), NOW).unwrap();
    assert_eq!(b.open(&line, NOW + 31_000), Err(ProtoError::Stale(31_000)));
    assert_eq!(b.open(&line, NOW - 31_000), Err(ProtoError::Stale(-31_000)));
    assert!(b.open(&line, NOW + 29_000).is_ok());
}

#[test]
fn handshake_nonce_cache_blocks_replayed_hello() {
    let mut cache = NonceCache::new(30_000);
    let n = new_nonce();
    assert!(cache.check_hello(&n, NOW, NOW + 10).is_ok());
    assert_eq!(cache.check_hello(&n, NOW, NOW + 20), Err(ProtoError::NonceReplay));
    assert_eq!(cache.check_hello(&new_nonce(), NOW - 60_000, NOW), Err(ProtoError::Stale(60_000)));
    assert_eq!(cache.check_hello("short", NOW, NOW), Err(ProtoError::NonceReplay));
    assert!(cache.check_hello(&new_nonce(), NOW, NOW).is_ok());
}

#[test]
fn version_size_and_content_validation() {
    // Unsupported version.
    let env = Envelope { v: 99, id: "x".into(), ts: NOW, seq: 1, sid: "s".into(), body: tick() };
    let line = encode(&env, PSK).unwrap();
    assert_eq!(decode(&line, PSK), Err(ProtoError::Version(99)));

    // Oversized frames, both directions.
    let big = Envelope { v: 1, id: "x".into(), ts: NOW, seq: 1, sid: "s".into(), body: Body::Bye { reason: "r".repeat(20_000) } };
    assert_eq!(encode(&big, PSK), Err(ProtoError::TooLarge));
    assert_eq!(decode(&"a".repeat(MAX_FRAME_BYTES + 1), PSK), Err(ProtoError::TooLarge));

    // Authentic but invalid content is still refused.
    let bad = [
        Body::Tick { symbol: "EURUSD".into(), time: NOW, bid: 1.2, ask: 1.1 },
        Body::Tick { symbol: "EUR USD;DROP".into(), time: NOW, bid: 1.1, ask: 1.2 },
        Body::Tick { symbol: "EURUSD".into(), time: NOW, bid: -1.0, ask: 1.2 },
        Body::Bar { symbol: "EURUSD".into(), tf: "M7".into(), time: NOW, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1, spread: 0.0, last: false },
        Body::Result { cid: "".into(), ok: true, retcode: 0, ticket: 0, price: 0.0, volume: 0.0, msg: "".into() },
        Body::HistoryReq { symbol: "EURUSD".into(), tf: "M1".into(), count: 0 },
    ];
    for body in bad {
        let env = Envelope { v: 1, id: "x".into(), ts: NOW, seq: 1, sid: "s".into(), body: body.clone() };
        let line = encode(&env, PSK).unwrap();
        assert!(matches!(decode(&line, PSK), Err(ProtoError::Invalid(_))), "{body:?}");
    }

    // An entry instruction without stop-loss is structurally invalid.
    let mut no_sl = order();
    if let Body::Order { sl, .. } = &mut no_sl {
        *sl = 0.0;
    }
    assert!(no_sl.validate().is_err());

    // Unknown message type / missing field.
    let raw = r#"{"v":1,"id":"x","ts":1,"seq":1,"sid":"s","t":"format_disk"}"#;
    let mut session = Session::new("s".into(), PSK.to_vec(), 30_000);
    let _ = &mut session;
    let line = format!("{} {}", hmac_for_test(raw), raw);
    assert!(matches!(decode(&line, PSK), Err(ProtoError::Invalid(_))));
    let raw = r#"{"v":1,"id":"x","ts":1,"seq":1,"sid":"s","t":"tick","symbol":"EURUSD"}"#;
    let line = format!("{} {}", hmac_for_test(raw), raw);
    assert!(matches!(decode(&line, PSK), Err(ProtoError::Invalid(_))));

    assert_eq!(check_key(b"short"), Err(ProtoError::WeakKey));
    assert!(check_key(PSK).is_ok());
}

fn hmac_for_test(json: &str) -> String {
    use hmac::{Hmac, Mac};
    let mut m = Hmac::<sha2::Sha256>::new_from_slice(PSK).unwrap();
    m.update(json.as_bytes());
    hex::encode(m.finalize().into_bytes())
}

#[test]
fn hmac_matches_rfc4231_test_vector() {
    // RFC 4231 test case 2 — guards the reference the MQL5 implementation must match.
    let key = b"Jefe";
    let env_json = "what do ya want for nothing?";
    use hmac::{Hmac, Mac};
    let mut m = Hmac::<sha2::Sha256>::new_from_slice(key).unwrap();
    m.update(env_json.as_bytes());
    assert_eq!(
        hex::encode(m.finalize().into_bytes()),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}

#[test]
fn domain_conversions_roundtrip() {
    let spec = SymbolSpec {
        symbol: "EURUSD".into(), digits: 5, point: 0.00001, contract_size: 100000.0, tick_size: 0.00001,
        tick_value: 1.0, volume_min: 0.01, volume_max: 100.0, volume_step: 0.01, stops_level_points: 5.0,
        swap_long: -1.0, swap_short: 0.5, trade_mode: SymbolTradeMode::Full, currency_profit: "USD".into(),
    };
    assert_eq!(Body::from_symbol_spec(&spec).to_symbol_spec().unwrap(), spec);
    assert!(tick().to_symbol_spec().is_none());
}

fn info() -> HelloInfo {
    HelloInfo { terminal: TerminalKind::Mt5, login: 5001, server: "Demo".into(), trade_mode: AccountTradeMode::Demo, build: "test".into() }
}

#[test]
fn handshake_establishes_a_working_session() {
    let mut nonces = NonceCache::new(30_000);
    let (hello, client_nonce) = client_hello(PSK, &info(), NOW).unwrap();
    let (welcome, mut server, seen) = server_accept(PSK, &hello, &mut nonces, 30_000, NOW + 5).unwrap();
    assert_eq!(seen, info());
    let mut client = client_finish(PSK, &welcome, &client_nonce, 30_000, NOW + 10).unwrap();
    assert_eq!(client.sid, server.sid);

    let line = client.seal(tick(), NOW + 20).unwrap();
    assert_eq!(server.open(&line, NOW + 21).unwrap().body, tick());
    let line = server.seal(order(), NOW + 30).unwrap();
    assert_eq!(client.open(&line, NOW + 31).unwrap().body, order());
    // The session key is not the pre-shared key: session frames do not verify with it.
    assert_eq!(decode(&line, PSK), Err(ProtoError::BadMac));
}

#[test]
fn handshake_rejects_wrong_key_replay_and_stale_hello() {
    let mut nonces = NonceCache::new(30_000);
    // Connector with the wrong secret.
    let (bad, _) = client_hello(b"wrong-key-wrong-key-wrong-key-12", &info(), NOW).unwrap();
    assert_eq!(server_accept(PSK, &bad, &mut nonces, 30_000, NOW).err(), Some(ProtoError::BadMac));

    let (hello, client_nonce) = client_hello(PSK, &info(), NOW).unwrap();
    let (welcome, _, _) = server_accept(PSK, &hello, &mut nonces, 30_000, NOW).unwrap();
    // The very same hello captured and replayed.
    assert_eq!(server_accept(PSK, &hello, &mut nonces, 30_000, NOW + 100).err(), Some(ProtoError::NonceReplay));
    // A hello captured long ago.
    let (old, _) = client_hello(PSK, &info(), NOW - 120_000).unwrap();
    assert!(matches!(server_accept(PSK, &old, &mut nonces, 30_000, NOW).err(), Some(ProtoError::Stale(_))));
    // Anything but a hello cannot open a session.
    let env = Envelope { v: 1, id: "x".into(), ts: NOW, seq: 0, sid: String::new(), body: tick() };
    assert!(matches!(server_accept(PSK, &encode(&env, PSK).unwrap(), &mut nonces, 30_000, NOW).err(), Some(ProtoError::Invalid(_))));

    // A welcome forged without the secret is refused by the connector.
    let forged = encode(&Envelope { v: 1, id: "w".into(), ts: NOW, seq: 0, sid: "sid-x".into(), body: Body::Welcome { nonce: new_nonce() } }, b"attacker-key-attacker-key-123456").unwrap();
    assert_eq!(client_finish(PSK, &forged, &client_nonce, 30_000, NOW).err(), Some(ProtoError::BadMac));
    // An old welcome replayed to a new connection yields a key the engine does not share.
    let (hello2, nonce2) = client_hello(PSK, &info(), NOW + 200).unwrap();
    let (_, mut server2, _) = server_accept(PSK, &hello2, &mut nonces, 30_000, NOW + 200).unwrap();
    let mut fooled = client_finish(PSK, &welcome, &nonce2, 30_000, NOW + 200).unwrap();
    let line = fooled.seal(tick(), NOW + 201).unwrap();
    assert_eq!(server2.open(&line, NOW + 201).err(), Some(ProtoError::BadMac));
}
