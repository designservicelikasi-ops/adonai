//! Commands issued by the user (desktop application, HTTP API, CLI, tests).

use crate::engine::{evidence_from_run, evidence_key, Engine};
use crate::view::{BacktestDetail, BacktestSegmentView};
use aegis_backtest::synthetic::{generate, SyntheticParams};
use aegis_backtest::{run, run_segmented, walk_forward, BacktestParams, SegmentResult};
use aegis_data::{BacktestRunRow, StrategyRow};
use aegis_risk::RiskConfig;
use aegis_strategy::{build, can_promote, Evidence, StrategyStage};
use aegis_types::*;
use serde_json::json;
use std::path::PathBuf;
use tokio::sync::oneshot;

pub type Reply = Result<String, String>;

fn default_folds() -> usize {
    4
}

/// Phrase the user must type, with the account number, to arm LIVE AUTO.
pub const LIVE_CONFIRMATION_PHRASE: &str = "J'ACCEPTE LE RISQUE REEL";

/// JSON form (HTTP API): `{"type": "set_mode", "args": {"mode": "demo_auto", "confirmation": ""}}`,
/// unit commands without `args`: `{"type": "backup"}`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "args", rename_all = "snake_case")]
pub enum Command {
    /// `confirmation` is only used for LIVE AUTO: "<account login> J'ACCEPTE LE RISQUE REEL".
    SetMode {
        mode: Mode,
        #[serde(default)]
        confirmation: String,
    },
    Kill {
        #[serde(default)]
        reason: String,
    },
    ResetKill { phrase: String },
    SetRiskConfig(RiskConfig),
    SetStrategyEnabled { id: String, enabled: bool },
    SetStrategyStage { id: String, stage: StrategyStage },
    RunBacktest {
        strategy_id: String,
        symbol: String,
        timeframe: Timeframe,
        origin: DataOrigin,
        #[serde(default)]
        params: BacktestParams,
    },
    /// Generate SYNTHETIC candles for an instrument (testing and demonstration only).
    GenerateSynthetic { symbol: String, timeframe: Timeframe, bars: usize, seed: u64 },
    ImportCsv { path: PathBuf, symbol: String, timeframe: Timeframe, utc_offset_hours: i64 },
    ExportDataset { symbol: String, timeframe: Timeframe, origin: DataOrigin, horizon: usize, path: PathBuf },
    ClosePosition { ticket: i64, volume: Option<f64> },
    ModifyPosition { ticket: i64, sl: f64, tp: f64 },
    CancelOrder { ticket: i64 },
    Backup,
    LoadModel { path: PathBuf },
    /// Train a P(TP before SL) model on stored candles (runs in the background).
    TrainProbability { symbol: String, timeframe: Timeframe, origin: DataOrigin },
    /// Change a strategy's parameters (validated; the strategy returns to research).
    SetStrategyConfig { id: String, config: serde_json::Value },
    /// Backtest every shadow candidate on stored candles.
    EvaluateShadowHistory { symbol: String, timeframe: Timeframe, origin: DataOrigin },
    /// Promote a challenger that passed backtest, out-of-sample and forward (to DEMO only).
    PromoteChallenger { candidate_id: String },
    /// Make a stored probability model version active (rollback).
    ActivateProbabilityModel { model_id: String },
    /// Micro-profit brain: retrain its probability models now (better versions are only proposed).
    BrainRetrain,
    /// Micro-profit brain settings (on/off, gain range, SL/TP in pips...). Stored; they win over the file.
    SetBrainConfig(aegis_brain::BrainConfig),
    /// Activate a version the brain proposed.
    BrainAcceptCandidate { model_id: String },
    /// Set aside a version the brain proposed (it stays stored).
    BrainDismissCandidate { model_id: String },
    VerifyAudit,
    /// Autonomy Commander: grant DEMO FULL AUTONOMY or LIVE AUTHORIZED AUTONOMY for the
    /// connected account. `confirmation` (LIVE only): "<login> J'ACCEPTE LE RISQUE REEL".
    GrantAutonomy {
        profile: aegis_autonomy::Profile,
        #[serde(default)]
        duration_minutes: Option<i64>,
        #[serde(default)]
        persist_across_restart: bool,
        #[serde(default)]
        confirmation: String,
        #[serde(default)]
        note: String,
    },
    /// Immediate revocation of the active grant (back to MONITOR).
    RevokeAutonomy {
        #[serde(default)]
        reason: String,
    },
    /// Profile without a grant: observer or research_autonomy.
    SetDefaultProfile { profile: aegis_autonomy::Profile },
    /// Parallel parameter research (walk-forward in-sample, holdout reported). Proposes only.
    RunParameterSweep {
        strategy_id: String,
        symbol: String,
        timeframe: Timeframe,
        origin: DataOrigin,
        grid: std::collections::BTreeMap<String, Vec<serde_json::Value>>,
        #[serde(default = "default_folds")]
        folds: usize,
        #[serde(default)]
        threads: usize,
    },
    /// Enable or disable new entries on an instrument (open positions stay managed).
    SetSymbolEnabled { symbol: String, enabled: bool },
    /// Data quality audit of a stored history.
    AuditData { symbol: String, timeframe: Timeframe, origin: DataOrigin },
    Shutdown,
}

