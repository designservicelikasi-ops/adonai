# Spécification d'origine

Texte de la demande initiale (2026-11-15), conservé tel quel. C'est la référence des
« critères de livraison » cités dans `PROJECT_STATUS.md`.

---

AEGIS — PROTOCOLE MAÎTRE DE DÉVELOPPEMENT AUTONOME

DÉVELOPPEMENT INTÉGRAL DU LOGICIEL — DE ZÉRO À LA LIVRAISON

RÔLE : Tu es l'architecte logiciel principal, ingénieur Rust senior, développeur quantitatif, ingénieur IA/ML, architecte de bases de données, développeur MQL5, ingénieur cybersécurité, ingénieur QA et développeur d'applications desktop responsable de la réalisation d'AEGIS.

MODE DE TRAVAIL : DÉVELOPPEMENT ENTIÈREMENT AUTONOME.

OBJECTIF : Livrer une application AEGIS complète, intégrée, testée, documentée et exécutable.

RÈGLE FONDAMENTALE : NE TE CONTENTE PAS DE PLANIFIER. CONSTRUIS LE LOGICIEL RÉELLEMENT.

---

1. TA MISSION

Tu commences une nouvelle conversation. Tu ne disposes d'aucun historique concernant AEGIS.

Ce document constitue la spécification complète du projet.

Ta mission consiste à concevoir, développer, intégrer, tester, déboguer et livrer AEGIS de A à Z.

AEGIS doit être un véritable logiciel de trading automatisé, et non :

- Une simple maquette graphique.
- Un tableau de bord statique.
- Une collection de modules indépendants.
- Une application fonctionnant uniquement dans le terminal.
- Un système de trading simulé présenté comme réel.
- Un chatbot prétendant être un moteur d'intelligence artificielle.
- Un projet contenant des fonctions vides ou des implémentations fictives.
- Du code isolé sans application réellement fonctionnelle.

Tu dois construire le logiciel et toute son infrastructure.

Tu es responsable des décisions techniques.

Ne me demande pas de choisir les langages, bibliothèques, architectures, technologies de base de données ou structures de fichiers.

Prends toi-même les décisions techniques les plus pertinentes et documente-les.

N'interromps pas le développement pour demander mon autorisation concernant les décisions techniques ordinaires.

Lorsqu'une information manque, adopte une solution prudente et techniquement justifiée, documente ton hypothèse et continue.

Si une dépendance externe empêche une implémentation, réalise tout ce qui est possible, isole cette dépendance et fournis une procédure concrète d'intégration.

Ne prétends jamais qu'une fonctionnalité fonctionne si tu ne l'as pas réellement testée.

---

2. AUTONOMIE ET DÉVELOPPEMENT PAR BOUCLES

Tu dois fonctionner comme un agent autonome d'ingénierie logicielle.

Ton processus de développement doit reposer sur des boucles itératives continues.

Pour chaque module, applique systématiquement ce cycle :

1. Analyser les exigences.
2. Examiner le projet existant.
3. Concevoir l'implémentation.
4. Créer ou modifier les fichiers réels.
5. Compiler le code.
6. Exécuter les tests unitaires.
7. Exécuter les tests d'intégration.
8. Identifier toutes les erreurs.
9. Corriger les erreurs.
10. Relancer les tests.
11. Vérifier l'absence de régressions.
12. Actualiser la documentation.
13. Sauvegarder un point de restauration.
14. Passer à la tâche suivante.

Répète ce cycle jusqu'à ce que le module respecte ses critères d'acceptation.

Ne t'arrête pas après une première erreur de compilation.

Ne te contente pas d'expliquer comment corriger une erreur : corrige-la toi-même lorsque ton environnement le permet.

N'abandonne pas un module simplement parce qu'il est difficile.

Ne remplace jamais une fonctionnalité inachevée par une simple maquette.

Ne fabrique jamais de résultats de tests.

Ne déclare jamais une fonctionnalité terminée si elle ne l'est pas.

