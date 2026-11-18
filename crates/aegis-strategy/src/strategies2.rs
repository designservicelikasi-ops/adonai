//! Strategies added in v0.2: RSI momentum, trend following, pullback,
//! Bollinger mean reversion, support/resistance rejection and volatility
//! expansion. Same contract as the original three: pure functions of closed
//! candles, stops and targets as ATR multiples, a written reason and the
//! indicator values that produced the signal.

use crate::strategies::{check_atr, closes, INTRADAY};
use crate::{OpenView, SignalIntent, Strategy, StrategyAction, StrategyMeta};
use aegis_market::indicators::{atr, bollinger, efficiency_ratio, ema_series, rsi, swing_levels};
use aegis_types::{Candle, Regime, Side};
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn intent(side: Side, a: f64, sl_atr: f64, tp_atr: f64, reason: String, evidence: Vec<(&str, f64)>) -> StrategyAction {
    StrategyAction::Enter(SignalIntent {
        side,
        sl_distance: a * sl_atr,
        tp_distance: a * tp_atr,
        reason,
        evidence: evidence.into_iter().map(|(k, v)| (k.to_string(), v)).chain(std::iter::once(("atr".to_string(), a))).collect(),
    })
}

fn meta(id: &str, name: &str, description: String, regimes: Vec<Regime>, min_bars: usize, rules: [String; 4]) -> StrategyMeta {
    let [entry_rule, exit_rule, stop_rule, target_rule] = rules;
    StrategyMeta {
        id: id.into(),
        name: name.into(),
        description,
        timeframes: INTRADAY.to_vec(),
        symbols: vec![],
        regimes,
        min_bars,
        entry_rule,
        exit_rule,
        stop_rule,
        target_rule,
    }
}

macro_rules! strategy_boilerplate {
    ($t:ty) => {
        fn meta(&self) -> &StrategyMeta {
            &self.meta
        }
        fn config(&self) -> Value {
            serde_json::to_value(&self.cfg).unwrap_or(Value::Null)
        }
    };
}

// ------------------------------------------------------------- RSI momentum

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RsiMomentumConfig {
    pub period: usize,
    /// RSI level crossed upward to buy (and `100 - level` downward to sell).
    pub level: f64,
    pub trend_ema: usize,
    /// The trend EMA must have moved at least `min_slope_atr` ATR over `slope_bars` bars.
    pub slope_bars: usize,
    pub min_slope_atr: f64,
    pub atr_period: usize,
    pub sl_atr: f64,
    pub tp_atr: f64,
}

impl Default for RsiMomentumConfig {
    fn default() -> Self {
        RsiMomentumConfig { period: 14, level: 55.0, trend_ema: 50, slope_bars: 5, min_slope_atr: 0.1, atr_period: 14, sl_atr: 1.5, tp_atr: 1.0 }
    }
}

/// Momentum: RSI crossing above 55 (below 45) in the direction of the EMA trend.
pub struct RsiMomentum {
    cfg: RsiMomentumConfig,
    meta: StrategyMeta,
}

impl RsiMomentum {
    pub fn new(cfg: RsiMomentumConfig) -> Result<Self, String> {
        if !(2..=100).contains(&cfg.period) || !(5..=250).contains(&cfg.trend_ema) {
            return Err("period within 2..100 and trend_ema within 5..250".into());
        }
        if !(cfg.level > 50.0 && cfg.level < 90.0) {
            return Err("level must be within (50, 90)".into());
        }
        if !(1..=50).contains(&cfg.slope_bars) || !(0.0..=5.0).contains(&cfg.min_slope_atr) {
            return Err("slope_bars within 1..50 and min_slope_atr within 0..5".into());
        }
        check_atr(cfg.atr_period, cfg.sl_atr, cfg.tp_atr)?;
        let meta = meta(
            "rsi_momentum",
            "Momentum RSI",
            format!("RSI({}) franchit {} dans le sens de l'EMA{}", cfg.period, cfg.level, cfg.trend_ema),
            vec![Regime::TrendUp, Regime::TrendDown],
            cfg.period.max(cfg.trend_ema + cfg.slope_bars).max(cfg.atr_period) + 3,
            [
                format!("Achat quand le RSI passe au-dessus de {} avec la clôture au-dessus d'une EMA{} montante (≥ {} ATR sur {} bougies); vente symétrique sous {}", cfg.level, cfg.trend_ema, cfg.min_slope_atr, cfg.slope_bars, 100.0 - cfg.level),
                "Sortie quand le RSI retraverse 50".into(),
                format!("Stop-loss à {} x ATR({})", cfg.sl_atr, cfg.atr_period),
                format!("Take-profit à {} x ATR({})", cfg.tp_atr, cfg.atr_period),
            ],
        );
        Ok(RsiMomentum { cfg, meta })
    }
}

