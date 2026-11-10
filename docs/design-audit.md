# Design audit — Midnight Brass

Reference: `docs/mockups/midnight-brass.html` (approved visual system).

## Tokens

All surfaces, glass, clay, gold, text, and ambient colors live in `src/theme.rs`
(`midnight_brass()`). Views read tokens; raw colors remaining in `shell.rs` are limited to
window traffic-light semantics (red/yellow/green) and transparent shadows.

- Warm black glass: `bg_gradient_start/end`, `glass_gradient_start/end`, `glass_border`,
  `glass_highlight`, `glass_inner`, `glass_hover`.
- Ivory clay: `clay_surface`, `clay_shade`, `clay_gradient_start/end`, `clay_dark`,
  `clay_dark_2`, `clay_ink`.
- Champagne gold: `gold`, `gold_light`, `gold_deep`, `gold_tint`, `gold_glow`.
- OS accent: `os_accent` (#3584E4) for focus/selection only.
- Ambient: `ambient_gold`, `ambient_teal`, `ambient_rose` behind the shell.

## Glass / clay hierarchy

- **Glass**: the shell fills the window with 24 px rounded corners (gradient + champagne border +
  inset top highlight + sheen), sidebar (thin translucent layer), lyrics panel, settings rows,
  search field, album covers frame. The outer window margin was removed so there is no square
  frame around the rounded shell; the player bar floats inside the shell with 18 px margins.
- **Clay**: player bar (ivory gradient + inset highlight/shade), active nav pill, logo mark,
  gold play/play-all buttons, dark "Pin lyrics" pills, search pill.
- **Not mixed**: clay is used for controls/player; glass for surfaces. Gold is state, not decoration.

## Typography

- Fraunces (bundled variable TTF) for display headings and the greeting.
- Inter (bundled variable TTF) for UI and lyrics.
- No pixel font in the default theme.

## Motion

- Route change fade: 180 ms ease-out.
- Equalizer bars animate while playing (620/780/700 ms ping-pong).
- Reduced motion: `App::set_reduce_motion` disables animation end states; Settings toggle.
- No idle animation (equalizer only while playing).

## Deliberate deviations from the mockup

- **No compositor blur**: Mutter does not implement `ext-background-effect-v1`. Glass is simulated
  with a translucent gradient + ambient color + inset highlights.
- **Full-window shell**: the mockup floats the shell 24 px inside the window; the app fills the
  window with the rounded shell and keeps the player bar inside it, avoiding a square frame.
- **Window controls**: macOS-style colored dots on the right (explicit user request), replacing the
  mockup's `— □ ×` actions.
- **Nav icons**: text labels only (no icon set bundled yet).
- **Album covers**: real Navidrome art instead of the mockup's gradient placeholders; gradient
  fallback shows the album initial.
- **Album grid**: fills 4 equal columns, matching the mockup's `repeat(4, minmax(0,1fr))`.

## Anti-slop checklist

- [x] No purple/neon/violet gradients.
- [x] No pixel or novelty font.
- [x] No emoji icons; text labels and glyph controls (▶, ⏭, ×).
- [x] Gold occupies less than ~10% of the surface; no gold-on-gold.
- [x] OS accent only on focus/selection (search field, setup field).
- [x] No nested card-in-card surfaces.
- [x] No gray-on-gray body text; contrast tokens are tested.
- [x] No layout shift on cover load (fixed aspect boxes).

## Headless screenshots

`MYUZIK_LIVE=1 cargo test --test screenshots -- --ignored --test-threads=1 --nocapture`
writes 1280×800 PNGs to `docs/screenshots/` (Home, Settings, Search, Now Playing, Favorites,
Artists). The headless wgpu renderer does not draw image textures, so album/cover art appears as
the brass gradient placeholder in those captures; the real app renders the cover art.

## Remaining review items

- Full keyboard focus ring on every control (nav items, cards, transport).
- Overlay screenshot and 1920×1080 captures.
- 200% scaling pass.
