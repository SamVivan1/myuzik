# v0.1.0 acceptance matrix

Date: 2026-11-10. Target: GNOME 51 Wayland, AMD Radeon 760M, Navidrome 0.64.2.

| Area | Check | Result |
|---|---|---|
| Setup | Clean config shows the in-app setup form | ✅ headless test `setup_form_validates_and_types` |
| Setup | Connect with URL/user/password saves config + 0600 credentials and reaches Home | ✅ `setup_form_connects_and_saves` |
| Setup | Wrong scheme rejected inline | ✅ `setup_form_validates_and_types` |
| Library | Newest albums, artists, playlists, favorites load | ✅ live `live_ping_and_library`, `live_artists_playlists_search` |
| Library | Album detail returns songs | ✅ live `live_album_detail_and_cover` |
| Covers | 512 px cover bytes cached and rendered | ✅ `home_album_track_play_flow`, real app |
| Search | Type + Enter returns grouped results; click plays | ✅ `search_flow_returns_results`, `stale_search_response_is_discarded` |
| Playback | Stream plays, position advances, duration reported | ✅ `live_player_plays_pauses_seeks` |
| Playback | Pause and seek work | ✅ live player test |
| Queue | Next/previous/repeat/shuffle, rapid 100× next/prev | ✅ `tests/queue.rs` (8 tests) |
| Playback | EOS advances to next track | ✅ implemented (`play_next`); MPRIS `next` verified live |
| Player bar | Play/pause, ±10 s, volume ±, progress, current cover | ✅ `home_album_track_play_flow` + real app |
| Shortcuts | Space, arrows, Shift+arrows, Ctrl+K, Ctrl+P, Esc | ✅ `global_shortcuts_work` |
| MPRIS | Bus name, status, metadata, artUrl, next/prev, seek, volume | ✅ `playerctl` on the real app |
| Lyrics | Navidrome `lyricsList` synced lyrics | ✅ `live_navidrome_lyrics_list_shape` |
| Lyrics | LRCLIB fallback + cache | ✅ `live_lyrics_pipeline` |
| Lyrics | Offset ±500 ms, stale response discarded | ✅ `home_album_track_play_flow`, `stale_lyrics_response_is_discarded` |
| Overlay | Open/close/toggle, lyrics + active line sync | ✅ `overlay_opens_and_closes`, flow test |
| Overlay | Geometry saved and restored | ✅ `overlay_opens_and_closes` (config written) |
| Overlay | GNOME pin extension | ⚠️ files + install script present; not installed/enabled in this session |
| Favorites | Starred albums load; star/unstar album | ✅ `home_album_track_play_flow`, live |
| Scrobble | Now-playing + submitted after threshold | ✅ `home_album_track_play_flow`, `tests/scrobble.rs` |
| Errors | Unreachable server → toast, app usable | ✅ `unreachable_server_shows_error_toast` |
| Theme | Contrast AA, gold state, OS accent focus | ✅ `tests/theme_contrast.rs` |
| Motion | Route fade + equalizer; reduced motion toggle | ✅ `settings_controls_update_config`, `AnimationExt` |
| Memory | Idle ≤ 200 MB, playing ≤ 250 MB | ✅ 125 MB idle / ~160–184 MB playing (release) |
| Secrets | Tokens/salts never logged | ✅ `src/platform/logging.rs` tests |
| Format | `cargo fmt --check`, clippy `-D warnings` | ✅ `scripts/test-all.sh` |
| Tests | Unit + headless UI + live suites | ✅ 30 lib + 25 integration + 7 live (ignored by default) |
| CI | GitHub Actions workflow present | ✅ `.github/workflows/ci.yml` (not executed on GitHub here) |
| Packaging | Desktop/AppStream/icon/install script | ✅ `desktop-file-validate`; AppStream warnings only for the not-yet-published repo URL |

## Known limitations

- GNOME is required for pinning the overlay above other windows (a normal Wayland app cannot do it).
- Mutter does not blur behind the window (`ext-background-effect-v1`); glass is simulated.
- MPRIS art URL uses the cached cover file; if the cover fetch fails, no art URL is published.
- No playlist editing, offline downloads, gapless playback, or Flatpak in v0.1.0.
- Global keyboard shortcuts require the main window to be focused; text fields keep their typing.
- Full a11y focus order/automated accessibility tree is still partial (see `docs/accessibility.md`).

## Release steps

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
MYUZIK_LIVE=1 ./scripts/test-all.sh
cargo build --release
git tag v0.1.0
```

Pushing the tag and creating the GitHub release is left to the maintainer.
