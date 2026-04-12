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

config_changed=0
ensure_line() {
  local line="$1"
  if ! grep -Fqx "$line" "$CONFIG_TXT"; then
    echo "$line" >> "$CONFIG_TXT"
    config_changed=1
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

# ---------------------------------------------------------------------------
# Boot-time optimisations (safe on all Pi Zero W / Trixie Lite installs)
# ---------------------------------------------------------------------------

# NetworkManager still manages WiFi; this service only blocks the boot
# sequence until every managed interface reports "online", which we never
# need – pirate-synth and sshd both handle their own readiness.
systemctl disable NetworkManager-wait-online.service 2>/dev/null || true

# Bluetooth shares the CYW43 chip with WiFi on the Pi Zero W but is unused.
# Masking these frees ~4-8 s on each boot.
systemctl mask bluetooth.service hciuart.service ModemManager.service 2>/dev/null || true

# avahi (mDNS / .local resolution) is not required; saves ~1-2 s and
# suppresses the associated socket chatter.
systemctl mask avahi-daemon.service avahi-daemon.socket 2>/dev/null || true

# Avoid apt background activity contending with this first-boot install
# session, but keep unattended/security updates enabled on future boots.
systemctl stop apt-daily.service apt-daily-upgrade.service apt-daily.timer apt-daily-upgrade.timer 2>/dev/null || true

# SSH socket activation: sshd only forks a process when a connection arrives,
# so it becomes "available" in milliseconds rather than ~3 s at boot.
# WiFi must still come up before clients can connect – no functional change.
if systemctl enable ssh.socket 2>/dev/null; then
  systemctl disable ssh.service 2>/dev/null || true
else
  echo "ssh.socket not available; leaving ssh.service unchanged"
fi

touch "$SENTINEL"

if [[ "$config_changed" -eq 1 ]]; then
  echo "First boot install complete. Rebooting to apply device-tree changes."
  reboot
else
  echo "First boot install complete. config.txt already up to date; no reboot needed."
  systemctl restart pirate-synth
fi
