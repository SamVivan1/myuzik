# Accessibility audit

Status: partial — the checklist below records what is implemented and what is still missing.

## Implemented

- **Contrast**: Midnight Brass tokens are checked in `tests/theme_contrast.rs`
  (body text ≥ 4.5:1, gold active ≥ 4.5:1, clay ink ≥ 4.5:1, focus blue ≥ 3:1).
- **Focus ring**: the search field and focused setup field use the OS accent
  (`#3584E4`) with a 2 px ring; the plan is to extend this to every interactive control.
- **Labels**: every icon-only control has an `aria_label`:
  window controls, play/pause, previous/next, seek ±10 s, volume ±, album cards
  (`Open album …`), track rows (`Play …`), search results, overlay close/header.
- **Text alternatives**: nav and settings rows use visible text labels.
- **Reduced motion**: Settings → Appearance → Reduce motion calls
  `App::set_reduce_motion`; `AnimationExt` automatically renders static end states.
- **Keyboard**: setup form (Tab/Enter/Backspace), search (type + Enter + Esc),
  scrollable content, and all buttons are clickable; overlay close is a button.
- **No color-only state**: active track uses gold text + border + background;
  scrobble/quality settings show text values.
- **Resizable layout**: content scrolls; album grid is 4 columns; the main window
  has a 960×640 minimum and a resize grip.

## Still to do

- Global transport shortcuts (Space, arrows, Ctrl+K, Ctrl+P) and a visible
  focus order across the whole shell.
- Screen-reader roles for lists/grids (currently generic containers).
- Automated `accesskit` tree assertions.
- High-contrast / forced-colors fallback.
- 200% scaling screenshots and review.
