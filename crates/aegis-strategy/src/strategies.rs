//! Built-in strategies. All stops and targets are ATR multiples.

use crate::{OpenView, SignalIntent, Strategy, StrategyAction, StrategyMeta};
use aegis_market::indicators::{atr, donchian, ema_series, rsi};
use aegis_types::{Candle, Regime, Side, Timeframe};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const INTRADAY: [Timeframe; 5] = [Timeframe::M1, Timeframe::M5, Timeframe::M15, Timeframe::M30, Timeframe::H1];

pub(crate) fn closes(bars: &[Candle]) -> Vec<f64> {
    bars.iter().map(|c| c.close).collect()
}

pub(crate) fn check_atr(period: usize, sl: f64, tp: f64) -> Result<(), String> {
    if !(2..=200).contains(&period) {
        return Err("atr_period must be within 2..200".into());
    }
    if !(sl.is_finite() && sl > 0.0 && sl <= 20.0 && tp.is_finite() && tp > 0.0 && tp <= 50.0) {
        return Err("sl_atr must be within (0, 20] and tp_atr within (0, 50]".into());
    }
    Ok(())
}

// ------------------------------------------------------------------ EMA trend

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EmaTrendConfig {
    pub fast: usize,
    pub slow: usize,
    pub atr_period: usize,
    pub sl_atr: f64,
    pub tp_atr: f64,
}

impl Default for EmaTrendConfig {
    fn default() -> Self {
        EmaTrendConfig { fast: 20, slow: 50, atr_period: 14, sl_atr: 2.0, tp_atr: 3.0 }
    }
}

/// Trend following: enter when the fast EMA crosses the slow EMA.
pub struct EmaTrend {
    cfg: EmaTrendConfig,
    meta: StrategyMeta,
}

impl EmaTrend {
    pub fn new(cfg: EmaTrendConfig) -> Result<Self, String> {
        if cfg.fast < 2 || cfg.slow <= cfg.fast || cfg.slow > 250 {
            return Err("periods must satisfy 2 <= fast < slow <= 250".into());
        }
        check_atr(cfg.atr_period, cfg.sl_atr, cfg.tp_atr)?;
        let meta = StrategyMeta {
            id: "ema_trend".into(),
            name: "Suivi de tendance EMA".into(),
            description: format!("Croisement des moyennes exponentielles {}/{}", cfg.fast, cfg.slow),
            timeframes: INTRADAY.to_vec(),
            symbols: vec![],
            regimes: vec![Regime::TrendUp, Regime::TrendDown, Regime::Range],
            min_bars: cfg.slow + cfg.atr_period + 2,
            entry_rule: format!("Achat quand l'EMA{} passe au-dessus de l'EMA{}; vente dans le cas inverse", cfg.fast, cfg.slow),
            exit_rule: "Sortie au croisement inverse".into(),
            stop_rule: format!("Stop-loss à {} x ATR({})", cfg.sl_atr, cfg.atr_period),
            target_rule: format!("Take-profit à {} x ATR({})", cfg.tp_atr, cfg.atr_period),
        };
        Ok(EmaTrend { cfg, meta })
    }
}

impl Strategy for EmaTrend {
    fn meta(&self) -> &StrategyMeta {
        &self.meta
    }
    fn config(&self) -> Value {
        serde_json::to_value(&self.cfg).unwrap_or(Value::Null)
    }
    fn evaluate(&self, bars: &[Candle], open: Option<&OpenView>) -> StrategyAction {
        if bars.len() < self.meta.min_bars {
            return StrategyAction::None;
        }
        let c = closes(bars);
        let fast = ema_series(&c, self.cfg.fast);
        let slow = ema_series(&c, self.cfg.slow);
        let n = c.len();
        let (Some(f1), Some(s1), Some(f0), Some(s0)) = (fast[n - 1], slow[n - 1], fast[n - 2], slow[n - 2]) else {
            return StrategyAction::None;
        };
        let crossed_up = f0 <= s0 && f1 > s1;
        let crossed_down = f0 >= s0 && f1 < s1;
        if let Some(pos) = open {
            let against = match pos.side {
                Side::Buy => crossed_down,
                Side::Sell => crossed_up,
            };
            return if against {
                StrategyAction::Exit { reason: "croisement inverse des EMA".into() }
            } else {
                StrategyAction::None
            };
        }
        let Some(a) = atr(bars, self.cfg.atr_period).filter(|a| *a > 0.0) else { return StrategyAction::None };
        let side = if crossed_up {
            Side::Buy
        } else if crossed_down {
            Side::Sell
        } else {
            return StrategyAction::None;
        };
        StrategyAction::Enter(SignalIntent {
            side,
            sl_distance: a * self.cfg.sl_atr,
            tp_distance: a * self.cfg.tp_atr,
            reason: format!(
                "EMA{} ({f1:.5}) a croisé l'EMA{} ({s1:.5}) vers le {}",
                self.cfg.fast,
                self.cfg.slow,
                if side == Side::Buy { "haut" } else { "bas" }
            ),
            evidence: vec![("ema_fast".into(), f1), ("ema_slow".into(), s1), ("atr".into(), a)],
        })
    }
}

