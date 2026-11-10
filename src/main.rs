use myuzik::config;
use myuzik::platform::mpris::start_mpris;
use myuzik::player::engine::spawn_player;
use myuzik::ui::Shell;

use gpui::{
    Bounds, WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowOptions, prelude::*,
    px, size,
};
use gpui_platform::application;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let paths = config::Paths::new().expect("paths");
    paths.ensure().expect("ensure dirs");
    let cfg = config::load_config(&paths).expect("config");
    tracing::info!(?paths.config, server = %cfg.server_url, "config loaded");

    let app = application();
    app.run(move |cx| {
        let fonts: Vec<std::borrow::Cow<'static, [u8]>> = vec![
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Fraunces.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter.ttf")),
        ];
        if let Err(e) = cx.text_system().add_fonts(fonts) {
            tracing::warn!("failed to load bundled fonts: {e:?}");
        }

        let player_handle = spawn_player();
        let mpris_handle = start_mpris(player_handle.tx.clone()).ok();

        let config = cfg.clone();
        let paths = paths.clone();

        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        let opts = WindowOptions {
            window_background: WindowBackgroundAppearance::Blurred,
            window_decorations: Some(WindowDecorations::Client),
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(960.), px(640.))),
            is_resizable: true,
            app_id: Some("io.github.samvivan1.Myuzik".to_string()),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("Myuzik".into()),
                appears_transparent: false,
                traffic_light_position: None,
            }),
            ..Default::default()
        };
        let handle = cx
            .open_window(opts, |_window, cx| {
                cx.new(|cx| Shell::new(cx, player_handle, config, paths, mpris_handle))
            })
            .expect("failed to open window");

        if let Ok(mode) = std::env::var("MYUZIK_WINDOW_TEST") {
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                match mode.as_str() {
                    "close" => {
                        tracing::info!("window test: closing");
                        let _ = handle.update(cx, |_view, window, _cx| window.remove_window());
                    }
                    "resize" => {
                        let _ = handle.update(cx, |_view, window, _cx| {
                            window.resize(gpui::size(gpui::px(1000.0), gpui::px(700.0)))
                        });
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(700))
                            .await;
                        let size = handle
                            .update(cx, |_view, window, _cx| window.viewport_size())
                            .ok();
                        tracing::info!("window test: resized size={size:?}");
                    }
                    "minimize" => {
                        let _ = handle.update(cx, |_view, window, _cx| window.minimize_window());
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(600))
                            .await;
                        let visibility = handle
                            .update(cx, |_view, window, _cx| window.visibility())
                            .ok();
                        tracing::info!("window test: visibility={visibility:?}");
                    }
                    _ => {
                        let _ = handle.update(cx, |_view, window, _cx| window.zoom_window());
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(700))
                            .await;
                        let maximized = handle
                            .update(cx, |_view, window, _cx| window.is_maximized())
                            .unwrap_or(false);
                        let size = handle
                            .update(cx, |_view, window, _cx| window.viewport_size())
                            .ok();
                        tracing::info!("window test: maximized={maximized} size={size:?}");
                    }
                }
            })
            .detach();
        }
    });
}
