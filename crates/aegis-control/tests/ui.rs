//! Interface tests without a window: the real engine (and the simulated
//! terminal) produce an `EngineView`; every page is rendered from it.

use aegis_control::ui::{dashboard_tiles, draw, Page, UiAction, UiState, UNAVAILABLE};
use aegis_core::view::EngineView;
use aegis_core::{resolve_shared_key, start, Command, EngineConfig, EngineHandle};
use aegis_data::{BacktestRunRow, Store};
use aegis_simterminal::{spawn, PriceFeed, SimTerminalConfig, SIM_SYMBOL};
use aegis_strategy::StrategyStage;
use aegis_types::*;
use eframe::egui;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

fn config(name: &str) -> EngineConfig {
    let mut cfg = EngineConfig::default();
    cfg.engine.data_dir = std::env::temp_dir().join(format!("aegis-ui-{name}-{}", new_id("t")));
    cfg.engine.signal_timeframes = vec![Timeframe::M5];
    cfg.engine.history_bars = 300;
    cfg.bridge.listen = "127.0.0.1:0".into();
    cfg.bridge.shared_key_env = "AEGIS_UI_TEST_KEY_NOT_SET".into();
    cfg
}

fn state() -> UiState {
    UiState::new("0123456789abcdef0123456789abcdef".into(), "config/aegis.toml".into(), "[engine]\n".into())
}

/// Render every page twice (layout needs two passes) and return the state.
fn render_all(view: &EngineView) -> UiState {
    let ctx = egui::Context::default();
    let mut st = state();
    st.selected_decision = view.decisions.first().map(|d| d.decision_id.clone());
    st.modify_ticket = view.positions.first().map(|p| p.ticket);
    for page in Page::ALL {
        st.page = page;
        for _ in 0..2 {
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))), ..Default::default() };
            let out = ctx.run(input, |ctx| draw(ctx, &mut st, view));
            assert!(!out.shapes.is_empty(), "page {page:?} drew nothing");
        }
    }
    st
}

