# Myuzik Logo — Design

**Status:** Implemented (mark chosen 2026-11-10, assets and packaging landed)
**Date:** 2026-11-10
**Owner:** SamVivan
**License:** MIT

---

## 1. What this is and why it exists

Myuzik shipped v0.1.0 with a placeholder app icon: three shapes in an SVG, of which one was
the letter **M** set in `font-family="Georgia, serif"`. A text glyph in an icon file is not an
icon — it depends on a font the system may not have, it is not drawn for a 16 px grid, and it
cannot be one shape with the rest of the artwork.

This document records the replacement: what the mark is, the three directions that were
compared, why the chosen one won, the exact geometry, and how to regenerate every derived file.

The brief, in the owner's words: a **cozy, luxurious, simple** music player.

## 2. Goals

- Read as one object at **16 px** (GNOME panel), and as something composed and quiet at 512 px.
- Say "music" without spelling it out — the product is a player, and the icon is its face in
  the app grid, the dock and the file manager.
- Belong to the Midnight Brass language already in the app: brass matte, warm near-black glass,
  gold at 5–10 % of the surface, no neon, no glitter.
- Be **path-only**: no text elements, no font attributes, no external references. Identical
  rendering on every system, with or without the bundled Fraunces and Inter.
- Install and uninstall through the existing scripts, with no new dependency at install time.

## 3. Anti-goals

- No AI-generated raster traced into vectors; the repo needs clean, reviewable path data.
- No second colour system: the OS accent stays functional (focus, selection), never decorative.
- No light/dark icon pair — one warm plate, the way the app itself ships one theme.
- No change to the GPUI UI in this pass (see §10).
- No README/social lockup.

## 4. Constraints inherited from the product

| Constraint | Source |
|---|---|
| Gold reserved for state; brass gradient `#E8D3A3 → #C9A86A`, matte, never metallic | `docs/design.md` §6.1–6.4 |
| Warm near-black `#0B0B0E` page ground; champagne hairline `rgba(232,211,163,0.16)` on glass | `docs/design.md` table |
| Icon-only controls need labels; no emoji as icons | `docs/design.md` §6.5 |
| App ID `io.github.samvivan1.Myuzik`, XDG hicolor layout, `Icon=` in the desktop entry | `packaging/`, `install.sh` |
| Linux/GNOME 51 Wayland first; panel and app grid both matter | `README.md` |

## 5. Approaches considered

Three real directions were drawn and rendered, not argued about:

| Direction | What it says at 16 px | Trade-off |
|---|---|---|
| **M + needle + groove arc** (monogram with a stylus landing on one hairline) | Just a monogram: needle and arc vanish below 32 px | Most restrained at large sizes; loses the music cue exactly where the icon is smallest |
| **M inside a brass ring, needle inside the disc** — *chosen* | A disc with a monogram in it, at every size | One extra element to pay for; in exchange the "record" reading survives 16 px |
| **M above a clay pill** (the player-bar pill as a plinth) | A monogram over a white blob | Warmest material story, but at 24–32 px the pill reads as a face or a shelf |

Rejected: a literal turntable (busy at any small size), a lyric-line/pill mark (reads as a chat
bubble, not music), a waveform (says "audio editing", not "player").

**Chosen:** the ring. The deciding evidence was the 16 px rendering — the ring is a strong closed
shape, so the icon still looks like a record when the needle has already disappeared. The
monogram alone is safe but generic; the ring is what makes it a music player.

## 6. The mark

Canvas 128 × 128, plate full-bleed (GNOME draws icons on its own grid, so no internal margin).

| Element | Specification |
|---|---|
| Plate | rounded rect `0,0 128,128`, `rx=28`, linear gradient at ~165°, `#1A1610 → #0C0B0E` |
| Hairline | rounded rect `1.5,1.5 125,125`, `rx=26.5`, stroke 1.5, `#E8D3A3` at `0.22` |
| Ring (the record) | circle `(64,64) r=46`, stroke 3.5, brass gradient, `stroke-opacity=0.6` |
| Monogram | stroke width 10.5, round joins, butt caps; stems `x=34` and `x=94`, top `y=46`, baseline `y=76`, valley vertex `(64,74)` |
| Stylus | wedge with a full-width base `(58.75,74)–(69.25,74)`, tapering to a tilted apex `(69.5,93)` |
| Brass | linear gradient `135°`, `#E8D3A3 → #C9A86A` |

The stylus is tilted rather than vertical on purpose: a straight needle under the valley makes
the whole figure read as a "Y" or an anchor. The tilt reads as a stylus landing on a record, and
breaks the emblem-like symmetry.

## 7. One contour, not two shapes

