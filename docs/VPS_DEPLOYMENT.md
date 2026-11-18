# Déploiement sur VPS

## Architecture recommandée (initiale)

```
            VPS Windows (24 h/24)                                Votre ordinateur
┌───────────────────────────────────────────────┐          ┌──────────────────────────┐
│ MetaTrader 5 + AegisConnector (EA)            │          │ AEGIS Desktop            │
│        │ TCP 127.0.0.1:7878 (HMAC)            │  HTTPS   │ (Windows / Linux / macOS)│
│        ▼                                      │◀────────▶│ certificat épinglé       │
│ aegisd — service Windows « AegisEngine »      │  :7879   │ + jeton                  │
│   base SQLite, journaux, sauvegardes          │          └──────────────────────────┘
└───────────────────────────────────────────────┘
```

MT5 et le moteur sur la **même** machine : la liaison MT5 reste sur la boucle locale. Seule
l'API de contrôle (HTTPS, épinglée, jeton) est ouverte, et seulement à votre adresse.
Fermer AEGIS Desktop n'arrête ni le moteur ni les positions.

Évolution possible : moteur sur un VPS Linux, MT5 sur un VPS Windows, reliés par WireGuard
(voir « Séparer MT5 et le moteur » plus bas).

## 1. Installer le moteur (VPS Windows)

1. Copier le dossier `RELEASES/server/windows/` (contient `aegisd.exe`, `config/`,
   `packaging/`) sur le VPS, par exemple dans `C:\AEGIS-setup`.
2. PowerShell **administrateur** :
   ```powershell
   cd C:\AEGIS-setup
   .\packaging\windows\install-service.ps1 -Bin .\aegisd.exe -Dir C:\AEGIS
   ```
   Le script copie le binaire, crée `C:\AEGIS\config\aegis.toml` à partir de l'exemple,
   enregistre le service `AegisEngine` (démarrage automatique, **redémarrage automatique après
   5 s en cas d'échec**, trois fois, compteur remis à zéro chaque jour), le démarre et affiche
   les informations d'appairage.
3. Éditer `C:\AEGIS\config\aegis.toml` :
   ```toml
   [api]
   listen = "0.0.0.0:7879"
   tls_names = ["<adresse IP publique du VPS>"]
   [risk]
   commission_per_lot_round_turn = <votre commission>
   ```
   puis `Restart-Service AegisEngine`.
4. Pare-feu : autoriser le port 7879 **uniquement depuis votre adresse** :
   ```powershell
   New-NetFirewallRule -DisplayName "AEGIS API" -Direction Inbound -Protocol TCP -LocalPort 7879 -RemoteAddress <votre IP> -Action Allow
   ```
5. Afficher de nouveau l'appairage : `C:\AEGIS\aegisd.exe --config C:\AEGIS\config\aegis.toml pairing`.

Désinstallation : `.\packaging\windows\uninstall-service.ps1` (les données sont conservées).

## 1 bis. Installer le moteur (VPS Linux)

```sh
sudo ./packaging/linux/install.sh ./aegisd
```

Crée l'utilisateur système `aegis`, `/etc/aegis/aegis.toml`, `/var/lib/aegis/`, le service
systemd `aegisd` (`Restart=always`, arrêt propre par SIGINT) et affiche l'appairage.
`journalctl -u aegisd -f` pour suivre ; `systemctl restart aegisd` après une modification.

## 2. Connecteur MT5

Voir [`MT5_CONNECTOR.md`](MT5_CONNECTOR.md). Sur le VPS : `InpHost = 127.0.0.1`,
`InpPort = 7878`, `InpSharedKey` = contenu de `C:\AEGIS\data\shared.key`, et ajouter
`127.0.0.1` dans *Outils → Options → Expert Advisors → Autoriser WebRequest/sockets*.

## 3. AEGIS Desktop

Page *Connexion* → *Moteur distant* : adresse `https://<IP du VPS>:7879`, jeton, empreinte.
« Lire l'empreinte » affiche celle présentée par le serveur : ne l'acceptez que si elle est
identique à celle de `aegisd pairing`.

## Fonctionnement continu

| Besoin | Mise en œuvre |
|---|---|
| Redémarrage automatique | service Windows (actions d'échec) / systemd `Restart=always` ; le moteur repart **en MONITOR** et réconcilie les ordres au sort inconnu |
| Heartbeat | liaison MT5 : battement et délai de silence (`bridge.peer_timeout_ms`) ; fichier `data/heartbeat` réécrit toutes les 10 s (date, mode, état du terminal) pour une surveillance externe ; `GET /api/v1/health` renvoie 503 si l'état publié a plus de 5 s |
| Reconnexion | le connecteur se reconnecte seul ; entrées bloquées jusqu'à réconciliation |
| Journaux | `data/logs/` (rotation quotidienne) + journal d'audit chaîné en base |
| CPU / RAM | page *Système* du desktop, `GET /api/v1/system` |
| Sauvegardes | automatiques toutes les `ops.backup_interval_hours` (24 h par défaut), `ops.backup_keep` conservées ; manuelles depuis le desktop ; restauration : `aegisd restore --from <fichier>` (voir `BACKUP_RECOVERY.md`) |
| Récupération après crash | base SQLite WAL `synchronous=FULL` ; au redémarrage : ordres au sort inconnu en réconciliation, arrêt d'urgence restauré, automatisation jamais restaurée |
| Alertes | page *Système*, journaux, et `ops.alert_webhook_url` (HTTPS) : arrêt d'urgence, terminal déconnecté en mode automatique, base indisponible, sauvegarde impossible |
| État des positions | tableau de bord, page *Positions & ordres*, journal des transactions |

## Séparer MT5 et le moteur (évolution)

MT5 n'accepte que des connexions sortantes : le moteur est le serveur. Avec deux machines,
relier MT5 et le moteur par **WireGuard** (liaison MT5 non chiffrée par elle-même) :
`bridge.listen = "10.0.0.1:7878"` sur le moteur, `InpHost = 10.0.0.1` dans MT5. Ne jamais
exposer le port 7878 sur Internet.

## Ce qui reste à vérifier sur un vrai VPS

Le service Windows, le script d'installation PowerShell et le connecteur n'ont pas pu être
exécutés sur la machine de développement (macOS). Le pipeline CI les compile sur des runners
Windows natifs ; leur installation réelle sur un VPS reste **NON VÉRIFIÉE** tant qu'elle n'a
pas été faite (procédure de vérification : `TESTING.md`, § « Essais à faire sur Windows »).
