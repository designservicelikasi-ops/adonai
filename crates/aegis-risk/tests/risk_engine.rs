use aegis_risk::*;
use aegis_types::*;
use std::collections::HashMap;

const NOW: i64 = 1_800_000_000_000;

fn eurusd() -> SymbolSpec {
    SymbolSpec {
        symbol: "EURUSD".into(),
        digits: 5,
        point: 0.00001,
        contract_size: 100_000.0,
        tick_size: 0.00001,
        tick_value: 1.0,
        volume_min: 0.01,
        volume_max: 100.0,
        volume_step: 0.01,
        stops_level_points: 10.0,
        swap_long: -6.5,
        swap_short: 1.2,
        trade_mode: SymbolTradeMode::Full,
        currency_profit: "USD".into(), currency_base: "EUR".into(), freeze_level_points: 0.0,
    }
}

fn account(equity: f64) -> AccountSnapshot {
    AccountSnapshot {
        ts_ms: NOW - 500,
        login: 5001,
        server: "Demo-Server".into(),
        currency: "USD".into(),
        balance: equity,
        equity,
        margin: 0.0,
        margin_free: equity,
        leverage: 100,
        trade_mode: AccountTradeMode::Demo,
        trade_allowed: true,
    }
}

struct World {
    mode: Mode,
    connected: bool,
    terminal: Option<TerminalKind>,
    account: Option<AccountSnapshot>,
    specs: HashMap<String, SymbolSpec>,
    tick: Option<Tick>,
    positions: Vec<Position>,
    pending: Vec<PendingOrder>,
    day: Option<DayStats>,
    peak: f64,
    live_armed: bool,
}

impl World {
    fn new() -> World {
        let mut specs = HashMap::new();
        specs.insert("EURUSD".to_string(), eurusd());
        World {
            mode: Mode::DemoAuto,
            connected: true,
            terminal: Some(TerminalKind::Mt5),
            account: Some(account(10_000.0)),
            specs,
            tick: Some(Tick { ts_ms: NOW - 200, bid: 1.10000, ask: 1.10010 }),
            positions: vec![],
            pending: vec![],
            day: Some(DayStats::new(NOW, 10_000.0, 10_000.0)),
            peak: 10_000.0,
            live_armed: false,
        }
    }
    fn ctx(&self) -> RiskContext<'_> {
        RiskContext {
            now_ms: NOW,
            mode: self.mode,
            connected: self.connected,
            terminal: self.terminal,
            account: self.account.as_ref(),
            specs: &self.specs,
            tick: self.tick.as_ref(),
            positions: &self.positions,
            pending: &self.pending,
            day: self.day.as_ref(),
            peak_equity: self.peak,
            live_armed: self.live_armed,
        }
    }
}

/// Buy at 1.10010, SL 200 points below, TP 400 points above, p = 0.6.
fn proposal(id: &str) -> TradeProposal {
    TradeProposal {
        decision_id: id.into(),
        strategy_id: "ema_cross".into(),
        symbol: "EURUSD".into(),
        side: Side::Buy,
        kind: OrderKind::Market,
        entry_price: 0.0,
        sl: 1.09810,
        tp: 1.10410,
        win_probability: Some(0.6),
        probability_source: "test".into(),
        claimed_risk_amount: None,
        requested_volume: None,
    }
}

/// Default limits, with probability thresholds lowered to 55 % so the reference proposal (p = 0.6) qualifies.
fn test_config() -> RiskConfig {
    RiskConfig { buy_min_probability: 0.55, sell_min_probability: 0.55, ..Default::default() }
}

fn engine() -> RiskEngine {
    RiskEngine::new(test_config()).unwrap()
}

fn position(ticket: i64, symbol: &str, volume: f64) -> Position {
    Position {
        ticket,
        symbol: symbol.into(),
        side: Side::Buy,
        volume,
        price_open: 1.1,
        sl: 1.09,
        tp: 1.12,
        profit: 0.0,
        swap: 0.0,
        commission: 0.0,
        magic: 0,
        comment: String::new(),
        open_ts: NOW - 60_000,
    }
}

