//! `aegisd` — headless AEGIS engine and command-line tools.
//!
//! ```text
//! aegisd [--config FILE] run [--simulator]
//! aegisd [--config FILE] backup
//! aegisd [--config FILE] restore --from BACKUP.db
//! aegisd [--config FILE] verify-audit
//! aegisd [--config FILE] generate-synthetic --symbol EURUSD.sim --tf M15 --bars 20000 [--seed 42]
//! aegisd [--config FILE] import-csv --file F.csv --symbol EURUSD --tf M15 [--utc-offset 2]
//! aegisd [--config FILE] backtest --strategy ema_trend --symbol EURUSD --tf M15 [--origin broker|synthetic]
//! aegisd [--config FILE] export-dataset --symbol EURUSD --tf M15 --out FILE.csv [--origin ..] [--horizon 4]
//! aegisd [--config FILE] verify-model --file models/model.json
//! aegisd [--config FILE] print-config
//! ```

use aegis_core::{logging, resolve_shared_key, start, Command, EngineConfig};
use aegis_data::Store;
use aegis_types::{DataOrigin, Timeframe};
use anyhow::{anyhow, bail, Context};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

struct Args {
    command: String,
    opts: HashMap<String, String>,
    flags: Vec<String>,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut command = String::new();
    let mut opts = HashMap::new();
    let mut flags = Vec::new();
    let mut it = std::env::args().skip(1).peekable();
    const FLAGS: [&str; 1] = ["simulator"];
    while let Some(a) = it.next() {
        if let Some(name) = a.strip_prefix("--") {
            let takes_value = !FLAGS.contains(&name) && it.peek().is_some_and(|v| !v.starts_with("--"));
            if takes_value {
                opts.insert(name.to_string(), it.next().expect("peeked"));
            } else {
                flags.push(name.to_string());
            }
        } else if command.is_empty() {
            command = a;
        } else {
            bail!("unexpected argument `{a}`");
        }
    }
    if command.is_empty() {
        command = "run".into();
    }
    Ok(Args { command, opts, flags })
}

impl Args {
    fn need(&self, key: &str) -> anyhow::Result<&str> {
        self.opts.get(key).map(String::as_str).ok_or_else(|| anyhow!("missing option --{key}"))
    }
    fn tf(&self) -> anyhow::Result<Timeframe> {
        let v = self.need("tf")?;
        Timeframe::parse(v).ok_or_else(|| anyhow!("unknown timeframe `{v}`"))
    }
    fn origin(&self) -> anyhow::Result<DataOrigin> {
        let v = self.opts.get("origin").map(String::as_str).unwrap_or("broker");
        DataOrigin::parse(v).ok_or_else(|| anyhow!("--origin must be `broker` or `synthetic`"))
    }
    fn num<T: std::str::FromStr>(&self, key: &str, default: T) -> anyhow::Result<T> {
        match self.opts.get(key) {
            Some(v) => v.parse().map_err(|_| anyhow!("--{key}: invalid number `{v}`")),
            None => Ok(default),
        }
    }
}

