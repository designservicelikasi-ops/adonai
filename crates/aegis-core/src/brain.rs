//! The micro-profit brain inside the engine: target choice for each signal,
//! lessons recomputed from the trade journal, and scheduled retraining of the
//! probability model whose better versions are PROPOSED to the user (never
//! activated automatically).

use crate::command::{Internal, Reply};
use crate::engine::{evidence_key, Engine};
use crate::intel::model_key;
use crate::view::{BrainCandidate, BrainChoiceView, BrainRetrainResult, BrainView};
use aegis_brain::{candidate_is_better, choose_target, learn, session_of, Lessons, TargetCandidate, TargetRequest};
use aegis_data::{DatasetRow, ProbabilityModelRow};
use aegis_probability::{holdout_score, train, HoldoutScore, ProbabilityModel};
use aegis_strategy::window;
use aegis_types::*;
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::oneshot;

const CANDIDATES_KEY: &str = "brain:candidates";
const RETRAIN_LOG_KEY: &str = "brain:retrain_log";
const LAST_RETRAIN_KEY: &str = "brain:last_retrain_ms";
/// Settings changed in the application (they win over the configuration file).
const CONFIG_KEY: &str = "brain:config";
/// Lessons are recomputed from the journal this often (and after each close).
const LEARN_EVERY_MS: i64 = 60_000;

#[derive(Default)]
pub(crate) struct BrainState {
    pub lessons: HashMap<String, Lessons>,
    pub choices: VecDeque<BrainChoiceView>,
    pub candidates: Vec<BrainCandidate>,
    pub retrain_log: VecDeque<BrainRetrainResult>,
    pub last_learn_ms: TimestampMs,
    pub last_retrain_ms: Option<TimestampMs>,
    pub retraining: bool,
    /// The journal changed (a trade closed): learn at the next timer tick.
    pub stale: bool,
}

/// Result of one background retraining (one instrument and timeframe).
pub(crate) struct RetrainOutcome {
    pub symbol: String,
    pub timeframe: Timeframe,
    pub result: Result<RetrainDone, String>,
}

pub(crate) struct RetrainDone {
    pub model: Box<ProbabilityModel>,
    pub bars_trained: usize,
    pub score: HoldoutScore,
    pub active: Option<(String, HoldoutScore)>,
}

/// What the brain decided for one signal.
pub(crate) struct BrainDecision {
    pub passed: bool,
    pub detail: String,
    pub pick: Option<TargetCandidate>,
    pub haircut: f64,
    /// Stop distance to use: the user's SL in pips, or the strategy's.
    pub sl_distance: f64,
}

impl Engine {
    pub(crate) async fn load_brain(&mut self) {
        match self.store.kv_get::<aegis_brain::BrainConfig>(CONFIG_KEY).await {
            Ok(Some(c)) => match c.validate() {
                Ok(()) => self.cfg.brain = c,
                Err(e) => self.log("warn", "brain", format!("réglages du cerveau enregistrés invalides, fichier utilisé: {e}")),
            },
            Ok(None) => {}
            Err(e) => self.log("warn", "brain", format!("réglages du cerveau illisibles: {e}")),
        }
        if let Ok(Some(c)) = self.store.kv_get::<Vec<BrainCandidate>>(CANDIDATES_KEY).await {
            self.brain.candidates = c;
        }
        if let Ok(Some(l)) = self.store.kv_get::<VecDeque<BrainRetrainResult>>(RETRAIN_LOG_KEY).await {
            self.brain.retrain_log = l;
        }
        self.brain.last_retrain_ms = self.store.kv_get::<TimestampMs>(LAST_RETRAIN_KEY).await.ok().flatten();
        self.brain.stale = true;
    }

    /// Commission + slippage per lot the configuration assumes for `symbol`.
    fn configured_cost_per_lot(&self, symbol: &str) -> f64 {
        let rc = self.risk.config();
        let slip = self.specs.get(symbol).map(|s| s.price_distance_value(rc.expected_slippage_points * s.point, 1.0)).unwrap_or(0.0);
        rc.commission_per_lot_round_turn + slip
    }

