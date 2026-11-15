# Backtesting

> **Un backtest décrit le passé sous des hypothèses données. Ce n'est ni une prévision ni une
> garantie.** AEGIS le rappelle à chaque résultat. Les résultats livrés avec le logiciel
> reposent sur des données **synthétiques** : ils montrent que le moteur fonctionne, ils ne
> disent rien d'un marché réel.

Code : `crates/aegis-backtest/`. Tests : 17 (voir `TEST_REPORT.md`).

## 1. Ce que fait le backtester

Il rejoue une stratégie, bougie après bougie, sur un instrument et une unité de temps. La
stratégie est **le même code** que celui du moteur en direct : elle reçoit les 300 dernières
bougies clôturées et rien d'autre.

Il produit :

- la liste des transactions (entrée, sortie, volume, stop, cible, résultat brut, commission,
  swap, résultat net, motif de sortie, durée, raison du signal) ;
- la courbe d'équité (une valeur après chaque transaction clôturée) ;
- les métriques du § 5.

Deux exécutions avec les mêmes données et les mêmes paramètres donnent **exactement** le même
résultat (testé).

## 2. Hypothèses d'exécution

| Sujet | Hypothèse |
|---|---|
| Moment de l'entrée | Un signal calculé à la clôture de la bougie `i` est exécuté à l'**ouverture de la bougie `i+1`**. Jamais au prix qui a produit le signal. |
| Prix | Les bougies sont en prix *bid*. Un achat s'exécute à `bid + spread`, une vente à `bid` ; les sorties, à l'inverse. |
| Spread | Celui enregistré dans chaque bougie, ou une valeur fixe si elle est fournie. |
| Slippage | La moitié du slippage configuré est payée à l'entrée, l'autre moitié à la sortie (au stop et aux sorties au marché ; pas à la cible, qui est un ordre à cours limité). |
| Stop et cible dans la même bougie | Si les deux ont pu être touchés, **le stop est supposé touché en premier** (hypothèse pessimiste). |
| Gap | Une bougie qui ouvre au-delà du stop est exécutée à **son ouverture**, pas au niveau du stop : la perte dépasse alors le risque prévu. |
| Vente | Le stop et la cible d'une vente sont comparés au prix *ask* (`bid + spread`). |
| Commission | Par lot, aller-retour, retirée à la clôture. |
| Swap | Swap du broker par lot × nombre de nuits UTC traversées (désactivable). |
| Taille de position | Même règle que le moteur en direct : `équité × risque % ÷ perte par lot au stop` (commission et demi-slippage inclus), **arrondie vers le bas** au pas de volume. |
| Volume insuffisant | Si le budget de risque n'achète pas le volume minimal, le signal est **ignoré** et compté dans `skipped_signals`. |
| Positions | Une seule position à la fois par exécution. |
| Fin des données | Une position encore ouverte est clôturée à la dernière clôture (motif `end_of_data`). |

Paramètres par défaut : équité initiale 10 000, risque 0,5 % par transaction, commission 7
par lot aller-retour, slippage 2 points, swaps appliqués, spread des bougies.

### Ce que le backtester ne modélise pas

- Le chemin du prix à l'intérieur d'une bougie (d'où l'hypothèse pessimiste ci-dessus).
- La liquidité, les exécutions partielles, les rejets et les requotes du broker.
- L'élargissement du spread pendant les annonces, au-delà de ce que les bougies ont
  enregistré.
- Les frais de conversion de devise (le moteur de risque en direct, lui, les compte).
- Plusieurs positions simultanées ou plusieurs instruments dans une même exécution : chaque
  instrument et chaque unité de temps se teste séparément.

Un résultat de backtest est donc, au mieux, une borne optimiste sur ces points.

## 3. Absence de biais d'anticipation

- La stratégie ne voit que `bougies[..=i]` : la bougie qui vient de clôturer et les
  précédentes.
- La décision prise sur la bougie `i` ne s'applique qu'à l'ouverture de `i+1`.
- Un test vérifie que modifier les bougies **futures** ne change aucune décision passée
  (`aegis-backtest` et `aegis-strategy`).
- Les variables de l'IA sont soumises au même test (`aegis-learning`).

## 4. Séparation des données

Chaque backtest lancé depuis l'application ou `aegisd` calcule trois choses sur les mêmes
bougies :

1. **Complet** : toute la période.
2. **Trois segments chronologiques disjoints** : entraînement (60 % les plus anciens),
   validation (20 %), **hors échantillon** (20 % les plus récents).
3. **Walk-forward** : 5 fenêtres consécutives qui ne se chevauchent pas.

Les paramètres des stratégies livrées sont fixes : AEGIS ne les optimise pas sur le segment
d'entraînement. Les segments servent à vérifier qu'un résultat tient sur des périodes
différentes, en particulier la plus récente.

**Quatrième étage : les tests prospectifs.** Ce sont les transactions réellement exécutées
en DEMO AUTO chez le broker. Elles sont comptées à part (`forward_trades`) et ne se mélangent
jamais aux backtests.

