//! HTTP control API of the headless engine. Every route except `/health`
//! requires `Authorization: Bearer <token>`.

use aegis_core::{Command, EngineHandle};
use aegis_types::Mode;
use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

pub const MIN_TOKEN_LEN: usize = 24;

#[derive(Clone)]
struct ApiState {
    engine: Arc<EngineHandle>,
    token: Arc<String>,
}

/// Compare without leaking the position of the first difference.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn require_token(State(st): State<ApiState>, req: Request, next: Next) -> Response {
    let given = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if constant_time_eq(given.as_bytes(), st.token.as_bytes()) {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, Json(json!({ "error": "unauthorized" }))).into_response()
    }
}

fn reply(r: Result<String, String>) -> Response {
    match r {
        Ok(message) => Json(json!({ "ok": true, "message": message })).into_response(),
        Err(error) => (StatusCode::CONFLICT, Json(json!({ "ok": false, "error": error }))).into_response(),
    }
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

pub fn router(engine: Arc<EngineHandle>, token: String) -> anyhow::Result<Router> {
    if token.len() < MIN_TOKEN_LEN {
        anyhow::bail!("the API token must contain at least {MIN_TOKEN_LEN} characters");
    }
    let st = ApiState { engine, token: Arc::new(token) };
    let protected = Router::new()
        .route("/api/v1/state", get(|State(st): State<ApiState>| async move { Json((*st.engine.snapshot()).clone()) }))
        .route(
            "/api/v1/mode",
            post(|State(st): State<ApiState>, Json(b): Json<ModeBody>| async move {
                reply(st.engine.command(Command::SetMode { mode: b.mode, confirmation: b.confirmation }).await)
            }),
        )
        .route(
            "/api/v1/kill",
            post(|State(st): State<ApiState>, Json(b): Json<KillBody>| async move { reply(st.engine.command(Command::Kill { reason: b.reason }).await) }),
        )
        .route(
            "/api/v1/kill/reset",
            post(|State(st): State<ApiState>, Json(b): Json<ResetBody>| async move { reply(st.engine.command(Command::ResetKill { phrase: b.phrase }).await) }),
        )
        .route_layer(middleware::from_fn_with_state(st.clone(), require_token));
    Ok(Router::new().route("/api/v1/health", get(|| async { Json(json!({ "status": "ok" })) })).merge(protected).with_state(st))
}
