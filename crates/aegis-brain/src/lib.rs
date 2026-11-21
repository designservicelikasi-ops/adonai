//! AEGIS BRAIN — the micro-profit brain.
//!
//! Goal: take trades whose NET result if the target is reached (after spread,
//! commission, slippage and the extra costs learned from real fills) lies
//! between `min_net_win` and `max_net_win` (0.08 to 2.00 in account currency
//! by default), and among those pick the one with the best net expectation.
//!
//! It gets better with time in three measurable ways, all learned from the
//! trade journal (closed AEGIS trades) and recomputed continuously:
//!
//! 1. **Real costs** — commission and slippage actually paid per lot. When the
//!    broker costs more than configured, the difference is added to every
//!    future estimate (never subtracted: a cheaper broker does not make it bolder).
//! 2. **Calibration** — wins actually obtained versus wins the probabilities
//!    promised. When the model was too optimistic, every probability is reduced
//!    by the observed ratio (the "haircut"); it is never increased above the model.
//! 3. **Contexts** — results per strategy, side and trading session. A context
//!    that is losing money with statistical confidence is no longer traded.
//!
//! A fourth improvement — retraining the probability model on the latest
//! candles and comparing it with the active one on data it never saw — is
//! orchestrated by the engine (`aegis-core`); this crate holds the comparison.
//! A better version is only ever *proposed*: the user activates it.
//!
//! The brain never changes the risk limits: it chooses a target and may ask for
//! a SMALLER volume than the risk engine allows (to keep the gain under the
//! maximum), never a larger one. The risk engine re-checks everything.

use aegis_data::JournalEntry;
use aegis_probability::HoldoutScore;
use aegis_risk::{evaluate_micro_profit, RiskConfig};
use aegis_types::{Side, SymbolSpec, TimestampMs};
use serde::{Deserialize, Serialize};

// ------------------------------------------------------------------ config

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrainConfig {
    /// Off by default: strategies then keep their own targets.
    pub enabled: bool,
    /// Instruments the brain trades. A broker variant matches too (`EURUSD` covers
    /// `EURUSDm`, `EURUSD.r`, `EURUSD.sim`...).
    pub symbols: Vec<String>,
    /// Smallest net gain worth taking if the target is reached (account currency).
    pub min_net_win: f64,
    /// Largest net gain per trade (account currency).
    pub max_net_win: f64,
    /// The chosen trade must have a net expectation strictly above this.
    pub min_expected_net: f64,
    /// Stop-loss in pips. 0 = the strategy's own stop.
    pub sl_pips: f64,
    /// Fixed take-profit in pips. 0 = the brain chooses it in the net gain range.
    pub tp_pips: f64,
    /// Farthest take-profit the brain may choose, in pips. 0 = no limit.
    pub max_tp_pips: f64,
    /// Targets tried per volume, spread evenly (log scale) between the min and max gain.
    pub target_steps: usize,
    /// A target closer than this fraction of the ATR is market noise: never used.
    pub min_tp_atr: f64,
    /// Strength of the prior (in trades): lessons weigh n / (n + prior_trades).
    pub prior_trades: f64,
    /// Trades needed in a context before it can be blocked.
    pub context_min_trades: usize,
    /// Most recent trades used to measure the calibration.
    pub calibration_window: usize,
    /// Automatic retraining period of the probability model, in hours. 0 = never.
    pub retrain_every_hours: u32,
    /// Most recent share of the history kept aside to compare versions (percent).
    pub holdout_pct: f64,
    /// Brier improvement a new version needs over the active one to be proposed.
    pub min_brier_gain: f64,
}

impl Default for BrainConfig {
    fn default() -> Self {
        BrainConfig {
            enabled: false,
            symbols: vec!["EURUSD".into()],
            min_net_win: 0.08,
            max_net_win: 2.0,
            min_expected_net: 0.0,
            sl_pips: 0.0,
            tp_pips: 0.0,
            max_tp_pips: 0.0,
            target_steps: 12,
            min_tp_atr: 0.1,
            prior_trades: 20.0,
            context_min_trades: 30,
            calibration_window: 200,
            retrain_every_hours: 24,
            holdout_pct: 20.0,
            min_brier_gain: 0.0005,
        }
    }
}

