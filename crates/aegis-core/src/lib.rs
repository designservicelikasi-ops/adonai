//! AEGIS CORE — the engine: lifecycle, configuration, orchestration of every
//! module, supervision, and the bridge to the trading terminal.

pub mod autonomy;
mod brain;
mod bridge;
mod command;
pub mod config;
mod decide;
mod engine;
mod guardian;
mod intel;
pub mod latency;
pub mod performance;
mod shadowlink;
pub mod ops;
pub mod logging;
mod positions;
pub mod scanner;
pub mod view;

pub use command::{Command, Reply, LIVE_CONFIRMATION_PHRASE};
pub use config::{resolve_api_token, resolve_shared_key, EngineConfig};
pub use engine::{start, EngineHandle};
pub use view::EngineView;
