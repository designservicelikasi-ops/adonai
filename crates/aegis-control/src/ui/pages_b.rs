//! Research, risk and system pages.

use super::*;
use aegis_core::LIVE_CONFIRMATION_PHRASE;
use aegis_risk::KillPositionPolicy;
use aegis_strategy::StrategyStage;
use egui_plot::{Line, Plot, PlotPoints};

fn stage_label(s: &str) -> &'static str {
    StrategyStage::parse(s).map(|x| x.label()).unwrap_or("?")
}

pub fn strategy_lab(ui: &mut egui::Ui, st: &mut UiState, v: &EngineView) {
    note(ui, "Une stratégie ne passe jamais d'un stade à l'autre automatiquement. Recherche → Démo exige un backtest hors échantillon positif (30 transactions minimum); Démo → Réel exige 50 transactions réellement exécutées en démo avec un résultat positif.");
    for s in &v.strategies {
        ui.add_space(8.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(ui.available_width() - 8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&s.meta.name).strong().size(15.0));
                ui.monospace(&s.meta.id);
                let c = match s.stage.as_str() {
                    "demo" => AMBER,
                    "live_approved" => RED,
                    "disabled" => GREY,
                    _ => BLUE,
                };
                badge(ui, stage_label(&s.stage), c);
                if !s.enabled {
                    badge(ui, "Désactivée", GREY);
                }
            });
            ui.label(&s.meta.description);
            kv(ui, &format!("meta-{}", s.meta.id), &[
                ("Entrée", s.meta.entry_rule.clone()),
                ("Sortie", s.meta.exit_rule.clone()),
                ("Stop-loss", s.meta.stop_rule.clone()),
                ("Take-profit", s.meta.target_rule.clone()),
                ("Unités de temps", s.meta.timeframes.iter().map(|t| t.as_str()).collect::<Vec<_>>().join(", ")),
                ("Instruments", if s.meta.symbols.is_empty() { "tous".into() } else { s.meta.symbols.join(", ") }),
                ("Régimes autorisés", s.meta.regimes.iter().map(|r| r.label()).collect::<Vec<_>>().join(", ")),
                ("Paramètres", s.config.to_string()),
                ("Signaux depuis le démarrage", s.signals.to_string()),
                ("Transactions réellement exécutées", format!("{} (dont {} gagnantes), net {}", s.forward.trades, s.forward.wins, signed(s.forward.net))),
            ]);
            if s.evidence.is_empty() {
                note(ui, "Aucun backtest hors échantillon enregistré: aucune probabilité de gain disponible, donc aucune exécution possible.");
            } else {
                egui::Grid::new(format!("ev-{}", s.meta.id)).num_columns(6).striped(true).show(ui, |ui| {
                    header_row(ui, &["Instrument", "Unité", "Données", "Hors échantillon", "Net hors échantillon", "Probabilité prudente"]);
                    for e in &s.evidence {
                        ui.label(&e.symbol);
                        ui.label(&e.timeframe);
                        ui.label(if e.origin == DataOrigin::Synthetic { RichText::new("SYNTHÉTIQUES").color(AMBER) } else { RichText::new("Broker").color(GREEN) });
                        ui.label(format!("{} gagnantes / {}", e.wins, e.trades));
                        ui.label(RichText::new(signed(e.net)).color(pnl_color(e.net)));
                        ui.label(e.win_probability.map(|p| format!("{:.1} %", p * 100.0)).unwrap_or_else(|| "Indisponible (< 30 transactions)".into()));
                        ui.end_row();
                    }
                });
            }
            ui.horizontal(|ui| {
                let id = s.meta.id.clone();
                if ui.button(if s.enabled { "Désactiver" } else { "Activer" }).clicked() {
                    st.engine("Stratégie", Command::SetStrategyEnabled { id: id.clone(), enabled: !s.enabled });
                }
                ui.separator();
                ui.label("Demander le stade:");
                for (stage, text) in [(StrategyStage::Research, "Recherche"), (StrategyStage::Demo, "Démo"), (StrategyStage::LiveApproved, "Approuvée réel")] {
                    if ui.add_enabled(s.stage != stage.as_str(), egui::Button::new(text)).clicked() {
                        st.engine("Stade de la stratégie", Command::SetStrategyStage { id: id.clone(), stage });
                    }
                }
            });
        });
    }
}

fn tf_combo(ui: &mut egui::Ui, id: &str, tf: &mut Timeframe) {
    egui::ComboBox::from_id_salt(id).selected_text(tf.as_str()).show_ui(ui, |ui| {
        for t in [Timeframe::M5, Timeframe::M15, Timeframe::M30, Timeframe::H1] {
            ui.selectable_value(tf, t, t.as_str());
        }
    });
}

fn metrics_grid(ui: &mut egui::Ui, id: &str, m: &aegis_backtest::Metrics) {
    kv(ui, id, &[
        ("Profit net", signed(m.net_profit)),
        ("Profit brut", money(m.gross_profit)),
        ("Perte brute", money(m.gross_loss)),
        ("Nombre de transactions", m.trades.to_string()),
        ("Taux de réussite", format!("{:.1} %", m.win_rate * 100.0)),
        ("Profit factor", m.profit_factor.map(|p| format!("{p:.2}")).unwrap_or_else(|| "Non défini (aucune perte)".into())),
        ("Drawdown maximal", format!("{} ({:.2} %)", money(m.max_drawdown), m.max_drawdown_pct)),
        ("Gain moyen par transaction", signed(m.avg_trade)),
        ("Espérance mathématique", signed(m.expectancy)),
        ("Durée moyenne des positions", format!("{:.1} bougies ({:.0} min)", m.avg_bars_held, m.avg_duration_ms / 60_000.0)),
        ("Commissions payées", signed(m.total_commission)),
        ("Swaps", signed(m.total_swap)),
        ("Signaux ignorés (volume minimum impossible)", m.skipped_signals.to_string()),
        ("Capital final", money(m.final_equity)),
    ]);
}

