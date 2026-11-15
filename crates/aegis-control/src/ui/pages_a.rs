//! Overview and trading pages.

use super::*;
use aegis_core::view::DecisionView;
use egui_plot::{Line, Plot, PlotPoints};

pub fn dashboard(ui: &mut egui::Ui, _st: &mut UiState, v: &EngineView) {
    if v.is_simulator() {
        note(ui, "Simulateur connecté: les prix, le compte et les transactions affichés sont fictifs (adaptateur de test).");
    }
    let tiles = dashboard_tiles(v);
    egui::Grid::new("tiles").num_columns(4).spacing([14.0, 14.0]).show(ui, |ui| {
        for (i, t) in tiles.iter().enumerate() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_width(215.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(t.label).color(GREY).size(12.0));
                    ui.label(RichText::new(&t.value).color(t.color).strong().size(16.0));
                });
            });
            if i % 4 == 3 {
                ui.end_row();
            }
        }
    });

    section(ui, "Équité");
    if v.equity_history.len() >= 2 {
        let pts: PlotPoints = v.equity_history.iter().map(|(t, e)| [*t as f64 / 1000.0, *e]).collect();
        Plot::new("equity").height(190.0).allow_scroll(false).show_x(false).show(ui, |p| p.line(Line::new(pts)));
    } else {
        note(ui, "Courbe d'équité indisponible: aucun instantané de compte enregistré.");
    }

    section(ui, "Derniers événements");
    for l in v.logs.iter().take(8) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(ts(l.ts_ms)).color(GREY).monospace());
            let c = match l.level.as_str() {
                "error" => RED,
                "warn" => AMBER,
                _ => Color32::LIGHT_GRAY,
            };
            ui.label(RichText::new(&l.text).color(c));
        });
    }
    if v.logs.is_empty() {
        note(ui, "Aucun événement.");
    }
}

pub fn market(ui: &mut egui::Ui, st: &mut UiState, v: &EngineView) {
    if v.markets.is_empty() {
        note(ui, "Aucune donnée de marché: connectez le terminal MT5 (page « Connexion MT5 »).");
        return;
    }
    egui::Grid::new("quotes").num_columns(8).striped(true).spacing([18.0, 6.0]).show(ui, |ui| {
        header_row(ui, &["Instrument", "Bid", "Ask", "Spread (pts)", "Session", "Ticks reçus", "Qualité des données", ""]);
        for m in &v.markets {
            ui.label(RichText::new(&m.symbol).strong());
            match &m.tick {
                Some(t) => {
                    ui.monospace(format!("{:.5}", t.bid));
                    ui.monospace(format!("{:.5}", t.ask));
                }
                None => {
                    ui.label(UNAVAILABLE);
                    ui.label(UNAVAILABLE);
                }
            }
            ui.label(opt_num(m.spread_points, 1));
            ui.label(m.session.label());
            ui.label(m.ticks_received.to_string());
            if m.issues.is_empty() {
                ui.label(RichText::new("Valides").color(GREEN));
            } else {
                let text = m.issues.iter().map(|i| i.describe()).collect::<Vec<_>>().join(" ; ");
                ui.label(RichText::new(text).color(if m.tradable() { AMBER } else { RED }));
            }
            if ui.small_button("Graphique").clicked() {
                st.chart_symbol = m.symbol.clone();
            }
            ui.end_row();
        }
    });

    let symbol = if v.markets.iter().any(|m| m.symbol == st.chart_symbol) { st.chart_symbol.clone() } else { v.markets[0].symbol.clone() };
    let Some(m) = v.market(&symbol) else { return };

    section(ui, &format!("Indicateurs — {symbol}"));
    egui::Grid::new("indicators").num_columns(8).striped(true).spacing([18.0, 6.0]).show(ui, |ui| {
        header_row(ui, &["Unité", "Bougies", "EMA 20", "EMA 50", "RSI 14", "ATR 14", "Volatilité", "Régime"]);
        for i in &m.indicators {
            ui.label(i.timeframe.as_str());
            ui.label(i.bars.to_string());
            ui.label(opt_num(i.ema_fast, 5));
            ui.label(opt_num(i.ema_slow, 5));
            ui.label(opt_num(i.rsi, 1));
            ui.label(opt_num(i.atr, 5));
            ui.label(i.volatility.map(|x| format!("{:.4} %", x * 100.0)).unwrap_or_else(|| UNAVAILABLE.into()));
            ui.label(i.regime.regime.label());
            ui.end_row();
        }
    });
    note(ui, "« Indisponible » signifie que l'historique reçu est encore trop court pour calculer l'indicateur.");

    for (s, tf, closes) in v.charts.iter().filter(|(s, _, _)| *s == symbol) {
        section(ui, &format!("Clôtures {s} — {tf}"));
        if closes.len() >= 2 {
            let pts: PlotPoints = closes.iter().map(|(t, c)| [*t as f64 / 1000.0, *c]).collect();
            Plot::new(format!("chart-{s}-{tf}")).height(230.0).allow_scroll(false).show_x(false).show(ui, |p| p.line(Line::new(pts)));
        } else {
            note(ui, "Pas encore assez de bougies pour tracer un graphique.");
        }
    }
}

