# Historique des versions

## 0.3.0 — en cours (directive « Master Upgrade », 2026-11-26)

### Phase 5 — Micro-Profit 2.0, Recovery & Hedge

- Faisabilité de chaque objectif net (`[micro] targets`) après tous les coûts, avec la
  probabilité calibrée de la cible et la raison d'un refus.
- Règles de récupération (`[risk.recovery]`) : anti-moyenne, anti-martingale (risque ET
  volume), re-entry sur nouvelle bougie et nouvelle structure, plafond quotidien, couverture
  unique et jamais plus grosse que la position. Type d'entrée et position liée au journal.
- Analyses Recovery & Hedge (section `recovery`). `docs/RECOVERY_HEDGE.md`.

### Phase 4 — Régimes, multi-unités, structure, scanner

- Régimes détaillés (tendance, transition, consolidation, latéral, désordonné ; niveau et
  dynamique de volatilité ; conditions anormales ; annonce proche), historique des changements
  (table `regime_history`), régime détaillé de chaque transaction.
- Unité M30 ajoutée ; H4 et D1 reconstruits depuis H1 (UTC). Lecture multi-unités par rôle,
  sans vote majoritaire, unités indisponibles signalées.
- Détecteurs de structure définis et testés : swings, HH/HL/LH/LL, BOS, CHoCH, cassure,
  fausse cassure, retest, repli, rejet, divergence, compression/expansion, liquidité,
  déplacement, déséquilibre.
- Scanner d'opportunités dans le moteur : score expliqué (≠ probabilité), probabilité
  calibrée séparée, espérance nette, admissibilité motivée. Section `scanner` de l'état.
- Contrôles `regime_detaille`, `multi_unites`, `transition`, `score_configuration` ;
  section `[intelligence]`. Migration 0008 réversible (colonnes du journal + historique).

### Phase 3 — AEGIS AUTONOMY COMMANDER

- Crate `aegis-autonomy` : 18 capacités, 4 profils (OBSERVER, RESEARCH, DEMO FULL, LIVE
  AUTHORIZED), autorisations liées au compte, datées, révocables ; LIVE lié à l'empreinte de
  la configuration de risque, 7 jours au plus, jamais restauré.
- Commandes `grant_autonomy`, `revoke_autonomy`, `set_default_profile` ; `set_mode` passe par
  le Commander (compatibilité conservée). Contrôle `autonomie` dans les décisions ; permissions
  vérifiées pour la gestion des positions, les sorties de stratégie et le réentraînement
  automatique ; actions de protection jamais bloquées mais enregistrées.
- Migration 0007 (`permission_grants`, `permission_events`) avec script de retour arrière
  testé. Section `[autonomy]`. Section `autonomy` de l'état publié. `docs/AUTONOMY.md`.
- Test d'architecture : seul le canal utilisateur peut confirmer une autorisation.

### Phase 2 — Risk Guardian 2.0 et exécution

