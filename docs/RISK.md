# Gestion des risques

Le moteur de risque (`crates/aegis-risk`) est le seul à pouvoir autoriser une entrée.

## Taille de position

```
budget            = équité × risque_par_transaction %
perte par lot     = valeur(|entrée − stop|, 1 lot) + commission aller-retour + slippage estimé
volume            = budget / perte par lot, arrondi VERS LE BAS au pas de volume du broker
```

`valeur(distance, lots) = distance / taille_du_tick × valeur_du_tick × lots`, avec la
valeur du tick fournie par le broker dans la devise du compte : la conversion de devise est
donc celle du broker. Si le volume obtenu est inférieur au minimum du broker, l'entrée est
**refusée** ; AEGIS n'arrondit jamais vers le haut. Un volume demandé par l'appelant ne peut
que réduire la taille. Un montant de risque annoncé par un autre module n'est jamais cru :
il est comparé au calcul interne et signalé s'il diffère.

## Coûts et espérance nette (micro-profits)

```
résultat net attendu = résultat brut attendu − spread − commissions − swaps − slippage estimé − frais de conversion
résultat brut attendu = p × gain brut − (1 − p) × perte brute
```

- Le brut est le déplacement du prix milieu ; l'aller-retour paie le spread entier.
- `p` vient d'un backtest hors échantillon (borne basse de Wilson, 30 transactions minimum).
  Sans `p` : « non calculable » ⇒ refus.
- L'entrée exige `résultat net attendu ≥ min_net_expected_profit` et `> 0`.

## Contrôles appliqués à chaque proposition

| Contrôle | Refus si |
|---|---|
| `arret_urgence` | l'arrêt d'urgence est actif |
| `mode` | mode MONITOR |
| `decision_unique` | la décision a déjà été évaluée |
| `connexion` | terminal déconnecté |
| `compte_valide`, `compte_frais`, `compte_trading_autorise` | compte absent, incohérent, périmé, trading interdit |
| `mode_compte` | DEMO AUTO sur compte réel ; LIVE AUTO sans armement, sans compte réel ou avec le simulateur |
| `instrument_spec`, `instrument_negociable` | spécification absente/invalide, instrument non négociable |
| `prix_valide`, `prix_frais` | prix absent, incohérent, trop vieux ou daté du futur |
| `spread` | spread au-dessus du maximum |
| `perte_journaliere` | perte nette du jour ≥ limite |
| `objectif_journalier` | objectif quotidien (devise du compte, brut ou net) bloquant selon sa politique |
| `transactions_max` | nombre maximal d'entrées du jour atteint |
| `pertes_consecutives` | série de pertes ≥ limite et pause pas encore écoulée |
| `couts_vs_gains` | coûts du jour > part autorisée des gains bruts (après N transactions) |
| `horaires` | hors des plages de trading UTC |
| `debit_ordres` | trop d'entrées sur la dernière minute |
| `drawdown` | drawdown ≥ limite |
| `positions_max`, `positions_par_instrument` | trop de positions/ordres |
| `niveaux_valides`, `niveaux_coherents`, `distance_stops`, `prix_ordre_attente` | stop ou cible absent, du mauvais côté, trop proche |
| `taille_position`, `risque_par_transaction` | volume minimum impossible, risque au-dessus du budget |
| `exposition`, `exposition_devise`, `marge_libre` | notionnel total ou par devise > limite, exposition incalculable, marge insuffisante |
| `mode_marge` | compte **netting** (ou mode inconnu, connecteur < 1.2) : une position existe déjà sur l'instrument — l'ordre fusionnerait, réduirait ou inverserait au lieu de s'ajouter |
| `couverture` | compte **hedging** : position opposée sur l'instrument alors que `allow_hedging = false` |
| `jours_autorises` | jour UTC hors de `allowed_weekdays` |
| `risque_portefeuille` | pertes aux stops ACTUELS de toutes les positions et ordres + la nouvelle > limite ; une position sans stop rend le risque non borné |
| `budget_risque_journalier` | risque cumulé des entrées du jour > `daily_risk_budget_pct` |
| `slippage_observe` | slippage médian des exécutions récentes de l'instrument > `max_observed_slippage_points` |
| `reduction_apres_pertes` | (information) budget de risque réduit après une série de pertes |
| `seuil_probabilite` | P(TP avant SL) absente ou sous le seuil BUY/SELL de l'utilisateur |
| `esperance_nette` | espérance nette insuffisante ou non calculable |

