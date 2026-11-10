//! Full UI flow against a mocked Navidrome: home loads -> click album -> click track -> plays.
//! Run with --test-threads=1.

use gpui::{AppContext, TestAppContext, VisualTestContext, WindowOptions};
use httpmock::prelude::*;
use myuzik::config::{AppConfig, Paths};
use myuzik::player::engine::PlayerHandle;
use myuzik::ui::Shell;
use std::time::{Duration, Instant};

fn temp_paths(tag: &str) -> Paths {
    let base = std::env::temp_dir().join(format!("myuzik-flow-{}-{}", std::process::id(), tag));
    Paths {
        config: base.join("config.toml"),
        cache: base.clone(),
        covers: base.join("covers"),
        lyrics: base.join("lyrics"),
        logs: base.join("logs"),
    }
}

#[gpui::test]
fn home_album_track_play_flow(cx: &mut TestAppContext) {
    let server = MockServer::start();
    let ok = |body: &str| {
        format!(r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1",{body}}}}}"#)
    };
    let albums = ok(
        r#""albumList2":{"album":[{"id":"a1","name":"Alpha","artist":"Artist A","songCount":2,"duration":300,"year":2026},{"id":"a2","name":"Beta","artist":"Artist B","songCount":1,"duration":200,"year":2025}]}"#,
    );
    let _m1 = server.mock(|when, then| {
        when.method(GET)
            .path("/rest/getAlbumList2.view")
            .query_param("type", "newest");
        then.status(200)
            .header("content-type", "application/json")
            .body(&albums);
    });
    let _m2 = server.mock(|when, then| {
        when.method(GET).path("/rest/getAlbumList2.view").query_param("type", "starred");
        then.status(200).header("content-type", "application/json").body(ok(r#""albumList2":{"album":[{"id":"a1","name":"Alpha","artist":"Artist A","songCount":2,"duration":300,"year":2026}]}"#));
    });
    let _m3 = server.mock(|when, then| {
        when.method(GET).path("/rest/getArtists.view");
        then.status(200).header("content-type", "application/json").body(ok(r#""artists":{"index":[{"artist":[{"id":"ar1","name":"Artist A","albumCount":1}]}]}"#));
    });
    let _m4 = server.mock(|when, then| {
        when.method(GET).path("/rest/getPlaylists.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""playlists":{}"#));
    });
    let _m5 = server.mock(|when, then| {
        when.method(GET).path("/rest/getAlbum.view").query_param("id", "a1");
        then.status(200).header("content-type", "application/json").body(ok(r#""album":{"id":"a1","name":"Alpha","artist":"Artist A","songCount":2,"duration":300,"year":2026,"song":[{"id":"s1","title":"First","artist":"Artist A","album":"Alpha","duration":120},{"id":"s2","title":"Second","artist":"Artist A","album":"Alpha","duration":180}]}"#));
    });
    let _m6 = server.mock(|when, then| {
        when.method(GET).path("/rest/getLyricsBySongId.view");
        then.status(200).header("content-type", "application/json").body(ok(r#""lyricsList":{"structuredLyrics":[{"synced":true,"line":[{"start":1000,"value":"one"},{"start":2000,"value":"two"}]}]}"#));
    });
    let now_playing = server.mock(|when, then| {
        when.method(GET)
            .path("/rest/scrobble.view")
            .query_param("submission", "false");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(""));
    });
    let submitted = server.mock(|when, then| {
        when.method(GET)
            .path("/rest/scrobble.view")
            .query_param("submission", "true");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(""));
    });

    let base = server.base_url();
    let window = cx.update(move |cx| {
        let paths = temp_paths("flow");
        std::fs::create_dir_all(&paths.cache).unwrap();
        std::fs::write(paths.config.parent().unwrap().join("credentials"), "secret").unwrap();
        let config = AppConfig {
            server_url: base,
            username: "tester".to_string(),
            ..Default::default()
        };
        cx.open_window(WindowOptions::default(), move |_, cx| {
            let (tx, cmd_rx) = flume::unbounded();
            let (evt_tx, rx) = flume::unbounded();
            let shell = cx.new(|cx| Shell::new(cx, PlayerHandle { tx, rx }, config, paths, None));
            TEST_CMD_RX.with(|c| *c.borrow_mut() = Some(cmd_rx));
            TEST_EVT_TX.with(|c| *c.borrow_mut() = Some(evt_tx));
            shell
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        cx.run_until_parked();
        if shell.read_with(&cx, |s, cx| s.loaded_album_count(cx)) == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        shell.read_with(&cx, |s, cx| s.loaded_album_count(cx)),
        2,
        "albums did not load"
    );
    assert_eq!(
        shell.read_with(&cx, |s, cx| s.favorites_count(cx)),
        1,
        "starred albums did not load"
    );

    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let album = cx.debug_bounds("album-a1").expect("album card bounds");
    cx.simulate_click(album.center(), gpui::Modifiers::default());
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        cx.run_until_parked();
        if shell.read_with(&cx, |s, _| s.current_album_song_count()) == Some(2) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        shell.read_with(&cx, |s, _| s.current_album_song_count()),
        Some(2),
        "album detail did not load"
    );
    assert_eq!(
        shell.read_with(&cx, |s, _| s.selected_album_id().map(str::to_string)),
        Some("a1".into())
    );

    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let track = cx.debug_bounds("track-0").expect("track bounds");
    cx.simulate_click(track.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        shell.read_with(&cx, |s, cx| s.current_track_title(cx)),
        Some("First".into())
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        cx.run_until_parked();
        if shell.read_with(&cx, |s, cx| s.overlay_line_count(cx)) == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        shell.read_with(&cx, |s, cx| s.overlay_line_count(cx)),
        2,
        "lyrics did not reach overlay"
    );
    send_event(myuzik::player::engine::PlayerEvent::Position(
        Duration::from_secs(2),
    ));
    cx.run_until_parked();
    assert_eq!(
        shell.read_with(&cx, |s, cx| s.overlay_active(cx)),
        Some(1),
        "overlay active line wrong"
    );

    let cmds = drain_cmds();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, myuzik::player::engine::PlayerCommand::Load { .. })),
        "Load command not sent"
    );
    assert!(
        cmds.iter()
            .any(|c| matches!(c, myuzik::player::engine::PlayerCommand::Play)),
        "Play command not sent"
    );

    // Simulate engine events for scrobbling.
    send_event(myuzik::player::engine::PlayerEvent::TrackStarted(
        "s1".into(),
    ));
    send_event(myuzik::player::engine::PlayerEvent::Duration(
        Duration::from_secs(120),
    ));
    for sec in 1..=70u64 {
        send_event(myuzik::player::engine::PlayerEvent::Position(
            Duration::from_secs(sec),
        ));
        cx.run_until_parked();
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && submitted.calls() == 0 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(now_playing.calls() >= 1, "now-playing scrobble not sent");
    assert!(submitted.calls() >= 1, "submitted scrobble not sent");

    // Volume controls.
    let before = shell.read_with(&cx, |s, cx| s.volume(cx));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let up = cx.debug_bounds("vol-up").expect("vol-up");
    cx.simulate_click(up.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let after = shell.read_with(&cx, |s, cx| s.volume(cx));
    assert!(
        after > before,
        "volume did not increase ({before} -> {after})"
    );
    assert!(
        drain_cmds()
            .iter()
            .any(|c| matches!(c, myuzik::player::engine::PlayerCommand::SetVolume(_))),
        "SetVolume not sent"
    );

    // Seek buttons use the current playhead.
    send_event(myuzik::player::engine::PlayerEvent::Position(
        Duration::from_secs(30),
    ));
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert_eq!(
        shell.read_with(&cx, |s, cx| s.playhead(cx)),
        Duration::from_secs(30)
    );
    let fwd = cx.debug_bounds("seek-fwd").expect("seek-fwd");
    cx.simulate_click(fwd.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    send_event(myuzik::player::engine::PlayerEvent::Error(
        myuzik::player::engine::PlayerError::Message("decoder failed".into()),
    ));
    cx.run_until_parked();
    assert!(
        shell
            .read_with(&cx, |s, _| s.toast_text().map(str::to_string))
            .is_some(),
        "player error did not produce a toast"
    );
    let seeks: Vec<_> = drain_cmds();
    assert!(seeks.iter().any(|c| matches!(c, myuzik::player::engine::PlayerCommand::Seek(d) if *d == Duration::from_secs(40))), "seek +10 not sent: {seeks:?}");
}

fn drain_cmds() -> Vec<myuzik::player::engine::PlayerCommand> {
    let mut out = Vec::new();
    TEST_CMD_RX.with(|c| {
        if let Some(rx) = c.borrow().as_ref() {
            while let Ok(cmd) = rx.try_recv() {
                out.push(cmd);
            }
        }
    });
    out
}

fn send_event(ev: myuzik::player::engine::PlayerEvent) {
    TEST_EVT_TX.with(|c| {
        if let Some(tx) = c.borrow().as_ref() {
            let _ = tx.send(ev);
        }
    });
}

thread_local! {
    static TEST_CMD_RX: std::cell::RefCell<Option<flume::Receiver<myuzik::player::engine::PlayerCommand>>> =
        const { std::cell::RefCell::new(None) };
    static TEST_EVT_TX: std::cell::RefCell<Option<flume::Sender<myuzik::player::engine::PlayerEvent>>> =
        const { std::cell::RefCell::new(None) };
}

#[gpui::test]
fn search_flow_returns_results(cx: &mut TestAppContext) {
    let server = MockServer::start();
    let ok = |body: &str| {
        format!(r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1",{body}}}}}"#)
    };
    let _m1 = server.mock(|when, then| {
        when.method(GET).path("/rest/getAlbumList2.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""albumList2":{"album":[]}"#));
    });
    let _m2 = server.mock(|when, then| {
        when.method(GET).path("/rest/getArtists.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""artists":{"index":[]}"#));
    });
    let _m3 = server.mock(|when, then| {
        when.method(GET).path("/rest/getPlaylists.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""playlists":{}"#));
    });
    let _m4 = server.mock(|when, then| {
        when.method(GET).path("/rest/search3.view").query_param("query", "alpha");
        then.status(200).header("content-type", "application/json").body(ok(r#""searchResult3":{"song":[{"id":"s1","title":"Alpha Song","artist":"A","album":"Alpha","duration":100}],"album":[],"artist":[]}"#));
    });
    let _m5 = server.mock(|when, then| {
        when.method(GET).path("/rest/getLyricsBySongId.view");
        then.status(404);
    });

    let base = server.base_url();
    let window = cx.update(move |cx| {
        let paths = temp_paths("searchflow");
        std::fs::create_dir_all(&paths.cache).unwrap();
        std::fs::write(paths.config.parent().unwrap().join("credentials"), "secret").unwrap();
        let config = AppConfig {
            server_url: base,
            username: "tester".to_string(),
            ..Default::default()
        };
        cx.open_window(WindowOptions::default(), move |_, cx| {
            let (tx, cmd_rx) = flume::unbounded();
            let (evt_tx, rx) = flume::unbounded();
            std::mem::forget(evt_tx);
            std::mem::forget(cmd_rx);
            cx.new(|cx| Shell::new(cx, PlayerHandle { tx, rx }, config, paths, None))
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();
    cx.run_until_parked();

    let search = cx.debug_bounds("nav-search").expect("nav-search");
    cx.simulate_click(search.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_keystrokes("alpha");
    cx.run_until_parked();
    assert_eq!(
        shell.read_with(&cx, |s, _| s.search_query().to_string()),
        "alpha"
    );
    cx.simulate_keystrokes("enter");

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        cx.run_until_parked();
        if shell.read_with(&cx, |s, _| s.search_result_count()) > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        shell.read_with(&cx, |s, _| s.search_result_count()),
        1,
        "search returned no results"
    );
}

#[gpui::test]
fn playlist_click_plays_tracks(cx: &mut TestAppContext) {
    let server = MockServer::start();
    let ok = |body: &str| {
        format!(r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1",{body}}}}}"#)
    };
    let _m1 = server.mock(|when, then| {
        when.method(GET).path("/rest/getAlbumList2.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""albumList2":{"album":[]}"#));
    });
    let _m2 = server.mock(|when, then| {
        when.method(GET).path("/rest/getArtists.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""artists":{"index":[]}"#));
    });
    let _m3 = server.mock(|when, then| {
        when.method(GET).path("/rest/getPlaylists.view");
        then.status(200).header("content-type", "application/json").body(ok(r#""playlists":{"playlist":[{"id":"pl1","name":"Mix","songCount":2,"duration":300,"owner":"tester"}]}"#));
    });
    let _m4 = server.mock(|when, then| {
        when.method(GET).path("/rest/getPlaylist.view").query_param("id", "pl1");
        then.status(200).header("content-type", "application/json").body(ok(r#""playlist":{"id":"pl1","name":"Mix","songCount":2,"duration":300,"owner":"tester"},"entry":[{"id":"p1","title":"PL One","artist":"A","album":"Mix","duration":100},{"id":"p2","title":"PL Two","artist":"A","album":"Mix","duration":200}]"#));
    });
    let _m5 = server.mock(|when, then| {
        when.method(GET).path("/rest/getLyricsBySongId.view");
        then.status(404);
    });

    let base = server.base_url();
    let window = cx.update(move |cx| {
        let paths = temp_paths("playlistflow");
        std::fs::create_dir_all(&paths.cache).unwrap();
        std::fs::write(paths.config.parent().unwrap().join("credentials"), "secret").unwrap();
        let config = AppConfig {
            server_url: base,
            username: "tester".to_string(),
            ..Default::default()
        };
        cx.open_window(WindowOptions::default(), move |_, cx| {
            let (tx, cmd_rx) = flume::unbounded();
            let (evt_tx, rx) = flume::unbounded();
            std::mem::forget(evt_tx);
            let shell = cx.new(|cx| Shell::new(cx, PlayerHandle { tx, rx }, config, paths, None));
            TEST_CMD_RX.with(|c| *c.borrow_mut() = Some(cmd_rx));
            shell
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();

    // Wait for playlists to load.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        cx.run_until_parked();
        if shell.read_with(&cx, |s, cx| s.playlist_count(cx)) == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let playlists = cx.debug_bounds("nav-playlists").expect("nav-playlists");
    cx.simulate_click(playlists.center(), gpui::Modifiers::default());
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let row = cx.debug_bounds("playlist-pl1").expect("playlist row");
    cx.simulate_click(row.center(), gpui::Modifiers::default());

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        cx.run_until_parked();
        if shell
            .read_with(&cx, |s, cx| s.current_track_title(cx))
            .as_deref()
            == Some("PL One")
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        shell.read_with(&cx, |s, cx| s.current_track_title(cx)),
        Some("PL One".into())
    );
    assert!(
        drain_cmds()
            .iter()
            .any(|c| matches!(c, myuzik::player::engine::PlayerCommand::Play)),
        "play not sent"
    );
}

#[gpui::test]
fn artist_click_shows_albums(cx: &mut TestAppContext) {
    let server = MockServer::start();
    let ok = |body: &str| {
        format!(r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1",{body}}}}}"#)
    };
    let _m1 = server.mock(|when, then| {
        when.method(GET).path("/rest/getAlbumList2.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""albumList2":{"album":[]}"#));
    });
    let _m2 = server.mock(|when, then| {
        when.method(GET).path("/rest/getArtists.view");
        then.status(200).header("content-type", "application/json").body(ok(r#""artists":{"index":[{"artist":[{"id":"ar1","name":"Artist A","albumCount":1}]}]}"#));
    });
    let _m3 = server.mock(|when, then| {
        when.method(GET).path("/rest/getPlaylists.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""playlists":{}"#));
    });
    let _m4 = server.mock(|when, then| {
        when.method(GET).path("/rest/getArtist.view").query_param("id", "ar1");
        then.status(200).header("content-type", "application/json").body(ok(r#""artist":{"id":"ar1","name":"Artist A","albumCount":1},"album":[{"id":"a1","name":"Alpha","artist":"Artist A","songCount":1,"duration":100,"year":2026}]"#));
    });
    let _m5 = server.mock(|when, then| {
        when.method(GET).path("/rest/getAlbum.view").query_param("id", "a1");
        then.status(200).header("content-type", "application/json").body(ok(r#""album":{"id":"a1","name":"Alpha","artist":"Artist A","songCount":1,"duration":100,"year":2026,"song":[{"id":"s1","title":"Only","artist":"Artist A","album":"Alpha","duration":100}]}"#));
    });
    let _m6 = server.mock(|when, then| {
        when.method(GET).path("/rest/getLyricsBySongId.view");
        then.status(404);
    });

    let base = server.base_url();
    let window = cx.update(move |cx| {
        let paths = temp_paths("artistflow");
        std::fs::create_dir_all(&paths.cache).unwrap();
        std::fs::write(paths.config.parent().unwrap().join("credentials"), "secret").unwrap();
        let config = AppConfig {
            server_url: base,
            username: "tester".to_string(),
            ..Default::default()
        };
        cx.open_window(WindowOptions::default(), move |_, cx| {
            let (tx, _crx) = flume::unbounded();
            let (evt_tx, rx) = flume::unbounded();
            std::mem::forget(evt_tx);
            std::mem::forget(_crx);
            cx.new(|cx| Shell::new(cx, PlayerHandle { tx, rx }, config, paths, None))
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        cx.run_until_parked();
        if shell.read_with(&cx, |s, cx| s.loaded_artist_count(cx)) == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let artists = cx.debug_bounds("nav-artists").expect("nav-artists");
    cx.simulate_click(artists.center(), gpui::Modifiers::default());
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let artist = cx.debug_bounds("artist-ar1").expect("artist row");
    cx.simulate_click(artist.center(), gpui::Modifiers::default());
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        cx.run_until_parked();
        if shell.read_with(&cx, |s, _| s.selected_artist_album_count()) == Some(1) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        shell.read_with(&cx, |s, _| s.selected_artist_album_count()),
        Some(1)
    );

    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let album = cx.debug_bounds("artist-album-a1").expect("artist album");
    cx.simulate_click(album.center(), gpui::Modifiers::default());
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        cx.run_until_parked();
        if shell.read_with(&cx, |s, _| s.current_album_song_count()) == Some(1) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        shell.read_with(&cx, |s, _| s.current_album_song_count()),
        Some(1)
    );
}
