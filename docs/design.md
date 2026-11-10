# Myuzik — Design Document

**Status:** Approved for planning (visual direction approved; implementation not started)
**Date:** 2026-11-09
**Owner:** SamVivan
**Project:** Myuzik — a personal, low-RAM Navidrome music player for Linux
**License:** MIT

---

## 1. Summary

Myuzik is a native Linux music player for a self-hosted Navidrome library. It exists because existing music players and browser-based players (including YouTube Music) consume too much memory for a machine that is also used for work. Myuzik should feel like a small, fast instrument: launch it, browse the library, play music, read synced lyrics, and pin the lyrics above other windows while working.

The product is lyric-first. The now-playing screen and the pinned desktop lyric panel are the signature features, not add-ons. The visual language is **Midnight Brass**: warm black glass surfaces, ivory clay controls, champagne-gold state accents, and the OS accent color only for focus and selection. It is built with Rust + GPUI, streams from Navidrome over the Subsonic API, uses LRCLIB and Navidrome lyrics with a local cache, exposes MPRIS, and ships with a small companion GNOME extension that pins the lyric window above other windows and across workspaces.

---

## 2. Why This Exists

### Problem

- Electron/browser players and YouTube Music consume hundreds of MB to multiple GB of RAM.
- Running music in a browser competes with work tabs; the browser stays open all day.
- Existing native players rarely make lyrics a first-class, always-available surface.
- The user already owns a Navidrome server and wants a private, simple client that feels like their own tool.

### Success

- A daily-driver player that replaces YouTube Music for library listening.
- Idle memory target: **≤ 200 MB RSS**; playing target: **≤ 250 MB RSS** (measured, not guessed).
- Lyrics are readable, synced, and can stay pinned above other apps until explicitly turned off.
- The app starts fast, survives a flaky network, and never blocks the UI on I/O.
- Open-source codebase that another Linux user can build and run with documented setup.

### Anti-goals

- Not a Navidrome server manager, tag editor, or local-file library manager.
- Not a browser wrapper or web-tech shell.
- Not a multi-platform app in v1; Linux (GNOME 51 Wayland first) is the target.
- Not a social/streaming service; no accounts beyond the user's Navidrome.

---

## 3. Goals, Non-Goals, Constraints

### Goals

1. Stream a Navidrome library with reliable play/pause/seek/next/previous and queue control.
2. Browse Home, Albums, Artists, Playlists, Favorites, and Search without loading the whole library into memory.
3. Fetch time-synced lyrics from Navidrome (OpenSubsonic) or LRCLIB, cache them, and keep the active line centered.
4. Provide a pinned lyric overlay window that stays above other windows on GNOME 51 Wayland until unpinned.
5. Expose MPRIS so the desktop's media controls and the user's existing GNOME widgets can follow Myuzik.
6. Use the OS accent color for focus/selection while keeping brass gold as the brand/state accent.
7. Be open source, buildable from a clean machine with documented dependencies.

### Non-goals (v1)

- Offline downloads or a local media library.
- Playlist editing (create/rename/reorder/add/remove). Read and play only.
- Last.fm/ListenBrainz scrobbling. Navidrome scrobble only.
- Equalizer, visualizer, crossfade, gapless playback guarantees.
- Windows/macOS, mobile, web.
- Pixel-art theme in the default; it may return later as an optional theme.
- Server discovery, sharing, or multi-user account switching.

### Constraints

- **OS:** GNOME 51 Wayland on Ubuntu 26.10 (primary); GNOME 51 X11/XWayland and other Wayland compositors should not crash.
- **Toolkit:** GPUI pinned to a Zed commit after 2026-10-30 for `ext-background-effect-v1` support. The crates.io `gpui 0.2.2` (2026-10-16) predates that support.
- **Blur:** Native blur is only available on compositors implementing `ext-background-effect-v1` (Mutter 51+). Design must look correct without blur.
- **Audio:** GStreamer 1.24+ runtime (system 1.28.7), `playbin3`/`playbin`, `autoaudiosink`; development headers required at build time.
- **Pinned overlay:** A normal GPUI window cannot make itself always-on-top on GNOME Wayland. A small companion GNOME Shell extension calls `Meta.Window.make_above()` and `stick()` for the window titled `Myuzik Lyrics`. Without the extension, the overlay still opens but is not pinned.
- **Memory:** No full-library model in RAM, no full-resolution artwork retained, no per-frame work when idle.
- **Open source:** No secrets in the repo; credentials stored in Secret Service; no telemetry.

