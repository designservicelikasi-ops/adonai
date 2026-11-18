//! HTTP control API of the headless engine (used by AEGIS Desktop, locally or
//! on a VPS). Every route except `/api/v1/health` requires
//! `Authorization: Bearer <token>`. Repeated authentication failures from one
//! address are throttled. Commands that name server files are confined to the
//! exchange folder `<data_dir>/files/`.

pub mod daemon;
pub mod monitor;
pub mod service;
pub mod tls;

use aegis_core::{Command, EngineHandle};
use aegis_types::{now_ms, Mode, TimestampMs};
use axum::extract::{ConnectInfo, Path as UrlPath, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const MIN_TOKEN_LEN: usize = 24;
/// Failed authentications allowed per address and per minute.
pub const MAX_AUTH_FAILURES: u32 = 10;

#[derive(Clone)]
struct ApiState {
    engine: Arc<EngineHandle>,
    token: Arc<String>,
    files_dir: Arc<PathBuf>,
    failures: Arc<Mutex<HashMap<IpAddr, (u32, TimestampMs)>>>,
    monitor: Option<monitor::SystemMonitor>,
}

/// Compare without leaking the position of the first difference.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn peer(req: &Request) -> Option<IpAddr> {
    req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip())
}

async fn require_token(State(st): State<ApiState>, req: Request, next: Next) -> Response {
    let ip = peer(&req);
    let now = now_ms();
    if let Some(ip) = ip {
        let f = st.failures.lock().expect("lock");
        if let Some((n, since)) = f.get(&ip) {
            if *n >= MAX_AUTH_FAILURES && now - since < 60_000 {
                return (StatusCode::TOO_MANY_REQUESTS, Json(json!({ "error": "too many failed authentications, retry in a minute" }))).into_response();
            }
        }
    }
    let given = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("");
    if constant_time_eq(given.as_bytes(), st.token.as_bytes()) {
        if let Some(ip) = ip {
            st.failures.lock().expect("lock").remove(&ip);
        }
        next.run(req).await
    } else {
        if let Some(ip) = ip {
            let mut f = st.failures.lock().expect("lock");
            let e = f.entry(ip).or_insert((0, now));
            if now - e.1 >= 60_000 {
                *e = (0, now);
            }
            e.0 += 1;
        }
        tracing::warn!("API: authentication refused from {}", ip.map(|i| i.to_string()).unwrap_or_else(|| "?".into()));
        (StatusCode::UNAUTHORIZED, Json(json!({ "error": "unauthorized" }))).into_response()
    }
}

fn reply(r: Result<String, String>) -> Response {
    match r {
        Ok(message) => Json(json!({ "ok": true, "message": message })).into_response(),
        Err(error) => (StatusCode::CONFLICT, Json(json!({ "ok": false, "error": error }))).into_response(),
    }
}

/// A relative path inside the exchange folder, or an error. No `..`, no absolute path.
pub fn confine(files_dir: &Path, requested: &Path) -> Result<PathBuf, String> {
    if requested.as_os_str().is_empty() || requested.is_absolute() {
        return Err("chemin refusé: indiquer un nom relatif au dossier d'échange du serveur".into());
    }
    if requested.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err("chemin refusé: « .. » et les racines sont interdits".into());
    }
    Ok(files_dir.join(requested))
}

/// Rewrite or refuse a command received from the network.
fn sanitize(st: &ApiState, c: Command) -> Result<Command, String> {
    let dir = st.files_dir.as_path();
    Ok(match c {
        Command::Shutdown => return Err("l'arrêt du moteur se fait par le gestionnaire de service, pas par l'API".into()),
        Command::ImportCsv { path, symbol, timeframe, utc_offset_hours } => Command::ImportCsv { path: confine(dir, &path)?, symbol, timeframe, utc_offset_hours },
        Command::ExportDataset { symbol, timeframe, origin, horizon, path } => Command::ExportDataset { symbol, timeframe, origin, horizon, path: confine(dir, &path)? },
        Command::LoadModel { path } => Command::LoadModel { path: confine(dir, &path)? },
        other => other,
    })
}

