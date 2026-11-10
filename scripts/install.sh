#!/usr/bin/env bash
#
# Compatibility wrapper: the installer lives at the repository root.
# Use ./install.sh (single command, no arguments needed).
#
# Almost every option is the root installer's; --uninstall is kept for older
# documentation and forwards to ./uninstall.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

case "${1:-}" in
  --uninstall)
    shift
    exec ./uninstall.sh "$@"
    ;;
esac

exec ./install.sh "$@"
