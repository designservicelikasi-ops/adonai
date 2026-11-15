use aegis_types::{DataOrigin, OrderKind, Side, TimestampMs};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub ts_ms: TimestampMs,
    pub actor: String,
    pub event: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyRow {
    pub strategy_id: String,
    pub name: String,
    /// `research` | `demo` | `live_approved` | `disabled`
    pub stage: String,
    pub enabled: bool,
    pub config: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub decision_id: String,
    pub ts_ms: TimestampMs,
    pub symbol: String,
    pub strategy_id: String,
    pub side: Side,
    pub mode: String,
    /// Full signal with its explanation.
    pub signal: Value,
    /// `pending` | `approved` | `rejected` | `observed`
    pub outcome: String,
    pub verdict: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderRecord {
    pub cid: String,
    pub decision_id: String,
    pub auth_id: String,
    pub symbol: String,
    pub side: Side,
    pub kind: OrderKind,
    pub volume: f64,
    pub price: f64,
    pub sl: f64,
    pub tp: f64,
    pub status: String,
    pub ticket: Option<i64>,
    pub fill_price: Option<f64>,
    pub fill_volume: Option<f64>,
    pub retcode: Option<i64>,
    pub message: Option<String>,
    pub created_ms: TimestampMs,
    pub updated_ms: TimestampMs,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OrderUpdate {
    pub cid: String,
    pub status: String,
    pub ticket: Option<i64>,
    pub fill_price: Option<f64>,
    pub fill_volume: Option<f64>,
    pub retcode: Option<i64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRow {
    pub model_id: String,
    pub created_ms: TimestampMs,
    /// `candidate` | `rejected` | `validated` | `retired`
    pub status: String,
    pub origin: DataOrigin,
    pub path: String,
    pub metrics: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestRunRow {
    pub run_id: String,
    pub ts_ms: TimestampMs,
    pub strategy_id: String,
    pub symbol: String,
    pub timeframe: String,
    pub origin: DataOrigin,
    pub params: Value,
    pub metrics: Value,
}

/// A risk-reducing management command (close / modify / cancel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandRecord {
    pub cid: String,
    /// `close` | `modify` | `cancel`
    pub action: String,
    pub ticket: i64,
    pub symbol: String,
    pub volume: f64,
    pub sl: f64,
    pub tp: f64,
    pub reason: String,
    pub status: String,
    pub retcode: Option<i64>,
    pub message: Option<String>,
    pub created_ms: TimestampMs,
    pub updated_ms: TimestampMs,
}
