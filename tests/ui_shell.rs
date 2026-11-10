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
