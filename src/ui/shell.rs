use crate::app::Route;
use crate::config::{AppConfig, Paths};
use crate::lyrics::provider::LyricsProvider;
use crate::navidrome::client::NavidromeClient;
use crate::platform::mpris::MprisHandle;
use crate::player::engine::{PlayerCommand, PlayerEvent, PlayerHandle};
use crate::player::state::{PlaybackStatus, PlayerState};
use crate::theme::midnight_brass;
use crate::ui::now_playing::NowPlayingView;
use gpui::{
    AnimationExt, Context, InteractiveElement, Render, StatefulInteractiveElement, Window, div,
    prelude::*,
};
use std::sync::Arc;

#[allow(dead_code)]
pub struct Shell {
    player_state: gpui::Entity<PlayerState>,
    player_handle: PlayerHandle,
    config: AppConfig,
    paths: Paths,
    route: Route,
    #[allow(dead_code)]
    client: Option<Arc<NavidromeClient>>,
    lyrics_provider: Option<Arc<LyricsProvider>>,
    home_state: gpui::Entity<HomeState>,
    selected_album: Option<String>,
    album_detail: Option<crate::library::AlbumDetail>,
    album_loading: bool,
    artist_detail: Option<crate::library::ArtistDetail>,
    search_query: String,
    search_results: Option<crate::library::SearchResults>,
    search_focus: gpui::FocusHandle,
    now_playing: gpui::Entity<NowPlayingView>,
    _np_subscription: gpui::Subscription,
    mpris_handle: Option<MprisHandle>,
    overlay_state: gpui::Entity<crate::overlay::OverlayState>,
    overlay_handle: Option<gpui::WindowHandle<crate::overlay::PinnedLyricsView>>,
    /// Window id of the pinned overlay, so a close event can be matched to it.
    overlay_window_id: Option<gpui::WindowId>,
    overlay_geometry: Arc<std::sync::Mutex<crate::overlay::OverlayGeometry>>,
    resize_start: Option<(gpui::Point<gpui::Pixels>, gpui::Size<gpui::Pixels>)>,
    toast: Option<String>,
    lyrics_generation: Arc<std::sync::atomic::AtomicU64>,
    scrobble_enabled: Arc<std::sync::atomic::AtomicBool>,
    search_generation: Arc<std::sync::atomic::AtomicU64>,
    /// Autoplay radio state (genre paging, recent-history filter).
    radio: Arc<std::sync::Mutex<crate::library::radio::Radio>>,
    /// Mirrors `config.autoplay` so the async completion can re-check it.
    autoplay_enabled: Arc<std::sync::atomic::AtomicBool>,
    /// Lyric offset in ms, mirrored for the player event loop (which has no
    /// access to `self`): the pinned overlay has to honour the same offset as
    /// the Now Playing screen.
    lyrics_offset_ms: Arc<std::sync::atomic::AtomicI64>,
    /// One radio fetch at a time, so repeated triggers cannot stampede.
    radio_in_flight: Arc<std::sync::atomic::AtomicBool>,
    /// Set when playback ran out before the fetch landed: resume on arrival.
    radio_continue_pending: Arc<std::sync::atomic::AtomicBool>,
    /// Handle used to resume playback from the async radio completion.
    self_weak: gpui::WeakEntity<Shell>,
    /// System tray, when one could be started. A slot rather than a plain field
    /// because it is attached after construction while the player event loop
    /// (created inside `new`) also reports into it.
    tray_slot: Arc<std::sync::Mutex<Option<Arc<crate::platform::tray::Tray>>>>,
    setup_server: String,
    setup_user: String,
    setup_pass: String,
    setup_field: usize,
    setup_error: Option<String>,
    setup_connecting: bool,
    setup_focus: gpui::FocusHandle,
    setup_focus_requested: bool,
    root_focus: gpui::FocusHandle,
    root_focus_requested: bool,
    /// Focus handles for every tabbable control in the shell, keyed by a
    /// logical name (`nav-home`, `player-play`, `album:a1`, …). Created lazily
    /// while rendering and cached so a control keeps the same focus handle
    /// (and therefore the same accesskit node) across frames.
    a11y_focus: std::cell::RefCell<std::collections::HashMap<String, gpui::FocusHandle>>,
}

