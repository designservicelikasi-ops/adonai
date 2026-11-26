# Décisions techniques

Chaque décision indique ce qui a été choisi, pourquoi, et ce que cela coûte.

## D1 — Rust pour tout le moteur, workspace de 13 crates
Typage strict, pas de ramasse-miettes, un binaire par cible. Chaque module de la spécification
est un crate : les dépendances interdites (par exemple l'exécution qui contournerait le risque)
deviennent des erreurs de compilation.

## D2 — SQLite via SQLx, requêtes à l'exécution
SQLite ne demande aucune installation. Les requêtes ne sont pas vérifiées à la compilation
(il faudrait une base présente pour compiler) ; en contrepartie, chaque requête est couverte
par des tests. Le SQL reste portable vers PostgreSQL.

## D3 — egui/eframe pour l'application de bureau (remplacée en 0.2 : voir D21)
Rust natif, un seul binaire, Windows/Linux/macOS. Le moteur tourne **dans le même processus**
que l'interface : pas de protocole supplémentaire à sécuriser entre les deux. Coût : pas
d'interface distante ; pour un VPS on utilise `aegisd` et son API HTTP.

## D4 — Protocole maison : JSON plat sur TCP, HMAC-SHA256
MQL5 sait ouvrir un socket client et calculer un SHA-256, mais n'a ni JSON ni HMAC natifs.
Un objet JSON **plat** se lit avec un analyseur d'une centaine de lignes ; le HMAC se construit
sur le SHA-256 du terminal. Le MAC porte sur la chaîne exacte envoyée : aucune
« canonicalisation » à faire des deux côtés. Clé de session dérivée de deux nonces, numéro de
séquence strict, fenêtre d'horodatage : pas de rejeu possible.
**Coût assumé : la liaison est authentifiée mais pas chiffrée.** Elle doit rester sur la même
machine ou dans un tunnel privé (voir `docs/DEPLOYMENT.md`).

## D5 — Le connecteur MT5 est le client, le moteur est le serveur
MT5 n'autorise que les connexions sortantes vers des adresses listées. Le moteur peut donc
vivre sur un VPS Linux pendant que MT5 tourne sous Windows.

## D6 — Démarrage toujours en MONITOR
Le mode n'est jamais enregistré. Après un redémarrage, une panne ou une réinitialisation de
l'arrêt d'urgence, l'automatisation doit être réactivée à la main.

## D7 — En MONITOR, aucune trame d'ordre ne sort, sans exception
Y compris les clôtures manuelles et le plan de l'arrêt d'urgence. C'est la seule façon de
garantir, par un test simple, que « MONITOR n'exécute jamais ». Coût : pour clôturer une
position depuis AEGIS il faut être en DEMO AUTO ou LIVE AUTO (ou le faire dans MT5).

## D8 — Probabilité de gain : preuve hors échantillon ou rien
L'espérance nette exige une probabilité. AEGIS n'en invente pas : elle vient du dernier
backtest hors échantillon (20 % finaux des données) de la stratégie sur cet instrument et
cette unité de temps, en prenant la **borne basse de Wilson** du taux de réussite (prudente
quand l'échantillon est petit), avec 30 transactions minimum. Sinon : entrée refusée.

## D9 — Le modèle IA filtre, il ne décide pas
Le modèle estime P(hausse sur N bougies). Ce n'est pas une probabilité de gain d'une
transaction avec stop et cible : l'utiliser comme telle serait malhonnête. Un modèle **validé**
peut seulement opposer son veto à un signal qui le contredit. Un modèle non validé est affiché
à titre indicatif et n'a aucun effet.

## D10 — Régression logistique calibrée plutôt que réseau de neurones
Peu de variables, données bruitées, besoin d'explicabilité et de parité exacte entre
l'entraînement (Python) et l'inférence (Rust). Le modèle s'exporte en quelques nombres ;
l'inférence Rust tient en vingt lignes et sa parité est vérifiée à chaque chargement
(vecteurs de test inclus dans l'artefact). PyTorch et ONNX n'apportaient rien ici et pesaient
lourd sur une machine de 8 Go ; le format d'artefact porte un champ `kind` pour en ajouter.

## D11 — Les variables explicatives sont calculées en Rust uniquement
Le moteur exporte le jeu de données ; Python ne recalcule aucune variable. Il ne peut donc pas
y avoir d'écart entre ce sur quoi le modèle a appris et ce qu'il reçoit en direct.

## D12 — Simulateur = adaptateur de test, étiqueté partout
`aegis-simterminal` parle le vrai protocole par le vrai réseau, ce qui teste réellement le
pont, l'exécution et la réconciliation. Il s'annonce `simulator`, ne traite que `EURUSD.sim`,
ses bougies sont stockées `synthetic`, et LIVE AUTO lui est interdit. Les données synthétiques
ne peuvent être générées que pour un instrument en `.sim`.

## D13 — Temps de marché accéléré dans le simulateur
Le simulateur fait avancer l'horloge « broker » de 15 s par tick pour que des bougies se
forment en quelques secondes. La comparaison horloge broker / horloge moteur est donc
désactivée pour lui (et pour lui seul).

## D14 — Fraîcheur jugée sur l'horloge du moteur
L'âge d'un prix se mesure depuis sa réception par le moteur, pas depuis l'heure du broker :
un décalage d'horloge ne peut ni faire passer un vieux prix pour frais, ni bloquer à tort.

## D15 — Journée comptable = journée UTC
Simple, sans ambiguïté de fuseau. Résultat du jour = équité actuelle − équité de début de
journée − dépôts/retraits. Voir `docs/RISK.md`.

## D16 — Arrêt d'urgence : positions conservées par défaut
Clôturer au marché pendant un incident peut coûter cher (spread élargi, slippage). Par défaut
l'arrêt bloque les entrées, annule les ordres en attente d'AEGIS et **laisse** les positions
avec leurs stops. La politique « clôturer » est configurable. Dans les deux cas, les positions
ouvertes hors d'AEGIS ne sont jamais touchées.

## D17 — Montants en `f64`
Les volumes sont toujours ramenés au pas du broker (arrondi vers le bas) et les prix arrondis
au nombre de décimales de l'instrument. Les écarts d'arrondi sont de l'ordre de 1e-9 sur des
montants affichés au centime. Un type décimal exact serait préférable pour une comptabilité
officielle ; ce n'est pas le rôle d'AEGIS (le broker fait foi).

## D18 — Bougies H4/D1 non servies par le connecteur
Les bougies du broker sont alignées sur l'heure serveur, pas sur UTC. Jusqu'à H1 l'alignement
est identique ; au-delà non. Le moteur sait agréger du H1 en H4/D1 UTC (`aggregate`), mais les
stratégies livrées travaillent de M5 à H1.

## D19 — Clé partagée : variable d'environnement, sinon fichier privé
`AEGIS_SHARED_KEY` a priorité. À défaut, une clé aléatoire de 256 bits est créée dans
`data/shared.key` avec les droits `0600`. Elle n'apparaît ni dans la configuration ni dans les
journaux ; l'application la montre masquée, avec un bouton pour la copier vers MT5.

## D20 — Versions de bibliothèques épinglées à des séries connues
eframe 0.31, SQLx 0.8, Axum 0.7, hmac 0.12, sha2 0.10, rand 0.8, toml 0.8, alors que des
versions plus récentes existent : des API connues et stables plutôt que les dernières sorties.
`Cargo.lock` est livré pour une compilation reproductible.

---

# Décisions de la version 0.2 (spécification v2, 2026-11-18)

## D21 — AEGIS Desktop en Tauri 2, client de l'API
L'application egui (même processus que le moteur) est remplacée par Tauri 2 + React +
TypeScript + Tailwind, qui ne contient pas le moteur : elle pilote `aegisd` (local ou VPS) par
l'API. Fermer la fenêtre n'arrête rien. L'installateur embarque `aegisd` (`externalBin`) ; le
« moteur local » est lancé en **processus détaché**, jamais comme enfant de la fenêtre.
Versions épinglées à des séries stables connues (React 18, Vite 6, TypeScript 5, Tailwind 3,
Lightweight Charts 4) plutôt qu'aux dernières majeures. Tauri 2 et non Tauri 3 (alpha).

## D22 — TLS sur l'API, certificat auto-signé épinglé
Un VPS n'a souvent pas de nom de domaine. Le moteur crée un certificat ECDSA au premier
démarrage ; le desktop épingle son empreinte SHA-256 (`aegis-client`, vérificateur rustls
dédié). Aucune autorité de certification n'est approuvée : un autre certificat est refusé avant
l'envoi du jeton. Les appels réseau sont faits côté Rust, jamais depuis la vue web.
`rustls` avec `ring` (pas d'`aws-lc`, qui exige CMake/NASM sous Windows).

## D23 — Liaison MT5 inchangée (HMAC, non chiffrée)
MQL5 sait faire du TLS mais pas épingler un certificat auto-signé de façon vérifiable sans
terminal. La liaison reste sur la boucle locale du VPS (ou un tunnel).

## D24 — P(TP avant SL) par modèle de scénario « triple barrière »
La probabilité utilisée est celle qu'un scénario précis (TP et SL en ATR) touche sa cible
d'abord — jamais une probabilité directionnelle. Étiquettes pessimistes (TP et SL dans la même
bougie = perte), régression logistique (parité facile, explicable, rapide sur 8 Go),
walk-forward purgé, calibration de Platt croisée. Le modèle complet doit battre un modèle de
référence « scénario seul » ; sinon la référence calibrée ; sinon rien. La probabilité retenue
est la plus prudente des sources validées.

## D25 — Objectif quotidien en devise du compte, brut ou net, sur les transactions clôturées
L'objectif ne modifie jamais la taille, les seuils ni la fréquence : il peut seulement arrêter
les entrées (`stop_entries`), les arrêter si le gain redescend (`protect_gains`) ou être suivi
(`track_only`). Mesuré sur le réalisé (le flottant n'est pas « gagné »). Défaut : 15, **net**.

## D26 — Shadow dans une tâche séparée, sans dépendance vers l'exécution
Le Shadow reçoit des copies (bougies, décisions, transactions) par un canal borné et produit
un rapport. Son crate ne dépend ni de l'exécution ni du protocole : il ne peut pas construire
d'ordre. Une promotion est une commande de l'utilisateur, vers la Démo uniquement.

## D27 — Plafond de lots pour le micro-trading
Avec des stops serrés (M1/M5), la taille « au risque » devient énorme et l'exposition bloque
tout. `max_lots_per_trade` plafonne la taille ; il ne peut que la réduire.

## D28 — Paramètres de stratégie modifiables, stratégie renvoyée en Recherche
Les preuves (backtests, forward) ont été obtenues avec d'autres paramètres : elles sont
retirées et la stratégie revient au stade Recherche. Les paramètres enregistrés survivent aux
redémarrages (en 0.1, ils étaient écrasés par les valeurs par défaut au démarrage).

## D29 — API : commandes génériques, chemins confinés, pas d'arrêt par le réseau
`POST /api/v1/command` accepte toute commande du moteur (même validation que l'interface).
Les chemins de fichiers sont relatifs à `<data_dir>/files/` ; `shutdown` est refusé (c'est le
rôle du gestionnaire de service).

## D30 — Calendrier économique public, jamais « calme » par défaut
Source par défaut : flux JSON public de calendrier (configurable, ou fichier local). Une valeur
publiée absente de la source reste vide. Un calendrier injoignable met en « prudence » (ou
suspend, au choix). Aucun modèle de langage n'est utilisé : cette option prévue par la
spécification n'a pas été implémentée (elle exigerait une clé d'API et n'apporterait rien de
vérifiable aux règles de fenêtre).

## D31 — Cerveau micro-profits : il choisit la cible, il ne touche pas au risque
Demande : viser des gains NETS de 0,08 à 2,00 par transaction sur EURUSD et s'améliorer avec
le temps. Le cerveau (`aegis-brain`, section `[brain]`, désactivé par défaut) garde le stop de
la stratégie et essaie, pour le volume du Risk Engine puis des volumes plus petits, des cibles
dont le gain net (coûts appris inclus) tombe dans la fourchette ; il retient la meilleure
espérance nette parmi celles qui passent les seuils du Risk Engine (probabilité minimale du
côté, espérance minimale). Il ne demande jamais plus de volume que le Risk Engine, qui
revérifie tout. La preuve de backtest d'une stratégie n'est utilisée que pour une cible au
plus égale à la sienne (pour un même stop, une cible plus proche est au moins aussi probable) ;
au-delà, seul le modèle de scénario compte. Sans probabilité validée : pas de transaction.

## D32 — Apprentissage uniquement dans le sens de la prudence
Les leçons sont recalculées depuis le journal (même type de terminal uniquement) : frais réels
par lot (seul un surcoût est ajouté ; un broker moins cher ne rend pas plus audacieux),
correction des probabilités = gains obtenus / gains promis, rétrécie vers 1 par
`prior_trades` et plafonnée à 1 (calculée sur la probabilité BRUTE, colonne `probability_raw`,
sinon la correction se corrigerait elle-même), contextes stratégie x sens x session bloqués
quand la moyenne + une erreur type reste négative après `context_min_trades` transactions.
Un contexte n'est jamais « favorisé » par une taille plus grande.

## D33 — Réentraînement automatique, activation par l'utilisateur
Choix de l'utilisateur (2026-11-21) : une nouvelle version n'est jamais activée seule, même en
démo. Toutes les `retrain_every_hours`, le moteur entraîne une version sur l'historique sans
sa partie la plus récente (`holdout_pct`), puis note la nouvelle version et la version active
sur cette période (Brier, couverture). La version active a pu voir ces bougies : la
comparaison la favorise, ce qui n'oblige que davantage la nouvelle. Meilleure : enregistrée
inactive, alerte, boutons « Activer » / « Écarter » (page Cerveau IA). Ce qui est activé est
exactement ce qui a été comparé.


---

# Décisions de la version 0.3 (directive « Master Upgrade », 2026-11-26)

## D34 — Mode du compte : champ facultatif, inconnu = netting
Le protocole garde `v = 1` ; `account.margin_mode` et `result.broker_us` sont facultatifs. Un
connecteur ancien ne casse rien, mais son mode inconnu est traité comme le plus restrictif
(netting) : mieux vaut refuser une seconde position que fusionner sans le savoir.

## D35 — Risque du portefeuille aux stops actuels, défaut « automatique »
La limite par défaut vaut `positions max x risque par transaction`, ce qui ne change rien aux
configurations existantes pour des positions protégées ; la nouveauté est qu'une position
**sans stop** (même ouverte à la main) bloque toute entrée : le risque n'est plus mesurable.

## D36 — Stop adaptatif sans emprunter la preuve de la stratégie
Un taux de réussite de backtest ne vaut que pour les stops avec lesquels il a été mesuré. Avec
un stop adapté, seule une probabilité de scénario (fonction de SL et TP) est acceptée. Le
mode par défaut reste « stratégie » pour ne pas bloquer silencieusement les configurations
actuelles.

## D37 — Garde de protection : réinstaller, sinon clôturer, jamais en MONITOR
Une position sans stop est le pire état possible. Le stop réinstallé est celui **prévu à la
décision** (jamais un autre inventé après coup) ; s'il est déjà dépassé, ou refusé deux fois,
la position est clôturée. En MONITOR rien ne part (D7) : alerte seulement.

## D38 — Le mode découle de l'autorisation
DEMO AUTO et LIVE AUTO n'existent plus que sous une autorisation du Commander ; une boucle
vérifie à chaque tour qu'un mode d'exécution correspond à une autorisation valable, sinon
MONITOR. `set_mode` est conservé et crée/révoque l'autorisation correspondante.

## D39 — Reprise après redémarrage : DÉMO seulement, sur demande
D6 (démarrage en MONITOR) est conservé. Sur un VPS, l'autonomie DÉMO doit pouvoir survivre à
un redémarrage du service : l'utilisateur peut marquer une autorisation DÉMO « persistante » ;
elle reprend quand le même compte se reconnecte. Jamais pour LIVE.

## D40 — Les actions de protection ne demandent pas de permission
Un permis manquant ne doit jamais empêcher de réinstaller un stop ou d'appliquer l'arrêt
d'urgence : ces actions réduisent le risque. Elles restent tracées.

## D41 — Profil par défaut RESEARCH AUTONOMY
Le cerveau réentraînait déjà automatiquement ses modèles en 0.2 ; OBSERVER l'aurait arrêté
sans que l'utilisateur l'ait demandé. Le défaut conserve ce comportement ; OBSERVER se choisit.

## D42 — Notification Telegram : jeton hors configuration
Le jeton d'un bot Telegram donne le contrôle du bot : il n'est jamais écrit dans
`aegis.toml`, la base ou les journaux. Le moteur le lit dans une variable d'environnement au
moment de l'envoi et n'affiche jamais l'URL (qui le contient) dans une erreur. Telegram reçoit
les mêmes alertes que le webhook (critiques et avertissements), en texte brut. L'envoi se fait
hors de la boucle du moteur : une panne de Telegram ne ralentit rien.
