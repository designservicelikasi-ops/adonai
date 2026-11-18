//! Client of the AEGIS control API.
//!
//! HTTPS connections accept exactly one certificate: the one whose SHA-256
//! fingerprint was paired (shown by `aegisd pairing`). No certificate authority
//! is trusted, so a self-signed certificate on a VPS is safe, and a different
//! certificate (man in the middle) is refused. Plain HTTP is only allowed to
//! the loopback address.

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Endpoint {
    /// `https://host:port` (or `http://127.0.0.1:port`).
    pub url: String,
    pub token: String,
    /// SHA-256 of the server certificate, hex with or without colons.
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClientError {
    /// Address or pairing problem detected before any request.
    Config(String),
    /// Network / TLS failure (includes a certificate that does not match the pairing).
    Transport(String),
    Unauthorized,
    /// The engine refused the request (status, message).
    Refused(u16, String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Config(m) => write!(f, "configuration: {m}"),
            ClientError::Transport(m) => write!(f, "connexion impossible: {m}"),
            ClientError::Unauthorized => write!(f, "jeton refusé par le moteur"),
            ClientError::Refused(code, m) => write!(f, "refusé ({code}): {m}"),
        }
    }
}

fn normalize_fp(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_hexdigit()).collect::<String>().to_ascii_lowercase()
}

pub fn fingerprint_hex(der: &[u8]) -> String {
    Sha256::digest(der).iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
}

#[derive(Debug)]
struct Pinned {
    expected: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(&self, end_entity: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        let fp = fingerprint_hex(end_entity.as_ref());
        if let Ok(mut s) = self.seen.lock() {
            *s = Some(fp.clone());
        }
        match &self.expected {
            Some(e) if normalize_fp(e) == normalize_fp(&fp) => Ok(ServerCertVerified::assertion()),
            // Probe mode: accept once to read the fingerprint; no request is ever sent with a token.
            None => Ok(ServerCertVerified::assertion()),
            Some(_) => Err(rustls::Error::General(format!("certificat inattendu (empreinte {fp}): appairage à refaire ou interception"))),
        }
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(m, c, d, &self.provider.signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(m, c, d, &self.provider.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

fn agent(expected: Option<String>) -> (ureq::Agent, Arc<Mutex<Option<String>>>) {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let seen = Arc::new(Mutex::new(None));
    let verifier = Arc::new(Pinned { expected, seen: seen.clone(), provider: provider.clone() });
    let tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    let a = ureq::AgentBuilder::new().tls_config(Arc::new(tls)).timeout_connect(Duration::from_secs(5)).timeout(Duration::from_secs(30)).build();
    (a, seen)
}

fn is_loopback_http(url: &str) -> bool {
    url.starts_with("http://127.0.0.1:") || url.starts_with("http://localhost:") || url.starts_with("http://[::1]:")
}

/// Read the certificate fingerprint of a server (first pairing). No token is sent.
/// The user must compare it with the one printed by `aegisd pairing` on the server.
pub fn probe_fingerprint(url: &str) -> Result<String, ClientError> {
    if !url.starts_with("https://") {
        return Err(ClientError::Config("l'empreinte n'existe que pour une adresse https://".into()));
    }
    let (a, seen) = agent(None);
    let _ = a.get(&format!("{}/api/v1/health", url.trim_end_matches('/'))).call();
    let fp = seen.lock().ok().and_then(|s| s.clone());
    fp.ok_or_else(|| ClientError::Transport("aucun certificat reçu".into()))
}

pub struct Client {
    agent: ureq::Agent,
    base: String,
    token: String,
}

impl Client {
    pub fn new(ep: &Endpoint) -> Result<Client, ClientError> {
        let base = ep.url.trim_end_matches('/').to_string();
        if ep.token.len() < 24 {
            return Err(ClientError::Config("jeton trop court".into()));
        }
        if base.starts_with("https://") {
            if normalize_fp(&ep.fingerprint).len() != 64 {
                return Err(ClientError::Config("empreinte SHA-256 du certificat requise (64 caractères hexadécimaux)".into()));
            }
            Ok(Client { agent: agent(Some(ep.fingerprint.clone())).0, base, token: ep.token.clone() })
        } else if is_loopback_http(&base) {
            Ok(Client { agent: ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build(), base, token: ep.token.clone() })
        } else {
            Err(ClientError::Config("http:// sans chiffrement n'est accepté que vers 127.0.0.1".into()))
        }
    }

    fn finish(r: Result<ureq::Response, ureq::Error>) -> Result<Value, ClientError> {
        match r {
            Ok(resp) => resp.into_json::<Value>().map_err(|e| ClientError::Transport(e.to_string())),
            Err(ureq::Error::Status(401, _)) => Err(ClientError::Unauthorized),
            Err(ureq::Error::Status(code, resp)) => {
                let body: Value = resp.into_json().unwrap_or(Value::Null);
                let msg = body.get("error").and_then(Value::as_str).unwrap_or("").to_string();
                Err(ClientError::Refused(code, msg))
            }
            Err(e) => Err(ClientError::Transport(e.to_string())),
        }
    }

    pub fn health(&self) -> Result<Value, ClientError> {
        Self::finish(self.agent.get(&format!("{}/api/v1/health", self.base)).call())
    }

    pub fn get(&self, path: &str) -> Result<Value, ClientError> {
        Self::finish(self.agent.get(&format!("{}{path}", self.base)).set("Authorization", &format!("Bearer {}", self.token)).call())
    }

    pub fn post(&self, path: &str, body: &Value) -> Result<Value, ClientError> {
        Self::finish(self.agent.post(&format!("{}{path}", self.base)).set("Authorization", &format!("Bearer {}", self.token)).send_json(body.clone()))
    }

    /// Send an engine command (`{"type": ..., "args": {...}}`); returns the engine's message.
    pub fn command(&self, cmd: &Value) -> Result<String, ClientError> {
        let v = self.post("/api/v1/command", cmd)?;
        Ok(v.get("message").and_then(Value::as_str).unwrap_or("").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_unsafe_endpoints() {
        let ep = |url: &str, fp: &str| Endpoint { url: url.into(), token: "t".repeat(32), fingerprint: fp.into() };
        assert!(matches!(Client::new(&ep("http://203.0.113.7:7879", "")), Err(ClientError::Config(_))), "no plain http over the network");
        assert!(matches!(Client::new(&ep("https://203.0.113.7:7879", "abc")), Err(ClientError::Config(_))), "pinning is mandatory");
        assert!(Client::new(&ep("http://127.0.0.1:7879", "")).is_ok());
        assert!(Client::new(&ep("https://vps:7879", &"ab:".repeat(31).chars().chain("ab".chars()).collect::<String>())).is_ok());
        assert!(matches!(Client::new(&Endpoint { token: "short".into(), ..ep("http://127.0.0.1:1", "") }), Err(ClientError::Config(_))));
        assert_eq!(normalize_fp("AB:cd:01"), "abcd01");
    }
}
