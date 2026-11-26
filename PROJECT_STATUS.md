# AEGIS — État du projet et rapport de livraison

## RAPPORT FINAL 0.3.0 (2026-11-26) — directive « Master Upgrade »

Audit et plan : `docs/AUDIT_V3.md`. Détail des ajouts : `CHANGELOG.md` (section 0.3.0).
Tests : `TEST_REPORT.md`. Limites : `KNOWN_LIMITATIONS.md` (31 points).

### En une phrase

Les neuf phases de la directive sont réalisées et testées **sur macOS avec le simulateur**
(306 tests, application 0.3.0 compilée) ; AEGIS **n'est pas encore prêt pour un compte réel** :
le connecteur 1.20 n'a jamais tourné dans un vrai terminal MT5, et aucune stratégie ni aucun
modèle n'a été évalué sur des données de marché réelles.

| Phase | État | Commit |
|---|---|---|
| 1 Audit | fait | `ca159c6` |
| 2 Risk Guardian 2.0 et exécution | fait (testé contre le simulateur) | `5fc84e6` |
| 3 Autonomy Commander | fait | `1129c62` |
| 4 Régimes, multi-unités, structure, scanner | fait | `1729d4a` |
| 5 Micro-Profit 2.0, Recovery & Hedge | fait | `ef768ac` |
| 6 Fast Learning | fait | `81c599a` |
| 7 Multi-actifs, exposition corrélée | fait | `9c80b6d` |
| 8 Desktop blanc et or (25 pages) | fait (contrôle visuel en mode aperçu) | `e09c50b` |
| 9 Validation, Telegram, documentation, livraison | fait ; CI 0.3 : `TEST_REPORT.md` § 6 | ce commit |

### Ce qui est livré

- **Moteur** (`aegisd` 0.3.0) : Risk Guardian 2.0 (risque de portefeuille aux stops, budget
  journalier, réduction après pertes, jours autorisés, garde de protection des stops chez le
  broker, stops structure / volatilité), Autonomy Commander (4 profils, autorisations liées au
  compte, datées, révocables, journal des permissions), régimes détaillés, lecture
  multi-unités, structure de marché, scanner d'opportunités, Micro-Profit 2.0, Recovery &
  Hedge anti-martingale, Fast Learning (recherche de paramètres parallèle, audit de données,
  retour arrière sur dérive), multi-actifs et corrélations, qualité d'exécution, alertes
  webhook et **Telegram** (jeton hors configuration, D42).
- **Connecteur** MQL5 1.20 (protocole 1.2 : mode de marge, latence broker).
- **AEGIS Desktop** 0.3.0 : thème blanc et or, 25 pages en 5 groupes, palette Ctrl/Cmd+K.
- **Documentation** : README, ARCHITECTURE, KNOWN_LIMITATIONS, TEST_REPORT, CHANGELOG,
  DECISIONS (D42), `docs/` (dont AUTONOMY, MARKET_INTELLIGENCE, RECOVERY_HEDGE,
  FAST_LEARNING, MULTI_ASSET).
- **`RELEASES/`** (hors Git, sauf son README) : application et moteur macOS 0.3.0, sources du
  connecteur, documentation, notes de version, empreintes ; la 0.2.0 est rangée dans
  `RELEASES/archive-0.2.0/`.

### Vérifié

- 306 tests automatisés réussis (dont 29 de bout en bout avec le vrai protocole et le
  simulateur), `cargo check --locked` du desktop, `npm run build`.
- Moteur et `AEGIS.app` 0.3.0 compilés sur macOS arm64 ; moteur lancé avec le simulateur :
  backtest, recherche de paramètres, audit de données, modèle de probabilité, autorisation
  DEMO, refus motivés des signaux.
- Les 25 pages rendues avec les vraies réponses du moteur, sans erreur.
- CI GitHub du 2026-11-21 (0.2 + cerveau) : Windows et Linux (tests, moteurs, installateurs)
  et compilation MQL5 du connecteur 1.10 réussis. CI de la 0.3 : `TEST_REPORT.md` § 6.

### NON VÉRIFIÉ

- Connecteur dans un vrai terminal MT5 et sur un compte démo (procédure :
  `docs/MT5_CONNECTOR.md`) ; latence broker réelle.
- Stratégies et modèles sur données réelles.
- Service Windows et VPS ; envoi réel vers Telegram ou un webhook.
- Pages de positions et d'ordres avec de vraies positions ; fenêtre native et Ctrl/Cmd+K au
  clavier.
