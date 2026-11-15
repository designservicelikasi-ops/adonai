#!/bin/sh
# AEGIS — sauvegarde cohérente de la base (utilisable moteur arrêté ; sinon utilisez le bouton de l'application).
cd "$(dirname "$0")/.."
BIN=target/release/aegisd
[ -x "$BIN" ] || { echo "Lancez d'abord ./scripts/setup.sh"; exit 1; }
exec "$BIN" --config config/aegis.toml backup
