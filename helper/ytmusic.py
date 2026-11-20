#!/usr/bin/env python3
"""YouTube Music helper for Myuzik.

One request per process: one JSON object on stdin, one JSON object on stdout,
then exit. There is no server, no daemon and no idle worker.

The helper is intentionally small. It uses ytmusicapi for the catalog and
yt-dlp for audio resolution, then hands the caller a local file. Myuzik plays
that file with GStreamer, so signed URLs never reach the player or the logs.

This is an unofficial client. YouTube's terms do not grant third-party clients
a right to extract streams, and upstream changes can break this at any time.
"""

from __future__ import annotations

import json
import os
import platform
import re
import sys
from pathlib import Path
from urllib.parse import urlparse

MAX_INPUT = 1024 * 1024
MAX_OUTPUT = 2 * 1024 * 1024
MAX_TRACK_BYTES = 32 * 1024 * 1024
VIDEO_ID = re.compile(r"^[A-Za-z0-9_-]{11}$")
URL_RE = re.compile(r"https?://\S+")


def redact(message: str) -> str:
    """Keep diagnostics useful without printing signed media URLs."""
    return URL_RE.sub("<url>", str(message))


def parse_seconds(value) -> int:
    if isinstance(value, bool):
        return 0
    if isinstance(value, (int, float)):
        return max(0, int(value))
    if not isinstance(value, str):
        return 0
    parts = value.strip().split(":")
    if not parts or len(parts) > 3:
        return 0
    try:
        numbers = [int(part) for part in parts]
    except ValueError:
        return 0
    if len(numbers) == 2:
        return max(0, numbers[0] * 60 + numbers[1])
    if len(numbers) == 3:
        return max(0, numbers[0] * 3600 + numbers[1] * 60 + numbers[2])
    return 0


def artwork(item: dict) -> str:
    thumbs = item.get("thumbnails") or item.get("thumbnail") or []
    if not isinstance(thumbs, list) or not thumbs:
        return ""
    last = thumbs[-1]
    if not isinstance(last, dict):
        return ""
    src = last.get("url") or ""
    if not isinstance(src, str):
        return ""
    # Google serves tiny thumbnails to search clients. Ask the same image
    # service for a 544px square, exactly as Sung does.
    if "googleusercontent.com/" in src or "ggpht.com/" in src:
        src = re.sub(r"=w\d+-h\d+[^?]*$", "=w544-h544-l90-rj", src)
    return src


def normalize_song(item: dict) -> dict | None:
    if not isinstance(item, dict):
        return None
    video = item.get("videoId") or ""
    if not isinstance(video, str) or not VIDEO_ID.fullmatch(video):
        return None
    if item.get("isAvailable") is False:
        return None
    if (
        item.get("isDelete") is True
        or item.get("is_delete") is True
        or item.get("isUnlisted") is True
        or item.get("is_unlisted") is True
    ):
        return None

    artists = item.get("artists") or []
    if isinstance(artists, str):
        artists = [{"name": artists}]
    artist = ", ".join(
        a.get("name", "") for a in artists if isinstance(a, dict) and a.get("name")
    )
    if not artist:
        author = item.get("author")
        if isinstance(author, dict):
            artist = author.get("name", "") or ""
        elif isinstance(author, str):
            artist = author

    album = item.get("album") or {}
    if isinstance(album, str):
        album = {"name": album}
    album_name = album.get("name", "") if isinstance(album, dict) else ""

    seconds = parse_seconds(item.get("duration_seconds"))
    if seconds <= 0:
        seconds = parse_seconds(item.get("duration") or item.get("length"))

    title = item.get("title") or item.get("name") or ""
    if not isinstance(title, str) or not title.strip():
        return None

    return {
        "videoId": video,
        "title": title.strip(),
        "artist": artist.strip(),
        "album": album_name.strip() if isinstance(album_name, str) else "",
        "seconds": seconds,
        "art": artwork(item),
    }