impl Strategy for RsiMomentum {
    strategy_boilerplate!(RsiMomentum);
    fn evaluate(&self, bars: &[Candle], open: Option<&OpenView>) -> StrategyAction {
        if bars.len() < self.meta.min_bars {
            return StrategyAction::None;
        }
        let c = closes(bars);
        let (Some(now), Some(prev)) = (rsi(&c, self.cfg.period), rsi(&c[..c.len() - 1], self.cfg.period)) else {
            return StrategyAction::None;
        };
        if let Some(pos) = open {
            let faded = match pos.side {
                Side::Buy => now < 50.0,
                Side::Sell => now > 50.0,
            };
            return if faded { StrategyAction::Exit { reason: format!("momentum perdu: RSI {now:.1}") } } else { StrategyAction::None };
        }
        let es = ema_series(&c, self.cfg.trend_ema);
        let (Some(trend), Some(trend_before)) = (es[c.len() - 1], es[c.len() - 1 - self.cfg.slope_bars]) else { return StrategyAction::None };
        let Some(a) = atr(bars, self.cfg.atr_period).filter(|a| *a > 0.0) else { return StrategyAction::None };
        let slope = (trend - trend_before) / a;
        let close = c[c.len() - 1];
        let lo = 100.0 - self.cfg.level;
        let side = if prev <= self.cfg.level && now > self.cfg.level && close > trend && slope >= self.cfg.min_slope_atr {
            Side::Buy
        } else if prev >= lo && now < lo && close < trend && slope <= -self.cfg.min_slope_atr {
            Side::Sell
        } else {
            return StrategyAction::None;
        };
        intent(side, a, self.cfg.sl_atr, self.cfg.tp_atr, format!("RSI({}) {prev:.1} → {now:.1}, clôture {} l'EMA{}", self.cfg.period, if side == Side::Buy { "au-dessus de" } else { "sous" }, self.cfg.trend_ema), vec![("rsi", now), ("rsi_prev", prev), ("ema_trend", trend), ("ema_slope_atr", slope)])
    }
}

// ----------------------------------------------------------- Trend following

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TrendFollowingConfig {
    pub fast: usize,
    pub mid: usize,
    pub slow: usize,
    /// Minimum Kaufman efficiency ratio over `er_period` bars (0..1).
    pub min_efficiency: f64,
    pub er_period: usize,
    pub atr_period: usize,
    pub sl_atr: f64,
    pub tp_atr: f64,
}

impl Default for TrendFollowingConfig {
    fn default() -> Self {
        TrendFollowingConfig { fast: 20, mid: 50, slow: 100, min_efficiency: 0.3, er_period: 20, atr_period: 14, sl_atr: 2.0, tp_atr: 2.0 }
    }
}

/// Trend following: enter when three EMAs become aligned and price moves efficiently.
pub struct TrendFollowing {
    cfg: TrendFollowingConfig,
    meta: StrategyMeta,
}

