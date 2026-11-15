//! Centralised risk engine. Nothing reaches the broker without an
//! `Authorization` minted here.

use crate::auth::{AuthError, Authorization};
use crate::config::{ConfigError, KillPositionPolicy, RiskConfig};
use crate::costs::{evaluate_micro_profit, MicroProfitEvaluation};
use crate::day::{drawdown_pct, DayStats};
use aegis_types::{
    new_id, AccountSnapshot, AccountTradeMode, Mode, OrderKind, PendingOrder, Position, Side,
    SymbolSpec, SymbolTradeMode, TerminalKind, Tick, TimestampMs,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const KILL_RESET_PHRASE: &str = "RESET AEGIS";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct KillSwitchState {
    pub active: bool,
    pub reason: String,
    pub actor: String,
    pub activated_ms: TimestampMs,
}

/// What the execution layer must do when the emergency stop engages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KillPlan {
    pub cancel_pending: bool,
    pub close_positions: bool,
}

/// A candidate trade submitted to the risk engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeProposal {
    pub decision_id: String,
    pub strategy_id: String,
    pub symbol: String,
    pub side: Side,
    pub kind: OrderKind,
    /// Order price for pending orders; ignored for market orders.
    pub entry_price: f64,
    pub sl: f64,
    pub tp: f64,
    /// Win probability from a validated source, if any.
    pub win_probability: Option<f64>,
    pub probability_source: String,
    /// Risk amount claimed by the caller. Never trusted: only compared and reported.
    pub claimed_risk_amount: Option<f64>,
    /// Optional volume cap requested by the caller. Can only reduce the size.
    pub requested_volume: Option<f64>,
}

/// Everything the engine needs to know about the world, supplied by the caller.
pub struct RiskContext<'a> {
    pub now_ms: TimestampMs,
    pub mode: Mode,
    pub connected: bool,
    pub terminal: Option<TerminalKind>,
    pub account: Option<&'a AccountSnapshot>,
    pub specs: &'a HashMap<String, SymbolSpec>,
    /// Latest tick of the proposal's symbol.
    pub tick: Option<&'a Tick>,
    pub positions: &'a [Position],
    pub pending: &'a [PendingOrder],
    pub day: Option<&'a DayStats>,
    pub peak_equity: f64,
    /// LIVE AUTO was explicitly armed by the user for this account.
    pub live_armed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub decision_id: String,
    pub ts_ms: TimestampMs,
    pub approved: bool,
    pub checks: Vec<Check>,
    /// Volume computed by the engine (0 when sizing was impossible).
    pub volume: f64,
    /// Money lost if the stop-loss is hit, computed by the engine.
    pub risk_amount: f64,
    pub risk_budget: f64,
    pub entry_price: f64,
    pub micro: Option<MicroProfitEvaluation>,
    pub authorization: Option<Authorization>,
}

impl RiskAssessment {
    pub fn failed_checks(&self) -> Vec<&Check> {
        self.checks.iter().filter(|c| !c.passed).collect()
    }
    pub fn rejection_summary(&self) -> String {
        self.failed_checks().iter().map(|c| format!("{}: {}", c.name, c.detail)).collect::<Vec<_>>().join(" ; ")
    }
}

pub struct RiskEngine {
    config: RiskConfig,
    key: [u8; 32],
    seen_decisions: HashSet<String>,
    consumed: HashSet<String>,
    kill: KillSwitchState,
}

struct Checks(Vec<Check>);

impl Checks {
    fn add(&mut self, name: &str, passed: bool, detail: impl Into<String>) -> bool {
        self.0.push(Check { name: name.to_string(), passed, detail: detail.into() });
        passed
    }
}

impl RiskEngine {
    pub fn new(config: RiskConfig) -> Result<Self, ConfigError> {
        config.validate()?;
        Ok(RiskEngine {
            config,
            key: rand::random(),
            seen_decisions: HashSet::new(),
            consumed: HashSet::new(),
            kill: KillSwitchState::default(),
        })
    }

    pub fn config(&self) -> &RiskConfig {
        &self.config
    }

