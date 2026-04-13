# Contributing

## Prerequisites

- Rust stable toolchain (`rustup toolchain install stable`)
- `libasound2-dev` for local builds
- ARM cross toolchain for packaging (`gcc-arm-linux-gnueabihf libc6-dev-armhf-cross`)

```bash
sudo apt-get install -y --no-install-recommends \
  libasound2-dev gcc-arm-linux-gnueabihf libc6-dev-armhf-cross
```

## Running tests

```bash
cargo test --workspace
```

## Building

Run the packaging script to produce SD card bundles in `dist/`:

```bash
./scripts/package.sh
```

See the README for CI/ARMv6 options (`PIRATE_SYNTH_ARMV6_BINARY`).

## UI development without hardware

To iterate on the display menu without a physical Pi or ST7789 display:

```bash
cargo run -- --render-ui
```

This renders a PPM screenshot to `/tmp/pirate-synth-menu.ppm` without
requiring the SPI display or GPIO buttons. Any PPM viewer (e.g. `feh`,
`eog`, or `convert` from ImageMagick) can open it.

## Pull requests

- Tests must pass: `cargo test --workspace`
- Match the existing code style (`cargo fmt`, `cargo clippy`)
- Keep changes focused — one logical change per PR
