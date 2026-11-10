#!/usr/bin/env bash
#
# Myuzik — single-command uninstaller.
#
# Removes everything ./install.sh created: the binary, desktop entry, icon,
# AppStream metadata, the GNOME pin extension and the refreshed caches.
#
#   ./uninstall.sh              remove the app and the extension
#   ./uninstall.sh --keep-extension   keep the GNOME pin extension
#   ./uninstall.sh --purge      also delete the config and the caches
#   ./uninstall.sh --dry-run    show what would be removed
#   ./uninstall.sh --help
#
# It is safe to run repeatedly: anything already gone is reported as such.
set -euo pipefail

cd "$(dirname "$0")"

APP_ID="io.github.samvivan1.Myuzik"

PREFIX="${PREFIX:-$HOME/.local}"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}"
BIN_DIR="$PREFIX/bin"
APPS_DIR="$DATA_HOME/applications"
ICON_DIR="$DATA_HOME/icons/hicolor/scalable/apps"
METAINFO_DIR="$DATA_HOME/metainfo"

DRY=0
PURGE=0
KEEP_EXTENSION=0

usage() {
  sed -n '3,14p' "$0" | sed 's/^# \{0,1\}//'
  echo
  echo "Environment: PREFIX (default ~/.local), XDG_DATA_HOME, XDG_CONFIG_HOME, XDG_CACHE_HOME"
}

for arg in "$@"; do
  case "$arg" in
    --purge) PURGE=1 ;;
    --keep-extension) KEEP_EXTENSION=1 ;;
    --dry-run) DRY=1 ;;
    --prefix=*) PREFIX="${arg#*=}"; BIN_DIR="$PREFIX/bin" ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; usage; exit 1 ;;
  esac
done

say()  { printf '%s\n' "$*"; }
step() { printf '\n== %s ==\n' "$*"; }

# remove a file/symlink/dir, reporting what happened
remove_path() {
  local path="$1" kind="$2"
  if [ -e "$path" ] || [ -L "$path" ]; then
    if [ "$DRY" = 1 ]; then
      say "would remove $kind: $path"
    else
      rm -rf "$path"
      say "removed $kind: $path"
    fi
    return 0
  fi
  say "not installed ($kind): $path"
  return 1
}

REMOVED=0

step "Removing Myuzik"
remove_path "$BIN_DIR/myuzik" "binary" && REMOVED=1
remove_path "$APPS_DIR/$APP_ID.desktop" "desktop entry" && REMOVED=1
remove_path "$ICON_DIR/$APP_ID.svg" "icon" && REMOVED=1
remove_path "$METAINFO_DIR/$APP_ID.metainfo.xml" "AppStream metadata" && REMOVED=1

if [ "$KEEP_EXTENSION" = 0 ]; then
  step "Removing the GNOME pin extension"
  if [ "$DRY" = 1 ]; then
    say "would run: scripts/uninstall-extension.sh"
  elif [ -x scripts/uninstall-extension.sh ] || [ -f scripts/uninstall-extension.sh ]; then
    bash scripts/uninstall-extension.sh || say "extension was not installed"
  else
    say "scripts/uninstall-extension.sh not found; skipping"
  fi
else
  step "Keeping the GNOME pin extension (--keep-extension)"
fi

if [ "$PURGE" = 1 ]; then
  step "Purging configuration and caches"
  remove_path "$CONFIG_HOME/myuzik" "config directory" || true
  remove_path "$CACHE_HOME/myuzik" "cache directory" || true
else
  step "Keeping configuration and caches"
  say "config: $CONFIG_HOME/myuzik"
  say "cache:  $CACHE_HOME/myuzik"
  say "pass --purge to delete them as well"
fi

if [ "$DRY" = 0 ]; then
  step "Refreshing desktop and icon caches"
  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APPS_DIR" >/dev/null 2>&1 || true
  fi
  if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$DATA_HOME/icons/hicolor" >/dev/null 2>&1 || true
  fi
  say "done"
fi

step "Finished"
if [ "$REMOVED" = 0 ]; then
  say "Myuzik was not installed in $PREFIX — nothing to remove."
elif [ "$DRY" = 1 ]; then
  say "Dry run only; nothing was deleted."
else
  say "Myuzik has been removed."
  say "If it still appears in Activities, log out and back in once."
fi