impl Shell {
    pub fn new(
        cx: &mut gpui::Context<Self>,
        player_handle: PlayerHandle,
        config: AppConfig,
        paths: Paths,
        mpris_handle: Option<MprisHandle>,
    ) -> Self {
        let player_state = cx.new(|_cx| PlayerState::new());
        player_state.update(cx, |ps, _cx| ps.set_volume(config.volume));
        player_handle
            .tx
            .send(PlayerCommand::SetVolume(config.volume))
            .ok();
        let home_state = cx.new(|_cx| HomeState::default());
        let selected_album = None;
        let album_detail = None;
        let album_loading = false;
        let artist_detail = None;
        let search_query = String::new();
        let search_results = None;
        let search_focus = cx.focus_handle();
        let now_playing = cx.new(|cx| NowPlayingView::new(cx, player_handle.clone()));
        let np_subscription = cx.subscribe(
            &now_playing,
            |shell, _np, event: &crate::ui::now_playing::NowPlayingEvent, cx| match event {
                crate::ui::now_playing::NowPlayingEvent::PinToggle => {
                    if shell.overlay_handle.is_some() {
                        shell.close_overlay(cx);
                    } else {
                        shell.open_overlay(cx);
                    }
                }
                crate::ui::now_playing::NowPlayingEvent::OffsetDelta(delta) => {
                    shell.adjust_lyrics_offset(*delta, cx);
                }
            },
        );
        let overlay_state = cx.new(|_cx| crate::overlay::OverlayState::default());

        // The overlay closes its own window, so react to the close event instead of
        // waiting for the main window's next frame: while that window is occluded no
        // frame is drawn, and the tray kept offering "Unpin lyrics" for a window that
        // was already gone (pressing it did nothing).
        let self_weak_on_closed = cx.entity().downgrade();
        cx.on_window_closed(move |app, closed| {
            let Some(shell) = self_weak_on_closed.upgrade() else {
                return;
            };
            // This fires from inside the window update that removed the window, so the
            // shell entity is leased right now: updating it here panics with
            // "already being updated" and kills the tray task. Defer to the next
            // effect cycle instead.
            app.defer(move |app| {
                shell.update(app, |this, cx| {
                    if this.overlay_window_id == Some(closed) {
                        this.overlay_window_id = None;
                        this.overlay_handle = None;
                        if let Some(tray) = this.tray() {
                            tray.set_pinned(false);
                        }
                        cx.notify();
                    }
                });
            });
        })
        .detach();
        let overlay_handle = None;
        let overlay_window_id = None;
        let overlay_geometry = Arc::new(std::sync::Mutex::new(crate::overlay::OverlayGeometry {
            x: config.overlay_x,
            y: config.overlay_y,
            width: config.overlay_width.max(crate::overlay::MIN_WIDTH),
            height: config.overlay_height.max(crate::overlay::MIN_HEIGHT),
        }));
        let resize_start = None;
        let toast = None;
        let lyrics_generation = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let scrobble_enabled = Arc::new(std::sync::atomic::AtomicBool::new(config.scrobble));
        let search_generation = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let setup_server = if config.server_url.is_empty() {
            "https://".to_string()
        } else {
            config.server_url.clone()
        };
        let setup_user = config.username.clone();
        let setup_pass = String::new();
        let setup_field = 0;
        let setup_error = None;
        let setup_connecting = false;
        let setup_focus = cx.focus_handle();
        let setup_focus_requested = false;
        let root_focus = cx.focus_handle();
        let root_focus_requested = false;
        let password = crate::config::load_password(&paths);
        let client = if !config.server_url.is_empty() {
            password
                .and_then(|pw| NavidromeClient::new(&config.server_url, &config.username, &pw).ok())
                .map(Arc::new)
        } else {
            None
        };
        let lyrics_provider = client.as_ref().map(|_| {
            let pw = crate::config::load_password(&paths).unwrap_or_default();
            let owned = NavidromeClient::new(&config.server_url, &config.username, &pw)
                .expect("client already validated");
            Arc::new(LyricsProvider::new(
                owned,
                crate::lyrics::cache::LyricsCache::new(paths.lyrics.clone()),
                config.prefer_navidrome_lyrics,
            ))
        });
        let tray_slot: Arc<std::sync::Mutex<Option<Arc<crate::platform::tray::Tray>>>> =
            Arc::new(std::sync::Mutex::new(None));
        let radio = Arc::new(std::sync::Mutex::new(crate::library::radio::Radio::new()));
        let autoplay_enabled = Arc::new(std::sync::atomic::AtomicBool::new(config.autoplay));
        let radio_in_flight = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let radio_continue_pending = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let self_weak = cx.entity().downgrade();
        let route = if client.is_none() {
            Route::Setup
        } else {
            Route::Home
        };
        // Poll player events in background
        let state = player_state.clone();
        let rx = player_handle.rx.clone();
        let mpris_for_loop = mpris_handle.clone();
        let now_playing_for_loop = now_playing.clone();
        let self_entity = cx.entity();
        let scrobble_client = client.clone();
        let scrobble_enabled_for_loop = scrobble_enabled.clone();
        let overlay_for_loop = overlay_state.clone();
        let tray_slot_for_loop = tray_slot.clone();
        let lyrics_offset_for_loop =
            Arc::new(std::sync::atomic::AtomicI64::new(config.lyrics_offset_ms));
        let lyrics_offset_field = lyrics_offset_for_loop.clone();
        let covers_for_loop = paths.covers.clone();
        // Fetch home data asynchronously
        if let Some(client) = client.clone() {
            spawn_home_fetch(
                client,
                home_state.clone(),
                self_entity.clone(),
                paths.covers.clone(),
                cx,
            );
        }
        cx.spawn(async move |_, cx| {
            let mut scrobble = crate::player::scrobble::ScrobbleTracker::new();
            let mut last_pos = std::time::Duration::ZERO;
            let mut track_duration = std::time::Duration::ZERO;
            let mut scrobble_track: Option<String> = None;
            loop {
                match rx.recv_async().await {
                    Ok(PlayerEvent::StateChanged(s)) => {
                        cx.update(|app| {
                            state.update(app, |ps, _cx| {
                                ps.set_status(s);
                            });
                            let np = now_playing_for_loop.clone();
                            app.update_entity(&np, |view, _cx| {
                                view.set_playing(s == PlaybackStatus::Playing);
                            });
                            if let Some(tray) =
                                tray_slot_for_loop.lock().ok().and_then(|slot| slot.clone())
                            {
                                tray.set_playing(s == PlaybackStatus::Playing);
                            }
                        });
                        if let Some(mh) = &mpris_for_loop {
                            mh.update_status(s);
                        }
                    }
                    Ok(PlayerEvent::Position(pos)) => {
                        cx.update(|app| {
                            state.update(app, |ps, _cx| {
                                ps.set_position(pos);
                            });
                        });
                        if let Some(mh) = &mpris_for_loop {
                            mh.update_position(pos);
                        }
                        let delta = pos.saturating_sub(last_pos);
                        last_pos = pos;
                        if scrobble_enabled_for_loop.load(std::sync::atomic::Ordering::Relaxed)
                            && delta < std::time::Duration::from_secs(2)
                            && delta > std::time::Duration::ZERO
                            && scrobble.tick(delta, track_duration)
                            && let Some(id) = &scrobble_track
                            && let Some(client) = &scrobble_client
                        {
                            let id = id.clone();
                            let client = client.clone();
                            let now_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as i64)
                                .unwrap_or(0);
                            cx.background_executor()
                                .spawn(async move {
                                    let _ = client.scrobble(&id, true, now_ms);
                                })
                                .detach();
                        }
                        let np = now_playing_for_loop.clone();
                        let dur = cx.update(|app| {
                            state
                                .read(app)
                                .duration
                                .map(|d| d.as_secs_f64())
                                .unwrap_or(0.0)
                        });
                        cx.update(|app| {
                            app.update_entity(&np, |v, cx| {
                                v.set_position(cx, pos.as_secs_f64(), dur);
                            });
                        });
                        let os = overlay_for_loop.clone();
                        let offset_ms =
                            lyrics_offset_for_loop.load(std::sync::atomic::Ordering::Relaxed);
                        cx.update(|app| {
                            // The overlay applies the same offset as Now Playing,
                            // and follows plain transcripts by progress rather
                            // than faking a synced line.
                            let duration = state.read(app).duration;
                            app.update_entity(&os, |st, cx| {
                                st.sync_position(pos, offset_ms, duration);
                                // `Entity::update` alone does not notify observers, so
                                // the overlay window would not repaint until it was
                                // clicked. Notify explicitly.
                                cx.notify();
                            });
                        });
                    }
                    Ok(PlayerEvent::Duration(dur)) => {
                        track_duration = dur;
                        cx.update(|app| {
                            state.update(app, |ps, _cx| {
                                ps.duration = Some(dur);
                            });
                        });
                    }
                    Ok(PlayerEvent::Eos) => {
                        cx.update(|app| {
                            state.update(app, |ps, _cx| {
                                ps.set_status(PlaybackStatus::Stopped);
                            });
                            let entity = self_entity.clone();
                            entity.update(app, |shell, cx| {
                                shell.play_next(cx);
                            });
                        });
                    }
                    Ok(PlayerEvent::Error(e)) => {
                        tracing::error!("player error: {:?}", e);
                        let entity = self_entity.clone();
                        let msg = format!("Playback error: {e:?}");
                        cx.update(|app| {
                            entity.update(app, |shell, _cx| {
                                shell.toast = Some(msg);
                            });
                        });
                    }
                    Ok(PlayerEvent::TrackStarted(id)) => {
                        tracing::info!("track started: {}", id);
                        scrobble.reset();
                        last_pos = std::time::Duration::ZERO;
                        scrobble_track = Some(id.clone());
                        if let Some(mh) = &mpris_for_loop {
                            let current = cx.update(|app| state.read(app).current.clone());
                            if let Some(track) = current {
                                let art = track.cover_art_id.as_ref().and_then(|cid| {
                                    let jpg = covers_for_loop.join(format!("{cid}.jpg"));
                                    let png = covers_for_loop.join(format!("{cid}.png"));
                                    if jpg.exists() {
                                        Some(format!("file://{}", jpg.display()))
                                    } else if png.exists() {
                                        Some(format!("file://{}", png.display()))
                                    } else {
                                        None
                                    }
                                });
                                mh.update_metadata(&track, art);
                            }
                        }
                        if scrobble_enabled_for_loop.load(std::sync::atomic::Ordering::Relaxed)
                            && let Some(client) = &scrobble_client
                        {
                            let client = client.clone();
                            let now_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as i64)
                                .unwrap_or(0);
                            cx.background_executor()
                                .spawn(async move {
                                    let _ = client.scrobble(&id, false, now_ms);
                                })
                                .detach();
                        }
                    }
                    Ok(PlayerEvent::Buffering(p)) => {
                        tracing::debug!("buffering: {}%", p);
                    }
                    Ok(PlayerEvent::NextRequested) => {
                        let entity = self_entity.clone();
                        cx.update(|app| {
                            entity.update(app, |shell, cx| shell.play_next(cx));
                        });
                    }
                    Ok(PlayerEvent::PreviousRequested) => {
                        let entity = self_entity.clone();
                        cx.update(|app| {
                            entity.update(app, |shell, cx| shell.play_previous(cx));
                        });
                    }
                    Err(_) => break,
                }
            }
        })
        .detach();

        Self {
            player_state,
            player_handle,
            config,
            paths,
            route,
            client,
            lyrics_provider,
            home_state,
            selected_album,
            album_detail,
            album_loading,
            artist_detail,
            search_query,
            search_results,
            search_focus,
            now_playing,
            _np_subscription: np_subscription,
            overlay_state,
            overlay_handle,
            overlay_window_id,
            overlay_geometry,
            resize_start,
            toast,
            lyrics_generation,
            scrobble_enabled,
            search_generation,
            radio,
            autoplay_enabled,
            lyrics_offset_ms: lyrics_offset_field,
            radio_in_flight,
            radio_continue_pending,
            self_weak,
            tray_slot,
            setup_server,
            setup_user,
            setup_pass,
            setup_field,
            setup_error,
            setup_connecting,
            setup_focus,
            setup_focus_requested,
            root_focus,
            root_focus_requested,
            a11y_focus: std::cell::RefCell::new(std::collections::HashMap::new()),
            mpris_handle,
        }
    }

    /// Give the shell the system tray so it can keep it in step with playback.
    pub fn attach_tray(&self, tray: Arc<crate::platform::tray::Tray>) {
        if let Ok(mut slot) = self.tray_slot.lock() {
            *slot = Some(tray);
        }
    }

    fn tray(&self) -> Option<Arc<crate::platform::tray::Tray>> {
        self.tray_slot.lock().ok().and_then(|slot| slot.clone())
    }

    pub fn toggle_playback(&self) {
        self.player_handle.tx.send(PlayerCommand::Toggle).ok();
    }

    pub fn play(&self) {
        self.player_handle.tx.send(PlayerCommand::Play).ok();
    }

    pub fn pause(&self) {
        self.player_handle.tx.send(PlayerCommand::Pause).ok();
    }

    pub fn next(&self) {
        self.player_handle.tx.send(PlayerCommand::Stop).ok();
    }

    pub fn previous(&self) {
        self.player_handle.tx.send(PlayerCommand::Stop).ok();
    }

    pub fn seek(&self, pos: std::time::Duration) {
        self.player_handle.tx.send(PlayerCommand::Seek(pos)).ok();
    }

    pub fn set_volume(&self, v: f32) {
        self.player_handle.tx.send(PlayerCommand::SetVolume(v)).ok();
    }

    // --- Keyboard focus order and focus ring -------------------------------
    //
    // gpui sorts tab stops by `tab_index` and then by paint order, so these
    // group numbers define the whole window's focus order: sidebar navigation
    // first, then the page content, then the player bar, then the window
    // chrome. Every tabbable control gets a handle from `focus_for` and a
    // 2 px OS-accent ring from `focus_border` (see docs/accessibility.md).

    pub const TAB_NAV: isize = 0;
    pub const TAB_CONTENT: isize = 1;
    pub const TAB_PLAYER: isize = 2;
    pub const TAB_CHROME: isize = 3;

    /// The focus handle for a tabbable control, created on first use.
    fn focus_for(
        &self,
        key: &str,
        tab_index: isize,
        cx: &gpui::Context<Self>,
    ) -> gpui::FocusHandle {
        let mut handles = self.a11y_focus.borrow_mut();
        let handle = handles
            .entry(key.to_string())
            .or_insert_with(|| cx.focus_handle())
            .clone();
        // An element that supplies its own focus handle is inserted into gpui's
        // tab-stop map with *that* handle, so the tab settings have to live on
        // the handle itself (not only on the element).
        handle.tab_stop(true).tab_index(tab_index)
    }

    /// Border colour for a keyboard-focusable control: the OS accent while it
    /// has keyboard focus, otherwise the component's own resting border.
    ///
    /// Controls are drawn with a reserved 2 px border so that gaining focus
    /// never shifts the layout (gpui at this revision has no outline API). A
    /// control that has no border of its own passes
    /// a fully transparent [`gpui::rgba`], so the ring is invisible until it is
    /// focused: focus is the only thing that draws a border. Controls that
    /// already have a visible border (search fields, settings rows, cards)
    /// pass their own token and keep it.
    fn focus_border(
        &self,
        handle: &gpui::FocusHandle,
        window: &gpui::Window,
        rest: impl Into<gpui::Rgba>,
        t: &crate::theme::Colors,
    ) -> gpui::Rgba {
        if handle.is_focused(window) {
            t.os_accent
        } else {
            rest.into()
        }
    }

    /// The logical name of the focused control, when it is one of the shell's
    /// tabbable controls. Used by `tests/a11y.rs` to assert the tab order.
    pub fn focused_control(&self, window: &gpui::Window) -> Option<String> {
        self.a11y_focus
            .borrow()
            .iter()
            .find(|(_, handle)| handle.is_focused(window))
            .map(|(key, _)| key.clone())
    }

    /// The logical names of every control registered for keyboard focus.
    pub fn focus_controls(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.a11y_focus.borrow().keys().cloned().collect();
        keys.sort();
        keys
    }

    /// Whether the pinned window is still on screen. Reads the open-window list
    /// instead of calling `WindowHandle::update`, which panics while the shell entity
    /// is leased.
    fn overlay_alive(&self, cx: &gpui::App) -> bool {
        let Some(id) = self.overlay_window_id else {
            return false;
        };
        cx.windows().iter().any(|window| window.window_id() == id)
    }

    /// Toggle the pinned overlay, deciding from the window that actually exists.
    pub fn toggle_overlay(&mut self, cx: &mut gpui::App) {
        if self.overlay_alive(cx) {
            self.close_overlay(cx);
        } else {
            self.overlay_handle = None;
            self.overlay_window_id = None;
            self.open_overlay(cx);
        }
    }

    pub fn open_overlay(&mut self, cx: &mut gpui::App) {
        if let Some(tray) = self.tray() {
            tray.set_pinned(true);
        }
        if self.overlay_alive(cx) {
            tracing::debug!("overlay is already open");
            return;
        }
        self.overlay_handle = None;
        self.overlay_window_id = None;
        self.persist_overlay_geometry();
        let g = *self.overlay_geometry.lock().unwrap();
        let clamped = match cx.displays().first() {
            Some(display) => {
                let area = display.bounds();
                crate::overlay::clamp_geometry(
                    g,
                    f32::from(area.origin.x) as i32,
                    f32::from(area.origin.y) as i32,
                    f32::from(area.size.width) as u32,
                    f32::from(area.size.height) as u32,
                )
            }
            None => g,
        };
        if let Ok(mut stored) = self.overlay_geometry.lock() {
            *stored = clamped;
        }
        let g = clamped;
        tracing::debug!(
            x = g.x,
            y = g.y,
            w = g.width,
            h = g.height,
            "opening lyrics overlay"
        );
        let bounds = if g.x != 0 || g.y != 0 {
            gpui::Bounds {
                origin: gpui::point(gpui::px(g.x as f32), gpui::px(g.y as f32)),
                size: gpui::size(gpui::px(g.width as f32), gpui::px(g.height as f32)),
            }
        } else {
            gpui::Bounds::centered(
                None,
                gpui::size(gpui::px(g.width as f32), gpui::px(g.height as f32)),
                cx,
            )
        };
        // One place creates the overlay window. This used to build its own
        // `WindowOptions` with stale values (an opaque `Blurred` background and no
        // minimum size), so the window opened from the app or the tray never picked
        // up the fixes made to `overlay::open_overlay`.
        let state = self.overlay_state.clone();
        let geometry = self.overlay_geometry.clone();
        let handle = crate::overlay::open_overlay(cx, state, geometry, bounds);
        handle
            .update(cx, |_view, window, _cx| {
                window.set_window_title(crate::overlay::OVERLAY_TITLE);
            })
            .ok();
        self.overlay_window_id = Some(gpui::AnyWindowHandle::from(handle).window_id());
        self.overlay_handle = Some(handle);
    }

    fn persist_overlay_geometry(&mut self) {
        if let Ok(g) = self.overlay_geometry.lock() {
            self.config.overlay_x = g.x;
            self.config.overlay_y = g.y;
            self.config.overlay_width = g.width.max(crate::overlay::MIN_WIDTH);
            self.config.overlay_height = g.height.max(crate::overlay::MIN_HEIGHT);
            let _ = crate::config::save_config(&self.paths, &self.config);
        }
    }

    pub fn reload_config(&mut self, cx: &mut gpui::App) {
        if let Ok(cfg) = crate::config::load_config(&self.paths) {
            self.config = cfg.clone();
            let password = crate::config::load_password(&self.paths);
            self.client = if !cfg.server_url.is_empty() {
                password
                    .and_then(|pw| NavidromeClient::new(&cfg.server_url, &cfg.username, &pw).ok())
                    .map(Arc::new)
            } else {
                None
            };
            self.route = if cfg.server_url.is_empty() {
                Route::Setup
            } else {
                Route::Home
            };
            if let Some(client) = self.client.clone() {
                let hs = self.home_state.clone();
                cx.spawn(async move |cx| {
                    let client_bg = client.clone();
                    let res = cx
                        .background_executor()
                        .spawn(async move {
                            client_bg.album_list(
                                crate::navidrome::client::AlbumListKind::Newest,
                                100,
                                0,
                            )
                        })
                        .await;
                    cx.update(|app| {
                        app.update_entity(&hs, |h, _| {
                            h.loading = false;
                            match res {
                                Ok(albums) => h.albums = albums,
                                Err(e) => h.error = Some(format!("{:?}", e)),
                            }
                        });
                    });
                })
                .detach();
            }
        }
    }

    pub fn play_track(&mut self, track: crate::library::Track, cx: &mut gpui::App) {
        self.player_state.update(cx, |ps, _cx| {
            ps.play_now(vec![track.clone()], 0);
        });
        // A single track leaves nothing queued, so autoplay has to seed itself
        // from this track while it plays.
        {
            let mut radio = self.radio.lock().unwrap_or_else(|e| e.into_inner());
            radio.reset();
            radio.note_played(&track.id);
        }
        // One path for loading: engine, cover, lyrics, Now Playing and the
        // pinned overlay. Duplicating it here is what left the overlay empty for
        // tracks started from search or the queue.
        self.load_and_play(&track, cx);
        if self.config.autoplay {
            self.ensure_radio(false, cx);
        }
    }

    pub fn play_tracks(
        &mut self,
        tracks: Vec<crate::library::Track>,
        start: usize,
        cx: &mut gpui::App,
    ) {
        if tracks.is_empty() || start >= tracks.len() {
            return;
        }
        self.player_state.update(cx, |ps, _cx| {
            ps.play_now(tracks.clone(), start);
        });
        self.radio.lock().unwrap_or_else(|e| e.into_inner()).reset();
        self.load_and_play(&tracks[start], cx);
    }

    pub fn play_next(&mut self, cx: &mut gpui::App) {
        let next = self.player_state.update(cx, |ps, _cx| ps.next());
        match next {
            Some(track) => self.load_and_play(&track, cx),
            None => {
                // The queue ran out. With autoplay on, fetch similar tracks and
                // resume on the first of them once they arrive.
                if self.config.autoplay {
                    self.ensure_radio(true, cx);
                }
            }
        }
    }

    pub fn play_previous(&mut self, cx: &mut gpui::App) {
        let prev = self.player_state.update(cx, |ps, _cx| ps.previous());
        if let Some(track) = prev {
            self.load_and_play(&track, cx);
        }
    }

    fn cache_track_cover(&mut self, track: &crate::library::Track, cx: &mut gpui::App) {
        let Some(cid) = track.cover_art_id.clone() else {
            return;
        };
        if self.home_state.read(cx).covers.contains_key(&cid) {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        let covers_dir = self.paths.covers.clone();
        let os_for_cover = self.overlay_state.clone();
        let hs = self.home_state.clone();
        let mh = self.mpris_handle.clone();
        let np = self.now_playing.clone();
        let track = track.clone();
        cx.spawn(async move |cx| {
            let c = client.clone();
            let cid2 = cid.clone();
            if let Ok(bytes) = cx
                .background_executor()
                .spawn(async move { c.cover_bytes(&cid2, 512) })
                .await
            {
                let is_png = bytes.starts_with(&[0x89, b'P', b'N', b'G']);
                let ext = if is_png { "png" } else { "jpg" };
                let _ = std::fs::create_dir_all(&covers_dir);
                let path = covers_dir.join(format!("{cid}.{ext}"));
                let _ = std::fs::write(&path, &bytes);
                let format = if is_png {
                    gpui::ImageFormat::Png
                } else {
                    gpui::ImageFormat::Jpeg
                };
                let image = std::sync::Arc::new(gpui::Image {
                    format,
                    bytes,
                    id: fxhash_id(&cid),
                });
                cx.update(|app| {
                    app.update_entity(&hs, |h, _| {
                        h.insert_cover(cid.clone(), image.clone());
                    });
                    app.update_entity(&np, |view, _cx| {
                        view.set_cover(Some(image.clone()));
                    });
                    let rounded = crate::overlay::rounded_cover(&image);
                    app.update_entity(&os_for_cover, |state, cx| {
                        state.set_cover(Some(rounded));
                        cx.notify();
                    });
                });
                if let Some(mh) = &mh {
                    mh.update_metadata(&track, Some(format!("file://{}", path.display())));
                }
            }
        })
        .detach();
    }

    fn load_and_play(&mut self, track: &crate::library::Track, cx: &mut gpui::App) {
        if let Some(client) = &self.client
            && let Ok(uri) = client.stream_url(&track.id, self.config.quality)
        {
            self.player_handle
                .tx
                .send(PlayerCommand::Load {
                    uri,
                    track_id: track.id.clone(),
                })
                .ok();
            self.player_handle.tx.send(PlayerCommand::Play).ok();
        }
        self.cache_track_cover(track, cx);
        // Show the new track's placeholder immediately: without this the pin kept
        // the previous song's lines until the fetch resolved.
        // Reuse the cached artwork so the PiP shows it immediately; if it is not
        // cached yet, `cache_track_cover` fills it in when it lands.
        let cached_cover = track
            .cover_art_id
            .as_ref()
            .and_then(|cid| self.home_state.read(cx).covers.get(cid).cloned())
            .map(|cover| crate::overlay::rounded_cover(&cover));
        self.overlay_state.update(cx, |st, cx| {
            st.begin(track.title.clone(), track.artist.clone());
            st.set_cover(cached_cover);
            cx.notify();
        });
        if let Some(tray) = self.tray() {
            tray.set_track(&track.title, &track.artist);
        }
        if let Some(provider) = &self.lyrics_provider {
            let p = provider.clone();
            let t = track.clone();
            let np = self.now_playing.clone();
            let os = self.overlay_state.clone();
            let cover = track
                .cover_art_id
                .as_ref()
                .and_then(|cid| self.home_state.read(cx).covers.get(cid).cloned());
            let generation = self
                .lyrics_generation
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                + 1;
            let generation_counter = self.lyrics_generation.clone();
            let source_label = if self.config.prefer_navidrome_lyrics {
                "Navidrome first"
            } else {
                "LRCLIB first"
            }
            .to_string();
            cx.spawn(async move |cx| {
                let t_bg = t.clone();
                let p_bg = p.clone();
                let status = cx
                    .background_executor()
                    .spawn(async move {
                        p_bg.fetch(&t_bg)
                            .unwrap_or(crate::lyrics::model::LyricsStatus::NotFound)
                    })
                    .await;
                if generation_counter.load(std::sync::atomic::Ordering::SeqCst) != generation {
                    return;
                }
                cx.update(|app| {
                    app.update_entity(&np, |v, cx| {
                        v.set_track(t.title.clone(), t.artist.clone(), t.album.clone());
                        v.set_cover(cover.clone());
                        v.set_source_label(source_label.clone());
                        v.set_lyrics(cx, status.clone());
                    });
                    app.update_entity(&os, |st, cx| {
                        st.begin(t.title.clone(), t.artist.clone());
                        st.set_status(&status);
                        cx.notify();
                    });
                });
            })
            .detach();
        }

        // Autoplay bookkeeping: remember this track so the radio will not
        // suggest it again soon, and top the queue up *before* it runs dry so
        // the next track starts without waiting for the network.
        self.radio
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .note_played(&track.id);
        if self.config.autoplay && self.player_state.read(cx).queue.remaining() == 0 {
            self.ensure_radio(false, cx);
        }
    }

    /// Autoplay: keep the queue from running dry.
    ///
    /// `continue_playback` means playback already stopped at the end of the
    /// queue, so the batch has to resume playback instead of only extending it.
    /// Everything happens off the UI thread; the flags make repeated calls
    /// (prefetch + end-of-stream racing) collapse into one fetch.
    fn ensure_radio(&mut self, continue_playback: bool, cx: &mut gpui::App) {
        use std::sync::atomic::Ordering;

        if !self.config.autoplay {
            return;
        }
        if self.radio_in_flight.swap(true, Ordering::SeqCst) {
            // A fetch is already running; it must resume playback when it lands.
            if continue_playback {
                self.radio_continue_pending.store(true, Ordering::SeqCst);
            }
            return;
        }
        let reset = |flag: &Arc<std::sync::atomic::AtomicBool>| flag.store(false, Ordering::SeqCst);
        let Some(client) = self.client.clone() else {
            reset(&self.radio_in_flight);
            return;
        };
        let Some(seed) = self.player_state.read(cx).current.clone() else {
            reset(&self.radio_in_flight);
            return;
        };
        {
            let radio = self.radio.lock().unwrap_or_else(|e| e.into_inner());
            if radio.is_stalled() {
                drop(radio);
                reset(&self.radio_in_flight);
                return;
            }
        }
        if continue_playback {
            self.radio_continue_pending.store(true, Ordering::SeqCst);
        }

        let exclude = self.player_state.read(cx).queue.ids();
        let radio = self.radio.clone();
        let inflight = self.radio_in_flight.clone();
        let pending = self.radio_continue_pending.clone();
        let autoplay = self.autoplay_enabled.clone();
        let weak = self.self_weak.clone();
        let ps = self.player_state.clone();

        cx.spawn(async move |cx| {
            let seed_bg = seed.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut guard = radio.lock().unwrap_or_else(|e| e.into_inner());
                    guard.next_batch(&client, &seed_bg, &exclude)
                })
                .await;
            let want_continue = pending.swap(false, Ordering::SeqCst);
            cx.update(|app| {
                inflight.store(false, Ordering::SeqCst);
                if !autoplay.load(Ordering::SeqCst) {
                    return;
                }
                match result {
                    Ok(tracks) if !tracks.is_empty() => {
                        if want_continue {
                            let resumed = ps.update(app, |ps, _| {
                                let resumed = ps.queue.append_continue(tracks);
                                if let Some(t) = &resumed {
                                    ps.adopt(t.clone());
                                }
                                ps.queue.trim_front_to(crate::player::queue::MAX_QUEUE);
                                resumed
                            });
                            if let Some(track) = resumed {
                                let _ = weak.update(app, |shell, cx| {
                                    shell.load_and_play(&track, cx);
                                });
                            }
                        } else {
                            ps.update(app, |ps, _| {
                                ps.queue.append(tracks);
                                ps.queue.trim_front_to(crate::player::queue::MAX_QUEUE);
                            });
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("autoplay radio failed: {e:?}");
                        let msg = format!("Autoplay unavailable: {e:?}");
                        let _ = weak.update(app, |shell, _cx| shell.toast = Some(msg));
                    }
                }
            });
        })
        .detach();
    }

    pub fn set_setup_fields(&mut self, server: &str, user: &str, pass: &str) {
        self.setup_server = server.to_string();
        self.setup_user = user.to_string();
        self.setup_pass = pass.to_string();
    }

    pub fn setup_error(&self) -> Option<&str> {
        self.setup_error.as_deref()
    }

    pub fn setup_server_value(&self) -> &str {
        &self.setup_server
    }

    fn active_setup_mut(&mut self) -> &mut String {
        match self.setup_field {
            0 => &mut self.setup_server,
            1 => &mut self.setup_user,
            _ => &mut self.setup_pass,
        }
    }

    pub fn connect_setup(&mut self, cx: &mut gpui::Context<Self>) {
        let server = self.setup_server.trim().trim_end_matches('/').to_string();
        let user = self.setup_user.trim().to_string();
        let pass = self.setup_pass.clone();
        if !(server.starts_with("http://") || server.starts_with("https://")) {
            self.setup_error = Some("Server URL must start with http:// or https://".to_string());
            return;
        }
        if user.is_empty() || pass.is_empty() {
            self.setup_error = Some("Username and password are required".to_string());
            return;
        }
        let probe = match NavidromeClient::new(&server, &user, &pass) {
            Ok(c) => c,
            Err(e) => {
                self.setup_error = Some(format!("Invalid server URL: {e}"));
                return;
            }
        };
        self.setup_connecting = true;
        self.setup_error = None;
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { probe.ping() })
                .await;
            cx.update(|app| {
                entity.update(app, |shell, cx| {
                    shell.setup_connecting = false;
                    match res {
                        Ok(_) => {
                            shell.config.server_url = server.clone();
                            shell.config.username = user.clone();
                            let _ = crate::config::save_config(&shell.paths, &shell.config);
                            let _ = crate::config::save_password(&shell.paths, &pass);
                            let make = || {
                                NavidromeClient::new(&server, &user, &pass)
                                    .expect("validated client")
                            };
                            shell.client = Some(Arc::new(make()));
                            shell.lyrics_provider = Some(Arc::new(LyricsProvider::new(
                                make(),
                                crate::lyrics::cache::LyricsCache::new(shell.paths.lyrics.clone()),
                                shell.config.prefer_navidrome_lyrics,
                            )));
                            shell.route = Route::Home;
                            if let Some(client) = shell.client.clone() {
                                spawn_home_fetch(
                                    client,
                                    shell.home_state.clone(),
                                    cx.entity(),
                                    shell.paths.covers.clone(),
                                    cx,
                                );
                            }
                        }
                        Err(e) => {
                            shell.setup_error = Some(format!("Connection failed: {e}"));
                        }
                    }
                });
            });
        })
        .detach();
    }

    fn persist_config(&self) {
        let _ = crate::config::save_config(&self.paths, &self.config);
    }

    pub fn cycle_quality(&mut self) {
        self.config.quality = match self.config.quality {
            crate::config::StreamQuality::Original => crate::config::StreamQuality::Kbps320,
            crate::config::StreamQuality::Kbps320 => crate::config::StreamQuality::Kbps192,
            crate::config::StreamQuality::Kbps192 => crate::config::StreamQuality::Original,
        };
        self.persist_config();
    }

    pub fn toggle_scrobble(&mut self) {
        self.config.scrobble = !self.config.scrobble;
        self.scrobble_enabled
            .store(self.config.scrobble, std::sync::atomic::Ordering::Relaxed);
        self.persist_config();
    }

    /// Close the main window.
    ///
    /// Removing the window is all that happens here: `main.rs` runs the app with
    /// an explicit quit mode and a supervisor that keeps the tray icon visible
    /// only while there is no window, so "close to tray" and "close quits" are one
    /// decision instead of two code paths that can disagree.
    pub fn close_window(&mut self, window: &mut gpui::Window, _cx: &mut gpui::Context<Self>) {
        window.remove_window();
    }

    /// Hide the window to the tray on close instead of quitting.
    pub fn toggle_close_to_tray(&mut self) {
        self.config.close_to_tray = !self.config.close_to_tray;
        self.persist_config();
    }

    /// Turn autoplay radio on or off; the choice is persisted.
    pub fn toggle_autoplay(&mut self) {
        self.config.autoplay = !self.config.autoplay;
        self.autoplay_enabled
            .store(self.config.autoplay, std::sync::atomic::Ordering::Relaxed);
        self.persist_config();
    }

    pub fn toggle_lyrics_source(&mut self) {
        self.config.prefer_navidrome_lyrics = !self.config.prefer_navidrome_lyrics;
        self.persist_config();
    }

    pub fn adjust_lyrics_offset(&mut self, delta: i64, cx: &mut gpui::Context<Self>) {
        self.config.lyrics_offset_ms =
            (self.config.lyrics_offset_ms + delta).clamp(-10_000, 10_000);
        self.persist_config();
        let offset = self.config.lyrics_offset_ms;
        self.lyrics_offset_ms
            .store(offset, std::sync::atomic::Ordering::Relaxed);
        self.now_playing.update(cx, |np, cx| {
            np.set_offset_ms(offset);
            np.lyrics.update(cx, |view, _cx| view.set_offset_ms(offset));
        });
    }

    pub fn toggle_reduce_motion(&mut self, cx: &mut gpui::Context<Self>) {
        self.config.reduce_motion = !self.config.reduce_motion;
        cx.set_reduce_motion(self.config.reduce_motion);
        self.persist_config();
    }

    pub fn quality_label(&self) -> &'static str {
        match self.config.quality {
            crate::config::StreamQuality::Original => "Original",
            crate::config::StreamQuality::Kbps320 => "320 kbps",
            crate::config::StreamQuality::Kbps192 => "192 kbps",
        }
    }

    pub fn lyrics_offset(&self) -> i64 {
        self.config.lyrics_offset_ms
    }

    pub fn prefer_navidrome_lyrics(&self) -> bool {
        self.config.prefer_navidrome_lyrics
    }

    pub fn scrobble_on(&self) -> bool {
        self.config.scrobble
    }

    pub fn open_album(&mut self, id: String, cx: &mut gpui::Context<Self>) {
        self.selected_album = Some(id.clone());
        self.album_detail = None;
        self.album_loading = true;
        let Some(client) = self.client.clone() else {
            return;
        };
        let entity = cx.entity();
        let client_for_cover = client.clone();
        cx.spawn(async move |_, cx| {
            let client_for_album = client.clone();
            let album_id = id.clone();
            let res = cx
                .background_executor()
                .spawn(async move { client_for_album.album(&album_id) })
                .await;
            let cover = match &res {
                Ok(d) => match d.album.cover_art_id.clone() {
                    Some(cid) => {
                        let client = client_for_cover.clone();
                        let cid_for_fetch = cid.clone();
                        cx.background_executor()
                            .spawn(async move { client.cover_bytes(&cid_for_fetch, 512) })
                            .await
                            .ok()
                            .map(|bytes| (cid, bytes))
                    }
                    None => None,
                },
                Err(_) => None,
            };
            cx.update(|app| {
                entity.update(app, |shell, cx| {
                    shell.album_loading = false;
                    if let Ok(d) = res {
                        shell.album_detail = Some(d);
                        if let Some((cid, bytes)) = cover {
                            let is_png = bytes.starts_with(&[0x89, b'P', b'N', b'G']);
                            let format = if is_png {
                                gpui::ImageFormat::Png
                            } else {
                                gpui::ImageFormat::Jpeg
                            };
                            let image = std::sync::Arc::new(gpui::Image {
                                format,
                                bytes,
                                id: fxhash_id(&cid),
                            });
                            shell.home_state.update(cx, |h, _cx| {
                                h.insert_cover(cid, image);
                            });
                        }
                    }
                });
            });
        })
        .detach();
    }

    pub fn select_artist(&mut self, id: String, cx: &mut gpui::Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let detail = cx
                .background_executor()
                .spawn(async move { client.artist(&id) })
                .await;
            if let Ok(d) = detail {
                cx.update(|app| {
                    entity.update(app, |shell, _cx| {
                        shell.artist_detail = Some(d);
                    });
                });
            }
        })
        .detach();
    }

    pub fn play_playlist(&mut self, id: String, cx: &mut gpui::Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let detail = cx
                .background_executor()
                .spawn(async move { client.playlist(&id) })
                .await;
            if let Ok(d) = detail {
                cx.update(|app| {
                    entity.update(app, |shell, cx| {
                        shell.play_tracks(d.songs, 0, cx);
                    });
                });
            }
        })
        .detach();
    }

    pub fn toggle_album_star(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(detail) = &self.album_detail else {
            return;
        };
        let Some(client) = self.client.clone() else {
            return;
        };
        let id = detail.album.id.clone();
        let next = !detail.album.starred;
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { client.set_starred(&id, next) })
                .await;
            cx.update(|app| {
                if res.is_ok() {
                    app.update_entity(&entity, |shell, _cx| {
                        if let Some(d) = &mut shell.album_detail {
                            d.album.starred = next;
                        }
                    });
                }
            });
        })
        .detach();
    }

    pub fn go(&mut self, route: Route) {
        self.route = route;
    }

    pub fn route(&self) -> Route {
        self.route.clone()
    }

    pub fn loaded_album_count(&self, cx: &gpui::App) -> usize {
        self.home_state.read(cx).albums.len()
    }

    /// Number of tracks in the queue (used by the autoplay tests and MPRIS).
    pub fn queue_len(&self, cx: &gpui::App) -> usize {
        self.player_state.read(cx).queue.len()
    }

    /// Tracks still to play before the queue runs out.
    pub fn queue_remaining(&self, cx: &gpui::App) -> usize {
        self.player_state.read(cx).queue.remaining()
    }

    /// Id of the track that is playing, if any.
    pub fn current_track_id(&self, cx: &gpui::App) -> Option<String> {
        self.player_state
            .read(cx)
            .current
            .as_ref()
            .map(|t| t.id.clone())
    }

    /// Whether closing the window should leave the app running in the tray.
    pub fn close_to_tray(&self) -> bool {
        self.config.close_to_tray
    }

    /// Whether autoplay radio is enabled.
    pub fn autoplay_on(&self) -> bool {
        self.config.autoplay
    }

    pub fn loaded_artist_count(&self, cx: &gpui::App) -> usize {
        self.home_state.read(cx).artists.len()
    }

    pub fn artist_with_albums(&self, cx: &gpui::App) -> Option<String> {
        self.home_state
            .read(cx)
            .artists
            .iter()
            .find(|a| a.album_count.unwrap_or(0) > 0)
            .map(|a| a.id.clone())
    }

    pub fn first_artist_album_id(&self) -> Option<String> {
        self.artist_detail
            .as_ref()
            .and_then(|d| d.albums.first().map(|a| a.id.clone()))
    }

    pub fn select_first_artist_with_albums(&mut self, cx: &mut gpui::Context<Self>) {
        if let Some(id) = self.artist_with_albums(cx) {
            self.route = Route::Artists;
            self.select_artist(id, cx);
        }
    }

    pub fn open_first_artist_album(&mut self, cx: &mut gpui::Context<Self>) {
        if let Some(id) = self.first_artist_album_id() {
            self.route = Route::Home;
            self.open_album(id, cx);
        }
    }

    pub fn loaded_cover_count(&self, cx: &gpui::App) -> usize {
        self.home_state.read(cx).covers.len()
    }

    pub fn selected_album_id(&self) -> Option<&str> {
        self.selected_album.as_deref()
    }

    pub fn current_album_song_count(&self) -> Option<usize> {
        self.album_detail.as_ref().map(|d| d.songs.len())
    }

    pub fn current_album_starred(&self) -> Option<bool> {
        self.album_detail.as_ref().map(|d| d.album.starred)
    }

    pub fn playback_status(&self, cx: &gpui::App) -> crate::player::state::PlaybackStatus {
        self.player_state.read(cx).status
    }

    pub fn volume(&self, cx: &gpui::App) -> f32 {
        self.player_state.read(cx).volume
    }

    pub fn playhead(&self, cx: &gpui::App) -> std::time::Duration {
        self.player_state.read(cx).position
    }

    pub fn overlay_line_count(&self, cx: &gpui::App) -> usize {
        self.overlay_state.read(cx).line_count()
    }

    pub fn overlay_active(&self, cx: &gpui::App) -> Option<usize> {
        self.overlay_state.read(cx).active()
    }

    pub fn overlay_first_line(&self, cx: &gpui::App) -> Option<String> {
        self.overlay_state.read(cx).first_visible_line()
    }

    /// How many rows the pinned overlay is drawing right now (never more than
    /// the karaoke band, however long the song's lyrics are).
    pub fn overlay_visible_rows(&self, cx: &gpui::App) -> usize {
        self.overlay_state.read(cx).visible_rows().len()
    }

    /// The message the overlay shows when it has no lines to draw.
    pub fn overlay_message(&self, cx: &gpui::App) -> Option<String> {
        let state = self.overlay_state.read(cx);
        if state.line_count() == 0 {
            Some(state.message.clone())
        } else {
            None
        }
    }

    pub fn current_track_title(&self, cx: &gpui::App) -> Option<String> {
        self.player_state
            .read(cx)
            .current
            .as_ref()
            .map(|t| t.title.clone())
    }

    pub fn toast_text(&self) -> Option<&str> {
        self.toast.as_deref()
    }

    pub fn overlay_open(&self) -> bool {
        self.overlay_handle.is_some()
    }

    /// Drop the handle once the overlay window is gone, so the pin toggles and the
    /// tray stay in step with what is actually on screen.
    fn prune_overlay_handle(&mut self, cx: &mut gpui::Context<Self>) {
        if self.overlay_handle.is_none() || self.overlay_alive(cx) {
            return;
        }
        self.overlay_handle = None;
        self.overlay_window_id = None;
        if let Some(tray) = self.tray() {
            tray.set_pinned(false);
        }
        cx.notify();
    }

    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    pub fn search_result_count(&self) -> usize {
        self.search_results
            .as_ref()
            .map(|r| r.songs.len() + r.albums.len() + r.artists.len())
            .unwrap_or(0)
    }

    pub fn favorites_count(&self, cx: &gpui::App) -> usize {
        self.home_state.read(cx).favorites.len()
    }

    pub fn playlist_count(&self, cx: &gpui::App) -> usize {
        self.home_state.read(cx).playlists.len()
    }

    pub fn selected_artist_album_count(&self) -> Option<usize> {
        self.artist_detail.as_ref().map(|d| d.albums.len())
    }

    pub fn close_overlay(&mut self, cx: &mut gpui::App) {
        if let Some(tray) = self.tray() {
            tray.set_pinned(false);
        }
        self.overlay_window_id = None;
        if let Some(handle) = self.overlay_handle.take() {
            let geometry = self.overlay_geometry.clone();
            let _ = handle.update(cx, move |_view, window, _cx| {
                let bounds = window.bounds();
                if let Ok(mut g) = geometry.lock() {
                    *g = crate::overlay::OverlayGeometry {
                        x: f32::from(bounds.origin.x) as i32,
                        y: f32::from(bounds.origin.y) as i32,
                        width: f32::from(bounds.size.width) as u32,
                        height: f32::from(bounds.size.height) as u32,
                    };
                }
                window.remove_window();
            });
        }
        self.persist_overlay_geometry();
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = midnight_brass();
        let t = &theme.colors;

        if self.route == Route::Setup {
            if !self.setup_focus_requested {
                window.focus(&self.setup_focus, cx);
                self.setup_focus_requested = true;
            }
            let setup_entity = cx.entity();
            let row = |idx: usize, label: &str, value: &str, secret: bool| {
                let shown = if secret {
                    "•".repeat(value.chars().count())
                } else {
                    value.to_string()
                };
                let active = self.setup_field == idx;
                let e = setup_entity.clone();
                div()
                    .id(format!("setup-field-{}", idx))
                    .role(gpui::Role::TextInput)
                    .aria_label(format!("{} field", label))
                    .debug_selector({
                        let d = idx;
                        move || format!("setup-field-{}", d)
                    })
                    .on_mouse_down(gpui::MouseButton::Left, move |_ev, window, app| {
                        app.update_entity(&e, |this, _cx| {
                            this.setup_field = idx;
                        });
                        let handle = app.update_entity(&e, |this, _cx| this.setup_focus.clone());
                        window.focus(&handle, app);
                    })
                    .flex()
                    .flex_col()
                    .gap(gpui::px(4.0))
                    .child(
                        div()
                            .text_size(gpui::px(9.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(t.text_tertiary)
                            .child(label.to_uppercase()),
                    )
                    .child(
                        div()
                            .px(gpui::px(12.0))
                            .py(gpui::px(9.0))
                            .rounded(gpui::px(10.0))
                            .bg(t.glass_tint)
                            .border_2()
                            .border_color(if active { t.os_accent } else { t.glass_border })
                            .text_size(gpui::px(12.0))
                            .text_color(if shown.is_empty() {
                                t.text_tertiary
                            } else {
                                t.text_primary
                            })
                            .child(if shown.is_empty() {
                                "…".to_string()
                            } else {
                                shown
                            }),
                    )
            };
            let key_entity = setup_entity.clone();
            return div()
                .size_full()
                .bg(t.bg_base)
                .text_color(t.text_primary)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .id("setup-card")
                        .track_focus(&self.setup_focus)
                        .focusable()
                        .on_key_down(move |ev, _window, app| {
                            app.update_entity(&key_entity, |this, cx| {
                                match ev.keystroke.key.as_str() {
                                    "backspace" => {
                                        this.active_setup_mut().pop();
                                    }
                                    "tab" => {
                                        this.setup_field = (this.setup_field + 1) % 3;
                                    }
                                    "enter" => {
                                        this.connect_setup(cx);
                                    }
                                    key
                                        if !ev.keystroke.modifiers.control
                                            && !ev.keystroke.modifiers.alt =>
                                    {
                                        if let Some(ch) = &ev.keystroke.key_char {
                                            this.active_setup_mut().push_str(ch);
                                        } else if key.len() == 1 {
                                            this.active_setup_mut().push_str(key);
                                        }
                                    }
                                    _ => {}
                                }
                                cx.notify();
                            });
                        })
                        .w(gpui::px(560.0))
                        .flex()
                        .flex_col()
                        .gap(gpui::px(14.0))
                        .p(gpui::px(28.0))
                        .rounded(gpui::px(24.0))
                        .bg(t.glass_tint)
                        .border_1()
                        .border_color(t.glass_border)
                        .child(
                            div()
                                .font_family(theme.display_font)
                                .text_size(gpui::px(26.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child("Connect to Navidrome"),
                        )
                        .child(
                            div()
                                .text_size(gpui::px(11.0))
                                .text_color(t.text_secondary)
                                .child("Tab switches fields · Enter connects. Credentials are stored in the Secret Service or a 0600 file."),
                        )
                        .child(row(0, "Server URL", &self.setup_server, false))
                        .child(row(1, "Username", &self.setup_user, false))
                        .child(row(2, "Password", &self.setup_pass, true))
                        .child(if let Some(err) = &self.setup_error {
                            div()
                                .text_size(gpui::px(11.0))
                                .text_color(t.danger)
                                .child(err.clone())
                                .into_any_element()
                        } else {
                            div().into_any_element()
                        })
                        .child(
                            div()
                                .id("setup-connect")
                                .role(gpui::Role::Button)
                                .aria_label("Connect to Navidrome")
                                .debug_selector(|| "setup-connect".to_string())
                                .cursor_pointer()
                                .px(gpui::px(18.0))
                                .py(gpui::px(10.0))
                                .rounded(gpui::px(999.0))
                                .bg(gpui::linear_gradient(
                                    145.0,
                                    gpui::linear_color_stop(t.gold_light, 0.0),
                                    gpui::linear_color_stop(t.gold, 1.0),
                                ))
                                .text_color(t.clay_ink)
                                .text_size(gpui::px(11.0))
                                .font_weight(gpui::FontWeight::BOLD)
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.connect_setup(cx);
                                }))
                                .child(if self.setup_connecting {
                                    "Connecting…".to_string()
                                } else {
                                    "Connect".to_string()
                                }),
                        ),
                )
                .into_any_element();
        }

        // Keyboard-focus handles for the persistent chrome. The sidebar, the
        // player bar and the window controls are always rendered, so their
        // handles exist up front; content controls create theirs in the branch
        // that renders them.
        let nav_focus: Vec<gpui::FocusHandle> = [
            "nav-home",
            "nav-nowplaying",
            "nav-search",
            "nav-albums",
            "nav-artists",
            "nav-playlists",
            "nav-favorites",
            "nav-settings",
        ]
        .iter()
        .map(|key| self.focus_for(key, Self::TAB_NAV, cx))
        .collect();
        let f_np_open = self.focus_for("player-now-playing", Self::TAB_PLAYER, cx);
        let f_seek_back = self.focus_for("player-seek-back", Self::TAB_PLAYER, cx);
        let f_prev = self.focus_for("player-prev", Self::TAB_PLAYER, cx);
        let f_play = self.focus_for("player-play", Self::TAB_PLAYER, cx);
        let f_next = self.focus_for("player-next", Self::TAB_PLAYER, cx);
        let f_seek_fwd = self.focus_for("player-seek-forward", Self::TAB_PLAYER, cx);
        let f_vol_down = self.focus_for("player-volume-down", Self::TAB_PLAYER, cx);
        let f_vol_up = self.focus_for("player-volume-up", Self::TAB_PLAYER, cx);
        let f_pin = self.focus_for("player-pin-lyrics", Self::TAB_PLAYER, cx);
        let f_sidebar_pin = self.focus_for("sidebar-pin-lyrics", Self::TAB_NAV, cx);
        let f_win_close = self.focus_for("window-close", Self::TAB_CHROME, cx);
        let f_win_min = self.focus_for("window-minimize", Self::TAB_CHROME, cx);
        let f_win_max = self.focus_for("window-maximize", Self::TAB_CHROME, cx);
        let f_search_shortcut = self.focus_for("content-search-shortcut", Self::TAB_CONTENT, cx);
        let f_see_all = self.focus_for("content-see-all-albums", Self::TAB_CONTENT, cx);

        let decorations = window.window_decorations();
        let (shadow_size, rounded_window) = match decorations {
            gpui::Decorations::Client { tiling } if !tiling.is_tiled() => (gpui::px(10.0), true),
            _ => (gpui::px(0.0), false),
        };
        window.set_client_inset(shadow_size);

        let status = self.player_state.read(cx).status;
        let is_playing = status == PlaybackStatus::Playing;
        let current_title = self
            .player_state
            .read(cx)
            .current
            .as_ref()
            .map(|t| t.title.clone())
            .unwrap_or_default();
        let current_artist = self
            .player_state
            .read(cx)
            .current
            .as_ref()
            .map(|t| t.artist.clone())
            .unwrap_or_default();
        let current_cover = self
            .player_state
            .read(cx)
            .current
            .as_ref()
            .and_then(|t| t.cover_art_id.clone())
            .and_then(|cid| self.home_state.read(cx).covers.get(&cid).cloned());
        let position = self.player_state.read(cx).position;
        let duration = self
            .player_state
            .read(cx)
            .duration
            .unwrap_or(std::time::Duration::ZERO);

        let progress_pct = if duration.as_secs_f64() > 0.0 {
            (position.as_secs_f64() / duration.as_secs_f64() * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        // Player bar
        let player = div()
            .absolute()
            .left(shadow_size + gpui::px(18.0))
            .right(shadow_size + gpui::px(18.0))
            .bottom(shadow_size + gpui::px(18.0))
            .h(gpui::px(88.0))
            .rounded(gpui::px(30.0))
            .bg(gpui::linear_gradient(
                145.0,
                gpui::linear_color_stop(t.clay_gradient_start, 0.0),
                gpui::linear_color_stop(t.clay_gradient_end, 1.0),
            ))
            .border_1()
            .border_color(gpui::rgba(0xFFFFFFFF))
            .shadow(vec![
                gpui::BoxShadow {
                    color: gpui::hsla(0.0, 0.0, 0.0, 0.45),
                    offset: gpui::point(gpui::px(0.0), gpui::px(24.0)),
                    blur_radius: gpui::px(52.0),
                    spread_radius: gpui::px(0.0),
                    inset: false,
                },
                gpui::BoxShadow {
                    color: gpui::rgba(0xFFFFFFCC).into(),
                    offset: gpui::point(gpui::px(2.0), gpui::px(2.0)),
                    blur_radius: gpui::px(5.0),
                    spread_radius: gpui::px(0.0),
                    inset: true,
                },
                gpui::BoxShadow {
                    color: t.shadow_deep.into(),
                    offset: gpui::point(gpui::px(-4.0), gpui::px(-4.0)),
                    blur_radius: gpui::px(10.0),
                    spread_radius: gpui::px(0.0),
                    inset: true,
                },
            ])
            .flex()
            .items_center()
            .gap(gpui::px(16.0))
            .px(gpui::px(20.0))
            .child(
                // Now playing info
                div()
                    .flex()
                    .items_center()
                    .gap(gpui::px(12.0))
                    .w(gpui::px(260.0))
                    .cursor_pointer()
                    .id("np-open")
                    .track_focus(&f_np_open)
                    .tab_index(Self::TAB_PLAYER)
                    .role(gpui::Role::Button)
                    .aria_label("Open Now Playing")
                    .border_2()
                    .border_color(self.focus_border(&f_np_open, window, gpui::rgba(0x00000000), t))
                    .debug_selector(|| "np-open".to_string())
                    .on_click(cx.listener(|this, _event, _window, _cx| {
                        this.route = Route::NowPlaying;
                        this.album_detail = None;
                        this.selected_album = None;
                    }))
                    .child(
                        div()
                            .w(gpui::px(52.0))
                            .h(gpui::px(52.0))
                            .rounded(gpui::px(14.0))
                            .bg(t.glass_tint)
                            .border_1()
                            .border_color(t.glass_border)
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(if let Some(image) = current_cover.clone() {
                                gpui::img(gpui::ImageSource::Image(image))
                                    .w(gpui::px(52.0))
                                    .h(gpui::px(52.0))
                                    .rounded(gpui::px(14.0))
                                    .object_fit(gpui::ObjectFit::Cover)
                                    .into_any_element()
                            } else {
                                gpui::div()
                                    .text_size(gpui::px(18.0))
                                    .text_color(t.clay_ink)
                                    .child("♪")
                                    .into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(gpui::px(2.0))
                            .child(
                                div()
                                    .text_size(gpui::px(13.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(t.clay_ink)
                                    .child(if current_title.is_empty() {
                                        "Nothing playing".to_string()
                                    } else {
                                        current_title.clone()
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(gpui::px(10.0))
                                    .text_color(t.clay_ink)
                                    .opacity(0.7)
                                    .child(current_artist.clone()),
                            ),
                    )
                    .child(if is_playing {
                        div()
                            .flex()
                            .items_end()
                            .gap(gpui::px(2.0))
                            .h(gpui::px(14.0))
                            .child(eq_bar("eq1", 620, 4.0, 12.0, t.gold_deep))
                            .child(eq_bar("eq2", 780, 6.0, 14.0, t.gold))
                            .child(eq_bar("eq3", 700, 3.0, 10.0, t.gold_light))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    }),
            )
            .child(
                // Transport
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(gpui::px(6.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(gpui::px(14.0))
                            .child(
                                div()
                                    .w(gpui::px(30.0))
                                    .h(gpui::px(24.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(gpui::px(10.0))
                                    .text_color(t.clay_ink)
                                    .opacity(0.7)
                                    .cursor_pointer()
                                    .id("seek-back")
                                    .aria_label("Seek back 10 seconds")
                                    .track_focus(&f_seek_back)
                                    .tab_index(Self::TAB_PLAYER)
                                    .role(gpui::Role::Button)
                                    .border_2()
                                    .border_color(self.focus_border(
                                        &f_seek_back,
                                        window,
                                        gpui::rgba(0x00000000),
                                        t,
                                    ))
                                    .debug_selector(|| "seek-back".to_string())
                                    .on_click(cx.listener(move |this, _event, _window, _cx| {
                                        let back = position
                                            .saturating_sub(std::time::Duration::from_secs(10));
                                        this.player_handle.tx.send(PlayerCommand::Seek(back)).ok();
                                    }))
                                    .child("−10"),
                            )
                            .child(
                                div()
                                    .w(gpui::px(24.0))
                                    .h(gpui::px(24.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(t.clay_ink)
                                    .cursor_pointer()
                                    .id("prev-btn")
                                    .track_focus(&f_prev)
                                    .tab_index(Self::TAB_PLAYER)
                                    .role(gpui::Role::Button)
                                    .border_2()
                                    .border_color(self.focus_border(
                                        &f_prev,
                                        window,
                                        gpui::rgba(0x00000000),
                                        t,
                                    ))
                                    .aria_label("Previous track")
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.play_previous(cx);
                                    }))
                                    .child("⏮"),
                            )
                            .child(
                                div()
                                    .w(gpui::px(44.0))
                                    .h(gpui::px(44.0))
                                    .rounded(gpui::px(999.0))
                                    .bg(gpui::linear_gradient(
                                        145.0,
                                        gpui::linear_color_stop(t.gold_light, 0.0),
                                        gpui::linear_color_stop(t.gold, 1.0),
                                    ))
                                    .shadow(vec![
                                        gpui::BoxShadow {
                                            color: gpui::rgba(0xFFFFFFB3).into(),
                                            offset: gpui::point(gpui::px(1.5), gpui::px(1.5)),
                                            blur_radius: gpui::px(3.0),
                                            spread_radius: gpui::px(0.0),
                                            inset: true,
                                        },
                                        gpui::BoxShadow {
                                            color: gpui::rgba(0x5A411E55).into(),
                                            offset: gpui::point(gpui::px(-2.5), gpui::px(-2.5)),
                                            blur_radius: gpui::px(6.0),
                                            spread_radius: gpui::px(0.0),
                                            inset: true,
                                        },
                                    ])
                                    .text_color(t.clay_ink)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(gpui::px(16.0))
                                    .cursor_pointer()
                                    .id("play-btn")
                                    .track_focus(&f_play)
                                    .tab_index(Self::TAB_PLAYER)
                                    .role(gpui::Role::Button)
                                    .border_2()
                                    .border_color(self.focus_border(
                                        &f_play,
                                        window,
                                        gpui::rgba(0x00000000),
                                        t,
                                    ))
                                    .aria_label(if is_playing { "Pause" } else { "Play" })
                                    .debug_selector(|| "play-btn".to_string())
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.player_state.update(cx, |ps, _cx| ps.toggle());
                                        this.player_handle.tx.send(PlayerCommand::Toggle).ok();
                                    }))
                                    .child(if is_playing { "⏸" } else { "▶" }),
                            )
                            .child(
                                div()
                                    .w(gpui::px(24.0))
                                    .h(gpui::px(24.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(t.clay_ink)
                                    .cursor_pointer()
                                    .id("next-btn")
                                    .track_focus(&f_next)
                                    .tab_index(Self::TAB_PLAYER)
                                    .role(gpui::Role::Button)
                                    .border_2()
                                    .border_color(self.focus_border(
                                        &f_next,
                                        window,
                                        gpui::rgba(0x00000000),
                                        t,
                                    ))
                                    .aria_label("Next track")
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.play_next(cx);
                                    }))
                                    .child("⏭"),
                            )
                            .child(
                                div()
                                    .w(gpui::px(30.0))
                                    .h(gpui::px(24.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(gpui::px(10.0))
                                    .text_color(t.clay_ink)
                                    .opacity(0.7)
                                    .cursor_pointer()
                                    .id("seek-fwd")
                                    .aria_label("Seek forward 10 seconds")
                                    .track_focus(&f_seek_fwd)
                                    .tab_index(Self::TAB_PLAYER)
                                    .role(gpui::Role::Button)
                                    .border_2()
                                    .border_color(self.focus_border(
                                        &f_seek_fwd,
                                        window,
                                        gpui::rgba(0x00000000),
                                        t,
                                    ))
                                    .debug_selector(|| "seek-fwd".to_string())
                                    .on_click(cx.listener(move |this, _event, _window, _cx| {
                                        let fwd = position
                                            .saturating_add(std::time::Duration::from_secs(10));
                                        this.player_handle.tx.send(PlayerCommand::Seek(fwd)).ok();
                                    }))
                                    .child("+10"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(gpui::px(8.0))
                            .w(gpui::px(320.0))
                            .child(
                                div()
                                    .text_size(gpui::px(8.0))
                                    .text_color(t.clay_ink)
                                    .opacity(0.7)
                                    .child(format_time(position.as_secs_f64())),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(gpui::px(6.0))
                                    .rounded(gpui::px(999.0))
                                    .bg(t.clay_shade)
                                    .child(
                                        div()
                                            .w(gpui::px(progress_pct as f32 * 3.2))
                                            .h_full()
                                            .rounded(gpui::px(999.0))
                                            .bg(t.gold),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(gpui::px(8.0))
                                    .text_color(t.clay_ink)
                                    .opacity(0.7)
                                    .child(format_time(duration.as_secs_f64())),
                            ),
                    ),
            )
            .child(
                // Right controls
                div()
                    .flex()
                    .items_center()
                    .gap(gpui::px(12.0))
                    .w(gpui::px(180.0))
                    .child(
                        div()
                            .text_size(gpui::px(9.0))
                            .text_color(t.clay_ink)
                            .child("VOL"),
                    )
                    .child(
                        div()
                            .id("vol-down")
                            .track_focus(&f_vol_down)
                            .tab_index(Self::TAB_PLAYER)
                            .role(gpui::Role::Button)
                            .aria_label("Volume down")
                            .border_2()
                            .border_color(self.focus_border(
                                &f_vol_down,
                                window,
                                gpui::rgba(0x00000000),
                                t,
                            ))
                            .debug_selector(|| "vol-down".to_string())
                            .cursor_pointer()
                            .px(gpui::px(4.0))
                            .text_size(gpui::px(11.0))
                            .text_color(t.clay_ink)
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                let v = (this.player_state.read(cx).volume - 0.05).clamp(0.0, 1.0);
                                this.player_state.update(cx, |ps, _cx| ps.set_volume(v));
                                this.player_handle.tx.send(PlayerCommand::SetVolume(v)).ok();
                                if let Some(mh) = &this.mpris_handle {
                                    mh.update_volume(v);
                                }
                            }))
                            .child("−"),
                    )
                    .child(
                        div()
                            .w(gpui::px(60.0))
                            .h(gpui::px(5.0))
                            .rounded(gpui::px(999.0))
                            .bg(t.clay_shade)
                            .child(
                                div()
                                    .w(gpui::px(
                                        (self.player_state.read(cx).volume * 60.0).max(2.0),
                                    ))
                                    .h_full()
                                    .rounded(gpui::px(999.0))
                                    .bg(t.os_accent),
                            ),
                    )
                    .child(
                        div()
                            .id("vol-up")
                            .track_focus(&f_vol_up)
                            .tab_index(Self::TAB_PLAYER)
                            .role(gpui::Role::Button)
                            .aria_label("Volume up")
                            .border_2()
                            .border_color(self.focus_border(
                                &f_vol_up,
                                window,
                                gpui::rgba(0x00000000),
                                t,
                            ))
                            .debug_selector(|| "vol-up".to_string())
                            .cursor_pointer()
                            .px(gpui::px(4.0))
                            .text_size(gpui::px(11.0))
                            .text_color(t.clay_ink)
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                let v = (this.player_state.read(cx).volume + 0.05).clamp(0.0, 1.0);
                                this.player_state.update(cx, |ps, _cx| ps.set_volume(v));
                                this.player_handle.tx.send(PlayerCommand::SetVolume(v)).ok();
                                if let Some(mh) = &this.mpris_handle {
                                    mh.update_volume(v);
                                }
                            }))
                            .child("+"),
                    )
                    .child(
                        div()
                            .px(gpui::px(10.0))
                            .py(gpui::px(6.0))
                            .rounded(gpui::px(999.0))
                            .bg(gpui::linear_gradient(
                                145.0,
                                gpui::linear_color_stop(t.clay_dark, 0.0),
                                gpui::linear_color_stop(t.clay_dark_2, 1.0),
                            ))
                            .shadow(vec![
                                gpui::BoxShadow {
                                    color: t.glass_highlight_strong.into(),
                                    offset: gpui::point(gpui::px(1.5), gpui::px(1.5)),
                                    blur_radius: gpui::px(3.0),
                                    spread_radius: gpui::px(0.0),
                                    inset: true,
                                },
                                gpui::BoxShadow {
                                    color: t.scrim.into(),
                                    offset: gpui::point(gpui::px(-3.0), gpui::px(-3.0)),
                                    blur_radius: gpui::px(7.0),
                                    spread_radius: gpui::px(0.0),
                                    inset: true,
                                },
                            ])
                            .text_color(t.gold_light)
                            .text_size(gpui::px(8.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .cursor_pointer()
                            .id("pin-btn")
                            .track_focus(&f_pin)
                            .tab_index(Self::TAB_PLAYER)
                            .role(gpui::Role::Button)
                            .aria_label("Pin or unpin the lyrics overlay")
                            .border_2()
                            .border_color(self.focus_border(
                                &f_pin,
                                window,
                                gpui::rgba(0x00000000),
                                t,
                            ))
                            .active(|style| style.opacity(0.9))
                            .on_click(cx.listener(|this, _event, _window, _cx| {
                                if this.overlay_handle.is_some() {
                                    this.close_overlay(_cx);
                                } else {
                                    this.open_overlay(_cx);
                                }
                            }))
                            .child("Pin lyrics"),
                    ),
            );

        self.prune_overlay_handle(cx);
        if !self.root_focus_requested {
            window.focus(&self.root_focus, cx);
            self.root_focus_requested = true;
        }
        let key_entity = cx.entity();
        // Glass shell with home content
        div()
            .size_full()
            .text_color(t.text_primary)
            .id("app-root")
            .debug_selector(|| "app-root".to_string())
            .track_focus(&self.root_focus)
            .focusable()
            .on_key_down(move |ev, window, app| {
                let entity = key_entity.clone();
                app.update_entity(&entity, |this, cx| {
                    let key = ev.keystroke.key.as_str();
                    let mods = ev.keystroke.modifiers;
                    // Tab moves through the shell's focus order (sidebar →
                    // content → player bar → window chrome). It is handled
                    // before the text-entry guard so Tab also leaves the
                    // search field.
                    if key == "tab" && !mods.control && !mods.alt {
                        if mods.shift {
                            window.focus_prev(cx);
                        } else {
                            window.focus_next(cx);
                        }
                        cx.notify();
                        return;
                    }
                    if this.search_focus.is_focused(window) {
                        return;
                    }
                    match key {
                        "space" if !mods.control && !mods.alt => {
                            this.player_state.update(cx, |ps, _cx| ps.toggle());
                            this.player_handle.tx.send(PlayerCommand::Toggle).ok();
                        }
                        "left" if mods.shift => this.play_previous(cx),
                        "right" if mods.shift => this.play_next(cx),
                        "left" => {
                            let pos = this
                                .player_state
                                .read(cx)
                                .position
                                .saturating_sub(std::time::Duration::from_secs(5));
                            this.player_handle.tx.send(PlayerCommand::Seek(pos)).ok();
                        }
                        "right" => {
                            let pos = this
                                .player_state
                                .read(cx)
                                .position
                                .saturating_add(std::time::Duration::from_secs(5));
                            this.player_handle.tx.send(PlayerCommand::Seek(pos)).ok();
                        }
                        "up" => {
                            let v = (this.player_state.read(cx).volume + 0.05).clamp(0.0, 1.0);
                            this.player_state.update(cx, |ps, _cx| ps.set_volume(v));
                            this.player_handle.tx.send(PlayerCommand::SetVolume(v)).ok();
                        }
                        "down" => {
                            let v = (this.player_state.read(cx).volume - 0.05).clamp(0.0, 1.0);
                            this.player_state.update(cx, |ps, _cx| ps.set_volume(v));
                            this.player_handle.tx.send(PlayerCommand::SetVolume(v)).ok();
                        }
                        "k" if mods.control => {
                            this.route = Route::Search; this.album_detail = None; this.selected_album = None;
                            window.focus(&this.search_focus, cx);
                        }
                        "p" if mods.control => {
                            if this.overlay_handle.is_some() {
                                this.close_overlay(cx);
                            } else {
                                this.open_overlay(cx);
                            }
                        }
                        "escape" if this.overlay_handle.is_some() => {
                            this.close_overlay(cx);
                        }
                        _ => {}
                    }
                    cx.notify();
                });
            })
            .child(
                div()
                    .absolute()
                    .inset(shadow_size)
                    .rounded(if rounded_window { gpui::px(24.0) } else { gpui::px(0.0) })
                    .bg(gpui::linear_gradient(
                        135.0,
                        gpui::linear_color_stop(t.glass_gradient_start, 0.0),
                        gpui::linear_color_stop(t.glass_gradient_end, 1.0),
                    ))
                    .shadow(vec![
                        gpui::BoxShadow {
                            color: gpui::hsla(0.0, 0.0, 0.0, 0.5),
                            offset: gpui::point(gpui::px(0.0), gpui::px(4.0)),
                            blur_radius: gpui::px(20.0),
                            spread_radius: gpui::px(0.0),
                            inset: false,
                        },
                        gpui::BoxShadow {
                            color: t.glass_highlight.into(),
                            offset: gpui::point(gpui::px(0.0), gpui::px(1.0)),
                            blur_radius: gpui::px(0.0),
                            spread_radius: gpui::px(0.0),
                            inset: true,
                        },
                    ])
                    .flex()
                    .flex_col()
                    .overflow_hidden()                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .bg(gpui::linear_gradient(
                                135.0,
                                gpui::linear_color_stop(t.bg_gradient_start, 0.0),
                                gpui::linear_color_stop(t.bg_gradient_end, 1.0),
                            )),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(gpui::px(-220.0))
                            .left(gpui::px(-100.0))
                            .w(gpui::px(820.0))
                            .h(gpui::px(620.0))
                            .rounded(gpui::px(999.0))
                            .bg(t.gold_glow)
                            .shadow(vec![gpui::BoxShadow {
                                color: t.gold_glow_border.into(),
                                offset: gpui::point(gpui::px(0.0), gpui::px(0.0)),
                                blur_radius: gpui::px(260.0),
                                spread_radius: gpui::px(0.0),
                                inset: false,
                            }]),
                    )
                    .child(
                        div()
                            .absolute()
                            .bottom(gpui::px(-240.0))
                            .right(gpui::px(-120.0))
                            .w(gpui::px(760.0))
                            .h(gpui::px(560.0))
                            .rounded(gpui::px(999.0))
                            .bg(t.ambient_teal)
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::rgba(0x2F6B6348).into(),
                                offset: gpui::point(gpui::px(0.0), gpui::px(0.0)),
                                blur_radius: gpui::px(240.0),
                                spread_radius: gpui::px(0.0),
                                inset: false,
                            }]),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(gpui::px(80.0))
                            .right(gpui::px(-160.0))
                            .w(gpui::px(560.0))
                            .h(gpui::px(480.0))
                            .rounded(gpui::px(999.0))
                            .bg(t.ambient_rose)
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::rgba(0x7A4A5540).into(),
                                offset: gpui::point(gpui::px(0.0), gpui::px(0.0)),
                                blur_radius: gpui::px(220.0),
                                spread_radius: gpui::px(0.0),
                                inset: false,
                            }]),
                    )

                    .child(
                        // Glass sheen
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(gpui::px(24.0))
                            .bg(gpui::linear_gradient(
                                135.0,
                                gpui::linear_color_stop(t.glass_highlight, 0.0),
                                gpui::linear_color_stop(gpui::rgba(0xFFFFFF00), 0.42),
                            )),
                    )
                    .child(
                        // Titlebar
                        div()
                            .id("titlebar")
                            .h(gpui::px(38.0))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(gpui::px(10.0))
                            .px(gpui::px(15.0))
                            .bg(t.glass_inner_strong)
                            .border_b_1()
                            .border_color(t.glass_border_soft)
                            .child(
                                div()
                                    .flex_1()
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .id("drag-area")
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|_this, _event, window, _cx| window.start_window_move()))
                                    .text_size(gpui::px(10.0))
                                    .text_color(t.text_tertiary)
                                    .child("myuzik"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(gpui::px(8.0))
                                    .child(
                                        div()
                                            .id("win-close").track_focus(&f_win_close).tab_index(Self::TAB_CHROME).role(gpui::Role::Button).border_2().border_color(self.focus_border(&f_win_close, window, gpui::rgba(0x00000000), t)).debug_selector(|| "win-close".to_string()).aria_label("Close window")
                                            .cursor_pointer()
                                            .w(gpui::px(14.0))
                                            .h(gpui::px(14.0))
                                            .rounded(gpui::px(999.0))
                                            .bg(gpui::rgb(0xFF5F57))
                                            .border_1()
                                            .border_color(gpui::rgba(0x00000033))
                                            .hover(|style| style.bg(gpui::rgb(0xFF7B73)))
                                            .on_click(cx.listener(|this, _event, window, cx| this.close_window(window, cx))),
                                    )
                                    .child(
                                        div()
                                            .id("win-min").track_focus(&f_win_min).tab_index(Self::TAB_CHROME).role(gpui::Role::Button).border_2().border_color(self.focus_border(&f_win_min, window, gpui::rgba(0x00000000), t)).debug_selector(|| "win-min".to_string()).aria_label("Minimize window")
                                            .cursor_pointer()
                                            .w(gpui::px(14.0))
                                            .h(gpui::px(14.0))
                                            .rounded(gpui::px(999.0))
                                            .bg(gpui::rgb(0xFEBC2E))
                                            .border_1()
                                            .border_color(gpui::rgba(0x00000033))
                                            .hover(|style| style.bg(gpui::rgb(0xFFD04A)))
                                            .on_click(cx.listener(|_this, _event, window, _cx| window.minimize_window())),
                                    )
                                    .child(
                                        div()
                                            .id("win-max").track_focus(&f_win_max).tab_index(Self::TAB_CHROME).role(gpui::Role::Button).border_2().border_color(self.focus_border(&f_win_max, window, gpui::rgba(0x00000000), t)).debug_selector(|| "win-max".to_string()).aria_label("Maximize window")
                                            .cursor_pointer()
                                            .w(gpui::px(14.0))
                                            .h(gpui::px(14.0))
                                            .rounded(gpui::px(999.0))
                                            .bg(gpui::rgb(0x28C840))
                                            .border_1()
                                            .border_color(gpui::rgba(0x00000033))
                                            .hover(|style| style.bg(gpui::rgb(0x4AE05F)))
                                            .on_click(cx.listener(|_this, _event, window, _cx| window.zoom_window())),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .overflow_hidden()
                    .child(
                        // Sidebar
                        div()
                            .w(gpui::px(212.0))
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .gap(gpui::px(12.0))
                            .px(gpui::px(16.0))
                            .pt(gpui::px(18.0))
                            .pb(gpui::px(118.0))
                            .bg(t.glass_inner)
                            .shadow(vec![gpui::BoxShadow {
                                color: t.glass_highlight_soft.into(),
                                offset: gpui::point(gpui::px(0.0), gpui::px(1.0)),
                                blur_radius: gpui::px(0.0),
                                spread_radius: gpui::px(0.0),
                                inset: true,
                            }])
                            .border_r_1()
                            .border_color(t.glass_border)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(gpui::px(10.0))
                                    .child(
                                        div()
                                            .w(gpui::px(30.0))
                                            .h(gpui::px(30.0))
                                            .rounded(gpui::px(10.0))
                                            .bg(gpui::linear_gradient(
                                                145.0,
                                                gpui::linear_color_stop(t.gold_light, 0.0),
                                                gpui::linear_color_stop(t.gold, 1.0),
                                            ))
                                            .shadow(vec![
                                                gpui::BoxShadow {
                                                    color: gpui::rgba(0xFFFFFF99).into(),
                                                    offset: gpui::point(gpui::px(1.5), gpui::px(1.5)),
                                                    blur_radius: gpui::px(3.0),
                                                    spread_radius: gpui::px(0.0),
                                                    inset: true,
                                                },
                                                gpui::BoxShadow {
                                                    color: gpui::rgba(0x5A411E66).into(),
                                                    offset: gpui::point(gpui::px(-2.0), gpui::px(-2.0)),
                                                    blur_radius: gpui::px(4.0),
                                                    spread_radius: gpui::px(0.0),
                                                    inset: true,
                                                },
                                            ])
                                            .text_color(t.clay_ink)
                                            .font_family(theme.display_font)
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_size(gpui::px(18.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child("M"),
                                    )
                                    .child(
                                        div()
                                            .font_family(theme.display_font)
                                            .text_size(gpui::px(22.0))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child("myuzik"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(gpui::px(4.0))
                                    .child(nav_item("Home", self.route == Route::Home, t).id("nav-home").track_focus(&nav_focus[0]).tab_index(Self::TAB_NAV).role(gpui::Role::Link).aria_label("Home").aria_selected(self.route == Route::Home).border_2().border_color(self.focus_border(&nav_focus[0], window, gpui::rgba(0x00000000), t)).debug_selector(|| "nav-home".to_string()).active(|style| style.bg(t.clay_surface).text_color(t.clay_ink)).on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::Home; this.selected_album = None; this.album_detail = None; })))
                                    .child(nav_item("Now Playing", self.route == Route::NowPlaying, t).id("nav-nowplaying").track_focus(&nav_focus[1]).tab_index(Self::TAB_NAV).role(gpui::Role::Link).aria_label("Now Playing").aria_selected(self.route == Route::NowPlaying).border_2().border_color(self.focus_border(&nav_focus[1], window, gpui::rgba(0x00000000), t)).debug_selector(|| "nav-nowplaying".to_string()).active(|style| style.bg(t.clay_surface).text_color(t.clay_ink)).on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::NowPlaying; this.album_detail = None; this.selected_album = None; })))
                                    .child(nav_item("Search", self.route == Route::Search, t).id("nav-search").track_focus(&nav_focus[2]).tab_index(Self::TAB_NAV).role(gpui::Role::Link).aria_label("Search").aria_selected(self.route == Route::Search).border_2().border_color(self.focus_border(&nav_focus[2], window, gpui::rgba(0x00000000), t)).debug_selector(|| "nav-search".to_string()).active(|style| style.bg(t.clay_surface).text_color(t.clay_ink)).on_click(cx.listener(|this, _event, window, cx| { this.route = Route::Search; this.album_detail = None; this.selected_album = None; window.focus(&this.search_focus, cx); })))
                                    .child(nav_item("Albums", self.route == Route::Albums, t).id("nav-albums").track_focus(&nav_focus[3]).tab_index(Self::TAB_NAV).role(gpui::Role::Link).aria_label("Albums").aria_selected(self.route == Route::Albums).border_2().border_color(self.focus_border(&nav_focus[3], window, gpui::rgba(0x00000000), t)).debug_selector(|| "nav-albums".to_string()).active(|style| style.bg(t.clay_surface).text_color(t.clay_ink)).on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::Albums; this.album_detail = None; this.selected_album = None; })))
                                    .child(nav_item("Artists", self.route == Route::Artists, t).id("nav-artists").track_focus(&nav_focus[4]).tab_index(Self::TAB_NAV).role(gpui::Role::Link).aria_label("Artists").aria_selected(self.route == Route::Artists).border_2().border_color(self.focus_border(&nav_focus[4], window, gpui::rgba(0x00000000), t)).debug_selector(|| "nav-artists".to_string()).active(|style| style.bg(t.clay_surface).text_color(t.clay_ink)).on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::Artists; this.album_detail = None; this.selected_album = None; })))
                                    .child(nav_item("Playlists", self.route == Route::Playlists, t).id("nav-playlists").track_focus(&nav_focus[5]).tab_index(Self::TAB_NAV).role(gpui::Role::Link).aria_label("Playlists").aria_selected(self.route == Route::Playlists).border_2().border_color(self.focus_border(&nav_focus[5], window, gpui::rgba(0x00000000), t)).debug_selector(|| "nav-playlists".to_string()).active(|style| style.bg(t.clay_surface).text_color(t.clay_ink)).on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::Playlists; this.album_detail = None; this.selected_album = None; })))
                                    .child(nav_item("Favorites", self.route == Route::Favorites, t).id("nav-favorites").track_focus(&nav_focus[6]).tab_index(Self::TAB_NAV).role(gpui::Role::Link).aria_label("Favorites").aria_selected(self.route == Route::Favorites).border_2().border_color(self.focus_border(&nav_focus[6], window, gpui::rgba(0x00000000), t)).debug_selector(|| "nav-favorites".to_string()).active(|style| style.bg(t.clay_surface).text_color(t.clay_ink)).on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::Favorites; this.album_detail = None; this.selected_album = None; })))
                                    .child(nav_item("Settings", self.route == Route::Settings, t).id("nav-settings").track_focus(&nav_focus[7]).tab_index(Self::TAB_NAV).role(gpui::Role::Link).aria_label("Settings").aria_selected(self.route == Route::Settings).border_2().border_color(self.focus_border(&nav_focus[7], window, gpui::rgba(0x00000000), t)).debug_selector(|| "nav-settings".to_string()).active(|style| style.bg(t.clay_surface).text_color(t.clay_ink)).on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::Settings; this.album_detail = None; this.selected_album = None; }))),
                            )
                            .child(
                                div()
                                    .mt_auto()
                                    .flex()
                                    .flex_col()
                                    .gap(gpui::px(8.0))
                                    .child(
                                        div()
                                            .px(gpui::px(14.0))
                                            .py(gpui::px(10.0))
                                            .rounded(gpui::px(theme.radii.button))
                                            .bg(gpui::linear_gradient(
                                                145.0,
                                                gpui::linear_color_stop(t.clay_dark, 0.0),
                                                gpui::linear_color_stop(t.clay_dark_2, 1.0),
                                            ))
                                            .shadow(vec![
                                                gpui::BoxShadow {
                                                    color: t.glass_highlight_strong.into(),
                                                    offset: gpui::point(gpui::px(1.5), gpui::px(1.5)),
                                                    blur_radius: gpui::px(3.0),
                                                    spread_radius: gpui::px(0.0),
                                                    inset: true,
                                                },
                                                gpui::BoxShadow {
                                                    color: t.scrim.into(),
                                                    offset: gpui::point(gpui::px(-3.0), gpui::px(-3.0)),
                                                    blur_radius: gpui::px(7.0),
                                                    spread_radius: gpui::px(0.0),
                                                    inset: true,
                                                },
                                            ])
                                            .text_color(t.gold_light)
                                            .font_family(theme.ui_font)
                                            .text_size(gpui::px(9.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .cursor_pointer()
                                            .id("pin-btn").track_focus(&f_sidebar_pin).tab_index(Self::TAB_NAV).role(gpui::Role::Button).aria_label("Pin or unpin the lyrics overlay").border_2().border_color(self.focus_border(&f_sidebar_pin, window, gpui::rgba(0x00000000), t)).active(|style| style.opacity(0.9))
                                            .on_click(cx.listener(|this, _event, _window, _cx| {
                                                if this.overlay_handle.is_some() { this.close_overlay(_cx); } else { this.open_overlay(_cx); }
                                            }))
                                            .child("Pin lyrics"),
                                    ),
                            ),
                    )
                    .child(
                        // Main content
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(gpui::px(16.0))
                            .px(gpui::px(24.0))
                            .py(gpui::px(18.0))
                            .pb(gpui::px(112.0))
                            .id("main-scroll")
                            .overflow_y_scroll()
                            .child(if self.route == Route::Home && self.selected_album.is_some() {
                                div()
                            } else {
                                div()
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap(gpui::px(16.0))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(gpui::px(4.0))
                                            .child(
                                                div()
                                                    .font_family(theme.display_font)
                                                    .text_size(gpui::px(38.0))
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .child(match self.route {
                                                        crate::app::Route::Home => "Good evening",
                                                        crate::app::Route::Search => "Search",
                                                        crate::app::Route::NowPlaying => "Now Playing",
                                                        crate::app::Route::Settings => "Settings",
                                                        crate::app::Route::Setup => "Setup",
                                                        crate::app::Route::Albums => "Albums",
                                                        crate::app::Route::Artists => "Artists",
                                                        crate::app::Route::Playlists => "Playlists",
                                                        crate::app::Route::Favorites => "Favorites",
                                                        _ => "Myuzik",
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .text_size(gpui::px(9.0))
                                                    .text_color(t.text_tertiary)
                                                    .child({
                                                        let hs = self.home_state.read(cx);
                                                        if hs.loading {
                                                            "Loading library…".to_string()
                                                        } else if let Some(e) = &hs.error {
                                                            format!("Library error: {}", e)
                                                        } else if hs.albums.is_empty() {
                                                            "No albums loaded".to_string()
                                                        } else {
                                                            format!("{} albums loaded", hs.albums.len())
                                                        }
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("home-search")
                                            .track_focus(&f_search_shortcut)
                                            .tab_index(Self::TAB_CONTENT)
                                            .role(gpui::Role::Button)
                                            .aria_label("Search your library (Ctrl K)")
                                            .border_2()
                                            .border_color(self.focus_border(&f_search_shortcut, window, t.glass_border, t))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .gap(gpui::px(8.0))
                                            .w(gpui::px(252.0))
                                            .px(gpui::px(14.0))
                                            .py(gpui::px(10.0))
                                            .rounded(gpui::px(999.0))
                                            .bg(gpui::linear_gradient(
                                                145.0,
                                                gpui::linear_color_stop(t.clay_surface, 0.0),
                                                gpui::linear_color_stop(t.clay_shade, 1.0),
                                            ))
                                            .shadow(vec![
                                                gpui::BoxShadow {
                                                    color: gpui::rgba(0xFFFFFFFF).into(),
                                                    offset: gpui::point(gpui::px(1.5), gpui::px(1.5)),
                                                    blur_radius: gpui::px(3.0),
                                                    spread_radius: gpui::px(0.0),
                                                    inset: true,
                                                },
                                                gpui::BoxShadow {
                                                    color: t.shadow_deep.into(),
                                                    offset: gpui::point(gpui::px(-2.0), gpui::px(-2.0)),
                                                    blur_radius: gpui::px(5.0),
                                                    spread_radius: gpui::px(0.0),
                                                    inset: true,
                                                },
                                            ])
                                            .text_color(t.clay_ink)
                                            .text_size(gpui::px(11.0))
                                            .on_click(cx.listener(|this, _event, window, cx| {
                                                this.route = Route::Search; this.album_detail = None; this.selected_album = None;
                                                window.focus(&this.search_focus, cx);
                                            }))
                                            .child("Search your library…")
                                            .child(div().flex_1())
                                            .child(
                                                div()
                                                    .text_size(gpui::px(9.0))
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(t.os_accent)
                                                    .child("Ctrl K"),
                                            ),
                                    )
                            })
                            .child(if self.selected_album.is_some()
                                || matches!(self.route, Route::NowPlaying | Route::Search | Route::Settings | Route::Artists | Route::Playlists | Route::Favorites) {
                                div()
                            } else {
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .font_family(theme.display_font)
                                            .text_size(gpui::px(18.0))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child("Recently added"),
                                    )
                                    .child(
                                        div()
                                            .id("see-all-albums")
                                            .track_focus(&f_see_all)
                                            .tab_index(Self::TAB_CONTENT)
                                            .role(gpui::Role::Link)
                                            .aria_label("See all albums")
                                            .border_2()
                                            .border_color(self.focus_border(&f_see_all, window, gpui::rgba(0x00000000), t))
                                            .cursor_pointer()
                                            .text_size(gpui::px(10.0))
                                            .text_color(t.text_secondary)
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::Albums; this.album_detail = None; this.selected_album = None; }))
                                            .child("See all"),
                                    )
                            })
                            .child(if self.route == Route::NowPlaying {
                                div().child(self.now_playing.clone())
                            } else if self.route == Route::Search {
                                let mut col = div().flex().flex_col().gap(gpui::px(12.0));
                                let search_entity = cx.entity();
                                col = col.child(
                                    div()
                                        .id("search-box")
                                        .track_focus(&self.search_focus)
                                        .tab_index(Self::TAB_CONTENT)
                                        .role(gpui::Role::TextInput)
                                        .aria_label("Search your library")
                                        .focusable()
                                        .on_key_down(move |ev, _window, app| {
                                            let entity = search_entity.clone();
                                            app.update_entity(&entity, |this, cx| {
                                                let k = ev.keystroke.key.as_str();
                                                if k == "backspace" {
                                                    this.search_query.pop();
                                                } else if k == "escape" {
                                                    this.search_query.clear();
                                                    this.search_results = None;
                                                    this.search_generation
                                                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                                } else if k == "enter" {
                                                    let q = this.search_query.trim().to_string();
                                                    if !q.is_empty()
                                                        && let Some(client) = this.client.clone() {
                                                        let entity2 = cx.entity();
                                                        let generation = this
                                                            .search_generation
                                                            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                                                            + 1;
                                                        let counter = this.search_generation.clone();
                                                        cx.spawn(async move |_, cx| {
                                                            let res = cx.background_executor().spawn(async move { client.search(&q) }).await;
                                                            if counter.load(std::sync::atomic::Ordering::SeqCst) != generation {
                                                                return;
                                                            }
                                                            cx.update(|app| {
                                                                app.update_entity(&entity2, |shell, _| {
                                                                    if let Ok(r) = res { shell.search_results = Some(r); }
                                                                });
                                                            });
                                                        }).detach();
                                                    }
                                                } else if !ev.keystroke.modifiers.control && !ev.keystroke.modifiers.alt {
                                                    if let Some(ch) = &ev.keystroke.key_char {
                                                        this.search_query.push_str(ch);
                                                    } else if k.len() == 1 && k.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
                                                        this.search_query.push_str(k);
                                                    }
                                                }
                                                cx.notify();
                                            });
                                        })
                                        .flex()
                                        .items_center()
                                        .gap(gpui::px(10.0))
                                        .px(gpui::px(16.0))
                                        .py(gpui::px(12.0))
                                        .rounded(gpui::px(999.0))
                                        .bg(t.glass_tint)
                                        .border_2()
                                        .border_color(self.focus_border(
                                            &self.search_focus,
                                            window,
                                            t.glass_border,
                                            t,
                                        ))
                                        .cursor_text()
                                        .child(
                                            div()
                                                .text_size(gpui::px(12.0))
                                                .text_color(t.text_primary)
                                                .child(if self.search_query.is_empty() {
                                                    "Type to search artists, albums, songs…".to_string()
                                                } else {
                                                    self.search_query.clone()
                                                }),
                                        ),
                                );
                                if self.search_query.is_empty() {
                                    col = col.child(
                                        div()
                                            .text_size(gpui::px(11.0))
                                            .text_color(t.text_secondary)
                                            .child("Press Enter to search. Esc clears."),
                                    );
                                }
                                if let Some(results) = &self.search_results {
                                    if !results.songs.is_empty() {
                                        col = col.child(div().text_size(gpui::px(9.0)).font_weight(gpui::FontWeight::BOLD).text_color(t.gold).child("SONGS"));
                                        let songs = results.songs.clone();
                                        for (i, tr) in results.songs.iter().take(20).enumerate() {
                                            let songs2 = songs.clone();
                                            let f_search_song =
                                                self.focus_for(&format!("search-song-{i}"), Self::TAB_CONTENT, cx);
                                            col = col.child(
                                                div()
                                                    .id(format!("search-song-{}", i))
                                                    .track_focus(&f_search_song)
                                                    .tab_index(Self::TAB_CONTENT)
                                                    .role(gpui::Role::Button)
                                                    .border_2()
                                                    .border_color(self.focus_border(
                                                        &f_search_song,
                                                        window,
                                                        t.glass_border,
                                                        t,
                                                    ))
                                                    .aria_label(format!("Play {}", tr.title))
                                                    .cursor_pointer()
                                                    .px(gpui::px(12.0))
                                                    .py(gpui::px(8.0))
                                                    .rounded(gpui::px(10.0))
                                                    .bg(t.glass_tint)
                                                    .border_1()
                                                    .border_color(t.glass_border)
                                                    .text_size(gpui::px(12.0))
                                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                                        this.play_tracks(songs2.clone(), i, cx);
                                                    }))
                                                    .child(format!("{} — {}", tr.title, tr.artist)),
                                            );
                                        }
                                    }
                                    if results.songs.is_empty() && results.albums.is_empty() && results.artists.is_empty() {
                                        col = col.child(
                                            div().text_size(gpui::px(11.0)).text_color(t.text_secondary).child("No results"),
                                        );
                                    }
                                }
                                col
                            } else if self.route == Route::Settings {
                                let mut col = div().flex().flex_col().gap(gpui::px(8.0));
                                let f_set_quality = self.focus_for("settings-quality", Self::TAB_CONTENT, cx);
                                let f_set_scrobble = self.focus_for("settings-scrobble", Self::TAB_CONTENT, cx);
                                let f_set_lyrics_source =
                                    self.focus_for("settings-lyrics-source", Self::TAB_CONTENT, cx);
                                let f_set_reduce_motion =
                                    self.focus_for("settings-reduce-motion", Self::TAB_CONTENT, cx);
                                let f_set_autoplay =
                                    self.focus_for("settings-autoplay", Self::TAB_CONTENT, cx);
                                let f_set_close_to_tray =
                                    self.focus_for("settings-close-to-tray", Self::TAB_CONTENT, cx);
                                let f_offset_down = self.focus_for("settings-offset-down", Self::TAB_CONTENT, cx);
                                let f_offset_up = self.focus_for("settings-offset-up", Self::TAB_CONTENT, cx);
                                col = col
                                    .child(settings_section("Account", t))
                                    .child(setting_row("Server URL", &self.config.server_url, t))
                                    .child(setting_row("Username", &self.config.username, t))
                                    .child(settings_section("Playback", t))
                                    .child(
                                        setting_row("Stream quality", self.quality_label(), t)
                                            .id("set-quality").debug_selector(|| "set-quality".to_string())
                                            .track_focus(&f_set_quality).tab_index(Self::TAB_CONTENT).role(gpui::Role::Button).aria_label(format!("Stream quality, {}", self.quality_label())).border_2().border_color(self.focus_border(&f_set_quality, window, t.glass_border, t))
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _event, _window, _cx| {
                                                this.cycle_quality();
                                            })),
                                    )
                                    .child(
                                        setting_row(
                                            "Scrobble",
                                            if self.config.scrobble { "On" } else { "Off" },
                                            t,
                                        )
                                        .id("set-scrobble").debug_selector(|| "set-scrobble".to_string())
                                            .track_focus(&f_set_scrobble).tab_index(Self::TAB_CONTENT).role(gpui::Role::Button).aria_label(format!("Scrobble, {}", if self.config.scrobble { "on" } else { "off" })).border_2().border_color(self.focus_border(&f_set_scrobble, window, t.glass_border, t))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _event, _window, _cx| {
                                            this.toggle_scrobble();
                                        })),
                                    )
                                    .child(
                                        setting_row(
                                            "Autoplay similar",
                                            if self.config.autoplay { "On" } else { "Off" },
                                            t,
                                        )
                                        .id("set-autoplay").debug_selector(|| "set-autoplay".to_string())
                                        .track_focus(&f_set_autoplay).tab_index(Self::TAB_CONTENT).role(gpui::Role::Button)
                                        .aria_label(format!("Autoplay similar tracks, {}", if self.config.autoplay { "on" } else { "off" }))
                                        .border_2().border_color(self.focus_border(&f_set_autoplay, window, t.glass_border, t))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _event, _window, _cx| {
                                            this.toggle_autoplay();
                                        })),
                                    )
                                    .child(
                                        setting_row(
                                            "Close to tray",
                                            if self.config.close_to_tray { "On" } else { "Off" },
                                            t,
                                        )
                                        .id("set-close-to-tray").debug_selector(|| "set-close-to-tray".to_string())
                                        .track_focus(&f_set_close_to_tray).tab_index(Self::TAB_CONTENT).role(gpui::Role::Button)
                                        .aria_label(format!("Close to tray, {}", if self.config.close_to_tray { "on" } else { "off" }))
                                        .border_2().border_color(self.focus_border(&f_set_close_to_tray, window, t.glass_border, t))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _event, _window, _cx| {
                                            this.toggle_close_to_tray();
                                        })),
                                    )
                                    .child(setting_row(
                                        "Volume",
                                        &format!(
                                            "{}%",
                                            (self.player_state.read(cx).volume * 100.0) as i32
                                        ),
                                        t,
                                    ))
                                    .child(settings_section("Lyrics", t))
                                    .child(setting_row(
                                        "Offset",
                                        &format!("{} ms", self.config.lyrics_offset_ms),
                                        t,
                                    ))
                                    .child(
                                        div()
                                            .flex()
                                            .gap(gpui::px(8.0))
                                            .child(
                                                div()
                                                    .id("offset-down").debug_selector(|| "offset-down".to_string())
                                            .track_focus(&f_offset_down).tab_index(Self::TAB_CONTENT).role(gpui::Role::Button).aria_label("Decrease lyrics offset by 500 ms").border_2().border_color(self.focus_border(&f_offset_down, window, t.glass_border, t))
                                                    .cursor_pointer()
                                                    .px(gpui::px(12.0))
                                                    .py(gpui::px(6.0))
                                                    .rounded(gpui::px(999.0))
                                                    .bg(t.glass_highlight_soft)
                                                    .border_1()
                                                    .border_color(t.glass_border)
                                                    .text_size(gpui::px(10.0))
                                                    .on_click(cx.listener(
                                                        |this, _event, _window, cx| {
                                                            this.adjust_lyrics_offset(-500, cx);
                                                        },
                                                    ))
                                                    .child("−500 ms"),
                                            )
                                            .child(
                                                div()
                                                    .id("offset-up").debug_selector(|| "offset-up".to_string())
                                            .track_focus(&f_offset_up).tab_index(Self::TAB_CONTENT).role(gpui::Role::Button).aria_label("Increase lyrics offset by 500 ms").border_2().border_color(self.focus_border(&f_offset_up, window, t.glass_border, t))
                                                    .cursor_pointer()
                                                    .px(gpui::px(12.0))
                                                    .py(gpui::px(6.0))
                                                    .rounded(gpui::px(999.0))
                                                    .bg(t.glass_highlight_soft)
                                                    .border_1()
                                                    .border_color(t.glass_border)
                                                    .text_size(gpui::px(10.0))
                                                    .on_click(cx.listener(
                                                        |this, _event, _window, cx| {
                                                            this.adjust_lyrics_offset(500, cx);
                                                        },
                                                    ))
                                                    .child("+500 ms"),
                                            ),
                                    )
                                    .child(
                                        setting_row(
                                            "Source",
                                            if self.config.prefer_navidrome_lyrics {
                                                "Navidrome first"
                                            } else {
                                                "LRCLIB first"
                                            },
                                            t,
                                        )
                                        .id("set-lyrics-source").debug_selector(|| "set-lyrics-source".to_string())
                                            .track_focus(&f_set_lyrics_source).tab_index(Self::TAB_CONTENT).role(gpui::Role::Button).aria_label(format!("Lyrics source, {}", if self.config.prefer_navidrome_lyrics { "Navidrome first" } else { "LRCLIB first" })).border_2().border_color(self.focus_border(&f_set_lyrics_source, window, t.glass_border, t))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _event, _window, _cx| {
                                            this.toggle_lyrics_source();
                                        })),
                                    )
                                    .child(settings_section("Appearance", t))
                                    .child(
                                        setting_row(
                                            "Reduce motion",
                                            if self.config.reduce_motion { "On" } else { "Off" },
                                            t,
                                        )
                                        .id("set-reduce-motion").debug_selector(|| "set-reduce-motion".to_string())
                                            .track_focus(&f_set_reduce_motion).tab_index(Self::TAB_CONTENT).role(gpui::Role::Button).aria_label(format!("Reduce motion, {}", if self.config.reduce_motion { "on" } else { "off" })).border_2().border_color(self.focus_border(&f_set_reduce_motion, window, t.glass_border, t))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _event, _window, cx| {
                                            this.toggle_reduce_motion(cx);
                                        })),
                                    )
                                    .child(settings_section("About", t))
                                    .child(setting_row("Version", "0.1.0", t))
                                    .child(setting_row("License", "MIT", t));
                                col
                            } else if self.route == Route::Artists {
                                if let Some(detail) = &self.artist_detail {
                                    let albums = detail.albums.clone();
                                    let artist_name = detail.artist.name.clone();
                                    let f_back_link =
                                        self.focus_for("artists-back", Self::TAB_CONTENT, cx);
                                    let artist_grid_label = format!("Albums by {}", artist_name);
                                    let mut grid = div()
                                        .grid()
                                        .grid_cols(4)
                                        .gap(gpui::px(14.0))
                                        .id("artist-album-grid")
                                        .role(gpui::Role::Grid)
                                        .aria_label(artist_grid_label);
                                    for a in albums.iter() {
                                        let aid = a.id.clone();
                                        let f_album =
                                            self.focus_for(&format!("artist-album:{}", aid), Self::TAB_CONTENT, cx);
                                        grid = grid.child(
                                            album_card(&a.name, &a.artist, None, t)
                                                .id(format!("artist-album-{}", aid))
                                                .track_focus(&f_album)
                                                .tab_index(Self::TAB_CONTENT)
                                                .role(gpui::Role::Button)
                                                .aria_label(format!("Open album {}", a.name))
                                                .border_2()
                                                .border_color(self.focus_border(
                                                    &f_album,
                                                    window,
                                                    t.glass_border,
                                                    t,
                                                ))
                                                .debug_selector({
                                                    let d = aid.clone();
                                                    move || format!("artist-album-{}", d)
                                                })
                                                .cursor_pointer()
                                                .on_click(cx.listener(move |this, _event, _window, cx| {
                                                    this.route = Route::Home;
                                                    this.open_album(aid.clone(), cx);
                                                })),
                                        );
                                    }
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(gpui::px(14.0))
                                        .child(
                                            div()
                                                .id("artist-back")
                                                .track_focus(&f_back_link)
                                                .tab_index(Self::TAB_CONTENT)
                                                .role(gpui::Role::Link)
                                                .aria_label("Back to artists")
                                                .border_2()
                                                .border_color(self.focus_border(
                                                    &f_back_link,
                                                    window,
                                                    t.glass_border_soft,
                                                    t,
                                                ))
                                                .debug_selector(|| "artist-back".to_string())
                                                .cursor_pointer()
                                                .text_size(gpui::px(10.0))
                                                .text_color(t.text_secondary)
                                                .on_click(cx.listener(|this, _event, _window, _cx| {
                                                    this.artist_detail = None;
                                                }))
                                                .child("← Back to artists"),
                                        )
                                        .child(
                                            div()
                                                .font_family(theme.display_font)
                                                .text_size(gpui::px(26.0))
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .child(artist_name),
                                        )
                                        .child(grid)
                                } else {
                                    let hs = self.home_state.read(cx);
                                    let mut col = div()
                                        .flex()
                                        .flex_col()
                                        .gap(gpui::px(4.0))
                                        .id("artist-list")
                                        .role(gpui::Role::List)
                                        .aria_label("Artists");
                                    for a in &hs.artists {
                                        let aid = a.id.clone();
                                        let f_artist =
                                            self.focus_for(&format!("artist:{}", aid), Self::TAB_CONTENT, cx);
                                        col = col.child(
                                            div()
                                                .id(format!("artist-{}", aid))
                                                .track_focus(&f_artist)
                                                .tab_index(Self::TAB_CONTENT)
                                                .role(gpui::Role::ListItem)
                                                .aria_label(format!(
                                                    "Show albums by {}",
                                                    a.name
                                                ))
                                                .border_2()
                                                .border_color(self.focus_border(
                                                    &f_artist,
                                                    window,
                                                    t.glass_border,
                                                    t,
                                                ))
                                                .debug_selector({
                                                    let d = aid.clone();
                                                    move || format!("artist-{}", d)
                                                })
                                                .cursor_pointer()
                                                .px(gpui::px(12.0))
                                                .py(gpui::px(9.0))
                                                .rounded(gpui::px(10.0))
                                                .bg(t.glass_tint)
                                                .border_1()
                                                .border_color(t.glass_border)
                                                .text_size(gpui::px(12.0))
                                                .on_click(cx.listener(move |this, _event, _window, cx| {
                                                    this.select_artist(aid.clone(), cx);
                                                }))
                                                .child(a.name.clone()),
                                        );
                                    }
                                    div().w_full().child(col)
                                }
                            } else if self.route == Route::Playlists {
                                let hs = self.home_state.read(cx);
                                let mut col = div()
                                    .flex()
                                    .flex_col()
                                    .gap(gpui::px(4.0))
                                    .id("playlist-list")
                                    .role(gpui::Role::List)
                                    .aria_label("Playlists");
                                for p in &hs.playlists {
                                    let pid = p.id.clone();
                                    let f_playlist =
                                        self.focus_for(&format!("playlist:{}", pid), Self::TAB_CONTENT, cx);
                                    col = col.child(
                                        div()
                                            .id(format!("playlist-{}", pid))
                                            .track_focus(&f_playlist)
                                            .tab_index(Self::TAB_CONTENT)
                                            .role(gpui::Role::ListItem)
                                            .aria_label(format!(
                                                "Play playlist {}",
                                                p.name
                                            ))
                                            .border_2()
                                            .border_color(self.focus_border(
                                                &f_playlist,
                                                window,
                                                t.glass_border,
                                                t,
                                            ))
                                            .debug_selector({
                                                let d = pid.clone();
                                                move || format!("playlist-{}", d)
                                            })
                                            .cursor_pointer()
                                            .px(gpui::px(12.0))
                                            .py(gpui::px(9.0))
                                            .rounded(gpui::px(10.0))
                                            .bg(t.glass_tint)
                                            .border_1()
                                            .border_color(t.glass_border)
                                            .text_size(gpui::px(12.0))
                                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                                this.play_playlist(pid.clone(), cx);
                                            }))
                                            .child(format!("{} · {} tracks", p.name, p.song_count)),
                                    );
                                }
                                div().w_full().child(col)
                            } else if self.selected_album.is_some() {
                                div()
                            } else {
                                let hs = self.home_state.read(cx);
                                let grid_label = match self.route {
                                    Route::Favorites => "Favorite albums",
                                    Route::Albums => "Albums",
                                    _ => "Recently added albums",
                                };
                                let mut grid = div()
                                    .grid()
                                    .grid_cols(4)
                                    .gap(gpui::px(14.0))
                                    .id("album-grid")
                                    .role(gpui::Role::Grid)
                                    .aria_label(grid_label);
                                if hs.loading && hs.albums.is_empty() {
                                    for i in 0..8 {
                                        grid = grid.child(album_skeleton(i, t));
                                    }
                                } else if hs.albums.is_empty() {
                                    grid = grid.child(album_card("No albums", "Connect a server", None, t));
                                } else {
                                    let source = if self.route == Route::Favorites { &hs.favorites } else { &hs.albums };
                                let limit = if matches!(self.route, Route::Albums | Route::Favorites) { source.len() } else { 8 };
                                    for a in source.iter().take(limit) {
                                        let aid = a.id.clone();
                                        let f_album =
                                            self.focus_for(&format!("album:{}", aid), Self::TAB_CONTENT, cx);
                                        let cover = a.cover_art_id.as_ref().and_then(|cid| hs.covers.get(cid)).cloned();
                                        grid = grid.child(
                                            album_card(&a.name, &a.artist, cover, t)
                                                .id(format!("album-{}", aid))
                                                .track_focus(&f_album)
                                                .tab_index(Self::TAB_CONTENT)
                                                .role(gpui::Role::Button)
                                                .border_2()
                                                .border_color(self.focus_border(
                                                    &f_album,
                                                    window,
                                                    t.glass_border_soft,
                                                    t,
                                                ))
                                                .debug_selector({
                                                    let d = aid.clone();
                                                    move || format!("album-{}", d)
                                                })
                                                .aria_label(format!("Open album {}", a.name))
                                                .active(|style| style.opacity(0.94))
                                                .cursor_pointer()
                                                .on_click(cx.listener(move |this, _event, _window, cx| {
                                                    this.open_album(aid.clone(), cx);
                                                })),
                                        );
                                    }
                                }
                                div().w_full().child(grid)
                            })
                            .child(if self.route == Route::Home && let Some(detail) = &self.album_detail {
                                let mut col = div().flex().flex_col().gap(gpui::px(6.0));
                                let f_album_back = self.focus_for("album-back", Self::TAB_CONTENT, cx);
                                let f_album_star = self.focus_for("album-star", Self::TAB_CONTENT, cx);
                                let f_album_play_all =
                                    self.focus_for("album-play-all", Self::TAB_CONTENT, cx);
                                let songs_all = detail.songs.clone();
                                let album_name = detail.album.name.clone();
                                let album_artist = detail.album.artist.clone();
                                let album_meta = {
                                    let mut parts = Vec::new();
                                    if let Some(year) = detail.album.year {
                                        parts.push(year.to_string());
                                    }
                                    let count = detail.songs.len();
                                    parts.push(if count == 1 {
                                        "1 song".to_string()
                                    } else {
                                        format!("{count} songs")
                                    });
                                    if let Some(duration) = detail.album.duration {
                                        parts.push(format!("{} min", duration.as_secs() / 60));
                                    }
                                    parts.join(" · ")
                                };
                                let album_cover = detail
                                    .album
                                    .cover_art_id
                                    .as_ref()
                                    .and_then(|cid| self.home_state.read(cx).covers.get(cid))
                                    .cloned();
                                let initial = detail
                                    .album
                                    .name
                                    .chars()
                                    .next()
                                    .map(|c| c.to_uppercase().to_string())
                                    .unwrap_or_else(|| "?".to_string());
                                col = col.child(
                                    div()
                                        .flex()
                                        .items_start()
                                        .gap(gpui::px(18.0))
                                        .child(
                                            div()
                                                .w(gpui::px(160.0))
                                                .h(gpui::px(160.0))
                                                .flex_shrink_0()
                                                .rounded(gpui::px(18.0))
                                                .bg(gpui::linear_gradient(
                                                    135.0,
                                                    gpui::linear_color_stop(t.gold_deep, 0.0),
                                                    gpui::linear_color_stop(t.clay_surface, 1.0),
                                                ))
                                                .border_1()
                                                .border_color(t.glass_border)
                                                .overflow_hidden()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(if let Some(image) = album_cover {
                                                    gpui::img(gpui::ImageSource::Image(image))
                                                        .w(gpui::px(160.0))
                                                        .h(gpui::px(160.0))
                                                        .rounded(gpui::px(18.0))
                                                        .object_fit(gpui::ObjectFit::Cover)
                                                        .into_any_element()
                                                } else {
                                                    div()
                                                        .font_family(theme.display_font)
                                                        .text_size(gpui::px(48.0))
                                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                                        .text_color(t.clay_ink)
                                                        .opacity(0.85)
                                                        .child(initial)
                                                        .into_any_element()
                                                }),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .items_start()
                                                .gap(gpui::px(8.0))
                                                .child(
                                                    div()
                                                        .id("album-back")
                                                        .track_focus(&f_album_back)
                                                        .tab_index(Self::TAB_CONTENT)
                                                        .role(gpui::Role::Link)
                                                        .aria_label("Back to library")
                                                        .border_2()
                                                        .border_color(self.focus_border(
                                                            &f_album_back,
                                                            window,
                                                            t.glass_border_soft,
                                                            t,
                                                        ))
                                                        .cursor_pointer()
                                                        .text_size(gpui::px(10.0))
                                                        .text_color(t.text_secondary)
                                                        .on_click(cx.listener(|this, _event, _window, _cx| {
                                                            this.selected_album = None;
                                                            this.album_detail = None;
                                                        }))
                                                        .child("← Back to library"),
                                                )
                                                .child(
                                                    div()
                                                        .font_family(theme.display_font)
                                                        .text_size(gpui::px(28.0))
                                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                                        .child(album_name),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(gpui::px(12.0))
                                                        .text_color(t.text_secondary)
                                                        .child(album_artist),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(gpui::px(10.0))
                                                        .text_color(t.text_tertiary)
                                                        .child(album_meta),
                                                )
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap(gpui::px(8.0))
                                                        .child(
                                                            div()
                                                                .id("album-star")
                                                                .track_focus(&f_album_star)
                                                                .tab_index(Self::TAB_CONTENT)
                                                                .role(gpui::Role::Button)
                                                                .aria_label(if detail.album.starred {
                                                                    "Remove album from favorites"
                                                                } else {
                                                                    "Add album to favorites"
                                                                })
                                                                .border_2()
                                                                .border_color(self.focus_border(
                                                                    &f_album_star,
                                                                    window,
                                                                    t.glass_border,
                                                                    t,
                                                                ))
                                                                .debug_selector(|| "album-star".to_string())
                                                                .cursor_pointer()
                                                                .px(gpui::px(14.0))
                                                                .py(gpui::px(8.0))
                                                                .rounded(gpui::px(999.0))
                                                                .bg(t.glass_highlight_soft)
                                                                .border_1()
                                                                .border_color(t.glass_border)
                                                                .text_color(t.gold_light)
                                                                .text_size(gpui::px(10.0))
                                                                .font_weight(gpui::FontWeight::BOLD)
                                                                .on_click(cx.listener(|this, _event, _window, cx| {
                                                                    this.toggle_album_star(cx);
                                                                }))
                                                                .child(if detail.album.starred { "★ Favorited" } else { "☆ Favorite" }),
                                                        )
                                                        .child(
                                                            div()
                                                                .id("album-play-all")
                                                                .track_focus(&f_album_play_all)
                                                                .tab_index(Self::TAB_CONTENT)
                                                                .role(gpui::Role::Button)
                                                                .aria_label("Play all tracks")
                                                                .border_2()
                                                                .border_color(self.focus_border(
                                                                    &f_album_play_all,
                                                                    window,
                                                                    t.gold_light,
                                                                    t,
                                                                ))
                                                                .debug_selector(|| "album-play-all".to_string())
                                                                .cursor_pointer()
                                                                .px(gpui::px(16.0))
                                                                .py(gpui::px(8.0))
                                                                .rounded(gpui::px(999.0))
                                                                .bg(gpui::linear_gradient(
                                                                    145.0,
                                                                    gpui::linear_color_stop(t.gold_light, 0.0),
                                                                    gpui::linear_color_stop(t.gold, 1.0),
                                                                ))
                                                                .text_color(t.clay_ink)
                                                                .text_size(gpui::px(10.0))
                                                                .font_weight(gpui::FontWeight::BOLD)
                                                                .on_click(cx.listener(move |this, _event, _window, cx| {
                                                                    this.play_tracks(songs_all.clone(), 0, cx);
                                                                }))
                                                                .child("Play all"),
                                                        ),
                                                ),
                                        ),
                                );
                                let songs = detail.songs.clone();
                                let current_id = self.player_state.read(cx).current.as_ref().map(|c| c.id.clone());
                                // The rows are a list so a screen reader announces the
                                // track count and lets the user walk the album.
                                let tracks_label = format!("Tracks in {}", detail.album.name);
                                let mut tracks = div()
                                    .flex()
                                    .flex_col()
                                    .gap(gpui::px(6.0))
                                    .id("track-list")
                                    .role(gpui::Role::List)
                                    .aria_label(tracks_label);
                                for (i, t_) in detail.songs.iter().enumerate() {
                                    let songs2 = songs.clone();
                                    let is_current = current_id.as_deref() == Some(t_.id.as_str());
                                    let f_track = self.focus_for(&format!("track-{i}"), Self::TAB_CONTENT, cx);
                                    tracks = tracks.child(
                                        track_row(
                                            &format!("{:02}", i + 1),
                                            &t_.title,
                                            &t_.artist,
                                            &format_time(t_.duration.as_secs_f64()),
                                            is_current,
                                            t,
                                        )
                                        .id(format!("track-{}", i))
                                        .track_focus(&f_track)
                                        .tab_index(Self::TAB_CONTENT)
                                        .role(gpui::Role::ListItem)
                                        .border_2()
                                        .border_color(self.focus_border(
                                            &f_track,
                                            window,
                                            if is_current {
                                                t.gold_glow_border
                                            } else {
                                                t.glass_border
                                            },
                                            t,
                                        ))
                                        .debug_selector(move || format!("track-{}", i))
                                        .aria_label(format!("Play {}", t_.title))
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _event, _window, cx| {
                                            this.play_tracks(songs2.clone(), i, cx);
                                        })),
                                    );
                                }
                                col.child(tracks)
                            } else {
                                div()
                            }),
                    ),
                ),
            )
            .child(player)
            .child({
                let grip_entity = cx.entity();
                div()
                    .absolute()
                    .right(shadow_size + gpui::px(4.0))
                    .bottom(shadow_size + gpui::px(4.0))
                    .w(gpui::px(18.0))
                    .h(gpui::px(18.0))
                    .id("resize-grip")
                    .cursor(gpui::CursorStyle::ResizeUpLeftDownRight)
                    .on_mouse_down(gpui::MouseButton::Left, {
                        let e = grip_entity.clone();
                        move |event, window, app| {
                            let e = e.clone();
                            app.update_entity(&e, |this, _cx| {
                                this.resize_start = Some((event.position, window.bounds().size));
                            });
                        }
                    })
                    .on_mouse_move({
                        let e = grip_entity.clone();
                        move |event, window, app| {
                            let e = e.clone();
                            app.update_entity(&e, |this, _cx| {
                                if let Some((start, size)) = this.resize_start {
                                    let dx = event.position.x - start.x;
                                    let dy = event.position.y - start.y;
                                    let w = (size.width + dx).max(gpui::px(960.0));
                                    let h = (size.height + dy).max(gpui::px(640.0));
                                    window.resize(gpui::size(w, h));
                                }
                            });
                        }
                    })
                    .on_mouse_up(gpui::MouseButton::Left, {
                        let e = grip_entity.clone();
                        move |_event, _window, app| {
                            let e = e.clone();
                            app.update_entity(&e, |this, _cx| {
                                this.resize_start = None;
                            });
                        }
                    })
            })
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(shadow_size)
                    .cursor(gpui::CursorStyle::ResizeUpDown)
                    .on_mouse_down(gpui::MouseButton::Left, |_event, window, _cx| {
                        window.start_window_resize(gpui::ResizeEdge::Top);
                    }),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(shadow_size)
                    .cursor(gpui::CursorStyle::ResizeUpDown)
                    .on_mouse_down(gpui::MouseButton::Left, |_event, window, _cx| {
                        window.start_window_resize(gpui::ResizeEdge::Bottom);
                    }),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(shadow_size)
                    .cursor(gpui::CursorStyle::ResizeLeftRight)
                    .on_mouse_down(gpui::MouseButton::Left, |_event, window, _cx| {
                        window.start_window_resize(gpui::ResizeEdge::Left);
                    }),
            )
            .child(
                div()
                    .absolute()
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .w(shadow_size)
                    .cursor(gpui::CursorStyle::ResizeLeftRight)
                    .on_mouse_down(gpui::MouseButton::Left, |_event, window, _cx| {
                        window.start_window_resize(gpui::ResizeEdge::Right);
                    }),
            )
            .with_animation(
                format!("route-{:?}", self.route),
                gpui::Animation::new(std::time::Duration::from_millis(180))
                    .with_easing(gpui::ease_out_quint()),
                |el, delta| el.opacity(0.35 + 0.65 * delta),
            )
            .into_any_element()
    }
}