pub fn ai(ui: &mut egui::Ui, st: &mut UiState, v: &EngineView) {
    note(ui, "AEGIS utilise un modèle statistique entraîné sur des données historiques. Il estime une probabilité; il ne prédit pas l'avenir et ne garantit rien.");
    let Some(m) = &v.model else {
        section(ui, "État");
        match &v.model_error {
            Some(e) => {
                ui.label(RichText::new(format!("Le modèle n'a pas pu être chargé: {e}")).color(RED));
            }
            None => {
                ui.label("Aucun modèle n'est chargé. Les décisions reposent uniquement sur les stratégies et le moteur de risque.");
            }
        }
        if ui.button("Aller à la gestion des modèles").clicked() {
            st.page = Page::Models;
        }
        return;
    };
    section(ui, "Modèle en service");
    ui.horizontal(|ui| {
        ui.label(RichText::new(&m.model_id).strong());
        if m.gate.passed {
            badge(ui, "VALIDÉ — peut filtrer les entrées", GREEN);
        } else {
            badge(ui, "NON VALIDÉ — affichage indicatif uniquement", AMBER);
        }
    });
    kv(ui, "ai-model", &[
        ("Cible estimée", m.target.clone()),
        ("Instrument / unité", format!("{} {}", m.symbol, m.timeframe)),
        ("Horizon", format!("{} bougies", m.horizon_bars)),
        ("Origine des données d'entraînement", if m.origin == DataOrigin::Synthetic { "SYNTHÉTIQUES".into() } else { "Broker".into() }),
    ]);

    section(ui, "Dernière estimation");
    match m.last_prediction {
        Some((t, p)) => {
            ui.label(RichText::new(format!("Probabilité de hausse sur l'horizon: {:.1} %", p * 100.0)).size(18.0).color(if m.gate.passed { Color32::WHITE } else { GREY }));
            ui.label(format!("Calculée sur la bougie du {}", ts(t)));
            if !m.gate.passed {
                note(ui, "Ce chiffre provient d'un modèle qui n'a pas passé la validation: il n'a aucun effet sur l'exécution.");
            }
        }
        None => note(ui, "Aucune estimation: en attente d'une bougie clôturée sur l'instrument du modèle."),
    }

    section(ui, "Performance réelle depuis le chargement");
    kv(ui, "ai-perf", &[
        ("Estimations vérifiées", m.performance.resolved.to_string()),
        ("En attente de leur horizon", m.performance.pending.to_string()),
        ("Score de Brier en direct", opt_num(m.performance.brier, 4)),
        ("Taux de bonne direction", m.performance.accuracy.map(|a| format!("{:.1} %", a * 100.0)).unwrap_or_else(|| UNAVAILABLE.into())),
    ]);

    section(ui, "Dérive des variables (PSI)");
    if m.drift.psi.is_empty() {
        note(ui, &format!("Indisponible: {} échantillons en direct (100 nécessaires) ou modèle sans référence de dérive.", m.drift.samples));
    } else {
        egui::Grid::new("psi").num_columns(3).striped(true).show(ui, |ui| {
            header_row(ui, &["Variable", "PSI", "État"]);
            for (name, psi) in &m.drift.psi {
                ui.label(name);
                ui.label(format!("{psi:.3}"));
                if *psi > 0.25 {
                    ui.label(RichText::new("Dérive significative").color(RED));
                } else if *psi > 0.1 {
                    ui.label(RichText::new("Dérive modérée").color(AMBER));
                } else {
                    ui.label(RichText::new("Stable").color(GREEN));
                }
                ui.end_row();
            }
        });
    }
}