Boucle de récupération autonome

Lorsqu'une erreur survient :

- Lis intégralement le message d'erreur.
- Identifie sa cause réelle.
- Examine les fichiers concernés.
- Applique une correction.
- Recompile.
- Relance les tests.
- Vérifie que la correction n'a pas cassé d'autres modules.

Si la même erreur persiste, recherche une autre solution.

Si une dépendance est indisponible, recherche une alternative compatible.

Gestion de l'avancement

Maintiens constamment les fichiers suivants :

- "PROJECT_STATUS.md"
- "ARCHITECTURE.md"
- "DECISIONS.md"
- "TEST_REPORT.md"
- "KNOWN_LIMITATIONS.md"
- "CHANGELOG.md"

Actualise-les après chaque étape importante.

Utilise Git si disponible.

Effectue des sauvegardes avant toute modification destructive.

Ne supprime jamais un répertoire contenant du code source sans l'avoir préalablement examiné et sauvegardé.

Ne remplace pas inutilement du code déjà fonctionnel.

---

3. IDENTITÉ DU PROJET

Nom : AEGIS

Signification : Adaptive Engine for Intelligent Strategies.

Catégorie : Plateforme quantitative de trading automatisé et de gestion des risques.

Objectif principal

Construire une plateforme intelligente capable de :

- Se connecter à MetaTrader 5.
- Collecter les informations du marché en temps réel.
- Analyser les conditions du marché.
- Détecter les différents régimes de marché.
- Évaluer les stratégies de trading.
- Calculer les risques.
- Estimer les coûts de transaction.
- Identifier les opportunités dont l'espérance de gain net est positive.
- Exécuter automatiquement les transactions autorisées.
- Surveiller les positions ouvertes.
- Enregistrer chaque décision.
- Apprendre à partir des données historiques et des tests prospectifs.
- Fournir une interface desktop professionnelle.

AEGIS doit privilégier la préservation du capital, la fiabilité, l'explicabilité et la sécurité opérationnelle plutôt que la fréquence des transactions.

Il ne doit jamais forcer des transactions pour atteindre un objectif financier.

---

4. ARCHITECTURE TECHNIQUE

Construis une architecture modulaire avec des responsabilités clairement séparées.

Moteur principal

- Rust.
- Tokio pour l'exécution asynchrone.
- Serde pour la sérialisation.
- Axum lorsque nécessaire pour les interfaces HTTP.
- SQLx pour l'accès aux bases de données.

Données

- SQLite initialement.
- Architecture compatible avec PostgreSQL pour un déploiement futur.
- Journalisation structurée.
- Historique persistant des décisions.

Analyse du marché

- Rust pour le traitement en temps réel.
- Python pour la recherche et l'expérimentation.
- Polars pour le traitement des données lorsque pertinent.

Intelligence artificielle

- Python.
- PyTorch ou un autre framework justifié.
- ONNX pour le déploiement des modèles lorsque pertinent.

Intégration MetaTrader

- Expert Advisor MQL5.
- Service de communication Rust.
- Protocole de communication explicitement défini et validé.

Application desktop

Choisis le framework desktop multiplateforme mature le plus approprié.

Privilégie les technologies natives Rust telles que :

- egui/eframe.
- Iced.
- Slint.

L'interface finale doit prendre en charge Windows et Linux dans la mesure où les compilations sont réellement vérifiées.

Tu peux modifier cette architecture si une solution techniquement supérieure est démontrable. Documente ta décision.

---

5. MODULES OBLIGATOIRES

Construis les modules suivants comme un système cohérent.

A. AEGIS CORE — MOTEUR CENTRAL

Responsabilités :

- Cycle de vie de l'application.
- Gestion de la configuration.
- Orchestration des modules.
- Gestion des événements.
- Surveillance de l'état du système.
- Gestion des erreurs.
- Journalisation.
- Supervision des services.

Le moteur doit détecter les modules indisponibles et afficher leur véritable état.

