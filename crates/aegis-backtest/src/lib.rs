//! AEGIS BACKTEST — bar-based backtesting with realistic execution assumptions.
//!
//! Execution model (documented in docs/BACKTESTING.md):
//! * A signal computed on the close of bar `i` is filled at the OPEN of bar `i+1`.
//! * Candles are bid prices. Buys fill at bid + spread, sells at bid; exits the
//!   other way round. Half of the configured slippage is charged on each fill.
//! * Stops/targets are checked against each bar's high/low. If both could have
//!   been hit inside one bar, the STOP is assumed first (pessimistic).
//! * A bar that opens beyond the stop fills at the open (gap), not at the stop.
//! * Position size comes from the same fixed-fractional rule as the live risk engine.
//!
//! A backtest describes the past under these assumptions. It is never a
//! prediction or a guarantee of future results.

pub mod csv;
pub mod synthetic;

use aegis_strategy::{window, OpenView, Strategy, StrategyAction};
use aegis_types::{utc_day, Candle, Side, SymbolSpec, Timeframe, TimestampMs};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BacktestParams {
    pub initial_equity: f64,
    pub risk_per_trade_pct: f64,
    /// Fixed spread in points. `None` uses each candle's recorded spread.
    pub spread_points: Option<f64>,
    pub commission_per_lot_round_turn: f64,
    /// Total slippage per round trip, in points.
    pub slippage_points: f64,
    pub apply_swaps: bool,
}