fn outcome_badge(ui: &mut egui::Ui, d: &DecisionView) {
    match d.outcome.as_str() {
        "approved" => badge(ui, "AUTORISÉ", GREEN),
        "rejected" => badge(ui, "REJETÉ", RED),
        _ => badge(ui, "OBSERVÉ", BLUE),
    }
}

fn decision_detail(ui: &mut egui::Ui, d: &DecisionView) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new(format!("Décision {}", d.decision_id)).strong());
        ui.label(format!("Pourquoi ce signal: {}", d.reason));
        if !d.evidence.is_empty() {
            ui.label(RichText::new(d.evidence.iter().map(|(k, x)| format!("{k} = {x:.5}")).collect::<Vec<_>>().join("   ")).monospace());
        }
        ui.add_space(4.0);
        kv(ui, "dec-levels", &[
            ("Entrée prévue", format!("{:.5}", d.entry)),
            ("Stop-loss", format!("{:.5}", d.sl)),
            ("Take-profit", format!("{:.5}", d.tp)),
            ("Volume calculé", format!("{:.2} lot(s)", d.assessment.volume)),
            ("Risque si le stop est touché", format!("{} (budget {})", money(d.assessment.risk_amount), money(d.assessment.risk_budget))),
        ]);
        if let Some(m) = d.micro() {
            ui.add_space(4.0);
            ui.label(RichText::new("Coûts de transaction estimés").strong());
            kv(ui, "dec-costs", &[
                ("Spread", money(m.costs.spread)),
                ("Commission", money(m.costs.commission)),
                ("Slippage", money(m.costs.slippage)),
                ("Swap", money(m.costs.swap)),
                ("Conversion", money(m.costs.conversion)),
                ("Total", money(m.costs.total())),
                ("Net si la cible est atteinte", signed(m.net_if_win)),
                ("Net si le stop est touché", signed(m.net_if_loss)),
                ("Probabilité utilisée", m.win_probability.map(|p| format!("{:.1} %", p * 100.0)).unwrap_or_else(|| "Aucune (non calculable)".into())),
                ("Source de la probabilité", m.probability_source.clone()),
                ("Espérance nette", m.expected_net.map(signed).unwrap_or_else(|| "Non calculable".into())),
                ("Probabilité d'équilibre", format!("{:.1} %", m.breakeven_probability * 100.0)),
            ]);
        }
        ui.add_space(4.0);
        ui.label(RichText::new("Contrôles").strong());
        egui::Grid::new("dec-checks").num_columns(3).striped(true).show(ui, |ui| {
            for c in d.engine_checks.iter().chain(d.assessment.checks.iter()) {
                ui.label(RichText::new(if c.passed { "OK" } else { "ÉCHEC" }).color(if c.passed { GREEN } else { RED }).strong());
                ui.label(&c.name);
                ui.label(&c.detail);
                ui.end_row();
            }
        });
        if let Some(cid) = &d.cid {
            ui.label(format!("Ordre créé: {cid}"));
        }
    });
}

pub fn signals(ui: &mut egui::Ui, st: &mut UiState, v: &EngineView) {
    note(ui, "Chaque signal est enregistré avec la raison de son déclenchement, les coûts estimés et le résultat de chaque contrôle de risque.");
    if v.decisions.is_empty() {
        ui.label("Aucun signal depuis le démarrage du moteur.");
        return;
    }
    if let Some(d) = st.selected_decision.as_ref().and_then(|id| v.decisions.iter().find(|d| &d.decision_id == id)) {
        decision_detail(ui, d);
        if ui.button("Fermer le détail").clicked() {
            st.selected_decision = None;
        }
        ui.add_space(8.0);
    }
    egui::Grid::new("signals").num_columns(8).striped(true).spacing([14.0, 6.0]).show(ui, |ui| {
        header_row(ui, &["Heure", "Stratégie", "Instrument", "Sens", "Mode", "Résultat", "Motif", ""]);
        for d in v.decisions.iter().take(120) {
            ui.label(RichText::new(ts(d.ts_ms)).monospace());
            ui.label(&d.strategy_id);
            ui.label(format!("{} {}", d.symbol, d.timeframe));
            ui.label(RichText::new(if d.side == aegis_types::Side::Buy { "ACHAT" } else { "VENTE" }).color(if d.side == aegis_types::Side::Buy { GREEN } else { RED }));
            ui.label(d.mode.label());
            outcome_badge(ui, d);
            let why = match d.outcome.as_str() {
                "approved" => "tous les contrôles sont passés".to_string(),
                _ => d.failures().first().cloned().unwrap_or_default(),
            };
            ui.label(why.chars().take(90).collect::<String>());
            if ui.small_button("Détail").clicked() {
                st.selected_decision = Some(d.decision_id.clone());
            }
            ui.end_row();
        }
    });
}

