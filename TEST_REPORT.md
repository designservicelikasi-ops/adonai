# Rapport de tests

Exécution du 2026-11-15 sur macOS 27 (Apple Silicon M1, 8 Go), Rust 1.100.0, Python 3.12.15.

Commande : `cargo test --workspace --all-features` (ou `./scripts/test.sh`).

**Résultat : 159 tests exécutés, 159 réussis, 0 échoué, 0 ignoré.**
La suite de bout en bout a en plus été relancée 10 fois de suite sans échec.

Ces chiffres sont ceux réellement obtenus ; rien n'est simulé ni extrapolé.

## Par module

| Suite | Tests | Ce qui est vérifié |
|---|---|---|
| `aegis-types` (unitaires) | 6 | Arrondi des volumes au pas du broker (toujours vers le bas), valeur d'une distance de prix, validation des spécifications et des bougies |
| `aegis-data` `tests/store.rs` | 12 | Migrations, intégrité, **doublons refusés** (décision, ordre, autorisation), **clés étrangères**, lot de bougies invalide annulé en entier, cycle de vie d'un ordre tracé, synchronisation des positions, journal d'audit **ajout seul et chaîné** (falsification détectée), **état conservé après arrêt brutal**, sauvegarde/restauration, rétention |
| `aegis-risk` (unitaires) | 8 | Coûts recalculés à la main, petit gain brut absorbé par les frais, swap et conversion, comptabilité journalière (réalisé / flottant / dépôts), drawdown |
| `aegis-risk` `tests/risk_engine.rs` | 17 | **Tous les cas d'autorisation et de rejet** : taille depuis les spécifications broker (y compris devise de cotation ≠ compte), MONITOR, arrêt d'urgence, connexion, compte absent/périmé/incohérent, prix absent/périmé/futur/invalide, instrument, spread, niveaux SL/TP, espérance nette, perte journalière, objectif journalier, drawdown, nombre de positions, exposition, marge, volume jamais arrondi vers le haut, montant de risque externe ignoré, décision dupliquée, démo ≠ réel, LIVE exige armement + compte réel + MT5, autorisation à usage unique / expirée / falsifiée, configurations invalides refusées |
| `aegis-protocol` (unitaires `io`) | 4 | Découpage des trames, trame trop grande, flux tronqué, **lecture interrompue sans perte d'octets** |
| `aegis-protocol` `tests/protocol.rs` | 12 | Aller-retour de tous les messages, **trames non authentifiées ou altérées rejetées**, **rejeu / doublon / désordre rejetés**, trames d'une autre session, horodatage hors fenêtre, version, taille, contenu invalide, poignée de main (mauvaise clé, hello rejoué, welcome forgé), vecteur RFC 4231 |
| `aegis-protocol` `tests/mql5_compat.rs` | 4 | Les messages **tels que le connecteur MQL5 les fabrique** sont acceptés ; les instructions du moteur contiennent tous les champs que le connecteur lit ; valeurs de référence de `AegisSelfTest.mq5` |
| `aegis-market` (unitaires) | 17 | EMA/RSI/ATR contre des valeurs calculées à la main, agrégation tick → bougie, série ordonnée et sans doublon, bougies manquantes, agrégation multi-unités, régimes, sessions, **ticks invalides rejetés**, **prix obsolète**, **spread anormal**, **décalage d'horloge**, indicateurs indisponibles sans historique |
| `aegis-strategy` (unitaires + `tests/strategies.rs`) | 13 | Entrées/sorties des trois stratégies, silence sans historique, **décision indépendante des bougies futures**, configuration validée, promotion : un bon backtest seul n'atteint jamais le réel, borne de Wilson |
| `aegis-backtest` (unitaires + `tests/backtest.rs`) | 17 | Arithmétique exacte d'une transaction (entrée à l'ouverture suivante + spread, cible, stop), **stop supposé avant la cible** dans la même bougie, **gap au-delà du stop**, vente stoppée sur l'ask, commission/slippage/swap, sortie sur signal, compte trop petit, métriques recalculées à la main, **reproductibilité**, **absence de biais d'anticipation**, les coûts dégradent toujours le résultat, segments disjoints, walk-forward, CSV, générateur déterministe |
| `aegis-learning` (unitaires) | 12 | Variables finies et bornées, **indépendantes du futur**, libellés, inférence recalculée à la main, artefacts incohérents refusés, **portail recalculé depuis les métriques**, **modèle entraîné en Python rechargé avec la même inférence** (écart < 1e-12), PSI, prédictions notées seulement après leur horizon |
| `aegis-execution` `tests/execution.rs` | 10 | **Une autorisation = un seul ordre**, autorisation forgée/expirée/arrêt d'urgence ⇒ aucun ordre, statut dicté par le broker (exécuté, rejeté, partiel, en attente), succès sans ticket ≠ exécuté, silence ⇒ `unknown` puis réconciliation, redémarrage, trame non livrée, position ouverte **et** clôturée avant toute confirmation, commandes de gestion qui ne peuvent que réduire le risque |
| `aegis-simterminal` (unitaires) | 6 | Comptabilité du broker simulé, stops/cibles, **instruction dupliquée non réexécutée**, expiration, volume, écart de prix, injection de pannes, ordres en attente |
| `aegis-core` (unitaires) | 3 | Configuration par défaut sûre, fichier invalide refusé (faute de frappe incluse), fichier de clé créé en `0600` |
| `aegis-core` `tests/e2e.rs` | 14 | Voir « Bout en bout » ci-dessous |
| `aegisd` `tests/api.rs` | 1 | API HTTP : jeton obligatoire, requête non authentifiée sans effet, commandes relayées au moteur, **arrêt propre du moteur** alors que l'API en détient encore des références (base refermée, journal d'écriture vidé) |
| `aegis-control` `tests/ui.rs` | 3 | Interface ↔ moteur réel (voir ci-dessous) |

