#!/usr/bin/env bash
set -euo pipefail

EXT_DIR="$(cd "$(dirname "$0")/../gnome-extension/myuzik-pin@samvivan1.github.io" && pwd)"
OUT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

if ! command -v gnome-extensions >/dev/null 2>&1; then
    echo "gnome-extensions not found. Install gnome-shell first." >&2
    exit 1
fi

cd "$EXT_DIR"
gnome-extensions pack . --force --out-dir="$OUT_DIR"
ZIP="$OUT_DIR/myuzik-pin@samvivan1.github.io.shell-extension.zip"
gnome-extensions install --force "$ZIP"
gnome-extensions enable myuzik-pin@samvivan1.github.io

echo "Installed myuzik-pin@samvivan1.github.io"
echo "If this is the first install on Wayland, log out and log in so GNOME Shell loads it."
