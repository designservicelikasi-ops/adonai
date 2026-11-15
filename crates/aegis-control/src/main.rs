//! `aegis` — AEGIS desktop application.
//!
//! Usage: `aegis [--config config/aegis.toml]`

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use aegis_control::app::AegisApp;
use eframe::egui;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut config = PathBuf::from("config/aegis.toml");
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => config = PathBuf::from(args.next().ok_or_else(|| anyhow::anyhow!("--config needs a file"))?),
            "--help" | "-h" => {
                println!("AEGIS desktop application\n\nUsage: aegis [--config FILE]\n\nThe engine always starts in MONITOR mode (no order is ever sent).");
                return Ok(());
            }
            other => anyhow::bail!("unknown argument `{other}`"),
        }
    }
    let app = AegisApp::new(&config)?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("AEGIS — Adaptive Engine for Intelligent Strategies").with_inner_size([1440.0, 900.0]).with_min_inner_size([1100.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "AEGIS",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("cannot open the application window: {e}"))
}
