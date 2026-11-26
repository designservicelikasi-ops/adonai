//! Market Regime, Multi-Timeframe and Opportunity Scanner inside the engine.
//!
//! On every closed bar of a signal timeframe, for every instrument, the engine reads all the
//! timeframes it has (H4 and D1 are rebuilt from its own UTC H1 bars), classifies the detailed
//! regime, detects the structure, scores the opportunities and evaluates them against the
//! calibrated probability and the costs. This runs in the engine, with or without the desktop.

use crate::engine::Engine;
use aegis_market::mtf::{read_tf, TfRead};
use aegis_market::regime::{detect_regime_detail, RegimeDetail, RegimeKind};
use aegis_market::scanner::{scan, Opportunity};
use aegis_market::structure::{analyze, StructureParams};
use aegis_strategy::window;
use aegis_types::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IntelligenceSection {
    /// Entries need a scanner opportunity of the same side with at least this score. 0 = off.
    pub min_opportunity_score: u8,
    /// Entries are refused when context, structure or confirmation timeframes oppose them.
    pub require_mtf_alignment: bool,
    /// No entry in a chaotic market.
    pub block_chaotic: bool,
    /// In a trend transition, the probability required is raised by this much. 0 = off.
    pub transition_probability_margin: f64,
    /// Opportunities kept for display.
    pub keep: usize,
}

impl Default for IntelligenceSection {
    fn default() -> Self {
        IntelligenceSection { min_opportunity_score: 0, require_mtf_alignment: false, block_chaotic: true, transition_probability_margin: 0.0, keep: 200 }
    }
}

impl IntelligenceSection {
    pub fn validate(&self) -> Result<(), String> {
        if self.min_opportunity_score > 100 || !(0.0..=0.3).contains(&self.transition_probability_margin) || !(10..=5_000).contains(&self.keep) {
            return Err("intelligence: min_opportunity_score within 0..100, transition_probability_margin within 0..0.3, keep within 10..5000".into());
        }
        Ok(())
    }
}

