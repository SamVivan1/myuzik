//! End-to-end autoplay: playing one track keeps going into similar music.
//!
//! Drives the real `Shell` against a mocked Navidrome, so the queue extension,
//! the async fetch and the engine `Load` command are all exercised together.
//! Run with --test-threads=1 (the harness keeps the player channels in
//! thread-local storage).

use gpui::{AppContext, TestAppContext, VisualTestContext, WindowOptions};
use httpmock::prelude::*;
use myuzik::config::{AppConfig, Paths};
use myuzik::library::Track;
use myuzik::player::engine::{PlayerCommand, PlayerEvent, PlayerHandle};
use myuzik::ui::Shell;
use std::time::{Duration, Instant};

thread_local! {
    static TEST_CMD_RX: std::cell::RefCell<Option<flume::Receiver<PlayerCommand>>> =
        const { std::cell::RefCell::new(None) };
    static TEST_EVT_TX: std::cell::RefCell<Option<flume::Sender<PlayerEvent>>> =
        const { std::cell::RefCell::new(None) };
}

fn temp_paths(tag: &str) -> Paths {
    let base = std::env::temp_dir().join(format!("myuzik-autoplay-{}-{}", std::process::id(), tag));
    Paths {
        config: base.join("config.toml"),
        cache: base.clone(),
        covers: base.join("covers"),
        lyrics: base.join("lyrics"),
        logs: base.join("logs"),
    }
}