fn closed(ts: i64, profit: f64, commission: f64) -> Deal {
    Deal {
        deal_ticket: ts,
        position_ticket: ts,
        symbol: "EURUSD".into(),
        side: Side::Buy,
        volume: 0.1,
        price: 1.1,
        profit,
        swap: 0.0,
        commission,
        fee: 0.0,
        ts_ms: ts,
        comment: String::new(),
        origin: String::new(),
    }
}

fn failed(a: &RiskAssessment) -> Vec<String> {
    a.failed_checks().iter().map(|c| c.name.clone()).collect()
}

fn assert_rejected_for(a: &RiskAssessment, name: &str) {
    assert!(!a.approved, "expected a rejection");
    assert!(a.authorization.is_none(), "a rejected proposal must never carry an authorization");
    assert!(failed(a).contains(&name.to_string()), "expected `{name}` among {:?}", failed(a));
}

#[test]
fn valid_proposal_is_approved_and_sized_from_broker_spec() {
    let w = World::new();
    let mut e = engine();
    let a = e.evaluate(&proposal("d1"), &w.ctx());
    assert!(a.approved, "{}", a.rejection_summary());
    // Budget 0.5% of 10 000 = 50. Loss per lot = 200 (SL) + 7 (commission) + 2 (slippage) = 209.
    // 50 / 209 = 0.239 -> rounded DOWN to 0.23 lot.
    assert_eq!(a.volume, 0.23);
    assert!((a.risk_amount - 0.23 * 209.0).abs() < 1e-6);
    assert!(a.risk_amount <= a.risk_budget);
    let auth = a.authorization.as_ref().unwrap();
    assert_eq!(auth.volume, 0.23);
    assert_eq!(auth.decision_id, "d1");
    assert_eq!(auth.expires_ms, NOW + 3_000);
    let micro = a.micro.unwrap();
    assert!(micro.expected_net.unwrap() > 0.5);
}

#[test]
fn sizing_uses_tick_value_for_non_usd_quote() {
    // USDJPY-like: 3 digits, tick value 0.65 USD per 0.001 per lot.
    let mut w = World::new();
    let spec = SymbolSpec {
        symbol: "USDJPY".into(),
        digits: 3,
        point: 0.001,
        tick_size: 0.001,
        tick_value: 0.65,
        ..eurusd()
    };
    w.specs.insert("USDJPY".into(), spec);
    w.tick = Some(Tick { ts_ms: NOW - 100, bid: 150.000, ask: 150.010 });
    let mut p = proposal("jpy");
    p.symbol = "USDJPY".into();
    p.sl = 149.810; // 200 points
    p.tp = 150.610;
    let mut cfg = test_config();
    cfg.max_notional_leverage = 50.0;
    let mut e = RiskEngine::new(cfg).unwrap();
    let a = e.evaluate(&p, &w.ctx());
    assert!(a.approved, "{}", a.rejection_summary());
    // Loss per lot = 200 * 0.65 + 7 + 2 * 0.65 = 138.3 ; 50 / 138.3 = 0.3615 -> 0.36
    assert_eq!(a.volume, 0.36);
}

#[test]
fn monitor_mode_never_authorizes_but_still_explains() {
    let mut w = World::new();
    w.mode = Mode::Monitor;
    let mut e = engine();
    let a = e.evaluate(&proposal("d1"), &w.ctx());
    assert_rejected_for(&a, "mode");
    assert_eq!(failed(&a), vec!["mode"], "everything else passes");
    assert!(a.micro.is_some(), "costs are still computed for explanation");
}

