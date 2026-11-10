#!/usr/bin/env bash
# Pack and install the Myuzik Pin GNOME Shell extension for the current user.
# No sudo required. On Wayland the shell only scans for new extensions at login,
# so a first install needs a log out / log in before it can be enabled.
set -euo pipefail
cd "$(dirname "$0")/.."

REPO_DIR="$(pwd)"
UUID="myuzik-pin@samvivan1.github.io"
EXT_DIR="$REPO_DIR/gnome-extension/$UUID"
BUILD_DIR="$REPO_DIR/target/gnome-extension"
ZIP="$BUILD_DIR/$UUID.shell-extension.zip"

usage() {
  echo "Usage: scripts/install-extension.sh [--pack-only]"
  echo "  --pack-only  validate and pack the extension, do not install or enable it"
}

PACK_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --pack-only) PACK_ONLY=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $arg"; usage; exit 1 ;;
  esac
done

if [ ! -d "$EXT_DIR" ]; then
  echo "Extension sources not found at $EXT_DIR" >&2
  exit 1
fi

if ! command -v gnome-extensions >/dev/null 2>&1; then
  echo "gnome-extensions not found. Install gnome-shell first." >&2
  exit 1
fi

mkdir -p "$BUILD_DIR"
gnome-extensions pack "$EXT_DIR" --force --out-dir="$BUILD_DIR"
echo "Packed $ZIP"

if [ "$PACK_ONLY" = "1" ]; then
  exit 0
fi

gnome-extensions install --force "$ZIP"
echo "Installed $UUID into ${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/$UUID"

if enable_output="$(gnome-extensions enable "$UUID" 2>&1)"; then
  echo "Enabled $UUID"
else
  echo "Could not enable $UUID yet:"
  echo "  $enable_output"
  echo "GNOME only scans extension directories at session start, so a first install"
  echo "on Wayland needs a log out / log in. Then run:"
  echo "  gnome-extensions enable $UUID"
fi

echo
echo "Verify with:"
echo "  gnome-extensions info $UUID"
echo "Uninstall with:"
echo "  scripts/uninstall-extension.sh"
