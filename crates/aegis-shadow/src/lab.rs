//! The shadow laboratory: paper trading of champions and challengers,
//! hypothetical outcomes of refused signals, trade analysis.

use crate::*;
use aegis_backtest::{run, run_segmented, BacktestParams};
use aegis_strategy::{build, window, OpenView, Strategy, StrategyAction, STRATEGY_IDS};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperTrade {
    pub symbol: String,
    pub timeframe: Timeframe,
    pub side: Side,
    pub entry: f64,
    pub exit: f64,
    pub entry_ts: TimestampMs,
    pub exit_ts: TimestampMs,
    /// Per 1.0 lot.
    pub gross: f64,
    pub net: f64,
    /// Result in multiples of the initial risk.
    pub r: f64,
}

#[derive(Debug, Clone)]
struct PaperPos {
    side: Side,
    entry: f64,
    sl: f64,
    tp: f64,
    entry_ts: TimestampMs,
    bars: usize,
}

pub struct Candidate {
    pub id: String,
    pub strategy_id: String,
    pub role: Role,
    pub config: Value,
    pub live_enabled: bool,
    strategy: Box<dyn Strategy>,
    open: HashMap<Key, PaperPos>,
    pub recent: Vec<PaperTrade>,
    pub forward: ForwardStats,
    cum: f64,
    peak: f64,
    win_sum: f64,
    loss_sum: f64,
    pub oos: Vec<OosSummary>,
}

impl Candidate {
    fn new(id: String, strategy_id: &str, role: Role, config: Value, live_enabled: bool) -> Option<Candidate> {
        let strategy = build(strategy_id, &config).ok()?;
        let config = strategy.config();
        Some(Candidate {
            id,
            strategy_id: strategy_id.to_string(),
            role,
            config,
            live_enabled,
            strategy,
            open: HashMap::new(),
            recent: Vec::new(),
            forward: ForwardStats::default(),
            cum: 0.0,
            peak: 0.0,
            win_sum: 0.0,
            loss_sum: 0.0,
            oos: Vec::new(),
        })
    }

    fn record(&mut self, t: PaperTrade) {
        let f = &mut self.forward;
        f.trades += 1;
        if t.net > 0.0 {
            f.wins += 1;
            self.win_sum += t.net;
        } else {
            self.loss_sum += t.net;
        }
        f.net += t.net;
        f.gross += t.gross;
        f.sum_r += t.r;
        self.cum += t.net;
        self.peak = self.peak.max(self.cum);
        f.max_drawdown = f.max_drawdown.max(self.peak - self.cum);
        f.expectancy = Some(f.net / f.trades as f64);
        f.profit_factor = (self.loss_sum < 0.0).then(|| self.win_sum / -self.loss_sum);
        self.recent.push(t);
        if self.recent.len() > 200 {
            self.recent.remove(0);
        }
    }

    /// Paper-trade one closed bar. Mirrors the live engine: entries at the close
    /// of the signal bar (BUY at the ask), exits on SL/TP inside later bars
    /// (both touched in one bar = the stop, pessimistic) or on the strategy's exit.
    fn on_bar(&mut self, key: &Key, bars: &[Candle], spec: &SymbolSpec, commission: f64) {
        let Some(bar) = bars.last().copied() else { return };
        let spread = bar.spread_points.max(0.0) * spec.point;
        let money = |side: Side, entry: f64, exit: f64| {
            let d = (exit - entry) * side.sign();
            spec.price_distance_value(d, 1.0) * d.signum()
        };
        let mut closed_now = false;
        if let Some(mut p) = self.open.remove(key) {
            p.bars += 1;
            let (lo, hi) = match p.side {
                Side::Buy => (bar.low, bar.high),
                Side::Sell => (bar.low + spread, bar.high + spread), // ask prices
            };
            let (stop_hit, target_hit) = match p.side {
                Side::Buy => (lo <= p.sl, hi >= p.tp),
                Side::Sell => (hi >= p.sl, lo <= p.tp),
            };
            let exit = if stop_hit {
                Some(p.sl)
            } else if target_hit {
                Some(p.tp)
            } else {
                let view = OpenView { side: p.side, entry_price: p.entry, bars_held: p.bars };
                match self.strategy.evaluate(window(bars), Some(&view)) {
                    StrategyAction::Exit { .. } => Some(if p.side == Side::Buy { bar.close } else { bar.close + spread }),
                    _ => None,
                }
            };
            match exit {
                Some(x) => {
                    let gross = money(p.side, p.entry, x);
                    let risk = (p.entry - p.sl).abs();
                    self.record(PaperTrade {
                        symbol: key.0.clone(),
                        timeframe: key.1,
                        side: p.side,
                        entry: p.entry,
                        exit: x,
                        entry_ts: p.entry_ts,
                        exit_ts: bar.open_ts,
                        gross,
                        net: gross - commission,
                        r: if risk > 0.0 { (x - p.entry) * p.side.sign() / risk } else { 0.0 },
                    });
                    closed_now = true;
                }
                None => {
                    self.open.insert(key.clone(), p);
                }
            }
        }
        if !closed_now && !self.open.contains_key(key) {
            if let StrategyAction::Enter(sig) = self.strategy.evaluate(window(bars), None) {
                let entry = if sig.side == Side::Buy { bar.close + spread } else { bar.close };
                let sl = entry - sig.side.sign() * sig.sl_distance;
                let tp = entry + sig.side.sign() * sig.tp_distance;
                if sl > 0.0 && tp > 0.0 {
                    self.open.insert(key.clone(), PaperPos { side: sig.side, entry, sl, tp, entry_ts: bar.open_ts, bars: 0 });
                }
            }
        }
    }
}