#[derive(Deserialize)]
struct ModeBody {
    mode: Mode,
    #[serde(default)]
    confirmation: String,
}

#[derive(Deserialize)]
struct KillBody {
    #[serde(default)]
    reason: String,
}

#[derive(Deserialize)]
struct ResetBody {
    phrase: String,
}

/// Fields of the view sent to the dashboard every second (the rest is fetched per page).
const SUMMARY_KEYS: [&str; 27] = [
    "now_ms", "started_ms", "version", "mode", "live_armed", "live_trading_enabled_in_config", "connection", "bridge", "account",
    "markets", "positions", "position_decisions", "pending", "entry_states", "goal", "day", "peak_equity", "drawdown_pct",
    "floating_pnl", "daily_net", "daily_net_pct", "daily_halt", "kill", "health", "risk_config", "shadow_dropped", "alerts",
];

fn view_value(st: &ApiState) -> Value {
    serde_json::to_value(&*st.engine.snapshot()).unwrap_or(Value::Null)
}

fn summary(st: &ApiState) -> Value {
    let v = view_value(st);
    let mut out = serde_json::Map::new();
    for k in SUMMARY_KEYS {
        out.insert(k.into(), v.get(k).cloned().unwrap_or(Value::Null));
    }
    let take = |key: &str, n: usize| Value::Array(v.get(key).and_then(Value::as_array).map(|a| a.iter().take(n).cloned().collect()).unwrap_or_default());
    out.insert("decisions".into(), take("decisions", 20));
    out.insert("logs".into(), take("logs", 40));
    out.insert("deals".into(), take("deals", 20));
    let perf = v.get("performance").cloned().unwrap_or(Value::Null);
    out.insert("performance".into(), json!({ "today": perf.get("today"), "last_7_days": perf.get("last_7_days"), "all_time": perf.get("all_time") }));
    let news = v.get("news").cloned().unwrap_or(Value::Null);
    out.insert("news".into(), json!({ "enabled": news.get("enabled"), "feed_ok": news.get("feed_ok"), "last_error": news.get("last_error"), "by_symbol": news.get("by_symbol"), "upcoming": news.get("upcoming").and_then(Value::as_array).map(|a| a.iter().take(8).cloned().collect::<Vec<_>>()) }));
    let shadow = v.get("shadow").cloned().unwrap_or(Value::Null);
    out.insert("shadow".into(), if shadow.is_null() { Value::Null } else { json!({ "updated_ms": shadow.get("updated_ms"), "bars_processed": shadow.get("bars_processed"), "decisions_seen": shadow.get("decisions_seen"), "candidates": shadow.get("candidates").and_then(Value::as_array).map(Vec::len), "ready": shadow.get("candidates").and_then(Value::as_array).map(|a| a.iter().filter(|c| c["stage"] == "forward" && c["role"] == "challenger").count()) }) });
    out.insert("strategies".into(), Value::Array(v.get("strategies").and_then(Value::as_array).map(|a| a.iter().map(|s| json!({ "id": s["meta"]["id"], "name": s["meta"]["name"], "stage": s["stage"], "enabled": s["enabled"], "signals": s["signals"], "forward": s["forward"] })).collect()).unwrap_or_default()));
    if let Some(m) = &st.monitor {
        out.insert("system".into(), serde_json::to_value(m.snapshot()).unwrap_or(Value::Null));
    }
    Value::Object(out)
}

pub struct ApiOptions {
    pub token: String,
    /// Folder the network may read from / write to (`<data_dir>/files`).
    pub files_dir: PathBuf,
    pub monitor: Option<monitor::SystemMonitor>,
}

/// Router with default options: files confined to `<data_dir>/files`.
pub fn router(engine: Arc<EngineHandle>, token: String) -> anyhow::Result<Router> {
    let data_dir = PathBuf::from(&engine.snapshot().data.db_path).parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("data"));
    router_with(engine, ApiOptions { token, files_dir: data_dir.join("files"), monitor: None })
}

