//! Transaction-cost model and net expectation (the micro-profit calculation).
//!
//! Convention: the *gross* result is the move of the mid price between entry and
//! exit. A round trip then pays the full spread, the commission, the estimated
//! slippage, the swap and any conversion fee. For a buy entered at the ask with a
//! take-profit hit on the bid this is exactly `tp - ask`, minus the other costs.

use crate::config::RiskConfig;
use aegis_types::{Side, SymbolSpec};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CostBreakdown {
    pub spread: f64,
    pub commission: f64,
    pub slippage: f64,
    /// Swap cost (positive = cost, negative = credit).
    pub swap: f64,
    pub conversion: f64,
}

impl CostBreakdown {
    pub fn total(&self) -> f64 {
        self.spread + self.commission + self.slippage + self.swap + self.conversion
    }
}

/// Costs of one round trip, in account currency.
pub fn estimate_costs(
    spec: &SymbolSpec,
    cfg: &RiskConfig,
    side: Side,
    volume: f64,
    spread_price: f64,
    gross_reference: f64,
) -> CostBreakdown {
    let swap_credit = spec.swap_per_lot(side) * volume * cfg.expected_holding_nights;
    CostBreakdown {
        spread: spec.price_distance_value(spread_price.max(0.0), volume),
        commission: cfg.commission_per_lot_round_turn * volume,
        slippage: spec.price_distance_value(cfg.expected_slippage_points * spec.point, volume),
        swap: -swap_credit,
        conversion: gross_reference.abs() * cfg.conversion_fee_pct / 100.0,
    }
}

/// Result of the micro-profit evaluation for one candidate trade.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MicroProfitEvaluation {
    pub volume: f64,
    /// Gross gain if the take-profit is reached (mid-to-mid move).
    pub gross_win: f64,
    /// Gross loss if the stop-loss is reached (mid-to-mid move, positive number).
    pub gross_loss: f64,
    pub costs: CostBreakdown,
    /// Net result if the take-profit is reached.
    pub net_if_win: f64,
    /// Net result if the stop-loss is reached (negative number).
    pub net_if_loss: f64,
    /// Win probability used, when a validated source provided one.
    pub win_probability: Option<f64>,
    pub probability_source: String,
    /// Probability-weighted gross result. `None` without a probability.
    pub expected_gross: Option<f64>,
    /// Probability-weighted net result. `None` without a probability.
    pub expected_net: Option<f64>,
    /// Win probability above which the trade has a positive net expectation.
    pub breakeven_probability: f64,
}

/// `entry` is the expected fill price (ask for a buy, bid for a sell, or the pending price).
#[allow(clippy::too_many_arguments)]
pub fn evaluate_micro_profit(
    spec: &SymbolSpec,
    cfg: &RiskConfig,
    side: Side,
    volume: f64,
    entry: f64,
    sl: f64,
    tp: f64,
    spread_price: f64,
    win_probability: Option<f64>,
    probability_source: &str,
) -> MicroProfitEvaluation {
    let half = spread_price.max(0.0) / 2.0;
    // Mid-to-mid distances: the fill-to-target distance plus/minus half a spread.
    let win_dist = (tp - entry).abs() + half * 2.0;
    let loss_dist = ((entry - sl).abs() - half * 2.0).max(0.0);
    let gross_win = spec.price_distance_value(win_dist, volume);
    let gross_loss = spec.price_distance_value(loss_dist, volume);
    let costs = estimate_costs(spec, cfg, side, volume, spread_price, gross_win);
    let net_if_win = gross_win - costs.total();
    let net_if_loss = -gross_loss - costs.total();
    let p = win_probability.filter(|p| p.is_finite() && (0.0..=1.0).contains(p));
    let expected_gross = p.map(|p| p * gross_win - (1.0 - p) * gross_loss);
    let expected_net = p.map(|p| p * net_if_win + (1.0 - p) * net_if_loss);
    let span = net_if_win - net_if_loss;
    let breakeven_probability = if span > 0.0 { (-net_if_loss / span).clamp(0.0, 1.0) } else { 1.0 };
    MicroProfitEvaluation {
        volume,
        gross_win,
        gross_loss,
        costs,
        net_if_win,
        net_if_loss,
        win_probability: p,
        probability_source: probability_source.to_string(),
        expected_gross,
        expected_net,
        breakeven_probability,
    }
}