impl Default for BacktestParams {
    fn default() -> Self {
        BacktestParams {
            initial_equity: 10_000.0,
            risk_per_trade_pct: 0.5,
            spread_points: None,
            commission_per_lot_round_turn: 7.0,
            slippage_points: 2.0,
            apply_swaps: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    StopLoss,
    TakeProfit,
    Signal,
    EndOfData,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trade {
    pub entry_ts: TimestampMs,
    pub exit_ts: TimestampMs,
    pub side: Side,
    pub volume: f64,
    pub entry: f64,
    pub exit: f64,
    pub sl: f64,
    pub tp: f64,
    /// Price P&L from fill to fill (spread and slippage are inside the fills).
    pub gross: f64,
    pub commission: f64,
    pub swap: f64,
    pub net: f64,
    pub exit_reason: ExitReason,
    pub bars_held: usize,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Metrics {
    pub trades: u32,
    pub wins: u32,
    pub net_profit: f64,
    pub gross_profit: f64,
    pub gross_loss: f64,
    pub win_rate: f64,
    /// `None` when there is no losing trade (the ratio is undefined).
    pub profit_factor: Option<f64>,
    pub max_drawdown: f64,
    pub max_drawdown_pct: f64,
    pub avg_trade: f64,
    /// Mathematical expectancy per trade: win_rate x avg_win - loss_rate x avg_loss.
    pub expectancy: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
    pub avg_bars_held: f64,
    pub avg_duration_ms: f64,
    pub total_commission: f64,
    pub total_swap: f64,
    pub final_equity: f64,
    /// Signals skipped because the risk budget could not buy the minimum volume.
    pub skipped_signals: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestResult {
    pub trades: Vec<Trade>,
    /// Equity after each closed trade: (exit timestamp, equity).
    pub equity_curve: Vec<(TimestampMs, f64)>,
    pub metrics: Metrics,
}

struct Open {
    side: Side,
    volume: f64,
    entry: f64,
    sl: f64,
    tp: f64,
    entry_ts: TimestampMs,
    entry_index: usize,
    reason: String,
}

fn spread_of(bar: &Candle, spec: &SymbolSpec, p: &BacktestParams) -> f64 {
    p.spread_points.unwrap_or(bar.spread_points).max(0.0) * spec.point
}

/// Run one strategy over one instrument. Deterministic: same inputs, same output.
pub fn run(strategy: &dyn Strategy, spec: &SymbolSpec, bars: &[Candle], params: &BacktestParams) -> BacktestResult {
    let half_slip = params.slippage_points * spec.point / 2.0;
    let mut equity = params.initial_equity;
    let mut trades: Vec<Trade> = Vec::new();
    let mut curve = vec![(bars.first().map(|b| b.open_ts).unwrap_or(0), equity)];
    let mut open: Option<Open> = None;
    let mut skipped = 0u32;
    // Entry/exit decided at the close of bar i, applied at the open of bar i+1.
    let mut pending_entry: Option<aegis_strategy::SignalIntent> = None;
    let mut pending_exit = false;

    let close_trade = |o: Open, exit: f64, exit_ts: TimestampMs, index: usize, why: ExitReason, equity: &mut f64, trades: &mut Vec<Trade>, curve: &mut Vec<(TimestampMs, f64)>| {
        let gross = spec.price_distance_value(exit - o.entry, o.volume) * if (exit - o.entry) * o.side.sign() >= 0.0 { 1.0 } else { -1.0 };
        let commission = -params.commission_per_lot_round_turn * o.volume;
        let nights = (utc_day(exit_ts) - utc_day(o.entry_ts)).max(0) as f64;
        let swap = if params.apply_swaps { spec.swap_per_lot(o.side) * o.volume * nights } else { 0.0 };
        let net = gross + commission + swap;
        *equity += net;
        curve.push((exit_ts, *equity));
        trades.push(Trade {
            entry_ts: o.entry_ts,
            exit_ts,
            side: o.side,
            volume: o.volume,
            entry: o.entry,
            exit,
            sl: o.sl,
            tp: o.tp,
            gross,
            commission,
            swap,
            net,
            exit_reason: why,
            bars_held: index - o.entry_index,
            reason: o.reason,
        });
    };

    for i in 0..bars.len() {
        let bar = &bars[i];
        let spread = spread_of(bar, spec, params);

        // 1. Apply what was decided on the previous close, at this bar's open.
        if pending_exit {
            pending_exit = false;
            if let Some(o) = open.take() {
                let exit = match o.side {
                    Side::Buy => bar.open - half_slip,
                    Side::Sell => bar.open + spread + half_slip,
                };
                close_trade(o, exit, bar.open_ts, i, ExitReason::Signal, &mut equity, &mut trades, &mut curve);
            }
        }
        if let Some(sig) = pending_entry.take() {
            if open.is_none() && equity > 0.0 {
                let entry = match sig.side {
                    Side::Buy => bar.open + spread + half_slip,
                    Side::Sell => bar.open - half_slip,
                };
                let sl = entry - sig.side.sign() * sig.sl_distance;
                let tp = entry + sig.side.sign() * sig.tp_distance;
                let per_lot_loss = spec.price_distance_value(sig.sl_distance, 1.0)
                    + params.commission_per_lot_round_turn
                    + spec.price_distance_value(half_slip, 1.0);
                let volume = spec.normalize_volume(equity * params.risk_per_trade_pct / 100.0 / per_lot_loss);
                if volume > 0.0 && sl > 0.0 {
                    open = Some(Open { side: sig.side, volume, entry, sl, tp, entry_ts: bar.open_ts, entry_index: i, reason: sig.reason });
                } else {
                    skipped += 1;
                }
            }
        }

        // 2. Intrabar stop / target, pessimistic ordering.
        if let Some(o) = &open {
            let hit = match o.side {
                Side::Buy => {
                    if bar.open <= o.sl {
                        Some((bar.open - half_slip, ExitReason::StopLoss))
                    } else if bar.low <= o.sl {
                        Some((o.sl - half_slip, ExitReason::StopLoss))
                    } else if bar.high >= o.tp {
                        Some((o.tp, ExitReason::TakeProfit))
                    } else {
                        None
                    }
                }
                Side::Sell => {
                    if bar.open + spread >= o.sl {
                        Some((bar.open + spread + half_slip, ExitReason::StopLoss))
                    } else if bar.high + spread >= o.sl {
                        Some((o.sl + half_slip, ExitReason::StopLoss))
                    } else if bar.low + spread <= o.tp {
                        Some((o.tp, ExitReason::TakeProfit))
                    } else {
                        None
                    }
                }
            };
            if let Some((exit, why)) = hit {
                let o = open.take().expect("position present");
                close_trade(o, exit, bar.open_ts, i, why, &mut equity, &mut trades, &mut curve);
            }
        }

        // 3. Decide on the close of this bar (only past and current bars are visible).
        if i + 1 < bars.len() {
            let view = open.as_ref().map(|o| OpenView { side: o.side, entry_price: o.entry, bars_held: i - o.entry_index });
            match strategy.evaluate(window(&bars[..=i]), view.as_ref()) {
                StrategyAction::Enter(sig) if open.is_none() => pending_entry = Some(sig),
                StrategyAction::Exit { .. } if open.is_some() => pending_exit = true,
                _ => {}
            }
        }
    }

    // 4. Whatever is still open is closed at the last close.
    if let (Some(o), Some(last)) = (open.take(), bars.last()) {
        let spread = spread_of(last, spec, params);
        let exit = match o.side {
            Side::Buy => last.close - half_slip,
            Side::Sell => last.close + spread + half_slip,
        };
        close_trade(o, exit, last.open_ts, bars.len() - 1, ExitReason::EndOfData, &mut equity, &mut trades, &mut curve);
    }

    let metrics = compute_metrics(&trades, &curve, params.initial_equity, skipped);
    BacktestResult { trades, equity_curve: curve, metrics }
}

pub fn compute_metrics(trades: &[Trade], curve: &[(TimestampMs, f64)], initial_equity: f64, skipped: u32) -> Metrics {
    let n = trades.len() as f64;
    let wins: Vec<&Trade> = trades.iter().filter(|t| t.net > 0.0).collect();
    let losses: Vec<&Trade> = trades.iter().filter(|t| t.net <= 0.0).collect();
    let gross_profit: f64 = wins.iter().map(|t| t.net).sum();
    let gross_loss: f64 = -losses.iter().map(|t| t.net).sum::<f64>();
    let net = gross_profit - gross_loss;
    let (mut peak, mut max_dd, mut max_dd_pct) = (initial_equity, 0.0f64, 0.0f64);
    for (_, eq) in curve {
        peak = peak.max(*eq);
        let dd = peak - eq;
        if dd > max_dd {
            max_dd = dd;
        }
        if peak > 0.0 {
            max_dd_pct = max_dd_pct.max(dd / peak * 100.0);
        }
    }
    let avg = |v: &[&Trade]| if v.is_empty() { 0.0 } else { v.iter().map(|t| t.net).sum::<f64>() / v.len() as f64 };
    let win_rate = if n > 0.0 { wins.len() as f64 / n } else { 0.0 };
    let avg_win = avg(&wins);
    let avg_loss = -avg(&losses);
    Metrics {
        trades: trades.len() as u32,
        wins: wins.len() as u32,
        net_profit: net,
        gross_profit,
        gross_loss,
        win_rate,
        profit_factor: if gross_loss > 0.0 { Some(gross_profit / gross_loss) } else { None },
        max_drawdown: max_dd,
        max_drawdown_pct: max_dd_pct,
        avg_trade: if n > 0.0 { net / n } else { 0.0 },
        expectancy: win_rate * avg_win - (1.0 - win_rate) * avg_loss,
        avg_win,
        avg_loss,
        avg_bars_held: if n > 0.0 { trades.iter().map(|t| t.bars_held as f64).sum::<f64>() / n } else { 0.0 },
        avg_duration_ms: if n > 0.0 { trades.iter().map(|t| (t.exit_ts - t.entry_ts) as f64).sum::<f64>() / n } else { 0.0 },
        total_commission: trades.iter().map(|t| t.commission).sum(),
        total_swap: trades.iter().map(|t| t.swap).sum(),
        final_equity: initial_equity + net,
        skipped_signals: skipped,
    }
}

// ------------------------------------------------------------ data splitting

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentResult {
    /// `train` | `validation` | `oos` | `fold-N`
    pub segment: String,
    pub from_ts: TimestampMs,
    pub to_ts: TimestampMs,
    pub bars: usize,
    pub result: BacktestResult,
}

/// Chronological split: first `train_frac`, then `validation_frac`, the rest is out-of-sample.
/// The segments never overlap, so out-of-sample bars are never used earlier.
pub fn split<'a>(bars: &'a [Candle], train_frac: f64, validation_frac: f64) -> Option<[(&'static str, &'a [Candle]); 3]> {
    if !(train_frac > 0.0 && validation_frac > 0.0 && train_frac + validation_frac < 1.0) || bars.len() < 30 {
        return None;
    }
    let a = (bars.len() as f64 * train_frac) as usize;
    let b = (bars.len() as f64 * (train_frac + validation_frac)) as usize;
    Some([("train", &bars[..a]), ("validation", &bars[a..b]), ("oos", &bars[b..])])
}

fn segment(name: &str, strategy: &dyn Strategy, spec: &SymbolSpec, bars: &[Candle], params: &BacktestParams) -> SegmentResult {
    SegmentResult {
        segment: name.to_string(),
        from_ts: bars.first().map(|b| b.open_ts).unwrap_or(0),
        to_ts: bars.last().map(|b| b.open_ts).unwrap_or(0),
        bars: bars.len(),
        result: run(strategy, spec, bars, params),
    }
}

/// Backtest each chronological segment independently (60 % / 20 % / 20 % by default).
pub fn run_segmented(strategy: &dyn Strategy, spec: &SymbolSpec, bars: &[Candle], params: &BacktestParams, train_frac: f64, validation_frac: f64) -> Vec<SegmentResult> {
    match split(bars, train_frac, validation_frac) {
        Some(parts) => parts.iter().map(|(name, b)| segment(name, strategy, spec, b, params)).collect(),
        None => Vec::new(),
    }
}

/// Walk-forward evaluation: `folds` consecutive, non-overlapping test windows.
/// Strategy parameters are fixed, so each fold is a pure out-of-sample test.
pub fn walk_forward(strategy: &dyn Strategy, spec: &SymbolSpec, bars: &[Candle], params: &BacktestParams, folds: usize) -> Vec<SegmentResult> {
    if folds == 0 || bars.len() < folds * 30 {
        return Vec::new();
    }
    let size = bars.len() / folds;
    (0..folds)
        .map(|k| {
            let end = if k + 1 == folds { bars.len() } else { (k + 1) * size };
            segment(&format!("fold-{}", k + 1), strategy, spec, &bars[k * size..end], params)
        })
        .collect()
}

pub fn timeframe_label(tf: Timeframe) -> &'static str {
    tf.as_str()
}