def bounded_requests(api, timeout: int) -> None:
    """Give ytmusicapi's session a hard request timeout.

    ytmusicapi does not expose a timeout parameter on every call, and a hung
    HTTP request would outlive the parent's watchdog only until the process
    group is killed. A session-level timeout makes failures local and quick.
    """
    session = getattr(api, "_session", None)
    if session is None:
        return
    original = session.request

    def request(*args, **kwargs):
        kwargs.setdefault("timeout", timeout)
        return original(*args, **kwargs)

    session.request = request


def audio_format(quality: str, fallback: bool = False) -> str:
    """A yt-dlp format chain.

    Standard prefers Opus/WebM (~130 kbps). Data saver caps the bitrate, which
    lands on YouTube's ~67 kbps Opus rung. A retry swaps the container, so it
    genuinely tries something else rather than repeating the same failure.
    """
    cap = "[abr<=80]" if quality == "saver" else ""
    containers = ("m4a", "webm") if fallback else ("webm", "m4a")
    choices = [f"bestaudio[ext={container}]{cap}" for container in containers]
    choices.append("bestaudio" + cap)
    if cap:
        # Nothing under the cap means a recording YouTube only offers above it.
        choices.append("worstaudio")
    return "/".join(choices)


def op_version(_req: dict) -> dict:
    import yt_dlp
    import ytmusicapi

    return {
        "ytmusicapi": getattr(ytmusicapi, "__version__", "unknown"),
        "yt_dlp": getattr(getattr(yt_dlp, "version", None), "__version__", "unknown"),
        "python": platform.python_version(),
    }


def op_radio(req: dict) -> dict:
    """YouTube Music radio for a seed song.

    This is the behaviour of the real client: opening a song queues the song's
    watch playlist / radio, not the search results that happened to contain it.
    """
    from ytmusicapi import YTMusic

    video_id = req.get("id", "")
    if not isinstance(video_id, str) or not VIDEO_ID.fullmatch(video_id):
        raise ValueError("Invalid YouTube video ID")
    limit = req.get("limit", 30)
    try:
        limit = min(max(int(limit), 1), 50)
    except (TypeError, ValueError):
        limit = 30

    api = YTMusic(requests_session=True)
    bounded_requests(api, 20)
    data = api.get_watch_playlist(videoId=video_id, radio=True, limit=limit)
    tracks = data.get("tracks", []) if isinstance(data, dict) else []
    items = []
    seen = set()
    for item in tracks:
        normalized = normalize_song(item)
        if not normalized or normalized["videoId"] in seen:
            continue
        if normalized["seconds"] > 20 * 60:
            continue
        seen.add(normalized["videoId"])
        items.append(normalized)
    return {"items": items[:limit]}


def op_search(req: dict) -> dict:
    from ytmusicapi import YTMusic

    query = req.get("query")
    if not isinstance(query, str) or not query.strip():
        raise ValueError("Search query is empty")
    limit = req.get("limit", 20)
    try:
        limit = min(max(int(limit), 1), 50)
    except (TypeError, ValueError):
        limit = 20
    requested_filter = req.get("filter") or "songs"
    if requested_filter not in ("songs", "albums", "artists", "playlists", "videos"):
        requested_filter = "songs"

    api = YTMusic(requests_session=True)
    bounded_requests(api, 20)
    results = api.search(query.strip(), filter=requested_filter, limit=limit)
    if not isinstance(results, list):
        return {"items": []}

    items = []
    seen = set()
    for item in results:
        normalized = normalize_song(item)
        if not normalized:
            continue
        if normalized["videoId"] in seen:
            continue
        # M1 is a song player, not a podcast/mix browser. A three-hour upload
        # would also blow past the 32 MiB buffer cap.
        if normalized["seconds"] > 20 * 60:
            continue
        seen.add(normalized["videoId"])
        items.append(normalized)
    return {"items": items[:limit]}


