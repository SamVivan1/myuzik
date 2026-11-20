//! End-to-end YouTube flow with the standard-library fixture helper.
//!
//! No Navidrome, no Python packages, no network. The real `Shell` is driven
//! headlessly and the player channel is inspected to prove the buffer was handed
//! to GStreamer as a local file.

use gpui::{AppContext, TestAppContext, VisualTestContext, WindowOptions};
use myuzik::app::Route;
use myuzik::config::{AppConfig, Paths};
use myuzik::player::engine::{PlayerCommand, PlayerHandle};
use myuzik::ui::Shell;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

thread_local! {
    static COMMANDS: RefCell<Option<flume::Receiver<PlayerCommand>>> = const { RefCell::new(None) };
}

fn temp_paths(tag: &str) -> Paths {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "myuzik-youtube-flow-{tag}-{}-{nanos}",
        std::process::id()
    ));
    Paths {
        config: base.join("config.toml"),
        cache: base.clone(),
        covers: base.join("covers"),
        lyrics: base.join("lyrics"),
        logs: base.join("logs"),
    }
}

fn fixture_helper() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ytmusic_fixture.py")
}

#[gpui::test]
fn youtube_search_and_play_without_navidrome(cx: &mut TestAppContext) {
    // Integration tests run serially (`--test-threads=1`), so process-global
    // environment is safe here and avoids exposing test-only constructor paths
    // through the Shell.
    unsafe {
        std::env::set_var("MYUZIK_YT_HELPER", fixture_helper());
        std::env::set_var("MYUZIK_YT_PYTHON", "python3");
    }

    let paths = temp_paths("flow");
    let cache_root = paths.cache.join("streams");
    let window = cx.update(move |cx| {
        std::fs::create_dir_all(&paths.cache).expect("cache");
        let config = AppConfig {
            youtube_enabled: true,
            ..Default::default()
        };
        cx.open_window(WindowOptions::default(), move |_, cx| {
            let (tx, cmd_rx) = flume::unbounded();
            let (evt_tx, rx) = flume::unbounded();
            std::mem::forget(evt_tx);
            COMMANDS.with(|cell| *cell.borrow_mut() = Some(cmd_rx));
            cx.new(|cx| Shell::new(cx, PlayerHandle { tx, rx }, config, paths, None))
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();
    cx.run_until_parked();

    assert_eq!(
        shell.read_with(&cx, |shell, _| shell.route()),
        Route::Home,
        "a YouTube-only install should open on its home feed"
    );
    assert_eq!(
        shell.read_with(&cx, |shell, _| shell.active_source()),
        myuzik::library::TrackSource::YouTube,
        "the saved Navidrome default must fall back to YouTube when it is the only source"
    );

    // Click the Search nav item: it also focuses the search field, which is
    // what makes the simulated keystrokes land in the right place.
    let search = cx.debug_bounds("nav-search").expect("nav-search");
    cx.simulate_click(search.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_keystrokes("lofi");
    cx.executor().advance_clock(Duration::from_millis(500));

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        cx.run_until_parked();
        if shell.read_with(&cx, |shell, _| shell.search_result_count()) == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        shell.read_with(&cx, |shell, _| shell.search_result_count()),
        2,
        "YouTube results did not arrive"
    );

    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let commands = COMMANDS.with(|cell| cell.borrow_mut().take().expect("commands"));
    let row = cx
        .debug_bounds("search-youtube-0")
        .expect("YouTube result row");
    cx.simulate_click(row.center(), gpui::Modifiers::default());

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut load_uri = None;
    while Instant::now() < deadline {
        cx.run_until_parked();
        while let Ok(command) = commands.try_recv() {
            if let PlayerCommand::Load { uri, .. } = command {
                load_uri = Some(uri);
                break;
            }
        }
        if load_uri.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let uri = load_uri.expect("the buffered file was never loaded");
    assert!(
        uri.starts_with("file://"),
        "YouTube must not hand a signed URL to the player: {uri}"
    );
    let path = PathBuf::from(uri.trim_start_matches("file://"));
    assert!(path.is_file(), "buffered file is missing: {path:?}");
    assert!(
        path.starts_with(&cache_root),
        "buffered file escaped the cache: {path:?}"
    );

    // One search result plus the two fixture radio tracks. The search results
    // themselves must not be queued.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        cx.run_until_parked();
        if shell.read_with(&cx, |shell, cx| shell.queue_len(cx)) == 3 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        shell.read_with(&cx, |shell, cx| shell.queue_len(cx)),
        3,
        "clicking a YouTube result should queue its radio, not the search page"
    );
    let mut queue = shell.read_with(&cx, |shell, cx| shell.queue_ids(cx));
    queue.sort();
    assert_eq!(
        queue,
        vec![
            "yt:abcdefghijk".to_string(),
            "yt:radiotrack1".to_string(),
            "yt:radiotrack2".to_string()
        ],
        "the queue must be the seed plus its radio tracks, not the search results"
    );
    assert_eq!(
        shell.read_with(&cx, |shell, _| shell.search_result_count()),
        2,
        "the search results should stay search results"
    );
}

fn youtube_only_window(
    cx: &mut TestAppContext,
    tag: &str,
) -> (
    gpui::WindowHandle<Shell>,
    gpui::Entity<Shell>,
    VisualTestContext,
) {
    unsafe {
        std::env::set_var("MYUZIK_YT_HELPER", fixture_helper());
        std::env::set_var("MYUZIK_YT_PYTHON", "python3");
    }
    let paths = temp_paths(tag);
    let window = cx.update(move |cx| {
        std::fs::create_dir_all(&paths.cache).expect("cache");
        let config = AppConfig {
            youtube_enabled: true,
            ..Default::default()
        };
        cx.open_window(WindowOptions::default(), move |_, cx| {
            let (tx, _cmd_rx) = flume::unbounded();
            let (evt_tx, rx) = flume::unbounded();
            std::mem::forget(_cmd_rx);
            std::mem::forget(evt_tx);
            cx.new(|cx| Shell::new(cx, PlayerHandle { tx, rx }, config, paths, None))
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();
    (window, shell, cx)
}

fn wait_for(
    cx: VisualTestContext,
    shell: &gpui::Entity<Shell>,
    selector: &'static str,
) -> VisualTestContext {
    let mut cx = cx;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        if cx.debug_bounds(selector).is_some() {
            return cx;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let route = shell.read_with(&cx, |shell, _| shell.route());
    panic!("{selector} never appeared (route: {route:?})");
}

#[gpui::test]
fn youtube_home_browses_album_playlist_and_moods(cx: &mut TestAppContext) {
    let (_window, shell, mut cx) = youtube_only_window(cx, "browse");

    assert_eq!(
        shell.read_with(&cx, |shell, _| shell.route()),
        Route::Home,
        "a YouTube-only install opens on Home"
    );

    cx = wait_for(cx, &shell, "yt-card-1-0");

    let album = cx.debug_bounds("yt-card-1-0").expect("album card");
    cx.simulate_click(album.center(), gpui::Modifiers::default());
    cx.run_until_parked();

    let route = shell.read_with(&cx, |shell, _| shell.route());
    assert!(
        matches!(route, Route::Album(_)),
        "clicking an album card should open the album, got {route:?}"
    );
    cx = wait_for(cx, &shell, "detail-play-all");

    let play = cx.debug_bounds("detail-play-all").expect("play album");
    cx.simulate_click(play.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        shell.read_with(&cx, |shell, cx| shell.queue_track_ids(cx)),
        vec!["yt:abcdefghijk".to_string(), "yt:lmnopqrstuv".to_string()],
        "Play all should queue the album tracks in order"
    );

    let add = cx.debug_bounds("detail-add-1").expect("add to queue");
    cx.simulate_click(add.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        shell
            .read_with(&cx, |shell, cx| shell.queue_track_ids(cx))
            .len(),
        3,
        "Add to queue should append the chosen track"
    );
    let next = cx.debug_bounds("detail-next-1").expect("play next");
    cx.simulate_click(next.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        shell.read_with(&cx, |shell, cx| shell.queue_track_ids(cx))[1],
        "yt:lmnopqrstuv".to_string(),
        "Play next should insert immediately after the current track"
    );

    let back = cx.debug_bounds("detail-back").expect("back link");
    cx.simulate_click(back.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    cx = wait_for(cx, &shell, "yt-card-1-1");

    let playlist = cx.debug_bounds("yt-card-1-1").expect("playlist card");
    cx.simulate_click(playlist.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let route = shell.read_with(&cx, |shell, _| shell.route());
    assert!(
        matches!(route, Route::Playlist(_)),
        "clicking a playlist card should open the playlist, got {route:?}"
    );
    cx = wait_for(cx, &shell, "playlist-play-all");
    let play = cx.debug_bounds("playlist-play-all").expect("play playlist");
    cx.simulate_click(play.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        shell
            .read_with(&cx, |shell, cx| shell.queue_track_ids(cx))
            .len(),
        3,
        "the playlist's three fixture tracks should be queued"
    );

    shell.update(&mut cx, |shell, cx| shell.open_moods(cx));
    cx.run_until_parked();
    cx = wait_for(cx, &shell, "mood-0");
    let mood = cx.debug_bounds("mood-0").expect("mood chip");
    cx.simulate_click(mood.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    cx = wait_for(cx, &shell, "mood-playlist-0");
    let playlist = cx.debug_bounds("mood-playlist-0").expect("mood playlist");
    cx.simulate_click(playlist.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let route = shell.read_with(&cx, |shell, _| shell.route());
    assert!(
        matches!(route, Route::Playlist(_)),
        "a mood playlist should open its playlist page, got {route:?}"
    );
}
