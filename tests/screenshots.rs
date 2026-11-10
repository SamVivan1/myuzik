//! Headless UI screenshots using GPUI's wgpu headless renderer.
//! Run with:
//!   MYUZIK_LIVE=1 cargo test --test screenshots -- --ignored --test-threads=1 --nocapture
//! Saves PNGs under docs/screenshots/.

use gpui::{AnyWindowHandle, AppContext, HeadlessAppContext, WindowHandle};
use gpui_wgpu::CosmicTextSystem;
use myuzik::app::Route;
use myuzik::config::{Paths, load_config, load_password};
use myuzik::navidrome::client::NavidromeClient;
use myuzik::player::engine::PlayerHandle;
use myuzik::ui::Shell;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn dummy_player() -> (
    PlayerHandle,
    flume::Sender<myuzik::player::engine::PlayerEvent>,
    flume::Receiver<myuzik::player::engine::PlayerCommand>,
) {
    let (tx, cmd_rx) = flume::unbounded();
    let (evt_tx, rx) = flume::unbounded();
    (PlayerHandle { tx, rx }, evt_tx, cmd_rx)
}

fn wait(
    cx: &mut HeadlessAppContext,
    _shell: &gpui::Entity<Shell>,
    mut f: impl FnMut(&gpui::App) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        cx.run_until_parked();
        if cx.update(|app| f(app)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timeout");
}

fn save(cx: &mut HeadlessAppContext, window: AnyWindowHandle, name: &str) {
    cx.update(|app| app.refresh_windows());
    cx.run_until_parked();
    let _ = cx.update_window(window, |_view, window, cx| {
        let _ = window.draw(cx);
    });
    match cx.capture_screenshot(window) {
        Ok(image) => {
            let image = if image.width() > 1280 {
                image::imageops::resize(&image, 1280, 800, image::imageops::FilterType::Lanczos3)
            } else {
                image
            };
            let path = format!("docs/screenshots/{name}.png");
            image.save(&path).expect("save");
            eprintln!("wrote {path} ({}x{})", image.width(), image.height());
        }
        Err(e) => eprintln!("capture {name} failed: {e:?}"),
    }
}

#[test]
#[ignore = "screenshots need MYUZIK_LIVE=1 and a GPU/renderer"]
fn capture_ui() {
    if std::env::var("MYUZIK_LIVE").ok().as_deref() != Some("1") {
        eprintln!("skipped: set MYUZIK_LIVE=1");
        return;
    }
    let paths = Paths::new().unwrap();
    let config = load_config(&paths).unwrap();
    let Some(pw) = load_password(&paths) else {
        eprintln!("skipped: no credentials");
        return;
    };
    // Prime the client so the first frame already has a Home route.
    let client = NavidromeClient::new(&config.server_url, &config.username, &pw);
    if client.is_err() {
        eprintln!("skipped: invalid client");
        return;
    }

    let text_system = Arc::new(CosmicTextSystem::new("sans-serif"));
    let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(()), || {
        gpui_platform::current_headless_renderer()
    });
    cx.update(|app| app.set_reduce_motion(true));
    let (player, evt_tx, _cmd_rx) = dummy_player();
    let window: WindowHandle<Shell> = cx
        .open_window(
            gpui::size(gpui::px(1280.0), gpui::px(800.0)),
            move |_, cx| cx.new(move |cx| Shell::new(cx, player, config, paths, None)),
        )
        .expect("open window");
    let any: AnyWindowHandle = window.into();
    let shell = window.root(&mut cx).expect("root");

    wait(&mut cx, &shell, |app| {
        shell.read(app).loaded_album_count(app) > 0
    });
    wait(&mut cx, &shell, |app| {
        shell.read(app).loaded_cover_count(app) > 0
    });
    eprintln!(
        "albums at capture: {}, covers: {}",
        cx.update(|app| shell.read(app).loaded_album_count(app)),
        cx.update(|app| shell.read(app).loaded_cover_count(app))
    );
    save(&mut cx, any, "home");

    // Album detail: click the first card if a selector exists.
    // (Album card ids derive from the server, so we only capture Home/Settings/Now Playing.)

    cx.update(|app| shell.update(app, |s, _cx| s.go(Route::Settings)));
    save(&mut cx, any, "settings");

    cx.update(|app| shell.update(app, |s, _cx| s.go(Route::Search)));
    save(&mut cx, any, "search");

    cx.update(|app| shell.update(app, |s, _cx| s.go(Route::NowPlaying)));
    // Drive a fake playback state so Now Playing shows metadata and lyrics.
    let _ = evt_tx.send(myuzik::player::engine::PlayerEvent::TrackStarted(
        "fake".into(),
    ));
    let _ = evt_tx.send(myuzik::player::engine::PlayerEvent::Duration(
        Duration::from_secs(214),
    ));
    let _ = evt_tx.send(myuzik::player::engine::PlayerEvent::Position(
        Duration::from_secs(64),
    ));
    cx.run_until_parked();
    save(&mut cx, any, "now-playing");

    cx.update(|app| shell.update(app, |s, _cx| s.go(Route::Favorites)));
    save(&mut cx, any, "favorites");

    cx.update(|app| shell.update(app, |s, _cx| s.go(Route::Artists)));
    save(&mut cx, any, "artists");
}