pub fn backtesting(ui: &mut egui::Ui, st: &mut UiState, v: &EngineView) {
    ui.label(RichText::new("Un backtest décrit le passé sous des hypothèses d'exécution données. Ce n'est ni une prévision ni une garantie de résultats futurs.").color(AMBER));

    section(ui, "Données disponibles");
    if v.data.candles.is_empty() {
        note(ui, "Aucune bougie en base. Connectez MT5 pour recevoir l'historique du broker, importez un export CSV, ou générez des données synthétiques (tests uniquement).");
    } else {
        egui::Grid::new("candles").num_columns(4).striped(true).show(ui, |ui| {
            header_row(ui, &["Instrument", "Unité", "Origine", "Bougies"]);
            for (s, tf, origin, n) in &v.data.candles {
                ui.label(s);
                ui.label(tf);
                ui.label(if origin == "synthetic" { RichText::new("SYNTHÉTIQUES").color(AMBER) } else { RichText::new("Broker").color(GREEN) });
                ui.label(n.to_string());
                ui.end_row();
            }
        });
    }

    section(ui, "Lancer un backtest");
    if st.bt.symbol.is_empty() {
        if let Some(s) = v.specs.first() {
            st.bt.symbol = s.symbol.clone();
        }
    }
    egui::Grid::new("bt-form").num_columns(2).spacing([20.0, 6.0]).show(ui, |ui| {
        ui.label("Stratégie");
        egui::ComboBox::from_id_salt("bt-strategy").selected_text(st.bt.strategy.clone()).show_ui(ui, |ui| {
            for s in &v.strategies {
                ui.selectable_value(&mut st.bt.strategy, s.meta.id.clone(), &s.meta.name);
            }
        });
        ui.end_row();
        ui.label("Instrument");
        ui.text_edit_singleline(&mut st.bt.symbol);
        ui.end_row();
        ui.label("Unité de temps");
        tf_combo(ui, "bt-tf", &mut st.bt.timeframe);
        ui.end_row();
        ui.label("Données");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut st.bt.origin, DataOrigin::Broker, "Broker");
            ui.selectable_value(&mut st.bt.origin, DataOrigin::Synthetic, "Synthétiques (test)");
        });
        ui.end_row();
        ui.label("Capital initial");
        ui.add(egui::DragValue::new(&mut st.bt.params.initial_equity).speed(100.0).range(100.0..=10_000_000.0));
        ui.end_row();
        ui.label("Risque par transaction (%)");
        ui.add(egui::DragValue::new(&mut st.bt.params.risk_per_trade_pct).speed(0.05).range(0.05..=5.0));
        ui.end_row();
        ui.label("Commission aller-retour par lot");
        ui.add(egui::DragValue::new(&mut st.bt.params.commission_per_lot_round_turn).speed(0.5).range(0.0..=100.0));
        ui.end_row();
        ui.label("Slippage total (points)");
        ui.add(egui::DragValue::new(&mut st.bt.params.slippage_points).speed(0.5).range(0.0..=200.0));
        ui.end_row();
        ui.label("Swaps");
        ui.checkbox(&mut st.bt.params.apply_swaps, "appliquer les swaps de nuit");
        ui.end_row();
    });
    if ui.button("Lancer le backtest").clicked() {
        st.engine("Backtest", Command::RunBacktest {
            strategy_id: st.bt.strategy.clone(),
            symbol: st.bt.symbol.trim().to_string(),
            timeframe: st.bt.timeframe,
            origin: st.bt.origin,
            params: st.bt.params.clone(),
        });
    }

    if let Some(bt) = &v.last_backtest {
        section(ui, &format!("Dernier résultat — {} {} {} ({})", bt.run.strategy_id, bt.run.symbol, bt.run.timeframe, if bt.run.origin == DataOrigin::Synthetic { "données SYNTHÉTIQUES" } else { "données broker" }));
        metrics_grid(ui, "bt-full", &bt.full);
        if bt.equity_curve.len() >= 2 {
            let pts: PlotPoints = bt.equity_curve.iter().map(|(t, e)| [*t as f64 / 1000.0, *e]).collect();
            Plot::new("bt-equity").height(220.0).allow_scroll(false).show_x(false).show(ui, |p| p.line(Line::new(pts)));
        }
        ui.add_space(6.0);
        ui.label(RichText::new("Par segment chronologique (apprentissage 60 % / validation 20 % / hors échantillon 20 %)").strong());
        egui::Grid::new("bt-seg").num_columns(7).striped(true).show(ui, |ui| {
            header_row(ui, &["Segment", "Bougies", "Transactions", "Réussite", "Net", "Profit factor", "Drawdown max"]);
            for s in bt.segments.iter().chain(bt.walk_forward.iter()) {
                ui.label(match s.segment.as_str() {
                    "train" => "Apprentissage".to_string(),
                    "validation" => "Validation".to_string(),
                    "oos" => "Hors échantillon".to_string(),
                    other => other.replace("fold-", "Walk-forward "),
                });
                ui.label(s.bars.to_string());
                ui.label(s.metrics.trades.to_string());
                ui.label(format!("{:.1} %", s.metrics.win_rate * 100.0));
                ui.label(RichText::new(signed(s.metrics.net_profit)).color(pnl_color(s.metrics.net_profit)));
                ui.label(s.metrics.profit_factor.map(|p| format!("{p:.2}")).unwrap_or_else(|| "-".into()));
                ui.label(format!("{:.2} %", s.metrics.max_drawdown_pct));
                ui.end_row();
            }
        });
        ui.add_space(6.0);
        ui.collapsing(format!("Transactions une par une ({})", bt.trades.len()), |ui| {
            egui::Grid::new("bt-trades").num_columns(8).striped(true).show(ui, |ui| {
                header_row(ui, &["Entrée", "Sortie", "Sens", "Volume", "Prix d'entrée", "Prix de sortie", "Motif de sortie", "Net"]);
                for t in bt.trades.iter().take(500) {
                    ui.label(ts(t.entry_ts));
                    ui.label(ts(t.exit_ts));
                    ui.label(t.side.as_str());
                    ui.label(format!("{:.2}", t.volume));
                    ui.monospace(format!("{:.5}", t.entry));
                    ui.monospace(format!("{:.5}", t.exit));
                    ui.label(format!("{:?}", t.exit_reason));
                    ui.label(RichText::new(signed(t.net)).color(pnl_color(t.net)));
                    ui.end_row();
                }
            });
        });
    }

    section(ui, "Historique des backtests");
    if v.backtests.is_empty() {
        note(ui, "Aucun backtest enregistré.");
    } else {
        egui::Grid::new("bt-runs").num_columns(7).striped(true).show(ui, |ui| {
            header_row(ui, &["Date", "Stratégie", "Instrument", "Unité", "Données", "Transactions", "Net"]);
            for r in v.backtests.iter().take(30) {
                ui.label(ts(r.ts_ms));
                ui.label(&r.strategy_id);
                ui.label(&r.symbol);
                ui.label(&r.timeframe);
                ui.label(if r.origin == DataOrigin::Synthetic { "SYNTHÉTIQUES" } else { "Broker" });
                ui.label(r.metrics["full"]["trades"].as_u64().map(|n| n.to_string()).unwrap_or_else(|| "-".into()));
                ui.label(r.metrics["full"]["net_profit"].as_f64().map(signed).unwrap_or_else(|| "-".into()));
                ui.end_row();
            }
        });
    }

    section(ui, "Préparer des données");
    ui.label(RichText::new("Importer un export MT5 (CSV)").strong());
    ui.horizontal(|ui| {
        ui.label("Fichier");
        ui.add(egui::TextEdit::singleline(&mut st.bt.csv_path).desired_width(320.0));
        ui.label("Décalage UTC du serveur (h)");
        ui.add(egui::DragValue::new(&mut st.bt.csv_offset).range(-12..=14));
        if ui.button("Importer").clicked() {
            st.engine("Import CSV", Command::ImportCsv { path: st.bt.csv_path.trim().into(), symbol: st.bt.symbol.trim().to_string(), timeframe: st.bt.timeframe, utc_offset_hours: st.bt.csv_offset });
        }
    });
    ui.add_space(4.0);
    ui.label(RichText::new("Générer des données SYNTHÉTIQUES (tests du logiciel uniquement, instrument en « .sim »)").strong());
    ui.horizontal(|ui| {
        ui.label("Bougies");
        ui.add(egui::DragValue::new(&mut st.bt.synth_bars).range(300..=200_000));
        ui.label("Graine");
        ui.add(egui::DragValue::new(&mut st.bt.synth_seed));
        if ui.button("Générer pour EURUSD.sim").clicked() {
            st.engine("Données synthétiques", Command::GenerateSynthetic { symbol: "EURUSD.sim".into(), timeframe: st.bt.timeframe, bars: st.bt.synth_bars, seed: st.bt.synth_seed });
        }
    });
}

