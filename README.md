# pirate-synth

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE) [![Quality Gate Status](https://sonarcloud.io/api/project_badges/measure?project=danvincent_pirate-synth&metric=alert_status)](https://sonarcloud.io/summary/new_code?id=danvincent_pirate-synth) ![Built for Raspberry Pi](https://img.shields.io/badge/built_for-raspberry_pi-white)

Boot-to-synth Raspberry Pi Zero project for the [Pimoroni Pirate Audio Headphone Amp](https://shop.pimoroni.com/products/pirate-audio-headphone-amp?variant=31189750480979).



## Repository layout

- `crates/engine` - wavetable oscillator engine + key-to-frequency tuning
- `crates/audio_alsa` - ALSA playback loop
- `crates/ui` - ST7789 menu renderer + Pirate Audio buttons (active-low GPIO)
- `crates/controller` - debounced synth controller; bridges UI state to audio commands
- `crates/visuals_drm` - HDMI scope/visualizer (DRM)
- `crates/app` - synth binary wiring config, UI, and audio
- `assets/wavetables` - source wavetable files
- `assets/WAV` - granular source WAV files (PCM16/float32), including a default `placeholder.wav`
- `sdcard/boot/firmware/pirate-synth` - first-boot installer/services/config staged for SD card
- `scripts/package.sh` - cross-build + SD card bundle packaging

## Features

- Wavetable synthesis with 8 built-in waveforms (sine, triangle, sawtooth, square, pulse, etc.)
- Granular synthesis engine (auto-selected when WAV files are present in `wav_dir`)
- Bytebeat synthesis engine with 10 algorithms (Basic, Sierpinski, Melody, Harmony, Acid, Wobble, Glitch, Pulse, Storm, Echo) plus a random-cycling mode; up to 8 pitched oscillators with per-oscillator drift LFO and stereo spread
- Independent on/off and volume controls for Wavetable, Granular, and Bytebeat layers; all fade in/out over 5 seconds
- Per-oscillator fade when oscillator count changes (only added/removed oscillators fade)
- Per-voice fade when granular voice count changes (only added/removed sources fade)
- Per-oscillator detune, drift LFO, stereo spread
- Effects: reverb (Schroeder), tremolo, crossfade, filter sweep, FM, subtractive
- Class-compliant USB MIDI keyboard note input (monophonic, most recent note latches key/octave)
- MIDI CC control for cents detune (`midi_cents_cc` in config)
- 9 scale modes (N/A, major, minor, pentatonic, dorian, etc.)
- 14-item ST7789 240×240 display menu via SPI (includes `VIDEO: OFF|ON|NO HDMI` status)
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
- `dist/pirate-synth-sdcard-gpi.tar.gz`
- `dist/pirate-synth-sdcard-gpi.zip`

Artifact targets:

- `pirate-synth-sdcard.*`: existing `arm-unknown-linux-gnueabihf` build (ARMv7+ armhf Linux target, e.g. Pi 2/3/4 running 32-bit Raspberry Pi OS)
- `pirate-synth-sdcard-armv6.*`: Pi Zero / Zero W compatible ARMv6 build
- `pirate-synth-sdcard-gpi.*`: GPi CASE bundle for Pi Zero / Zero W using the GPi hardware profile

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

## GPi CASE Setup

The GPi CASE uses the Retroflag GPi CASE with a Raspberry Pi Zero.

Prerequisites:

- Raspberry Pi Zero or Zero W
- Retroflag GPi CASE
- Raspberry Pi OS Lite

Install steps:

1. Download the `pirate-synth-sdcard-gpi` bundle (`.tar.gz` or `.zip`).
2. Flash Raspberry Pi OS Lite to SD card.
3. Mount the boot partition.
4. Extract the GPi bundle.
5. Copy `boot/firmware/*` from the extracted bundle onto the SD card boot partition.
6. Insert SD card into the GPi CASE and boot.

Controls:

- D-pad Up/Down: navigate menu
- D-pad Left: Back
- D-pad Right: Select
- Hold Up + Down for 5 seconds: safe shutdown

Note: A/B/X/Y/Start/Select buttons are not used.

## What first boot does

`pirate-synth-firstboot.service` runs the appropriate hardware-specific installer once on first boot — typically `/boot/firmware/pirate-synth/install.sh`, or `/boot/firmware/pirate-synth/install-gpi.sh` for the GPi bundle — and then writes sentinel file `/var/lib/pirate-synth/firstboot.done`.

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
- Reboots (only if `config.txt` was changed to apply device-tree settings; otherwise restarts `pirate-synth.service` immediately)

## Config

Config file: `/etc/pirate-synth/config.toml`

The installed config.toml is fully commented and contains all available settings. Code defaults (used when a key is absent from the config file) are shown below for reference; the shipped config.toml overrides many of these for the target hardware.

```toml
sample_rate = 48000
buffer_frames = 256
spi_device = "/dev/spidev0.1"

root_key = "A"
root_octave = 1
fine_tune_cents = 0.0
midi_cents_cc = 1

scale_index = 7          # 0=N/A 1=Major 2=Minor 3=Penta 4=Dorian 5=Mixo 6=Whole 7=Hirajoshi 8=Lydian
stereo_spread = 100      # 0–100

oscillators = 8
oscillators_active = false
wavetable_dir = "/var/lib/pirate-synth/wavetables"
bank_index = 0           # 0=A 1=B 2=C 3=D

volume = 50              # master output 0–100

transition_secs = 3.0    # smooth transition duration for cents/scale/bank changes
note_transition_ms = 0.0 # glide time in ms; 0 = snap immediately

reverb_enabled = true
reverb_wet = 0.20
reverb_feedback = 0.84
reverb_damp = 0.20
reverb_comb_count = 4

granular_reverb_enabled = true
granular_reverb_wet = 0.45
granular_reverb_feedback = 0.88
granular_reverb_damp = 0.12
granular_reverb_comb_count = 8

tremolo_enabled = true
tremolo_depth = 0.35

crossfade_enabled = true
crossfade_rate = 0.05

filter_sweep_enabled = true
filter_sweep_min = 0.15
filter_sweep_max = 0.80
filter_sweep_rate_hz = 0.008

fm_enabled = false
fm_depth = 0.15

subtractive_enabled = false
subtractive_depth = 0.30

wav_dir = "/var/lib/pirate-synth/WAV"
granular_active = false
granular_wavs = 8
granular_volume = 50
granular_grain_size_ms = 120.0
granular_density_hz = 24.0
granular_max_overlap = 16
granular_position = 0.5
granular_position_jitter = 0.15
granular_attack_ms = 10.0
granular_release_ms = 25.0
granular_note_ms = 4000.0
granular_spawn_jitter = 0.5

hdmi_visuals_enabled = false
```

- `oscillators` controls simultaneous oscillators (allocated at startup); `oscillators_active` sets whether the wavetable layer starts active at boot
- `root_key`, `root_octave`, `fine_tune_cents` tune the drone via the UI
- USB MIDI note-on updates `root_key` and `root_octave` live (note-off is ignored so notes latch)
- `midi_cents_cc` defines which MIDI CC number (0-127) controls `fine_tune_cents` from -100 to +100
- `scale_index` selects the scale mode (0=N/A means no scale constraint / free chromatic)
- `stereo_spread` (0–100) controls how wide oscillators are spread across the stereo field
- `bank_index` selects which wavetable bank (0=A, 1=B, 2=C, 3=D) is active at startup
- `volume` sets the master output level (0–100)
- `transition_secs` smooths cents/scale/bank changes over that many seconds (±20% per-oscillator jitter)
- `note_transition_ms` sets the glide time in milliseconds for note/frequency changes (0 = snap)
- Engine selection is automatic by source-folder contents:
  - if `wav_dir` contains `.wav` files, granular synthesis mode is used
  - otherwise the wavetable engine uses `wavetable_dir`
- Granular mode currently supports WAV PCM16 and float32 sources (TODO: add more WAV variants/modulation features)
- `granular_wavs` controls the number of active granular source lanes (adjustable as `GR Voices` in the UI):
  - `0` disables granular playback
  - values above loaded WAV file count round-robin the available files
- `granular_volume` sets the initial granular output level (0–100); adjustable as `GR Vol` in the UI
- `granular_active` sets whether the granular layer starts active at boot; toggleable as `Granular` in the UI
- The Bytebeat layer is controlled entirely from the UI (`Bytebeat →` submenu): On/Off, Volume (0–100), Algorithm (10 named + Random), and Oscillator count (1–8)
- `hdmi_visuals_enabled` gates HDMI visualizer startup (`Video: Off/On/No HDMI` in the UI)
- Both Wavetable/Granular and Bytebeat layers can be active simultaneously; their volumes are mixed independently

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