#[test]
fn kill_switch_blocks_everything_until_manual_reset() {
    let w = World::new();
    let mut e = engine();
    let plan = e.engage_kill("test", "user", NOW);
    assert!(plan.cancel_pending);
    assert!(!plan.close_positions, "default policy keeps positions");
    assert_rejected_for(&e.evaluate(&proposal("d1"), &w.ctx()), "arret_urgence");
    // A second engagement keeps the original reason.
    e.engage_kill("other", "system", NOW + 1);
    assert_eq!(e.kill_state().reason, "test");
    assert!(e.reset_kill("please").is_err());
    assert!(e.kill_state().active);
    e.reset_kill(KILL_RESET_PHRASE).unwrap();
    assert!(e.reset_kill(KILL_RESET_PHRASE).is_err(), "nothing to reset");
    assert!(e.evaluate(&proposal("d2"), &w.ctx()).approved);
}

#[test]
fn kill_policy_close_is_configurable_and_state_restorable() {
    let cfg = RiskConfig { kill_position_policy: KillPositionPolicy::Close, kill_cancel_pending: false, ..test_config() };
    let mut e = RiskEngine::new(cfg).unwrap();
    let plan = e.engage_kill("dd", "system", NOW);
    assert!(plan.close_positions && !plan.cancel_pending);
    let saved = e.kill_state().clone();
    let mut e2 = engine();
    e2.restore_kill_state(saved);
    assert_rejected_for(&e2.evaluate(&proposal("d1"), &World::new().ctx()), "arret_urgence");
}

#[test]
fn connection_account_and_market_data_must_be_valid_and_fresh() {
    let mut e = engine();

    let mut w = World::new();
    w.connected = false;
    assert_rejected_for(&e.evaluate(&proposal("a"), &w.ctx()), "connexion");

    let mut w = World::new();
    w.account = None;
    assert_rejected_for(&e.evaluate(&proposal("b"), &w.ctx()), "compte_valide");

    let mut w = World::new();
    w.account.as_mut().unwrap().ts_ms = NOW - 60_000;
    assert_rejected_for(&e.evaluate(&proposal("c"), &w.ctx()), "compte_frais");

    let mut w = World::new();
    w.account.as_mut().unwrap().equity = f64::NAN;
    assert_rejected_for(&e.evaluate(&proposal("d"), &w.ctx()), "compte_valide");

    let mut w = World::new();
    w.account.as_mut().unwrap().trade_allowed = false;
    assert_rejected_for(&e.evaluate(&proposal("e"), &w.ctx()), "compte_trading_autorise");

    let mut w = World::new();
    w.tick = None;
    assert_rejected_for(&e.evaluate(&proposal("f"), &w.ctx()), "prix_valide");

    let mut w = World::new();
    w.tick.as_mut().unwrap().ts_ms = NOW - 30_000;
    assert_rejected_for(&e.evaluate(&proposal("g"), &w.ctx()), "prix_frais");

    // A timestamp from the future is as suspicious as a stale one.
    let mut w = World::new();
    w.tick.as_mut().unwrap().ts_ms = NOW + 60_000;
    assert_rejected_for(&e.evaluate(&proposal("h"), &w.ctx()), "prix_frais");

    let mut w = World::new();
    w.tick = Some(Tick { ts_ms: NOW, bid: 1.2, ask: 1.1 });
    assert_rejected_for(&e.evaluate(&proposal("i"), &w.ctx()), "prix_valide");
}

#[test]
fn instrument_checks() {
    let mut e = engine();
    let mut w = World::new();
    w.specs.clear();
    assert_rejected_for(&e.evaluate(&proposal("a"), &w.ctx()), "instrument_spec");

    let mut w = World::new();
    w.specs.get_mut("EURUSD").unwrap().trade_mode = SymbolTradeMode::CloseOnly;
    assert_rejected_for(&e.evaluate(&proposal("b"), &w.ctx()), "instrument_negociable");

    let mut w = World::new();
    w.specs.get_mut("EURUSD").unwrap().tick_value = 0.0;
    assert_rejected_for(&e.evaluate(&proposal("c"), &w.ctx()), "instrument_spec");

    let mut w = World::new();
    w.tick = Some(Tick { ts_ms: NOW, bid: 1.10000, ask: 1.10040 }); // 40 points
    assert_rejected_for(&e.evaluate(&proposal("d"), &w.ctx()), "spread");
}

