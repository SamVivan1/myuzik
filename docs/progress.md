# Myuzik — Implementation Progress

Last updated: 2026-11-10

## Status legend
- [x] Done and tested
- [~] Partial
- [ ] Not started

## Phase 0 — Walking Skeleton
- [x] Task 1: Repo scaffold, licenses (MIT), CI skeleton
- [x] Task 2: Feasibility spike (second window, overlay, GNOME pin extension)

## Phase 1 — Foundations
- [x] Task 3: Midnight Brass theme tokens + contrast tests
- [x] Task 4: Config, XDG paths, credential store
- [x] Task 5: Navidrome client, token auth, typed errors
- [x] Task 6: Domain models, caches, browse helpers

## Phase 2 — Shell and Library UI
- [x] Task 7: App root, routes, interactive setup form, shell
- [x] Task 8: Home screen, real covers, album cards, track rows
- [x] Task 9: Album, artist, playlist, favorites screens
- [x] Task 10: Keyboard search with stale-response protection

## Phase 3 — Playback and MPRIS
- [x] Task 11: GStreamer player thread + state machine (wired)
- [x] Task 12: Queue, next/previous, EOS auto-advance. Shuffle and repeat exist
  in `Queue` and are covered by tests, but no control or shortcut exposes them
  yet, and MPRIS does not advertise `LoopStatus`/`Shuffle` (see
  `docs/audit/flow-logic.md` FL-08 / `docs/audit/integration.md` INT-04).
- [x] Task 13: Player bar, seek ±10s, volume, global shortcuts (Space/arrows/Ctrl+K/Ctrl+P)
- [x] Task 14: MPRIS (metadata, status, next/prev, seek, volume)

## Phase 4 — Lyrics
- [x] Task 15: LRC parser, active line, offset
- [x] Task 16: Navidrome `lyricsList` + LRCLIB fallback + cache
- [x] Task 17: Shared lyrics view + Now Playing screen with cover

## Phase 5 — Pinned Overlay
- [x] Task 18: Overlay window/view/toggle, lyrics sync, geometry persistence
- [x] Task 19: GNOME extension hardened (title + app-id matching, first-frame wait, idempotent
  pin/unpin, full teardown), install + uninstall scripts, README, `docs/gnome-extension.md`
  with a verified-vs-unverified matrix, 18 behaviour tests under gjs. Live-session pinning
  still needs an install + logout/login (Wayland), so it is documented as unverified.

## Phase 6 — Settings, Polish, Integrity
- [x] Task 20: Settings (quality, scrobble, lyrics offset/source, reduced motion, accent)
- [x] Task 21: Scrobbling (now-playing + submit), favorites, queue actions
- [x] Task 22: Error toasts, setup errors, URL/token log redaction, aria labels, global shortcuts;
  full keyboard focus order (nav → content → player bar → chrome) with OS-accent focus rings,
  accesskit roles and labels, and 7 automated assertions in `tests/a11y.rs`. Tree-level
  accesskit assertions and forced-colors are blocked by the pinned gpui revision, not by this
  code (see docs/accessibility.md).
- [x] Task 23: Design polish and anti-slop audit (docs/design-audit.md)
- [x] Task 24: Memory measured and inside budget with the release binary (idle 133.0 MB peak,
  startup 1.12 s, CPU 0.7 %), `scripts/measure-ram.sh`, `docs/performance.md`, and 6 automated
  stress tests in `tests/stress.rs` (20k-track library and queue, 200 searches, 500 cover
  inserts). The 24 h idle soak stays manual (needs wall-clock time).

## Phase 7 — Packaging, Docs, Release
- [x] Task 25: Desktop file, AppStream metainfo, icon, install script, README, CI
- [x] Task 26: Final acceptance matrix re-executed and corrected (`docs/acceptance.md` now
  cites the real test names), v0.1.0 release docs, `git tag v0.1.0`.

## Post-0.1.0 — Autoplay radio
- [x] Endless playback: when the queue runs out, similar music continues automatically
  (`getSimilarSongs` → `getSimilarSongs2` → paged `getSongsByGenre` → `getRandomSongs`, with
  deduplication against the queue and a bounded recent-history ring). Prefetches before the
  queue runs dry so there is no gap, resumes if the fetch arrives after the track ended,
  caps the queue at 500 tracks, and stops after repeated empty batches instead of hammering
  the server. Toggle in Settings → Playback (`autoplay` in the config). Nine engine tests,
  three end-to-end flow tests, eight queue tests and one live test cover it.

