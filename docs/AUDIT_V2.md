# Audit du dépôt et plan v2 (phase 0, 2026-11-18)

## État constaté au départ

- Dépôt Git propre, branche `main`, 11 commits, pas de dépôt distant.
- Workspace Rust de 13 crates (~16 000 lignes), connecteur MQL5 (jamais compilé), recherche
  Python, application de bureau **egui**, démon `aegisd` avec une API HTTP minimale
  (`state`, `mode`, `kill`, `kill/reset`).
- Tests : 159 réussis après correction d'un défaut d'environnement (deux tests lisaient un
  fichier du dépôt à l'exécution ; sur macOS 27 la première lecture d'un binaire neuf échoue
  par intermittence avec « No such file ». Les fichiers sont désormais embarqués à la
  compilation).
- Outils : Rust 1.100, Node 22.23, npm 10.9 ; pas de Docker, pas de `gh`, pas de MetaTrader.
  Machine : macOS Apple Silicon, 8 Go de RAM.

## Écarts entre l'existant et la spécification v2

| Exigence v2 | Existant | Travail |
|---|---|---|
| Objectif quotidien en devise, brut ou net, restant, politique | objectif en % d'équité, net, arrêt seulement | **Daily Goal Manager** |
| Brut / net / commission / spread / slippage / swap séparés | net du jour + somme brute des clôtures | journal des transactions + ventilation des coûts |
| Limites : transactions max, pertes consécutives, cooldown, exposition par devise, horaires, débit d'ordres | positions max, exposition notionnelle globale | extension du moteur de risque |
| 8 stratégies | 3 (EMA, RSI réversion, Donchian) | 5 nouvelles (RSI momentum, suivi de tendance, pullback, rejet S/R, volatilité) |
| P(TP avant SL) calibrée, Brier, log loss, fiabilité, walk-forward, historisée | borne de Wilson d'un backtest hors échantillon | **Probability Engine** (modèle de scénario triple barrière) |
| Seuils BUY/SELL minimum | aucun | seuils dans la configuration de risque |
| BUY / SELL / WAIT par instrument, probabilités affichées | décisions seulement quand une stratégie se déclenche | **Entry Intelligence** |
| HOLD / SECURE / ADJUST SL / ADJUST TP / CLOSE | sortie de stratégie seulement | **Position Intelligence** |
| Performance court/moyen/long terme, par stratégie, instrument, heure, régime | métriques de backtest | module de performance |
| Actualités économiques | aucune | **News Intelligence** |
| Shadow, champion/challenger | cycle de vie des stratégies (recherche → démo → réel) | **Shadow Intelligence** |
| Versions de jeux de données, retour arrière de modèle | versions de modèles | registre des jeux de données, retour arrière |
| Desktop Tauri 2 + React + TS, distant (VPS) | egui dans le même processus que le moteur | **nouvelle application**, cliente de l'API |
| TLS | liaison authentifiée non chiffrée | TLS sur l'API (desktop ↔ VPS) avec certificat épinglé |
| Service Windows / systemd, CPU/RAM, sauvegardes planifiées, alertes | lancement manuel | mode service + supervision |
| Builds Windows/Linux, installateurs | workflow CI non exécuté (egui) | workflow Tauri + serveur + compilation MQL5 en CI |
| 16 documents nommés | 8 documents | documentation complète |
| `RELEASES/` | `dist/` (macOS) | dossier de livraison avec artefacts réels uniquement |

## Décisions de phase 1

- **D21 — Desktop Tauri 2 client de l'API.** L'application egui (`aegis-control`) est
  remplacée par `desktop/` (Tauri 2, React, TypeScript, Tailwind). Elle ne contient pas le
  moteur : elle se connecte à `aegisd` (local ou VPS). Fermer l'application n'arrête rien.
  L'installateur fournit aussi `aegisd` et peut le lancer comme processus détaché.
- **D22 — TLS sur l'API, certificat auto-signé épinglé.** `aegisd` génère son certificat au
  premier démarrage ; le desktop vérifie l'empreinte SHA-256 (pas d'autorité de certification
  à gérer sur un VPS). Les appels réseau partent du côté Rust de Tauri, pas du navigateur.
- **D23 — Liaison MT5 inchangée (HMAC sur TCP).** Elle reste sur la boucle locale du VPS.
  Le TLS de MQL5 (`SocketTlsHandshake`) n'est pas vérifiable sans terminal : non ajouté.
- **D24 — Probabilité de TP avant SL par modèle de scénario.** Étiquettes « triple barrière »
  calculées sur l'historique (TP, SL, durée max, ambiguïté intrabougie comptée comme perte),
  régression logistique en Rust, validation walk-forward, calibration, Brier/log loss/
  fiabilité comparés au taux de base. Un modèle qui ne bat pas le taux de base n'est pas
  utilisé. La probabilité retenue pour un signal est la plus prudente des sources validées.
- **D25 — Objectif quotidien en devise du compte.** Brut (somme des résultats des
  transactions clôturées, avant commission et swap) ou net (après). Politiques : arrêter les
  entrées, ou continuer en protégeant une part du gain. L'objectif ne modifie jamais la taille.
- **D26 — Shadow dans une tâche séparée sans accès à l'exécution.** Il reçoit des copies des
  bougies et des transactions ; son type ne donne accès à aucun canal d'ordre.

## Ordre de réalisation

A. Risque étendu, objectif quotidien, brut/net · B. stratégies et gestion des positions ·
C. moteur de probabilités et BUY/SELL/WAIT · D. journal et performance · E. actualités ·
F. shadow et apprentissage · G. API complète, TLS, service, supervision · H. desktop Tauri ·
I. intégration, test de charge · J. CI Windows/Linux, compilation MQL5 · K. documentation,
`RELEASES/`, rapport final.
