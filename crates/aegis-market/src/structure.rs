//! Market structure and setup detection — every concept is defined technically here and
//! computed ONLY on completed candles (oldest first, the bar in progress is never passed).
//!
//! Definitions (k = swing strength, ATR = ATR(14) of the series):
//! - **Swing high / low**: a bar whose high (low) is the extreme of the k bars on each side.
//!   Confirmed only once k later bars exist (no look-ahead).
//! - **HH / LH**: a swing high above / below the previous swing high. **HL / LL** likewise for lows.
//! - **Structure bias**: bullish when the last two swing highs AND lows rise (HH + HL),
//!   bearish when both fall (LH + LL), neutral otherwise.
//! - **BOS (break of structure)**: the last close is beyond the last confirmed swing high
//!   (bullish) / low (bearish) IN the direction of the bias: continuation.
//! - **CHoCH (change of character)**: the same break AGAINST the bias: potential reversal.
//! - **Breakout**: the last close is beyond the highest high / lowest low of the previous
//!   `range` bars. **False breakout**: the last bar pierced that level but closed back inside.
//! - **Retest**: a breakout happened in the last `retest_bars` bars, the last bar came back
//!   within 0.25 ATR of the broken level and closed on the breakout side.
//! - **Pullback**: bias bullish (bearish), the last low (high) touched the EMA(20) zone
//!   (± 0.25 ATR) and the bar closed back above (below) it.
//! - **Rejection**: a wick of at least 60 % of the bar range against a swing level within 0.3 ATR.
//! - **Divergence**: price makes HH (LL) between the last two swing highs (lows) while RSI(14)
//!   at those bars makes a lower high (higher low): bearish (bullish).
//! - **Compression / expansion**: ATR(14) / median true range of the last 100 bars below 0.7 /
//!   above 1.5.
//! - **Liquidity zone**: at least two swing highs (lows) within 0.15 ATR of each other (equal
//!   highs / lows), above (below) the price.
//! - **Displacement**: the last bar's body is at least 1.8 ATR.
//! - **Imbalance (fair value gap)**: bars i-2 and i do not overlap (low[i] > high[i-2], bullish;
//!   high[i] < low[i-2], bearish), not filled since.

