//! AEGIS AUTONOMY COMMANDER — capabilities, authorization profiles, grants and their audit.
//!
//! The Commander decides WHICH operations AEGIS may carry out by itself. It never decides
//! whether a particular trade is acceptable: that is the Risk Guardian's veto, which applies on
//! top of any permission. "Carte blanche" means maximal operational autonomy inside an
//! explicitly granted perimeter, never unlimited power over the account.
//!
//! Rules enforced here:
//! - a grant is bound to one account (login + server), optionally to a duration, and LIVE
//!   grants to the exact risk configuration (hash) they were given for;
//! - a grant can only be created from a [`UserConfirmation`], which only the user command
//!   handler constructs (an architecture test checks it); no engine, model or AI can grant
//!   itself a permission;
//! - a grant stops applying the moment it expires, is revoked, the account changes, or (LIVE)
//!   the risk configuration changes; the Commander then falls back to the default profile;
//! - LIVE needs the configuration switch, a real MT5 account, the account-bound phrase and a
//!   duration; it is never restored after a restart.

use aegis_types::{AccountTradeMode, Mode, TerminalKind, TimestampMs};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Phrase the user types, after the account number, to grant LIVE autonomy.
pub const LIVE_GRANT_PHRASE: &str = "J'ACCEPTE LE RISQUE REEL";
/// Longest LIVE grant (7 days): a live authorization always ends.
pub const MAX_LIVE_GRANT_MS: TimestampMs = 7 * 86_400_000;
/// Longest DEMO grant (365 days).
pub const MAX_DEMO_GRANT_MS: TimestampMs = 365 * 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    ReadMarketData,
    AnalyzeMarkets,
    RunStrategies,
    EvaluateProbabilities,
    RunBacktests,
    TrainModels,
    ProposeStrategies,
    DemoTrading,
    OpenPositions,
    ClosePositions,
    ModifyProtections,
    ManagePortfolio,
    RunShadow,
    GenerateReports,
    RestartServices,
    MonitorConnection,
    ManageAlerts,
    Backups,
}

impl Capability {
    pub const ALL: [Capability; 18] = [
        Capability::ReadMarketData,
        Capability::AnalyzeMarkets,
        Capability::RunStrategies,
        Capability::EvaluateProbabilities,
        Capability::RunBacktests,
        Capability::TrainModels,
        Capability::ProposeStrategies,
        Capability::DemoTrading,
        Capability::OpenPositions,
        Capability::ClosePositions,
        Capability::ModifyProtections,
        Capability::ManagePortfolio,
        Capability::RunShadow,
        Capability::GenerateReports,
        Capability::RestartServices,
        Capability::MonitorConnection,
        Capability::ManageAlerts,
        Capability::Backups,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Capability::ReadMarketData => "Lire les données MT5",
            Capability::AnalyzeMarkets => "Analyser les marchés",
            Capability::RunStrategies => "Lancer les stratégies",
            Capability::EvaluateProbabilities => "Évaluer les probabilités",
            Capability::RunBacktests => "Effectuer des backtests",
            Capability::TrainModels => "Entraîner des modèles",
            Capability::ProposeStrategies => "Proposer des stratégies",
            Capability::DemoTrading => "Opérations de démonstration",
            Capability::OpenPositions => "Ouvrir des positions",
            Capability::ClosePositions => "Fermer des positions",
            Capability::ModifyProtections => "Modifier SL et TP",
            Capability::ManagePortfolio => "Gérer le portefeuille",
            Capability::RunShadow => "Analyses Shadow",
            Capability::GenerateReports => "Générer des rapports",
            Capability::RestartServices => "Redémarrer les services",
            Capability::MonitorConnection => "Surveiller la connexion",
            Capability::ManageAlerts => "Gérer les alertes",
            Capability::Backups => "Sauvegardes",
        }
    }

    /// Capabilities that send orders to the broker.
    pub fn is_trading(self) -> bool {
        matches!(self, Capability::DemoTrading | Capability::OpenPositions | Capability::ClosePositions | Capability::ModifyProtections | Capability::ManagePortfolio)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Analyse and display. No automatic trading, no automatic research.
    Observer,
    /// Automatic research (training, backtests, comparisons, Shadow, reports). No trading.
    ResearchAutonomy,
    /// Full autonomy on an explicitly configured DEMO account.
    DemoFullAutonomy,
    /// Autonomy on an identified REAL account, explicitly granted, time-limited.
    LiveAuthorizedAutonomy,
}

