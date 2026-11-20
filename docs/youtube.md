# YouTube Music (optional, unofficial)

Myuzik can search YouTube Music and play the results next to a Navidrome library. The feature is
optional and off by default. It is **unofficial**: YouTube's terms do not grant third-party
clients a right to extract streams, and upstream changes can break playback at any time. Use it
for personal listening and do not add a download/export feature.

## How it works

```
Home / Search / Album / Artist / Playlist / Moods
  └─ MusicService → YouTubeService (Rust, background executor)
       └─ YouTubeProvider
            └─ one-shot Python helper (helper/ytmusic.py)
                 ├─ ytmusicapi  → search, home, browse, moods
                 └─ yt-dlp      → download the audio
  └─ buffered file in ~/.cache/myuzik/streams
       └─ GStreamer playbin (local file)
```

Why buffer instead of streaming the signed URL:

- signed URLs expire, and a paused track can fail on resume;
- seeking and duration are unreliable on a raw YouTube URL;
- MPRIS position and the lyric overlay need a stable clock;
- a local file keeps signed URLs and headers out of the player and the logs.

The helper runs once per request and exits. It is not a daemon, so idle RAM is unchanged. The
helper is killed as a process group when a search is replaced or a track is skipped.

## Install

```bash
./scripts/setup-youtube.sh
# or, during the normal install:
./install.sh --with-youtube
```

The script creates a private Python environment at `~/.local/lib/myuzik/runtime`, copies the
helper to `~/.local/lib/myuzik/ytmusic.py`, and installs the pinned requirements:

- `ytmusicapi==1.12.3` (MIT)
- `yt-dlp[default]==2026.8.19` (Unlicense)

It also checks for Node.js 20+ (needed by `yt-dlp` for YouTube's JavaScript challenge) and
warns if `ffmpeg` is missing. It never modifies the system Python.

Enable the source in **Settings → Sources → YouTube Music**, or choose **Use YouTube Music
instead** on the first-run setup form. The setting is `youtube_enabled` in
`~/.config/myuzik/config.toml`. Once enabled, the sidebar shows a **Library / YouTube** switch;
the last choice is stored as `active_source` (`navidrome` or `youtube`).

## Use

1. Choose **YouTube** in the sidebar switch. **Home** shows the YouTube Music front page:
   personalised shelves (quick picks, song mixes, community playlists, new releases) and a
   **Moods & genres** item in the menu.
2. Open an album, artist, playlist or mood to browse. Album and playlist pages have **Play all**
   and **Add to queue**; artist pages list albums; mood pages list curated playlists.
3. **Search** follows the active source, so YouTube search runs only in YouTube mode and
   library search runs only in Library mode. YouTube results arrive after a 350 ms debounce.
4. Click a song. Myuzik starts it and queues **YouTube Music's radio / watch playlist** for it —
   the song plus related tracks chosen by YouTube Music. It does **not** queue the search page or
   the album page. This mirrors the real client (AudioTube's `UserPlaylistModel` uses
   `get_watch_playlist`; Sung exposes the same idea as its `radio` op). If radio is unavailable,
   only the selected song is queued.
5. Myuzik buffers the song to `~/.cache/myuzik/streams`, then starts GStreamer on the local file.
   The player shows `Buffering` until the buffer is ready.
6. Cover art, LRCLIB lyrics, the overlay, MPRIS, next/previous and the queue work as usual.
   Scrobbling is skipped for YouTube tracks (there is no Navidrome id to submit). When the radio
   queue runs out, Myuzik fetches another radio batch from the last song if autoplay is on.

Songs longer than 20 minutes are filtered out of search and radio (long mixes and podcasts would
also exceed the 32 MiB per-track buffer cap). The helper deduplicates by video id. Home shelves,
albums and playlists cap at 200 tracks per page.

## Quality and cache

| Setting | Value | Meaning |
|---|---|---|
| `youtube_quality` | `standard` | Best audio-only stream; normally Opus ~130 kbps |
| `youtube_quality` | `saver` | Cap at ~80 kbps, landing on YouTube's ~67 kbps Opus rung |
| `stream_cache_mb` | `1024` | Bound for `~/.cache/myuzik/streams` |

There is no lossless mode; use local files or a music server set to Original for lossless.

The stream cache is an implementation detail, not a library download. It is pruned oldest-first
when the bound is exceeded. **Settings → Sources → Stream cache** clears it.

## Cookies

Some tracks require sign-in (age/region restrictions). Put a Netscape-format cookie file path in
`youtube_cookies`:

```toml
youtube_cookies = "/home/you/.config/myuzik/youtube-cookies.txt"
```

Keep the file at mode `0600`. Myuzik passes the path to `yt-dlp`; it does not read, copy or log
the contents. A file picker and a clear button are planned for M3.

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| "YouTube support is not installed" | Run `./scripts/setup-youtube.sh`; check `~/.local/lib/myuzik/ytmusic.py` and the runtime. |
| "Python 3 was not found" | Install Python 3 and `python3-venv`, then rerun the setup. |
| "Node.js 20+ is required" | Install Node.js 20+; `yt-dlp` uses it for YouTube's JS challenge. |
| "This track isn't available" | Region/copyright block or a removed upload; try another result. |
| "This track is too large to buffer" | The audio exceeds 32 MiB; Myuzik does not fall back to direct streaming. |
| Playback stopped after an upstream change | Update the helper: `~/.local/lib/myuzik/runtime/bin/python -m pip install --upgrade -r ~/.local/lib/myuzik/requirements.txt`. |
| Search works but playback fails | Check `yt-dlp --version` and Node; run the helper manually with `MYUZIK_YT_DEBUG=1` to see stderr. |

Debug the helper directly:

```bash
echo '{"op":"search","query":"lofi","limit":3,"filter":"songs"}' |
  ~/.local/lib/myuzik/runtime/bin/python ~/.local/lib/myuzik/ytmusic.py
```

Signed URLs may appear in debug output; do not paste it into a public issue.

## Scope

The YouTube source provides home shelves, search, album/artist/playlist browsing, moods and
genres, buffered playback, song radio, cover art and lyrics. Deferred:

- preloading the next YouTube track (there is a short gap between tracks);
- deep paging beyond the first artist/playlist shelf;
- paste-a-link;
- cookie import UI;
- additional providers (Audius, Internet Archive).

## Attribution and licenses

The helper protocol and buffer-first design were informed by
[Sung](https://github.com/yappologistic/Sung) (MIT, Copyright (c) 2026 Sung contributors).
[AudioTube](https://invent.kde.org/multimedia/audiotube) (GPL) was studied for behaviour only; no
AudioTube code is included. `ytmusicapi` is MIT and `yt-dlp` is Unlicense. See `NOTICE`.
