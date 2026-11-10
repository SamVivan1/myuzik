# Performance, memory and stress behaviour

Myuzik's v1 budgets and how they are checked: one measurement script for the real binary,
one headless stress test suite for the library, caches and queue, and the acceptance run
recorded in [acceptance.md](acceptance.md).

## Budgets

| Metric | Budget | Where it comes from |
|---|---|---|
| Idle RSS, main window open | ≤ 200 MB | [plan, task 24](superpowers/plans/2026-11-09-myuzik-v1.md) (`- Memory budgets: idle ≤ 200 MB RSS`) |
| RSS while playing + lyrics overlay open | ≤ 250 MB | same plan line |
| Startup to a usable frame | ≤ 1.5 s | same plan line (SSD) |

`VmRSS` from `/proc/<pid>/status` is the metric: shared pages are counted once per process,
which is how the budget was written and how the numbers below were taken.

## Measuring the real binary

```bash
cargo build --release
scripts/measure-ram.sh --duration 60 --interactive
```

`scripts/measure-ram.sh` launches the built binary on the current session and reports:

| What | How |
|---|---|
| RSS (peak / last / min per phase) | `/proc/<pid>/status` → `VmRSS` |
| CPU % over the phase | `/proc/<pid>/stat` → `utime + stime`, divided by wall time |
| Threads and open fds | `/proc/<pid>/status`, `/proc/<pid>/fd` (growth = a leak) |
| Startup | one probe run with `MYUZIK_WINDOW_TEST=close`, which closes the window 1 s after it opens; wall time − 1 s |
| Cover / lyrics cache on disk | file count and `du -sm` under `~/.cache/myuzik` |

Phases: `idle` always; `playing` and `playing + overlay` when run with `--interactive` on a
desktop (start a track, press Enter; press `Ctrl+P` for the overlay, press Enter).

Options and env overrides:

```text
-b/--binary PATH      binary to measure        (default target/release/myuzik, env MYUZIK_BIN)
-d/--duration SEC     seconds per phase        (default 60, accepts 30m/1h, env MYUZIK_MEASURE_SECONDS)
-s/--sample SEC       seconds between samples  (default 5, env MYUZIK_SAMPLE_SECONDS)
--interactive         prompt before playback and overlay phases
--non-interactive     idle only (what CI or a terminal-less run can do)
--no-startup          skip the startup probe
--strict              exit 1 when a measured number is over budget
```

It degrades instead of failing: with `WAYLAND_DISPLAY` and `DISPLAY` both unset it explains
that a GPUI window cannot be sampled there and exits 0; with no Navidrome configured it says
so and only reports the idle phase, which is still valid on the setup form.

For a long idle soak, use a long duration and no interaction:

```bash
scripts/measure-ram.sh --duration 30m --sample 30 --non-interactive   # 30 min idle
```

## Measured numbers

### Release build, measured 2026-11-10

Environment: AMD Ryzen 5 8640U / Radeon 760M, 12 threads, 14.4 GiB RAM, GNOME Shell 51.0 on
Wayland, kernel 7.3.0-10-generic, rustc 1.99.1. Binary: `target/release/myuzik`, built
2026-11-10 17:12 from the v0.1.0 working tree (the release commit).

| Run | Idle peak RSS | CPU (idle) | Startup to window | Threads / fds |
|---|---|---|---|---|
| `--duration 15 --sample 3 --non-interactive --strict` | 133.0 MB | 0.7 % | 1.12 s | 33 / 51 |
| `--duration 5 --sample 1` (same binary, earlier run) | 123.6 MB | 1.8 % | 1.07 s | 33 / 51 |
| `--duration 20 --sample 4 --strict` (pre-a11y build) | 132.0 MB | 0.4 % | 0.33 s | 33 / 51 |
| **Budget** | **≤ 200 MB** | — | **≤ 1.5 s** | — |

All within budget. The idle footprint stays flat between samples (min 120.7 MB → peak
133.0 MB) with no growth during the phase and no change in threads or fds, and the
accessibility work did not move the number (132.0 MB before it, 133.0 MB after). The
startup spread is cold/warm: the first launch after a rebuild compiles shaders and warms
the GPU cache (1.07–1.12 s), a second launch is 0.33 s.

Cache on disk at measurement time: covers 36 files / 4 MB, lyrics 14 files / 1 MB.

MPRIS was confirmed live during the same run: `playerctl -l` lists
`io.github.samvivan1.Myuzik` and `playerctl -p io.github.samvivan1.Myuzik status` reports
`Stopped` with nothing playing.

### Previously measured (not re-measured here)

[acceptance.md](acceptance.md) records **125 MB idle / ~160–184 MB playing** (release) from
the v0.1.0 acceptance run. Those are earlier numbers from the same machine and budget; the
playing + overlay phase was not re-measured for this document because it needs a person to
start playback and press `Ctrl+P` (`--interactive`), which a terminal-less run cannot do.

## Automated stress tests

`tests/stress.rs` (6 tests, headless, offline — the only server is a local `httpmock`
instance on `127.0.0.1`). Run:

```bash
cargo test --test stress -- --test-threads=1            # add --nocapture for timings
```

