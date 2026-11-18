# Manuel utilisateur — AEGIS Desktop

## 1. Ce qu'il faut savoir avant de commencer

- Le moteur **démarre toujours en MONITOR** : il observe, calcule et explique, il n'envoie
  aucun ordre.
- Rien n'est exécuté sans le moteur de risque, et chaque décision s'explique (page *Décisions*).
- AEGIS ne promet aucun gain. L'objectif quotidien est poursuivi, jamais forcé.
- Commencez **toujours** par le simulateur, puis un compte de démonstration.

## 2. Installer et lancer

Installateurs : `INSTALLATION_WINDOWS.md`, `INSTALLATION_LINUX.md`. Depuis les sources :
`./scripts/setup.sh` puis `./scripts/run.sh`.

Au premier lancement, la page **Connexion** propose :

- **Moteur local** — « Démarrer le moteur local » : le moteur installé avec l'application
  démarre en arrière-plan, avec sa propre configuration, et AEGIS s'y connecte. Il continue de
  tourner quand vous fermez la fenêtre (vos positions restent gérées).
- **… avec le simulateur de test** — pour découvrir le logiciel sans MetaTrader : prix
  fictifs sur `EURUSD.sim`, compte fictif. La barre d'état affiche « SIMULATEUR (test) ».
- **Moteur distant (VPS)** — adresse, jeton et empreinte donnés par `aegisd pairing` sur le
  serveur. « Lire l'empreinte » affiche celle du serveur : comparez-la caractère par caractère
  avant de l'accepter.

## 3. Connecter MetaTrader 5

Voir `MT5_CONNECTOR.md` (installation, `AegisSelfTest`, paramètres). La clé partagée est dans
`<dossier de données du moteur>/shared.key`. Quand le connecteur est attaché, la barre d'état
passe à « MT5 connecté » avec le serveur, le compte, son type (DEMO/REAL) et la latence.

**Commission** : MT5 ne la communique pas. Saisissez-la (*Risque & configuration → Coûts*),
sinon les coûts estimés seront faux.

## 4. Les pages

| Page | Contenu |
|---|---|
| **Tableau de bord** | objectif quotidien (réalisé brut ou net, restant, progression), journée (profit brut, perte brute, net, commissions, spread, slippage, swap, frais), compte (équité, flottant, drawdown, perte du jour), marchés (prix, spread, volatilité, régime, **P(BUY)**, **P(SELL)**, décision BUY/SELL/WAIT et sa raison), graphique, positions avec la décision de gestion, stratégies, actualités, Shadow |
| **Performance** | aujourd'hui, 7 jours, 30 jours, historique : brut, net, win rate, profit factor, espérance, moyennes, coût moyen, part des coûts, efficacité des sorties ; courbe cumulée et drawdown ; Sharpe journalier ; par heure, jour, semaine, mois, stratégie, instrument, heure d'ouverture, régime ; dégradations détectées |
| **Positions & ordres** | positions (clôture, SL/TP), ordres en attente, ordres, commandes, **journal** (décision → probabilité → coûts estimés → résultat réel, MFE/MAE) |
| **Décisions** | chaque signal : contrôles OK/échec, coûts, net si TP/SL, probabilité et sa source, espérance nette |
| **Stratégies** | 9 stratégies, règles, paramètres (JSON validé), stades, preuves, backtests (complet, segments, walk-forward) |
| **Probabilités & Shadow** | modèles P(TP avant SL) par instrument : Brier, log loss, ECE, diagrammes de fiabilité, entraînement, retour arrière ; calibration réelle ; Shadow : champions et challengers, étapes, opportunités manquées, entrées/sorties prématurées, promotion |
| **Actualités** | recommandation par instrument (autorisé / prudent / suspendu), annonces des 48 h |
| **Risque & configuration** | modes, réinitialisation de l'arrêt d'urgence, toute la configuration de risque (objectif, seuils, limites, volume, coûts, gestion des positions, horaires) |
| **Système** | santé des modules, CPU/RAM du serveur, liaison MT5, alertes, base et sauvegardes, journal, audit |

## 5. Comprendre une décision

Résultats : **AUTORISÉ** (ordre envoyé), **REJETÉ** (au moins un contrôle a échoué),
**OBSERVÉ** (MONITOR). Le premier contrôle en échec s'affiche dans la liste ; un clic ouvre le
détail complet. Une décision BUY/SELL n'apparaît sur le tableau de bord que si un signal de la
dernière bougie a été autorisé ; sinon **WAIT**, avec la raison — c'est le cas le plus fréquent
et c'est normal.

**P(BUY) / P(SELL)** sur le tableau de bord : probabilité calibrée qu'un achat (une vente)
avec une cible de 0,5 ATR et un stop de 1 ATR touche la cible d'abord. Ce n'est pas la
probabilité que le prix monte.

## 6. Faire trader une stratégie en démonstration

1. Laisser le connecteur collecter l'historique (ou importer un CSV).
2. *Probabilités* : entraîner le modèle sur l'instrument et l'unité de temps (données broker).
3. *Stratégies* : backtest ; le segment hors échantillon doit compter ≥ 30 transactions au net
   positif ; puis « → Démo ».
4. *Risque & configuration* : régler la commission, le plafond de lots, l'objectif, les seuils.
5. « DEMO AUTO » (refusé si le compte est réel).

« Approuvée réel » exige en plus des transactions réellement exécutées en démo, au net positif.

## 7. LIVE AUTO

Désactivé par défaut. Toutes ces conditions sont requises : `safety.live_trading_enabled =
true` dans la configuration du serveur ; un vrai terminal sur un compte **réel** ;
`InpAllowRealAccount = true` dans le connecteur ; une stratégie « Approuvée réel » ; arrêt
d'urgence inactif ; saisie exacte de `<numéro de compte> J'ACCEPTE LE RISQUE REEL`. Abandonné
à la moindre déconnexion, jamais restauré. **Argent réel : vous pouvez perdre.** Non testé.

## 8. Arrêt d'urgence

Bouton rouge de la barre d'état, toujours visible. Effet immédiat sur les nouvelles entrées ;
ordres en attente d'AEGIS annulés ; positions conservées avec leurs stops ou clôturées, selon
la politique ; persistant après redémarrage ; réinitialisation avec `RESET AEGIS`
(*Risque & configuration*). Déclenché aussi au drawdown maximal. Les positions ouvertes hors
d'AEGIS ne sont jamais touchées.

## 9. En cas de problème

Voir `TROUBLESHOOTING.md`.
