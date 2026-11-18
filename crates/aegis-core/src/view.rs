//! Read-only picture of the engine published to the desktop application and
//! the HTTP API. Every value comes from real engine state; what is unknown is
//! `None`/empty, never a placeholder number.

use aegis_data::{AuditEntry, BacktestRunRow, CommandRecord, OrderRecord};
use aegis_learning::{DriftReport, GateResult, LivePerformance, ValidationReport};
use aegis_market::MarketSnapshot;
use aegis_risk::{DayStats, KillSwitchState, MicroProfitEvaluation, RiskAssessment, RiskConfig};
use aegis_strategy::StrategyMeta;
use aegis_types::{
    AccountSnapshot, AccountTradeMode, DataOrigin, Deal, Mode, PendingOrder, Position, Side, SymbolSpec,
    TerminalKind, Timeframe, TimestampMs,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Ok,
    Degraded,
    Down,
    Inactive,
}

impl HealthStatus {
    pub fn label(self) -> &'static str {
        match self {
            HealthStatus::Ok => "OK",
            HealthStatus::Degraded => "Dégradé",
            HealthStatus::Down => "En panne",
            HealthStatus::Inactive => "Inactif",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleHealth {
    pub name: String,
    pub status: HealthStatus,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionView {
    pub terminal: TerminalKind,
    pub login: i64,
    pub server: String,
    pub trade_mode: AccountTradeMode,
    pub build: String,
    pub peer: String,
    pub connected_ms: TimestampMs,
    pub last_rx_ms: TimestampMs,
    pub frames_in: u64,
    pub frames_out: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BridgeStats {
    pub listening: String,
    pub key_source: String,
    pub connections_accepted: u64,
    pub handshakes_refused: u64,
    pub protocol_violations: u64,
    pub last_refusal: String,
    pub last_disconnect: String,
}

/// One trading decision with its full explanation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionView {
    pub decision_id: String,
    pub ts_ms: TimestampMs,
    pub symbol: String,
    pub timeframe: Timeframe,
    pub strategy_id: String,
    pub side: Side,
    pub mode: Mode,
    /// Why the strategy fired.
    pub reason: String,
    pub evidence: Vec<(String, f64)>,
    pub entry: f64,
    pub sl: f64,
    pub tp: f64,
    /// `approved` | `rejected` | `observed`
    pub outcome: String,
    /// Checks added by the engine (strategy stage, regime, data quality, AI filter...).
    pub engine_checks: Vec<aegis_risk::Check>,
    pub assessment: RiskAssessment,
    /// Order created from this decision, if any.
    pub cid: Option<String>,
}

impl DecisionView {
    pub fn micro(&self) -> Option<&MicroProfitEvaluation> {
        self.assessment.micro.as_ref()
    }
    /// All failed checks, engine-level first.
    pub fn failures(&self) -> Vec<String> {
        self.engine_checks
            .iter()
            .chain(self.assessment.checks.iter())
            .filter(|c| !c.passed)
            .map(|c| format!("{}: {}", c.name, c.detail))
            .collect()
    }
}

/// Entry Intelligence for one instrument and signal timeframe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryState {
    pub symbol: String,
    pub timeframe: Timeframe,
    pub ts_ms: TimestampMs,
    /// `BUY` | `SELL` | `WAIT` — WAIT is a normal decision.
    pub decision: String,
    pub reason: String,
    pub decision_id: Option<String>,
    /// P(TP before SL) of the display scenario, when a usable model exists.
    pub buy_probability: Option<f64>,
    pub sell_probability: Option<f64>,
    pub probability_source: String,
}

impl EntryState {
    pub fn wait(symbol: &str, tf: Timeframe) -> EntryState {
        EntryState {
            symbol: symbol.into(),
            timeframe: tf,
            ts_ms: 0,
            decision: "WAIT".into(),
            reason: "en attente de la clôture d'une bougie".into(),
            decision_id: None,
            buy_probability: None,
            sell_probability: None,
            probability_source: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbabilityModelView {
    pub model_id: String,
    pub created_ms: TimestampMs,
    pub symbol: String,
    pub timeframe: String,
    pub origin: DataOrigin,
    pub dataset_hash: String,
    pub bars: usize,
    /// False for a synthetic-data model while a real terminal is connected.
    pub usable: bool,
    pub buy_selected: aegis_probability::Selected,
    pub sell_selected: aegis_probability::Selected,
    pub buy_reason: String,
    pub sell_reason: String,
    pub buy_report: aegis_probability::metrics::CalibrationReport,
    pub sell_report: aegis_probability::metrics::CalibrationReport,
}

/// Latest Position Intelligence decision for one open position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PositionDecisionView {
    pub ts_ms: TimestampMs,
    pub ticket: i64,
    pub symbol: String,
    pub action: aegis_risk::PositionAction,
    /// The instruction left the engine (execution modes only).
    pub executed: bool,
    pub note: String,
    pub net_realizable: f64,
    pub initial_sl: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceView {
    pub strategy_id: String,
    pub symbol: String,
    pub timeframe: String,
    pub run_id: String,
    pub origin: DataOrigin,
    pub trades: u32,
    pub wins: u32,
    pub net: f64,
    /// Conservative win probability (Wilson lower bound); `None` below 30 trades.
    pub win_probability: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ForwardStats {
    pub trades: u32,
    pub wins: u32,
    pub net: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyView {
    pub meta: StrategyMeta,
    pub config: Value,
    pub stage: String,
    pub enabled: bool,
    pub evidence: Vec<EvidenceView>,
    /// Trades really executed through the connected kind of terminal.
    pub forward: ForwardStats,
    pub signals: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelView {
    pub model_id: String,
    pub path: String,
    pub origin: DataOrigin,
    pub symbol: String,
    pub timeframe: String,
    pub horizon_bars: usize,
    pub target: String,
    pub validation: ValidationReport,
    pub gate: GateResult,
    /// Latest probability, with its bar time. Shown as indicative unless the gate passed.
    pub last_prediction: Option<(TimestampMs, f64)>,
    pub performance: LivePerformance,
    pub drift: DriftReport,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestSegmentView {
    pub segment: String,
    pub bars: usize,
    pub from_ts: TimestampMs,
    pub to_ts: TimestampMs,
    pub metrics: aegis_backtest::Metrics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestDetail {
    pub run: BacktestRunRow,
    pub full: aegis_backtest::Metrics,
    pub segments: Vec<BacktestSegmentView>,
    pub walk_forward: Vec<BacktestSegmentView>,
    pub equity_curve: Vec<(TimestampMs, f64)>,
    pub trades: Vec<aegis_backtest::Trade>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogLine {
    pub ts_ms: TimestampMs,
    pub level: String,
    pub source: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataStats {
    pub db_path: String,
    pub db_ok: bool,
    pub audit_entries_verified: Option<u64>,
    pub audit_error: Option<String>,
    pub last_backup: Option<String>,
    pub candles: Vec<(String, String, String, i64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineView {
    pub now_ms: TimestampMs,
    pub started_ms: TimestampMs,
    pub version: String,
    pub mode: Mode,
    pub live_armed: bool,
    pub live_trading_enabled_in_config: bool,
    pub connection: Option<ConnectionView>,
    pub bridge: BridgeStats,
    pub account: Option<AccountSnapshot>,
    pub specs: Vec<SymbolSpec>,
    pub markets: Vec<MarketSnapshot>,
    /// Recent closes for charts: (symbol, timeframe, [(bar time, close)]).
    pub charts: Vec<(String, Timeframe, Vec<(TimestampMs, f64)>)>,
    pub signal_timeframes: Vec<Timeframe>,
    pub positions: Vec<Position>,
    pub position_decisions: Vec<PositionDecisionView>,
    /// BUY / SELL / WAIT per instrument and signal timeframe.
    pub entry_states: Vec<EntryState>,
    pub probability_models: Vec<ProbabilityModelView>,
    /// Calibration of the probabilities used for real trades (journal, 90 days).
    pub probability_live: Option<aegis_probability::metrics::CalibrationReport>,
    pub journal: Vec<aegis_data::JournalEntry>,
    /// Tickets of positions AEGIS did not open.
    pub foreign_tickets: Vec<i64>,
    pub pending: Vec<PendingOrder>,
    pub deals: Vec<Deal>,
    pub orders: Vec<OrderRecord>,
    pub commands: Vec<CommandRecord>,
    pub decisions: Vec<DecisionView>,
    pub strategies: Vec<StrategyView>,
    pub risk_config: RiskConfig,
    pub kill: KillSwitchState,
    pub day: Option<DayStats>,
    pub peak_equity: f64,
    pub drawdown_pct: Option<f64>,
    pub floating_pnl: Option<f64>,
    pub daily_net: Option<f64>,
    pub daily_net_pct: Option<f64>,
    /// Why new entries are currently blocked for the day, if they are.
    pub daily_halt: Option<String>,
    /// Daily Profit Goal Manager status.
    pub goal: Option<aegis_risk::GoalStatus>,
    pub model: Option<ModelView>,
    pub model_error: Option<String>,
    pub backtests: Vec<BacktestRunRow>,
    pub last_backtest: Option<BacktestDetail>,
    pub equity_history: Vec<(TimestampMs, f64)>,
    pub health: Vec<ModuleHealth>,
    pub logs: Vec<LogLine>,
    pub audit: Vec<AuditEntry>,
    pub data: DataStats,
}

impl EngineView {
    pub fn market(&self, symbol: &str) -> Option<&MarketSnapshot> {
        self.markets.iter().find(|m| m.symbol == symbol)
    }
    pub fn is_simulator(&self) -> bool {
        self.connection.as_ref().is_some_and(|c| c.terminal == TerminalKind::Simulator)
    }
}
