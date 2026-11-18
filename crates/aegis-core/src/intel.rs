//! Probability Engine and Entry Intelligence inside the engine, plus the trade
//! journal that ties each decision to its real outcome.

use crate::command::{Internal, Reply};
use crate::engine::Engine;
use crate::view::{EntryState, ProbabilityModelView};
use aegis_data::{DatasetRow, JournalEntry, ProbabilityModelRow};
use aegis_probability::{train, ProbabilityModel, Selected};
use aegis_strategy::window;
use aegis_types::*;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::oneshot;

pub(crate) fn model_key(symbol: &str, tf: Timeframe) -> (String, Timeframe) {
    (symbol.to_string(), tf)
}

impl Engine {
    /// Load the active probability model of each instrument/timeframe.
    pub(crate) async fn load_probability_models(&mut self) {
        match self.store.probability_models(Some(true)).await {
            Ok(rows) => {
                for r in rows {
                    let Some(tf) = Timeframe::parse(&r.timeframe) else { continue };
                    match serde_json::from_str::<ProbabilityModel>(&r.model) {
                        Ok(m) => {
                            self.prob_models.insert(model_key(&r.symbol, tf), Arc::new(m));
                        }
                        Err(e) => self.log("warn", "probability", format!("modèle {} illisible: {e}", r.model_id)),
                    }
                }
            }
            Err(e) => self.db_failed(&e.to_string()),
        }
    }

    /// A model trained on synthetic candles is only usable against the simulator.
    pub(crate) fn probability_model_usable(&self, m: &ProbabilityModel) -> bool {
        m.origin == DataOrigin::Broker || self.terminal() == Some(TerminalKind::Simulator)
    }

    pub(crate) async fn start_probability_training(&mut self, symbol: String, tf: Timeframe, origin: DataOrigin, reply: oneshot::Sender<Reply>) {
        let Some(spec) = self.specs.get(&symbol).cloned() else {
            let _ = reply.send(Err(format!("spécification de {symbol} inconnue")));
            return;
        };
        let bars = match self.store.load_candles(&symbol, tf, origin, 200_000).await {
            Ok(b) => b,
            Err(e) => {
                let _ = reply.send(Err(format!("chargement des bougies impossible: {e}")));
                return;
            }
        };
        let cfg = self.cfg.probability.clone();
        let tx = self.internal_tx.clone();
        self.log("info", "probability", format!("entraînement du modèle de probabilité {symbol} {tf} sur {} bougies ({})", bars.len(), origin.as_str()));
        tokio::task::spawn_blocking(move || {
            let id = new_id("prob");
            let outcome = train(&id, &symbol, tf.as_str(), origin, &bars, spec.point, &cfg, now_ms()).map(Box::new).map_err(|e| e.to_string());
            let _ = tx.blocking_send(Internal::ProbabilityDone { reply, outcome });
        });
    }

    pub(crate) async fn on_probability_trained(&mut self, outcome: Result<Box<ProbabilityModel>, String>) -> Reply {
        let m = *outcome?;
        let Some(tf) = Timeframe::parse(&m.timeframe) else { return Err("unité de temps inconnue".into()) };
        let row = ProbabilityModelRow {
            model_id: m.model_id.clone(),
            created_ms: m.created_ms,
            symbol: m.symbol.clone(),
            timeframe: m.timeframe.clone(),
            origin: m.origin.as_str().into(),
            dataset_hash: m.dataset_hash.clone(),
            buy_selected: format!("{:?}", m.buy.selected),
            sell_selected: format!("{:?}", m.sell.selected),
            active: true,
            model: serde_json::to_string(&m).map_err(|e| e.to_string())?,
        };
        let ds = DatasetRow {
            dataset_hash: m.dataset_hash.clone(),
            created_ms: m.created_ms,
            symbol: m.symbol.clone(),
            timeframe: m.timeframe.clone(),
            origin: m.origin.as_str().into(),
            bars: m.bars as i64,
            from_ts: m.from_ts,
            to_ts: m.to_ts,
        };
        self.store.register_dataset(&ds).await.map_err(|e| e.to_string())?;
        self.store.save_probability_model(&row).await.map_err(|e| format!("modèle calculé mais non enregistré: {e}"))?;
        let summary = format!(
            "modèle {} {} {}: BUY {:?} (Brier {:.4}, ECE {:.3}), SELL {:?} (Brier {:.4}, ECE {:.3}) — {}",
            m.model_id,
            m.symbol,
            m.timeframe,
            m.buy.selected,
            m.buy.full_report.brier,
            m.buy.full_report.ece,
            m.sell.selected,
            m.sell.full_report.brier,
            m.sell.full_report.ece,
            if m.origin == DataOrigin::Synthetic { "données SYNTHÉTIQUES" } else { "données broker" }
        );
        self.audit("user", "probability_model_trained", json!({ "model_id": m.model_id, "symbol": m.symbol, "timeframe": m.timeframe, "origin": m.origin.as_str(), "dataset": m.dataset_hash, "buy": m.buy.selected, "sell": m.sell.selected })).await;
        self.log("info", "probability", summary.clone());
        self.prob_models.insert(model_key(&m.symbol, tf), Arc::new(m));
        self.cache.dirty = true;
        Ok(summary)
    }

