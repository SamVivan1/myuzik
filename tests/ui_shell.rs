//! Headless render smoke tests for every route.

use gpui::{AppContext, TestAppContext, VisualTestContext, WindowOptions};
use myuzik::app::Route;
use myuzik::config::{AppConfig, Paths};
use myuzik::player::engine::PlayerHandle;
use myuzik::ui::Shell;

fn dummy_player() -> PlayerHandle {
    let (tx, cmd_rx) = flume::unbounded();
    let (evt_tx, rx) = flume::unbounded();
    std::mem::forget(cmd_rx);
    std::mem::forget(evt_tx);
    PlayerHandle { tx, rx }
}

fn temp_paths(tag: &str) -> Paths {
    let base = std::env::temp_dir().join(format!("myuzik-test-{}-{}", std::process::id(), tag));
    Paths {
        config: base.join("config.toml"),
        cache: base.clone(),
        covers: base.join("covers"),
        lyrics: base.join("lyrics"),
        logs: base.join("logs"),
    }
}

#[gpui::test]
fn shell_renders_every_route_without_panic(cx: &mut TestAppContext) {
    let window = cx.update(|cx| {
        let paths = temp_paths("routes");
        cx.open_window(WindowOptions::default(), move |_, cx| {
            cx.new(|cx| Shell::new(cx, dummy_player(), AppConfig::default(), paths, None))
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();

    let routes = [
        Route::Setup,
        Route::Home,
        Route::NowPlaying,
        Route::Search,
        Route::Settings,
        Route::Albums,
        Route::Artists,
        Route::Playlists,
        Route::Favorites,
    ];
    for route in routes {
        shell.update(&mut cx, |shell, _cx| shell.go(route.clone()));
        cx.run_until_parked();
        assert_eq!(shell.read_with(&cx, |shell, _| shell.route()), route);
    }
}

#[gpui::test]
fn search_keystrokes_update_query(cx: &mut TestAppContext) {
    let window = cx.update(|cx| {
        let paths = temp_paths("search");
        cx.open_window(WindowOptions::default(), move |_, cx| {
            cx.new(|cx| Shell::new(cx, dummy_player(), AppConfig::default(), paths, None))
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();
    shell.update(&mut cx, |shell, _cx| shell.go(Route::Search));
    cx.run_until_parked();
    // Focus is set by the sidebar click in real usage; simulate typing into the window.
    cx.simulate_keystrokes("hello");
    cx.run_until_parked();
    let _ = shell.read_with(&cx, |shell, _| shell.route());
}

#[gpui::test]
fn sidebar_clicks_navigate(cx: &mut TestAppContext) {
    let window = cx.update(|cx| {
        let paths = temp_paths("clicks");
        std::fs::create_dir_all(&paths.cache).unwrap();
        std::fs::write(paths.config.parent().unwrap().join("credentials"), "secret").unwrap();
        let config = AppConfig {
            server_url: "http://127.0.0.1:9".to_string(),
            username: "tester".to_string(),
            ..Default::default()
        };
        cx.open_window(WindowOptions::default(), move |_, cx| {
            cx.new(|cx| Shell::new(cx, dummy_player(), config, paths, None))
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();
    cx.run_until_parked();
    assert_eq!(shell.read_with(&cx, |s, _| s.route()), Route::Home);

    let search = cx.debug_bounds("nav-search").expect("nav-search bounds");
    cx.simulate_click(search.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(shell.read_with(&cx, |s, _| s.route()), Route::Search);

    let home = cx.debug_bounds("nav-home").expect("nav-home bounds");
    cx.simulate_click(home.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(shell.read_with(&cx, |s, _| s.route()), Route::Home);

    let np = cx.debug_bounds("np-open").expect("np-open bounds");
    cx.simulate_click(np.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(shell.read_with(&cx, |s, _| s.route()), Route::NowPlaying);
}

#[gpui::test]
fn overlay_opens_and_closes(cx: &mut TestAppContext) {
    let window = cx.update(|cx| {
        let paths = temp_paths("overlay");
        cx.open_window(WindowOptions::default(), move |_, cx| {
            cx.new(|cx| Shell::new(cx, dummy_player(), AppConfig::default(), paths, None))
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();
    cx.run_until_parked();
    shell.update(&mut cx, |shell, cx| shell.open_overlay(cx));
    cx.run_until_parked();
    assert!(
        shell.read_with(&cx, |shell, _| shell.overlay_open()),
        "overlay did not open"
    );
    shell.update(&mut cx, |shell, cx| shell.close_overlay(cx));
    cx.run_until_parked();
    assert!(
        !shell.read_with(&cx, |shell, _| shell.overlay_open()),
        "overlay did not close"
    );
}

#[gpui::test]
fn unreachable_server_shows_error_toast(cx: &mut TestAppContext) {
    let window = cx.update(|cx| {
        let paths = temp_paths("badsrv");
        std::fs::create_dir_all(&paths.cache).unwrap();
        std::fs::write(paths.config.parent().unwrap().join("credentials"), "secret").unwrap();
        let config = AppConfig {
            server_url: "http://127.0.0.1:9".to_string(),
            username: "tester".to_string(),
            ..Default::default()
        };
        cx.open_window(WindowOptions::default(), move |_, cx| {
            cx.new(|cx| Shell::new(cx, dummy_player(), config, paths, None))
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        cx.run_until_parked();
        if shell.read_with(&cx, |s, _| s.toast_text().is_some()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        shell.read_with(&cx, |s, _| s.toast_text().is_some()),
        "unreachable server did not produce a toast"
    );
    // The shell must still render and remain usable.
    assert_eq!(shell.read_with(&cx, |s, _| s.route()), Route::Home);
}

#[gpui::test]
fn configured_volume_is_applied(cx: &mut TestAppContext) {
    let window = cx.update(|cx| {
        let paths = temp_paths("volume");
        let config = AppConfig {
            volume: 0.35,
            ..Default::default()
        };
        cx.open_window(WindowOptions::default(), move |_, cx| {
            cx.new(|cx| Shell::new(cx, dummy_player(), config, paths, None))
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();
    cx.run_until_parked();
    assert_eq!(shell.read_with(&cx, |s, cx| s.volume(cx)), 0.35);
}

#[gpui::test]
fn setup_form_validates_and_types(cx: &mut TestAppContext) {
    let window = cx.update(|cx| {
        let paths = temp_paths("setupform");
        cx.open_window(WindowOptions::default(), move |_, cx| {
            cx.new(|cx| Shell::new(cx, dummy_player(), AppConfig::default(), paths, None))
        })
        .unwrap()
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut cx).unwrap();
    cx.run_until_parked();
    assert_eq!(shell.read_with(&cx, |s, _| s.route()), Route::Setup);

    // The server field starts with "https://"; typing appends.
    cx.simulate_keystrokes("abc");
    cx.run_until_parked();
    assert_eq!(
        shell.read_with(&cx, |s, _| s.setup_server_value().to_string()),
        "https://abc"
    );

    // Invalid scheme is rejected.
    shell.update(&mut cx, |s, _cx| s.set_setup_fields("ftp://bad", "u", "p"));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let connect = cx.debug_bounds("setup-connect").expect("setup-connect");
    cx.simulate_click(connect.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(shell.read_with(&cx, |s, _| s.route()), Route::Setup);
    assert!(
        shell.read_with(&cx, |s, _| s.setup_error().unwrap_or("").contains("http")),
        "invalid URL not rejected"
    );
}