def first_artist(item: dict) -> tuple[str, str]:
    artists = item.get("artists") or []
    if isinstance(artists, str):
        artists = [{"name": artists}]
    if not isinstance(artists, list):
        artists = []
    for a in artists:
        if isinstance(a, dict) and a.get("name"):
            return str(a["name"]), str(a.get("id") or "")
    artist = item.get("artist")
    if isinstance(artist, dict):
        return str(artist.get("name") or ""), str(artist.get("id") or "")
    if isinstance(artist, str):
        return artist, ""
    author = item.get("author")
    if isinstance(author, dict):
        return str(author.get("name") or ""), ""
    if isinstance(author, str):
        return author, ""
    return "", ""


def normalize_card(item: dict) -> dict | None:
    """Turn a ytmusicapi browse/search row into one UI card.

    The UI only needs four kinds: Song, Album, Playlist and Artist. Keeping the
    shape uniform means the home feed, search and mood grids can share a
    renderer without special cases per YouTube shelf type.
    """
    if not isinstance(item, dict):
        return None

    title = item.get("title") or item.get("name") or ""
    if not isinstance(title, str) or not title.strip():
        return None
    title = title.strip()
    art = artwork(item)
    artist, artist_id = first_artist(item)

    video = item.get("videoId") or ""
    if isinstance(video, str) and VIDEO_ID.fullmatch(video) and item.get("isAvailable") is not False:
        seconds = parse_seconds(item.get("duration_seconds"))
        if seconds <= 0:
            seconds = parse_seconds(item.get("duration") or item.get("length"))
        album = item.get("album") or {}
        if isinstance(album, str):
            album = {"name": album}
        return {
            "kind": "song",
            "id": video,
            "title": title,
            "artist": artist,
            "artistId": artist_id,
            "album": str(album.get("name") or "") if isinstance(album, dict) else "",
            "albumId": str(album.get("id") or "") if isinstance(album, dict) else "",
            "seconds": seconds,
            "art": art,
        }

    browse_id = item.get("browseId") or ""
    if isinstance(browse_id, str) and browse_id.startswith("MPRE"):
        return {
            "kind": "album",
            "id": browse_id,
            "title": title,
            "artist": artist,
            "artistId": artist_id,
            "year": str(item.get("year") or ""),
            "art": art,
        }

    playlist_id = item.get("playlistId") or ""
    if not playlist_id and isinstance(browse_id, str) and browse_id.startswith("VL"):
        playlist_id = browse_id[2:]
    if playlist_id:
        return {
            "kind": "playlist",
            "id": str(playlist_id),
            "title": title,
            "artist": artist,
            "art": art,
            "count": item.get("itemCount") or item.get("trackCount") or "",
        }

    if isinstance(browse_id, str) and browse_id.startswith("UC"):
        return {
            "kind": "artist",
            "id": browse_id,
            "title": title,
            "artist": "",
            "art": art,
        }
    return None


def cards(items) -> list[dict]:
    out = []
    seen = set()
    for item in items or []:
        card = normalize_card(item)
        if not card:
            continue
        key = (card["kind"], card["id"])
        if key in seen:
            continue
        seen.add(key)
        out.append(card)
    return out


def duration_seconds(value) -> int:
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return max(0, int(value))
    if not isinstance(value, str):
        return 0
    # ytmusicapi uses "4:36" for tracks and "1 hour, 57 minutes" for playlists.
    if "hour" in value or "minute" in value:
        total = 0
        match = re.search(r"(\d+)\s*hour", value)
        if match:
            total += int(match.group(1)) * 3600
        match = re.search(r"(\d+)\s*minute", value)
        if match:
            total += int(match.group(1)) * 60
        return total
    return parse_seconds(value)


def op_home(req: dict) -> dict:
    from ytmusicapi import YTMusic

    limit = req.get("limit", 6)
    try:
        limit = min(max(int(limit), 1), 12)
    except (TypeError, ValueError):
        limit = 6

    api = YTMusic(requests_session=True)
    bounded_requests(api, 20)
    sections = api.get_home(limit=limit)
    if not isinstance(sections, list):
        return {"sections": []}
    out = []
    for section in sections:
        if not isinstance(section, dict):
            continue
        title = section.get("title") or ""
        if not isinstance(title, str):
            title = ""
        items = cards(section.get("contents"))
        if items:
            out.append({"title": title.strip(), "items": items})
    return {"sections": out}