### À quoi sert le segment hors échantillon

C'est la seule source de la **probabilité de gain** utilisée par le moteur de risque :

- au moins **30 transactions** hors échantillon, sinon l'espérance nette est « non
  calculable » et l'entrée est refusée ;
- la probabilité retenue est la **borne basse de Wilson** du taux de réussite (z = 1), plus
  prudente que le taux observé quand l'échantillon est petit.

### Promotion d'une stratégie

| Passage | Conditions |
|---|---|
| Recherche → Démo | ≥ 30 transactions hors échantillon, résultat net positif, **données broker** (pas synthétiques). |
| Démo → Approuvée réel | ≥ 50 transactions réellement exécutées en démo, résultat net positif. Impossible avec le simulateur. |
| Recherche → Approuvée réel | **Impossible** : le stade Démo est obligatoire. |

La promotion est toujours une action manuelle. Un bon backtest ne promeut jamais rien de
lui-même. Avec le simulateur, la règle « données broker » est levée pour pouvoir tester le
logiciel ; rien de ce qui en sort n'est utilisable avec un vrai broker.

## 5. Métriques

| Métrique | Définition |
|---|---|
| Profit net | Somme des résultats nets (après commission et swap). |
| Profit brut / perte brute | Somme des transactions gagnantes / perdantes (en net). |
| Taux de réussite | Transactions gagnantes ÷ transactions. Une transaction à résultat nul compte comme perdante. |
| Profit factor | Profit brut ÷ perte brute. **Indisponible** s'il n'y a aucune perte (pas de division par zéro, pas de valeur inventée). |
| Drawdown maximal | Plus forte baisse de l'équité depuis un plus haut, en montant et en %. Mesuré sur l'équité **après clôture** des transactions : les creux en cours de position ne sont pas vus. |
| Gain moyen par transaction | Profit net ÷ transactions. |
| Espérance | Taux de réussite × gain moyen − taux d'échec × perte moyenne. |
| Durée moyenne | En bougies et en temps. |
| Commissions, swaps | Totaux payés. |
| Signaux ignorés | Entrées abandonnées faute de volume minimal. |

## 6. Données

Chaque bougie stockée porte son **origine** : `broker` ou `synthetic`. Un backtest porte
l'origine de ses données, et cette origine le suit partout (base, interface, promotion).

| Source | Comment | Origine |
|---|---|---|
| Terminal MT5 | Reçues par le connecteur (historique demandé à la connexion, puis bougies clôturées en direct). | `broker` |
| Fichier CSV | `aegisd import-csv --file F.csv --symbol EURUSD --tf M15 --utc-offset 2` | `broker` |
| Générateur | `aegisd generate-synthetic --symbol EURUSD.sim --tf M15 --bars 20000 --seed 42` | `synthetic` |
| Simulateur | Bougies formées pendant qu'il tourne. | `synthetic` |

Formats CSV acceptés :

- AEGIS : `open_ts_ms,open,high,low,close,tick_volume,spread_points` (en-tête facultatif) ;
- export d'historique MetaTrader 5 (tabulations) :
  `<DATE> <TIME> <OPEN> <HIGH> <LOW> <CLOSE> <TICKVOL> <VOL> <SPREAD>`.

Les exports MT5 sont à l'heure du serveur du broker : indiquez son décalage avec
`--utc-offset` pour que les bougies soient stockées en UTC. Chaque ligne est validée ; la
première ligne invalide annule tout l'import.

Le générateur synthétique est un pas aléatoire à régimes (tendance, range, forte volatilité),
déterministe pour une graine donnée. Il n'accepte que des instruments en `.sim`.

Un backtest exige au moins **500 bougies** et en charge 200 000 au plus.

## 7. Lancer un backtest

Dans l'application : page **Backtesting** (stratégie, instrument, unité de temps, origine des
données, capital initial), puis **Lancer le backtest**. Le calcul tourne à part ; le moteur continue de
fonctionner.

En ligne de commande :

```sh
target/release/aegisd backtest --strategy ema_trend --symbol EURUSD --tf M15 --origin broker
```

Stratégies livrées : `ema_trend` (croisement d'EMA 20/50), `rsi_reversion` (retour du RSI 14
depuis 30/70), `donchian_breakout` (sortie du canal de 20 bougies). Stops et cibles sont des
multiples de l'ATR 14.

Chaque exécution est enregistrée (`backtest_runs`, `backtest_trades`) avec ses paramètres, la
configuration de la stratégie, la période couverte et l'origine des données.

## 8. Lire un résultat avec prudence

- Regardez d'abord le segment **hors échantillon** et les fenêtres walk-forward, pas le
  résultat complet.
- Un bon résultat sur un seul segment, ou porté par deux ou trois transactions, n'est pas
  une preuve.
- Moins de 30 transactions : aucun chiffre n'est exploitable.
- Vérifiez que la commission saisie est celle de votre broker : MT5 ne la fournit pas.
- Un résultat sur données `synthetic` ne vaut que comme test du logiciel.
