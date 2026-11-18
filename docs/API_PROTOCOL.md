# API et protocoles

AEGIS a deux liaisons réseau :

1. **Connecteur MT5 → moteur** : protocole maison sur TCP, une trame JSON par ligne, signée
   HMAC-SHA256. Spécification complète : [`MT5_PROTOCOL_V1.md`](MT5_PROTOCOL_V1.md). Version 1.1
   (compatible) : `deal.fee`, `spec.currency_base`, `spec.freeze_level` facultatifs.
2. **AEGIS Desktop (ou tout client) → moteur** : API HTTP(S) décrite ici.

## Connexion

- Adresse par défaut : `https://127.0.0.1:7879` (section `[api]`).
- Certificat auto-signé créé au premier démarrage (`<data_dir>/tls/`). Le client **doit**
  vérifier son empreinte SHA-256 (`aegisd pairing` l'affiche). La crate `aegis-client` le fait.
- En-tête obligatoire sauf pour `/api/v1/health` : `Authorization: Bearer <jeton>`.
- 10 échecs d'authentification en une minute depuis une adresse ⇒ `429` pendant une minute.

## Lecture

| Méthode et chemin | Réponse |
|---|---|
| `GET /api/v1/health` | `{"status":"ok"\|"stalled","version","view_age_ms","mode"}` (503 si l'état publié a plus de 5 s) |
| `GET /api/v1/summary` | état compact pour un tableau de bord (≈ 1 requête/s) : mode, connexion, compte, marchés, positions, décisions de positions, BUY/SELL/WAIT, objectif, journée, drawdown, arrêt d'urgence, santé, alertes, configuration de risque, 20 dernières décisions, 40 dernières lignes de journal, performance (jour, 7 jours, total), actualités, Shadow (synthèse), stratégies, CPU/RAM |
| `GET /api/v1/state` | état complet (`EngineView`, volumineux) |
| `GET /api/v1/view/<section>` | une section de l'état complet : `decisions`, `orders`, `commands`, `deals`, `journal`, `performance`, `strategies`, `probability_models`, `probability_live`, `shadow`, `news`, `charts`, `logs`, `audit`, `data`, `backtests`, `last_backtest`, `risk_config`, `markets`, `specs`, `positions`… (`404` si inconnue) |
| `GET /api/v1/system` | CPU et mémoire du processus et de la machine |

## Commandes

`POST /api/v1/command` avec `{"type": "<commande>", "args": {...}}` (sans `args` pour les
commandes sans paramètre). Réponse `200 {"ok":true,"message":…}` ou `409 {"ok":false,"error":…}`
(refus du moteur), `400` (JSON invalide), `403` (interdit sur le réseau).

| `type` | `args` | Remarque |
|---|---|---|
| `set_mode` | `{"mode":"monitor"\|"demo_auto"\|"live_auto","confirmation":""}` | LIVE : `"<compte> J'ACCEPTE LE RISQUE REEL"` |
| `kill` | `{"reason":""}` | arrêt d'urgence |
| `reset_kill` | `{"phrase":"RESET AEGIS"}` | le moteur reste en MONITOR |
| `set_risk_config` | objet `RiskConfig` complet | validé, versionné, audité |
| `set_strategy_enabled` | `{"id","enabled"}` | |
| `set_strategy_stage` | `{"id","stage":"research"\|"demo"\|"live_approved"\|"disabled"}` | soumis aux preuves |
| `set_strategy_config` | `{"id","config":{…}}` | stratégie renvoyée en Recherche |
| `run_backtest` | `{"strategy_id","symbol","timeframe","origin":"broker"\|"synthetic","params"?}` | |
| `train_probability` | `{"symbol","timeframe","origin"}` | nouvelle version, en arrière-plan |
| `activate_probability_model` | `{"model_id"}` | retour arrière |
| `evaluate_shadow_history` | `{"symbol","timeframe","origin"}` | backtest de tous les candidats |
| `promote_challenger` | `{"candidate_id"}` | vers la Démo uniquement, si validé |
| `close_position` | `{"ticket","volume":null}` | refusé en MONITOR |
| `modify_position` | `{"ticket","sl","tp"}` | stop jamais élargi |
| `cancel_order` | `{"ticket"}` | |
| `generate_synthetic` | `{"symbol":"X.sim","timeframe","bars","seed"}` | instruments `.sim` seulement |
| `import_csv` | `{"path","symbol","timeframe","utc_offset_hours"}` | `path` relatif à `<data_dir>/files/` |
| `export_dataset` | `{"symbol","timeframe","origin","horizon","path"}` | idem |
| `load_model` | `{"path"}` | idem |
| `backup`, `verify_audit` | — | |
| `shutdown` | — | **403** : réservé au gestionnaire de service |

Routes historiques conservées : `POST /api/v1/mode`, `/api/v1/kill`, `/api/v1/kill/reset`.

## Exemple (curl, en épinglant l'empreinte)

`curl` ne sait pas épingler une empreinte SHA-256 de certificat ; `--pinnedpubkey` épingle la
clé publique. Pour un essai local : `curl -k` suivi d'une vérification manuelle de
l'empreinte avec `openssl s_client -connect 127.0.0.1:7879 | openssl x509 -noout -fingerprint -sha256`.

```sh
TOKEN=$(cat data/api.token)
curl -sk -H "Authorization: Bearer $TOKEN" https://127.0.0.1:7879/api/v1/summary
curl -sk -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
     -d '{"type":"verify_audit"}' https://127.0.0.1:7879/api/v1/command
```
