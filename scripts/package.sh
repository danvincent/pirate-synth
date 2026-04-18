#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="arm-unknown-linux-gnueabihf"
BINARY_NAME="pirate_synth"
DIST_DIR="$ROOT_DIR/dist"
ARMHF_BUNDLE_NAME="pirate-synth-sdcard"
ARMV6_BUNDLE_NAME="pirate-synth-sdcard-armv6"
ARMHF_STAGE_DIR="$DIST_DIR/$ARMHF_BUNDLE_NAME"
ARMV6_STAGE_DIR="$DIST_DIR/$ARMV6_BUNDLE_NAME"
BOOT_SRC="$ROOT_DIR/sdcard/boot/firmware"
BUILD_BIN_DIR="$DIST_DIR/.binaries"
ARMHF_BINARY="$BUILD_BIN_DIR/${BINARY_NAME}-armhf"
ARMV6_BINARY="$BUILD_BIN_DIR/${BINARY_NAME}-armv6"
ARMV6_CPU_RUSTFLAGS="-C target-cpu=arm1176jzf-s"
ARMV6_SOURCE_BINARY="${PIRATE_SYNTH_ARMV6_BINARY:-}"

stage_bundle() {
  local stage_dir="$1"
  local bundle_binary="$2"
  local boot_dst="$stage_dir/boot/firmware"

  rm -rf "$stage_dir"
  mkdir -p "$boot_dst"
  cp -a "$BOOT_SRC/." "$boot_dst/"
  install -m 0755 "$bundle_binary" "$boot_dst/pirate-synth/bin/$BINARY_NAME"

  mkdir -p "$boot_dst/pirate-synth/wavetables"
  if compgen -G "$ROOT_DIR/assets/wavetables/*" >/dev/null; then
    cp -a "$ROOT_DIR/assets/wavetables/." "$boot_dst/pirate-synth/wavetables/"
  fi
  mkdir -p "$boot_dst/pirate-synth/WAV"
  if compgen -G "$ROOT_DIR/assets/WAV/*" >/dev/null; then
    cp -a "$ROOT_DIR/assets/WAV/." "$boot_dst/pirate-synth/WAV/"
  fi
}

archive_bundle() {
  local bundle_name="$1"

  tar -C "$DIST_DIR" -czf "$DIST_DIR/$bundle_name.tar.gz" "$bundle_name"
  if command -v zip >/dev/null 2>&1; then
    (cd "$DIST_DIR" && zip -qr "$bundle_name.zip" "$bundle_name")
  fi
}

rustup target add "$TARGET"

mkdir -p "$BUILD_BIN_DIR"

# Cross-compilation for ARM requires the ALSA pkg-config metadata from the
# armhf sysroot. Set PKG_CONFIG env vars so alsa-sys finds the right headers.
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_LIBDIR=/usr/lib/arm-linux-gnueabihf/pkgconfig:/usr/share/pkgconfig

cargo build --release --target "$TARGET" -p pirate_synth
install -m 0755 "$ROOT_DIR/target/$TARGET/release/$BINARY_NAME" "$ARMHF_BINARY"

if [[ -n "$ARMV6_SOURCE_BINARY" ]]; then
  install -m 0755 "$ARMV6_SOURCE_BINARY" "$ARMV6_BINARY"
else
  CARGO_TARGET_ARM_UNKNOWN_LINUX_GNUEABIHF_RUSTFLAGS="$ARMV6_CPU_RUSTFLAGS" \
    cargo build --release --target "$TARGET" -p pirate_synth
  install -m 0755 "$ROOT_DIR/target/$TARGET/release/$BINARY_NAME" "$ARMV6_BINARY"
fi

stage_bundle "$ARMHF_STAGE_DIR" "$ARMHF_BINARY"
stage_bundle "$ARMV6_STAGE_DIR" "$ARMV6_BINARY"

archive_bundle "$ARMHF_BUNDLE_NAME"
archive_bundle "$ARMV6_BUNDLE_NAME"

# Remove staging dirs and temporary binaries now that archives are created.
rm -rf "$BUILD_BIN_DIR" "$ARMHF_STAGE_DIR" "$ARMV6_STAGE_DIR"

# Clean cross-compiled release and incremental artefacts now that packaging
# is complete; these are only useful during active development and can
# consume significant space after a release build.
cargo clean --release --target "$TARGET" 2>/dev/null || true

echo "Packaged bundles in: $DIST_DIR"