    fn terminal_kind(&self) -> &'static str {
        if self.terminal() == Some(TerminalKind::Simulator) { "simulator" } else { "mt5" }
    }

    /// Recompute the lessons of every brain instrument from the closed trades
    /// of the same kind of terminal (simulator results never teach the broker).
    pub(crate) async fn brain_learn(&mut self, now: TimestampMs) {
        self.brain.last_learn_ms = now;
        self.brain.stale = false;
        let trades = match self.store.journal_closed_since(now - 365 * 86_400_000, 20_000).await {
            Ok(t) => t,
            Err(e) => {
                self.db_failed(&e.to_string());
                return;
            }
        };
        let kind = self.terminal_kind();
        let mut symbols: Vec<String> = self.specs.keys().filter(|s| self.cfg.brain.covers(s)).cloned().collect();
        for t in &trades {
            if self.cfg.brain.covers(&t.symbol) && !symbols.contains(&t.symbol) {
                symbols.push(t.symbol.clone());
            }
        }
        for s in symbols {
            let mine: Vec<aegis_data::JournalEntry> = trades.iter().filter(|t| t.symbol == s && t.terminal == kind).cloned().collect();
            let lessons = learn(&self.cfg.brain, &s, &mine, self.configured_cost_per_lot(&s), now);
            if let Some(old) = self.brain.lessons.get(&s).cloned() {
                // Tell the user when the brain becomes noticeably more careful.
                if lessons.haircut() < 0.8 && old.haircut() >= 0.8 {
                    self.alert("warning", "brain", format!(
                        "{s}: les probabilités ont été trop optimistes ({} gains obtenus pour {:.1} promis): le cerveau les réduit de {:.0} %. Un réentraînement est conseillé.",
                        lessons.calibration.actual_wins, lessons.calibration.promised_wins, (1.0 - lessons.haircut()) * 100.0
                    ));
                }
                for c in lessons.contexts.iter().filter(|c| c.blocked) {
                    if !old.contexts.iter().any(|o| o.blocked && o.strategy_id == c.strategy_id && o.side == c.side && o.session == c.session) {
                        self.log("warn", "brain", format!("{s}: {} {} en session {}: {}", c.strategy_id, c.side, c.session, c.verdict));
                    }
                }
            }
            self.brain.lessons.insert(s, lessons);
        }
        self.cache.dirty = true;
    }

    /// New brain settings from the application: validated, stored, applied at once.
    pub(crate) async fn set_brain_config(&mut self, c: aegis_brain::BrainConfig) -> Reply {
        c.validate()?;
        self.store.kv_set(CONFIG_KEY, &c).await.map_err(|e| e.to_string())?;
        self.audit("user", "brain_config", serde_json::to_value(&c).unwrap_or_default()).await;
        let was = self.cfg.brain.enabled;
        self.cfg.brain = c;
        self.brain.stale = true;
        self.cache.dirty = true;
        let b = &self.cfg.brain;
        let sl = if b.sl_pips > 0.0 { format!("SL {} pips", b.sl_pips) } else { "SL de la stratégie".into() };
        let tp = if b.tp_pips > 0.0 { format!("TP fixe {} pips", b.tp_pips) } else if b.max_tp_pips > 0.0 { format!("TP choisi, au plus {} pips", b.max_tp_pips) } else { "TP choisi".into() };
        let state = match (was, b.enabled) {
            (false, true) => "cerveau ACTIVÉ",
            (true, false) => "cerveau désactivé",
            _ => "réglages du cerveau enregistrés",
        };
        Ok(format!("{state}: gain net {:.2} à {:.2}, {sl}, {tp}", b.min_net_win, b.max_net_win))
    }

    /// Choose the target of a signal. Called only when the brain applies to `symbol`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn brain_decide(&self, strategy_id: &str, symbol: &str, tf: Timeframe, side: Side, strategy_sl_distance: f64, strategy_tp_distance: f64, now: TimestampMs) -> (BrainDecision, BrainChoiceView) {
        let session = session_of(now);
        let mut view = BrainChoiceView {
            ts_ms: now,
            decision_id: String::new(),
            symbol: symbol.into(),
            timeframe: tf,
            strategy_id: strategy_id.into(),
            side,
            session: session.into(),
            traded: false,
            reason: String::new(),
            strategy_tp_distance,
            sl_distance: strategy_sl_distance,
            tried: 0,
            in_range: 0,
            chosen: None,
            best: None,
        };
        let lessons = self.brain.lessons.get(symbol).cloned().unwrap_or_else(|| Lessons::empty(symbol, self.configured_cost_per_lot(symbol)));
        let haircut = lessons.haircut();
        let fail = |mut view: BrainChoiceView, why: String| {
            view.reason = why.clone();
            (BrainDecision { passed: false, detail: why, pick: None, haircut, sl_distance: strategy_sl_distance }, view)
        };
        if let Some(c) = lessons.context(strategy_id, side, session).filter(|c| c.blocked) {
            return fail(view, format!("contexte appris {strategy_id} {side} en session {session}: {}", c.verdict));
        }
        let (Some(spec), Some((tick, _)), Some(account)) = (self.specs.get(symbol), self.market.last_tick(symbol), self.account.as_ref()) else {
            return fail(view, "cotation, instrument ou compte indisponible".into());
        };
        let rc = self.risk.config();
        let b = &self.cfg.brain;
        let pip = aegis_brain::pip_size(spec);
        let sl_distance = if b.sl_pips > 0.0 { b.sl_pips * pip } else { strategy_sl_distance };
        let sign = side.sign();
        let entry = if side == Side::Buy { tick.ask } else { tick.bid };
        let sl = spec.round_price(entry - sign * sl_distance);
        let bars = self.market.candles(symbol, tf);
        let atr = aegis_market::indicators::atr(window(&bars), 14);
        // Backtest evidence was measured with the strategy's own target and stop: for a
        // CLOSER target and a stop at least as WIDE it remains a prudent lower bound;
        // otherwise it says nothing.
        let terminal = self.terminal();
        let evidence = self.evidence.get(&evidence_key(strategy_id, symbol, tf.as_str())).and_then(|ev| {
            let usable = ev.origin == DataOrigin::Broker || terminal == Some(TerminalKind::Simulator);
            ev.win_probability.filter(|_| usable).map(|p| (p, format!("backtest hors échantillon {}: {}/{} gagnants (borne basse de Wilson)", ev.run_id, ev.wins, ev.trades)))
        });
        let probability = |d: f64| {
            let (sp, ss) = self.scenario_probability(symbol, tf, side, d, sl_distance);
            let ev = evidence.clone().filter(|_| d <= strategy_tp_distance * 1.000_1 + spec.point && sl_distance >= strategy_sl_distance * 0.999_9 - spec.point);
            match (ev, sp) {
                (Some((a, sa)), Some(b)) => if b < a { (Some(b), ss) } else { (Some(a), sa) },
                (None, Some(b)) => (Some(b), ss),
                (Some((a, sa)), None) => (Some(a), format!("{sa}, cible plus proche que celle de la stratégie")),
                (None, None) => (None, format!("{ss}; la preuve de backtest ne vaut que pour une cible au plus égale et un stop au moins égal à ceux de la stratégie")),
            }
        };
        let req = TargetRequest {
            spec,
            risk: rc,
            side,
            entry,
            sl,
            spread_price: tick.spread(),
            risk_volume: aegis_risk::size_volume(spec, rc, account.equity, entry, sl),
            atr,
            strategy_tp_distance,
            extra_cost_per_lot: lessons.extra_cost_per_lot(),
            haircut,
            fixed_tp_distance: (b.tp_pips > 0.0).then_some(b.tp_pips * pip),
            max_tp_distance: (b.max_tp_pips > 0.0).then_some(b.max_tp_pips * pip),
        };
        let choice = choose_target(&self.cfg.brain, &req, probability);
        view.tried = choice.tried;
        view.in_range = choice.in_range;
        view.best = choice.best.clone();
        view.chosen = choice.chosen.clone();
        view.traded = choice.chosen.is_some();
        view.reason = choice.reason.clone();
        let mut detail = choice.reason.clone();
        if haircut < 1.0 - 1e-9 || lessons.extra_cost_per_lot() > 0.0 {
            detail.push_str(&format!(
                " [appris sur {} transactions: probabilités x {:.3}, frais réels +{:.2} par lot]",
                lessons.trades,
                haircut,
                lessons.extra_cost_per_lot()
            ));
        }
        view.sl_distance = sl_distance;
        (BrainDecision { passed: choice.chosen.is_some(), detail, pick: choice.chosen, haircut, sl_distance }, view)
    }

    pub(crate) fn brain_record(&mut self, mut view: BrainChoiceView, decision_id: &str, executed: bool) {
        view.decision_id = decision_id.into();
        view.traded = view.traded && executed;
        self.brain.choices.push_front(view);
        self.brain.choices.truncate(100);
    }

    // ------------------------------------------------------------ retraining

    /// Periodic work: lessons every minute (or after a close), retraining on schedule.
    pub(crate) async fn brain_tick(&mut self, now: TimestampMs) {
        if !self.cfg.brain.enabled {
            return;
        }
        if self.brain.stale || now - self.brain.last_learn_ms >= LEARN_EVERY_MS {
            self.brain_learn(now).await;
        }
        let every = self.cfg.brain.retrain_every_hours as i64 * 3_600_000;
        // Never on unknown data: the origin of the candles depends on the connected terminal.
        if every > 0 && self.terminal().is_some() && !self.brain.retraining {
            let due = match self.brain.last_retrain_ms {
                Some(t) => now - t >= every,
                None => {
                    // First run: one period after the start, not at every restart.
                    self.brain.last_retrain_ms = Some(now);
                    let _ = self.store.kv_set(LAST_RETRAIN_KEY, &now).await;
                    false
                }
            };
            if due {
                self.start_brain_retrain(None).await;
            }
        }
    }

    /// Train a new version of every brain instrument/timeframe in the background.
    pub(crate) async fn start_brain_retrain(&mut self, reply: Option<oneshot::Sender<Reply>>) {
        if self.brain.retraining {
            if let Some(r) = reply {
                let _ = r.send(Err("un réentraînement est déjà en cours".into()));
            }
            return;
        }
        if self.terminal().is_none() {
            if let Some(r) = reply {
                let _ = r.send(Err("aucun terminal connecté: l'origine des données (broker ou simulateur) est inconnue".into()));
            }
            return;
        }
        let now = now_ms();
        self.brain.last_retrain_ms = Some(now);
        let _ = self.store.kv_set(LAST_RETRAIN_KEY, &now).await;
        let origin = self.origin();
        let mut jobs = Vec::new();
        let mut symbols: Vec<SymbolSpec> = self.specs.values().filter(|s| self.cfg.brain.covers(&s.symbol)).cloned().collect();
        symbols.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        for spec in symbols {
            for tf in self.cfg.engine.signal_timeframes.clone() {
                match self.store.load_candles(&spec.symbol, tf, origin, 200_000).await {
                    Ok(bars) => {
                        let active = self.prob_models.get(&model_key(&spec.symbol, tf)).filter(|m| self.probability_model_usable(m)).cloned();
                        jobs.push((spec.clone(), tf, bars, active));
                    }
                    Err(e) => self.log("warn", "brain", format!("bougies {} {tf} illisibles: {e}", spec.symbol)),
                }
            }
        }
        if jobs.is_empty() {
            if let Some(r) = reply {
                let _ = r.send(Err("aucun instrument du cerveau n'est disponible sur le terminal connecté".into()));
            }
            return;
        }
        self.brain.retraining = true;
        self.cache.dirty = true;
        self.log("info", "brain", format!("réentraînement de {} modèle(s) sur les dernières bougies ({})", jobs.len(), origin.as_str()));
        let (cfg, holdout_pct, tx) = (self.cfg.probability.clone(), self.cfg.brain.holdout_pct, self.internal_tx.clone());
        tokio::task::spawn_blocking(move || {
            let results = jobs.into_iter().map(|(spec, tf, bars, active)| retrain_one(&spec, tf, origin, &bars, active, &cfg, holdout_pct)).collect();
            let _ = tx.blocking_send(Internal::BrainRetrained { reply, results });
        });
    }

    pub(crate) async fn on_brain_retrained(&mut self, results: Vec<RetrainOutcome>) -> Reply {
        self.brain.retraining = false;
        let mut lines = Vec::new();
        for r in results {
            let (proposed, outcome) = match r.result {
                Err(e) => (false, e),
                Ok(done) => {
                    let (better, why) = candidate_is_better(&self.cfg.brain, &done.score, done.active.as_ref().map(|a| &a.1));
                    if better {
                        match self.save_candidate(&done).await {
                            Ok(()) => {
                                let c = BrainCandidate {
                                    model_id: done.model.model_id.clone(),
                                    symbol: r.symbol.clone(),
                                    timeframe: r.timeframe,
                                    created_ms: done.model.created_ms,
                                    bars_trained: done.bars_trained,
                                    score: done.score.clone(),
                                    active_model_id: done.active.as_ref().map(|a| a.0.clone()),
                                    active_score: done.active.as_ref().map(|a| a.1.clone()),
                                    verdict: why.clone(),
                                };
                                self.brain.candidates.retain(|x| !(x.symbol == c.symbol && x.timeframe == c.timeframe));
                                self.brain.candidates.push(c);
                                self.alert("warning", "brain", format!(
                                    "{} {}: nouvelle version {} prête — {why}. Elle ne sera utilisée que si vous l'activez (page Cerveau).",
                                    r.symbol, r.timeframe, done.model.model_id
                                ));
                                (true, format!("version {} proposée: {why}", done.model.model_id))
                            }
                            Err(e) => (false, format!("version meilleure mais non enregistrée: {e}")),
                        }
                    } else {
                        (false, format!("nouvelle version écartée: {why}"))
                    }
                }
            };
            self.log("info", "brain", format!("{} {}: {outcome}", r.symbol, r.timeframe));
            lines.push(format!("{} {}: {outcome}", r.symbol, r.timeframe));
            self.brain.retrain_log.push_front(BrainRetrainResult { ts_ms: now_ms(), symbol: r.symbol, timeframe: r.timeframe, proposed, outcome });
        }
        self.brain.retrain_log.truncate(30);
        let _ = self.store.kv_set(CANDIDATES_KEY, &self.brain.candidates).await;
        let _ = self.store.kv_set(RETRAIN_LOG_KEY, &self.brain.retrain_log).await;
        self.cache.dirty = true;
        Ok(lines.join(" | "))
    }

    /// Store a candidate as an INACTIVE version (it can be activated or rolled back later).
    async fn save_candidate(&mut self, done: &RetrainDone) -> Result<(), String> {
        let m = &done.model;
        let row = ProbabilityModelRow {
            model_id: m.model_id.clone(),
            created_ms: m.created_ms,
            symbol: m.symbol.clone(),
            timeframe: m.timeframe.clone(),
            origin: m.origin.as_str().into(),
            dataset_hash: m.dataset_hash.clone(),
            buy_selected: format!("{:?}", m.buy.selected),
            sell_selected: format!("{:?}", m.sell.selected),
            active: false,
            model: serde_json::to_string(&**m).map_err(|e| e.to_string())?,
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
        self.store.save_probability_model(&row).await.map_err(|e| e.to_string())?;
        self.audit("brain", "probability_model_proposed", json!({ "model_id": m.model_id, "symbol": m.symbol, "timeframe": m.timeframe, "dataset": m.dataset_hash })).await;
        Ok(())
    }

    /// The user accepts a proposed version.
    pub(crate) async fn brain_activate(&mut self, model_id: &str) -> Reply {
        if !self.brain.candidates.iter().any(|c| c.model_id == model_id) {
            return Err(format!("aucune version proposée {model_id}"));
        }
        let msg = self.activate_probability_model(model_id).await?;
        self.brain.candidates.retain(|c| c.model_id != model_id);
        let _ = self.store.kv_set(CANDIDATES_KEY, &self.brain.candidates).await;
        self.audit("user", "brain_candidate_accepted", json!({ "model_id": model_id })).await;
        Ok(format!("{msg} (version proposée par le cerveau, acceptée)"))
    }

    pub(crate) async fn brain_dismiss(&mut self, model_id: &str) -> Reply {
        let before = self.brain.candidates.len();
        self.brain.candidates.retain(|c| c.model_id != model_id);
        if self.brain.candidates.len() == before {
            return Err(format!("aucune version proposée {model_id}"));
        }
        let _ = self.store.kv_set(CANDIDATES_KEY, &self.brain.candidates).await;
        self.audit("user", "brain_candidate_dismissed", json!({ "model_id": model_id })).await;
        self.cache.dirty = true;
        Ok(format!("version {model_id} écartée (elle reste enregistrée et peut être activée plus tard)"))
    }

    pub(crate) fn brain_view(&self, now: TimestampMs) -> BrainView {
        let b = &self.cfg.brain;
        let mut warnings = Vec::new();
        let rc = self.risk.config();
        if b.enabled && rc.min_net_expected_profit >= b.max_net_win {
            warnings.push(format!(
                "Le réglage de risque « espérance nette minimale » ({:.2}) est supérieur au gain maximal du cerveau ({:.2}): aucune transaction ne peut passer. Baissez-le (par exemple 0,01) dans Risque & configuration.",
                rc.min_net_expected_profit, b.max_net_win
            ));
        } else if b.enabled && rc.min_net_expected_profit > 0.1 {
            warnings.push(format!(
                "Espérance nette minimale de {:.2} par transaction: avec des cibles de {:.2} à {:.2}, la plupart des micro-gains seront refusés.",
                rc.min_net_expected_profit, b.min_net_win, b.max_net_win
            ));
        }
        let mut symbols: Vec<String> = self.specs.keys().filter(|s| b.covers(s)).cloned().collect();
        symbols.sort();
        if b.enabled && self.conn.is_some() && symbols.is_empty() {
            warnings.push(format!("Aucun instrument du terminal ne correspond à {:?}.", b.symbols));
        }
        for s in &symbols {
            for tf in &self.cfg.engine.signal_timeframes {
                if !self.prob_models.contains_key(&model_key(s, *tf)) {
                    warnings.push(format!("{s} {tf}: aucun modèle de probabilité — seules les cibles plus proches que celle de la stratégie (preuve de backtest) peuvent être évaluées."));
                }
            }
        }
        let mut lessons: Vec<Lessons> = self.brain.lessons.values().cloned().collect();
        lessons.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        let every = b.retrain_every_hours as i64 * 3_600_000;
        BrainView {
            enabled: b.enabled,
            config: b.clone(),
            symbols,
            lessons,
            choices: self.brain.choices.iter().take(50).cloned().collect(),
            candidates: self.brain.candidates.clone(),
            retraining: self.brain.retraining,
            last_retrain_ms: self.brain.last_retrain_ms,
            next_retrain_ms: (b.enabled && every > 0).then(|| self.brain.last_retrain_ms.unwrap_or(now) + every),
            retrain_log: self.brain.retrain_log.iter().cloned().collect(),
            warnings,
        }
    }
}