const OBSERVE: &[Capability] = &[
    Capability::ReadMarketData,
    Capability::AnalyzeMarkets,
    Capability::RunStrategies,
    Capability::EvaluateProbabilities,
    Capability::GenerateReports,
    Capability::MonitorConnection,
    Capability::ManageAlerts,
    Capability::Backups,
];
const RESEARCH: &[Capability] = &[Capability::RunBacktests, Capability::TrainModels, Capability::ProposeStrategies, Capability::RunShadow, Capability::RestartServices];
const TRADE: &[Capability] = &[Capability::OpenPositions, Capability::ClosePositions, Capability::ModifyProtections, Capability::ManagePortfolio];

impl Profile {
    pub const ALL: [Profile; 4] = [Profile::Observer, Profile::ResearchAutonomy, Profile::DemoFullAutonomy, Profile::LiveAuthorizedAutonomy];

    pub fn label(self) -> &'static str {
        match self {
            Profile::Observer => "OBSERVER",
            Profile::ResearchAutonomy => "RESEARCH AUTONOMY",
            Profile::DemoFullAutonomy => "DEMO FULL AUTONOMY",
            Profile::LiveAuthorizedAutonomy => "LIVE AUTHORIZED AUTONOMY",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Profile::Observer => "observer",
            Profile::ResearchAutonomy => "research_autonomy",
            Profile::DemoFullAutonomy => "demo_full_autonomy",
            Profile::LiveAuthorizedAutonomy => "live_authorized_autonomy",
        }
    }

    pub fn parse(s: &str) -> Option<Profile> {
        Profile::ALL.into_iter().find(|p| p.as_str() == s)
    }

    pub fn capabilities(self) -> Vec<Capability> {
        let mut v: Vec<Capability> = OBSERVE.to_vec();
        if self != Profile::Observer {
            v.extend_from_slice(RESEARCH);
        }
        match self {
            Profile::DemoFullAutonomy => {
                v.push(Capability::DemoTrading);
                v.extend_from_slice(TRADE);
            }
            Profile::LiveAuthorizedAutonomy => v.extend_from_slice(TRADE),
            _ => {}
        }
        v.sort();
        v
    }

    pub fn allows(self, c: Capability) -> bool {
        self.capabilities().contains(&c)
    }

    /// Execution mode the profile corresponds to.
    pub fn mode(self) -> Mode {
        match self {
            Profile::Observer | Profile::ResearchAutonomy => Mode::Monitor,
            Profile::DemoFullAutonomy => Mode::DemoAuto,
            Profile::LiveAuthorizedAutonomy => Mode::LiveAuto,
        }
    }

    /// Profiles that need an explicit grant (the two others can be the default).
    pub fn needs_grant(self) -> bool {
        matches!(self, Profile::DemoFullAutonomy | Profile::LiveAuthorizedAutonomy)
    }
}

/// Proof that a human asked for a grant through the user command channel.
///
/// Only `aegis-core`'s user command handler builds it (checked by an architecture test).
/// Engines, models and the AI layer never hold one, so they can never grant themselves a
/// permission.
#[derive(Debug, Clone)]
pub struct UserConfirmation {
    actor: String,
    phrase: String,
}

impl UserConfirmation {
    /// `actor` identifies the channel (e.g. "user:api", "user:desktop"); it must start with "user".
    pub fn from_user_command(actor: &str, phrase: &str) -> Option<UserConfirmation> {
        actor.starts_with("user").then(|| UserConfirmation { actor: actor.to_string(), phrase: phrase.trim().to_string() })
    }
    pub fn actor(&self) -> &str {
        &self.actor
    }
}

/// The account and configuration a decision is made for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AuthorityContext {
    pub now_ms: TimestampMs,
    pub terminal: Option<TerminalKind>,
    pub account_login: Option<i64>,
    pub account_server: Option<String>,
    pub account_trade_mode: Option<AccountTradeMode>,
    /// SHA-256 of the active risk configuration (see [`risk_config_hash`]).
    pub risk_config_hash: String,
    /// `safety.live_trading_enabled` in the configuration file.
    pub live_enabled_in_config: bool,
}

/// Stable fingerprint of a risk configuration (its JSON form).
pub fn risk_config_hash(cfg: &serde_json::Value) -> String {
    hex::encode(Sha256::digest(serde_json::to_string(cfg).unwrap_or_default().as_bytes()))[..16].to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    pub grant_id: String,
    pub profile: Profile,
    pub account_login: Option<i64>,
    pub account_server: Option<String>,
    pub granted_by: String,
    pub granted_ms: TimestampMs,
    pub expires_ms: Option<TimestampMs>,
    /// LIVE: the grant only applies to this exact risk configuration.
    pub risk_config_hash: Option<String>,
    /// DEMO only: re-applied after an engine restart when the same account reconnects.
    pub persist_across_restart: bool,
    pub revoked_ms: Option<TimestampMs>,
    pub revoked_by: Option<String>,
    pub revoke_reason: Option<String>,
    pub note: String,
}