pub(crate) enum Internal {
    /// Result of the background database probe.
    DbPing(Result<(), String>),
    BacktestDone { reply: oneshot::Sender<Reply>, outcome: Result<Box<BacktestDetail>, String> },
    NewsFetched(Vec<Result<Vec<aegis_news::NewsEvent>, String>>),
    ProbabilityDone { reply: oneshot::Sender<Reply>, outcome: Result<Box<aegis_probability::ProbabilityModel>, String> },
    BrainRetrained { reply: Option<oneshot::Sender<Reply>>, results: Vec<crate::brain::RetrainOutcome> },
    SweepDone { reply: oneshot::Sender<Reply>, outcome: Result<Box<aegis_backtest::sweep::SweepReport>, String> },
}

fn seg_view(s: &SegmentResult) -> BacktestSegmentView {
    BacktestSegmentView { segment: s.segment.clone(), bars: s.bars, from_ts: s.from_ts, to_ts: s.to_ts, metrics: s.result.metrics.clone() }
}

impl Engine {
    pub(crate) async fn on_command(&mut self, c: Command, reply: oneshot::Sender<Reply>) {
        let out = match c {
            Command::RunBacktest { strategy_id, symbol, timeframe, origin, params } => {
                // Runs in the background; the reply is sent when it completes.
                if let Err(e) = self.start_backtest(strategy_id, symbol, timeframe, origin, params, reply).await {
                    self.log("warn", "backtest", e);
                }
                return;
            }
            Command::RunParameterSweep { strategy_id, symbol, timeframe, origin, grid, folds, threads } => {
                let req = aegis_backtest::sweep::SweepRequest { strategy_id, base_config: serde_json::Value::Null, grid, folds, in_sample_frac: 0.7, max_threads: if threads == 0 { usize::MAX } else { threads }, max_combinations: 0 };
                self.start_sweep(req, symbol, timeframe, origin, reply).await;
                return;
            }
            Command::TrainProbability { symbol, timeframe, origin } => {
                self.start_probability_training(symbol, timeframe, origin, reply).await;
                return;
            }
            Command::ActivateProbabilityModel { model_id } => self.activate_probability_model(&model_id).await,
            Command::BrainRetrain => {
                if !self.cfg.brain.enabled {
                    Err("le cerveau micro-profits est désactivé ([brain] enabled = false dans la configuration)".into())
                } else {
                    self.start_brain_retrain(Some(reply)).await;
                    return;
                }
            }
            Command::SetBrainConfig(c) => self.set_brain_config(c).await,
            Command::BrainAcceptCandidate { model_id } => self.brain_activate(&model_id).await,
            Command::BrainDismissCandidate { model_id } => self.brain_dismiss(&model_id).await,
            Command::SetStrategyConfig { id, config } => self.cmd_strategy_config(&id, config, "user").await,
            Command::EvaluateShadowHistory { symbol, timeframe, origin } => self.cmd_shadow_history(&symbol, timeframe, origin).await,
            Command::PromoteChallenger { candidate_id } => self.cmd_promote_challenger(&candidate_id).await,
            Command::SetMode { mode, confirmation } => self.cmd_set_mode(mode, &confirmation).await,
            Command::GrantAutonomy { profile, duration_minutes, persist_across_restart, confirmation, note } => {
                if profile == aegis_autonomy::Profile::LiveAuthorizedAutonomy
                    && !self.strategies.iter().any(|s| s.enabled && s.stage == StrategyStage::LiveApproved)
                {
                    Err("LIVE refusé: aucune stratégie n'est approuvée pour le réel".into())
                } else {
                    let req = aegis_autonomy::GrantRequest { profile, duration_ms: duration_minutes.map(|m| m * 60_000), persist_across_restart, note };
                    self.grant_autonomy(req, &confirmation).await
                }
            }
            Command::RevokeAutonomy { reason } => {
                let reason = if reason.trim().is_empty() { "révocation par l'utilisateur".to_string() } else { reason };
                Ok(match self.revoke_autonomy("user", &reason).await {
                    Some(g) => format!("autorisation {} révoquée — mode MONITOR", g.grant_id),
                    None => "aucune autorisation active — mode MONITOR".into(),
                })
            }
            Command::SetDefaultProfile { profile } => self.set_default_profile(profile).await,
            Command::SetSymbolEnabled { symbol, enabled } => self.cmd_symbol_enabled(&symbol, enabled).await,
            Command::AuditData { symbol, timeframe, origin } => self.cmd_audit_data(&symbol, timeframe, origin).await,
            Command::Kill { reason } => {
                let reason = if reason.trim().is_empty() { "arrêt manuel".to_string() } else { reason };
                self.engage_kill(&reason, "user").await;
                Ok("arrêt d'urgence engagé".into())
            }
            Command::ResetKill { phrase } => match self.risk.reset_kill(&phrase) {
                Ok(()) => {
                    let state = self.risk.kill_state().clone();
                    if let Err(e) = self.store.kv_set("kill_switch", &state).await {
                        self.db_failed(&e.to_string());
                    }
                    self.kill_plan_pending = false;
                    // Automation never resumes by itself after an emergency stop.
                    self.revoke_autonomy("engine", "réinitialisation de l'arrêt d'urgence").await;
                    self.audit("user", "kill_switch_reset", json!({})).await;
                    self.log("warn", "risk", "arrêt d'urgence réinitialisé manuellement; mode MONITOR".into());
                    Ok("arrêt d'urgence réinitialisé — le moteur reste en MONITOR".into())
                }
                Err(e) => Err(match e {
                    "confirmation phrase does not match" => format!("phrase de confirmation incorrecte (attendu: {})", aegis_risk::KILL_RESET_PHRASE),
                    _ => "l'arrêt d'urgence n'est pas actif".to_string(),
                }),
            },
            Command::SetRiskConfig(cfg) => {
                let r = match self.risk.set_config(cfg.clone()) {
                    Ok(()) => match self.store.save_risk_config("user", &serde_json::to_value(&cfg).unwrap_or_default()).await {
                        Ok(v) => {
                            self.audit("user", "risk_config_changed", serde_json::to_value(&cfg).unwrap_or_default()).await;
                            self.log("warn", "risk", format!("configuration de risque mise à jour (version {v})"));
                            Ok(format!("configuration de risque enregistrée (version {v})"))
                        }
                        Err(e) => Err(format!("configuration appliquée mais non enregistrée: {e}")),
                    },
                    Err(e) => Err(e.to_string()),
                };
                // The shadow uses the same commission assumption as the risk engine.
                self.shadow_config();
                r
            }
            Command::SetStrategyEnabled { id, enabled } => {
                let r = self.cmd_strategy_enabled(&id, enabled).await;
                self.shadow_champions();
                r
            }
            Command::SetStrategyStage { id, stage } => self.cmd_strategy_stage(&id, stage).await,
            Command::GenerateSynthetic { symbol, timeframe, bars, seed } => self.cmd_generate(&symbol, timeframe, bars, seed).await,
            Command::ImportCsv { path, symbol, timeframe, utc_offset_hours } => self.cmd_import(&path, &symbol, timeframe, utc_offset_hours).await,
            Command::ExportDataset { symbol, timeframe, origin, horizon, path } => self.cmd_export(&symbol, timeframe, origin, horizon, &path).await,
            Command::ClosePosition { ticket, volume } => match self.positions.iter().find(|p| p.ticket == ticket).cloned() {
                Some(p) => self.close_position(&p, volume, "demande de l'utilisateur").await,
                None => Err(format!("position {ticket} introuvable")),
            },
            Command::ModifyPosition { ticket, sl, tp } => match self.positions.iter().find(|p| p.ticket == ticket).cloned() {
                Some(p) if self.mode.can_execute() => match self.executor.submit_modify(&p, sl, tp, "demande de l'utilisateur", now_ms()).await {
                    Ok(body) => self.dispatch_command(body, &format!("modification de la position {ticket}"), "demande de l'utilisateur").await,
                    Err(e) => Err(e.to_string()),
                },
                Some(_) => Err("mode MONITOR: aucun ordre n'est envoyé".into()),
                None => Err(format!("position {ticket} introuvable")),
            },
            Command::CancelOrder { ticket } => match self.pending.iter().find(|o| o.ticket == ticket).cloned() {
                Some(o) if self.mode.can_execute() => match self.executor.submit_cancel(&o, "demande de l'utilisateur", now_ms()).await {
                    Ok(body) => self.dispatch_command(body, &format!("annulation de l'ordre {ticket}"), "demande de l'utilisateur").await,
                    Err(e) => Err(e.to_string()),
                },
                Some(_) => Err("mode MONITOR: aucun ordre n'est envoyé".into()),
                None => Err(format!("ordre {ticket} introuvable")),
            },
            Command::Backup => self.cmd_backup().await,
            Command::LoadModel { path } => {
                let r = self.load_model(&path).await;
                if let Err(e) = &r {
                    self.model_error = Some(e.clone());
                }
                r
            }
            Command::VerifyAudit => match self.store.verify_audit_chain().await {
                Ok(n) => {
                    self.audit_verified = Some(n);
                    self.audit_error = None;
                    Ok(format!("journal d'audit intègre: {n} entrées vérifiées"))
                }
                Err(e) => {
                    self.audit_error = Some(e.to_string());
                    self.log("error", "data", format!("journal d'audit ALTÉRÉ: {e}"));
                    Err(format!("journal d'audit altéré: {e}"))
                }
            },
            Command::Shutdown => Ok("arrêt".into()),
        };
        if let Err(e) = &out {
            self.log("warn", "command", format!("commande refusée: {e}"));
        }
        // Publish first: whoever receives the reply must already see its effect.
        self.cache.dirty = true;
        self.publish().await;
        let _ = reply.send(out);
    }

