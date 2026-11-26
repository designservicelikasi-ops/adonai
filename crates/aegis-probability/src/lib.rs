//! AEGIS PROBABILITY ENGINE — P(take-profit reached before stop-loss).
//!
//! This is NOT a directional probability. For a trade scenario (side, TP and SL
//! distances expressed in ATR), the engine estimates the probability that the
//! take-profit is hit first. Method:
//!
//! 1. **Labels** ("triple barrier") from historical candles: entry at the close
//!    (BUY pays the spread), then the next `horizon_bars` bars decide — TP first
//!    = 1; SL first, both inside the same bar (unknown order), or neither within
//!    the horizon = 0. Pessimistic by construction.
//! 2. **Features**: the 11 market features of `aegis-learning` (same code live and
//!    in training) plus the scenario (tp_atr, sl_atr, ln(sl/tp)).
//! 3. **Model**: ridge logistic regression (IRLS) on standardized features, one
//!    model per side. A *scenario-only* model (no market features) is the
//!    baseline the full model must beat.
//! 4. **Walk-forward** validation with a purge gap; Platt calibration cross-fitted
//!    across folds; Brier score, log loss, reliability diagram, ECE.
//! 5. **Selection**: the full model is used only if it beats the baseline by a
//!    margin; otherwise the calibrated baseline is used; if neither is calibrated
//!    the side has no usable probability.

mod linalg;
mod logistic;
pub mod metrics;

use aegis_learning::{compute_features, FEATURE_NAMES};
use aegis_types::{Candle, DataOrigin, Side, TimestampMs};
use logistic::{fit_logistic, Standardizer};
use metrics::{reliability, CalibrationReport};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use logistic::sigmoid;

pub const SCENARIO_FEATURES: [&str; 3] = ["tp_atr", "sl_atr", "prior_logit"];
/// Bars handed to the feature code for each row (same window as strategies).
const WINDOW: usize = 300;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProbabilityConfig {
    /// Take-profit distances (in ATR) covered by the training grid.
    pub tp_grid: Vec<f64>,
    /// Stop-loss distances (in ATR) covered by the training grid.
    pub sl_grid: Vec<f64>,
    /// Bars after which an open scenario counts as a failure.
    pub horizon_bars: usize,
    /// One row every `stride` bars (consecutive labels overlap heavily).
    pub stride: usize,
    pub folds: usize,
    pub ridge: f64,
    /// Minimum out-of-sample rows for a side to be usable.
    pub min_samples: usize,
    /// Brier improvement over the baseline the full model needs to be selected.
    pub min_brier_skill: f64,
    /// Maximum expected calibration error of the selected model.
    pub max_ece: f64,
    /// Scenario shown on the dashboard as "BUY/SELL probability".
    pub display_tp_atr: f64,
    pub display_sl_atr: f64,
}

impl Default for ProbabilityConfig {
    fn default() -> Self {
        ProbabilityConfig {
            tp_grid: vec![0.3, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0],
            sl_grid: vec![0.5, 1.0, 1.5, 2.0, 3.0],
            horizon_bars: 60,
            stride: 2,
            folds: 5,
            ridge: 1.0,
            min_samples: 2_000,
            min_brier_skill: 0.001,
            max_ece: 0.05,
            display_tp_atr: 0.5,
            display_sl_atr: 1.0,
        }
    }
}

impl ProbabilityConfig {
    pub fn validate(&self) -> Result<(), String> {
        let grid_ok = |g: &[f64]| !g.is_empty() && g.len() <= 20 && g.iter().all(|v| v.is_finite() && *v > 0.0 && *v <= 20.0);
        if !grid_ok(&self.tp_grid) || !grid_ok(&self.sl_grid) {
            return Err("probability grids need 1..20 positive ATR multiples up to 20".into());
        }
        if !(5..=2_000).contains(&self.horizon_bars) || !(1..=50).contains(&self.stride) || !(2..=20).contains(&self.folds) {
            return Err("horizon_bars 5..2000, stride 1..50, folds 2..20".into());
        }
        if !(self.ridge >= 0.0 && self.ridge <= 1e6) || !(0.0..=0.5).contains(&self.max_ece) || !(0.0..=0.25).contains(&self.min_brier_skill) {
            return Err("ridge 0..1e6, max_ece 0..0.5, min_brier_skill 0..0.25".into());
        }
        if !(self.display_tp_atr > 0.0 && self.display_sl_atr > 0.0) {
            return Err("display scenario must be positive".into());
        }
        Ok(())
    }

