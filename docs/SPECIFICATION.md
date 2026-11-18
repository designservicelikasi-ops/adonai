# AEGIS — Spécification v2 (brief du 2026-11-18)

Ce fichier résume fidèlement le brief « Ultimate Autonomous Development & Production Master
Prompt » remis le 2026-11-18. Il remplace la spécification v1 (`SPECIFICATION_V1.md`, conservée)
comme référence. Les numéros de partie sont ceux du brief.

## I — Produit final, pas une maquette
Chaque fonction annoncée doit exister et être vérifiable. Une connexion simulée n'est pas une
connexion réelle ; une probabilité inventée n'est pas une intelligence. Ne jamais déclarer un
test réussi sans l'avoir exécuté ; ne jamais prétendre avoir testé MT5 avec un simulateur.
Quand un environnement manque : construire les tests possibles, documenter la limite, préparer
une procédure reproductible.

## II — Autonomie
Boucle audit → architecture → plan → code → compilation → tests → corrections → intégration →
validation → documentation → checkpoint. Ne pas demander les choix techniques. Ne pas détruire
l'existant. Ne jamais fabriquer un identifiant, une clé ou un résultat.

## III — Objectif économique : accumulation de micro-gains
- Nombreuses petites transactions (ex. +0,05 à +0,30 USD brut) quand l'espérance **nette** est
  favorable ; chaque entrée validée indépendamment.
- **Daily Profit Goal Manager** : objectif quotidien configurable (5, 10, 15, 20, 30, 50 USD…),
  en brut **ou** en net ; calcul du restant ; progression affichée ; arrêt des entrées selon la
  politique ; jamais une garantie, jamais une raison de forcer.
- Distinction stricte brut / net : Gross Profit, Gross Loss, Net Profit, Commission, Spread
  Cost, Slippage, Swap, Other Fees.
- Suivi court terme (transaction, heure, jour, coût moyen, efficacité des sorties), moyen terme
  (semaine, mois, par stratégie, par instrument, dégradation), long terme (cumul, drawdown
  historique, stabilité, capital, rendement ajusté au risque).

## IV — Volume élevé
Pas de limite basse arbitraire ; capacité étudiée et testée jusqu'à ~2 000 transactions/jour si
broker et conditions le permettent. Étudier débit, latences, limites MT5/broker (lots, pas,
stops level, freeze level, remplissage). Mesurer coût moyen, gain brut/net moyen, espérance,
win rate, profit factor, rejets, slippage, exécution, performance par heure et par régime.
Détecter quand commissions et spreads absorbent les micro-gains. Avant chaque ordre : gain brut
potentiel, perte potentielle, coût, espérance nette, P(TP avant SL), risques de slippage,
retournement, exécution. Ne jamais multiplier les transactions pour atteindre un objectif.

## V — Architecture
Rust (Tokio, Axum, Serde, SQLite/SQLx, tracing) ; desktop **Tauri 2 + React + TypeScript +
Tailwind** ; connecteur MQL5 ; communication TCP/TLS ou WebSocket/TLS ; Python pour
l'entraînement, Rust (ONNX si adapté) pour l'inférence ; Windows et Linux.
Trois composants : **AEGIS ENGINE** (serveur Rust autonome sur VPS), **AEGIS CONNECTOR** (EA
MQL5), **AEGIS DESKTOP** (Tauri). Le moteur continue si le dashboard est fermé.

## VI — Market Intelligence + News Intelligence
Bid, ask, spread, ticks, OHLC, volumes, ATR, volatilité, momentum, tendances, consolidations,
supports/résistances, breakouts, pullbacks, rejets, structure, régimes ; multi-instruments ;
H1, M15, M5, M1 ; événementiel ; données horodatées ; anciennes/manquantes/incohérentes
détectées. Module d'actualités asynchrone : événements, devises, importance, attendu, publié,
sources, horodatages ; recommandation autorisé / prudent / suspendu ; un LLM peut interpréter
mais n'exécute jamais ; rien de non vérifié présenté comme un fait.

## VII — Décision
Stratégies : EMA Crossover, RSI Momentum, Trend Following, Breakout, Pullback, Mean Reversion,
Support/Resistance Rejection, Volatility-Based. Comparaison par régime.
**Probability Engine** : P(TP avant SL) — pas une probabilité directionnelle — calculée,
testée, calibrée, comparée au réel, historisée (Brier, Log Loss, diagrammes de fiabilité,
walk-forward). Seuils BUY/SELL configurables (ex. 75 %) ; adaptatifs possibles dans une plage,
jamais sous le minimum de l'utilisateur.
Entry Intelligence : BUY / SELL / WAIT (WAIT est normal).
Position Intelligence : HOLD / SECURE / ADJUST SL / ADJUST TP / CLOSE ; jamais élargir un SL ;
sorties selon le résultat net réalisable.

## VIII — Risque et exécution
Moteur de risque indépendant : capital, risque/transaction, perte journalière max, drawdown max,
positions max, transactions max, pertes consécutives max, cooldown, spread max, exposition par
devise et globale, horaires, objectif quotidien. Aucune stratégie ne le contourne.
Exécution : création, validation, transmission, confirmation, rejet, partiel, modification,
clôture, annulation, réconciliation ; identifiants uniques, idempotence ; état inconnu ≠ non
exécuté ; le broker fait foi ; déconnexion ⇒ entrées bloquées jusqu'à réconciliation.