## Bout en bout (`crates/aegis-core/tests/e2e.rs`)

Le vrai moteur écoute sur un port TCP ; le **simulateur (adaptateur de test)** s'y connecte par
le vrai protocole. Ce n'est pas une connexion à un broker.

| Test | Scénario |
|---|---|
| `full_cycle_signal_risk_order_fill_close_reconcile` | Les 15 étapes de la spécification : démarrage, connexion, prix, analyse, signal, coûts, risque, autorisation, ordre exécuté, confirmation, position suivie, base à jour, clôture à la cible, réconciliation, résultat publié. Vérifie aussi que l'objectif journalier atteint arrête les entrées, que les bougies du simulateur sont stockées « synthetic », et que le journal d'audit est intact. |
| `monitor_mode_observes_but_never_trades` | En MONITOR le signal est évalué et expliqué, mais aucun ordre, position ni commande n'existe ; les actions manuelles sont refusées. |
| `research_stage_and_missing_evidence_block_execution` | Stratégie au stade Recherche ⇒ rejet. Sans preuve hors échantillon ⇒ promotion refusée et espérance « non calculable ». |
| `broker_rejection_and_partial_fill_are_reported_as_they_happened` | Rejet du broker et exécution partielle rapportés tels quels. |
| `lost_acknowledgement_is_unknown_until_reconciliation_finds_the_position` | Confirmation perdue ⇒ jamais « exécuté » par supposition ; la réconciliation retrouve la vérité. |
| `lost_request_ends_as_not_executed` | Requête perdue ⇒ `unknown` puis `not_executed`. |
| `unauthenticated_terminals_and_replayed_frames_are_rejected` | Mauvaise clé, données brutes, second terminal, trame rejouée ⇒ refus, session fermée, audit. |
| `disconnection_and_stale_data_are_detected_and_block_entries` | Prix figés signalés ; coupure réseau détectée ; reconnexion. |
| `kill_switch_blocks_entries_survives_restart_and_needs_manual_reset` | Arrêt d'urgence, persistance après redémarrage, phrase de réinitialisation, retour en MONITOR. |
| `drawdown_triggers_the_emergency_stop_automatically` | Drawdown maximal ⇒ arrêt d'urgence automatique. |
| `live_auto_is_off_by_default_and_cannot_be_enabled_casually` | LIVE AUTO refusé par défaut, et toujours refusé avec le simulateur. |
| `risk_configuration_is_validated_persisted_and_restored` | Limites invalides refusées ; limites valides conservées après redémarrage. |
| `backtests_are_reproducible_stored_and_feed_the_evidence` | Deux backtests identiques ⇒ résultats identiques ; export du jeu de données ; sauvegarde. |
| `database_failure_blocks_entries_and_is_reported` | Base verrouillée ⇒ entrées bloquées, signalé, reprise automatique. |

## Interface (`crates/aegis-control/tests/ui.rs`)