    /// Replace the configuration. An invalid configuration is refused and the old one kept.
    pub fn set_config(&mut self, config: RiskConfig) -> Result<(), ConfigError> {
        config.validate()?;
        self.config = config;
        Ok(())
    }

    // ---------------------------------------------------------- kill switch

    pub fn kill_state(&self) -> &KillSwitchState {
        &self.kill
    }

    /// Restore a persisted state at start-up (an active stop stays active after a restart).
    pub fn restore_kill_state(&mut self, state: KillSwitchState) {
        self.kill = state;
    }

    /// Engage the emergency stop. Idempotent: the first reason is kept.
    pub fn engage_kill(&mut self, reason: &str, actor: &str, now_ms: TimestampMs) -> KillPlan {
        if !self.kill.active {
            self.kill = KillSwitchState {
                active: true,
                reason: reason.to_string(),
                actor: actor.to_string(),
                activated_ms: now_ms,
            };
        }
        KillPlan {
            cancel_pending: self.config.kill_cancel_pending,
            close_positions: self.config.kill_position_policy == KillPositionPolicy::Close,
        }
    }

    /// Manual reset. Requires the exact confirmation phrase.
    pub fn reset_kill(&mut self, confirmation: &str) -> Result<(), &'static str> {
        if !self.kill.active {
            return Err("emergency stop is not active");
        }
        if confirmation != KILL_RESET_PHRASE {
            return Err("confirmation phrase does not match");
        }
        self.kill = KillSwitchState::default();
        Ok(())
    }

    // -------------------------------------------------------- authorization

    /// Validate and consume an authorization. Each authorization works once.
    pub fn consume_authorization(&mut self, auth: &Authorization, now_ms: TimestampMs) -> Result<(), AuthError> {
        if self.kill.active {
            return Err(AuthError::KillSwitchActive);
        }
        if !auth.mac_is_valid(&self.key) {
            return Err(AuthError::BadSignature);
        }
        if now_ms > auth.expires_ms || now_ms < auth.issued_ms {
            return Err(AuthError::Expired);
        }
        if !self.consumed.insert(auth.auth_id.clone()) {
            return Err(AuthError::AlreadyUsed);
        }
        Ok(())
    }

    // ----------------------------------------------------------- evaluation

    /// Evaluate a proposal. Every check is recorded so the decision can be explained.
    /// An `Authorization` is attached only when every check passed.
    pub fn evaluate(&mut self, p: &TradeProposal, ctx: &RiskContext<'_>) -> RiskAssessment {
        let cfg = self.config.clone();
        let mut c = Checks(Vec::new());
        let mut out = RiskAssessment {
            decision_id: p.decision_id.clone(),
            ts_ms: ctx.now_ms,
            approved: false,
            checks: Vec::new(),
            volume: 0.0,
            risk_amount: 0.0,
            risk_budget: 0.0,
            entry_price: 0.0,
            micro: None,
            authorization: None,
        };

        c.add("arret_urgence", !self.kill.active, if self.kill.active {
            format!("arrêt d'urgence actif: {}", self.kill.reason)
        } else {
            "inactif".to_string()
        });
        c.add("mode", ctx.mode.can_execute(), format!("mode {}", ctx.mode.label()));
        let fresh_decision = !self.seen_decisions.contains(&p.decision_id) && !p.decision_id.is_empty();
        c.add("decision_unique", fresh_decision, if fresh_decision { "nouvelle décision" } else { "décision déjà évaluée (doublon)" });
        if !p.decision_id.is_empty() {
            self.seen_decisions.insert(p.decision_id.clone());
        }
        c.add("connexion", ctx.connected, if ctx.connected { "terminal connecté" } else { "terminal déconnecté" });

        // ---- account
        let account = match ctx.account {
            Some(a) if a.is_sane() => {
                let age = ctx.now_ms - a.ts_ms;
                let fresh = (-2_000..=cfg.max_account_age_ms).contains(&age);
                c.add("compte_frais", fresh, format!("âge des données de compte: {age} ms (max {})", cfg.max_account_age_ms));
                c.add("compte_trading_autorise", a.trade_allowed, if a.trade_allowed { "trading autorisé par le broker" } else { "trading désactivé sur le compte" });
                Some(a)
            }
            Some(_) => {
                c.add("compte_valide", false, "informations de compte incohérentes");
                None
            }
            None => {
                c.add("compte_valide", false, "aucune information de compte");
                None
            }
        };

        if let Some(a) = account {
            let (ok, detail) = match ctx.mode {
                Mode::Monitor => (true, "aucune exécution en surveillance".to_string()),
                Mode::DemoAuto => {
                    let demo = ctx.terminal == Some(TerminalKind::Simulator)
                        || matches!(a.trade_mode, AccountTradeMode::Demo | AccountTradeMode::Contest);
                    (demo, format!("compte {} requis: démo — constaté: {}", a.login, a.trade_mode.as_str()))
                }
                Mode::LiveAuto => {
                    let ok = ctx.terminal == Some(TerminalKind::Mt5)
                        && a.trade_mode == AccountTradeMode::Real
                        && ctx.live_armed;
                    (ok, format!("réel armé: {} — type de compte: {}", ctx.live_armed, a.trade_mode.as_str()))
                }
            };
            c.add("mode_compte", ok, detail);
        }

        // ---- instrument
        let spec = match ctx.specs.get(&p.symbol) {
            Some(s) => match s.validate() {
                Ok(()) => {
                    let tradable = s.trade_mode == SymbolTradeMode::Full;
                    c.add("instrument_negociable", tradable, format!("{} mode {:?}", s.symbol, s.trade_mode));
                    Some(s)
                }
                Err(e) => {
                    c.add("instrument_spec", false, e.to_string());
                    None
                }
            },
            None => {
                c.add("instrument_spec", false, format!("spécification broker absente pour {}", p.symbol));
                None
            }
        };

        // ---- market data
        let tick = match ctx.tick {
            Some(t) if t.bid.is_finite() && t.ask.is_finite() && t.bid > 0.0 && t.ask >= t.bid => {
                let age = ctx.now_ms - t.ts_ms;
                let fresh = (-2_000..=cfg.max_tick_age_ms).contains(&age);
                c.add("prix_frais", fresh, format!("âge du prix: {age} ms (max {})", cfg.max_tick_age_ms));
                Some(t)
            }
            Some(_) => {
                c.add("prix_valide", false, "bid/ask invalides");
                None
            }
            None => {
                c.add("prix_valide", false, "aucun prix reçu");
                None
            }
        };

        if let (Some(spec), Some(tick)) = (spec, tick) {
            let spread_pts = tick.spread() / spec.point;
            c.add("spread", spread_pts <= cfg.max_spread_points + 1e-9, format!("{spread_pts:.1} points (max {:.1})", cfg.max_spread_points));
        }

        // ---- limits
        if let (Some(a), Some(day)) = (account, ctx.day) {
            let pnl_pct = day.net_pnl_pct(a.equity);
            c.add("perte_journaliere", pnl_pct > -cfg.max_daily_loss_pct, format!("résultat net du jour {pnl_pct:.2} % (limite -{:.2} %)", cfg.max_daily_loss_pct));
            let target_ok = cfg.daily_profit_target_pct <= 0.0 || pnl_pct < cfg.daily_profit_target_pct;
            c.add("objectif_journalier", target_ok, if target_ok {
                format!("objectif {:.2} % non atteint", cfg.daily_profit_target_pct)
            } else {
                format!("objectif {:.2} % atteint ({pnl_pct:.2} %): nouvelles entrées arrêtées", cfg.daily_profit_target_pct)
            });
            let dd = drawdown_pct(ctx.peak_equity, a.equity);
            c.add("drawdown", dd < cfg.max_drawdown_pct, format!("drawdown {dd:.2} % (max {:.2} %)", cfg.max_drawdown_pct));
        } else if account.is_some() {
            c.add("comptabilite_journaliere", false, "statistiques du jour indisponibles");
        }

        let open_count = ctx.positions.len() + ctx.pending.len();
        c.add("positions_max", open_count < cfg.max_open_positions, format!("{open_count} ouvertes/en attente (max {})", cfg.max_open_positions));
        let on_symbol = ctx.positions.iter().filter(|x| x.symbol == p.symbol).count()
            + ctx.pending.iter().filter(|x| x.symbol == p.symbol).count();
        c.add("positions_par_instrument", on_symbol < cfg.max_positions_per_symbol, format!("{on_symbol} sur {} (max {})", p.symbol, cfg.max_positions_per_symbol));

        // ---- price levels, sizing, exposure, expectation
        if let (Some(a), Some(spec), Some(tick)) = (account, spec, tick) {
            let entry = match p.kind {
                OrderKind::Market => match p.side {
                    Side::Buy => tick.ask,
                    Side::Sell => tick.bid,
                },
                _ => p.entry_price,
            };
            out.entry_price = entry;
            let levels_ok = self.check_levels(&mut c, p, spec, tick, entry);

            if levels_ok {
                out.risk_budget = a.equity * cfg.risk_per_trade_pct / 100.0;
                let per_lot_loss = spec.price_distance_value((entry - p.sl).abs(), 1.0)
                    + cfg.commission_per_lot_round_turn
                    + spec.price_distance_value(cfg.expected_slippage_points * spec.point, 1.0);
                let mut volume = spec.normalize_volume(out.risk_budget / per_lot_loss);
                if let Some(req) = p.requested_volume {
                    volume = volume.min(spec.normalize_volume(req));
                }
                out.volume = volume;
                out.risk_amount = volume * per_lot_loss;
                let sized = volume > 0.0 && spec.is_volume_valid(volume);
                c.add("taille_position", sized, if sized {
                    format!("{volume:.2} lot(s): risque {:.2} {} pour un budget de {:.2}", out.risk_amount, a.currency, out.risk_budget)
                } else {
                    format!("le budget de risque {:.2} {} ne permet pas le volume minimum {}", out.risk_budget, a.currency, spec.volume_min)
                });
                if sized {
                    c.add("risque_par_transaction", out.risk_amount <= out.risk_budget + 1e-6, format!("{:.2} <= {:.2}", out.risk_amount, out.risk_budget));
                }
                if let Some(claimed) = p.claimed_risk_amount {
                    let agrees = (claimed - out.risk_amount).abs() <= 0.05 * out.risk_amount.max(0.01);
                    // Informational: the engine's own figure is always the one applied.
                    c.add("risque_declare", true, if agrees {
                        "montant déclaré cohérent avec le calcul interne".to_string()
                    } else {
                        format!("montant déclaré {claimed:.2} ignoré; calcul interne {:.2}", out.risk_amount)
                    });
                }

                if sized {
                    self.check_exposure(&mut c, ctx, a, spec, entry, volume);
                    let micro = evaluate_micro_profit(
                        spec, &cfg, p.side, volume, entry, p.sl, p.tp, tick.spread(),
                        p.win_probability, &p.probability_source,
                    );
                    match micro.expected_net {
                        Some(net) => {
                            c.add("esperance_nette", net >= cfg.min_net_expected_profit && net > 0.0, format!(
                                "espérance nette {net:.2} {} (minimum {:.2}); coûts {:.2}; p={:.3} [{}]",
                                a.currency, cfg.min_net_expected_profit, micro.costs.total(),
                                micro.win_probability.unwrap_or(f64::NAN), micro.probability_source
                            ));
                        }
                        None => {
                            c.add("esperance_nette", false, "aucune probabilité validée: espérance nette non calculable");
                        }
                    }
                    out.micro = Some(micro);
                }
            }
        }

        out.approved = c.0.iter().all(|x| x.passed);
        out.checks = c.0;
        if out.approved {
            let mut auth = Authorization {
                auth_id: new_id("auth"),
                decision_id: p.decision_id.clone(),
                symbol: p.symbol.clone(),
                side: p.side,
                kind: p.kind,
                volume: out.volume,
                price: out.entry_price,
                sl: p.sl,
                tp: p.tp,
                max_slippage_points: cfg.max_slippage_points,
                risk_amount: out.risk_amount,
                issued_ms: ctx.now_ms,
                expires_ms: ctx.now_ms + cfg.authorization_ttl_ms,
                mac: String::new(),
            };
            auth.sign(&self.key);
            out.authorization = Some(auth);
        }
        out
    }

    fn check_levels(&self, c: &mut Checks, p: &TradeProposal, spec: &SymbolSpec, tick: &Tick, entry: f64) -> bool {
        let finite = [entry, p.sl, p.tp].iter().all(|v| v.is_finite() && *v > 0.0);
        if !c.add("niveaux_valides", finite, if finite { "entrée, SL et TP définis" } else { "entrée, stop-loss et take-profit obligatoires" }) {
            return false;
        }
        let dir = p.side.sign();
        let sides_ok = (entry - p.sl) * dir > 0.0 && (p.tp - entry) * dir > 0.0;
        if !c.add("niveaux_coherents", sides_ok, if sides_ok { "SL et TP du bon côté du prix" } else { "SL/TP du mauvais côté du prix d'entrée" }) {
            return false;
        }
        let min_dist = spec.stops_level_points * spec.point;
        // Stops are measured against the price that will trigger them.
        let trigger = match (p.kind, p.side) {
            (OrderKind::Market, Side::Buy) => tick.bid,
            (OrderKind::Market, Side::Sell) => tick.ask,
            _ => entry,
        };
        let far = (trigger - p.sl) * dir >= min_dist - 1e-12 && (p.tp - trigger) * dir >= min_dist - 1e-12;
        let mut ok = c.add("distance_stops", far, format!("distance minimale broker {:.0} points", spec.stops_level_points));
        if p.kind != OrderKind::Market {
            let pending_ok = match (p.kind, p.side) {
                (OrderKind::Limit, Side::Buy) | (OrderKind::Stop, Side::Sell) => entry < tick.bid,
                (OrderKind::Limit, Side::Sell) | (OrderKind::Stop, Side::Buy) => entry > tick.ask,
                _ => false,
            };
            ok &= c.add("prix_ordre_attente", pending_ok, if pending_ok { "prix d'ordre cohérent avec le marché" } else { "prix d'ordre en attente incohérent avec le marché" });
        }
        ok
    }

    fn check_exposure(&self, c: &mut Checks, ctx: &RiskContext<'_>, a: &AccountSnapshot, spec: &SymbolSpec, entry: f64, volume: f64) {
        let notional = |s: &SymbolSpec, price: f64, vol: f64| price / s.tick_size * s.tick_value * vol;
        let mut total = notional(spec, entry, volume);
        let mut unknown = Vec::new();
        for (sym, price, vol) in ctx
            .positions
            .iter()
            .map(|x| (&x.symbol, x.price_open, x.volume))
            .chain(ctx.pending.iter().map(|x| (&x.symbol, x.price, x.volume)))
        {
            match ctx.specs.get(sym) {
                Some(s) if s.validate().is_ok() => total += notional(s, price, vol),
                _ => unknown.push(sym.clone()),
            }
        }
        if !unknown.is_empty() {
            c.add("exposition", false, format!("exposition incalculable: spécification absente pour {}", unknown.join(", ")));
            return;
        }
        let lev = if a.equity > 0.0 { total / a.equity } else { f64::INFINITY };
        c.add("exposition", lev <= self.config.max_notional_leverage, format!("exposition notionnelle {lev:.2}x l'équité (max {:.2}x)", self.config.max_notional_leverage));
        if a.leverage > 0 {
            let margin_needed = notional(spec, entry, volume) / a.leverage as f64;
            c.add("marge_libre", a.margin_free >= margin_needed * 1.5, format!("marge estimée {margin_needed:.2}, marge libre {:.2}", a.margin_free));
        } else {
            c.add("marge_libre", false, "levier du compte inconnu");
        }
    }
}