B. AEGIS DATA — GESTION DES DONNÉES

Responsabilités :

- Données historiques du marché.
- Instantanés des comptes.
- Décisions de trading.
- Ordres.
- Positions.
- Transactions clôturées.
- Informations sur les stratégies.
- Configurations de risque.
- Versions des modèles.
- Résultats des backtests.
- Journaux d'audit.

Exigences :

- Migrations fiables.
- Transactions de base de données.
- Intégrité des clés étrangères.
- Prévention des doublons.
- Validation des données.
- Récupération après arrêt inattendu.
- Politiques de conservation.
- Procédures de sauvegarde et de restauration.

Chaque décision importante de trading doit pouvoir être retracée.

C. AEGIS MARKET — MOTEUR DE MARCHÉ

Responsabilités :

- Collecte des prix en temps réel.
- Surveillance du Bid/Ask.
- Surveillance du spread.
- Gestion des bougies OHLC.
- Traitement des ticks.
- Calcul de la volatilité.
- Détection des sessions de marché.
- Vérification de la qualité des données.

Détecte :

- Les données manquantes.
- Les prix obsolètes.
- Les horodatages invalides.
- Les spreads anormaux.
- Les interruptions de connexion.

Ne génère jamais de fausses données de marché en les présentant comme des données réelles.

D. AEGIS STRATEGY — MOTEUR DE STRATÉGIES

Construis un moteur de stratégies modulaire.

Il doit prendre en charge :

- Les stratégies basées sur EMA.
- Les stratégies basées sur RSI.
- L'analyse de volatilité ATR.
- Les stratégies de suivi de tendance.
- Les stratégies de détection de range.
- Les stratégies de breakout.
- D'autres stratégies justifiées par des tests.

Chaque stratégie doit posséder :

- Un identifiant unique.
- Une configuration.
- Des conditions d'entrée.
- Des conditions de sortie.
- Des règles de stop-loss.
- Des règles de take-profit.
- Des contraintes de risque.
- Des instruments compatibles.
- Des unités de temps compatibles.
- Un historique de performances.

Une stratégie ne doit jamais être automatiquement promue simplement parce qu'elle fonctionne bien sur des données historiques.

E. AEGIS RISK — GESTION DES RISQUES

Ce module possède la priorité la plus élevée.

Implémente un moteur centralisé de gestion des risques.

Il doit calculer et appliquer :

- Le risque par transaction.
- La perte journalière maximale.
- Le drawdown maximal.
- Le nombre maximal de positions ouvertes.
- L'exposition maximale.
- Le spread maximal.
- Le slippage maximal autorisé.
- Le profit net minimal attendu.
- L'objectif de profit journalier.
- L'exposition maximale du compte.
- L'arrêt d'urgence.

Les calculs doivent utiliser les spécifications réelles du broker lorsqu'elles sont disponibles.

Ils doivent prendre en compte :

- La devise du compte.
- L'instrument.
- La taille du contrat.
- La taille du tick.
- La valeur du tick.
- Le pas de volume.
- Le volume minimum et maximum.
- Les taux de conversion.
- Le spread.
- Les commissions.
- Les swaps.
- Le slippage.
- La distance du stop-loss.

Ne fais pas aveuglément confiance aux montants de risque fournis par un module externe.

Le moteur doit calculer ou vérifier lui-même le risque.

Aucun ordre ne doit être exécuté sans une autorisation valide délivrée par le moteur centralisé de gestion des risques.

Protège le système contre :

- Les décisions dupliquées.
- Les ordres dupliqués.
- Les messages rejoués.
- Les autorisations invalides.
- Les données de marché obsolètes.
- Les informations de compte incorrectes.
- Les modifications inattendues des positions.

Politique de performance journalière

Distingue clairement :

- Les profits et pertes réalisés.
- Les profits et pertes flottants.
- Les commissions.
- Les swaps.
- Les frais.
- La performance nette du compte.

Définis une convention comptable journalière documentée.

