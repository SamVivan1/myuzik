# Design overhaul review

Status: core behaviour and layout are stable enough to start a visual redesign.
This review is scoped to the UI layer; the backend contract is in
[ui-contract.md](ui-contract.md).

## Phase 1 — Done

Liquid Glass tokens are in `src/theme.rs`. The palette is now deep ink + translucent
glass + cool indigo accent (overridden at runtime from cover art). Typography is Inter
at 12/14/16/20/28/36. Radii are 20/20/24/999/999. Sidebar is 220 px, player bar
is 64 px, nav items have icon+text, and the accent is extracted from the currently
playing cover.

## Phase 2 — Done

Glass surfaces are applied to the player bar (64 px capsule) and sidebar (220 px).
Nav items now have icon+text. The accent is dynamic from cover art. Layout
numbers updated: sidebar 220, player bar 64, cover 48, bottom padding 86/92.

## What is solid now

- One playback engine and queue for both sources; queue edits stay in sync with
  MPRIS.
- Provider-qualified routes: `Album(SourceId)`, `Artist(SourceId)`,
  `Playlist(SourceId)`, plus YouTube `Home` and `Moods`.
- `MusicService`/`SourceRegistry` mean screens do not talk to Subsonic or the
  YouTube helper directly.
- Keyboard focus ring, Enter activation, reduced motion and text clamping are
  already enforced by tests.
- Toast is rendered (the old audit item UI-02 is stale).
- The sidebar has a Library/YouTube source switch, and the menu follows it.

## Problems visible in the current build

### Information architecture

1. ~~Two different Home pages share one title.~~ Resolved: the sidebar switch
   names the active source and scopes the menu; a small source badge also sits
   in the player bar and Search.
2. **Search is a dead end.** The empty state is only a placeholder; there are
   no recent searches, no source hints and no suggested moods/albums.
3. **Navigation is flat.** Albums, Artists, Playlists and Favorites are
   Navidrome-only, but they sit beside YouTube Home without a label. Moods is
   only reachable from a pill on Home.
4. **Back behaviour depends on the source.** A Navidrome album goes back to
   Albums, a YouTube album goes back to Home. That is logical but invisible.

### Layout and hierarchy

1. **The player bar is visually quiet.** Title/artist are small, the transport
   symbols have little spacing and the queue button reads as a menu glyph.
2. **Detail headers have too many small labels.** Back link, type label, title,
   artist and meta are stacked in a narrow column; the type label duplicates
   the page title.
3. **Cards and rows use unrelated sizes.** YouTube shelves use 150 px cards,
   Navidrome grids use a 4-column grid, artist albums use the same card at a
   different width, and track rows are full-width bars.
4. **Small type is doing too much work.** Many labels are 8–10 px. Secondary
   metadata is hard to scan at a distance, especially on the queue drawer.
5. **Horizontal shelves have no scroll affordance.** There is no fade, arrow
   or "See all"; cards simply clip at the right edge.
6. **Empty/loading/error states are inconsistent.** Some use skeletons, some a
   single line of text, some a bordered alert. Error panels appear in the grid
   and in the detail route with different shapes.

### Accessibility

- Focus order is correct, but many hit targets are 22–30 px; the player bar
  buttons should be at least 32–36 px.
- Contrast of secondary text (`text_tertiary` on `bg_base`) is borderline in
  the headless captures; verify with a real contrast pass.
- The queue drawer's 8 px metadata and 10 px titles are below a comfortable
  reading size for a keyboard/screen-reader user.

## Proposed overhaul scope

Replace only:

- `src/theme.rs` tokens (keep semantic names).
- `src/ui/shell/browse.rs` (YouTube home, detail screens, moods).
- `src/ui/shell/queue.rs` (queue drawer).
- `src/ui/now_playing.rs`, `src/ui/components/lyrics_view.rs`.
- Sidebar/player-bar composition inside `src/ui/shell.rs`.

Do not change: services, providers, Navidrome client, player, library caches,
lyrics, overlay or the `Route`/`AppAction` semantics.

## Component inventory to build

| Component | Used by | Notes |
|---|---|---|
| `AppShell` | all routes | sidebar + content + fixed player bar |
| `SourceBadge` | Home, Search, detail | "YouTube Music" / "Library" |
| `MediaCard` | shelves, album grids | one size system, optional play overlay |
| `Shelf` | YouTube home, moods | title, scroll fade/arrows, optional See all |
| `TrackRow` | album, playlist, queue | cover, title, artist, duration, row actions |
| `DetailHeader` | album, artist, playlist | breadcrumb, cover, title, actions |
| `QueueDrawer` | global | larger rows, clear section boundaries |
| `PlayerBar` | global | larger transport, source/quality hint |
| `SearchField` + `SearchEmpty` | Search | recent queries, source hints |
| `SettingsRow` | Settings | switch, value, description, focus ring |
| `Toast` | global | already wired; restyle only |
| `EmptyState` / `Skeleton` | all lists | one visual language |

## Suggested direction

Keep **Midnight Brass** (clay, gold, warm glass) but tighten the system:

- 4 px spacing scale, 8 px radius step, three type sizes for body UI
  (11/13/15), two for display (22/34).
- Cards: one 1:1 cover size per context; no mixed aspect ratios.
- Player bar: 64 px tall, 34 px transport targets, queue icon with a badge for
  upcoming count.
- Detail pages: breadcrumb replaces the "Back" + type label stack.
- Shelves: 5.5 cards visible at 1280 px with a right-edge fade; "See all"
  opens a source-aware grid route.
- Search: source badge, recent queries, and three suggestion chips
  (Moods, Albums, Artists) instead of an empty canvas.

## Phased implementation

1. **Tokens and shell** — type scale, spacing, player bar, sidebar source badge.
2. **Cards and shelves** — `MediaCard`, `Shelf`, consistent grids.
3. **Detail pages** — `DetailHeader`, `TrackRow`, actions, breadcrumbs.
4. **Queue and Search** — drawer hierarchy, search empty state.
5. **Polish** — motion, skeletons, error/empty states, contrast pass,
   screenshot refresh.

Each phase should keep `tests/a11y.rs`, `tests/ui_flow.rs`,
`tests/youtube_flow.rs` and the screenshot capture green.

## Open decisions

1. Should the sidebar show **one** source at a time (Library vs YouTube) or both
   with section headers?
2. Should enabling YouTube replace Home permanently, or should Home have an
   explicit source switch persisted in config?
3. Keep the pinned-lyrics overlay visually as-is, or restyle it with the same
   tokens as Now Playing?