---

## 4. Users and Context

**Primary user:** SamVivan, working at a Linux desktop for long sessions, listening to a personal Navidrome library, wanting lyrics visible while working in other apps.

**Secondary user (open source):** A Linux user with a Navidrome server who wants a lightweight native client and is comfortable installing a GNOME extension.

Usage modes:

1. **Lean-back listening:** Home → album/playlist → now playing with lyrics.
2. **Work companion:** Music keeps playing; a pinned lyric panel stays on screen; the main window can be hidden/minimized.
3. **Quick search:** Ctrl+K → search → play; no mouse required.
4. **Setup once:** Server URL + credentials; then never again.

---

## 5. Experience

### 5.1 Main window

Spotify/Feishin-style three-zone layout:

- **Left sidebar (approx. 212 px):** logo, Home, Search, Albums, Artists, Playlists, Favorites; playlist shortcuts; bottom "Pin lyrics" toggle and overlay status.
- **Main content:** route-dependent; Home shows "Recently added", "Recently played", "Your playlists", and a random/shuffle shelf. Album and artist grids use cover art. Track lists are virtualized.
- **Bottom player bar:** floating ivory clay pill, album art, title/artist, transport, seek bar, volume, "Lyrics" button. The player bar is the primary clay object; it anchors the material system.

The app window uses the glass shell: warm-black translucent background with native blur, thin champagne border, and a subtle inner highlight. Content panels are glass; controls are clay; gold is reserved for state.

### 5.2 Home

- Greeting in Fraunces ("Good evening").
- Search field (clay inset) with Ctrl+K hint and a blue focus ring.
- Shelves:
  - Recently added (server `getAlbumList2` newest).
  - Recently played (server `getAlbumList2` recent).
  - Your playlists.
  - Shuffle / random albums (explicitly labeled, not fake recommendations).
- Empty/error/loading states with useful actions.

### 5.3 Album, artist, playlist, favorites

- Album: cover, title, artist, year, track list, play/shuffle, star/unstar, "Add to queue" (queue addition v1; playlist editing not).
- Artist: artist image if available, albums, top songs if returned.
- Playlist: read-only; play/shuffle; track list.
- Favorites: starred albums/artists/songs from Navidrome; star/unstar.

### 5.4 Search

- Ctrl+K or sidebar; 250 ms debounce; grouped results: Artists, Albums, Songs, Playlists.
- Keyboard navigation; Enter plays the first result or activates the selected one; Esc returns.
- Recent searches optional; not v1.

### 5.5 Now Playing + lyrics

- Two-column desktop layout: large album art and metadata on the left; synced lyrics panel on the right.
- Lyrics panel: source label (Navidrome/LRCLIB), offset controls (−0.5 s / +0.5 s), "Pin to desktop" button.
- Active line is centered vertically, in champagne gold with a gold dot and a soft underline; surrounding lines are muted ivory. The list animates with a 380 ms ease-out scroll when the active line changes. Plain lyrics show as a readable block; instrumental shows "Instrumental"; no lyrics shows a calm empty state with a retry.
- Seeking resyncs immediately; offset changes resync immediately.

### 5.6 Pinned lyric overlay

- A separate GPUI window titled exactly `Myuzik Lyrics`, app ID `io.github.samvivan1.Myuzik`.
- Borderless, transparent, native blur, draggable by its surface; size ~392 × 160–220 px depending on lyric wrapping; position/size persisted.
- Content: small gold "Pinned lyrics" pill, close button, album art, title/artist, previous/active/next lines, progress bar, pin status.
- Toggle from player bar/sidebar (and Ctrl+P). Opening the window = pinned; closing = unpinned. The GNOME extension auto-pins windows with the matching title/app ID using `make_above()` + `stick()`; it can also hide it from the window list.
- If the extension is not installed, show a one-time hint in Settings/status: install `myuzik-pin@samvivan1.github.io`; the overlay still works as a normal floating window.
- The overlay is readable over any wallpaper: dark tint ≥ ~72% opacity, high-contrast ivory text, gold active line, no reliance on blur for contrast.