pub fn models(ui: &mut egui::Ui, st: &mut UiState, v: &EngineView) {
    note(ui, "Les modèles sont entraînés dans l'environnement de recherche Python (dossier research/). AEGIS recalcule lui-même le verdict de validation à partir des métriques: un modèle non validé n'influence jamais l'exécution.");
    section(ui, "Charger un modèle");
    ui.horizontal(|ui| {
        ui.label("Fichier");
        ui.add(egui::TextEdit::singleline(&mut st.model_path).desired_width(360.0));
        if ui.button("Charger et vérifier").clicked() {
            st.engine("Modèle", Command::LoadModel { path: st.model_path.trim().into() });
        }
    });
    if let Some(e) = &v.model_error {
        ui.label(RichText::new(format!("Dernière erreur: {e}")).color(RED));
    }

    section(ui, "Exporter un jeu de données d'entraînement");
    ui.horizontal(|ui| {
        ui.label("Instrument");
        ui.add(egui::TextEdit::singleline(&mut st.bt.symbol).desired_width(110.0));
        tf_combo(ui, "ex-tf", &mut st.bt.timeframe);
        ui.selectable_value(&mut st.bt.origin, DataOrigin::Broker, "Broker");
        ui.selectable_value(&mut st.bt.origin, DataOrigin::Synthetic, "Synthétiques");
        ui.label("Horizon (bougies)");
        ui.add(egui::DragValue::new(&mut st.bt.export_horizon).range(1..=96));
    });
    ui.horizontal(|ui| {
        ui.label("Vers");
        ui.add(egui::TextEdit::singleline(&mut st.bt.export_path).desired_width(320.0));
        if ui.button("Exporter").clicked() {
            st.engine("Export", Command::ExportDataset { symbol: st.bt.symbol.trim().to_string(), timeframe: st.bt.timeframe, origin: st.bt.origin, horizon: st.bt.export_horizon, path: st.bt.export_path.trim().into() });
        }
    });

    let Some(m) = &v.model else {
        section(ui, "Modèle chargé");
        note(ui, "Aucun modèle chargé.");
        return;
    };
    section(ui, &format!("Modèle chargé — {}", m.model_id));
    ui.horizontal(|ui| {
        if m.gate.passed {
            badge(ui, "VALIDATION RÉUSSIE", GREEN);
        } else {
            badge(ui, "VALIDATION ÉCHOUÉE", RED);
        }
        ui.label(&m.path);
    });
    if !m.gate.passed {
        ui.label(RichText::new("Critères non satisfaits:").strong());
        for f in &m.gate.failures {
            ui.label(RichText::new(format!("• {f}")).color(AMBER));
        }
    }
    let val = &m.validation;
    kv(ui, "model-val", &[
        ("Méthode", val.method.clone()),
        ("Échantillons", val.n_samples.to_string()),
        ("AUC moyenne hors échantillon", format!("{:.4}", val.auc_mean)),
        ("Score de Brier", format!("{:.4} (référence naïve {:.4})", val.brier_mean, val.baseline_brier)),
        ("Erreur de calibration (ECE)", format!("{:.4}", val.ece)),
        ("Taux de hausses dans les données", format!("{:.1} %", val.positive_rate * 100.0)),
    ]);
    egui::Grid::new("folds").num_columns(5).striped(true).show(ui, |ui| {
        header_row(ui, &["Pli", "Échantillons", "AUC", "Brier", "Log-loss"]);
        for (i, f) in val.folds.iter().enumerate() {
            ui.label((i + 1).to_string());
            ui.label(f.n.to_string());
            ui.label(format!("{:.4}", f.auc));
            ui.label(format!("{:.4}", f.brier));
            ui.label(format!("{:.4}", f.log_loss));
            ui.end_row();
        }
    });
    note(ui, "Une AUC de 0,50 correspond au hasard. Ces chiffres mesurent le passé et ne garantissent pas la précision future.");
}

