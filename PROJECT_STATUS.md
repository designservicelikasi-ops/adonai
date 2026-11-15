# AEGIS — État du projet

Ce fichier permet de reprendre le développement sans perdre le contexte.
Dernière mise à jour : 2026-11-15 (version 0.1.0).

## En une phrase

Le logiciel est complet, compilé et testé **sur macOS avec le simulateur**. Il **n'est pas
encore déclaré terminé** au sens de la spécification : deux critères de livraison dépendent
d'un terminal MetaTrader 5 sous Windows, indisponible sur la machine de développement
(critères 7 et 12 ci-dessous).

## Phases

| Phase | Contenu | État |
|---|---|---|
| 0 | Environnement, workspace Rust, Git | Fait |
| 1 | Core + Data (SQLite, migrations, audit chaîné, sauvegarde/restauration) | Fait, testé |
| 2 | Moteur de risque (taille, coûts, limites, autorisations, arrêt d'urgence) | Fait, testé |
| 3 | Protocole authentifié (HMAC, session, anti-rejeu) | Fait, testé |
| 4 | Connecteur MT5 (Expert Advisor MQL5, auto-test) | **Écrit, jamais compilé** — pas de MetaEditor |
| 5 | Moteur de marché (bougies, indicateurs, régimes, qualité) | Fait, testé |
| 6 | Stratégies + backtesting | Fait, testé |
| 7 | IA : variables, entraînement Python, inférence Rust, portail, dérive | Fait, testé sur données synthétiques |
| 8 | Exécution + micro-profits + réconciliation | Fait, testé de bout en bout avec le simulateur |
| 9 | Application de bureau (18 pages) + `aegisd` (API HTTP, ligne de commande) | Fait, testé sans fenêtre ; aspect visuel non inspecté |
| 10 | Intégration globale | Fait : 14 tests de bout en bout |
| 11 | Compilation, documentation, archives | Fait pour macOS ; Windows et Linux non compilés |

Tests : **159 réussis, 0 échoué** (`./scripts/test.sh`). Détail dans `TEST_REPORT.md`.

## Critères de livraison de la spécification (§ 12)

| # | Critère | État | Preuve ou raison |
|---|---|---|---|
| 1 | Le projet compile dans l'environnement cible vérifié | **Vérifié sur macOS (Apple Silicon)** ; Windows et Linux **NON VÉRIFIÉ** | `cargo build --release` : binaires `aegis` et `aegisd` produits et lancés |
| 2 | Tous les modules implémentés sont intégrés | Vérifié | `aegis-core/tests/e2e.rs` : cycle complet en 15 étapes |
| 3 | La base fonctionne et résiste aux redémarrages | Vérifié | `aegis-data/tests/store.rs`, tests de redémarrage et de panne de base |
| 4 | Le moteur de risque réussit ses tests | Vérifié | 25 tests (`aegis-risk`) |
| 5 | Les ordres non autorisés sont bloqués | Vérifié | autorisation forgée, expirée, réutilisée, arrêt d'urgence, MONITOR |
| 6 | Le protocole réussit ses tests | Vérifié côté moteur | 20 tests (`aegis-protocol`) + tests de bout en bout |
| 7 | Le connecteur MT5 compile lorsque MetaEditor est disponible | **NON VÉRIFIÉ** | MetaEditor indisponible (macOS) |
| 8 | Le backtesting produit des résultats reproductibles | Vérifié | tests de reproductibilité (unitaire et bout en bout) |
| 9 | Le sous-système IA a des résultats de validation documentés | Vérifié **sur données synthétiques** | `models/demo-synthetic-model.report.md` ; portail **non passé**, comme attendu. Aucun modèle sur données réelles |
| 10 | L'interface communique avec le véritable backend | Vérifié sans fenêtre ; aspect visuel **NON VÉRIFIÉ** | `aegis-control/tests/ui.rs` ; application lancée |
| 11 | MONITOR ne peut pas exécuter de transactions | Vérifié | `monitor_mode_observes_but_never_trades` |
| 12 | DEMO AUTO fonctionne dans un environnement de démonstration vérifié | **NON VÉRIFIÉ** sur un compte de démonstration MT5 | Vérifié uniquement avec le simulateur, qui est un adaptateur de test et non un broker |
| 13 | LIVE AUTO reste désactivé par défaut | Vérifié | `live_auto_is_off_by_default_and_cannot_be_enabled_casually` |
| 14 | Journaux et traces d'audit persistants | Vérifié | audit chaîné, falsification détectée, conservé après arrêt brutal |
| 15 | Procédures d'installation et de récupération documentées | Fait | `docs/DEPLOYMENT.md`, `docs/BACKUP_RECOVERY.md`, `scripts/` (`setup.ps1` non exécuté) |
| 16 | Rapport complet de tests livré | Fait | `TEST_REPORT.md` |

## Prochaines tâches concrètes

Dans cet ordre. Les trois premières demandent un PC Windows avec MetaTrader 5.

1. **Compiler le connecteur** dans MetaEditor et corriger les erreurs (`mt5/`).
   Procédure : `docs/DEVELOPER_MANUAL.md`, § 14.
2. Lancer `AegisSelfTest` : il doit afficher `ALL PASSED`.
3. **Essai sur un compte de démonstration** : suivre la liste de contrôle de
   `docs/DEPLOYMENT.md`, § 6, moteur en MONITOR d'abord, puis DEMO AUTO. Reporter ce qui est
   réellement constaté dans `TEST_REPORT.md`.
4. Compiler sous Windows et sous Linux (`scripts/setup.ps1`, `scripts/setup.sh`, ou le flux
   `.github/workflows/build.yml` après un envoi sur GitHub).
5. Regarder l'application à l'écran, page par page, et corriger la mise en page si besoin.
6. Collecter des bougies **réelles** par le connecteur, relancer backtests et entraînement
   (`./scripts/research.sh EURUSD M15 broker`). Aucune stratégie ni aucun modèle n'a encore
   été évalué sur des données de marché réelles.

Améliorations possibles ensuite : voir `KNOWN_LIMITATIONS.md` (liaison non chiffrée, comptes
« netting », paramétrage des stratégies depuis l'interface, journée comptable à l'heure du
broker).

## Comment reprendre

```sh
. "$HOME/.cargo/env"
cd aegis
./scripts/test.sh                 # tous les tests, sortie compacte
./scripts/run.sh                  # application de bureau (après ./scripts/setup.sh)
./scripts/run-daemon.sh --simulator   # moteur sans interface + simulateur de test
```

Lire d'abord : `README.md`, puis `ARCHITECTURE.md`, `DECISIONS.md` et
`docs/DEVELOPER_MANUAL.md`.

## Archives

`dist/` (non suivi par Git) :

- `aegis-0.1.0-source.zip` : projet complet (code, connecteur MT5, migrations, configuration
  d'exemple, scripts, documentation, rapport de tests) ;
- `aegis-0.1.0-macos-arm64.zip` : binaires `aegis` et `aegisd` compilés pour macOS Apple
  Silicon, avec la configuration d'exemple et la documentation.

Il n'existe pas de binaire Windows ni Linux : ils n'ont pas pu être compilés ici.

## Décisions à ne pas perdre

Voir `DECISIONS.md`. Les plus structurantes :

- Le moteur démarre **toujours** en MONITOR ; aucun mode automatique n'est restauré.
- En MONITOR, **aucune** trame d'ordre ne sort du moteur (y compris les clôtures manuelles).
- Le simulateur (`aegis-simterminal`) est un adaptateur de test : instrument `EURUSD.sim`,
  données stockées comme `synthetic`, jamais éligible au réel.
- Une probabilité de gain n'existe que si un backtest hors échantillon (≥ 30 transactions)
  est enregistré ; sinon l'espérance nette est « non calculable » et l'entrée est refusée.
- Le modèle IA ne peut que s'opposer à un signal ; il n'en crée jamais.