pub fn positions(ui: &mut egui::Ui, st: &mut UiState, v: &EngineView) {
    if v.connection.is_none() {
        note(ui, "Terminal déconnecté: l'état réel du compte est inconnu. Les positions ci-dessous sont les dernières reçues.");
    }
    if v.positions.is_empty() {
        ui.label("Aucune position ouverte sur le compte.");
        return;
    }
    let can_act = v.mode.can_execute() && v.connection.is_some();
    if !can_act {
        note(ui, "En mode MONITOR, AEGIS n'envoie aucun ordre: les actions sont désactivées.");
    }
    egui::Grid::new("positions").num_columns(11).striped(true).spacing([14.0, 6.0]).show(ui, |ui| {
        header_row(ui, &["Ticket", "Instrument", "Sens", "Volume", "Prix d'entrée", "Stop-loss", "Take-profit", "Résultat", "Origine", "", ""]);
        for p in &v.positions {
            ui.label(p.ticket.to_string());
            ui.label(&p.symbol);
            ui.label(RichText::new(if p.side == aegis_types::Side::Buy { "ACHAT" } else { "VENTE" }).color(if p.side == aegis_types::Side::Buy { GREEN } else { RED }));
            ui.label(format!("{:.2}", p.volume));
            ui.monospace(format!("{:.5}", p.price_open));
            ui.monospace(if p.sl > 0.0 { format!("{:.5}", p.sl) } else { "AUCUN".into() });
            ui.monospace(if p.tp > 0.0 { format!("{:.5}", p.tp) } else { "aucun".into() });
            ui.label(RichText::new(signed(p.floating())).color(pnl_color(p.floating())));
            if v.foreign_tickets.contains(&p.ticket) {
                ui.label(RichText::new("Hors AEGIS").color(AMBER)).on_hover_text("Position ouverte en dehors d'AEGIS: affichée mais non gérée");
            } else {
                ui.label("AEGIS");
            }
            if ui.add_enabled(can_act, egui::Button::new("Clôturer").small()).clicked() {
                st.engine("Clôture", Command::ClosePosition { ticket: p.ticket, volume: None });
            }
            if ui.add_enabled(can_act, egui::Button::new("Modifier").small()).clicked() {
                st.modify_ticket = Some(p.ticket);
                st.modify_sl = format!("{:.5}", p.sl);
                st.modify_tp = format!("{:.5}", p.tp);
            }
            ui.end_row();
        }
    });
    if let Some(ticket) = st.modify_ticket {
        section(ui, &format!("Modifier la position {ticket}"));
        note(ui, "Le stop-loss ne peut être que resserré ou conservé: il ne peut ni être retiré ni éloigné.");
        ui.horizontal(|ui| {
            ui.label("Stop-loss");
            ui.text_edit_singleline(&mut st.modify_sl);
            ui.label("Take-profit");
            ui.text_edit_singleline(&mut st.modify_tp);
        });
        ui.horizontal(|ui| {
            let parsed = (st.modify_sl.trim().parse::<f64>(), st.modify_tp.trim().parse::<f64>());
            if ui.add_enabled(parsed.0.is_ok() && parsed.1.is_ok(), egui::Button::new("Envoyer la modification")).clicked() {
                if let (Ok(sl), Ok(tp)) = parsed {
                    st.engine("Modification", Command::ModifyPosition { ticket, sl, tp });
                    st.modify_ticket = None;
                }
            }
            if ui.button("Annuler").clicked() {
                st.modify_ticket = None;
            }
        });
    }
}