fn risk_row(ui: &mut egui::Ui, label: &str, v: &mut f64, speed: f64, range: std::ops::RangeInclusive<f64>, hint: &str) -> bool {
    ui.label(label);
    let changed = ui.add(egui::DragValue::new(v).speed(speed).range(range)).changed();
    ui.label(RichText::new(hint).color(GREY));
    ui.end_row();
    changed
}

pub fn risk(ui: &mut egui::Ui, st: &mut UiState, v: &EngineView) {
    section(ui, "État actuel");
    kv(ui, "risk-now", &[
        ("Arrêt d'urgence", if v.kill.active { format!("ACTIF depuis le {} — {} (par {})", ts(v.kill.activated_ms), v.kill.reason, v.kill.actor) } else { "inactif".into() }),
        ("Résultat net du jour", v.daily_net_pct.map(|p| format!("{} ({p:+.2} %)", opt_money(v.daily_net))).unwrap_or_else(|| UNAVAILABLE.into())),
        ("Limite de perte journalière", format!("-{:.2} %", v.risk_config.max_daily_loss_pct)),
        ("Drawdown depuis le plus haut", v.drawdown_pct.map(|d| format!("{d:.2} % (limite {:.2} %)", v.risk_config.max_drawdown_pct)).unwrap_or_else(|| UNAVAILABLE.into())),
        ("Plus haut d'équité", if v.peak_equity > 0.0 { money(v.peak_equity) } else { UNAVAILABLE.into() }),
        ("Positions + ordres en attente", if v.connection.is_some() { format!("{} / {}", v.positions.len() + v.pending.len(), v.risk_config.max_open_positions) } else { UNAVAILABLE.into() }),
        ("Nouvelles entrées", match (&v.kill.active, &v.daily_halt) {
            (true, _) => "BLOQUÉES (arrêt d'urgence)".into(),
            (_, Some(h)) => format!("BLOQUÉES ({h})"),
            _ => "autorisées sous réserve des contrôles".into(),
        }),
    ]);

    section(ui, "Limites");
    note(ui, "Convention comptable: la journée est la journée UTC. Résultat net du jour = équité actuelle − équité de début de journée − dépôts/retraits; il inclut donc le flottant, les commissions et les swaps.");
    let mut form = st.risk_form.clone().unwrap_or_else(|| v.risk_config.clone());
    let mut changed = false;
    egui::Grid::new("risk-form").num_columns(3).spacing([18.0, 6.0]).show(ui, |ui| {
        changed |= risk_row(ui, "Risque par transaction (%)", &mut form.risk_per_trade_pct, 0.05, 0.01..=5.0, "perte maximale si le stop est touché");
        changed |= risk_row(ui, "Perte journalière maximale (%)", &mut form.max_daily_loss_pct, 0.1, 0.1..=20.0, "réalisé + flottant; bloque les nouvelles entrées");
        changed |= risk_row(ui, "Drawdown maximal (%)", &mut form.max_drawdown_pct, 0.5, 0.5..=50.0, "déclenche l'arrêt d'urgence automatique");
        changed |= risk_row(ui, "Exposition notionnelle max (x équité)", &mut form.max_notional_leverage, 0.5, 0.1..=100.0, "somme des positions / équité");
        changed |= risk_row(ui, "Spread maximal (points)", &mut form.max_spread_points, 1.0, 0.0..=10_000.0, "");
        changed |= risk_row(ui, "Slippage maximal autorisé (points)", &mut form.max_slippage_points, 1.0, 0.0..=10_000.0, "au-delà, le connecteur refuse l'exécution");
        changed |= risk_row(ui, "Profit net minimal attendu", &mut form.min_net_expected_profit, 0.1, 0.0..=100_000.0, "en devise du compte, après tous les coûts");
        changed |= risk_row(ui, "Commission aller-retour par lot", &mut form.commission_per_lot_round_turn, 0.5, 0.0..=1_000.0, "non fournie par MT5: à renseigner selon votre broker");
        changed |= risk_row(ui, "Slippage estimé (points)", &mut form.expected_slippage_points, 0.5, 0.0..=10_000.0, "pour l'estimation des coûts");
        changed |= risk_row(ui, "Frais de conversion (%)", &mut form.conversion_fee_pct, 0.05, 0.0..=10.0, "");
        changed |= risk_row(ui, "Nuits de détention prévues", &mut form.expected_holding_nights, 0.5, 0.0..=365.0, "pour l'estimation des swaps");
        ui.label("Positions ouvertes max");
        changed |= ui.add(egui::DragValue::new(&mut form.max_open_positions).range(1..=100)).changed();
        ui.label("");
        ui.end_row();
        ui.label("Positions max par instrument");
        changed |= ui.add(egui::DragValue::new(&mut form.max_positions_per_symbol).range(1..=20)).changed();
        ui.label("");
        ui.end_row();
        ui.label("Arrêt d'urgence: ordres en attente");
        changed |= ui.checkbox(&mut form.kill_cancel_pending, "annuler les ordres en attente d'AEGIS").changed();
        ui.label("");
        ui.end_row();
        ui.label("Arrêt d'urgence: positions ouvertes");
        ui.horizontal(|ui| {
            changed |= ui.selectable_value(&mut form.kill_position_policy, KillPositionPolicy::Keep, "Conserver avec leurs SL/TP").changed();
            changed |= ui.selectable_value(&mut form.kill_position_policy, KillPositionPolicy::Close, "Clôturer au marché").changed();
        });
        ui.label(RichText::new("s'applique uniquement aux positions ouvertes par AEGIS").color(GREY));
        ui.end_row();
    });
    if changed {
        st.risk_form = Some(form.clone());
    }
    match form.validate() {
        Ok(()) => {}
        Err(e) => {
            ui.label(RichText::new(format!("Configuration invalide: {e}")).color(RED));
        }
    }
    ui.horizontal(|ui| {
        let dirty = st.risk_form.as_ref().is_some_and(|f| f != &v.risk_config);
        if ui.add_enabled(dirty && form.validate().is_ok(), egui::Button::new("Enregistrer les limites")).clicked() {
            st.engine("Limites de risque", Command::SetRiskConfig(form.clone()));
            st.risk_form = None;
        }
        if ui.add_enabled(dirty, egui::Button::new("Annuler les modifications")).clicked() {
            st.risk_form = None;
        }
    });
}

