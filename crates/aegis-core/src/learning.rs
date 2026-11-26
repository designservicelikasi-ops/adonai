//! Fast Learning: parallel parameter research, data audits and automatic rollback of a
//! probability model whose live calibration drifts (Champion / Challenger safety net).

use crate::command::Reply;
use crate::engine::Engine;
use aegis_backtest::sweep::{SweepReport, SweepRequest};
use aegis_data::JournalEntry;
use aegis_market::audit::DataAudit;
use aegis_types::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::oneshot;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModelsSection {
    /// Roll a model back (or retire it) when its live calibration drifts.
    pub auto_rollback: bool,
    /// Live trades needed before judging a model.
    pub drift_min_trades: usize,
    /// Live Brier allowed above the model's out-of-sample Brier.
    pub drift_brier_tolerance: f64,
    /// Threads a parameter sweep may use (never more than the machine minus one).
    pub research_threads: usize,
    pub max_sweep_combinations: usize,
}

impl Default for ModelsSection {
    fn default() -> Self {
        ModelsSection { auto_rollback: true, drift_min_trades: 30, drift_brier_tolerance: 0.08, research_threads: 4, max_sweep_combinations: 400 }
    }
}

impl ModelsSection {
    pub fn validate(&self) -> Result<(), String> {
        if !(10..=10_000).contains(&self.drift_min_trades) || !(0.01..=0.5).contains(&self.drift_brier_tolerance) || !(1..=64).contains(&self.research_threads) || !(1..=10_000).contains(&self.max_sweep_combinations) {
            return Err("models: drift_min_trades within 10..10000, drift_brier_tolerance within 0.01..0.5, research_threads within 1..64, max_sweep_combinations within 1..10000".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriftVerdict {
    pub model_id: String,
    pub trades: usize,
    pub live_brier: f64,
    pub reference_brier: f64,
    pub drifting: bool,
    pub detail: String,
}

/// Live calibration of `model_id` on the closed trades that used it.
pub fn assess_drift(closed: &[JournalEntry], model_id: &str, reference_brier: f64, min_trades: usize, tolerance: f64) -> Option<DriftVerdict> {
    let preds: Vec<(f64, f64)> = closed
        .iter()
        .filter(|e| e.closed_ms.is_some() && e.probability_source.contains(model_id))
        .filter_map(|e| e.probability.map(|p| (p, if e.net > 0.0 { 1.0 } else { 0.0 })))
        .collect();
    if preds.is_empty() {
        return None;
    }
    let live = aegis_probability::metrics::brier(&preds);
    let enough = preds.len() >= min_trades;
    let drifting = enough && live > reference_brier + tolerance;
    Some(DriftVerdict {
        model_id: model_id.into(),
        trades: preds.len(),
        live_brier: live,
        reference_brier,
        drifting,
        detail: if !enough {
            format!("{} transaction(s) réelle(s) sur {min_trades}: données insuffisantes pour juger", preds.len())
        } else if drifting {
            format!("Brier réel {live:.3} > hors échantillon {reference_brier:.3} + {tolerance:.2}: dérive")
        } else {
            format!("Brier réel {live:.3}, hors échantillon {reference_brier:.3}: conforme")
        },
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LearningView {
    pub sweeps: Vec<SweepReport>,
    pub sweep_running: bool,
    pub audits: Vec<(String, String, String, DataAudit)>,
    pub drift: Vec<DriftVerdict>,
    pub rollbacks: Vec<String>,
    pub settings: ModelsSection,
    pub machine_threads: usize,
}

#[derive(Default)]
pub(crate) struct LearningState {
    pub sweeps: std::collections::VecDeque<SweepReport>,
    pub sweep_running: bool,
    pub audits: std::collections::VecDeque<(String, String, String, DataAudit)>,
    pub drift: Vec<DriftVerdict>,
    pub rollbacks: Vec<String>,
}

impl Engine {
    /// Called after the journal is refreshed: judge every active probability model on its live trades.
    pub(crate) async fn check_model_drift(&mut self, closed: &[JournalEntry]) {
        let mc = self.cfg.models.clone();
        let mut verdicts = Vec::new();
        for ((symbol, tf), m) in self.prob_models.clone() {
            let refs: Vec<f64> = [Side::Buy, Side::Sell].iter().filter_map(|s| m.side(*s).report().map(|r| r.brier)).collect();
            let Some(reference) = refs.iter().cloned().reduce(f64::max) else { continue };
            let Some(v) = assess_drift(closed, &m.model_id, reference, mc.drift_min_trades, mc.drift_brier_tolerance) else { continue };
            if v.drifting && mc.auto_rollback && !self.learning.rollbacks.iter().any(|r| r.contains(&m.model_id)) {
                let previous = self
                    .store
                    .probability_models(None)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|r| r.symbol == symbol && r.timeframe == tf.as_str() && r.model_id != m.model_id && r.created_ms < m.created_ms)
                    .max_by_key(|r| r.created_ms);
                let msg = match previous {
                    Some(p) => {
                        let r = self.activate_probability_model(&p.model_id).await;
                        format!("modèle {} en dérive ({}) — retour automatique à {}: {}", m.model_id, v.detail, p.model_id, r.unwrap_or_else(|e| e))
                    }
                    None => {
                        self.prob_models.remove(&(symbol.clone(), tf));
                        let _ = self.store.deactivate_probability_model(&m.model_id).await;
                        format!("modèle {} en dérive ({}) — retiré: aucune version précédente, aucune probabilité tant qu'un modèle n'est pas validé", m.model_id, v.detail)
                    }
                };
                self.audit("learning", "probability_model_rollback", json!({ "model_id": m.model_id, "verdict": v })).await;
                self.alert("warn", "learning", msg.clone());
                self.learning.rollbacks.push(msg);
            }
            verdicts.push(v);
        }
        self.learning.drift = verdicts;
    }

    pub(crate) async fn start_sweep(&mut self, mut req: SweepRequest, symbol: String, timeframe: Timeframe, origin: DataOrigin, reply: oneshot::Sender<Reply>) {
        if self.learning.sweep_running {
            let _ = reply.send(Err("une recherche de paramètres est déjà en cours".into()));
            return;
        }
        if origin == DataOrigin::Synthetic && !symbol.ends_with(".sim") {
            let _ = reply.send(Err("données synthétiques: seulement pour un instrument de test (.sim)".into()));
            return;
        }
        let Some(spec) = self.specs.get(&symbol).cloned() else {
            let _ = reply.send(Err(format!("spécification broker absente pour {symbol}")));
            return;
        };
        let bars = match self.store.load_candles(&symbol, timeframe, origin, 1_000_000).await {
            Ok(b) => b,
            Err(e) => {
                let _ = reply.send(Err(e.to_string()));
                return;
            }
        };
        if req.base_config.is_null() {
            req.base_config = self.strategies.iter().find(|s| s.strategy.meta().id == req.strategy_id).map(|s| s.strategy.config()).unwrap_or_default();
        }
        req.max_threads = req.max_threads.min(self.cfg.models.research_threads).max(1);
        req.max_combinations = self.cfg.models.max_sweep_combinations;
        let rc = self.risk.config();
        let params = aegis_backtest::BacktestParams {
            risk_per_trade_pct: rc.risk_per_trade_pct,
            commission_per_lot_round_turn: rc.commission_per_lot_round_turn,
            slippage_points: rc.expected_slippage_points,
            ..Default::default()
        };
        self.learning.sweep_running = true;
        let tx = self.internal_tx.clone();
        tokio::task::spawn_blocking(move || {
            let build = |id: &str, c: &serde_json::Value| aegis_strategy::build(id, c).map_err(|e| e.to_string());
            let outcome = aegis_backtest::sweep::sweep(&req, &spec, &bars, &params, &build).map(Box::new);
            let _ = tx.blocking_send(crate::command::Internal::SweepDone { reply, outcome });
        });
    }

    pub(crate) async fn on_sweep_done(&mut self, reply: oneshot::Sender<Reply>, outcome: Result<Box<SweepReport>, String>) {
        self.learning.sweep_running = false;
        let r = match outcome {
            Ok(rep) => {
                let best = rep.points.first().map(|p| format!("meilleur en apprentissage: net {:.2}, contrôle {:.2}{}", p.in_sample.net_profit, p.holdout.net_profit, if p.overfit || p.unstable { " (à écarter)" } else { "" })).unwrap_or_default();
                let msg = format!("{} combinaison(s) évaluée(s) sur {} fil(s) en {} ms; {best}", rep.evaluated, rep.threads, rep.elapsed_ms);
                self.audit("user", "parameter_sweep", json!({ "strategy": rep.strategy_id, "combinations": rep.combinations, "threads": rep.threads })).await;
                self.learning.sweeps.push_front(*rep);
                self.learning.sweeps.truncate(10);
                // Parameters are never changed by a sweep: it only proposes (Strategy Lab, then the user).
                Ok(msg)
            }
            Err(e) => Err(e),
        };
        let _ = reply.send(r);
    }

    pub(crate) async fn cmd_audit_data(&mut self, symbol: &str, tf: Timeframe, origin: DataOrigin) -> Reply {
        let bars = self.store.load_candles(symbol, tf, origin, 1_000_000).await.map_err(|e| e.to_string())?;
        let a = aegis_market::audit::audit(&bars, tf);
        let msg = format!("{} bougies {symbol} {tf} ({}): {} manquante(s), {} doublon(s), {} aberrante(s) — {}", a.bars, origin.as_str(), a.missing_bars, a.duplicates, a.outliers.len(), a.verdict);
        self.learning.audits.push_front((symbol.into(), tf.as_str().into(), origin.as_str().into(), a));
        self.learning.audits.truncate(20);
        Ok(msg)
    }

    pub(crate) fn learning_view(&self) -> LearningView {
        LearningView {
            sweeps: self.learning.sweeps.iter().cloned().collect(),
            sweep_running: self.learning.sweep_running,
            audits: self.learning.audits.iter().cloned().collect(),
            drift: self.learning.drift.clone(),
            rollbacks: self.learning.rollbacks.clone(),
            settings: self.cfg.models.clone(),
            machine_threads: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(model: &str, p: f64, win: bool) -> JournalEntry {
        JournalEntry { probability: Some(p), probability_source: format!("modèle {model}"), net: if win { 1.0 } else { -1.0 }, closed_ms: Some(1), ..Default::default() }
    }

    #[test]
    fn drift_needs_enough_trades_and_a_clear_degradation() {
        let few: Vec<JournalEntry> = (0..10).map(|_| trade("m1", 0.8, false)).collect();
        let v = assess_drift(&few, "m1", 0.20, 30, 0.08).unwrap();
        assert!(!v.drifting && v.detail.contains("insuffisantes"));
        // 40 trades promised at 80 %, a quarter won: Brier ~0.49 vs 0.20 out of sample.
        let bad: Vec<JournalEntry> = (0..40).map(|i| trade("m1", 0.8, i % 4 == 0)).collect();
        assert!(assess_drift(&bad, "m1", 0.20, 30, 0.08).unwrap().drifting);
        // Calibrated: 80 % promised, 80 % won.
        let good: Vec<JournalEntry> = (0..40).map(|i| trade("m1", 0.8, i % 5 != 0)).collect();
        assert!(!assess_drift(&good, "m1", 0.20, 30, 0.08).unwrap().drifting);
        // Trades of another model are not counted.
        assert!(assess_drift(&bad, "m2", 0.20, 30, 0.08).is_none());
    }
}
