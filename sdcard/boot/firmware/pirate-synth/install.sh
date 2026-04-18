#!/usr/bin/env bash
set -euo pipefail

SENTINEL="/var/lib/pirate-synth/firstboot.done"
BOOT_DIR="/boot/firmware/pirate-synth"
SYSTEMD_DIR="/etc/systemd/system"
CONFIG_TXT="/boot/firmware/config.txt"

mkdir -p /var/lib/pirate-synth /etc/pirate-synth
if [[ ! -f "$CONFIG_TXT" ]]; then
  echo "Creating missing $CONFIG_TXT"
  touch "$CONFIG_TXT"
fi

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

# ---------------------------------------------------------------------------
# Purge packages that serve no purpose on a dedicated headless audio
# appliance.  Names that are absent are skipped, while real purge failures
# remain visible and still stop the installer under set -e.
# ---------------------------------------------------------------------------
purge_candidates=(
  avahi-daemon
  bluez
  bluez-firmware
  pi-bluetooth
  modemmanager
  triggerhappy
  nfs-common
  rpcbind
  usb-modeswitch
  usb-modeswitch-data
  man-db
  manpages
  raspi-config
  tasksel
  tasksel-data
  apt-listchanges
  installation-report
  ppp
  cifs-utils
)

installed_purge_candidates=()
for pkg in "${purge_candidates[@]}"; do
  if dpkg-query -W -f='${db:Status-Abbrev}' "$pkg" 2>/dev/null | grep -q '^ii'; then
    installed_purge_candidates+=("$pkg")
  fi
done

if (( ${#installed_purge_candidates[@]} > 0 )); then
  apt-get purge -y --auto-remove "${installed_purge_candidates[@]}"
fi
apt-get clean

# Drop offline documentation and locale data not removed by purge.
rm -rf /usr/share/doc/* /usr/share/man/* /usr/share/locale/*

install -m 0755 "$BOOT_DIR/bin/pirate_synth" /usr/local/bin/pirate_synth
install -m 0644 "$BOOT_DIR/config/config.toml" /etc/pirate-synth/config.toml
if [[ ! -d /var/lib/pirate-synth/wavetables ]]; then
  cp -a "$BOOT_DIR/wavetables" /var/lib/pirate-synth/wavetables
fi
if [[ ! -d /var/lib/pirate-synth/WAV ]]; then
  cp -a "$BOOT_DIR/WAV" /var/lib/pirate-synth/WAV
fi

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