    /// Roll back (or forward) to a stored model version.
    pub(crate) async fn activate_probability_model(&mut self, model_id: &str) -> Reply {
        let row = self.store.activate_probability_model(model_id).await.map_err(|e| e.to_string())?.ok_or_else(|| format!("modèle {model_id} introuvable"))?;
        let m: ProbabilityModel = serde_json::from_str(&row.model).map_err(|e| e.to_string())?;
        let tf = Timeframe::parse(&row.timeframe).ok_or("unité de temps inconnue")?;
        self.prob_models.insert(model_key(&row.symbol, tf), Arc::new(m));
        self.audit("user", "probability_model_activated", json!({ "model_id": model_id })).await;
        self.cache.dirty = true;
        Ok(format!("modèle {model_id} actif pour {} {}", row.symbol, row.timeframe))
    }

    /// P(TP before SL) for a signal: the most prudent of the validated sources.
    pub(crate) fn scenario_probability(&self, symbol: &str, tf: Timeframe, side: Side, tp_distance: f64, sl_distance: f64) -> (Option<f64>, String) {
        let Some(m) = self.prob_models.get(&model_key(symbol, tf)) else { return (None, "aucun modèle de probabilité pour cet instrument/unité".into()) };
        if !self.probability_model_usable(m) {
            return (None, format!("modèle {} entraîné sur données synthétiques: inutilisable avec un vrai broker", m.model_id));
        }
        let bars = self.market.candles(symbol, tf);
        let est = m.estimate(window(&bars), side, tp_distance, sl_distance);
        (est.probability, est.source)
    }

    /// Recompute the dashboard's BUY/SELL probabilities after a bar closes.
    pub(crate) fn refresh_entry_state(&mut self, symbol: &str, tf: Timeframe) {
        let cfg = &self.cfg.probability;
        let bars = self.market.candles(symbol, tf);
        let a = aegis_market::indicators::atr(window(&bars), 14);
        let (mut buy, mut sell, mut source) = (None, None, "aucun modèle de probabilité".to_string());
        if let (Some(a), Some(m)) = (a, self.prob_models.get(&model_key(symbol, tf))) {
            if self.probability_model_usable(m) {
                let b = m.estimate(window(&bars), Side::Buy, cfg.display_tp_atr * a, cfg.display_sl_atr * a);
                let s = m.estimate(window(&bars), Side::Sell, cfg.display_tp_atr * a, cfg.display_sl_atr * a);
                buy = b.probability;
                sell = s.probability;
                source = format!("TP {} ATR / SL {} ATR — {}", cfg.display_tp_atr, cfg.display_sl_atr, if buy.is_some() { b.source } else { s.source });
            } else {
                source = "modèle sur données synthétiques: inutilisable avec un vrai broker".into();
            }
        }
        let e = self.entry_states.entry(model_key(symbol, tf)).or_insert_with(|| EntryState::wait(symbol, tf));
        e.ts_ms = now_ms();
        e.buy_probability = buy;
        e.sell_probability = sell;
        e.probability_source = source;
        // The decision is WAIT until a signal of this bar is approved (see decide.rs).
        e.decision = "WAIT".into();
        e.reason = "aucun signal validé sur la dernière bougie".into();
        e.decision_id = None;
    }

    pub(crate) fn note_entry_decision(&mut self, symbol: &str, tf: Timeframe, side: Side, outcome: &str, why: String, decision_id: &str) {
        let e = self.entry_states.entry(model_key(symbol, tf)).or_insert_with(|| EntryState::wait(symbol, tf));
        // An approved order wins over a rejected signal of the same bar.
        if e.decision != "WAIT" && outcome != "approved" {
            return;
        }
        e.ts_ms = now_ms();
        e.decision_id = Some(decision_id.to_string());
        match outcome {
            "approved" => {
                e.decision = side.as_str().to_uppercase();
                e.reason = why;
            }
            _ => {
                e.decision = "WAIT".into();
                e.reason = why;
            }
        }
    }

