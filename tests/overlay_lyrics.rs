//! Pinned-overlay lyrics behaviour, driven through the real `Shell`.
//!
//! The overlay used to draw every line of the song into a 392x180 window, which
//! is what made it unreadable. These tests pin the replacement: a band of at most
//! six rows around the active line, the lyric offset honoured, and the state
//! carried into the window instead of an empty box.
//!
//! Lyrics come from the mocked Navidrome `getLyricsBySongId`, which is the
//! authoritative source and keeps the test off the network (LRCLIB is only
//! consulted when the server has nothing).
//!
//! Run with --test-threads=1 (the harness keeps the player channels per thread).

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

const LYRICS_LINES: usize = 70;
const LINE_SPACING_SECS: u64 = 5;

fn temp_paths(tag: &str) -> Paths {
    let base = std::env::temp_dir().join(format!("myuzik-lyr-{}-{}", std::process::id(), tag));
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

/// N synced lines, `LINE_SPACING_SECS` apart, in the OpenSubsonic shape.
fn open_subsonic_synced(n: usize) -> String {
    let lines: Vec<String> = (0..n)
        .map(|i| {
            format!(
                r#"{{"start":{},"value":"line {i}"}}"#,
                i as u64 * LINE_SPACING_SECS * 1000
            )
        })
        .collect();
    ok(&format!(
        r#""lyricsList":{{"structuredLyrics":[{{"synced":true,"lang":"en","line":[{}]}}]}}"#,
        lines.join(",")
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
        duration: Duration::from_secs(LYRICS_LINES as u64 * LINE_SPACING_SECS),
        track_number: Some(1),
        disc_number: Some(1),
        year: Some(2026),
        starred: false,
        genre: None,
    }
}

fn shell_with(
    server: &MockServer,
    tag: &str,
    cx: &mut TestAppContext,
) -> (gpui::Entity<Shell>, VisualTestContext) {
    server.mock(|when, then| {
        when.method(GET)
            .path("/rest/getAlbumList2.view")
            .query_param("type", "newest");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""albumList2":{"album":[{"id":"a1","name":"Alpha","artist":"Artist A","songCount":2,"duration":300,"year":2026}]}"#));
    });

    let config = AppConfig {
        server_url: server.base_url(),
        username: "tester".to_string(),
        // Keep this test focused on lyrics: no radio fetching in the background.
        autoplay: false,
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

/// Play the seed track and wait for its lyrics to reach the overlay.
fn play_with_lyrics(
    server: &MockServer,
    tag: &str,
    cx: &mut TestAppContext,
) -> (gpui::Entity<Shell>, VisualTestContext) {
    server.mock(|when, then| {
        when.method(GET).path("/rest/getLyricsBySongId.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(open_subsonic_synced(LYRICS_LINES));
    });
    let (shell, mut vcx) = shell_with(server, tag, cx);
    shell.update(&mut vcx, |s, cx| s.play_track(seed_track(), cx));
    let loaded = wait_until(&mut vcx, &shell, |s, cx| s.overlay_line_count(cx) > 0);
    assert!(loaded, "the overlay never received lyrics");
    (shell, vcx)
}

#[gpui::test]
fn overlay_draws_a_band_not_the_whole_song(cx: &mut TestAppContext) {
    let server = MockServer::start();
    let (shell, vcx) = play_with_lyrics(&server, "band", cx);

    assert_eq!(
        shell.read_with(&vcx, |s, cx| s.overlay_line_count(cx)),
        LYRICS_LINES,
        "all lyrics are kept in state"
    );

    // Mid-song: the overlay must draw a small band, never the 70 lines.
    send_event(PlayerEvent::Duration(Duration::from_secs(
        LYRICS_LINES as u64 * LINE_SPACING_SECS,
    )));
    send_event(PlayerEvent::Position(Duration::from_secs(300)));
    vcx.run_until_parked();

    let rows = shell.read_with(&vcx, |s, cx| s.overlay_visible_rows(cx));
    assert!(
        rows <= 7,
        "the overlay drew {rows} rows; it must stay within the karaoke band"
    );
    assert!(rows >= 3, "the band should still show context, got {rows}");

    // At 300 s with 5 s lines, line 60 is the one playing.
    assert_eq!(
        shell.read_with(&vcx, |s, cx| s.overlay_active(cx)),
        Some(60)
    );
    assert_eq!(
        shell
            .read_with(&vcx, |s, cx| s.overlay_first_line(cx))
            .as_deref(),
        Some("line 57"),
        "the band should start three lines before the active one"
    );
}

#[gpui::test]
fn lyric_offset_shifts_the_overlay(cx: &mut TestAppContext) {
    let server = MockServer::start();
    let (shell, mut vcx) = play_with_lyrics(&server, "offset", cx);

    send_event(PlayerEvent::Position(Duration::from_secs(300)));
    vcx.run_until_parked();
    assert_eq!(
        shell.read_with(&vcx, |s, cx| s.overlay_active(cx)),
        Some(60)
    );

    // Move the lyrics one line earlier; the same playback position must now
    // highlight the previous line. This is what used to be ignored entirely.
    shell.update(&mut vcx, |s, cx| s.adjust_lyrics_offset(-5_000, cx));
    send_event(PlayerEvent::Position(Duration::from_secs(300)));
    vcx.run_until_parked();
    assert_eq!(
        shell.read_with(&vcx, |s, cx| s.overlay_active(cx)),
        Some(59),
        "the offset was not applied to the pinned overlay"
    );
}

#[gpui::test]
fn overlay_shows_a_message_when_lyrics_are_missing(cx: &mut TestAppContext) {
    let server = MockServer::start();
    // The server has no lyrics for this track.
    server.mock(|when, then| {
        when.method(GET).path("/rest/getLyricsBySongId.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""lyricsList":{"structuredLyrics":[]}"#));
    });
    let (shell, mut vcx) = shell_with(&server, "message", cx);
    shell.update(&mut vcx, |s, cx| s.play_track(seed_track(), cx));
    vcx.run_until_parked();

    // The pin must say something straight away instead of showing an empty box
    // (or, worse, the previous track's lyrics).
    assert_eq!(shell.read_with(&vcx, |s, cx| s.overlay_line_count(cx)), 0);
    let message = shell
        .read_with(&vcx, |s, cx| s.overlay_message(cx))
        .unwrap_or_default();
    // Which message it is depends on how fast the fetch resolved; what matters
    // is that the overlay explains itself instead of drawing an empty box.
    assert!(
        message.contains("Loading") || message.contains("No lyrics"),
        "the overlay should explain its state, got {message:?}"
    );
    assert_eq!(
        shell.read_with(&vcx, |s, cx| s.overlay_visible_rows(cx)),
        0,
        "a message state draws no lyric rows"
    );
}