// -------------------------------------------------------------- RSI reversion

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RsiReversionConfig {
    pub period: usize,
    pub oversold: f64,
    pub overbought: f64,
    pub atr_period: usize,
    pub sl_atr: f64,
    pub tp_atr: f64,
}

impl Default for RsiReversionConfig {
    fn default() -> Self {
        RsiReversionConfig { period: 14, oversold: 30.0, overbought: 70.0, atr_period: 14, sl_atr: 1.5, tp_atr: 1.5 }
    }
}

/// Range trading: fade RSI extremes once the RSI turns back.
pub struct RsiReversion {
    cfg: RsiReversionConfig,
    meta: StrategyMeta,
}

impl RsiReversion {
    pub fn new(cfg: RsiReversionConfig) -> Result<Self, String> {
        if !(2..=100).contains(&cfg.period) {
            return Err("period must be within 2..100".into());
        }
        if !(cfg.oversold > 0.0 && cfg.oversold < 50.0 && cfg.overbought > 50.0 && cfg.overbought < 100.0) {
            return Err("thresholds must satisfy 0 < oversold < 50 < overbought < 100".into());
        }
        check_atr(cfg.atr_period, cfg.sl_atr, cfg.tp_atr)?;
        let meta = StrategyMeta {
            id: "rsi_reversion".into(),
            name: "Retour à la moyenne RSI".into(),
            description: format!("RSI({}) en zone extrême dans un marché en range", cfg.period),
            timeframes: INTRADAY.to_vec(),
            symbols: vec![],
            regimes: vec![Regime::Range],
            min_bars: cfg.period.max(cfg.atr_period) + 3,
            entry_rule: format!("Achat quand le RSI remonte au-dessus de {}; vente quand il redescend sous {}", cfg.oversold, cfg.overbought),
            exit_rule: "Sortie quand le RSI retraverse 50".into(),
            stop_rule: format!("Stop-loss à {} x ATR({})", cfg.sl_atr, cfg.atr_period),
            target_rule: format!("Take-profit à {} x ATR({})", cfg.tp_atr, cfg.atr_period),
        };
        Ok(RsiReversion { cfg, meta })
    }
}

impl Strategy for RsiReversion {
    fn meta(&self) -> &StrategyMeta {
        &self.meta
    }
    fn config(&self) -> Value {
        serde_json::to_value(&self.cfg).unwrap_or(Value::Null)
    }
    fn evaluate(&self, bars: &[Candle], open: Option<&OpenView>) -> StrategyAction {
        if bars.len() < self.meta.min_bars {
            return StrategyAction::None;
        }
        let c = closes(bars);
        let (Some(now), Some(prev)) = (rsi(&c, self.cfg.period), rsi(&c[..c.len() - 1], self.cfg.period)) else {
            return StrategyAction::None;
        };
        if let Some(pos) = open {
            let done = match pos.side {
                Side::Buy => now >= 50.0,
                Side::Sell => now <= 50.0,
            };
            return if done {
                StrategyAction::Exit { reason: format!("RSI revenu à {now:.1}") }
            } else {
                StrategyAction::None
            };
        }
        let Some(a) = atr(bars, self.cfg.atr_period).filter(|a| *a > 0.0) else { return StrategyAction::None };
        let side = if prev < self.cfg.oversold && now >= self.cfg.oversold {
            Side::Buy
        } else if prev > self.cfg.overbought && now <= self.cfg.overbought {
            Side::Sell
        } else {
            return StrategyAction::None;
        };
        StrategyAction::Enter(SignalIntent {
            side,
            sl_distance: a * self.cfg.sl_atr,
            tp_distance: a * self.cfg.tp_atr,
            reason: format!("RSI({}) passé de {prev:.1} à {now:.1}: sortie de zone extrême", self.cfg.period),
            evidence: vec![("rsi".into(), now), ("rsi_prev".into(), prev), ("atr".into(), a)],
        })
    }
}

