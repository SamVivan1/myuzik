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

## Testing
- `./scripts/test-all.sh` — fmt, clippy, 79 tests serial (31 unit + 48 headless integration;
  9 live/screenshot tests are `#[ignore]`d).
- `MYUZIK_LIVE=1 ./scripts/test-all.sh` — adds the live Navidrome (7) and GStreamer (1) suites.
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
- The 24 h idle soak is a manual run of `scripts/measure-ram.sh`; the growth-prone parts
  (caches, queue) are covered automatically.
