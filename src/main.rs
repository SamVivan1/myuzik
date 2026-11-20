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
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// Window icon for X11/XWayland. Wayland surfaces have no icon property and
/// rely on the desktop entry/app id; X11 docks read `_NET_WM_ICON`, so the
/// bundled mark is attached explicitly.
fn app_window_icon() -> Option<std::sync::Arc<image::RgbaImage>> {
    let bytes =
        include_bytes!("../packaging/icons/hicolor/128x128/apps/io.github.samvivan1.Myuzik.png");
    image::load_from_memory(bytes)
        .ok()
        .map(|image| std::sync::Arc::new(image.to_rgba8()))
}

/// Options for the main window, shared by the first open and by the tray reopening it.
fn main_window_options(bounds: Bounds<gpui::Pixels>) -> WindowOptions {
    WindowOptions {
        window_background: WindowBackgroundAppearance::Blurred,
        window_decorations: Some(WindowDecorations::Client),
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(960.), px(640.))),
        is_resizable: true,
        app_id: Some("io.github.samvivan1.Myuzik".to_string()),
        icon: app_window_icon(),
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

/// Raise the main window, reopening it when it was destroyed.
///
/// Two callers need exactly this: the tray's "Show" (INT-02, the tray must never
/// be a dead end) and a second launch asking the running app to come forward
/// (INT-06). Reopening instead of `activate_window()` is what makes the second
/// case work while the window is closed to the tray.
fn show_main_window(
    app: &mut gpui::App,
    shell: &gpui::Entity<Shell>,
    main_window: &std::sync::Mutex<Option<AnyWindowHandle>>,
    fallback_bounds: Bounds<gpui::Pixels>,
) {
    match show_action(main_window_alive(app, main_window)) {
        ShowAction::Focus => {
            let handle = *main_window
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(handle) = handle
                && let Err(error) =
                    handle.update(app, |_shell, window, _cx| window.activate_window())
            {
                tracing::warn!("could not raise the main window: {error}");
            }
        }
        ShowAction::Reopen => {
            let opts = main_window_options(fallback_bounds);
            let mut slot = main_window
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match app.open_window(opts, {
                let shell = shell.clone();
                move |_window, _cx| shell.clone()
            }) {
                Ok(reopened) => *slot = Some(reopened.into()),
                Err(error) => tracing::warn!("could not reopen the main window: {error}"),
            }
        }
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let paths = config::Paths::new().expect("paths");
    paths.ensure().expect("ensure dirs");

    // INT-06: one process only. Two copies mean two tray icons, two players
    // (double audio), double scrobbling, and two last-write-wins writers of
    // config.toml and the credentials file. The lock is advisory (`flock`), so it
    // disappears with the process and no stale file can block the next start.
    let lock_path = instance_lock_path_for(&paths);
    // Held for the rest of `main`: dropping it would let a second copy in.
    let _instance_lock = match instance_decision(acquire_instance_lock(&lock_path)) {
        InstanceDecision::Run(lock) => {
            match &lock {
                Some(lock) => {
                    tracing::debug!(path = %lock.path().display(), "single-instance lock held")
                }
                None => tracing::warn!("running without a single-instance lock"),
            }
            lock
        }
        InstanceDecision::Exit(reason) => {
            // A clean exit with a reason instead of a second app. The running
            // instance is asked to show its window and picks that up on its next
            // tick (the `take_show_request` task below).
            tracing::error!("{reason}");
            let request = show_request_path(&lock_path);
            if let Err(error) = request_show(&request) {
                tracing::warn!(
                    path = %request.display(),
                    %error,
                    "could not ask the running instance to show its window"
                );
            }
            return;
        }
    };

    // INT-08: a corrupt config no longer disappears. The loader keeps the file as
    // `config.toml.corrupt-<timestamp>`, starts on defaults, and reports what
    // happened. `error!` rather than `warn!` because the default filter is ERROR,
    // so a warning was invisible.
    let loaded = config::load_config_recovering(&paths);
    let cfg = loaded.config.clone();
    let startup_notice = loaded.notice();
    if let Some(notice) = &startup_notice {
        tracing::error!(%notice, path = %paths.config.display(), "config recovered");
    }
    tracing::info!(
        ?paths.config,
        server = %cfg.server_url,
        version = cfg.version,
        "config loaded"
    );

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
        // A second launch cannot raise this window; it leaves this request beside
        // the lock instead (see `take_show_request`).
        let show_request = show_request_path(&lock_path);

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
        // INT-08: a corrupt config.toml is moved aside and defaults are loaded.
        // The user has to hear about that, not just the log.
        if let Some(notice) = startup_notice.clone() {
            shell.update(cx, |shell, cx| shell.notice(notice, cx));
        }
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

        // INT-06: when a second launch is refused the lock it asks the running
        // instance to come forward by leaving `myuzik.show-request` next to the
        // lock. Consuming it here means "launching Myuzik again" focuses the
        // window the user wanted, instead of doing nothing. One `unlink` per
        // tick, and it works whether or not the tray could start.
        {
            let shell = shell.clone();
            let main_window = main_window.clone();
            let show_request = show_request.clone();
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(1000))
                        .await;
                    if !take_show_request(&show_request) {
                        continue;
                    }
                    tracing::debug!("a second launch asked for the main window");
                    cx.update(|app| show_main_window(app, &shell, &main_window, bounds_for_tray));
                }
            })
            .detach();
        }

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
                                        .timer(std::time::Duration::from_millis(1000))
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
                                            show_main_window(
                                                app,
                                                &shell,
                                                &main_window,
                                                bounds_for_tray,
                                            )
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

