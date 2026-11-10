#!/usr/bin/env bash
# Regenerate the Myuzik PNG icon set from the scalable SVG master.
#
# The SVG is the single source of truth. The PNGs are build products, but they are
# committed so that installing the app never needs Inkscape.
#
#   scripts/render-icons.sh           rewrite every PNG from the SVG
#   scripts/render-icons.sh --check   verify each PNG exists and is newer than the SVG
#   scripts/render-icons.sh --help
#
# Needs Inkscape (any 1.x). --check needs no external tool.
set -euo pipefail

cd "$(dirname "$0")/.."

APP_ID="io.github.samvivan1.Myuzik"
ICON_ROOT="packaging/icons/hicolor"
MASTER="$ICON_ROOT/scalable/apps/$APP_ID.svg"
SIZES="16 24 32 48 64 128 256 512"

CHECK=0
for arg in "$@"; do
  case "$arg" in
    --check) CHECK=1 ;;
    -h|--help) sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; exit 1 ;;
  esac
done

[ -f "$MASTER" ] || { echo "error: missing $MASTER" >&2; exit 1; }

if [ "$CHECK" = 1 ]; then
  FAIL=0
  for size in $SIZES; do
    png="$ICON_ROOT/${size}x${size}/apps/$APP_ID.png"
    if [ ! -f "$png" ]; then
      echo "missing: $png" >&2
      FAIL=1
    elif [ "$MASTER" -nt "$png" ]; then
      echo "stale (SVG is newer): $png" >&2
      FAIL=1
    fi
  done
  if [ "$FAIL" != 0 ]; then
    echo "run scripts/render-icons.sh to regenerate the set" >&2
    exit 1
  fi
  echo "icon set is up to date (${SIZES// /, } px)"
  exit 0
fi

command -v inkscape >/dev/null 2>&1 \
  || { echo "error: inkscape not found; install it to regenerate the PNG icons" >&2; exit 1; }

for size in $SIZES; do
  png="$ICON_ROOT/${size}x${size}/apps/$APP_ID.png"
  mkdir -p "$(dirname "$png")"
  inkscape "$MASTER" --export-type=png --export-filename="$png" \
    --export-width="$size" --export-height="$size" >/dev/null
  printf '%s\n' "$png"
done

echo "done: $(echo $SIZES | wc -w) sizes from ${MASTER#"$PWD"/}"