pub fn goals(ui: &mut egui::Ui, st: &mut UiState, v: &EngineView) {
    ui.label(RichText::new("L'objectif journalier est une condition d'arrêt, pas une promesse de gain. Quand il est atteint, AEGIS cesse d'ouvrir des positions. Il n'augmente jamais le risque et ne force jamais une transaction pour l'atteindre.").color(AMBER));
    section(ui, "Aujourd'hui (journée UTC)");
    let target = v.risk_config.daily_profit_target_pct;
    match (&v.day, v.daily_net, v.daily_net_pct) {
        (Some(day), Some(net), Some(pct)) => {
            kv(ui, "goal-day", &[
                ("Équité en début de journée", money(day.start_equity)),
                ("Résultat net (flottant inclus)", format!("{} ({pct:+.2} %)", signed(net))),
                ("Dont réalisé net", signed(day.realized_net())),
                ("Dont profit brut réalisé", signed(day.realized_gross)),
                ("Dont commissions", signed(day.commissions)),
                ("Dont swaps", signed(day.swaps)),
                ("Flottant", opt_money(v.floating_pnl)),
                ("Dépôts / retraits du jour", signed(day.deposits)),
                ("Transactions clôturées", format!("{} (dont {} gagnantes)", day.closed_trades, day.winning_trades)),
            ]);
            ui.add_space(6.0);
            if target > 0.0 {
                let frac = (pct / target).clamp(0.0, 1.0) as f32;
                ui.label(format!("Progression vers l'objectif de {target:.2} %"));
                ui.add(egui::ProgressBar::new(frac).show_percentage());
                if pct >= target {
                    ui.label(RichText::new("Objectif atteint: les nouvelles entrées sont arrêtées jusqu'à demain (UTC).").color(GREEN).strong());
                }
            } else {
                note(ui, "Aucun objectif journalier n'est défini.");
            }
            let limit = v.risk_config.max_daily_loss_pct;
            ui.label(format!("Marge avant la limite de perte journalière (-{limit:.2} %): {:.2} points de pourcentage", pct + limit));
        }
        _ => note(ui, "Indisponible: aucune information de compte reçue aujourd'hui."),
    }

    section(ui, "Régler l'objectif");
    let mut form = st.risk_form.clone().unwrap_or_else(|| v.risk_config.clone());
    ui.horizontal(|ui| {
        ui.label("Objectif de profit journalier (% de l'équité de début de journée, 0 = désactivé)");
        if ui.add(egui::DragValue::new(&mut form.daily_profit_target_pct).speed(0.05).range(0.0..=100.0)).changed() {
            st.risk_form = Some(form.clone());
        }
    });
    let dirty = st.risk_form.as_ref().is_some_and(|f| f != &v.risk_config);
    if ui.add_enabled(dirty && form.validate().is_ok(), egui::Button::new("Enregistrer l'objectif")).clicked() {
        st.engine("Objectif", Command::SetRiskConfig(form));
        st.risk_form = None;
    }
}

