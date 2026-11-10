#!/usr/bin/env bash
#
# Myuzik — single-command installer.
#
# Builds the release binary and installs everything into the user's home
# (no sudo): binary, desktop entry, icon, AppStream metadata, desktop/icon
# caches, and optionally the GNOME pin extension for the lyrics overlay.
#
#   ./install.sh                 install (builds first)
#   ./install.sh --no-extension  skip the GNOME pin extension
#   ./install.sh --skip-build    reuse target/release/myuzik
#   ./install.sh --prefix DIR    install somewhere other than ~/.local
#   ./install.sh --help
#
# Why the desktop entry gets an absolute Exec path
# -----------------------------------------------
# GNOME Shell runs as a systemd user service and launches apps with the
# *session* PATH, which on many systems does not include ~/.local/bin. A
# desktop entry saying "Exec=myuzik" then cannot be loaded at all, and an app
# that cannot be loaded is not listed in the overview and cannot be found by
# search. Writing the absolute path makes the entry load in any environment,
# with no PATH change and no logout required.
#
# Uninstall with ./uninstall.sh (add --purge to drop config and caches).
set -euo pipefail

cd "$(dirname "$0")"

APP_ID="io.github.samvivan1.Myuzik"
EXT_UUID="myuzik-pin@samvivan1.github.io"

PREFIX="${PREFIX:-$HOME/.local}"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
BIN_DIR="$PREFIX/bin"
APPS_DIR="$DATA_HOME/applications"
ICON_DIR="$DATA_HOME/icons/hicolor/scalable/apps"
METAINFO_DIR="$DATA_HOME/metainfo"

DESKTOP_SRC="packaging/$APP_ID.desktop"
ICON_SRC="packaging/icons/hicolor/scalable/apps/$APP_ID.svg"
METAINFO_SRC="packaging/$APP_ID.metainfo.xml"

BUILD=1
EXTENSION=1

usage() {
  sed -n '3,14p' "$0" | sed 's/^# \{0,1\}//'
  echo
  echo "Environment: PREFIX (default ~/.local), XDG_DATA_HOME (default ~/.local/share)"
  echo "Extension-only controls: scripts/install-extension.sh --help"
}

for arg in "$@"; do
  case "$arg" in
    --no-extension) EXTENSION=0 ;;
    --skip-build) BUILD=0 ;;
    --prefix=*) PREFIX="${arg#*=}"; BIN_DIR="$PREFIX/bin" ;;
    --prefix) echo "Use --prefix=DIR" >&2; exit 1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; usage; exit 1 ;;
  esac
done

