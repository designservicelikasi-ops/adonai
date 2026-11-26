use super::*;

const NOW: i64 = 1_800_000_000_000;

fn ctx(terminal: TerminalKind, mode: AccountTradeMode, login: i64) -> AuthorityContext {
    AuthorityContext {
        now_ms: NOW,
        terminal: Some(terminal),
        account_login: Some(login),
        account_server: Some("Broker-1".into()),
        account_trade_mode: Some(mode),
        risk_config_hash: "abc".into(),
        live_enabled_in_config: true,
    }
}

fn user(phrase: &str) -> UserConfirmation {
    UserConfirmation::from_user_command("user:api", phrase).unwrap()
}

fn req(profile: Profile, duration_ms: Option<i64>) -> GrantRequest {
    GrantRequest { profile, duration_ms, persist_across_restart: false, note: String::new() }
}

#[test]
fn profiles_have_strictly_growing_capabilities() {
    let obs = Profile::Observer.capabilities();
    let res = Profile::ResearchAutonomy.capabilities();
    let demo = Profile::DemoFullAutonomy.capabilities();
    let live = Profile::LiveAuthorizedAutonomy.capabilities();
    assert!(obs.iter().all(|c| !c.is_trading()) && res.iter().all(|c| !c.is_trading()));
    assert!(!obs.contains(&Capability::TrainModels) && res.contains(&Capability::TrainModels));
    assert!(obs.iter().all(|c| res.contains(c)) && res.iter().all(|c| demo.contains(c)));
    assert!(demo.contains(&Capability::OpenPositions) && live.contains(&Capability::OpenPositions));
    assert!(!live.contains(&Capability::DemoTrading), "a live grant is not a demo permission");
    assert_eq!(Capability::ALL.len(), 18);
    assert_eq!(Profile::DemoFullAutonomy.mode(), Mode::DemoAuto);
    assert_eq!(Profile::ResearchAutonomy.mode(), Mode::Monitor);
}

#[test]
fn only_the_user_channel_can_confirm() {
    assert!(UserConfirmation::from_user_command("brain", "x").is_none());
    assert!(UserConfirmation::from_user_command("commander", "x").is_none());
    assert!(UserConfirmation::from_user_command("user:desktop", "x").is_some());
}

#[test]
fn without_a_grant_nothing_trades() {
    let c = Commander::new(Profile::ResearchAutonomy);
    let x = ctx(TerminalKind::Mt5, AccountTradeMode::Demo, 7);
    assert!(!c.check(Capability::OpenPositions, Requester::Strategy, &x).allowed);
    assert!(c.check(Capability::TrainModels, Requester::Brain, &x).allowed);
    let c = Commander::new(Profile::Observer);
    assert!(!c.check(Capability::TrainModels, Requester::Brain, &x).allowed, "observer: no automatic research");
    assert!(c.check(Capability::TrainModels, Requester::User, &x).allowed, "the user may still ask for it");
    assert!(!c.check(Capability::OpenPositions, Requester::User, &x).allowed, "user orders need a trading profile too");
    // Protective actions are never blocked.
    assert!(c.check(Capability::ClosePositions, Requester::RiskGuardian, &x).allowed);
    // The default can never be a trading profile.
    assert_eq!(Commander::new(Profile::LiveAuthorizedAutonomy).default_profile(), Profile::ResearchAutonomy);
    assert!(Commander::new(Profile::Observer).set_default_profile(Profile::DemoFullAutonomy).is_err());
}

#[test]
fn demo_grant_is_bound_to_a_demo_account_and_can_expire() {
    let mut c = Commander::new(Profile::ResearchAutonomy);
    let real = ctx(TerminalKind::Mt5, AccountTradeMode::Real, 7);
    assert!(c.grant(req(Profile::DemoFullAutonomy, None), &user(""), &real, "g0".into()).is_err());
    let demo = ctx(TerminalKind::Mt5, AccountTradeMode::Demo, 7);
    let g = c.grant(req(Profile::DemoFullAutonomy, Some(60_000)), &user(""), &demo, "g1".into()).unwrap();
    assert_eq!((g.account_login, g.granted_by.as_str()), (Some(7), "user:api"));
    assert!(c.check(Capability::OpenPositions, Requester::Strategy, &demo).allowed);
    // Another account connected: the grant does not apply there.
    let other = ctx(TerminalKind::Mt5, AccountTradeMode::Demo, 8);
    assert_eq!(c.profile(&other), Profile::ResearchAutonomy);
    // Expiry: the commander lapses it.
    let later = AuthorityContext { now_ms: NOW + 60_000, ..demo.clone() };
    assert!(!c.check(Capability::OpenPositions, Requester::Strategy, &later).allowed);
    let lapse = c.lapse_if_invalid(&later).unwrap();
    assert!(lapse.reason.contains("expirée") && lapse.grant.revoked_ms.is_some());
    assert!(c.active_grant().is_none());
}

