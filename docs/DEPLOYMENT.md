# Guide de déploiement

## 1. Prérequis

| Composant | Besoin |
|---|---|
| Compilation | Rust stable (1.100 utilisé), un compilateur C (Xcode CLT, `build-essential`, ou Build Tools Visual Studio) |
| Application de bureau sous Linux | `pkg-config libxkbcommon-dev libwayland-dev libx11-dev libgl1-mesa-dev` |
| Connecteur | MetaTrader 5 et MetaEditor (Windows, ou Wine/CrossOver) |
| Recherche IA (facultatif) | `uv` et Python 3.11+ |

Seule la compilation sous **macOS (Apple Silicon)** a été vérifiée. Windows et Linux : non vérifié.

## 2. Installation sur un poste (tout sur la même machine)

```sh
./scripts/setup.sh
./scripts/run.sh
```

Windows : `scripts\setup.ps1`, puis `target\release\aegis.exe --config config\aegis.toml`.

MT5 et AEGIS sur le même PC : laissez `bridge.listen = "127.0.0.1:7878"`. C'est la
configuration la plus simple et la plus sûre (rien ne sort de la machine).

## 3. Moteur sur un VPS Linux, MT5 sous Windows

MetaTrader 5 ne fonctionne pas nativement sous Linux : il reste sur une machine Windows
(votre PC ou un VPS Windows). Le moteur tourne sur le VPS Linux sans interface.

```
[ Windows : MT5 + AegisConnector ] ──(tunnel privé)──▶ [ VPS Linux : aegisd ]
```

**La liaison est authentifiée mais non chiffrée.** Ne l'exposez jamais directement sur
Internet. Utilisez un tunnel :

- **WireGuard** (recommandé) : les deux machines sur un réseau privé `10.x`. Sur le VPS,
  `bridge.listen = "10.0.0.1:7878"` ; dans MT5, `InpHost = 10.0.0.1`.
- **Tunnel SSH** depuis Windows : `ssh -N -L 7878:127.0.0.1:7878 utilisateur@vps`. Le moteur
  reste sur `127.0.0.1:7878` ; dans MT5, `InpHost = 127.0.0.1`.

Sur le VPS :

```sh
./scripts/setup.sh
export AEGIS_SHARED_KEY='une-longue-clé-aléatoire-d-au-moins-32-caractères'
./scripts/run-daemon.sh
```

La même clé se saisit dans `InpSharedKey` côté MT5.

### Service systemd (exemple, non vérifié)

```ini
# /etc/systemd/system/aegis.service
[Unit]
Description=AEGIS engine
After=network-online.target

[Service]
User=aegis
WorkingDirectory=/opt/aegis
EnvironmentFile=/etc/aegis/env          # AEGIS_SHARED_KEY=... et AEGIS_API_TOKEN=... ; droits 0600
ExecStart=/opt/aegis/target/release/aegisd --config /opt/aegis/config/aegis.toml run
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

Après un redémarrage (voulu ou non), le moteur revient **en MONITOR** : c'est délibéré.

### API de contrôle

Dans `config/aegis.toml` : `[api] enabled = true`. Le jeton (24 caractères minimum) vient de
`AEGIS_API_TOKEN`. Gardez `listen` sur `127.0.0.1` et passez par le tunnel.

```sh
curl -H "Authorization: Bearer $AEGIS_API_TOKEN" http://127.0.0.1:7879/api/v1/state
curl -X POST -H "Authorization: Bearer $AEGIS_API_TOKEN" -H 'Content-Type: application/json' \
     -d '{"reason":"arrêt manuel"}' http://127.0.0.1:7879/api/v1/kill
curl -X POST ... -d '{"mode":"demo_auto"}' http://127.0.0.1:7879/api/v1/mode
curl -X POST ... -d '{"phrase":"RESET AEGIS"}' http://127.0.0.1:7879/api/v1/kill/reset
```

## 4. Secrets

| Secret | Où | Jamais dans |
|---|---|---|
| Clé partagée moteur ↔ MT5 | `AEGIS_SHARED_KEY`, sinon `data/shared.key` (droits `0600`) | le fichier de configuration, les journaux |
| Jeton de l'API | `AEGIS_API_TOKEN` | idem |

Changer la clé : arrêtez le moteur, changez la variable (ou supprimez `data/shared.key`),
relancez, puis mettez à jour `InpSharedKey` dans MT5.

## 5. Fichiers produits

```
data/aegis.db            base de données (+ aegis.db-wal, aegis.db-shm pendant l'exécution)
data/shared.key          clé partagée (si la variable d'environnement n'est pas définie)
data/logs/aegis.log.*    journaux JSON, un fichier par jour
data/backups/            sauvegardes créées depuis l'application
```

## 6. Liste de contrôle avant un compte de démonstration

- [ ] `AegisSelfTest` affiche « ALL PASSED » dans MT5
- [ ] le connecteur se connecte ; AEGIS affiche le bon numéro de compte, type « demo »
- [ ] la commission par lot est renseignée dans Gestion des risques
- [ ] les spécifications des instruments (page Gestion du compte) sont cohérentes avec MT5
- [ ] un ordre de test minimal passe : statut `filled`, position visible des deux côtés avec SL et TP
- [ ] l'arrêt d'urgence bloque bien une entrée ; la réinitialisation fonctionne
- [ ] couper le réseau : AEGIS passe à « déconnecté », puis se reconnecte
- [ ] sauvegarde créée et restauration essayée (voir `BACKUP_RECOVERY.md`)