### 5.7 Settings

- Account: server URL, username, password (change), test connection.
- Playback: stream quality (Original / 320 kbps MP3 / 192 kbps MP3), volume, scrobble on/off.
- Lyrics: offset slider, font size (later), source preference (Navidrome first / LRCLIB first), clear cache.
- Overlay: opacity, always-on-top status, install-extension instructions, reset position.
- Appearance: reduce motion, follow OS accent (on by default), theme info (Midnight Brass v1).
- About: version, license, logs path, "copy diagnostics".

### 5.8 First-run setup

- Full-window setup card: server URL, username, password, "Test connection" and "Save".
- Validation: URL parse, ping, auth error, TLS error, timeout. Errors are inline and actionable.
- Credentials: password saved in Secret Service; if unavailable, warn and offer a 0600 config fallback (explicit user choice).
- After success: Home.

---

## 6. Visual Design — Midnight Brass

### 6.1 Palette

| Token | Value | Use |
|---|---|---|
| `bg.base` | `#0B0B0E` | Warm near-black window/page background |
| `bg.raised` | `#121218` | Raised dark surfaces |
| `glass.tint` | `rgba(12,12,16,0.72)` | Glass shell/panels |
| `glass.border` | `rgba(232,211,163,0.16)` | 1px champagne hairline |
| `text.primary` | `#F5EFE4` | Ivory text |
| `text.secondary` | `#B8B0A2` | Muted body |
| `text.tertiary` | `#7C756A` | Disabled/captions |
| `gold.primary` | `#C9A86A` | Brass state, progress, play, active lyric |
| `gold.highlight` | `#E8D3A3` | Active lyric text, highlights |
| `gold.deep` | `#8C6D3F` | Deep gold shadows/details |
| `clay.surface` | `#F4EBDD` | Clay control surface |
| `clay.shade` | `#E7D8C2` | Clay gradient/depth |
| `clay.ink` | `#2A241C` | Text/icons on clay |
| `os.accent` | `#3584E4` fallback | Focus rings, selection, shortcut hints; read from portal |
| `danger` | `#D96C6C` | Destructive/error |

**Rules**

- Gold is 5–10% of the screen: active lyric, progress, play, logo, small state labels. Never a large surface.
- OS accent is functional only: focus ring, text selection, keyboard hints. Not decorative, not the play button.
- Glass is the room; clay is the object; both are neutral. Album art supplies the color.
- No purple/blue neon gradients, no glitter, no pure `#FFD700`, no rainbow accents.
- Text contrast: body ≥ 4.5:1; large display text ≥ 3:1; focus ring ≥ 3:1 against adjacent colors. Gold text only on dark glass and only at ≥ 13 px semibold; body text is ivory.
- Clay uses warm shadows (`rgba(90,65,30,…)`), never cool purple shadows.

### 6.2 Typography

- **Display:** Fraunces (variable, soft serif) — page titles, greeting, album/now-playing titles, section headings.
- **UI/lyrics:** Inter — navigation, labels, body, track metadata, lyrics.
- Bundled with OFL licenses; system fallback: `Cantarell`, `Inter`, `sans-serif` for UI; `Georgia`, serif for display.
- Scale: display 30–38 px; section 16–18 px; body 13–14 px; captions 9–11 px; lyrics 15 px inactive / 21 px active; pinned overlay 11–13 px. No pixel font in v1.
- Line height: body 1.45–1.6; lyrics 1.35–1.45; display 1.1–1.2.

### 6.3 Materials and shape

- **Glass:** 18–24 px radius, 1px champagne border, `blur(24–30px)`, inner top highlight, soft dark outer shadow.
- **Clay:** 14–30 px radius, ivory/sand gradient, 1 px light border, inner light + inner dark shadows, soft warm outer shadow; press state scales to ~0.97 and compresses the shadow (120–160 ms).
- **Gold state:** matte gradient `#E8D3A3 → #C9A86A`; no metallic texture.
- **Focus:** 2 px OS-accent ring, 2–3 px offset, visible on clay and glass. Use a dark spacer ring when needed for contrast.