impl TrendFollowing {
    pub fn new(cfg: TrendFollowingConfig) -> Result<Self, String> {
        if !(cfg.fast >= 2 && cfg.fast < cfg.mid && cfg.mid < cfg.slow && cfg.slow <= 250) {
            return Err("periods must satisfy 2 <= fast < mid < slow <= 250".into());
        }
        if !(0.0..1.0).contains(&cfg.min_efficiency) || !(5..=200).contains(&cfg.er_period) {
            return Err("min_efficiency within 0..1 and er_period within 5..200".into());
        }
        check_atr(cfg.atr_period, cfg.sl_atr, cfg.tp_atr)?;
        let meta = meta(
            "trend_following",
            "Suivi de tendance (alignement)",
            format!("EMA{}/{}/{} alignées et efficacité ≥ {}", cfg.fast, cfg.mid, cfg.slow, cfg.min_efficiency),
            vec![Regime::TrendUp, Regime::TrendDown],
            cfg.slow.max(cfg.er_period).max(cfg.atr_period) + 3,
            [
                format!("Achat quand EMA{} > EMA{} > EMA{} devient vrai avec un ratio d'efficacité ≥ {}; vente symétrique", cfg.fast, cfg.mid, cfg.slow, cfg.min_efficiency),
                format!("Sortie quand la clôture repasse de l'autre côté de l'EMA{}", cfg.mid),
                format!("Stop-loss à {} x ATR({})", cfg.sl_atr, cfg.atr_period),
                format!("Take-profit à {} x ATR({})", cfg.tp_atr, cfg.atr_period),
            ],
        );
        Ok(TrendFollowing { cfg, meta })
    }

    fn alignment(&self, f: Option<f64>, m: Option<f64>, s: Option<f64>) -> Option<Side> {
        let (f, m, s) = (f?, m?, s?);
        if f > m && m > s {
            Some(Side::Buy)
        } else if f < m && m < s {
            Some(Side::Sell)
        } else {
            None
        }
    }
}

impl Strategy for TrendFollowing {
    strategy_boilerplate!(TrendFollowing);
    fn evaluate(&self, bars: &[Candle], open: Option<&OpenView>) -> StrategyAction {
        if bars.len() < self.meta.min_bars {
            return StrategyAction::None;
        }
        let c = closes(bars);
        let n = c.len();
        let (f, m, s) = (ema_series(&c, self.cfg.fast), ema_series(&c, self.cfg.mid), ema_series(&c, self.cfg.slow));
        if let Some(pos) = open {
            let Some(mid) = m[n - 1] else { return StrategyAction::None };
            let broken = match pos.side {
                Side::Buy => c[n - 1] < mid,
                Side::Sell => c[n - 1] > mid,
            };
            return if broken { StrategyAction::Exit { reason: format!("clôture de l'autre côté de l'EMA{}", self.cfg.mid) } } else { StrategyAction::None };
        }
        // The previous bar must be fully computable: a signal never fires just because data became available.
        if s[n - 2].is_none() {
            return StrategyAction::None;
        }
        let now = self.alignment(f[n - 1], m[n - 1], s[n - 1]);
        let before = self.alignment(f[n - 2], m[n - 2], s[n - 2]);
        let Some(side) = now.filter(|x| Some(*x) != before) else { return StrategyAction::None };
        let Some(er) = efficiency_ratio(&c, self.cfg.er_period) else { return StrategyAction::None };
        if er < self.cfg.min_efficiency {
            return StrategyAction::None;
        }
        let Some(a) = atr(bars, self.cfg.atr_period).filter(|a| *a > 0.0) else { return StrategyAction::None };
        intent(side, a, self.cfg.sl_atr, self.cfg.tp_atr, format!("EMA{}/{}/{} viennent de s'aligner, efficacité {er:.2}", self.cfg.fast, self.cfg.mid, self.cfg.slow), vec![("efficiency", er), ("ema_mid", m[n - 1].unwrap_or(f64::NAN))])
    }
}

// ------------------------------------------------------------------ Pullback

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PullbackConfig {
    pub fast: usize,
    pub slow: usize,
    pub atr_period: usize,
    pub sl_atr: f64,
    pub tp_atr: f64,
}