- Sans terminal : les 9 valeurs inconnues du tableau de bord affichent « Indisponible », jamais
  un nombre inventé ; les 18 pages se dessinent.
- Avec le moteur réel + simulateur, après une transaction complète, un backtest et un arrêt
  d'urgence : le tableau de bord affiche exactement les valeurs du moteur (solde, équité, P&L
  du jour, mode) ; les 18 pages se dessinent sur cet état.
- Le rendu est fait **sans fenêtre** (contexte egui hors écran). L'application a par ailleurs
  été lancée réellement : processus actif, moteur à l'écoute, fichiers créés. **L'aspect visuel
  n'a pas été inspecté** (capture d'écran refusée par le système).

## Chaîne IA (exécutée manuellement, résultat dans `models/`)

```
aegisd generate-synthetic --symbol EURUSD.sim --tf M15 --bars 30000 --seed 42
aegisd export-dataset --symbol EURUSD.sim --tf M15 --origin synthetic --horizon 4 --out dataset.csv
uv run train.py --dataset dataset.csv --out model.json
aegisd verify-model --file model.json
```

Résultat obtenu : 29 896 échantillons, 5 plis walk-forward, AUC hors échantillon 0,6016,
Brier 0,2424 (référence 0,2500), ECE 0,0071, **écart d'inférence Rust/Python 1,1e-16**,
**portail : NON PASSÉ** (données synthétiques). L'AUC de 0,60 reflète la dérive construite
dans le générateur synthétique ; elle ne dit rien d'un marché réel.

## Analyse statique

`cargo clippy --workspace --all-targets` : aucune erreur ; il reste une dizaine de remarques de
style (nombre d'arguments, types longs), sans incidence sur le comportement.

## Défauts trouvés par les tests pendant le développement (et corrigés)

1. **Réconciliation** : une position ouverte puis clôturée avant l'arrivée de toute
   confirmation était déclarée « non exécutée ». Les clôtures portent désormais l'identifiant
   de l'ordre d'origine, ce qui prouve l'exécution.
2. **Réseau** : la lecture d'une trame pouvait perdre des octets quand elle était interrompue
   par une autre tâche, coupant la session par intermittence. Le lecteur conserve maintenant
   la trame partielle.
3. **Publication** : la réponse à une commande pouvait arriver avant que l'état publié ne
   reflète son effet.
4. **Base de données** : une base verrouillée figeait le moteur. La sonde tourne désormais à
   part et les écritures non essentielles sont suspendues.
5. **Arrêt d'`aegisd`** : avec l'API HTTP activée, Ctrl-C quittait sans arrêter le moteur ni
   refermer la base (trouvé en essayant le binaire de livraison). L'arrêt passe désormais par
   une référence partagée, et la fermeture replie le journal d'écriture dans `aegis.db`.

## Binaires de livraison (essai manuel, macOS Apple Silicon)

`cargo build --release -p aegis-control -p aegisd` : réussi. Puis, sur ces binaires :

- `aegisd run --simulator` avec l'API activée : moteur en **MONITOR**, simulateur connecté
  (≈ 800 trames reçues en 8 s), 6 composants sur 8 à « ok » (Exécution et IA « inactive »,
  comme attendu en MONITOR sans modèle), `/api/v1/health` 200, `/api/v1/state` 401 sans
  jeton et 200 avec ; Ctrl-C : code de sortie 0, il ne reste que `aegis.db` ;
- `aegisd verify-audit` sur cette base : chaîne d'audit intacte, intégrité « ok » ;
- `aegis` (application de bureau) : le processus reste actif et le port du pont écoute. La
  fenêtre n'a pas été inspectée.

## NON VÉRIFIÉ

| Élément | Raison |
|---|---|
| Compilation du connecteur MQL5 | Pas de MetaEditor (Windows) sur la machine de développement |
| Connexion à un vrai compte MT5 de démonstration, exécution réelle chez un broker | Pas de terminal MT5 disponible |
| Compilation et exécution sous Windows et sous Linux | Machine de développement : macOS uniquement |
| Aspect visuel de l'application | Capture d'écran refusée par macOS dans cet environnement |
| LIVE AUTO sur un compte réel | Volontairement non testé |
| Tenue sur plusieurs jours, charge réelle de ticks | Non mesurée |

Critères de livraison de la spécification : voir `PROJECT_STATUS.md`.
