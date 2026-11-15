//! AEGIS PROTOCOL v1 — authenticated line protocol between the engine and the
//! MT5 connector. Full specification: docs/PROTOCOL.md.
//!
//! Wire format, one frame per line:
//!
//! ```text
//! <64 hex chars: HMAC-SHA256(key, json)> <space> <json object> \n
//! ```
//!
//! The JSON object is flat (no nesting) so that the MQL5 side needs only a tiny
//! parser. The `hello`/`welcome` handshake is authenticated with the pre-shared
//! key; every later frame uses a per-session key derived from both nonces, a
//! strictly increasing sequence number and a bounded timestamp, which together
//! defeat replays within and across sessions.

mod messages;
pub use messages::*;

use aegis_types::TimestampMs;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;

type HmacSha256 = Hmac<Sha256>;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 16 * 1024;
pub const MIN_KEY_BYTES: usize = 16;

#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum ProtoError {
    #[error("frame too large")]
    TooLarge,
    #[error("malformed frame")]
    Malformed,
    #[error("authentication failed")]
    BadMac,
    #[error("invalid message: {0}")]
    Invalid(String),
    #[error("unsupported protocol version {0}")]
    Version(u32),
    #[error("wrong session")]
    WrongSession,
    #[error("sequence error: expected {expected}, got {got} (replay or loss)")]
    Sequence { expected: u64, got: u64 },
    #[error("timestamp outside the accepted window ({0} ms skew)")]
    Stale(i64),
    #[error("nonce already used (replayed handshake)")]
    NonceReplay,
    #[error("shared key too short")]
    WeakKey,
}

fn mac_hex(key: &[u8], data: &[u8]) -> String {
    let mut m = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    m.update(data);
    hex::encode(m.finalize().into_bytes())
}

/// Serialize and authenticate one envelope into a wire line (without the trailing newline).
pub fn encode(env: &Envelope, key: &[u8]) -> Result<String, ProtoError> {
    let json = serde_json::to_string(env).map_err(|e| ProtoError::Invalid(e.to_string()))?;
    if json.len() + 65 > MAX_FRAME_BYTES {
        return Err(ProtoError::TooLarge);
    }
    Ok(format!("{} {}", mac_hex(key, json.as_bytes()), json))
}

/// Authenticate, parse and validate one wire line. The MAC is checked before
/// anything is parsed.
pub fn decode(line: &str, key: &[u8]) -> Result<Envelope, ProtoError> {
    let line = line.trim_end_matches(['\r', '\n']);
    if line.len() > MAX_FRAME_BYTES {
        return Err(ProtoError::TooLarge);
    }
    if line.len() < 66 || !line.is_char_boundary(64) || line.as_bytes()[64] != b' ' {
        return Err(ProtoError::Malformed);
    }
    let (mac_part, json) = (&line[..64], &line[65..]);
    let given = hex::decode(mac_part).map_err(|_| ProtoError::Malformed)?;
    let mut m = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    m.update(json.as_bytes());
    m.verify_slice(&given).map_err(|_| ProtoError::BadMac)?;
    let env: Envelope = serde_json::from_str(json).map_err(|e| ProtoError::Invalid(e.to_string()))?;
    if env.v != PROTOCOL_VERSION {
        return Err(ProtoError::Version(env.v));
    }
    env.body.validate().map_err(ProtoError::Invalid)?;
    Ok(env)
}

pub fn check_key(key: &[u8]) -> Result<(), ProtoError> {
    if key.len() < MIN_KEY_BYTES {
        Err(ProtoError::WeakKey)
    } else {
        Ok(())
    }
}

pub fn new_nonce() -> String {
    hex::encode(rand::random::<[u8; 16]>())
}

/// Session key = HMAC(psk, "AEGIS1|<client_nonce>|<server_nonce>").
pub fn derive_session_key(psk: &[u8], client_nonce: &str, server_nonce: &str) -> Vec<u8> {
    let mut m = HmacSha256::new_from_slice(psk).expect("hmac accepts any key length");
    m.update(format!("AEGIS1|{client_nonce}|{server_nonce}").as_bytes());
    m.finalize().into_bytes().to_vec()
}

/// Remembers recently seen handshake nonces so a captured `hello` cannot be replayed.
pub struct NonceCache {
    seen: HashMap<String, TimestampMs>,
    window_ms: i64,
}

impl NonceCache {
    pub fn new(window_ms: i64) -> Self {
        NonceCache { seen: HashMap::new(), window_ms }
    }

    /// Validate a hello's timestamp and nonce.
    pub fn check_hello(&mut self, nonce: &str, ts: TimestampMs, now: TimestampMs) -> Result<(), ProtoError> {
        let skew = now - ts;
        if skew.abs() > self.window_ms {
            return Err(ProtoError::Stale(skew));
        }
        self.seen.retain(|_, t| now - *t <= self.window_ms * 2);
        if nonce.len() < 16 || self.seen.insert(nonce.to_string(), now).is_some() {
            return Err(ProtoError::NonceReplay);
        }
        Ok(())
    }
}

/// Per-connection replay protection and outgoing stamping (one per direction pair).
pub struct Session {
    pub sid: String,
    key: Vec<u8>,
    last_in: u64,
    next_out: u64,
    max_skew_ms: i64,
}

impl Session {
    pub fn new(sid: String, key: Vec<u8>, max_skew_ms: i64) -> Self {
        Session { sid, key, last_in: 0, next_out: 1, max_skew_ms }
    }

    /// Wrap a body into the next outgoing envelope and encode it.
    pub fn seal(&mut self, body: Body, now: TimestampMs) -> Result<String, ProtoError> {
        let env = Envelope {
            v: PROTOCOL_VERSION,
            id: format!("{}-{}", &self.sid[..self.sid.len().min(8)], self.next_out),
            ts: now,
            seq: self.next_out,
            sid: self.sid.clone(),
            body,
        };
        let line = encode(&env, &self.key)?;
        self.next_out += 1;
        Ok(line)
    }

    /// Decode an incoming line and enforce session id, strict sequence and freshness.
    pub fn open(&mut self, line: &str, now: TimestampMs) -> Result<Envelope, ProtoError> {
        let env = decode(line, &self.key)?;
        if env.sid != self.sid {
            return Err(ProtoError::WrongSession);
        }
        if env.seq != self.last_in + 1 {
            return Err(ProtoError::Sequence { expected: self.last_in + 1, got: env.seq });
        }
        let skew = now - env.ts;
        if skew.abs() > self.max_skew_ms {
            return Err(ProtoError::Stale(skew));
        }
        self.last_in = env.seq;
        Ok(env)
    }
}
