# Rapport de tests — AEGIS 0.3.0

Date : 2026-11-26. Machine de développement : macOS 27 (Darwin 27.2), Apple Silicon (arm64),
Rust 1.100, Node 22. Seuls les résultats **observés** figurent ici ; ce qui n'a pas pu être
exécuté est marqué **NON VÉRIFIÉ** avec la raison. Historique : 223 tests en 0.2.0, 237 avec le
cerveau micro-profits (2026-11-21), **306** en 0.3.0.

## 1. Tests automatisés

Commande : `./scripts/test.sh` (équivalent `cargo test --workspace --all-features`).
Résultat : **306 réussis, 0 échoué, 2 ignorés volontairement** (test réseau du calendrier et
test de capacité, lancés à part : § 3). `cargo clippy --workspace --all-targets` : aucune
erreur, avertissements de style uniquement.

| Module | Tests | Ce qui est couvert |
|---|---|---|
| `aegis-types` | 6 | types, arrondis, volumes, spécifications |
| `aegis-data` | 2 + 14 | migrations, audit chaîné et falsification, journal des transactions, versions de modèles et retour arrière, jeux de données, sauvegarde/restauration, rétention |
| `aegis-autonomy` | 7 | **0.3** — capacités strictement croissantes par profil ; sans autorisation rien ne trade ; DEMO liée à un compte démo et expirante ; LIVE exige configuration, compte réel, phrase, durée et la même configuration de risque (empreinte) ; seule la voie utilisateur confirme ; révocation immédiate ; seule une autorisation DEMO persistante survit au redémarrage |
| `aegis-risk` | 42 + 30 | objectif quotidien et mensuel, comptabilité du jour, Position Intelligence (SECURE, suiveur, clôture partielle unique, cible rapprochée, sortie au temps, jamais d'élargissement) ; **0.3** : réglages du gardien qui ne peuvent que réduire, **garde de protection** (stop manquant remis puis position clôturée si refus, modification vérifiée dans l'instantané, prix au-delà du stop prévu), **stop initial** structure / volatilité / bornes broker et spread, **re-entry et couverture** (moyenne à la baisse refusée, nouvelle bougie et nouvelle structure exigées, couverture unique jamais plus grosse que la position), corrélation des rendements, série de pertes qui ne peut que réduire la taille, stop plus large ⇒ volume plus petit, coûts résolus pour chaque cible nette |
| `aegis-protocol` | 4 + 5 + 12 | trames, HMAC (vecteur RFC 4231), poignée de main, anti-rejeu, compatibilité MQL5 ; **0.3** : champs du protocole 1.2 et connecteurs plus anciens acceptés |
| `aegis-market` | 31 | bougies, indicateurs, régimes, qualité ; **0.3** : régimes détaillés, **multi-unités** (un contexte opposé l'emporte sur une majorité, unité absente jamais comptée comme accord), **structure** (swings confirmés, BOS / CHoCH, cassure et fausse cassure, retest, compression / expansion / déplacement / déséquilibre, divergences, sommets égaux), **scanner** (score expliqué et borné, distinct de la probabilité), **audit d'historique** (trous, doublons, aberrations, week-ends) |
| `aegis-strategy` | 6 + 14 | 9 stratégies, absence de regard vers le futur, validation des paramètres |
| `aegis-backtest` | 7 + 12 | coûts, reproductibilité, segments, walk-forward, CSV ; **0.3** : **recherche de paramètres** évaluée en parallèle, classée en apprentissage, rapportée hors échantillon, requêtes bornées |
| `aegis-learning` | 12 | variables, inférence, portail, dérive, parité Python/Rust |
| `aegis-brain` | 13 | choix de cible dans la fourchette nette, frais, calibration, sessions, versions proposées seulement si meilleures, cible de l'utilisateur en pips respectée |
| `aegis-probability` | 10 | triple barrière, IRLS, Platt, Brier/log loss/ECE, absence d'apport sur marché aléatoire, reproductibilité |
| `aegis-news` | 5 (+1 réseau) | calendrier, fuseaux, fenêtres, calendrier indisponible ≠ calme |
| `aegis-shadow` | 5 | champion/challengers, signaux refusés suivis, évaluation historique, tâche indépendante |
| `aegis-execution` | 10 | cycle de vie, idempotence, délais, état inconnu, réconciliation |
| `aegis-simterminal` | 7 | broker simulé (dont mode netting et retrait de stop pour les tests de protection) |
| `aegis-client` | 1 | refus des adresses non sûres, épinglage obligatoire |
| `aegis-core` | 13 + 2 + 29 (+1 capacité) | configuration (l'exemple livré se charge et se valide), qualité d'exécution, dérive, performance, classes d'actifs ; **0.3** : alertes **Telegram** (identifiant de chat, message borné en texte brut) ; **architecture** : seule la voie utilisateur confirme une autorisation, Shadow et stratégies sans accès à l'exécution ni aux permissions ; **bout en bout** avec le vrai protocole TCP et le simulateur : cycle complet, MONITOR, rejets et partiels, acquittement et requête perdus, déconnexion, arrêt d'urgence et redémarrage, drawdown, LIVE désactivé, panne de base, actualités, Shadow, cerveau ; **0.3** : instrument suspendu, **compte netting** reconnu sans empilement, **position qui perd son stop chez le broker protégée à nouveau**, stop adapté sans probabilité empruntée, **autorisations** liées, auditées, révocables, cibles nettes et règles de récupération étudiées à chaque décision, **recherche de paramètres** qui propose seulement + audit de données, **scanner** multi-unités, fonctionnement sans surveillance (heartbeat, sauvegardes, alertes, **notification de test** avec Telegram configuré sans jeton) |
| `aegisd` | 5 + 1 + 1 | chemins confinés, unité systemd, certificat ; API : jeton obligatoire ; API sur vrai TLS avec le client épinglé |
| `desktop/src-tauri` | `cargo check --locked` sans erreur | — |
| `desktop` (TypeScript) | `npm run build` (`tsc` + Vite) sans erreur | — |

## 2. Compilation

### Locale (macOS arm64)

| Cible | Résultat |
|---|---|
| Moteur (`cargo build --release -p aegisd`) | **Vérifié** — 0.3.0, lancé avec le simulateur (§ 4) |
| AEGIS Desktop (`npx tauri build --bundles app`) | **Vérifié** — `AEGIS.app` 0.3.0, 23 Mo, moteur embarqué identique au binaire ci-dessus et démarré en MONITOR ; fenêtre non ouverte pendant cet essai |

### CI GitHub (runners natifs, `.github/workflows/build.yml`)

Exécution du **2026-11-21**, commit `8a96ddb` (version 0.2 + cerveau, connecteur 1.10),
résultats lus sur l'API publique de GitHub :

| Job | Résultat observé |
|---|---|
| `test (ubuntu-22.04)` | réussi |
| `test (windows-latest)` | réussi (échouait aux deux exécutions précédentes ; corrigé par `8a96ddb`, « simulateur plus lent sous Windows ») |
| `test (macos-14)` | **annulé au bout d'une seconde** : GitHub refuse ce runner pour ce compte (« The configuration 'macos-14-arm64-us-xl' is not supported »). Les tests macOS sont faits localement (§ 1) |
| `server` Linux x64 et Windows x64 | réussis (compilation, test de fumée simulateur + API TLS, archive) |
| `desktop` Linux et Windows | réussis (installateurs, installation et désinstallation automatiques) |
| `mql5` | **réussi : connecteur 1.10 et `AegisSelfTest` compilés par MetaEditor, 0 erreur** |

Exécution 0.3 : § 6.

## 3. Capacité (2 000 transactions par jour)

Mesure de la 0.2.0 (`cargo test --release -p aegis-core --test capacity -- --ignored
--nocapture`) : 2 000 entrées et 2 000 sorties par la vraie chaîne logicielle et le broker
**simulé** en 1,66 s ; aller-retour médian 0,76 ms, 99 % 2,47 ms ; décision de risque médiane
7 µs. Non remesuré en 0.3. **Non mesuré** : latence réseau vers MT5 et temps d'exécution du
broker (compte démo réel nécessaire).

## 4. Essais manuels réalisés en 0.3

| Essai | Résultat observé |
|---|---|
| Moteur `aegisd run --simulator` isolé (données temporaires, ports 17878/17879) | démarrage en MONITOR ; 4 000 bougies synthétiques M15 ; backtest `ema_trend` (36 transactions, net +224,35, données SYNTHÉTIQUES) ; audit de données (0 manquante, 0 doublon, 1 aberrante — « good ») ; recherche de paramètres (4 combinaisons sur 4 fils en 11 ms, toutes signalées sur-apprises : aucune recommandation) ; modèle de probabilité (Brier 0,218 / 0,214) ; journal d'audit intègre ; profil RESEARCH AUTONOMY puis DEMO FULL AUTONOMY accordée sur le compte simulé 900001 |
| Même essai, environ 10 min en DEMO AUTO | **aucune position ouverte** : signaux refusés (« aucune probabilité calibrée disponible ; espérance nette non calculable ») — comportement attendu, le modèle étant sur M15 et les signaux sur M1/M5 |
| Interface | 26 sections du moteur capturées ; aperçu (`VITE_AEGIS_PREVIEW=1`) compilé hors du dépôt ; **25 pages** capturées dans Chrome sans fenêtre (1440 × 900) : aucune erreur de console après correction de deux pages qui plantaient (Backtesting, Fast Learning : le résumé n'a pas `meta` dans `strategies`). Pages de positions, ordres, couvertures et latences vues **vides** seulement |

## 5. Ce qui reste à vérifier (avec la procédure)

1. **Connecteur sur compte démo** : `docs/MT5_CONNECTOR.md`, § « Vérification sur compte
   démo » — exécuter `AegisSelfTest`, puis MONITOR, puis DEMO FULL AUTONOMY.
2. **VPS** : installation du service, redémarrage après arrêt forcé, appairage distant
   (`docs/VPS_DEPLOYMENT.md`).
3. **Données réelles** : collecter l'historique broker, entraîner, backtester, lancer la
   recherche de paramètres — aucune stratégie ni aucun modèle n'a été évalué sur un marché réel.
4. **Alertes externes** : configurer un bot Telegram (ou un webhook), puis page
   *Notifications* → « Envoyer une notification de test ».

## 6. CI de la 0.3

En attente de l'exécution déclenchée par l'envoi sur GitHub.
