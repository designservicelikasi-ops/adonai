# Limites connues

Cette liste est volontairement complète. Rien ici n'est caché ailleurs.

## Non vérifié (à faire avant toute utilisation avec un broker)

1. **Le connecteur MQL5 n'a jamais été compilé.** `mt5/AegisConnector.mq5`,
   `AegisProtocol.mqh` et `AegisSelfTest.mq5` ont été écrits d'après la référence MQL5, sans
   MetaEditor. Attendez-vous à devoir corriger des erreurs de compilation. Procédure :
   compiler, lancer le script `AegisSelfTest` (il doit afficher « ALL PASSED »), puis tester
   sur un compte de démonstration.
2. **Aucun essai sur un vrai terminal MT5.** Tout le cycle a été validé contre le simulateur,
   qui applique les mêmes règles que le connecteur mais n'est pas un broker.
3. **Windows et Linux non compilés.** Le code n'utilise rien de spécifique à macOS, et le
   fichier `.github/workflows/build.yml` décrit la compilation sur les trois systèmes, mais
   aucune n'a été exécutée ailleurs que sur macOS (Apple Silicon).
4. **L'apparence de l'application n'a pas été regardée.** Elle se lance et ses 18 pages se
   dessinent sans erreur, mais personne n'a vérifié la mise en page à l'écran.

## Limites de conception

5. **Liaison non chiffrée.** Le protocole authentifie et empêche le rejeu, mais ne chiffre
   pas : prix et ordres circulent en clair. À garder sur une seule machine ou dans un tunnel
   (WireGuard, SSH). Voir `docs/DEPLOYMENT.md`.
6. **Un seul terminal, un seul compte à la fois.**
7. **Comptes « hedging » visés.** Sur un compte « netting » (une position par instrument), la
   correspondance ordre → position repose sur un repli non testé.
8. **Commission non fournie par MT5** : elle se saisit à la main (Gestion des risques). Une
   valeur fausse fausse l'estimation des coûts.
9. **Swaps** : convertis seulement quand le broker les exprime en points ou en devise du
   compte ; sinon transmis à zéro (l'estimation des coûts ignore alors le swap).
10. **En MONITOR, AEGIS ne peut pas clôturer une position** (aucune trame d'ordre ne sort).
    Passez en DEMO AUTO/LIVE AUTO ou agissez dans MT5.
11. **Arrêt d'urgence en MONITOR ou hors connexion** : il bloque les entrées immédiatement,
    mais annulations et clôtures restent « en attente » jusqu'au retour dans un mode
    d'exécution avec le terminal connecté.
12. **Ordres en attente** : gérés par le moteur de risque, le protocole, le connecteur et le
    simulateur, mais les trois stratégies livrées n'émettent que des ordres au marché.
13. **Journée comptable = UTC**, pas l'heure du broker.
14. **Plus haut d'équité** : mesuré depuis la première connexion du compte, ajusté des
    dépôts/retraits que le connecteur signale pendant qu'il tourne. Un dépôt fait pendant que
    le connecteur est arrêté n'est pas vu comme tel.
15. **Sessions de marché** en heures UTC fixes (pas d'heure d'été).
16. **Bougies H4/D1** non demandées au terminal.
17. **Interface locale uniquement** : pas d'application de bureau pilotant un moteur distant
    (utiliser l'API HTTP d'`aegisd`).
18. **Pas de paramétrage des stratégies depuis l'interface** : leurs paramètres par défaut
    sont dans le code ; un autre jeu de paramètres demande une modification du code.
19. **Journal d'audit pendant une panne de base** : les événements sont écrits dans le fichier
    journal mais pas dans la base ; les entrées de trading sont bloquées pendant ce temps.
20. **Montants en virgule flottante** (`f64`) : précision largement suffisante pour le
    contrôle du risque, pas pour une comptabilité officielle.

## Ce qu'AEGIS ne fait pas, par principe

- Il ne promet ni ne suggère aucune rentabilité.
- Il n'augmente jamais la taille après une perte et ne force jamais une transaction pour
  atteindre l'objectif du jour.
- Il n'active jamais LIVE AUTO de lui-même et ne le restaure jamais après un redémarrage.
- Il n'utilise jamais un backtest sur données synthétiques, ni un modèle entraîné dessus, avec
  un vrai broker.

## Sur les résultats livrés

Les backtests et le modèle fournis (`models/demo-synthetic-model.json`) reposent sur des
données **synthétiques**. Ils démontrent que le logiciel fonctionne ; ils ne disent rien de la
rentabilité d'une stratégie sur un marché réel. **Aucune stratégie livrée n'a été validée sur
des données de marché réelles.**
