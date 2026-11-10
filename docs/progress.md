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
- [x] Task 12: Queue, shuffle, repeat, next/previous, EOS auto-advance
- [~] Task 13: Player bar, seek ±10s, volume; global keyboard shortcuts pending
- [x] Task 14: MPRIS (metadata, status, next/prev, seek, volume)

## Phase 4 — Lyrics
- [x] Task 15: LRC parser, active line, offset
- [x] Task 16: Navidrome `lyricsList` + LRCLIB fallback + cache
- [x] Task 17: Shared lyrics view + Now Playing screen with cover

## Phase 5 — Pinned Overlay
- [~] Task 18: Overlay window/view/toggle + lyrics sync implemented; geometry persistence pending
- [~] Task 19: GNOME extension present; install script; version matrix notes pending

## Phase 6 — Settings, Polish, Integrity
- [x] Task 20: Settings (quality, scrobble, lyrics offset/source, reduced motion, accent)
- [x] Task 21: Scrobbling (now-playing + submit), favorites, queue actions
- [~] Task 22: Error toasts, setup errors, URL/token log redaction, aria labels;
  full keyboard focus order + automated a11y tree pending (see docs/accessibility.md)
- [ ] Task 23: Design polish and anti-slop audit
- [~] Task 24: Memory measured (idle 125 MB, playing 159 MB); stress tests pending

## Phase 7 — Packaging, Docs, Release
- [x] Task 25: Desktop file, AppStream metainfo, icon, install script, README, CI
- [ ] Task 26: Final acceptance matrix and v0.1.0 tag/release

## Testing
- `./scripts/test-all.sh` — fmt, clippy, 56 unit/UI tests (serial).
- `MYUZIK_LIVE=1 ./scripts/test-all.sh` — live Navidrome + GStreamer suites.
- See `docs/testing.md` for the coverage map.

## Known gaps
- Global keyboard shortcuts (Space, arrows, Ctrl+K, Ctrl+P) are not bound yet.
- Overlay geometry is not persisted across restarts.
- Playlist/artist detail screens are read/play only; no editing.
- Settings cover quality/scrobble/lyrics/motion; account password change is via setup.
- Stress tests (20k tracks, rapid skips, 24 h idle) are not automated.
