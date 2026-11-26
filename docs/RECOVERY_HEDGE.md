# Recovery & Hedge Intelligence, Micro-Profit 2.0 (0.3)

## Principe

Une perte précédente n'est **jamais** un signal d'entrée. Une seconde position n'est possible
que si une **nouvelle opportunité indépendante** existe (re-entry) ou qu'une **couverture**
est réellement nécessaire et permise — et elle est alors évaluée par le Risk Engine comme un
risque entièrement nouveau (son propre stop, sa taille, le risque du portefeuille). Les règles
ci-dessous ne peuvent que **refuser** (`crates/aegis-risk/src/recovery.rs`, tests inclus).

## Interdictions vérifiées à chaque entrée

| Contrôle | Refus si |
|---|---|
| `anti_moyenne` | une position AEGIS du même sens sur l'instrument est en perte (moyenne à la baisse, grille) |
| `anti_martingale` | re-entrée qui risque plus d'argent **ou** plus de volume que la transaction perdue |
| `reentry_nouveau_signal` | la bougie du signal a commencé avant la clôture en perte |
| `reentry_structure` | même sens, aucun swing confirmé depuis la perte (structure inchangée) |
| `reentry_max` | plus de `max_reentries_per_symbol_per_day` re-entrées sur l'instrument |
| `couverture_unique` | une couverture existe déjà pour la position (pas de grille de couvertures) |
| `couverture_volume` | couverture plus grosse que `max_hedge_ratio` x la position couverte (≤ 1) |

Plus, dans le Risk Engine : `mode_marge` (netting : aucune couverture possible, un ordre
opposé réduirait ou inverserait), `couverture` (hedging : seulement avec `allow_hedging`),
`risque_portefeuille`, et la réduction de taille après une série de pertes
(`loss_streak_risk_scale`, valeurs ≤ 1), la pause après pertes consécutives.

Une entrée est **re-entry** quand une transaction AEGIS perdante sur le même instrument a été
clôturée depuis moins de `recovery.reentry_window_minutes` ; **hedge** quand une position AEGIS
de sens opposé est ouverte. Le journal garde le type (`entry_kind`) et la position liée.

## Analyses (section `recovery` de l'état, page Recovery & Hedge)

Par type d'entrée : nombre, gagnantes, résultat net, frais. Pour chaque couverture : résultat
de la couverture, de la position couverte, résultat combiné, efficace ou non, frais. Pour
chaque re-entry : perte d'origine, résultat de la re-entry, résultat de la séquence. Stratégies
avec au moins 3 pertes d'affilée. En dessous de 30 transactions clôturées, l'état dit
explicitement que c'est trop peu pour conclure.

## Micro-Profit 2.0

Pour chaque décision, chaque objectif net de `[micro] targets` (0,05 … 0,50 par défaut) est
résolu : distance de TP qui paie exactement cet objectif **après** spread, commission,
slippage, swap et conversion ; probabilité calibrée de cette cible (modèle de scénario) ;
espérance nette ; probabilité de point mort. Non faisable si la cible est sous la distance
broker, sans probabilité calibrée, ou d'espérance insuffisante — avec la raison. Exemple
vérifié par les tests : 0,05 net sur 0,01 lot avec un stop de 200 points coûte 0,19 de frais
et demanderait ~98 % de réussite : refusé.

## Objectif quotidien

Gestionnaire existant (objectif brut ou net, jour ou mois, politiques arrêter / protéger /
suivre) complété par le budget de risque journalier, les jours autorisés et les plages
horaires. L'objectif ne modifie **jamais** la taille, les seuils ni la fréquence.