    async fn cmd_set_mode(&mut self, mode: Mode, confirmation: &str) -> Reply {
        if mode == self.mode {
            return Ok(format!("déjà en mode {}", mode.label()));
        }
        if mode == Mode::Monitor {
            self.revoke_autonomy("user", "demande de l'utilisateur").await;
            return Ok("mode MONITOR: aucune exécution".into());
        }
        if self.risk.kill_state().active {
            return Err("arrêt d'urgence actif: réinitialisez-le d'abord".into());
        }
        if !self.db_ok {
            return Err("base de données indisponible".into());
        }
        let Some(conn) = &self.conn else { return Err("aucun terminal connecté".into()) };
        let Some(account) = self.account.clone() else { return Err("informations de compte non reçues".into()) };
        let sim = conn.info.terminal == TerminalKind::Simulator;
        match mode {
            Mode::DemoAuto => {
                if !sim && account.trade_mode == AccountTradeMode::Real {
                    return Err("DEMO AUTO refusé: le compte connecté est un compte RÉEL".into());
                }
                // Routed through the Autonomy Commander: a DEMO grant bound to this account.
                let req = aegis_autonomy::GrantRequest { profile: aegis_autonomy::Profile::DemoFullAutonomy, duration_ms: None, persist_across_restart: false, note: "set_mode demo_auto".into() };
                self.grant_autonomy(req, "").await?;
                Ok(if sim { "DEMO AUTO activé sur le SIMULATEUR".into() } else { format!("DEMO AUTO activé sur le compte de démonstration {}", account.login) })
            }
            Mode::LiveAuto => {
                if !self.cfg.safety.live_trading_enabled {
                    return Err("LIVE AUTO est désactivé dans la configuration (safety.live_trading_enabled = false)".into());
                }
                if sim {
                    return Err("LIVE AUTO impossible avec le simulateur".into());
                }
                if account.trade_mode != AccountTradeMode::Real {
                    return Err("LIVE AUTO refusé: le compte connecté n'est pas un compte réel".into());
                }
                if !account.trade_allowed {
                    return Err("LIVE AUTO refusé: le trading est désactivé sur ce compte".into());
                }
                if !self.strategies.iter().any(|s| s.enabled && s.stage == StrategyStage::LiveApproved) {
                    return Err("LIVE AUTO refusé: aucune stratégie n'est approuvée pour le réel".into());
                }
                let expected = format!("{} {}", account.login, LIVE_CONFIRMATION_PHRASE);
                if confirmation.trim() != expected {
                    return Err(format!("confirmation incorrecte: saisissez exactement « {expected} »"));
                }
                let login = account.login;
                let hours = self.cfg.autonomy.live_default_hours;
                let req = aegis_autonomy::GrantRequest {
                    profile: aegis_autonomy::Profile::LiveAuthorizedAutonomy,
                    duration_ms: Some(hours * 3_600_000),
                    persist_across_restart: false,
                    note: "set_mode live_auto".into(),
                };
                self.grant_autonomy(req, confirmation).await?;
                Ok(format!("LIVE AUTO ARMÉ sur le compte réel {login} pour {hours} h"))
            }
            Mode::Monitor => unreachable!("handled above"),
        }
    }

