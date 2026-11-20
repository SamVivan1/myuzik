# AGENTS.md — Myuzik

Agent-facing guide to this repository. **Read this before touching code.** It is the
single source of truth for how the project is built, structured, and what is (and is
not) actually implemented. Claims in other docs sometimes drift from the code — when
they conflict, trust the code and the audit under `docs/audit/`.

## 1. Project at a glance

- **Name/version:** Myuzik `0.1.0` (MIT).
- **Purpose:** Low-RAM, native Linux music player for a self-hosted **Navidrome**
  server (Subsonic API), with synced lyrics and a desktop-pinned lyric overlay.
- **Stack:** Rust 2024 (rust-version 1.99), [GPUI](https://github.com/zed-industries/zed)
  (pinned rev `14b7b0ef…`), GStreamer (`playbin`), zbus (MPRIS, tray, keyring),
  `ureq` (blocking HTTP), tokio (rt), serde/toml, directories, image.
- **Platform:** Linux Wayland primary (X11/XWayland works); `gpui_platform` must
  have the `wayland` feature or GPUI runs headless.
- **Feature flags:** `default = ["audio"]`; `audio` enables `dep:gstreamer`. Tests
  build with the default features.

## 2. Build & run

```bash
cargo run --release
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1          # 79+ tests; needs RUST_MIN_STACK (see .cargo/config.toml)
MYUZIK_LIVE=1 cargo test -- --test-threads=1   # adds live Navidrome/GStreamer tests
./scripts/test-all.sh                   # fmt + clippy + tests
./scripts/measure-ram.sh                # RSS/CPU/startup of release binary
```

- `.cargo/config.toml` sets `RUST_MIN_STACK = 33554432` (32 MiB) for test threads;
  without it `cargo test` can stack-overflow. Do not remove it.
- CI: `.github/workflows/ci.yml` (ubuntu-24.04, rustfmt + clippy + tests + release build).

## 3. Prerequisites

- Rust 1.99+.
- System libs: see README.md (Debian/Fedora lists). GStreamer good+bad plugins.
- A running Navidrome server (tested 0.64.x) + a Subsonic user.

## 4. Module map (`src/`)

```
src/
├── main.rs            # process entry: logging, single-instance lock, config load, GPUI app,
│                      # tray supervisor, window reopen, show-request polling
├── lib.rs             # module declarations only
├── app.rs             # Route enum (shared by shell + tests)
├── config.rs          # AppConfig, Paths, credential store, load_config_recovering
├── theme.rs           # Midnight Brass design tokens (glass/clay/gold/ambient)
├── navidrome/         # Subsonic client: auth (md5 token), client, typed errors, types
├── library/           # Track/Album/Artist/AlbumDetail/ArtistDetail; cache, cover_cache,
│                      # radio (autoplay), search_index
├── player/            # engine (GStreamer playbin, PlayerHandle/cmds/events), queue,
│                      # scrobble tracker, state (PlaybackStatus, RetryPolicy)
├── provider/          # optional YouTube Music: one-shot helper, search/home/browse
│                      # mapping, 32 MiB buffer, stream cache (off by default)
├── services/          # MusicService contract, SourceRegistry, NavidromeService,
│                      # YouTubeService, PlaybackSource/CoverSource
├── lyrics/            # model, LRC parser, LyricsProvider (Navidrome+LRCLIB), disk cache
├── overlay/           # pinned lyrics overlay window + OverlayState state machine
├── platform/          # accent (OS accent portal — currently dead code), logging,
│                      # mpris (mpris-server), tray (StatusNotifierItem)
└── ui/                # shell (main view, routes, async orchestration), now_playing,
                       # components/lyrics_view
```

Other top-level: `assets/` (fonts), `gnome-extension/`, `packaging/`, `scripts/`,
`tests/` (ui_flow, screenshots, a11y, stress), `docs/`, `examples/`.

## 5. Threading model

- **UI thread (GPUI main):** all rendering and `cx.spawn`/`background_executor` task
  completion land here. Never block it.
- **Player thread:** GStreamer `playbin` + message bus; communicates via channels
  (`PlayerCommand`/`PlayerEvent`).
- **Background executor (`cx.background_executor()`):** used for all HTTP I/O
  (`ureq`), file writes, cover decode/encode. Every network phase has explicit
  timeouts (8s connect / 15s recv / 25s global; LRCLIB 8/12/20s).
- **Tray worker:** registered on the session bus in a background task; supervisor
  polls main-window liveness every 400 ms.
- **Radio mutex:** `Mutex<Radio>` is held across `next_batch` (up to 7 blocking
  HTTP calls) — this is P0 FL-01; UI thread also locks it. Known hazard.

## 6. Data flow (high level)

1. `main()` loads config (recovering from corruption), spawns player + MPRIS, opens
   window with `Shell`.
2. `Shell::new` wires config/paths/player/tray; resolves `active_source` against the
   configured sources and shows Setup when neither is available.
3. Browse: the sidebar Library/YouTube switch scopes Home, the menu and Search.
   Library → Home (recent/starred), Artists, Albums, Playlists, Favorites → Navidrome
   client; YouTube → Home shelves, Moods & genres and detail pages.
4. Play: `play_now` on queue → engine loads stream URL → scrobble now-playing.
5. End of track: EOS → queue advance; if empty and `autoplay`, radio fetches
   similar → same genre (paged) → random.
5a. YouTube (optional): Home/Search/browse → `YouTubeService` → `YouTubeProvider`
   → one-shot Python helper (ytmusicapi + yt-dlp) → buffered local file → GStreamer.
   Home shelves, albums, artists, playlists and moods come from ytmusicapi. No signed
   URL reaches the player or logs; scrobbling and Navidrome radio are skipped for YouTube.
6. Lyrics: Navidrome `lyricsList` then LRCLIB fallback (matched by title/artist/
   duration); disk cache; offset applied.
7. Overlay: second window reading shared state; optional GNOME extension pins it.
8. Tray: icon visible only when main window is closed (close-to-tray).

## 7. Storage paths

- Config: `~/.config/myuzik/config.toml` (XDG via `directories::ProjectDirs`,
  qualifier `io.github`, org `samvivan1`, app `Myuzik`).
- Credentials: `~/.config/myuzik/credentials` (mode 0600) — **plaintext today**
  (keyring crate present but unused; P0 INT-01). Secret Service is NOT used despite
  docs/setup.md claims.
- Caches: `~/.cache/myuzik/{covers,lyrics,logs}`.
- Single-instance lock: `$XDG_RUNTIME_DIR/myuzik.lock` (fallback cache dir); show
  request file beside it.

## 8. Conventions & invariants (do not violate)

- **No `ureq` in render/update path.** All network in background executor.
- **Generation guards** for async responses exist for search and lyrics; copy the
  pattern for cover/radio/album (currently missing — FL-02/04/06).
- **Theme tokens only** (`theme::midnight_brass`); raw colors are a code smell.
- **Focus ring** always reserved via `border_2()` to avoid layout shift.
- **Errors** should surface via toast — but toast is currently never rendered
  (P0 UI-02); do not assume users see failures.
- **Tests** use `#[ignore]` for live/network/GStreamer cases; run serially.
- **Reduced motion** is honored by GPUI's animation engine once `cx.set_reduce_motion`
  is called (currently not applied at start — UI-08).
- Logging redacts URL/token.

## 9. Known gaps (do not claim these work)

From `docs/audit/*` (read before asserting feature status):

- `keyring` crate unused → credentials stored plaintext (INT-01).
- Repeat/shuffle exist in `Queue` + tests but have **no UI control or shortcut**
  and MPRIS does not advertise `LoopStatus`/`Shuffle` (FL-08, INT-04).
- `read_os_accent()` returns fallback and is never called (UI-13).
- Cover disk cache is written but never read (PF-05).
- `LyricsCache::prune` is dead code (PF-06, INT-13).
- Toast never rendered (UI-02); `PlaybackStatus::Error`/`Buffering` never formed.
- Enter/Space do not activate controls (UI-01, WCAG 2.1.1).
- `Route::Album/Artist/Playlist` never constructed; `album_loading` set but unread.
- `Shell::next/previous` sends `Stop` (dead-ish).
- `scrobble_submitted` written but unread.
- GNOME extension live pinning unverified on this machine.
- Accesskit tree-level assertions, forced-colors, 200% scaling open.
- 24h idle soak manual.
- YouTube is home/browse/search/playback + song radio: no next-track preload, no deep
  paging beyond the first artist/playlist shelf, and no cookie import UI. Search and
  Home follow `active_source`; library lists are hidden in YouTube mode. The helper is
  unofficial and can break when YouTube changes; `youtube_enabled` is off by default.

Full list + evidence: `docs/audit/README.md`, `docs/audit/fixes/README.md`.

## 10. Where to find things

- Architecture/flows: `docs/ARCHITECTURE.md`, `docs/design.md`.
- Module details: `docs/MODULES.md`.
- Conventions: `docs/CONVENTIONS.md`.
- Setup/install/troubleshooting: `docs/setup.md`, `docs/troubleshooting.md`,
  `docs/gnome-extension.md`.
- Performance/memory: `docs/performance.md`, `scripts/measure-ram.sh`.
- UI/backend contract: `docs/ui-contract.md`.
- Design overhaul review: `docs/design-overhaul.md`.
- Accessibility: `docs/accessibility.md`.
- Cache design: `docs/cache.md`.
- Testing: `docs/testing.md`, `docs/acceptance.md`.
- Audit evidence: `docs/audit/*` (112 findings, file:line).
- Progress/status: `docs/progress.md`, `CHANGELOG.md`.

## 11. Gotchas

- GPUI revision is pinned; do not bump casually (APIs like accesskit tree
  assertions missing in this rev).
- `wayland` feature on `gpui_platform` is mandatory.
- Test stack size requirement (see §2).
- Overlay window title must be exactly `Myuzik Lyrics` for GNOME extension matching.
- Single-instance lock: a second launch exits and leaves a show-request file; do
  not remove that mechanism.
- Close-to-tray: main window handle (not window count) determines tray visibility.