### 6.4 Motion

- Lyrics scroll: 380 ms ease-out-cubic, active line centered; jump immediately on seek or when reduced motion is enabled.
- Clay press: 120–160 ms springy scale/shadows; release returns with a short ease-out.
- Route transitions: 160–220 ms fade/slide; no long decorative transitions.
- Hover: 150–200 ms; never rely on hover for essential actions.
- Reduced motion: read `enable-animations` (or app setting); disable lyric scroll animation, route slides, and press bounce; keep opacity changes minimal.

### 6.5 Iconography

- Rounded line icons, 1.8 px stroke, 24 px viewBox; inline SVGs / assets. No emoji as icons.
- Icon-only buttons require an accessible label and tooltip.
- Logo (app icon): a brass "M" whose valley descends into a tilted stylus, inside a single brass
  ring on a warm near-black plate. One unioned path, no text and no font dependency, so it renders
  the same on every system. Source of truth
  `packaging/icons/hicolor/scalable/apps/io.github.samvivan1.Myuzik.svg`; the PNG sizes come from
  `scripts/render-icons.sh`. Rationale and rejected directions:
  [superpowers/specs/2026-11-10-logo-design.md](superpowers/specs/2026-11-10-logo-design.md).
- In-app sidebar mark: brass clay tile with a Fraunces "M", used at 30–32 px. It still renders a
  font glyph, so it matches the app icon's idea rather than its exact contours; unifying the two
  is a UI change tracked in the logo design doc.

### 6.6 Accessibility

- Full keyboard navigation; visible focus; Space/arrows/Enter/Esc; Ctrl+K, Ctrl+P.
- Every control has an accessible label; icon-only buttons are named.
- Contrast verified for text, focus rings, active lyric, and clay buttons.
- No color-only state: pin state has label/icon; errors have text.
- Reduced motion and font scaling supported.
- Persistent player/overlay must not obscure focused elements; focus scrolls into view.
- Lyrics never truncate the active line in the pinned overlay; wrap long lines.

---

## 7. Technical Architecture

### 7.1 Stack

| Layer | Choice |
|---|---|
| Language | Rust 1.99 (edition 2024) |
| UI | GPUI (Zed) pinned to `14b7b0ef3764d62a85070c8b5ccea105a2ad3235` or later; features `wayland`, `x11`, `font-kit`, `image-default-formats` |
| Audio | GStreamer 0.25 bindings, feature `v1_24`; `playbin3`/`playbin`, `autoaudiosink`; system GStreamer 1.24+ |
| HTTP | `ureq` 3.5 with rustls; blocking calls on GPUI background tasks |
| Server API | Navidrome Subsonic API 1.16.1 + OpenSubsonic extensions |
| Lyrics | Navidrome `getLyricsBySongId` (OpenSubsonic) + LRCLIB; local JSON cache |
| MPRIS | `mpris-server` 0.11 + zbus |
| Credentials | `keyring` 4.2 (Secret Service); 0600 config fallback with explicit consent |
| Config | TOML at `~/.config/myuzik/config.toml` |
| Cache | `~/.cache/myuzik/{covers,lyrics}`; LRU cover cache; hashed lyric JSON |
| Fonts | Bundled Fraunces + Inter (OFL) |
| Packaging | Cargo release build, desktop file, icon, install script, GNOME extension |
| OS integration | XDG portal `org.freedesktop.appearance accent-color` via `ashpd` 0.14; GNOME extension `myuzik-pin@samvivan1.github.io` |

### 7.2 Process and threads

- **UI thread:** GPUI app, views, entity state. No blocking I/O.
- **HTTP/background pool:** GPUI `background_spawn` for API calls, cover downloads, lyric fetches; results sent back via channels/entities.
- **Player thread:** owns GStreamer pipeline and a GLib main loop; receives commands over a channel; emits state/position/duration/EOS/error events; 100 ms position tick while playing.
- **MPRIS thread:** zbus event loop; reads shared `PlayerSnapshot` and forwards commands; updates properties.
- **Overlay window:** same GPUI process/app; second window; observes the same player/lyrics entities.

