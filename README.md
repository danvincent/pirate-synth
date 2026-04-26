# pirate-synth

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Boot-to-synth Raspberry Pi Zero project for the Pimoroni Pirate Audio Headphone Amp.

## Repository layout

- `crates/engine` - wavetable oscillator engine + key-to-frequency tuning
- `crates/audio_alsa` - ALSA playback loop
- `crates/ui` - ST7789 menu renderer + Pirate Audio buttons (active-low GPIO)
- `crates/app` - synth binary wiring config, UI, and audio
- `assets/wavetables` - source wavetable files
- `assets/WAV` - granular source WAV files (PCM16/float32), including a default `placeholder.wav`
- `sdcard/boot/firmware/pirate-synth` - first-boot installer/services/config staged for SD card
- `scripts/package.sh` - cross-build + SD card bundle packaging

## Features

- Wavetable synthesis with 8 built-in waveforms (sine, triangle, sawtooth, square, pulse, etc.)
- Granular synthesis engine (auto-selected when WAV files are present in `wav_dir`)
- Independent on/off and volume controls for Wavetable and Granular layers; both fade in/out over 5 seconds
- Per-oscillator fade when oscillator count changes (only added/removed oscillators fade)
- Per-voice fade when granular voice count changes (only added/removed sources fade)
- Per-oscillator detune, drift LFO, stereo spread
- Effects: reverb (Schroeder), tremolo, crossfade, filter sweep, FM, subtractive
- Class-compliant USB MIDI keyboard note input (monophonic, most recent note latches key/octave)
- MIDI CC control for cents detune (`midi_cents_cc` in config)
- 9 scale modes (chromatic, major, minor, pentatonic, dorian, etc.)
- 13-item ST7789 240×240 display menu via SPI (includes `VIDEO: OFF|ON|NO HDMI` status)
- First-boot installer for Raspberry Pi OS Lite
- Offline UI rendering (`--render-ui`) for development without hardware
- Safe shutdown: hold **Up** and **Down** simultaneously for 5 seconds; the display shows "Powering down" and the system halts cleanly via `shutdown -h now`

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

- `pirate-synth-sdcard.*`: existing `arm-unknown-linux-gnueabihf` build (ARMv7+ armhf Linux target, e.g. Pi 2/3/4 running 32-bit Raspberry Pi OS)
- `pirate-synth-sdcard-armv6.*`: Pi Zero / Zero W compatible ARMv6 build

To package with a prebuilt ARMv6 binary (used by CI), set:

```bash
PIRATE_SYNTH_ARMV6_BINARY=/absolute/path/to/pirate_synth ./scripts/package.sh
```

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
- Copies granular WAV sources to `/var/lib/pirate-synth/WAV`
- Enables `pirate-synth.service`
- Reboots

## Config

Config file: `/etc/pirate-synth/config.toml`

Default values:

```toml
sample_rate = 48000
buffer_frames = 256
oscillators = 8
root_key = "A"
root_octave = 1
fine_tune_cents = 0
midi_cents_cc = 1
wavetable_dir = "/var/lib/pirate-synth/wavetables"
wav_dir = "/var/lib/pirate-synth/WAV"
granular_grain_size_ms = 120.0
granular_density_hz = 24.0
granular_max_overlap = 16
granular_position = 0.5
granular_position_jitter = 0.15
granular_attack_ms = 10.0
granular_release_ms = 25.0
granular_wavs = 8
granular_volume = 50
granular_active = false
hdmi_visuals_enabled = false
```

- `oscillators` controls simultaneous oscillators (allocated at startup)
- `root_key`, `root_octave`, `fine_tune_cents` tune the drone via the UI
- USB MIDI note-on updates `root_key` and `root_octave` live (note-off is ignored so notes latch)
- `midi_cents_cc` defines which MIDI CC number (0-127) controls `fine_tune_cents` from -100 to +100
- Engine selection is automatic by source-folder origin:
  - if `wav_dir` contains `.wav` files, granular synthesis mode is used
  - otherwise the wavetable engine uses `wavetable_dir`
- Granular mode currently supports WAV PCM16 and float32 sources (TODO: add more WAV variants/modulation features)
- `granular_wavs` controls the number of active granular source lanes (adjustable as `GR VOICES` in the UI):
  - `0` disables granular playback
  - values above loaded WAV file count round-robin the available files
- `granular_volume` sets the initial granular output level (0–100); adjustable as `GR VOL` in the UI
- `granular_active` sets whether the granular layer starts active at boot; toggleable as `GRANULAR` in the UI
- `hdmi_visuals_enabled` gates HDMI visualizer startup (`VIDEO: OFF|ON|NO HDMI` in the UI)
- Both layers can be active simultaneously; their volumes are mixed independently

## GPIO/SPI assumptions

- Buttons are active-low on BCM GPIO 5, 6, 16, 24 (mapped to Up/Down/Select/Back)
- Holding Up + Down together for 5 seconds triggers a safe system shutdown (display shows "Powering down")
- Display uses ST7789 over SPI device `/dev/spidev0.1`
- Display control pins: DC BCM9, backlight BCM13
- Runtime access is through `rppal` (GPIO character-device `/dev/gpiomem` + SPI `/dev/spidev*`), not legacy `/sys/class/gpio`
- `pirate-synth.service` logs startup and hardware init to journald (`journalctl -u pirate-synth -b`)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for build prerequisites, running tests, and pull request guidelines.

## License

MIT — see [LICENSE](LICENSE). Copyright 2026 Daniel Vincent.
