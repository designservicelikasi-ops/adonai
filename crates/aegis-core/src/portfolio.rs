//! Multi-asset portfolio: instrument catalogue (each with its own broker specification),
//! enable/disable per instrument, correlations for the Global Exposure Manager, exposure per
//! currency and per correlated group, and the news that concerns each instrument.

use crate::engine::Engine;
use aegis_types::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

const CORRELATION_EVERY_MS: TimestampMs = 15 * 60_000;

/// Asset class from the instrument name (broker names vary; unknown = "autre").
pub fn category(symbol: &str) -> &'static str {
    let s = symbol.to_ascii_uppercase();
    let any = |keys: &[&str]| keys.iter().any(|k| s.contains(k));
    if any(&["XAU", "XAG", "XPT", "XPD", "GOLD", "SILVER"]) {
        "metaux"
    } else if any(&["BTC", "ETH", "LTC", "XRP", "SOL", "CRYPTO"]) {
        "crypto"
    } else if any(&["OIL", "WTI", "BRENT", "NGAS", "GAS"]) {
        "energie"
    } else if any(&["NAS", "US30", "US100", "US500", "SPX", "SP500", "DJ", "DAX", "GER", "UK100", "FTSE", "JP225", "NIK", "CAC", "STOXX"]) {
        "indices"
    } else if s.len() >= 6 && s[..6].chars().all(|c| c.is_ascii_alphabetic()) {
        "forex"
    } else {
        "autre"
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstrumentView {
    pub symbol: String,
    pub category: String,
    pub enabled: bool,
    pub tradable_at_broker: bool,
    pub spec: SymbolSpec,
    pub spread_points: Option<f64>,
    /// Notional of the minimum volume and its margin at the account leverage (estimates).
    pub min_lot_notional: Option<f64>,
    pub min_lot_margin: Option<f64>,
    pub open_buy_volume: f64,
    pub open_sell_volume: f64,
    pub floating: f64,
    pub news: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorrelationView {
    pub a: String,
    pub b: String,
    pub rho: f64,
    pub returns: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PortfolioView {
    pub instruments: Vec<InstrumentView>,
    pub correlations: Vec<CorrelationView>,
    pub correlations_ms: Option<TimestampMs>,
    pub correlation_timeframe: String,
    /// Gross notional per currency divided by equity.
    pub currency_exposure: Vec<(String, f64)>,
    pub account_currency: String,
}

#[derive(Default)]
pub(crate) struct PortfolioState {
    pub disabled: std::collections::BTreeSet<String>,
    pub correlations: Vec<CorrelationView>,
    pub computed_ms: Option<TimestampMs>,
    pub timeframe: Option<Timeframe>,
}

impl Engine {
    pub(crate) async fn load_portfolio(&mut self) {
        if let Ok(Some(d)) = self.store.kv_get::<std::collections::BTreeSet<String>>("disabled_symbols").await {
            self.portfolio.disabled = d;
        }
    }

    pub(crate) fn symbol_enabled(&self, symbol: &str) -> bool {
        !self.portfolio.disabled.contains(symbol)
    }

    pub(crate) async fn cmd_symbol_enabled(&mut self, symbol: &str, enabled: bool) -> Result<String, String> {
        if !self.specs.contains_key(symbol) {
            return Err(format!("instrument {symbol} inconnu (aucune spécification reçue du terminal)"));
        }
        if enabled {
            self.portfolio.disabled.remove(symbol);
        } else {
            self.portfolio.disabled.insert(symbol.to_string());
        }
        self.store.kv_set("disabled_symbols", &self.portfolio.disabled).await.map_err(|e| e.to_string())?;
        self.audit("user", "symbol_enabled", serde_json::json!({ "symbol": symbol, "enabled": enabled })).await;
        Ok(format!("{symbol}: {}", if enabled { "activé" } else { "désactivé (aucune nouvelle entrée; les positions restent gérées)" }))
    }

    /// Correlations of returns between every pair of instruments, on H1 (or M15) bars.
    pub(crate) fn portfolio_tick(&mut self, now: TimestampMs) {
        if self.portfolio.computed_ms.is_some_and(|t| now - t < CORRELATION_EVERY_MS) {
            return;
        }
        self.portfolio.computed_ms = Some(now);
        let tf = [Timeframe::H1, Timeframe::M15, Timeframe::M5].into_iter().find(|t| self.market.config().timeframes.contains(t)).unwrap_or(Timeframe::M5);
        self.portfolio.timeframe = Some(tf);
        let mut symbols: Vec<String> = self.specs.keys().cloned().collect();
        symbols.sort();
        let series: BTreeMap<String, Vec<(TimestampMs, f64)>> = symbols
            .iter()
            .map(|s| (s.clone(), self.market.candles(s, tf).iter().rev().take(300).rev().map(|c| (c.open_ts, c.close)).collect()))
            .collect();
        let mut out = Vec::new();
        let mut table = HashMap::new();
        for (i, a) in symbols.iter().enumerate() {
            for b in symbols.iter().skip(i + 1) {
                if let Some((rho, n)) = aegis_risk::exposure::correlation(&series[a], &series[b]) {
                    table.insert((a.clone(), b.clone()), rho);
                    out.push(CorrelationView { a: a.clone(), b: b.clone(), rho, returns: n });
                }
            }
        }
        self.risk.set_correlations(table);
        self.portfolio.correlations = out;
    }

    pub(crate) fn portfolio_view(&self, now: TimestampMs) -> PortfolioView {
        let acc = self.account.as_ref();
        let mut symbols: Vec<&String> = self.specs.keys().collect();
        symbols.sort();
        let mut ccy: BTreeMap<String, f64> = BTreeMap::new();
        for p in &self.positions {
            if let Some(s) = self.specs.get(&p.symbol) {
                let n = p.price_open / s.tick_size * s.tick_value * p.volume;
                for c in aegis_risk::currencies_of(s) {
                    *ccy.entry(c).or_default() += n;
                }
            }
        }
        let equity = acc.map(|a| a.equity).unwrap_or(0.0);
        PortfolioView {
            instruments: symbols
                .into_iter()
                .map(|sym| {
                    let s = &self.specs[sym];
                    let tick = self.market.last_tick(sym).map(|(t, _)| t);
                    let notional = tick.map(|t| t.mid() / s.tick_size * s.tick_value * s.volume_min);
                    let news = self.news_assessment(sym, now);
                    let mine: Vec<&Position> = self.positions.iter().filter(|p| &p.symbol == sym).collect();
                    InstrumentView {
                        symbol: sym.clone(),
                        category: category(sym).into(),
                        enabled: self.symbol_enabled(sym),
                        tradable_at_broker: s.trade_mode == SymbolTradeMode::Full,
                        spec: s.clone(),
                        spread_points: tick.map(|t| t.spread() / s.point),
                        min_lot_notional: notional,
                        min_lot_margin: notional.zip(acc.filter(|a| a.leverage > 0)).map(|(n, a)| n / a.leverage as f64),
                        open_buy_volume: mine.iter().filter(|p| p.side == Side::Buy).map(|p| p.volume).sum(),
                        open_sell_volume: mine.iter().filter(|p| p.side == Side::Sell).map(|p| p.volume).sum(),
                        floating: mine.iter().map(|p| p.floating()).sum(),
                        news: format!("{}: {}", news.recommendation.label(), news.reasons.join(" ; ")),
                    }
                })
                .collect(),
            correlations: self.portfolio.correlations.clone(),
            correlations_ms: self.portfolio.computed_ms,
            correlation_timeframe: self.portfolio.timeframe.map(|t| t.as_str().to_string()).unwrap_or_default(),
            currency_exposure: ccy.into_iter().map(|(c, n)| (c, if equity > 0.0 { n / equity } else { 0.0 })).collect(),
            account_currency: acc.map(|a| a.currency.clone()).unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::category;

    #[test]
    fn asset_classes_from_broker_names() {
        for (s, c) in [("EURUSD", "forex"), ("EURUSD.sim", "forex"), ("XAUUSD", "metaux"), ("NAS100", "indices"), ("US30.cash", "indices"), ("USOIL", "energie"), ("UKOIL", "energie"), ("BTCUSD", "crypto"), ("SPX500", "indices"), ("ABC", "autre")] {
            assert_eq!(category(s), c, "{s}");
        }
    }
}
