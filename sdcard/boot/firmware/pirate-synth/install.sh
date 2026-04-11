#!/usr/bin/env bash
set -euo pipefail

SENTINEL="/var/lib/pirate-synth/firstboot.done"
BOOT_DIR="/boot/firmware/pirate-synth"
SYSTEMD_DIR="/etc/systemd/system"
CONFIG_TXT="/boot/firmware/config.txt"

mkdir -p /var/lib/pirate-synth /etc/pirate-synth

if [[ -f "$SENTINEL" ]]; then
  echo "pirate-synth first boot already completed"
  exit 0
fi

ensure_line() {
  local line="$1"
  if ! grep -Fqx "$line" "$CONFIG_TXT"; then
    echo "$line" >> "$CONFIG_TXT"
  fi
}

ensure_line "dtoverlay=hifiberry-dac"
ensure_line "dtparam=audio=off"
ensure_line "dtparam=spi=on"
ensure_line "gpio=25=op,dh"

apt-get update
apt-get install -y --no-install-recommends alsa-utils

install -m 0755 "$BOOT_DIR/bin/pirate_synth" /usr/local/bin/pirate_synth
install -m 0644 "$BOOT_DIR/config/config.toml" /etc/pirate-synth/config.toml
rm -rf /var/lib/pirate-synth/wavetables
cp -a "$BOOT_DIR/wavetables" /var/lib/pirate-synth/wavetables

install -m 0644 "$BOOT_DIR/pirate-synth.service" "$SYSTEMD_DIR/pirate-synth.service"
systemctl daemon-reload
systemctl enable pirate-synth.service

touch "$SENTINEL"

echo "First boot install complete. Rebooting to apply device-tree changes."
reboot