impl BrainConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(self.min_net_win > 0.0 && self.max_net_win >= self.min_net_win && self.max_net_win <= 1_000.0) {
            return Err("brain: 0 < min_net_win <= max_net_win <= 1000".into());
        }
        if !self.min_expected_net.is_finite() || self.min_expected_net < 0.0 {
            return Err("brain: min_expected_net must be >= 0".into());
        }
        for (name, v) in [("sl_pips", self.sl_pips), ("tp_pips", self.tp_pips), ("max_tp_pips", self.max_tp_pips)] {
            if !(v.is_finite() && (0.0..=10_000.0).contains(&v)) {
                return Err(format!("brain: {name} must be within 0..10000 (0 = automatic)"));
            }
        }
        if self.tp_pips > 0.0 && self.max_tp_pips > 0.0 && self.tp_pips > self.max_tp_pips {
            return Err("brain: tp_pips cannot exceed max_tp_pips".into());
        }
        if !(2..=50).contains(&self.target_steps) {
            return Err("brain: target_steps 2..50".into());
        }
        if !(0.0..=5.0).contains(&self.min_tp_atr) || !(0.0..=1_000.0).contains(&self.prior_trades) {
            return Err("brain: min_tp_atr 0..5, prior_trades 0..1000".into());
        }
        if !(5..=10_000).contains(&self.context_min_trades) || !(10..=100_000).contains(&self.calibration_window) {
            return Err("brain: context_min_trades 5..10000, calibration_window 10..100000".into());
        }
        if !(5.0..=50.0).contains(&self.holdout_pct) || !(0.0..=0.1).contains(&self.min_brier_gain) || self.retrain_every_hours > 24 * 90 {
            return Err("brain: holdout_pct 5..50, min_brier_gain 0..0.1, retrain_every_hours <= 2160".into());
        }
        if self.symbols.iter().any(|s| s.trim().is_empty()) {
            return Err("brain: empty symbol".into());
        }
        Ok(())
    }

    /// Whether the brain chooses the targets of `symbol`.
    pub fn applies_to(&self, symbol: &str) -> bool {
        self.enabled && self.covers(symbol)
    }

    /// `symbol` is one of the brain's instruments (or a broker variant of one).
    pub fn covers(&self, symbol: &str) -> bool {
        let up = symbol.to_ascii_uppercase();
        self.symbols.iter().any(|s| up.starts_with(&s.to_ascii_uppercase()))
    }
}

// ----------------------------------------------------------------- targets

/// Size of one pip: 10 points on 3- and 5-digit quotes (EURUSD 0.00010), else one point.
pub fn pip_size(spec: &SymbolSpec) -> f64 {
    if spec.digits == 3 || spec.digits == 5 { spec.point * 10.0 } else { spec.point }
}

/// Everything needed to choose the target of one signal.
#[derive(Debug, Clone)]
pub struct TargetRequest<'a> {
    pub spec: &'a SymbolSpec,
    pub risk: &'a RiskConfig,
    pub side: Side,
    /// Expected fill (ask for a buy, bid for a sell).
    pub entry: f64,
    /// Stop-loss price (from the strategy; the brain never moves it).
    pub sl: f64,
    pub spread_price: f64,
    /// Volume the risk engine would allow (`aegis_risk::size_volume`).
    pub risk_volume: f64,
    pub atr: Option<f64>,
    /// The strategy's own target distance (always tried too).
    pub strategy_tp_distance: f64,
    /// Learned extra cost per lot (`Lessons::extra_cost_per_lot`).
    pub extra_cost_per_lot: f64,
    /// Learned calibration factor in (0, 1] (`Lessons::haircut`).
    pub haircut: f64,
    /// The user's fixed take-profit distance (price): the only target tried.
    pub fixed_tp_distance: Option<f64>,
    /// The user's farthest take-profit distance (price).
    pub max_tp_distance: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetCandidate {
    pub volume: f64,
    pub tp_distance: f64,
    pub tp: f64,
    /// Probability from the model(s), before the haircut.
    pub probability_raw: f64,
    /// Probability used: raw x haircut.
    pub probability: f64,
    pub probability_source: String,
    /// Net result if the target is reached, learned costs included.
    pub net_if_win: f64,
    /// Net result if the stop is reached (negative), learned costs included.
    pub net_if_loss: f64,
    pub expected_net: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetChoice {
    /// Best expectation among the targets that pass the risk engine's thresholds
    /// (minimum probability for the side, minimum net expectation).
    pub chosen: Option<TargetCandidate>,
    /// The chosen one, or else the best expectation even below the thresholds (shown, never traded).
    pub best: Option<TargetCandidate>,
    /// Targets tried / inside the gain range / with a usable probability.
    pub tried: usize,
    pub in_range: usize,
    pub with_probability: usize,
    pub reason: String,
}

/// Volumes tried: the risk volume, then halves of it down to the broker minimum.
fn volumes(spec: &SymbolSpec, risk_volume: f64) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    let mut v = spec.normalize_volume(risk_volume);
    while v > 0.0 && out.len() < 12 {
        if !out.iter().any(|x| (x - v).abs() < 1e-9) {
            out.push(v);
        }
        let next = spec.normalize_volume(v / 2.0);
        if next <= 0.0 || (next - v).abs() < 1e-9 {
            break;
        }
        v = next;
    }
    let min = spec.normalize_volume(spec.volume_min);
    if min > 0.0 && min <= risk_volume + 1e-9 && !out.iter().any(|x| (x - min).abs() < 1e-9) {
        out.push(min);
    }
    out
}