// ---------------------------------------------------------- Donchian breakout

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DonchianBreakoutConfig {
    pub period: usize,
    pub atr_period: usize,
    pub sl_atr: f64,
    pub tp_atr: f64,
}

impl Default for DonchianBreakoutConfig {
    fn default() -> Self {
        DonchianBreakoutConfig { period: 20, atr_period: 14, sl_atr: 2.0, tp_atr: 4.0 }
    }
}

/// Breakout: enter when the close leaves the channel of the previous `period` bars.
pub struct DonchianBreakout {
    cfg: DonchianBreakoutConfig,
    meta: StrategyMeta,
}

impl DonchianBreakout {
    pub fn new(cfg: DonchianBreakoutConfig) -> Result<Self, String> {
        if !(5..=250).contains(&cfg.period) {
            return Err("period must be within 5..250".into());
        }
        check_atr(cfg.atr_period, cfg.sl_atr, cfg.tp_atr)?;
        let meta = StrategyMeta {
            id: "donchian_breakout".into(),
            name: "Breakout de canal".into(),
            description: format!("Cassure du canal de Donchian sur {} bougies", cfg.period),
            timeframes: INTRADAY.to_vec(),
            symbols: vec![],
            regimes: vec![Regime::Range, Regime::TrendUp, Regime::TrendDown],
            min_bars: cfg.period.max(cfg.atr_period) + 2,
            entry_rule: format!("Achat si la clôture dépasse le plus haut des {} bougies précédentes; vente sous le plus bas", cfg.period),
            exit_rule: "Sortie si la clôture revient au milieu du canal".into(),
            stop_rule: format!("Stop-loss à {} x ATR({})", cfg.sl_atr, cfg.atr_period),
            target_rule: format!("Take-profit à {} x ATR({})", cfg.tp_atr, cfg.atr_period),
        };
        Ok(DonchianBreakout { cfg, meta })
    }
}

impl Strategy for DonchianBreakout {
    fn meta(&self) -> &StrategyMeta {
        &self.meta
    }
    fn config(&self) -> Value {
        serde_json::to_value(&self.cfg).unwrap_or(Value::Null)
    }
    fn evaluate(&self, bars: &[Candle], open: Option<&OpenView>) -> StrategyAction {
        if bars.len() < self.meta.min_bars {
            return StrategyAction::None;
        }
        let last = bars[bars.len() - 1];
        // Channel of the bars BEFORE the one that just closed.
        let Some((hi, lo)) = donchian(&bars[..bars.len() - 1], self.cfg.period) else { return StrategyAction::None };
        if let Some(pos) = open {
            let mid = (hi + lo) / 2.0;
            let failed = match pos.side {
                Side::Buy => last.close < mid,
                Side::Sell => last.close > mid,
            };
            return if failed {
                StrategyAction::Exit { reason: "retour au milieu du canal".into() }
            } else {
                StrategyAction::None
            };
        }
        let Some(a) = atr(bars, self.cfg.atr_period).filter(|a| *a > 0.0) else { return StrategyAction::None };
        let side = if last.close > hi {
            Side::Buy
        } else if last.close < lo {
            Side::Sell
        } else {
            return StrategyAction::None;
        };
        StrategyAction::Enter(SignalIntent {
            side,
            sl_distance: a * self.cfg.sl_atr,
            tp_distance: a * self.cfg.tp_atr,
            reason: format!("clôture {:.5} hors du canal [{lo:.5} ; {hi:.5}]", last.close),
            evidence: vec![("channel_high".into(), hi), ("channel_low".into(), lo), ("atr".into(), a)],
        })
    }
}
