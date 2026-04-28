#!/usr/bin/env bash
# deploy.sh — push a built package to one or more pirate-synth targets and
# run the update script on each.
#
# Usage:
#   scripts/deploy.sh [--armv6] <user@host> [<user@host> ...]
#
# Options:
#   --armv6   Deploy the ARMv6 build (default: ARMhf)
#   --gpi     Deploy the GPi CASE build (ARMv6 + GPi config/service)
#
# Examples:
#   scripts/deploy.sh user@192.168.1.50
#   scripts/deploy.sh --armv6 user@pizero1 user@pizero2
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"
BUNDLE_NAME="pirate-synth-sdcard"
CONFIG_FILE="config.toml"
UPDATE_SCRIPT="$ROOT_DIR/scripts/update.sh"

# --- argument parsing -------------------------------------------------------
TARGETS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --armv6)
      BUNDLE_NAME="pirate-synth-sdcard-armv6"
      shift
      ;;
    --gpi)
      BUNDLE_NAME="pirate-synth-sdcard-gpi"
      CONFIG_FILE="config-gpi.toml"
      shift
      ;;
    -*)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
    *)
      TARGETS+=("$1")
      shift
      ;;
  esac
done

if [[ ${#TARGETS[@]} -eq 0 ]]; then
  echo "Usage: $0 [--armv6] <user@host> [<user@host> ...]" >&2
  exit 1
fi

read -rp "Reset config to default on target(s)? [y/N] " RESET_ANSWER
RESET_CONFIG=false
[[ "$RESET_ANSWER" =~ ^[Yy]$ ]] && RESET_CONFIG=true

SRC_DIR="$DIST_DIR/$BUNDLE_NAME/boot/firmware/pirate-synth"

if [[ ! -d "$SRC_DIR" ]]; then
  echo "ERROR: $SRC_DIR not found. Did you run scripts/package.sh first?" >&2
  exit 1
fi

# --- deploy to each target --------------------------------------------------
for TARGET in "${TARGETS[@]}"; do
  echo "===> Deploying to $TARGET"

  REMOTE_HOME=$(ssh "$TARGET" 'echo $HOME')
  REMOTE_STAGING="$REMOTE_HOME/pirate-synth"
  REMOTE_UPDATE_SCRIPT="$REMOTE_HOME/update.sh"

  echo "  --> Syncing files to $TARGET:$REMOTE_STAGING ..."
  rsync -az --delete "$SRC_DIR/" "$TARGET:$REMOTE_STAGING/"

  if [[ "$RESET_CONFIG" == "true" ]]; then
    echo "  --> Resetting config to default on $TARGET ..."
    ssh -t -t "$TARGET" "sudo install -m 0644 $REMOTE_STAGING/config/$CONFIG_FILE /etc/pirate-synth/config.toml"
  fi

  echo "  --> Copying update script to $TARGET:$REMOTE_UPDATE_SCRIPT ..."
  rsync -az "$UPDATE_SCRIPT" "$TARGET:$REMOTE_UPDATE_SCRIPT"

  echo "  --> Running update on $TARGET (sudo required) ..."
  # -t -t forces PTY allocation even when local stdin is not a terminal, allowing sudo to prompt for a password.
  ssh -t -t "$TARGET" "sudo bash $REMOTE_UPDATE_SCRIPT"

  echo "===> $TARGET done"
done
