# Manuel utilisateur

## 1. Ce qu'il faut savoir avant de commencer

- AEGIS **démarre toujours en mode MONITOR** : il observe et explique, il n'envoie aucun ordre.
- Rien n'est exécuté sans passer par le moteur de risque. Vous pouvez toujours lire pourquoi
  un signal a été autorisé ou rejeté.
- AEGIS ne promet aucun gain. Un objectif journalier est une condition d'arrêt, pas une cible
  à atteindre coûte que coûte.
- Commencez **toujours** par un compte de démonstration.

## 2. Installer et lancer

```sh
./scripts/setup.sh     # une fois : installe Rust si besoin, compile, crée config/aegis.toml
./scripts/run.sh       # à chaque fois
```

La fenêtre s'ouvre sur le tableau de bord. Sans terminal connecté, les valeurs du compte
affichent « Indisponible » : c'est normal, AEGIS n'invente aucun chiffre.

## 3. Connecter MetaTrader 5

> Le connecteur MQL5 n'a pas pu être compilé sur la machine de développement. La première
> compilation dans MetaEditor peut révéler des erreurs à corriger (voir `KNOWN_LIMITATIONS.md`).

1. Dans MT5 : *Fichier → Ouvrir le dossier des données*, puis `MQL5/Experts/`. Créez un dossier
   `AEGIS` et copiez-y les trois fichiers du dossier `mt5/` du projet.
2. Ouvrez `AegisConnector.mq5` dans MetaEditor et compilez (F7). Compilez aussi
   `AegisSelfTest.mq5`.
3. Dans MT5, lancez le script **AegisSelfTest** sur un graphique. L'onglet *Experts* doit
   afficher `AEGIS self-test: ALL PASSED`. Sinon, n'allez pas plus loin.
4. *Outils → Options → Expert Advisors* : cochez « Autoriser le trading algorithmique » et
   ajoutez l'adresse du moteur (par défaut `127.0.0.1`) à la liste des adresses autorisées.
5. Dans AEGIS, page **Connexion MT5** : cliquez sur *Copier* à côté de la clé partagée.
6. Glissez l'Expert Advisor **AegisConnector** sur un graphique et renseignez :
   - `InpHost` / `InpPort` : l'adresse et le port affichés dans AEGIS ;
   - `InpSharedKey` : la clé copiée ;
   - `InpSymbols` : les instruments, séparés par des virgules (vide = celui du graphique) ;
   - `InpAllowRealAccount` : laissez `false`.
7. AEGIS affiche « TERMINAL MT5 CONNECTÉ » avec le numéro du compte.

L'horloge du PC qui fait tourner MT5 doit être à l'heure (écart toléré : 30 secondes).

**Commission** : MT5 ne la communique pas. Saisissez la commission aller-retour par lot de
votre broker dans *Gestion des risques*, sinon les coûts estimés seront faux.

## 4. Essayer sans MetaTrader : le simulateur

Page **Connexion MT5 → Démarrer le simulateur**. Il remplace MT5 par un terminal fictif :
prix synthétiques, compte de 10 000 fictifs, instrument `EURUSD.sim`. Un bandeau orange
« SIMULATEUR — données fictives » reste affiché en permanence. Rien de ce qu'il produit n'a de
valeur comme indication de performance.

## 5. Les 18 pages

| Page | À quoi elle sert |
|---|---|
| Tableau de bord | Solde, équité, P&L du jour, flottant, drawdown, spread, positions, ordres, stratégie, régime, connexion, moteur, modèle, risques |
| Marché en direct | Prix, spread, session, qualité des données, indicateurs par unité de temps, graphique |
| Intelligence artificielle | Modèle en service, dernière estimation, performance réelle, dérive |
| Signaux de trading | Chaque signal, son résultat, et le détail complet (bouton *Détail*) |
| Positions ouvertes | Positions du compte, clôture, resserrement du stop |
| Ordres en attente | Ordres en attente et toutes les instructions envoyées, avec leur statut réel |
| Historique des transactions | Clôtures : brut, commission, swap, net — pertes comprises |
| Laboratoire de stratégies | Règles de chaque stratégie, preuves disponibles, changement de stade |
| Backtesting | Lancer un backtest, lire les métriques par segment, importer des données |
| Gestion des modèles IA | Charger un modèle, lire sa validation, exporter un jeu de données |
| Gestion des risques | État des limites et réglages |
| Gestion des objectifs | Progression du jour, objectif journalier |
| Moteur de micro-profits | Pour chaque opportunité : brut, coûts, net, espérance |
| Gestion du compte | Compte et spécifications des instruments reçues du broker |
| Connexion MT5 | État de la liaison, clé, simulateur |
| Surveillance du système | Santé des modules, base de données, journal, piste d'audit |
| Paramètres | Configuration en vigueur |
| Centre de contrôle | Modes, arrêt d'urgence |

