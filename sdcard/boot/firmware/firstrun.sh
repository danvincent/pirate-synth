#!/usr/bin/env bash
set -euo pipefail

SERVICE_SRC="/boot/firmware/pirate-synth/pirate-synth-firstboot.service"
SERVICE_DST="/etc/systemd/system/pirate-synth-firstboot.service"

if [[ -f "$SERVICE_SRC" ]]; then
  install -m 0644 "$SERVICE_SRC" "$SERVICE_DST"
  systemctl daemon-reload
  systemctl enable pirate-synth-firstboot.service
fi
