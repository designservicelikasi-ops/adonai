# Architecture d'AEGIS (0.2)

## Trois composants

```
 MetaTrader 5 (Windows)                AEGIS ENGINE — aegisd (Windows / Linux, VPS)              AEGIS DESKTOP (Tauri 2)
┌───────────────────────┐ TCP, HMAC ┌──────────────────────────────────────────────────────┐ HTTPS ┌─────────────────────┐
│ AegisConnector.mq5    │──────────▶│ bridge → engine (boucle d'événements unique)         │◀─────▶│ React + TypeScript  │
│ prix, compte,         │◀──────────│  market → strategies → probability → risk → exec     │épinglé│ côté Rust : client  │
│ positions, clôtures,  │ ordres    │  positions (Position Intelligence), goal, journal    │+jeton │ aegis-client        │
│ exécute les ordres    │ autorisés │  news (tâche), shadow (tâche indépendante)           │       │ (épinglage TLS)     │
│ authentifiés          │           │  data (SQLite, audit chaîné), ops (heartbeat,        │       │ moteur local        │
└───────────────────────┘           │  sauvegardes, alertes), API Axum + rustls, CPU/RAM   │       │ détaché (optionnel) │
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
| `aegis-market` | bougies à partir des ticks, indicateurs (EMA, RSI, ATR, Donchian, Bollinger, pivots), régimes, sessions, qualité des données |
| `aegis-strategy` | 9 stratégies : EMA crossover, RSI momentum, suivi de tendance, breakout, pullback, retour à la moyenne RSI, retour à la moyenne Bollinger, rejet S/R, expansion de volatilité ; cycle de vie |
| `aegis-probability` | **Probability Engine** : P(TP avant SL), triple barrière, régression logistique, walk-forward, calibration, Brier/log loss/fiabilité |
| `aegis-risk` | **Risk Engine** : taille, coûts, espérance nette, limites, seuils de probabilité, **Daily Goal Manager**, **Position Intelligence**, autorisations signées, arrêt d'urgence |
| `aegis-execution` | cycle de vie des ordres, idempotence, délais, état inconnu, réconciliation |
| `aegis-protocol` | protocole MT5 : trames, HMAC, poignée de main, session anti-rejeu |
| `aegis-backtest` | backtest avec coûts, segments, walk-forward, CSV, générateur synthétique |
| `aegis-learning` | variables explicatives (source unique), filtre logistique Python, dérive |
| `aegis-news` | **News Intelligence** : calendrier, recommandation autorisé / prudent / suspendu |
| `aegis-shadow` | **Shadow Intelligence** : champion/challenger papier, opportunités manquées, analyse des sorties ; **aucune dépendance** vers l'exécution |
| `aegis-core` | moteur : configuration, orchestration, décisions (`decide.rs`), positions, intelligence (`intel.rs`), performance, lien Shadow, ops, état publié |
| `aegis-simterminal` | **adaptateur de test** : terminal et broker simulés parlant le vrai protocole |
| `aegis-client` | client de l'API avec épinglage du certificat (utilisé par le desktop) |
| `aegisd` | binaire du moteur : service, API Axum + TLS, surveillance système, outils en ligne de commande |

Hors workspace : `desktop/` (Tauri 2 : `src/` React, `src-tauri/` Rust), `mt5/` (MQL5),
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
4. **Contrôles du moteur** : base disponible, stade de la stratégie, régime, qualité des
   données, ordre déjà en cours, filtre IA, **actualités**.
5. **Probabilité** P(TP avant SL) : la plus prudente des sources validées (modèle de scénario
   calibré, borne de Wilson du backtest hors échantillon). Sans source : refus.
6. **Risque** : taille (au risque, plafonnée), coûts (spread, commission, slippage, swap,
   conversion), espérance nette, seuil BUY/SELL, limites (jour, objectif, transactions, pertes
   consécutives, débit, horaires, exposition globale et par devise, coûts du jour).
7. **Décision écrite en base avant tout envoi** ; journal préparé.
8. **Autorisation consommée** (signée, 3 s, usage unique) → ordre persisté → trame.
9. **Connecteur** : revérifie, exécute, rapporte la réponse du broker.
10. **Exécution** : `filled` / `rejected` / partiel / `unknown` → réconciliation. Journal :
    prix exécuté, slippage mesuré.
11. **Position Intelligence** à chaque bougie : HOLD / SECURE / ADJUST SL / ADJUST TP / CLOSE.
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
Shadow, calendrier, stops initiaux).
