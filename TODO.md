# TODO — Myuzik polish & remaining work

Tracked here (no todo tool available). Status updated as work lands.

## Done this session
- [x] AGENTS.md + docs/ (ARCHITECTURE, MODULES, CONVENTIONS, docs index) + README link.
- [x] UI-30 time-aware greeting (Good morning/afternoon/evening).
- [x] UI-29 album duration computed from songs.
- [x] UI-24 gold section labels raised to ≥13 px / text_secondary.
- [x] UI-27 raw colors replaced with theme tokens (+3 new tokens).
- [x] INT-13 TTL (30 days) for lyrics cache (+fetched_at, backward compat).
- [x] PF-07 cover thumbnails fetched concurrently (bounded by join_all).

## Remaining (from docs/audit)
### Performance
- [x] PF-07 N+1 cover fetch (parallelized via join_all).
- [ ] PF-11 overlay repaint 10 Hz (notify only on active row change) — needs OverlayState refactor.
- [x] PF-13 linear scan in set_position (binary search).
- [ ] PF-12 allocations per lyric row.
- [ ] PF-14 album grid virtualization/memoization.
- [x] PF-16 search debounce (250 ms, done in wave 2).
- [x] PF-17 supervisor polling reduced to 1000 ms.
- [x] PF-18 MPRIS position reduced to 500 ms (command drain stays 100 ms).
- [ ] PF-19 a11y_focus unbounded growth (attempted; reverted to keep tests green).
- [ ] PF-20 skeleton animation .repeat().
### Correctness/cache
- [x] INT-13 TTL (30 days) for lyrics cache (+ fetched_at, backward compat).
- [x] PF-05 cover disk cache read path (read-through in home fetch).
- [x] PF-06 prune covers (2000 files / 200 MB at startup).
### Accessibility
- [ ] UI-01 AT client (on_a11y_action).
- [ ] UI-26 arrow-key navigation in lists/grids.
### UX/consistency
- [ ] UI-21 unify search field component.
- [~] UI-23 settings affordance honesty: misleading hover removed from static rows (missing actions still open).
- [ ] UI-25 model route (Route::Album/Artist/Playlist).
- [x] UI-27 test enforcing no raw colors (ui sources).
### Verification
- [ ] Run live tests (MYUZIK_LIVE=1) with real Navidrome/DBus.
- [ ] Measure RAM/startup on release build.

## Notes
- Baseline tests green (cargo test -- --test-threads=1). Run before/after each change.