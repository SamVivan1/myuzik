#!/usr/bin/env bash
#
# Install Myuzik's optional YouTube Music runtime.
#
#   ./scripts/setup-youtube.sh
#   PREFIX=/somewhere ./scripts/setup-youtube.sh
#
# Installs a private Python venv under $PREFIX/lib/myuzik/runtime and copies the
# helper there. The core player never imports Python; only the optional YouTube
# provider does.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
LIB_DIR="$PREFIX/lib/myuzik"
RUNTIME="$LIB_DIR/runtime"

say()  { printf '%s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }

for arg in "$@"; do
  case "$arg" in
    --prefix=*) PREFIX="${arg#*=}"; LIB_DIR="$PREFIX/lib/myuzik"; RUNTIME="$LIB_DIR/runtime" ;;
    -h|--help)
      sed -n '3,10p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) die "unknown option: $arg" ;;
  esac
done

command -v python3 >/dev/null 2>&1 || die "python3 not found (install python3 and python3-venv)"
[ -f "$ROOT/helper/ytmusic.py" ] || die "missing helper/ytmusic.py"
[ -f "$ROOT/helper/requirements.txt" ] || die "missing helper/requirements.txt"

if ! command -v node >/dev/null 2>&1; then
  warn "Node.js was not found. yt-dlp needs Node.js 20+ for YouTube playback."
else
  NODE_MAJOR="$(node -p 'Number(process.versions.node.split(".")[0])' 2>/dev/null || echo 0)"
  if [ "${NODE_MAJOR:-0}" -lt 20 ]; then
    warn "Node.js $NODE_MAJOR is too old; yt-dlp needs Node.js 20+."
  fi
fi
command -v ffmpeg >/dev/null 2>&1 || warn "ffmpeg was not found; most audio-only downloads still work, but keeping it installed is recommended."

say "Installing the YouTube helper into $LIB_DIR"
mkdir -p "$LIB_DIR"
install -m644 "$ROOT/helper/ytmusic.py" "$LIB_DIR/ytmusic.py"
install -m644 "$ROOT/helper/requirements.txt" "$LIB_DIR/requirements.txt"

if [ ! -x "$RUNTIME/bin/python" ]; then
  say "Creating the private Python environment"
  python3 -m venv "$RUNTIME" || die "could not create a venv; install python3-venv"
fi

say "Installing ytmusicapi and yt-dlp (pinned)"
"$RUNTIME/bin/python" -m pip install --disable-pip-version-check --upgrade -r "$LIB_DIR/requirements.txt"

say ""
say "YouTube Music support is installed."
say "Enable it in Myuzik: Settings → Sources → YouTube Music, or use the"
say "\"Use YouTube Music instead\" button on the first-run setup screen."
say ""
say "Update later with:"
say "  $RUNTIME/bin/python -m pip install --upgrade -r $LIB_DIR/requirements.txt"
