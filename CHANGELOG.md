# Changelog

## Unreleased

### Added
- **System tray** (`src/platform/tray.rs`): a StatusNotifierItem plus DBusMenu on the session bus,
  with Show/Hide, Play/Pause, Previous, Next, Pin lyrics and Quit. The menu labels follow the
  current track and play state. Without a session bus the app runs normally, just without a tray.
- **Close to tray**: closing the main window destroys it and leaves the process running — queue,
  player thread and MPRIS all survive — so playback continues and the tray icon brings the same
  window back. The app runs with an explicit quit mode and a supervisor that shows the icon only
  while no window exists (tray `Status` flips between `Passive` and `Active`), so the icon and the
  window are never both present. Configurable (`close_to_tray`, default on) with a
  Settings → Playback row.

### Changed
- The pinned overlay is redesigned to the approved reference: two columns — artwork, title, artist
  and a real progress bar with elapsed/total time on the left; a seven-line lyric band on the right.
  Line tiers (active / next / far / dim) replace the old flat list, the panel has its own warm
  material with ambient light (Mutter provides no backdrop blur, so the surface cannot rely on the
  wallpaper), and the default window grew from 392x180 to 620x330 to hold the layout.
- Overlay artwork reuses the cached cover, and the lyric state is read without cloning the whole
  lyric list on every frame.
- The overlay was not live: it only caught up when the window was clicked. `Entity::update` does not
  notify observers in GPUI, and every overlay update in the shell passed `_cx`, so nothing ever asked
  the view to repaint; the window only redrew when an input event arrived. Each update now calls
  `cx.notify()`, the view observes the lyric state, and a 120 ms safety ticker repaints when the
  position, active line or line count moved (so a paused overlay stays idle). Covered by a test that
  renders the window twice with a position jump in between and requires the pixels to change, with no
  input event sent.
- Long lines wrap inside the lyric column instead of running past the panel edge, and each row's
  height is estimated from the wrap so neighbouring lines no longer overlap; the scroll target adds
  those heights up, keeping the active line centred.
- The minimum overlay size is 520x300, the window pins that as its `window_min_size`, and nothing in
  the left column can be shrunk by flex. The artwork now shrinks to give the text block room, so a
  two-line title at the minimum height no longer pushes the timestamps past the bottom edge.
- The lyric band now scrolls instead of swapping a fixed slice: every line is laid out once and the
  column glides (380 ms, ease-out) so the line being sung stays centred, with the surrounding lines
  clipped at the edges — the behaviour of the media widget in `device-tweak/Widget`. Rows use one
  uniform height and type size, so emphasis comes from brightness rather than from changing sizes,
  and reduced motion is honoured automatically.

### Fixed
- **The pinned lyric overlay is now readable.** It used to render *every* line of the song into a
  392×180 window — a 70-line lyric sheet collapsed into an unreadable pile — with the whole text
  wrapping at 11 px. It now draws a karaoke band of at most six lines (two before the active line,
  three after), keeps the active line at 16 px gold, wraps long lines, drops blank rows, and shows
  a message ("Loading lyrics…", "♪ Instrumental", "No lyrics found", "Lyrics unavailable") instead
  of an empty box.
- **The overlay honours the lyric offset.** The ±500 ms setting only affected Now Playing; the pin
  ignored it and drifted out of sync. Both now use the same offset.
- **Plain lyrics no longer fake a synced line.** Untimed transcripts were stored with every
  timestamp at zero, so the *last* line of the song was highlighted as active for the whole track.
  They now follow progress through the track proportionally, marked "unsynced".
- **Tracks started from search reached the overlay.** `play_track` had its own lyrics path that
  updated only the Now Playing screen, so the pin showed nothing (or the previous song) for those
  tracks. There is now one loading path.
- **LRCLIB matching no longer returns another song's lyrics.** Two causes: `/api/get` always sent
  `album_name`, which makes LRCLIB demand an exact album match and 404 on correct songs (verified:
  the same request without the album returns 200 with synced lyrics); and the search fallback took
  the first hit, which for popular titles can be a different edit ("APT." returned 161 s, 170 s and
  172 s against a 170 s track). Albums are now omitted and search hits are ranked by duration
  proximity, accepting a synced record only within 3 s. On a 6-track sample this took synced
  resolution from ~1/6 to 6/6.
- **LRC parsing hardened**: blank lines dropped, `[offset:±ms]` honoured, mid-line timestamps,
  enhanced-LRC `<mm:ss.xx>` word tags stripped, BOM/CRLF handled, consecutive duplicates merged.
- Lyrics cache keys are versioned, so entries written by the old matcher are refetched.

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
