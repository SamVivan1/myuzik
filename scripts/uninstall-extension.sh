#!/usr/bin/env bash
# Remove the Myuzik Pin GNOME Shell extension from the current user.
# No sudo required, and safe to run repeatedly: every step is skipped when it is
# already done.
set -euo pipefail
cd "$(dirname "$0")/.."

REPO_DIR="$(pwd)"
UUID="myuzik-pin@samvivan1.github.io"
EXTENSIONS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions"
INSTALL_DIR="$EXTENSIONS_DIR/$UUID"
ZIP_NAME="$UUID.shell-extension.zip"

usage() {
  echo "Usage: scripts/uninstall-extension.sh"
}

for arg in "$@"; do
  case "$arg" in
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $arg"; usage; exit 1 ;;
  esac
done

# The running shell knows the extension only if it was installed before login.
if command -v gnome-extensions >/dev/null 2>&1; then
  if gnome-extensions disable "$UUID" 2>/dev/null; then
    echo "Disabled $UUID"
  fi
  if gnome-extensions uninstall "$UUID" 2>/dev/null; then
    echo "Uninstalled $UUID"
  fi
fi

# An install directory can exist without the running shell knowing about it.
if [ -d "$INSTALL_DIR" ]; then
  rm -rf -- "$INSTALL_DIR"
  echo "Removed $INSTALL_DIR"
fi

# Packed zips left behind by scripts/install-extension.sh
for zip in "$REPO_DIR/$ZIP_NAME" "$REPO_DIR/target/gnome-extension/$ZIP_NAME"; do
  if [ -f "$zip" ]; then
    rm -f -- "$zip"
    echo "Removed $zip"
  fi
done

# If the shell was never told about the extension, the UUID can linger in the
# enabled-extensions list until someone removes it.
if command -v gsettings >/dev/null 2>&1; then
  enabled="$(gsettings get org.gnome.shell enabled-extensions 2>/dev/null || true)"
  case "$enabled" in
    *"'$UUID'"*)
      updated="$(printf '%s\n' "$enabled" \
        | sed -e "s/'$UUID', //" -e "s/, '$UUID'//" -e "s/'$UUID'//")"
      case "$updated" in
        \[*\])
          gsettings set org.gnome.shell enabled-extensions "$updated"
          echo "Removed $UUID from org.gnome.shell enabled-extensions"
          ;;
        *)
          echo "Could not rewrite org.gnome.shell enabled-extensions safely." >&2
          echo "Remove '$UUID' from it with the Extensions app." >&2
          ;;
      esac
      ;;
  esac
fi

if [ -e "$INSTALL_DIR" ] || { command -v gnome-extensions >/dev/null 2>&1 \
     && gnome-extensions info "$UUID" >/dev/null 2>&1; }; then
  echo "$UUID is still present." >&2
  exit 1
fi

echo "myuzik-pin is not installed."
