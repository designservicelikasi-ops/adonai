#!/bin/sh
# AEGIS — installation (macOS / Linux). Installe Rust si besoin, compile, prépare la configuration.
set -e
cd "$(dirname "$0")/.."

if ! command -v cargo >/dev/null 2>&1 && [ ! -x "$HOME/.cargo/bin/cargo" ]; then
    echo "Rust n'est pas installé. Installation (dans votre dossier personnel)..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

case "$(uname -s)" in
Linux)
    echo "Sous Linux, l'application de bureau a besoin de bibliothèques système. Sous Debian/Ubuntu :"
    echo "  sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev libx11-dev libgl1-mesa-dev"
    ;;
esac

echo "Compilation (plusieurs minutes la première fois)..."
cargo build --release -p aegis-control -p aegisd

mkdir -p config data
if [ ! -f config/aegis.toml ]; then
    cp config/aegis.example.toml config/aegis.toml
    echo "Configuration créée : config/aegis.toml"
fi
echo
echo "Terminé. Lancez l'application avec : ./scripts/run.sh"
