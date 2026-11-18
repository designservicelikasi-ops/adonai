#!/bin/sh
# AEGIS — compilation complète (macOS / Linux) : moteur, puis AEGIS Desktop si Node.js est présent.
set -e
cd "$(dirname "$0")/.."
if ! command -v cargo >/dev/null 2>&1 && [ ! -x "$HOME/.cargo/bin/cargo" ]; then
    echo "Rust n'est pas installé. Installation (dans votre dossier personnel)..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

echo "1/2 Moteur (aegisd)..."
cargo build --release -p aegisd
mkdir -p config data
[ -f config/aegis.toml ] || { cp config/aegis.example.toml config/aegis.toml; echo "Configuration créée : config/aegis.toml"; }

if command -v npm >/dev/null 2>&1; then
    echo "2/2 AEGIS Desktop (Tauri)..."
    if [ "$(uname -s)" = Linux ]; then
        echo "Bibliothèques requises (Debian/Ubuntu) : sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev"
    fi
    TRIPLE=$(rustc -vV | sed -n 's/host: //p')
    mkdir -p desktop/src-tauri/binaries
    cp target/release/aegisd "desktop/src-tauri/binaries/aegisd-$TRIPLE"
    (cd desktop && npm ci && npx tauri build)
    echo "Installateurs : desktop/src-tauri/target/release/bundle/"
else
    echo "2/2 Node.js absent : AEGIS Desktop non compilé (le moteur seul est prêt)."
fi
echo "Moteur : ./scripts/run-daemon.sh   ·   interface en développement : ./scripts/desktop-dev.sh"