## Post-0.1.0 — Core contract and queue
- [x] Backend seam: `MusicService`/`SourceRegistry`, `PlaybackSource`/`CoverSource`, `AppAction`
  and `QueueAction`. Playback, detail loading, covers and favorites dispatch by `TrackSource`.
- [x] Queue drawer: jump, remove, reorder, clear upcoming, add album to queue, MPRIS sync.
- [x] YouTube M2: Home shelves, album/artist/playlist detail routes, moods/genres,
  YouTube-primary Home, and no-Navidrome startup.
- [x] Split the queue drawer and browse/detail screens out of `ui/shell.rs` (`ui/shell/queue.rs`,
  `ui/shell/browse.rs`).
- [x] Sidebar source switch (`active_source`): Library/YouTube scopes Home, nav and Search;
  queue/player stay global; last choice persisted with fallbacks.
- [x] Phase 1 — Liquid Glass tokens: deep ink base, translucent glass surfaces,
  cool indigo accent, Inter typography, rounded-20 radii, dynamic accent from cover art.
- [x] Phase 2 — Liquid Glass layout: glass player bar (64 px capsule), glass sidebar
  (220 px), icon+text nav, dynamic accent from cover art, glass surfaces.
- [x] Phase 3 — Liquid Glass polish: shelf fade edge + "See all", detail header
  breadcrumb + 220 px cover, search empty state, tighter track-row styling.
- [x] Phase 4 — Liquid Glass motion: 240 ms fade-in on browse cards, route transition
  fade, spring-like easing. Skeletons, contrast pass (all AA), screenshot refresh.
- [x] Phase 5 — Component extraction: `album_card`, `album_skeleton`, `track_row`,
  `format_time`, `nav_item` extracted to `src/ui/components/{cards,rows,nav}.rs`.
- [ ] Next: Phase 6 — motion refinement, skeleton consistency, contrast audit, final polish.

## Post-0.1.0 — YouTube Music (optional)
- [x] M1: optional YouTube Music search and playback behind `youtube_enabled` (off by default).
  Search results in their own section; one-shot Python helper (`ytmusicapi` + `yt-dlp`) buffers
  to `~/.cache/myuzik/streams`; GStreamer plays the local file; cover art, LRCLIB lyrics,
  overlay and MPRIS work; scrobbling and Navidrome radio are skipped, while a YouTube search
  result starts that song's YouTube radio/watch playlist. No download/export surface and no
  signed URL reaches the player or logs. `tests/provider_youtube.rs` (9 offline + 1 ignored live)
  and `tests/youtube_flow.rs` cover the process boundary and the real Shell.
- [ ] M2: YouTube album/artist/playlist browse, category pages, no-Navidrome library routes.
- [ ] M3: preload next, cookie import UI, richer history/likes.
- [ ] M4: provider trait + Audius/Internet Archive.

## Testing
- `./scripts/test-all.sh` — fmt, clippy, full offline suite serial.
- `MYUZIK_LIVE=1 ./scripts/test-all.sh` — adds live Navidrome, GStreamer and YouTube tests.
- `scripts/measure-ram.sh` — RSS / CPU / startup of the release binary (see `docs/performance.md`).
- `tests/a11y.rs` — keyboard focus order, roles/labels, search not hijacked.
- `tests/stress.rs` — 20k-track library and queue, 200 searches, 500 cover inserts.
- See `docs/testing.md` for the coverage map.

## Known gaps
- Playlist/artist detail screens are read/play only; no editing.
- Settings cover quality/scrobble/lyrics/motion; account password change is via setup.
- The GNOME extension's live pinning is unverified on this machine (needs install +
  logout/login); its logic and every Mutter API it calls are tested.
- Accesskit tree-level assertions, forced-colors/high-contrast and a 200% scaling pass are
  open; the first two are missing APIs in the pinned gpui revision.
- YouTube M1 is search + playback + song radio only; album/artist/playlist browse, category
  pages, preload and cookie UI are later. The helper is unofficial and off by default.
- The 24 h idle soak is a manual run of `scripts/measure-ram.sh`; the growth-prone parts
  (caches, queue) are covered automatically.