- **Mode du compte** (hedging / netting / exchange) transmis par le connecteur (protocole 1.2,
  connecteur 1.20) ; un connecteur 1.1 reste accepté (mode inconnu = traité comme netting).
  Contrôles `mode_marge` (netting : jamais d'empilement ni de compensation) et `couverture`
  (hedging : position opposée seulement si `allow_hedging`). Simulateur : mode netting
  (fusion, réduction, inversion).
- **Risque du portefeuille** (`risque_portefeuille`) mesuré aux stops actuels ; une position
  sans stop bloque les entrées. **Budget de risque journalier**, **jours autorisés**,
  **réduction après une série de pertes** (multiplicateurs ≤ 1 uniquement), **slippage
  observé**.
- **Stop initial adaptatif** (`[risk.stops]`, `mode = "adaptive"`) : structurel, volatilité ou
  stratégie, dans des bornes ; volume recalculé ; la preuve de backtest n'est pas réutilisée
  pour un stop différent.
- **Garde de protection** : SL manquant détecté et réinstallé (ou position clôturée selon la
  politique), modifications vérifiées dans l'instantané suivant, incidents et audit.
- **CLOSE PARTIAL** (Position Intelligence) avec respect du volume minimum et du pas.
- **Qualité d'exécution** : temps interne, broker (`result.broker_us`), réseau ; slippage par
  instrument. Section `guardian` de l'état publié.

## Non publiée — branche `cerveau-micro-profits` (2026-11-21)

### Ajouté

- **Cerveau micro-profits** (`aegis-brain`, section `[brain]`, désactivé par défaut) : pour
  chaque signal sur EURUSD, choisit la cible (et si besoin un volume plus petit) dont le gain
  NET si gagné est entre 0,08 et 2,00, avec la meilleure espérance nette parmi celles que le
  Risk Engine accepterait. Contrôle `cerveau` dans chaque décision, avec son explication.
- **Apprentissage continu** depuis le journal : frais réels par lot, correction des
  probabilités trop optimistes, contextes (stratégie x sens x session) perdants bloqués ;
  alerte quand le cerveau devient nettement plus prudent.
- **Réentraînement automatique** (toutes les 24 h par défaut) avec période de contrôle ; une
  meilleure version est **proposée** (alerte) et n'est utilisée qu'après « Activer ».
  Commandes `brain_retrain`, `brain_accept_candidate`, `brain_dismiss_candidate`.
- Page **Cerveau IA** dans AEGIS Desktop ; section `brain` de l'état publié.
- Journal : colonnes `probability_raw` et `brain` (migration 0006).
- `aegis_risk::size_volume` : le calcul de taille du Risk Engine, partagé avec le cerveau.
- `aegis_probability::holdout_score` : note d'un modèle sur des bougies qu'il n'a pas vues.
- **Objectif mensuel** : `daily_goal.period = "day" | "month"` (mois civil UTC, cumulé d'un
  jour à l'autre) ; mêmes politiques (arrêter, protéger le gain, suivre).
- **SL / TP en pips** pour le cerveau (`sl_pips`, `tp_pips` fixe, `max_tp_pips`) ; réglages du
  cerveau modifiables dans l'application (`set_brain_config`), conservés en base.
- Unités de temps par défaut : **M1 et M5** ; historique demandé : 5000 bougies.
- Moteur local d'AEGIS Desktop créé avec : cerveau actif sur EURUSD, M1/M5, espérance
  minimale 0,01, objectif « protéger le gain ».

## 0.2.0 — 2026-11-18

Réalisée d'après la spécification v2 (`docs/SPECIFICATION.md`). Développée et testée sur macOS
(Apple Silicon) ; ce qui n'a pas pu être vérifié est listé dans `PROJECT_STATUS.md`.

### Ajouté

- **Daily Profit Goal Manager** : objectif en devise du compte, brut ou net, restant,
  progression, politiques (arrêter, protéger le gain, suivre). Comptabilité du jour séparant
  profit brut, perte brute, net, commissions, spread estimé, slippage mesuré, swap, frais.
- **Moteur de risque étendu** : seuils BUY/SELL de P(TP avant SL), transactions max par jour
  (jusqu'à 10 000), pertes consécutives et pause, débit d'ordres, horaires UTC, exposition par
  devise, coûts du jour vs gains bruts, plafond de lots par transaction.
- **6 stratégies** : RSI momentum, suivi de tendance, pullback, retour à la moyenne Bollinger,
  rejet support/résistance, expansion de volatilité (9 au total) ; unité M1 permise.
- **Position Intelligence** : HOLD, SECURE (point mort + coûts), ADJUST SL (stop suiveur),
  ADJUST TP, CLOSE (temps, net réalisable) ; jamais d'élargissement ; distances et gel broker.
- **Probability Engine** (`aegis-probability`) : P(TP avant SL) par triple barrière,
  régression logistique, walk-forward purgé, calibration croisée, Brier/log loss/ECE/fiabilité,
  sélection contre un modèle de référence ; versions, jeux de données, retour arrière ;
  calibration réelle des transactions exécutées.
- **Entry Intelligence** : BUY / SELL / WAIT par instrument avec probabilités affichées.
- **Journal des transactions** et **performance** (heure, jour, semaine, mois, stratégie,
  instrument, heure, régime ; profit factor, espérance, coûts, efficacité des sorties,
  drawdown, Sharpe, dégradation).
- **News Intelligence** (`aegis-news`) : calendrier économique, autorisé / prudent / suspendu.
- **Shadow Intelligence** (`aegis-shadow`) : champion / challenger papier, opportunités
  manquées, entrées et sorties prématurées, évaluation historique, promotion explicite vers la
  démo.
- **aegisd** : API complète (résumé, sections, commandes génériques), HTTPS avec certificat
  épinglé (`aegis-client`), jeton automatique, limitation des échecs, appairage, surveillance
  CPU/RAM, service Windows, unité systemd, heartbeat, sauvegardes automatiques, alertes
  (webhook).
- **AEGIS Desktop** (Tauri 2, React, TypeScript, Tailwind) remplaçant l'application egui :
  10 pages, moteur local détaché ou VPS, simulateur de test, identité visuelle (logo, icônes).
- **Pipeline CI** (Windows, Linux, macOS) : tests, serveur, installateurs, installation
  automatique, compilation MQL5, dossier `RELEASES/`.
- **Connecteur 1.10** (protocole 1.1, compatible) : frais, devise de base, niveau de gel.
- Test de capacité : 2 000 allers-retours.
- Documentation : modes, sécurité, configuration, API, VPS, installations Windows/Linux,
  compilation, connecteur, validation des modèles, tests, dépannage.

### Modifié

- L'API est activée par défaut (boucle locale, TLS).
- L'objectif journalier en % d'équité (`daily_profit_target_pct`) est remplacé par
  `daily_goal` (une configuration 0.1 contenant l'ancienne clé doit être mise à jour).
- Les paramètres de stratégie enregistrés survivent aux redémarrages.
- Les tests embarquent leurs fichiers du dépôt à la compilation (défaut d'environnement macOS).

### Retiré

- Application de bureau egui (`aegis-control`), remplacée par AEGIS Desktop.

## 0.1.0 — 2026-11-15

Première version complète. Tout a été développé et testé sur macOS (Apple Silicon).
Ce qui n'a pas pu être vérifié est listé dans `KNOWN_LIMITATIONS.md` et `PROJECT_STATUS.md`.

### Ajouté, par phase

- **Phases 0 à 2 — fondations, données, risque.** Workspace Rust de 13 crates. Types
  partagés. Stockage SQLite (migrations, clés étrangères, refus des doublons, journal d'audit
  en ajout seul chaîné par SHA-256, sauvegarde et restauration vérifiées, rétention). Moteur
  de risque : taille de position d'après les spécifications du broker, coûts (spread,
  commission, swap, slippage, conversion), espérance nette, limites journalières, drawdown,
  exposition, autorisations signées à usage unique, arrêt d'urgence persistant.
- **Phases 3 et 5 — protocole, marché.** Protocole v1 : trames JSON plates sur TCP,
  HMAC-SHA256, poignée de main, clé de session, séquence stricte, fenêtre d'horodatage.
  Moteur de marché : bougies depuis les ticks, EMA/RSI/ATR, régimes, sessions, contrôle de
  qualité (prix obsolètes, spreads anormaux, horloge, données manquantes).
- **Phase 6 — stratégies, backtesting.** Trois stratégies (`ema_trend`, `rsi_reversion`,
  `donchian_breakout`), cycle de vie Recherche → Démo → Approuvée réel avec preuves exigées.
  Backtester à hypothèses pessimistes, métriques, segments chronologiques, walk-forward,
  import CSV (dont exports MT5), générateur synthétique déterministe.
- **Phase 7 — IA.** Variables explicatives calculées en Rust, export du jeu de données,
  entraînement Python (régression logistique calibrée, walk-forward avec embargo), artefact
  JSON, inférence Rust à parité vérifiée, portail de validation réévalué par le moteur, suivi
  de dérive (PSI). Le modèle ne peut que filtrer un signal, jamais en créer un.
- **Phases 8 et 10 — exécution, intégration.** Cycle de vie des ordres idempotent, état
  `unknown` et réconciliation, gestion des positions, moteur central (boucle unique, pont
  réseau, chaîne de décision, supervision), simulateur de terminal (adaptateur de test),
  14 tests de bout en bout.
- **Phase 9 — application.** Application de bureau `aegis` (egui, 18 pages) avec le moteur
  dans le même processus ; moteur sans interface `aegisd` avec API HTTP à jeton et outils en
  ligne de commande.
- **Phase 4 — connecteur MT5.** Expert Advisor `AegisConnector.mq5`, bibliothèque
  `AegisProtocol.mqh`, script `AegisSelfTest.mq5`. **Jamais compilé** (pas de MetaEditor) ;
  des tests Rust figent les formats que le code MQL5 produit et lit.
- **Phase 11 — livraison.** Configuration d'exemple, scripts d'installation, de lancement, de
  test, de sauvegarde et de recherche, flux de compilation GitHub Actions (non exécuté),
  documentation complète (`README.md`, `ARCHITECTURE.md`, `DECISIONS.md`, `docs/`), rapport
  de tests, archives ZIP.

### Corrigé pendant le développement

Défauts trouvés par les tests, tous corrigés :

- **Réconciliation** : une position ouverte puis clôturée avant toute confirmation était
  déclarée « non exécutée ». Les clôtures portent maintenant l'identifiant de l'ordre
  d'origine (migration `0003_deal_origin`).
- **Réseau** : une lecture de trame interrompue pouvait perdre des octets et couper la
  session. Le lecteur conserve la trame partielle.
- **Publication** : la réponse à une commande pouvait précéder l'état publié qui en reflète
  l'effet.
- **Base verrouillée** : une base indisponible figeait le moteur. La sonde (qui écrit, pour
  détecter une base lisible mais non inscriptible) tourne désormais à part, les écritures non
  essentielles sont suspendues, les entrées sont bloquées et le moteur reprend seul.
- **Arrêt d'`aegisd` avec l'API activée** : le moteur n'était pas arrêté proprement (base non
  refermée). L'arrêt passe maintenant par une référence partagée (`EngineHandle::stop`).
- **Fermeture de la base** : le journal d'écriture (WAL) est replié dans `aegis.db` à l'arrêt ;
  après un arrêt propre, ce fichier seul contient toutes les données.

### Non vérifié dans cette version

- Compilation du connecteur MQL5 et connexion à un vrai terminal MT5.
- DEMO AUTO sur un compte de démonstration réel (vérifié avec le simulateur uniquement).
- Compilation et exécution sous Windows et sous Linux.
- Aspect visuel de l'application.
