use myuzik::config;
use myuzik::platform::mpris::start_mpris;
use myuzik::player::engine::spawn_player;
use myuzik::ui::Shell;

use gpui::{
    WindowBackgroundAppearance, WindowDecorations, WindowOptions, prelude::*,
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
        let player_handle = spawn_player();
        let mpris_handle = start_mpris(player_handle.tx.clone()).ok();

        let config = cfg.clone();
        let paths = paths.clone();

        let opts = WindowOptions {
            window_background: WindowBackgroundAppearance::Blurred,
            window_decorations: Some(WindowDecorations::Client),
            ..Default::default()
        };
        cx.open_window(opts, |_window, cx| {
            cx.new(|cx| Shell::new(cx, player_handle, config, paths, mpris_handle))
        })
        .expect("failed to open window");
    });
}
