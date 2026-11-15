//! AEGIS CONTROL — desktop application.
//!
//! The engine runs inside the application process (on a Tokio runtime); the
//! interface reads its published `EngineView` and sends it `Command`s. Nothing
//! shown on screen is computed or invented by the interface itself.

pub mod app;
pub mod ui;
