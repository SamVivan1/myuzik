use myuzik::config;
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
    app.run(|cx| {
        let mut opts = WindowOptions::default();
        opts.window_background = WindowBackgroundAppearance::Blurred;
        opts.window_decorations = Some(WindowDecorations::Client);
        cx.open_window(opts, |_window, cx| {
            cx.new(|_cx| Shell::new())
        })
        .expect("failed to open window");
    });
}
