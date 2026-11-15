# Architecture d'AEGIS

## Vue d'ensemble

```
 MetaTrader 5 (Windows)                         Moteur AEGIS (Rust : macOS / Linux / Windows)
┌─────────────────────────┐   TCP, 1 trame/ligne   ┌───────────────────────────────────────────────┐
│ AegisConnector.mq5      │  HMAC-SHA256 + session │ bridge  → engine (boucle d'événements unique)  │
│  - prix, compte,        │ ─────────────────────▶ │   market → strategy → (learning) → risk        │
│    positions, clôtures  │ ◀───────────────────── │                         │                      │
│  - exécute les ordres   │   ordres autorisés     │                    execution ──▶ bridge        │
│    authentifiés         │                        │   data (SQLite, audit chaîné)                  │
└─────────────────────────┘                        │   view (état publié 4×/s)                      │
                                                   └───────────────┬───────────────────────────────┘
                                                                   │ même processus
                                              ┌────────────────────┴───────────┐
                                              │ aegis (application de bureau)  │   ou  aegisd (sans interface + API HTTP)
                                              └────────────────────────────────┘
```

## Les modules (un crate chacun, dans `crates/`)

| Crate | Rôle | Dépend de |
|---|---|---|
| `aegis-types` | Types partagés : instrument, tick, bougie, compte, position, modes | — |
| `aegis-data` | Stockage SQLite (SQLx), migrations, audit chaîné, sauvegarde/restauration, rétention | types |
| `aegis-risk` | **Moteur de risque** : taille, coûts, espérance nette (micro-profits), limites, autorisations, arrêt d'urgence | types |
| `aegis-protocol` | Protocole v1 : trames, HMAC, poignée de main, session anti-rejeu, lecture bornée | types |
| `aegis-market` | Bougies depuis les ticks, indicateurs, régimes, sessions, qualité des données | types |
| `aegis-strategy` | Cadre de stratégies + EMA, RSI, breakout ; cycle de vie (recherche → démo → réel) | types, market |
| `aegis-backtest` | Backtester, métriques, découpage chronologique, walk-forward, CSV, générateur synthétique | strategy |
| `aegis-learning` | Variables explicatives, inférence, portail de validation, dérive, suivi en direct | market |
| `aegis-execution` | Cycle de vie des ordres, idempotence, délais, réconciliation | data, risk, protocol |
| `aegis-core` | Moteur : configuration, orchestration, pont réseau, décisions, supervision, vue publiée | tous |
| `aegis-simterminal` | **Adaptateur de test** : terminal + broker simulés parlant le vrai protocole | protocol, backtest |
| `aegis-control` | Application de bureau (egui/eframe), binaire `aegis` | core, simterminal |
| `aegisd` | Moteur sans interface, API HTTP (Axum), outils en ligne de commande | core |

Hors Rust : `mt5/` (connecteur MQL5), `research/` (entraînement Python).

Il n'y a aucune dépendance circulaire : le graphe ci-dessus est un arbre orienté vers `aegis-core`.

## Le chemin d'une décision

1. **Tick** reçu du terminal → `MarketEngine::on_tick` valide le tick, met à jour les bougies.
2. Une **bougie se clôture** sur une unité de temps de signal → chaque stratégie active reçoit
   les 300 dernières bougies clôturées (jamais la bougie en cours, jamais le futur).
3. Une stratégie renvoie une **intention d'entrée** (sens, distance de stop, distance de cible, raison).
4. Le moteur ajoute ses contrôles : base de données disponible, stade de la stratégie compatible
   avec le mode, régime de marché, qualité des données, pas d'ordre déjà en cours, filtre IA.
5. La **probabilité de gain** vient du dernier backtest hors échantillon de cette
   stratégie/instrument/unité (borne basse de Wilson, 30 transactions minimum). Sans elle,
   l'espérance nette est « non calculable » et l'entrée est refusée.
6. `RiskEngine::evaluate` calcule la taille, les coûts, l'espérance nette et vérifie toutes les
   limites. Chaque contrôle est enregistré avec son détail.
7. La décision est **écrite en base avant tout envoi**. Si l'écriture échoue, rien ne part.
8. Si tout est passé : `Executor::submit_entry` **consomme** l'autorisation (usage unique,
   signée, 3 s de validité), enregistre l'ordre, et la trame part vers le terminal.
9. Le connecteur revérifie (authentification, expiration, doublon, volume, écart de prix) puis
   appelle le broker et **rapporte ce que le broker a répondu**.
10. Le moteur met à jour l'ordre. Sans réponse : état `unknown`, tranché par la réconciliation
    (position trouvée, clôture trouvée, ou absence prouvée).

## Principes de sûreté dans le code

- **Un seul point de sortie** vers le terminal (`Engine::send_order`), fermé en mode MONITOR.
- **Une entrée ne peut être construite qu'à partir d'une autorisation** du moteur de risque
  (`submit_entry` exige `&mut RiskEngine` et l'autorisation ; le type `Authorization` ne peut
  pas être signé hors du crate `aegis-risk`).
- **Échec fermé** : donnée manquante, périmée, incohérente ou non finie ⇒ refus.
- **Aucune automatisation restaurée** au redémarrage ; l'arrêt d'urgence, lui, est restauré.
- **Origine des données** (`broker` / `synthetic`) portée par chaque bougie, backtest et modèle.

## Persistance

SQLite en mode WAL, `synchronous=FULL`, clés étrangères actives. Tables : `symbols`, `candles`,
`account_snapshots`, `strategies`, `decisions`, `orders`, `order_events`, `commands`,
`positions`, `deals`, `risk_configs`, `model_versions`, `backtest_runs`, `backtest_trades`,
`audit_log` (ajout seul, chaîné par SHA-256), `kv_state`. Le SQL évite les types propres à
SQLite pour rester portable vers PostgreSQL.

## Concurrence

Le moteur est une **boucle d'événements unique** (Tokio) : messages du terminal, commandes de
l'utilisateur, minuterie. L'état n'est donc jamais modifié par deux tâches à la fois. Les
calculs longs (backtests) et la sonde de base de données tournent à part et reviennent sous
forme de messages. L'interface lit un instantané immuable (`EngineView`) publié quatre fois
par seconde et après chaque commande.
