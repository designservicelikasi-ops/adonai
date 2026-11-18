# Manuel développeur

Pour comprendre l'architecture, lisez d'abord `ARCHITECTURE.md` (les modules et le chemin
d'une décision) et `DECISIONS.md` (pourquoi c'est fait ainsi). Ce manuel explique comment
travailler dans le code.

## 1. Outils

| Outil | Version utilisée | Rôle |
|---|---|---|
| Rust (stable) | 1.100.0 | Tout le moteur, côté natif de l'application |
| Node.js | 22 | Interface d'AEGIS Desktop (`desktop/`) |
| uv + Python | 3.12 | Entraînement des modèles (`research/`) |
| MetaEditor | — | Compilation du connecteur (`mt5/`), **non disponible** sur la machine de développement |

Sous Linux, AEGIS Desktop demande en plus :
`libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev` (voir `BUILD_GUIDE.md`).

## 2. Commandes

```sh
./scripts/test.sh                                   # tous les tests, sortie compacte
./scripts/t.sh test -p aegis-risk                   # un seul crate
./scripts/t.sh test -p aegis-core --test e2e        # la suite de bout en bout
./scripts/t.sh clippy --workspace --all-targets     # analyse statique
cargo build --release -p aegisd                     # moteur : target/release/aegisd
cargo run -p aegisd -- run --simulator              # moteur + simulateur de test
./scripts/desktop-dev.sh                            # AEGIS Desktop en développement
```

`scripts/t.sh` lance `cargo` et n'affiche que les erreurs de compilation, les tests échoués
et le total. Pour la sortie complète, utilisez `cargo` directement.

La première compilation de l'application prend plusieurs minutes (Tauri, WebKit). Sur une machine de
8 Go, évitez de lancer deux compilations en même temps.

## 3. Où est quoi

```
crates/
  aegis-types/        types partagés (instrument, bougie, compte, position, transaction, modes)
  aegis-data/         SQLite : lib.rs (Store), journal.rs (journal, modèles, jeux de données), migrations/*.sql
  aegis-risk/         engine.rs (contrôles), costs.rs, day.rs (journée brut/net), goal.rs (objectif),
                      position.rs (Position Intelligence), auth.rs, config.rs (bornes)
  aegis-protocol/     protocole MT5 : lib.rs (trames, session), handshake.rs, messages.rs, io.rs
  aegis-market/       candles.rs, indicators.rs, regime.rs, lib.rs (qualité des données)
  aegis-strategy/     lib.rs (trait, registre), strategies.rs, strategies2.rs, lifecycle.rs
  aegis-probability/  lib.rs (triple barrière, entraînement, sélection), logistic.rs, metrics.rs, linalg.rs
  aegis-backtest/     lib.rs (moteur, métriques, segments), csv.rs, synthetic.rs
  aegis-learning/     features.rs (source unique des variables), model.rs, monitor.rs
  aegis-news/         calendrier, recommandation
  aegis-shadow/       lib.rs (messages, rapport), lab.rs (papier, hypothèses, analyse)
  aegis-execution/    cycle de vie des ordres, réconciliation
  aegis-core/         engine.rs (boucle), decide.rs (chaîne de décision), positions.rs, intel.rs
                      (probabilités, BUY/SELL/WAIT, journal, actualités), performance.rs,
                      shadowlink.rs, ops.rs (heartbeat, sauvegardes, alertes), command.rs,
                      bridge.rs, config.rs, view.rs (état publié), logging.rs
  aegis-simterminal/  broker.rs, terminal.rs — ADAPTATEUR DE TEST
  aegis-client/       client de l'API avec épinglage TLS
  aegisd/             main.rs (ligne de commande), lib.rs (API), daemon.rs, tls.rs, monitor.rs, service.rs
desktop/              src/ (React : App.tsx, pages/, components/), src-tauri/ (Rust : lib.rs)
mt5/                  AegisConnector.mq5, AegisProtocol.mqh, AegisSelfTest.mq5
packaging/            installation des services (systemd, Windows)
research/             train.py, pyproject.toml, uv.lock
config/               aegis.example.toml
scripts/              installation, lancement, tests, sauvegarde, recherche
```

