#!/usr/bin/env bash
# Measure Myuzik's memory, CPU and startup time against the v1 budgets.
#
# Budgets (docs/superpowers/plans/2026-11-09-myuzik-v1.md):
#   idle RSS                      <= 200 MB
#   playing + lyrics overlay RSS  <= 250 MB
#   startup (time to a window)    <= 1.5 s
#
# Usage:
#   scripts/measure-ram.sh [options]
#
# Options:
#   -b, --binary PATH     binary to measure (default: target/release/myuzik)
#   -d, --duration SEC    seconds sampled per phase (default: 60; accepts 30m, 1h)
#   -s, --sample SEC      seconds between samples (default: 5)
#       --interactive     prompt before the playback and overlay phases
#       --non-interactive skip the playback and overlay phases
#       --no-startup      skip the startup probe
#       --strict          exit 1 when a measured number is over budget
#   -h, --help            show this help
#
# Env overrides: MYUZIK_BIN, MYUZIK_MEASURE_SECONDS, MYUZIK_SAMPLE_SECONDS
#
# What is measured:
#   * RSS from /proc/<pid>/status VmRSS (peak, last, min per phase),
#   * CPU % from /proc/<pid>/stat (utime + stime over the phase wall time),
#   * threads and open fds (growth there would mean a leak),
#   * startup: the binary is run once with MYUZIK_WINDOW_TEST=close, which closes the
#     window 1 s after it opens and exits, so (wall time - 1 s) is the time to a window.
#     That includes process teardown, so treat it as a slight upper bound. It measures
#     the window being created, not a presented frame (see docs/performance.md).
#   * cover/lyrics cache size on disk, which is part of the memory story.
#
# It needs a built binary and a graphical session (Wayland or X11). Without either it
# says what it cannot measure and exits 0, so it is safe to run from CI or over ssh.
# The script itself never touches the network; the app talks to its configured server.
#
# Example:
#   cargo build --release
#   scripts/measure-ram.sh --duration 60 --interactive
set -euo pipefail
cd "$(dirname "$0")/.."

IDLE_BUDGET_MB=200
PLAYING_BUDGET_MB=250
STARTUP_BUDGET_S=1.5

BINARY="${MYUZIK_BIN:-target/release/myuzik}"
DURATION="${MYUZIK_MEASURE_SECONDS:-60}"
SAMPLE="${MYUZIK_SAMPLE_SECONDS:-5}"
INTERACTIVE=auto
DO_STARTUP=1
STRICT=0

TMP_FILES=()
APP_PID=""
APP_EXITED=0
CONFIGURED=1
FAILED=0
PHASE_RESULT=""
PHASE_PEAK_KB=0
PHASE_LAST_KB=0
PEAK_PLAYING_KB=0
STARTUP_S=""

usage() {
  cat <<'EOF'
Measure Myuzik's memory, CPU and startup time against the v1 budgets.

Usage: scripts/measure-ram.sh [options]

  -b, --binary PATH     binary to measure (default: target/release/myuzik)
  -d, --duration SEC    seconds sampled per phase (default: 60; accepts 30m, 1h)
  -s, --sample SEC      seconds between samples (default: 5)
      --interactive     prompt before the playback and overlay phases
      --non-interactive skip the playback and overlay phases
      --no-startup      skip the startup probe
      --strict          exit 1 when a measured number is over budget
  -h, --help            show this help

Budgets: idle RSS <= 200 MB, playing + overlay RSS <= 250 MB, startup <= 1.5 s.

Env: MYUZIK_BIN, MYUZIK_MEASURE_SECONDS, MYUZIK_SAMPLE_SECONDS.

Needs a built binary and a Wayland/X11 session; without them it explains what it cannot
measure and exits 0. Playback and overlay phases need a configured Navidrome library.
EOF
}