use crate::indicators::{atr, ema, median, rsi, true_range};
use aegis_types::{Candle, Side};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwingKind {
    High,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwingLabel {
    HigherHigh,
    LowerHigh,
    HigherLow,
    LowerLow,
    First,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Swing {
    pub index: usize,
    pub ts_ms: i64,
    pub price: f64,
    pub kind: SwingKind,
    pub label: SwingLabel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bias {
    Bullish,
    Bearish,
    Neutral,
}

impl Bias {
    pub fn side(self) -> Option<Side> {
        match self {
            Bias::Bullish => Some(Side::Buy),
            Bias::Bearish => Some(Side::Sell),
            Bias::Neutral => None,
        }
    }
    pub fn opposes(self, side: Side) -> bool {
        self.side().is_some_and(|s| s != side)
    }
    pub fn label(self) -> &'static str {
        match self {
            Bias::Bullish => "haussier",
            Bias::Bearish => "baissier",
            Bias::Neutral => "neutre",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SetupKind {
    Breakout,
    FalseBreakout,
    Pullback,
    Retest,
    Rejection,
    BreakOfStructure,
    ChangeOfCharacter,
    Divergence,
    Compression,
    Expansion,
    LiquidityZone,
    Displacement,
    Imbalance,
}

impl SetupKind {
    pub fn label(self) -> &'static str {
        match self {
            SetupKind::Breakout => "cassure",
            SetupKind::FalseBreakout => "fausse cassure",
            SetupKind::Pullback => "repli",
            SetupKind::Retest => "retest",
            SetupKind::Rejection => "rejet",
            SetupKind::BreakOfStructure => "BOS",
            SetupKind::ChangeOfCharacter => "CHoCH",
            SetupKind::Divergence => "divergence",
            SetupKind::Compression => "compression",
            SetupKind::Expansion => "expansion",
            SetupKind::LiquidityZone => "zone de liquidité",
            SetupKind::Displacement => "déplacement rapide",
            SetupKind::Imbalance => "déséquilibre",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Setup {
    pub kind: SetupKind,
    /// Direction the setup points to (None for compression / expansion).
    pub side: Option<Side>,
    /// 0..1, how clear the configuration is (NOT a probability).
    pub strength: f64,
    pub level: Option<f64>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructureParams {
    pub swing_strength: usize,
    pub range_bars: usize,
    pub retest_bars: usize,
}

impl Default for StructureParams {
    fn default() -> Self {
        StructureParams { swing_strength: 3, range_bars: 20, retest_bars: 8 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructureReport {
    pub bars: usize,
    pub bias: Bias,
    pub swings: Vec<Swing>,
    pub atr: Option<f64>,
    pub volatility_ratio: Option<f64>,
    pub setups: Vec<Setup>,
}

/// Confirmed swings, oldest first, labelled against the previous swing of the same kind.
pub fn swings(candles: &[Candle], k: usize) -> Vec<Swing> {
    let mut out: Vec<Swing> = Vec::new();
    if k == 0 || candles.len() < 2 * k + 1 {
        return out;
    }
    let (mut last_high, mut last_low): (Option<f64>, Option<f64>) = (None, None);
    for i in k..candles.len() - k {
        let w = &candles[i - k..=i + k];
        let c = &candles[i];
        // Strict on the left, non-strict on the right: a flat top is counted once.
        let left = &candles[i - k..i];
        if left.iter().all(|x| x.high < c.high) && w.iter().all(|x| x.high <= c.high) {
            let label = match last_high {
                None => SwingLabel::First,
                Some(p) if c.high > p => SwingLabel::HigherHigh,
                Some(_) => SwingLabel::LowerHigh,
            };
            last_high = Some(c.high);
            out.push(Swing { index: i, ts_ms: c.open_ts, price: c.high, kind: SwingKind::High, label });
        }
        if left.iter().all(|x| x.low > c.low) && w.iter().all(|x| x.low >= c.low) {
            let label = match last_low {
                None => SwingLabel::First,
                Some(p) if c.low > p => SwingLabel::HigherLow,
                Some(_) => SwingLabel::LowerLow,
            };
            last_low = Some(c.low);
            out.push(Swing { index: i, ts_ms: c.open_ts, price: c.low, kind: SwingKind::Low, label });
        }
    }
    out
}

pub fn bias_of(swings: &[Swing]) -> Bias {
    let last = |k: SwingKind| swings.iter().rev().find(|s| s.kind == k).map(|s| s.label);
    match (last(SwingKind::High), last(SwingKind::Low)) {
        (Some(SwingLabel::HigherHigh), Some(SwingLabel::HigherLow)) => Bias::Bullish,
        (Some(SwingLabel::LowerHigh), Some(SwingLabel::LowerLow)) => Bias::Bearish,
        _ => Bias::Neutral,
    }
}

fn volatility_ratio(candles: &[Candle]) -> Option<f64> {
    let a = atr(candles, 14)?;
    let w = &candles[candles.len().saturating_sub(101)..];
    let trs: Vec<f64> = (1..w.len()).map(|i| true_range(w[i - 1].close, &w[i])).collect();
    let m = median(&trs)?;
    (m > 0.0).then(|| a / m)
}

/// Full structure reading of a completed series.
pub fn analyze(candles: &[Candle], p: &StructureParams) -> StructureReport {
    let sw = swings(candles, p.swing_strength);
    let bias = bias_of(&sw);
    let a = atr(candles, 14);
    let vr = volatility_ratio(candles);
    let mut setups = Vec::new();
    if let (Some(a), Some(last)) = (a.filter(|a| *a > 0.0), candles.last()) {
        let n = candles.len();
        let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();

        // ---- BOS / CHoCH
        let last_high = sw.iter().rev().find(|s| s.kind == SwingKind::High);
        let last_low = sw.iter().rev().find(|s| s.kind == SwingKind::Low);
        if let Some(h) = last_high.filter(|h| last.close > h.price && n >= 2 && candles[n - 2].close <= h.price) {
            let kind = if bias == Bias::Bearish { SetupKind::ChangeOfCharacter } else { SetupKind::BreakOfStructure };
            setups.push(Setup { kind, side: Some(Side::Buy), strength: ((last.close - h.price) / a).clamp(0.0, 1.0), level: Some(h.price), detail: format!("clôture au-dessus du dernier swing haut {:.5} (structure {})", h.price, bias.label()) });
        }
        if let Some(l) = last_low.filter(|l| last.close < l.price && n >= 2 && candles[n - 2].close >= l.price) {
            let kind = if bias == Bias::Bullish { SetupKind::ChangeOfCharacter } else { SetupKind::BreakOfStructure };
            setups.push(Setup { kind, side: Some(Side::Sell), strength: ((l.price - last.close) / a).clamp(0.0, 1.0), level: Some(l.price), detail: format!("clôture sous le dernier swing bas {:.5} (structure {})", l.price, bias.label()) });
        }

        // ---- breakout / false breakout over the previous `range_bars`
        if n > p.range_bars + 1 {
            let prev = &candles[n - 1 - p.range_bars..n - 1];
            let hi = prev.iter().map(|c| c.high).fold(f64::MIN, f64::max);
            let lo = prev.iter().map(|c| c.low).fold(f64::MAX, f64::min);
            if last.close > hi {
                setups.push(Setup { kind: SetupKind::Breakout, side: Some(Side::Buy), strength: ((last.close - hi) / a).clamp(0.0, 1.0), level: Some(hi), detail: format!("clôture au-dessus du plus haut de {} bougies {hi:.5}", p.range_bars) });
            } else if last.high > hi {
                setups.push(Setup { kind: SetupKind::FalseBreakout, side: Some(Side::Sell), strength: ((last.high - last.close) / (last.high - last.low).max(1e-12)).clamp(0.0, 1.0), level: Some(hi), detail: format!("plus haut {hi:.5} percé puis clôture revenue dans le range") });
            }
            if last.close < lo {
                setups.push(Setup { kind: SetupKind::Breakout, side: Some(Side::Sell), strength: ((lo - last.close) / a).clamp(0.0, 1.0), level: Some(lo), detail: format!("clôture sous le plus bas de {} bougies {lo:.5}", p.range_bars) });
            } else if last.low < lo {
                setups.push(Setup { kind: SetupKind::FalseBreakout, side: Some(Side::Buy), strength: ((last.close - last.low) / (last.high - last.low).max(1e-12)).clamp(0.0, 1.0), level: Some(lo), detail: format!("plus bas {lo:.5} percé puis clôture revenue dans le range") });
            }
        }

        // ---- retest of a recent breakout level
        if n > p.range_bars + p.retest_bars + 2 {
            for back in 2..=p.retest_bars {
                let j = n - back; // index of the breakout bar
                let prev = &candles[j - p.range_bars..j];
                let hi = prev.iter().map(|c| c.high).fold(f64::MIN, f64::max);
                let lo = prev.iter().map(|c| c.low).fold(f64::MAX, f64::min);
                if candles[j].close > hi && (last.low - hi).abs() <= 0.25 * a && last.close > hi {
                    setups.push(Setup { kind: SetupKind::Retest, side: Some(Side::Buy), strength: 0.7, level: Some(hi), detail: format!("retour sur le niveau cassé {hi:.5} il y a {back} bougies, clôture au-dessus") });
                    break;
                }
                if candles[j].close < lo && (last.high - lo).abs() <= 0.25 * a && last.close < lo {
                    setups.push(Setup { kind: SetupKind::Retest, side: Some(Side::Sell), strength: 0.7, level: Some(lo), detail: format!("retour sur le niveau cassé {lo:.5} il y a {back} bougies, clôture en dessous") });
                    break;
                }
            }
        }

        // ---- pullback to EMA(20) in the direction of the structure
        if let Some(e) = ema(&closes, 20) {
            if bias == Bias::Bullish && last.low <= e + 0.25 * a && last.close > e {
                setups.push(Setup { kind: SetupKind::Pullback, side: Some(Side::Buy), strength: 0.6, level: Some(e), detail: format!("repli sur l'EMA20 {e:.5} dans une structure haussière") });
            }
            if bias == Bias::Bearish && last.high >= e - 0.25 * a && last.close < e {
                setups.push(Setup { kind: SetupKind::Pullback, side: Some(Side::Sell), strength: 0.6, level: Some(e), detail: format!("repli sur l'EMA20 {e:.5} dans une structure baissière") });
            }
        }

        // ---- rejection wick at a swing level
        let range = (last.high - last.low).max(1e-12);
        let upper = last.high - last.open.max(last.close);
        let lower = last.open.min(last.close) - last.low;
        if let Some(h) = sw.iter().rev().filter(|s| s.kind == SwingKind::High).find(|s| (last.high - s.price).abs() <= 0.3 * a) {
            if upper / range >= 0.6 {
                setups.push(Setup { kind: SetupKind::Rejection, side: Some(Side::Sell), strength: upper / range, level: Some(h.price), detail: format!("mèche haute {:.0} % sur la résistance {:.5}", upper / range * 100.0, h.price) });
            }
        }
        if let Some(l) = sw.iter().rev().filter(|s| s.kind == SwingKind::Low).find(|s| (last.low - s.price).abs() <= 0.3 * a) {
            if lower / range >= 0.6 {
                setups.push(Setup { kind: SetupKind::Rejection, side: Some(Side::Buy), strength: lower / range, level: Some(l.price), detail: format!("mèche basse {:.0} % sur le support {:.5}", lower / range * 100.0, l.price) });
            }
        }

        // ---- RSI divergence between the last two swings of a kind
        let rsi_at = |i: usize| rsi(&closes[..=i], 14);
        let pair = |k: SwingKind| {
            let v: Vec<&Swing> = sw.iter().filter(|s| s.kind == k).collect();
            (v.len() >= 2).then(|| (*v[v.len() - 2], *v[v.len() - 1]))
        };
        if let Some((a1, a2)) = pair(SwingKind::High) {
            if let (Some(r1), Some(r2)) = (rsi_at(a1.index), rsi_at(a2.index)) {
                if a2.price > a1.price && r2 < r1 - 2.0 {
                    setups.push(Setup { kind: SetupKind::Divergence, side: Some(Side::Sell), strength: ((r1 - r2) / 20.0).clamp(0.0, 1.0), level: Some(a2.price), detail: format!("plus haut plus haut, RSI plus bas ({r1:.0} → {r2:.0}): divergence baissière") });
                }
            }
        }
        if let Some((b1, b2)) = pair(SwingKind::Low) {
            if let (Some(r1), Some(r2)) = (rsi_at(b1.index), rsi_at(b2.index)) {
                if b2.price < b1.price && r2 > r1 + 2.0 {
                    setups.push(Setup { kind: SetupKind::Divergence, side: Some(Side::Buy), strength: ((r2 - r1) / 20.0).clamp(0.0, 1.0), level: Some(b2.price), detail: format!("plus bas plus bas, RSI plus haut ({r1:.0} → {r2:.0}): divergence haussière") });
                }
            }
        }

        // ---- volatility compression / expansion
        if let Some(v) = vr {
            if v < 0.7 {
                setups.push(Setup { kind: SetupKind::Compression, side: None, strength: (1.0 - v).clamp(0.0, 1.0), level: None, detail: format!("ATR à {:.0} % de la normale récente: compression", v * 100.0) });
            } else if v > 1.5 {
                setups.push(Setup { kind: SetupKind::Expansion, side: None, strength: ((v - 1.0) / 2.0).clamp(0.0, 1.0), level: None, detail: format!("ATR à {:.0} % de la normale récente: expansion", v * 100.0) });
            }
        }

        // ---- liquidity: equal highs above / equal lows below the price
        let tol = 0.15 * a;
        let highs: Vec<f64> = sw.iter().filter(|s| s.kind == SwingKind::High && s.price > last.close).map(|s| s.price).collect();
        let lows: Vec<f64> = sw.iter().filter(|s| s.kind == SwingKind::Low && s.price < last.close).map(|s| s.price).collect();
        let cluster = |v: &[f64]| -> Option<(f64, usize)> {
            v.iter().map(|x| (*x, v.iter().filter(|y| (*y - x).abs() <= tol).count())).filter(|(_, c)| *c >= 2).max_by_key(|(_, c)| *c)
        };
        if let Some((lvl, c)) = cluster(&highs) {
            setups.push(Setup { kind: SetupKind::LiquidityZone, side: Some(Side::Buy), strength: (c as f64 / 4.0).min(1.0), level: Some(lvl), detail: format!("{c} sommets égaux vers {lvl:.5} au-dessus du prix (liquidité)") });
        }
        if let Some((lvl, c)) = cluster(&lows) {
            setups.push(Setup { kind: SetupKind::LiquidityZone, side: Some(Side::Sell), strength: (c as f64 / 4.0).min(1.0), level: Some(lvl), detail: format!("{c} creux égaux vers {lvl:.5} sous le prix (liquidité)") });
        }

        // ---- displacement
        let body = (last.close - last.open).abs();
        if body >= 1.8 * a {
            let side = if last.close > last.open { Side::Buy } else { Side::Sell };
            setups.push(Setup { kind: SetupKind::Displacement, side: Some(side), strength: (body / a / 3.0).min(1.0), level: None, detail: format!("corps de {:.1} ATR: déplacement rapide", body / a) });
        }

        // ---- unfilled imbalance among the last 30 bars (most recent first)
        for i in (n.saturating_sub(30).max(2)..n).rev() {
            let (c0, c2) = (&candles[i - 2], &candles[i]);
            if c2.low > c0.high {
                let filled = candles[i + 1..].iter().any(|c| c.low <= c0.high);
                if !filled {
                    setups.push(Setup { kind: SetupKind::Imbalance, side: Some(Side::Buy), strength: ((c2.low - c0.high) / a).min(1.0), level: Some(c0.high), detail: format!("déséquilibre haussier non comblé {:.5}–{:.5}", c0.high, c2.low) });
                    break;
                }
            }
            if c2.high < c0.low {
                let filled = candles[i + 1..].iter().any(|c| c.high >= c0.low);
                if !filled {
                    setups.push(Setup { kind: SetupKind::Imbalance, side: Some(Side::Sell), strength: ((c0.low - c2.high) / a).min(1.0), level: Some(c0.low), detail: format!("déséquilibre baissier non comblé {:.5}–{:.5}", c2.high, c0.low) });
                    break;
                }
            }
        }
    }
    StructureReport { bars: candles.len(), bias, swings: sw, atr: a, volatility_ratio: vr, setups }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Candles from a close path; each bar spans `wick` around open/close.
    fn path(closes: &[f64], wick: f64) -> Vec<Candle> {
        closes
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let o = if i == 0 { *c } else { closes[i - 1] };
                Candle { open_ts: i as i64 * 60_000, open: o, high: o.max(*c) + wick, low: o.min(*c) - wick, close: *c, tick_volume: 1, spread_points: 0.0 }
            })
            .collect()
    }

    /// Zig-zag with rising highs and lows.
    fn uptrend() -> Vec<f64> {
        let mut v = Vec::new();
        let mut base = 1.1000;
        for _ in 0..8 {
            for i in 0..6 {
                v.push(base + i as f64 * 0.0004);
            }
            for i in 0..4 {
                v.push(base + 0.0020 - i as f64 * 0.0003);
            }
            base += 0.0010;
        }
        v
    }

    #[test]
    fn swings_are_confirmed_labelled_and_give_the_bias() {
        let c = path(&uptrend(), 0.00005);
        let sw = swings(&c, 2);
        assert!(sw.len() >= 6);
        assert!(sw.iter().all(|s| s.index + 2 < c.len()), "only confirmed swings");
        assert_eq!(bias_of(&sw), Bias::Bullish);
        let down: Vec<f64> = uptrend().iter().map(|x| 2.2 - x).collect();
        assert_eq!(bias_of(&swings(&path(&down, 0.00005), 2)), Bias::Bearish);
        // Adding a bar never changes an already-confirmed swing (no look-ahead).
        let longer = swings(&path(&[uptrend(), vec![1.2]].concat(), 0.00005), 2);
        for s in &sw {
            assert!(longer.contains(s));
        }
    }

    #[test]
    fn breakout_and_false_breakout() {
        let mut v = vec![1.1000; 30];
        for (i, x) in v.iter_mut().enumerate() {
            *x += if i % 2 == 0 { 0.0003 } else { -0.0003 };
        }
        let mut up = v.clone();
        up.push(1.1020);
        let r = analyze(&path(&up, 0.0001), &StructureParams::default());
        assert!(r.setups.iter().any(|s| s.kind == SetupKind::Breakout && s.side == Some(Side::Buy)), "{:?}", r.setups);
        // A bar that pierces the high but closes back inside.
        let mut c = path(&v, 0.0001);
        let last = *c.last().unwrap();
        c.push(Candle { open_ts: last.open_ts + 60_000, open: 1.1000, high: 1.1012, low: 1.0998, close: 1.1001, ..last });
        let r = analyze(&c, &StructureParams::default());
        assert!(r.setups.iter().any(|s| s.kind == SetupKind::FalseBreakout && s.side == Some(Side::Sell)), "{:?}", r.setups);
    }

    #[test]
    fn bos_in_the_trend_and_choch_against_it() {
        let mut v = uptrend();
        let top = v.iter().cloned().fold(f64::MIN, f64::max);
        v.push(top + 0.0010);
        let r = analyze(&path(&v, 0.00005), &StructureParams { swing_strength: 2, ..Default::default() });
        assert!(r.setups.iter().any(|s| s.kind == SetupKind::BreakOfStructure && s.side == Some(Side::Buy)), "{:?}", r.setups);
        // In the same bullish structure, a close below the last swing low is a change of character.
        let mut v = uptrend();
        v.push(1.0990);
        let r = analyze(&path(&v, 0.00005), &StructureParams { swing_strength: 2, ..Default::default() });
        assert_eq!(r.bias, Bias::Bullish);
        assert!(r.setups.iter().any(|s| s.kind == SetupKind::ChangeOfCharacter && s.side == Some(Side::Sell)), "{:?}", r.setups);
    }

    #[test]
    fn compression_expansion_displacement_and_imbalance() {
        let mut v: Vec<f64> = (0..110).map(|i| 1.1 + if i % 2 == 0 { 0.0010 } else { -0.0010 }).collect();
        v.extend((0..20).map(|i| 1.1 + if i % 2 == 0 { 0.0001 } else { -0.0001 }));
        let r = analyze(&path(&v, 0.00002), &StructureParams::default());
        assert!(r.setups.iter().any(|s| s.kind == SetupKind::Compression), "{:?}", r.volatility_ratio);
        let mut w: Vec<f64> = (0..110).map(|i| 1.1 + if i % 2 == 0 { 0.0001 } else { -0.0001 }).collect();
        w.extend([1.1010, 1.1030, 1.1060]);
        let r = analyze(&path(&w, 0.00002), &StructureParams::default());
        assert!(r.setups.iter().any(|s| s.kind == SetupKind::Expansion));
        assert!(r.setups.iter().any(|s| s.kind == SetupKind::Displacement && s.side == Some(Side::Buy)));
        assert!(r.setups.iter().any(|s| s.kind == SetupKind::Imbalance && s.side == Some(Side::Buy)), "{:?}", r.setups);
    }

    #[test]
    fn divergence_and_equal_highs() {
        // Two pushes up: the second higher but slower (RSI lower).
        let mut v: Vec<f64> = (0..30).map(|i| 1.1 + i as f64 * 0.0005).collect(); // strong push
        v.extend((0..6).map(|i| 1.1145 - i as f64 * 0.0004)); // pullback
        v.extend((0..20).map(|i| 1.1125 + i as f64 * 0.00013)); // weak push to a slightly higher high
        v.extend((0..6).map(|i| 1.1150 - i as f64 * 0.0004));
        let r = analyze(&path(&v, 0.00003), &StructureParams { swing_strength: 3, ..Default::default() });
        assert!(r.setups.iter().any(|s| s.kind == SetupKind::Divergence && s.side == Some(Side::Sell)), "{:?}", r.swings);
        // Equal highs above the price.
        let mut e: Vec<f64> = Vec::new();
        for _ in 0..4 {
            e.extend([1.1000, 1.1005, 1.1010, 1.1005, 1.1000]);
        }
        e.extend([1.0995, 1.0990]);
        let r = analyze(&path(&e, 0.00002), &StructureParams { swing_strength: 2, ..Default::default() });
        assert!(r.setups.iter().any(|s| s.kind == SetupKind::LiquidityZone && s.side == Some(Side::Buy)), "{:?}", r.setups);
    }

    #[test]
    fn retest_pullback_and_rejection() {
        // Range, breakout bar, two bars higher, then a bar back to the broken level closing above it.
        let mut v: Vec<f64> = (0..30).map(|i| 1.1 + if i % 2 == 0 { 0.0003 } else { -0.0003 }).collect();
        v.extend([1.1015, 1.1020, 1.1018]);
        let mut c = path(&v, 0.0001);
        let last = *c.last().unwrap();
        let broken = c[c.len() - 4..c.len() - 3][0].high.max(1.1004); // previous range high
        c.push(Candle { open_ts: last.open_ts + 60_000, open: 1.1014, high: 1.1016, low: broken + 0.00002, close: 1.1012, ..last });
        let r = analyze(&c, &StructureParams::default());
        assert!(r.setups.iter().any(|s| s.kind == SetupKind::Retest && s.side == Some(Side::Buy)), "{:?}", r.setups);

        // Pullback: bullish structure, last bar dips to the EMA20 and closes above it.
        let mut c = path(&uptrend(), 0.00005);
        let closes: Vec<f64> = c.iter().map(|x| x.close).collect();
        let e = crate::indicators::ema(&closes, 20).unwrap();
        let last = *c.last().unwrap();
        c.push(Candle { open_ts: last.open_ts + 60_000, open: e + 0.0004, high: e + 0.0006, low: e - 0.00002, close: e + 0.0003, ..last });
        let r = analyze(&c, &StructureParams { swing_strength: 2, ..Default::default() });
        assert_eq!(r.bias, Bias::Bullish);
        assert!(r.setups.iter().any(|s| s.kind == SetupKind::Pullback), "{:?}", r.setups);

        // Rejection: a long lower wick on the last swing low.
        let mut e: Vec<f64> = Vec::new();
        for _ in 0..4 {
            e.extend([1.1010, 1.1005, 1.1000, 1.1005, 1.1010]);
        }
        let mut c = path(&e, 0.00002);
        let last = *c.last().unwrap();
        c.push(Candle { open_ts: last.open_ts + 60_000, open: 1.1009, high: 1.1010, low: 1.09998, close: 1.1008, ..last });
        let r = analyze(&c, &StructureParams { swing_strength: 2, ..Default::default() });
        assert!(r.setups.iter().any(|s| s.kind == SetupKind::Rejection && s.side == Some(Side::Buy)), "{:?}", r.setups);
    }
}
