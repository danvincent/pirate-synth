#!/usr/bin/env bash
# Install all system packages required to build pirate-synth on a fresh Debian/Ubuntu machine.
#
# What this installs and why:
#   build-essential            - native gcc, make, etc. (needed by Cargo build scripts)
#   pkg-config                 - used by alsa-sys to locate ALSA headers
#   libasound2-dev             - ALSA headers for native builds (midir → alsa-sys)
#   gcc-arm-linux-gnueabihf    - ARM cross-linker used by .cargo/config.toml
#   libasound2-dev:armhf       - ALSA headers for the armhf cross-build (requires armhf arch)
#   zip                        - used by scripts/package.sh to produce .zip bundles
#
# Rust / Cargo / rustup are NOT managed here.  If rustup is not present the
# script will print instructions and exit rather than silently installing it,
# since rustup wants to run as the target user, not root.
set -euo pipefail

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
info()  { printf '\033[1;34m==> %s\033[0m\n' "$*"; }
warn()  { printf '\033[1;33mWARN: %s\033[0m\n' "$*"; }
die()   { printf '\033[1;31mERROR: %s\033[0m\n' "$*" >&2; exit 1; }

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "'$1' not found. $2"
}

# ---------------------------------------------------------------------------
# Sanity checks
# ---------------------------------------------------------------------------
if [[ "$(uname -s)" != "Linux" ]]; then
    die "This script targets Debian/Ubuntu Linux only."
fi

if ! command -v apt-get >/dev/null 2>&1; then
    die "apt-get not found. Only Debian/Ubuntu-based distros are supported."
fi

# ---------------------------------------------------------------------------
# Resolve the user whose rustup / cargo we should use.
# When invoked via `sudo`, SUDO_USER is the original caller.
# When invoked directly as root, fall back to root itself.
# ---------------------------------------------------------------------------
REAL_USER="${SUDO_USER:-$(whoami)}"
REAL_HOME="$(eval echo "~$REAL_USER")"
RUSTUP_BIN="$REAL_HOME/.cargo/bin/rustup"

run_as_user() {
    if [[ "$REAL_USER" == "$(whoami)" ]]; then
        "$@"
    else
        sudo -u "$REAL_USER" "$@"
    fi
}

# Run a privileged command: directly if already root, via sudo otherwise.
apt_cmd() {
    if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
        "$@"
    else
        require_cmd sudo "sudo is required to install packages. Re-run as root or install sudo."
        sudo "$@"
    fi
}

# ---------------------------------------------------------------------------
# Rust toolchain check
# ---------------------------------------------------------------------------
if [[ ! -x "$RUSTUP_BIN" ]]; then
    cat <<EOF

rustup is not installed for user '$REAL_USER'. Install it as that user with:

    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

Then re-run this script.
EOF
    exit 1
fi

# ---------------------------------------------------------------------------
# System packages
# ---------------------------------------------------------------------------
PACKAGES=(
    build-essential
    pkg-config
    libasound2-dev
    gcc-arm-linux-gnueabihf
    zip
)

info "Enabling armhf architecture (required for libasound2-dev:armhf)"
apt_cmd dpkg --add-architecture armhf

info "Updating package index"
apt_cmd apt-get update -q

info "Installing native packages: ${PACKAGES[*]}"
apt_cmd apt-get install -y "${PACKAGES[@]}"

info "Installing armhf ALSA headers (for ARM cross-compilation)"
apt_cmd apt-get install -y libasound2-dev:armhf

# ---------------------------------------------------------------------------
# Rust target
# ---------------------------------------------------------------------------
TARGET="arm-unknown-linux-gnueabihf"
info "Adding Rust target: $TARGET (as user '$REAL_USER')"
run_as_user "$RUSTUP_BIN" target add "$TARGET"

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------
info "All build dependencies installed."
echo
echo "You can now build with:"
echo "  cargo build --release --target $TARGET"
echo "  scripts/package.sh"
