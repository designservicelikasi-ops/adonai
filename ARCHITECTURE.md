# Architecture d'AEGIS (0.3)

## Trois composants

```
 MetaTrader 5 (Windows)                AEGIS ENGINE — aegisd (Windows / Linux, VPS)              AEGIS DESKTOP (Tauri 2)
┌───────────────────────┐ TCP, HMAC ┌──────────────────────────────────────────────────────┐ HTTPS ┌─────────────────────┐
│ AegisConnector.mq5    │──────────▶│ bridge → engine (boucle d'événements unique)         │◀─────▶│ React + TypeScript  │
│ prix, compte,         │◀──────────│  market → strategies → probability → risk → exec     │épinglé│ côté Rust : client  │
│ positions, clôtures,  │ ordres    │  positions (Position Intelligence), goal, journal    │+jeton │ aegis-client        │
│ exécute les ordres    │ autorisés │  guardian, autonomy, scanner, recovery, portfolio    │       │ (épinglage TLS)     │
│ authentifiés          │           │  news, shadow (tâches) ; data (SQLite, audit chaîné) │       │ moteur local        │
└───────────────────────┘           │  ops (heartbeat, sauvegardes, alertes webhook/Telegram), API Axum + rustls │       │ détaché (optionnel) │
                                    └──────────────────────────────────────────────────────┘       └─────────────────────┘
```

- Le moteur est autonome : il tourne en service (Windows SCM ou systemd), sans interface.
- Le desktop n'est qu'un client : le fermer n'arrête ni le moteur ni les positions.
- La liaison MT5 reste locale (même VPS) ou dans un tunnel ; seule l'API est exposée, chiffrée
  et épinglée.

## Modules (un crate chacun, dans `crates/`)

| Crate | Rôle |
|---|---|
| `aegis-types` | types partagés : instrument, tick, bougie, compte, position, transaction, modes |
| `aegis-data` | SQLite (SQLx) : migrations, audit chaîné, ordres, décisions, **journal des transactions**, **versions de modèles de probabilité**, **jeux de données**, sauvegarde/restauration |
| `aegis-market` | bougies à partir des ticks (M1 à D1), indicateurs (EMA, RSI, ATR, Donchian, Bollinger, pivots), régimes détaillés, sessions, qualité des données ; **lecture multi-unités** (`mtf.rs`) ; **structure** (swings, cassures, changements de caractère, zones de déséquilibre — bougies clôturées seulement, `structure.rs`) ; **scanner d'opportunités** avec score expliqué, distinct de la probabilité (`scanner.rs`) ; **audit d'un historique** (`audit.rs`) |
| `aegis-strategy` | 9 stratégies : EMA crossover, RSI momentum, suivi de tendance, breakout, pullback, retour à la moyenne RSI, retour à la moyenne Bollinger, rejet S/R, expansion de volatilité ; cycle de vie |
| `aegis-probability` | **Probability Engine** : P(TP avant SL), triple barrière, régression logistique, walk-forward, calibration, Brier/log loss/fiabilité |
| `aegis-brain` | **Cerveau micro-profits** : choix de la cible dans une fourchette de gain net, leçons des transactions réelles (frais, calibration, contextes), comparaison des versions de modèle |
| `aegis-risk` | **Risk Engine / Risk Guardian** : taille, coûts, espérance nette, limites, seuils de probabilité, **risque de portefeuille aux stops**, budget journalier, réduction après pertes, jours autorisés, **exposition corrélée** (`exposure.rs`), **Daily Goal Manager**, **Position Intelligence**, **stop initial** stratégie / structure / volatilité (`stops.rs`), **garde de protection** (stop manquant, modification non constatée, `protection.rs`), **règles de re-entry et de couverture** anti-martingale (`recovery.rs`), autorisations signées, arrêt d'urgence |
| `aegis-execution` | cycle de vie des ordres, idempotence, délais, état inconnu, réconciliation |
| `aegis-protocol` | protocole MT5 : trames, HMAC, poignée de main, session anti-rejeu |
| `aegis-backtest` | backtest avec coûts, segments, walk-forward, CSV, générateur synthétique ; **recherche de paramètres en parallèle** (walk-forward sur l'apprentissage, contrôle jamais vu, sur-apprentissage et instabilité signalés, `sweep.rs`) |
| `aegis-learning` | variables explicatives (source unique), filtre logistique Python, dérive |
| `aegis-news` | **News Intelligence** : calendrier, recommandation autorisé / prudent / suspendu |
| `aegis-shadow` | **Shadow Intelligence** : champion/challenger papier, opportunités manquées, analyse des sorties ; **aucune dépendance** vers l'exécution |
| `aegis-autonomy` | **Autonomy Commander** : capacités, 4 profils (OBSERVER, RESEARCH, DEMO FULL, LIVE AUTHORIZED), autorisations liées au compte, datées, révocables ; seule une confirmation de l'utilisateur peut en créer une (test d'architecture) |
| `aegis-core` | moteur : configuration, orchestration, décisions (`decide.rs`), positions, intelligence (`intel.rs`), cerveau (`brain.rs`), **gardien** (`guardian.rs`), **autonomie** (`autonomy.rs`), **scanner** (`scanner.rs`), **micro-profit et récupération** (`recovery.rs`), **Fast Learning** (`learning.rs`), **portefeuille et corrélations** (`portfolio.rs`), **qualité d'exécution** (`latency.rs`), performance, lien Shadow, ops et alertes (`ops.rs`), état publié |
| `aegis-simterminal` | **adaptateur de test** : terminal et broker simulés parlant le vrai protocole |
| `aegis-client` | client de l'API avec épinglage du certificat (utilisé par le desktop) |
| `aegisd` | binaire du moteur : service, API Axum + TLS, surveillance système, outils en ligne de commande |