def op_album(req: dict) -> dict:
    from ytmusicapi import YTMusic

    browse_id = req.get("id", "")
    if not isinstance(browse_id, str) or not browse_id.startswith("MPRE"):
        raise ValueError("Invalid YouTube album id")
    api = YTMusic(requests_session=True)
    bounded_requests(api, 20)
    data = api.get_album(browse_id)
    if not isinstance(data, dict):
        raise ValueError("YouTube returned no album")
    artist, artist_id = first_artist(data)
    tracks = [t for t in (normalize_song(item) for item in data.get("tracks") or []) if t]
    return {
        "id": browse_id,
        "title": str(data.get("title") or ""),
        "artist": artist,
        "artistId": artist_id,
        "year": str(data.get("year") or ""),
        "art": artwork(data),
        "seconds": duration_seconds(data.get("duration")),
        "items": tracks,
    }


def op_artist(req: dict) -> dict:
    from ytmusicapi import YTMusic

    channel_id = req.get("id", "")
    if not isinstance(channel_id, str) or not channel_id.startswith("UC"):
        raise ValueError("Invalid YouTube artist id")
    api = YTMusic(requests_session=True)
    bounded_requests(api, 20)
    data = api.get_artist(channel_id)
    if not isinstance(data, dict):
        raise ValueError("YouTube returned no artist")

    def section(name: str) -> list[dict]:
        sec = data.get(name)
        if not isinstance(sec, dict):
            return []
        return cards(sec.get("results"))

    top = section("songs")
    return {
        "id": channel_id,
        "name": str(data.get("name") or ""),
        "description": str(data.get("description") or ""),
        "art": artwork(data),
        "songs": top,
        "albums": section("albums"),
        "singles": section("singles"),
        "playlists": section("playlists"),
    }


def op_playlist(req: dict) -> dict:
    from ytmusicapi import YTMusic

    playlist_id = req.get("id", "")
    if not isinstance(playlist_id, str) or not playlist_id:
        raise ValueError("Invalid YouTube playlist id")
    limit = req.get("limit", 200)
    try:
        limit = min(max(int(limit), 1), 500)
    except (TypeError, ValueError):
        limit = 200

    api = YTMusic(requests_session=True)
    bounded_requests(api, 20)
    data = api.get_playlist(playlist_id, limit=limit)
    if not isinstance(data, dict):
        raise ValueError("YouTube returned no playlist")
    tracks = [t for t in (normalize_song(item) for item in data.get("tracks") or []) if t]
    owner = data.get("author")
    if isinstance(owner, dict):
        owner = owner.get("name")
    if not isinstance(owner, str):
        owner = ""
    return {
        "id": playlist_id,
        "title": str(data.get("title") or ""),
        "owner": owner.strip(),
        "art": artwork(data),
        "seconds": duration_seconds(data.get("duration")),
        "count": data.get("trackCount") or len(tracks),
        "items": tracks,
    }


def op_moods(_req: dict) -> dict:
    from ytmusicapi import YTMusic

    api = YTMusic(requests_session=True)
    bounded_requests(api, 20)
    data = api.get_mood_categories()
    if not isinstance(data, dict):
        return {"categories": []}
    categories = []
    for title, entries in data.items():
        items = []
        for entry in entries or []:
            if not isinstance(entry, dict):
                continue
            name = entry.get("title") or ""
            params = entry.get("params") or ""
            if isinstance(name, str) and name and isinstance(params, str) and params:
                items.append({"title": name, "params": params})
        if items:
            categories.append({"title": str(title), "items": items})
    return {"categories": categories}


def op_mood(req: dict) -> dict:
    from ytmusicapi import YTMusic

    params = req.get("params", "")
    if not isinstance(params, str) or not params:
        raise ValueError("Missing mood parameters")
    api = YTMusic(requests_session=True)
    bounded_requests(api, 20)
    data = api.get_mood_playlists(params)
    return {"items": cards(data)}