    fn in_grid(&self, tp_atr: f64, sl_atr: f64) -> bool {
        let span = |g: &[f64]| (g.iter().copied().fold(f64::INFINITY, f64::min), g.iter().copied().fold(0.0, f64::max));
        let ((tlo, thi), (slo, shi)) = (span(&self.tp_grid), span(&self.sl_grid));
        tp_atr >= tlo * 0.999 && tp_atr <= thi * 1.001 && sl_atr >= slo * 0.999 && sl_atr <= shi * 1.001
    }
}

/// Outcome of one scenario from bar `i`. `None` when the future is not fully known.
pub fn triple_barrier(bars: &[Candle], i: usize, side: Side, tp: f64, sl: f64, spread: f64, horizon: usize) -> Option<bool> {
    if i + horizon >= bars.len() || !(tp > 0.0 && sl > 0.0) {
        return None;
    }
    let close = bars[i].close;
    for b in &bars[i + 1..=i + horizon] {
        // Candles are bid prices; the ask is bid + spread.
        let (win, loss) = match side {
            Side::Buy => {
                let entry = close + spread;
                (b.high >= entry + tp, b.low <= entry - sl)
            }
            Side::Sell => {
                let entry = close;
                (b.low + spread <= entry - tp, b.high + spread >= entry + sl)
            }
        };
        if loss {
            return Some(false); // includes "both in the same bar": order unknown, assume the worst
        }
        if win {
            return Some(true);
        }
    }
    Some(false)
}

fn scenario_features(tp_atr: f64, sl_atr: f64) -> [f64; 3] {
    [tp_atr, sl_atr, (sl_atr / tp_atr).ln()]
}

/// Hash of the bars a model was trained on (dataset version).
pub fn dataset_hash(bars: &[Candle]) -> String {
    let mut h = Sha256::new();
    for b in bars {
        h.update(b.open_ts.to_le_bytes());
        for v in [b.open, b.high, b.low, b.close, b.spread_points] {
            h.update(v.to_le_bytes());
        }
    }
    hex::encode(h.finalize())
}

#[derive(Debug, Clone)]
struct Row {
    bar: usize,
    market: [f64; 11],
    scenario: [f64; 3],
    y: f64,
}

impl Row {
    fn full(&self) -> Vec<f64> {
        self.market.iter().chain(self.scenario.iter()).copied().collect()
    }
    fn base(&self) -> Vec<f64> {
        self.scenario.to_vec()
    }
}

fn build_rows(bars: &[Candle], side: Side, point: f64, cfg: &ProbabilityConfig) -> Vec<Row> {
    let mut rows = Vec::new();
    let start = aegis_learning::FEATURE_MIN_BARS.max(15);
    let mut i = start;
    while i + cfg.horizon_bars < bars.len() {
        let w = &bars[(i + 1).saturating_sub(WINDOW)..=i];
        if let (Some(market), Some(a)) = (compute_features(w), aegis_market::indicators::atr(w, 14).filter(|a| *a > 0.0)) {
            let spread = bars[i].spread_points.max(0.0) * point;
            for &tp in &cfg.tp_grid {
                for &sl in &cfg.sl_grid {
                    if let Some(y) = triple_barrier(bars, i, side, tp * a, sl * a, spread, cfg.horizon_bars) {
                        rows.push(Row { bar: i, market, scenario: scenario_features(tp, sl), y: if y { 1.0 } else { 0.0 } });
                    }
                }
            }
        }
        i += cfg.stride;
    }
    rows
}

/// A fitted logistic model on standardized inputs, with its Platt calibration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FittedModel {
    pub feature_names: Vec<String>,
    pub standardizer: Standardizer,
    /// Intercept first.
    pub weights: Vec<f64>,
    /// Calibrated p = sigmoid(a * logit + b).
    pub platt: (f64, f64),
}

