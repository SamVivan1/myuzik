use myuzik::config;
use myuzik::platform::mpris::{MprisHandle, start_mpris};
use myuzik::player::engine::{PlayerCommand, PlayerHandle, spawn_player};
use myuzik::ui::Shell;

use gpui::{
    Window, WindowBackgroundAppearance, WindowDecorations, WindowOptions, prelude::*,
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
    app.run(|cx| {
        let player_handle = spawn_player();
        let mpris_handle = start_mpris(player_handle.tx.clone()).ok();

        let mut opts = WindowOptions::default();
        opts.window_background = WindowBackgroundAppearance::Blurred;
        opts.window_decorations = Some(WindowDecorations::Client);
        cx.open_window(opts, |_window, cx| {
            cx.new(|cx| Shell::new(cx, player_handle, mpris_handle))
        })
        .expect("failed to open window");
    });
}
