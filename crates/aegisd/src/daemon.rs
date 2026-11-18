//! The long-running engine process: engine + control API (TLS) + monitoring.
//! Shared by `aegisd run` and the Windows service.

use crate::{monitor, router_with, tls, ApiOptions};
use aegis_core::{logging, resolve_api_token, resolve_shared_key, start, EngineConfig};
use anyhow::Context;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

/// Run until `shutdown` completes. The engine always starts in MONITOR.
pub async fn serve(cfg: EngineConfig, simulator: bool, shutdown: impl Future<Output = ()>) -> anyhow::Result<()> {
    let _log_guard = logging::init(&cfg.engine.data_dir.join("logs"), true)?;
    let engine = Arc::new(start(cfg.clone()).await?);
    tracing::info!("AEGIS engine {} running in MONITOR mode; terminal endpoint {}", env!("CARGO_PKG_VERSION"), engine.bridge_addr);
    let monitor = monitor::SystemMonitor::start(Duration::from_secs(5));

    let mut api_server: Option<axum_server::Handle> = None;
    if cfg.api.enabled {
        let (token, source) = resolve_api_token(&cfg)?;
        tracing::info!("control API token: {source}");
        let app = router_with(engine.clone(), ApiOptions { token, files_dir: cfg.engine.data_dir.join("files"), monitor: Some(monitor.clone()) })?;
        let addr: std::net::SocketAddr = cfg.api.listen.parse().context("api.listen")?;
        let handle = axum_server::Handle::new();
        if cfg.api.tls {
            tls::install_crypto_provider();
            let files = tls::ensure(&cfg.engine.data_dir.join("tls"), &cfg.api.tls_names)?;
            tracing::info!("control API listening on https://{addr} — certificate fingerprint SHA-256 {}", files.fingerprint);
            let rustls = axum_server::tls_rustls::RustlsConfig::from_pem_file(&files.cert, &files.key).await?;
            let h = handle.clone();
            tokio::spawn(async move {
                if let Err(e) = axum_server::bind_rustls(addr, rustls).handle(h).serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await {
                    tracing::error!("API stopped: {e}");
                }
            });
        } else {
            tracing::warn!("control API listening WITHOUT TLS on http://{addr} (loopback only)");
            let h = handle.clone();
            tokio::spawn(async move {
                if let Err(e) = axum_server::bind(addr).handle(h).serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await {
                    tracing::error!("API stopped: {e}");
                }
            });
        }
        api_server = Some(handle);
    }

    let sim = if simulator {
        let (psk, _) = resolve_shared_key(&cfg)?;
        tracing::warn!("starting the built-in SIMULATOR (test adapter, synthetic prices, instrument EURUSD.sim) — this is not a broker");
        let sim_cfg = aegis_simterminal::SimTerminalConfig::new(&engine.bridge_addr.to_string(), &psk).with_synthetic_history(600, aegis_types::now_ms(), 11);
        Some(aegis_simterminal::spawn(sim_cfg))
    } else {
        None
    };

    shutdown.await;
    tracing::info!("shutdown requested");
    if let Some(h) = api_server {
        h.graceful_shutdown(Some(Duration::from_secs(5)));
    }
    if let Some(s) = sim {
        s.stop().await;
    }
    // The API task holds clones of the handle: stop through the shared reference so
    // the engine always closes its database cleanly.
    engine.stop().await;
    Ok(())
}
