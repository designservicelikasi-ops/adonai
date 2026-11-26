//! Opportunity Scanner: scores configurations found on completed candles.
//!
//! The **score** (0..100) says how clean a configuration looks; it is NOT a probability and
//! is never presented as one. The probability of reaching the target before the stop comes
//! only from a calibrated model (Probability Engine), and the engine adds it separately.
//!
//! Score components (all explained):
//! - configuration (max 30): strength of the setups pointing to the side, by kind;
//! - multi-timeframe (max 30): the alignment score x 0.3;
//! - regime fit (max 20): continuation setups in a trend, reversal setups in a range;
//!   0 in a chaotic market, reduced in a transition;
//! - cost (max 10): spread relative to ATR;
//! - volatility (max 10): normal volatility best, extremes penalised;
//! - minus 10 per setup on the same timeframe pointing the other way.

use crate::mtf::{align, Alignment, TfRead};
use crate::regime::{RegimeDetail, RegimeKind, VolatilityLevel};
use crate::structure::{Setup, SetupKind, StructureReport};
use aegis_types::{Side, Timeframe, TimestampMs};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreComponent {
    pub name: String,
    pub points: f64,
    pub max: f64,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Opportunity {
    pub id: String,
    pub ts_ms: TimestampMs,
    pub symbol: String,
    pub timeframe: Timeframe,
    pub side: Side,
    /// 0..100 configuration score — NOT a probability.
    pub score: u8,
    pub components: Vec<ScoreComponent>,
    pub setups: Vec<Setup>,
    pub regime: RegimeDetail,
    pub alignment: Alignment,
    pub contradictions: Vec<String>,
}

fn kind_weight(k: SetupKind) -> f64 {
    match k {
        SetupKind::BreakOfStructure | SetupKind::Retest | SetupKind::Pullback => 1.0,
        SetupKind::Breakout | SetupKind::ChangeOfCharacter | SetupKind::FalseBreakout => 0.85,
        SetupKind::Rejection | SetupKind::Divergence | SetupKind::Displacement => 0.7,
        SetupKind::Imbalance | SetupKind::LiquidityZone => 0.4,
        SetupKind::Compression | SetupKind::Expansion => 0.0,
    }
}

fn is_continuation(k: SetupKind) -> bool {
    matches!(k, SetupKind::BreakOfStructure | SetupKind::Retest | SetupKind::Pullback | SetupKind::Breakout | SetupKind::Displacement)
}

/// Candidate opportunities on `timeframe` (one per side that has a directional setup).
pub fn scan(symbol: &str, timeframe: Timeframe, ts_ms: TimestampMs, report: &StructureReport, regime: &RegimeDetail, reads: &[TfRead], spread: f64) -> Vec<Opportunity> {
    let mut out = Vec::new();
    for side in [Side::Buy, Side::Sell] {
        let mine: Vec<Setup> = report.setups.iter().filter(|s| s.side == Some(side) && kind_weight(s.kind) > 0.0).cloned().collect();
        if mine.is_empty() {
            continue;
        }
        let against: Vec<&Setup> = report.setups.iter().filter(|s| s.side == Some(side.opposite()) && kind_weight(s.kind) >= 0.7).collect();
        let mut comps = Vec::new();
        let setup_pts = (mine.iter().map(|s| kind_weight(s.kind) * (0.5 + 0.5 * s.strength)).sum::<f64>() * 18.0).min(30.0);
        comps.push(ScoreComponent { name: "configuration".into(), points: setup_pts, max: 30.0, detail: mine.iter().map(|s| s.kind.label()).collect::<Vec<_>>().join(", ") });
        let alignment = align(reads, side);
        comps.push(ScoreComponent {
            name: "multi_unites".into(),
            points: alignment.score * 0.3,
            max: 30.0,
            detail: if alignment.contradictions.is_empty() { format!("alignement {:.0}/100", alignment.score) } else { format!("alignement {:.0}/100; contradictions: {}", alignment.score, alignment.contradictions.join(" ; ")) },
        });
        let continuation = mine.iter().any(|s| is_continuation(s.kind));
        let with_trend = matches!((regime.kind, side), (RegimeKind::TrendUp, Side::Buy) | (RegimeKind::TrendDown, Side::Sell));
        let (regime_pts, regime_why) = match regime.kind {
            RegimeKind::Chaotic => (0.0, "marché désordonné"),
            RegimeKind::Unknown => (0.0, "régime indéterminé"),
            RegimeKind::Transition => (6.0, "transition: prudence"),
            RegimeKind::TrendUp | RegimeKind::TrendDown if with_trend && continuation => (20.0, "continuation dans le sens de la tendance"),
            RegimeKind::TrendUp | RegimeKind::TrendDown if with_trend => (12.0, "dans le sens de la tendance"),
            RegimeKind::TrendUp | RegimeKind::TrendDown => (3.0, "contre la tendance"),
            RegimeKind::Range | RegimeKind::Consolidation if !continuation => (16.0, "retournement dans un marché latéral"),
            RegimeKind::Range | RegimeKind::Consolidation => (8.0, "continuation dans un marché latéral"),
        };
        comps.push(ScoreComponent { name: "regime".into(), points: regime_pts, max: 20.0, detail: format!("{}: {regime_why}", regime.kind.label()) });
        let cost_pts = match report.atr {
            Some(a) if a > 0.0 && spread >= 0.0 => {
                let r = spread / a;
                (10.0 * (1.0 - (r - 0.1) / 0.2)).clamp(0.0, 10.0)
            }
            _ => 0.0,
        };
        comps.push(ScoreComponent { name: "cout".into(), points: cost_pts, max: 10.0, detail: report.atr.map(|a| format!("spread = {:.0} % de l'ATR", spread / a * 100.0)).unwrap_or_else(|| "ATR indisponible".into()) });
        let vol_pts = match regime.volatility {
            VolatilityLevel::Normal => 10.0,
            VolatilityLevel::Low => 5.0,
            VolatilityLevel::High => 2.0,
        };
        comps.push(ScoreComponent { name: "volatilite".into(), points: vol_pts, max: 10.0, detail: format!("{:?}", regime.volatility).to_lowercase() });
        let penalty = against.len() as f64 * 10.0;
        if penalty > 0.0 {
            comps.push(ScoreComponent { name: "signaux_contraires".into(), points: -penalty, max: 0.0, detail: against.iter().map(|s| s.kind.label()).collect::<Vec<_>>().join(", ") });
        }
        let total: f64 = comps.iter().map(|c| c.points).sum();
        let mut contradictions = alignment.contradictions.clone();
        contradictions.extend(against.iter().map(|s| format!("{} {} sur {timeframe}", s.kind.label(), side.opposite())));
        out.push(Opportunity {
            id: format!("{symbol}-{timeframe}-{ts_ms}-{side}"),
            ts_ms,
            symbol: symbol.into(),
            timeframe,
            side,
            score: total.round().clamp(0.0, 100.0) as u8,
            components: comps,
            setups: mine,
            regime: regime.clone(),
            alignment,
            contradictions,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mtf::TfRole;
    use crate::regime::VolatilityDynamics;
    use crate::structure::Bias;

    fn report(setups: Vec<Setup>) -> StructureReport {
        StructureReport { bars: 200, bias: Bias::Bullish, swings: Vec::new(), atr: Some(0.0010), volatility_ratio: Some(1.0), setups }
    }
    fn setup(kind: SetupKind, side: Side) -> Setup {
        Setup { kind, side: Some(side), strength: 0.8, level: None, detail: String::new() }
    }
    fn regime(kind: RegimeKind) -> RegimeDetail {
        RegimeDetail { kind, volatility: VolatilityLevel::Normal, dynamics: VolatilityDynamics::Stable, efficiency: 0.4, volatility_ratio: 1.0, dynamics_ratio: 1.0, flags: Vec::new() }
    }
    fn read(tf: Timeframe, bias: Bias) -> TfRead {
        TfRead { timeframe: tf, role: TfRole::of(tf), bars: 200, available: true, bias, regime: regime(RegimeKind::TrendUp), setups: Vec::new() }
    }

    #[test]
    fn score_is_explained_and_bounded_and_rewards_clean_continuations() {
        let reads = vec![read(Timeframe::H4, Bias::Bullish), read(Timeframe::H1, Bias::Bullish), read(Timeframe::M15, Bias::Bullish), read(Timeframe::M5, Bias::Bullish)];
        let good = scan("EURUSD", Timeframe::M5, 0, &report(vec![setup(SetupKind::BreakOfStructure, Side::Buy), setup(SetupKind::Retest, Side::Buy)]), &regime(RegimeKind::TrendUp), &reads, 0.0001);
        assert_eq!(good.len(), 1);
        let g = &good[0];
        assert!(g.score >= 70, "{g:?}");
        assert!((g.components.iter().map(|c| c.points).sum::<f64>().round() as u8) == g.score);
        assert!(g.components.iter().all(|c| c.points <= c.max + 1e-9 || c.max == 0.0));
        // Same setup in a chaotic market with an opposing context and a contrary signal: much lower.
        let bad_reads = vec![read(Timeframe::H4, Bias::Bearish), read(Timeframe::H1, Bias::Bullish), read(Timeframe::M15, Bias::Bullish), read(Timeframe::M5, Bias::Bullish)];
        let bad = scan("EURUSD", Timeframe::M5, 0, &report(vec![setup(SetupKind::BreakOfStructure, Side::Buy), setup(SetupKind::Rejection, Side::Sell)]), &regime(RegimeKind::Chaotic), &bad_reads, 0.0004);
        let b = bad.iter().find(|o| o.side == Side::Buy).unwrap();
        assert!(b.score + 30 < g.score, "{} vs {}", b.score, g.score);
        assert!(!b.alignment.admissible && !b.contradictions.is_empty());
    }

    #[test]
    fn non_directional_setups_alone_make_no_opportunity() {
        let r = report(vec![Setup { kind: SetupKind::Compression, side: None, strength: 1.0, level: None, detail: String::new() }]);
        assert!(scan("EURUSD", Timeframe::M5, 0, &r, &regime(RegimeKind::Range), &[], 0.0001).is_empty());
    }
}
