# Validation des modèles

AEGIS utilise deux familles de modèles statistiques. Aucun ne peut créer une transaction ;
aucun n'est utilisé sans validation hors échantillon ; un modèle entraîné sur données
synthétiques n'est jamais utilisé avec un vrai broker.

## 1. Probability Engine — P(TP avant SL)

Crate `aegis-probability`. Ce n'est **pas** une probabilité de hausse : c'est la probabilité
qu'un scénario précis (sens, TP et SL exprimés en ATR) touche sa cible avant son stop.

**Données.** Bougies stockées (broker, ou synthétiques pour tester le logiciel). Chaque version
de jeu de données est identifiée par l'empreinte SHA-256 de ses bougies (table `datasets`).

**Étiquettes « triple barrière ».** Entrée à la clôture (un ACHAT paie le spread de la
bougie), puis `horizon_bars` bougies : TP d'abord = 1 ; SL d'abord = 0 ; **TP et SL dans la
même bougie = 0** (ordre inconnu, hypothèse pessimiste) ; ni l'un ni l'autre = 0.

**Variables.** Les 11 variables de marché de `aegis-learning` (même code à l'entraînement et
en direct) + le scénario (`tp_atr`, `sl_atr`, `ln(sl/tp)`).

**Modèles.** Régression logistique à pénalité ridge (IRLS) sur variables standardisées, un
modèle par sens. Un **modèle de référence** n'utilise que le scénario : c'est la probabilité
« de base » d'un tel scénario sur cet instrument.

**Validation walk-forward.** `folds` blocs chronologiques ; chaque bloc est prédit par un
modèle entraîné sur le passé seul, avec une **purge** de `horizon_bars` bougies entre
entraînement et test (les étiquettes regardent vers l'avenir). Les plis dont l'entraînement
représente moins de 30 % de l'historique ne sont pas évalués.

**Calibration.** Platt (`p = σ(a·logit + b)`) **croisée** : chaque pli est calibré avec les
prévisions des autres plis seulement. Le modèle final est entraîné sur tout l'historique et
calibré sur l'ensemble des prévisions hors échantillon.

**Mesures publiées.** Brier, log loss, ECE (10 classes), diagramme de fiabilité, Brier par
pli, taux observé et prévu, nombre d'observations — pour le modèle complet et la référence.

**Sélection, par sens.**

| Résultat | Condition |
|---|---|
| modèle complet | ≥ `min_samples` observations hors échantillon, Brier ≤ Brier de la référence − `min_brier_skill`, ECE ≤ `max_ece` |
| scénario seul | sinon, si la référence est calibrée (ECE ≤ `max_ece`) |
| aucun | sinon : pas de probabilité, donc aucune entrée par cette voie |

**Usage.** Pour un signal, la probabilité retenue est la **plus prudente** des sources
validées (modèle de scénario, borne basse de Wilson du backtest hors échantillon de la
stratégie). Elle doit dépasser le seuil BUY/SELL de l'utilisateur (jamais abaissé
automatiquement), puis l'espérance nette après coûts doit être positive et au-dessus du minimum.

**Versions et retour arrière.** Chaque entraînement crée une version (table
`probability_models`) qui devient active ; `activate_probability_model` réactive une version
précédente.

**Suivi réel.** Chaque transaction exécutée enregistre la probabilité utilisée (journal). La
page *Probabilités* compare ces probabilités aux résultats réels (Brier, ECE, fiabilité sur
90 jours). Une dérive se lit directement : points du diagramme loin de la diagonale.

## 2. Filtre logistique Python (hérité de la v1)

Recherche en Python (`research/train.py`), inférence en Rust avec parité vérifiée. Il estime
P(hausse sur N bougies) et ne peut **qu'opposer un veto** à un signal contraire, une fois
passé son portail (AUC, Brier, ECE, nombre d'échantillons, plis). Voir `BACKTESTING.md` et
`models/demo-synthetic-model.report.md`.

## 3. Stratégies (preuves)

Backtest avec coûts déclarés, découpage entraînement / validation / hors échantillon, walk-
forward en 5 plis, reproductible. Promotion en Démo : ≥ 30 transactions hors échantillon au net
positif sur données broker. Le Shadow ajoute le **forward** papier sur bougies réelles et la
comparaison champion / challenger ; voir `TRADING_MODES.md`.

## Ce qui n'a pas encore été fait

Aucun modèle n'a été entraîné ni validé sur des **données de marché réelles** : il faut d'abord
collecter l'historique par le connecteur (compte démo). Les seuls résultats existants
portent sur des données synthétiques ou construites (tests), qui démontrent que la chaîne
fonctionne — un marché aléatoire donne bien « aucun apport des variables de marché », un
momentum planté est bien détecté — et ne disent rien de la rentabilité.