/// Choose the target (and possibly a smaller volume) of one signal.
/// `probability(tp_distance)` gives the validated P(TP before SL) for a target
/// distance and why; `None` when no validated source covers that target.
pub fn choose_target(cfg: &BrainConfig, req: &TargetRequest<'_>, probability: impl Fn(f64) -> (Option<f64>, String)) -> TargetChoice {
    let mut choice = TargetChoice { chosen: None, best: None, tried: 0, in_range: 0, with_probability: 0, reason: String::new() };
    let spec = req.spec;
    let sign = req.side.sign();
    let haircut = req.haircut.clamp(0.0, 1.0);
    let extra = req.extra_cost_per_lot.max(0.0);
    let min_distance = (spec.stops_level_points + 1.0) * spec.point;
    let noise = req.atr.map(|a| a * cfg.min_tp_atr).unwrap_or(0.0);
    let vols = volumes(spec, req.risk_volume);
    if vols.is_empty() {
        choice.reason = "le budget de risque ne permet même pas le volume minimum".into();
        return choice;
    }
    // The risk engine's thresholds: a target it would refuse is never chosen.
    let min_p = match req.side {
        Side::Buy => req.risk.buy_min_probability,
        Side::Sell => req.risk.sell_min_probability,
    };
    let min_ev = req.risk.min_net_expected_profit.max(cfg.min_expected_net);
    let passes = |c: &TargetCandidate| c.probability >= min_p && c.expected_net > cfg.min_expected_net && c.expected_net >= min_ev && c.expected_net > 0.0;
    // Why targets had no probability: (reason, count), most frequent shown.
    let mut missing: Vec<(String, usize)> = Vec::new();
    let evaluate = |v: f64, d: f64| {
        let tp = spec.round_price(req.entry + sign * d);
        let m = evaluate_micro_profit(spec, req.risk, req.side, v, req.entry, req.sl, tp, req.spread_price, None, "");
        let real_d = (tp - req.entry).abs();
        (tp, real_d, m.net_if_win - extra * v, m.net_if_loss - extra * v)
    };
    for &v in &vols {
        // net_if_win is linear in the target distance: solve it for each gain.
        let (_, d1, n1, _) = evaluate(v, 100.0 * spec.point);
        let (_, d2, n2, _) = evaluate(v, 1_000.0 * spec.point);
        let slope = (n2 - n1) / (d2 - d1);
        if slope.is_nan() || slope <= 0.0 {
            continue;
        }
        let distances: Vec<f64> = match req.fixed_tp_distance {
            Some(fixed) => vec![fixed],
            None => {
                let mut v_d: Vec<f64> = (0..cfg.target_steps)
                    .map(|k| {
                        let t = k as f64 / (cfg.target_steps - 1) as f64;
                        let gain = cfg.min_net_win * (cfg.max_net_win / cfg.min_net_win).powf(t);
                        d1 + (gain - n1) / slope
                    })
                    .collect();
                if (v - vols[0]).abs() < 1e-9 && req.strategy_tp_distance > 0.0 {
                    v_d.push(req.strategy_tp_distance);
                }
                v_d
            }
        };
        for d in distances {
            if !(d.is_finite() && d >= min_distance && (d >= noise || req.fixed_tp_distance.is_some())) {
                continue;
            }
            if req.max_tp_distance.is_some_and(|m| d > m * 1.000_001) {
                continue;
            }
            choice.tried += 1;
            let (tp, real_d, win, loss) = evaluate(v, d);
            if win < cfg.min_net_win - 1e-9 || win > cfg.max_net_win + 1e-9 {
                continue;
            }
            choice.in_range += 1;
            let (p, source) = probability(real_d);
            let Some(p_raw) = p.filter(|p| p.is_finite() && (0.0..=1.0).contains(p)) else {
                // Numbers vary per target; group on the text before the first digit.
                let key: String = source.chars().take_while(|c| !c.is_ascii_digit()).collect();
                match missing.iter_mut().find(|(k, _)| k.chars().take_while(|c| !c.is_ascii_digit()).collect::<String>() == key) {
                    Some(m) => m.1 += 1,
                    None => missing.push((source, 1)),
                }
                continue;
            };
            choice.with_probability += 1;
            let p_used = p_raw * haircut;
            let expected = p_used * win + (1.0 - p_used) * loss;
            let c = TargetCandidate {
                volume: v,
                tp_distance: real_d,
                tp,
                probability_raw: p_raw,
                probability: p_used,
                probability_source: source,
                net_if_win: win,
                net_if_loss: loss,
                expected_net: expected,
            };
            if passes(&c) && choice.chosen.as_ref().is_none_or(|b| c.expected_net > b.expected_net + 1e-12) {
                choice.chosen = Some(c.clone());
            }
            if choice.best.as_ref().is_none_or(|b| c.expected_net > b.expected_net + 1e-12) {
                choice.best = Some(c);
            }
        }
    }
    let lo = cfg.min_net_win;
    let hi = cfg.max_net_win;
    if let Some(b) = &choice.chosen {
        // The best of the targets that pass every threshold is the one shown.
        choice.best = Some(b.clone());
    }
    match (&choice.chosen, &choice.best) {
        (Some(b), _) => {
            choice.reason = format!(
                "cible {:+.2} net si gagné ({:.2} lot, TP à {:.1} points), p {:.1} % → espérance {:+.3} par transaction",
                b.net_if_win,
                b.volume,
                b.tp_distance / spec.point,
                b.probability * 100.0,
                b.expected_net
            );
        }
        (None, Some(b)) if b.expected_net <= 0.0 => {
            choice.reason = format!(
                "aucune cible rentable entre {lo:.2} et {hi:.2} net: la meilleure ({:+.2} si gagné, p {:.1} %) a une espérance de {:+.3}",
                b.net_if_win,
                b.probability * 100.0,
                b.expected_net
            );
        }
        (None, Some(b)) => {
            choice.reason = format!(
                "aucune cible ne passe les seuils de risque (probabilité ≥ {:.0} %, espérance ≥ {min_ev:.2}): la meilleure ({:+.2} si gagné) a p {:.1} % et une espérance de {:+.3}",
                min_p * 100.0,
                b.net_if_win,
                b.probability * 100.0,
                b.expected_net
            );
        }
        (None, None) if choice.in_range == 0 => {
            choice.reason = format!(
                "aucune cible possible entre {lo:.2} et {hi:.2} net ({} essayées): frais trop élevés ou distance sous le bruit du marché",
                choice.tried
            );
        }
        (None, None) => {
            missing.sort_by(|a, b| b.1.cmp(&a.1));
            let why: Vec<String> = missing.iter().take(2).map(|(s, n)| format!("{s} ({n} cible(s))")).collect();
            choice.reason = format!("aucune probabilité validée pour les {} cibles entre {lo:.2} et {hi:.2} net: {}", choice.in_range, why.join(" ; "));
        }
    }
    choice
}

