#!/usr/bin/env bash
# Build and install Myuzik into the user's XDG directories. No sudo required.
set -euo pipefail
cd "$(dirname "$0")/.."

PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
APP_DIR="$PREFIX/share/applications"
ICON_DIR="$PREFIX/share/icons/hicolor/scalable/apps"

usage() {
  echo "Usage: scripts/install.sh [--uninstall] [--with-extension]"
  echo "  --uninstall       remove installed Myuzik files"
  echo "  --with-extension  also install/enable the GNOME pin extension"
}

uninstall() {
  rm -f "$BIN_DIR/myuzik"
  rm -f "$APP_DIR/io.github.samvivan1.Myuzik.desktop"
  rm -f "$ICON_DIR/io.github.samvivan1.Myuzik.svg"
  echo "Removed Myuzik from $PREFIX"
}

install_app() {
  echo "Building release binary..."
  cargo build --release
  install -Dm755 "target/release/myuzik" "$BIN_DIR/myuzik"
  install -Dm644 "packaging/io.github.samvivan1.Myuzik.desktop" "$APP_DIR/io.github.samvivan1.Myuzik.desktop"
  install -Dm644 "packaging/icons/hicolor/scalable/apps/io.github.samvivan1.Myuzik.svg" "$ICON_DIR/io.github.samvivan1.Myuzik.svg"
  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APP_DIR" || true
  fi
  if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" >/dev/null 2>&1 || true
  fi
  echo "Installed myuzik to $BIN_DIR/myuzik"
  echo "Ensure $BIN_DIR is on your PATH."
}

WITH_EXTENSION=0
for arg in "$@"; do
  case "$arg" in
    --uninstall) uninstall; exit 0 ;;
    --with-extension) WITH_EXTENSION=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $arg"; usage; exit 1 ;;
  esac
done

install_app
if [ "$WITH_EXTENSION" = "1" ]; then
  bash scripts/install-extension.sh
fi