struct Hypothetical {
    d: ShadowDecision,
    bars: usize,
}

pub(crate) struct Lab {
    cfg: ShadowConfig,
    started_ms: TimestampMs,
    updated_ms: TimestampMs,
    specs: SpecMap,
    candidates: Vec<Candidate>,
    hypotheticals: Vec<Hypothetical>,
    missed: HashMap<String, MissedByCheck>,
    analysis: TradeAnalysis,
    eff_sum: f64,
    eff_n: u32,
    bars_processed: u64,
    decisions_seen: u64,
}

fn variants(config: &Value) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for (field, factors) in [("tp_atr", [0.6, 1.5]), ("sl_atr", [0.75, 1.33])] {
        if let Some(v) = config.get(field).and_then(Value::as_f64) {
            for f in factors {
                let mut c = config.clone();
                c[field] = Value::from((v * f * 100.0).round() / 100.0);
                out.push((format!("{field}x{f}"), c));
            }
        }
    }
    out
}

impl Lab {
    pub(crate) fn new(cfg: ShadowConfig, now: TimestampMs) -> Lab {
        let mut lab = Lab {
            cfg,
            started_ms: now,
            updated_ms: now,
            specs: HashMap::new(),
            candidates: Vec::new(),
            hypotheticals: Vec::new(),
            missed: HashMap::new(),
            analysis: TradeAnalysis::default(),
            eff_sum: 0.0,
            eff_n: 0,
            bars_processed: 0,
            decisions_seen: 0,
        };
        lab.set_champions(STRATEGY_IDS.iter().map(|id| (id.to_string(), Value::Null, false)).collect());
        lab
    }

    /// (Re)build the candidate list; candidates whose id and configuration are
    /// unchanged keep their forward history.
    fn set_champions(&mut self, champions: Vec<(String, Value, bool)>) {
        let mut old: HashMap<String, Candidate> = self.candidates.drain(..).map(|c| (c.id.clone(), c)).collect();
        let mut next = Vec::new();
        for (sid, cfg, enabled) in champions {
            let mut list = vec![(sid.clone(), Role::Champion, cfg.clone())];
            if self.cfg.variants {
                let base = build(&sid, &cfg).map(|s| s.config()).unwrap_or(cfg.clone());
                for (label, c) in variants(&base) {
                    list.push((format!("{sid}~{label}"), Role::Challenger, c));
                }
            }
            for (id, role, c) in list {
                let Some(fresh) = Candidate::new(id.clone(), &sid, role, c, enabled && role == Role::Champion) else { continue };
                match old.remove(&id) {
                    Some(mut prev) if prev.config == fresh.config => {
                        prev.live_enabled = fresh.live_enabled;
                        next.push(prev);
                    }
                    _ => next.push(fresh),
                }
            }
        }
        self.candidates = next;
    }

    pub(crate) fn handle(&mut self, msg: ShadowInput) {
        match msg {
            ShadowInput::Config(c) => {
                let rebuild = c.variants != self.cfg.variants;
                self.cfg = c;
                if rebuild {
                    let champs = self.candidates.iter().filter(|c| c.role == Role::Champion).map(|c| (c.strategy_id.clone(), c.config.clone(), c.live_enabled)).collect();
                    self.set_champions(champs);
                }
            }
            ShadowInput::Spec(s) => {
                self.specs.insert(s.symbol.clone(), s);
            }
            ShadowInput::Champions(list) => self.set_champions(list),
            ShadowInput::Bar { symbol, timeframe, window } => self.on_bar(symbol, timeframe, &window),
            ShadowInput::Decision(d) => {
                self.decisions_seen += 1;
                if d.outcome != "approved" && d.entry > 0.0 && d.sl > 0.0 && d.tp > 0.0 {
                    self.hypotheticals.push(Hypothetical { d, bars: 0 });
                    if self.hypotheticals.len() > 5_000 {
                        self.hypotheticals.remove(0);
                    }
                }
            }
            ShadowInput::TradeClosed(t) => self.on_trade(t),
            ShadowInput::History { symbol, timeframe, origin, bars } => self.evaluate_history(&symbol, timeframe, &origin, &bars),
            ShadowInput::Restore(r) => self.restore(*r),
        }
    }

