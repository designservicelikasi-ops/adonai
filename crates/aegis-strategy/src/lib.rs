//! AEGIS STRATEGY — modular strategy framework.
//!
//! A strategy is a pure function of completed candles: the same code runs in
//! the backtester and in the live engine, and it can never see the future.

mod lifecycle;
mod strategies;
mod strategies2;

pub use lifecycle::{can_promote, wilson_lower_bound, Evidence, StrategyStage};
pub use strategies::{DonchianBreakout, EmaTrend, RsiReversion};
pub use strategies2::{
    BollingerReversion, Pullback, RsiMomentum, SrRejection, TrendFollowing, VolatilityExpansion,
};

use aegis_types::{Candle, Regime, Side, Timeframe};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Number of most recent bars handed to a strategy, live and in backtests.
pub const LOOKBACK: usize = 300;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyMeta {
    pub id: String,
    pub name: String,
    pub description: String,
    pub timeframes: Vec<Timeframe>,
    /// Empty = any instrument.
    pub symbols: Vec<String>,
    /// Regimes in which new entries are allowed.
    pub regimes: Vec<Regime>,
    pub min_bars: usize,
    pub entry_rule: String,
    pub exit_rule: String,
    pub stop_rule: String,
    pub target_rule: String,
}

impl StrategyMeta {
    pub fn supports_symbol(&self, symbol: &str) -> bool {
        self.symbols.is_empty() || self.symbols.iter().any(|s| s == symbol)
    }
    pub fn supports_timeframe(&self, tf: Timeframe) -> bool {
        self.timeframes.contains(&tf)
    }
    pub fn supports_regime(&self, regime: Regime) -> bool {
        self.regimes.contains(&regime)
    }
}

/// An entry idea. Stops are distances from the actual fill price, so they stay
/// valid whatever the spread is at execution time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalIntent {
    pub side: Side,
    pub sl_distance: f64,
    pub tp_distance: f64,
    /// Human-readable explanation of why the signal fired.
    pub reason: String,
    /// Indicator values that produced the signal.
    pub evidence: Vec<(String, f64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StrategyAction {
    None,
    Enter(SignalIntent),
    Exit { reason: String },
}

/// The position currently held by this strategy on this instrument, if any.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenView {
    pub side: Side,
    pub entry_price: f64,
    pub bars_held: usize,
}

pub trait Strategy: Send + Sync {
    fn meta(&self) -> &StrategyMeta;
    fn config(&self) -> Value;
    /// `bars` are completed candles, oldest first, ending with the bar that just closed.
    fn evaluate(&self, bars: &[Candle], open: Option<&OpenView>) -> StrategyAction;
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum StrategyError {
    #[error("unknown strategy `{0}`")]
    Unknown(String),
    #[error("invalid configuration for `{id}`: {problem}")]
    InvalidConfig { id: String, problem: String },
}

pub const STRATEGY_IDS: [&str; 9] = [
    "ema_trend",
    "rsi_momentum",
    "trend_following",
    "donchian_breakout",
    "pullback",
    "rsi_reversion",
    "bollinger_reversion",
    "sr_rejection",
    "volatility_expansion",
];

/// Build a strategy from its id and an optional JSON configuration (`null` = defaults).
pub fn build(id: &str, config: &Value) -> Result<Box<dyn Strategy>, StrategyError> {
    fn parse<T: serde::de::DeserializeOwned + Default>(id: &str, v: &Value) -> Result<T, StrategyError> {
        if v.is_null() {
            return Ok(T::default());
        }
        serde_json::from_value(v.clone())
            .map_err(|e| StrategyError::InvalidConfig { id: id.to_string(), problem: e.to_string() })
    }
    let invalid = |problem: String| StrategyError::InvalidConfig { id: id.to_string(), problem };
    match id {
        "ema_trend" => Ok(Box::new(EmaTrend::new(parse(id, config)?).map_err(invalid)?)),
        "rsi_reversion" => Ok(Box::new(RsiReversion::new(parse(id, config)?).map_err(invalid)?)),
        "donchian_breakout" => Ok(Box::new(DonchianBreakout::new(parse(id, config)?).map_err(invalid)?)),
        "rsi_momentum" => Ok(Box::new(RsiMomentum::new(parse(id, config)?).map_err(invalid)?)),
        "trend_following" => Ok(Box::new(TrendFollowing::new(parse(id, config)?).map_err(invalid)?)),
        "pullback" => Ok(Box::new(Pullback::new(parse(id, config)?).map_err(invalid)?)),
        "bollinger_reversion" => Ok(Box::new(BollingerReversion::new(parse(id, config)?).map_err(invalid)?)),
        "sr_rejection" => Ok(Box::new(SrRejection::new(parse(id, config)?).map_err(invalid)?)),
        "volatility_expansion" => Ok(Box::new(VolatilityExpansion::new(parse(id, config)?).map_err(invalid)?)),
        other => Err(StrategyError::Unknown(other.to_string())),
    }
}

pub fn default_strategies() -> Vec<Box<dyn Strategy>> {
    STRATEGY_IDS.iter().map(|id| build(id, &Value::Null).expect("default configuration is valid")).collect()
}

/// The slice a strategy is allowed to see: the last `LOOKBACK` bars.
pub fn window(bars: &[Candle]) -> &[Candle] {
    &bars[bars.len().saturating_sub(LOOKBACK)..]
}