Contrôles ajoutés par le moteur central : `base_de_donnees`, `stade_strategie`,
`regime_marche`, `qualite_donnees`, `ordre_en_cours`, `filtre_ia`, `actualites` (suspendu
autour d'une annonce forte ; en prudence, probabilité exigée = seuil + marge).

La taille est calculée au risque (`risk_per_trade_pct`) puis éventuellement **réduite** par
`max_lots_per_trade` ; elle n'est jamais augmentée.

## Risk Guardian 2.0 (0.3)

**Risque du portefeuille.** Somme, pour chaque position et ordre en attente, de la perte si
son stop ACTUEL est touché (prix d'ouverture → stop, plus commission), plus le risque de la
nouvelle transaction. Un stop déplacé au point mort compte 0. Une position **sans stop**
(y compris ouverte hors d'AEGIS) rend le risque non borné : toute entrée est refusée tant
qu'elle existe. Limite : `max_portfolio_risk_pct`, ou par défaut
`max_open_positions x risk_per_trade_pct` (ce qui ne change rien pour des positions
protégées normalement).

**Série de pertes.** `loss_streak_risk_scale` multiplie le budget de risque après N pertes
d'affilée ; chaque valeur est ≤ 1 (refus à la validation sinon) : AEGIS ne peut **jamais**
augmenter la taille après une perte.

**Mode du compte.** Depuis le protocole 1.2, le connecteur transmet `ACCOUNT_MARGIN_MODE`.
- *Netting* : une seule position par instrument ; un ordre de même sens fusionne (la position
  prend le nouveau stop), un ordre opposé réduit, clôture ou inverse. AEGIS refuse donc toute
  entrée sur un instrument où une position existe.
- *Hedging* : positions indépendantes ; une position opposée (couverture) n'est permise que si
  `allow_hedging = true`, et elle est évaluée comme un nouveau risque complet (son propre stop,
  sa taille, le risque du portefeuille).
- *Inconnu* (connecteur 1.1) : traité comme netting.

**Stop initial.** `[risk.stops] mode = "adaptive"` : stop **structurel** (derrière le dernier
swing confirmé qui laisse la place minimale, plus un tampon en ATR et le spread), sinon stop de
**volatilité** (`volatility_atr` x ATR), sinon celui de la stratégie ; toujours entre
`min_atr` (et la distance broker, et `min_spreads` spreads) et `max_atr`. La taille est
recalculée sur la distance : **un stop plus éloigné donne un volume plus petit pour le même
risque monétaire** ; si même le volume minimum du broker dépasse le budget, la transaction est
refusée. Un backtest mesuré avec les stops de la stratégie n'est **pas** utilisé comme
probabilité pour un stop adapté : seul le modèle de scénario (qui prend SL et TP en entrée)
peut en fournir une.

Les six rôles de stop distingués : initial (stratégie), structurel, volatilité (choix
initial) ; break-even et suiveur (Position Intelligence) ; protection d'urgence (garde).

**Garde de protection.** Après chaque instantané du broker :
- une position AEGIS sans stop au-delà de `missing_stop_grace_ms` est un incident. Politique
  `restore` (défaut) : le stop prévu à la décision est réinstallé s'il est encore du bon côté du
  prix et au-delà de la distance broker ; sinon, ou après deux refus du broker, la position est
  clôturée. `close` : clôture directe. `alert` : alerte seulement. En MONITOR (ou sans
  terminal), rien n'est envoyé : alerte seulement ;
- une modification acceptée par le broker doit apparaître dans l'instantané suivant ; sinon,
  incident « modification non constatée » et alerte.
Tout est écrit au journal d'audit (`stop_restore`, `protective_close`, `protection_alert`).

**CLOSE PARTIAL.** `position.partial_close_at_r` (0 = désactivé) : une fois par position,
clôture de `partial_close_fraction` du volume, arrondie **vers le bas** au pas du broker,
seulement si la partie clôturée ET le reste sont au moins le volume minimum et sur le pas.

**Qualité d'exécution.** Le moteur sépare : temps interne (décision → envoi, mesuré par le
moteur), temps broker (dans `OrderSend`, mesuré par le connecteur 1.2), réseau + terminal
(aller-retour − temps broker), et le slippage de chaque exécution. La vitesse du moteur n'est
jamais présentée comme la vitesse d'exécution du marché.

## Objectif quotidien et gestion des positions

Voir `CONFIGURATION.md` (objectif : brut ou net, politiques) et `DECISIONS.md` (D25). La
gestion des positions (SECURE au point mort + coûts, stop suiveur, rapprochement du TP,
sortie au temps ou au net réalisable) ne déplace jamais un stop dans le sens défavorable et
respecte les distances minimales et le niveau de gel du broker.

## Autorisation

Quand tout passe, le moteur de risque émet une autorisation : identifiant, décision, instrument,
sens, volume, prix, stop, cible, slippage maximal, expiration (3 s), signée par HMAC avec une
clé propre au processus. L'exécution la **consomme** : elle ne sert qu'une fois, ne survit pas
à un redémarrage, et toute modification d'un champ l'invalide. L'arrêt d'urgence annule les
autorisations en circulation.

## Convention comptable journalière

- La journée est la **journée UTC**.
- **Résultat net du jour** = équité actuelle − équité de début de journée − dépôts/retraits.
  Il contient le réalisé, le flottant, les commissions et les swaps, tels que le broker les
  applique à l'équité.
- **Réalisé net** = somme (profit + commission + swap) des clôtures du jour.
- **Flottant** = somme (profit + swap) des positions ouvertes.
- La **limite de perte journalière** porte sur le résultat net (réalisé + flottant).
- L'**objectif journalier** porte sur le même résultat net. Atteint, il arrête les nouvelles
  entrées jusqu'au lendemain. Il ne ferme pas les positions et ne modifie jamais le risque.
- Un dépôt n'est pas un gain ; un retrait n'est pas une perte.

## Arrêt d'urgence

Déclenchement : bouton, API, ou automatiquement quand le drawdown maximal est atteint.

1. Bloque immédiatement toute nouvelle entrée et invalide les autorisations en cours.
2. Annule les ordres en attente d'AEGIS si `kill_cancel_pending = true`.
3. Positions d'AEGIS : `keep` (défaut) les laisse avec leurs SL/TP ; `close` demande leur
   clôture au marché. Les positions ouvertes hors d'AEGIS ne sont jamais touchées.
4. L'événement est enregistré (base + journal d'audit).
5. Reste actif après redémarrage. Réinitialisation manuelle par la phrase `RESET AEGIS`,
   après quoi le moteur est en MONITOR.

Les étapes 2 et 3 nécessitent un terminal connecté et un mode d'exécution ; sinon elles sont
mises en attente et signalées.

## Limites dures de configuration

Une configuration hors de ces bornes est **refusée** (pas corrigée en silence) :
risque par transaction 0,01–5 % et ≤ perte journalière ; perte journalière 0,1–20 % ;
drawdown 0,5–50 % ; positions 1–100 ; âge maximal d'un prix 0,5–600 s ; validité d'une
autorisation 0,5–60 s.