Hors workspace : `desktop/` (Tauri 2 : `src/` React, 25 pages dans `src/pages/`, registre et
navigation dans `src/App.tsx` ; `src-tauri/` Rust), `mt5/` (MQL5, connecteur 1.20),
`research/` (Python), `packaging/` (installation des services).

Les dépendances interdites sont des erreurs de compilation : la stratégie ne voit ni le risque
ni l'exécution ; le Shadow ne voit ni l'exécution ni le protocole ; une entrée ne peut être
construite qu'à partir d'une `Authorization` que seul `aegis-risk` sait signer.

## Le chemin d'une décision

1. **Tick** → `MarketEngine` valide, met à jour les bougies et les excursions des positions.
2. **Bougie clôturée** sur une unité de signal → BUY/SELL de référence recalculés (Entry
   Intelligence), copie au Shadow, puis chaque stratégie active évalue les 300 dernières
   bougies (jamais la bougie en cours).
3. **Intention** (sens, distance de stop, distance de cible, raison).
4. **Contrôles du moteur** : **autorisation de l'Autonomy Commander**, base disponible,
   instrument actif, stade de la stratégie, régime, qualité des données, ordre déjà en cours,
   filtre IA, **actualités** ; choix du stop initial (le volume est recalculé sur sa distance).
5. **Probabilité** P(TP avant SL) : la plus prudente des sources validées (modèle de scénario
   calibré, borne de Wilson du backtest hors échantillon). Sans source : refus.
6. **Risque** : taille (au risque, plafonnée), coûts (spread, commission, slippage, swap,
   conversion), espérance nette, seuil BUY/SELL, limites (jour, objectif, transactions, pertes
   consécutives, débit, horaires et jours, exposition globale, par devise et corrélée,
   risque de portefeuille aux stops, budget de risque du jour, coûts du jour ; re-entry et
   couverture : règles anti-martingale).
7. **Décision écrite en base avant tout envoi** ; journal préparé.
8. **Autorisation consommée** (signée, 3 s, usage unique) → ordre persisté → trame.
9. **Connecteur** : revérifie, exécute, rapporte la réponse du broker.
10. **Exécution** : `filled` / `rejected` / partiel / `unknown` → réconciliation. Journal :
    prix exécuté, slippage mesuré.
11. **Position Intelligence** à chaque bougie : HOLD / SECURE / ADJUST SL / ADJUST TP /
    clôture partielle / CLOSE ; chaque modification est **vérifiée** dans l'instantané broker
    suivant ; une position sans stop déclenche la garde de protection.
12. **Clôture** : transaction du broker → comptabilité du jour (brut, net, coûts), objectif,
    journal (résultat, MFE/MAE), performance, calibration réelle, Shadow.

## Concurrence

Boucle d'événements unique (Tokio) pour l'état du moteur. À part, en tâches : réseau MT5,
calendrier (tâche bloquante), entraînement des probabilités et backtests (`spawn_blocking`),
sonde de base, Shadow (tâche indépendante recevant des copies par canal borné ; une copie
perdue est comptée, jamais bloquante), API (Axum), surveillance CPU/RAM (fil dédié).
L'interface lit un instantané immuable (`EngineView`) publié quatre fois par seconde.

## Persistance

SQLite WAL, `synchronous=FULL`, clés étrangères. Tables : `symbols`, `candles`,
`account_snapshots`, `strategies`, `decisions`, `orders`, `order_events`, `commands`,
`positions`, `deals` (avec frais), `risk_configs` (versions), `model_versions`,
`backtest_runs`, `backtest_trades`, `trade_journal`, `probability_models`, `datasets`,
`audit_log` (ajout seul, chaîné SHA-256), `kv_state` (comptabilité, arrêt d'urgence, rapport
Shadow, calendrier, stops initiaux), `permission_grants` et `permission_events` (Autonomy
Commander, 0.3), `regime_history` (0.3). Chaque migration 0.3 a son script de retour arrière
(`crates/aegis-data/migrations_down/`).
