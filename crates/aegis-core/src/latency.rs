//! Execution quality: what part of an order's delay is AEGIS, what part is the network +
//! terminal, what part is the broker, and how much slippage the fills cost.
//!
//! - internal: from the start of the decision to the frame leaving the engine (measured here);
//! - broker: time inside `OrderSend`, measured by the connector (protocol 1.2, `broker_us`);
//! - network: round trip minus broker time (network, terminal queue, connector).
//!
//! The speed of the Rust engine is never presented as market execution speed: the three are
//! reported separately, and the broker part is "not reported" for connectors older than 1.2.

use aegis_types::TimestampMs;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

const KEEP: usize = 300;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LatencySample {
    pub cid: String,
    pub symbol: String,
    pub action: String,
    pub sent_ms: TimestampMs,
    pub internal_us: Option<i64>,
    pub roundtrip_ms: Option<i64>,
    pub broker_ms: Option<f64>,
    pub network_ms: Option<f64>,
    pub ok: Option<bool>,
    /// Adverse slippage of the fill in points (positive = cost).
    pub slippage_points: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Quantiles {
    pub count: usize,
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub max: Option<f64>,
}

fn quantiles(mut v: Vec<f64>) -> Quantiles {
    v.retain(|x| x.is_finite());
    if v.is_empty() {
        return Quantiles::default();
    }
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    Quantiles { count: v.len(), p50: Some(at(0.5)), p95: Some(at(0.95)), max: v.last().copied() }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SymbolSlippage {
    pub symbol: String,
    pub fills: usize,
    pub median_points: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ExecutionQuality {
    pub internal_us: Quantiles,
    pub roundtrip_ms: Quantiles,
    pub broker_ms: Quantiles,
    pub network_ms: Quantiles,
    pub slippage_points: Quantiles,
    pub by_symbol: Vec<SymbolSlippage>,
    pub rejected: usize,
    /// Results that carried no broker time (connector older than protocol 1.2).
    pub broker_time_missing: usize,
    pub recent: Vec<LatencySample>,
}

#[derive(Default)]
pub struct LatencyTracker {
    open: HashMap<String, LatencySample>,
    done: VecDeque<LatencySample>,
}

impl LatencyTracker {
    pub fn on_sent(&mut self, cid: &str, symbol: &str, action: &str, internal_us: Option<i64>, sent_ms: TimestampMs) {
        if cid.is_empty() {
            return;
        }
        self.open.insert(cid.into(), LatencySample { cid: cid.into(), symbol: symbol.into(), action: action.into(), sent_ms, internal_us, ..Default::default() });
        if self.open.len() > 1_000 {
            // Instructions never answered: keep the most recent ones only.
            let mut old: Vec<(String, TimestampMs)> = self.open.iter().map(|(k, v)| (k.clone(), v.sent_ms)).collect();
            old.sort_by_key(|x| x.1);
            for (k, _) in old.into_iter().take(500) {
                self.open.remove(&k);
            }
        }
    }

    pub fn on_result(&mut self, cid: &str, ok: bool, broker_us: i64, received_ms: TimestampMs) {
        let Some(mut s) = self.open.remove(cid) else { return };
        let rt = (received_ms - s.sent_ms).max(0);
        s.roundtrip_ms = Some(rt);
        s.ok = Some(ok);
        if broker_us > 0 {
            let b = broker_us as f64 / 1_000.0;
            s.broker_ms = Some(b);
            s.network_ms = Some((rt as f64 - b).max(0.0));
        }
        self.done.push_back(s);
        while self.done.len() > KEEP {
            self.done.pop_front();
        }
    }

    pub fn on_fill_slippage(&mut self, cid: &str, slippage_points: f64) {
        if let Some(s) = self.done.iter_mut().rev().find(|s| s.cid == cid) {
            s.slippage_points = Some(slippage_points);
        }
    }

    /// Median adverse slippage of the last `n` fills on `symbol`.
    pub fn median_slippage(&self, symbol: &str, n: usize) -> Option<f64> {
        let v: Vec<f64> = self.done.iter().rev().filter(|s| s.symbol == symbol).filter_map(|s| s.slippage_points).take(n).collect();
        quantiles(v).p50
    }

    pub fn report(&self) -> ExecutionQuality {
        let col = |f: &dyn Fn(&LatencySample) -> Option<f64>| quantiles(self.done.iter().filter_map(f).collect());
        let mut symbols: Vec<String> = self.done.iter().filter(|s| s.slippage_points.is_some()).map(|s| s.symbol.clone()).collect();
        symbols.sort();
        symbols.dedup();
        ExecutionQuality {
            internal_us: col(&|s| s.internal_us.map(|v| v as f64)),
            roundtrip_ms: col(&|s| s.roundtrip_ms.map(|v| v as f64)),
            broker_ms: col(&|s| s.broker_ms),
            network_ms: col(&|s| s.network_ms),
            slippage_points: col(&|s| s.slippage_points),
            by_symbol: symbols
                .into_iter()
                .map(|sym| {
                    let fills = self.done.iter().filter(|s| s.symbol == sym && s.slippage_points.is_some()).count();
                    SymbolSlippage { median_points: self.median_slippage(&sym, 50), symbol: sym, fills }
                })
                .collect(),
            rejected: self.done.iter().filter(|s| s.ok == Some(false)).count(),
            broker_time_missing: self.done.iter().filter(|s| s.roundtrip_ms.is_some() && s.broker_ms.is_none()).count(),
            recent: self.done.iter().rev().take(50).cloned().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_are_split_and_slippage_is_tracked_per_symbol() {
        let mut t = LatencyTracker::default();
        t.on_sent("a", "EURUSD", "open", Some(350), 1_000);
        t.on_result("a", true, 40_000, 1_120); // 120 ms round trip, 40 ms broker
        t.on_fill_slippage("a", 1.5);
        t.on_sent("b", "EURUSD", "open", Some(250), 2_000);
        t.on_result("b", false, 0, 2_050); // old connector: no broker time
        t.on_sent("c", "GBPUSD", "open", None, 3_000);
        t.on_result("c", true, 10_000, 3_030);
        t.on_fill_slippage("c", -0.5);
        t.on_result("unknown", true, 1, 9_999); // never sent: ignored
        let r = t.report();
        assert_eq!(r.roundtrip_ms.count, 3);
        assert_eq!(r.broker_ms.count, 2);
        let a = r.recent.iter().find(|s| s.cid == "a").unwrap();
        assert_eq!((a.broker_ms, a.network_ms), (Some(40.0), Some(80.0)));
        assert_eq!((r.rejected, r.broker_time_missing), (1, 1));
        assert_eq!(t.median_slippage("EURUSD", 20), Some(1.5));
        assert_eq!(t.median_slippage("USDJPY", 20), None);
        assert_eq!(r.by_symbol.len(), 2);
    }
}
