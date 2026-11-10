# Testing Myuzik

Run the full suite:

```bash
./scripts/test-all.sh
```

This runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and all tests serially
(GPUI tests share process-global state, so `--test-threads=1` is required). The default run is
79 tests (31 unit + 48 integration); 9 more (`live_navidrome` ×7, `live_player` ×1,
`screenshots` ×1) are `#[ignore]`d because they need a real server or a GPU renderer.

## Live server tests

The live tests read `~/.config/myuzik/config.toml` plus `~/.config/myuzik/credentials`:

```bash
MYUZIK_LIVE=1 ./scripts/test-all.sh
```

They are `#[ignore]` by default and cover:

- `tests/live_navidrome.rs`: ping, newest albums, album detail + songs, cover bytes, artists,
  playlists, search, stream URL fetch, Navidrome `lyricsList` parsing, lyrics provider pipeline.
- `tests/live_player.rs`: GStreamer play, position advance, duration, seek, pause.

## Unit tests (inline in `src/`)

31 tests, run by the same `cargo test`: config defaults/round-trip (2), LRC parsing and
offsets (6), Subsonic DTO parsing incl. lyrics shapes (3), overlay geometry clamping (4),
OS accent mapping (3), log redaction (2), scrobble thresholds (6), theme contrast (5).

## Mocked UI/integration tests

- `tests/ui_flow.rs`: a full mocked Navidrome drives the real `Shell` headlessly —
  home load, album click/detail, track click/play command, star toggle, lyrics → overlay sync,
  scrobble now-playing + submission, volume, seek, error toast, search keystrokes, playlist play,
  artist → album navigation, first-run setup form (connect + save 0600 credentials), and stale
  lyrics responses being discarded.
- `tests/ui_shell.rs`: every route renders, sidebar navigation clicks, overlay open/close,
  unreachable-server toast, setup form typing/validation, configured volume, and editable settings
  (quality, scrobble, lyric offset/source, reduced motion) persisting to disk.
- `tests/mpris_metadata.rs`: MPRIS metadata mapping and microsecond positions.
- `tests/lyrics_view.rs`: active-line centering math.
- `tests/a11y.rs`: 7 tests for keyboard accessibility — Tab walks nav → content → player bar →
  chrome over gpui's real tab-stop map, Shift+Tab retraces it, typing in the search field is not
  hijacked by the transport shortcuts, the persistent chrome stays tabbable on every route, and
  every icon-only control carries a role, a focus handle and an `aria_label` (checked against the
  rendering code, because accesskit's tree is not built in the headless harness — see
  [accessibility.md](accessibility.md)).
- `tests/queue.rs`: queue semantics, repeat/shuffle, and rapid next/previous.
- `tests/cover_lru.rs`: cover cache eviction and no growth on re-insert.
- `tests/navidrome_auth.rs`: Subsonic token/salt and auth parameters.
- `tests/stress.rs`: 6 capacity tests — a 20 000-track library (DTO mapping and windowed
  paging), 200 searches through the generation guard, 8 concurrent out-of-order responses,
  500 cover-LRU inserts against `COVER_CACHE_CAP`, and a 20 000-track queue under rapid
  next/previous. Headless and offline; bounds are in [performance.md](performance.md).

## Performance measurement

`scripts/measure-ram.sh` launches the built binary and reports RSS, CPU, threads, fds,
startup time and cache sizes; `--strict` exits non-zero when a budget is exceeded. See
[performance.md](performance.md).


## Screenshots

```bash
MYUZIK_LIVE=1 cargo test --test screenshots -- --ignored --test-threads=1 --nocapture
```

Writes `docs/screenshots/{home,settings,search,now-playing,favorites,artists,artist-detail,album-detail}.png` using GPUI's
headless wgpu renderer. The headless renderer does not draw image textures, so covers appear as
gradient placeholders in captures.

## Manual smoke test

```bash
# Plays the newest album's first track on startup.
MYUZIK_AUTOPLAY=1 cargo run
playerctl -p io.github.samvivan1.Myuzik status
playerctl -p io.github.samvivan1.Myuzik metadata
playerctl -p io.github.samvivan1.Myuzik next
playerctl -p io.github.samvivan1.Myuzik volume 0.5
```

Window controls can be exercised without clicking:

```bash
MYUZIK_WINDOW_TEST=maximize cargo run   # logs maximized=true
MYUZIK_WINDOW_TEST=resize cargo run     # logs the new viewport size
MYUZIK_WINDOW_TEST=minimize cargo run
MYUZIK_WINDOW_TEST=close cargo run      # exits
```
