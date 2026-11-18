//! AEGIS CORE — the engine: lifecycle, configuration, orchestration of every
//! module, supervision, and the bridge to the trading terminal.

mod bridge;
mod command;
pub mod config;
mod decide;
mod engine;
pub mod logging;
mod positions;
pub mod view;

pub use command::{Command, Reply, LIVE_CONFIRMATION_PHRASE};
pub use config::{resolve_shared_key, EngineConfig};
pub use engine::{start, EngineHandle};
pub use view::EngineView;
