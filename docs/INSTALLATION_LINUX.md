# Installation sous Linux

## AEGIS Desktop

Paquets produits (dossier `RELEASES/linux/`) :

| Format | Installation | Désinstallation |
|---|---|---|
| AppImage (toutes distributions) | `chmod +x AEGIS_<v>_amd64.AppImage && ./AEGIS_<v>_amd64.AppImage` | supprimer le fichier |
| `.deb` (Debian, Ubuntu 22.04+) | `sudo apt install ./AEGIS_<v>_amd64.deb` | `sudo apt remove aegis` |
| `.rpm` (Fedora, RHEL 9+) | `sudo dnf install ./AEGIS-<v>-1.x86_64.rpm` | `sudo dnf remove aegis` |

Les paquets `.deb` et `.rpm` installent l'application et le moteur `aegisd` dans `/usr/bin`.

Dépendances système (installées automatiquement par `apt`/`dnf`) : WebKitGTK 4.1
(`libwebkit2gtk-4.1-0`), GTK 3, `libayatana-appindicator3` ou `libappindicator3`, `librsvg2`.

Au premier lancement : page *Connexion* → moteur local (démarré en processus détaché, données
dans `~/.local/share/app.aegis.desktop/engine/`) ou moteur distant.

## Moteur seul (serveur)

`RELEASES/server/linux/` contient `aegisd`, la configuration d'exemple et
`packaging/linux/install.sh` (service systemd). Voir `VPS_DEPLOYMENT.md`, § 1 bis.

Le moteur n'a besoin d'aucune bibliothèque partagée particulière au-delà de la glibc
(SQLite et TLS sont intégrés).

## État de vérification

Les paquets sont construits sur un runner Ubuntu 22.04 natif ; le job installe le `.deb`,
vérifie la présence des deux binaires, lance `aegisd print-config` et désinstalle. Tant qu'un
run n'a pas été observé, ces étapes sont **NON VÉRIFIÉES** (voir `PROJECT_STATUS.md`).
L'AppImage et le `.rpm` ne sont pas installés par le pipeline.