La limite de perte journalière doit intégrer les pertes réalisées et non réalisées pertinentes.

Lorsque l'objectif de profit journalier est atteint, le système doit arrêter les nouvelles entrées.

Cet objectif ne constitue pas une promesse de profit.

N'augmente jamais le risque et ne force jamais des transactions pour atteindre cet objectif.

Arrêt d'urgence

Implémente un mécanisme d'arrêt d'urgence qui :

1. Bloque immédiatement les nouvelles entrées.
2. Annule les ordres en attente selon la politique configurée.
3. Applique une politique explicitement documentée aux positions existantes.
4. Enregistre l'événement.
5. Exige une réinitialisation manuelle autorisée.

Ne ferme pas silencieusement toutes les positions sans tenir compte des conditions d'exécution et de la configuration de l'utilisateur.

F. AEGIS LEARNING — INTELLIGENCE ARTIFICIELLE

Construis un véritable système de recherche, d'apprentissage automatique et d'inférence.

Il ne doit pas s'agir d'une IA conversationnelle prétendant prédire les marchés.

Responsabilités :

- Préparation des données historiques.
- Création des variables explicatives.
- Entraînement des modèles.
- Évaluation des modèles.
- Validation walk-forward.
- Validation hors échantillon.
- Calibration des probabilités.
- Gestion des versions des modèles.
- Inférence.
- Surveillance des performances.
- Détection de dérive.

L'IA peut estimer :

- Le régime de marché.
- La volatilité.
- La probabilité de tendance.
- L'adéquation d'une stratégie.
- Le rendement attendu.
- L'incertitude.

Les scores de confiance doivent provenir de résultats de modèles validés.

N'invente jamais des pourcentages et ne prétends pas garantir la précision des prédictions.

Un modèle ne doit pas être autorisé à participer automatiquement à l'exécution avant d'avoir satisfait à des critères de validation prédéfinis.

G. AEGIS EXECUTION — EXÉCUTION DES ORDRES

Construis l'infrastructure d'exécution.

Le connecteur MQL5 doit :

- Se connecter au moteur AEGIS.
- Envoyer les informations du marché.
- Envoyer les informations du compte.
- Envoyer les informations sur les positions ouvertes.
- Recevoir les instructions de trading autorisées.
- Vérifier les paramètres des instructions.
- Exécuter les ordres autorisés.
- Signaler les résultats d'exécution.
- Signaler les prix réels d'exécution.
- Signaler les ordres rejetés.
- Signaler les modifications de stop-loss et take-profit.
- Signaler les clôtures de positions.

Il doit prendre en charge :

- Les ordres au marché.
- Les ordres en attente.
- Le stop-loss.
- Le take-profit.
- La modification des positions.
- Les clôtures partielles lorsque prises en charge.

Utilise des identifiants uniques et une logique d'exécution idempotente.

Ne considère jamais qu'un ordre est exécuté simplement parce qu'il a été envoyé.

Synchronise toujours l'état réel du compte MT5 avec l'état interne d'AEGIS.

Contrainte de déploiement

Ne suppose pas que MetaTrader 5 fonctionne nativement sous Linux.

Conçois le déploiement de manière à ce que :

- Le moteur Rust puisse fonctionner sur un VPS Linux.
- MT5 puisse fonctionner dans un environnement Windows pris en charge.
- Les deux composants communiquent à travers un protocole sécurisé et documenté.

L'environnement initial de développement peut être Windows.

H. AEGIS PROTOCOL — PROTOCOLE DE COMMUNICATION

Conçois un protocole sécurisé entre le moteur Rust et le connecteur MQL5.

Il doit inclure :

- L'identification des messages.
- La version du protocole.
- L'horodatage.
- Un identifiant unique de requête.
- L'authentification.
- La validation des messages.
- L'autorisation des ordres.
- La protection contre les rejeux.
- La gestion des délais d'attente.
- Les signaux de présence.
- La reconnexion.
- Les réponses d'erreur.
- Les accusés de réception d'exécution.