impl Grant {
    /// Why the grant does not apply in `ctx`, or `None` when it does.
    pub fn invalid_reason(&self, ctx: &AuthorityContext) -> Option<String> {
        if let Some(t) = self.revoked_ms {
            return Some(format!("révoquée à {}", aegis_types::format_ts(t)));
        }
        if self.expires_ms.is_some_and(|e| ctx.now_ms >= e) {
            return Some("expirée".into());
        }
        if self.account_login.is_some() && self.account_login != ctx.account_login {
            return Some(format!("liée au compte {} (connecté: {})", self.account_login.unwrap_or(0), ctx.account_login.map(|l| l.to_string()).unwrap_or_else(|| "aucun".into())));
        }
        if self.account_server.is_some() && self.account_server != ctx.account_server {
            return Some("liée à un autre serveur".into());
        }
        if self.profile == Profile::LiveAuthorizedAutonomy {
            if !ctx.live_enabled_in_config {
                return Some("LIVE désactivé dans la configuration".into());
            }
            if ctx.terminal != Some(TerminalKind::Mt5) || ctx.account_trade_mode != Some(AccountTradeMode::Real) {
                return Some("LIVE exige un vrai compte MT5 réel".into());
            }
            if self.risk_config_hash.as_deref() != Some(ctx.risk_config_hash.as_str()) {
                return Some("la configuration de risque a changé depuis l'autorisation".into());
            }
        }
        if self.profile == Profile::DemoFullAutonomy && ctx.account_trade_mode == Some(AccountTradeMode::Real) && ctx.terminal != Some(TerminalKind::Simulator) {
            return Some("autorisation DÉMO sur un compte RÉEL".into());
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrantRequest {
    pub profile: Profile,
    /// Duration in ms; mandatory for LIVE.
    pub duration_ms: Option<TimestampMs>,
    pub persist_across_restart: bool,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum GrantError {
    #[error("{0}")]
    Refused(String),
}

fn refused<T>(msg: impl Into<String>) -> Result<T, GrantError> {
    Err(GrantError::Refused(msg.into()))
}

/// Who asks for a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Requester {
    /// The user, through a command: user actions are the user's authority (the Risk Guardian
    /// still applies to orders).
    User,
    Strategy,
    Brain,
    PositionIntelligence,
    Learning,
    Shadow,
    Ops,
    /// Protective actions (missing stop, emergency stop): they REDUCE risk and are never
    /// blocked by a permission. They are still recorded.
    RiskGuardian,
}

impl Requester {
    pub fn as_str(self) -> &'static str {
        match self {
            Requester::User => "user",
            Requester::Strategy => "strategy",
            Requester::Brain => "brain",
            Requester::PositionIntelligence => "position_intelligence",
            Requester::Learning => "learning",
            Requester::Shadow => "shadow",
            Requester::Ops => "ops",
            Requester::RiskGuardian => "risk_guardian",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionDecision {
    pub capability: Capability,
    pub requester: Requester,
    pub allowed: bool,
    pub profile: Profile,
    pub grant_id: Option<String>,
    pub reason: String,
}

/// What changed when the Commander re-checked its grant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lapse {
    pub grant: Grant,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Commander {
    default_profile: Profile,
    active: Option<Grant>,
}

impl Commander {
    /// `default_profile` applies without a grant; only Observer or ResearchAutonomy.
    pub fn new(default_profile: Profile) -> Commander {
        let default_profile = if default_profile.needs_grant() { Profile::ResearchAutonomy } else { default_profile };
        Commander { default_profile, active: None }
    }

    pub fn default_profile(&self) -> Profile {
        self.default_profile
    }

    pub fn set_default_profile(&mut self, p: Profile) -> Result<(), GrantError> {
        if p.needs_grant() {
            return refused("le profil par défaut ne peut être que OBSERVER ou RESEARCH AUTONOMY");
        }
        self.default_profile = p;
        Ok(())
    }

    pub fn active_grant(&self) -> Option<&Grant> {
        self.active.as_ref()
    }

    /// Profile in force in `ctx`: the grant's when it applies, otherwise the default.
    pub fn profile(&self, ctx: &AuthorityContext) -> Profile {
        match &self.active {
            Some(g) if g.invalid_reason(ctx).is_none() => g.profile,
            _ => self.default_profile,
        }
    }

    pub fn check(&self, capability: Capability, requester: Requester, ctx: &AuthorityContext) -> PermissionDecision {
        let profile = self.profile(ctx);
        let grant_id = self.active.as_ref().filter(|g| g.invalid_reason(ctx).is_none()).map(|g| g.grant_id.clone());
        let (allowed, reason) = match requester {
            Requester::RiskGuardian => (true, "action de protection: jamais bloquée par une permission".to_string()),
            Requester::User if !capability.is_trading() => (true, "action demandée par l'utilisateur".to_string()),
            _ if profile.allows(capability) => (true, format!("{} autorise « {} »", profile.label(), capability.label())),
            _ => (false, format!("{} n'autorise pas « {} »", profile.label(), capability.label())),
        };
        PermissionDecision { capability, requester, allowed, profile, grant_id, reason }
    }

    /// Create a grant. Requires a user confirmation; LIVE requires the account-bound phrase.
    pub fn grant(&mut self, req: GrantRequest, confirmation: &UserConfirmation, ctx: &AuthorityContext, grant_id: String) -> Result<Grant, GrantError> {
        let login = ctx.account_login;
        if req.profile.needs_grant() && login.is_none() {
            return refused("aucun compte connecté: une autorisation est toujours liée à un compte");
        }
        if let Some(d) = req.duration_ms {
            if d <= 0 {
                return refused("durée invalide");
            }
        }
        let sim = ctx.terminal == Some(TerminalKind::Simulator);
        match req.profile {
            Profile::Observer | Profile::ResearchAutonomy => {}
            Profile::DemoFullAutonomy => {
                if !sim && !matches!(ctx.account_trade_mode, Some(AccountTradeMode::Demo | AccountTradeMode::Contest)) {
                    return refused("DEMO FULL AUTONOMY refusé: le compte connecté n'est pas un compte de démonstration");
                }
                if req.duration_ms.is_some_and(|d| d > MAX_DEMO_GRANT_MS) {
                    return refused("durée maximale d'une autorisation DÉMO: 365 jours");
                }
            }
            Profile::LiveAuthorizedAutonomy => {
                if !ctx.live_enabled_in_config {
                    return refused("LIVE désactivé dans la configuration (safety.live_trading_enabled = false)");
                }
                if sim || ctx.terminal != Some(TerminalKind::Mt5) || ctx.account_trade_mode != Some(AccountTradeMode::Real) {
                    return refused("LIVE exige un vrai compte MT5 réel connecté");
                }
                let Some(d) = req.duration_ms else { return refused("une autorisation LIVE a toujours une durée") };
                if d > MAX_LIVE_GRANT_MS {
                    return refused("durée maximale d'une autorisation LIVE: 7 jours");
                }
                let expected = format!("{} {}", login.unwrap_or(0), LIVE_GRANT_PHRASE);
                if confirmation.phrase != expected {
                    return refused(format!("confirmation incorrecte: saisissez exactement « {expected} »"));
                }
                if req.persist_across_restart {
                    return refused("une autorisation LIVE n'est jamais restaurée après un redémarrage");
                }
            }
        }
        let g = Grant {
            grant_id,
            profile: req.profile,
            account_login: login.filter(|_| req.profile.needs_grant()),
            account_server: ctx.account_server.clone().filter(|_| req.profile.needs_grant()),
            granted_by: confirmation.actor.clone(),
            granted_ms: ctx.now_ms,
            expires_ms: req.duration_ms.map(|d| ctx.now_ms + d),
            risk_config_hash: (req.profile == Profile::LiveAuthorizedAutonomy).then(|| ctx.risk_config_hash.clone()),
            persist_across_restart: req.persist_across_restart && req.profile == Profile::DemoFullAutonomy,
            revoked_ms: None,
            revoked_by: None,
            revoke_reason: None,
            note: req.note,
        };
        self.active = Some(g.clone());
        Ok(g)
    }

    /// Revoke the active grant at once. Returns it (as revoked) when there was one.
    pub fn revoke(&mut self, by: &str, reason: &str, now: TimestampMs) -> Option<Grant> {
        let mut g = self.active.take()?;
        g.revoked_ms = Some(now);
        g.revoked_by = Some(by.into());
        g.revoke_reason = Some(reason.into());
        Some(g)
    }

    /// Drop the active grant when it no longer applies (expiry, account or configuration change).
    pub fn lapse_if_invalid(&mut self, ctx: &AuthorityContext) -> Option<Lapse> {
        let reason = self.active.as_ref()?.invalid_reason(ctx)?;
        // A grant bound to an account simply waits while no account is connected (reconnection).
        if ctx.account_login.is_none() && !reason.contains("expirée") && !reason.contains("révoquée") {
            return None;
        }
        let g = self.revoke("commander", &reason, ctx.now_ms)?;
        Some(Lapse { grant: g, reason })
    }

    /// Restore a persisted DEMO grant after a restart (LIVE grants are never restored).
    pub fn restore(&mut self, g: Grant, now: TimestampMs) -> bool {
        let ok = g.profile == Profile::DemoFullAutonomy && g.persist_across_restart && g.revoked_ms.is_none() && g.expires_ms.is_none_or(|e| now < e);
        if ok {
            self.active = Some(g);
        }
        ok
    }
}

#[cfg(test)]
mod tests;