cleanup() {
  if [ -n "$APP_PID" ] && kill -0 "$APP_PID" 2>/dev/null; then
    kill "$APP_PID" 2>/dev/null || true
    wait "$APP_PID" 2>/dev/null || true
  fi
  if [ "${#TMP_FILES[@]}" -gt 0 ]; then
    rm -f "${TMP_FILES[@]}"
  fi
}
trap cleanup EXIT INT TERM

die() {
  echo "measure-ram: $*" >&2
  exit 1
}

need_arg() { # option name, remaining count
  [ "$2" -ge 2 ] || die "option $1 needs a value (try --help)"
}

# "30" / "30s" / "5m" / "1h" -> seconds
to_seconds() {
  local raw=$1
  case "$raw" in
    *h) echo $(( ${raw%h} * 3600 )) ;;
    *m) echo $(( ${raw%m} * 60 )) ;;
    *s) echo "${raw%s}" ;;
    *) echo "$raw" ;;
  esac
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    -b|--binary) need_arg "$1" "$#"; BINARY="$2"; shift 2 ;;
    -d|--duration) need_arg "$1" "$#"; DURATION="$(to_seconds "$2")"; shift 2 ;;
    -s|--sample) need_arg "$1" "$#"; SAMPLE="$(to_seconds "$2")"; shift 2 ;;
    --interactive) INTERACTIVE=yes; shift ;;
    --non-interactive) INTERACTIVE=no; shift ;;
    --no-startup) DO_STARTUP=0; shift ;;
    --strict) STRICT=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
done

case "$DURATION" in
  ''|*[!0-9]*) die "--duration must be whole seconds, minutes (30m) or hours (1h), got '$DURATION'" ;;
esac
case "$SAMPLE" in
  ''|*[!0-9]*) die "--sample must be whole seconds, got '$SAMPLE'" ;;
esac
[ "$SAMPLE" -ge 1 ] || die "--sample must be at least 1 second"
[ "$DURATION" -ge "$SAMPLE" ] || die "--duration must be at least one sample interval"

if [ "$INTERACTIVE" = auto ]; then
  if [ -t 0 ]; then INTERACTIVE=yes; else INTERACTIVE=no; fi
fi

# ---- environment facts ----------------------------------------------------

[ -x "$BINARY" ] || die "no binary at '$BINARY' (build it with: cargo build --release, or pass --binary PATH)"

if [ -z "${WAYLAND_DISPLAY:-}" ] && [ -z "${DISPLAY:-}" ]; then
  cat <<'EOF'
measure-ram: no graphical session (WAYLAND_DISPLAY and DISPLAY are both unset).

Myuzik is a GPUI window, so RSS, CPU and startup can only be measured where a window can
be opened. Run this on the desktop session:

    cargo build --release && scripts/measure-ram.sh --duration 60 --interactive

Skipping the measurement (exit 0).
EOF
  exit 0
fi

CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/myuzik"
CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/myuzik"
CONFIG_FILE="$CONFIG_DIR/config.toml"
CREDENTIALS="$CONFIG_DIR/credentials"
APP_LOG="$CACHE_DIR/logs/run.log"

if [ ! -f "$CONFIG_FILE" ] || [ ! -f "$CREDENTIALS" ]; then
  CONFIGURED=0
fi
if [ -f "$CONFIG_FILE" ] && ! grep -Eq '^[[:space:]]*server_url[[:space:]]*=[[:space:]]*"[^"]+"' "$CONFIG_FILE"; then
  CONFIGURED=0
fi

other=$(pgrep -x myuzik 2>/dev/null | head -n 1 || true)
if [ -n "$other" ]; then
  echo "measure-ram: note: another Myuzik (pid $other) is already running; close it for a clean number."
fi

# ---- /proc helpers -------------------------------------------------------

now_ms() { date +%s%3N; }

rss_kb() {
  local pid=$1 kb=""
  kb=$(awk '/^VmRSS:/ { print $2 }' "/proc/$pid/status" 2>/dev/null) || kb=""
  echo "${kb:-0}"
}