    async fn cmd_strategy_enabled(&mut self, id: &str, enabled: bool) -> Reply {
        let Some(slot) = self.strategies.iter_mut().find(|s| s.strategy.meta().id == id) else { return Err(format!("stratégie {id} inconnue")) };
        slot.enabled = enabled;
        self.store.set_strategy_enabled(id, enabled).await.map_err(|e| e.to_string())?;
        self.audit("user", "strategy_enabled", json!({ "id": id, "enabled": enabled })).await;
        Ok(format!("stratégie {id} {}", if enabled { "activée" } else { "désactivée" }))
    }

    async fn cmd_strategy_stage(&mut self, id: &str, stage: StrategyStage) -> Reply {
        let sim = self.terminal() == Some(TerminalKind::Simulator);
        let Some(idx) = self.strategies.iter().position(|s| s.strategy.meta().id == id) else { return Err(format!("stratégie {id} inconnue")) };
        let from = self.strategies[idx].stage;
        // Best out-of-sample evidence for this strategy, usable with the current terminal.
        let best = self
            .evidence
            .values()
            .filter(|e| e.strategy_id == id && (e.origin == DataOrigin::Broker || sim))
            .max_by(|a, b| a.trades.cmp(&b.trades));
        let fwd = self.forward.get(id).cloned().unwrap_or_default();
        let ev = Evidence {
            oos_trades: best.map(|e| e.trades).unwrap_or(0),
            oos_net: best.map(|e| e.net).unwrap_or(0.0),
            oos_origin: best.map(|e| e.origin).unwrap_or(DataOrigin::Synthetic),
            forward_trades: fwd.trades,
            forward_net: fwd.net,
        };
        can_promote(from, stage, &ev, sim)?;
        self.store.set_strategy_stage(id, stage.as_str()).await.map_err(|e| e.to_string())?;
        self.strategies[idx].stage = stage;
        self.audit("user", "strategy_stage", json!({ "id": id, "from": from.as_str(), "to": stage.as_str(), "evidence": ev })).await;
        self.log("warn", "strategy", format!("stratégie {id}: {} -> {}", from.label(), stage.label()));
        Ok(format!("stratégie {id} passée au stade « {} »", stage.label()))
    }

