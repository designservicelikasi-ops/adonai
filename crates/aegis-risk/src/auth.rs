//! Order authorizations: short-lived, single-use, MAC-protected tokens.
//! Only the risk engine can mint one; the execution layer refuses anything else.

use aegis_types::{OrderKind, Side, TimestampMs};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Authorization {
    pub auth_id: String,
    pub decision_id: String,
    pub symbol: String,
    pub side: Side,
    pub kind: OrderKind,
    pub volume: f64,
    /// Expected fill price (market) or order price (pending).
    pub price: f64,
    pub sl: f64,
    pub tp: f64,
    pub max_slippage_points: f64,
    /// Money at risk if the stop-loss is hit, as computed by the risk engine.
    pub risk_amount: f64,
    pub issued_ms: TimestampMs,
    pub expires_ms: TimestampMs,
    pub mac: String,
}

impl Authorization {
    fn canonical(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{:.8}|{:.8}|{:.8}|{:.8}|{:.4}|{:.4}|{}|{}",
            self.auth_id,
            self.decision_id,
            self.symbol,
            self.side.as_str(),
            self.kind.as_str(),
            self.volume,
            self.price,
            self.sl,
            self.tp,
            self.max_slippage_points,
            self.risk_amount,
            self.issued_ms,
            self.expires_ms
        )
    }

    pub(crate) fn sign(&mut self, key: &[u8]) {
        let mut m = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
        m.update(self.canonical().as_bytes());
        self.mac = hex::encode(m.finalize().into_bytes());
    }

    pub(crate) fn mac_is_valid(&self, key: &[u8]) -> bool {
        let Ok(given) = hex::decode(&self.mac) else { return false };
        let mut m = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
        m.update(self.canonical().as_bytes());
        m.verify_slice(&given).is_ok()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum AuthError {
    #[error("authorization signature is invalid")]
    BadSignature,
    #[error("authorization expired")]
    Expired,
    #[error("authorization was already used")]
    AlreadyUsed,
    #[error("emergency stop is active")]
    KillSwitchActive,
}
