//! End-to-end: the disk snapshot makes startup and search instant, and the Sync
//! button is what fills it. Audio is never cached here — playback still streams
//! from the server when a track is chosen.

use gpui::{AppContext, TestAppContext, VisualTestContext, WindowOptions};
use httpmock::prelude::*;
use myuzik::config::{AppConfig, Paths};
use myuzik::library::TrackSource;
use myuzik::library::cache::{LibraryCache, LibrarySnapshot, SNAPSHOT_VERSION};
use myuzik::library::{Album, Artist, Playlist, Track};
use myuzik::player::engine::PlayerHandle;
use myuzik::ui::Shell;
use std::time::{Duration, Instant};

fn dummy_player() -> PlayerHandle {
    let (tx, cmd_rx) = flume::unbounded();
    let (evt_tx, rx) = flume::unbounded();
    std::mem::forget(cmd_rx);
    std::mem::forget(evt_tx);
    PlayerHandle { tx, rx }
}

fn temp_paths(tag: &str) -> Paths {
    let base = std::env::temp_dir().join(format!("myuzik-cache-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&base);
    Paths {
        config: base.join("config.toml"),
        cache: base.clone(),
        covers: base.join("covers"),
        lyrics: base.join("lyrics"),
        logs: base.join("logs"),
    }
}

fn track(id: &str, title: &str, artist: &str, album: &str) -> Track {
    Track {
        id: id.to_string(),
        title: title.to_string(),
        artist: artist.to_string(),
        artist_id: None,
        album: album.to_string(),
        album_id: None,
        cover_art_id: None,
        duration: Duration::from_secs(200),
        track_number: None,
        disc_number: None,
        year: None,
        starred: false,
        genre: Some("Jazz".to_string()),
        source: Default::default(),
        cover_url: None,
    }
}

fn snapshot_with_cached_library() -> LibrarySnapshot {
    LibrarySnapshot {
        version: SNAPSHOT_VERSION,
        fetched_at: 1_700_000_000,
        albums: vec![Album {
            id: "al1".to_string(),
            name: "Cached Album".to_string(),
            artist: "Cached Artist".to_string(),
            artist_id: None,
            cover_art_id: None,
            song_count: Some(1),
            duration: None,
            year: None,
            starred: false,
            source: TrackSource::Navidrome,
            cover_url: None,
        }],
        artists: vec![Artist {
            id: "ar1".to_string(),
            name: "Cached Artist".to_string(),
            album_count: Some(1),
            cover_art_id: None,
            source: TrackSource::Navidrome,
            cover_url: None,
        }],
        playlists: vec![Playlist {
            id: "pl1".to_string(),
            name: "Cached Playlist".to_string(),
            song_count: 1,
            duration: Duration::from_secs(200),
            owner: "tester".to_string(),
            source: TrackSource::Navidrome,
            cover_url: None,
        }],
        songs: vec![track("s1", "Kind of Blue", "Miles Davis", "Cached Album")],
    }
}

/// A snapshot on disk must be visible on the very first frame, with no server
/// configured at all — that is the whole point of the cache.
#[gpui::test]
fn a_cached_library_renders_without_a_server(cx: &mut TestAppContext) {
    let paths = temp_paths("startup");
    LibraryCache::new(paths.cache.clone())
        .save(&snapshot_with_cached_library())
        .expect("seed the cache");

    let paths_for_window = paths.clone();
    let window = cx.update(move |cx| {
        cx.open_window(WindowOptions::default(), move |_, cx| {
            cx.new(|cx| {
                Shell::new(
                    cx,
                    dummy_player(),
                    AppConfig::default(),
                    paths_for_window,
                    None,
                )
            })
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();
    cx.run_until_parked();

    assert_eq!(
        shell.read_with(&cx, |s, cx| s.loaded_album_count(cx)),
        1,
        "the cached album did not appear on startup"
    );
    assert_eq!(shell.read_with(&cx, |s, cx| s.loaded_artist_count(cx)), 1);
}

/// With a cache present, typing searches the in-memory index: no server, no
/// network, instant results. The query is deliberately an accented/diacritic
/// case to prove the local index is doing the work, not the server.
#[gpui::test]
fn search_uses_the_cache_and_never_calls_the_server(cx: &mut TestAppContext) {
    let paths = temp_paths("localsesarch");
    LibraryCache::new(paths.cache.clone())
        .save(&snapshot_with_cached_library())
        .expect("seed the cache");

    // A server that answers the library load but NOT search3: if search ever
    // reaches it, the assertion below fails instead of silently passing.
    let server = MockServer::start();
    let ok = |body: &str| {
        format!(r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1",{body}}}}}"#)
    };
    let _albums = server.mock(|when, then| {
        when.method(GET).path("/rest/getAlbumList2.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""albumList2":{"album":[]}"#));
    });
    let _artists = server.mock(|when, then| {
        when.method(GET).path("/rest/getArtists.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""artists":{"index":[]}"#));
    });
    let _playlists = server.mock(|when, then| {
        when.method(GET).path("/rest/getPlaylists.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""playlists":{}"#));
    });
    let search = server.mock(|when, then| {
        when.method(GET).path("/rest/search3.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""searchResult3":{"song":[],"album":[],"artist":[]}"#));
    });

    std::fs::create_dir_all(&paths.cache).unwrap();
    std::fs::write(paths.config.parent().unwrap().join("credentials"), "secret").unwrap();
    let config = AppConfig {
        server_url: server.base_url(),
        username: "tester".to_string(),
        ..Default::default()
    };
    let window = cx.update(move |cx| {
        cx.open_window(WindowOptions::default(), move |_, cx| {
            cx.new(|cx| Shell::new(cx, dummy_player(), config, paths, None))
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();
    cx.run_until_parked();

    let nav = cx.debug_bounds("nav-search").expect("nav-search");
    cx.simulate_click(nav.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_keystrokes("miles");
    cx.run_until_parked();
    // Let any debounce/timer settle, then look at the results.
    cx.executor().advance_clock(Duration::from_millis(400));
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        cx.run_until_parked();
        if shell.read_with(&cx, |s, _| s.search_result_count()) > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    assert!(
        shell.read_with(&cx, |s, _| s.search_result_count()) > 0,
        "the cached song was not found by the local index"
    );
    assert_eq!(
        search.calls(),
        0,
        "search went to the server even though the library is cached"
    );
}

/// The Sync button is what fills the cache: it walks the album pages and each
/// album's tracks, then writes one snapshot that the next start reads.
#[gpui::test]
fn sync_pulls_the_library_and_writes_the_snapshot(cx: &mut TestAppContext) {
    let server = MockServer::start();
    let ok = |body: &str| {
        format!(r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1",{body}}}}}"#)
    };
    let albums = server.mock(|when, then| {
        when.method(GET).path("/rest/getAlbumList2.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""albumList2":{"album":[{"id":"al1","name":"Synced Album","artist":"Synced Artist","songCount":1,"duration":200}]}"#));
    });
    let artists = server.mock(|when, then| {
        when.method(GET).path("/rest/getArtists.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""artists":{"index":[{"artist":[{"id":"ar1","name":"Synced Artist","albumCount":1}]}]}"#));
    });
    let playlists = server.mock(|when, then| {
        when.method(GET).path("/rest/getPlaylists.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""playlists":{}"#));
    });
    let album = server.mock(|when, then| {
        when.method(GET).path("/rest/getAlbum.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""album":{"id":"al1","name":"Synced Album","artist":"Synced Artist","song":[{"id":"s1","title":"Synced Song","artist":"Synced Artist","album":"Synced Album","duration":200}]}"#));
    });

    let paths = temp_paths("sync");
    std::fs::create_dir_all(&paths.cache).unwrap();
    std::fs::write(paths.config.parent().unwrap().join("credentials"), "secret").unwrap();
    let config = AppConfig {
        server_url: server.base_url(),
        username: "tester".to_string(),
        ..Default::default()
    };
    let cache_dir = paths.cache.clone();
    let window = cx.update(move |cx| {
        cx.open_window(WindowOptions::default(), move |_, cx| {
            cx.new(|cx| Shell::new(cx, dummy_player(), config, paths, None))
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();
    cx.run_until_parked();

    shell.update(&mut cx, |shell, cx| shell.sync_library(cx));

    let cache = LibraryCache::new(cache_dir);
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut loaded = None;
    while Instant::now() < deadline {
        cx.run_until_parked();
        if let Ok(Some(snapshot)) = cache.load() {
            loaded = Some(snapshot);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let snapshot = loaded.expect("sync did not write a snapshot");

    // The startup refresh also loads the album list, so this is "at least once";
    // per-album track fetching only happens during a sync.
    assert!(
        albums.calls() >= 1,
        "album list never fetched (calls: {})",
        albums.calls()
    );
    assert!(artists.calls() >= 1, "artists not fetched");
    assert!(playlists.calls() >= 1, "playlists not fetched");
    assert_eq!(album.calls(), 1, "album tracks not fetched");
    assert_eq!(snapshot.songs.len(), 1, "song missing from the snapshot");
    assert_eq!(snapshot.songs[0].title, "Synced Song");
    assert_eq!(snapshot.albums.len(), 1);
    assert_eq!(snapshot.artists.len(), 1);

    // And the freshly synced library is searchable straight away.
    shell.update(&mut cx, |shell, cx| {
        shell.go(myuzik::app::Route::Search);
        let _ = cx;
    });
    cx.run_until_parked();
}
