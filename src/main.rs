use myuzik::config;
use myuzik::platform::mpris::start_mpris;
use myuzik::player::engine::spawn_player;
use myuzik::ui::Shell;

use gpui::{
    Bounds, WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowOptions, size, px, prelude::*,
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
        cx.open_window(opts, |_window, cx| {
            cx.new(|cx| Shell::new(cx, player_handle, config, paths, mpris_handle))
        })
        .expect("failed to open window");
    });
}