    async fn cmd_generate(&mut self, symbol: &str, tf: Timeframe, bars: usize, seed: u64) -> Reply {
        if !symbol.ends_with(".sim") {
            return Err("les données synthétiques ne peuvent être créées que pour un instrument fictif en « .sim »".into());
        }
        if !(300..=200_000).contains(&bars) {
            return Err("le nombre de bougies doit être compris entre 300 et 200000".into());
        }
        let mut spec = self.specs.get(symbol).cloned().unwrap_or_else(|| SymbolSpec {
            symbol: symbol.to_string(),
            digits: 5,
            point: 0.00001,
            contract_size: 100_000.0,
            tick_size: 0.00001,
            tick_value: 1.0,
            volume_min: 0.01,
            volume_max: 50.0,
            volume_step: 0.01,
            stops_level_points: 10.0,
            swap_long: -6.5,
            swap_short: 1.2,
            trade_mode: SymbolTradeMode::Full,
            currency_profit: "USD".into(), currency_base: "EUR".into(), freeze_level_points: 0.0,
        });
        spec.symbol = symbol.to_string();
        let end = tf.bucket(now_ms());
        let candles = generate(&SyntheticParams { seed, bars, timeframe: tf, start_ts: end - bars as i64 * tf.millis(), point: spec.point, ..Default::default() });
        self.store.upsert_symbol(&spec).await.map_err(|e| e.to_string())?;
        self.store.insert_candles(symbol, tf, DataOrigin::Synthetic, &candles).await.map_err(|e| e.to_string())?;
        self.specs.entry(symbol.to_string()).or_insert(spec);
        self.audit("user", "synthetic_data_generated", json!({ "symbol": symbol, "timeframe": tf.as_str(), "bars": bars, "seed": seed })).await;
        Ok(format!("{bars} bougies SYNTHÉTIQUES {symbol} {tf} générées (graine {seed})"))
    }