/// Can a given NET target be reached on this trade, after every cost?
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetFeasibility {
    /// Net gain wanted if the target is reached (account currency).
    pub target_net: f64,
    /// Distance from the fill price to the take-profit that pays exactly `target_net`.
    pub tp_distance: f64,
    pub tp_points: f64,
    /// Gross (mid-to-mid) gain needed.
    pub gross_needed: f64,
    pub costs: CostBreakdown,
    pub net_if_loss: f64,
    /// Calibrated probability for that take-profit, when a model provides one.
    pub probability: Option<f64>,
    pub probability_source: String,
    pub expected_net: Option<f64>,
    pub breakeven_probability: f64,
    pub feasible: bool,
    pub reason: String,
}

/// Feasibility of each net target for a trade of `volume` with stop `sl`.
/// `probability(tp_distance)` returns a calibrated probability (and its source) or None.
#[allow(clippy::too_many_arguments)]
pub fn target_feasibility(
    spec: &SymbolSpec,
    cfg: &RiskConfig,
    side: Side,
    volume: f64,
    entry: f64,
    sl: f64,
    spread_price: f64,
    targets: &[f64],
    probability: &dyn Fn(f64) -> (Option<f64>, String),
) -> Vec<TargetFeasibility> {
    let per_price = spec.price_distance_value(1.0, volume);
    let mut out = Vec::new();
    for &target in targets {
        if !(target.is_finite() && target > 0.0 && volume > 0.0 && per_price > 0.0) {
            continue;
        }
        // Costs that do not depend on the gross, then the conversion fee proportional to it.
        let fixed = estimate_costs(spec, cfg, side, volume, spread_price, 0.0);
        let rate = cfg.conversion_fee_pct / 100.0;
        let gross = (target + fixed.total()) / (1.0 - rate).max(1e-9);
        let win_dist = gross / per_price;
        let tp_distance = (win_dist - spread_price.max(0.0)).max(0.0);
        let tp = entry + side.sign() * tp_distance;
        let ev = evaluate_micro_profit(spec, cfg, side, volume, entry, sl, tp, spread_price, None, "");
        let (p, src) = probability(tp_distance);
        let expected = p.map(|p| p * ev.net_if_win + (1.0 - p) * ev.net_if_loss);
        let min_dist = spec.stops_level_points * spec.point;
        let (feasible, reason) = if tp_distance < min_dist || tp_distance <= 0.0 {
            (false, format!("cible à {:.1} points: sous la distance minimale du broker ({:.0} points)", tp_distance / spec.point, spec.stops_level_points))
        } else if p.is_none() {
            (false, format!("aucune probabilité calibrée pour une cible à {:.1} points ({src})", tp_distance / spec.point))
        } else if expected.is_some_and(|e| e < cfg.min_net_expected_profit || e <= 0.0) {
            (false, format!("espérance nette {:.2} insuffisante (probabilité nécessaire {:.0} %)", expected.unwrap_or(0.0), ev.breakeven_probability * 100.0))
        } else {
            (true, format!("espérance nette {:.2} après {:.2} de coûts", expected.unwrap_or(0.0), ev.costs.total()))
        };
        out.push(TargetFeasibility {
            target_net: target,
            tp_distance,
            tp_points: tp_distance / spec.point,
            gross_needed: ev.gross_win,
            costs: ev.costs,
            net_if_loss: ev.net_if_loss,
            probability: p,
            probability_source: src,
            expected_net: expected,
            breakeven_probability: ev.breakeven_probability,
            feasible,
            reason,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::eurusd;

    #[test]
    fn buy_at_ask_with_tp_on_bid_matches_first_principles() {
        let spec = eurusd();
        let cfg = RiskConfig { expected_slippage_points: 0.0, commission_per_lot_round_turn: 0.0, ..Default::default() };
        // bid 1.10000 / ask 1.10010, buy 1 lot, TP 1.10110 (100 points above ask), SL 1.09910.
        let e = evaluate_micro_profit(&spec, &cfg, Side::Buy, 1.0, 1.10010, 1.09910, 1.10110, 0.00010, Some(0.5), "test");
        // Cash result if TP hit: (1.10110 - 1.10010) = 100 points = $100.
        assert!((e.net_if_win - 100.0).abs() < 1e-6, "{e:?}");
        // Cash result if SL hit: -(1.10010 - 1.09910) = -$100.
        assert!((e.net_if_loss + 100.0).abs() < 1e-6, "{e:?}");
        assert!((e.costs.spread - 10.0).abs() < 1e-6);
        assert!((e.expected_net.unwrap()).abs() < 1e-6);
        assert!((e.breakeven_probability - 0.5).abs() < 1e-9);
    }

    #[test]
    fn small_gross_profit_is_eaten_by_costs() {
        let spec = eurusd();
        let cfg = RiskConfig::default(); // $7/lot commission, 2 points slippage
        // TP only 5 points above the ask, 12-point spread.
        let e = evaluate_micro_profit(&spec, &cfg, Side::Buy, 1.0, 1.10012, 1.09912, 1.10017, 0.00012, Some(0.9), "test");
        assert!(e.gross_win > 0.0);
        assert!(e.net_if_win < 0.0, "5 points cannot pay $7 commission + slippage: {e:?}");
        assert!(e.expected_net.unwrap() < 0.0);
        assert_eq!(e.breakeven_probability, 1.0);
    }

    #[test]
    fn swap_and_conversion_are_charged() {
        let spec = eurusd(); // swap_long = -6.5 per lot per night
        let cfg = RiskConfig {
            expected_holding_nights: 2.0,
            conversion_fee_pct: 1.0,
            commission_per_lot_round_turn: 0.0,
            expected_slippage_points: 0.0,
            ..Default::default()
        };
        let c = estimate_costs(&spec, &cfg, Side::Buy, 0.5, 0.0, 200.0);
        assert!((c.swap - 6.5).abs() < 1e-9, "0.5 lot x 2 nights x 6.5");
        assert!((c.conversion - 2.0).abs() < 1e-9);
        // A positive swap is a credit (negative cost).
        let c = estimate_costs(&spec, &cfg, Side::Sell, 0.5, 0.0, 0.0);
        assert!(c.swap < 0.0);
    }

    #[test]
    fn missing_or_invalid_probability_yields_no_expectation() {
        let spec = eurusd();
        let cfg = RiskConfig::default();
        for p in [None, Some(f64::NAN), Some(1.2), Some(-0.1)] {
            let e = evaluate_micro_profit(&spec, &cfg, Side::Sell, 0.1, 1.1, 1.102, 1.097, 0.0001, p, "none");
            assert!(e.expected_net.is_none());
            assert!(e.win_probability.is_none());
        }
    }

    #[test]
    fn each_net_target_is_solved_after_every_cost() {
        let spec = eurusd();
        let cfg = RiskConfig { commission_per_lot_round_turn: 7.0, expected_slippage_points: 2.0, min_net_expected_profit: 0.01, ..Default::default() };
        let prob = |d: f64| if d < 0.0005 { (Some(0.8), "test".to_string()) } else { (None, "hors grille".to_string()) };
        let rows = target_feasibility(&spec, &cfg, Side::Buy, 0.01, 1.10010, 1.09810, 0.00010, &[0.05, 0.10, 0.50, 5.0], &prob);
        assert_eq!(rows.len(), 4);
        for r in &rows {
            // The take-profit found pays exactly the target after costs.
            let ev = evaluate_micro_profit(&spec, &cfg, Side::Buy, 0.01, 1.10010, 1.09810, 1.10010 + r.tp_distance, 0.00010, None, "");
            if r.tp_distance > 0.0 {
                assert!((ev.net_if_win - r.target_net).abs() < 1e-6, "{} vs {}", ev.net_if_win, r.target_net);
            }
        }
        // 0.05 net on 0.01 lot needs a tiny move: below the broker's stops level (10 points).
        // 0.05 net on 0.01 lot with a 200-point stop: the target is reachable (14 points after
        // 0.19 of costs) but breaking even would need ~98 % of wins: refused, with the reason.
        assert!(!rows[0].feasible && rows[0].reason.contains("insuffisante"), "{}", rows[0].reason);
        assert!(rows[0].breakeven_probability > 0.95);
        // With a broker stops level of 30 points, the same target is simply too close.
        let wide = SymbolSpec { stops_level_points: 30.0, ..eurusd() };
        let r = target_feasibility(&wide, &cfg, Side::Buy, 0.01, 1.10010, 1.09810, 0.00010, &[0.05], &prob);
        assert!(!r[0].feasible && r[0].reason.contains("distance minimale"), "{}", r[0].reason);
        // A far target has no calibrated probability: not feasible, and said so.
        assert!(!rows[3].feasible && rows[3].probability.is_none());
        assert!(rows.iter().all(|r| r.costs.commission > 0.0));
    }
}