impl FittedModel {
    pub fn logit(&self, x: &[f64]) -> Option<f64> {
        if x.len() + 1 != self.weights.len() || x.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let z = self.standardizer.apply(x);
        Some(self.weights[0] + z.iter().zip(&self.weights[1..]).map(|(a, b)| a * b).sum::<f64>())
    }
    pub fn predict(&self, x: &[f64]) -> Option<f64> {
        let l = self.logit(x)?;
        let p = sigmoid(self.platt.0 * l + self.platt.1);
        p.is_finite().then_some(p)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Selected {
    /// Market features + scenario: beat the baseline out of sample.
    Full,
    /// Scenario only: calibrated, but market features added nothing measurable.
    ScenarioOnly,
    /// Not calibrated or not enough data: no probability for this side.
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SideModel {
    pub side: Side,
    pub full: FittedModel,
    pub baseline: FittedModel,
    /// Out-of-sample, cross-fitted calibrated metrics.
    pub full_report: CalibrationReport,
    pub baseline_report: CalibrationReport,
    pub selected: Selected,
    pub selection_reason: String,
    pub base_rate: f64,
}

impl SideModel {
    pub fn predict(&self, market: &[f64; 11], tp_atr: f64, sl_atr: f64) -> Option<f64> {
        let sc = scenario_features(tp_atr, sl_atr);
        match self.selected {
            Selected::Full => self.full.predict(&market.iter().chain(sc.iter()).copied().collect::<Vec<_>>()),
            Selected::ScenarioOnly => self.baseline.predict(&sc),
            Selected::None => None,
        }
    }
    pub fn report(&self) -> Option<&CalibrationReport> {
        match self.selected {
            Selected::Full => Some(&self.full_report),
            Selected::ScenarioOnly => Some(&self.baseline_report),
            Selected::None => None,
        }
    }
}

/// A trained probability model for one instrument and timeframe (both sides).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbabilityModel {
    pub model_id: String,
    pub created_ms: TimestampMs,
    pub symbol: String,
    pub timeframe: String,
    pub origin: DataOrigin,
    pub dataset_hash: String,
    pub bars: usize,
    pub from_ts: TimestampMs,
    pub to_ts: TimestampMs,
    pub config: ProbabilityConfig,
    pub buy: SideModel,
    pub sell: SideModel,
}

/// Why a probability is or is not available, for the decision log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Estimate {
    pub probability: Option<f64>,
    pub source: String,
}

impl ProbabilityModel {
    pub fn side(&self, side: Side) -> &SideModel {
        match side {
            Side::Buy => &self.buy,
            Side::Sell => &self.sell,
        }
    }