    pub(crate) fn probability_views(&self) -> Vec<ProbabilityModelView> {
        let mut v: Vec<ProbabilityModelView> = self
            .prob_models
            .values()
            .map(|m| ProbabilityModelView {
                model_id: m.model_id.clone(),
                created_ms: m.created_ms,
                symbol: m.symbol.clone(),
                timeframe: m.timeframe.clone(),
                origin: m.origin,
                dataset_hash: m.dataset_hash.clone(),
                bars: m.bars,
                usable: self.probability_model_usable(m),
                buy_selected: m.buy.selected,
                sell_selected: m.sell.selected,
                buy_reason: m.buy.selection_reason.clone(),
                sell_reason: m.sell.selection_reason.clone(),
                buy_report: if m.buy.selected == Selected::ScenarioOnly { m.buy.baseline_report.clone() } else { m.buy.full_report.clone() },
                sell_report: if m.sell.selected == Selected::ScenarioOnly { m.sell.baseline_report.clone() } else { m.sell.full_report.clone() },
            })
            .collect();
        v.sort_by(|a, b| (&a.symbol, &a.timeframe).cmp(&(&b.symbol, &b.timeframe)));
        v
    }

    // ------------------------------------------------------------- journal

    pub(crate) async fn journal_on_fill(&mut self, cid: &str, ticket: i64, fill_price: f64, volume: f64, slippage_cost: f64) {
        let Some(mut e) = self.journal_seeds.remove(cid) else { return };
        e.ticket = ticket;
        e.entry_filled = fill_price;
        e.volume = volume;
        e.slippage_cost = slippage_cost;
        e.opened_ms = now_ms();
        if let Err(err) = self.store.journal_open(&e).await {
            self.db_failed(&err.to_string());
        }
        self.excursions.insert(ticket, (e.side.unwrap_or(Side::Buy), e.entry_filled, 0.0, 0.0));
    }

    pub(crate) async fn journal_on_deal(&mut self, d: &Deal) {
        if let Some((_, _, mfe, mae)) = self.excursions.remove(&d.position_ticket) {
            let _ = self.store.journal_excursion(d.position_ticket, mfe, mae).await;
        }
        let reason = if d.comment.is_empty() { "close".to_string() } else { d.comment.clone() };
        if let Err(e) = self.store.journal_close(d.position_ticket, d.ts_ms, d.price, d.profit, d.commission, d.swap, d.fee, &reason).await {
            self.db_failed(&e.to_string());
        }
    }

    /// Maximum favourable/adverse excursion of AEGIS positions, from the quotes.
    pub(crate) fn track_excursions(&mut self, symbol: &str, tick: &Tick) {
        let tickets: Vec<i64> = self.positions.iter().filter(|p| p.symbol == symbol).map(|p| p.ticket).collect();
        for t in tickets {
            if let Some((side, entry, mfe, mae)) = self.excursions.get_mut(&t) {
                let exit = if *side == Side::Buy { tick.bid } else { tick.ask };
                let moved = (exit - *entry) * side.sign();
                *mfe = mfe.max(moved);
                *mae = mae.max(-moved);
            }
        }
    }
}

pub(crate) fn seed_journal(
    decision_id: &str,
    cid: &str,
    strategy_id: &str,
    symbol: &str,
    tf: Timeframe,
    side: Side,
    regime: &str,
    terminal: &str,
    requested: f64,
    sl: f64,
    tp: f64,
    micro: Option<&aegis_risk::MicroProfitEvaluation>,
) -> JournalEntry {
    JournalEntry {
        decision_id: decision_id.into(),
        cid: cid.into(),
        strategy_id: strategy_id.into(),
        symbol: symbol.into(),
        timeframe: tf.as_str().into(),
        side: Some(side),
        regime: regime.into(),
        terminal: terminal.into(),
        entry_requested: requested,
        sl_initial: sl,
        tp_initial: tp,
        probability: micro.and_then(|m| m.win_probability),
        probability_source: micro.map(|m| m.probability_source.clone()).unwrap_or_default(),
        expected_net: micro.and_then(|m| m.expected_net),
        est_spread_cost: micro.map(|m| m.costs.spread).unwrap_or(0.0),
        est_commission: micro.map(|m| m.costs.commission).unwrap_or(0.0),
        ..Default::default()
    }
}