## 4. Règles à ne pas casser

Ces règles sont vérifiées par des tests. Si un changement en fait échouer un, c'est le
changement qui est faux, pas le test.

1. **Un seul point de sortie vers le terminal** : `Engine::send_order` (`aegis-core`). Il est
   fermé en MONITOR. N'ajoutez pas d'autre chemin.
2. **Pas d'entrée sans autorisation** : `Executor::submit_entry` exige l'autorisation du
   moteur de risque et la consomme. Le type `Authorization` ne peut pas être signé hors de
   `aegis-risk`.
3. **Écrire avant d'envoyer** : décision et ordre sont enregistrés en base avant tout envoi.
   Si l'écriture échoue, rien ne part.
4. **Échec fermé** : une donnée absente, périmée, incohérente ou non finie produit un refus,
   jamais une valeur par défaut.
5. **Jamais de nombre inventé** : une valeur inconnue s'affiche « Indisponible ». Une
   probabilité n'existe que si une preuve hors échantillon existe.
6. **Aucune automatisation restaurée** : le moteur démarre en MONITOR. Ne persistez pas le
   mode.
7. **L'origine des données suit les données** (`broker` / `synthetic`) : bougies, backtests,
   modèles. Rien de synthétique ne doit pouvoir atteindre un vrai broker.
8. **Une stratégie ne voit pas le futur** : elle reçoit des bougies clôturées, rien d'autre.
9. **Un ordre n'est exécuté que si le broker le dit** ou si la réconciliation trouve la
   position. Pas d'état « exécuté » par supposition.
10. **Le moteur ne doit jamais attendre la base** dans sa boucle au point de se figer : les
    écritures non essentielles sont sautées quand `db_ok` est faux, et la sonde tourne à part.

Dépendances entre crates : voir le tableau d'`ARCHITECTURE.md`. `aegis-execution` ne dépend
pas de `aegis-strategy`, `aegis-risk` ne dépend que de `aegis-types` : gardez-le ainsi.

## 5. Ajouter une stratégie

Dans `crates/aegis-strategy/src/strategies.rs` :

1. Créez la configuration, avec `#[serde(default, deny_unknown_fields)]` et un `Default`.
2. Créez la structure et son `new(cfg) -> Result<Self, String>` qui **valide** la
   configuration (périodes > 0, multiples > 0, etc.) et construit le `StrategyMeta` :
   identifiant unique, unités de temps, instruments (vide = tous), régimes autorisés,
   `min_bars`, et les quatre règles en clair (entrée, sortie, stop, cible).
3. Implémentez `Strategy::evaluate(bars, open)`. `bars` sont des bougies clôturées, de la
   plus ancienne à la plus récente. Renvoyez `StrategyAction::None` tant que l'historique est
   insuffisant. Une entrée renvoie un `SignalIntent` : sens, **distances** de stop et de
   cible (pas des prix), raison lisible, valeurs d'indicateurs.

Puis dans `crates/aegis-strategy/src/lib.rs` : ajoutez l'identifiant à `STRATEGY_IDS`, le cas
dans `build`, et l'export `pub use`.

Tests à écrire dans `crates/aegis-strategy/tests/strategies.rs` : une entrée et une sortie
attendues sur des bougies construites à la main ; silence sans historique ; décision
inchangée quand on modifie les bougies futures ; configuration invalide refusée.

La stratégie apparaît alors d'elle-même dans le moteur (enregistrée au stade **Recherche**),
dans le backtester et dans l'application. Elle ne tradera qu'après promotion manuelle (voir
`docs/BACKTESTING.md`, § 4).

## 6. Ajouter une table ou une colonne