### 7.3 Modules

```
src/
  main.rs                 app bootstrap, logging, single-instance guard
  app.rs                  root app, global entities, window management, routes
  config.rs               config/cache paths, TOML, credentials, defaults
  theme.rs                Midnight Brass tokens, materials, typography, accent
  assets.rs               embedded fonts/icons, GPUI AssetSource
  util.rs                 time formatting, hashing, debounce, LRU
  navidrome/
    mod.rs
    client.rs             auth, URL builder, GET/stream/cover URLs, error mapping
    types.rs              Subsonic DTOs + domain mapping
    error.rs
  library/
    mod.rs                library state/entities, routes, pagination
    browse.rs             home shelves, album/artist/playlist loading
    search.rs             debounced search3
    cache.rs              in-memory LRU for albums/artists/covers
  player/
    mod.rs
    engine.rs             GStreamer thread, commands/events
    queue.rs              queue, shuffle, repeat, next/prev
    state.rs              PlayerState entity, current track, position
    scrobble.rs           threshold and submission logic
  lyrics/
    mod.rs
    lrc.rs                parse LRC, active index, offsets
    provider.rs           Navidrome + LRCLIB fallback, cancellation
    cache.rs              disk cache keys/read/write
    model.rs              LyricLine, LyricsStatus
  ui/
    mod.rs
    shell.rs              sidebar, content router, player bar, toasts
    setup.rs              first-run form
    home.rs
    album.rs
    artist.rs
    playlist.rs
    search.rs
    favorites.rs
    now_playing.rs
    settings.rs
    components/
      mod.rs
      glass_panel.rs
      clay_button.rs
      album_card.rs
      track_row.rs
      lyrics_view.rs      shared in-app/overlay lyrics renderer
      progress.rs
      icons.rs
  overlay/
    mod.rs                window open/close, title/app_id, geometry persistence
    view.rs               pinned lyric panel
  platform/
    accent.rs             portal accent + fallback
    mpris.rs              MPRIS server thread
    gnome.rs              optional extension detection/hint
gnome-extension/
  myuzik-pin@samvivan1.github.io/
    metadata.json
    extension.js
    README.md
assets/
  fonts/ (Fraunces, Inter, OFL)
  icons/ (SVG)
  app-icon.svg
packaging/
  io.github.samvivan1.Myuzik.desktop
  io.github.samvivan1.Myuzik.metainfo.xml
  icons/hicolor/scalable/apps/io.github.samvivan1.Myuzik.svg
tests/
  (integration tests + mock Subsonic server)
```

### 7.4 Navidrome client

- Base URL from config; endpoints under `/rest/<method>.view`.
- Auth: random 16-byte salt (`s`), token `t = md5(password + salt)`, plus `u`, `v=1.16.1`, `c=myuzik`, `f=json`.
- Generate a fresh salt per request; never log the token/salt.
- Parse `{"subsonic-response":{"status":"ok"|"failed",...}}`; map `error.code`/`message` to typed errors.
- Endpoints used in v1:
  - `ping`
  - `getAlbumList2` types `newest`, `recent`, `random`, `frequent`, `starred` (size/offset)
  - `getArtists`, `getArtist`, `getAlbum`
  - `getPlaylists`, `getPlaylist`
  - `search3`
  - `getSong`
  - `getCoverArt` (`size=512`)
  - `stream` (`format` optional; original by default)
  - `star`, `unstar`
  - `scrobble` (`submission=true`, `time` in ms)
  - `getLyricsBySongId` (OpenSubsonic; ignore if unsupported)
- `stream_url` and `cover_art_url` are URLs with auth params; GStreamer fetches the stream; cover art via `ureq`.
- Response size limits: JSON ≤ 8 MB, cover ≤ 8 MB; timeouts 12–20 s; cancellation for stale searches/lyrics.

### 7.5 Playback