impl Default for PullbackConfig {
    fn default() -> Self {
        PullbackConfig { fast: 20, slow: 50, atr_period: 14, sl_atr: 1.5, tp_atr: 0.8 }
    }
}

/// Pullback: in an EMA trend, the previous bar dipped to the fast EMA and the
/// bar that just closed resumed the trend.
pub struct Pullback {
    cfg: PullbackConfig,
    meta: StrategyMeta,
}

impl Pullback {
    pub fn new(cfg: PullbackConfig) -> Result<Self, String> {
        if cfg.fast < 2 || cfg.slow <= cfg.fast || cfg.slow > 250 {
            return Err("periods must satisfy 2 <= fast < slow <= 250".into());
        }
        check_atr(cfg.atr_period, cfg.sl_atr, cfg.tp_atr)?;
        let meta = meta(
            "pullback",
            "Pullback sur EMA",
            format!("Repli sur l'EMA{} dans une tendance EMA{}/{}", cfg.fast, cfg.fast, cfg.slow),
            vec![Regime::TrendUp, Regime::TrendDown],
            cfg.slow.max(cfg.atr_period) + 3,
            [
                format!("Achat si EMA{} > EMA{}, que la bougie précédente a touché l'EMA{} et que la dernière clôture repart au-dessus en bougie haussière; vente symétrique", cfg.fast, cfg.slow, cfg.fast),
                format!("Sortie si la clôture casse l'EMA{}", cfg.slow),
                format!("Stop-loss à {} x ATR({})", cfg.sl_atr, cfg.atr_period),
                format!("Take-profit à {} x ATR({})", cfg.tp_atr, cfg.atr_period),
            ],
        );
        Ok(Pullback { cfg, meta })
    }
}

impl Strategy for Pullback {
    strategy_boilerplate!(Pullback);
    fn evaluate(&self, bars: &[Candle], open: Option<&OpenView>) -> StrategyAction {
        if bars.len() < self.meta.min_bars {
            return StrategyAction::None;
        }
        let c = closes(bars);
        let n = c.len();
        let (fs, ss) = (ema_series(&c, self.cfg.fast), ema_series(&c, self.cfg.slow));
        let (Some(f1), Some(s1), Some(f0)) = (fs[n - 1], ss[n - 1], fs[n - 2]) else { return StrategyAction::None };
        let (prev, last) = (bars[n - 2], bars[n - 1]);
        if let Some(pos) = open {
            let broken = match pos.side {
                Side::Buy => last.close < s1,
                Side::Sell => last.close > s1,
            };
            return if broken { StrategyAction::Exit { reason: format!("tendance cassée: clôture de l'autre côté de l'EMA{}", self.cfg.slow) } } else { StrategyAction::None };
        }
        let side = if f1 > s1 && prev.low <= f0 && last.close > f1 && last.close > last.open {
            Side::Buy
        } else if f1 < s1 && prev.high >= f0 && last.close < f1 && last.close < last.open {
            Side::Sell
        } else {
            return StrategyAction::None;
        };
        let Some(a) = atr(bars, self.cfg.atr_period).filter(|a| *a > 0.0) else { return StrategyAction::None };
        intent(side, a, self.cfg.sl_atr, self.cfg.tp_atr, format!("repli sur l'EMA{} ({f0:.5}) puis reprise de la tendance", self.cfg.fast), vec![("ema_fast", f1), ("ema_slow", s1)])
    }
}

// --------------------------------------------------- Bollinger mean reversion

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BollingerReversionConfig {
    pub period: usize,
    pub k: f64,
    pub atr_period: usize,
    pub sl_atr: f64,
    pub tp_atr: f64,
}

impl Default for BollingerReversionConfig {
    fn default() -> Self {
        BollingerReversionConfig { period: 20, k: 2.0, atr_period: 14, sl_atr: 1.5, tp_atr: 0.7 }
    }
}

/// Mean reversion: a close outside a Bollinger band followed by a close back inside.
pub struct BollingerReversion {
    cfg: BollingerReversionConfig,
    meta: StrategyMeta,
}

