//! User interface. `draw` is a pure function of the engine view and the UI
//! state: it can be rendered without a window, which is how it is tested.
//!
//! Rule: every number shown comes from `EngineView`. When the engine does not
//! know a value the interface says "Indisponible" — it never shows a default.

mod pages_a;
mod pages_b;

use aegis_backtest::BacktestParams;
use aegis_core::view::{EngineView, HealthStatus};
use aegis_core::Command;
use aegis_risk::RiskConfig;
use aegis_types::{DataOrigin, Mode, TerminalKind, Timeframe};
use eframe::egui::{self, Color32, RichText};

pub const UNAVAILABLE: &str = "Indisponible";

pub const GREEN: Color32 = Color32::from_rgb(64, 190, 120);
pub const RED: Color32 = Color32::from_rgb(230, 80, 80);
pub const AMBER: Color32 = Color32::from_rgb(235, 170, 50);
pub const BLUE: Color32 = Color32::from_rgb(90, 160, 240);
pub const GREY: Color32 = Color32::from_rgb(140, 145, 155);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Market,
    Ai,
    Signals,
    Positions,
    Pending,
    History,
    StrategyLab,
    Backtesting,
    Models,
    Risk,
    Goals,
    MicroProfit,
    Account,
    Mt5,
    System,
    Settings,
    Control,
}

impl Page {
    pub const ALL: [Page; 18] = [
        Page::Dashboard,
        Page::Market,
        Page::Ai,
        Page::Signals,
        Page::Positions,
        Page::Pending,
        Page::History,
        Page::StrategyLab,
        Page::Backtesting,
        Page::Models,
        Page::Risk,
        Page::Goals,
        Page::MicroProfit,
        Page::Account,
        Page::Mt5,
        Page::System,
        Page::Settings,
        Page::Control,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Page::Dashboard => "Tableau de bord",
            Page::Market => "Marché en direct",
            Page::Ai => "Intelligence artificielle",
            Page::Signals => "Signaux de trading",
            Page::Positions => "Positions ouvertes",
            Page::Pending => "Ordres en attente",
            Page::History => "Historique des transactions",
            Page::StrategyLab => "Laboratoire de stratégies",
            Page::Backtesting => "Backtesting",
            Page::Models => "Gestion des modèles IA",
            Page::Risk => "Gestion des risques",
            Page::Goals => "Gestion des objectifs",
            Page::MicroProfit => "Moteur de micro-profits",
            Page::Account => "Gestion du compte",
            Page::Mt5 => "Connexion MT5",
            Page::System => "Surveillance du système",
            Page::Settings => "Paramètres",
            Page::Control => "Centre de contrôle",
        }
    }

    fn group(self) -> &'static str {
        match self {
            Page::Dashboard | Page::Market | Page::Ai | Page::Signals => "Vue d'ensemble",
            Page::Positions | Page::Pending | Page::History => "Trading",
            Page::StrategyLab | Page::Backtesting | Page::Models => "Recherche",
            Page::Risk | Page::Goals | Page::MicroProfit => "Risque",
            _ => "Système",
        }
    }
}

/// Something the user asked for; executed by the application shell.
#[derive(Debug, Clone)]
pub enum UiAction {
    Engine { label: String, command: Command },
    StartSimulator,
    StopSimulator,
}

pub struct BacktestForm {
    pub strategy: String,
    pub symbol: String,
    pub timeframe: Timeframe,
    pub origin: DataOrigin,
    pub params: BacktestParams,
    pub synth_bars: usize,
    pub synth_seed: u64,
    pub csv_path: String,
    pub csv_offset: i64,
    pub export_path: String,
    pub export_horizon: usize,
}

