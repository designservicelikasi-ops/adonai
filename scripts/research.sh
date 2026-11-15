#!/bin/sh
# AEGIS — chaîne de recherche IA : export du jeu de données -> entraînement Python -> vérification par le moteur.
#   ./scripts/research.sh EURUSD M15 broker        (données reçues de MT5)
#   ./scripts/research.sh EURUSD.sim M15 synthetic (données fictives : pour essayer la chaîne)
set -e
cd "$(dirname "$0")/.."
SYMBOL="${1:?instrument}"; TF="${2:?unité de temps}"; ORIGIN="${3:-broker}"
command -v uv >/dev/null 2>&1 || { echo "uv est requis : https://docs.astral.sh/uv/"; exit 1; }
BIN=target/release/aegisd
[ -x "$BIN" ] || BIN=target/debug/aegisd
mkdir -p research/out models
"$BIN" --config config/aegis.toml export-dataset --symbol "$SYMBOL" --tf "$TF" --origin "$ORIGIN" --horizon 4 --out research/out/dataset.csv
(cd research && uv run --python 3.12 train.py --dataset out/dataset.csv --out ../models/model.json)
"$BIN" --config config/aegis.toml verify-model --file models/model.json