async fn wait_for(h: &EngineHandle, what: &str, f: impl Fn(&EngineView) -> bool) -> Arc<EngineView> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let v = h.snapshot();
        if f(&v) {
            return v;
        }
        assert!(tokio::time::Instant::now() < deadline, "timeout waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn tile<'a>(tiles: &'a [aegis_control::ui::Tile], label: &str) -> &'a str {
    &tiles.iter().find(|t| t.label == label).unwrap_or_else(|| panic!("no tile `{label}`")).value
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_terminal_nothing_is_invented() {
    let h = start(config("empty")).await.unwrap();
    let v = h.snapshot();
    let tiles = dashboard_tiles(&v);
    let labels: Vec<&str> = tiles.iter().map(|t| t.label).collect();
    // The fourteen values required on the dashboard.
    for expected in [
        "Solde du compte", "Équité", "P&L net journalier", "P&L flottant", "Drawdown", "Spread actuel", "Positions ouvertes",
        "Ordres en attente", "Stratégie active", "Régime de marché", "Connexion MT5", "État du moteur", "Modèle IA", "État des risques",
    ] {
        assert!(labels.contains(&expected), "dashboard lacks `{expected}`");
    }
    for unknown in ["Solde du compte", "Équité", "P&L net journalier", "P&L flottant", "Drawdown", "Spread actuel", "Positions ouvertes", "Ordres en attente", "Régime de marché"] {
        assert_eq!(tile(&tiles, unknown), UNAVAILABLE, "`{unknown}` must not show a made-up value");
    }
    assert_eq!(tile(&tiles, "Connexion MT5"), "Déconnecté");
    assert_eq!(tile(&tiles, "État du moteur"), "Actif — MONITOR");
    assert_eq!(tile(&tiles, "Modèle IA"), "Aucun modèle chargé");
    let st = render_all(&v);
    assert!(st.actions.is_empty(), "rendering alone triggers no action");
    assert_eq!(Page::ALL.len(), 18);
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pages_render_the_real_backend_state_after_a_full_trade() {
    let cfg = config("full");
    // Out-of-sample evidence fixture so the breakout strategy can be promoted and trade.
    {
        let store = Store::open(&cfg.db_path()).await.unwrap();
        store
            .save_backtest(
                &BacktestRunRow {
                    run_id: new_id("bt-fixture"), ts_ms: now_ms(), strategy_id: "donchian_breakout".into(), symbol: SIM_SYMBOL.into(),
                    timeframe: "M5".into(), origin: DataOrigin::Synthetic, params: json!({}),
                    metrics: json!({ "segments": [{ "segment": "oos", "metrics": { "trades": 100, "wins": 60, "net_profit": 500.0 } }] }),
                },
                &[],
            )
            .await
            .unwrap();
        store.close().await;
    }
    let h = start(cfg.clone()).await.unwrap();
    let (psk, _) = resolve_shared_key(&cfg).unwrap();
    let mut sim_cfg = SimTerminalConfig::new(&h.bridge_addr.to_string(), &psk);
    let tf = Timeframe::M5;
    let end = tf.bucket(now_ms());
    sim_cfg.history = (0..300)
        .map(|i| Candle {
            open_ts: end - (300 - i) as i64 * tf.millis(),
            open: if i % 2 == 0 { 1.0998 } else { 1.1002 },
            high: 1.1004,
            low: 1.0996,
            close: if i % 2 == 0 { 1.1002 } else { 1.0998 },
            tick_volume: 20,
            spread_points: 10.0,
        })
        .collect();
    let mut script: Vec<f64> = (0..60).map(|i| if i % 2 == 0 { 1.1000 } else { 1.1001 }).collect();
    script.extend([1.1010, 1.1016, 1.1020, 1.1023, 1.1025]);
    script.extend((1..=80).map(|i| 1.1025 + i as f64 * 0.0001));
    sim_cfg.feed = PriceFeed::Script(script);
    sim_cfg.tick_interval_ms = 12;
    sim_cfg.sim_ms_per_tick = 60_000;
    let sim = spawn(sim_cfg);

    let v = wait_for(&h, "simulator account", |v| v.account.is_some() && !v.markets.is_empty()).await;
    // The dashboard shows exactly what the backend holds.
    let tiles = dashboard_tiles(&v);
    let a = v.account.as_ref().unwrap();
    assert_eq!(tile(&tiles, "Solde du compte"), format!("{:.2} USD", a.balance));
    assert_eq!(tile(&tiles, "Équité"), format!("{:.2} USD", a.equity));
    assert_eq!(tile(&tiles, "Connexion MT5"), "SIMULATEUR (test)", "the simulator is never labelled as MT5");
    assert_eq!(tile(&tiles, "Positions ouvertes"), "0");

    h.command(Command::SetStrategyStage { id: "donchian_breakout".into(), stage: StrategyStage::Demo }).await.unwrap();
    h.command(Command::SetMode { mode: Mode::DemoAuto, confirmation: String::new() }).await.unwrap();
    let v = wait_for(&h, "a closed trade", |v| !v.deals.is_empty() && !v.orders.is_empty()).await;
    let tiles = dashboard_tiles(&v);
    assert_eq!(tile(&tiles, "État du moteur"), "Actif — DEMO AUTO");
    assert_eq!(tile(&tiles, "P&L net journalier"), format!("{:+.2} USD", v.daily_net.unwrap()));
    assert!(tile(&tiles, "Stratégie active").contains("Breakout"));

    // A backtest so the research pages have content too.
    h.command(Command::GenerateSynthetic { symbol: SIM_SYMBOL.into(), timeframe: Timeframe::M15, bars: 3000, seed: 5 }).await.unwrap();
    h.command(Command::RunBacktest { strategy_id: "ema_trend".into(), symbol: SIM_SYMBOL.into(), timeframe: Timeframe::M15, origin: DataOrigin::Synthetic, params: Default::default() }).await.unwrap();
    h.command(Command::Kill { reason: "test de l'interface".into() }).await.unwrap();
    let v = h.snapshot();
    assert!(v.last_backtest.is_some() && v.kill.active && !v.decisions.is_empty());
    assert_eq!(tile(&dashboard_tiles(&v), "État des risques"), "ARRÊT D'URGENCE");

    // Every page renders from this populated state.
    render_all(&v);

    sim.stop().await;
    h.shutdown().await;
}

#[test]
fn ui_actions_are_queued_not_executed_by_the_interface() {
    let mut st = state();
    st.engine("Arrêt d'urgence", Command::Kill { reason: "x".into() });
    st.actions.push(UiAction::StartSimulator);
    assert_eq!(st.actions.len(), 2);
    assert!(matches!(&st.actions[0], UiAction::Engine { label, command: Command::Kill { .. } } if label == "Arrêt d'urgence"));
}