impl BollingerReversion {
    pub fn new(cfg: BollingerReversionConfig) -> Result<Self, String> {
        if !(5..=200).contains(&cfg.period) || !(cfg.k > 0.5 && cfg.k <= 4.0) {
            return Err("period within 5..200 and k within (0.5, 4]".into());
        }
        check_atr(cfg.atr_period, cfg.sl_atr, cfg.tp_atr)?;
        let meta = meta(
            "bollinger_reversion",
            "Retour à la moyenne (Bollinger)",
            format!("Réintégration des bandes de Bollinger({}, {})", cfg.period, cfg.k),
            vec![Regime::Range],
            cfg.period.max(cfg.atr_period) + 3,
            [
                "Achat quand la clôture précédente était sous la bande basse et la dernière revient à l'intérieur; vente symétrique".into(),
                format!("Sortie au retour sur la moyenne mobile {}", cfg.period),
                format!("Stop-loss à {} x ATR({})", cfg.sl_atr, cfg.atr_period),
                format!("Take-profit à {} x ATR({})", cfg.tp_atr, cfg.atr_period),
            ],
        );
        Ok(BollingerReversion { cfg, meta })
    }
}

impl Strategy for BollingerReversion {
    strategy_boilerplate!(BollingerReversion);
    fn evaluate(&self, bars: &[Candle], open: Option<&OpenView>) -> StrategyAction {
        if bars.len() < self.meta.min_bars {
            return StrategyAction::None;
        }
        let c = closes(bars);
        let n = c.len();
        let (Some((mid, up, lo)), Some((_, up0, lo0))) = (bollinger(&c, self.cfg.period, self.cfg.k), bollinger(&c[..n - 1], self.cfg.period, self.cfg.k)) else {
            return StrategyAction::None;
        };
        if let Some(pos) = open {
            let back = match pos.side {
                Side::Buy => c[n - 1] >= mid,
                Side::Sell => c[n - 1] <= mid,
            };
            return if back { StrategyAction::Exit { reason: "retour sur la moyenne".into() } } else { StrategyAction::None };
        }
        let side = if c[n - 2] < lo0 && c[n - 1] >= lo && c[n - 1] < mid {
            Side::Buy
        } else if c[n - 2] > up0 && c[n - 1] <= up && c[n - 1] > mid {
            Side::Sell
        } else {
            return StrategyAction::None;
        };
        let Some(a) = atr(bars, self.cfg.atr_period).filter(|a| *a > 0.0) else { return StrategyAction::None };
        intent(side, a, self.cfg.sl_atr, self.cfg.tp_atr, format!("clôture revenue dans les bandes [{lo:.5} ; {up:.5}]"), vec![("bb_mid", mid), ("bb_upper", up), ("bb_lower", lo)])
    }
}

// ------------------------------------------------ Support/resistance rejection

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SrRejectionConfig {
    /// Bars on each side that confirm a swing high/low.
    pub pivot_strength: usize,
    /// How close (in ATR) the wick must come to a level.
    pub touch_atr: f64,
    /// Minimum rejection wick as a share of the bar's range.
    pub min_wick_ratio: f64,
    pub atr_period: usize,
    pub sl_atr: f64,
    pub tp_atr: f64,
}

impl Default for SrRejectionConfig {
    fn default() -> Self {
        SrRejectionConfig { pivot_strength: 3, touch_atr: 0.25, min_wick_ratio: 0.5, atr_period: 14, sl_atr: 1.2, tp_atr: 0.8 }
    }
}

/// Rejection: a wick tests a recent swing level and the bar closes back away from it.
pub struct SrRejection {
    cfg: SrRejectionConfig,
    meta: StrategyMeta,
}

