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
| `objectif_journalier` | objectif du jour atteint |
| `drawdown` | drawdown ≥ limite |
| `positions_max`, `positions_par_instrument` | trop de positions/ordres |
| `niveaux_valides`, `niveaux_coherents`, `distance_stops`, `prix_ordre_attente` | stop ou cible absent, du mauvais côté, trop proche |
| `taille_position`, `risque_par_transaction` | volume minimum impossible, risque au-dessus du budget |
| `exposition`, `marge_libre` | notionnel total > limite, exposition incalculable, marge insuffisante |
| `esperance_nette` | espérance nette insuffisante ou non calculable |

Contrôles ajoutés par le moteur central : `base_de_donnees`, `stade_strategie`,
`regime_marche`, `qualite_donnees`, `ordre_en_cours`, `filtre_ia`.

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
