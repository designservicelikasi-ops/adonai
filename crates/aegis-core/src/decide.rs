//! From a closed candle to an order: strategy signal, engine checks, costs,
//! risk assessment, persistence and (only when everything passes) execution.

use crate::engine::{evidence_key, Engine};
use crate::view::{DecisionView, ForwardStats};
use aegis_data::DecisionRecord;
use aegis_market::ClosedBar;
use aegis_risk::{Check, RiskContext, TradeProposal};
use aegis_strategy::{window, OpenView, SignalIntent, StrategyAction, StrategyStage};
use aegis_types::*;
use serde_json::json;

fn check(name: &str, passed: bool, detail: impl Into<String>) -> Check {
    Check { name: name.to_string(), passed, detail: detail.into() }
}

impl Engine {
    pub(crate) async fn load_forward(&mut self) {
        let kind = if self.terminal() == Some(TerminalKind::Simulator) { "simulator" } else { "mt5" };
        self.forward.clear();
        let ids: Vec<String> = self.strategies.iter().map(|s| s.strategy.meta().id.clone()).collect();
        for id in ids {
            if let Ok(Some(f)) = self.store.kv_get::<ForwardStats>(&format!("forward:{kind}:{id}")).await {
                self.forward.insert(id, f);
            }
        }
    }

    pub(crate) async fn on_closed_bar(&mut self, bar: ClosedBar) {
        let origin = self.origin();
        if self.db_ok {
            if let Err(e) = self.store.insert_candles(&bar.symbol, bar.timeframe, origin, &[bar.candle]).await {
                self.db_failed(&e.to_string());
            }
        }
        let bars = self.market.candles(&bar.symbol, bar.timeframe);

        // Model inference on the bar that just closed.
        if let Some(l) = self.model.as_mut() {
            if l.artifact.symbol == bar.symbol && l.artifact.timeframe == bar.timeframe.as_str() {
                if let Some(f) = aegis_learning::compute_features(window(&bars)) {
                    if let Some(p) = l.artifact.predict(&f) {
                        l.monitor.on_bar(bar.candle.open_ts, bar.candle.close, &f, p);
                    }
                }
            }
        }

        if !self.cfg.engine.signal_timeframes.contains(&bar.timeframe) {
            return;
        }
        self.refresh_entry_state(&bar.symbol, bar.timeframe);
        for idx in 0..self.strategies.len() {
            let (id, runs) = {
                let s = &self.strategies[idx];
                let m = s.strategy.meta();
                (m.id.clone(), s.enabled && s.stage != StrategyStage::Disabled && m.supports_timeframe(bar.timeframe) && m.supports_symbol(&bar.symbol))
            };
            if !runs {
                continue;
            }
            let held = self.positions.iter().find(|p| p.symbol == bar.symbol && self.owners.get(&p.ticket) == Some(&id)).cloned();
            let view = held.as_ref().map(|p| OpenView {
                side: p.side,
                entry_price: p.price_open,
                bars_held: ((bar.candle.open_ts - p.open_ts).max(0) / bar.timeframe.millis()) as usize,
            });
            match self.strategies[idx].strategy.evaluate(window(&bars), view.as_ref()) {
                StrategyAction::Enter(intent) => {
                    self.strategies[idx].signals += 1;
                    self.handle_entry(idx, &bar.symbol, bar.timeframe, bar.candle.open_ts, intent).await;
                }
                StrategyAction::Exit { reason } => {
                    if let Some(p) = held {
                        if self.mode.can_execute() {
                            let _ = self.close_position(&p, None, &format!("sortie de la stratégie {id}: {reason}")).await;
                        } else {
                            self.log("info", "strategy", format!("{id}: signal de sortie sur {} ({reason}) — non exécuté en mode MONITOR", p.symbol));
                        }
                    }
                }
                StrategyAction::None => {}
            }
        }
        self.manage_positions(&bar.symbol, bar.timeframe).await;
    }