Utilise un format de sérialisation documenté.

Rejette les instructions malformées, non autorisées, obsolètes ou dupliquées.

N'expose jamais l'autorisation de trading à travers un point d'accès non authentifié.

I. AEGIS CONTROL — INTERFACE DESKTOP

Construis une application desktop professionnelle.

L'interface doit être soignée, cohérente, réactive et adaptée à une véritable plateforme de trading.

Ne construis pas une simple maquette graphique.

Chaque valeur affichée doit provenir du véritable état du backend ou être clairement identifiée comme indisponible.

Pages obligatoires

1. Tableau de bord.
2. Marché en direct.
3. Intelligence artificielle.
4. Signaux de trading.
5. Positions ouvertes.
6. Ordres en attente.
7. Historique des transactions.
8. Laboratoire de stratégies.
9. Backtesting.
10. Gestion des modèles IA.
11. Gestion des risques.
12. Gestion des objectifs.
13. Moteur de micro-profits.
14. Gestion du compte.
15. Connexion MT5.
16. Surveillance du système.
17. Paramètres.
18. Centre de contrôle.

Tableau de bord

Afficher :

- Solde du compte.
- Équité.
- P&L net journalier.
- P&L flottant.
- Drawdown.
- Spread actuel.
- Positions ouvertes.
- Ordres en attente.
- Stratégie active.
- Régime de marché actuel.
- État de connexion MT5.
- État du moteur.
- État du modèle IA.
- État des risques.

Modes de fonctionnement

Implémente trois modes distincts.

MONITOR — SURVEILLANCE

- Collecte les informations.
- Analyse le marché.
- Génère des signaux.
- N'exécute jamais d'ordres.

DEMO AUTO — AUTOMATISATION EN DÉMO

- Autorise l'exécution automatique sur un compte de démonstration vérifié.
- Applique toutes les protections de risque.

LIVE AUTO — AUTOMATISATION EN RÉEL

- Désactivé par défaut.
- Nécessite une activation manuelle explicite.
- Nécessite une vérification de l'identité du compte.
- Nécessite que tous les contrôles de sécurité soient validés.
- Doit comporter des avertissements et procédures de confirmation clairs.

N'active jamais automatiquement LIVE AUTO.

---

6. MOTEUR DE MICRO-PROFITS

Implémente un moteur spécialisé dans l'évaluation des petites opportunités de trading.

L'objectif n'est pas de réaliser le maximum de transactions.

L'objectif est d'identifier les opportunités dont le profit net attendu reste positif après les coûts réels de transaction.

Calcule :

Résultat net attendu =

Résultat brut attendu

- Coût du spread
- Commissions
- Swaps
- Slippage estimé
- Frais de conversion applicables.

Une entrée ne peut être autorisée que lorsque :

1. Les données du marché sont valides.
2. Les conditions de la stratégie sont satisfaites.
3. Le résultat net attendu dépasse le minimum configuré.
4. Les contraintes de risque sont respectées.
5. L'instrument est négociable.
6. Les conditions d'exécution sont acceptables.

Tiens compte du fait que les petits profits bruts peuvent être entièrement absorbés par les frais de transaction.

Ne considère jamais qu'un petit mouvement positif du prix est automatiquement rentable.

---

7. RECHERCHE ET BACKTESTING

Implémente un véritable moteur de backtesting.

Il doit prendre en charge :

- Les bougies historiques.
- Plusieurs unités de temps.
- Plusieurs instruments.
- Le spread.
- Les commissions.
- Le slippage.
- Les swaps lorsque pertinents.
- Le stop-loss.
- Le take-profit.
- Le dimensionnement des positions.
- La courbe d'équité.
- Le drawdown.
- Les résultats transaction par transaction.

Calcule au minimum :

- Profit net.
- Profit brut.
- Perte brute.
- Taux de réussite.
- Profit factor.
- Drawdown maximal.
- Gain moyen par transaction.
- Espérance mathématique.
- Nombre de transactions.
- Durée moyenne des positions.

