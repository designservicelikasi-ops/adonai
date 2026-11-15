#!/bin/sh
# AEGIS — exécute tous les tests automatisés et affiche un résumé.
cd "$(dirname "$0")/.."
exec ./scripts/t.sh test --workspace --all-features
