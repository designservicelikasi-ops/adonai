# Historique des versions

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
