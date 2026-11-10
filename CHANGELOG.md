# Changelog

## Unreleased

### Added
- `./install.sh` — one command installs the release binary, a desktop entry, the icon,
  AppStream metadata, refreshes the desktop/icon caches, and installs the GNOME pin
  extension. Options: `--no-extension`, `--skip-build`, `--prefix=DIR`.
- `./uninstall.sh` — one command removes all of the above plus the extension; `--purge`
  also deletes config and caches, `--dry-run` shows what would go, `--keep-extension`
  leaves the extension alone. Both scripts are idempotent.
- `scripts/install.sh` is now a thin wrapper for the root installer, so the documented
  `--uninstall`/`--with-extension` paths keep working.

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
