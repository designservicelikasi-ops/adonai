//! Parallel parameter research with an honest protocol.
//!
//! - The history is split chronologically: the first `in_sample_frac` (default 70 %) is used to
//!   RANK parameter sets; the rest (holdout) is only used to REPORT how the in-sample choices
//!   behave out of sample. The holdout never influences the ranking.
//! - In-sample, each set is evaluated by walk-forward folds: positive folds and the spread of
//!   fold results measure stability.
//! - Overfitting is flagged when a set that is profitable in-sample loses (or keeps less than
//!   half its expectancy) on the holdout. A set whose grid neighbours lose in-sample is flagged
//!   unstable (an isolated optimum is usually noise).
//! - Work is spread over a bounded number of threads (never all cores).

use crate::{compute_metrics, run, walk_forward, BacktestParams, Metrics};
use aegis_strategy::Strategy;
use aegis_types::{Candle, SymbolSpec};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepRequest {
    pub strategy_id: String,
    /// Configuration the grid values are applied on.
    pub base_config: Value,
    /// Parameter name -> values to try.
    pub grid: BTreeMap<String, Vec<Value>>,
    pub folds: usize,
    pub in_sample_frac: f64,
    pub max_threads: usize,
    pub max_combinations: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepPoint {
    pub config: Value,
    pub in_sample: Metrics,
    pub fold_nets: Vec<f64>,
    pub positive_folds: usize,
    pub fold_spread: f64,
    pub holdout: Metrics,
    pub overfit: bool,
    pub unstable: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepReport {
    pub strategy_id: String,
    pub combinations: usize,
    pub evaluated: usize,
    pub threads: usize,
    pub in_sample_bars: usize,
    pub holdout_bars: usize,
    pub elapsed_ms: u128,
    /// Ranked by in-sample robustness (never by holdout).
    pub points: Vec<SweepPoint>,
    pub warnings: Vec<String>,
}

fn combos(grid: &BTreeMap<String, Vec<Value>>) -> Vec<Vec<(String, Value)>> {
    let mut out: Vec<Vec<(String, Value)>> = vec![Vec::new()];
    for (k, values) in grid {
        let mut next = Vec::new();
        for prefix in &out {
            for v in values {
                let mut c = prefix.clone();
                c.push((k.clone(), v.clone()));
                next.push(c);
            }
        }
        out = next;
    }
    out
}

fn apply(base: &Value, c: &[(String, Value)]) -> Value {
    let mut v = if base.is_object() { base.clone() } else { Value::Object(Default::default()) };
    for (k, x) in c {
        v[k] = x.clone();
    }
    v
}

/// Robustness score used for the ranking: in-sample net, discounted by instability.
fn robustness(p: &SweepPoint, folds: usize) -> f64 {
    let share = if folds > 0 { p.positive_folds as f64 / folds as f64 } else { 0.0 };
    p.in_sample.net_profit * share - p.fold_spread * 0.5
}

pub fn sweep(
    req: &SweepRequest,
    spec: &SymbolSpec,
    bars: &[Candle],
    params: &BacktestParams,
    build: &(dyn Fn(&str, &Value) -> Result<Box<dyn Strategy>, String> + Sync),
) -> Result<SweepReport, String> {
    let started = std::time::Instant::now();
    if !(0.5..=0.9).contains(&req.in_sample_frac) || req.folds < 2 || req.folds > 20 {
        return Err("in_sample_frac within 0.5..0.9 and folds within 2..20".into());
    }
    let all = combos(&req.grid);
    if all.is_empty() || all.len() > req.max_combinations.max(1) {
        return Err(format!("{} combinaisons (maximum {})", all.len(), req.max_combinations));
    }
    let cut = (bars.len() as f64 * req.in_sample_frac) as usize;
    let (is_bars, ho_bars) = bars.split_at(cut);
    if is_bars.len() < req.folds * 60 || ho_bars.len() < 60 {
        return Err(format!("historique insuffisant: {} bougies (au moins {} en apprentissage et 60 en contrôle)", bars.len(), req.folds * 60));
    }
    let available = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    let threads = req.max_threads.clamp(1, available.saturating_sub(1).max(1)).min(all.len());
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, Result<SweepPoint, String>)>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= all.len() {
                    break;
                }
                let config = apply(&req.base_config, &all[i]);
                let r = build(&req.strategy_id, &config).map(|strategy| {
                    let folds = walk_forward(strategy.as_ref(), spec, is_bars, params, req.folds);
                    let fold_nets: Vec<f64> = folds.iter().map(|f| f.result.metrics.net_profit).collect();
                    let trades: Vec<_> = folds.iter().flat_map(|f| f.result.trades.clone()).collect();
                    let in_sample = compute_metrics(&trades, &[], params.initial_equity, folds.iter().map(|f| f.result.metrics.skipped_signals).sum());
                    let mean = fold_nets.iter().sum::<f64>() / fold_nets.len().max(1) as f64;
                    let spread = (fold_nets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / fold_nets.len().max(1) as f64).sqrt();
                    let holdout = run(strategy.as_ref(), spec, ho_bars, params).metrics;
                    let mut notes = Vec::new();
                    let overfit = in_sample.net_profit > 0.0 && (holdout.net_profit <= 0.0 || (in_sample.expectancy > 0.0 && holdout.expectancy < 0.5 * in_sample.expectancy));
                    if overfit {
                        notes.push(format!("sur-apprentissage probable: espérance {:.2} en apprentissage, {:.2} en contrôle", in_sample.expectancy, holdout.expectancy));
                    }
                    if in_sample.trades < 30 {
                        notes.push(format!("{} transactions en apprentissage: trop peu pour conclure", in_sample.trades));
                    }
                    SweepPoint { config, in_sample, positive_folds: fold_nets.iter().filter(|x| **x > 0.0).count(), fold_spread: spread, fold_nets, holdout, overfit, unstable: false, notes }
                });
                results.lock().unwrap().push((i, r));
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(i, _)| *i);
    let mut warnings = Vec::new();
    let mut points: Vec<(usize, SweepPoint)> = Vec::new();
    for (i, r) in results {
        match r {
            Ok(p) => points.push((i, p)),
            Err(e) => warnings.push(format!("combinaison {i} invalide: {e}")),
        }
    }
    // Instability: grid neighbours (one parameter one step away) losing in-sample.
    let index: BTreeMap<usize, usize> = points.iter().enumerate().map(|(pos, (i, _))| (*i, pos)).collect();
    let keys: Vec<&String> = req.grid.keys().collect();
    let sizes: Vec<usize> = req.grid.values().map(|v| v.len()).collect();
    let nets: Vec<f64> = points.iter().map(|(_, p)| p.in_sample.net_profit).collect();
    for (pos, (i, p)) in points.iter_mut().enumerate() {
        if nets[pos] <= 0.0 {
            continue;
        }
        // Decode the mixed-radix index (the last key varies fastest).
        let mut coords = vec![0usize; sizes.len()];
        let mut rest = *i;
        for d in (0..sizes.len()).rev() {
            coords[d] = rest % sizes[d];
            rest /= sizes[d];
        }
        let mut neighbours = Vec::new();
        for d in 0..sizes.len() {
            for delta in [-1i64, 1] {
                let c = coords[d] as i64 + delta;
                if c < 0 || c as usize >= sizes[d] {
                    continue;
                }
                let mut nc = coords.clone();
                nc[d] = c as usize;
                let mut idx = 0;
                for (k, v) in nc.iter().enumerate() {
                    idx = idx * sizes[k] + v;
                }
                if let Some(&npos) = index.get(&idx) {
                    neighbours.push((keys[d].clone(), nets[npos]));
                }
            }
        }
        let losing: Vec<String> = neighbours.iter().filter(|(_, n)| *n <= 0.0).map(|(k, _)| k.clone()).collect();
        if !neighbours.is_empty() && losing.len() * 2 >= neighbours.len() {
            p.unstable = true;
            p.notes.push(format!("paramètres instables: les voisins ({}) perdent en apprentissage", losing.join(", ")));
        }
    }
    let mut ranked: Vec<SweepPoint> = points.into_iter().map(|(_, p)| p).collect();
    ranked.sort_by(|a, b| robustness(b, req.folds).total_cmp(&robustness(a, req.folds)));
    if ranked.first().is_some_and(|b| b.overfit || b.unstable) {
        warnings.push("le meilleur jeu en apprentissage est instable ou sur-appris: aucune recommandation".into());
    }
    Ok(SweepReport {
        strategy_id: req.strategy_id.clone(),
        combinations: all.len(),
        evaluated: ranked.len(),
        threads,
        in_sample_bars: is_bars.len(),
        holdout_bars: ho_bars.len(),
        elapsed_ms: started.elapsed().as_millis(),
        points: ranked,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthetic::{generate, SyntheticParams};
    use aegis_types::{SymbolTradeMode, Timeframe};

    fn spec() -> SymbolSpec {
        SymbolSpec {
            symbol: "EURUSD.sim".into(), digits: 5, point: 0.00001, contract_size: 100_000.0, tick_size: 0.00001, tick_value: 1.0,
            volume_min: 0.01, volume_max: 50.0, volume_step: 0.01, stops_level_points: 0.0, swap_long: 0.0, swap_short: 0.0,
            trade_mode: SymbolTradeMode::Full, currency_profit: "USD".into(), currency_base: "EUR".into(), freeze_level_points: 0.0,
        }
    }

    #[test]
    fn grid_is_evaluated_in_parallel_ranked_in_sample_and_reported_out_of_sample() {
        let bars = generate(&SyntheticParams { seed: 11, bars: 4_000, timeframe: Timeframe::M5, start_ts: 1_700_000_000_000, ..Default::default() });
        let mut grid = BTreeMap::new();
        grid.insert("period".to_string(), vec![serde_json::json!(10), serde_json::json!(20), serde_json::json!(40)]);
        grid.insert("sl_atr".to_string(), vec![serde_json::json!(1.0), serde_json::json!(2.0)]);
        let base = aegis_strategy::build("donchian_breakout", &Value::Null).unwrap().config();
        let req = SweepRequest { strategy_id: "donchian_breakout".into(), base_config: base, grid, folds: 4, in_sample_frac: 0.7, max_threads: 4, max_combinations: 50 };
        let build = |id: &str, c: &Value| aegis_strategy::build(id, c).map_err(|e| e.to_string());
        let r = sweep(&req, &spec(), &bars, &BacktestParams::default(), &build).unwrap();
        assert_eq!((r.combinations, r.evaluated), (6, 6), "{:?}", r.warnings);
        assert!(r.threads >= 1 && r.threads <= 4);
        assert_eq!(r.in_sample_bars + r.holdout_bars, 4_000);
        for p in &r.points {
            assert_eq!(p.fold_nets.len(), 4);
            // Overfit flag follows its definition.
            let expected = p.in_sample.net_profit > 0.0 && (p.holdout.net_profit <= 0.0 || (p.in_sample.expectancy > 0.0 && p.holdout.expectancy < 0.5 * p.in_sample.expectancy));
            assert_eq!(p.overfit, expected);
        }
        // Deterministic: same request, same ranking.
        let again = sweep(&req, &spec(), &bars, &BacktestParams::default(), &build).unwrap();
        assert_eq!(r.points.iter().map(|p| p.config.clone()).collect::<Vec<_>>(), again.points.iter().map(|p| p.config.clone()).collect::<Vec<_>>());
    }

    #[test]
    fn requests_are_bounded() {
        let bars = generate(&SyntheticParams { seed: 1, bars: 200, timeframe: Timeframe::M5, start_ts: 0, ..Default::default() });
        let build = |id: &str, c: &Value| aegis_strategy::build(id, c).map_err(|e| e.to_string());
        let mut grid = BTreeMap::new();
        grid.insert("period".to_string(), (0..30).map(|i| serde_json::json!(i + 5)).collect());
        let req = SweepRequest { strategy_id: "donchian_breakout".into(), base_config: Value::Null, grid: grid.clone(), folds: 4, in_sample_frac: 0.7, max_threads: 2, max_combinations: 10 };
        assert!(sweep(&req, &spec(), &bars, &BacktestParams::default(), &build).is_err(), "too many combinations");
        let req = SweepRequest { max_combinations: 100, ..req };
        assert!(sweep(&req, &spec(), &bars, &BacktestParams::default(), &build).unwrap_err().contains("insuffisant"));
    }
}
