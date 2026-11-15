//! Structured logging: JSON lines in a daily-rotated file plus a compact console output.

use std::path::Path;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

/// Initialise logging. Keep the returned guard alive for the whole process:
/// dropping it flushes and stops the file writer.
pub fn init(log_dir: &Path, console: bool) -> std::io::Result<tracing_appender::non_blocking::WorkerGuard> {
    std::fs::create_dir_all(log_dir)?;
    let file = tracing_appender::rolling::daily(log_dir, "aegis.log");
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = EnvFilter::try_from_env("AEGIS_LOG").unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));
    let registry = tracing_subscriber::registry().with(filter).with(fmt::layer().json().with_writer(writer));
    let result = if console {
        registry.with(fmt::layer().compact().with_target(false)).try_init()
    } else {
        registry.try_init()
    };
    // A second initialisation (tests, embedded use) is not an error worth failing for.
    let _ = result;
    Ok(guard)
}