- Test macOS dans la CI : GitHub refuse le runner `macos-14` pour ce compte (les tests macOS
  sont faits localement). Retirer ou remplacer ce runner dans le workflow est une décision à
  prendre par l'utilisateur.

### Prochaines étapes (dans cet ordre)

1. Lire le résultat de la CI 0.3 (`TEST_REPORT.md` § 6) et corriger ce qu'elle révèle,
   notamment la compilation MQL5 du connecteur 1.20.
2. Compte démo : `AegisSelfTest`, puis MONITOR, puis DEMO FULL AUTONOMY ; reporter les
   mesures dans `TEST_REPORT.md`.
3. Collecter l'historique broker, entraîner, backtester, lancer la recherche de paramètres.
4. Configurer Telegram (`[ops] telegram_chat_id` + variable `AEGIS_TELEGRAM_TOKEN`) et
   envoyer une notification de test.

### Reprendre
```sh
cd aegis && . "$HOME/.cargo/env"
git config core.filemode false     # disque exFAT : ignorer les droits
./scripts/test.sh                  # 306 tests attendus
cd desktop && npm run build
```

---

## Historique 0.2.0

Dernière mise à jour : 2026-11-18. Référence : spécification v2 (`docs/SPECIFICATION.md`),
audit et plan (`docs/AUDIT_V2.md`). Ce fichier permet de reprendre le travail sans perdre le
contexte et répond à la partie XVIII de la spécification. **Tout résultat ci-dessous a été
observé ; ce qui ne l'a pas été est marqué NON VÉRIFIÉ, avec la raison et la procédure.**

## Mise à jour du 2026-11-21 — cerveau micro-profits (branche `cerveau-micro-profits`)

Ajout du **cerveau micro-profits** (`crates/aegis-brain`, `crates/aegis-core/src/brain.rs`,
page *Cerveau IA*) : il choisit la cible de chaque signal sur EURUSD pour un gain **net** de
0,08 à 2,00, apprend des transactions clôturées (frais réels, correction des probabilités,
contextes perdants) et réentraîne le modèle de probabilité toutes les 24 h ; une meilleure
version est **proposée**, jamais activée sans l'utilisateur (choix de l'utilisateur, D33).
Désactivé par défaut : `[brain] enabled = true`, et baisser `min_net_expected_profit`
(≈ 0,01) sinon les micro-gains sont refusés.

Vérifié : 237 tests (dont 2 de bout en bout : transaction du cerveau exécutée, clôturée et
apprise ; version proposée puis activée par l'utilisateur seulement) ; essai du moteur avec
le simulateur : réentraînement, proposition, activation, refus motivés des signaux sans
probabilité calibrée ; page *Cerveau IA* affichée avec ces données réelles.
**NON VÉRIFIÉ** : comportement sur un vrai compte démo MT5 (même raison que § 7-8) ; aucune
transaction du cerveau basée sur le modèle de scénario n'a été observée en direct (les
signaux de l'essai étaient refusés à juste titre : côté vente non calibré, espérance négative
côté achat).

## En une phrase

Le logiciel est complet et testé **sur macOS avec le simulateur** (223 tests, application
compilée et lancée, capacité mesurée) ; il **n'est pas encore livrable au sens de la
spécification** car trois vérifications exigent des environnements absents de la machine de
développement : builds/installateurs Windows et Linux (pipeline CI prêt, jamais exécuté faute
de dépôt distant), compilation du connecteur MQL5 et essai sur un vrai compte démo MT5.

## 1. Architecture réellement réalisée

Trois composants (détail : `ARCHITECTURE.md`) :

- **AEGIS ENGINE** — `aegisd`, workspace Rust de 16 crates : marché, 9 stratégies, Probability
  Engine, risque (objectif quotidien, gestion des positions), exécution, journal, performance,
  actualités, Shadow, données (SQLite), API Axum + TLS, service Windows/systemd, surveillance.
- **AEGIS CONNECTOR** — `mt5/AegisConnector.mq5` 1.10 (protocole 1.1).
- **AEGIS DESKTOP** — `desktop/` : Tauri 2 + React 18 + TypeScript + Tailwind, client épinglé de
  l'API, moteur local embarqué lancé en processus détaché.

## 2. Modules opérationnels (vérifiés par des tests exécutés)