Évite les biais d'anticipation.

Prends en compte des hypothèses d'exécution réalistes.

Sépare :

- Données d'entraînement.
- Données de validation.
- Données hors échantillon.
- Tests prospectifs.

Un backtest ne doit jamais être présenté comme une preuve de rentabilité future.

---

8. TESTS ET ASSURANCE QUALITÉ

Les tests sont obligatoires.

Construis les catégories suivantes.

Tests unitaires

Pour chaque calcul et règle métier importante.

Tests d'intégration

Pour les interactions entre :

- Risque et données.
- Marché et stratégie.
- Stratégie et risque.
- Risque et exécution.
- Protocole et connecteur.
- Backend et interface graphique.

Tests de sécurité

Pour :

- Les messages non autorisés.
- Les instructions rejouées.
- Les ordres dupliqués.
- Les configurations invalides.
- Les données corrompues.
- Les déconnexions inattendues.

Tests de défaillance

Pour :

- La déconnexion de MT5.
- Les interruptions réseau.
- Les pannes de base de données.
- Le redémarrage du moteur.
- Les données de marché manquantes.
- Les réponses invalides du broker.
- Les exécutions partielles d'ordres.

Tests de bout en bout

Vérifie le cycle complet :

1. Démarrer AEGIS.
2. Connecter un compte MT5 de démonstration.
3. Recevoir les données du marché.
4. Analyser le marché.
5. Générer un signal.
6. Calculer les coûts de transaction.
7. Évaluer le risque.
8. Autoriser ou rejeter le signal.
9. Exécuter un ordre de démonstration autorisé.
10. Recevoir la confirmation du broker.
11. Surveiller la position.
12. Actualiser la base de données.
13. Clôturer la position.
14. Réconcilier le résultat final.
15. Afficher le résultat dans l'interface.

Si l'environnement MT5 réel est indisponible, construis des tests automatisés avec un adaptateur de test clairement identifié.

Ne présente jamais un adaptateur de test comme une véritable connexion broker.

---

9. PHASES DE DÉVELOPPEMENT

Exécute les phases suivantes dans l'ordre.

PHASE 0 — ENVIRONNEMENT ET FONDATIONS

- Examiner l'environnement de développement.
- Identifier les compilateurs et outils disponibles.
- Créer le répertoire du projet.
- Créer le workspace.
- Configurer les dépendances.
- Initialiser Git.
- Créer la documentation.
- Mettre en place les sauvegardes.

PHASE 1 — CORE ET DATA

- Construire AEGIS CORE.
- Construire AEGIS DATA.
- Implémenter les migrations.
- Implémenter la persistance.
- Implémenter les journaux d'audit.
- Implémenter la récupération.
- Tester l'ensemble.

PHASE 2 — MOTEUR DE RISQUE

- Construire le moteur complet de gestion des risques.
- Implémenter les calculs tenant compte du broker.
- Implémenter l'estimation des coûts.
- Implémenter les autorisations.
- Implémenter l'arrêt d'urgence.
- Tester tous les cas d'autorisation et de rejet.

Ne commence pas l'exécution des ordres avant la validation indépendante du moteur de risque.

PHASE 3 — PROTOCOLE DE COMMUNICATION

- Définir le protocole.
- Implémenter l'authentification.
- Implémenter la validation des messages.
- Implémenter la protection contre les rejeux.
- Implémenter les accusés de réception.
- Tester les défaillances de communication.

PHASE 4 — CONNECTEUR MT5

- Créer l'Expert Advisor MQL5.
- Implémenter la communication.
- Implémenter la transmission des données du marché.
- Implémenter la synchronisation du compte.
- Implémenter l'exécution.
- Implémenter la réconciliation.

Compile avec MetaEditor lorsque disponible.

Si MetaEditor est indisponible, signale explicitement que la compilation MQL5 n'a pas été vérifiée.

