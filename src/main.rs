use myuzik::config;
use myuzik::platform::mpris::start_mpris;
use myuzik::platform::tray::{
    ShowAction, Tray, TrayCommand, report_registration, show_action, tray_visibility,
};
use myuzik::player::engine::{PlayerCommand, spawn_player};
use myuzik::ui::Shell;

use gpui::{
    AnyWindowHandle, Bounds, WindowBackgroundAppearance, WindowBounds, WindowDecorations,
    WindowOptions, prelude::*, px, size,
};
use gpui_platform::application;

/// Options for the main window, shared by the first open and by the tray reopening it.
fn main_window_options(bounds: Bounds<gpui::Pixels>) -> WindowOptions {
    WindowOptions {
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
    }
}

/// Whether the *main* window is still open.
///
/// INT-02: the tray must not follow `app.windows()`, because the lyrics overlay
/// is a window on this same `App`. It follows the main window's handle instead.
fn main_window_alive(
    app: &gpui::App,
    main_window: &std::sync::Mutex<Option<AnyWindowHandle>>,
) -> bool {
    let handle = *main_window
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    handle.is_some_and(|handle| app.windows().contains(&handle))
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let paths = config::Paths::new().expect("paths");
    paths.ensure().expect("ensure dirs");
    // A corrupt or unreadable config must not stop the app from starting: fall back
    // to defaults and say so. `expect` here meant a typo in the file was a crash with
    // no way in.
    let cfg = match config::load_config(&paths) {
        Ok(cfg) => cfg,
        Err(error) => {
            tracing::warn!(%error, "config could not be read, using defaults");
            config::AppConfig::default()
        }
    };
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
        let tray_player_tx = player_handle.tx.clone();
        let mpris_handle = start_mpris(player_handle.tx.clone(), cfg.volume).ok();

        let config = cfg.clone();
        let paths = paths.clone();

        // Closing the window must not end the process: with close-to-tray the app
        // keeps playing and only the tray icon remains.
        cx.set_quit_mode(gpui::QuitMode::Explicit);

        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        let opts = WindowOptions {
            window_background: WindowBackgroundAppearance::Transparent,
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
        // The shell lives at app scope, not inside the window, so destroying the
        // window keeps the queue, the player and MPRIS alive and reopening it
        // restores the same view.
        let shell = cx.new(|cx| Shell::new(cx, player_handle, config, paths, mpris_handle));
        let handle = cx
            .open_window(opts, {
                let shell = shell.clone();
                move |_window, _cx| shell.clone()
            })
            .expect("failed to open window");
        let bounds_for_tray = bounds;
        // INT-02: the tray asks "is the main window there?", not "how many
        // windows are there?". The lyrics overlay is a window too, and counting
        // windows made the icon `Passive` — invisible — while the main window was
        // closed with the overlay open, which left the app with no way back.
        let main_window: std::sync::Arc<std::sync::Mutex<Option<AnyWindowHandle>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Some(AnyWindowHandle::from(handle))));

        // System tray: keeps the app reachable when the window is closed
        // (close-to-tray) and gives playback controls from the top bar.
        let tray_icon =
            include_bytes!("../packaging/icons/hicolor/32x32/apps/io.github.samvivan1.Myuzik.png");
        match image::load_from_memory(tray_icon) {
            Ok(decoded) => {
                let rgba = decoded.to_rgba8();
                let (w, h) = rgba.dimensions();
                let pixels = rgba.into_raw();
                let icon_side = w.min(h);
                // PF-09: registering on the session bus must not hold the first
                // frame. The worker starts here; its result is reported from a
                // background task, so a slow or absent bus costs nothing.
                match Tray::start_background(pixels, icon_side) {
                    Ok((tray, commands, ready)) => {
                        cx.background_executor()
                            .spawn(async move { report_registration(ready) })
                            .detach();
                        let tray = std::sync::Arc::new(tray);
                        shell.update(cx, |shell, _cx| shell.attach_tray(tray.clone()));
                        // The icon is only useful while the main window is gone.
                        let main_window_alive_now = main_window_alive(cx, &main_window);
                        tray.set_visible(tray_visibility(main_window_alive_now));
                        let tray_player_tx = tray_player_tx.clone();
                        // One supervisor keeps the icon and the main window in
                        // step, whichever way the window was closed (red button,
                        // WM, Alt+F4), so the two never contradict each other.
                        {
                            let tray = tray.clone();
                            let shell = shell.clone();
                            let main_window = main_window.clone();
                            cx.spawn(async move |cx| {
                                loop {
                                    cx.background_executor()
                                        .timer(std::time::Duration::from_millis(400))
                                        .await;
                                    let alive =
                                        cx.update(|app| main_window_alive(app, &main_window));
                                    let close_to_tray =
                                        cx.update(|app| shell.read(app).close_to_tray());
                                    tray.set_visible(tray_visibility(alive));
                                    // The main window is the app: losing it with
                                    // close-to-tray off means quit, even while the
                                    // lyrics overlay is still on screen.
                                    if !alive && !close_to_tray {
                                        cx.update(|app| app.quit());
                                        break;
                                    }
                                }
                            })
                            .detach();
                        }
                        cx.spawn(async move |cx| {
                            while let Ok(command) = commands.recv_async().await {
                                tracing::debug!(?command, "tray command received");
                                match command {
                                    TrayCommand::Show => {
                                        cx.update(|app| {
                                            match show_action(main_window_alive(app, &main_window))
                                            {
                                                ShowAction::Focus => {
                                                    let handle =
                                                        *main_window.lock().unwrap_or_else(
                                                            |poisoned| poisoned.into_inner(),
                                                        );
                                                    if let Some(handle) = handle
                                                        && let Err(error) = handle.update(
                                                            app,
                                                            |_shell, window, _cx| {
                                                                window.activate_window()
                                                            },
                                                        )
                                                    {
                                                        tracing::warn!(
                                                            "tray: could not raise the main \
                                                             window: {error}"
                                                        );
                                                    }
                                                }
                                                ShowAction::Reopen => {
                                                    // The main window was destroyed;
                                                    // build a new one so the tray is
                                                    // never a dead end.
                                                    let opts = main_window_options(bounds_for_tray);
                                                    let shell = shell.clone();
                                                    let mut slot =
                                                        main_window.lock().unwrap_or_else(
                                                            |poisoned| poisoned.into_inner(),
                                                        );
                                                    match app.open_window(opts, {
                                                        let shell = shell.clone();
                                                        move |_window, _cx| shell.clone()
                                                    }) {
                                                        Ok(reopened) => {
                                                            *slot = Some(reopened.into());
                                                        }
                                                        Err(error) => tracing::warn!(
                                                            "tray: could not reopen the main \
                                                             window: {error}"
                                                        ),
                                                    }
                                                }
                                            }
                                        });
                                    }
                                    TrayCommand::Hide => {
                                        cx.update(|app| {
                                            let handle = *main_window
                                                .lock()
                                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                                            let Some(handle) = handle else { return };
                                            if let Err(error) = handle
                                                .update(app, |_shell, window, _cx| {
                                                    window.minimize_window()
                                                })
                                            {
                                                tracing::debug!("tray: no window to hide: {error}");
                                            }
                                        });
                                    }
                                    TrayCommand::PlayPause => {
                                        tray_player_tx.send(PlayerCommand::Toggle).ok();
                                    }
                                    TrayCommand::Next => {
                                        cx.update(|app| {
                                            shell.update(app, |shell, cx| shell.play_next(cx));
                                        });
                                    }
                                    TrayCommand::Previous => {
                                        cx.update(|app| {
                                            shell.update(app, |shell, cx| shell.play_previous(cx));
                                        });
                                    }
                                    TrayCommand::PinLyrics => {
                                        cx.update(|app| {
                                            shell.update(app, |shell, cx| shell.toggle_overlay(cx));
                                        });
                                    }
                                    TrayCommand::Quit => {
                                        cx.update(|app| app.quit());
                                        break;
                                    }
                                }
                                cx.update(|app| {
                                    tracing::debug!(
                                        windows = app.windows().len(),
                                        "tray command handled"
                                    );
                                });
                            }
                        })
                        .detach();
                    }
                    Err(e) => tracing::warn!("system tray unavailable: {e}"),
                }
            }
            Err(e) => tracing::warn!("tray icon could not be decoded: {e}"),
        }

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