## IX — Apprentissage
**Shadow Intelligence** indépendant, sans pouvoir sur le LIVE : analyse gains/pertes, entrées
et sorties prématurées, opportunités manquées, autres paramètres, autres stratégies ; sans
look-ahead ; pipeline BACKTEST → OUT-OF-SAMPLE → FORWARD → DEMO → VALIDATION ;
Champion / Challenger ; aucun candidat non validé ne remplace le LIVE.
Learning Engine : Python pour la recherche, Rust pour l'inférence ; versions de jeux de données
et de modèles, reproductibilité, calibration, suivi, dérive, retour arrière.

## X — Desktop premium
Tauri 2 + React + TS, identité propre (terminal de trading sombre, institutionnel).
Dashboard : état moteur, connexion MT5, broker, mode, latence, instrument, prix, spread,
volatilité, BUY/SELL probability, WAIT, décision, position, flottant, profit du jour, objectif,
progression, perte du jour, drawdown, nombre de transactions, coûts, stratégies, actualités,
état Shadow. Écran performance (brut, net, gains, pertes, win rate, PF, expectancy, drawdown,
transactions/jour, moyennes, hebdo, mensuel, historique). Configuration complète sans code,
validée, enregistrée, journalisée. Logo, icônes Windows/Linux, écran de démarrage, états de
chargement, erreurs, notifications, états de connexion.

## XI — Windows et Linux (non négociable)
Windows : application x64, installateur `.exe`, `.msi` si possible, icône, métadonnées,
installation/désinstallation. Linux : AppImage, `.deb`, `.rpm` si vérifiable. Pipeline
reproductible (GitHub Actions, runners natifs) : dépendances, Rust, React, tests, Tauri,
installateurs, artefacts, logs, version. Distribution du moteur (binaire serveur, config,
installation, démarrage, arrêt, logs, surveillance, redémarrage). L'application démarre et
affiche un état compréhensible sans MT5 ; trading désactivé tant que la sécurité n'est pas
validée. Tester l'installation quand c'est possible, sinon CI + tests restants listés.

## XII — VPS
Windows VPS : MT5 + EA + moteur + base ; desktop local connecté de façon sécurisée.
Redémarrage automatique, heartbeat, reconnexion, logs, CPU/RAM, sauvegardes, récupération,
alertes, état des positions.

## XIII — Sécurité
TLS, authentification, sessions, anti-rejeu, HMAC, validation, secrets, idempotence,
réconciliation, kill switch, permissions, journal des actions critiques. MONITOR, DEMO AUTO,
LIVE AUTO (désactivé par défaut, jamais d'ordre réel sans autorisation explicite). Données
obsolètes, déconnexion, état inconnu, erreur critique, échec du risque, incohérence ⇒ entrées
bloquées.

## XIV — Tests
Unitaires, intégration, régression, propriétés, charge, injection de pannes, récupération ;
trading (historique, spread variable, commissions, slippage, swap, partiels, rejets,
reconnexions, simultanées, limites, objectifs, sorties) ; performance de bout en bout
(réception, calcul, décision, transmission, réponse MT5, débit, CPU, RAM) ; rentabilité
(backtest, hors échantillon, walk-forward, forward, DEMO) en **net**.

## XV — Documentation et livraison
README, ARCHITECTURE, PROJECT_STATUS, INSTALLATION_WINDOWS, INSTALLATION_LINUX, BUILD_GUIDE,
VPS_DEPLOYMENT, MT5_CONNECTOR, CONFIGURATION, SECURITY, TESTING, API_PROTOCOL,
MODEL_VALIDATION, TRADING_MODES, TROUBLESHOOTING, CHANGELOG. Dossier `RELEASES/` ne contenant
**que** des fichiers réellement produits.

## XVI — Phases
0 audit · 1 architecture · 2 core/serveur · 3 communication sécurisée · 4 MQL5 · 5 market ·
6 stratégies · 7 probabilités · 8 entrées · 9 positions · 10 risque · 11 exécution ·
12 micro-profits · 13 objectif quotidien · 14 actualités · 15 shadow · 16 apprentissage ·
17 base/historique · 18 desktop · 19 intégration · 20 backtests · 21 DEMO · 22 build Windows ·
23 build Linux · 24 VPS · 25 audit final.

## XVII — Acceptation
Application (démarre, professionnelle, configurable, données réelles ou explicitement simulées,
erreurs visibles) ; moteur (démarre, sans dashboard, événements, modules, persistance) ; MT5
(EA compile, connexion DEMO réelle, données, ordres, confirmations, réconciliation) ; trading
(BUY/SELL/WAIT, probabilités mesurées, coûts, blocages, positions, objectif, kill switch) ;
intelligence (shadow indépendant, reproductible, versionné, validé avant promotion) ;
multiplateforme (builds Windows/Linux réellement produits, installateurs, pipeline, procédure) ;
VPS (autonome, redémarrage, reconnexion, monitoring, sauvegarde, documentation).

## XVIII — Rapport final
Architecture réalisée, modules opérationnels / partiels / non réalisés, tests, compilation,
MT5, DEMO, builds Windows/Linux, livrables, performances mesurées, limites, problèmes,
installation, déploiement VPS — chaque résultat avec sa preuve. Ne jamais transformer un
résultat attendu en résultat obtenu.