#[test]
fn stop_and_target_levels_are_mandatory_and_coherent() {
    let mut e = engine();
    let w = World::new();

    let mut p = proposal("a");
    p.sl = 0.0;
    assert_rejected_for(&e.evaluate(&p, &w.ctx()), "niveaux_valides");

    let mut p = proposal("b");
    p.sl = 1.10500; // above the entry of a buy
    assert_rejected_for(&e.evaluate(&p, &w.ctx()), "niveaux_coherents");

    let mut p = proposal("c");
    p.sl = 1.09995; // 5 points from the bid, broker minimum is 10
    assert_rejected_for(&e.evaluate(&p, &w.ctx()), "distance_stops");

    // Mirror case for a sell.
    let mut p = proposal("d");
    p.side = Side::Sell;
    p.sl = 1.10200;
    p.tp = 1.09600;
    assert!(e.evaluate(&p, &w.ctx()).approved);

    // Pending order priced on the wrong side of the market.
    let mut p = proposal("e");
    p.kind = OrderKind::Limit;
    p.entry_price = 1.10200; // a buy limit must be below the market
    p.sl = 1.10000;
    p.tp = 1.10600;
    assert_rejected_for(&e.evaluate(&p, &w.ctx()), "prix_ordre_attente");

    let mut p = proposal("f");
    p.kind = OrderKind::Limit;
    p.entry_price = 1.09900;
    p.sl = 1.09700;
    p.tp = 1.10300;
    let a = e.evaluate(&p, &w.ctx());
    assert!(a.approved, "{}", a.rejection_summary());
    assert_eq!(a.authorization.unwrap().price, 1.09900);
}

#[test]
fn net_expectation_gate() {
    let mut e = engine();
    let w = World::new();

    let mut p = proposal("a");
    p.win_probability = None;
    assert_rejected_for(&e.evaluate(&p, &w.ctx()), "esperance_nette");

    // 2:1 payoff needs more than ~35% once costs are paid; 30% is a negative expectation.
    let mut p = proposal("b");
    p.win_probability = Some(0.30);
    let a = e.evaluate(&p, &w.ctx());
    assert_rejected_for(&a, "esperance_nette");
    assert!(a.micro.unwrap().expected_net.unwrap() < 0.0);

    // Positive but below the configured minimum.
    let cfg = RiskConfig { min_net_expected_profit: 1_000.0, ..test_config() };
    let mut strict = RiskEngine::new(cfg).unwrap();
    assert_rejected_for(&strict.evaluate(&proposal("c"), &w.ctx()), "esperance_nette");
}

#[test]
fn daily_loss_profit_target_and_drawdown_stop_new_entries() {
    let mut e = engine();

    let mut w = World::new();
    w.account = Some(account(9_790.0)); // -2.1 % on the day, floating included
    assert_rejected_for(&e.evaluate(&proposal("a"), &w.ctx()), "perte_journaliere");

    // Daily goal (15 net by default) reached on closed trades -> stop, do not push further.
    let mut w = World::new();
    w.day.as_mut().unwrap().on_deal(&closed(NOW - 1_000, 16.0, -0.5));
    w.account = Some(account(10_015.5));
    w.peak = 10_015.5;
    assert_rejected_for(&e.evaluate(&proposal("b"), &w.ctx()), "objectif_journalier");

    // A deposit is not profit: the goal is not considered reached.
    let mut w = World::new();
    w.account = Some(account(10_500.0));
    w.peak = 10_500.0;
    w.day.as_mut().unwrap().on_balance_operation(500.0);
    assert!(e.evaluate(&proposal("c"), &w.ctx()).approved);

    let mut w = World::new();
    w.peak = 11_200.0; // equity 10 000 => 10.7 % below the peak
    assert_rejected_for(&e.evaluate(&proposal("d"), &w.ctx()), "drawdown");

    let mut w = World::new();
    w.day = None;
    assert_rejected_for(&e.evaluate(&proposal("e"), &w.ctx()), "comptabilite_journaliere");
}