pub struct UiState {
    pub page: Page,
    pub actions: Vec<UiAction>,
    /// Edited copy of the risk configuration; `None` while in sync with the engine.
    pub risk_form: Option<RiskConfig>,
    pub live_confirm: String,
    pub kill_reason: String,
    pub kill_reset: String,
    pub bt: BacktestForm,
    pub model_path: String,
    pub chart_symbol: String,
    pub selected_decision: Option<String>,
    pub modify_sl: String,
    pub modify_tp: String,
    pub modify_ticket: Option<i64>,
    pub simulator_running: bool,
    pub shared_key: String,
    pub show_key: bool,
    pub config_path: String,
    pub config_toml: String,
    /// Latest command outcomes, newest first: (ok, text).
    pub messages: Vec<(bool, String)>,
}

impl UiState {
    pub fn new(shared_key: String, config_path: String, config_toml: String) -> Self {
        UiState {
            page: Page::Dashboard,
            actions: Vec::new(),
            risk_form: None,
            live_confirm: String::new(),
            kill_reason: String::new(),
            kill_reset: String::new(),
            bt: BacktestForm {
                strategy: "ema_trend".into(),
                symbol: String::new(),
                timeframe: Timeframe::M15,
                origin: DataOrigin::Broker,
                params: BacktestParams::default(),
                synth_bars: 20_000,
                synth_seed: 42,
                csv_path: String::new(),
                csv_offset: 0,
                export_path: "research/out/dataset.csv".into(),
                export_horizon: 4,
            },
            model_path: "models/model.json".into(),
            chart_symbol: String::new(),
            selected_decision: None,
            modify_sl: String::new(),
            modify_tp: String::new(),
            modify_ticket: None,
            simulator_running: false,
            shared_key,
            show_key: false,
            config_path,
            config_toml,
            messages: Vec::new(),
        }
    }

    pub fn engine(&mut self, label: &str, command: Command) {
        self.actions.push(UiAction::Engine { label: label.to_string(), command });
    }
}

// ----------------------------------------------------------------- helpers

pub fn money(v: f64) -> String {
    format!("{v:.2}")
}

pub fn signed(v: f64) -> String {
    format!("{v:+.2}")
}

pub fn opt_money(v: Option<f64>) -> String {
    v.map(money).unwrap_or_else(|| UNAVAILABLE.to_string())
}

pub fn opt_num(v: Option<f64>, digits: usize) -> String {
    v.map(|x| format!("{x:.digits$}")).unwrap_or_else(|| UNAVAILABLE.to_string())
}

pub fn pnl_color(v: f64) -> Color32 {
    if v > 0.0 {
        GREEN
    } else if v < 0.0 {
        RED
    } else {
        GREY
    }
}

pub fn health_color(s: HealthStatus) -> Color32 {
    match s {
        HealthStatus::Ok => GREEN,
        HealthStatus::Degraded => AMBER,
        HealthStatus::Down => RED,
        HealthStatus::Inactive => GREY,
    }
}

pub fn mode_color(m: Mode) -> Color32 {
    match m {
        Mode::Monitor => BLUE,
        Mode::DemoAuto => AMBER,
        Mode::LiveAuto => RED,
    }
}

pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.22))
        .stroke(egui::Stroke::new(1.0_f32, color))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color).strong().size(12.0));
        });
}

pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).strong().size(15.0));
    ui.separator();
}

pub fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(GREY).italics());
}

/// Two-column key/value table.
pub fn kv(ui: &mut egui::Ui, id: &str, rows: &[(&str, String)]) {
    egui::Grid::new(id).num_columns(2).striped(true).spacing([24.0, 4.0]).show(ui, |ui| {
        for (k, v) in rows {
            ui.label(RichText::new(*k).color(GREY));
            ui.label(v);
            ui.end_row();
        }
    });
}

pub fn header_row(ui: &mut egui::Ui, cols: &[&str]) {
    for c in cols {
        ui.label(RichText::new(*c).strong().color(GREY));
    }
    ui.end_row();
}

pub fn ts(ms: i64) -> String {
    aegis_types::format_ts(ms)
}

// ------------------------------------------------------------------- shell

/// A dashboard tile: label, displayed value, colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    pub label: &'static str,
    pub value: String,
    pub color: Color32,
}

