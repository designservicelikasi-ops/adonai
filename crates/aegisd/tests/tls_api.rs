//! The control API over real TLS, with the pinned client used by AEGIS Desktop.

use aegis_client::{probe_fingerprint, Client, ClientError, Endpoint};
use aegis_core::{start, EngineConfig};
use aegis_types::new_id;
use serde_json::json;
use std::sync::Arc;

const TOKEN: &str = "tls-test-token-0123456789-abcdef";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pinned_tls_api_end_to_end() {
    let dir = std::env::temp_dir().join(format!("aegis-tls-api-{}", new_id("t")));
    let mut cfg = EngineConfig::default();
    cfg.engine.data_dir = dir.clone();
    cfg.bridge.listen = "127.0.0.1:0".into();
    cfg.bridge.shared_key_env = "AEGIS_TLS_TEST_KEY_NOT_SET".into();
    cfg.news.enabled = false;
    let engine = Arc::new(start(cfg).await.unwrap());
    let app = aegisd::router_with(engine.clone(), aegisd::ApiOptions { token: TOKEN.into(), files_dir: dir.join("files"), monitor: None }).unwrap();
    aegisd::tls::install_crypto_provider();
    let files = aegisd::tls::ensure(&dir.join("tls"), &[]).unwrap();
    let rustls = axum_server::tls_rustls::RustlsConfig::from_pem_file(&files.cert, &files.key).await.unwrap();
    let handle = axum_server::Handle::new();
    let h2 = handle.clone();
    tokio::spawn(async move {
        axum_server::bind_rustls("127.0.0.1:0".parse().unwrap(), rustls).handle(h2).serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
    });
    let addr = handle.listening().await.unwrap();
    let url = format!("https://127.0.0.1:{}", addr.port());

    let fp = files.fingerprint.clone();
    let (u2, fp2) = (url.clone(), fp.clone());
    let checks = tokio::task::spawn_blocking(move || {
        // First pairing: the fingerprint read over the network matches the server's.
        assert_eq!(probe_fingerprint(&u2).unwrap(), fp2);
        let c = Client::new(&Endpoint { url: u2.clone(), token: TOKEN.into(), fingerprint: fp2.clone() }).unwrap();
        assert_eq!(c.health().unwrap()["status"], "ok");
        let s = c.get("/api/v1/summary").unwrap();
        assert_eq!(s["mode"], "monitor");
        assert!(s.get("journal").is_none(), "the summary stays small");
        assert!(c.get("/api/v1/view/performance").unwrap().get("all_time").is_some());
        assert!(matches!(c.get("/api/v1/view/nope"), Err(ClientError::Refused(404, _))));
        // Generic commands, validated by the engine.
        assert!(c.command(&json!({ "type": "verify_audit" })).unwrap().contains("intègre"));
        assert!(matches!(c.command(&json!({ "type": "set_mode", "args": { "mode": "demo_auto" } })), Err(ClientError::Refused(409, _))));
        assert!(matches!(c.command(&json!({ "type": "no_such_command" })), Err(ClientError::Refused(400, _))));
        // Network clients cannot stop the engine nor touch files outside the exchange folder.
        assert!(matches!(c.command(&json!({ "type": "shutdown" })), Err(ClientError::Refused(403, _))));
        let escape = json!({ "type": "export_dataset", "args": { "symbol": "X", "timeframe": "M5", "origin": "broker", "horizon": 4, "path": "../../etc/x.csv" } });
        assert!(matches!(c.command(&escape), Err(ClientError::Refused(403, _))));
        // A different certificate (interception) is refused before any token is sent.
        let wrong = Client::new(&Endpoint { url: u2.clone(), token: TOKEN.into(), fingerprint: "00".repeat(32) }).unwrap();
        assert!(matches!(wrong.health(), Err(ClientError::Transport(_))));
        // Brute force: after 10 failures from one address, even the right token waits a minute.
        let bad = Client::new(&Endpoint { url: u2.clone(), token: "x".repeat(32), fingerprint: fp2.clone() }).unwrap();
        for _ in 0..10 {
            assert!(matches!(bad.get("/api/v1/summary"), Err(ClientError::Unauthorized)));
        }
        assert!(matches!(c.get("/api/v1/summary"), Err(ClientError::Refused(429, _))));
    });
    checks.await.unwrap();
    handle.shutdown();
    engine.stop().await;
}