fn eq_bar(
    id: &'static str,
    duration_ms: u64,
    min: f32,
    max: f32,
    color: gpui::Rgba,
) -> impl IntoElement {
    div()
        .w(gpui::px(3.0))
        .h(gpui::px(max))
        .rounded(gpui::px(999.0))
        .bg(color)
        .with_animation(
            id,
            gpui::Animation::new(std::time::Duration::from_millis(duration_ms))
                .repeat()
                .with_easing(gpui::bounce(gpui::linear)),
            move |el, delta| el.h(gpui::px(min + (max - min) * delta)),
        )
}

fn spawn_home_fetch(
    client: Arc<NavidromeClient>,
    hs: gpui::Entity<HomeState>,
    entity: gpui::Entity<Shell>,
    covers_dir: std::path::PathBuf,
    cx: &mut gpui::Context<Shell>,
) {
    cx.spawn(async move |_, cx| {
        cx.update(|app| {
            app.update_entity(&hs, |h, _| {
                h.loading = true;
            });
        });
        let client_bg = client.clone();
        let res = cx
            .background_executor()
            .spawn(async move {
                client_bg.album_list(crate::navidrome::client::AlbumListKind::Newest, 100, 0)
            })
            .await;
        let albums = match res {
            Ok(a) => a,
            Err(e) => {
                let entity = entity.clone();
                let msg = format!("Library error: {e:?}");
                cx.update(|app| {
                    app.update_entity(&hs, |h, _| {
                        h.loading = false;
                        h.error = Some(msg.clone());
                    });
                    entity.update(app, |shell, _cx| {
                        shell.toast = Some(msg);
                    });
                });
                return;
            }
        };
        cx.update(|app| {
            app.update_entity(&hs, |h, _| {
                h.loading = false;
                h.albums = albums.clone();
            });
        });
        let client_bg = client.clone();
        let artists = cx
            .background_executor()
            .spawn(async move { client_bg.artists() })
            .await
            .unwrap_or_default();
        let client_bg = client.clone();
        let playlists = cx
            .background_executor()
            .spawn(async move { client_bg.playlists() })
            .await
            .unwrap_or_default();
        let client_bg = client.clone();
        let favorites = cx
            .background_executor()
            .spawn(async move {
                client_bg.album_list(crate::navidrome::client::AlbumListKind::Starred, 100, 0)
            })
            .await
            .unwrap_or_default();
        cx.update(|app| {
            app.update_entity(&hs, |h, _| {
                h.artists = artists;
                h.playlists = playlists;
                h.favorites = favorites;
            });
        });
        for album in albums.iter().take(24) {
            if let Some(cover_id) = &album.cover_art_id {
                let cid_bg = cover_id.clone();
                let client_bg = client.clone();
                let fetched = cx
                    .background_executor()
                    .spawn(async move { client_bg.cover_bytes(&cid_bg, 512) })
                    .await;
                if let Ok(bytes) = fetched {
                    let format = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
                        gpui::ImageFormat::Png
                    } else {
                        gpui::ImageFormat::Jpeg
                    };
                    let image = std::sync::Arc::new(gpui::Image {
                        format,
                        bytes,
                        id: fxhash_id(cover_id),
                    });
                    let ext = if matches!(format, gpui::ImageFormat::Png) {
                        "png"
                    } else {
                        "jpg"
                    };
                    let _ = std::fs::create_dir_all(&covers_dir);
                    let _ =
                        std::fs::write(covers_dir.join(format!("{cover_id}.{ext}")), &image.bytes);
                    let cid = cover_id.clone();
                    cx.update(|app| {
                        app.update_entity(&hs, |h, _| {
                            h.insert_cover(cid, image);
                        });
                    });
                }
            }
        }

        if std::env::var("MYUZIK_AUTOPLAY").ok().as_deref() == Some("1")
            && let Some(first) = albums.first()
        {
            let client_bg = client.clone();
            let aid = first.id.clone();
            if let Ok(detail) = cx
                .background_executor()
                .spawn(async move { client_bg.album(&aid) })
                .await
            {
                let entity = entity.clone();
                cx.update(|app| {
                    entity.update(app, |shell, cx| {
                        shell.play_tracks(detail.songs, 0, cx);
                    });
                });
            }
        }
    })
    .detach();
}

