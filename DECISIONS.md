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

## D3 — egui/eframe pour l'application de bureau
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
