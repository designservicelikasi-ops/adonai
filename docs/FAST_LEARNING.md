# Fast Learning & recherche quantitative (0.3)

La vitesse ne remplace jamais la validation. Ce qui existe et a été vérifié :

## Recherche de paramètres parallèle (`run_parameter_sweep`)

- Grille de paramètres (≤ `models.max_sweep_combinations`), exécutée sur
  `min(threads, models.research_threads, cœurs - 1)` fils : la machine n'est jamais saturée.
- Protocole : les 70 % les plus anciens servent à **classer** (walk-forward en plis : nombre de
  plis positifs et dispersion) ; les 30 % récents ne servent qu'à **rapporter** le comportement
  hors échantillon des choix faits. Ils n'influencent jamais le classement.
- Drapeaux : *sur-apprentissage* (rentable en apprentissage, perdant ou moins de la moitié de
  l'espérance en contrôle), *instable* (la moitié des voisins de grille perdent), *trop peu de
  transactions* (< 30).
- Une recherche **propose** : elle ne modifie jamais une stratégie. Appliquer un jeu passe par
  `set_strategy_config` (la stratégie revient alors en Recherche, D28) puis par la promotion.
- Mesuré (macOS arm64, debug) : 6 combinaisons x 4 plis sur 4 000 bougies, < 1 s.

## Audit des données (`audit_data`)

Doublons, désordre, bougies incohérentes, trous (hors fermeture hebdomadaire du FX),
valeurs aberrantes (> 8 x le vrai range médian) ; verdict good / usable / poor.

## Champion / Challenger et retour arrière automatique

- Probabilités : versions stockées, une active par instrument x unité ; le cerveau propose les
  nouvelles versions, l'utilisateur active (D33).
- **Dérive** : chaque modèle actif est jugé sur ses transactions réelles (au moins
  `models.drift_min_trades`). Si le Brier réel dépasse le Brier hors échantillon du modèle de
  plus de `drift_brier_tolerance`, le moteur revient à la version précédente — ou retire le
  modèle s'il n'y en a pas (plus aucune probabilité, donc plus d'entrée, jusqu'à un nouveau
  modèle validé). Alerte et audit `probability_model_rollback`.
- Stratégies : Shadow (champion/challenger papier, tâche séparée sans accès à l'exécution),
  promotion vers la démo seulement, par l'utilisateur, après backtest + hors échantillon + forward.

## Apprentissage incrémental

Après chaque transaction : journal complet (conditions, régime détaillé, stratégie,
probabilité brute et corrigée, SL, TP, spread, commission, slippage, durée, résultat net,
motif). Le cerveau en tire des leçons uniquement dans le sens de la prudence (D32) et réentraîne
périodiquement (RESEARCH AUTONOMY requis) ; le modèle de production n'est jamais remplacé
automatiquement.

## Non réalisé

Pas de GPU, pas de modèle non linéaire (D10/D24) ; pas d'entraînement déporté automatique sur
le VPS (le moteur du VPS fait ses propres recherches s'il en a les données).