    /// P(TP before SL) for a scenario at the last bar of `window`.
    pub fn estimate(&self, window: &[Candle], side: Side, tp_distance: f64, sl_distance: f64) -> Estimate {
        let none = |source: String| Estimate { probability: None, source };
        let m = self.side(side);
        if m.selected == Selected::None {
            return none(format!("modèle {} {}: {}", self.model_id, side.as_str(), m.selection_reason));
        }
        let Some(a) = aegis_market::indicators::atr(window, 14).filter(|a| *a > 0.0) else { return none("ATR indisponible".into()) };
        let (tp_atr, sl_atr) = (tp_distance / a, sl_distance / a);
        if !self.config.in_grid(tp_atr, sl_atr) {
            return none(format!("scénario TP {tp_atr:.2} ATR / SL {sl_atr:.2} ATR hors de la grille d'entraînement"));
        }
        let Some(market) = compute_features(window) else { return none("variables de marché indisponibles".into()) };
        match m.predict(&market, tp_atr, sl_atr) {
            Some(p) => {
                let r = m.report().map(|r| format!(", Brier {:.4}, ECE {:.3}, {} obs.", r.brier, r.ece, r.n)).unwrap_or_default();
                Estimate { probability: Some(p), source: format!("modèle de scénario {} ({:?}{r})", self.model_id, m.selected) }
            }
            None => none("prédiction impossible".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrainError {
    Config(String),
    NotEnoughData(usize),
}

impl std::fmt::Display for TrainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrainError::Config(m) => write!(f, "configuration invalide: {m}"),
            TrainError::NotEnoughData(n) => write!(f, "historique insuffisant: {n} bougies"),
        }
    }
}

/// Walk-forward folds by bar index: (train rows, test rows) with a purge gap.
fn folds(rows: &[Row], n_bars: usize, k: usize, purge: usize) -> Vec<(Vec<usize>, Vec<usize>)> {
    let block = n_bars / (k + 1);
    (1..=k)
        .map(|f| {
            let (lo, hi) = (f * block, if f == k { usize::MAX } else { (f + 1) * block });
            let train = rows.iter().enumerate().filter(|(_, r)| r.bar + purge < lo).map(|(i, _)| i).collect();
            let test = rows.iter().enumerate().filter(|(_, r)| r.bar >= lo && r.bar < hi).map(|(i, _)| i).collect();
            (train, test)
        })
        .collect()
}

fn fit_side(rows: &[Row], n_bars: usize, side: Side, cfg: &ProbabilityConfig) -> SideModel {
    let full_x: Vec<Vec<f64>> = rows.iter().map(Row::full).collect();
    let base_x: Vec<Vec<f64>> = rows.iter().map(Row::base).collect();
    let y: Vec<f64> = rows.iter().map(|r| r.y).collect();
    let base_rate = if y.is_empty() { 0.0 } else { y.iter().sum::<f64>() / y.len() as f64 };
    let fl = folds(rows, n_bars, cfg.folds, cfg.horizon_bars);

    // Out-of-sample logits for both models.
    let mut oos: Vec<(usize, usize, f64, f64)> = Vec::new(); // (fold, row, full logit, base logit)
    for (f, (train, test)) in fl.iter().enumerate() {
        // A fold trained on a small share of the history is not representative of the
        // final model (trained on everything): it would only add extreme, noisy forecasts.
        if train.len() < 50 || (train.len() as f64) < 0.3 * rows.len() as f64 || test.is_empty() {
            continue;
        }
        let pick = |x: &[Vec<f64>], idx: &[usize]| idx.iter().map(|i| x[*i].clone()).collect::<Vec<_>>();
        let ty: Vec<f64> = train.iter().map(|i| y[*i]).collect();
        let full = fit_logistic(&pick(&full_x, train), &ty, cfg.ridge);
        let base = fit_logistic(&pick(&base_x, train), &ty, cfg.ridge);
        for &i in test {
            if let (Some(a), Some(b)) = (full.logit(&full_x[i]), base.logit(&base_x[i])) {
                oos.push((f, i, a, b));
            }
        }
    }
    // Cross-fitted Platt calibration: each fold is calibrated on the OTHER folds only.
    let fold_ids: Vec<usize> = {
        let mut v: Vec<usize> = oos.iter().map(|o| o.0).collect();
        v.dedup();
        v
    };
    let calibrate = |which: usize| -> Vec<(f64, f64)> {
        let logit = |o: &(usize, usize, f64, f64)| if which == 0 { o.2 } else { o.3 };
        let params: Vec<(usize, (f64, f64))> = fold_ids
            .iter()
            .map(|f| {
                let others: Vec<&(usize, usize, f64, f64)> = oos.iter().filter(|o| o.0 != *f).collect();
                let p = if others.len() >= 50 {
                    logistic::fit_platt(&others.iter().map(|o| logit(o)).collect::<Vec<_>>(), &others.iter().map(|o| y[o.1]).collect::<Vec<_>>())
                } else {
                    (1.0, 0.0)
                };
                (*f, p)
            })
            .collect();
        oos.iter()
            .map(|o| {
                let (a, b) = params.iter().find(|(f, _)| *f == o.0).map(|x| x.1).unwrap_or((1.0, 0.0));
                (sigmoid(a * logit(o) + b), y[o.1])
            })
            .collect()
    };
    let full_report = reliability(&calibrate(0), &oos.iter().map(|o| o.0).collect::<Vec<_>>());
    let base_report = reliability(&calibrate(1), &oos.iter().map(|o| o.0).collect::<Vec<_>>());

    // Final models on all rows, calibrated on all out-of-sample logits.
    let mut full = fit_logistic(&full_x, &y, cfg.ridge);
    let mut baseline = fit_logistic(&base_x, &y, cfg.ridge);
    if oos.len() >= 50 {
        let oy: Vec<f64> = oos.iter().map(|o| y[o.1]).collect();
        full.platt = logistic::fit_platt(&oos.iter().map(|o| o.2).collect::<Vec<_>>(), &oy);
        baseline.platt = logistic::fit_platt(&oos.iter().map(|o| o.3).collect::<Vec<_>>(), &oy);
    }
    full.feature_names = FEATURE_NAMES.iter().chain(SCENARIO_FEATURES.iter()).map(|s| s.to_string()).collect();
    baseline.feature_names = SCENARIO_FEATURES.iter().map(|s| s.to_string()).collect();

    let (selected, reason) = if full_report.n < cfg.min_samples {
        (Selected::None, format!("{} observations hors échantillon (minimum {})", full_report.n, cfg.min_samples))
    } else if full_report.brier <= base_report.brier - cfg.min_brier_skill && full_report.ece <= cfg.max_ece {
        (Selected::Full, format!("Brier {:.4} contre {:.4} pour la référence: les variables de marché apportent une information", full_report.brier, base_report.brier))
    } else if base_report.ece <= cfg.max_ece {
        let why = if full_report.brier <= base_report.brier - cfg.min_brier_skill {
            format!("modèle complet meilleur (Brier {:.4} contre {:.4}) mais mal calibré (ECE {:.3}, maximum {:.3})", full_report.brier, base_report.brier, full_report.ece, cfg.max_ece)
        } else {
            format!("variables de marché sans apport mesurable (Brier {:.4} contre {:.4})", full_report.brier, base_report.brier)
        };
        (Selected::ScenarioOnly, format!("{why}: probabilité du scénario seul, calibrée"))
    } else {
        (Selected::None, format!("calibration insuffisante (ECE {:.3}, maximum {:.3})", base_report.ece, cfg.max_ece))
    };
    SideModel { side, full, baseline, full_report, baseline_report: base_report, selected, selection_reason: reason, base_rate }
}

/// Train both sides on `bars` (oldest first). `point` converts the candles' spread to price.
pub fn train(
    model_id: &str,
    symbol: &str,
    timeframe: &str,
    origin: DataOrigin,
    bars: &[Candle],
    point: f64,
    cfg: &ProbabilityConfig,
    now_ms: TimestampMs,
) -> Result<ProbabilityModel, TrainError> {
    cfg.validate().map_err(TrainError::Config)?;
    if !(point > 0.0) {
        return Err(TrainError::Config("point must be > 0".into()));
    }
    let needed = aegis_learning::FEATURE_MIN_BARS + cfg.horizon_bars * (cfg.folds + 2);
    if bars.len() < needed {
        return Err(TrainError::NotEnoughData(bars.len()));
    }
    let buy_rows = build_rows(bars, Side::Buy, point, cfg);
    let sell_rows = build_rows(bars, Side::Sell, point, cfg);
    Ok(ProbabilityModel {
        model_id: model_id.to_string(),
        created_ms: now_ms,
        symbol: symbol.to_string(),
        timeframe: timeframe.to_string(),
        origin,
        dataset_hash: dataset_hash(bars),
        bars: bars.len(),
        from_ts: bars[0].open_ts,
        to_ts: bars[bars.len() - 1].open_ts,
        config: cfg.clone(),
        buy: fit_side(&buy_rows, bars.len(), Side::Buy, cfg),
        sell: fit_side(&sell_rows, bars.len(), Side::Sell, cfg),
    })
}

/// Out-of-sample quality of one side of a model on a holdout period.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SideScore {
    /// Scenarios labelled on the holdout period.
    pub rows: usize,
    /// Scenarios for which the model gave a probability.
    pub predicted: usize,
    /// Brier score on the predicted scenarios (lower is better).
    pub brier: Option<f64>,
}

impl SideScore {
    pub fn coverage(&self) -> f64 {
        if self.rows == 0 { 0.0 } else { self.predicted as f64 / self.rows as f64 }
    }
}

/// Score of a model on bars it was NOT trained on (the most recent period).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HoldoutScore {
    pub from_ts: TimestampMs,
    pub to_ts: TimestampMs,
    pub bars: usize,
    pub buy: SideScore,
    pub sell: SideScore,
}