pub fn micro_profit(ui: &mut egui::Ui, st: &mut UiState, v: &EngineView) {
    ui.label("Résultat net attendu = résultat brut attendu − spread − commissions − swaps − slippage estimé − frais de conversion.");
    note(ui, "Un petit mouvement favorable du prix n'est pas automatiquement rentable: une entrée n'est autorisée que si le résultat net attendu dépasse le minimum configuré.");
    kv(ui, "micro-cfg", &[
        ("Profit net minimal attendu", money(v.risk_config.min_net_expected_profit)),
        ("Commission aller-retour par lot", money(v.risk_config.commission_per_lot_round_turn)),
        ("Slippage estimé", format!("{:.1} points", v.risk_config.expected_slippage_points)),
        ("Frais de conversion", format!("{:.2} %", v.risk_config.conversion_fee_pct)),
        ("Nuits de détention prévues", format!("{:.1}", v.risk_config.expected_holding_nights)),
    ]);
    if ui.button("Modifier ces paramètres (Gestion des risques)").clicked() {
        st.page = Page::Risk;
    }

    section(ui, "Opportunités évaluées");
    let evaluated: Vec<_> = v.decisions.iter().filter(|d| d.micro().is_some()).take(80).collect();
    if evaluated.is_empty() {
        note(ui, "Aucune opportunité évaluée pour l'instant.");
        return;
    }
    egui::Grid::new("micro").num_columns(11).striped(true).spacing([12.0, 6.0]).show(ui, |ui| {
        header_row(ui, &["Heure", "Instrument", "Sens", "Volume", "Brut si cible", "Coûts", "Net si cible", "Net si stop", "Probabilité", "Espérance nette", "Verdict"]);
        for d in evaluated {
            let m = d.micro().expect("filtered above");
            ui.label(RichText::new(ts(d.ts_ms)).monospace());
            ui.label(&d.symbol);
            ui.label(d.side.as_str());
            ui.label(format!("{:.2}", m.volume));
            ui.label(money(m.gross_win));
            ui.label(money(m.costs.total()));
            ui.label(RichText::new(signed(m.net_if_win)).color(pnl_color(m.net_if_win)));
            ui.label(RichText::new(signed(m.net_if_loss)).color(pnl_color(m.net_if_loss)));
            ui.label(m.win_probability.map(|p| format!("{:.1} %", p * 100.0)).unwrap_or_else(|| "aucune".into()));
            match m.expected_net {
                Some(e) => ui.label(RichText::new(signed(e)).color(pnl_color(e)).strong()),
                None => ui.label(RichText::new("non calculable").color(GREY)),
            };
            let ok = m.expected_net.is_some_and(|e| e > 0.0 && e >= v.risk_config.min_net_expected_profit);
            ui.label(if ok { RichText::new("Net positif").color(GREEN) } else { RichText::new("Insuffisant").color(RED) });
            ui.end_row();
        }
    });
}

pub fn account(ui: &mut egui::Ui, _st: &mut UiState, v: &EngineView) {
    let Some(a) = &v.account else {
        note(ui, "Indisponible: aucune information de compte reçue. Connectez le terminal MT5.");
        return;
    };
    if v.is_simulator() {
        ui.label(RichText::new("Compte FICTIF du simulateur (adaptateur de test).").color(AMBER));
    }
    if v.connection.is_none() {
        ui.label(RichText::new("Terminal déconnecté: ces valeurs sont les dernières reçues et peuvent être périmées.").color(RED));
    }
    kv(ui, "account", &[
        ("Numéro de compte", a.login.to_string()),
        ("Serveur", a.server.clone()),
        ("Type de compte", match a.trade_mode {
            aegis_types::AccountTradeMode::Demo => "Démonstration".into(),
            aegis_types::AccountTradeMode::Contest => "Concours".into(),
            aegis_types::AccountTradeMode::Real => "RÉEL".into(),
        }),
        ("Devise", a.currency.clone()),
        ("Levier", format!("1:{}", a.leverage)),
        ("Solde", money(a.balance)),
        ("Équité", money(a.equity)),
        ("Marge utilisée", money(a.margin)),
        ("Marge libre", money(a.margin_free)),
        ("Trading autorisé par le broker", if a.trade_allowed { "oui".into() } else { "NON".into() }),
        ("Dernière mise à jour", format!("{} (il y a {} s)", ts(a.ts_ms), (v.now_ms - a.ts_ms) / 1000)),
    ]);

    section(ui, "Instruments et spécifications du broker");
    egui::Grid::new("specs").num_columns(9).striped(true).spacing([14.0, 6.0]).show(ui, |ui| {
        header_row(ui, &["Instrument", "Décimales", "Taille du contrat", "Valeur du tick", "Volume min", "Pas", "Volume max", "Distance stops (pts)", "Swap achat / vente"]);
        for s in &v.specs {
            ui.label(&s.symbol);
            ui.label(s.digits.to_string());
            ui.label(format!("{}", s.contract_size));
            ui.label(format!("{} / {}", s.tick_value, s.tick_size));
            ui.label(format!("{}", s.volume_min));
            ui.label(format!("{}", s.volume_step));
            ui.label(format!("{}", s.volume_max));
            ui.label(format!("{}", s.stops_level_points));
            ui.label(format!("{} / {}", s.swap_long, s.swap_short));
            ui.end_row();
        }
    });
}

