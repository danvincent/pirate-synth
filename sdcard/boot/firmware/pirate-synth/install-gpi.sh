#!/usr/bin/env bash
set -euo pipefail

SENTINEL="/var/lib/pirate-synth/firstboot.done"
BOOT_DIR="/boot/firmware/pirate-synth"
SYSTEMD_DIR="/etc/systemd/system"
CONFIG_TXT="/boot/firmware/config.txt"
OVERLAYS_DIR="/boot/firmware/overlays"

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

ensure_absent() {
  local line="$1"
  if grep -Fqx "$line" "$CONFIG_TXT"; then
    sed -i "s|^${line}$||" "$CONFIG_TXT"
    config_changed=1
  fi
}

ensure_line "dtparam=audio=on"
ensure_line "dtparam=spi=on"

CMDLINE_TXT="/boot/firmware/cmdline.txt"
if [[ -f "$CMDLINE_TXT" ]]; then
  # Remove any existing enable_headphones parameter (handles =0 and duplicates)
  if grep -q 'snd_bcm2835\.enable_headphones' "$CMDLINE_TXT"; then
    sed -i 's/ snd_bcm2835\.enable_headphones=[01]//g' "$CMDLINE_TXT"
    config_changed=1
  fi
  # Re-add with =1
  if ! grep -q 'snd_bcm2835\.enable_headphones=1' "$CMDLINE_TXT"; then
    sed -i 's/$/ snd_bcm2835.enable_headphones=1/' "$CMDLINE_TXT"
    config_changed=1
  fi
  if ! grep -q 'fbcon=map:1' "$CMDLINE_TXT"; then
    sed -i 's/$/ fbcon=map:1/' "$CMDLINE_TXT"
    config_changed=1
  fi
fi

ensure_line "gpio=27=op,dh"

# GPi CASE DPI display — remove overlays that conflict with DPI framebuffer
ensure_absent "dtoverlay=vc4-kms-v3d"
ensure_absent "max_framebuffers=2"
ensure_absent "disable_fw_kms_setup=1"
ensure_absent "display_auto_detect=1"
# Remove stale audio entries from earlier installs
ensure_absent "dtoverlay=pwm-audio-pi-zero-gpi"
ensure_absent "dtoverlay=audremap,pins_18_19"
ensure_absent "audio_pwm_mode=2"

# DPI display config (320x240 parallel LCD)
ensure_line "dtoverlay=dpi24"
ensure_line "enable_dpi_lcd=1"
ensure_line "display_default_lcd=1"
ensure_line "dpi_group=2"
ensure_line "dpi_mode=87"
ensure_line "dpi_output_format=0x6016"
ensure_line "hdmi_timings=240 1 38 10 20 320 1 20 4 4 0 0 0 60 0 6400000 1"
ensure_line "display_rotate=1"
ensure_line "framebuffer_width=320"
ensure_line "framebuffer_height=240"

# GPi CASE PWM audio: route GPIO 18-19 to PWM for the amp circuit.
# Requires the custom dpi24 overlay (installed below) which only claims
# GPIO 2-17 for DPI, leaving GPIO 18-19 free.
ensure_line "dtoverlay=pwm-2chan,pin=18,func=2,pin2=19,func2=2"
ensure_line "disable_audio_dither=1"
ensure_line "dtoverlay=pwm-audio-pi-zero"

apt-get update
apt-get install -y --no-install-recommends alsa-utils device-tree-compiler

# Install the custom dpi24 overlay. The stock Pi OS dpi24 overlay claims
# GPIO 0-23; our version claims only GPIO 2-17 so GPIO 18-19 remain free
# for PWM audio. Compiled from source to keep the repo free of binaries.
echo "Compiling custom dpi24 overlay..."
dtc -W no-unit_address_vs_reg -I dts -O dtb \
    -o "$OVERLAYS_DIR/dpi24.dtbo" \
    "$BOOT_DIR/overlays/dpi24.dts"
echo "Overlay installed: $OVERLAYS_DIR/dpi24.dtbo"
config_changed=1

install -m 0755 "$BOOT_DIR/bin/pirate_synth" /usr/local/bin/pirate_synth
install -m 0644 "$BOOT_DIR/config/config-gpi.toml" /etc/pirate-synth/config.toml
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

systemctl disable NetworkManager-wait-online.service 2>/dev/null || true
systemctl mask bluetooth.service hciuart.service ModemManager.service 2>/dev/null || true
systemctl mask avahi-daemon.service avahi-daemon.socket 2>/dev/null || true
systemctl stop apt-daily.service apt-daily-upgrade.service apt-daily.timer apt-daily-upgrade.timer 2>/dev/null || true

if systemctl enable ssh.socket 2>/dev/null; then
  systemctl disable ssh.service 2>/dev/null || true
else
  echo "ssh.socket not available; leaving ssh.service unchanged"
fi

# Prevent cloud-init from resetting config.txt and hostname on future boots.
touch /etc/cloud/cloud-init.disabled

touch "$SENTINEL"

if [[ "$config_changed" -eq 1 ]]; then
  echo "First boot install complete. Rebooting to apply device-tree changes."
  reboot
else
  echo "First boot install complete. config.txt already up to date; no reboot needed."
  systemctl restart pirate-synth
fi