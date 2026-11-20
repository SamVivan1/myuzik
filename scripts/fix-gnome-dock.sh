#!/usr/bin/env bash
#
# Diagnose and fix the GNOME dock icon for Myuzik.
#
# Myuzik's desktop entry and icons are standard (app id
# io.github.samvivan1.Myuzik, hicolor icon set). If the icon is missing or a
# different app's icon appears in the dock, the usual cause is the third-party
# Dash2Dock Animated extension not supporting the running GNOME Shell version.
# Ubuntu Dock is version-matched to the Shell and is the safe fallback.
#
#   scripts/fix-gnome-dock.sh                 show status only
#   scripts/fix-gnome-dock.sh --use-ubuntu-dock
#   scripts/fix-gnome-dock.sh --restore-dash2dock
set -euo pipefail

DASH="dash2dock-lite@icedman.github.com"
UBUNTU="ubuntu-dock@ubuntu.com"
ACTION="status"

for arg in "$@"; do
  case "$arg" in
    --use-ubuntu-dock) ACTION="ubuntu" ;;
    --restore-dash2dock) ACTION="dash" ;;
    -h|--help) sed -n '3,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $arg" >&2; exit 1 ;;
  esac
done

say() { printf '%s\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }

have gnome-extensions || { say "gnome-extensions not found; this script is for GNOME."; exit 0; }

SHELL_VERSION="$(gnome-shell --version 2>/dev/null | awk '{print $3}' || true)"
SHELL_MAJOR="${SHELL_VERSION%%.*}"
DASH_DIR="$HOME/.local/share/gnome-shell/extensions/$DASH"
DASH_META="$DASH_DIR/metadata.json"
DASH_SUPPORTED="unknown"
if [ -f "$DASH_META" ] && [ -n "$SHELL_MAJOR" ]; then
  if grep -q "\"$SHELL_MAJOR\"" "$DASH_META"; then
    DASH_SUPPORTED="yes"
  else
    DASH_SUPPORTED="no"
  fi
fi

dash_enabled() { gnome-extensions info "$DASH" 2>/dev/null | grep -q 'Enabled: Yes'; }
ubuntu_enabled() { gnome-extensions info "$UBUNTU" 2>/dev/null | grep -q 'Enabled: Yes'; }

case "$ACTION" in
  status)
    say "GNOME Shell: ${SHELL_VERSION:-unknown}"
    say "Dash2Dock Lite installed: $([ -d "$DASH_DIR" ] && echo yes || echo no)"
    say "Dash2Dock Lite supports this Shell: $DASH_SUPPORTED"
    say "Dash2Dock Lite enabled: $(dash_enabled && echo yes || echo no)"
    say "Ubuntu Dock enabled:    $(ubuntu_enabled && echo yes || echo no)"
    say ""
    if dash_enabled && [ "$DASH_SUPPORTED" = "no" ]; then
      say "The active Dash2Dock build does not list GNOME Shell $SHELL_MAJOR in its metadata."
      say "That extension reads GNOME Shell internals and can render missing/wrong dock icons."
      say "Safe fallback: scripts/fix-gnome-dock.sh --use-ubuntu-dock"
    elif ! dash_enabled && ! ubuntu_enabled; then
      say "No external dock is active. GNOME's built-in dash should still use the desktop icon."
    fi
    ;;
  ubuntu)
    if [ ! -d "/usr/share/gnome-shell/extensions/$UBUNTU" ] && [ ! -d "$HOME/.local/share/gnome-shell/extensions/$UBUNTU" ]; then
      say "Ubuntu Dock is not installed; update GNOME Shell's dock extension instead." >&2
      exit 1
    fi
    if dash_enabled; then gnome-extensions disable "$DASH"; fi
    gnome-extensions enable "$UBUNTU"
    say "Switched to Ubuntu Dock. Restore with: $0 --restore-dash2dock"
    ;;
  dash)
    if ubuntu_enabled; then gnome-extensions disable "$UBUNTU"; fi
    gnome-extensions enable "$DASH"
    say "Dash2Dock Lite re-enabled. If dock icons are wrong, its build likely does not support GNOME Shell ${SHELL_MAJOR:-this version}."
    ;;
esac
