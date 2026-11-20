# Design audit fixes — execution plan

Status: in progress. Source: screenshot audit 2026-11-20.

## P0 — broken layout

- [x] T1 Shelf overflow, fade, and a working "See all"
  - [x] Wrap shelf row in a `min_w_0` / `overflow_hidden` container.
  - [x] Add a theme `fade` token and a non-interactive right fade.
  - [x] Implement `Route::Shelf` + `Shell::open_shelf(section)` and a grid page, or remove "See all" until a grid exists.
- [x] T2 Player bar
  - [x] Increase bar height to fit cover, title, transport, progress, and timecodes.
  - [x] Put progress + timecodes on one row, aligned.
  - [x] Add enough bottom padding (>= 120 px) so the last shelf clears the bar.
  - [x] Reduce empty gap between the now-playing block and transport.

## P1 — visual quality

- [x] T3 Palette and contrast
  - [x] Reduce aurora saturation/opacity; neutralize the background.
  - [x] Raise `text_tertiary` contrast.
  - [x] Unify cover radii at 20 px.
  - [x] Add `fade` token, remove raw `rgba(0x0A0B10…)`.
  - [x] Make active nav/source-pill states consistent.
- [x] T4 Icons
  - [x] Replace Unicode nav glyphs with inline SVG icons (home, now playing, search, albums, artists, playlists, favorites, moods, settings).
  - [x] Mark icons decorative for accessibility.
- [x] T5 Redundancy and IA
  - [x] Remove duplicate "YouTube Music" label.
  - [x] Replace repeated "Playlist" subtitle with track count/owner.
  - [x] Remove duplicate "Pin lyrics" entry.
  - [x] Make the header search placeholder source-aware.
- [x] T6 Responsive layout
  - [ ] Verify 1280 / 1440 / 1600 / 1920 widths (pending).
  - [x] Ensure shelves never clip mid-card.

## P2 — accessibility and polish

- [x] T7 Accessibility
  - [x] "See all" focusable with role + aria label.
  - [x] Progress bar role/aria value.
  - [x] Source-switch targets >= 32 px.
  - [ ] Icon aria-hidden (parent nav items already labelled).
- [x] T8 Verification
  - [x] `cargo fmt --check`
  - [x] `cargo clippy --all-targets -- -D warnings`
  - [x] Full test suite
  - [x] Live screenshots
  - [x] Update `docs/progress.md`, `docs/design-overhaul.md`, `CHANGELOG.md`

## Ownership

- Main agent: T1, T2, T5, T6, T7, T8.
- Subagent (beam-free): T3 (theme + card radii).
- Subagent (beam-free): T4 (SVG icon module + nav).
