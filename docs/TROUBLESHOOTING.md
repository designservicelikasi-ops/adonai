# Dépannage

| Symptôme | Cause probable | Que faire |
|---|---|---|
| Le moteur refuse de démarrer : « configuration file is invalid » / « configuration rejected » | clé inconnue (faute de frappe) ou valeur hors bornes | lire le message : il nomme la clé ; `aegisd print-config` avec un fichier minimal pour comparer |
| « cannot listen on 127.0.0.1:7878 » | un autre moteur tourne déjà | arrêter l'autre (`aegisd`, service `AegisEngine`, moteur local du desktop) |
| Desktop : « Moteur injoignable — TRANSPORT … certificat inattendu » | le certificat du serveur a changé (dossier `tls/` supprimé, nouveau serveur) ou interception | refaire l'appairage : `aegisd pairing` sur le serveur, comparer l'empreinte, page *Connexion* |
| Desktop : « jeton refusé » | jeton changé (`api.token` supprimé, variable `AEGIS_API_TOKEN`) | refaire l'appairage |
| HTTP 429 | 10 échecs d'authentification en une minute | attendre une minute, corriger le jeton |
| Desktop : « moteur local introuvable » | application installée sans `aegisd` (compilation manuelle) | `BUILD_GUIDE.md` : copier `aegisd` dans `src-tauri/binaries/` avant `tauri build` |
| MT5 : « MT5 déconnecté » en permanence | adresse non autorisée dans MT5, mauvaise clé, pare-feu | *Outils → Options → Expert Advisors* ; `InpSharedKey` = `data/shared.key` ; page *Système* : « poignées de main refusées » et « dernier refus » |
| Toutes les entrées « REJETÉ — seuil_probabilite : aucune probabilité calibrée » | aucun modèle de probabilité ni preuve hors échantillon | entraîner un modèle (*Probabilités*) sur l'historique **broker** ; lancer des backtests |
| « modèle … entraîné sur données synthétiques : inutilisable avec un vrai broker » | modèle entraîné sur `.sim` | réentraîner sur données broker |
| « variables de marché indisponibles » | moins de 101 bougies sur l'unité de temps | attendre l'historique (demandé au terminal à la connexion) |
| Entrées refusées « exposition » sur M1/M5 | stops serrés ⇒ taille au risque très grande | `max_lots_per_trade` (plafond de lots) ou unité de temps plus longue |
| « couts_vs_gains » | les coûts du jour dépassent la part autorisée des gains bruts | normal : les micro-gains sont mangés par les coûts ; revoir commission, spread, cibles |
| « pertes_consecutives : pause jusqu'à … » | série de pertes | attendre la fin de la pause (réglable) |
| « actualites : trading suspendu » | annonce à fort impact sur une devise de l'instrument | attendre la fin de la fenêtre ; page *Actualités* |
| « calendrier économique indisponible » | pas d'accès réseau à la source | entrées en « prudence » (ou suspendues selon `suspend_when_unavailable`) ; vérifier le réseau ou désactiver `news` |
| Ordre au statut `unknown` | aucune réponse du connecteur | rien à faire : la réconciliation tranche avec les positions/transactions du broker ; nouvelles entrées bloquées sur l'instrument d'ici là |
| « base de données indisponible » | disque plein, fichier verrouillé | libérer de l'espace ; le moteur réessaie toutes les 3 s ; les entrées restent bloquées |
| Arrêt d'urgence actif après redémarrage | il est volontairement conservé | *Risque & configuration* → réinitialiser avec `RESET AEGIS` |
| `cargo test` échoue au premier lancement sous macOS sur un fichier du dépôt | comportement de macOS 27 | relancer ; les tests du dépôt embarquent leurs fichiers à la compilation |

Journaux : `<data_dir>/logs/` (rotation quotidienne), page *Système* (journal et audit),
`journalctl -u aegisd` (Linux), Observateur d'événements (service Windows).
