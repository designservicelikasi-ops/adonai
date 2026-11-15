#!/bin/sh
# AEGIS — lance le moteur sans interface (VPS Linux). Ctrl-C pour l'arrêter proprement.
#   ./scripts/run-daemon.sh               moteur seul
#   ./scripts/run-daemon.sh --simulator   avec le simulateur de test (données fictives)
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
[ -f config/aegis.toml ] || cp config/aegis.example.toml config/aegis.toml
BIN=target/release/aegisd
[ -x "$BIN" ] || { echo "Moteur non compilé : lancez d'abord ./scripts/setup.sh"; exit 1; }
exec "$BIN" --config config/aegis.toml run "$@"
