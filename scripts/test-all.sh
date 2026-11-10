#!/usr/bin/env bash
# Full verification: format, lint, unit tests, headless UI tests, and (optionally) live server tests.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== fmt =="
cargo fmt --check

echo "== clippy =="
cargo clippy --all-targets -- -D warnings

echo "== tests (serial; GPUI tests share global state) =="
cargo test -- --test-threads=1

if [ "${MYUZIK_LIVE:-0}" = "1" ]; then
  echo "== live Navidrome + GStreamer tests =="
  cargo test --test live_navidrome --test live_player -- --ignored --nocapture --test-threads=1
else
  echo "== live tests skipped (set MYUZIK_LIVE=1 to run) =="
fi
