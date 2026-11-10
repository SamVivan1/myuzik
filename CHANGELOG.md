# Changelog

## Unreleased

### Added
- **Endless autoplay radio.** When the queue runs out, playback continues with similar music
  instead of stopping: tracks similar to the current song, then to its artist, then more from
  the same genre (paged with a rotating offset so a large genre keeps producing tracks the
  queue has not seen), then random as a backstop. Playing a single song is enough to start it.
  It prefetches before the queue runs dry so the transition has no gap, resumes playback when
  a fetch lands after the track already ended, filters out anything already queued or
  recently played (bounded 100-track history), caps the queue at 500 tracks so a long session
  cannot grow without limit, and stops querying after repeated empty batches instead of
  hammering the server. New setting: Settings → Playback → **Autoplay similar**
  (`autoplay = true` in `config.toml`, on by default).
- `Track.genre` is now populated from the Subsonic payload (the field was already returned by
  the server and previously dropped), giving the radio its genre seed.
- New client methods: `similar_songs`, `similar_songs_for_song`, `songs_by_genre`,
  `random_songs`.
- `./install.sh` — one command installs the release binary, a desktop entry, the icon,
  AppStream metadata, refreshes the desktop/icon caches, and installs the GNOME pin
  extension. Options: `--no-extension`, `--skip-build`, `--prefix=DIR`.
- `./uninstall.sh` — one command removes all of the above plus the extension; `--purge`
  also deletes config and caches, `--dry-run` shows what would go, `--keep-extension`
  leaves the extension alone. Both scripts are idempotent.
- `scripts/install.sh` is now a thin wrapper for the root installer, so the documented
  `--uninstall`/`--with-extension` paths keep working.
- `scripts/render-icons.sh` — regenerates the PNG icon set from the scalable SVG master, with a
  `--check` mode that verifies the set exists and is not older than the master (no Inkscape
  needed for `--check`).

### Changed
- **New app icon.** The placeholder was an "M" set in Georgia — a font glyph, which changes
  shape on a system without that font and was never drawn for a 16 px grid. It is replaced by a
  path-only mark: a brass M whose valley descends into a tilted stylus, inside a single brass
  ring on a warm near-black plate. The monogram and the stylus are one unioned contour, so no
  seam shows where they meet; the file has no text elements, no font attributes and no external
  references. Ships with a symbolic variant and eight PNG sizes (16–512 px); `install.sh` and
  `uninstall.sh` now install and remove the whole set, and stop with a clear message when the
  generated set is missing. Design, alternatives and evidence:
  [docs/superpowers/specs/2026-11-10-logo-design.md](docs/superpowers/specs/2026-11-10-logo-design.md).

### Fixed
- **Myuzik did not appear in GNOME Activities search.** The desktop entry shipped
  `Exec=myuzik`, but GNOME Shell launches apps with the systemd *session* PATH, which
  does not include `~/.local/bin` on this system; `GAppInfo` therefore failed to load
  the entry (`gio: Unable to load application information`) and an entry that cannot be
  loaded is never listed or searched. `install.sh` now writes the absolute executable
  path and verifies the result through `GAppInfo`. Documented in
  [docs/troubleshooting.md](docs/troubleshooting.md).

## v0.1.0 — 2026-11-10

First functional release.

### Added
- Navidrome (Subsonic) setup form, library, search, favorites, artists, playlists.
- GStreamer playback: queue, next/previous, seek, volume, EOS auto-advance.
- Synced lyrics from Navidrome `lyricsList` with LRCLIB fallback and disk cache.
- Pinned lyrics overlay with active-line sync and geometry persistence.
- MPRIS metadata/controls including cover art URL.
- Scrobbling, album star/unstar, editable settings, global keyboard shortcuts.
- Midnight Brass theme with bundled Fraunces + Inter fonts.
- Desktop entry, AppStream metadata, icon, install script, CI, docs and headless screenshots.
- **Full keyboard navigation**: a four-group tab order (sidebar → content → player bar →
  window chrome), a 2 px OS-accent focus ring on every control, and Tab/Shift+Tab wired to
  gpui's tab stops. The ring is only drawn while a control is focused, so the visual design
  is unchanged at rest.
- **Accessibility metadata**: accesskit roles and `aria_label`s on every icon-only control,
  `Role::Grid`/`List`/`ListItem` container roles, `Role::TextInput` search and setup fields,
  and `aria_selected` on the active nav item. 7 automated assertions in `tests/a11y.rs`.
- **Hardened GNOME pin extension**: matches the overlay by exact title with an app-id
  fallback (GTK app id → `Shell.WindowTracker` → WM_CLASS), waits for the first frame,
  rescans windows already open when enabled, is idempotent per window, fully disconnects and
  reverses the pin on disable, and logs at debug level only. 18 behaviour tests run outside a
  shell with plain `gjs`; new `scripts/uninstall-extension.sh`.
- **Performance tooling**: `scripts/measure-ram.sh` (RSS, CPU, threads, fds, startup, cache
  sizes) and `docs/performance.md`, plus 6 stress tests in `tests/stress.rs` covering a
  20 000-track library and queue, 200 searches with stale-response protection, and 500
  cover-cache inserts against the exported `COVER_CACHE_CAP` bound.
- Docs: `docs/gnome-extension.md`, `docs/performance.md`, rewritten `docs/accessibility.md`
  and `docs/acceptance.md` (every cited test name verified against the real suite).

### Changed
- The focus ring reserves 2 px of border on every focusable control so gaining focus never
  shifts the layout; controls without a border of their own stay borderless until focused.
- The search field's border is now the accent only while focused.
- Removed the unused placeholder view modules (`ui::home::HomeView`, `ui::search::SearchView`,
  `ui::settings::SettingsView`) and the superseded `app::MyuzikApp` stub. The running UI is
  `ui::shell::Shell`; `app` now only holds `Route`.
- Screenshots in `docs/screenshots/` regenerated from a 100-album live library.

### Known limitations
- GNOME extension needed to pin the overlay above other windows; its live pinning is
  unverified on this machine (install + logout/login required on Wayland). GNOME 45–50 and
  X11 are documented but untested.
- No blur behind the window on Mutter; glass is simulated.
- No playlist editing, offline downloads, gapless playback, or Flatpak yet.
- A genuinely fullscreen window can still cover the pinned overlay.
- Accesskit tree-level assertions, forced-colors/high-contrast and a 200% scaling review are
  open (the first two need APIs the pinned gpui revision does not expose).
