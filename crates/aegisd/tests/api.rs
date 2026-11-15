use aegis_core::{start, EngineConfig};
use aegis_types::new_id;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TOKEN: &str = "test-token-0123456789-abcdefghij";

async fn http(addr: std::net::SocketAddr, method: &str, path: &str, token: Option<&str>, body: &str) -> (u16, String) {
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{auth}\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    let status: u16 = out.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = out.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn api_requires_the_token_and_drives_the_engine() {
    let dir = std::env::temp_dir().join(format!("aegis-api-{}", new_id("t")));
    let mut cfg = EngineConfig::default();
    cfg.engine.data_dir = dir;
    cfg.bridge.listen = "127.0.0.1:0".into();
    cfg.bridge.shared_key_env = "AEGIS_API_TEST_KEY_NOT_SET".into();
    let engine = Arc::new(start(cfg).await.unwrap());

    // A short token is refused outright.
    assert!(aegisd::router(engine.clone(), "short".into()).is_err());
    let app = aegisd::router(engine.clone(), TOKEN.into()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    assert_eq!(http(addr, "GET", "/api/v1/health", None, "").await.0, 200);
    // No token, wrong token, token of another length.
    assert_eq!(http(addr, "GET", "/api/v1/state", None, "").await.0, 401);
    assert_eq!(http(addr, "GET", "/api/v1/state", Some("test-token-0123456789-abcdefghiX"), "").await.0, 401);
    assert_eq!(http(addr, "POST", "/api/v1/kill", Some("nope"), "{}").await.0, 401);
    assert!(!engine.snapshot().kill.active, "an unauthenticated request changes nothing");

    let (status, body) = http(addr, "GET", "/api/v1/state", Some(TOKEN), "").await;
    assert_eq!(status, 200);
    let state: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(state["mode"], "monitor");
    assert_eq!(state["live_trading_enabled_in_config"], false);

    // Switching to an automatic mode without a terminal is refused by the engine.
    let (status, body) = http(addr, "POST", "/api/v1/mode", Some(TOKEN), r#"{"mode":"demo_auto"}"#).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(http(addr, "POST", "/api/v1/mode", Some(TOKEN), r#"{"mode":"live_auto","confirmation":"x"}"#).await.0, 409);

    let (status, _) = http(addr, "POST", "/api/v1/kill", Some(TOKEN), r#"{"reason":"api test"}"#).await;
    assert_eq!(status, 200);
    assert!(engine.snapshot().kill.active);
    assert_eq!(http(addr, "POST", "/api/v1/kill/reset", Some(TOKEN), r#"{"phrase":"wrong"}"#).await.0, 409);
    assert_eq!(http(addr, "POST", "/api/v1/kill/reset", Some(TOKEN), r#"{"phrase":"RESET AEGIS"}"#).await.0, 200);
    assert!(!engine.snapshot().kill.active);
}
