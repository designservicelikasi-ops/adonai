# AEGIS — Adaptive Engine for Intelligent Strategies

Plateforme de trading automatisé et de gestion des risques pour MetaTrader 5.
Priorité : préserver le capital, expliquer chaque décision, ne jamais présenter une
simulation comme du réel.

> **AEGIS ne promet aucun gain.** Un backtest ou un modèle décrivent le passé. Le trading
> comporte un risque de perte. Validez toujours sur un compte de **démonstration** d'abord.

## Ce que fait le logiciel

1. Le **connecteur MT5** (`mt5/`) envoie au moteur les prix, le compte et les positions.
2. Le **moteur** (Rust) construit les bougies, calcule les indicateurs, détecte le régime de
   marché et fait tourner les stratégies.
3. Chaque signal est chiffré : coûts (spread, commission, swap, slippage, conversion),
   espérance nette, puis **moteur de risque** (taille de position, limites, arrêt d'urgence).
4. Seul un signal qui passe **tous** les contrôles reçoit une autorisation à usage unique ;
   l'ordre part alors vers MT5, et n'est considéré exécuté que sur confirmation du broker.
5. Tout est enregistré (base SQLite + journal d'audit chaîné) et visible dans
   l'**application de bureau** (18 pages).

Trois modes : **MONITOR** (aucun ordre, mode de démarrage systématique), **DEMO AUTO**
(compte de démonstration vérifié), **LIVE AUTO** (désactivé par défaut, armement manuel).

## Démarrage rapide (macOS / Linux)

```sh
./scripts/setup.sh        # installe Rust si besoin, compile, crée config/aegis.toml
./scripts/run.sh          # ouvre l'application (moteur en MONITOR)
```

Pour essayer sans MetaTrader : page **Connexion MT5 → Démarrer le simulateur**. Le
simulateur est un adaptateur de test (prix fictifs, instrument `EURUSD.sim`) ; l'application
l'indique partout en orange.

Pour connecter un vrai terminal MT5 : voir `docs/USER_MANUAL.md` (§ 3).

Windows : `scripts/setup.ps1` (**non vérifié**, voir plus bas).

## État de vérification

| Élément | État |
|---|---|
| Moteur, risque, protocole, backtest, exécution, IA (Rust) | **Vérifié** — 159+ tests automatisés, voir `TEST_REPORT.md` |
| Cycle complet signal → ordre → clôture | **Vérifié avec le simulateur** (adaptateur de test) |
| Application de bureau | **Compilée et lancée sur macOS (Apple Silicon)** ; rendu des 18 pages testé sans fenêtre ; aspect visuel non inspecté |
| Chaîne IA (export → entraînement Python → rechargement) | **Vérifiée** (parité Rust/Python : 1e-16) |
| Connecteur MQL5 | **NON VÉRIFIÉ** — écrit, jamais compilé (pas de MetaEditor sur la machine de développement) |
| Connexion à un vrai compte MT5 de démonstration | **NON VÉRIFIÉ** |
| Compilation Windows et Linux | **NON VÉRIFIÉ** |

Les limites connues sont listées sans détour dans `KNOWN_LIMITATIONS.md`.

## Documentation

| Fichier | Contenu |
|---|---|
| `docs/USER_MANUAL.md` | Utiliser l'application, connecter MT5, comprendre une décision |
| `docs/DEPLOYMENT.md` | Installation, moteur sur VPS Linux + MT5 sous Windows |
| `docs/BACKUP_RECOVERY.md` | Sauvegarde, restauration, reprise après incident |
| `docs/RISK.md` | Règles de risque, convention comptable, arrêt d'urgence |
| `docs/PROTOCOL.md` | Protocole moteur ↔ connecteur |
| `docs/BACKTESTING.md` | Hypothèses du backtester, séparation des données |
| `docs/DEVELOPER_MANUAL.md` | Structure du code, tests, ajout d'une stratégie |
| `ARCHITECTURE.md`, `DECISIONS.md` | Architecture et décisions techniques |
| `TEST_REPORT.md`, `KNOWN_LIMITATIONS.md` | Ce qui a été testé, ce qui ne l'a pas été |
| `PROJECT_STATUS.md`, `CHANGELOG.md` | Avancement et historique |
| `docs/SPECIFICATION.md` | La demande d'origine, telle quelle |
