# Market Intelligence : régimes, multi-unités, structure, scanner (0.3)

Tout est calculé sur des **bougies clôturées** (la bougie en cours n'est jamais utilisée) et
testé dans `crates/aegis-market/src/{regime,structure,mtf,scanner}.rs`.

## Régimes détaillés

| Régime | Règle |
|---|---|
| tendance haussière / baissière | ER(20) ≥ 0,30 et EMA20 au-dessus / au-dessous de l'EMA50 |
| transition de tendance | tendance par la règle ci-dessus, mais croisement EMA20/50 dans les 10 dernières bougies, ou mouvement efficace (ER(10) ≥ 0,4) contre la tendance |
| marché désordonné | volatilité forte (ATR ≥ 1,8 × normale) et ER(20) < 0,20 |
| consolidation | ER(20) < 0,30 avec volatilité faible ou en compression |
| marché latéral | ER(20) < 0,30 sinon |
| volatilité forte / normale / faible | ATR(14) / médiane des vrais ranges sur 100 bougies : ≥ 1,8 / entre / ≤ 0,6 |
| expansion / compression | ATR(5) / ATR(20) > 1,3 / < 0,75 |
| conditions anormales | problème de qualité de données bloquant (prix périmé, spread anormal, horodatage…) |
| proche d'une annonce | recommandation News ≠ autorisé |

Chaque changement de régime (instrument × unité) est enregistré (table `regime_history`) ;
chaque transaction garde son régime détaillé (`trade_journal.regime_detail`), d'où la
performance par régime.

Effets sur les entrées (`[intelligence]`) : marché désordonné → refus (`block_chaotic`,
activé) ; transition → probabilité exigée + `transition_probability_margin` (0 par défaut,
0,05 recommandé) ; les stratégies gardent leur liste de régimes compatibles (0.2).

## Multi-unités

Rôles : D1/H4 contexte, H1/M30 structure, M15 confirmation, M5 préparation, M1 précision.
H4 et D1 sont **reconstruits depuis les bougies H1 du moteur** (alignées UTC) : les bougies
H4/D1 du broker sont alignées sur l'heure du serveur.

Ce n'est **pas un vote** : un contexte, une structure ou une confirmation opposés rendent
l'opportunité non admissible quel que soit le nombre d'unités d'accord ; une unité sans
assez de données est « non disponible » et ne compte jamais comme un accord. Contrôle
`multi_unites` dans chaque décision (informatif, ou bloquant avec `require_mtf_alignment`).

## Structure et configurations

Définitions exactes en tête de `structure.rs` : swings confirmés, HH/HL/LH/LL, biais de
structure, BOS, CHoCH, cassure, fausse cassure, retest, repli, rejet, divergence RSI,
compression/expansion, zones de liquidité (sommets/creux égaux), déplacement rapide,
déséquilibre (fair value gap non comblé). Ce sont des descriptions de configurations, pas des
stratégies supplémentaires : les 9 stratégies existantes restent les seules à émettre des
signaux.

## Scanner d'opportunités

À chaque bougie clôturée d'une unité de signal, pour chaque instrument, dans le moteur (avec
ou sans l'application). Une opportunité par sens ayant au moins une configuration orientée.

**Score de configuration (0–100)** — jamais une probabilité :
configuration (30) + multi-unités (30) + adéquation au régime (20) + coût (spread/ATR, 10) +
volatilité (10) − 10 par configuration contraire. Chaque composante est affichée.

**Probabilité** : P(TP avant SL) du modèle de scénario calibré, pour le scénario affiché
(TP/SL en ATR de `[probability]`) ; absente si aucun modèle validé, avec la raison.
**Espérance nette** après coûts, au volume que le Risk Engine calculerait.

**Admissible** seulement si : aucune contradiction de contexte/structure/confirmation, pas de
marché désordonné ni de conditions anormales, probabilité calibrée au-dessus du seuil du
sens, espérance nette suffisante, score au moins `min_opportunity_score`. Le scanner n'envoie
**aucun ordre** : il éclaire les décisions ; avec `min_opportunity_score > 0`, une entrée sans
configuration suffisante du même sens est refusée (`score_configuration`).