- Player thread:
  - Build `playbin3` (fallback `playbin`), `autoaudiosink`, `user-agent=Myuzik/<version>`.
  - Commands: `Load { uri, track_id }`, `Play`, `Pause`, `Toggle`, `Seek(Duration)`, `SetVolume(f64)`, `Stop`.
  - Bus: EOS → next; error → player state error + toast; buffering → UI indicator; state-changed → update.
  - Position poll: 100 ms timer while playing; sends `Position` and `Duration`.
- Queue: `VecDeque<Track>` + current index; repeat off/all/one; shuffle uses a deterministic permutation and preserves current; next/prev; add-to-queue; "play next" optional.
- Scrobble: send `submission=false` when a track starts; send `submission=true` once when position ≥ 50% or ≥ 4 minutes, whichever comes first; never double-submit per track instance.
- Stream quality: Original by default; 320/192 kbps MP3 options map to `format=mp3&maxBitRate=...`.
- Error handling: network drop → pause + toast + retry control; unsupported codec → suggest transcoded quality; 401 → return to setup/account.

### 7.6 Lyrics

- Source order (configurable): Navidrome `getLyricsBySongId` → LRCLIB; or LRCLIB first.
- Normalize match key: lowercase trimmed artist/title/album + rounded duration; cache key SHA-256.
- LRCLIB: `/api/get` with artist/track/album/duration; fallback `/api/search`; User-Agent `Myuzik/<version> (repo URL)`; 12 s timeout; 1 MB max.
- Parse LRC: `[mm:ss.xx]`, multiple stamps per line, hours, centiseconds/milliseconds; sort; active index by binary search.
- Cache JSON in `~/.cache/myuzik/lyrics/<sha256>.json`; read-through; write atomically; prune with age/size policy (e.g., 500 files / 50 MB).
- Prefetch next track when queue has a next item; cancel stale fetch on track change.
- Lyrics status: `Loading`, `Synced(lines)`, `Plain(text)`, `Instrumental`, `NotFound`, `Error`.
- Offset: user ms offset applied to position before active-index lookup.
- Shared renderer: `LyricsView` used in Now Playing and overlay; line heights measured once per width; active line centered; animate translation; wrap long lines in overlay.

### 7.7 Pinned overlay and GNOME extension

- Overlay window:
  - `WindowOptions { window_decorations: None, window_background: Blurred, focus: false, show: true, app_id: Some(APP_ID) }`; title exactly `Myuzik Lyrics`.
  - Borderless; drag via `window.start_window_move()` on the header; resize handle optional; persist x/y/w/h in config.
  - Toggle opens/closes the window; no separate process.
  - Native blur only on supported compositors; fallback to a 78–86% opaque dark tint.
- GNOME extension:
  - UUID `myuzik-pin@samvivan1.github.io`; shell versions 45–51.
  - On `window-created`, match `metaWindow.get_title() === "Myuzik Lyrics"` or app ID `io.github.samvivan1.Myuzik`; after first frame call `make_above()` and `stick()`; optionally `hide_from_window_list()`.
  - On enable, scan existing windows; on disable, leave windows normal.
  - No settings needed for v1; a `settings-schema` can add `enabled`/`hide-from-window-list` later.
  - Install instructions: `gnome-extensions pack`, `gnome-extensions install --force`, enable; on Wayland, log out/in if the extension was just installed.
- Other compositors: overlay works as a normal window; a future layer-shell backend is out of scope v1.

### 7.8 MPRIS

- Identity: `org.mpris.MediaPlayer2.Myuzik`; capabilities: raise, quit, can-control.
- Player interface: Play/Pause/PlayPause/Stop/Next/Previous/Seek/SetPosition/OpenUri; `PlaybackStatus`, `LoopStatus`, `Rate`, `Shuffle`, `Metadata`, `Volume`, `Position`, `MinimumRate`, `MaximumRate`, `CanGoNext`, `CanGoPrevious`, `CanPlay`, `CanPause`, `CanSeek`, `CanControl`.
- Metadata: `xesam:title`, `xesam:artist`, `xesam:album`, `xesam:albumArtist`, `mpris:artUrl` (cached file URI), `mpris:length`, `xesam:trackNumber`, `xesam:discNumber`, `xesam:url`.
- Art URL: cached cover file path; if missing, empty.
- Position: update via MPRIS `PropertiesChanged` at ~1 Hz when playing; seek updates immediately.
- This also lets the user's existing GNOME media widget / media-controller extension follow Myuzik.