pub fn pending(ui: &mut egui::Ui, st: &mut UiState, v: &EngineView) {
    if v.pending.is_empty() {
        ui.label("Aucun ordre en attente sur le compte.");
    } else {
        let can_act = v.mode.can_execute() && v.connection.is_some();
        egui::Grid::new("pending").num_columns(9).striped(true).spacing([14.0, 6.0]).show(ui, |ui| {
            header_row(ui, &["Ticket", "Instrument", "Type", "Volume", "Prix", "Stop-loss", "Take-profit", "Placé le", ""]);
            for o in &v.pending {
                ui.label(o.ticket.to_string());
                ui.label(&o.symbol);
                ui.label(format!("{} {}", if o.side == aegis_types::Side::Buy { "Achat" } else { "Vente" }, o.kind.as_str()));
                ui.label(format!("{:.2}", o.volume));
                ui.monospace(format!("{:.5}", o.price));
                ui.monospace(format!("{:.5}", o.sl));
                ui.monospace(format!("{:.5}", o.tp));
                ui.label(ts(o.setup_ts));
                if ui.add_enabled(can_act, egui::Button::new("Annuler").small()).clicked() {
                    st.engine("Annulation", Command::CancelOrder { ticket: o.ticket });
                }
                ui.end_row();
            }
        });
    }

    section(ui, "Instructions envoyées par AEGIS");
    note(ui, "« sent » ne veut pas dire exécuté: seul le statut confirmé par le broker (filled, placed, rejected...) fait foi.");
    if v.orders.is_empty() {
        ui.label("Aucune instruction envoyée.");
        return;
    }
    egui::Grid::new("orders").num_columns(9).striped(true).spacing([14.0, 6.0]).show(ui, |ui| {
        header_row(ui, &["Créé le", "Identifiant", "Instrument", "Sens", "Volume", "Statut", "Ticket", "Prix exécuté", "Message du broker"]);
        for o in v.orders.iter().take(100) {
            ui.label(RichText::new(ts(o.created_ms)).monospace());
            ui.monospace(&o.cid);
            ui.label(&o.symbol);
            ui.label(o.side.as_str());
            ui.label(format!("{:.2}", o.volume));
            let c = match o.status.as_str() {
                "filled" | "placed" => GREEN,
                "rejected" | "not_executed" => RED,
                "unknown" | "partially_filled" => AMBER,
                _ => GREY,
            };
            ui.label(RichText::new(&o.status).color(c).strong());
            ui.label(o.ticket.map(|t| t.to_string()).unwrap_or_else(|| "-".into()));
            ui.label(o.fill_price.map(|p| format!("{p:.5}")).unwrap_or_else(|| "-".into()));
            ui.label(o.message.clone().unwrap_or_default());
            ui.end_row();
        }
    });
}

pub fn history(ui: &mut egui::Ui, _st: &mut UiState, v: &EngineView) {
    if v.deals.is_empty() {
        ui.label("Aucune transaction clôturée enregistrée.");
        return;
    }
    let net: f64 = v.deals.iter().map(|d| d.net()).sum();
    let wins = v.deals.iter().filter(|d| d.net() > 0.0).count();
    kv(ui, "hist-sum", &[
        ("Transactions affichées", v.deals.len().to_string()),
        ("Gagnantes", format!("{wins}")),
        ("Résultat net cumulé", signed(net)),
        ("Dont commissions", signed(v.deals.iter().map(|d| d.commission).sum())),
        ("Dont swaps", signed(v.deals.iter().map(|d| d.swap).sum())),
    ]);
    note(ui, "Les pertes sont affichées au même titre que les gains. Résultat net = profit brut + commission + swap, tel que rapporté par le broker.");
    ui.add_space(6.0);
    egui::Grid::new("deals").num_columns(10).striped(true).spacing([14.0, 6.0]).show(ui, |ui| {
        header_row(ui, &["Clôturé le", "Position", "Instrument", "Sens", "Volume", "Prix de sortie", "Brut", "Commission", "Swap", "Net"]);
        for d in &v.deals {
            ui.label(RichText::new(ts(d.ts_ms)).monospace());
            ui.label(d.position_ticket.to_string());
            ui.label(&d.symbol);
            ui.label(d.side.as_str());
            ui.label(format!("{:.2}", d.volume));
            ui.monospace(format!("{:.5}", d.price));
            ui.label(signed(d.profit));
            ui.label(signed(d.commission));
            ui.label(signed(d.swap));
            ui.label(RichText::new(signed(d.net())).color(pnl_color(d.net())).strong());
            ui.end_row();
        }
    });
}