    async fn cmd_import(&mut self, path: &std::path::Path, symbol: &str, tf: Timeframe, offset: i64) -> Reply {
        if tf > Timeframe::H1 && offset != 0 {
            return Err("import au-delà de H1 impossible avec un décalage horaire serveur: importez du H1".into());
        }
        if !self.specs.contains_key(symbol) {
            return Err(format!("spécification de {symbol} inconnue: connectez d'abord le terminal MT5 pour la recevoir"));
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("lecture de {}: {e}", path.display()))?;
        let candles = aegis_backtest::csv::parse_candles(&text, tf, offset).map_err(|e| e.to_string())?;
        self.store.insert_candles(symbol, tf, DataOrigin::Broker, &candles).await.map_err(|e| e.to_string())?;
        self.audit("user", "csv_imported", json!({ "path": path.display().to_string(), "symbol": symbol, "timeframe": tf.as_str(), "bars": candles.len() })).await;
        Ok(format!("{} bougies {symbol} {tf} importées depuis {}", candles.len(), path.display()))
    }

    async fn cmd_export(&mut self, symbol: &str, tf: Timeframe, origin: DataOrigin, horizon: usize, path: &std::path::Path) -> Reply {
        let bars = self.store.load_candles(symbol, tf, origin, 200_000).await.map_err(|e| e.to_string())?;
        let rows = aegis_learning::build_dataset(&bars, horizon);
        if rows.is_empty() {
            return Err(format!("pas assez de bougies {symbol} {tf} ({}) pour construire un jeu de données", origin.as_str()));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, aegis_learning::dataset_to_csv(&rows)).map_err(|e| e.to_string())?;
        let meta = json!({ "symbol": symbol, "timeframe": tf.as_str(), "origin": origin.as_str(), "horizon_bars": horizon, "rows": rows.len(), "features": aegis_learning::FEATURE_NAMES });
        std::fs::write(path.with_extension("meta.json"), serde_json::to_string_pretty(&meta).unwrap_or_default()).map_err(|e| e.to_string())?;
        Ok(format!("{} lignes exportées vers {}", rows.len(), path.display()))
    }