#[test]
fn live_needs_config_real_mt5_phrase_duration_and_the_same_risk_configuration() {
    let real = ctx(TerminalKind::Mt5, AccountTradeMode::Real, 4242);
    let phrase = "4242 J'ACCEPTE LE RISQUE REEL";
    let mut c = Commander::new(Profile::Observer);
    let off = AuthorityContext { live_enabled_in_config: false, ..real.clone() };
    assert!(c.grant(req(Profile::LiveAuthorizedAutonomy, Some(3_600_000)), &user(phrase), &off, "a".into()).is_err());
    let sim = ctx(TerminalKind::Simulator, AccountTradeMode::Real, 4242);
    assert!(c.grant(req(Profile::LiveAuthorizedAutonomy, Some(3_600_000)), &user(phrase), &sim, "b".into()).is_err());
    assert!(c.grant(req(Profile::LiveAuthorizedAutonomy, None), &user(phrase), &real, "c".into()).is_err(), "always a duration");
    assert!(c.grant(req(Profile::LiveAuthorizedAutonomy, Some(MAX_LIVE_GRANT_MS + 1)), &user(phrase), &real, "d".into()).is_err());
    assert!(c.grant(req(Profile::LiveAuthorizedAutonomy, Some(3_600_000)), &user("4242 oui"), &real, "e".into()).is_err());
    assert!(c.grant(req(Profile::LiveAuthorizedAutonomy, Some(3_600_000)), &user("9999 J'ACCEPTE LE RISQUE REEL"), &real, "f".into()).is_err());
    let persist = GrantRequest { persist_across_restart: true, ..req(Profile::LiveAuthorizedAutonomy, Some(3_600_000)) };
    assert!(c.grant(persist, &user(phrase), &real, "g".into()).is_err(), "live is never restored after a restart");
    let g = c.grant(req(Profile::LiveAuthorizedAutonomy, Some(3_600_000)), &user(phrase), &real, "ok".into()).unwrap();
    assert_eq!(g.risk_config_hash.as_deref(), Some("abc"));
    assert!(c.check(Capability::OpenPositions, Requester::Brain, &real).allowed);
    // The risk configuration changes: the live grant no longer applies and lapses.
    let changed = AuthorityContext { risk_config_hash: "def".into(), ..real.clone() };
    assert!(!c.check(Capability::OpenPositions, Requester::Brain, &changed).allowed);
    assert!(c.lapse_if_invalid(&changed).unwrap().reason.contains("configuration de risque"));
}

#[test]
fn revocation_is_immediate_and_restart_only_restores_persistent_demo_grants() {
    let demo = ctx(TerminalKind::Mt5, AccountTradeMode::Demo, 7);
    let mut c = Commander::new(Profile::ResearchAutonomy);
    let g = c.grant(GrantRequest { persist_across_restart: true, ..req(Profile::DemoFullAutonomy, None) }, &user(""), &demo, "p".into()).unwrap();
    let revoked = c.revoke("user:api", "fin de test", NOW + 1).unwrap();
    assert_eq!(revoked.revoked_by.as_deref(), Some("user:api"));
    assert!(!c.check(Capability::OpenPositions, Requester::Strategy, &demo).allowed);
    // Restart: a persistent, unrevoked demo grant comes back; a revoked one does not.
    let mut after = Commander::new(Profile::ResearchAutonomy);
    assert!(!after.restore(revoked, NOW));
    assert!(after.restore(g.clone(), NOW));
    assert_eq!(after.profile(&demo), Profile::DemoFullAutonomy);
    let mut live = g;
    live.profile = Profile::LiveAuthorizedAutonomy;
    assert!(!Commander::new(Profile::Observer).restore(live, NOW));
    // A disconnection does not drop an account-bound grant (it waits for the reconnection).
    let gone = AuthorityContext { account_login: None, account_server: None, ..demo };
    assert!(after.lapse_if_invalid(&gone).is_none());
    assert!(after.active_grant().is_some());
}

#[test]
fn risk_hash_is_stable_and_sensitive() {
    let a = risk_config_hash(&serde_json::json!({ "risk_per_trade_pct": 0.5 }));
    assert_eq!(a, risk_config_hash(&serde_json::json!({ "risk_per_trade_pct": 0.5 })));
    assert_ne!(a, risk_config_hash(&serde_json::json!({ "risk_per_trade_pct": 0.6 })));
}