cpu_ticks() {
  local pid=$1 ticks=""
  ticks=$(sed 's/^[^)]*) //' "/proc/$pid/stat" 2>/dev/null | awk '{ print $12 + $13 }') || ticks=""
  echo "${ticks:-0}"
}

thread_count() {
  local pid=$1 n=""
  n=$(awk '/^Threads:/ { print $2 }' "/proc/$pid/status" 2>/dev/null) || n=""
  echo "${n:-0}"
}

fd_count() {
  local pid=$1 n=0
  n=$(ls "/proc/$pid/fd" 2>/dev/null | wc -l) || n=0
  echo "$n"
}

cpu_pct() { # ticks_start ticks_end ms_start ms_end
  local hz
  hz=$(getconf CLK_TCK 2>/dev/null) || hz=100
  awk -v c0="$1" -v c1="$2" -v t0="$3" -v t1="$4" -v hz="${hz:-100}" 'BEGIN {
    dt = (t1 - t0) / 1000.0
    if (dt <= 0) { print "0.0"; exit }
    printf "%.1f\n", ((c1 - c0) / hz) / dt * 100.0
  }'
}

mb() { awk -v kb="$1" 'BEGIN { printf "%.1f", kb / 1024.0 }'; }

verdict() { # value budget
  awk -v v="$1" -v b="$2" 'BEGIN { print (v <= b) ? "PASS" : "FAIL" }'
}

report_phase() { # label seconds peak_kb min_kb cpu_pct samples budget_mb
  local label=$1 seconds=$2 peak=$3 minimum=$4 cpu=$5 samples=$6 budget=$7
  local peak_mb
  peak_mb=$(mb "$peak")
  PHASE_RESULT=$(verdict "$peak_mb" "$budget")
  printf '   %-9s %3ss sampled every %ss (%s samples)\n' "$label" "$seconds" "$SAMPLE" "$samples"
  printf '             RSS  peak %s MB   last %s MB   min %s MB     (budget <= %s MB)  %s\n' \
    "$peak_mb" "$(mb "$PHASE_LAST_KB")" "$(mb "$minimum")" "$budget" "$PHASE_RESULT"
  printf '             CPU  %s %%   threads %s   fds %s\n' \
    "$cpu" "$(thread_count "$APP_PID")" "$(fd_count "$APP_PID")"
}

sample_phase() { # label seconds budget_mb
  local label=$1 seconds=$2 budget=$3
  local waited=0 first=1 peak=0 minimum=0 last=0 count=0 rss
  local t_start t_end cpu_start cpu_end
  t_start=$(now_ms)
  cpu_start=$(cpu_ticks "$APP_PID")
  while [ "$waited" -lt "$seconds" ]; do
    if ! kill -0 "$APP_PID" 2>/dev/null; then
      echo "measure-ram: the app exited while sampling the $label phase (crash?)" >&2
      APP_EXITED=1
      return 1
    fi
    rss=$(rss_kb "$APP_PID")
    if [ "$first" = 1 ]; then
      minimum=$rss
      first=0
    fi
    if [ "$rss" -gt "$peak" ]; then peak=$rss; fi
    if [ "$rss" -lt "$minimum" ]; then minimum=$rss; fi
    last=$rss
    count=$(( count + 1 ))
    sleep "$SAMPLE"
    waited=$(( waited + SAMPLE ))
  done
  t_end=$(now_ms)
  cpu_end=$(cpu_ticks "$APP_PID")
  PHASE_PEAK_KB=$peak
  PHASE_LAST_KB=$last
  report_phase "$label" "$seconds" "$peak" "$minimum" \
    "$(cpu_pct "$cpu_start" "$cpu_end" "$t_start" "$t_end")" "$count" "$budget"
}

wait_for_window() { # pid; give the app up to 10 s to reach its steady footprint
  local pid=$1 tries=0 rss=0
  while [ "$tries" -lt 100 ]; do
    if ! kill -0 "$pid" 2>/dev/null; then return 1; fi
    rss=$(rss_kb "$pid")
    # A live GPUI window has mapped its fonts and buffers well before 20 MB; the app's
    # steady state is far higher, so this only skips the very first frames.
    if [ "$rss" -gt 20480 ]; then return 0; fi
    sleep 0.1
    tries=$(( tries + 1 ))
  done
  return 0
}