pub fn mt5(ui: &mut egui::Ui, st: &mut UiState, v: &EngineView) {
    section(ui, "État de la connexion");
    match &v.connection {
        Some(c) => {
            ui.horizontal(|ui| {
                if c.terminal == TerminalKind::Simulator {
                    badge(ui, "SIMULATEUR CONNECTÉ — ce n'est pas un broker", AMBER);
                } else {
                    badge(ui, "TERMINAL MT5 CONNECTÉ", GREEN);
                }
            });
            kv(ui, "conn", &[
                ("Compte", format!("{} sur {} ({})", c.login, c.server, c.trade_mode.as_str())),
                ("Connecteur", c.build.clone()),
                ("Adresse du terminal", c.peer.clone()),
                ("Connecté depuis", ts(c.connected_ms)),
                ("Dernier message reçu", format!("il y a {} ms", v.now_ms - c.last_rx_ms)),
                ("Messages reçus / envoyés", format!("{} / {}", c.frames_in, c.frames_out)),
            ]);
        }
        None => {
            badge(ui, "AUCUN TERMINAL CONNECTÉ", RED);
            if !v.bridge.last_disconnect.is_empty() {
                ui.label(format!("Dernière déconnexion: {}", v.bridge.last_disconnect));
            }
        }
    }
    kv(ui, "bridge", &[
        ("Adresse d'écoute du moteur", v.bridge.listening.clone()),
        ("Origine de la clé partagée", v.bridge.key_source.clone()),
        ("Connexions acceptées", v.bridge.connections_accepted.to_string()),
        ("Tentatives refusées", v.bridge.handshakes_refused.to_string()),
        ("Violations du protocole", v.bridge.protocol_violations.to_string()),
        ("Dernier refus", if v.bridge.last_refusal.is_empty() { "-".into() } else { v.bridge.last_refusal.clone() }),
    ]);

    section(ui, "Connecter MetaTrader 5");
    ui.label("1. Copiez le dossier mt5/ du projet dans MQL5/Experts/AEGIS de votre terminal, puis compilez AegisConnector.mq5 dans MetaEditor.");
    ui.label(format!("2. Dans MT5: Outils → Options → Expert Advisors → autorisez l'adresse {}.", v.bridge.listening.split(':').next().unwrap_or("127.0.0.1")));
    ui.label("3. Attachez l'Expert Advisor à un graphique et renseignez l'adresse, le port et la clé partagée ci-dessous.");
    ui.label("4. Utilisez d'abord un compte de DÉMONSTRATION.");
    ui.horizontal(|ui| {
        ui.label("Clé partagée:");
        if st.show_key {
            ui.monospace(&st.shared_key);
        } else {
            ui.monospace("•".repeat(24));
        }
        if ui.button(if st.show_key { "Masquer" } else { "Afficher" }).clicked() {
            st.show_key = !st.show_key;
        }
        if ui.button("Copier").clicked() {
            ui.ctx().copy_text(st.shared_key.clone());
        }
    });
    note(ui, "Cette clé authentifie le connecteur. Ne la partagez pas. La liaison est authentifiée mais non chiffrée: gardez-la sur la même machine ou dans un tunnel privé (voir docs/DEPLOYMENT.md).");

    section(ui, "Simulateur intégré (adaptateur de test)");
    ui.label("Le simulateur remplace MT5 pour essayer le logiciel: prix synthétiques, compte fictif, instrument EURUSD.sim. Rien de ce qu'il produit n'est réel.");
    ui.horizontal(|ui| {
        if st.simulator_running {
            if ui.button("Arrêter le simulateur").clicked() {
                st.actions.push(UiAction::StopSimulator);
            }
            badge(ui, "simulateur lancé", AMBER);
        } else if ui.add_enabled(v.connection.is_none(), egui::Button::new("Démarrer le simulateur")).clicked() {
            st.actions.push(UiAction::StartSimulator);
        }
    });
}

pub fn system(ui: &mut egui::Ui, st: &mut UiState, v: &EngineView) {
    section(ui, "Modules");
    egui::Grid::new("health").num_columns(3).striped(true).spacing([18.0, 6.0]).show(ui, |ui| {
        header_row(ui, &["Module", "État", "Détail"]);
        for h in &v.health {
            ui.label(RichText::new(&h.name).strong());
            ui.label(RichText::new(h.status.label()).color(health_color(h.status)).strong());
            ui.label(&h.detail);
            ui.end_row();
        }
    });

    section(ui, "Base de données");
    kv(ui, "db", &[
        ("Fichier", v.data.db_path.clone()),
        ("Accessible", if v.data.db_ok { "oui".into() } else { "NON".into() }),
        ("Journal d'audit", match (&v.data.audit_entries_verified, &v.data.audit_error) {
            (_, Some(e)) => format!("ALTÉRÉ: {e}"),
            (Some(n), None) => format!("intègre ({n} entrées vérifiées)"),
            (None, None) => "non vérifié depuis le démarrage".into(),
        }),
        ("Dernière sauvegarde", v.data.last_backup.clone().unwrap_or_else(|| "aucune depuis le démarrage".into())),
    ]);
    ui.horizontal(|ui| {
        if ui.button("Vérifier l'intégrité du journal d'audit").clicked() {
            st.engine("Audit", Command::VerifyAudit);
        }
        if ui.button("Créer une sauvegarde").clicked() {
            st.engine("Sauvegarde", Command::Backup);
        }
    });

    section(ui, "Journal du moteur");
    egui::Grid::new("logs").num_columns(4).striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
        for l in v.logs.iter().take(150) {
            ui.label(RichText::new(ts(l.ts_ms)).monospace().color(GREY));
            let c = match l.level.as_str() {
                "error" => RED,
                "warn" => AMBER,
                _ => GREY,
            };
            ui.label(RichText::new(l.level.to_uppercase()).color(c).strong());
            ui.label(RichText::new(&l.source).color(BLUE));
            ui.label(&l.text);
            ui.end_row();
        }
    });

    section(ui, "Piste d'audit (dernières entrées)");
    egui::Grid::new("audit").num_columns(4).striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
        header_row(ui, &["N°", "Date", "Acteur", "Événement"]);
        for a in v.audit.iter().take(60) {
            ui.label(a.id.to_string());
            ui.label(RichText::new(ts(a.ts_ms)).monospace());
            ui.label(&a.actor);
            ui.label(&a.event).on_hover_text(&a.detail);
            ui.end_row();
        }
    });
}

pub fn settings(ui: &mut egui::Ui, st: &mut UiState, v: &EngineView) {
    note(ui, "La configuration du moteur est lue au démarrage. Modifiez le fichier puis relancez l'application. Les limites de risque se règlent dans « Gestion des risques » et sont appliquées immédiatement.");
    kv(ui, "settings", &[
        ("Fichier de configuration", st.config_path.clone()),
        ("Base de données", v.data.db_path.clone()),
        ("Unités de temps des signaux", v.signal_timeframes.iter().map(|t| t.as_str()).collect::<Vec<_>>().join(", ")),
        ("Trading réel autorisé dans la configuration", if v.live_trading_enabled_in_config { "OUI".into() } else { "non (valeur par défaut)".into() }),
        ("Version", v.version.clone()),
    ]);
    section(ui, "Configuration en vigueur");
    egui::ScrollArea::vertical().id_salt("cfg").max_height(420.0).show(ui, |ui| {
        ui.add(egui::TextEdit::multiline(&mut st.config_toml.clone()).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).interactive(false));
    });
    note(ui, "Aucun secret n'apparaît ici: la clé partagée et le jeton de l'API viennent de variables d'environnement ou du fichier de clé protégé.");
}

