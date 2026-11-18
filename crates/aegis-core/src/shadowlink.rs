//! Link between the live engine and the independent shadow engine.
//! The engine only SENDS copies; the shadow never receives a way back.

use crate::command::Reply;
use crate::engine::Engine;
use aegis_shadow::{PipelineStage, Role, ShadowDecision, ShadowInput, ShadowTrade};
use aegis_strategy::{build, StrategyStage};
use aegis_types::*;
use serde_json::{json, Value};
use std::sync::Arc;

impl Engine {
    fn to_shadow(&self, msg: ShadowInput) {
        if let Some(s) = &self.shadow {
            // Never block the live engine: a full queue drops the copy (reported in the view).
            if s.tx.try_send(msg).is_err() {
                self.shadow_dropped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    pub(crate) fn shadow_champions(&self) {
        let list = self.strategies.iter().map(|s| (s.strategy.meta().id.clone(), s.strategy.config(), s.enabled && s.stage != StrategyStage::Disabled)).collect();
        self.to_shadow(ShadowInput::Champions(list));
    }

    pub(crate) fn shadow_config(&self) {
        let mut c = self.cfg.shadow.clone();
        c.commission_per_lot_round_turn = self.risk.config().commission_per_lot_round_turn;
        self.to_shadow(ShadowInput::Config(c));
    }

    pub(crate) fn shadow_spec(&self, spec: &SymbolSpec) {
        self.to_shadow(ShadowInput::Spec(spec.clone()));
    }

    pub(crate) fn shadow_bar(&self, symbol: &str, tf: Timeframe) {
        let bars = self.market.candles(symbol, tf);
        let w = aegis_strategy::window(&bars).to_vec();
        self.to_shadow(ShadowInput::Bar { symbol: symbol.into(), timeframe: tf, window: Arc::new(w) });
    }

    pub(crate) fn shadow_decision(&self, d: &crate::view::DecisionView) {
        self.to_shadow(ShadowInput::Decision(ShadowDecision {
            decision_id: d.decision_id.clone(),
            ts_ms: d.ts_ms,
            symbol: d.symbol.clone(),
            timeframe: d.timeframe,
            strategy_id: d.strategy_id.clone(),
            side: d.side,
            entry: d.entry,
            sl: d.sl,
            tp: d.tp,
            outcome: d.outcome.clone(),
            failed_checks: d.engine_checks.iter().chain(d.assessment.checks.iter()).filter(|c| !c.passed && c.name != "mode").map(|c| c.name.clone()).collect(),
        }));
    }

    pub(crate) async fn shadow_trade(&self, ticket: i64) {
        if let Ok(Some(j)) = self.store.journal_get(ticket).await {
            if let (Some(side), Some(exit)) = (j.side, j.exit_price) {
                self.to_shadow(ShadowInput::TradeClosed(ShadowTrade {
                    ticket,
                    strategy_id: j.strategy_id,
                    side,
                    entry: j.entry_filled,
                    exit,
                    sl_initial: j.sl_initial,
                    tp_initial: j.tp_initial,
                    mfe_price: j.mfe_price,
                    mae_price: j.mae_price,
                    net: j.net,
                }));
            }
        }
    }

    pub(crate) async fn persist_shadow(&mut self, now: TimestampMs) {
        if now - self.shadow_saved_ms < 300_000 {
            return;
        }
        self.shadow_saved_ms = now;
        if let Some(s) = &self.shadow {
            let report = s.report.borrow().clone();
            let _ = self.store.kv_set("shadow_report", &*report).await;
        }
    }

    /// Send stored candles so the shadow can backtest every candidate (in its own task).
    pub(crate) async fn cmd_shadow_history(&mut self, symbol: &str, tf: Timeframe, origin: DataOrigin) -> Reply {
        if self.shadow.is_none() {
            return Err("Shadow désactivé dans la configuration".into());
        }
        let bars = self.store.load_candles(symbol, tf, origin, 200_000).await.map_err(|e| e.to_string())?;
        if bars.len() < 500 {
            return Err(format!("seulement {} bougies {symbol} {tf} ({}): 500 au minimum", bars.len(), origin.as_str()));
        }
        if let Some(spec) = self.specs.get(symbol) {
            self.shadow_spec(spec);
        }
        let n = bars.len();
        self.to_shadow(ShadowInput::History { symbol: symbol.into(), timeframe: tf, origin: origin.as_str().into(), bars: Arc::new(bars) });
        self.audit("user", "shadow_history", json!({ "symbol": symbol, "timeframe": tf.as_str(), "origin": origin.as_str(), "bars": n })).await;
        Ok(format!("évaluation Shadow lancée sur {n} bougies {symbol} {tf} ({})", origin.as_str()))
    }

    /// Change a strategy's parameters. Its evidence was earned with other parameters:
    /// the strategy goes back to the research stage.
    pub(crate) async fn cmd_strategy_config(&mut self, id: &str, config: Value, actor: &str) -> Reply {
        let idx = self.strategies.iter().position(|s| s.strategy.meta().id == id).ok_or_else(|| format!("stratégie {id} inconnue"))?;
        let built = build(id, &config).map_err(|e| e.to_string())?;
        let normalized = built.config();
        let name = built.meta().name.clone();
        let enabled = self.strategies[idx].enabled;
        self.store
            .upsert_strategy(&aegis_data::StrategyRow { strategy_id: id.into(), name, stage: "research".into(), enabled, config: normalized.clone() })
            .await
            .map_err(|e| e.to_string())?;
        self.store.set_strategy_stage(id, "research").await.map_err(|e| e.to_string())?;
        let from = self.strategies[idx].stage;
        self.strategies[idx].strategy = built;
        self.strategies[idx].stage = StrategyStage::Research;
        self.evidence.retain(|_, e| e.strategy_id != id);
        self.audit(actor, "strategy_config", json!({ "id": id, "config": normalized, "previous_stage": from.as_str() })).await;
        self.log("warn", "strategy", format!("stratégie {id}: nouveaux paramètres, retour au stade Recherche"));
        self.shadow_champions();
        Ok(format!("paramètres de {id} enregistrés; stade Recherche (les preuves précédentes ne s'appliquent plus)"))
    }

    /// Promote a challenger that passed BACKTEST -> OUT-OF-SAMPLE -> FORWARD: its
    /// parameters replace the strategy's and the strategy goes to DEMO only.
    /// LIVE still requires the usual forward evidence on the demo account.
    pub(crate) async fn cmd_promote_challenger(&mut self, candidate_id: &str) -> Reply {
        let report = self.shadow.as_ref().map(|s| s.report.borrow().clone()).ok_or("Shadow désactivé")?;
        let c = report.candidates.iter().find(|c| c.id == candidate_id).ok_or_else(|| format!("candidat {candidate_id} introuvable"))?;
        if c.role != Role::Challenger {
            return Err("seul un challenger peut être promu".into());
        }
        if c.stage != PipelineStage::Forward {
            return Err(format!("candidat non validé ({:?}): {}", c.stage, c.verdict));
        }
        let sim = self.terminal() == Some(TerminalKind::Simulator);
        if !sim && !c.oos.iter().any(|o| o.origin == "broker" && o.oos_net > 0.0) {
            return Err("les preuves hors échantillon reposent sur des données synthétiques: données broker requises".into());
        }
        let (sid, cfg) = (c.strategy_id.clone(), c.config.clone());
        self.cmd_strategy_config(&sid, cfg.clone(), "shadow-promotion").await?;
        self.store.set_strategy_stage(&sid, "demo").await.map_err(|e| e.to_string())?;
        if let Some(slot) = self.strategies.iter_mut().find(|s| s.strategy.meta().id == sid) {
            slot.stage = StrategyStage::Demo;
        }
        self.audit("user", "challenger_promoted", json!({ "candidate": candidate_id, "strategy": sid, "config": cfg, "forward": c.forward, "oos": c.oos })).await;
        self.log("warn", "shadow", format!("challenger {candidate_id} promu: {sid} en DÉMO avec ses paramètres"));
        Ok(format!("{candidate_id} promu: {sid} passe en Démo avec les paramètres du challenger"))
    }
}
