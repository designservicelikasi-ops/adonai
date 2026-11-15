#!/bin/sh
# AEGIS — lance l'application de bureau. Le moteur démarre toujours en mode MONITOR.
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
[ -f config/aegis.toml ] || cp config/aegis.example.toml config/aegis.toml
BIN=target/release/aegis
[ -x "$BIN" ] || { echo "Application non compilée : lancez d'abord ./scripts/setup.sh"; exit 1; }
exec "$BIN" --config config/aegis.toml "$@"