| Module | Preuve |
|---|---|
| Market Intelligence (ticks, bougies M1–H1, ATR, volatilité, momentum, tendance, Bollinger, supports/résistances, régimes, qualité) | `aegis-market` 18 tests ; e2e |
| Strategy Engine — 9 stratégies (EMA crossover, RSI momentum, suivi de tendance, breakout, pullback, retour à la moyenne RSI et Bollinger, rejet S/R, volatilité) | `aegis-strategy` 20 tests (un scénario par stratégie, pas de regard vers le futur) |
| Probability Engine — P(TP avant SL), Brier, log loss, fiabilité, walk-forward, calibration, versions, retour arrière | `aegis-probability` 10 tests ; e2e `probability_models_…` ; essai en release (§ 12) |
| Entry Intelligence — BUY / SELL / WAIT | e2e ; observé dans l'interface |
| Position Intelligence — HOLD / SECURE / ADJUST SL / ADJUST TP / CLOSE | `aegis-risk` 7 tests ; e2e (stop sécurisé constaté sur le simulateur) |
| Risk Engine (toutes les limites de la partie VIII) | `aegis-risk` 47 tests |
| Execution Engine (idempotence, partiels, rejets, inconnu, réconciliation) | `aegis-execution` 10 tests ; e2e |
| Micro Profit Engine (coûts, espérance nette) | `aegis-risk` costs ; capacité (§ 12) |
| Daily Goal Manager (brut/net, restant, politiques) | `aegis-risk` goal ; e2e |
| News Intelligence | `aegis-news` 5 tests + lecture réelle (125 événements) ; e2e suspension |
| Shadow Intelligence (champion/challenger, opportunités manquées, analyses) | `aegis-shadow` 5 tests ; e2e indépendance et refus de promotion |
| Learning (versions de jeux de données et de modèles, calibration, dérive, retour arrière) | `aegis-data` journal ; `aegis-learning` 12 tests |
| Base de données et historique (journal, performance) | `aegis-data` 14 tests ; `performance` 4 tests ; e2e |
| API sécurisée (TLS épinglé, jeton, limitation, chemins confinés) | `aegisd` tests `api`, `tls_api` ; essai réel avec `openssl` |
| Fonctionnement continu (heartbeat, sauvegardes, alertes, CPU/RAM, unité systemd) | e2e `unattended_operation_…` ; tests `monitor`, `service` |
| Desktop | compilé (`AEGIS.app`), lancé ; 10 pages rendues avec des données réelles du moteur |

## 3. Modules partiels

| Module | Ce qui manque |
|---|---|
| Connecteur MT5 | écrit et aligné sur le protocole 1.1, **jamais compilé** (pas de MetaEditor) |
| Service Windows | écrit, **jamais compilé** (code Windows uniquement) |
| News | pas d'interprétation par modèle de langage (option, voir `DECISIONS.md` D30) ; la source ne publie pas les valeurs réelles |
| Shadow | forward papier sans slippage, positions papier non persistées entre deux redémarrages |

## 4. Modules non réalisés

Aucun module de la spécification n'est absent. Non réalisés : l'interprétation des actualités
par modèle de langage (facultative) ; un modèle ONNX (la régression logistique suffit, D10/D24).

## 5. Résultats des tests

**223 réussis, 0 échoué** (`./scripts/test.sh`) + 2 tests lancés à part (réseau, capacité),
réussis. TypeScript : `tsc --noEmit` sans erreur. Détail par module : `TEST_REPORT.md`.

## 6. Résultats de compilation

| Cible | État |
|---|---|
| Moteur macOS arm64 (release) | **Vérifié** — 13,5 Mo |
| AEGIS Desktop macOS arm64 (`.app`) | **Vérifié** — 21,5 Mo, moteur embarqué |
| Moteur et installateurs Windows x64 (.exe NSIS, .msi) | **NON VÉRIFIÉ** — job CI `server`/`desktop` |
| Moteur et paquets Linux x64 (AppImage, .deb, .rpm) | **NON VÉRIFIÉ** — job CI `server`/`desktop` |
| Connecteur MQL5 (.ex5) | **NON VÉRIFIÉ** — job CI `mql5` |

## 7. Tests MT5

**NON VÉRIFIÉ.** Aucun terminal MT5 disponible. Couverture actuelle : tests de compatibilité
qui rejouent exactement les trames produites par le code MQL5 (protocoles 1.0 et 1.1) et
toute la chaîne contre le simulateur. Procédure : `docs/MT5_CONNECTOR.md`.

