//! TLS for the control API. On first start the engine creates a self-signed
//! certificate (ECDSA P-256) in `<data_dir>/tls/`; AEGIS Desktop pins its
//! SHA-256 fingerprint instead of trusting a certificate authority, which is
//! what makes a self-signed certificate safe on a VPS with no domain name.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub struct TlsFiles {
    pub cert: PathBuf,
    pub key: PathBuf,
    /// SHA-256 of the certificate (DER), lowercase hex with colons.
    pub fingerprint: String,
}

pub fn install_crypto_provider() {
    // Idempotent: a second install returns an error that is safe to ignore.
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn pem_to_der(pem: &str) -> Option<Vec<u8>> {
    let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    decode_base64(&body)
}

fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let bytes: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            acc |= val(*c)? << (18 - 6 * i);
        }
        let n = chunk.len();
        out.push((acc >> 16) as u8);
        if n > 2 {
            out.push((acc >> 8) as u8);
        }
        if n > 3 {
            out.push(acc as u8);
        }
    }
    Some(out)
}

pub fn fingerprint_of_der(der: &[u8]) -> String {
    let h = Sha256::digest(der);
    h.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
}

/// Load the certificate, or create it on first use. `names` are the host names /
/// addresses the desktop will use (e.g. the VPS address).
pub fn ensure(dir: &Path, names: &[String]) -> anyhow::Result<TlsFiles> {
    std::fs::create_dir_all(dir)?;
    let cert = dir.join("cert.pem");
    let key = dir.join("key.pem");
    if !cert.exists() || !key.exists() {
        let mut sans: Vec<String> = vec!["localhost".into(), "127.0.0.1".into()];
        sans.extend(names.iter().filter(|n| !n.is_empty()).cloned());
        sans.dedup();
        let ck = rcgen::generate_simple_self_signed(sans)?;
        std::fs::write(&cert, ck.cert.pem())?;
        write_private(&key, &ck.key_pair.serialize_pem())?;
    }
    let pem = std::fs::read_to_string(&cert)?;
    let der = pem_to_der(&pem).ok_or_else(|| anyhow::anyhow!("{}: not a PEM certificate", cert.display()))?;
    Ok(TlsFiles { fingerprint: fingerprint_of_der(&der), cert, key })
}

fn write_private(path: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)?.write_all(content.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_reference_values() {
        assert_eq!(decode_base64("TWFu").unwrap(), b"Man");
        assert_eq!(decode_base64("TWE=").unwrap(), b"Ma");
        assert_eq!(decode_base64("TQ==").unwrap(), b"M");
        assert!(decode_base64("T*==").is_none());
    }

    #[test]
    fn certificate_is_created_once_and_its_fingerprint_is_stable() {
        let dir = std::env::temp_dir().join(format!("aegis-tls-{}", aegis_types::new_id("t")));
        let a = ensure(&dir, &["203.0.113.7".into()]).unwrap();
        let b = ensure(&dir, &[]).unwrap();
        assert_eq!(a.fingerprint, b.fingerprint);
        assert_eq!(a.fingerprint.len(), 32 * 3 - 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&a.key).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
