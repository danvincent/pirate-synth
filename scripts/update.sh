#!/usr/bin/env bash
# update.sh — run on the pirate-synth target to update the installed version.
#
# Expects the sibling pirate-synth directory next to this script to be
# populated with the contents of sdcard/boot/firmware/pirate-synth
# (i.e. copied there via scp/rsync).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC_DIR="$SCRIPT_DIR/pirate-synth"
SERVICE_NAME="pirate-synth"
SYSTEMD_DIR="/etc/systemd/system"

if [[ ! -d "$SRC_DIR" ]]; then
  echo "ERROR: source directory $SRC_DIR not found" >&2
  exit 1
fi

echo "==> Stopping $SERVICE_NAME service..."
systemctl stop "$SERVICE_NAME.service"

echo "==> Installing binary..."
install -m 0755 "$SRC_DIR/bin/pirate_synth" /usr/local/bin/pirate_synth

echo "==> Installing service file..."
install -m 0644 "$SRC_DIR/$SERVICE_NAME.service" "$SYSTEMD_DIR/$SERVICE_NAME.service"
systemctl daemon-reload

echo "==> Starting $SERVICE_NAME service..."
systemctl start "$SERVICE_NAME.service"

echo "==> Done. Service status:"
systemctl status "$SERVICE_NAME.service" --no-pager
