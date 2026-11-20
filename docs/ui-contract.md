# UI ↔ backend contract

This is the seam a design overhaul may replace. The goal is that screens can be
redesigned without touching providers, playback, library caches or the player
engine, and that a new source can be added without touching screens.

## Backend seam

`src/services/mod.rs`:

- `MusicService` — blocking operations: `search`, `home`, `album`, `artist`,
  `playlist`, `radio`, `moods`, `mood_playlists`, `resolve`, `cover`, `cover_id`,
  `set_starred`, `scrobble`.
- `SourceRegistry` — dispatches by `TrackSource` and returns `Disabled` when a
  source is off.
- `PlaybackSource` — `Url` for Navidrome, `LocalFile` for buffered YouTube.
- `CoverSource` — bytes or a remote URL.

Adapters today: `NavidromeService` (Subsonic) and `YouTubeService` (one-shot
Python helper). Adding Audius or Internet Archive means one more adapter plus a
registry entry; screens do not change.

Rules:

- Call services from the background executor, never from `render`/`update`.
- The UI never constructs a provider client or a signed media URL.
- Source-specific extras that have no generic meaning (moods, home shelves) are
  default methods on `MusicService` that return `Unsupported`.

## UI seam

- `app::Route` — top-level destinations, including provider-qualified
  `Album(SourceId)`, `Artist(SourceId)`, `Playlist(SourceId)` and `Moods`.
- `app::AppAction` — screen intent: queue edits, open album/artist/playlist,
  open moods, refresh the YouTube home. `Shell::dispatch` owns the transition.
- `app::QueueAction` — `TogglePanel`, `PlayAt`, `Remove`, `Move`,
  `ClearUpcoming`.
- `AppConfig.active_source` + `app::resolve_active_source` — the remembered
  Library/YouTube choice, resolved against what is configured at startup. Home,
  the sidebar menu and Search follow it; queue/playback do not.

Screens may call `Shell` methods directly today (Search, Settings and the
Navidrome home still live in `ui/shell.rs`). New browse/detail screens live in
`ui/shell/browse.rs` and the queue drawer in `ui/shell/queue.rs`; those files
plus `theme.rs` and `now_playing.rs` are the intended redesign surface.

## What a redesign may change

Safe to replace without backend work:

- `src/theme.rs` tokens (keep the semantic names).
- `src/ui/shell/browse.rs`, `src/ui/shell/queue.rs`.
- Card/row layout, typography, spacing, motion, empty/loading/error states.
- `src/ui/now_playing.rs`, `src/ui/components/lyrics_view.rs`.
- Sidebar/player-bar composition inside `ui/shell.rs`.

Not part of a visual redesign:

- `src/services/**`, `src/provider/**`, `src/navidrome/**`.
- `src/player/**`, `src/library/**`, `src/lyrics/**`, `src/overlay/mod.rs`.
- `Route`/`AppAction`/`QueueAction` semantics unless a feature requires it.

## Verification

- `tests/services.rs` proves registry dispatch and disabled-source behavior.
- `tests/youtube_flow.rs` drives Home → album/playlist/moods with the real
  `Shell` and a fixture helper.
- `tests/ui_flow.rs` drives mocked Navidrome browse/detail/playback.
- `tests/a11y.rs` checks focus order, labels and Enter activation.
- `MYUZIK_LIVE=1 cargo test --test screenshots -- --ignored` renders the current
  screens for visual review.