impl SrRejection {
    pub fn new(cfg: SrRejectionConfig) -> Result<Self, String> {
        if !(1..=20).contains(&cfg.pivot_strength) || !(cfg.touch_atr > 0.0 && cfg.touch_atr <= 2.0) || !(0.1..=0.95).contains(&cfg.min_wick_ratio) {
            return Err("pivot_strength 1..20, touch_atr (0, 2], min_wick_ratio 0.1..0.95".into());
        }
        check_atr(cfg.atr_period, cfg.sl_atr, cfg.tp_atr)?;
        let meta = meta(
            "sr_rejection",
            "Rejet support / résistance",
            format!("Mèche de rejet sur un niveau pivot ({} bougies de chaque côté)", cfg.pivot_strength),
            vec![Regime::Range, Regime::TrendUp, Regime::TrendDown],
            100,
            [
                format!("Achat si la mèche basse (≥ {:.0} % de la bougie) touche un support à moins de {} ATR et que la clôture repart au-dessus; vente symétrique sur résistance", cfg.min_wick_ratio * 100.0, cfg.touch_atr),
                "Sortie si la clôture traverse le niveau rejeté".into(),
                format!("Stop-loss à {} x ATR({})", cfg.sl_atr, cfg.atr_period),
                format!("Take-profit à {} x ATR({})", cfg.tp_atr, cfg.atr_period),
            ],
        );
        Ok(SrRejection { cfg, meta })
    }
}

impl Strategy for SrRejection {
    strategy_boilerplate!(SrRejection);
    fn evaluate(&self, bars: &[Candle], open: Option<&OpenView>) -> StrategyAction {
        if bars.len() < self.meta.min_bars {
            return StrategyAction::None;
        }
        let last = bars[bars.len() - 1];
        let Some(a) = atr(bars, self.cfg.atr_period).filter(|a| *a > 0.0) else { return StrategyAction::None };
        // Levels from the bars BEFORE the current one, so the current bar cannot create its own level.
        let (res, sup) = swing_levels(&bars[..bars.len() - 1], self.cfg.pivot_strength, 5);
        if let Some(pos) = open {
            let level_lost = match pos.side {
                Side::Buy => sup.iter().any(|l| (pos.entry_price - l).abs() < 2.0 * a && last.close < *l),
                Side::Sell => res.iter().any(|l| (pos.entry_price - l).abs() < 2.0 * a && last.close > *l),
            };
            return if level_lost { StrategyAction::Exit { reason: "clôture au-delà du niveau rejeté".into() } } else { StrategyAction::None };
        }
        let range = last.high - last.low;
        if range <= 0.0 {
            return StrategyAction::None;
        }
        let lower_wick = last.open.min(last.close) - last.low;
        let upper_wick = last.high - last.open.max(last.close);
        let tol = self.cfg.touch_atr * a;
        if lower_wick / range >= self.cfg.min_wick_ratio {
            if let Some(l) = sup.iter().find(|l| (last.low - **l).abs() <= tol && last.close > **l) {
                return intent(Side::Buy, a, self.cfg.sl_atr, self.cfg.tp_atr, format!("rejet du support {l:.5} (mèche basse {:.0} % de la bougie)", lower_wick / range * 100.0), vec![("level", *l), ("wick_ratio", lower_wick / range)]);
            }
        }
        if upper_wick / range >= self.cfg.min_wick_ratio {
            if let Some(l) = res.iter().find(|l| (last.high - **l).abs() <= tol && last.close < **l) {
                return intent(Side::Sell, a, self.cfg.sl_atr, self.cfg.tp_atr, format!("rejet de la résistance {l:.5} (mèche haute {:.0} % de la bougie)", upper_wick / range * 100.0), vec![("level", *l), ("wick_ratio", upper_wick / range)]);
            }
        }
        StrategyAction::None
    }
}

// --------------------------------------------------------- Volatility expansion

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VolatilityExpansionConfig {
    pub atr_period: usize,
    pub baseline_period: usize,
    /// Compression: ATR / baseline ATR below this on the previous bar.
    pub squeeze_ratio: f64,
    /// Expansion: the current bar's range above this many ATR.
    pub expansion_atr: f64,
    pub max_bars_held: usize,
    pub sl_atr: f64,
    pub tp_atr: f64,
}