#[test]
fn position_count_and_exposure_limits() {
    let mut e = engine();

    let mut w = World::new();
    w.specs.insert("GBPUSD".into(), SymbolSpec { symbol: "GBPUSD".into(), ..eurusd() });
    w.positions = vec![position(1, "GBPUSD", 0.01), position(2, "GBPUSD", 0.01), position(3, "GBPUSD", 0.01)];
    assert_rejected_for(&e.evaluate(&proposal("a"), &w.ctx()), "positions_max");

    let mut w = World::new();
    w.positions = vec![position(1, "EURUSD", 0.01)];
    assert_rejected_for(&e.evaluate(&proposal("b"), &w.ctx()), "positions_par_instrument");

    // Existing 0.45 lot (~49 500 notional) + new 0.23 lot (~25 300) > 5x of 10 000 equity.
    let mut w = World::new();
    w.specs.insert("GBPUSD".into(), SymbolSpec { symbol: "GBPUSD".into(), ..eurusd() });
    w.positions = vec![position(1, "GBPUSD", 0.45)];
    assert_rejected_for(&e.evaluate(&proposal("c"), &w.ctx()), "exposition");

    // A position on an instrument without specification makes exposure unknowable.
    let mut w = World::new();
    w.positions = vec![position(1, "XAUUSD", 0.01)];
    assert_rejected_for(&e.evaluate(&proposal("d"), &w.ctx()), "exposition");

    let mut w = World::new();
    w.account.as_mut().unwrap().margin_free = 100.0;
    assert_rejected_for(&e.evaluate(&proposal("e"), &w.ctx()), "marge_libre");
}

#[test]
fn volume_is_never_rounded_up_and_requests_can_only_reduce_it() {
    let mut e = engine();

    // 100 of equity => budget 0.50; minimum lot would risk 2.09: refuse instead of rounding up.
    let mut w = World::new();
    w.account = Some(account(100.0));
    w.day = Some(DayStats::new(NOW, 100.0, 100.0));
    w.peak = 100.0;
    let a = e.evaluate(&proposal("a"), &w.ctx());
    assert_rejected_for(&a, "taille_position");
    assert_eq!(a.volume, 0.0);

    let w = World::new();
    let mut p = proposal("b");
    p.requested_volume = Some(0.05);
    assert_eq!(e.evaluate(&p, &w.ctx()).volume, 0.05);

    let mut p = proposal("c");
    p.requested_volume = Some(50.0); // cannot increase beyond the risk-based size
    assert_eq!(e.evaluate(&p, &w.ctx()).volume, 0.23);
}

#[test]
fn externally_claimed_risk_is_not_trusted() {
    let mut e = engine();
    let w = World::new();
    let mut p = proposal("a");
    p.claimed_risk_amount = Some(1.0); // wildly understated
    let a = e.evaluate(&p, &w.ctx());
    assert!(a.approved);
    assert!((a.risk_amount - 48.07).abs() < 1e-6, "the engine's own figure is applied");
    let note = a.checks.iter().find(|c| c.name == "risque_declare").unwrap();
    assert!(note.detail.contains("ignoré"));
}

#[test]
fn duplicate_decisions_are_rejected() {
    let mut e = engine();
    let w = World::new();
    assert!(e.evaluate(&proposal("same"), &w.ctx()).approved);
    assert_rejected_for(&e.evaluate(&proposal("same"), &w.ctx()), "decision_unique");
    assert_rejected_for(&e.evaluate(&proposal(""), &w.ctx()), "decision_unique");
}