say()  { printf '%s\n' "$*"; }
step() { printf '\n== %s ==\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }

step "Checking prerequisites"
if [ "$BUILD" = 1 ]; then
  command -v cargo >/dev/null 2>&1 || die "cargo not found (install Rust, or pass --skip-build)"
fi
[ -f "$DESKTOP_SRC" ]  || die "missing $DESKTOP_SRC"
[ -f "$ICON_SRC" ]     || die "missing $ICON_SRC"

step "Building release binary"
if [ "$BUILD" = 1 ]; then
  cargo build --release
else
  say "skipped (--skip-build)"
fi
BINARY="target/release/myuzik"
[ -x "$BINARY" ] || die "$BINARY not found — run without --skip-build"

step "Installing binary"
install -Dm755 "$BINARY" "$BIN_DIR/myuzik"
say "$BIN_DIR/myuzik"

step "Installing desktop entry"
mkdir -p "$APPS_DIR"
# Absolute Exec so the entry loads under the systemd session PATH.
if printf '%s' "$BIN_DIR" | grep -q ' '; then
  EXEC_VALUE="\"$BIN_DIR/myuzik\""
else
  EXEC_VALUE="$BIN_DIR/myuzik"
fi
sed "s|^Exec=.*|Exec=$EXEC_VALUE|" "$DESKTOP_SRC" > "$APPS_DIR/$APP_ID.desktop"
chmod 644 "$APPS_DIR/$APP_ID.desktop"
say "$APPS_DIR/$APP_ID.desktop  (Exec=$EXEC_VALUE)"

step "Installing icon and AppStream metadata"
install -Dm644 "$ICON_SRC" "$ICON_DIR/$APP_ID.svg"
say "$ICON_DIR/$APP_ID.svg"
if [ -f "$METAINFO_SRC" ]; then
  mkdir -p "$METAINFO_DIR"
  install -m644 "$METAINFO_SRC" "$METAINFO_DIR/$APP_ID.metainfo.xml"
  say "$METAINFO_DIR/$APP_ID.metainfo.xml"
fi

step "Refreshing desktop and icon caches"
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APPS_DIR" >/dev/null 2>&1 || warn "update-desktop-database failed"
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$DATA_HOME/icons/hicolor" >/dev/null 2>&1 \
    || warn "gtk-update-icon-cache failed"
fi
say "done"

if [ "$EXTENSION" = 1 ]; then
  step "Installing the GNOME pin extension (optional)"
  if command -v gnome-extensions >/dev/null 2>&1; then
    # Enabling can legitimately fail on Wayland until the next login; that is
    # not an install failure, so do not abort on it.
    if ! bash scripts/install-extension.sh; then
      warn "extension could not be enabled now — log out and back in, then run:"
      warn "  gnome-extensions enable $EXT_UUID"
    fi
  else
    warn "gnome-extensions not found; skipping (the overlay still works as a normal window)"
  fi
else
  step "Skipping the GNOME pin extension (--no-extension)"
  say "install later with: scripts/install-extension.sh"
fi

step "Verifying"
FAIL=0
[ -x "$BIN_DIR/myuzik" ] || { warn "binary missing"; FAIL=1; }
if command -v desktop-file-validate >/dev/null 2>&1; then
  if desktop-file-validate "$APPS_DIR/$APP_ID.desktop"; then
    say "desktop entry is valid"
  else
    warn "desktop entry has validation warnings"
    FAIL=1
  fi
fi

# Ask GAppInfo — the same registry GNOME Shell's search uses — whether the app
# is visible, with the PATH the shell would use rather than this shell's PATH.
SESSION_PATH="$(systemctl --user show-environment 2>/dev/null | sed -n 's/^PATH=//p' || true)"
[ -n "${SESSION_PATH:-}" ] || SESSION_PATH="$PATH"
if command -v python3 >/dev/null 2>&1 && python3 -c 'import gi' >/dev/null 2>&1; then
  RESULT="$(PATH="$SESSION_PATH" python3 - "$APP_ID" <<'PY' 2>/dev/null || true
import sys
import gi
gi.require_version('Gio', '2.0')
from gi.repository import Gio
want = sys.argv[1] + '.desktop'
for app in Gio.AppInfo.get_all():
    if app.get_id() == want:
        print('visible' if app.should_show() else 'hidden')
        break
else:
    print('not-listed')
PY
)"
  case "$RESULT" in
    visible)
      say "app registry (GNOME search) sees Myuzik and will show it" ;;
    hidden)
      warn "app registry sees Myuzik but hides it (should_show=false)"; FAIL=1 ;;
    not-listed)
      warn "app registry does not list Myuzik under the session PATH"; FAIL=1 ;;
    *)
      warn "could not query the app registry (python3-gi unavailable?) — skipping check" ;;
  esac
else
  warn "python3-gi not available; skipped the app-registry check"
fi

step "Installed"
say "Launch:   $BIN_DIR/myuzik"
say "Search:   open Activities and type \"Myuzik\""
if [ "$FAIL" != 0 ]; then
  say ""
  warn "some checks failed — see the messages above"
  exit 1
fi
say ""
say "If the entry does not show up immediately, log out and back in once."
say "Remove everything later with: ./uninstall.sh          (add --purge to drop config + caches)"
