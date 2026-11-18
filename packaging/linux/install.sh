#!/bin/sh
# AEGIS engine — installation as a systemd service on a Linux VPS.
# Usage (root): ./install.sh [path/to/aegisd]
set -eu
BIN="${1:-./aegisd}"
[ -x "$BIN" ] || { echo "aegisd introuvable: $BIN" >&2; exit 1; }
id aegis >/dev/null 2>&1 || useradd --system --home /var/lib/aegis --shell /usr/sbin/nologin aegis
install -d -o aegis -g aegis -m 0750 /var/lib/aegis
install -d -m 0755 /etc/aegis
install -m 0755 "$BIN" /usr/local/bin/aegisd
if [ ! -f /etc/aegis/aegis.toml ]; then
  sed 's#^data_dir = .*#data_dir = "/var/lib/aegis/data"#' "$(dirname "$0")/../config/aegis.example.toml" > /etc/aegis/aegis.toml 2>/dev/null \
    || printf '[engine]\ndata_dir = "/var/lib/aegis/data"\n' > /etc/aegis/aegis.toml
fi
cd /var/lib/aegis
/usr/local/bin/aegisd --config /etc/aegis/aegis.toml systemd-unit --user aegis > /etc/systemd/system/aegisd.service
systemctl daemon-reload
systemctl enable --now aegisd
sleep 3
systemctl --no-pager status aegisd | head -5
echo "Appairage d'AEGIS Desktop :"
sudo -u aegis /usr/local/bin/aegisd --config /etc/aegis/aegis.toml pairing
