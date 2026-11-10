# Myuzik

A low-RAM, native Linux music player for a self-hosted **Navidrome** server, with synced lyrics
and a desktop-pinned lyric overlay. Built with Rust, [GPUI](https://github.com/zed-industries/zed),
and GStreamer.

> v0.1.0. The core library, playback, lyrics, overlay, search, settings, scrobbling, and MPRIS
> flows work, with full keyboard navigation. Playlist editing, offline downloads, gapless
> playback, and Flatpak are not in this version.

## Features

- Browse **recent**, **starred**, **artists**, and **playlists** from Navidrome (Subsonic API).
- Real cover art, with an in-memory LRU and 512 px covers.
- Playback via GStreamer (`playbin`), with queue, next/previous, repeat/shuffle, seek, volume.
- **Synced lyrics** from Navidrome (OpenSubsonic) with LRCLIB fallback and a disk cache.
- **Pinned lyrics overlay** — a borderless, always-above window; on GNOME, a small Shell extension
  pins it above other windows and across workspaces.
- **MPRIS** controls and metadata (works with `playerctl`, GNOME media menu, etc.).
- **Scrobbling** (now-playing + submitted) to Navidrome.
- Debounced, keyboard-driven **search**.
- **Settings** for stream quality, scrobbling, lyric offset/source, reduced motion, accent.
- Midnight Brass design: warm black glass, ivory clay, champagne gold, OS accent focus rings.
- **Keyboard accessible**: tab order across sidebar → content → player bar → window chrome,
  OS-accent focus rings, accesskit roles/labels on every icon-only control, reduced motion.
- Bundled Fraunces + Inter fonts (OFL).
- No telemetry. Credentials in the Secret Service, with a 0600 file fallback.

## Prerequisites

- Linux with Wayland (primary) or X11/XWayland.
- Rust 1.99+ (edition 2024).
- System libraries:
  - Debian/Ubuntu: `libwayland-dev libxkbcommon-dev libvulkan-dev libfontconfig1-dev
    libfreetype-dev libssl-dev clang cmake pkg-config libgstreamer1.0-dev
    libgstreamer-plugins-base1.0-dev gstreamer1.0-plugins-good gstreamer1.0-plugins-bad`
  - Fedora: `wayland-devel libxkbcommon-devel vulkan-loader-devel fontconfig-devel freetype-devel
    openssl-devel clang cmake pkgconf gstreamer1-devel gstreamer1-plugins-base-devel
    gstreamer1-plugins-good gstreamer1-plugins-bad-free`
- A running Navidrome server (tested with 0.64.x) and a Subsonic user.

## Build and run

```bash
git clone https://github.com/SamVivan1/myuzik
cd myuzik
cargo run --release
```

On first launch Myuzik shows a **Connect to Navidrome** form. Enter the server URL
(`https://music.example.com`), username, and password. Tab switches fields, Enter connects.
The URL and username go to `~/.config/myuzik/config.toml`; the password is stored in the Secret
Service if available, otherwise in `~/.config/myuzik/credentials` with mode `0600`.

### Install for the current user

```bash
scripts/install.sh                # binary + desktop entry + icon in ~/.local
scripts/install.sh --with-extension  # also install the GNOME pin extension
myuzik
```

`scripts/install.sh --uninstall` removes only Myuzik files.

## GNOME pin extension

The overlay window cannot keep itself above other windows from a normal Wayland app. The companion
extension `myuzik-pin@samvivan1.github.io` calls `make_above()` and `stick()` on windows titled
exactly `Myuzik Lyrics`.

```bash
scripts/install-extension.sh
```

Log out/in after the first extension install, then enable **Myuzik Pin** in GNOME Extensions.
Without the extension the overlay still opens as a normal window. Remove it with
`scripts/uninstall-extension.sh`. Compatibility matrix, what pinning does and does not do, and
troubleshooting: [docs/gnome-extension.md](docs/gnome-extension.md).

## Keyboard and controls

- `Space` play/pause · `←`/`→` seek 5 s · `Shift+←`/`Shift+→` previous/next
- `↑`/`↓` volume · `Ctrl+K` search · `Ctrl+P` pin lyrics · `Esc` close overlay
- Transport buttons in the clay player bar: previous, play/pause, next, −10 s, +10 s.
- Volume −/+ (also available over MPRIS).
- Click the now-playing area or **Now Playing** in the sidebar for cover + lyrics.
- **Pin lyrics** toggles the desktop overlay.
- Search: click **Search**, type, press Enter; click a result to play.

## Memory

Measured with the release binary on GNOME 51 Wayland, AMD Radeon 760M:

| State | RSS |
|---|---|
| Idle (library loaded) | ~133 MB peak |
| Playing (48 kHz stream, covers loaded) | ~160–184 MB |
| Startup to a usable window | 1.12 s cold / 0.33 s warm |

Budgets: idle ≤ 200 MB, playing + overlay ≤ 250 MB, startup ≤ 1.5 s. Measure your own build
with `scripts/measure-ram.sh`; method and per-test bounds are in
[docs/performance.md](docs/performance.md).

## Testing

```bash
./scripts/test-all.sh                 # fmt + clippy + 79 tests (31 unit + 48 headless UI)
MYUZIK_LIVE=1 ./scripts/test-all.sh   # also live Navidrome (7) + GStreamer (1) tests
scripts/measure-ram.sh                # RSS / CPU / startup of the release binary
```

See [docs/testing.md](docs/testing.md) for the coverage map and manual smoke-test commands.

## Configuration

`~/.config/myuzik/config.toml`:

```toml
server_url = "https://navidrome.example.com"
username = "you"
quality = "original"          # original | kbps320 | kbps192
scrobble = true
volume = 0.8
lyrics_offset_ms = 0
prefer_navidrome_lyrics = true
reduce_motion = false
follow_os_accent = true
```

Caches live in `~/.cache/myuzik/` (`covers/`, `lyrics/`, `logs/`).

## Troubleshooting

See [docs/troubleshooting.md](docs/troubleshooting.md). Common issues:

- **No window appears**: ensure the `wayland` feature is enabled for `gpui_platform` (it is in this
  repo). Without it GPUI runs headless.
- **Overlay not above other windows**: install/enable the GNOME extension; log out/in after install.
- **Codec errors**: install `gstreamer1.0-plugins-good` / `-bad`.
- **Wrong password**: re-run the setup form or update `~/.config/myuzik/credentials`.
- **Blur**: Mutter does not implement `ext-background-effect-v1`; Myuzik simulates glass with
  translucency and ambient color instead of compositor blur.

## What is not in v0.1.0

- Playlist editing, offline downloads, gapless playback.
- Flatpak/AppImage packaging.
- Pixel theme (Midnight Brass only).
- Last.fm (Navidrome handles scrobbling).
- Accesskit tree-level assertions, forced-colors/high-contrast and a 200% scaling review
  (the first two need APIs the pinned gpui revision does not expose; see
  [docs/accessibility.md](docs/accessibility.md)).

## License

MIT. Bundled fonts are under the SIL Open Font License (see `assets/fonts/`).
