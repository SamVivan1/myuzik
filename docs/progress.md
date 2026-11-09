# Myuzik — Implementation Progress

Last updated: 2026-11-10

## Status legend
- [x] Done
- [~] In progress
- [ ] Not started

## Phase 0 — Walking Skeleton
- [x] Task 1: Repo scaffold, licenses, CI skeleton
- [x] Task 2: Feasibility spike (blur, second window, extension)

## Phase 1 — Foundations
- [x] Task 3: Midnight Brass theme tokens + contrast tests
- [x] Task 4: Config, paths, credential store
- [x] Task 5: Navidrome client, auth, typed errors
- [x] Task 6: Domain models, LRU cache, pagination helpers

## Phase 2 — Shell and Library UI
- [~] Task 7: App root, routes, setup screen, shell skeleton (shell exists, routes defined, setup not yet)
- [ ] Task 8: Home screen, album cards, cover cache, track rows
- [ ] Task 9: Album, artist, playlist, favorites screens
- [ ] Task 10: Search with debounce and cancellation

## Phase 3 — Playback and MPRIS
- [x] Task 11: Player engine thread + state machine (engine written, not yet wired to UI)
- [x] Task 12: Queue, shuffle, repeat, next/previous
- [ ] Task 13: Player bar UI and keyboard shortcuts (shell has placeholder)
- [ ] Task 14: MPRIS server (metadata mapping done, thread pending)

## Phase 4 — Lyrics
- [x] Task 15: LRC parser, active line, offset
- [x] Task 16: Lyrics provider, cache, prefetch
- [ ] Task 17: Shared lyrics view + Now Playing screen

## Phase 5 — Pinned Overlay
- [ ] Task 18: Overlay window, view, toggle, persistence
- [ ] Task 19: Finalize GNOME pin extension

## Phase 6 — Settings, Polish, Integrity
- [ ] Task 20: Settings, OS accent, reduced motion
- [ ] Task 21: Scrobble, favorites, queue actions
- [ ] Task 22: Error handling, logging redaction, accessibility
- [ ] Task 23: Design polish and anti-slop audit
- [ ] Task 24: Performance, memory, reliability

## Phase 7 — Packaging, Docs, Release
- [ ] Task 25: Packaging, desktop integration, README, CI
- [ ] Task 26: Final acceptance and v0.1.0 release

## Notes
- Audio dev packages installed (libgstreamer1.0-dev). cargo build (default features) passes.
- All 24 unit/integration tests pass.
- Next priority: lyrics view + Now Playing (core differentiator), then overlay.
