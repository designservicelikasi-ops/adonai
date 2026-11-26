# AEGIS 0.3.0 — livrables

Ce dossier ne contient **que des fichiers réellement produits et vérifiés** sur la machine de
développement (macOS, Apple Silicon), le 2026-11-26.

| Dossier | Contenu | État |
|---|---|---|
| `macos/` | `AEGIS-0.3.0-macos-arm64.zip` — AEGIS Desktop 0.3.0 (`AEGIS.app`, moteur `aegisd` embarqué) | compilé ; moteur embarqué démarré en MONITOR ; non signé (macOS demandera une confirmation à l'ouverture) |
| `server/macos/` | `aegisd` 0.3.0 (moteur, macOS arm64) + configuration d'exemple | compilé, lancé avec le simulateur |
| `connector/` | sources MQL5 du connecteur 1.20 (`.mq5`, `.mqh`) | pas de `.ex5` ici (MetaEditor indisponible sous macOS) ; compilé sans erreur par la CI le 2026-11-26 : artifact `aegis-connector-mt5` de l'exécution 45893676863 |
| `documentation/` | toute la documentation | — |
| `release-notes/` | `CHANGELOG.md` | — |
| `archive-0.2.0/` | livrables de la version 0.2.0 (2026-11-18) | conservés tels quels |

**Windows et Linux** (installateurs `.exe`, `.msi`, AppImage, `.deb`, `.rpm`, moteurs) et le
connecteur compilé `.ex5` sont fabriqués par la CI GitHub (`.github/workflows/build.yml`) sur
des runners natifs ; ils sont disponibles comme *artifacts* de l'exécution correspondante sur
GitHub, et le job `release` assemble ce même dossier pour toutes les plateformes lorsqu'une
étiquette `v*` est poussée.

Empreintes : `SHA256SUMS.txt`.