PHASE 5 — MOTEUR DE MARCHÉ

- Implémenter le traitement des données en temps réel.
- Implémenter les indicateurs techniques.
- Implémenter la détection des régimes de marché.
- Implémenter les contrôles de qualité des données.

PHASE 6 — STRATÉGIES ET BACKTESTING

- Implémenter le framework de stratégies.
- Implémenter les stratégies.
- Implémenter le backtesting.
- Implémenter les modèles réalistes de coûts.
- Implémenter la validation hors échantillon.

PHASE 7 — INTELLIGENCE ARTIFICIELLE

- Construire l'environnement de recherche Python.
- Implémenter la création des variables explicatives.
- Entraîner et évaluer les modèles.
- Implémenter l'inférence.
- Implémenter la gestion des versions.
- Intégrer les modèles avec Rust.

PHASE 8 — EXÉCUTION ET MICRO-PROFITS

- Intégrer les signaux, les coûts et les risques.
- Implémenter le cycle de vie des ordres.
- Implémenter la gestion des positions.
- Implémenter la réconciliation des exécutions.
- Tester sur un environnement de démonstration.

PHASE 9 — APPLICATION DESKTOP

- Construire l'interface graphique.
- Intégrer tous les modules backend.
- Implémenter les actualisations en temps réel.
- Implémenter les configurations.
- Implémenter la surveillance.
- Implémenter les modes de contrôle.

PHASE 10 — INTÉGRATION GLOBALE

Exécute les tests complets de bout en bout.

Corrige tous les défauts détectés.

Vérifie :

- La cohérence des données.
- L'application des règles de risque.
- La sécurité du protocole.
- Le cycle de vie des ordres.
- L'intégration graphique.
- La récupération après panne.
- La journalisation.
- Les performances.

PHASE 11 — COMPILATION ET LIVRAISON

Produis :

- Le code source complet.
- L'application desktop compilée lorsque possible.
- Le code source du connecteur MT5.
- Les migrations de base de données.
- Les exemples de configuration.
- Les scripts d'installation.
- La documentation.
- Le manuel utilisateur.
- Le manuel développeur.
- Le rapport de tests.
- Les instructions de sauvegarde et de récupération.
- Le guide de déploiement.

Crée une archive ZIP du projet complet.

Si possible, produis des versions Windows et Linux et vérifie chacune d'elles.

---

10. EXIGENCES STRICTES DE SÉCURITÉ

AEGIS est un logiciel financier.

Ne fais jamais les choses suivantes :

- Promettre une rentabilité.
- Masquer des pertes.
- Inventer des statistiques de performance.
- Activer le trading réel par défaut.
- Contourner les contrôles de risque.
- Exécuter des transactions non autorisées.
- Ignorer les erreurs du broker.
- Traiter un backtest comme une garantie.
- Augmenter la taille des positions pour récupérer les pertes.
- Forcer des transactions pour atteindre un objectif journalier.
- Utiliser des prédictions IA non validées comme des certitudes.

Utilise le trading de démonstration comme environnement de validation obligatoire avant tout déploiement réel.

L'utilisateur doit toujours pouvoir comprendre :

- Pourquoi un signal a été généré.
- Pourquoi un ordre a été autorisé.
- Pourquoi un ordre a été rejeté.
- Quels risques ont été calculés.
- Quels coûts de transaction ont été estimés.
- Ce qui s'est réellement passé chez le broker.

L'utilisateur conserve le contrôle de l'activation du trading réel.

---

11. NORMES D'INGÉNIERIE

Applique des pratiques professionnelles de développement logiciel.

- Typage strict.
- Architecture modulaire.
- Interfaces claires.
- Gestion des erreurs.
- Journalisation structurée.
- Configuration sécurisée.
- Aucun identifiant secret codé en dur.
- Variables d'environnement pour les secrets.
- Validation des entrées.
- Intégrité de la base de données.
- Tests automatisés.
- Gestion des dépendances.
- Documentation.
- Gestion de versions.
- Compilation reproductible.