fn ok(body: &str) -> String {
    format!(r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1",{body}}}}}"#)
}

fn song(id: &str) -> String {
    format!(
        r#"{{"id":"{id}","title":"Song {id}","artist":"Artist","artistId":"ar1","album":"Album","albumId":"al1","genre":"Indie Pop","duration":180}}"#
    )
}

fn similar(ids: &[&str]) -> String {
    let list: Vec<String> = ids.iter().map(|id| song(id)).collect();
    ok(&format!(
        r#""similarSongs":{{"song":[{}]}}"#,
        list.join(",")
    ))
}

fn seed_track() -> Track {
    Track {
        id: "seed".to_string(),
        title: "Seed Song".to_string(),
        artist: "Artist".to_string(),
        artist_id: Some("ar1".to_string()),
        album: "Album".to_string(),
        album_id: Some("al1".to_string()),
        cover_art_id: None,
        duration: Duration::from_secs(180),
        track_number: Some(1),
        disc_number: Some(1),
        year: Some(2026),
        starred: false,
        genre: Some("Indie Pop".to_string()),
    }
}

/// Home-load mocks so the shell has a working client and library.
fn stub_library(server: &MockServer) {
    server.mock(|when, then| {
        when.method(GET)
            .path("/rest/getAlbumList2.view")
            .query_param("type", "newest");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""albumList2":{"album":[{"id":"a1","name":"Alpha","artist":"Artist A","songCount":2,"duration":300,"year":2026}]}"#));
    });
}

/// Build a shell whose player channels the test controls.
fn shell_with_mock(
    cx: &mut TestAppContext,
    server: &MockServer,
    tag: &str,
) -> (gpui::Entity<Shell>, VisualTestContext) {
    let config = AppConfig {
        server_url: server.base_url(),
        username: "tester".to_string(),
        ..Default::default()
    };
    let paths = temp_paths(tag);
    std::fs::create_dir_all(&paths.cache).unwrap();
    std::fs::write(paths.config.parent().unwrap().join("credentials"), "secret").unwrap();

    let window = cx.update(|cx| {
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
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut vcx).unwrap();
    (shell, vcx)
}

fn send_event(ev: PlayerEvent) {
    TEST_EVT_TX.with(|c| {
        if let Some(tx) = c.borrow().as_ref() {
            let _ = tx.send(ev);
        }
    });
}

/// Pump the loop until `pred` holds or the deadline passes. The radio fetch runs
/// on the background executor, so this needs real wall-clock time.
fn wait_until(
    vcx: &mut VisualTestContext,
    shell: &gpui::Entity<Shell>,
    mut pred: impl FnMut(&Shell, &gpui::App) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        vcx.run_until_parked();
        if shell.read_with(vcx, |s, cx| pred(s, cx)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[gpui::test]
fn playing_one_track_queues_similar_music_and_continues(cx: &mut TestAppContext) {
    let server = MockServer::start();
    stub_library(&server);
    let similar_mock = server.mock(|when, then| {
        when.method(GET).path("/rest/getSimilarSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(similar(&["next-1", "next-2", "next-3"]));
    });

    let (shell, mut vcx) = shell_with_mock(cx, &server, "continue");

    // The user plays exactly one song.
    shell.update(&mut vcx, |s, cx| s.play_track(seed_track(), cx));
    // Checked before the event loop runs, so this is the state the click made.
    assert_eq!(
        shell.read_with(&vcx, |s, cx| s.queue_len(cx)),
        1,
        "playing one track queues exactly that one track"
    );

    // Autoplay tops the queue up without waiting for the song to end.
    let grew = wait_until(&mut vcx, &shell, |s, cx| s.queue_len(cx) > 1);
    assert!(
        grew,
        "autoplay did not queue similar tracks after a single track started"
    );
    assert_eq!(
        similar_mock.calls(),
        1,
        "the radio should fetch once per batch"
    );

    // The end of the seed track continues into the similar music instead of stopping.
    send_event(PlayerEvent::Eos);
    vcx.run_until_parked();
    let advanced = wait_until(&mut vcx, &shell, |s, cx| {
        s.current_track_id(cx).as_deref() != Some("seed")
    });
    assert!(
        advanced,
        "playback did not continue into the queued similar tracks"
    );

    let current = shell.read_with(&vcx, |s, cx| s.current_track_id(cx));
    assert!(
        matches!(current.as_deref(), Some("next-1" | "next-2" | "next-3")),
        "continued with an unexpected track: {current:?}"
    );
}

#[gpui::test]
fn autoplay_off_stops_at_the_end_of_the_queue(cx: &mut TestAppContext) {
    let server = MockServer::start();
    stub_library(&server);
    let similar_mock = server.mock(|when, then| {
        when.method(GET).path("/rest/getSimilarSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(similar(&["next-1"]));
    });

    let (shell, mut vcx) = shell_with_mock(cx, &server, "off");

    // Turn the feature off the way a user would.
    shell.update(&mut vcx, |s, _cx| s.toggle_autoplay());
    assert!(!shell.read_with(&vcx, |s, _| s.autoplay_on()));

    shell.update(&mut vcx, |s, cx| s.play_track(seed_track(), cx));
    vcx.run_until_parked();
    std::thread::sleep(Duration::from_millis(250));
    vcx.run_until_parked();

    assert_eq!(
        shell.read_with(&vcx, |s, cx| s.queue_len(cx)),
        1,
        "nothing may be queued while autoplay is off"
    );
    assert_eq!(similar_mock.calls(), 0, "the radio must not even ask");

    send_event(PlayerEvent::Eos);
    vcx.run_until_parked();
    assert_eq!(
        shell
            .read_with(&vcx, |s, cx| s.current_track_id(cx))
            .as_deref(),
        Some("seed"),
        "playback should stay on the finished track, not move on"
    );
}

#[gpui::test]
fn a_failing_radio_shows_one_toast_and_leaves_the_queue_alone(cx: &mut TestAppContext) {
    let server = MockServer::start();
    stub_library(&server);
    // Every radio source is broken.
    for path in [
        "/rest/getSimilarSongs.view",
        "/rest/getSimilarSongs2.view",
        "/rest/getSongsByGenre.view",
        "/rest/getRandomSongs.view",
    ] {
        server.mock(|when, then| {
            when.method(GET).path(path);
            then.status(500);
        });
    }

    let (shell, mut vcx) = shell_with_mock(cx, &server, "fail");
    shell.update(&mut vcx, |s, cx| s.play_track(seed_track(), cx));

    let toasted = wait_until(&mut vcx, &shell, |s, _| s.toast_text().is_some());
    assert!(toasted, "a broken radio should surface a single toast");
    assert_eq!(
        shell.read_with(&vcx, |s, cx| s.queue_len(cx)),
        1,
        "a failed fetch must not change the queue"
    );
}