The first build drew the monogram as a stroke and the stylus as a separate filled triangle. At
512 px that leaves a visible horizontal ledge on both sides where the needle's base meets the
rounded valley of the M — the two silhouettes do not share an outline.

The fix is typographic rather than cosmetic: the monogram and the stylus are unioned into a
single filled contour, and that path data is what ships. No ledge is possible inside one outline.
Once unioned, the stylus can also afford a **full-width base** (10.5, the same as the stroke);
a narrow base survived the ledge but read as a sliver at 64–128 px, where the mark is most often
seen. The recipe, should the monogram ever change:

```
1. draw  M + needle  as separate shapes in Inkscape
2. select all → object-stroke-to-path → path-union
3. paste the resulting single path into the SVG as fill="url(#brass)"
4. ./scripts/render-icons.sh
```

The committed path is 437 characters of arcs and lines (`A 5.250525 …` for the round joins),
which is why it is stored as path data with the construction recipe in a comment instead of
hand-tuned coordinates.

## 8. Files and regeneration

| File | Role |
|---|---|
| `packaging/icons/hicolor/scalable/apps/io.github.samvivan1.Myuzik.svg` | **Source of truth** (2.5 KB). Path-only: no text, no fonts, no external refs. |
| `packaging/icons/hicolor/symbolic/apps/io.github.samvivan1.Myuzik-symbolic.svg` | Monochrome variant on a 16 px grid: monogram + stylus only, no plate or ring (1.1 KB). |
| `packaging/icons/hicolor/{16,24,32,48,64,128,256,512}x*/apps/io.github.samvivan1.Myuzik.png` | Derived PNG set (116 KB total), committed so installing never needs Inkscape. |
| `scripts/render-icons.sh` | Regenerates the PNG set from the SVG; `--check` verifies the set is present and not older than the SVG. |
| `install.sh` / `uninstall.sh` | Install and remove all sizes plus the symbolic icon; both fail or report clearly. |

Why the symbolic variant simplifies: GTK recolours a `-symbolic` icon from its own palette, so it
must be a single opaque silhouette. The ring is dropped because at 16 px it turns the silhouette
into grey mush; the monogram and stylus carry the identity.

## 9. Verification (measured, not assumed)

| Check | Result |
|---|---|
| SVG well-formed, no text, no font attributes | `xmllint` clean; `count(//*[local-name()="text"])=0`; `count(//@font-family)=0` on both files |
| Contrast, brass on plate | `#C9A86A` on `#0C0B0E` = **8.69:1**; `#E8D3A3` on `#1A1610` = **12.24:1** |
| Plate on a white panel | `#1A1610` on `#FFFFFF` = **18.01:1**, so the icon keeps a hard edge on light docks |
| Legibility at size | 16 / 24 / 32 / 48 / 64 / 128 / 256 / 512 rendered and inspected; monogram reads at 16 px, ring reads from 24 px, stylus from 48 px |
| Rendered by the engine GNOME actually uses | librsvg 2 renders the master, and its output differs from the committed Inkscape PNGs by RMSE 0.10 % at 16 px, 0.07 % at 48 px and 0.17 % at 512 px — antialiasing only, not geometry |
| Icon theme resolution | GTK 4.24 `IconTheme.lookup_icon` returned the exact PNG for every size 16–256 and the symbolic SVG for `-symbolic` |
| Install into an empty prefix | `install.sh --skip-build --no-extension` wrote scalable + 8 PNG + symbolic + metainfo, refreshed the icon cache, and the desktop-entry and app-registry checks passed |
| Uninstall | `--dry-run` listed all 11 icon files; the real run removed them all, left nothing behind, and a second run exits 0 reporting everything as not installed |
| Failure path | With `32x32` removed, `install.sh` stops in prerequisites with `error: missing the 32px icon — run ./scripts/render-icons.sh` and exits 1 before touching anything |

## 10. What is left to the owner and to the builder

**Owner:** run `./install.sh --skip-build` on the real prefix and look at the icon in the app grid
and the dock. That is the one check a terminal cannot make.

**Not done, on purpose — the in-app mark.** The sidebar still renders a **Fraunces "M"** inside a
clay tile (`src/ui/shell.rs`, `theme.display_font`), as `docs/design.md` §6.5 specified. The
packaged icon no longer uses a font, so the two now differ in detail while sharing the monogram
idea. Making the sidebar draw the same contour means shipping the path into the GPUI scene or
embedding a raster, which is a UI change with its own review — a follow-up, not part of the icon.

**Regeneration guard.** `scripts/render-icons.sh --check` is safe to run in CI (no Inkscape
needed) and exits non-zero if a PNG is missing or older than the SVG. Wiring it into
`scripts/test-all.sh` was left out of this pass to keep the change to the icon and its packaging.