1. Ajoutez un fichier `crates/aegis-data/migrations/000N_nom.sql`. **Ne modifiez jamais une
   migration existante** : SQLx vérifie leur empreinte et refuserait d'ouvrir une base déjà
   créée.
2. Restez portable : pas de type ni de fonction propres à SQLite.
3. Ajoutez les méthodes dans `Store` (`src/lib.rs`) et les structures dans `records.rs`.
4. Couvrez chaque requête par un test dans `tests/store.rs` : les requêtes ne sont pas
   vérifiées à la compilation (décision D2).

Les migrations s'appliquent à l'ouverture de la base.

## 7. Ajouter un message au protocole

1. Ajoutez la variante dans `Body` (`crates/aegis-protocol/src/messages.rs`), son nom dans
   `kind()` et ses contrôles dans `validate()`. L'objet JSON doit rester **plat**.
2. Si le message ne doit circuler que dans un sens, ajoutez-le au filtre de `bridge.rs`.
3. Écrivez la même chose dans `mt5/AegisConnector.mq5`.
4. Ajoutez le message à `tests/protocol.rs` (aller-retour) et à `tests/mql5_compat.rs`
   (la chaîne exacte que le code MQL5 fabrique).
5. Mettez à jour `docs/PROTOCOL.md`.

Un changement incompatible exige d'augmenter `PROTOCOL_VERSION` des deux côtés.

## 8. Ajouter un contrôle de risque

Dans `crates/aegis-risk/src/engine.rs`, chaque contrôle ajoute une ligne au résultat (nom,
réussi ou non, détail chiffré) : c'est ce que l'utilisateur lit dans « Signaux de trading ».
Ajoutez la limite à `RiskConfig` avec ses **bornes dures** dans `config.rs`, puis un test
d'acceptation **et** un test de rejet dans `tests/risk_engine.rs`. Documentez-le dans
`docs/RISK.md`.

## 9. Ajouter une page ou une valeur à l'interface

- L'interface ne lit que l'état publié (`EngineView`, `aegis-core/src/view.rs`) par l'API :
  `/api/v1/summary` (rafraîchi chaque seconde) et `/api/v1/view/<section>` (par page). Pour
  afficher une nouvelle valeur : l'ajouter à la vue, la remplir dans le moteur, l'ajouter à
  `SUMMARY_KEYS` (`aegisd/src/lib.rs`) si le tableau de bord en a besoin, puis l'afficher.
  Aucun calcul métier dans l'interface.
- Une valeur qui peut manquer est un `Option` et s'affiche « — » ou avec la raison.
- Une action est une `Command` (`aegis-core/src/command.rs`), sérialisée
  `{"type","args"}` et envoyée par `api.command` (`desktop/src/api.ts`) ; l'interface affiche
  la réponse du moteur.
- Une nouvelle page : un fichier dans `desktop/src/pages/`, une entrée dans `PAGES`
  (`App.tsx`). Vérification visuelle : construire avec `VITE_AEGIS_PREVIEW=1`, fournir
  `preview-fixtures.json` (réponses réelles capturées) et ouvrir `#page=<id>`.
- Tous les appels réseau passent par le côté Rust (`desktop/src-tauri/src/lib.rs`), qui
  épingle le certificat ; la politique de sécurité du contenu interdit tout autre accès.

## 10. Chaîne de recherche (IA)

```sh
./scripts/research.sh EURUSD M15 broker          # données reçues de MT5
./scripts/research.sh EURUSD.sim M15 synthetic   # pour essayer la chaîne
```

1. `aegisd export-dataset` : le moteur calcule les variables (`features.rs`) et les libellés,
   et écrit le CSV. **Python ne recalcule aucune variable** (décision D11).
2. `research/train.py` : régression logistique standardisée + calibration de Platt,
   validation walk-forward à fenêtre croissante (5 plis) avec un embargo égal à l'horizon ;
   la calibration se fait toujours sur des données postérieures à l'ajustement.
3. L'artefact JSON contient les coefficients, les métriques brutes par pli, l'origine des
   données et des vecteurs de test.
