# Changelog

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

### Known limitations
- GNOME extension needed to pin the overlay above other windows.
- No blur behind the window on Mutter; glass is simulated.
- No playlist editing, offline downloads, gapless playback, or Flatpak yet.