pub fn router_with(engine: Arc<EngineHandle>, opts: ApiOptions) -> anyhow::Result<Router> {
    if opts.token.len() < MIN_TOKEN_LEN {
        anyhow::bail!("the API token must contain at least {MIN_TOKEN_LEN} characters");
    }
    let _ = std::fs::create_dir_all(&opts.files_dir);
    let st = ApiState { engine, token: Arc::new(opts.token), files_dir: Arc::new(opts.files_dir), failures: Arc::new(Mutex::new(HashMap::new())), monitor: opts.monitor };
    let protected = Router::new()
        .route("/api/v1/state", get(|State(st): State<ApiState>| async move { Json((*st.engine.snapshot()).clone()) }))
        .route("/api/v1/summary", get(|State(st): State<ApiState>| async move { Json(summary(&st)) }))
        .route(
            "/api/v1/view/:section",
            get(|State(st): State<ApiState>, UrlPath(section): UrlPath<String>| async move {
                match view_value(&st).get(&section) {
                    Some(v) => Json(v.clone()).into_response(),
                    None => (StatusCode::NOT_FOUND, Json(json!({ "error": format!("unknown section `{section}`") }))).into_response(),
                }
            }),
        )
        .route(
            "/api/v1/system",
            get(|State(st): State<ApiState>| async move { Json(st.monitor.as_ref().map(|m| serde_json::to_value(m.snapshot()).unwrap_or(Value::Null)).unwrap_or(Value::Null)) }),
        )
        .route(
            "/api/v1/command",
            post(|State(st): State<ApiState>, body: axum::body::Bytes| async move {
                let c: Command = match serde_json::from_slice(&body) {
                    Ok(c) => c,
                    Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": format!("invalid command: {e}") }))).into_response(),
                };
                match sanitize(&st, c) {
                    Ok(c) => {
                        tracing::info!("API command: {}", serde_json::to_value(&c).ok().and_then(|v| v.get("type").cloned()).unwrap_or_default());
                        reply(st.engine.command(c).await)
                    }
                    Err(e) => (StatusCode::FORBIDDEN, Json(json!({ "ok": false, "error": e }))).into_response(),
                }
            }),
        )
        .route(
            "/api/v1/mode",
            post(|State(st): State<ApiState>, Json(b): Json<ModeBody>| async move { reply(st.engine.command(Command::SetMode { mode: b.mode, confirmation: b.confirmation }).await) }),
        )
        .route("/api/v1/kill", post(|State(st): State<ApiState>, Json(b): Json<KillBody>| async move { reply(st.engine.command(Command::Kill { reason: b.reason }).await) }))
        .route(
            "/api/v1/kill/reset",
            post(|State(st): State<ApiState>, Json(b): Json<ResetBody>| async move { reply(st.engine.command(Command::ResetKill { phrase: b.phrase }).await) }),
        )
        .route_layer(middleware::from_fn_with_state(st.clone(), require_token));
    let health = |State(st): State<ApiState>| async move {
        let v = st.engine.snapshot();
        let age = now_ms() - v.now_ms;
        let alive = age < 5_000;
        let status = if alive { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
        (status, Json(json!({ "status": if alive { "ok" } else { "stalled" }, "version": v.version, "view_age_ms": age, "mode": v.mode })))
    };
    Ok(Router::new().route("/api/v1/health", get(health)).merge(protected).with_state(st))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_paths_are_confined_to_the_exchange_folder() {
        let d = Path::new("/srv/aegis/files");
        assert_eq!(confine(d, Path::new("exports/a.csv")).unwrap(), PathBuf::from("/srv/aegis/files/exports/a.csv"));
        for bad in ["/etc/passwd", "../aegis.db", "a/../../b", "./x", ""] {
            assert!(confine(d, Path::new(bad)).is_err(), "{bad}");
        }
    }
}