/// Start the engine on a private port, run one command, print its outcome, stop.
async fn one_shot(mut cfg: EngineConfig, command: Command) -> anyhow::Result<()> {
    cfg.bridge.listen = "127.0.0.1:0".into();
    cfg.news.enabled = false; // a one-shot tool has no use for the calendar
    let engine = start(cfg).await?;
    let result = engine.command(command).await;
    let detail = engine.snapshot().last_backtest.clone();
    engine.shutdown().await;
    match result {
        Ok(msg) => {
            println!("{msg}");
            if let Some(bt) = detail {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "run_id": bt.run.run_id, "origin": bt.run.origin.as_str(), "full": bt.full,
                    "segments": bt.segments, "walk_forward": bt.walk_forward,
                }))?);
                println!("NOTE: a backtest describes the past under stated assumptions. It is not a prediction.");
            }
            Ok(())
        }
        Err(e) => bail!("{e}"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = parse_args()?;
    let config_path = PathBuf::from(args.opts.get("config").cloned().unwrap_or_else(|| "config/aegis.toml".into()));
    let cfg = EngineConfig::load(&config_path).with_context(|| format!("loading {}", config_path.display()))?;

    match args.command.as_str() {
        "print-config" => {
            println!("{}", cfg.to_toml());
            Ok(())
        }
        "restore" => {
            let from = PathBuf::from(args.need("from")?);
            Store::restore_from(&from, &cfg.db_path()).await?;
            println!("database restored from {} (previous file kept as aegis.db.before-restore)", from.display());
            Ok(())
        }
        "verify-audit" => {
            let store = Store::open(&cfg.db_path()).await?;
            let n = store.verify_audit_chain().await?;
            let ok = store.integrity_check().await?;
            println!("audit chain intact: {n} entries verified; database integrity: {}", if ok { "ok" } else { "FAILED" });
            if !ok {
                bail!("database integrity check failed");
            }
            Ok(())
        }
        "verify-model" => {
            let path = PathBuf::from(args.need("file")?);
            let model = aegis_learning::ModelArtifact::load(&path)?;
            let gate = model.gate(&cfg.learning.gate);
            println!("model {} — inference parity error {:e}", model.model_id, model.parity_error());
            println!("validation: AUC {:.4}, Brier {:.4} (baseline {:.4}), ECE {:.4}, {} samples, {} folds",
                model.validation.auc_mean, model.validation.brier_mean, model.validation.baseline_brier,
                model.validation.ece, model.validation.n_samples, model.validation.folds.len());
            if gate.passed {
                println!("GATE: PASSED — the model may act as a filter on execution");
            } else {
                println!("GATE: NOT PASSED — display only, no effect on execution");
                for f in gate.failures {
                    println!("  - {f}");
                }
            }
            Ok(())
        }
        "backup" => one_shot(cfg, Command::Backup).await,
        "generate-synthetic" => {
            let c = Command::GenerateSynthetic { symbol: args.need("symbol")?.into(), timeframe: args.tf()?, bars: args.num("bars", 20_000usize)?, seed: args.num("seed", 42u64)? };
            one_shot(cfg, c).await
        }
        "import-csv" => {
            let c = Command::ImportCsv { path: args.need("file")?.into(), symbol: args.need("symbol")?.into(), timeframe: args.tf()?, utc_offset_hours: args.num("utc-offset", 0i64)? };
            one_shot(cfg, c).await
        }
        "backtest" => {
            let c = Command::RunBacktest { strategy_id: args.need("strategy")?.into(), symbol: args.need("symbol")?.into(), timeframe: args.tf()?, origin: args.origin()?, params: Default::default() };
            one_shot(cfg, c).await
        }
        "export-dataset" => {
            let c = Command::ExportDataset { symbol: args.need("symbol")?.into(), timeframe: args.tf()?, origin: args.origin()?, horizon: args.num("horizon", 4usize)?, path: args.need("out")?.into() };
            one_shot(cfg, c).await
        }
        "run" => run(cfg, args.flags.iter().any(|f| f == "simulator")).await,
        other => bail!("unknown command `{other}` (see the header of crates/aegisd/src/main.rs or docs/USER_MANUAL.md)"),
    }
}

async fn run(cfg: EngineConfig, simulator: bool) -> anyhow::Result<()> {
    let _log_guard = logging::init(&cfg.engine.data_dir.join("logs"), true)?;
    let engine = Arc::new(start(cfg.clone()).await?);
    tracing::info!("AEGIS engine running in MONITOR mode; terminal endpoint {}", engine.bridge_addr);

    if cfg.api.enabled {
        let token = std::env::var(&cfg.api.token_env).map_err(|_| anyhow!("the API is enabled but {} is not set", cfg.api.token_env))?;
        let app = aegisd::router(engine.clone(), token.trim().to_string())?;
        let listener = tokio::net::TcpListener::bind(&cfg.api.listen).await.with_context(|| format!("binding {}", cfg.api.listen))?;
        tracing::info!("control API listening on {}", cfg.api.listen);
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!("API stopped: {e}");
            }
        });
    }

    let sim = if simulator {
        let (psk, _) = resolve_shared_key(&cfg)?;
        tracing::warn!("starting the built-in SIMULATOR (test adapter, synthetic prices, instrument EURUSD.sim) — this is not a broker");
        let sim_cfg = aegis_simterminal::SimTerminalConfig::new(&engine.bridge_addr.to_string(), &psk).with_synthetic_history(600, aegis_types::now_ms(), 11);
        Some(aegis_simterminal::spawn(sim_cfg))
    } else {
        None
    };

    tokio::signal::ctrl_c().await?;
    tracing::info!("shutdown requested");
    if let Some(s) = sim {
        s.stop().await;
    }
    // The API task holds clones of the handle: stop through the shared reference so
    // the engine always closes its database cleanly.
    engine.stop().await;
    Ok(())
}