### 7.9 OS accent

- Read `org.freedesktop.portal.Settings` key `org.freedesktop.appearance` → `accent-color` via `ashpd`; returns RGB in [0,1].
- Fallback: GNOME `org.gnome.desktop.interface accent-color` mapping (blue/teal/green/yellow/orange/red/pink/purple/slate) via `gsettings` command or `gio`; fallback `#3584E4`.
- Use only for focus ring, selection, keyboard hint accents. Gold remains brand/state.
- If the OS accent is very light/dark, adjust focus ring contrast with a dark spacer ring; test against glass and clay surfaces.

### 7.10 Data flow

```
Setup ──> Config + Secret Service
   │
   ├─> NavidromeClient (background HTTP)
   │       ├─> LibraryState (albums/artists/playlists/search)
   │       ├─> CoverCache (disk + memory LRU)
   │       └─> LyricsProvider (Navidrome/LRCLIB + disk cache)
   │
   └─> PlayerEngine (GStreamer thread)
           ├─> PlayerState (queue/status/position)
           ├─> MPRIS thread
           └─> Lyrics sync (position + active line)
                   ├─> Now Playing view
                   └─> Pinned overlay window
```

### 7.11 Error handling

- Typed errors per subsystem: config, network, auth, API, playback, lyrics, storage.
- User-facing: plain-language toast/inline error with one action (retry, open settings, re-authenticate, choose transcoded quality).
- Logs: `tracing` to `~/.cache/myuzik/logs/myuzik.log` (rotating, 5 MB × 3); redact tokens/salts/passwords and full URLs with query strings.
- No panics for expected failures; no unwrap on I/O.
- Offline: show cached metadata/covers where available; playback errors are recoverable.

### 7.12 Security and privacy

- HTTPS by default; do not disable certificate validation in v1 (self-signed LAN servers are a documented limitation).
- Password only in Secret Service; fallback config is 0600 and requires explicit consent.
- Token/salt generated per request; never persisted.
- No telemetry/analytics.
- LRCLIB receives artist/title/album/duration only; cached locally; can be disabled.
- File cache names are SHA-256 hashes; no track titles in paths.
- Limit response bodies; validate image content type; reject oversized data.

### 7.13 Performance budgets

- Idle (no playback, main window open): **≤ 200 MB RSS**.
- Playing with lyrics + overlay: **≤ 250 MB RSS**.
- Overlay closed: no overlay render loop; overlay open + playing: animation only on active line changes, position ticks 10 Hz max.
- Startup: **≤ 1.5 s** to first usable frame on an SSD; no network required to show the shell (cached credentials/config; home loads async).
- Covers: request 512 px; memory LRU ≤ 200 entries; decode off the UI thread; release GPU images when windows close.
- Lyrics: only current/nearby tracks; prefetch next at most one; cache JSON small.
- Lists: virtualized; no full-library fetch; paginate 50–100 items.

### 7.14 Testing

