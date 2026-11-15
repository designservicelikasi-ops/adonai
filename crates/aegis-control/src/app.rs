//! Application shell: owns the engine, forwards user actions, polls outcomes.

use crate::ui::{self, UiAction, UiState};
use aegis_core::{logging, resolve_shared_key, start, EngineConfig, EngineHandle, Reply};
use aegis_simterminal::{SimHandle, SimTerminalConfig};
use eframe::egui;
use std::path::Path;
use std::time::Duration;
use tokio::sync::oneshot;

pub struct AegisApp {
    rt: tokio::runtime::Runtime,
    engine: Option<EngineHandle>,
    state: UiState,
    pending: Vec<(String, oneshot::Receiver<Reply>)>,
    sim: Option<SimHandle>,
    psk: Vec<u8>,
    /// Keeps the log file writer alive for the lifetime of the application.
    _log_guard: Option<Box<dyn std::any::Any>>,
}

impl AegisApp {
    /// Load the configuration, start the engine (always in MONITOR mode) and build the UI state.
    pub fn new(config_path: &Path) -> anyhow::Result<Self> {
        let cfg = EngineConfig::load(config_path)?;
        let guard = logging::init(&cfg.engine.data_dir.join("logs"), false).ok().map(|g| Box::new(g) as Box<dyn std::any::Any>);
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().thread_name("aegis-engine").build()?;
        let (psk, _source) = resolve_shared_key(&cfg)?;
        let engine = rt.block_on(start(cfg.clone()))?;
        let state = UiState::new(String::from_utf8_lossy(&psk).to_string(), config_path.display().to_string(), cfg.to_toml());
        Ok(AegisApp { rt, engine: Some(engine), state, pending: Vec::new(), sim: None, psk, _log_guard: guard })
    }

    fn message(&mut self, ok: bool, text: String) {
        self.state.messages.insert(0, (ok, text));
        self.state.messages.truncate(50);
    }

    fn poll_outcomes(&mut self) {
        let mut done = Vec::new();
        self.pending.retain_mut(|(label, rx)| match rx.try_recv() {
            Ok(Ok(msg)) => {
                done.push((true, format!("{label}: {msg}")));
                false
            }
            Ok(Err(e)) => {
                done.push((false, format!("{label}: {e}")));
                false
            }
            Err(oneshot::error::TryRecvError::Empty) => true,
            Err(oneshot::error::TryRecvError::Closed) => {
                done.push((false, format!("{label}: le moteur n'a pas répondu")));
                false
            }
        });
        for (ok, text) in done {
            self.message(ok, text);
        }
    }

    fn run_actions(&mut self) {
        for action in std::mem::take(&mut self.state.actions) {
            match action {
                UiAction::Engine { label, command } => {
                    let sent = self.engine.as_ref().map(|e| e.send(command)).unwrap_or_else(|| Err("moteur arrêté".into()));
                    match sent {
                        Ok(rx) => self.pending.push((label, rx)),
                        Err(e) => self.message(false, format!("{label}: {e}")),
                    }
                }
                UiAction::StartSimulator => {
                    if self.sim.is_none() {
                        if let Some(engine) = &self.engine {
                            let cfg = SimTerminalConfig::new(&engine.bridge_addr.to_string(), &self.psk).with_synthetic_history(600, aegis_types::now_ms(), 11);
                            let _ctx = self.rt.enter();
                            self.sim = Some(aegis_simterminal::spawn(cfg));
                            self.message(true, "Simulateur démarré (adaptateur de test, données fictives)".into());
                        }
                    }
                }
                UiAction::StopSimulator => {
                    if let Some(sim) = self.sim.take() {
                        self.rt.block_on(sim.stop());
                        self.message(true, "Simulateur arrêté".into());
                    }
                }
            }
        }
    }
}

impl eframe::App for AegisApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let Some(engine) = &self.engine else { return };
        let view = engine.snapshot();
        if self.sim.as_ref().is_some_and(|s| s.task.is_finished()) {
            self.sim = None;
            self.message(false, "Le simulateur s'est arrêté".into());
        }
        self.state.simulator_running = self.sim.is_some();
        self.poll_outcomes();
        ui::draw(ctx, &mut self.state, &view);
        self.run_actions();
        // The engine publishes four times per second; repaint at the same pace.
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(sim) = self.sim.take() {
            self.rt.block_on(sim.stop());
        }
        if let Some(engine) = self.engine.take() {
            self.rt.block_on(engine.shutdown());
        }
    }
}