    async fn handle_entry(&mut self, idx: usize, symbol: &str, tf: Timeframe, bar_ts: TimestampMs, intent: SignalIntent) {
        let now = now_ms();
        let decision_id = new_id("dec");
        let (strategy_id, stage, regimes_ok, regime) = {
            let s = &self.strategies[idx];
            let regime = self.market.snapshot(symbol, now).map(|m| m.regime(tf)).unwrap_or(Regime::Unknown);
            (s.strategy.meta().id.clone(), s.stage, s.strategy.meta().supports_regime(regime), regime)
        };
        let terminal = self.terminal();
        let spec = self.specs.get(symbol).cloned();
        let last = self.market.last_tick(symbol);
        let sign = intent.side.sign();

        // ---- engine-level checks (recorded with the decision)
        let mut checks = Vec::new();
        checks.push(check("base_de_donnees", self.db_ok, if self.db_ok { "base accessible" } else { "base de données indisponible: aucune entrée possible" }));
        let stage_ok = match self.mode {
            Mode::Monitor => true,
            Mode::DemoAuto => matches!(stage, StrategyStage::Demo | StrategyStage::LiveApproved),
            Mode::LiveAuto => stage == StrategyStage::LiveApproved,
        };
        checks.push(check("stade_strategie", stage_ok, format!("stratégie au stade « {} » en mode {}", stage.label(), self.mode.label())));
        checks.push(check("regime_marche", regimes_ok, format!("régime actuel: {}", regime.label())));
        let quality = self.market.quality(symbol, now);
        let blocking: Vec<String> = quality.iter().filter(|i| i.blocks_trading()).map(|i| i.describe()).collect();
        checks.push(check("qualite_donnees", blocking.is_empty(), if blocking.is_empty() { "données de marché valides".to_string() } else { blocking.join("; ") }));
        let busy = self.inflight.values().any(|s| s == symbol);
        checks.push(check("ordre_en_cours", !busy, if busy { "un ordre est déjà en cours sur cet instrument" } else { "aucun ordre en cours" }));

        // AI filter: only a validated model may veto; an unvalidated one is ignored.
        match &self.model {
            Some(l) if l.artifact.symbol == symbol && l.artifact.timeframe == tf.as_str() => {
                match (l.gate.passed, l.monitor.last_prediction) {
                    (true, Some((ts, p))) if ts == bar_ts => {
                        let agrees = if intent.side == Side::Buy { p >= 0.5 } else { p <= 0.5 };
                        checks.push(check("filtre_ia", agrees, format!("modèle validé {}: probabilité de hausse {:.3}", l.artifact.model_id, p)));
                    }
                    (true, _) => checks.push(check("filtre_ia", false, "modèle validé mais sans prédiction pour cette bougie")),
                    (false, _) => checks.push(check("filtre_ia", true, "modèle non validé: ignoré")),
                }
            }
            _ => checks.push(check("filtre_ia", true, "aucun modèle applicable")),
        }

        // ---- win probability from out-of-sample evidence
        let (win_probability, probability_source) = match self.evidence.get(&evidence_key(&strategy_id, symbol, tf.as_str())) {
            Some(ev) => {
                let usable = ev.origin == DataOrigin::Broker || terminal == Some(TerminalKind::Simulator);
                match (usable, ev.win_probability) {
                    (true, Some(p)) => (Some(p), format!("backtest hors échantillon {}: {}/{} gagnants, borne basse de Wilson", ev.run_id, ev.wins, ev.trades)),
                    (true, None) => (None, format!("backtest {}: seulement {} transactions hors échantillon (minimum 30)", ev.run_id, ev.trades)),
                    (false, _) => (None, "backtest sur données synthétiques: inutilisable avec un vrai broker".to_string()),
                }
            }
            None => (None, "aucun backtest hors échantillon pour cette stratégie/instrument/unité de temps".to_string()),
        };

        // ---- P(TP before SL) from the scenario model; the most prudent validated source wins.
        let (scenario_p, scenario_src) = self.scenario_probability(symbol, tf, intent.side, intent.tp_distance, intent.sl_distance);
        let (win_probability, probability_source) = match (win_probability, scenario_p) {
            (Some(a), Some(b)) if b < a => (Some(b), format!("{scenario_src} (plus prudent que: {probability_source})")),
            (Some(a), Some(_)) => (Some(a), format!("{probability_source} (plus prudent que: {scenario_src})")),
            (None, Some(b)) => (Some(b), scenario_src),
            (Some(a), None) => (Some(a), format!("{probability_source}; {scenario_src}")),
            (None, None) => (None, format!("{probability_source}; {scenario_src}")),
        };

        // ---- economic news: suspended = no entry; caution = a higher probability is required.
        let news = self.news_assessment(symbol, now);
        {
            let rc = self.risk.config();
            let min_p = if intent.side == Side::Buy { rc.buy_min_probability } else { rc.sell_min_probability };
            let margin = self.cfg.news.caution_probability_margin;
            let (ok, detail) = match news.recommendation {
                aegis_news::Recommendation::Allowed => (true, news.reasons.join(" ; ")),
                aegis_news::Recommendation::Suspended => (false, format!("{}: {}", news.recommendation.label(), news.reasons.join(" ; "))),
                aegis_news::Recommendation::Caution => {
                    let needed = (min_p + margin).min(0.99);
                    let ok = win_probability.is_some_and(|p| p >= needed);
                    (ok, format!("{}: {} — probabilité exigée {:.1} %", news.recommendation.label(), news.reasons.join(" ; "), needed * 100.0))
                }
            };
            checks.push(check("actualites", ok, detail));
        }

        // ---- levels from the real quote
        let (entry, sl, tp) = match (&spec, &last) {
            (Some(s), Some((t, _))) => {
                let e = if intent.side == Side::Buy { t.ask } else { t.bid };
                (e, s.round_price(e - sign * intent.sl_distance), s.round_price(e + sign * intent.tp_distance))
            }
            _ => (0.0, 0.0, 0.0),
        };
        let proposal = TradeProposal {
            decision_id: decision_id.clone(),
            strategy_id: strategy_id.clone(),
            symbol: symbol.to_string(),
            side: intent.side,
            kind: OrderKind::Market,
            entry_price: 0.0,
            sl,
            tp,
            win_probability,
            probability_source,
            claimed_risk_amount: None,
            requested_volume: None,
        };
        // Freshness is judged on the engine clock (receive time), not the broker clock.
        let fresh_tick = last.map(|(t, received)| Tick { ts_ms: received, ..t });
        let mut assessment = {
            let ctx = RiskContext {
                now_ms: now,
                mode: self.mode,
                connected: self.conn.is_some(),
                terminal,
                account: self.account.as_ref(),
                specs: &self.specs,
                tick: fresh_tick.as_ref(),
                positions: &self.positions,
                pending: &self.pending,
                day: self.accounting.as_ref().map(|a| &a.day),
                peak_equity: self.accounting.as_ref().map(|a| a.peak_equity).unwrap_or(0.0),
                live_armed: self.live_armed,
            };
            self.risk.evaluate(&proposal, &ctx)
        };
        let engine_ok = checks.iter().all(|c| c.passed);
        if !engine_ok {
            // An authorization minted while an engine-level check failed must not exist.
            assessment.authorization = None;
            assessment.approved = false;
        }
        let outcome = if !self.mode.can_execute() {
            "observed"
        } else if assessment.approved {
            "approved"
        } else {
            "rejected"
        };

        let mut dv = DecisionView {
            decision_id: decision_id.clone(),
            ts_ms: now,
            symbol: symbol.to_string(),
            timeframe: tf,
            strategy_id: strategy_id.clone(),
            side: intent.side,
            mode: self.mode,
            reason: intent.reason.clone(),
            evidence: intent.evidence.clone(),
            entry,
            sl,
            tp,
            outcome: outcome.to_string(),
            engine_checks: checks,
            assessment,
            cid: None,
        };

        // ---- persist the decision BEFORE anything is sent
        let record = DecisionRecord {
            decision_id: decision_id.clone(),
            ts_ms: now,
            symbol: symbol.to_string(),
            strategy_id: strategy_id.clone(),
            side: intent.side,
            mode: format!("{:?}", self.mode),
            signal: json!({ "timeframe": tf.as_str(), "bar": bar_ts, "reason": intent.reason, "evidence": intent.evidence, "regime": regime.as_str(), "entry": entry, "sl": sl, "tp": tp }),
            outcome: outcome.to_string(),
            verdict: Some(json!({ "engine_checks": dv.engine_checks, "risk": dv.assessment })),
        };
        // A decision that cannot be recorded can never lead to an order.
        let persisted = self.db_ok
            && match self.store.insert_decision(&record).await {
                Ok(()) => true,
                Err(e) => {
                    self.db_failed(&format!("décision: {e}"));
                    false
                }
            };

        if outcome == "approved" && persisted {
            let auth = dv.assessment.authorization.clone().expect("approved decisions carry an authorization");
            let point = spec.as_ref().map(|s| s.point).unwrap_or(0.0);
            match self.executor.submit_entry(&mut self.risk, &auth, point, now).await {
                Ok(body) => {
                    let cid = match &body {
                        aegis_protocol::Body::Order { cid, .. } => cid.clone(),
                        _ => String::new(),
                    };
                    match self.send_order(body) {
                        Ok(()) => {
                            let _ = self.executor.mark_sent(&cid).await;
                            self.inflight.insert(cid.clone(), symbol.to_string());
                            let terminal_kind = if terminal == Some(TerminalKind::Simulator) { "simulator" } else { "mt5" };
                            let seed = crate::intel::seed_journal(&decision_id, &cid, &strategy_id, symbol, tf, intent.side, regime.as_str(), terminal_kind, entry, sl, tp, dv.assessment.micro.as_ref());
                            self.journal_seeds.insert(cid.clone(), seed);
                            let spread_cost = dv.assessment.micro.as_ref().map(|m| m.costs.spread).unwrap_or(0.0);
                            if let Some(acc) = self.accounting.as_mut() {
                                acc.day.on_entry_sent(spread_cost);
                            }
                            self.persist_accounting().await;
                            self.log("info", "execution", format!(
                                "ordre {cid} envoyé: {} {} {} lot(s), SL {sl}, TP {tp} (décision {decision_id})",
                                intent.side, symbol, auth.volume
                            ));
                            dv.cid = Some(cid);
                        }
                        Err(why) => {
                            let _ = self.executor.mark_send_failed(&cid, &why).await;
                            self.log("warn", "execution", format!("ordre {cid} non envoyé: {why}"));
                            dv.outcome = "rejected".into();
                        }
                    }
                }
                Err(e) => {
                    self.log("warn", "execution", format!("décision {decision_id}: ordre refusé avant envoi: {e}"));
                    dv.outcome = "rejected".into();
                }
            }
        } else if outcome == "approved" {
            dv.outcome = "rejected".into();
        }

        let summary = match dv.outcome.as_str() {
            "approved" => "AUTORISÉ".to_string(),
            "observed" => format!("observé (MONITOR){}", if dv.failures().len() > 1 { format!(" — bloquerait: {}", dv.failures().into_iter().filter(|f| !f.starts_with("mode:")).collect::<Vec<_>>().join(" ; ")) } else { String::new() }),
            _ => format!("REJETÉ — {}", dv.failures().join(" ; ")),
        };
        self.log("info", "strategy", format!("signal {strategy_id} {} {symbol} {tf}: {} → {summary}", intent.side, intent.reason));
        let why = format!("{strategy_id}: {} → {summary}", intent.reason);
        self.note_entry_decision(symbol, tf, intent.side, &dv.outcome, why, &decision_id);
        self.decisions.push_back(dv);
        while self.decisions.len() > 300 {
            self.decisions.pop_front();
        }
        self.cache.dirty = true;
    }

