# Accessibility audit

Status: partial — keyboard focus order, focus rings, accesskit roles and
automated assertions are implemented; the remaining gaps are listed at the end
with the reason each one is still open.

## Implemented

### Contrast

Midnight Brass tokens are checked in `src/theme.rs` unit tests (body text
≥ 4.5:1, gold active ≥ 4.5:1, clay ink ≥ 4.5:1, focus blue ≥ 3:1 against both
glass and clay surfaces). View code uses tokens only — no raw colors.

### Keyboard focus order

Every interactive control is a real gpui tab stop (`.track_focus(...)` on a
cached `FocusHandle` plus `tab_stop(true)`/`tab_index(...)` on the handle —
gpui inserts the handle you supply into its tab map, so the settings must live
on the handle). `Shell::focus_for` caches one handle per logical control name,
so a control keeps its handle (and its accesskit node id) across frames, and
cards/rows get one per item id (`album:a1`, `track:3`, …).

The order is set by four `tab_index` groups on `Shell`, walked by Tab and
Shift+Tab (wired in the root `on_key_down`):

| group | `tab_index` | contents |
| --- | --- | --- |
| `TAB_NAV` | 0 | sidebar: Home, Now Playing, Search, Albums, Artists, Playlists, Favorites, Settings, pin lyrics |
| `TAB_CONTENT` | 1 | page content: search field and results, album cards/grid, artist and playlist rows, track rows, album detail buttons, settings rows |
| `TAB_PLAYER` | 2 | now-playing block, seek −10/+10, previous, play/pause, next, volume −/+, pin lyrics |
| `TAB_CHROME` | 3 | window close, minimize, maximize |

Lists and grids are the containers, so a screen reader announces the item
count: `Role::Grid` for the album grids, `Role::List` for the artist list, the
playlist list and the album's track list, `Role::ListItem` for their rows.

Text entry is protected: the root shortcut handler returns early while the
search field has focus, so typing (including Space, which would otherwise
toggle playback) goes to the field; Tab is handled before that guard so it
still leaves the field.

### Focus ring

Focused controls draw a 2 px ring in the OS accent token (`os_accent`,
`#3584E4`). Every focusable control reserves the 2 px border width, so gaining
focus never shifts the layout (gpui at this revision has no outline API); a
control without a border of its own passes a fully transparent `gpui::rgba(0x00000000)` to
`Shell::focus_border` and is therefore borderless until it is focused, while
controls that already had a visible border (search field, settings rows, cards,
track rows) keep it. Focus is the only thing that *adds* a border.

Gold stays reserved for state (now playing, active route); the accent is used
for focus only. The rendering is also captured in `docs/screenshots/`, so a
focus-ring style change is visible in the screenshot diff.

### Labels and roles

Every icon-only control carries an `aria_label` and an accesskit role:
transport and seek/volume buttons, window controls, album cards
(`Open album …`), track rows (`Play …`), search results, artist and playlist
rows, album detail buttons, settings rows (label includes the current value),
now-playing controls, overlay close, and the setup fields/connect button. Nav
items use `Role::Link` with `aria_selected` on the active route; the search
field is `Role::TextInput`.

### Text alternatives, reduced motion, no color-only state

Nav and settings rows use visible text labels. Settings → Appearance → Reduce
motion calls `App::set_reduce_motion`; `AnimationExt` renders static end states.
Active track uses gold text + border + background; toggle settings show text
values.

## Verified automatically

`tests/a11y.rs` (7 tests) drives the real `Shell` headlessly against a mocked
Navidrome:

- `tab_order_is_nav_then_content_then_player_bar_then_chrome` — walks the actual
  gpui tab map with Tab, asserts the order is nav → content → player bar →
  window chrome, and that the walk covers exactly the registered controls
  (nothing unreachable, nothing unexpected).
- `shift_tab_reverses_the_focus_order` — Shift+Tab retraces the ring.
- `typing_in_search_is_not_hijacked_and_tab_leaves_the_field` — a Space typed
  in the search field is inserted instead of toggling playback, and Tab moves
  focus to a control in the focus order.
- `persistent_chrome_stays_tabbable_on_every_route` — the sidebar and player
  bar remain tab stops on all eight routes.
- `icon_only_controls_expose_a_role_and_a_label` — every icon-only control in
  `src/ui/shell.rs`, `src/ui/now_playing.rs` and `src/overlay/mod.rs` carries
  `.role(...)` and `.aria_label(...)` in the element chain that builds it.
- `keyboard_focus_wiring_is_present_for_every_group` — all four tab groups are
  populated, focus rings use `.border_2()` with the accent token, Tab/Shift+Tab
  call `focus_next`/`focus_prev`, and grid/list/link/button/text-input roles are
  used.
- `a11y_tree_is_not_observable_in_the_headless_harness` — pins down why the
  assertions above are not tree-based (see below) and fails if that changes.

## Still to do

- **Tree-level assertions against the accesskit tree.** `Window::debug_a11y_tree_json()`
  exists in the pinned gpui rev, but gpui only builds the tree while an
  assistive technology is connected (`Window::is_a11y_active()`), and in the
  headless test platform `PlatformWindow::a11y_init` is the trait's default
  no-op, so the activation callback never fires: `is_a11y_enabled()` is true,
  `is_a11y_active()` is false, `debug_a11y_tree_json()` is `None`. Roles and
  labels are therefore asserted against the rendering code and the focus order
  against gpui's real tab map. `a11y_tree_is_not_observable_in_the_headless_harness`
  fails the day the tree becomes available, which is when these should be
  rewritten against it.
- **Forced-colors / high-contrast fallback.** gpui at this rev exposes no
  forced-colors or `prefers-contrast` signal at all (no `forced_colors` API in
  the crate), so the app cannot react to a system high-contrast theme; it would
  need a theme override plus a settings entry, not a stylesheet fix.
- **200% scaling review.** The headless harness renders at scale factor 1.0, so
  the layout at 200% (and the focus rings' 2 px at 2× density) can only be
  reviewed by hand on a HiDPI session. No automated check exists yet.
- **Live regions for state changes.** This gpui rev has no live-region API, so
  track changes and the error toast are not announced proactively; a screen
  reader user has to navigate to them.
- **Setup screen keyboard model.** The setup card keeps its own form-local Tab
  (it cycles Server/Username/Password) instead of entering the global ring, and
  the three fields are focusable only through the card. The fields do expose
  `Role::TextInput` with labels, but they are not individual tab stops.
- **Overlay drag header.** The pinned-lyrics overlay's header is a mouse drag
  area with a label but no interactive role; only its close button is a tab stop
  in that window.