    fn on_bar(&mut self, symbol: String, tf: Timeframe, bars: &[Candle]) {
        if !self.cfg.enabled {
            return;
        }
        let Some(spec) = self.specs.get(&symbol).cloned() else { return };
        let Some(last) = bars.last().copied() else { return };
        self.bars_processed += 1;
        self.updated_ms = last.open_ts;
        let key = (symbol.clone(), tf);
        let commission = self.cfg.commission_per_lot_round_turn;
        for c in &mut self.candidates {
            c.on_bar(&key, bars, &spec, commission);
        }
        // Hypothetical outcomes of refused signals: only bars that opened after the decision count.
        let spread = last.spread_points.max(0.0) * spec.point;
        let horizon = self.cfg.missed_horizon_bars;
        let mut resolved: Vec<(ShadowDecision, Option<bool>)> = Vec::new();
        self.hypotheticals.retain_mut(|h| {
            if h.d.symbol != symbol || h.d.timeframe != tf || last.open_ts < h.d.ts_ms {
                return true;
            }
            h.bars += 1;
            let (stop, target) = match h.d.side {
                Side::Buy => (last.low <= h.d.sl, last.high >= h.d.tp),
                Side::Sell => (last.high + spread >= h.d.sl, last.low + spread <= h.d.tp),
            };
            if stop || target {
                resolved.push((h.d.clone(), Some(!stop)));
                false
            } else if h.bars >= horizon {
                resolved.push((h.d.clone(), None));
                false
            } else {
                true
            }
        });
        for (d, won) in resolved {
            let checks = if d.failed_checks.is_empty() { vec!["mode_monitor".to_string()] } else { d.failed_checks.clone() };
            for check in checks {
                let m = self.missed.entry(check.clone()).or_insert_with(|| MissedByCheck { check, ..Default::default() });
                match won {
                    Some(true) => m.would_have_won += 1,
                    Some(false) => m.would_have_lost += 1,
                    None => m.undecided += 1,
                }
            }
        }
    }

    fn on_trade(&mut self, t: ShadowTrade) {
        let a = &mut self.analysis;
        a.trades += 1;
        let risk = (t.entry - t.sl_initial).abs();
        let target = (t.tp_initial - t.entry).abs();
        if risk > 0.0 && t.mae_price >= 0.8 * risk {
            a.premature_entries += 1;
            a.notes.push(format!("#{} {}: entrée prématurée — {:.0} % du risque initial subi avant la sortie", t.ticket, t.strategy_id, t.mae_price / risk * 100.0));
        }
        if t.mfe_price > 0.0 {
            let eff = ((t.exit - t.entry) * t.side.sign() / t.mfe_price).clamp(-5.0, 1.0);
            self.eff_sum += eff;
            self.eff_n += 1;
            if target > 0.0 && t.mfe_price >= 0.8 * target && eff < 0.3 {
                a.premature_exits += 1;
                a.notes.push(format!("#{} {}: sortie tardive/prématurée — {:.0} % de la cible atteinte puis {:.0} % du meilleur mouvement conservé", t.ticket, t.strategy_id, t.mfe_price / target * 100.0, eff * 100.0));
            }
        }
        a.avg_exit_efficiency = (self.eff_n > 0).then(|| self.eff_sum / self.eff_n as f64);
        if a.notes.len() > 30 {
            let n = a.notes.len() - 30;
            a.notes.drain(..n);
        }
    }

    fn evaluate_history(&mut self, symbol: &str, tf: Timeframe, origin: &str, bars: &[Candle]) {
        let Some(spec) = self.specs.get(symbol).cloned() else { return };
        let params = BacktestParams { commission_per_lot_round_turn: self.cfg.commission_per_lot_round_turn, ..Default::default() };
        for c in &mut self.candidates {
            let full = run(c.strategy.as_ref(), &spec, bars, &params);
            let seg = run_segmented(c.strategy.as_ref(), &spec, bars, &params, 0.6, 0.2);
            let Some(oos) = seg.iter().find(|s| s.segment == "oos") else { continue };
            let summary = OosSummary {
                symbol: symbol.into(),
                timeframe: tf.as_str().into(),
                origin: origin.into(),
                full_trades: full.metrics.trades,
                full_net: full.metrics.net_profit,
                oos_trades: oos.result.metrics.trades,
                oos_net: oos.result.metrics.net_profit,
                oos_profit_factor: oos.result.metrics.profit_factor,
            };
            c.oos.retain(|o| !(o.symbol == summary.symbol && o.timeframe == summary.timeframe && o.origin == summary.origin));
            c.oos.push(summary);
        }
    }