    /// Close a position (fully or partially). Refused in MONITOR mode.
    pub(crate) async fn close_position(&mut self, p: &Position, volume: Option<f64>, reason: &str) -> Result<String, String> {
        if !self.mode.can_execute() {
            return Err("mode MONITOR: aucun ordre n'est envoyé".into());
        }
        let body = self.executor.submit_close(p, volume, reason, now_ms()).await.map_err(|e| e.to_string())?;
        self.dispatch_command(body, &format!("clôture de la position {}", p.ticket), reason).await
    }

    pub(crate) async fn dispatch_command(&mut self, body: aegis_protocol::Body, what: &str, reason: &str) -> Result<String, String> {
        let cid = match &body {
            aegis_protocol::Body::Order { cid, .. } => cid.clone(),
            _ => String::new(),
        };
        match self.send_order(body) {
            Ok(()) => {
                let _ = self.executor.mark_sent(&cid).await;
                self.audit("execution", "command_sent", json!({ "cid": cid, "what": what, "reason": reason })).await;
                self.log("info", "execution", format!("{what} demandée ({reason})"));
                Ok(format!("{what} demandée"))
            }
            Err(why) => {
                let _ = self.executor.mark_send_failed(&cid, &why).await;
                Err(why)
            }
        }
    }

    // ---------------------------------------------------------- kill switch