## 8. Tests DEMO

**NON VÉRIFIÉ** sur un compte de démonstration réel. DEMO AUTO a fonctionné contre le
simulateur (e2e et 10 minutes d'exécution observées).

## 9. Builds Windows

**NON VÉRIFIÉS.** `.github/workflows/build.yml` construit sur `windows-latest` : moteur,
installateurs NSIS et MSI, installation silencieuse, présence d'`aegisd.exe`, désinstallation,
compilation MQL5. Il faut pousser le dépôt sur GitHub pour l'exécuter.

## 10. Builds Linux

**NON VÉRIFIÉS.** Même pipeline sur `ubuntu-22.04` : AppImage, .deb, .rpm ; installation et
désinstallation du .deb.

## 11. Fichiers livrables

`RELEASES/` (ne contient que des fichiers réellement produits, voir `RELEASES/README.md`) :
application macOS (zip), moteur macOS, sources du connecteur, documentation, notes de version.
Aucun fichier Windows ni Linux : ils n'existent pas encore.

## 12. Performances mesurées (macOS arm64, release)

| Mesure | Valeur |
|---|---|
| 2 000 allers-retours (entrée + sortie) par la chaîne complète, broker simulé | 1,66 s ; médiane 0,76 ms, p99 2,47 ms, max 4,76 ms |
| Décision de risque | médiane 7 µs |
| Entraînement P(TP avant SL), 8 000 bougies, 40 scénarios, 5 plis | 1,4 s |
| Moteur au repos avec simulateur | ≈ 23 Mo de mémoire |
| Latence MT5 / broker | **non mesurée** (compte démo requis) |

## 13. Limites connues

`KNOWN_LIMITATIONS.md` (25 points). Les plus importantes : rien n'est validé sur données de
marché réelles ; liaison MT5 non chiffrée (boucle locale ou tunnel) ; commission à saisir.

## 14. Problèmes restants

1. Exécuter le pipeline CI (Windows, Linux, MQL5) et corriger ce qu'il révélera — en
   particulier la compilation MQL5, écrite sans compilateur.
2. Essai complet sur compte démo (procédure : `MT5_CONNECTOR.md`).
3. Collecte d'historique broker, entraînement, backtests réels, Shadow sur bougies réelles ;
   aucune stratégie n'a de preuve réelle aujourd'hui.
4. Installer le service sur un VPS et éprouver le redémarrage automatique.

## 15. Instructions d'installation

`docs/INSTALLATION_WINDOWS.md`, `docs/INSTALLATION_LINUX.md` ; depuis les sources :
`./scripts/setup.sh` puis `./scripts/run.sh`.

## 16. Instructions de déploiement VPS

`docs/VPS_DEPLOYMENT.md` : `packaging/windows/install-service.ps1` (ou
`packaging/linux/install.sh`), pare-feu limité à votre adresse, `aegisd pairing`, connexion du
desktop.

## Comment reprendre

```sh
. "$HOME/.cargo/env"
./scripts/test.sh                                    # 223 tests
cargo run -p aegisd -- run --simulator               # moteur + simulateur
./scripts/desktop-dev.sh                             # interface en développement
cargo test --release -p aegis-core --test capacity -- --ignored --nocapture
```

Lire d'abord `README.md`, `ARCHITECTURE.md`, `DECISIONS.md` (D21–D30 pour la 0.2), puis
`docs/DEVELOPER_MANUAL.md`.

## Historique des phases (spécification v2)

| Phase | Contenu | Commit |
|---|---|---|
| 0 | audit, spécification v2, correctif d'environnement des tests | `a3c0a5f`, audit |
| A | objectif quotidien, brut/net, limites étendues, protocole 1.1 | `7471155` |
| B | 6 stratégies, Position Intelligence | `c6133bb` |
| C | Probability Engine, Entry Intelligence, journal | `3ceaa23` |
| D | performance | `cea3775` |
| E | News Intelligence | `78b0f43` |
| F | Shadow, paramètres de stratégie | `a3832a2` |
| G | API, TLS, service, ops | `d988c11` |
| H | AEGIS Desktop, CI, packaging, documentation | `24ebcf7` |
| I | capacité, version 0.2.0 | `2f680c4` |
| K | documentation finale, `RELEASES/` | dernier commit |
