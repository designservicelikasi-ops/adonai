# AEGIS — État du projet

Ce fichier permet de reprendre le développement sans perdre le contexte.
Dernière mise à jour : 2026-11-15.

## Où en est-on

| Phase | Contenu | État |
|---|---|---|
| 0 | Environnement, workspace Rust, Git | Fait |
| 1 | Core + Data (SQLite, migrations, audit chaîné, sauvegarde/restauration) | Fait, testé |
| 2 | Moteur de risque (taille, coûts, limites, autorisations, arrêt d'urgence) | Fait, testé |
| 3 | Protocole authentifié (HMAC, session, anti-rejeu) | Fait, testé |
| 4 | Connecteur MT5 (Expert Advisor MQL5) | À faire — compilation MetaEditor impossible sur cette machine |
| 5 | Moteur de marché (bougies, indicateurs, régimes, qualité) | Fait, testé |
| 6 | Stratégies + backtesting | Fait, testé |
| 7 | IA : côté Rust (variables, inférence, portail de validation, dérive) | Fait, testé — entraînement Python à faire |
| 8 | Exécution + micro-profits + réconciliation | Fait, testé de bout en bout avec le simulateur |
| 9 | Application desktop (egui) | À faire |
| 10 | Intégration globale | En cours (13 tests de bout en bout réussis) |
| 11 | Compilation, documentation, archive | À faire |

## Comment reprendre

```sh
. "$HOME/.cargo/env"
cd aegis
./scripts/t.sh test --workspace     # tous les tests, sortie compacte
```

Ordre des tâches restantes : démon + CLI → application desktop → connecteur MQL5 →
recherche Python → documentation → archive ZIP.

## Décisions à ne pas perdre

Voir `DECISIONS.md`. Les plus structurantes :

- Le moteur démarre **toujours** en MONITOR ; aucun mode automatique n'est restauré.
- En MONITOR, **aucune** trame d'ordre ne sort du moteur (y compris les clôtures manuelles).
- Le simulateur (`aegis-simterminal`) est un adaptateur de test : instrument `EURUSD.sim`,
  données stockées comme `synthetic`, jamais éligible au réel.
- Une probabilité de gain n'existe que si un backtest hors échantillon (≥ 30 transactions)
  est enregistré ; sinon l'espérance nette est « non calculable » et l'entrée est refusée.
