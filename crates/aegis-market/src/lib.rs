//! AEGIS MARKET — real-time market state: ticks, candles, indicators, regime,
//! sessions and data-quality checks. This crate never fabricates prices.

pub mod candles;
pub mod indicators;
pub mod regime;
pub mod structure;
pub mod mtf;
pub mod scanner;

pub use candles::{aggregate, CandleBuilder, CandleSeries};
pub use regime::{detect_regime, detect_regime_detail, session_at, RegimeDetail, RegimeKind, RegimeReading, Session, VolatilityDynamics, VolatilityLevel};

use aegis_types::{Candle, Regime, Tick, Timeframe, TimestampMs};
use indicators::{atr, ema, median, realized_volatility, rsi};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MarketConfig {
    pub timeframes: Vec<Timeframe>,
    pub series_capacity: usize,
    /// A price older than this (engine clock) is stale.
    pub stale_after_ms: i64,
    /// Spread above `factor x rolling median` is abnormal.
    pub abnormal_spread_factor: f64,
    /// Tolerated difference between broker time and engine time.
    pub max_clock_offset_ms: i64,
}

impl Default for MarketConfig {
    fn default() -> Self {
        MarketConfig {
            // M30 since 0.3 (multi-timeframe structure); H4/D1 are rebuilt from H1 by the engine.
            timeframes: vec![Timeframe::M1, Timeframe::M5, Timeframe::M15, Timeframe::M30, Timeframe::H1],
            series_capacity: 1_500,
            stale_after_ms: 5_000,
            abnormal_spread_factor: 4.0,
            max_clock_offset_ms: 120_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityIssue {
    NoData,
    StalePrice { age_ms: i64 },
    InvalidTimestamp { detail: String },
    AbnormalSpread { spread_points_x10: i64, median_points_x10: i64 },
    MissingBars { timeframe: Timeframe, count: usize },
    RejectedTicks { count: u64 },
}

impl QualityIssue {
    pub fn describe(&self) -> String {
        match self {
            QualityIssue::NoData => "aucune donnée reçue".into(),
            QualityIssue::StalePrice { age_ms } => format!("prix obsolète ({} s)", age_ms / 1000),
            QualityIssue::InvalidTimestamp { detail } => format!("horodatage invalide: {detail}"),
            QualityIssue::AbnormalSpread { spread_points_x10, median_points_x10 } => format!(
                "spread anormal: {:.1} points (médiane {:.1})",
                *spread_points_x10 as f64 / 10.0,
                *median_points_x10 as f64 / 10.0
            ),
            QualityIssue::MissingBars { timeframe, count } => format!("{count} bougie(s) {timeframe} manquante(s)"),
            QualityIssue::RejectedTicks { count } => format!("{count} tick(s) invalide(s) rejeté(s)"),
        }
    }
    /// Issues that make the current price unusable for a trading decision.
    pub fn blocks_trading(&self) -> bool {
        !matches!(self, QualityIssue::MissingBars { .. } | QualityIssue::RejectedTicks { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndicatorSnapshot {
    pub timeframe: Timeframe,
    pub bars: usize,
    pub ema_fast: Option<f64>,
    pub ema_slow: Option<f64>,
    pub rsi: Option<f64>,
    pub atr: Option<f64>,
    pub volatility: Option<f64>,
    pub regime: RegimeReading,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketSnapshot {
    pub symbol: String,
    pub tick: Option<Tick>,
    /// Engine receive time of the last tick.
    pub received_ms: TimestampMs,
    pub spread_points: Option<f64>,
    pub session: Session,
    pub indicators: Vec<IndicatorSnapshot>,
    pub issues: Vec<QualityIssue>,
    pub ticks_received: u64,
}

impl MarketSnapshot {
    pub fn tradable(&self) -> bool {
        self.tick.is_some() && !self.issues.iter().any(QualityIssue::blocks_trading)
    }
    pub fn regime(&self, tf: Timeframe) -> Regime {
        self.indicators.iter().find(|i| i.timeframe == tf).map(|i| i.regime.regime).unwrap_or(Regime::Unknown)
    }
}

/// A bar that has just closed.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosedBar {
    pub symbol: String,
    pub timeframe: Timeframe,
    pub candle: Candle,
}

struct SymbolMarket {
    point: f64,
    last_tick: Option<Tick>,
    received_ms: TimestampMs,
    builders: BTreeMap<Timeframe, CandleBuilder>,
    series: BTreeMap<Timeframe, CandleSeries>,
    spreads: VecDeque<f64>,
    ticks_received: u64,
    ticks_rejected: u64,
    timestamp_issue: Option<String>,
}

pub struct MarketEngine {
    cfg: MarketConfig,
    symbols: HashMap<String, SymbolMarket>,
}

const SPREAD_WINDOW: usize = 300;

impl MarketEngine {
    pub fn new(cfg: MarketConfig) -> Self {
        MarketEngine { cfg, symbols: HashMap::new() }
    }

    pub fn config(&self) -> &MarketConfig {
        &self.cfg
    }

    /// Enable or disable the broker/engine clock comparison. The simulator runs on
    /// accelerated market time, so the comparison is meaningless for it.
    pub fn set_clock_check(&mut self, enabled: bool, default_ms: i64) {
        self.cfg.max_clock_offset_ms = if enabled { default_ms } else { i64::MAX / 4 };
    }

    /// Register an instrument (idempotent). `point` comes from the broker specification.
    pub fn register_symbol(&mut self, symbol: &str, point: f64) {
        let cfg = &self.cfg;
        self.symbols.entry(symbol.to_string()).or_insert_with(|| SymbolMarket {
            point,
            last_tick: None,
            received_ms: 0,
            builders: cfg.timeframes.iter().map(|tf| (*tf, CandleBuilder::new(*tf, point))).collect(),
            series: cfg.timeframes.iter().map(|tf| (*tf, CandleSeries::new(*tf, cfg.series_capacity))).collect(),
            spreads: VecDeque::new(),
            ticks_received: 0,
            ticks_rejected: 0,
            timestamp_issue: None,
        });
    }

    pub fn symbols(&self) -> Vec<String> {
        let mut v: Vec<String> = self.symbols.keys().cloned().collect();
        v.sort();
        v
    }

    /// Process a live tick. `tick.ts_ms` is broker time (UTC); `received_ms` is the engine clock.
    /// Returns the bars this tick completed. Invalid ticks are counted and dropped.
    pub fn on_tick(&mut self, symbol: &str, tick: Tick, received_ms: TimestampMs) -> Vec<ClosedBar> {
        let max_offset = self.cfg.max_clock_offset_ms;
        let Some(m) = self.symbols.get_mut(symbol) else { return Vec::new() };
        let valid = tick.bid.is_finite() && tick.ask.is_finite() && tick.bid > 0.0 && tick.ask >= tick.bid && tick.ts_ms > 0;
        if !valid {
            m.ticks_rejected += 1;
            return Vec::new();
        }
        if let Some(prev) = &m.last_tick {
            if tick.ts_ms < prev.ts_ms {
                m.ticks_rejected += 1;
                m.timestamp_issue = Some(format!("tick antérieur au précédent ({} ms)", prev.ts_ms - tick.ts_ms));
                return Vec::new();
            }
        }
        let offset = tick.ts_ms - received_ms;
        m.timestamp_issue = if offset.abs() > max_offset {
            Some(format!("décalage d'horloge broker/moteur de {} s", offset / 1000))
        } else {
            None
        };
        m.ticks_received += 1;
        m.received_ms = received_ms;
        m.spreads.push_back(tick.spread() / m.point);
        if m.spreads.len() > SPREAD_WINDOW {
            m.spreads.pop_front();
        }
        m.last_tick = Some(tick);
        let mut closed = Vec::new();
        for (tf, builder) in m.builders.iter_mut() {
            if let Some(done) = builder.on_tick(&tick) {
                if let Some(series) = m.series.get_mut(tf) {
                    if series.push(done) {
                        closed.push(ClosedBar { symbol: symbol.to_string(), timeframe: *tf, candle: done });
                    }
                }
            }
        }
        closed
    }

    /// Load a historical bar (from the broker or the database). Returns false if rejected.
    pub fn on_history_bar(&mut self, symbol: &str, tf: Timeframe, candle: Candle) -> bool {
        self.symbols
            .get_mut(symbol)
            .and_then(|m| m.series.get_mut(&tf))
            .map(|s| s.push(candle))
            .unwrap_or(false)
    }

    pub fn candles(&self, symbol: &str, tf: Timeframe) -> Vec<Candle> {
        self.symbols.get(symbol).and_then(|m| m.series.get(&tf)).map(|s| s.to_vec()).unwrap_or_default()
    }

    pub fn last_tick(&self, symbol: &str) -> Option<(Tick, TimestampMs)> {
        self.symbols.get(symbol).and_then(|m| m.last_tick.map(|t| (t, m.received_ms)))
    }

    /// Current data-quality issues for one instrument.
    pub fn quality(&self, symbol: &str, now_ms: TimestampMs) -> Vec<QualityIssue> {
        let Some(m) = self.symbols.get(symbol) else { return vec![QualityIssue::NoData] };
        let mut issues = Vec::new();
        match &m.last_tick {
            None => issues.push(QualityIssue::NoData),
            Some(tick) => {
                let age = now_ms - m.received_ms;
                if age > self.cfg.stale_after_ms {
                    issues.push(QualityIssue::StalePrice { age_ms: age });
                }
                if m.spreads.len() >= 30 {
                    let all: Vec<f64> = m.spreads.iter().copied().collect();
                    let med = median(&all).unwrap_or(0.0);
                    let cur = tick.spread() / m.point;
                    if med > 0.0 && cur > med * self.cfg.abnormal_spread_factor {
                        issues.push(QualityIssue::AbnormalSpread {
                            spread_points_x10: (cur * 10.0).round() as i64,
                            median_points_x10: (med * 10.0).round() as i64,
                        });
                    }
                }
            }
        }
        if let Some(detail) = &m.timestamp_issue {
            issues.push(QualityIssue::InvalidTimestamp { detail: detail.clone() });
        }
        for (tf, s) in &m.series {
            let missing = s.missing_bars();
            if missing > 0 {
                issues.push(QualityIssue::MissingBars { timeframe: *tf, count: missing });
            }
        }
        if m.ticks_rejected > 0 {
            issues.push(QualityIssue::RejectedTicks { count: m.ticks_rejected });
        }
        issues
    }

    pub fn snapshot(&self, symbol: &str, now_ms: TimestampMs) -> Option<MarketSnapshot> {
        let m = self.symbols.get(symbol)?;
        let indicators = m
            .series
            .iter()
            .map(|(tf, s)| {
                let bars = s.to_vec();
                let closes = s.closes();
                IndicatorSnapshot {
                    timeframe: *tf,
                    bars: bars.len(),
                    ema_fast: ema(&closes, 20),
                    ema_slow: ema(&closes, 50),
                    rsi: rsi(&closes, 14),
                    atr: atr(&bars, 14),
                    volatility: realized_volatility(&closes, 30),
                    regime: detect_regime(&bars),
                }
            })
            .collect();
        Some(MarketSnapshot {
            symbol: symbol.to_string(),
            tick: m.last_tick,
            received_ms: m.received_ms,
            spread_points: m.last_tick.map(|t| t.spread() / m.point),
            session: session_at(m.last_tick.map(|t| t.ts_ms).unwrap_or(now_ms)),
            indicators,
            issues: self.quality(symbol, now_ms),
            ticks_received: m.ticks_received,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_800_000_000_000 - 1_800_000_000_000 % 60_000;

    fn engine() -> MarketEngine {
        let mut e = MarketEngine::new(MarketConfig::default());
        e.register_symbol("EURUSD", 0.00001);
        e
    }

    fn tick(ts: i64, bid: f64, spread_pts: f64) -> Tick {
        Tick { ts_ms: ts, bid, ask: bid + spread_pts * 0.00001 }
    }

    #[test]
    fn ticks_build_candles_on_every_timeframe() {
        let mut e = engine();
        let mut closed = Vec::new();
        for i in 0..(16 * 60) {
            let ts = T0 + i * 1_000;
            closed.extend(e.on_tick("EURUSD", tick(ts, 1.1 + (i % 7) as f64 * 0.0001, 10.0), ts));
        }
        let m1 = closed.iter().filter(|c| c.timeframe == Timeframe::M1).count();
        let m5 = closed.iter().filter(|c| c.timeframe == Timeframe::M5).count();
        assert_eq!(m1, 15);
        assert!(m5 >= 2);
        assert_eq!(e.candles("EURUSD", Timeframe::M1).len(), 15);
        assert!(e.candles("EURUSD", Timeframe::M1).iter().all(Candle::is_valid));
        let snap = e.snapshot("EURUSD", T0 + 16 * 60_000).unwrap();
        assert_eq!(snap.ticks_received, 960);
        assert!((snap.spread_points.unwrap() - 10.0).abs() < 1e-6);
        assert!(snap.tradable());
        // Unknown symbols are ignored, not invented.
        assert!(e.on_tick("XAUUSD", tick(T0, 2000.0, 10.0), T0).is_empty());
        assert!(e.snapshot("XAUUSD", T0).is_none());
    }

    #[test]
    fn invalid_ticks_are_rejected_and_reported() {
        let mut e = engine();
        e.on_tick("EURUSD", tick(T0, 1.1, 10.0), T0);
        e.on_tick("EURUSD", Tick { ts_ms: T0 + 1, bid: f64::NAN, ask: 1.1 }, T0 + 1);
        e.on_tick("EURUSD", Tick { ts_ms: T0 + 2, bid: 1.2, ask: 1.1 }, T0 + 2);
        e.on_tick("EURUSD", Tick { ts_ms: T0 + 3, bid: -1.0, ask: 1.1 }, T0 + 3);
        e.on_tick("EURUSD", tick(T0 - 5_000, 1.1, 10.0), T0 + 4); // goes back in time
        let issues = e.quality("EURUSD", T0 + 10);
        assert!(issues.contains(&QualityIssue::RejectedTicks { count: 4 }));
        assert!(issues.iter().any(|i| matches!(i, QualityIssue::InvalidTimestamp { .. })));
        assert_eq!(e.last_tick("EURUSD").unwrap().0.bid, 1.1);
        assert!(!e.snapshot("EURUSD", T0 + 10).unwrap().tradable());
    }

    #[test]
    fn stale_price_and_no_data_are_detected() {
        let mut e = engine();
        assert_eq!(e.quality("EURUSD", T0), vec![QualityIssue::NoData]);
        assert_eq!(e.quality("UNKNOWN", T0), vec![QualityIssue::NoData]);
        e.on_tick("EURUSD", tick(T0, 1.1, 10.0), T0);
        assert!(e.quality("EURUSD", T0 + 4_000).is_empty());
        assert_eq!(e.quality("EURUSD", T0 + 9_000), vec![QualityIssue::StalePrice { age_ms: 9_000 }]);
        assert!(!e.snapshot("EURUSD", T0 + 9_000).unwrap().tradable());
    }

    #[test]
    fn abnormal_spread_and_clock_offset_are_detected() {
        let mut e = engine();
        for i in 0..60 {
            e.on_tick("EURUSD", tick(T0 + i * 100, 1.1, 10.0), T0 + i * 100);
        }
        e.on_tick("EURUSD", tick(T0 + 6_100, 1.1, 80.0), T0 + 6_100);
        let issues = e.quality("EURUSD", T0 + 6_200);
        assert!(issues.iter().any(|i| matches!(i, QualityIssue::AbnormalSpread { spread_points_x10: 800, .. })), "{issues:?}");
        // Broker clock ten minutes away from the engine clock.
        e.on_tick("EURUSD", tick(T0 + 700_000, 1.1, 10.0), T0 + 7_000);
        assert!(e.quality("EURUSD", T0 + 7_100).iter().any(|i| matches!(i, QualityIssue::InvalidTimestamp { .. })));
    }

    #[test]
    fn history_and_gaps() {
        let mut e = engine();
        let bar = |i: i64| Candle { open_ts: T0 + i * 60_000, open: 1.1, high: 1.101, low: 1.099, close: 1.1005, tick_volume: 3, spread_points: 9.0 };
        for i in [0, 1, 2, 6, 7] {
            assert!(e.on_history_bar("EURUSD", Timeframe::M1, bar(i)));
        }
        assert!(!e.on_history_bar("EURUSD", Timeframe::D1, bar(0)), "timeframe not tracked");
        assert!(!e.on_history_bar("EURUSD", Timeframe::M1, Candle { low: 5.0, ..bar(9) }));
        e.on_tick("EURUSD", tick(T0 + 8 * 60_000, 1.1, 10.0), T0 + 8 * 60_000);
        let issues = e.quality("EURUSD", T0 + 8 * 60_000);
        assert!(issues.contains(&QualityIssue::MissingBars { timeframe: Timeframe::M1, count: 3 }));
        // Missing bars are reported but do not by themselves block the current price.
        assert!(e.snapshot("EURUSD", T0 + 8 * 60_000).unwrap().tradable());
    }

    #[test]
    fn indicators_need_enough_history() {
        let mut e = engine();
        for i in 0..10 {
            e.on_history_bar("EURUSD", Timeframe::M15, Candle { open_ts: i * 900_000, open: 1.1, high: 1.101, low: 1.099, close: 1.1, tick_volume: 1, spread_points: 0.0 });
        }
        let snap = e.snapshot("EURUSD", T0).unwrap();
        let m15 = snap.indicators.iter().find(|i| i.timeframe == Timeframe::M15).unwrap();
        assert_eq!(m15.bars, 10);
        assert!(m15.ema_fast.is_none() && m15.rsi.is_none() && m15.atr.is_none());
        assert_eq!(snap.regime(Timeframe::M15), Regime::Unknown);
    }
}
