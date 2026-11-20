# Module reference

Per-module purpose, key symbols, and gotchas. Paths are under `src/`.

## app.rs
- `Route` enum: Setup, Home, Search, Albums, Artists, Playlists, Favorites, Album(id),
  Artist(id), Playlist(id), NowPlaying, Settings.
- Gotcha: `Album/Artist/Playlist` variants are never constructed in current code.

## config.rs
- `AppConfig` (versioned schema, v1), `StreamQuality`, `Paths`, `load_config_recovering`,
  `credentials_path`, `save_password` (plaintext 0600 file).
- Gotcha: keyring crate unused; Secret Service not used (INT-01). Unknown keys
  collected into `unknown` map and round-tripped.

## theme.rs
- `midnight_brass()` tokens (~40 fields: glass/clay/gold/ambient).
- Gotcha: `text_tertiary` #7C756A = 4.29:1, fails AA (UI-06).

## navidrome/
- `auth`: `random_salt`, `subsonic_token` (md5), `auth_params` (v=1.16.1).
- `client`: `NavidromeClient` (browse, search, lyricsList, starred, playlists,
  similarSongs, songsByGenre, randomSongs).
- `error`: `NavidromeError` typed errors.
- `types`: serde DTOs.

## library/
- `mod.rs`: `Track`, `Album`, `AlbumDetail`, `Artist`, `ArtistDetail`, `Playlist`.
- `cache.rs`: in-memory library cache (recent/starred/artists/playlists).
- `cover_cache.rs`: LRU cover cache (512 px, `Arc<Image>`); disk write only.
- `radio.rs`: autoplay radio (`next_batch`: similar→genre paged→random), dedup,
  recent ring. Holds `Mutex<Radio>` across HTTP (P0 FL-01).
- `search_index.rs`: local search index.

## provider/
- `mod.rs`: `ProviderError`, `BufferedStream`, `HelperVersion`.
- `youtube.rs`: `YouTubeProvider` — one-shot Python helper, search/home/album/artist/
  playlist/mood mapping (`BrowseCard`, `BrowseSection`, `MoodCategory`), 32 MiB buffer,
  stream-cache pruning, timeout/cancel, signed-URL hygiene.
- Gotcha: the helper needs Python 3 + `ytmusicapi` + `yt-dlp` + Node 20+; the core app does
  not. `youtube_enabled` is the kill switch and is off by default.

## services/
- `mod.rs`: `MusicService` (search/home/browse/radio/moods/resolve/cover/star/scrobble),
  `SourceRegistry`, `PlaybackSource` (`Url` / `LocalFile`), `CoverSource`.
- Adapters: `NavidromeService`, `YouTubeService`. UI dispatches by `TrackSource` and does not
  touch provider clients directly.

## player/
- `engine.rs`: GStreamer playbin, `PlayerHandle`, `PlayerCommand`, `PlayerEvent`,
  `PlayerError`, `spawn_player`.
- `queue.rs`: `Queue`, `RepeatMode` (Off/All/One), shuffle; MAX_QUEUE=500.
- `scrobble.rs`: `ScrobbleTracker`, `scrobble_threshold`, `should_submit`.
- `state.rs`: `PlaybackStatus` (Stopped/Playing/Paused/Buffering/Error),
  `RetryPolicy`, `RetryDecision`.

## lyrics/
- `model.rs`: `LyricLine`, `LyricsStatus`.
- `lrc.rs`: `parse_lrc`, `active_line`, `apply_offset`.
- `provider.rs`: `LyricsProvider` (Navidrome + LRCLIB fallback, duration match).
- `cache.rs`: `LyricsCache` (disk); `prune` is dead code (PF-06).

## overlay/
- `mod.rs`: overlay window, `OverlayState`, constants (DEFAULT 620x330, band 3+3).
  `rounded_cover` decode/encode (P0 PF-01). `target_offset` (used only in tests).

## platform/
- `accent.rs`: `read_os_accent()` — returns fallback, never called (UI-13).
- `logging.rs`: tracing setup helpers.
- `mpris.rs`: `start_mpris`, `MprisHandle`; metadata, status, seek, volume.
  Volume not wired (INT-03); `quit()` not called.
- `tray.rs`: `Tray`, `TrayCommand`, `Tray::start_background`, visibility logic.
  Large file (1782 lines).

## ui/
- `shell.rs`: `Shell` (main view, routing, source switch, async orchestration, Settings,
  Search, Home/album grids, player bar, keyboard activation).
- `shell/browse.rs`: source-aware YouTube home shelves, album/artist/playlist detail screens
  and moods/genres.
- `shell/queue.rs`: the queue drawer.
- `now_playing.rs`: `NowPlayingView`.
- `components/lyrics_view.rs`: `LyricsView`, `target_offset`.

## main.rs
- Process entry, single-instance lock, config recovery, GPUI app, tray supervisor,
  window reopen, show-request poller. Many helper fns (`instance_lock_path`, etc.)
  with unit tests.

## tests/
- `ui_flow.rs`, `screenshots.rs`, plus `tests/a11y.rs`, `tests/stress.rs`.
- Headless GPUI; 10 live/screenshot tests `#[ignore]`d. Run serially with 32MB stack.