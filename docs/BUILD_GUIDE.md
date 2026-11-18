# Guide de compilation

## Prérequis

| Outil | Version utilisée | Usage |
|---|---|---|
| Rust (stable) | 1.100 | moteur, API, desktop (côté natif) |
| Node.js | 22 | interface du desktop |
| Compilateur C | Xcode CLT / `build-essential` / Build Tools Visual Studio | SQLite intégré, `ring` |
| Linux (desktop) | `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev rpm` | Tauri |
| MetaEditor (Windows) | MetaTrader 5 | connecteur MQL5 |

## Moteur et outils

```sh
cargo test --workspace          # ~230 tests (./scripts/test.sh pour un résumé compact)
cargo build --release -p aegisd # target/release/aegisd
```

Test de capacité (2 000 allers-retours, voir `TESTING.md`) :

```sh
cargo test --release -p aegis-core --test capacity -- --ignored --nocapture
```

## AEGIS Desktop (Tauri 2)

L'application embarque le moteur (`externalBin`). Le binaire doit être placé, avant la
compilation, sous le nom attendu par Tauri :

```sh
cargo build --release -p aegisd
TRIPLE=$(rustc -vV | sed -n 's/host: //p')
cp target/release/aegisd desktop/src-tauri/binaries/aegisd-$TRIPLE      # .exe sous Windows
cd desktop
npm ci
npm run typecheck
npx tauri build                 # installateurs de la plateforme courante
npx tauri dev                   # développement (interface rechargée à chaud)
```

Sorties : `desktop/src-tauri/target/release/bundle/` — `nsis/*.exe` et `msi/*.msi` sous
Windows ; `deb/`, `rpm/`, `appimage/` sous Linux ; `macos/AEGIS.app` sous macOS.

## Pipeline reproductible (GitHub Actions)

`.github/workflows/build.yml`, runners **natifs** (pas de compilation croisée) :

| Job | Plateformes | Produit |
|---|---|---|
| `test` | Ubuntu 22.04, Windows, macOS 14 | tests + clippy, journaux |
| `server` | Ubuntu 22.04, Windows | `aegisd` release, test de fumée (simulateur + API TLS), archive serveur |
| `desktop` | Ubuntu 22.04, Windows | installateurs Tauri, sommes SHA-256, installation et désinstallation automatiques |
| `mql5` | Windows | installation silencieuse de MT5, compilation `AegisConnector.mq5` et `AegisSelfTest.mq5` avec MetaEditor |
| `release` | sur une étiquette `v*` | dossier `RELEASES/` (fichiers réellement produits uniquement) |

Les versions sont identifiées par `version` dans `Cargo.toml` (moteur) et
`desktop/src-tauri/tauri.conf.json` (application). Chaque job publie ses journaux.

Pour lancer le pipeline : pousser le dépôt sur GitHub (aucun dépôt distant n'est configuré
aujourd'hui), puis *Actions → build → Run workflow*, ou créer une étiquette `v0.2.0`.