    pub(crate) async fn engage_kill(&mut self, reason: &str, actor: &str) {
        let was_active = self.risk.kill_state().active;
        self.risk.engage_kill(reason, actor, now_ms());
        if was_active {
            return;
        }
        let state = self.risk.kill_state().clone();
        if let Err(e) = self.store.kv_set("kill_switch", &state).await {
            self.db_failed(&e.to_string());
        }
        self.audit(actor, "kill_switch_engaged", json!({ "reason": reason })).await;
        self.log("error", "risk", format!("ARRÊT D'URGENCE engagé par {actor}: {reason} — nouvelles entrées bloquées"));
        self.kill_plan_pending = true;
        self.apply_kill_plan().await;
    }

    /// Apply the configured policy to AEGIS's own pending orders and positions.
    /// Positions opened outside AEGIS are never touched.
    pub(crate) async fn apply_kill_plan(&mut self) {
        let cfg = self.risk.config().clone();
        let close = cfg.kill_position_policy == aegis_risk::KillPositionPolicy::Close;
        let magic = self.cfg.execution.magic;
        let orders: Vec<PendingOrder> = if cfg.kill_cancel_pending { self.pending.iter().filter(|o| o.magic == magic).cloned().collect() } else { Vec::new() };
        let positions: Vec<Position> = if close { self.positions.iter().filter(|p| p.magic == magic).cloned().collect() } else { Vec::new() };
        if orders.is_empty() && positions.is_empty() {
            self.kill_plan_pending = false;
            if !close {
                let kept = self.positions.iter().filter(|p| p.magic == magic).count();
                if kept > 0 {
                    self.log("warn", "risk", format!("arrêt d'urgence: {kept} position(s) conservée(s) avec leurs SL/TP (politique « conserver »)"));
                }
            }
            return;
        }
        if !self.mode.can_execute() || self.conn.is_none() {
            self.log("warn", "risk", "arrêt d'urgence: annulations/clôtures en attente (mode MONITOR ou terminal déconnecté)".into());
            return;
        }
        self.kill_plan_pending = false;
        for o in orders {
            match self.executor.submit_cancel(&o, "arrêt d'urgence", now_ms()).await {
                Ok(body) => {
                    let _ = self.dispatch_command(body, &format!("annulation de l'ordre {}", o.ticket), "arrêt d'urgence").await;
                }
                Err(e) => self.log("warn", "risk", format!("annulation {} impossible: {e}", o.ticket)),
            }
        }
        for p in positions {
            if let Err(e) = self.close_position(&p, None, "arrêt d'urgence").await {
                self.log("warn", "risk", format!("clôture {} impossible: {e}", p.ticket));
            }
        }
    }
}