// ---------------------------------------------------------------------------
// Single instance (audit INT-06)
// ---------------------------------------------------------------------------

/// The lock this process holds while it runs.
///
/// Holding the `File` is the whole point: the advisory lock lives as long as the
/// open file description does, so it is released when the process exits (or
/// crashes) and never needs cleaning up.
struct InstanceLock {
    file: File,
    path: PathBuf,
}

impl InstanceLock {
    fn path(&self) -> &Path {
        &self.path
    }

    /// Write this pid into the lock file, so a second launch can name the owner
    /// instead of only saying "already running".
    fn record_owner(&self) -> std::io::Result<()> {
        use std::io::{Seek, SeekFrom, Write};
        let mut file = &self.file;
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        write!(file, "{}", std::process::id())?;
        file.flush()
    }
}

/// The outcome of trying to take the lock.
enum LockAttempt {
    /// This process owns the app.
    Acquired(InstanceLock),
    /// Another process owns it; the pid is read from the lock file.
    Held { pid: Option<u32> },
    /// The lock could not be attempted at all (read-only runtime dir, ...). The
    /// reason is logged where the path is known.
    Unavailable,
}

/// What the process should do, decided without doing it.
enum InstanceDecision {
    /// Run, holding the lock (or unguarded when it could not be taken).
    Run(Option<InstanceLock>),
    /// Another instance owns the app; exit with this reason.
    Exit(String),
}

/// Where the lock and the show request live.
///
/// `$XDG_RUNTIME_DIR` is per user and per session, which is the right scope: a
/// lock that survives a logout would keep the app from starting. Without it
/// (some minimal sessions do not set it), fall back to the cache directory.
fn instance_lock_path(runtime_dir: Option<&Path>, cache_dir: &Path) -> PathBuf {
    match runtime_dir {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join("myuzik.lock"),
        _ => cache_dir.join("myuzik.lock"),
    }
}

fn instance_lock_path_for(paths: &config::Paths) -> PathBuf {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| !dir.as_os_str().is_empty());
    instance_lock_path(runtime_dir.as_deref(), &paths.cache)
}

/// A second launch cannot raise the first window itself, but it can leave this
/// beside the lock for the running instance to pick up (see
/// [`take_show_request`]): one file, written by the newcomer, consumed by the
/// owner. No socket, no new dependency.
fn show_request_path(lock_path: &Path) -> PathBuf {
    lock_path.with_file_name("myuzik.show-request")
}

fn request_show(path: &Path) -> std::io::Result<()> {
    std::fs::write(path, b"show")
}

/// `true` when a second launch asked for the window; removes the request so it
/// fires once. A request that cannot be removed is reported as not seen, so the
/// window is not raised again on every tick.
fn take_show_request(path: &Path) -> bool {
    match std::fs::remove_file(path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "could not consume the show request");
            false
        }
    }
}

fn read_lock_pid(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Take the process lock, non-blocking.
///
/// `File::try_lock` is `flock(2)` underneath: the second process gets
/// `WouldBlock` instead of starting a second tray, player, MPRIS name and
/// config writer (INT-06).
fn acquire_instance_lock(path: &Path) -> LockAttempt {
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        tracing::warn!(
            path = %path.display(),
            %error,
            "single-instance lock unavailable, starting unguarded"
        );
        return LockAttempt::Unavailable;
    }
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
    {
        Ok(file) => file,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "single-instance lock unavailable, starting unguarded"
            );
            return LockAttempt::Unavailable;
        }
    };
    match file.try_lock() {
        Ok(()) => {
            let lock = InstanceLock {
                file,
                path: path.to_path_buf(),
            };
            if let Err(error) = lock.record_owner() {
                tracing::warn!(
                    path = %path.display(),
                    %error,
                    "could not write the instance pid into the lock file"
                );
            }
            LockAttempt::Acquired(lock)
        }
        Err(std::fs::TryLockError::WouldBlock) => LockAttempt::Held {
            pid: read_lock_pid(path),
        },
        Err(std::fs::TryLockError::Error(error)) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "single-instance lock unavailable, starting unguarded"
            );
            LockAttempt::Unavailable
        }
    }
}