pub fn control(ui: &mut egui::Ui, st: &mut UiState, v: &EngineView) {
    section(ui, "Mode de fonctionnement");
    ui.horizontal(|ui| {
        ui.label("Mode actuel:");
        badge(ui, v.mode.label(), mode_color(v.mode));
    });
    ui.add_space(6.0);

    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new("MONITOR — Surveillance").strong().color(BLUE));
        ui.label("Collecte les données, analyse le marché et génère des signaux. N'envoie jamais aucun ordre.");
        if ui.add_enabled(v.mode != Mode::Monitor, egui::Button::new("Passer en MONITOR")).clicked() {
            st.engine("Mode", Command::SetMode { mode: Mode::Monitor, confirmation: String::new() });
        }
    });
    ui.add_space(6.0);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new("DEMO AUTO — Automatisation en démo").strong().color(AMBER));
        ui.label("Exécute automatiquement les ordres autorisés par le moteur de risque, uniquement sur un compte de démonstration vérifié (ou le simulateur).");
        if ui.add_enabled(v.mode != Mode::DemoAuto, egui::Button::new("Activer DEMO AUTO")).clicked() {
            st.engine("Mode", Command::SetMode { mode: Mode::DemoAuto, confirmation: String::new() });
        }
    });
    ui.add_space(6.0);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new("LIVE AUTO — Automatisation en réel").strong().color(RED));
        ui.label(RichText::new("ATTENTION: ce mode engage de l'argent réel. Les pertes sont possibles et peuvent être rapides. Aucun résultat passé, backtest ou modèle ne garantit un gain.").color(RED));
        let blockers: Vec<String> = [
            (!v.live_trading_enabled_in_config).then(|| "le trading réel n'est pas autorisé dans le fichier de configuration (safety.live_trading_enabled)".to_string()),
            v.connection.is_none().then(|| "aucun terminal connecté".to_string()),
            v.is_simulator().then(|| "le simulateur est connecté".to_string()),
            v.account.as_ref().filter(|a| a.trade_mode != aegis_types::AccountTradeMode::Real).map(|_| "le compte connecté n'est pas un compte réel".to_string()),
            v.kill.active.then(|| "l'arrêt d'urgence est actif".to_string()),
            (!v.strategies.iter().any(|s| s.enabled && s.stage == "live_approved")).then(|| "aucune stratégie n'est approuvée pour le réel".to_string()),
        ]
        .into_iter()
        .flatten()
        .collect();
        if blockers.is_empty() {
            let login = v.account.as_ref().map(|a| a.login).unwrap_or(0);
            ui.label(format!("Pour armer LIVE AUTO, saisissez exactement:  {login} {LIVE_CONFIRMATION_PHRASE}"));
            ui.add(egui::TextEdit::singleline(&mut st.live_confirm).desired_width(360.0));
            let armed = egui::Button::new(RichText::new("ARMER LIVE AUTO").color(Color32::WHITE).strong()).fill(Color32::from_rgb(170, 40, 40));
            if ui.add_enabled(v.mode != Mode::LiveAuto, armed).clicked() {
                st.engine("LIVE AUTO", Command::SetMode { mode: Mode::LiveAuto, confirmation: st.live_confirm.clone() });
                st.live_confirm.clear();
            }
        } else {
            ui.label(RichText::new("LIVE AUTO est indisponible:").strong());
            for b in blockers {
                ui.label(format!("• {b}"));
            }
        }
    });

    section(ui, "Arrêt d'urgence");
    if v.kill.active {
        ui.label(RichText::new(format!("ACTIF depuis le {} — motif: {} (déclenché par {})", ts(v.kill.activated_ms), v.kill.reason, v.kill.actor)).color(RED).strong());
        ui.label("Les nouvelles entrées sont bloquées. La réinitialisation est manuelle et laisse le moteur en MONITOR.");
        ui.horizontal(|ui| {
            ui.label(format!("Saisissez « {} » pour réinitialiser:", aegis_risk::KILL_RESET_PHRASE));
            ui.add(egui::TextEdit::singleline(&mut st.kill_reset).desired_width(160.0));
            if ui.button("Réinitialiser").clicked() {
                st.engine("Réinitialisation", Command::ResetKill { phrase: st.kill_reset.trim().to_string() });
                st.kill_reset.clear();
            }
        });
    } else {
        ui.label("Bloque immédiatement toute nouvelle entrée, annule les ordres en attente d'AEGIS si configuré, et applique la politique choisie aux positions d'AEGIS (voir « Gestion des risques »).");
        ui.horizontal(|ui| {
            ui.label("Motif (facultatif)");
            ui.add(egui::TextEdit::singleline(&mut st.kill_reason).desired_width(260.0));
            let b = egui::Button::new(RichText::new("ENGAGER L'ARRÊT D'URGENCE").color(Color32::WHITE).strong()).fill(Color32::from_rgb(170, 40, 40));
            if ui.add(b).clicked() {
                st.engine("Arrêt d'urgence", Command::Kill { reason: st.kill_reason.trim().to_string() });
                st.kill_reason.clear();
            }
        });
    }

    section(ui, "Dernières actions");
    if st.messages.is_empty() {
        note(ui, "Aucune action depuis l'ouverture de l'application.");
    }
    for (ok, text) in st.messages.iter().take(12) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(if *ok { "OK" } else { "REFUSÉ" }).color(if *ok { GREEN } else { RED }).strong());
            ui.label(text);
        });
    }
}