/// The dashboard content, computed only from the engine view.
pub fn dashboard_tiles(v: &EngineView) -> Vec<Tile> {
    let t = |label, value: String, color| Tile { label, value, color };
    let na = || UNAVAILABLE.to_string();
    let ccy = v.account.as_ref().map(|a| format!(" {}", a.currency)).unwrap_or_default();
    let first_market = v.markets.iter().find(|m| m.tick.is_some());
    let regime = first_market.and_then(|m| v.signal_timeframes.first().map(|tf| m.regime(*tf)));
    let active: Vec<&str> = v.strategies.iter().filter(|s| s.enabled && s.stage != "disabled" && s.stage != "research").map(|s| s.meta.name.as_str()).collect();
    vec![
        t("Solde du compte", v.account.as_ref().map(|a| format!("{}{ccy}", money(a.balance))).unwrap_or_else(na), Color32::WHITE),
        t("Équité", v.account.as_ref().map(|a| format!("{}{ccy}", money(a.equity))).unwrap_or_else(na), Color32::WHITE),
        t("P&L net journalier", v.daily_net.map(|x| format!("{}{ccy}", signed(x))).unwrap_or_else(na), v.daily_net.map(pnl_color).unwrap_or(GREY)),
        t("P&L flottant", v.floating_pnl.map(|x| format!("{}{ccy}", signed(x))).unwrap_or_else(na), v.floating_pnl.map(pnl_color).unwrap_or(GREY)),
        t("Drawdown", v.drawdown_pct.map(|d| format!("{d:.2} %")).unwrap_or_else(na), v.drawdown_pct.map(|d| if d > 0.0 { AMBER } else { GREEN }).unwrap_or(GREY)),
        t(
            "Spread actuel",
            first_market.and_then(|m| m.spread_points.map(|s| format!("{s:.1} pts ({})", m.symbol))).unwrap_or_else(na),
            Color32::WHITE,
        ),
        t("Positions ouvertes", if v.connection.is_some() { v.positions.len().to_string() } else { na() }, Color32::WHITE),
        t("Ordres en attente", if v.connection.is_some() { v.pending.len().to_string() } else { na() }, Color32::WHITE),
        t(
            "Stratégie active",
            if active.is_empty() { "Aucune (toutes en recherche)".to_string() } else { active.join(", ") },
            if active.is_empty() { GREY } else { Color32::WHITE },
        ),
        t("Régime de marché", regime.map(|r| r.label().to_string()).unwrap_or_else(na), Color32::WHITE),
        t(
            "Connexion MT5",
            match &v.connection {
                Some(c) if c.terminal == TerminalKind::Simulator => "SIMULATEUR (test)".to_string(),
                Some(c) => format!("Connecté — compte {}", c.login),
                None => "Déconnecté".to_string(),
            },
            match &v.connection {
                Some(c) if c.terminal == TerminalKind::Simulator => AMBER,
                Some(_) => GREEN,
                None => RED,
            },
        ),
        t("État du moteur", format!("Actif — {}", v.mode.label()), mode_color(v.mode)),
        t(
            "Modèle IA",
            match (&v.model, &v.model_error) {
                (Some(m), _) if m.gate.passed => format!("{} — validé", m.model_id),
                (Some(m), _) => format!("{} — NON validé", m.model_id),
                (None, Some(_)) => "Erreur de chargement".to_string(),
                (None, None) => "Aucun modèle chargé".to_string(),
            },
            match &v.model {
                Some(m) if m.gate.passed => GREEN,
                Some(_) => AMBER,
                None => GREY,
            },
        ),
        t(
            "État des risques",
            if v.kill.active {
                "ARRÊT D'URGENCE".to_string()
            } else if let Some(h) = &v.daily_halt {
                h.clone()
            } else {
                "Limites respectées".to_string()
            },
            if v.kill.active { RED } else if v.daily_halt.is_some() { AMBER } else { GREEN },
        ),
    ]
}