/// Score `model` on the scenarios of `grid` (labels from the triple barrier) for the
/// bars `from..` of `bars`. Earlier bars are only used as feature history.
pub fn holdout_score(model: &ProbabilityModel, grid: &ProbabilityConfig, bars: &[Candle], from: usize, point: f64) -> HoldoutScore {
    let mut out = HoldoutScore { bars: bars.len().saturating_sub(from), ..Default::default() };
    if let (Some(a), Some(b)) = (bars.get(from), bars.last()) {
        out.from_ts = a.open_ts;
        out.to_ts = b.open_ts;
    }
    let start = from.max(aegis_learning::FEATURE_MIN_BARS.max(15));
    let mut sums = [(0usize, 0usize, 0.0f64); 2]; // (rows, predicted, squared error) for buy, sell
    let mut i = start;
    while i + grid.horizon_bars < bars.len() {
        let w = &bars[(i + 1).saturating_sub(WINDOW)..=i];
        if let (Some(market), Some(a)) = (compute_features(w), aegis_market::indicators::atr(w, 14).filter(|a| *a > 0.0)) {
            let spread = bars[i].spread_points.max(0.0) * point;
            for (k, side) in [Side::Buy, Side::Sell].into_iter().enumerate() {
                let m = model.side(side);
                for &tp in &grid.tp_grid {
                    for &sl in &grid.sl_grid {
                        if let Some(y) = triple_barrier(bars, i, side, tp * a, sl * a, spread, grid.horizon_bars) {
                            sums[k].0 += 1;
                            if let Some(p) = m.predict(&market, tp, sl) {
                                let y = if y { 1.0 } else { 0.0 };
                                sums[k].1 += 1;
                                sums[k].2 += (p - y) * (p - y);
                            }
                        }
                    }
                }
            }
        }
        i += grid.stride;
    }
    let side = |(rows, predicted, se): (usize, usize, f64)| SideScore { rows, predicted, brier: (predicted > 0).then(|| se / predicted as f64) };
    out.buy = side(sums[0]);
    out.sell = side(sums[1]);
    out
}

#[cfg(test)]
mod tests;
