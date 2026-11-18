//! AEGIS SHADOW INTELLIGENCE — an independent observer and challenger lab.
//!
//! The shadow engine runs in its own task and only RECEIVES copies of what the
//! live engine sees (closed bars, decisions, closed trades). This crate has no
//! dependency on execution, protocol or core: it cannot create, sign or send an
//! order. It produces a report:
//!
//! - **Champion / Challenger**: every strategy with its live parameters
//!   (champion) and parameter variants (challengers) are paper-traded on the
//!   same live bars with the same cost assumptions. A candidate moves along
//!   BACKTEST → OUT-OF-SAMPLE → FORWARD; DEMO and VALIDATION happen in the live
//!   engine and only by an explicit user command. Nothing is ever promoted
//!   automatically.
//! - **Missed opportunities**: refused or observed signals are followed on the
//!   next bars (TP first / SL first), grouped by the check that blocked them.
//! - **Trade analysis**: premature entries (nearly stopped out) and premature
//!   exits (most of the favourable move given back) from the trade journal.
//!
//! No look-ahead: a candidate only ever sees the bars up to the one that just
//! closed; a hypothetical outcome is only resolved with bars that came after it.

mod lab;

pub use lab::{Candidate, PaperTrade};

use aegis_types::{Candle, Side, SymbolSpec, Timeframe, TimestampMs};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShadowConfig {
    pub enabled: bool,
    /// Generate parameter variants (challengers) around each champion.
    pub variants: bool,
    /// Forward trades a challenger needs before it can be called ready.
    pub forward_min_trades: u32,
    /// Commission per lot used for paper results (kept in sync with the risk settings).
    pub commission_per_lot_round_turn: f64,
    /// A hypothetical (missed) trade is abandoned after this many bars.
    pub missed_horizon_bars: usize,
}

impl Default for ShadowConfig {
    fn default() -> Self {
        ShadowConfig { enabled: true, variants: true, forward_min_trades: 30, commission_per_lot_round_turn: 7.0, missed_horizon_bars: 200 }
    }
}

/// A live decision copied to the shadow engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowDecision {
    pub decision_id: String,
    pub ts_ms: TimestampMs,
    pub symbol: String,
    pub timeframe: Timeframe,
    pub strategy_id: String,
    pub side: Side,
    pub entry: f64,
    pub sl: f64,
    pub tp: f64,
    /// `approved` | `rejected` | `observed`
    pub outcome: String,
    /// Names of the checks that failed.
    pub failed_checks: Vec<String>,
}

/// A closed trade copied from the journal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowTrade {
    pub ticket: i64,
    pub strategy_id: String,
    pub side: Side,
    pub entry: f64,
    pub exit: f64,
    pub sl_initial: f64,
    pub tp_initial: f64,
    pub mfe_price: f64,
    pub mae_price: f64,
    pub net: f64,
}

/// Out-of-sample evidence computed by the shadow engine for a candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OosSummary {
    pub symbol: String,
    pub timeframe: String,
    pub origin: String,
    pub full_trades: u32,
    pub full_net: f64,
    pub oos_trades: u32,
    pub oos_net: f64,
    pub oos_profit_factor: Option<f64>,
}

pub enum ShadowInput {
    Config(ShadowConfig),
    Spec(SymbolSpec),
    /// Champion parameters: (strategy id, configuration, enabled live).
    Champions(Vec<(String, Value, bool)>),
    /// A bar closed: the window ends with it.
    Bar { symbol: String, timeframe: Timeframe, window: Arc<Vec<Candle>> },
    Decision(ShadowDecision),
    TradeClosed(ShadowTrade),
    /// Stored history for backtest + out-of-sample evaluation of every candidate.
    History { symbol: String, timeframe: Timeframe, origin: String, bars: Arc<Vec<Candle>> },
    /// Restore a previous report after a restart.
    Restore(Box<ShadowReport>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Champion,
    Challenger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStage {
    /// Being paper-traded, no evidence yet.
    Candidate,
    /// Full backtest positive.
    Backtest,
    /// Out-of-sample backtest positive with enough trades.
    OutOfSample,
    /// Forward (paper, live bars) positive, enough trades, not worse than its champion.
    Forward,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ForwardStats {
    pub trades: u32,
    pub wins: u32,
    /// Per 1.0 lot, account currency, after commission.
    pub net: f64,
    pub gross: f64,
    pub max_drawdown: f64,
    pub expectancy: Option<f64>,
    pub profit_factor: Option<f64>,
    pub sum_r: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateView {
    pub id: String,
    pub strategy_id: String,
    pub role: Role,
    pub config: Value,
    pub live_enabled: bool,
    pub stage: PipelineStage,
    pub forward: ForwardStats,
    pub oos: Vec<OosSummary>,
    pub open_positions: usize,
    /// Why the candidate is (not) ready for promotion.
    pub verdict: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MissedByCheck {
    pub check: String,
    pub would_have_won: u32,
    pub would_have_lost: u32,
    pub undecided: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TradeAnalysis {
    pub trades: u32,
    /// Came within 20 % of the initial stop before the end.
    pub premature_entries: u32,
    /// Reached 80 % of the target, then exited keeping less than 30 % of the best move.
    pub premature_exits: u32,
    pub avg_exit_efficiency: Option<f64>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ShadowReport {
    pub started_ms: TimestampMs,
    pub updated_ms: TimestampMs,
    pub bars_processed: u64,
    pub decisions_seen: u64,
    pub candidates: Vec<CandidateView>,
    pub missed: Vec<MissedByCheck>,
    pub pending_hypotheticals: usize,
    pub trade_analysis: TradeAnalysis,
}

/// Handle to the running shadow task.
pub struct ShadowHandle {
    pub tx: mpsc::Sender<ShadowInput>,
    pub report: watch::Receiver<Arc<ShadowReport>>,
    pub task: tokio::task::JoinHandle<()>,
}

/// Start the shadow engine in its own task.
pub fn spawn(cfg: ShadowConfig, now_ms: TimestampMs) -> ShadowHandle {
    let (tx, mut rx) = mpsc::channel::<ShadowInput>(1_024);
    let (rtx, rrx) = watch::channel(Arc::new(ShadowReport { started_ms: now_ms, ..Default::default() }));
    let task = tokio::spawn(async move {
        let mut lab = lab::Lab::new(cfg, now_ms);
        while let Some(msg) = rx.recv().await {
            let heavy = matches!(msg, ShadowInput::History { .. });
            if heavy {
                // Backtests are CPU work: keep them off the async workers.
                let moved = std::mem::replace(&mut lab, lab::Lab::new(ShadowConfig::default(), now_ms));
                lab = tokio::task::spawn_blocking(move || {
                    let mut l = moved;
                    l.handle(msg);
                    l
                })
                .await
                .expect("shadow backtest task");
            } else {
                lab.handle(msg);
            }
            if rx.is_empty() || heavy {
                let _ = rtx.send(Arc::new(lab.report()));
            }
        }
    });
    ShadowHandle { tx, report: rrx, task }
}

pub(crate) type Key = (String, Timeframe);
pub(crate) type SpecMap = HashMap<String, SymbolSpec>;

#[cfg(test)]
mod tests;