fn top_bar(ctx: &egui::Context, st: &mut UiState, v: &EngineView) {
    egui::TopBottomPanel::top("top").show(ctx, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("AEGIS").strong().size(20.0));
            ui.label(RichText::new(format!("v{}", v.version)).color(GREY));
            ui.separator();
            badge(ui, v.mode.label(), mode_color(v.mode));
            match &v.connection {
                Some(c) if c.terminal == TerminalKind::Simulator => badge(ui, "SIMULATEUR — données fictives", AMBER),
                Some(c) => badge(ui, &format!("MT5 {} ({})", c.login, c.trade_mode.as_str()), GREEN),
                None => badge(ui, "TERMINAL DÉCONNECTÉ", RED),
            }
            if v.kill.active {
                badge(ui, "ARRÊT D'URGENCE ACTIF", RED);
            }
            if let Some(h) = &v.daily_halt {
                badge(ui, h, AMBER);
            }
            if !v.data.db_ok {
                badge(ui, "BASE DE DONNÉES INDISPONIBLE", RED);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let stop = egui::Button::new(RichText::new("ARRÊT D'URGENCE").strong().color(Color32::WHITE)).fill(Color32::from_rgb(170, 40, 40));
                if ui.add_enabled(!v.kill.active, stop).on_hover_text("Bloque immédiatement toute nouvelle entrée").clicked() {
                    st.engine("Arrêt d'urgence", Command::Kill { reason: "bouton d'arrêt d'urgence".into() });
                }
                if let Some(a) = &v.account {
                    ui.label(format!("Équité {} {}", money(a.equity), a.currency));
                }
            });
        });
        ui.add_space(4.0);
    });
}

fn nav(ctx: &egui::Context, st: &mut UiState) {
    egui::SidePanel::left("nav").resizable(false).exact_width(215.0).show(ctx, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut group = "";
            for p in Page::ALL {
                if p.group() != group {
                    group = p.group();
                    ui.add_space(8.0);
                    ui.label(RichText::new(group.to_uppercase()).color(GREY).size(11.0));
                }
                ui.selectable_value(&mut st.page, p, p.title());
            }
        });
    });
}

fn status_bar(ctx: &egui::Context, st: &UiState) {
    egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
        ui.horizontal(|ui| match st.messages.first() {
            Some((ok, text)) => {
                ui.label(RichText::new(if *ok { "OK" } else { "REFUSÉ" }).strong().color(if *ok { GREEN } else { RED }));
                ui.label(text);
            }
            None => {
                ui.label(RichText::new("Prêt").color(GREY));
            }
        });
    });
}

/// Draw the whole application for one frame.
pub fn draw(ctx: &egui::Context, st: &mut UiState, v: &EngineView) {
    top_bar(ctx, st, v);
    status_bar(ctx, st);
    nav(ctx, st);
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading(st.page.title());
        ui.add_space(4.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match st.page {
            Page::Dashboard => pages_a::dashboard(ui, st, v),
            Page::Market => pages_a::market(ui, st, v),
            Page::Ai => pages_a::ai(ui, st, v),
            Page::Signals => pages_a::signals(ui, st, v),
            Page::Positions => pages_a::positions(ui, st, v),
            Page::Pending => pages_a::pending(ui, st, v),
            Page::History => pages_a::history(ui, st, v),
            Page::StrategyLab => pages_b::strategy_lab(ui, st, v),
            Page::Backtesting => pages_b::backtesting(ui, st, v),
            Page::Models => pages_b::models(ui, st, v),
            Page::Risk => pages_b::risk(ui, st, v),
            Page::Goals => pages_b::goals(ui, st, v),
            Page::MicroProfit => pages_b::micro_profit(ui, st, v),
            Page::Account => pages_b::account(ui, st, v),
            Page::Mt5 => pages_b::mt5(ui, st, v),
            Page::System => pages_b::system(ui, st, v),
            Page::Settings => pages_b::settings(ui, st, v),
            Page::Control => pages_b::control(ui, st, v),
        });
    });
}