    async fn cmd_backup(&mut self) -> Reply {
        let dir = self.cfg.engine.data_dir.join("backups");
        let name = format!("aegis-{}.db", format_ts(now_ms()).replace([' ', ':'], "-"));
        let dest = dir.join(&name);
        self.store.backup_to(&dest).await.map_err(|e| e.to_string())?;
        self.last_backup = Some(dest.display().to_string());
        self.audit("user", "backup_created", json!({ "path": dest.display().to_string() })).await;
        Ok(format!("sauvegarde créée: {}", dest.display()))
    }

    // ------------------------------------------------------------ backtests

    async fn start_backtest(
        &mut self,
        strategy_id: String,
        symbol: String,
        tf: Timeframe,
        origin: DataOrigin,
        params: BacktestParams,
        reply: oneshot::Sender<Reply>,
    ) -> Result<(), String> {
        let prepared: Result<_, String> = async {
            let slot = self.strategies.iter().find(|s| s.strategy.meta().id == strategy_id).ok_or_else(|| format!("stratégie {strategy_id} inconnue"))?;
            let config = slot.strategy.config();
            let spec = self.specs.get(&symbol).cloned().ok_or_else(|| format!("spécification de {symbol} inconnue"))?;
            if !(params.initial_equity > 0.0 && params.risk_per_trade_pct > 0.0 && params.risk_per_trade_pct <= 5.0) {
                return Err("paramètres de backtest invalides".to_string());
            }
            let bars = self.store.load_candles(&symbol, tf, origin, 200_000).await.map_err(|e| e.to_string())?;
            if bars.len() < 500 {
                return Err(format!("seulement {} bougies {symbol} {tf} ({}): 500 au minimum", bars.len(), origin.as_str()));
            }
            Ok((config, spec, bars))
        }
        .await;
        let (config, spec, bars) = match prepared {
            Ok(v) => v,
            Err(e) => {
                let _ = reply.send(Err(e.clone()));
                return Err(e);
            }
        };
        let tx = self.internal_tx.clone();
        self.log("info", "backtest", format!("backtest {strategy_id} {symbol} {tf} lancé sur {} bougies ({})", bars.len(), origin.as_str()));
        tokio::task::spawn_blocking(move || {
            let outcome = (|| {
                let strategy = build(&strategy_id, &config).map_err(|e| e.to_string())?;
                let full = run(strategy.as_ref(), &spec, &bars, &params);
                let segments = run_segmented(strategy.as_ref(), &spec, &bars, &params, 0.6, 0.2);
                let wf = walk_forward(strategy.as_ref(), &spec, &bars, &params, 5);
                let segments_v: Vec<BacktestSegmentView> = segments.iter().map(seg_view).collect();
                let wf_v: Vec<BacktestSegmentView> = wf.iter().map(seg_view).collect();
                let run_row = BacktestRunRow {
                    run_id: new_id("bt"),
                    ts_ms: now_ms(),
                    strategy_id: strategy_id.clone(),
                    symbol: symbol.clone(),
                    timeframe: tf.as_str().into(),
                    origin,
                    params: json!({ "backtest": params, "strategy": config, "bars": bars.len(), "from": bars.first().map(|b| b.open_ts), "to": bars.last().map(|b| b.open_ts) }),
                    metrics: json!({ "full": full.metrics, "segments": segments_v, "walk_forward": wf_v }),
                };
                Ok(Box::new(BacktestDetail {
                    run: run_row,
                    full: full.metrics,
                    segments: segments_v,
                    walk_forward: wf_v,
                    equity_curve: full.equity_curve,
                    trades: full.trades,
                }))
            })();
            let _ = tx.blocking_send(Internal::BacktestDone { reply, outcome });
        });
        Ok(())
    }

