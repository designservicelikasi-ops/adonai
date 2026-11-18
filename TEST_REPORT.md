# Rapport de tests — AEGIS 0.2.0

Date : 2026-11-18. Machine : macOS 27 (Darwin 27.2), Apple Silicon (arm64), 8 Go. Rust 1.100,
Node 22.23. Seuls les résultats **observés** figurent ici ; ce qui n'a pas pu être exécuté est
marqué **NON VÉRIFIÉ** avec la raison.

## 1. Tests automatisés

Commande : `./scripts/test.sh` (équivalent `cargo test --workspace --all-features`).
Résultat : **223 réussis, 0 échoué, 2 ignorés volontairement** (test réseau du calendrier,
test de capacité : lancés à part, voir § 3 et § 4).

| Module | Tests | Ce qui est couvert |
|---|---|---|
| `aegis-types` | 6 | types, arrondis, volumes, spécifications |
| `aegis-data` | 2 + 12 | migrations, audit chaîné et falsification, journal des transactions (idempotence, clôtures partielles), versions de modèles et retour arrière, jeux de données, sauvegarde/restauration, rétention |
| `aegis-risk` | 24 + 23 | objectif quotidien (brut/net, politiques), comptabilité du jour (brut, net, coûts, série de pertes), Position Intelligence (SECURE, trailing, TP, sortie, gel, distance broker, jamais d'élargissement), seuils BUY/SELL, transactions max, pause après pertes, débit d'ordres, horaires, exposition par devise, coûts vs gains, plafond de lots, autorisations, arrêt d'urgence |
| `aegis-protocol` | 4 + 12 + 4 | trames, HMAC, poignée de main, anti-rejeu, compatibilité MQL5 (protocole 1.0 et 1.1) |
| `aegis-market` | 18 | bougies, indicateurs (EMA, RSI, ATR, Bollinger, niveaux pivots), régimes, qualité |
| `aegis-strategy` | 6 + 14 | 9 stratégies (un scénario construit par stratégie), absence de regard vers le futur, validation des paramètres |
| `aegis-backtest` | 5 + 12 | coûts, reproductibilité, segments, walk-forward |
| `aegis-learning` | 12 | variables, inférence, portail, dérive, parité Python/Rust |
| `aegis-probability` | 10 | triple barrière (spread, pessimisme), IRLS (coefficients retrouvés), Platt, Brier/log loss/ECE, marché aléatoire ⇒ aucun apport des variables, momentum planté ⇒ détecté et calibré (ECE 0,012), reproductibilité |
| `aegis-news` | 5 (+1 réseau) | lecture du calendrier, fuseaux, entrées mal formées, fenêtres, calendrier indisponible ≠ calme |
| `aegis-shadow` | 5 | champion/challengers sur les mêmes bougies, signaux refusés suivis sans regard vers le futur, entrées/sorties prématurées, évaluation historique, restauration, tâche indépendante |
| `aegis-execution` | 10 | cycle de vie, idempotence, délais, état inconnu, réconciliation |
| `aegis-simterminal` | 6 | broker simulé |
| `aegis-client` | 1 | refus des adresses non sûres, épinglage obligatoire |
| `aegis-core` | 7 + 18 (+1 capacité) | configuration, performance (calendrier, ratios, dégradation) ; **bout en bout** avec le vrai protocole TCP et le simulateur : cycle complet signal → risque → ordre → exécution → clôture → réconciliation → journal → objectif → performance ; MONITOR n'exécute rien ; rejet et exécution partielle ; acquittement perdu ; requête perdue ; terminaux non authentifiés et rejeu ; déconnexion et données périmées ; arrêt d'urgence et redémarrage ; drawdown ; LIVE désactivé ; configuration persistée ; backtests ; panne de base ; modèles de probabilité (versions, restriction aux données synthétiques, retour arrière, redémarrage) ; **actualité à fort impact ⇒ entrée suspendue** ; **Shadow indépendant**, promotion refusée sans preuves, paramètres persistés ; heartbeat, sauvegardes automatiques avec rotation, alertes |
| `aegisd` | 5 + 1 + 1 | chemins confinés, unité systemd, certificat (création unique, droits 0600) ; API : jeton obligatoire ; **API sur vrai TLS** avec le client épinglé : empreinte, résumé, sections, commandes génériques, `shutdown` interdit, chemin `..` interdit, mauvais certificat refusé, limitation après 10 échecs |
| `desktop/src-tauri` | compile sans avertissement | — |
| `desktop` (TypeScript) | `tsc --noEmit` sans erreur | — |

## 2. Compilation

| Cible | Résultat | Preuve |
|---|---|---|
| Moteur macOS arm64 (`cargo build --release -p aegisd`) | **Vérifié** | binaire 13,5 Mo, lancé (§ 5) |
| AEGIS Desktop macOS arm64 (`npx tauri build --bundles app`) | **Vérifié** | `AEGIS.app` 21,5 Mo avec `aegisd` embarqué dans `Contents/MacOS/`, lancé |
| Moteur Windows x64, Linux x64 | **NON VÉRIFIÉ** | aucune machine Windows/Linux ; job CI `server` prêt, jamais exécuté (pas de dépôt distant) |
| Installateurs Windows (.exe NSIS, .msi), Linux (.deb, .rpm, AppImage) | **NON VÉRIFIÉ** | job CI `desktop` prêt, jamais exécuté |
| Service Windows (`windows-service`) | **NON VÉRIFIÉ** | code compilé seulement sous Windows |
| Connecteur MQL5 | **NON VÉRIFIÉ** | pas de MetaEditor sous macOS ; job CI `mql5` prêt |

## 3. Capacité (2 000 transactions par jour)

`cargo test --release -p aegis-core --test capacity -- --ignored --nocapture` : 2 000 entrées
et 2 000 sorties étalées sur une journée simulée, à travers la vraie chaîne logicielle
(évaluation du risque, décision enregistrée, autorisation signée à usage unique, ordre et
commande persistés en SQLite WAL `synchronous=FULL`, trames HMAC scellées et vérifiées des deux
côtés, exécution par le broker **simulé**, confirmation, comptabilité).

| Mesure | Valeur observée |
|---|---|
| Durée totale | 1,66 s |
| Allers-retours complets par seconde | 1 208 |
| Latence d'un aller-retour (entrée + sortie) | médiane 0,76 ms ; 99 % 2,47 ms ; max 4,76 ms |
| Décision de risque | médiane 7 µs |
| Ordres en base | 2 000 ; transactions clôturées 2 000 |
| Coût de 2 000 allers-retours à 0,01 lot | 300 USD (spread 160, commission 140) |

**Lecture.** 2 000 transactions par jour représentent une transaction toutes les 43 s : le
traitement propre d'AEGIS n'est pas une limite. **Non mesuré** : latence réseau vers MT5 et
temps d'exécution du broker (seul un compte démo réel peut les mesurer). La dernière ligne
illustre l'objectif « micro-gains » : un aller-retour à 0,01 lot coûte environ 0,15 USD ; un
gain brut de 0,05 à 0,15 USD sur ce volume est entièrement absorbé. C'est pourquoi chaque
entrée exige une espérance **nette** positive.

Pendant ce test, le moteur de risque a d'abord bloqué les entrées après 134 allers-retours
(perte journalière de 2 % atteinte par les seuls coûts) : comportement correct, la limite a été
élargie pour la mesure.

## 4. Essais manuels réalisés

| Essai | Résultat observé |
|---|---|
| `aegisd run --simulator` + API HTTPS | santé `ok`, résumé : mode MONITOR, terminal `simulator`, CPU/RAM mesurés ; empreinte de `aegisd pairing` **identique** à celle mesurée par `openssl s_client` ; arrêt propre par Ctrl-C |
| Calendrier économique réel (`cargo test -p aegis-news -- --ignored`) | 125 événements lus en HTTPS depuis la source par défaut |
| Entraînement P(TP avant SL) en release, 8 000 bougies M1 synthétiques | 1,4 s ; ACHAT et VENTE « modèle complet », Brier 0,216 / 0,215, ECE 0,004 / 0,009, 105 520 observations hors échantillon par sens |
| Moteur 10 min en DEMO AUTO sur le simulateur | décisions expliquées ; refus observés et justifiés : stade Recherche, exposition (lots trop gros sur M1 — d'où l'ajout de `max_lots_per_trade`), P(TP avant SL) 42,7 % < 75 %, espérance nette −21,73 USD après 21,47 USD de coûts |
| Interface | 10 pages rendues avec les **vraies réponses** du moteur ci-dessus (mode aperçu, Chrome sans tête) : tableau de bord, performance, positions, décisions, stratégies, probabilités (diagrammes de fiabilité), Shadow, actualités, configuration, système. La fenêtre native a été lancée mais n'a pas pu être photographiée (session macOS verrouillée) |

## 5. Ce qui reste à vérifier (avec la procédure)

1. **Pipeline CI** (Windows, Linux) : pousser le dépôt sur GitHub et lancer le workflow `build`
   — compilation, tests, installateurs, installation/désinstallation automatiques, compilation
   MQL5. Reporter les résultats ici.
2. **Connecteur sur compte démo** : `docs/MT5_CONNECTOR.md`, § « Vérification sur compte démo ».
3. **VPS** : installation du service, redémarrage après arrêt forcé, appairage distant
   (`docs/VPS_DEPLOYMENT.md`).
4. **Données réelles** : collecter l'historique broker, entraîner les modèles, backtester —
   aucune stratégie ni aucun modèle n'a encore été évalué sur un marché réel.
