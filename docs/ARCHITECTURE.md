# Architecture

Module dependency, threading model, and data flows for Myuzik. Complements
[`AGENTS.md`](../AGENTS.md). Evidence for claims: `file:line` where noted.

## 1. Module dependency (high level)

```
main.rs
 ├── config (Paths, AppConfig, load_config_recovering)
 ├── platform::mpris (start_mpris)
 ├── platform::tray (Tray::start_background)
 ├── player::engine (spawn_player → PlayerHandle)
 └── ui::Shell (cx.new)

ui::Shell
 ├── config::AppConfig / Paths
 ├── player (PlayerHandle, Queue, state)
 ├── library (Track/Album/Artist, cache, cover_cache, radio, search_index)
 ├── lyrics (LyricsProvider, LyricsCache, lrc)
 ├── services::SourceRegistry (NavidromeService / YouTubeService)
├── navidrome::NavidromeClient
 ├── overlay (toggle_overlay)
 ├── platform::tray (attach_tray)
 └── theme

services → adapters over navidrome::NavidromeClient / provider::YouTubeProvider
navidrome::NavidromeClient → ureq (blocking, called from background executor)
lyrics::LyricsProvider → navidrome + ureq (LRCLIB)
library::radio → navidrome (similar/genre/random)
overlay → shared Shell state (no clone of lyric list)
```

`lib.rs` only re-exports modules; no logic.

## 2. Threading model

| Thread/executor | Responsibility | Must not |
|---|---|---|
| GPUI main (UI) | render, `cx.spawn` completion, state mutation | block on I/O or mutex held by I/O |
| Player thread | GStreamer playbin + bus messages | touch GPUI |
| `background_executor` | all `ureq` HTTP, file writes, cover decode/encode | run unbounded |
| Tray worker (spawned) | DBus registration | block UI startup (PF-09: registered after first frame) |

Channels: `PlayerCommand` (UI→player), `PlayerEvent` (player→UI) via flume. MPRIS runs
on its own zbus connection and calls back into player tx / shell.

## 3. Key data flows

### 3.1 Startup
`main()` → `Paths::new().ensure()` → single-instance lock (`acquire_instance_lock`) →
`load_config_recovering` → `application().run` → add fonts → `spawn_player` →
`start_mpris` → `Shell::new` → open window → tray start (background) → show-request
poller task.

### 3.2 Play a track
Shell `play_track` → `Queue::play_now` → engine `PlayerCommand::Load(url)` → stream
metadata → scrobble `now_playing`. On EOS: queue advance; if empty + `autoplay`,
radio `next_batch` (background) → append → resume.

### 3.2a Browse and play YouTube (optional source)
Home → `MusicService::home` (YouTubeService → `YouTubeProvider`) → one-shot helper (`get_home`) → shelf cards. Album/artist/playlist/mood routes call the matching browse op. Playback → buffered file under `~/.cache/myuzik/streams` → `PlayerCommand::Load(file://…)` → GStreamer. Signed URLs never reach the player or logs. The helper is killed as a process group when a request is replaced or a track is skipped.

### 3.3 Lyrics
`load_lyrics(track)` (generation-guarded) → Navidrome `lyricsList` (if
`prefer_navidrome_lyrics`) → on miss/unsynced, LRCLIB search matched by title +
artist + duration (rejects wrong edits) → `LyricsCache` write → `apply_offset` →
`active_line(progress)` for UI/overlay.

### 3.4 Cover art
`cover_art_id` → `cover_cache` (in-memory LRU, 512 px) → decode on background
executor → `Arc<Image>` shared by main window and overlay. Disk write exists;
disk read is NOT implemented (PF-05).

### 3.5 Overlay (pinned lyrics)
Second GPUI window reading Shell state without cloning the lyric list. Artwork is
the same `Arc<Image>`. GNOME extension matches window title `Myuzik Lyrics` and
calls `make_above()`/`stick()`.

### 3.6 Close-to-tray
Main window handle tracked (not window count). When closed with `close_to_tray`,
process stays alive; tray icon becomes visible. Supervisor polls liveness every
400 ms and quits if closed with close-to-tray off.

### 3.7 Single instance
`flock` on lock file; second process writes `myuzik.show-request` beside lock and
exits; running instance polls and calls `show_main_window` (focus or reopen).

## 4. State ownership

- `Shell` (app-scoped entity) owns UI state, routes, caches, queue handle, config.
- `PlayerHandle` owns player thread channels; shared via `Arc`/clone.
- `MprisHandle` optional; `quit()` exists but is not called (INT lifecycle gap).
- `Tray` shared via `Arc`; visibility driven by main-window liveness.

## 5. Async discipline

- All HTTP via `background_executor`; timeouts explicit.
- Generation guards for search & lyrics; missing for cover/radio/album (FL-02/04/06).
- Radio holds `Mutex<Radio>` across `next_batch` (P0 FL-01) — hazard.

## 6. Memory budgets

Idle ≤ 200 MB, playing+overlay ≤ 250 MB, startup ≤ 1.5 s (measured: idle 133 MB
peak, startup 1.12 s cold). See `docs/performance.md`.

## UI/backend boundary

See [ui-contract.md](ui-contract.md) for the service/action seam a design overhaul may replace.
