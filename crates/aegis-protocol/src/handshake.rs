//! Handshake, as pure functions (the MQL5 connector mirrors the client side).
//!
//! 1. connector -> engine : `hello`   (MAC = pre-shared key, carries a client nonce)
//! 2. engine -> connector : `welcome` (MAC = pre-shared key, carries the session id and a server nonce)
//! 3. both derive the session key from the two nonces; every later frame uses it.

use crate::{decode, derive_session_key, encode, new_nonce, Body, Envelope, NonceCache, ProtoError, Session, PROTOCOL_VERSION};
use aegis_types::{new_id, AccountTradeMode, TerminalKind, TimestampMs};

/// Identity announced by the connector in its `hello`.
#[derive(Debug, Clone, PartialEq)]
pub struct HelloInfo {
    pub terminal: TerminalKind,
    pub login: i64,
    pub server: String,
    pub trade_mode: AccountTradeMode,
    pub build: String,
}

/// Connector side, step 1. Returns the wire line and the client nonce to keep.
pub fn client_hello(psk: &[u8], info: &HelloInfo, now: TimestampMs) -> Result<(String, String), ProtoError> {
    let nonce = new_nonce();
    let env = Envelope {
        v: PROTOCOL_VERSION,
        id: new_id("hello"),
        ts: now,
        seq: 0,
        sid: String::new(),
        body: Body::Hello {
            nonce: nonce.clone(),
            terminal: info.terminal,
            login: info.login,
            server: info.server.clone(),
            trade_mode: info.trade_mode,
            build: info.build.clone(),
        },
    };
    Ok((encode(&env, psk)?, nonce))
}

/// Engine side: authenticate a `hello`, open a session and produce the `welcome` line.
pub fn server_accept(
    psk: &[u8],
    hello_line: &str,
    nonces: &mut NonceCache,
    max_skew_ms: i64,
    now: TimestampMs,
) -> Result<(String, Session, HelloInfo), ProtoError> {
    let env = decode(hello_line, psk)?;
    let Body::Hello { nonce, terminal, login, server, trade_mode, build } = env.body else {
        return Err(ProtoError::Invalid("expected hello".into()));
    };
    if env.seq != 0 || !env.sid.is_empty() {
        return Err(ProtoError::Invalid("hello must open a new session".into()));
    }
    nonces.check_hello(&nonce, env.ts, now)?;
    let sid = new_id("sid");
    let server_nonce = new_nonce();
    let welcome = Envelope {
        v: PROTOCOL_VERSION,
        id: new_id("welcome"),
        ts: now,
        seq: 0,
        sid: sid.clone(),
        body: Body::Welcome { nonce: server_nonce.clone() },
    };
    let line = encode(&welcome, psk)?;
    let session = Session::new(sid, derive_session_key(psk, &nonce, &server_nonce), max_skew_ms);
    Ok((line, session, HelloInfo { terminal, login, server, trade_mode, build }))
}

/// Connector side, step 2: authenticate the `welcome` and open the session.
pub fn client_finish(psk: &[u8], welcome_line: &str, client_nonce: &str, max_skew_ms: i64, now: TimestampMs) -> Result<Session, ProtoError> {
    let env = decode(welcome_line, psk)?;
    let Body::Welcome { nonce } = env.body else {
        return Err(ProtoError::Invalid("expected welcome".into()));
    };
    if env.sid.is_empty() || env.seq != 0 {
        return Err(ProtoError::Invalid("welcome must carry a session id".into()));
    }
    let skew = now - env.ts;
    if skew.abs() > max_skew_ms {
        return Err(ProtoError::Stale(skew));
    }
    Ok(Session::new(env.sid, derive_session_key(psk, client_nonce, &nonce), max_skew_ms))
}