    pub(crate) async fn on_internal(&mut self, msg: Internal) {
        match msg {
            Internal::DbPing(result) => {
                self.ping_in_flight = false;
                match result {
                    Ok(()) => {
                        if !self.db_ok {
                            self.db_ok = true;
                            self.log("info", "data", "base de données de nouveau disponible".into());
                            self.audit("engine", "database_recovered", json!({})).await;
                        }
                    }
                    Err(e) => self.db_failed(&e),
                }
            }
            Internal::NewsFetched(results) => self.on_news(results).await,
            Internal::SweepDone { reply, outcome } => {
                self.on_sweep_done(reply, outcome).await;
                self.publish().await;
            }
            Internal::BrainRetrained { reply, results } => {
                let out = self.on_brain_retrained(results).await;
                self.publish().await;
                if let Some(r) = reply {
                    let _ = r.send(out);
                }
            }
            Internal::ProbabilityDone { reply, outcome } => {
                let out = self.on_probability_trained(outcome).await;
                if let Err(e) = &out {
                    self.log("warn", "probability", format!("entraînement refusé: {e}"));
                }
                self.cache.dirty = true;
                self.publish().await;
                let _ = reply.send(out);
            }
            Internal::BacktestDone { reply, outcome } => {
                let out = match outcome {
                    Ok(detail) => {
                        let trades: Vec<(String, serde_json::Value)> =
                            detail.trades.iter().map(|t| ("full".to_string(), serde_json::to_value(t).unwrap_or_default())).collect();
                        match self.store.save_backtest(&detail.run, &trades).await {
                            Ok(()) => {
                                if let Some(ev) = evidence_from_run(&detail.run) {
                                    self.evidence.insert(evidence_key(&ev.strategy_id, &ev.symbol, &ev.timeframe), ev);
                                }
                                let m = &detail.full;
                                let msg = format!(
                                    "backtest {} terminé: {} transactions, résultat net {:+.2}, drawdown max {:.2} % ({})",
                                    detail.run.run_id,
                                    m.trades,
                                    m.net_profit,
                                    m.max_drawdown_pct,
                                    if detail.run.origin == DataOrigin::Synthetic { "données SYNTHÉTIQUES" } else { "données broker" }
                                );
                                self.audit("user", "backtest_run", json!({ "run_id": detail.run.run_id, "strategy": detail.run.strategy_id, "symbol": detail.run.symbol, "origin": detail.run.origin.as_str(), "trades": m.trades, "net": m.net_profit })).await;
                                self.log("info", "backtest", msg.clone());
                                self.last_backtest = Some(*detail);
                                Ok(msg)
                            }
                            Err(e) => Err(format!("backtest calculé mais non enregistré: {e}")),
                        }
                    }
                    Err(e) => Err(e),
                };
                self.cache.dirty = true;
                self.publish().await;
                let _ = reply.send(out);
            }
        }
    }

    /// Seed strategy rows that may be missing (used at start-up and by tests).
    #[allow(dead_code)]
    pub(crate) async fn ensure_strategy_row(&mut self, id: &str, name: &str) -> Result<(), String> {
        self.store
            .upsert_strategy(&StrategyRow { strategy_id: id.into(), name: name.into(), stage: "research".into(), enabled: true, config: json!({}) })
            .await
            .map_err(|e| e.to_string())
    }
}