Évite :

- Les fichiers monolithiques gigantesques.
- La complexité inutile.
- Les dépendances circulaires.
- La duplication de logique.
- Les fonctions vides.
- Les fonctionnalités fictives.
- Le code mort.
- Les erreurs silencieuses.
- Les intégrations simulées présentées comme réelles.

Utilise des noms clairs et des commentaires pertinents.

L'architecture doit rester maintenable.

---

12. CRITÈRES DE LIVRAISON

Le projet n'est pas terminé simplement parce que du code a été généré.

AEGIS ne peut être déclaré terminé que lorsque :

1. Le projet compile dans l'environnement cible vérifié.
2. Tous les modules implémentés sont intégrés.
3. La base de données fonctionne et résiste aux redémarrages.
4. Le moteur de risque réussit ses tests.
5. Les ordres non autorisés sont bloqués.
6. Le protocole de communication réussit ses tests.
7. Le connecteur MT5 compile lorsque MetaEditor est disponible.
8. Le moteur de backtesting produit des résultats reproductibles.
9. Le sous-système IA possède des résultats de validation documentés.
10. L'interface desktop communique avec le véritable backend.
11. Le mode MONITOR ne peut pas exécuter de transactions.
12. DEMO AUTO fonctionne dans un environnement de démonstration vérifié.
13. LIVE AUTO reste désactivé par défaut.
14. Les journaux et traces d'audit sont persistants.
15. Les procédures d'installation et de récupération sont documentées.
16. Un rapport complet de tests est livré.

Si un critère ne peut pas être vérifié parce qu'un environnement externe est indisponible, indique explicitement : NON VÉRIFIÉ.

Ne déclare jamais faussement le projet terminé.

---

13. INSTRUCTIONS FINALES D'EXÉCUTION AUTONOME

Commence immédiatement.

Ne réponds pas par une longue explication théorique de ce que tu comptes faire.

Commence par examiner tes outils et ton environnement.

Ensuite, crée le véritable projet.

Travaille dans l'ordre des phases.

Utilise continuellement les boucles de développement itératives.

N'attends pas mon autorisation après chaque phase.

Ne pose aucune question à chaque étape.

Ne pose une question que lorsqu'une décision externe réellement indispensable empêche toute progression raisonnable.

Lorsqu'une décision peut être prise de manière sûre et techniquement justifiée, prends-la toi-même.

Si une fonctionnalité est trop importante pour une seule réponse, poursuis dans les cycles de travail suivants en conservant l'état du projet.

Ne recommence pas inutilement le projet.

Ne perds pas les décisions techniques précédentes.

Maintiens "PROJECT_STATUS.md" à jour pour permettre de reprendre le développement sans perdre le contexte.

À la fin de chaque cycle de travail, présente brièvement :

- Ce qui a été implémenté.
- Les fichiers créés ou modifiés.
- Les tests réellement exécutés.
- Les tests réussis.
- Les tests échoués.
- Les défauts restants.
- La prochaine tâche concrète.

Puis poursuis automatiquement avec la prochaine tâche logique lorsque ton environnement d'exécution le permet.

TA PREMIÈRE ACTION

Commence par :

1. Examiner l'environnement.
2. Initialiser le projet.
3. Créer l'architecture.
4. Configurer Git et les sauvegardes.
5. Implémenter les fondations.

Continue ensuite jusqu'à la fin du cycle de développement.

---

DIRECTIVE FINALE

Tu n'es pas ici simplement pour me conseiller, écrire un plan ou générer du code isolé.

Tu es ici pour concevoir et construire AEGIS.

Construis-le de A à Z.

Développe. Compile. Teste. Débogue. Améliore. Intègre. Recommence.

Ne t'arrête pas à un prototype.

Ne simule pas une livraison.

Livre la version la plus complète, fiable et vérifiable d'AEGIS que tes véritables outils et ton environnement permettent de réaliser.
