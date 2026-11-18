#!/bin/sh
# AEGIS — lance l'application de bureau compilée (macOS / Linux), ou indique comment la compiler.
cd "$(dirname "$0")/.."
case "$(uname -s)" in
Darwin) APP=desktop/src-tauri/target/release/bundle/macos/AEGIS.app; [ -d "$APP" ] && exec open "$APP" ;;
*) BIN=desktop/src-tauri/target/release/aegis-desktop; [ -x "$BIN" ] && exec "$BIN" ;;
esac
echo "AEGIS Desktop n'est pas compilé : ./scripts/setup.sh (ou ./scripts/desktop-dev.sh)."
exit 1