4. `aegisd verify-model` : le moteur recharge l'artefact, recalcule l'inférence sur les
   vecteurs de test (écart attendu < 1e-12) et **réévalue lui-même le portail** à partir des
   métriques (`[learning.gate]`). Rien dans le fichier ne peut « approuver » un modèle.

Pour ajouter une variable : `FEATURE_NAMES` et le calcul dans `features.rs`, la liste
`FEATURES` dans `train.py`, puis réentraînez. Un artefact dont la liste de variables ne
correspond pas est refusé au chargement.

## 11. Tests : conventions

- Les calculs sont comparés à des valeurs **recalculées à la main** dans le test, pas à la
  sortie du code testé.
- Chaque règle de sûreté a un test de rejet.
- Les tests de bout en bout (`aegis-core/tests/e2e.rs`) démarrent le vrai moteur sur un port
  libre, dans un dossier temporaire, et y connectent le simulateur par le vrai protocole. Ils
  attendent un état avec `wait_for` plutôt qu'un délai fixe.
- Un test instable est un défaut : la suite de bout en bout a été lancée 10 fois de suite
  sans échec avant livraison.
- Ne fabriquez jamais un résultat : `TEST_REPORT.md` ne contient que des chiffres obtenus.

## 12. Journaux et diagnostic

- Journaux structurés (JSON) : `data/logs/`, un fichier par jour. Niveau de détail : variable
  d'environnement `AEGIS_LOG` (par défaut `info,sqlx=warn`).
- Journal d'audit : table `audit_log`, ajout seul, chaîné par SHA-256. Vérification :
  `aegisd verify-audit` ou le bouton de la page « Surveillance du système ».
- État complet du moteur en JSON : `GET /api/v1/state` (API d'`aegisd`, jeton obligatoire).
- Configuration réellement appliquée : `aegisd print-config`.

## 13. Simulateur

`aegis-simterminal` est un **adaptateur de test**. Il se connecte au moteur comme le ferait
MT5, s'annonce `simulator`, ne traite que `EURUSD.sim` et accélère le temps de marché (15 s
par tick). Son broker simulé sait injecter des pannes (`Fault`) : rejet, confirmation perdue,
requête perdue, exécution partielle, succès sans ticket ; le terminal simulé sait aussi
suspendre ses prix et couper la connexion. Tout ce qui en sort est étiqueté
`synthetic`. Ne le présentez jamais comme une connexion broker, et ne relâchez pas les
contrôles qui lui interdisent LIVE AUTO.

## 14. Connecteur MQL5

Le code de `mt5/` n'a **jamais été compilé**. Première chose à faire sur une machine Windows
avec MetaTrader 5 :

1. copier les trois fichiers de `mt5/` dans un même dossier `MQL5/Experts/AEGIS/` du terminal
   (les deux `.mq5` incluent `AegisProtocol.mqh` par un chemin relatif : il doit rester à
   côté d'eux) ;
2. compiler dans MetaEditor et corriger les erreurs éventuelles ;
3. lancer le script `AegisSelfTest` : il doit afficher `ALL PASSED` (HMAC contre la RFC 4231,
   analyseur JSON, clé de session identique à celle du moteur) ;
4. tester sur un compte de **démonstration**, moteur en MONITOR d'abord ;
5. mettre à jour `TEST_REPORT.md` et `KNOWN_LIMITATIONS.md` avec ce qui a été réellement
   constaté.

Toute modification du format des messages côté MQL5 doit être répercutée dans
`crates/aegis-protocol/tests/mql5_compat.rs`.

## 15. Avant de livrer une modification

```sh
./scripts/test.sh
./scripts/t.sh clippy --workspace --all-targets
```

Puis : mettre à jour `CHANGELOG.md`, `PROJECT_STATUS.md`, et selon le cas `TEST_REPORT.md`,
`DECISIONS.md`, `KNOWN_LIMITATIONS.md`. Un commit par étape cohérente.