#[test]
fn demo_mode_refuses_real_accounts_and_live_needs_explicit_arming() {
    let mut e = engine();

    let mut w = World::new();
    w.account.as_mut().unwrap().trade_mode = AccountTradeMode::Real;
    assert_rejected_for(&e.evaluate(&proposal("a"), &w.ctx()), "mode_compte");

    // LIVE AUTO on a real account but not armed.
    let mut w = World::new();
    w.mode = Mode::LiveAuto;
    w.account.as_mut().unwrap().trade_mode = AccountTradeMode::Real;
    assert_rejected_for(&e.evaluate(&proposal("b"), &w.ctx()), "mode_compte");

    // Armed, but the account is a demo account.
    let mut w = World::new();
    w.mode = Mode::LiveAuto;
    w.live_armed = true;
    assert_rejected_for(&e.evaluate(&proposal("c"), &w.ctx()), "mode_compte");

    // Armed + real, but the peer is the simulator.
    let mut w = World::new();
    w.mode = Mode::LiveAuto;
    w.live_armed = true;
    w.terminal = Some(TerminalKind::Simulator);
    w.account.as_mut().unwrap().trade_mode = AccountTradeMode::Real;
    assert_rejected_for(&e.evaluate(&proposal("d"), &w.ctx()), "mode_compte");

    let mut w = World::new();
    w.mode = Mode::LiveAuto;
    w.live_armed = true;
    w.account.as_mut().unwrap().trade_mode = AccountTradeMode::Real;
    assert!(e.evaluate(&proposal("e"), &w.ctx()).approved);

    // The simulator is accepted in DEMO AUTO whatever account type it reports.
    let mut w = World::new();
    w.terminal = Some(TerminalKind::Simulator);
    w.account.as_mut().unwrap().trade_mode = AccountTradeMode::Real;
    assert!(e.evaluate(&proposal("f"), &w.ctx()).approved);
}

#[test]
fn authorization_is_single_use_expiring_and_tamper_proof() {
    let mut e = engine();
    let w = World::new();
    let auth = e.evaluate(&proposal("a"), &w.ctx()).authorization.unwrap();

    // Tampering with any field invalidates it.
    let mut forged = auth.clone();
    forged.volume = 5.0;
    assert_eq!(e.consume_authorization(&forged, NOW + 10), Err(AuthError::BadSignature));
    let mut forged = auth.clone();
    forged.sl = 1.0;
    assert_eq!(e.consume_authorization(&forged, NOW + 10), Err(AuthError::BadSignature));
    let mut forged = auth.clone();
    forged.mac = "zz".into();
    assert_eq!(e.consume_authorization(&forged, NOW + 10), Err(AuthError::BadSignature));

    assert_eq!(e.consume_authorization(&auth, NOW + 3_001), Err(AuthError::Expired));
    assert_eq!(e.consume_authorization(&auth, NOW + 10), Ok(()));
    assert_eq!(e.consume_authorization(&auth, NOW + 20), Err(AuthError::AlreadyUsed));

    // An authorization minted by another engine instance is worthless here.
    let mut other = engine();
    let foreign = other.evaluate(&proposal("b"), &w.ctx()).authorization.unwrap();
    assert_eq!(e.consume_authorization(&foreign, NOW + 10), Err(AuthError::BadSignature));

    // The emergency stop voids outstanding authorizations.
    let pending = e.evaluate(&proposal("c"), &w.ctx()).authorization.unwrap();
    e.engage_kill("stop", "user", NOW);
    assert_eq!(e.consume_authorization(&pending, NOW + 10), Err(AuthError::KillSwitchActive));
}