## 6. Comprendre une décision

Page **Signaux de trading → Détail**. Vous y lisez :

- **Pourquoi le signal** : la règle déclenchée et les valeurs des indicateurs ;
- **Les niveaux** : entrée, stop, cible, volume calculé, montant risqué et budget ;
- **Les coûts** : spread, commission, slippage, swap, conversion, et le net si la cible ou le
  stop est touché ;
- **La probabilité utilisée et sa source** (un backtest hors échantillon précis), ou la mention
  qu'aucune n'est disponible ;
- **Chaque contrôle** avec OK ou ÉCHEC et son explication.

Résultats possibles : **AUTORISÉ** (ordre envoyé), **REJETÉ** (au moins un contrôle a échoué),
**OBSERVÉ** (mode MONITOR : évalué mais jamais exécuté).

## 7. Faire trader une stratégie en démonstration

1. Recevez de l'historique (connexion MT5) ou importez un export CSV (page Backtesting).
2. Lancez un **backtest** de la stratégie sur l'instrument et l'unité de temps voulus.
   Le segment « Hors échantillon » doit compter au moins 30 transactions et être positif.
3. **Laboratoire de stratégies** : demandez le stade *Démo*. AEGIS refuse si la preuve manque.
4. **Centre de contrôle** : *Activer DEMO AUTO* (refusé si le compte est réel).

Passer au stade *Approuvée réel* exige en plus 50 transactions réellement exécutées en démo,
avec un résultat net positif. Ce n'est jamais automatique.

## 8. LIVE AUTO

Désactivé par défaut. Pour qu'il devienne possible, **toutes** ces conditions sont requises :

1. `live_trading_enabled = true` dans la section `[safety]` de `config/aegis.toml` ;
2. un vrai terminal MT5 connecté à un compte **réel** ;
3. `InpAllowRealAccount = true` dans le connecteur ;
4. au moins une stratégie au stade *Approuvée réel* ;
5. arrêt d'urgence inactif ;
6. saisie exacte, dans le Centre de contrôle, de `<numéro de compte> J'ACCEPTE LE RISQUE REEL`.

LIVE AUTO est abandonné à la moindre déconnexion et n'est jamais restauré au redémarrage.
**Ce mode engage de l'argent réel : vous pouvez perdre.** Il n'a pas été testé (voir
`KNOWN_LIMITATIONS.md`).

## 9. Arrêt d'urgence

Bouton rouge en haut à droite, toujours visible. Il :

1. bloque immédiatement toute nouvelle entrée ;
2. annule les ordres en attente d'AEGIS (réglable) ;
3. conserve les positions d'AEGIS avec leurs stops, ou les clôture au marché, selon la
   politique choisie dans *Gestion des risques* ;
4. enregistre l'événement ;
5. reste actif — même après un redémarrage — jusqu'à ce que vous saisissiez `RESET AEGIS`
   dans le Centre de contrôle. Le moteur repasse alors en MONITOR.

Il se déclenche aussi tout seul si le drawdown maximal est atteint.
Les positions ouvertes en dehors d'AEGIS ne sont jamais touchées.

## 10. Moteur sans interface (`aegisd`)

```sh
./scripts/run-daemon.sh                 # moteur seul, Ctrl-C pour arrêter
target/release/aegisd backtest --strategy ema_trend --symbol EURUSD --tf M15
target/release/aegisd verify-audit      # intégrité du journal d'audit
target/release/aegisd verify-model --file models/model.json
```

Liste complète des commandes : en-tête de `crates/aegisd/src/main.rs`.

## 11. En cas de problème

| Symptôme | Piste |
|---|---|
| « TERMINAL DÉCONNECTÉ » | Connecteur attaché ? adresse autorisée dans MT5 ? clé identique ? horloge du PC à l'heure ? Voir « Dernier refus » sur la page Connexion MT5 |
| Tous les signaux sont « OBSERVÉ » | Vous êtes en MONITOR |
| Rejet `esperance_nette: aucune probabilité validée` | Aucun backtest hors échantillon exploitable pour cette stratégie/instrument/unité |
| Rejet `stade_strategie` | La stratégie est encore au stade Recherche |
| Rejet `taille_position` | Le budget de risque ne permet pas le volume minimum ; AEGIS n'arrondit jamais vers le haut |
| « BASE DE DONNÉES INDISPONIBLE » | Disque plein ou fichier verrouillé ; les entrées sont bloquées jusqu'au retour à la normale |
| Ordre au statut `unknown` | Aucune confirmation reçue : la réconciliation tranchera au prochain état du compte |