// ----------------------------------------------------------------- lessons

/// Trading session of a time (UTC), the unit in which results are compared.
pub fn session_of(ts_ms: TimestampMs) -> &'static str {
    let hour = (ts_ms.rem_euclid(86_400_000) / 3_600_000) as u32;
    match hour {
        0..=6 => "asie",
        7..=11 => "londres",
        12..=15 => "londres_new_york",
        16..=20 => "new_york",
        _ => "fermeture",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CostLesson {
    pub trades: usize,
    /// Commission + slippage per lot assumed by the configuration.
    pub configured_per_lot: f64,
    /// Commission + fees + slippage actually paid per lot (mean).
    pub observed_per_lot: Option<f64>,
    /// Added to every estimate (per lot), never negative.
    pub extra_per_lot: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CalibrationLesson {
    pub trades: usize,
    /// Sum of the probabilities (wins the model promised).
    pub promised_wins: f64,
    pub actual_wins: usize,
    /// Multiplier applied to every probability, in (0, 1].
    pub haircut: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextLesson {
    pub strategy_id: String,
    pub side: Side,
    pub session: String,
    pub trades: usize,
    pub wins: usize,
    pub net: f64,
    pub mean_net: f64,
    /// Standard error of the mean net result.
    pub std_error: f64,
    /// Losing with confidence (mean + 1 standard error < 0, enough trades): not traded.
    pub blocked: bool,
    pub verdict: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Lessons {
    pub symbol: String,
    pub updated_ms: TimestampMs,
    /// Closed trades of this instrument (same kind of terminal) the lessons come from.
    pub trades: usize,
    pub net: f64,
    pub cost: CostLesson,
    pub calibration: CalibrationLesson,
    pub contexts: Vec<ContextLesson>,
}

impl Lessons {
    pub fn empty(symbol: &str, configured_per_lot: f64) -> Lessons {
        Lessons {
            symbol: symbol.into(),
            cost: CostLesson { configured_per_lot, ..Default::default() },
            calibration: CalibrationLesson { haircut: 1.0, ..Default::default() },
            ..Default::default()
        }
    }
    pub fn haircut(&self) -> f64 {
        if self.calibration.haircut > 0.0 { self.calibration.haircut } else { 1.0 }
    }
    pub fn extra_cost_per_lot(&self) -> f64 {
        self.cost.extra_per_lot.max(0.0)
    }
    pub fn context(&self, strategy_id: &str, side: Side, session: &str) -> Option<&ContextLesson> {
        self.contexts.iter().find(|c| c.strategy_id == strategy_id && c.side == side && c.session == session)
    }
}

/// Learn from the closed trades of one instrument (oldest first). The caller keeps
/// only trades of the same kind of terminal (a simulator never teaches the real broker).
pub fn learn(cfg: &BrainConfig, symbol: &str, trades: &[JournalEntry], configured_per_lot: f64, now_ms: TimestampMs) -> Lessons {
    let mut l = Lessons::empty(symbol, configured_per_lot);
    l.updated_ms = now_ms;
    let closed: Vec<&JournalEntry> = trades.iter().filter(|t| t.closed_ms.is_some() && t.side.is_some()).collect();
    l.trades = closed.len();
    l.net = closed.iter().map(|t| t.net).sum();
    let k = cfg.prior_trades.max(0.0);

    // 1. Real costs per lot.
    let per_lot: Vec<f64> = closed
        .iter()
        .filter(|t| t.volume > 0.0)
        .map(|t| ((-(t.commission + t.fee)).max(0.0) + t.slippage_cost) / t.volume)
        .filter(|x| x.is_finite())
        .collect();
    l.cost.trades = per_lot.len();
    if !per_lot.is_empty() {
        let n = per_lot.len() as f64;
        let mean = per_lot.iter().sum::<f64>() / n;
        l.cost.observed_per_lot = Some(mean);
        l.cost.extra_per_lot = (n / (n + k) * (mean - configured_per_lot)).max(0.0);
    }

    // 2. Calibration on the most recent trades that used a probability.
    let with_p: Vec<(f64, bool)> = closed
        .iter()
        .filter_map(|t| t.probability_raw.or(t.probability).filter(|p| p.is_finite()).map(|p| (p, t.net > 0.0)))
        .collect();
    let recent = &with_p[with_p.len().saturating_sub(cfg.calibration_window)..];
    l.calibration.trades = recent.len();
    if !recent.is_empty() {
        let promised: f64 = recent.iter().map(|x| x.0).sum();
        let wins = recent.iter().filter(|x| x.1).count();
        let p_bar = promised / recent.len() as f64;
        l.calibration.promised_wins = promised;
        l.calibration.actual_wins = wins;
        // Shrunk towards "the model is right": k trades that went exactly as promised.
        let ratio = (wins as f64 + k * p_bar) / (promised + k * p_bar);
        l.calibration.haircut = if ratio.is_finite() { ratio.clamp(0.05, 1.0) } else { 1.0 };
    }

    // 3. Contexts: strategy x side x session.
    let mut keys: Vec<(String, Side, &'static str)> = Vec::new();
    for t in &closed {
        let key = (t.strategy_id.clone(), t.side.unwrap_or(Side::Buy), session_of(t.opened_ms));
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    for (strategy_id, side, session) in keys {
        let nets: Vec<f64> = closed
            .iter()
            .filter(|t| t.strategy_id == strategy_id && t.side == Some(side) && session_of(t.opened_ms) == session)
            .map(|t| t.net)
            .collect();
        let n = nets.len();
        let net: f64 = nets.iter().sum();
        let mean = net / n as f64;
        let var = if n > 1 { nets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64 } else { 0.0 };
        let se = (var / n as f64).sqrt();
        let enough = n >= cfg.context_min_trades;
        let blocked = enough && mean + se < 0.0;
        let verdict = if !enough {
            format!("en apprentissage ({n}/{} transactions)", cfg.context_min_trades)
        } else if blocked {
            format!("perdant de façon répétée ({mean:+.3} par transaction): n'est plus tradé")
        } else if mean - se > 0.0 {
            format!("rentable de façon répétée ({mean:+.3} par transaction)")
        } else {
            format!("pas de différence nette avec zéro ({mean:+.3} ± {se:.3})")
        };
        l.contexts.push(ContextLesson {
            strategy_id,
            side,
            session: session.into(),
            trades: n,
            wins: nets.iter().filter(|x| **x > 0.0).count(),
            net,
            mean_net: mean,
            std_error: se,
            blocked,
            verdict,
        });
    }
    l.contexts.sort_by(|a, b| b.trades.cmp(&a.trades).then(a.strategy_id.cmp(&b.strategy_id)));
    l
}

// ------------------------------------------------------- version comparison

/// Whether a newly trained version is better than the active one on the holdout
/// period (data neither was selected on; the active one may even have seen it,
/// which only makes the comparison harder for the new version).
pub fn candidate_is_better(cfg: &BrainConfig, candidate: &HoldoutScore, active: Option<&HoldoutScore>) -> (bool, String) {
    let sides = |s: &HoldoutScore| [("achat", s.buy.clone()), ("vente", s.sell.clone())];
    let Some(active) = active.filter(|a| a.buy.predicted + a.sell.predicted > 0) else {
        let n = candidate.buy.predicted + candidate.sell.predicted;
        return if n > 0 {
            (true, format!("aucun modèle actif utilisable: la nouvelle version donne une probabilité pour {n} scénarios de la période de contrôle"))
        } else {
            (false, "la nouvelle version ne donne aucune probabilité sur la période de contrôle".into())
        };
    };
    let mut gains = Vec::new();
    let (mut weighted, mut weight) = (0.0, 0.0);
    for ((name, c), (_, a)) in sides(candidate).into_iter().zip(sides(active)) {
        match (c.brier, a.brier) {
            (Some(cb), Some(ab)) => {
                if c.coverage() + 1e-9 < 0.9 * a.coverage() {
                    return (false, format!("{name}: la nouvelle version couvre moins de scénarios ({:.0} % contre {:.0} %)", c.coverage() * 100.0, a.coverage() * 100.0));
                }
                let n = c.predicted as f64;
                weighted += (ab - cb) * n;
                weight += n;
                gains.push(format!("{name} Brier {cb:.4} contre {ab:.4}"));
            }
            (None, Some(_)) => return (false, format!("{name}: la nouvelle version ne donne plus de probabilité")),
            (Some(cb), None) => gains.push(format!("{name}: nouvelle couverture (Brier {cb:.4})")),
            (None, None) => {}
        }
    }
    let gain = if weight > 0.0 { weighted / weight } else { 0.0 };
    let detail = gains.join(" ; ");
    if weight == 0.0 && !gains.is_empty() {
        return (true, detail);
    }
    if gain >= cfg.min_brier_gain {
        (true, format!("meilleure de {gain:.4} en Brier sur la période de contrôle ({detail})"))
    } else {
        (false, format!("pas assez meilleure: gain {gain:+.4} en Brier (minimum {:.4}) — {detail}", cfg.min_brier_gain))
    }
}

#[cfg(test)]
mod tests;