- Unit: LRC parser/active index/offset; auth token/salt; Subsonic URL builder; queue next/prev/shuffle/repeat; scrobble threshold; cache keys; theme contrast ratios; config migration/defaults.
- Integration: mock Subsonic HTTP server (wiremock/httpmock) for ping/auth/error, album lists, stream URL, cover art, lyrics; test typed errors and retries.
- UI smoke: launch app with a fake/empty library and mock server; routes render; keyboard navigation; overlay open/close; no panic.
- Manual: GNOME 51 Wayland (blur + extension), GNOME 51 X11, no-extension overlay, no-blur compositor, 100%/200% scaling, multi-monitor, long/non-Latin lyrics, instrumental/no lyrics, network loss, bad credentials, large library, 24-hour idle memory.
- CI: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`; release build; GStreamer dev packages installed.

---

## 8. Scope and Milestones

### v1.0 Must

- Setup/auth, Home, Albums, Artists, Playlists (read), Favorites, Search.
- Playback + queue + seek + volume + MPRIS.
- Now Playing + synced/plain/instrumental lyrics, offset, cache.
- Pinned lyric overlay + GNOME pin extension.
- Settings, OS accent focus, reduced motion.
- Packaging, README, license, CI.

### v1.1 Could

- Playlist editing (create/rename/add/remove/reorder).
- Pixel theme as an optional theme.
- Last.fm/ListenBrainz, offline cache, gapless/crossfade, equalizer.
- Layer-shell overlay for KDE/Hyprland.
- Light theme variant.

### Won't (v1)

- Mobile/web/Windows/macOS, server management, tag editing, local files, social features.

---

## 9. Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| GPUI API churn | Build breaks | Pin exact Zed commit; isolate GPUI behind app modules; budget for upgrades |
| Native blur not in crates.io gpui | Glass looks flat | Pin post-2026-10-30 commit; fallback tint; test no-blur |
| GNOME extension matching fails | Overlay not pinned | Match both title and app ID; scan existing windows; log; provide troubleshooting |
| GStreamer dev/runtime missing | Cannot build/play | Document apt packages; CI installs; detect at startup with actionable error |
| Navidrome version lacks `getLyricsBySongId` | No server lyrics | Fall back to LRCLIB; treat 404/unsupported as normal |
| LRCLIB mismatch/rate limit | Wrong/no lyrics | Exact get → search; cache; normalize; show "not found"; no repeated requests |
| Large library | Slow/memory | Server pagination, virtualized lists, LRU caches, no full fetch |
| Credentials storage | Security | Secret Service; 0600 fallback with consent; never log |
| Wayland always-on-top limitations | UX expectation | Companion extension; explicit hint; no false promise |
| Long/non-Latin lyrics | Overflow/readability | Wrap, font fallback, dynamic line heights, test CJK/Arabic/Indic |
| Seek/EOS race | Wrong track/state | Single player thread, serialized commands, state machine, tests |
| Memory regressions | Core value lost | Budgets in CI/manual test; measure idle/playing; no per-frame idle work |

---

## 10. Open Decisions (for the builder/user)

1. Final app ID / repository owner string: design uses `io.github.samvivan1.Myuzik` and extension UUID `myuzik-pin@samvivan1.github.io`; change in one constants module if the GitHub handle differs.
2. License: MIT assumed; confirm before publishing.
3. Navidrome server URL/version and whether it has embedded lyrics; setup + endpoint probing handle both, but testing needs a real server.
4. Stream quality default (Original vs 320 MP3); design defaults to Original with a setting.
5. Secret Service availability on the target machine; fallback is defined.
6. Whether playlist editing is required for 1.0 (currently v1.1).
7. Whether to bundle fonts (yes for v1; adds ~1 MB) vs system fallback.
8. Whether to ship a Flatpak in v1 (not required; install script first).

---

## 11. Definition of Done (v1.0)

- [ ] A clean Ubuntu 26.10 + GNOME 51 Wayland machine can install dependencies, build, and launch following README.
- [ ] Setup connects to a real Navidrome, stores the password in Secret Service, and reaches Home.
- [ ] Play/pause/seek/next/prev/volume/shuffle/repeat work for original and transcoded streams.
- [ ] MPRIS controls work from GNOME's media menu and the user's existing media widget.
- [ ] Lyrics load from Navidrome or LRCLIB, cache, sync, offset, plain/instrumental/not-found states are correct.
- [ ] The pinned overlay opens/closes from the app, stays above windows across workspaces when the extension is enabled, and remains readable without blur.
- [ ] OS accent blue is used for focus/selection; gold is used only for state; Midnight Brass tokens match the spec.
- [ ] Keyboard navigation, focus visibility, reduced motion, and contrast checks pass.
- [ ] Idle RSS ≤ 200 MB and playing RSS ≤ 250 MB in the documented measurement.
- [ ] No secrets in logs; no telemetry; caches are bounded.
- [ ] CI passes fmt, clippy, tests; README has screenshots, setup, extension install, troubleshooting, and license.

---

## 12. Approval

Visual direction: approved by user ("nice banget") on 2026-11-09.
Implementation approach: pending approval of the implementation plan and execution mode.
