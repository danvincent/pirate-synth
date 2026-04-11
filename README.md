# pirate-synth

Boot-to-synth Raspberry Pi Zero project for the Pimoroni Pirate Audio Headphone Amp.

## Repository layout

- `crates/engine` - wavetable oscillator engine + key-to-frequency tuning
- `crates/audio_alsa` - ALSA playback loop
- `crates/ui` - ST7789 menu renderer + Pirate Audio buttons (active-low GPIO)
- `crates/app` - synth binary wiring config, UI, and audio
- `assets/wavetables` - source wavetable files
- `sdcard/boot/firmware/pirate-synth` - first-boot installer/services/config staged for SD card
- `scripts/package.sh` - cross-build + SD card bundle packaging

## Build and package in Codespaces/Linux

```bash
sudo apt-get update
sudo apt-get install -y --no-install-recommends libasound2-dev gcc-arm-linux-gnueabihf libc6-dev-armhf-cross zip
./scripts/package.sh
```

Outputs:

- `dist/pirate-synth-sdcard.tar.gz`
- `dist/pirate-synth-sdcard.zip`
- `dist/pirate-synth-sdcard-armv6.tar.gz`
- `dist/pirate-synth-sdcard-armv6.zip`

Artifact targets:

- `pirate-synth-sdcard.*`: existing `arm-unknown-linux-gnueabihf` build (ARMv7+ armhf Linux target)
- `pirate-synth-sdcard-armv6.*`: Pi Zero / Zero W compatible build with `target-cpu=arm1176jzf-s` (ARMv6)

## Copy bundle to SD card boot partition

1. Flash Raspberry Pi OS Lite to SD card.
2. Mount the boot partition.
3. Extract `pirate-synth-sdcard.tar.gz` (or zip).
4. Copy `boot/firmware/*` from the extracted bundle onto the SD card boot partition.
5. Boot the Pi Zero.

## What first boot does

`pirate-synth-firstboot.service` runs `/boot/firmware/pirate-synth/install.sh` once and then writes sentinel file `/var/lib/pirate-synth/firstboot.done`.

Installer actions:

- Ensures `/boot/firmware/config.txt` contains:
  - `dtoverlay=hifiberry-dac`
  - `dtparam=audio=off`
  - `dtparam=spi=on`
  - `gpio=25=op,dh`
- Installs runtime dependency `alsa-utils`
- Installs binary to `/usr/local/bin/pirate_synth`
- Installs config to `/etc/pirate-synth/config.toml`
- Copies wavetables to `/var/lib/pirate-synth/wavetables`
- Enables `pirate-synth.service`
- Reboots

## Config

Config file: `/etc/pirate-synth/config.toml`

Default values:

```toml
sample_rate = 48000
buffer_frames = 256
oscillators = 8
root_key = "C"
root_octave = 2
fine_tune_cents = 0
wavetable_dir = "/var/lib/pirate-synth/wavetables"
```

- `oscillators` controls simultaneous oscillators (allocated at startup)
- `root_key`, `root_octave`, `fine_tune_cents` tune the drone via the UI

## GPIO/SPI assumptions

- Buttons are active-low on BCM GPIO 5, 6, 16, 24 (mapped to Up/Down/Select/Back)
- Display uses ST7789 over SPI device `/dev/spidev0.0`
- Display control pins: DC BCM9, backlight BCM13
