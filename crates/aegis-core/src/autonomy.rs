//! AEGIS AUTONOMY COMMANDER inside the engine: grants, revocation, lapse, the permission
//! audit trail, and the invariant "an execution mode exists only under a trading grant".

use crate::engine::Engine;
use aegis_autonomy::{AuthorityContext, Capability, Grant, GrantRequest, PermissionDecision, Profile, Requester, UserConfirmation};
use aegis_data::{PermissionEventRow, PermissionGrantRow};
use aegis_types::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityView {
    pub capability: Capability,
    pub label: String,
    pub allowed: bool,
    pub trading: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AutonomyView {
    pub default_profile: String,
    pub profile: String,
    pub profile_label: String,
    pub active_grant: Option<Grant>,
    /// Why the active grant does not apply right now (e.g. account disconnected).
    pub grant_inactive_reason: Option<String>,
    pub capabilities: Vec<CapabilityView>,
    pub grants: Vec<PermissionGrantRow>,
    pub events: Vec<PermissionEventRow>,
    pub live_enabled_in_config: bool,
    pub live_phrase_hint: String,
}

pub(crate) fn grant_row(g: &Grant) -> PermissionGrantRow {
    PermissionGrantRow {
        grant_id: g.grant_id.clone(),
        profile: g.profile.as_str().into(),
        account_login: g.account_login,
        account_server: g.account_server.clone(),
        granted_by: g.granted_by.clone(),
        granted_ms: g.granted_ms,
        expires_ms: g.expires_ms,
        risk_config_hash: g.risk_config_hash.clone(),
        persist_across_restart: g.persist_across_restart,
        revoked_ms: g.revoked_ms,
        revoked_by: g.revoked_by.clone(),
        revoke_reason: g.revoke_reason.clone(),
        note: g.note.clone(),
    }
}

fn grant_of_row(r: &PermissionGrantRow) -> Option<Grant> {
    Some(Grant {
        grant_id: r.grant_id.clone(),
        profile: Profile::parse(&r.profile)?,
        account_login: r.account_login,
        account_server: r.account_server.clone(),
        granted_by: r.granted_by.clone(),
        granted_ms: r.granted_ms,
        expires_ms: r.expires_ms,
        risk_config_hash: r.risk_config_hash.clone(),
        persist_across_restart: r.persist_across_restart,
        revoked_ms: r.revoked_ms,
        revoked_by: r.revoked_by.clone(),
        revoke_reason: r.revoke_reason.clone(),
        note: r.note.clone(),
    })
}

impl Engine {
    pub(crate) fn authority_ctx(&self) -> AuthorityContext {
        AuthorityContext {
            now_ms: now_ms(),
            terminal: self.terminal(),
            account_login: self.account.as_ref().map(|a| a.login),
            account_server: self.account.as_ref().map(|a| a.server.clone()),
            account_trade_mode: self.account.as_ref().map(|a| a.trade_mode),
            risk_config_hash: aegis_autonomy::risk_config_hash(&serde_json::to_value(self.risk.config()).unwrap_or_default()),
            live_enabled_in_config: self.cfg.safety.live_trading_enabled,
        }
    }

    pub(crate) fn permit(&self, capability: Capability, requester: Requester) -> PermissionDecision {
        self.commander.check(capability, requester, &self.authority_ctx())
    }

    /// Write one line of the permission audit trail (and the chained audit for grant events).
    pub(crate) async fn permission_event(&mut self, event: &str, actor: &str, d: Option<&PermissionDecision>, detail: String) {
        let profile = d.map(|d| d.profile).unwrap_or_else(|| self.commander.profile(&self.authority_ctx()));
        let row = PermissionEventRow {
            id: 0,
            ts_ms: now_ms(),
            event: event.into(),
            actor: actor.into(),
            requester: d.map(|d| d.requester.as_str().to_string()).unwrap_or_default(),
            capability: d.map(|d| serde_json::to_value(d.capability).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()).unwrap_or_default(),
            profile: profile.as_str().into(),
            grant_id: d.and_then(|d| d.grant_id.clone()).or_else(|| self.commander.active_grant().map(|g| g.grant_id.clone())),
            account_login: self.account.as_ref().map(|a| a.login),
            allowed: d.is_none_or(|d| d.allowed),
            detail,
        };
        if self.db_ok {
            if let Err(e) = self.store.insert_permission_event(&row).await {
                self.db_failed(&e.to_string());
            }
        }
        if matches!(event, "grant" | "revoke" | "lapse" | "resume" | "default_profile") {
            self.audit(actor, &format!("autonomy_{event}"), json!({ "profile": row.profile, "grant_id": row.grant_id, "detail": row.detail })).await;
        }
        self.cache.dirty = true;
    }

    async fn persist_grant(&mut self, g: &Grant) {
        if let Err(e) = self.store.upsert_grant(&grant_row(g)).await {
            self.db_failed(&e.to_string());
        }
    }

    /// Start-up: the default profile from the file, and a persistent DEMO grant if any.
    pub(crate) async fn load_autonomy(&mut self) {
        if let Ok(Some(p)) = self.store.kv_get::<Profile>("autonomy_default_profile").await {
            let _ = self.commander.set_default_profile(p);
        }
        let Ok(open) = self.store.open_grants().await else { return };
        let now = now_ms();
        for row in open {
            let Some(g) = grant_of_row(&row) else { continue };
            if self.commander.active_grant().is_none() && self.commander.restore(g.clone(), now) {
                self.log("warn", "autonomy", format!("autorisation {} ({}) restaurée: reprise à la reconnexion du compte {}", g.grant_id, g.profile.label(), g.account_login.unwrap_or(0)));
                continue;
            }
            // Anything else (LIVE, non-persistent, expired) ends with the restart.
            let mut g = g;
            g.revoked_ms = Some(now);
            g.revoked_by = Some("engine".into());
            g.revoke_reason = Some("redémarrage du moteur (autorisation non persistante)".into());
            self.persist_grant(&g).await;
        }
    }

    /// Grant a trading profile. `phrase` is only checked for LIVE.
    pub(crate) async fn grant_autonomy(&mut self, req: GrantRequest, phrase: &str) -> Result<String, String> {
        if !req.profile.needs_grant() {
            return Err("OBSERVER et RESEARCH AUTONOMY ne s'accordent pas: choisissez-les comme profil par défaut".into());
        }
        if self.risk.kill_state().active {
            return Err("arrêt d'urgence actif: réinitialisez-le d'abord".into());
        }
        if !self.db_ok {
            return Err("base de données indisponible".into());
        }
        if self.conn.is_none() {
            return Err("aucun terminal connecté".into());
        }
        if self.account.is_none() {
            return Err("informations de compte non reçues".into());
        }
        let confirmation = UserConfirmation::from_user_command("user", phrase).expect("the user channel");
        let ctx = self.authority_ctx();
        let profile = req.profile;
        let g = self.commander.grant(req, &confirmation, &ctx, new_id("grant")).map_err(|e| e.to_string())?;
        self.persist_grant(&g).await;
        let until = g.expires_ms.map(format_ts).unwrap_or_else(|| "sans limite".into());
        self.permission_event("grant", "user", None, format!("{} sur le compte {} jusqu'à {until}{}", profile.label(), g.account_login.unwrap_or(0), if g.persist_across_restart { " (reprise après redémarrage)" } else { "" })).await;
        self.set_mode(profile.mode(), &format!("autorisation {} {}", profile.label(), g.grant_id)).await;
        if profile == Profile::LiveAuthorizedAutonomy {
            self.live_armed = true;
            self.audit("user", "live_armed", json!({ "login": g.account_login, "grant_id": g.grant_id, "expires_ms": g.expires_ms })).await;
        }
        Ok(format!("{} accordé sur le compte {} (autorisation {}, jusqu'à {until})", profile.label(), g.account_login.unwrap_or(0), g.grant_id))
    }

    /// Immediate revocation (user, emergency stop reset, account change).
    pub(crate) async fn revoke_autonomy(&mut self, by: &str, reason: &str) -> Option<Grant> {
        let g = self.commander.revoke(by, reason, now_ms());
        if let Some(g) = &g {
            self.persist_grant(g).await;
            self.permission_event("revoke", by, None, format!("{} révoquée: {reason}", g.grant_id)).await;
        }
        self.set_mode(Mode::Monitor, reason).await;
        g
    }

    pub(crate) async fn set_default_profile(&mut self, p: Profile) -> Result<String, String> {
        self.commander.set_default_profile(p).map_err(|e| e.to_string())?;
        let _ = self.store.kv_set("autonomy_default_profile", &p).await;
        self.permission_event("default_profile", "user", None, format!("profil par défaut: {}", p.label())).await;
        Ok(format!("profil par défaut: {}", p.label()))
    }

    /// Every timer tick: lapse invalid grants, keep the mode consistent with the permissions,
    /// resume a persistent DEMO grant after a reconnection.
    pub(crate) async fn autonomy_tick(&mut self) {
        let ctx = self.authority_ctx();
        if let Some(l) = self.commander.lapse_if_invalid(&ctx) {
            self.persist_grant(&l.grant).await;
            self.permission_event("lapse", "commander", None, format!("{} ({}) n'est plus valable: {}", l.grant.grant_id, l.grant.profile.label(), l.reason)).await;
            self.alert("warn", "autonomy", format!("autorisation {} terminée: {} — mode MONITOR", l.grant.profile.label(), l.reason));
            self.set_mode(Mode::Monitor, &format!("autorisation terminée: {}", l.reason)).await;
        }
        let profile = self.commander.profile(&ctx);
        if self.mode.can_execute() && (profile.mode() != self.mode || !profile.allows(Capability::OpenPositions)) {
            let d = self.permit(Capability::OpenPositions, Requester::Strategy);
            self.permission_event("denied", "commander", Some(&d), format!("mode {} sans autorisation correspondante", self.mode.label())).await;
            self.set_mode(Mode::Monitor, "mode d'exécution sans autorisation correspondante").await;
        }
        let resume = self.mode == Mode::Monitor
            && profile == Profile::DemoFullAutonomy
            && self.commander.active_grant().is_some_and(|g| g.persist_across_restart)
            && self.conn.is_some()
            && self.account.is_some()
            && self.db_ok
            && !self.risk.kill_state().active;
        if resume {
            self.set_mode(Mode::DemoAuto, "reprise de l'autorisation DÉMO persistante").await;
            self.permission_event("resume", "commander", None, "reconnexion du compte autorisé: DEMO AUTO repris".into()).await;
        }
    }

    pub(crate) fn autonomy_view(&self) -> AutonomyView {
        let ctx = self.authority_ctx();
        let profile = self.commander.profile(&ctx);
        AutonomyView {
            default_profile: self.commander.default_profile().as_str().into(),
            profile: profile.as_str().into(),
            profile_label: profile.label().into(),
            active_grant: self.commander.active_grant().cloned(),
            grant_inactive_reason: self.commander.active_grant().and_then(|g| g.invalid_reason(&ctx)),
            capabilities: Capability::ALL
                .iter()
                .map(|c| CapabilityView { capability: *c, label: c.label().into(), allowed: profile.allows(*c), trading: c.is_trading() })
                .collect(),
            grants: self.cache.grants.clone(),
            events: self.cache.permission_events.clone(),
            live_enabled_in_config: self.cfg.safety.live_trading_enabled,
            live_phrase_hint: self.account.as_ref().map(|a| format!("{} {}", a.login, aegis_autonomy::LIVE_GRANT_PHRASE)).unwrap_or_default(),
        }
    }
}
