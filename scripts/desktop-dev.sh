#!/bin/sh
# AEGIS Desktop en mode développement (rechargement à chaud de l'interface).
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
TRIPLE=$(rustc -vV | sed -n 's/host: //p')
[ -x "desktop/src-tauri/binaries/aegisd-$TRIPLE" ] || { cargo build --release -p aegisd && mkdir -p desktop/src-tauri/binaries && cp target/release/aegisd "desktop/src-tauri/binaries/aegisd-$TRIPLE"; }
cd desktop && [ -d node_modules ] || npm ci
exec npx tauri dev
