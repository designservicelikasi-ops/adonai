//! Multi-timeframe intelligence.
//!
//! Each timeframe has a role: D1/H4 context, H1/M30 intermediate structure, M15 confirmation,
//! M5 setup preparation, M1 entry precision. Agreement is NOT a majority vote: an opposing
//! context or structure is a contradiction whatever the other timeframes say, and a timeframe
//! without enough data is "unavailable", never counted as an agreement.

use crate::indicators::{ema, efficiency_ratio};
use crate::regime::{detect_regime_detail, RegimeDetail, REGIME_MIN_BARS};
use crate::structure::{analyze, Bias, SetupKind, StructureParams};
use aegis_types::{Candle, Side, Timeframe};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TfRole {
    Context,
    Structure,
    Confirmation,
    Setup,
    Trigger,
}

impl TfRole {
    pub fn of(tf: Timeframe) -> TfRole {
        match tf {
            Timeframe::D1 | Timeframe::H4 => TfRole::Context,
            Timeframe::H1 | Timeframe::M30 => TfRole::Structure,
            Timeframe::M15 => TfRole::Confirmation,
            Timeframe::M5 => TfRole::Setup,
            Timeframe::M1 => TfRole::Trigger,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            TfRole::Context => "contexte",
            TfRole::Structure => "structure",
            TfRole::Confirmation => "confirmation",
            TfRole::Setup => "préparation",
            TfRole::Trigger => "précision d'entrée",
        }
    }
    /// Points of the alignment score (sum = 100).
    fn weight(self) -> f64 {
        match self {
            TfRole::Context => 25.0,
            TfRole::Structure => 25.0,
            TfRole::Confirmation => 20.0,
            TfRole::Setup => 15.0,
            TfRole::Trigger => 15.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TfRead {
    pub timeframe: Timeframe,
    pub role: TfRole,
    pub bars: usize,
    pub available: bool,
    pub bias: Bias,
    pub regime: RegimeDetail,
    pub setups: Vec<(SetupKind, Option<Side>)>,
}

/// Bias of one timeframe: the swing structure, or (when the structure is neutral) the EMA20/50
/// relation if the market moves efficiently enough.
pub fn read_tf(tf: Timeframe, candles: &[Candle]) -> TfRead {
    let available = candles.len() >= REGIME_MIN_BARS;
    let report = analyze(candles, &StructureParams::default());
    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
    let bias = match report.bias {
        Bias::Neutral if available => match (ema(&closes, 20), ema(&closes, 50), efficiency_ratio(&closes, 20)) {
            (Some(f), Some(s), Some(er)) if er >= 0.25 && f > s => Bias::Bullish,
            (Some(f), Some(s), Some(er)) if er >= 0.25 && f < s => Bias::Bearish,
            _ => Bias::Neutral,
        },
        b if available => b,
        _ => Bias::Neutral,
    };
    TfRead {
        timeframe: tf,
        role: TfRole::of(tf),
        bars: candles.len(),
        available,
        bias,
        regime: detect_regime_detail(candles),
        setups: report.setups.iter().map(|s| (s.kind, s.side)).collect(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Agrees,
    Neutral,
    Opposes,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TfVerdict {
    pub timeframe: Timeframe,
    pub role: TfRole,
    pub verdict: Verdict,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alignment {
    pub side: Side,
    pub verdicts: Vec<TfVerdict>,
    pub contradictions: Vec<String>,
    /// 0..100: agreement weighted by role; unavailable timeframes earn nothing.
    pub score: f64,
    /// No opposing context or structure, confirmation not opposed.
    pub admissible: bool,
}

pub fn align(reads: &[TfRead], side: Side) -> Alignment {
    let mut verdicts = Vec::new();
    let mut contradictions = Vec::new();
    let mut score = 0.0;
    let mut admissible = true;
    for r in reads {
        let w = r.role.weight() / reads.iter().filter(|x| x.role == r.role).count().max(1) as f64;
        let setup_for = r.setups.iter().any(|(_, s)| *s == Some(side));
        let (verdict, detail) = if !r.available {
            (Verdict::Unavailable, format!("{} bougies: données insuffisantes", r.bars))
        } else if r.bias.opposes(side) {
            (Verdict::Opposes, format!("structure {}", r.bias.label()))
        } else if r.bias.side() == Some(side) || (r.role == TfRole::Setup && setup_for) || (r.role == TfRole::Trigger && setup_for) {
            (Verdict::Agrees, if setup_for { format!("structure {}, configuration présente", r.bias.label()) } else { format!("structure {}", r.bias.label()) })
        } else {
            (Verdict::Neutral, "structure neutre".to_string())
        };
        score += match verdict {
            Verdict::Agrees => w,
            Verdict::Neutral => w * 0.4,
            _ => 0.0,
        };
        if verdict == Verdict::Opposes {
            let msg = format!("{} ({}) {}", r.timeframe, r.role.label(), detail);
            if matches!(r.role, TfRole::Context | TfRole::Structure | TfRole::Confirmation) {
                admissible = false;
            }
            contradictions.push(msg);
        }
        verdicts.push(TfVerdict { timeframe: r.timeframe, role: r.role, verdict, detail });
    }
    Alignment { side, verdicts, contradictions, score: score.clamp(0.0, 100.0), admissible }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(tf: Timeframe, bias: Bias, available: bool) -> TfRead {
        TfRead { timeframe: tf, role: TfRole::of(tf), bars: if available { 200 } else { 10 }, available, bias, regime: RegimeDetail::unknown(), setups: Vec::new() }
    }

    #[test]
    fn a_majority_does_not_override_an_opposing_context() {
        // Five timeframes bullish, but H4 (context) bearish: not admissible for a BUY.
        let reads = vec![
            read(Timeframe::H4, Bias::Bearish, true),
            read(Timeframe::H1, Bias::Bullish, true),
            read(Timeframe::M30, Bias::Bullish, true),
            read(Timeframe::M15, Bias::Bullish, true),
            read(Timeframe::M5, Bias::Bullish, true),
            read(Timeframe::M1, Bias::Bullish, true),
        ];
        let a = align(&reads, Side::Buy);
        assert!(!a.admissible);
        assert_eq!(a.contradictions.len(), 1);
        assert!(a.contradictions[0].contains("H4"));
        // An opposing M1 alone is a minor contradiction (entry timing), not a veto.
        let mut reads2 = reads.clone();
        reads2[0].bias = Bias::Bullish;
        reads2[5].bias = Bias::Bearish;
        let a = align(&reads2, Side::Buy);
        assert!(a.admissible && a.contradictions.len() == 1);
    }

    #[test]
    fn unavailable_timeframes_are_never_counted_as_agreement() {
        let full = align(&[read(Timeframe::D1, Bias::Bullish, true), read(Timeframe::H1, Bias::Bullish, true)], Side::Buy);
        let missing = align(&[read(Timeframe::D1, Bias::Bullish, false), read(Timeframe::H1, Bias::Bullish, true)], Side::Buy);
        assert!(missing.score < full.score);
        assert_eq!(missing.verdicts[0].verdict, Verdict::Unavailable);
        assert!(missing.admissible, "missing data is not a contradiction");
    }

    #[test]
    fn roles_follow_the_directive() {
        assert_eq!(TfRole::of(Timeframe::D1), TfRole::Context);
        assert_eq!(TfRole::of(Timeframe::M30), TfRole::Structure);
        assert_eq!(TfRole::of(Timeframe::M15), TfRole::Confirmation);
        assert_eq!(TfRole::of(Timeframe::M5), TfRole::Setup);
        assert_eq!(TfRole::of(Timeframe::M1), TfRole::Trigger);
    }
}
