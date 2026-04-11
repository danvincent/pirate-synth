#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="arm-unknown-linux-gnueabihf"
BINARY_NAME="pirate_synth"
STAGE_DIR="$ROOT_DIR/dist/pirate-synth-sdcard"
BOOT_SRC="$ROOT_DIR/sdcard/boot/firmware"
BOOT_DST="$STAGE_DIR/boot/firmware"

rustup target add "$TARGET"

cargo build --release --target "$TARGET" -p pirate_synth

rm -rf "$STAGE_DIR"
mkdir -p "$BOOT_DST"
cp -a "$BOOT_SRC/." "$BOOT_DST/"
install -m 0755 \
  "$ROOT_DIR/target/$TARGET/release/$BINARY_NAME" \
  "$BOOT_DST/pirate-synth/bin/$BINARY_NAME"

cp -a "$ROOT_DIR/assets/wavetables/." "$BOOT_DST/pirate-synth/wavetables/"

tar -C "$ROOT_DIR/dist" -czf "$ROOT_DIR/dist/pirate-synth-sdcard.tar.gz" pirate-synth-sdcard
if command -v zip >/dev/null 2>&1; then
  (cd "$ROOT_DIR/dist" && zip -qr pirate-synth-sdcard.zip pirate-synth-sdcard)
fi

echo "Packaged bundle in: $ROOT_DIR/dist"