cache_report() {
  local covers="$CACHE_DIR/covers" lyrics="$CACHE_DIR/lyrics"
  local covers_files=0 lyrics_files=0 covers_mb=0 lyrics_mb=0
  if [ -d "$covers" ]; then
    covers_files=$(find "$covers" -maxdepth 1 -type f 2>/dev/null | wc -l)
    covers_mb=$(du -sm "$covers" 2>/dev/null | awk '{ print $1 }')
  fi
  if [ -d "$lyrics" ]; then
    lyrics_files=$(find "$lyrics" -maxdepth 1 -type f 2>/dev/null | wc -l)
    lyrics_mb=$(du -sm "$lyrics" 2>/dev/null | awk '{ print $1 }')
  fi
  printf '   covers   %s files, %s MB on disk   (in-memory LRU is capped at 200 entries)\n' \
    "${covers_files:-0}" "${covers_mb:-0}"
  printf '   lyrics   %s files, %s MB on disk   (pruned to newest 500 files / 50 MB)\n' \
    "${lyrics_files:-0}" "${lyrics_mb:-0}"
}

startup_probe() {
  echo "== startup =="
  local log total_ms startup_s result rc=0
  local start end
  log=$(mktemp)
  TMP_FILES+=("$log")
  start=$(now_ms)
  # MYUZIK_WINDOW_TEST=close makes the app close its window 1 s after opening it and exit,
  # so the wall time of this run is startup + 1 s + teardown (a slight upper bound).
  RUST_LOG="${RUST_LOG:-info}" MYUZIK_WINDOW_TEST=close timeout 30 "$BINARY" >"$log" 2>&1 || rc=$?
  end=$(now_ms)
  if [ "$rc" -ne 0 ]; then
    if [ "$rc" -eq 124 ]; then
      echo "   not measured: the probe run did not exit within 30 s"
    else
      echo "   not measured: the probe run failed (exit $rc)"
    fi
    sed 's/^/   | /' "$log" | tail -n 8
    return 0
  fi
  total_ms=$(( end - start ))
  local startup_ms=$(( total_ms - 1000 ))
  [ "$startup_ms" -lt 0 ] && startup_ms=0
  startup_s=$(awk -v ms="$startup_ms" 'BEGIN { printf "%.2f", ms / 1000.0 }')
  result=$(verdict "$startup_s" "$STARTUP_BUDGET_S")
  printf '             process start -> window   %s s   (budget <= %s s)  %s\n' \
    "$startup_s" "$STARTUP_BUDGET_S" "$result"
  printf '             method: wall time %s ms minus the app-to-probe 1 s window timer;\n' "$total_ms"
  printf '             includes teardown, so it is a slight upper bound.\n'
  STARTUP_S=$startup_s
  PHASE_RESULT=$result
}

# ---- run -----------------------------------------------------------------

echo "== Myuzik memory / CPU / startup measurement =="
printf '   binary   %s\n' "$BINARY"
printf '   session  %s (%s)\n' "${XDG_SESSION_TYPE:-unknown}" "${WAYLAND_DISPLAY:-${DISPLAY:-no display}}"
printf '   budgets  idle <= %s MB, playing+overlay <= %s MB, startup <= %s s\n' \
  "$IDLE_BUDGET_MB" "$PLAYING_BUDGET_MB" "$STARTUP_BUDGET_S"
if [ "$CONFIGURED" = 0 ]; then
  echo "   note: no Navidrome configured at $CONFIG_DIR, so the app shows the setup form."
  echo "         Idle numbers stay valid; the playback and overlay phases need a library."
fi
echo

if [ "$DO_STARTUP" = 1 ]; then
  startup_probe
  if [ "$PHASE_RESULT" = FAIL ]; then FAILED=1; fi
  echo
fi