    fn restore(&mut self, r: ShadowReport) {
        self.started_ms = r.started_ms;
        self.bars_processed = r.bars_processed;
        self.decisions_seen = r.decisions_seen;
        self.analysis = r.trade_analysis;
        self.missed = r.missed.into_iter().map(|m| (m.check.clone(), m)).collect();
        for v in r.candidates {
            if let Some(c) = self.candidates.iter_mut().find(|c| c.id == v.id && c.config == v.config) {
                c.forward = v.forward;
                c.oos = v.oos;
                c.cum = c.forward.net;
                c.peak = c.cum + c.forward.max_drawdown;
            }
        }
    }

    fn stage_and_verdict(&self, c: &Candidate) -> (PipelineStage, String) {
        let backtest = c.oos.iter().any(|o| o.full_net > 0.0);
        let oos = c.oos.iter().any(|o| o.full_net > 0.0 && o.oos_trades >= aegis_strategy::MIN_OOS_TRADES && o.oos_net > 0.0);
        let champion = self.candidates.iter().find(|x| x.strategy_id == c.strategy_id && x.role == Role::Champion);
        let f = &c.forward;
        let enough = f.trades >= self.cfg.forward_min_trades;
        let beats_champion = match champion.filter(|ch| ch.id != c.id && ch.forward.trades >= self.cfg.forward_min_trades) {
            Some(ch) => f.expectancy.unwrap_or(f64::NEG_INFINITY) >= ch.forward.expectancy.unwrap_or(f64::INFINITY),
            None => true,
        };
        let forward = oos && enough && f.net > 0.0 && beats_champion;
        let stage = if forward {
            PipelineStage::Forward
        } else if oos {
            PipelineStage::OutOfSample
        } else if backtest {
            PipelineStage::Backtest
        } else {
            PipelineStage::Candidate
        };
        let verdict = match stage {
            PipelineStage::Forward if c.role == Role::Challenger => "prêt pour la démo: promotion possible par une commande explicite".to_string(),
            PipelineStage::Forward => "champion confirmé en forward".to_string(),
            _ if c.oos.is_empty() => "aucun backtest: lancer l'évaluation de l'historique".to_string(),
            PipelineStage::Candidate => "backtest négatif".to_string(),
            PipelineStage::Backtest => format!("hors échantillon insuffisant (≥ {} transactions positives requises)", aegis_strategy::MIN_OOS_TRADES),
            PipelineStage::OutOfSample if !enough => format!("forward: {}/{} transactions", f.trades, self.cfg.forward_min_trades),
            PipelineStage::OutOfSample if f.net <= 0.0 => "forward négatif".to_string(),
            PipelineStage::OutOfSample => "forward moins bon que le champion".to_string(),
        };
        (stage, verdict)
    }

    pub(crate) fn report(&self) -> ShadowReport {
        let mut candidates: Vec<CandidateView> = self
            .candidates
            .iter()
            .map(|c| {
                let (stage, verdict) = self.stage_and_verdict(c);
                CandidateView {
                    id: c.id.clone(),
                    strategy_id: c.strategy_id.clone(),
                    role: c.role,
                    config: c.config.clone(),
                    live_enabled: c.live_enabled,
                    stage,
                    forward: c.forward.clone(),
                    oos: c.oos.clone(),
                    open_positions: c.open.len(),
                    verdict,
                }
            })
            .collect();
        candidates.sort_by(|a, b| (&a.strategy_id, a.role as u8, &a.id).cmp(&(&b.strategy_id, b.role as u8, &b.id)));
        let mut missed: Vec<MissedByCheck> = self.missed.values().cloned().collect();
        missed.sort_by(|a, b| (b.would_have_won + b.would_have_lost).cmp(&(a.would_have_won + a.would_have_lost)));
        ShadowReport {
            started_ms: self.started_ms,
            updated_ms: self.updated_ms,
            bars_processed: self.bars_processed,
            decisions_seen: self.decisions_seen,
            candidates,
            missed,
            pending_hypotheticals: self.hypotheticals.len(),
            trade_analysis: self.analysis.clone(),
        }
    }
}