#[test]
fn invalid_configurations_are_refused() {
    let bad = [
        RiskConfig { risk_per_trade_pct: 0.0, ..Default::default() },
        RiskConfig { risk_per_trade_pct: 25.0, ..Default::default() },
        RiskConfig { risk_per_trade_pct: 3.0, max_daily_loss_pct: 2.0, ..Default::default() },
        RiskConfig { max_daily_loss_pct: f64::NAN, ..Default::default() },
        RiskConfig { max_open_positions: 0, ..Default::default() },
        RiskConfig { max_tick_age_ms: 0, ..Default::default() },
        RiskConfig { authorization_ttl_ms: 3_600_000, ..Default::default() },
        RiskConfig { max_drawdown_pct: 90.0, ..Default::default() },
    ];
    for cfg in bad {
        assert!(RiskEngine::new(cfg.clone()).is_err(), "{cfg:?} should be refused");
    }
    let mut e = engine();
    let before = e.config().clone();
    assert!(e.set_config(RiskConfig { risk_per_trade_pct: 50.0, ..Default::default() }).is_err());
    assert_eq!(e.config(), &before, "a refused configuration leaves the old one in place");
    // Unknown fields in a stored configuration are an error, not silently dropped.
    assert!(serde_json::from_str::<RiskConfig>(r#"{"risk_per_trade_pct":0.5,"typo_field":1}"#).is_err());
}

#[test]
fn lot_cap_only_reduces_the_size() {
    let w = World::new();
    let free = engine().evaluate(&proposal("a"), &w.ctx());
    assert!(free.approved && free.volume > 0.05, "{}", free.volume);
    let mut capped = RiskEngine::new(RiskConfig { max_lots_per_trade: 0.05, ..test_config() }).unwrap();
    let a = capped.evaluate(&proposal("b"), &w.ctx());
    assert!(a.approved);
    assert!((a.volume - 0.05).abs() < 1e-9, "{}", a.volume);
    assert!(a.risk_amount < free.risk_amount, "smaller size, smaller risk");
    // A cap above the risk-based size changes nothing.
    let mut loose = RiskEngine::new(RiskConfig { max_lots_per_trade: 500.0, ..test_config() }).unwrap();
    assert_eq!(loose.evaluate(&proposal("c"), &w.ctx()).volume, free.volume);
}

#[test]
fn probability_thresholds_are_per_side_and_never_lowered() {
    let w = World::new();
    // Reference proposal: BUY with p = 0.6.
    let mut strict = RiskEngine::new(RiskConfig { buy_min_probability: 0.75, ..test_config() }).unwrap();
    assert_rejected_for(&strict.evaluate(&proposal("a"), &w.ctx()), "seuil_probabilite");
    let mut p = proposal("b");
    p.win_probability = Some(0.80);
    assert!(strict.evaluate(&p, &w.ctx()).approved, "80 % passes a 75 % BUY threshold");
    // The SELL threshold is independent of the BUY one.
    let mut e = RiskEngine::new(RiskConfig { buy_min_probability: 0.55, sell_min_probability: 0.9, ..Default::default() }).unwrap();
    let mut sell = proposal("c");
    sell.side = Side::Sell;
    sell.sl = 1.10200;
    sell.tp = 1.09600;
    assert_rejected_for(&e.evaluate(&sell, &w.ctx()), "seuil_probabilite");
    assert!(e.evaluate(&proposal("d"), &w.ctx()).approved);
}

#[test]
fn trade_count_losing_streak_cooldown_and_order_rate() {
    let mut w = World::new();
    let mut e = RiskEngine::new(RiskConfig { max_trades_per_day: 2, ..test_config() }).unwrap();
    w.day.as_mut().unwrap().on_entry_sent(0.1);
    assert!(e.evaluate(&proposal("a"), &w.ctx()).approved);
    w.day.as_mut().unwrap().on_entry_sent(0.1);
    assert_rejected_for(&e.evaluate(&proposal("b"), &w.ctx()), "transactions_max");

    // Three losses in a row with a 3-loss limit: 30 min pause from the last loss.
    let mut e = RiskEngine::new(RiskConfig { max_consecutive_losses: 3, loss_cooldown_minutes: 30, ..test_config() }).unwrap();
    let mut w = World::new();
    for i in 0..3 {
        w.day.as_mut().unwrap().on_deal(&closed(NOW - 10 * 60_000 + i, -1.0, 0.0));
    }
    w.account = Some(account(9_997.0));
    assert_rejected_for(&e.evaluate(&proposal("c"), &w.ctx()), "pertes_consecutives");
    let mut later = World::new();
    later.day = w.day.clone();
    later.account = Some(account(9_997.0));
    // 31 minutes after the last loss the pause is over.
    for d in later.day.iter_mut() {
        d.last_loss_ms = NOW - 31 * 60_000;
    }
    assert!(e.evaluate(&proposal("d"), &later.ctx()).approved);

    // Order rate: each consumed authorization counts for one rolling minute.
    let mut e = RiskEngine::new(RiskConfig { max_orders_per_minute: 2, ..test_config() }).unwrap();
    let w = World::new();
    for id in ["e", "f"] {
        let a = e.evaluate(&proposal(id), &w.ctx());
        e.consume_authorization(a.authorization.as_ref().unwrap(), NOW).unwrap();
    }
    assert_rejected_for(&e.evaluate(&proposal("g"), &w.ctx()), "debit_ordres");
}

#[test]
fn costs_eating_the_gains_stop_entries() {
    let mut e = RiskEngine::new(RiskConfig { max_cost_to_gross_ratio: 0.5, cost_check_min_trades: 3, daily_goal: DailyGoal { enabled: false, ..Default::default() }, ..test_config() }).unwrap();
    let mut w = World::new();
    // Three micro-wins of 0.10 gross, each paying 0.07 commission: costs are 70 % of the gross.
    for i in 0..3 {
        w.day.as_mut().unwrap().on_deal(&closed(NOW - 60_000 + i, 0.10, -0.07));
    }
    assert_rejected_for(&e.evaluate(&proposal("a"), &w.ctx()), "couts_vs_gains");
}

#[test]
fn trading_windows_and_currency_exposure() {
    let w = World::new();
    // NOW = 1 800 000 000 000 ms -> 08:00 UTC.
    let inside = RiskConfig { trading_windows: vec![TradingWindow { start: "07:00".into(), end: "17:00".into() }], ..test_config() };
    assert!(RiskEngine::new(inside).unwrap().evaluate(&proposal("a"), &w.ctx()).approved);
    let outside = RiskConfig { trading_windows: vec![TradingWindow { start: "12:00".into(), end: "17:00".into() }], ..test_config() };
    assert_rejected_for(&RiskEngine::new(outside).unwrap().evaluate(&proposal("b"), &w.ctx()), "horaires");

    // Existing USD exposure through GBPUSD counts against the USD limit of a new EURUSD trade.
    let mut w = World::new();
    w.specs.insert("GBPUSD".into(), SymbolSpec { symbol: "GBPUSD".into(), currency_base: "GBP".into(), ..eurusd() });
    w.positions = vec![position(1, "GBPUSD", 0.3)];
    let mut e = RiskEngine::new(RiskConfig { max_currency_exposure: 4.0, max_notional_leverage: 50.0, max_open_positions: 10, ..test_config() }).unwrap();
    let a = e.evaluate(&proposal("c"), &w.ctx());
    assert_rejected_for(&a, "exposition_devise");
    let detail = &a.checks.iter().find(|c| c.name == "exposition_devise").unwrap().detail;
    assert!(detail.starts_with("USD"), "{detail}");
}

#[test]
fn currencies_are_taken_from_the_spec_or_the_fx_name() {
    assert_eq!(currencies_of(&eurusd()), vec!["EUR".to_string(), "USD".to_string()]);
    let unknown = SymbolSpec { symbol: "GBPJPY.r".into(), currency_base: String::new(), currency_profit: "JPY".into(), ..eurusd() };
    assert_eq!(currencies_of(&unknown), vec!["GBP".to_string(), "JPY".to_string()]);
    let index = SymbolSpec { symbol: "US500".into(), currency_base: String::new(), currency_profit: "USD".into(), ..eurusd() };
    assert_eq!(currencies_of(&index), vec!["US500".to_string(), "USD".to_string()]);
}
