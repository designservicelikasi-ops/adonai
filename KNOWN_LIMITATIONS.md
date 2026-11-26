# Limites connues (0.2)

Cette liste est volontairement complète. Rien ici n'est caché ailleurs.

## Non vérifié (à faire avant toute utilisation avec un broker)

1. **Le connecteur MQL5 n'a jamais été compilé** (pas de MetaEditor sous macOS). Le job CI
   `mql5` le compile sur un runner Windows, mais n'a jamais été exécuté (pas de dépôt distant).
2. **Aucun essai sur un vrai terminal MT5.** Tout le cycle a été validé contre le simulateur,
   qui n'est pas un broker. Latence et comportement réels du broker non mesurés.
3. **Windows et Linux non compilés localement.** Le pipeline CI (runners natifs) construit le
   moteur, les installateurs (.exe, .msi, .deb, .rpm, AppImage), installe et désinstalle
   l'application — **aucun run observé**. Le dossier `RELEASES/` ne contient donc que des
   fichiers macOS et les sources du connecteur.
4. **Service Windows** (`windows-service`) et scripts PowerShell : jamais exécutés.
5. **Fenêtre native du desktop** : lancée sous macOS, mais pas photographiée (session
   verrouillée) ; les pages ont été rendues et vérifiées dans Chrome avec les vraies réponses du
   moteur (mode aperçu).
6. **Aucune stratégie ni aucun modèle évalué sur des données de marché réelles.**

## Limites de conception

7. **Liaison MT5 non chiffrée** (authentifiée, anti-rejeu) : boucle locale ou tunnel.
8. **Un terminal, un compte à la fois.**
9. **Comptes netting** : depuis 0.3 le mode est transmis (protocole 1.2) et AEGIS n'empile ni
   ne compense jamais de position sur un compte netting ; testé contre le simulateur en mode
   netting, **pas contre un vrai compte netting**. Le connecteur 1.20 n'a pas été compilé.
10. **Commission non fournie par MT5** : à saisir (une valeur fausse fausse les coûts).
11. **Swaps** convertis seulement s'ils sont en points ou en devise du compte.
12. **En MONITOR, aucune clôture** ne part (même manuelle).
13. **Arrêt d'urgence en MONITOR ou hors connexion** : annulations/clôtures « en attente ».
14. **Ordres en attente** : gérés partout, mais les 9 stratégies n'émettent que des ordres au
    marché.
15. **Journée comptable = UTC** ; sessions de marché en heures UTC fixes.
16. **Bougies H4/D1** non demandées au terminal.
17. **Coût du spread** : estimé à la décision (le broker l'inclut dans le résultat brut) ;
    le slippage est mesuré à l'exécution (entrées), pas à la sortie.
18. **Objectif quotidien mesuré sur le réalisé** (le flottant n'est pas compté).
19. **Exposition par devise** : brute (sans compensation entre positions), donc prudente.
20. **Probability Engine** : un seul horizon de temps par modèle ; grille de scénarios
    bornée (un scénario hors grille n'a pas de probabilité) ; régression logistique (pas de
    modèle non linéaire).
21. **Shadow** : forward papier à 1 lot normalisé, sans slippage ; positions papier perdues au
    redémarrage (les statistiques sont conservées) ; une transaction clôturée partiellement
    est analysée à chaque clôture.
22. **News** : un flux de calendrier (sans valeur publiée), pas d'interprétation par modèle de
    langage (option de la spécification non implémentée, voir D30).
23. **Profil du desktop** : jeton dans un fichier privé, pas dans le trousseau du système.
24. **Montants en `f64`** : suffisant pour le contrôle du risque, pas pour une comptabilité
    officielle (le broker fait foi).
25. **Journal d'audit pendant une panne de base** : écrit dans le fichier journal seulement.
26. **Cerveau micro-profits** : rien n'a été validé sur données réelles. Avec un broker
    cher, la plupart des micro-cibles sont refusées (c'est voulu). Il a besoin d'un modèle de
    probabilité entraîné sur beaucoup d'historique (le réentraînement refuse en dessous d'une
    période de contrôle suffisante) ; sans modèle, il ne peut viser que des cibles plus
    proches que celle de la stratégie. Ses leçons ne distinguent pas deux comptes démo
    différents d'un même type de terminal. La comparaison des versions porte sur la justesse
    des probabilités (Brier), pas directement sur le profit.

27. **Latence broker** : mesurée par le connecteur 1.20 autour de `OrderSend` ; non vérifiée
    sur un vrai terminal (le simulateur mesure son propre traitement).
28. **Stop adaptatif** : sans modèle de scénario entraîné, un stop adapté n'a aucune
    probabilité validée et la transaction est refusée (voulu) ; le mode par défaut reste
    « stratégie ».

## Ce qu'AEGIS ne fait pas, par principe

- Il ne promet aucune rentabilité ; un objectif quotidien n'est pas une garantie.
- Il n'augmente jamais la taille après une perte, ne force jamais une transaction pour
  atteindre l'objectif, n'abaisse jamais un seuil de probabilité de l'utilisateur.
- Il n'active jamais LIVE AUTO de lui-même et ne le restaure jamais après un redémarrage.
- Il n'utilise jamais un résultat ou un modèle obtenu sur données synthétiques avec un vrai
  broker.
- Il ne présente jamais le simulateur comme une connexion réelle.