| Test | What it proves | Bound asserted | Measured here |
|---|---|---|---|
| `twenty_thousand_track_library_maps_within_budget` | 20 000 Subsonic song/album DTOs map to domain models with bounded time and a bounded per-track footprint | < 5 s per mapping, ≤ 1 KiB/track, < 8 MiB of strings for 20k tracks | ~1.4–1.6 ms per mapping; 1.4 MiB total, 72 B/track |
| `twenty_thousand_albums_page_in_windows_not_whole` | 20 000 albums arrive through `album_list(kind, 500, offset)` in 40 contiguous, non-overlapping windows — never as one full-library fetch | exactly one request per window matched on `size=500` **and** `offset`, 40 requests, ids `album-0..album-19999` in order, < 15 s | ~250 ms, 40 requests |
| `two_hundred_searches_keep_only_the_newest_result` | 200 searches through the generation guard keep exactly one result set; every query is bounded server-side (song/album/artist counts); the cover cache stays at its cap while being filled ten times over | one retained `SearchResults`, stale generation rejected, `covers.len() == 200`, recency queue in sync | ~440 ms (2000 cover inserts) |
| `stale_out_of_order_search_responses_are_discarded` | with 8 concurrent searches whose delays are reversed (newest answers first), only the newest generation is applied and the stale ones are dropped after it | `applied == 1`, retained result is the newest query, all 8 requests served, ≥ 1 stale response arrived *after* the winner | ~245 ms, 7 stale responses discarded |
| `five_hundred_cover_inserts_stay_bounded_and_lru` | 500 inserts through the real `HomeState::insert_cover` LRU stay at the 200-entry cap, evict oldest-first, refresh on re-insert, and drop the evicted `Arc` (no leak); 2000 further churn inserts keep map and recency queue in sync | `covers.len() <= 200` after *every* insert, `covers.len() == cover_order.len()`, evicted image no longer upgradeable | not timed separately; a few ms |
| `twenty_thousand_track_queue_survives_rapid_churn` | a 20 000-track queue under rapid next/previous keeps the current track exact on every step, never changes length, still wraps correctly for repeat all/one/off, shuffle visits all 20k exactly once, and `PlayerState` stays in sync | current id equals the expected index, `len == 20 000` throughout, shuffle visits each track once, < 15 s | 5000 next/prev pairs in ~2.3 ms, shuffle walk ~12 ms |

Whole suite: **6 passed in ~1.0 s** on this machine (debug build). `tests/stress.rs` is part
of the default `cargo test` run, so `scripts/test-all.sh` exercises it on every CI run.

The tests are designed to fail if a bound is violated, not just to pass: tightening the cover
cap in the test from 200 to 100 makes `five_hundred_cover_inserts_stay_bounded_and_lru` fail
with `cover cache held 101 entries after insert 100`, and disabling the generation check makes
`stale_out_of_order_search_responses_are_discarded` fail with `the gate applied 8 responses
instead of 1`. Both checks were run and then reverted.

## Bounded structures these tests lean on

| Structure | Bound | Where |
|---|---|---|
| Cover LRU (in memory) | 200 entries, least-recently-added evicted | `HomeState::insert_cover` in [src/ui/shell.rs](../src/ui/shell.rs), bound exported as `COVER_CACHE_CAP` so the stress test asserts the real number |
| Lyrics cache (on disk) | newest 500 files / 50 MB, pruned on startup and after writes | `LyricsCache::prune` ([src/lyrics/cache.rs:79](../src/lyrics/cache.rs)) |
| Album list paging | `size` ≤ 500 per `getAlbumList2` request | `NavidromeClient::album_list` ([src/navidrome/client.rs:132](../src/navidrome/client.rs)) |
| Search result size | `songCount=40`, `albumCount=20`, `artistCount=20` per query | `NavidromeClient::search` ([src/navidrome/client.rs:201](../src/navidrome/client.rs)) |
| Cover download | at most 8 MB read per cover | `NavidromeClient::cover_bytes` ([src/navidrome/client.rs:257](../src/navidrome/client.rs)) |

## Known limits

- **No GPU/render memory.** `VmRSS` does not include textures, swapchain images or compositor
  buffers. Use `radeontop`/`nvidia-smi`/`intel_gpu_top` for VRAM; a hidden GPU leak is not
  visible here.
- **Startup is to a window, not to a presented frame.** The probe measures process start →
  window created (minus the app's own 1 s window-test timer, plus teardown). First-frame
  presentation happens after that, so this is optimistic by the compositor's first present.
- **The 24 h idle soak is not automated.** It needs wall-clock time and a desktop session;
  `scripts/measure-ram.sh --duration 30m --sample 30 --non-interactive` runs the long idle
  phase, and repeated runs over hours are the manual equivalent. What *is* automated is the
  part that would grow over time: bounded caches and no per-operation growth.
- **Playing + overlay needs a human.** No display-free way exists to click an album and press
  `Ctrl+P` on Wayland (no `wtype`/`ydotool` in CI), so that phase is measured interactively.
- **Tests are not micro-benchmarks.** The time ceilings are deliberately loose so they stay
  green on a loaded CI box; they catch order-of-magnitude regressions (an accidental
  full-library fetch or quadratic mapping), not a few milliseconds.
- **Single machine.** Compare against the budget, not against another machine's numbers.

## What to look at when a number regresses

| Symptom | Likely cause |
|---|---|
| Idle RSS above ~150 MB | cover cache growth or larger decoded images, per-frame allocations in the shell, retained `SearchResults`/album lists |
| Threads or fds climbing across phases | leaked sockets (per-request agent) or a GStreamer pipeline per track change |
| Startup creeping up | font loading, shader compile, first-run GPU cache; re-run to separate cold from warm |
| CPU above a few percent while idle | animation or polling running without a window change (reduced motion should not change idle CPU) |