fn instance_decision(attempt: LockAttempt) -> InstanceDecision {
    match attempt {
        LockAttempt::Acquired(lock) => InstanceDecision::Run(Some(lock)),
        LockAttempt::Held { pid } => {
            let owner = match pid {
                Some(pid) => format!(" (pid {pid})"),
                None => String::new(),
            };
            InstanceDecision::Exit(format!(
                "Myuzik is already running{owner}; this copy exits so two instances cannot \
                 fight over the tray icon, the player, MPRIS and config.toml"
            ))
        }
        // Running unguarded beats not starting at all.
        LockAttempt::Unavailable => InstanceDecision::Run(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "myuzik-instance-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    /// `$XDG_RUNTIME_DIR` is where a session-scoped lock belongs; without one
    /// (or with an empty one, which some minimal sessions set) the cache dir is
    /// the fallback the audit asked for.
    #[test]
    fn the_lock_path_prefers_the_runtime_dir() {
        let cache = Path::new("/home/sam/.cache/myuzik");
        assert_eq!(
            instance_lock_path(Some(Path::new("/run/user/1000")), cache),
            PathBuf::from("/run/user/1000/myuzik.lock")
        );
        assert_eq!(instance_lock_path(None, cache), cache.join("myuzik.lock"));
        assert_eq!(
            instance_lock_path(Some(Path::new("")), cache),
            cache.join("myuzik.lock")
        );
    }

    /// The show request a second launch leaves behind lives next to the lock, so
    /// both are per-session and per-user.
    #[test]
    fn the_show_request_lives_beside_the_lock() {
        assert_eq!(
            show_request_path(Path::new("/run/user/1000/myuzik.lock")),
            PathBuf::from("/run/user/1000/myuzik.show-request")
        );
    }

    /// The core of INT-06: the second process must not start. It also has to say
    /// who owns the app, because "already running" alone leaves nothing to act on.
    #[test]
    fn a_second_instance_is_told_to_exit_and_names_the_owner() {
        let dir = temp_dir("second");
        let path = dir.join("myuzik.lock");

        let decision = instance_decision(acquire_instance_lock(&path));
        let InstanceDecision::Run(Some(owner)) = decision else {
            panic!("the first instance must run");
        };
        assert_eq!(owner.path(), path, "the lock must be held on the lock file");
        assert_eq!(
            std::fs::read_to_string(&path).expect("lock file"),
            std::process::id().to_string(),
            "the lock file must name the owning process"
        );

        let decision = instance_decision(acquire_instance_lock(&path));
        let InstanceDecision::Exit(reason) = decision else {
            panic!("the second instance must exit, not run");
        };
        assert!(
            reason.contains(&std::process::id().to_string()),
            "the reason must name the running process: {reason}"
        );
        assert!(
            reason.to_lowercase().contains("already running"),
            "the reason must say the app is already running: {reason}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The lock is an advisory file lock, so it goes away with its owner: a
    /// crash (or a quit) must not leave the app permanently unstartable.
    #[test]
    fn the_lock_is_released_when_its_owner_goes_away() {
        let dir = temp_dir("released");
        let path = dir.join("myuzik.lock");

        let InstanceDecision::Run(Some(owner)) = instance_decision(acquire_instance_lock(&path))
        else {
            panic!("first instance must run");
        };
        assert!(matches!(
            instance_decision(acquire_instance_lock(&path)),
            InstanceDecision::Exit(_)
        ));

        drop(owner);

        assert!(
            matches!(
                instance_decision(acquire_instance_lock(&path)),
                InstanceDecision::Run(Some(_))
            ),
            "the lock survived its owner, so the next start would never be allowed"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A lock that cannot even be created (read-only runtime dir, full disk) must
    /// not stop the app: running unguarded is better than not starting at all.
    #[test]
    fn a_lock_that_cannot_be_created_does_not_block_startup() {
        let dir = temp_dir("unavailable");
        let blocker = dir.join("not-a-directory");
        std::fs::write(&blocker, b"x").expect("write blocker");
        let path = blocker.join("myuzik.lock");

        let decision = instance_decision(acquire_instance_lock(&path));

        assert!(
            matches!(decision, InstanceDecision::Run(None)),
            "an unusable lock location must not keep the app from starting"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The second launch cannot raise the first window itself, so it leaves a
    /// request; the running instance takes it exactly once.
    #[test]
    fn a_show_request_is_written_and_consumed_once() {
        let dir = temp_dir("show");
        let path = show_request_path(&dir.join("myuzik.lock"));

        assert!(!take_show_request(&path), "nothing was requested yet");

        request_show(&path).expect("write the show request");
        assert!(path.exists(), "the request must be on disk");

        assert!(take_show_request(&path), "the request must be seen");
        assert!(
            !path.exists(),
            "the request must be consumed, or every later tick would raise the window"
        );
        assert!(
            !take_show_request(&path),
            "a consumed request must not fire twice"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A stale request from a crashed second launch must not be left behind.
    #[test]
    fn a_missing_show_request_is_not_an_error() {
        let dir = temp_dir("no-request");
        assert!(!take_show_request(&dir.join("myuzik.show-request")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
