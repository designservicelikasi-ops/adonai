# AEGIS 0.2.0 — livrables

Ce dossier ne contient **que des fichiers réellement produits et vérifiés** sur la machine de
développement (macOS, Apple Silicon), le 2026-11-18.

| Dossier | Contenu | État |
|---|---|---|
| `macos/` | `AEGIS-0.2.0-macos-arm64.zip` — AEGIS Desktop (`AEGIS.app`, moteur `aegisd` embarqué) | compilé et lancé ; non signé (macOS demandera une confirmation à l'ouverture) |
| `server/macos/` | `aegisd` (moteur, macOS arm64) + configuration d'exemple | compilé, lancé avec le simulateur, API TLS vérifiée |
| `connector/` | sources MQL5 (`.mq5`, `.mqh`) | **NON COMPILÉ** : pas de `.ex5` (MetaEditor indisponible) |
| `documentation/` | toute la documentation | — |
| `release-notes/` | `CHANGELOG.md` | — |

**Absents, parce qu'ils n'ont pas encore été produits :** installateurs Windows (`.exe`,
`.msi`), paquets Linux (AppImage, `.deb`, `.rpm`), moteurs Windows et Linux, connecteur
compilé `.ex5`. Ils sont fabriqués par le pipeline `.github/workflows/build.yml` (runners
Windows et Linux natifs) dès que le dépôt est poussé sur GitHub ; le job `release` assemble
alors ce même dossier pour toutes les plateformes. Voir `documentation/BUILD_GUIDE.md`.

Empreintes : `SHA256SUMS.txt`.