echo "== idle =="
app_out=$(mktemp)
TMP_FILES+=("$app_out")
( unset MYUZIK_WINDOW_TEST; exec "$BINARY" ) >"$app_out" 2>&1 &
APP_PID=$!
if ! wait_for_window "$APP_PID"; then
  echo "measure-ram: the app exited immediately; its output was:" >&2
  sed 's/^/   | /' "$app_out" | tail -n 20 >&2
  die "could not keep the app running to sample it"
fi
sleep 1
sample_phase "idle" "$DURATION" "$IDLE_BUDGET_MB"
if [ "$PHASE_RESULT" = FAIL ]; then FAILED=1; fi
IDLE_PEAK_KB=$PHASE_PEAK_KB

if [ "$INTERACTIVE" = yes ] && [ "$CONFIGURED" = 1 ]; then
  echo
  echo "== playback =="
  echo "   In the Myuzik window, start a track (double-click an album), then press Enter here."
  read -r _ || true
  sample_phase "playing" "$DURATION" "$PLAYING_BUDGET_MB"
  if [ "$PHASE_RESULT" = FAIL ]; then FAILED=1; fi
  PEAK_PLAYING_KB=$PHASE_PEAK_KB

  echo
  echo "== playing + overlay =="
  echo "   Open the lyrics overlay with Ctrl+P, then press Enter here."
  read -r _ || true
  sample_phase "overlay" "$DURATION" "$PLAYING_BUDGET_MB"
  if [ "$PHASE_RESULT" = FAIL ]; then FAILED=1; fi
  if [ "$PHASE_PEAK_KB" -gt "$PEAK_PLAYING_KB" ]; then PEAK_PLAYING_KB=$PHASE_PEAK_KB; fi
else
  echo
  echo "== playback + overlay =="
  if [ "$CONFIGURED" = 0 ]; then
    echo "   not measured: no library is configured, so nothing can play."
  elif [ "$INTERACTIVE" = no ]; then
    echo "   not measured: run with --interactive on the desktop to sample playing and overlay."
  fi
fi

echo
echo "== caches =="
cache_report

echo
echo "== summary =="
IDLE_PEAK_MB=$(mb "$IDLE_PEAK_KB")
printf '   idle peak RSS                    %s MB   (budget <= %s MB)  %s\n' \
  "$IDLE_PEAK_MB" "$IDLE_BUDGET_MB" "$(verdict "$IDLE_PEAK_MB" "$IDLE_BUDGET_MB")"
if [ "$PEAK_PLAYING_KB" -gt 0 ]; then
  PLAYING_PEAK_MB=$(mb "$PEAK_PLAYING_KB")
  printf '   playing + overlay peak RSS       %s MB   (budget <= %s MB)  %s\n' \
    "$PLAYING_PEAK_MB" "$PLAYING_BUDGET_MB" "$(verdict "$PLAYING_PEAK_MB" "$PLAYING_BUDGET_MB")"
else
  printf '   playing + overlay peak RSS       not measured\n'
fi
if [ -n "$STARTUP_S" ]; then
  printf '   startup to window                %s s    (budget <= %s s)  %s\n' \
    "$STARTUP_S" "$STARTUP_BUDGET_S" "$(verdict "$STARTUP_S" "$STARTUP_BUDGET_S")"
else
  printf '   startup to window                not measured\n'
fi
printf '   process                          pid %s, %s threads, %s fds\n' \
  "$APP_PID" "$(thread_count "$APP_PID")" "$(fd_count "$APP_PID")"
echo
if [ -f "$APP_LOG" ]; then
  echo "   app log: $APP_LOG"
fi
if [ "$APP_EXITED" = 1 ]; then
  die "the app exited during a sampling phase; no numbers are trustworthy"
fi
if [ "$FAILED" = 1 ]; then
  echo "   at least one measured number is over budget (see FAIL above)."
  if [ "$STRICT" = 1 ]; then exit 1; fi
else
  echo "   all measured numbers are within budget."
fi
exit 0