impl Default for VolatilityExpansionConfig {
    fn default() -> Self {
        VolatilityExpansionConfig { atr_period: 14, baseline_period: 100, squeeze_ratio: 0.8, expansion_atr: 1.5, max_bars_held: 20, sl_atr: 1.5, tp_atr: 1.0 }
    }
}

/// Volatility-based: after a compression, a wide bar closing near its extreme starts a move.
pub struct VolatilityExpansion {
    cfg: VolatilityExpansionConfig,
    meta: StrategyMeta,
}

impl VolatilityExpansion {
    pub fn new(cfg: VolatilityExpansionConfig) -> Result<Self, String> {
        if cfg.baseline_period <= cfg.atr_period || cfg.baseline_period > 250 {
            return Err("baseline_period must exceed atr_period and be <= 250".into());
        }
        if !(cfg.squeeze_ratio > 0.1 && cfg.squeeze_ratio < 1.0) || !(cfg.expansion_atr > 0.5 && cfg.expansion_atr <= 10.0) || cfg.max_bars_held == 0 {
            return Err("squeeze_ratio (0.1, 1), expansion_atr (0.5, 10], max_bars_held > 0".into());
        }
        check_atr(cfg.atr_period, cfg.sl_atr, cfg.tp_atr)?;
        let meta = meta(
            "volatility_expansion",
            "Expansion de volatilité",
            format!("Compression ATR({})/ATR({}) < {} puis bougie > {} ATR", cfg.atr_period, cfg.baseline_period, cfg.squeeze_ratio, cfg.expansion_atr),
            vec![Regime::Range, Regime::TrendUp, Regime::TrendDown],
            cfg.baseline_period + 3,
            [
                "Achat (vente) quand une bougie large clôture dans son quart haut (bas) juste après une compression de volatilité".into(),
                format!("Sortie après {} bougies sans atteindre la cible", cfg.max_bars_held),
                format!("Stop-loss à {} x ATR({})", cfg.sl_atr, cfg.atr_period),
                format!("Take-profit à {} x ATR({})", cfg.tp_atr, cfg.atr_period),
            ],
        );
        Ok(VolatilityExpansion { cfg, meta })
    }
}

impl Strategy for VolatilityExpansion {
    strategy_boilerplate!(VolatilityExpansion);
    fn evaluate(&self, bars: &[Candle], open: Option<&OpenView>) -> StrategyAction {
        if bars.len() < self.meta.min_bars {
            return StrategyAction::None;
        }
        if let Some(pos) = open {
            return if pos.bars_held >= self.cfg.max_bars_held {
                StrategyAction::Exit { reason: format!("{} bougies sans atteindre la cible", pos.bars_held) }
            } else {
                StrategyAction::None
            };
        }
        let prev = &bars[..bars.len() - 1];
        let (Some(a_prev), Some(base_prev), Some(a)) = (atr(prev, self.cfg.atr_period), atr(prev, self.cfg.baseline_period), atr(bars, self.cfg.atr_period)) else {
            return StrategyAction::None;
        };
        if a_prev <= 0.0 || base_prev <= 0.0 || a <= 0.0 {
            return StrategyAction::None;
        }
        let ratio = a_prev / base_prev;
        let last = bars[bars.len() - 1];
        let range = last.high - last.low;
        if ratio >= self.cfg.squeeze_ratio || range < self.cfg.expansion_atr * a_prev {
            return StrategyAction::None;
        }
        let pos_in_range = (last.close - last.low) / range;
        let side = if pos_in_range >= 0.75 {
            Side::Buy
        } else if pos_in_range <= 0.25 {
            Side::Sell
        } else {
            return StrategyAction::None;
        };
        intent(side, a, self.cfg.sl_atr, self.cfg.tp_atr, format!("compression {ratio:.2} puis bougie de {:.1} ATR clôturant à {:.0} % de son range", range / a_prev, pos_in_range * 100.0), vec![("squeeze_ratio", ratio), ("range_atr", range / a_prev)])
    }
}
