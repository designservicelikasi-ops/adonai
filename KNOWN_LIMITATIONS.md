# Limites connues (0.3)

Cette liste est volontairement complète. Rien ici n'est caché ailleurs.

## Non vérifié (à faire avant toute utilisation avec un broker)

1. **Connecteur MQL5 1.20 (protocole 1.2) : compilation à confirmer par la CI.** Le job CI
   `mql5` (MetaEditor installé sur un runner Windows) a compilé le connecteur **1.10** et son
   auto-test sans erreur le 2026-11-21. La version 1.20 (mode de marge, latence broker) n'a pas
   encore été compilée au moment de cette rédaction : voir `TEST_REPORT.md` § 2 pour le
   résultat observé. L'auto-test `AegisSelfTest` n'a jamais été **exécuté** (il faut un
   terminal).
2. **Aucun essai sur un vrai terminal MT5.** Tout le cycle a été validé contre le simulateur,
   qui n'est pas un broker. Latence et comportement réels du broker non mesurés.
3. **Windows et Linux : vérifiés par la CI en 0.2 seulement.** Le 2026-11-21 (commit
   `8a96ddb`), tests, moteurs et installateurs Windows et Linux ont été construits avec succès
   sur des runners natifs. Pour la 0.3 : voir `TEST_REPORT.md` § 2.
4. **Service Windows** (`windows-service`) et scripts PowerShell d'installation du service :
   jamais exécutés hors de la CI.
5. **Fenêtre native du desktop** non photographiée ; les 25 pages ont été rendues dans Chrome
   avec les vraies réponses du moteur (mode aperçu). Pendant cet essai, aucune position n'a été
   ouverte : tableaux de positions, ordres, couvertures et latences vus **vides** seulement.
   Palette Ctrl/Cmd+K non essayée au clavier.
6. **Aucune stratégie ni aucun modèle évalué sur des données de marché réelles.**
7. **Telegram et webhook** : jamais essayés contre le vrai service (pas de bot ni d'adresse
   de test) ; le refus faute de jeton et le format des messages sont testés.

## Limites de conception

8. **Liaison MT5 non chiffrée** (authentifiée, anti-rejeu) : boucle locale ou tunnel.
9. **Un terminal, un compte à la fois.**
10. **Comptes netting** : depuis 0.3 le mode est transmis (protocole 1.2) et AEGIS n'empile ni
   ne compense jamais de position sur un compte netting ; testé contre le simulateur en mode
   netting, **pas contre un vrai compte netting**. Le connecteur 1.20 n'a pas été compilé.
11. **Commission non fournie par MT5** : à saisir (une valeur fausse fausse les coûts).
12. **Swaps** convertis seulement s'ils sont en points ou en devise du compte.
13. **En MONITOR, aucune clôture** ne part (même manuelle).
14. **Arrêt d'urgence en MONITOR ou hors connexion** : annulations/clôtures « en attente ».
15. **Ordres en attente** : gérés partout, mais les 9 stratégies n'émettent que des ordres au
    marché.
16. **Journée comptable = UTC** ; sessions de marché en heures UTC fixes.
17. **Bougies H4/D1** non demandées au terminal (ses barres ne sont pas alignées sur UTC) :
    le scanner les reconstruit à partir des barres H1 UTC, donc elles diffèrent de celles du
    graphique MT5 du broker.
18. **Coût du spread** : estimé à la décision (le broker l'inclut dans le résultat brut) ;
    le slippage est mesuré à l'exécution (entrées), pas à la sortie.
19. **Objectif quotidien mesuré sur le réalisé** (le flottant n'est pas compté).
20. **Exposition par devise** : brute (sans compensation entre positions), donc prudente.
21. **Probability Engine** : un seul horizon de temps par modèle ; grille de scénarios
    bornée (un scénario hors grille n'a pas de probabilité) ; régression logistique (pas de
    modèle non linéaire).
22. **Shadow** : forward papier à 1 lot normalisé, sans slippage ; positions papier perdues au
    redémarrage (les statistiques sont conservées) ; une transaction clôturée partiellement
    est analysée à chaque clôture.
23. **News** : un flux de calendrier (sans valeur publiée), pas d'interprétation par modèle de
    langage (option de la spécification non implémentée, voir D30).
24. **Profil du desktop** : jeton dans un fichier privé, pas dans le trousseau du système.
25. **Montants en `f64`** : suffisant pour le contrôle du risque, pas pour une comptabilité
    officielle (le broker fait foi).
26. **Journal d'audit pendant une panne de base** : écrit dans le fichier journal seulement.
27. **Cerveau micro-profits** : rien n'a été validé sur données réelles. Avec un broker
    cher, la plupart des micro-cibles sont refusées (c'est voulu). Il a besoin d'un modèle de
    probabilité entraîné sur beaucoup d'historique (le réentraînement refuse en dessous d'une
    période de contrôle suffisante) ; sans modèle, il ne peut viser que des cibles plus
    proches que celle de la stratégie. Ses leçons ne distinguent pas deux comptes démo
    différents d'un même type de terminal. La comparaison des versions porte sur la justesse
    des probabilités (Brier), pas directement sur le profit.

28. **Latence broker** : mesurée par le connecteur 1.20 autour de `OrderSend` ; non vérifiée
    sur un vrai terminal (le simulateur mesure son propre traitement).
29. **Stop adaptatif** : sans modèle de scénario entraîné, un stop adapté n'a aucune
    probabilité validée et la transaction est refusée (voulu) ; le mode par défaut reste
    « stratégie ».
30. **Alertes externes** : webhook et Telegram reçoivent toutes les alertes (critiques et
    avertissements), sans filtre de niveau ; un échec d'envoi est journalisé, pas répété.
31. **Autonomy Commander** : une autorisation DEMO peut être conservée après un redémarrage
    (option), une autorisation LIVE jamais ; l'interface ne permet pas de modifier une
    autorisation existante (révoquer puis accorder).

## Ce qu'AEGIS ne fait pas, par principe

- Il ne promet aucune rentabilité ; un objectif quotidien n'est pas une garantie.
- Il n'augmente jamais la taille après une perte, ne force jamais une transaction pour
  atteindre l'objectif, n'abaisse jamais un seuil de probabilité de l'utilisateur.
- Il n'active jamais LIVE AUTO de lui-même et ne le restaure jamais après un redémarrage.
- Il n'utilise jamais un résultat ou un modèle obtenu sur données synthétiques avec un vrai
  broker.
- Il ne présente jamais le simulateur comme une connexion réelle.