def safe_directory(req: dict) -> Path:
    root = Path(req.get("root", "")).expanduser().resolve()
    directory = Path(req.get("directory", "")).expanduser().resolve()
    root.mkdir(parents=True, exist_ok=True)
    if directory == root or root not in directory.parents:
        raise ValueError("Buffer directory is outside the cache root")
    directory.mkdir(parents=True, exist_ok=True)
    if not directory.is_dir():
        raise ValueError("Buffer path is not a directory")
    return directory


def op_buffer(req: dict) -> dict:
    import yt_dlp

    video_id = req.get("id", "")
    if not isinstance(video_id, str) or not VIDEO_ID.fullmatch(video_id):
        raise ValueError("Invalid YouTube video ID")

    directory = safe_directory(req)
    # This directory belongs to one attempt. Remove stale files from a previous
    # interrupted attempt, but never recurse and never touch the root.
    for child in directory.iterdir():
        if child.is_file() or child.is_symlink():
            child.unlink()

    quality = "saver" if req.get("quality") == "saver" else "standard"
    fallback = bool(req.get("fallback"))
    cookies = req.get("cookies") or ""
    if cookies:
        cookie_path = Path(cookies).expanduser()
        if not cookie_path.is_file():
            raise ValueError("Cookie file does not exist")

    def bound_size(progress):
        if progress.get("downloaded_bytes", 0) > MAX_TRACK_BYTES:
            raise RuntimeError("This track is too large for temporary buffering")

    opts = {
        "quiet": True,
        "noprogress": True,
        "no_warnings": True,
        "noplaylist": True,
        "format": audio_format(quality, fallback),
        "socket_timeout": 18,
        "retries": 2,
        "extractor_retries": 2,
        "cachedir": False,
        "js_runtimes": {"node": {}},
        "skip_download": False,
        "max_filesize": MAX_TRACK_BYTES,
        "outtmpl": str(directory / (video_id + ".%(ext)s")),
        "progress_hooks": [bound_size],
    }
    if cookies:
        opts["cookiefile"] = cookies

    try:
        with yt_dlp.YoutubeDL(opts) as downloader:
            info = downloader.extract_info(
                "https://music.youtube.com/watch?v=" + video_id, download=True
            )
            if not info:
                raise RuntimeError("YouTube returned no track information")
            path = Path(downloader.prepare_filename(info))
    except Exception:
        raise

    if not path.is_file():
        raise RuntimeError("Could not buffer this track")
    size = path.stat().st_size
    if size <= 0:
        path.unlink(missing_ok=True)
        raise RuntimeError("YouTube returned an empty audio file")
    if size > MAX_TRACK_BYTES:
        path.unlink(missing_ok=True)
        raise RuntimeError("This track is too large for temporary buffering")

    return {
        "file": str(path),
        "seconds": parse_seconds(info.get("duration")),
    }


OPS = {
    "version": op_version,
    "search": op_search,
    "radio": op_radio,
    "buffer": op_buffer,
    "home": op_home,
    "album": op_album,
    "artist": op_artist,
    "playlist": op_playlist,
    "moods": op_moods,
    "mood": op_mood,
}


def main() -> int:
    try:
        payload = sys.stdin.read(MAX_INPUT + 1)
        if len(payload) > MAX_INPUT:
            raise ValueError("Request is too large")
        request = json.loads(payload)
        if not isinstance(request, dict):
            raise ValueError("Request is not an object")
        operation = request.get("op", "")
        handler = OPS.get(operation)
        if handler is None:
            raise ValueError("Unknown request")
        data = handler(request)
        output = json.dumps({"ok": True, **data}, ensure_ascii=False)
        if len(output.encode("utf-8")) > MAX_OUTPUT:
            raise ValueError("Response is too large")
        print(output)
        return 0
    except Exception as exc:  # noqa: BLE001 - the protocol boundary is text
        message = redact(str(exc))[-1800:] or exc.__class__.__name__
        print(json.dumps({"ok": False, "error": message}, ensure_ascii=False))
        return 1


if __name__ == "__main__":
    sys.exit(main())