fn settings_section(title: &str, t: &crate::theme::Colors) -> gpui::Div {
    div()
        .mt(gpui::px(6.0))
        .text_size(gpui::px(9.0))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(t.gold)
        .child(title.to_uppercase())
}

fn setting_row(label: &str, value: &str, t: &crate::theme::Colors) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .px(gpui::px(12.0))
        .py(gpui::px(9.0))
        .rounded(gpui::px(10.0))
        .bg(t.glass_tint)
        .border_1()
        .border_color(t.glass_border)
        .hover(|style| style.bg(t.glass_highlight_soft))
        .child(
            div()
                .text_size(gpui::px(12.0))
                .text_color(t.text_secondary)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_size(gpui::px(12.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(t.text_primary)
                .child(value.to_string()),
        )
}

fn nav_item(label: &str, active: bool, t: &crate::theme::Colors) -> gpui::Div {
    if active {
        gpui::div()
            .px(gpui::px(10.0))
            .py(gpui::px(7.0))
            .rounded(gpui::px(12.0))
            .bg(gpui::linear_gradient(
                145.0,
                gpui::linear_color_stop(t.clay_surface, 0.0),
                gpui::linear_color_stop(t.clay_shade, 1.0),
            ))
            .text_color(t.clay_ink)
            .text_size(gpui::px(13.0))
            .font_weight(gpui::FontWeight::BOLD)
            .shadow(vec![
                gpui::BoxShadow {
                    color: gpui::rgba(0xFFFFFFFF).into(),
                    offset: gpui::point(gpui::px(1.5), gpui::px(1.5)),
                    blur_radius: gpui::px(3.0),
                    spread_radius: gpui::px(0.0),
                    inset: true,
                },
                gpui::BoxShadow {
                    color: t.shadow_deep.into(),
                    offset: gpui::point(gpui::px(-2.0), gpui::px(-2.0)),
                    blur_radius: gpui::px(5.0),
                    spread_radius: gpui::px(0.0),
                    inset: true,
                },
            ])
            .child(label.to_string())
    } else {
        gpui::div()
            .px(gpui::px(10.0))
            .py(gpui::px(7.0))
            .rounded(gpui::px(12.0))
            .text_color(t.text_secondary)
            .text_size(gpui::px(13.0))
            .hover(|style| style.bg(t.glass_hover))
            .child(label.to_string())
    }
}

fn album_skeleton(index: usize, t: &crate::theme::Colors) -> gpui::Div {
    let pulse = div()
        .w_full()
        .aspect_ratio(1.0)
        .rounded(gpui::px(16.0))
        .bg(t.glass_tint)
        .border_1()
        .border_color(t.glass_border)
        .with_animation(
            format!("skeleton-{index}"),
            gpui::Animation::new(std::time::Duration::from_millis(900))
                .repeat()
                .with_easing(gpui::ease_in_out),
            |el, delta| el.opacity(0.35 + 0.45 * delta),
        );
    div()
        .flex()
        .flex_col()
        .gap(gpui::px(7.0))
        .w_full()
        .child(pulse)
        .child(
            div()
                .h(gpui::px(10.0))
                .w_full()
                .rounded(gpui::px(4.0))
                .bg(t.glass_tint),
        )
        .child(
            div()
                .h(gpui::px(8.0))
                .w(gpui::px(90.0))
                .rounded(gpui::px(4.0))
                .bg(t.glass_tint),
        )
}

fn album_card(
    title: &str,
    artist: &str,
    cover: Option<std::sync::Arc<gpui::Image>>,
    t: &crate::theme::Colors,
) -> gpui::Div {
    let initial = title
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());
    gpui::div()
        .flex()
        .flex_col()
        .gap(gpui::px(7.0))
        .w_full()
        .hover(|style| style.bg(t.glass_hover).rounded(gpui::px(16.0)))
        .child(
            gpui::div()
                .w_full()
                .aspect_ratio(1.0)
                .rounded(gpui::px(16.0))
                .bg(gpui::linear_gradient(
                    135.0,
                    gpui::linear_color_stop(t.gold_deep, 0.0),
                    gpui::linear_color_stop(t.clay_surface, 1.0),
                ))
                .border_1()
                .border_color(t.glass_border)
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0.05, 0.4, 0.05, 0.35),
                    offset: gpui::point(gpui::px(0.0), gpui::px(14.0)),
                    blur_radius: gpui::px(32.0),
                    spread_radius: gpui::px(0.0),
                    inset: false,
                }])
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .child(if let Some(image) = cover {
                    gpui::img(gpui::ImageSource::Image(image))
                        .w_full()
                        .h_full()
                        .rounded(gpui::px(16.0))
                        .object_fit(gpui::ObjectFit::Cover)
                        .into_any_element()
                } else {
                    gpui::div()
                        .font_family("Fraunces")
                        .text_size(gpui::px(54.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(t.clay_ink)
                        .opacity(0.85)
                        .child(initial)
                        .into_any_element()
                })
                .child(
                    gpui::div()
                        .absolute()
                        .right(gpui::px(8.0))
                        .bottom(gpui::px(8.0))
                        .w(gpui::px(33.0))
                        .h(gpui::px(33.0))
                        .rounded(gpui::px(999.0))
                        .bg(gpui::linear_gradient(
                            145.0,
                            gpui::linear_color_stop(t.gold_light, 0.0),
                            gpui::linear_color_stop(t.gold, 1.0),
                        ))
                        .text_color(t.clay_ink)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(gpui::px(11.0))
                        .child("▶"),
                ),
        )
        .child(
            gpui::div()
                .w_full()
                .text_size(gpui::px(12.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(t.text_primary)
                .child(title.to_string()),
        )
        .child(
            gpui::div()
                .w_full()
                .text_size(gpui::px(10.0))
                .text_color(t.text_secondary)
                .child(artist.to_string()),
        )
}

fn track_row(
    idx: &str,
    title: &str,
    artist: &str,
    dur: &str,
    active: bool,
    t: &crate::theme::Colors,
) -> gpui::Div {
    let bg = if active { t.gold_tint } else { t.glass_tint };
    let text_color = t.text_primary;
    gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(10.0))
        .px(gpui::px(10.0))
        .py(gpui::px(7.0))
        .rounded(gpui::px(12.0))
        .bg(bg)
        .hover(|style| style.bg(t.glass_highlight_soft))
        .border_1()
        .border_color(if active {
            t.gold_glow_border
        } else {
            t.glass_border
        })
        .child(
            gpui::div()
                .w(gpui::px(15.0))
                .text_size(gpui::px(9.0))
                .text_color(if active { t.gold } else { t.text_tertiary })
                .child(idx.to_string()),
        )
        .child(
            gpui::div()
                .w(gpui::px(34.0))
                .h(gpui::px(34.0))
                .rounded(gpui::px(9.0))
                .bg(t.glass_tint)
                .border_1()
                .border_color(t.glass_border),
        )
        .child(
            gpui::div()
                .flex_1()
                .flex()
                .flex_col()
                .child(
                    gpui::div()
                        .text_size(gpui::px(12.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(text_color)
                        .child(title.to_string()),
                )
                .child(
                    gpui::div()
                        .text_size(gpui::px(10.0))
                        .text_color(t.text_secondary)
                        .child(artist.to_string()),
                ),
        )
        .child(
            gpui::div()
                .text_size(gpui::px(9.0))
                .text_color(if active { t.gold } else { t.text_tertiary })
                .child(dur.to_string()),
        )
}

fn format_time(seconds: f64) -> String {
    let mins = (seconds / 60.0) as u64;
    let secs = (seconds % 60.0) as u64;
    format!("{}:{:02}", mins, secs)
}

/// Maximum number of decoded cover images kept in memory (`HomeState::covers`).
///
/// Exported so the stress test asserts the real bound instead of repeating it.
pub const COVER_CACHE_CAP: usize = 200;

#[derive(Clone, Default)]
pub struct HomeState {
    pub albums: Vec<crate::library::Album>,
    pub loading: bool,
    pub error: Option<String>,
    pub covers: std::collections::HashMap<String, std::sync::Arc<gpui::Image>>,
    pub cover_order: std::collections::VecDeque<String>,
    pub artists: Vec<crate::library::Artist>,
    pub playlists: Vec<crate::library::Playlist>,
    pub favorites: Vec<crate::library::Album>,
}

impl HomeState {
    /// Insert a cover, evicting the least recently added beyond [`COVER_CACHE_CAP`].
    pub fn insert_cover(&mut self, id: String, image: std::sync::Arc<gpui::Image>) {
        if self.covers.insert(id.clone(), image).is_none() {
            self.cover_order.push_back(id);
        } else {
            self.cover_order.retain(|existing| existing != &id);
            self.cover_order.push_back(id);
        }
        while self.cover_order.len() > COVER_CACHE_CAP {
            if let Some(old) = self.cover_order.pop_front() {
                self.covers.remove(&old);
            }
        }
    }
}

fn fxhash_id(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
