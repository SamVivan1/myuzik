# Testing Myuzik

Run the full suite:

```bash
./scripts/test-all.sh
```

This runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and all tests serially
(GPUI tests share process-global state, so `--test-threads=1` is required).

## Live server tests

The live tests read `~/.config/myuzik/config.toml` plus `~/.config/myuzik/credentials`:

```bash
MYUZIK_LIVE=1 ./scripts/test-all.sh
```

They are `#[ignore]` by default and cover:

- `tests/live_navidrome.rs`: ping, newest albums, album detail + songs, cover bytes, artists,
  playlists, search, stream URL fetch, Navidrome `lyricsList` parsing, lyrics provider pipeline.
- `tests/live_player.rs`: GStreamer play, position advance, duration, seek, pause.

## Mocked UI/integration tests

- `tests/ui_flow.rs`: a full mocked Navidrome drives the real `Shell` headlessly —
  home load, album click/detail, track click/play command, star toggle, lyrics → overlay sync,
  scrobble now-playing + submission, volume, seek, error toast, search keystrokes, playlist play,
  artist → album navigation.
- `tests/ui_shell.rs`: every route renders, sidebar navigation clicks, overlay open/close.
- `tests/mpris_metadata.rs`: MPRIS metadata mapping and microsecond positions.
- `tests/lyrics_view.rs`: active-line centering math.

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