/// Train on everything except the most recent `holdout_pct` of the bars, then
/// score the new version and the active one on those unseen bars.
fn retrain_one(
    spec: &SymbolSpec,
    tf: Timeframe,
    origin: DataOrigin,
    bars: &[Candle],
    active: Option<Arc<ProbabilityModel>>,
    cfg: &aegis_probability::ProbabilityConfig,
    holdout_pct: f64,
) -> RetrainOutcome {
    let n = bars.len();
    let cut = n - ((n as f64 * holdout_pct / 100.0).round() as usize).min(n);
    // The control period must contain labelled scenarios: more than one horizon of bars.
    let result = if n - cut <= cfg.horizon_bars + 50 {
        Err(format!("historique insuffisant pour une période de contrôle ({n} bougies)"))
    } else {
        let id = new_id("prob");
        match train(&id, &spec.symbol, tf.as_str(), origin, &bars[..cut], spec.point, cfg, now_ms()) {
            Err(e) => Err(format!("entraînement impossible: {e}")),
            Ok(model) => {
                let score = holdout_score(&model, cfg, bars, cut, spec.point);
                let active = active.map(|a| (a.model_id.clone(), holdout_score(&a, cfg, bars, cut, spec.point)));
                Ok(RetrainDone { model: Box::new(model), bars_trained: cut, score, active })
            }
        }
    };
    RetrainOutcome { symbol: spec.symbol.clone(), timeframe: tf, result }
}
