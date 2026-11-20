#!/usr/bin/env python3
"""Deterministic helper for Myuzik's offline tests.

This is not shipped. It speaks the same JSON protocol as helper/ytmusic.py but
uses only the standard library, so the normal test suite never needs Python
packages, Node, or network access.
"""

import json
import os
import sys
import time
import wave
from pathlib import Path


def song(video_id: str, title: str) -> dict:
    return {
        "videoId": video_id,
        "title": title,
        "artist": "Fixture Artist",
        "album": "Fixture Album",
        "seconds": 120,
        "art": "https://example.com/fixture.jpg",
    }


def main() -> int:
    request = json.load(sys.stdin)
    if os.environ.get("MYUZIK_YT_REQUEST_LOG"):
        with open(os.environ["MYUZIK_YT_REQUEST_LOG"], "a", encoding="utf-8") as log:
            log.write(json.dumps(request) + "\n")

    op = request.get("op")
    if op == "version":
        return print(json.dumps({"ok": True, "ytmusicapi": "test", "yt_dlp": "test", "python": sys.version.split()[0]}))
    if op == "search":
        query = request.get("query", "")
        if query == "slow":
            time.sleep(2)
        if query == "error":
            return print(json.dumps({"ok": False, "error": "fixture offline"}))
        if query == "empty":
            return print(json.dumps({"ok": True, "items": []}))
        items = [song("abcdefghijk", "Fixture One"), song("lmnopqrstuv", "Fixture Two")]
        return print(json.dumps({"ok": True, "items": items}))
    if op == "radio":
        items = [
            song("radiotrack1", "Radio One"),
            song("radiotrack2", "Radio Two"),
        ]
        return print(json.dumps({"ok": True, "items": items}))
    if op == "home":
        return print(json.dumps({"ok": True, "sections": [
            {"title": "Quick picks", "items": [
                {"kind": "song", "id": "abcdefghijk", "title": "Fixture One", "artist": "Fixture Artist", "album": "Fixture Album", "seconds": 120, "art": "https://example.com/fixture.jpg"},
                {"kind": "song", "id": "lmnopqrstuv", "title": "Fixture Two", "artist": "Fixture Artist", "album": "Fixture Album", "seconds": 120, "art": "https://example.com/fixture.jpg"},
                {"kind": "song", "id": "picksong003", "title": "Fixture Three", "artist": "Fixture Artist", "album": "Fixture Album", "seconds": 120, "art": "https://example.com/fixture.jpg"},
                {"kind": "song", "id": "picksong004", "title": "Fixture Four", "artist": "Fixture Artist", "album": "Fixture Album", "seconds": 120, "art": "https://example.com/fixture.jpg"},
                {"kind": "song", "id": "picksong005", "title": "Fixture Five", "artist": "Fixture Artist", "album": "Fixture Album", "seconds": 120, "art": "https://example.com/fixture.jpg"},
                {"kind": "song", "id": "picksong006", "title": "Fixture Six", "artist": "Fixture Artist", "album": "Fixture Album", "seconds": 120, "art": "https://example.com/fixture.jpg"},
                {"kind": "song", "id": "picksong007", "title": "Fixture Seven", "artist": "Fixture Artist", "album": "Fixture Album", "seconds": 120, "art": "https://example.com/fixture.jpg"},
                {"kind": "song", "id": "picksong008", "title": "Fixture 8", "artist": "Fixture Artist", "album": "Fixture Album", "seconds": 120, "art": "https://example.com/fixture.jpg"},
                {"kind": "song", "id": "picksong009", "title": "Fixture 9", "artist": "Fixture Artist", "album": "Fixture Album", "seconds": 120, "art": "https://example.com/fixture.jpg"},
                {"kind": "song", "id": "picksong010", "title": "Fixture 10", "artist": "Fixture Artist", "album": "Fixture Album", "seconds": 120, "art": "https://example.com/fixture.jpg"},
                {"kind": "song", "id": "picksong011", "title": "Fixture 11", "artist": "Fixture Artist", "album": "Fixture Album", "seconds": 120, "art": "https://example.com/fixture.jpg"},
                {"kind": "song", "id": "picksong012", "title": "Fixture 12", "artist": "Fixture Artist", "album": "Fixture Album", "seconds": 120, "art": "https://example.com/fixture.jpg"},
            ]},
            {"title": "New releases", "items": [
                {"kind": "album", "id": "MPREb_fixture", "title": "Fixture Album", "artist": "Fixture Artist", "year": "2026", "art": "https://example.com/fixture.jpg"},
                {"kind": "playlist", "id": "PLfixture", "title": "Fixture Playlist", "artist": "Fixture Owner", "count": 3, "art": "https://example.com/fixture.jpg"},
                {"kind": "artist", "id": "UCfixture", "title": "Fixture Artist", "art": "https://example.com/fixture.jpg"},
            ]},
        ]}))
    if op == "album":
        return print(json.dumps({"ok": True, "id": "MPREb_fixture", "title": "Fixture Album", "artist": "Fixture Artist", "artistId": "UCfixture", "year": "2026", "art": "https://example.com/fixture.jpg", "seconds": 360, "items": [song("abcdefghijk", "Fixture One"), song("lmnopqrstuv", "Fixture Two")]}))
    if op == "artist":
        return print(json.dumps({"ok": True, "id": "UCfixture", "name": "Fixture Artist", "description": "Fixture biography", "art": "https://example.com/fixture.jpg", "songs": [], "albums": [{"kind": "album", "id": "MPREb_fixture", "title": "Fixture Album", "artist": "Fixture Artist", "year": "2026", "art": "https://example.com/fixture.jpg"}], "singles": [], "playlists": []}))
    if op == "playlist":
        return print(json.dumps({"ok": True, "id": "PLfixture", "title": "Fixture Playlist", "owner": "Fixture Owner", "art": "https://example.com/fixture.jpg", "seconds": 360, "count": 3, "items": [song("abcdefghijk", "Fixture One"), song("lmnopqrstuv", "Fixture Two"), song("zyxwvutsrqp", "Fixture Three")]}))
    if op == "moods":
        return print(json.dumps({"ok": True, "categories": [{"title": "Moods & moments", "items": [{"title": "Chill", "params": "fixture-params"}]}]}))
    if op == "mood":
        return print(json.dumps({"ok": True, "items": [{"kind": "playlist", "id": "PLfixture", "title": "Fixture Playlist", "artist": "Fixture Owner", "count": 3, "art": "https://example.com/fixture.jpg"}]}))
    if op == "buffer":
        video_id = request.get("id", "")
        if video_id == "failfailfai":
            return print(json.dumps({"ok": False, "error": "fixture buffer failure"}))
        if video_id == "outsidefile":
            return print(json.dumps({"ok": True, "file": "/etc/passwd", "seconds": 120}))
        directory = Path(request["directory"])
        directory.mkdir(parents=True, exist_ok=True)
        path = directory / (video_id + ".wav")
        with wave.open(str(path), "wb") as audio:
            audio.setnchannels(1)
            audio.setsampwidth(2)
            audio.setframerate(8000)
            audio.writeframes(b"\0" * 2 * 8000)
        return print(json.dumps({"ok": True, "file": str(path), "seconds": 120}))
    return print(json.dumps({"ok": False, "error": "unknown fixture request"}))


if __name__ == "__main__":
    sys.exit(main())