/// An opportunity with what the engine adds: probability (separate from the score), net
/// expectation after costs, and whether it is admissible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScannedOpportunity {
    #[serde(flatten)]
    pub opportunity: Opportunity,
    pub sl_distance: Option<f64>,
    pub tp_distance: Option<f64>,
    /// Calibrated P(TP before SL) for the scenario, or None with the reason.
    pub probability: Option<f64>,
    pub probability_source: String,
    pub expected_net: Option<f64>,
    pub costs: Option<f64>,
    pub admissible: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegimeChange {
    pub ts_ms: TimestampMs,
    pub symbol: String,
    pub timeframe: String,
    pub regime: String,
    pub previous: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ScannerView {
    pub opportunities: Vec<ScannedOpportunity>,
    /// Latest reading of every timeframe, per instrument.
    pub mtf: Vec<(String, Vec<TfRead>)>,
    pub regimes: Vec<(String, String, RegimeDetail)>,
    pub regime_history: Vec<RegimeChange>,
    pub scans: u64,
    pub settings: IntelligenceSection,
}

#[derive(Default)]
pub(crate) struct ScannerState {
    pub opportunities: std::collections::VecDeque<ScannedOpportunity>,
    pub mtf: std::collections::BTreeMap<String, Vec<TfRead>>,
    pub regimes: std::collections::BTreeMap<(String, Timeframe), RegimeDetail>,
    pub history: std::collections::VecDeque<RegimeChange>,
    pub scans: u64,
}

impl Engine {
    /// Candles of `tf` for the analysis: native series, or H4/D1 rebuilt from the UTC H1 bars.
    fn analysis_candles(&self, symbol: &str, tf: Timeframe) -> Vec<Candle> {
        if self.market.config().timeframes.contains(&tf) {
            return window(&self.market.candles(symbol, tf)).to_vec();
        }
        if matches!(tf, Timeframe::H4 | Timeframe::D1) && self.market.config().timeframes.contains(&Timeframe::H1) {
            return aegis_market::aggregate(&self.market.candles(symbol, Timeframe::H1), Timeframe::H1, tf);
        }
        Vec::new()
    }

    pub(crate) fn mtf_reads(&self, symbol: &str) -> Vec<TfRead> {
        [Timeframe::D1, Timeframe::H4, Timeframe::H1, Timeframe::M30, Timeframe::M15, Timeframe::M5, Timeframe::M1]
            .into_iter()
            .filter(|tf| self.market.config().timeframes.contains(tf) || (matches!(tf, Timeframe::H4 | Timeframe::D1) && self.market.config().timeframes.contains(&Timeframe::H1)))
            .map(|tf| read_tf(tf, &self.analysis_candles(symbol, tf)))
            .collect()
    }

    /// Detailed regime with the conditions the engine knows about (data quality, news).
    pub(crate) fn regime_detail(&self, symbol: &str, tf: Timeframe, now: TimestampMs) -> RegimeDetail {
        let mut d = detect_regime_detail(&self.analysis_candles(symbol, tf));
        if self.market.quality(symbol, now).iter().any(|i| i.blocks_trading()) {
            d.flags.push("conditions_anormales".into());
        }
        if self.news_assessment(symbol, now).recommendation != aegis_news::Recommendation::Allowed {
            d.flags.push("annonce_proche".into());
        }
        d
    }

    /// Scan one instrument on a closed bar of a signal timeframe.
    pub(crate) async fn scan_bar(&mut self, symbol: &str, tf: Timeframe) {
        let now = now_ms();
        let candles = self.analysis_candles(symbol, tf);
        let regime = self.regime_detail(symbol, tf, now);
        // Regime history: one row per change.
        let key = (symbol.to_string(), tf);
        let previous = self.scanner.regimes.get(&key).map(|r| r.kind);
        if previous != Some(regime.kind) && regime.kind != RegimeKind::Unknown {
            let ch = RegimeChange { ts_ms: now, symbol: symbol.into(), timeframe: tf.as_str().into(), regime: regime.kind.as_str().into(), previous: previous.map(|p| p.as_str().to_string()).unwrap_or_default(), detail: regime.describe() };
            if self.db_ok {
                let _ = self.store.insert_regime_change(now, symbol, tf.as_str(), &ch.regime, &ch.previous, &ch.detail).await;
            }
            self.scanner.history.push_front(ch);
            self.scanner.history.truncate(500);
        }
        self.scanner.regimes.insert(key, regime.clone());
        let reads = self.mtf_reads(symbol);
        self.scanner.mtf.insert(symbol.to_string(), reads.clone());
        self.scanner.scans += 1;
        let report = analyze(&candles, &StructureParams::default());
        let Some((tick, _)) = self.market.last_tick(symbol) else { return };
        let found = scan(symbol, tf, now, &report, &regime, &reads, tick.spread());
        for o in found {
            let scanned = self.evaluate_opportunity(o, &regime, tick);
            self.scanner.opportunities.push_front(scanned);
        }
        let keep = self.cfg.intelligence.keep;
        self.scanner.opportunities.truncate(keep);
    }

    fn evaluate_opportunity(&self, o: Opportunity, regime: &RegimeDetail, tick: Tick) -> ScannedOpportunity {
        let pc = &self.cfg.probability;
        let atr = aegis_market::indicators::atr(&self.analysis_candles(&o.symbol, o.timeframe), 14);
        let (sl, tp) = match atr {
            Some(a) if a > 0.0 => (Some(pc.display_sl_atr * a), Some(pc.display_tp_atr * a)),
            _ => (None, None),
        };
        let (probability, source) = match (sl, tp) {
            (Some(sl), Some(tp)) => {
                let (p, s) = self.scenario_probability(&o.symbol, o.timeframe, o.side, tp, sl);
                (p, format!("TP {} ATR / SL {} ATR — {s}", pc.display_tp_atr, pc.display_sl_atr))
            }
            _ => (None, "ATR indisponible".into()),
        };
        let rc = self.risk.config();
        let mut net = None;
        let mut costs = None;
        if let (Some(spec), Some(acc), Some(sl), Some(tp)) = (self.specs.get(&o.symbol), self.account.as_ref(), sl, tp) {
            let entry = if o.side == Side::Buy { tick.ask } else { tick.bid };
            let (slp, tpp) = (entry - o.side.sign() * sl, entry + o.side.sign() * tp);
            let vol = aegis_risk::size_volume(spec, rc, acc.equity, entry, slp);
            if vol > 0.0 {
                let m = aegis_risk::evaluate_micro_profit(spec, rc, o.side, vol, entry, slp, tpp, tick.spread(), probability, &source);
                net = m.expected_net;
                costs = Some(m.costs.total());
            }
        }
        let min_p = if o.side == Side::Buy { rc.buy_min_probability } else { rc.sell_min_probability };
        let mut reasons = Vec::new();
        if !o.alignment.admissible {
            reasons.push(format!("contradictions multi-unités: {}", o.alignment.contradictions.join(" ; ")));
        }
        if regime.kind == RegimeKind::Chaotic {
            reasons.push("marché désordonné".into());
        }
        if regime.flags.iter().any(|f| f == "conditions_anormales") {
            reasons.push("conditions de marché anormales".into());
        }
        match probability {
            None => reasons.push(format!("aucune probabilité calibrée ({source})")),
            Some(p) if p < min_p => reasons.push(format!("P(TP avant SL) {:.0} % sous le seuil {:.0} %", p * 100.0, min_p * 100.0)),
            _ => {}
        }
        match net {
            Some(n) if n < rc.min_net_expected_profit || n <= 0.0 => reasons.push(format!("espérance nette {n:.2} insuffisante")),
            None => reasons.push("espérance nette non calculable".into()),
            _ => {}
        }
        if self.cfg.intelligence.min_opportunity_score > 0 && o.score < self.cfg.intelligence.min_opportunity_score {
            reasons.push(format!("score {} sous le minimum {}", o.score, self.cfg.intelligence.min_opportunity_score));
        }
        ScannedOpportunity { admissible: reasons.is_empty(), opportunity: o, sl_distance: sl, tp_distance: tp, probability, probability_source: source, expected_net: net, costs, reasons }
    }

    /// Checks added to an entry decision by the market intelligence.
    pub(crate) fn intelligence_checks(&self, symbol: &str, tf: Timeframe, side: Side, now: TimestampMs) -> (Vec<aegis_risk::Check>, RegimeDetail, f64) {
        let ic = &self.cfg.intelligence;
        let regime = self.scanner.regimes.get(&(symbol.to_string(), tf)).cloned().unwrap_or_else(|| self.regime_detail(symbol, tf, now));
        let mut checks = Vec::new();
        let chaotic = regime.kind == RegimeKind::Chaotic;
        checks.push(aegis_risk::Check {
            name: "regime_detaille".into(),
            passed: !(chaotic && ic.block_chaotic),
            detail: if chaotic && ic.block_chaotic { format!("{} — aucune entrée", regime.describe()) } else { regime.describe() },
        });
        let reads = self.scanner.mtf.get(symbol).cloned().unwrap_or_else(|| self.mtf_reads(symbol));
        let a = aegis_market::mtf::align(&reads, side);
        checks.push(aegis_risk::Check {
            name: "multi_unites".into(),
            passed: a.admissible || !ic.require_mtf_alignment,
            detail: if a.contradictions.is_empty() {
                format!("alignement {:.0}/100, aucune contradiction", a.score)
            } else {
                format!("alignement {:.0}/100; contradictions: {}{}", a.score, a.contradictions.join(" ; "), if ic.require_mtf_alignment { "" } else { " (information)" })
            },
        });
        if ic.min_opportunity_score > 0 {
            let best = self.scanner.opportunities.iter().filter(|o| o.opportunity.symbol == symbol && o.opportunity.side == side && now - o.opportunity.ts_ms <= tf.millis()).map(|o| o.opportunity.score).max();
            checks.push(aegis_risk::Check {
                name: "score_configuration".into(),
                passed: best.is_some_and(|s| s >= ic.min_opportunity_score),
                detail: match best {
                    Some(s) => format!("score de configuration {s}/100 (minimum {}) — ce n'est pas une probabilité", ic.min_opportunity_score),
                    None => "aucune configuration détectée par le scanner pour ce sens".into(),
                },
            });
        }
        let margin = if regime.kind == RegimeKind::Transition { ic.transition_probability_margin } else { 0.0 };
        (checks, regime, margin)
    }

    pub(crate) fn scanner_view(&self) -> ScannerView {
        ScannerView {
            opportunities: self.scanner.opportunities.iter().take(100).cloned().collect(),
            mtf: self.scanner.mtf.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            regimes: self.scanner.regimes.iter().map(|((s, tf), r)| (s.clone(), tf.as_str().to_string(), r.clone())).collect(),
            regime_history: self.scanner.history.iter().take(100).cloned().collect(),
            scans: self.scanner.scans,
            settings: self.cfg.intelligence.clone(),
        }
    }
}
