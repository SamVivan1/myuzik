use crate::app::{AppAction, QueueAction, Route, SourceId};
use crate::config::{AppConfig, Paths, YouTubeQuality};
use crate::library::TrackSource;
use crate::lyrics::provider::LyricsProvider;
use crate::navidrome::client::NavidromeClient;
use crate::platform::mpris::MprisHandle;
use crate::player::engine::{PlayerCommand, PlayerEvent, PlayerHandle};
use crate::player::state::{PlaybackStatus, PlayerState};
use crate::provider::YouTubeProvider;
use crate::services::{
    MusicService, NavidromeService, PlaybackSource, SourceRegistry, YouTubeService,
};
use crate::theme::midnight_brass;
use crate::ui::components::{album_card, format_time, nav_item, track_row};
use crate::ui::now_playing::NowPlayingView;
use gpui::{
    AnimationExt, Context, InteractiveElement, Render, StatefulInteractiveElement, Window, div,
    prelude::*,
};
use std::sync::Arc;

/// Short-lived YouTube query cache: the helper is a process spawn, so a burst
/// of identical searches should not pay for it twice.
mod browse;
mod queue;

type YouTubeSearchCache = Arc<
    std::sync::Mutex<
        std::collections::HashMap<String, (std::time::Instant, Vec<crate::library::Track>)>,
    >,
>;

#[allow(dead_code)]
pub struct Shell {
    player_state: gpui::Entity<PlayerState>,
    player_handle: PlayerHandle,
    config: AppConfig,
    paths: Paths,
    route: Route,
    /// Which source the sidebar is browsing. Playback and the queue are global.
    active_source: TrackSource,
    /// Dynamic accent extracted from the currently playing cover art.
    accent_color: gpui::Rgba,
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
    /// Queue drawer visibility. The queue itself lives in `PlayerState`.
    queue_panel_open: bool,
    lyrics_generation: Arc<std::sync::atomic::AtomicU64>,
    scrobble_enabled: Arc<std::sync::atomic::AtomicBool>,
    search_generation: Arc<std::sync::atomic::AtomicU64>,
    /// Optional YouTube Music source. `None` when the user turned it off.
    youtube: Option<Arc<YouTubeProvider>>,
    /// The UI-facing backend contract. Screens use this instead of reaching
    /// for Navidrome/YouTube clients directly.
    services: SourceRegistry,
    youtube_search_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    youtube_buffer_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    youtube_radio_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    youtube_generation: Arc<std::sync::atomic::AtomicU64>,
    youtube_radio_generation: Arc<std::sync::atomic::AtomicU64>,
    /// Short-lived cache for repeated queries; the helper is a process spawn,
    /// so a burst of identical searches should not pay for it twice.
    youtube_search_cache: YouTubeSearchCache,
    youtube_loading: bool,
    youtube_error: Option<String>,
    /// YouTube Music home shelves. Loaded on startup when YouTube is the
    /// active source; empty for a Navidrome-only install.
    youtube_home: Option<Vec<crate::provider::youtube::BrowseSection>>,
    youtube_home_loading: bool,
    youtube_home_error: Option<String>,
    /// Detail screens are shared by both sources; the route carries the source.
    playlist_detail: Option<crate::library::PlaylistDetail>,
    detail_loading: bool,
    detail_error: Option<String>,
    /// Mood/genre categories and the playlists for the selected one.
    mood_categories: Option<Vec<crate::provider::youtube::MoodCategory>>,
    mood_title: Option<String>,
    mood_playlists: Vec<crate::library::Playlist>,
    mood_loading: bool,
    /// Cached size of the stream cache, refreshed after a buffer or a clear.
    /// Rendering Settings must not walk the cache directory.
    youtube_cache_bytes: u64,
    /// On-disk snapshot of the library, so startup and search do not wait on the
    /// server. Audio is still streamed on demand; only metadata is cached.
    library_cache: crate::library::cache::LibraryCache,
    /// In-memory search over that snapshot: instant and works offline.
    library_index: Option<Arc<crate::library::search_index::SearchIndex>>,
    /// Set while a full sync runs, so a second click is a no-op.
    syncing: Arc<std::sync::atomic::AtomicBool>,
    /// Items fetched so far during a sync, for the progress label.
    sync_progress: Arc<std::sync::atomic::AtomicUsize>,
    /// Unix seconds of the snapshot currently in memory (None when live).
    library_snapshot_at: Option<u64>,
    /// Owns `<cache>/covers`: disk reads/writes, pruning, and the shared
    /// in-flight set that dedupes concurrent downloads of the same cover.
    cover_cache: Arc<crate::library::cover_cache::CoverCache>,
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
        mut config: AppConfig,
        paths: Paths,
        mpris_handle: Option<MprisHandle>,
    ) -> Self {
        let player_state = cx.new(|_cx| PlayerState::new());
        player_state.update(cx, |ps, _cx| ps.set_volume(config.volume));
        player_handle
            .tx
            .send(PlayerCommand::SetVolume(config.volume))
            .ok();
        // Load the cached library before the first frame: the lists render from
        // disk, then a background refresh replaces them. This is what makes
        // startup and search instant instead of waiting on the server.
        let library_cache = crate::library::cache::LibraryCache::new(paths.cache.clone());
        let cached_snapshot = match library_cache.load() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!("could not read the library cache: {error}");
                None
            }
        };
        let library_index = cached_snapshot.as_ref().map(|snapshot| {
            Arc::new(crate::library::search_index::SearchIndex::build(
                &snapshot.albums,
                &snapshot.artists,
                &snapshot.playlists,
                &snapshot.songs,
            ))
        });
        let cached_snapshot_at = cached_snapshot.as_ref().map(|snapshot| snapshot.fetched_at);
        let home_state = {
            let snapshot = cached_snapshot.clone();
            cx.new(move |_cx| {
                let mut state = HomeState::default();
                if let Some(snapshot) = snapshot {
                    state.albums = snapshot.albums;
                    state.artists = snapshot.artists;
                    state.playlists = snapshot.playlists;
                    state.from_cache = true;
                }
                state
            })
        };
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
        let cover_cache = Arc::new(crate::library::cover_cache::CoverCache::new(
            paths.covers.clone(),
        ));
        // Bound the artwork directory once per start. It used to grow forever:
        // nothing ever pruned it, so it kept every cover the app had ever seen.
        {
            let covers = cover_cache.clone();
            cx.spawn(async move |_, cx| {
                cx.background_executor()
                    .spawn(async move {
                        let report = covers.prune(
                            crate::library::cover_cache::MAX_COVER_FILES,
                            crate::library::cover_cache::MAX_COVER_BYTES,
                        );
                        if report.removed_files > 0 {
                            tracing::info!(
                                "pruned {} cached covers ({} bytes)",
                                report.removed_files,
                                report.removed_bytes
                            );
                        }
                    })
                    .await;
            })
            .detach();
        }
        // UI-13: `follow_os_accent` was a setting nothing ever read. Resolve the
        // desktop accent once, off the main thread (it shells out to gsettings).
        if config.follow_os_accent {
            cx.spawn(async move |_, cx| {
                let accent = cx
                    .background_executor()
                    .spawn(async move { crate::platform::accent::read_os_accent() })
                    .await;
                crate::platform::accent::set_current_accent(accent);
            })
            .detach();
        }
        // FL-19: stop the player thread on a normal quit. A dropped handle already
        // ends it, but an explicit shutdown releases the GStreamer pipeline and
        // the GLib loop instead of leaving them to process teardown.
        {
            let player = player_handle.clone();
            cx.on_app_quit(move |_this, _cx| {
                let player = player.clone();
                async move {
                    if !player.shutdown_and_join(std::time::Duration::from_secs(2)) {
                        tracing::warn!("the player thread did not stop within 2 s");
                    }
                }
            })
            .detach();
        }
        // Honour the stored accessibility preference from the first frame. The
        // Settings toggle used to be the only caller, so a user who had turned
        // reduced motion on got the animations back after every restart.
        cx.set_reduce_motion(config.reduce_motion);
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
        let lyrics_provider = Some(Arc::new(LyricsProvider::with_optional_client(
            client.as_ref().map(|client| (**client).clone()),
            crate::lyrics::cache::LyricsCache::new(paths.lyrics.clone()),
            config.prefer_navidrome_lyrics,
        )));
        let youtube = if config.youtube_enabled {
            let cookies = {
                let path = config.youtube_cookies.trim();
                (!path.is_empty()).then(|| std::path::PathBuf::from(path))
            };
            Some(Arc::new(YouTubeProvider::new(
                paths.cache.join("streams"),
                config.youtube_quality,
                cookies,
                config.stream_cache_mb,
            )))
        } else {
            None
        };
        let navidrome_service = client.as_ref().map(|client| {
            Arc::new(NavidromeService::new((**client).clone(), config.quality))
                as Arc<dyn MusicService>
        });
        let youtube_service = youtube.as_ref().map(|provider| {
            Arc::new(YouTubeService::new(provider.clone())) as Arc<dyn MusicService>
        });
        let services = SourceRegistry::new(navidrome_service, youtube_service);
        let youtube_cache_bytes = youtube
            .as_ref()
            .map(|provider| provider.cache_bytes())
            .unwrap_or(0);
        let tray_slot: Arc<std::sync::Mutex<Option<Arc<crate::platform::tray::Tray>>>> =
            Arc::new(std::sync::Mutex::new(None));
        let radio = Arc::new(std::sync::Mutex::new(crate::library::radio::Radio::new()));
        let autoplay_enabled = Arc::new(std::sync::atomic::AtomicBool::new(config.autoplay));
        let radio_in_flight = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let radio_continue_pending = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let self_weak = cx.entity().downgrade();
        let resolved_source = crate::app::resolve_active_source(
            config.active_source,
            client.is_some(),
            youtube.is_some(),
        );
        let active_source = resolved_source.unwrap_or(TrackSource::Navidrome);
        config.active_source = active_source;
        let accent_color = gpui::rgb(0x6F7CFF);
        let route = if resolved_source.is_some() {
            Route::Home
        } else {
            Route::Setup
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
                cover_cache.clone(),
                cx,
            );
        }
        if active_source == TrackSource::YouTube && youtube.is_some() {
            // YouTube is the remembered source: the first frame shows the
            // loading state, then this task swaps in the shelves. Library data
            // still loads in the background for the sidebar and search.
            let entity = self_entity.clone();
            cx.spawn(async move |_, cx| {
                cx.update(|app| {
                    entity.update(app, |shell, cx| shell.refresh_youtube_home(cx));
                });
            })
            .detach();
        }
        cx.spawn(async move |_, cx| {
            let mut scrobble = crate::player::scrobble::ScrobbleTracker::new();
            let mut last_pos = std::time::Duration::ZERO;
            let mut last_pos_at = std::time::Instant::now();
            let mut track_duration = std::time::Duration::ZERO;
            let mut scrobble_track: Option<String> = None;
            // FL-07: a failed stream used to leave the UI claiming it was playing.
            let retry_policy = crate::player::state::RetryPolicy::default();
            let mut retry_attempts: u8 = 0;
            let mut failure_guard = crate::player::state::StreamFailureGuard::default();
            loop {
                match rx.recv_async().await {
                    Ok(PlayerEvent::StateChanged(s)) => {
                        // FL-13: the engine posts a transient Stopped while a load
                        // is pending; filtering it here stops the icon flickering.
                        let effective = cx.update(|app| {
                            let effective = state.update(app, |ps, _cx| {
                                ps.set_status_from_engine(s);
                                ps.status
                            });
                            let playing = effective == PlaybackStatus::Playing;
                            let np = now_playing_for_loop.clone();
                            app.update_entity(&np, |view, _cx| {
                                view.set_playing(playing);
                            });
                            if let Some(tray) =
                                tray_slot_for_loop.lock().ok().and_then(|slot| slot.clone())
                            {
                                tray.set_playing(playing);
                            }
                            effective
                        });
                        if effective == PlaybackStatus::Playing {
                            retry_attempts = 0;
                            failure_guard.record_success();
                        }
                        // MPRIS has no Buffering state, so do not publish it.
                        if let Some(mh) = &mpris_for_loop
                            && effective != PlaybackStatus::Buffering
                        {
                            mh.update_status(effective);
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
                        // A position jump larger than the elapsed wall time is a
                        // seek; count what was actually heard instead of dropping
                        // the tick (which silently under-counted on slow machines).
                        let position_delta = pos.saturating_sub(last_pos);
                        let wall_delta = last_pos_at.elapsed();
                        let delta =
                            crate::player::scrobble::listened_delta(position_delta, wall_delta);
                        last_pos = pos;
                        last_pos_at = std::time::Instant::now();
                        if scrobble_enabled_for_loop.load(std::sync::atomic::Ordering::Relaxed)
                            && !delta.is_zero()
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
                                    submit_scrobble(&client, &id, true, now_ms);
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
                        cx.update(|app| {
                            state.update(app, |ps, _cx| ps.set_error());
                            let np = now_playing_for_loop.clone();
                            app.update_entity(&np, |view, _cx| view.set_playing(false));
                            if let Some(tray) =
                                tray_slot_for_loop.lock().ok().and_then(|slot| slot.clone())
                            {
                                tray.set_playing(false);
                            }
                        });
                        if let Some(mh) = &mpris_for_loop {
                            mh.update_status(PlaybackStatus::Error);
                        }
                        // The track that failed is not being listened to.
                        scrobble.reset();
                        last_pos = std::time::Duration::ZERO;
                        scrobble_track = None;

                        // Report it straight away: a silent retry leaves the
                        // user staring at a frozen player with no explanation.
                        let decision = retry_policy.decide(retry_attempts + 1);
                        let message = match decision {
                            crate::player::state::RetryDecision::Retry { delay } => {
                                retry_attempts = retry_attempts.saturating_add(1);
                                tracing::warn!(
                                    "retrying playback in {:?} (attempt {})",
                                    delay,
                                    retry_attempts
                                );
                                let entity = self_entity.clone();
                                cx.spawn(async move |cx| {
                                    cx.background_executor().timer(delay).await;
                                    cx.update(|app| {
                                        entity.update(app, |shell, cx| {
                                            shell.retry_current_track(cx);
                                        });
                                    });
                                })
                                .detach();
                                format!("Playback error: {e:?}. Retrying…")
                            }
                            crate::player::state::RetryDecision::GiveUp => {
                                retry_attempts = 0;
                                if failure_guard.record_failure() {
                                    // The whole queue is failing; stop here rather
                                    // than walking through it track by track.
                                    format!("Playback error: {e:?}. Playback stopped.")
                                } else {
                                    let entity = self_entity.clone();
                                    cx.update(|app| {
                                        entity.update(app, |shell, cx| {
                                            shell.play_next(cx);
                                        });
                                    });
                                    format!("Playback error: {e:?}. Skipping this track.")
                                }
                            }
                        };
                        cx.update(|app| {
                            entity.update(app, |shell, cx| {
                                shell.show_toast(message, cx);
                            });
                        });
                    }
                    Ok(PlayerEvent::TrackStarted(id)) => {
                        tracing::info!("track started: {}", id);
                        retry_attempts = 0;
                        scrobble.reset();
                        // The threshold is a fraction of the track duration, so a
                        // stale duration made the first seconds of a new track use
                        // the previous track's length.
                        track_duration = std::time::Duration::ZERO;
                        last_pos = std::time::Duration::ZERO;
                        last_pos_at = std::time::Instant::now();
                        let current_track = cx.update(|app| state.read(app).current.clone());
                        scrobble_track = current_track
                            .as_ref()
                            .filter(|track| track.source == TrackSource::Navidrome)
                            .map(|_| id.clone());
                        if let Some(mh) = &mpris_for_loop
                            && let Some(track) = current_track
                        {
                            let art = track.cover_key().and_then(|key| {
                                let jpg = covers_for_loop.join(format!("{key}.jpg"));
                                let png = covers_for_loop.join(format!("{key}.png"));
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
                        if scrobble_track.is_some()
                            && scrobble_enabled_for_loop.load(std::sync::atomic::Ordering::Relaxed)
                            && let Some(client) = &scrobble_client
                        {
                            let client = client.clone();
                            let now_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as i64)
                                .unwrap_or(0);
                            cx.background_executor()
                                .spawn(async move {
                                    submit_scrobble(&client, &id, false, now_ms);
                                })
                                .detach();
                        }
                    }
                    Ok(PlayerEvent::Buffering(p)) => {
                        let effective = cx.update(|app| {
                            state.update(app, |ps, _cx| {
                                ps.set_buffering(p);
                                ps.status
                            })
                        });
                        if let Some(tray) =
                            tray_slot_for_loop.lock().ok().and_then(|slot| slot.clone())
                        {
                            tray.set_playing(effective == PlaybackStatus::Playing);
                        }
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
            active_source,
            accent_color,
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
            queue_panel_open: false,
            lyrics_generation,
            scrobble_enabled,
            search_generation,
            youtube,
            services,
            youtube_search_cancel: None,
            youtube_buffer_cancel: None,
            youtube_radio_cancel: None,
            youtube_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            youtube_radio_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            youtube_search_cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            youtube_loading: false,
            youtube_error: None,
            youtube_home: None,
            youtube_home_loading: false,
            youtube_home_error: None,
            playlist_detail: None,
            detail_loading: false,
            detail_error: None,
            mood_categories: None,
            mood_title: None,
            mood_playlists: Vec::new(),
            mood_loading: false,
            youtube_cache_bytes,
            library_cache,
            library_index,
            syncing: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            sync_progress: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            library_snapshot_at: cached_snapshot_at,
            cover_cache,
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

    pub fn seek(&self, pos: std::time::Duration) {
        self.player_handle.tx.send(PlayerCommand::Seek(pos)).ok();
    }

    pub fn set_volume(&self, v: f32) {
        self.player_handle.tx.send(PlayerCommand::SetVolume(v)).ok();
    }

    fn navigate_nav(&mut self, route: Route, cx: &mut gpui::Context<Self>) {
        // A focus handle can outlive the menu item that created it. Do not let
        // Enter on a stale Library control open a page while YouTube is active.
        if matches!(
            route,
            Route::Albums | Route::Artists | Route::Playlists | Route::Favorites
        ) && self.active_source != TrackSource::Navidrome
        {
            return;
        }
        if route == Route::Moods && self.active_source != TrackSource::YouTube {
            return;
        }
        self.route = route;
        self.selected_album = None;
        self.album_detail = None;
        self.artist_detail = None;
        self.playlist_detail = None;
        self.detail_error = None;
        cx.notify();
    }

    #[allow(clippy::too_many_arguments)]
    fn nav_link(
        &mut self,
        key: &'static str,
        icon: &'static str,
        label: &'static str,
        active: bool,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
        t: &crate::theme::Colors,
    ) -> gpui::Stateful<gpui::Div> {
        let handle = self.focus_for(key, Self::TAB_NAV, cx);
        nav_item(icon, label, active, t)
            .id(key)
            .debug_selector(move || key.to_string())
            .track_focus(&handle)
            .tab_index(Self::TAB_NAV)
            .role(gpui::Role::Link)
            .aria_label(label)
            .aria_selected(active)
            .border_2()
            .border_color(self.focus_border(&handle, window, gpui::rgba(0x00000000), t))
            .active(|style| style.bg(t.clay_surface).text_color(t.clay_ink))
            .on_click(cx.listener(move |this, _event, window, cx| {
                this.activate_focused(key, window, cx);
            }))
    }

    fn source_pill(
        &mut self,
        source: TrackSource,
        label: &'static str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
        t: &crate::theme::Colors,
    ) -> gpui::Stateful<gpui::Div> {
        let key = match source {
            TrackSource::Navidrome => "source-library",
            TrackSource::YouTube => "source-youtube",
        };
        let active = self.active_source == source;
        let handle = self.focus_for(key, Self::TAB_NAV, cx);
        div()
            .id(key)
            .debug_selector(move || key.to_string())
            .track_focus(&handle)
            .tab_index(Self::TAB_NAV)
            .role(gpui::Role::Button)
            .aria_label(format!("Browse {}", label))
            .aria_selected(active)
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .py(gpui::px(5.0))
            .rounded(gpui::px(999.0))
            .bg(if active {
                t.clay_surface
            } else {
                gpui::rgba(0x00000000)
            })
            .text_color(if active { t.clay_ink } else { t.text_secondary })
            .text_size(gpui::px(10.0))
            .font_weight(gpui::FontWeight::BOLD)
            .border_2()
            .border_color(self.focus_border(&handle, window, gpui::rgba(0x00000000), t))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.set_active_source(source, cx);
            }))
            .child(label)
    }

    fn render_sidebar_nav(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
        t: &crate::theme::Colors,
    ) -> gpui::Div {
        let library_available = self.source_available(TrackSource::Navidrome);
        let youtube_available = self.source_available(TrackSource::YouTube);
        let mut column = div().flex().flex_col().gap(gpui::px(12.0));

        if library_available && youtube_available {
            column = column.child(
                div()
                    .flex()
                    .items_center()
                    .gap(gpui::px(2.0))
                    .p(gpui::px(3.0))
                    .rounded(gpui::px(999.0))
                    .bg(t.glass_tint)
                    .border_1()
                    .border_color(t.glass_border)
                    .child(self.source_pill(TrackSource::Navidrome, "Library", window, cx, t))
                    .child(self.source_pill(TrackSource::YouTube, "YouTube", window, cx, t)),
            );
        } else if youtube_available {
            column = column.child(
                div()
                    .px(gpui::px(10.0))
                    .py(gpui::px(6.0))
                    .rounded(gpui::px(999.0))
                    .bg(t.glass_tint)
                    .border_1()
                    .border_color(t.glass_border)
                    .text_size(gpui::px(9.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(t.text_tertiary)
                    .child("Source · YouTube Music"),
            );
        } else if library_available {
            column = column.child(
                div()
                    .px(gpui::px(10.0))
                    .py(gpui::px(6.0))
                    .rounded(gpui::px(999.0))
                    .bg(t.glass_tint)
                    .border_1()
                    .border_color(t.glass_border)
                    .text_size(gpui::px(9.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(t.text_tertiary)
                    .child("Source · Library"),
            );
        }

        let mut nav = div().flex().flex_col().gap(gpui::px(4.0));
        nav = nav.child(self.nav_link(
            "nav-home",
            "⌂",
            "Home",
            self.route == Route::Home,
            window,
            cx,
            t,
        ));
        nav = nav.child(self.nav_link(
            "nav-nowplaying",
            "♫",
            "Now Playing",
            self.route == Route::NowPlaying,
            window,
            cx,
            t,
        ));
        nav = nav.child(self.nav_link(
            "nav-search",
            "⌕",
            "Search",
            self.route == Route::Search,
            window,
            cx,
            t,
        ));
        if self.active_source == TrackSource::Navidrome {
            nav = nav.child(self.nav_link(
                "nav-albums",
                " ",
                "Albums",
                self.route == Route::Albums,
                window,
                cx,
                t,
            ));
            nav = nav.child(self.nav_link(
                "nav-artists",
                "◎",
                "Artists",
                self.route == Route::Artists,
                window,
                cx,
                t,
            ));
            nav = nav.child(self.nav_link(
                "nav-playlists",
                "≡",
                "Playlists",
                self.route == Route::Playlists,
                window,
                cx,
                t,
            ));
            nav = nav.child(self.nav_link(
                "nav-favorites",
                "★",
                "Favorites",
                self.route == Route::Favorites,
                window,
                cx,
                t,
            ));
        } else {
            nav = nav.child(self.nav_link(
                "nav-moods",
                "◐",
                "Moods & genres",
                self.route == Route::Moods,
                window,
                cx,
                t,
            ));
        }
        nav = nav.child(self.nav_link(
            "nav-settings",
            "⚙",
            "Settings",
            self.route == Route::Settings,
            window,
            cx,
            t,
        ));
        column.child(nav)
    }

    /// Run the action of whatever control currently has keyboard focus.
    ///
    /// gpui at this revision wires `on_click` to the mouse only, so a control
    /// that is merely focusable stays unreachable without a pointer: Tab could
    /// reach 25 controls and none of them would respond to Enter. Each arm here
    /// runs the same code its click handler runs, which is what makes the focus
    /// ring real instead of decoration (WCAG 2.1.1 Keyboard, level A).
    fn activate_focused(
        &mut self,
        key: &str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        match key {
            // Sidebar navigation.
            "nav-home" => self.navigate_nav(Route::Home, cx),
            "nav-nowplaying" => self.navigate_nav(Route::NowPlaying, cx),
            "nav-search" => {
                self.navigate_nav(Route::Search, cx);
                window.focus(&self.search_focus, cx);
            }
            "nav-albums" => self.navigate_nav(Route::Albums, cx),
            "nav-artists" => self.navigate_nav(Route::Artists, cx),
            "nav-playlists" => self.navigate_nav(Route::Playlists, cx),
            "nav-favorites" => self.navigate_nav(Route::Favorites, cx),
            "nav-moods" => self.dispatch(AppAction::OpenMoods, cx),
            "nav-settings" => self.navigate_nav(Route::Settings, cx),
            "source-library" => {
                self.set_active_source(TrackSource::Navidrome, cx);
            }
            "source-youtube" => {
                self.set_active_source(TrackSource::YouTube, cx);
            }
            "sidebar-pin-lyrics" | "player-pin-lyrics" => self.toggle_overlay(cx),
            // Page content.
            "content-search-shortcut" => self.go(Route::Search),
            "content-see-all-albums" => self.go(Route::Albums),
            "content-sync" => self.sync_library(cx),
            "content-library-retry" => self.retry_home_fetch(cx),
            "album-back" => self.go(Route::Home),
            "artists-back" => self.go(Route::Artists),
            "album-play-all" => {
                if let Some(detail) = self.album_detail.clone() {
                    self.play_tracks(detail.songs, 0, cx);
                }
            }
            "album-enqueue" => {
                if let Some(detail) = self.album_detail.clone() {
                    self.enqueue_tracks(detail.songs, cx);
                }
            }
            "album-star" => self.toggle_album_star(cx),
            // Player bar.
            "player-now-playing" => self.go(Route::NowPlaying),
            "player-play" => {
                self.player_state.update(cx, |ps, _cx| ps.toggle());
                self.player_handle.tx.send(PlayerCommand::Toggle).ok();
            }
            "player-shuffle" => self.toggle_shuffle(cx),
            "player-repeat" => self.toggle_repeat(cx),
            "player-queue" => self.dispatch(AppAction::Queue(QueueAction::TogglePanel), cx),
            "queue-close" => self.dispatch(AppAction::Queue(QueueAction::TogglePanel), cx),
            "queue-clear-upcoming" => {
                self.dispatch(AppAction::Queue(QueueAction::ClearUpcoming), cx)
            }
            "player-prev" => self.play_previous(cx),
            "player-next" => self.play_next(cx),
            "player-seek-back" => {
                let pos = self
                    .player_state
                    .read(cx)
                    .position
                    .saturating_sub(std::time::Duration::from_secs(10));
                self.player_handle.tx.send(PlayerCommand::Seek(pos)).ok();
            }
            "player-seek-forward" => {
                let pos = self
                    .player_state
                    .read(cx)
                    .position
                    .saturating_add(std::time::Duration::from_secs(10));
                self.player_handle.tx.send(PlayerCommand::Seek(pos)).ok();
            }
            "player-volume-down" => self.adjust_volume(-0.05, cx),
            "player-volume-up" => self.adjust_volume(0.05, cx),
            // Settings rows.
            "settings-quality" => self.cycle_quality(),
            "settings-youtube" => self.toggle_youtube(cx),
            "settings-youtube-quality" => self.cycle_youtube_quality(cx),
            "settings-youtube-cache" => self.clear_youtube_cache(cx),
            "settings-server" => {
                self.route = Route::Setup;
                self.setup_error = None;
                self.setup_focus_requested = false;
            }
            "search-youtube-retry" => self.schedule_search(true, cx),
            "youtube-moods" => self.open_moods(cx),
            "youtube-home-retry" => self.refresh_youtube_home(cx),
            "detail-back" => self.detail_back(cx),
            "detail-play-all" => {
                if let Some(detail) = self.album_detail.clone() {
                    self.play_tracks(detail.songs, 0, cx);
                }
            }
            "detail-enqueue" => {
                if let Some(detail) = self.album_detail.clone() {
                    self.enqueue_tracks(detail.songs, cx);
                }
            }
            "detail-star" => self.toggle_album_star(cx),
            "playlist-play-all" => {
                if let Some(detail) = self.playlist_detail.clone() {
                    self.play_tracks(detail.songs, 0, cx);
                }
            }
            "playlist-enqueue" => {
                if let Some(detail) = self.playlist_detail.clone() {
                    self.enqueue_tracks(detail.songs, cx);
                }
            }
            "setup-youtube" => self.use_youtube_setup(cx),
            "settings-scrobble" => self.toggle_scrobble(),
            "settings-clear-cache" => self.clear_library_cache(cx),
            "settings-shuffle" => self.toggle_shuffle(cx),
            "settings-repeat" => self.toggle_repeat(cx),
            "settings-reduce-motion" => self.toggle_reduce_motion(cx),
            "settings-close-to-tray" => self.toggle_close_to_tray(),
            "settings-autoplay" => self.toggle_autoplay(),
            "settings-lyrics-source" => self.toggle_lyrics_source(),
            "settings-offset-up" => self.adjust_lyrics_offset(500, cx),
            "settings-offset-down" => self.adjust_lyrics_offset(-500, cx),
            // Window chrome.
            "window-close" => self.close_window(window, cx),
            "window-minimize" => window.minimize_window(),
            "window-maximize" => window.zoom_window(),
            // Dynamic content keys: `album:<id>`, `artist:<id>`,
            // `playlist:<id>` and `track-<index>`.
            other => {
                if let Some(id) = other.strip_prefix("album:") {
                    self.open_album(source_for_id(id), id.to_string(), cx);
                } else if let Some(id) = other.strip_prefix("artist:") {
                    self.open_artist(source_for_id(id), id.to_string(), cx);
                } else if let Some(id) = other.strip_prefix("playlist:") {
                    self.open_playlist(source_for_id(id), id.to_string(), cx);
                } else if let Some(index) = other
                    .strip_prefix("track-")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    if let Some(detail) = self.album_detail.clone() {
                        self.play_tracks(detail.songs, index, cx);
                    }
                } else if let Some(index) = other
                    .strip_prefix("search-song-")
                    .and_then(|i| i.parse::<usize>().ok())
                    && let Some(songs) = self.search_results.as_ref().map(|r| r.songs.clone())
                    && index < songs.len()
                {
                    self.play_tracks(songs, index, cx);
                } else if let Some(index) = other
                    .strip_prefix("detail-track-")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    if let Some(detail) = self.album_detail.clone() {
                        self.play_tracks(detail.songs, index, cx);
                    }
                } else if let Some(index) = other
                    .strip_prefix("detail-next-")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    if let Some(track) = self
                        .album_detail
                        .as_ref()
                        .and_then(|detail| detail.songs.get(index))
                        .cloned()
                    {
                        self.enqueue_next(track, cx);
                    }
                } else if let Some(index) = other
                    .strip_prefix("detail-add-")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    if let Some(track) = self
                        .album_detail
                        .as_ref()
                        .and_then(|detail| detail.songs.get(index))
                        .cloned()
                    {
                        self.enqueue_tracks(vec![track], cx);
                    }
                } else if let Some(index) = other
                    .strip_prefix("playlist-next-")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    if let Some(track) = self
                        .playlist_detail
                        .as_ref()
                        .and_then(|detail| detail.songs.get(index))
                        .cloned()
                    {
                        self.enqueue_next(track, cx);
                    }
                } else if let Some(index) = other
                    .strip_prefix("playlist-add-")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    if let Some(track) = self
                        .playlist_detail
                        .as_ref()
                        .and_then(|detail| detail.songs.get(index))
                        .cloned()
                    {
                        self.enqueue_tracks(vec![track], cx);
                    }
                } else if let Some(index) = other
                    .strip_prefix("playlist-track-")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    if let Some(detail) = self.playlist_detail.clone() {
                        self.play_tracks(detail.songs, index, cx);
                    }
                } else if let Some(index) = other
                    .strip_prefix("mood-playlist-")
                    .and_then(|i| i.parse::<usize>().ok())
                    && let Some(playlist) = self.mood_playlists.get(index).cloned()
                {
                    self.open_playlist(TrackSource::YouTube, playlist.id, cx);
                } else if let Some(index) = other
                    .strip_prefix("mood-")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    let mood = self.mood_categories.as_ref().and_then(|categories| {
                        categories
                            .iter()
                            .flat_map(|category| category.moods.iter())
                            .nth(index)
                            .cloned()
                    });
                    if let Some(mood) = mood {
                        self.open_mood(mood.title, mood.params, cx);
                    }
                } else if let Some(key) = other.strip_prefix("yt-card-")
                    && let Some((section, index)) = key.split_once('-')
                    && let (Ok(section), Ok(index)) =
                        (section.parse::<usize>(), index.parse::<usize>())
                {
                    let card = self
                        .youtube_home
                        .as_ref()
                        .and_then(|sections| sections.get(section))
                        .and_then(|section| section.items.get(index))
                        .cloned();
                    if let Some(card) = card {
                        use crate::provider::youtube::BrowseCard;
                        match card {
                            BrowseCard::Song(track) => self.play_youtube_result(track, cx),
                            BrowseCard::Album(album) => {
                                self.open_album(TrackSource::YouTube, album.id, cx)
                            }
                            BrowseCard::Playlist(playlist) => {
                                self.open_playlist(TrackSource::YouTube, playlist.id, cx)
                            }
                            BrowseCard::Artist(artist) => {
                                self.open_artist(TrackSource::YouTube, artist.id, cx)
                            }
                        }
                    }
                } else if let Some(index) = other
                    .strip_prefix("queue-play-")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    self.dispatch(AppAction::Queue(QueueAction::PlayAt(index)), cx);
                } else if let Some(index) = other
                    .strip_prefix("queue-up-")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    if index > 0 {
                        self.dispatch(
                            AppAction::Queue(QueueAction::Move {
                                from: index,
                                to: index - 1,
                            }),
                            cx,
                        );
                    }
                } else if let Some(index) = other
                    .strip_prefix("queue-down-")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    let len = self.player_state.read(cx).queue.len();
                    if index + 1 < len {
                        self.dispatch(
                            AppAction::Queue(QueueAction::Move {
                                from: index,
                                to: index + 1,
                            }),
                            cx,
                        );
                    }
                } else if let Some(index) = other
                    .strip_prefix("queue-remove-")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    self.dispatch(AppAction::Queue(QueueAction::Remove(index)), cx);
                } else if let Some(index) = other
                    .strip_prefix("search-youtube-")
                    .and_then(|i| i.parse::<usize>().ok())
                    && let Some(track) = self
                        .search_results
                        .as_ref()
                        .and_then(|results| results.youtube.get(index))
                        .cloned()
                {
                    self.play_youtube_result(track, cx);
                } else if let Some(index) = other
                    .strip_prefix("search-album-")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    if let Some((source, id)) = self
                        .search_results
                        .as_ref()
                        .and_then(|r| r.albums.get(index))
                        .map(|a| (a.source, a.id.clone()))
                    {
                        self.open_album(source, id, cx);
                    }
                } else if let Some(index) = other
                    .strip_prefix("search-artist-")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    if let Some((source, id)) = self
                        .search_results
                        .as_ref()
                        .and_then(|r| r.artists.get(index))
                        .map(|a| (a.source, a.id.clone()))
                    {
                        self.open_artist(source, id, cx);
                    }
                } else if let Some(index) = other
                    .strip_prefix("search-playlist-")
                    .and_then(|i| i.parse::<usize>().ok())
                    && let Some((source, id)) = self
                        .search_results
                        .as_ref()
                        .and_then(|r| r.playlists.get(index))
                        .map(|p| (p.source, p.id.clone()))
                {
                    self.play_playlist(source, id, cx);
                }
            }
        }
        cx.notify();
    }

    /// Reload the current track after a failed stream.
    ///
    /// `adopt` keeps the queue position and marks the load pending, which is what
    /// makes the next transient `Stopped` from the engine filtered out.
    fn retry_current_track(&mut self, cx: &mut gpui::App) {
        let Some(track) = self.player_state.read(cx).current.clone() else {
            return;
        };
        self.player_state
            .update(cx, |ps, _cx| ps.adopt(track.clone()));
        self.load_and_play(&track, cx);
    }

    /// Tell the user something that happened before a window existed.
    ///
    /// A corrupt `config.toml` is moved aside and defaults are loaded; without
    /// this the recovery is only a log line nobody reads.
    pub fn notice(&mut self, message: impl Into<String>, cx: &mut gpui::Context<Self>) {
        self.show_toast(message, cx);
    }

    /// Throw away the cached library and its artwork.
    ///
    /// The snapshot and the search index are dropped immediately; covers are
    /// pruned to nothing so a corrupted or stale set cannot survive. Both are
    /// re-downloadable, so this is safe — it only costs one Sync.
    pub fn clear_library_cache(&mut self, cx: &mut gpui::Context<Self>) {
        let cache = crate::library::cache::LibraryCache::new(self.paths.cache.clone());
        let covers = self.cover_cache.clone();
        match cache.clear() {
            Ok(()) => {
                self.library_index = None;
                self.library_snapshot_at = None;
                self.home_state.update(cx, |state, cx| {
                    state.albums.clear();
                    state.artists.clear();
                    state.playlists.clear();
                    state.favorites.clear();
                    state.covers.clear();
                    state.cover_order.clear();
                    state.from_cache = false;
                    state.error = None;
                    cx.notify();
                });
                let report = covers.prune(0, 0);
                self.show_toast(
                    format!(
                        "Cache cleared ({} covers removed). Press Sync to fill it again.",
                        report.removed_files
                    ),
                    cx,
                );
            }
            Err(error) => {
                let message = format!("Could not clear the cache: {error}");
                self.show_toast(message, cx);
            }
        }
    }

    /// A short, honest line about where the library came from.
    ///
    /// Replaces the old "100 albums loaded" subtitle, which read like a debug
    /// build and said nothing the grid did not already show.
    pub fn library_status(&self, cx: &gpui::App) -> Option<String> {
        if self.syncing.load(std::sync::atomic::Ordering::Relaxed) {
            let done = self
                .sync_progress
                .load(std::sync::atomic::Ordering::Relaxed);
            return Some(if done == 0 {
                "Syncing…".to_string()
            } else {
                format!("Syncing… {done} albums")
            });
        }
        if self.home_state.read(cx).error.is_some() {
            return None;
        }
        let snapshot_at = self.library_snapshot_at?;
        if !self.home_state.read(cx).from_cache {
            return None;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Some(format!(
            "Offline copy · synced {}",
            humanise_age(now.saturating_sub(snapshot_at))
        ))
    }

    /// The library load error, for the UI and for tests.
    pub fn home_error(&self, cx: &gpui::App) -> Option<String> {
        self.home_state.read(cx).error.clone()
    }

    /// Try the library load again after a failure.
    ///
    /// docs/design.md:463 asks for an error with exactly one action; this is it.
    pub fn retry_home_fetch(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(client) = self.client.clone() else {
            self.show_toast("Connect a server first.", cx);
            return;
        };
        self.home_state.update(cx, |state, cx| {
            state.error = None;
            state.loading = true;
            cx.notify();
        });
        spawn_home_fetch(
            client,
            self.home_state.clone(),
            cx.entity(),
            self.cover_cache.clone(),
            cx,
        );
    }

    /// Whether shuffle is on.
    pub fn shuffle_on(&self, cx: &gpui::App) -> bool {
        self.player_state.read(cx).queue.shuffle()
    }

    /// The repeat button's label, which is also its state readout.
    pub fn repeat_label(&self, cx: &gpui::App) -> &'static str {
        match self.player_state.read(cx).queue.repeat() {
            crate::player::queue::RepeatMode::Off => "Repeat: off",
            crate::player::queue::RepeatMode::All => "Repeat: all",
            crate::player::queue::RepeatMode::One => "Repeat: one",
        }
    }

    /// Toggle shuffle. The queue already supported it; nothing in the UI reached
    /// it, so the feature was unreachable.
    pub fn toggle_shuffle(&mut self, cx: &mut gpui::Context<Self>) {
        let on = !self.player_state.read(cx).queue.shuffle();
        self.player_state.update(cx, |ps, _cx| ps.set_shuffle(on));
        self.sync_mpris_queue(cx);
        cx.notify();
    }

    /// Cycle repeat off -> all -> one -> off.
    pub fn toggle_repeat(&mut self, cx: &mut gpui::Context<Self>) {
        use crate::player::queue::RepeatMode;
        let next = match self.player_state.read(cx).queue.repeat() {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
        self.player_state.update(cx, |ps, _cx| ps.set_repeat(next));
        self.sync_mpris_queue(cx);
        cx.notify();
    }

    /// Push the queue to MPRIS so it can advertise LoopStatus, Shuffle and
    /// whether Next/Previous are usable.
    ///
    /// Without this the media widget shows Next/Previous permanently disabled:
    /// MPRIS cannot guess a queue it was never told about.
    fn sync_mpris_queue(&self, cx: &mut gpui::App) {
        if let Some(mh) = &self.mpris_handle {
            let queue = self.player_state.read(cx).queue.clone();
            mh.update_queue(&queue);
        }
    }

    /// Insert one track immediately after the current one.
    pub fn enqueue_next(&mut self, track: crate::library::Track, cx: &mut gpui::App) {
        let title = track.title.clone();
        self.player_state.update(cx, |ps, _cx| {
            ps.queue.play_next(track);
            ps.queue.trim_front_to(crate::player::queue::MAX_QUEUE);
        });
        self.sync_mpris_queue(cx);
        self.toast_from_app(format!("Playing {title} next."), cx);
    }

    /// Append a collection to the queue without disturbing playback.
    pub fn enqueue_tracks(&mut self, tracks: Vec<crate::library::Track>, cx: &mut gpui::App) {
        if tracks.is_empty() {
            return;
        }
        let count = tracks.len();
        self.player_state.update(cx, |ps, _cx| {
            ps.queue.append(tracks);
            ps.queue.trim_front_to(crate::player::queue::MAX_QUEUE);
        });
        self.sync_mpris_queue(cx);
        self.toast_from_app(
            format!(
                "Added {count} track{} to the queue.",
                if count == 1 { "" } else { "s" }
            ),
            cx,
        );
    }

    /// Nudge the volume by `delta`, keeping state, engine and MPRIS in step.
    fn adjust_volume(&mut self, delta: f32, cx: &mut gpui::Context<Self>) {
        let v = (self.player_state.read(cx).volume + delta).clamp(0.0, 1.0);
        self.player_state.update(cx, |ps, _cx| ps.set_volume(v));
        self.player_handle.tx.send(PlayerCommand::SetVolume(v)).ok();
        if let Some(mh) = &self.mpris_handle {
            mh.update_volume(v);
        }
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
            self.lyrics_provider = Some(Arc::new(LyricsProvider::with_optional_client(
                self.client.as_ref().map(|client| (**client).clone()),
                crate::lyrics::cache::LyricsCache::new(self.paths.lyrics.clone()),
                cfg.prefer_navidrome_lyrics,
            )));
            self.youtube = if cfg.youtube_enabled {
                let cookies = {
                    let path = cfg.youtube_cookies.trim();
                    (!path.is_empty()).then(|| std::path::PathBuf::from(path))
                };
                Some(Arc::new(YouTubeProvider::new(
                    self.paths.cache.join("streams"),
                    cfg.youtube_quality,
                    cookies,
                    cfg.stream_cache_mb,
                )))
            } else {
                None
            };
            self.youtube_cache_bytes = self
                .youtube
                .as_ref()
                .map(|provider| provider.cache_bytes())
                .unwrap_or(0);
            self.rebuild_services();
            self.route = if self.client.is_none() && cfg.youtube_enabled {
                Route::Search
            } else if self.client.is_none() {
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
                                Err(e) => h.error = Some(error_text(&e)),
                            }
                        });
                    });
                })
                .detach();
            }
        }
    }

    /// Pull the whole library from the server into the on-disk snapshot.
    ///
    /// This is the one explicit "sync" action: it walks every album page, then
    /// every album's tracks, saves one JSON snapshot and rebuilds the local
    /// search index. Audio is never downloaded — only metadata, so playback
    /// still streams on demand when a track is chosen.
    pub fn sync_library(&mut self, cx: &mut gpui::Context<Self>) {
        use std::sync::atomic::Ordering;
        if self.syncing.swap(true, Ordering::SeqCst) {
            self.show_toast("A sync is already running.", cx);
            return;
        }
        let Some(client) = self.client.clone() else {
            self.syncing.store(false, Ordering::SeqCst);
            self.show_toast("Connect a server first.", cx);
            return;
        };
        self.sync_progress.store(0, Ordering::SeqCst);
        self.show_toast("Syncing library…", cx);

        let entity = cx.entity();
        let cache_dir = self.paths.cache.clone();
        let syncing = self.syncing.clone();
        let progress = self.sync_progress.clone();

        // Keep the status line moving while a long sync runs: the fetch cannot
        // notify the UI itself (it runs on the background executor).
        {
            let syncing_tick = self.syncing.clone();
            let weak_tick = cx.entity().downgrade();
            cx.spawn(async move |_, cx| {
                while syncing_tick.load(std::sync::atomic::Ordering::Relaxed) {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(500))
                        .await;
                    if weak_tick.upgrade().is_none() {
                        return;
                    }
                    let alive =
                        cx.update(|app| weak_tick.update(app, |_shell, cx| cx.notify()).is_ok());
                    if !alive {
                        return;
                    }
                }
            })
            .detach();
        }
        cx.spawn(async move |_, cx| {
            let progress_bg = progress.clone();
            let result = cx
                .background_executor()
                .spawn(async move { fetch_full_library(&client, &progress_bg) })
                .await;

            match result {
                Ok(snapshot) => {
                    let save = crate::library::cache::LibraryCache::new(cache_dir).save(&snapshot);
                    let index = Arc::new(crate::library::search_index::SearchIndex::build(
                        &snapshot.albums,
                        &snapshot.artists,
                        &snapshot.playlists,
                        &snapshot.songs,
                    ));
                    let summary = format!(
                        "Synced {} albums · {} artists · {} songs.",
                        snapshot.albums.len(),
                        snapshot.artists.len(),
                        snapshot.songs.len()
                    );
                    cx.update(|app| {
                        entity.update(app, |shell, cx| {
                            syncing.store(false, Ordering::SeqCst);
                            shell.library_index = Some(index);
                            shell.library_snapshot_at = Some(snapshot.fetched_at);
                            let albums = snapshot.albums.clone();
                            let artists = snapshot.artists.clone();
                            let playlists = snapshot.playlists.clone();
                            shell.home_state.update(cx, |state, cx| {
                                state.albums = albums;
                                state.artists = artists;
                                state.playlists = playlists;
                                state.from_cache = false;
                                state.loading = false;
                                state.error = None;
                                cx.notify();
                            });
                            match save {
                                Ok(()) => shell.show_toast(summary, cx),
                                Err(error) => shell.show_toast(
                                    format!("{summary} Could not write the cache: {error}"),
                                    cx,
                                ),
                            }
                        });
                    });
                }
                Err(error) => {
                    let message = format!("Sync failed. {}", error_text(&error));
                    cx.update(|app| {
                        entity.update(app, |shell, cx| {
                            syncing.store(false, Ordering::SeqCst);
                            shell.show_toast(message, cx);
                        });
                    });
                }
            }
        })
        .detach();
    }

    /// Run a search for the current query.
    ///
    /// Typing used to do nothing until Enter was pressed, while the field
    /// promised "Type to search". A short debounce coalesces keystrokes, and the
    /// generation counter still discards out-of-order responses.
    ///
    /// Search follows the sidebar's active source. Library results are local
    /// or Navidrome; YouTube results use the one-shot helper. The two are never
    /// merged, so switching source cannot leave stale results from the other.
    fn schedule_search(&mut self, immediate: bool, cx: &mut gpui::Context<Self>) {
        use std::sync::atomic::Ordering;

        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            self.search_generation.fetch_add(1, Ordering::SeqCst);
            self.cancel_youtube_search();
            self.search_results = None;
            self.youtube_loading = false;
            self.youtube_error = None;
            cx.notify();
            return;
        }

        let generation = self.search_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.cancel_youtube_search();

        // Search only the source the sidebar is browsing. Switching source
        // clears the other source's results, so no stale mix can reappear.
        if self.active_source == TrackSource::Navidrome {
            self.youtube_loading = false;
            self.youtube_error = None;
            // Local results immediately. The server is only asked when no
            // snapshot exists yet (first run, before the first sync).
            if let Some(index) = &self.library_index
                && !index.is_empty()
            {
                self.search_results = Some(index.search(&query));
                cx.notify();
            } else if let Some(client) = self.client.clone() {
                let counter = self.search_generation.clone();
                let entity = cx.entity();
                let query_bg = query.clone();
                cx.spawn(async move |_, cx| {
                    if !immediate {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(250))
                            .await;
                        if counter.load(Ordering::SeqCst) != generation {
                            return;
                        }
                    }
                    let request = client.clone();
                    let res = cx
                        .background_executor()
                        .spawn(async move { request.search(&query_bg) })
                        .await;
                    if counter.load(Ordering::SeqCst) != generation {
                        return;
                    }
                    cx.update(|app| {
                        app.update_entity(&entity, |shell, cx| {
                            if let Ok(results) = res {
                                shell.search_results = Some(results);
                                cx.notify();
                            }
                        });
                    });
                })
                .detach();
            }

            return;
        }

        // YouTube only: do not mix library groups into the results.
        self.search_results = None;

        // YouTube Music. The helper is a process spawn, so debounce more than
        // the local path and reuse a very recent identical query.
        let Some(provider) = self.youtube.clone() else {
            self.youtube_loading = false;
            self.youtube_error = None;
            cx.notify();
            return;
        };
        if let Ok(cache) = self.youtube_search_cache.lock()
            && let Some((cached_at, cached)) = cache.get(&query)
            && cached_at.elapsed() < std::time::Duration::from_secs(60)
        {
            let cached = cached.clone();
            if let Some(results) = &mut self.search_results {
                results.youtube = cached;
            } else {
                self.search_results = Some(crate::library::SearchResults {
                    youtube: cached,
                    ..Default::default()
                });
            }
            self.youtube_loading = false;
            self.youtube_error = None;
            cx.notify();
            return;
        }

        self.youtube_loading = true;
        self.youtube_error = None;
        cx.notify();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.youtube_search_cancel = Some(cancel.clone());
        let counter = self.search_generation.clone();
        let cache = self.youtube_search_cache.clone();
        let entity = cx.entity();
        let query_bg = query.clone();
        cx.spawn(async move |_, cx| {
            if !immediate {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(350))
                    .await;
                if counter.load(Ordering::SeqCst) != generation || cancel.load(Ordering::Relaxed) {
                    return;
                }
            }
            let provider_bg = provider.clone();
            let query_for_helper = query_bg.clone();
            let cancel_bg = cancel.clone();
            let res = cx
                .background_executor()
                .spawn(async move { provider_bg.search(&query_for_helper, 20, &cancel_bg) })
                .await;
            if counter.load(Ordering::SeqCst) != generation || cancel.load(Ordering::Relaxed) {
                return;
            }
            cx.update(|app| {
                app.update_entity(&entity, |shell, cx| {
                    shell.youtube_loading = false;
                    match res {
                        Ok(tracks) => {
                            shell.youtube_error = None;
                            if let Ok(mut cache) = cache.lock() {
                                cache.insert(
                                    query_bg.clone(),
                                    (std::time::Instant::now(), tracks.clone()),
                                );
                                if cache.len() > 32 {
                                    let oldest = cache
                                        .iter()
                                        .min_by_key(|(_, (at, _))| *at)
                                        .map(|(key, _)| key.clone());
                                    if let Some(key) = oldest {
                                        cache.remove(&key);
                                    }
                                }
                            }
                            if let Some(results) = &mut shell.search_results {
                                results.youtube = tracks;
                            } else {
                                shell.search_results = Some(crate::library::SearchResults {
                                    youtube: tracks,
                                    ..Default::default()
                                });
                            }
                        }
                        Err(crate::provider::ProviderError::Cancelled) => {}
                        Err(error) => {
                            shell.youtube_error = Some(provider_search_error_text(&error));
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn cancel_youtube_search(&mut self) {
        if let Some(cancel) = self.youtube_search_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn play_track(&mut self, track: crate::library::Track, cx: &mut gpui::App) {
        if track.is_youtube() {
            self.play_youtube_result(track, cx);
            return;
        }
        self.cancel_youtube_radio();
        self.player_state.update(cx, |ps, _cx| {
            ps.play_now(vec![track.clone()], 0);
        });
        self.sync_mpris_queue(cx);
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
        self.cancel_youtube_radio();
        if tracks.len() == 1 && tracks[0].is_youtube() {
            self.play_youtube_result(tracks[0].clone(), cx);
            return;
        }
        self.player_state.update(cx, |ps, _cx| {
            ps.play_now(tracks.clone(), start);
        });
        self.sync_mpris_queue(cx);
        self.radio.lock().unwrap_or_else(|e| e.into_inner()).reset();
        self.load_and_play(&tracks[start], cx);
    }

    /// Play one YouTube search result and queue its YouTube Music radio.
    ///
    /// The real client does not put the whole search page into the queue. A
    /// song opens with its watch playlist/radio: the song plus related tracks
    /// chosen by YouTube Music. AudioTube's `UserPlaylistModel` does the same
    /// through `get_watch_playlist`; Sung exposes it as the `radio` op.
    fn play_youtube_result(&mut self, track: crate::library::Track, cx: &mut gpui::App) {
        self.cancel_youtube_radio();
        self.player_state.update(cx, |ps, _cx| {
            ps.play_now(vec![track.clone()], 0);
        });
        self.sync_mpris_queue(cx);
        self.radio.lock().unwrap_or_else(|e| e.into_inner()).reset();
        self.load_and_play(&track, cx);
        self.fetch_youtube_radio(track, false, cx);
    }

    fn cancel_youtube_radio(&mut self) {
        if let Some(cancel) = self.youtube_radio_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.youtube_radio_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// Fetch the radio/watch playlist for `seed` and append it to the queue.
    ///
    /// `continue_playback` is used when the queue has just run out: the first
    /// new track is adopted and loaded, instead of only being appended.
    fn fetch_youtube_radio(
        &mut self,
        seed: crate::library::Track,
        continue_playback: bool,
        cx: &mut gpui::App,
    ) {
        use std::sync::atomic::Ordering;

        let Some(provider) = self.youtube.clone() else {
            return;
        };
        if let Some(cancel) = self.youtube_radio_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.youtube_radio_cancel = Some(cancel.clone());
        let generation = self.youtube_radio_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let counter = self.youtube_radio_generation.clone();
        let entity = self.self_weak.clone();
        let seed_id = seed.id.clone();
        let seed_for_fetch = seed.clone();
        cx.spawn(async move |cx| {
            let provider_bg = provider.clone();
            let cancel_bg = cancel.clone();
            let result = cx
                .background_executor()
                .spawn(async move { provider_bg.radio(&seed_for_fetch, 30, &cancel_bg) })
                .await;
            if counter.load(Ordering::SeqCst) != generation || cancel.load(Ordering::Relaxed) {
                return;
            }
            cx.update(|app| {
                let _ = entity.update(app, |shell, cx| {
                    let still_current = shell
                        .player_state
                        .read(cx)
                        .current
                        .as_ref()
                        .is_some_and(|track| track.id == seed_id);
                    if !still_current {
                        return;
                    }
                    match result {
                        Ok(items) => {
                            let mut existing = shell.player_state.read(cx).queue.ids();
                            let mut fresh = Vec::new();
                            for item in items {
                                if item.id != seed_id && existing.insert(item.id.clone()) {
                                    fresh.push(item);
                                }
                            }
                            if fresh.is_empty() {
                                return;
                            }
                            if continue_playback {
                                let resumed = shell.player_state.update(cx, |ps, _cx| {
                                    let resumed = ps.queue.append_continue(fresh);
                                    if let Some(track) = &resumed {
                                        ps.adopt(track.clone());
                                    }
                                    ps.queue.trim_front_to(crate::player::queue::MAX_QUEUE);
                                    resumed
                                });
                                if let Some(track) = resumed {
                                    shell.load_and_play(&track, cx);
                                }
                            } else {
                                shell.player_state.update(cx, |ps, _cx| {
                                    ps.queue.append(fresh);
                                    ps.queue.trim_front_to(crate::player::queue::MAX_QUEUE);
                                });
                            }
                            shell.sync_mpris_queue(cx);
                        }
                        Err(crate::provider::ProviderError::Cancelled) => {}
                        Err(error) => {
                            shell.show_toast(
                                format!(
                                    "YouTube radio unavailable. {}",
                                    provider_error_text(&error)
                                ),
                                cx,
                            );
                        }
                    }
                });
            });
        })
        .detach();
    }

    pub fn play_next(&mut self, cx: &mut gpui::App) {
        self.cancel_youtube_radio();
        let next = self.player_state.update(cx, |ps, _cx| ps.next());
        self.sync_mpris_queue(cx);
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
        self.cancel_youtube_radio();
        let prev = self.player_state.update(cx, |ps, _cx| ps.previous());
        self.sync_mpris_queue(cx);
        if let Some(track) = prev {
            self.load_and_play(&track, cx);
        }
    }

    fn cache_track_cover(&mut self, track: &crate::library::Track, cx: &mut gpui::App) {
        let Some(key) = track.cover_key() else {
            return;
        };
        tracing::debug!(cover = %key, "track cover request");
        if self.home_state.read(cx).covers.contains_key(&key) {
            tracing::debug!(cover = %key, "track cover already in memory");
            return;
        }
        let Some(service) = self.services.service(track.source).cloned() else {
            return;
        };
        let covers = self.cover_cache.clone();
        let track_for_cover = track.clone();
        // Claim the download before spawning: skipping quickly through an album
        // used to fire several downloads of the same cover at once.
        let Some(claim) = covers.begin_fetch(&key) else {
            return;
        };
        let os_for_cover = self.overlay_state.clone();
        let hs = self.home_state.clone();
        let mh = self.mpris_handle.clone();
        let np = self.now_playing.clone();
        let shell_weak = self.self_weak.clone();
        let track = track.clone();
        cx.spawn(async move |cx| {
            let key2 = key.clone();
            // Decoding the cover, masking its corners into a fresh PNG and
            // writing it to disk are all CPU/IO work. They used to run inside
            // `cx.spawn`, which gpui polls on the main thread, so every track
            // change paid for a decode + per-pixel loop + PNG encode on the UI
            // thread. All of it now happens on the background executor.
            let prepared = cx
                .background_executor()
                .spawn(async move {
                    // Hold the in-flight claim until the bytes are on disk.
                    let _claim = claim;
                    // Disk first: the bytes are usually already there.
                    let (bytes, format, path) = match covers.read(&key2) {
                        Some(bytes) => {
                            let format = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
                                gpui::ImageFormat::Png
                            } else {
                                gpui::ImageFormat::Jpeg
                            };
                            let path = covers.existing_path(&key2).unwrap_or_default();
                            (bytes, format, path)
                        }
                        None => {
                            let bytes = match service.cover(&track_for_cover, 512).ok()? {
                                crate::services::CoverSource::Bytes(bytes) => bytes,
                                crate::services::CoverSource::Url(url) => fetch_remote_cover(&url)?,
                            };
                            let is_png = bytes.starts_with(&[0x89, b'P', b'N', b'G']);
                            let path = covers.write(&key2, &bytes).ok()?;
                            let format = if is_png {
                                gpui::ImageFormat::Png
                            } else {
                                gpui::ImageFormat::Jpeg
                            };
                            (bytes, format, path)
                        }
                    };
                    let accent = crate::theme::accent_from_bytes(&bytes);
                    let image = std::sync::Arc::new(gpui::Image {
                        format,
                        bytes,
                        id: fxhash_id(&key2),
                    });
                    let rounded = crate::overlay::rounded_cover(&image);
                    Some((image, rounded, path, accent))
                })
                .await;
            let Some((image, rounded, path, accent)) = prepared else {
                tracing::debug!(cover = %key, "track cover produced no image");
                return;
            };
            tracing::debug!(cover = %key, "track cover ready");
            cx.update(|app| {
                app.update_entity(&hs, |h, _| {
                    h.insert_cover(key.clone(), image.clone());
                });
                if let Some(accent) = accent
                    && let Some(shell) = shell_weak.upgrade()
                {
                    shell.update(app, |s, cx| {
                        s.accent_color = accent;
                        cx.notify();
                    });
                }
                app.update_entity(&np, |view, _cx| {
                    view.set_cover(Some(image.clone()));
                });
                app.update_entity(&os_for_cover, |state, cx| {
                    state.set_cover(Some(rounded.clone()));
                    cx.notify();
                });
            });
            if let Some(mh) = &mh {
                mh.update_metadata(&track, Some(format!("file://{}", path.display())));
            }
        })
        .detach();
    }

    /// Fetch any cover bytes into the shared cover cache and the in-memory map.
    /// The closure runs on the background executor, so callers may do network
    /// or disk work there.
    fn cache_cover_bytes<F>(&mut self, key: String, fetch: F, cx: &mut gpui::App)
    where
        F: FnOnce() -> Option<Vec<u8>> + Send + 'static,
    {
        if self.home_state.read(cx).covers.contains_key(&key) {
            return;
        }
        let covers = self.cover_cache.clone();
        let Some(claim) = covers.begin_fetch(&key) else {
            return;
        };
        let hs = self.home_state.clone();
        let shell = self.self_weak.clone();
        let key_for_task = key.clone();
        cx.spawn(async move |cx| {
            let prepared = cx
                .background_executor()
                .spawn(async move {
                    let _claim = claim;
                    let (bytes, format) = match covers.read(&key_for_task) {
                        Some(bytes) => {
                            let format = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
                                gpui::ImageFormat::Png
                            } else {
                                gpui::ImageFormat::Jpeg
                            };
                            (bytes, format)
                        }
                        None => {
                            let bytes = fetch()?;
                            let is_png = bytes.starts_with(&[0x89, b'P', b'N', b'G']);
                            covers.write(&key_for_task, &bytes).ok()?;
                            let format = if is_png {
                                gpui::ImageFormat::Png
                            } else {
                                gpui::ImageFormat::Jpeg
                            };
                            (bytes, format)
                        }
                    };
                    let accent = crate::theme::accent_from_bytes(&bytes);
                    let image = std::sync::Arc::new(gpui::Image {
                        format,
                        bytes,
                        id: fxhash_id(&key_for_task),
                    });
                    Some((image, accent))
                })
                .await;
            let Some((image, accent)) = prepared else {
                return;
            };
            cx.update(|app| {
                app.update_entity(&hs, |h, _| {
                    h.insert_cover(key.clone(), image);
                });
                if let Some(accent) = accent {
                    if let Some(shell) = shell.upgrade() {
                        shell.update(app, |s, cx| {
                            s.accent_color = accent;
                            cx.notify();
                        });
                    }
                } else if let Some(shell) = shell.upgrade() {
                    shell.update(app, |_, cx| cx.notify());
                }
            });
        })
        .detach();
    }

    fn cache_remote_cover(&mut self, url: String, cx: &mut gpui::App) {
        let key = crate::library::url_cover_key(&url);
        self.cache_cover_bytes(key, move || fetch_remote_cover(&url), cx);
    }

    fn cache_cover_id(&mut self, source: TrackSource, cover_id: String, cx: &mut gpui::App) {
        let Some(service) = self.services.service(source).cloned() else {
            return;
        };
        let key = cover_id.clone();
        self.cache_cover_bytes(
            key,
            move || match service.cover_id(&cover_id, 512).ok()? {
                crate::services::CoverSource::Bytes(bytes) => Some(bytes),
                crate::services::CoverSource::Url(url) => fetch_remote_cover(&url),
            },
            cx,
        );
    }

    fn load_and_play(&mut self, track: &crate::library::Track, cx: &mut gpui::App) {
        match track.source {
            TrackSource::YouTube => self.play_youtube(track, cx),
            TrackSource::Navidrome => {
                if let Some(service) = self.services.service(TrackSource::Navidrome) {
                    let cancel = std::sync::atomic::AtomicBool::new(false);
                    match service.resolve(track, &cancel) {
                        Ok(PlaybackSource::Url(uri)) => {
                            self.player_handle
                                .tx
                                .send(PlayerCommand::Load {
                                    uri,
                                    track_id: track.id.clone(),
                                })
                                .ok();
                            self.player_handle.tx.send(PlayerCommand::Play).ok();
                        }
                        Ok(PlaybackSource::LocalFile(path)) => {
                            self.player_handle
                                .tx
                                .send(PlayerCommand::Load {
                                    uri: format!("file://{}", path.display()),
                                    track_id: track.id.clone(),
                                })
                                .ok();
                            self.player_handle.tx.send(PlayerCommand::Play).ok();
                        }
                        Err(error) => {
                            self.toast_from_app(
                                format!("Could not resolve this track: {error}"),
                                cx,
                            );
                        }
                    }
                } else {
                    self.toast_from_app("Connect a music server to play this track.", cx);
                }
            }
        }
        self.cache_track_cover(track, cx);
        // Show the new track's placeholder immediately: without this the pin kept
        // the previous song's lines until the fetch resolved.
        // Reuse the cached artwork so the PiP shows it immediately; if it is not
        // cached yet, `cache_track_cover` fills it in when it lands.
        let cached_cover = track
            .cover_key()
            .and_then(|key| self.home_state.read(cx).covers.get(&key).cloned())
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
                .cover_key()
                .and_then(|key| self.home_state.read(cx).covers.get(&key).cloned());
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
                        // INT-11: every failure (401 after a password change,
                        // a timeout, an unreachable LRCLIB) used to be reported
                        // as "No lyrics", which sent the user looking in the
                        // wrong place. The real reason reaches the panel now.
                        p_bg.fetch(&t_bg).unwrap_or_else(|error| {
                            crate::lyrics::model::LyricsStatus::Error(
                                error.user_message().to_string(),
                            )
                        })
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
                        // `begin` is for a new track and clears the artwork.
                        // This is a late label update, so it must not wipe a
                        // cover that the artwork fetch already painted.
                        st.set_track(t.title.clone(), t.artist.clone());
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
        if track.source == TrackSource::Navidrome
            && self.config.autoplay
            && self.player_state.read(cx).queue.remaining() == 0
        {
            self.ensure_radio(false, cx);
        }
    }

    /// Resolve a YouTube track to a local file and hand it to the player.
    ///
    /// The helper downloads the whole track first. That costs a short start
    /// delay, but it gives GStreamer a stable, seekable source and keeps the
    /// signed googlevideo URL out of the player and the logs.
    fn play_youtube(&mut self, track: &crate::library::Track, cx: &mut gpui::App) {
        use std::sync::atomic::Ordering;

        let Some(provider) = self.youtube.clone() else {
            self.toast_from_app("YouTube Music is disabled in Settings.", cx);
            return;
        };
        if track.video_id().is_none() {
            self.toast_from_app("This YouTube track has no video id.", cx);
            return;
        }
        if let Some(cancel) = self.youtube_buffer_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.youtube_buffer_cancel = Some(cancel.clone());
        let generation = self.youtube_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.player_state.update(cx, |state, cx| {
            state.set_status(PlaybackStatus::Buffering);
            cx.notify();
        });

        let counter = self.youtube_generation.clone();
        let entity = self.self_weak.clone();
        let track_id = track.id.clone();
        let track_for_buffer = track.clone();
        cx.spawn(async move |cx| {
            let provider_bg = provider.clone();
            let cancel_bg = cancel.clone();
            let first = provider_bg.clone();
            let track_for_retry = track_for_buffer.clone();
            let mut result = cx
                .background_executor()
                .spawn(async move { first.buffer(&track_for_buffer, &cancel_bg) })
                .await;
            if matches!(
                result,
                Err(crate::provider::ProviderError::Unavailable(_))
                    | Err(crate::provider::ProviderError::Network(_))
            ) && !cancel.load(Ordering::Relaxed)
            {
                // A container-specific failure is worth one different attempt.
                let cancel_retry = cancel.clone();
                result = cx
                    .background_executor()
                    .spawn(async move {
                        provider_bg.buffer_with_fallback(&track_for_retry, &cancel_retry, true)
                    })
                    .await;
            }
            if counter.load(Ordering::SeqCst) != generation || cancel.load(Ordering::Relaxed) {
                return;
            }
            cx.update(|app| {
                let _ = entity.update(app, |shell, cx| {
                    let still_current = shell
                        .player_state
                        .read(cx)
                        .current
                        .as_ref()
                        .is_some_and(|current| current.id == track_id);
                    if !still_current {
                        return;
                    }
                    match result {
                        Ok(stream) => {
                            shell.youtube_cache_bytes = stream.cache_bytes;
                            let uri = format!("file://{}", stream.file.display());
                            shell
                                .player_handle
                                .tx
                                .send(PlayerCommand::Load {
                                    uri,
                                    track_id: track_id.clone(),
                                })
                                .ok();
                            shell.player_handle.tx.send(PlayerCommand::Play).ok();
                        }
                        Err(crate::provider::ProviderError::Cancelled) => {}
                        Err(error) => {
                            shell.player_state.update(cx, |state, _| {
                                state.set_status(PlaybackStatus::Stopped);
                            });
                            shell.show_toast(provider_error_text(&error), cx);
                        }
                    }
                });
            });
        })
        .detach();
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
        if seed.is_youtube() {
            reset(&self.radio_in_flight);
            self.fetch_youtube_radio(seed, continue_playback, cx);
            return;
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
                    // Plan under a short lock, fetch with no lock held, commit
                    // under a short lock. The UI thread therefore only ever waits
                    // for these two brief sections, never for the up-to-seven
                    // blocking requests a batch can make.
                    let plan = {
                        let mut guard = radio.lock().unwrap_or_else(|e| e.into_inner());
                        guard.begin(&seed_bg, &exclude)
                    };
                    let Some(plan) = plan else {
                        // This seed is stalled; a different one clears it.
                        return Ok(Vec::new());
                    };
                    let outcome = crate::library::radio::fetch(&client, &plan);
                    let mut guard = radio.lock().unwrap_or_else(|e| e.into_inner());
                    guard.commit(&plan, outcome)
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
                            let _ = weak.update(app, |shell, cx| shell.sync_mpris_queue(cx));
                        } else {
                            ps.update(app, |ps, _| {
                                ps.queue.append(tracks);
                                ps.queue.trim_front_to(crate::player::queue::MAX_QUEUE);
                            });
                            let _ = weak.update(app, |shell, cx| shell.sync_mpris_queue(cx));
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("autoplay radio failed: {e:?}");
                        let msg = format!("Autoplay unavailable. {}", error_text(&e));
                        let _ = weak.update(app, |shell, cx| shell.show_toast(msg, cx));
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
                            match crate::config::save_password(&shell.paths, &pass) {
                                Ok(store) => {
                                    shell.show_toast(store.describe(), cx);
                                }
                                Err(error) => {
                                    shell.setup_error = Some(format!(
                                        "Could not save your password: {error}. \
                                         You will be asked for it again."
                                    ));
                                    return;
                                }
                            }
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
                            shell.rebuild_services();
                            shell.active_source = TrackSource::Navidrome;
                            shell.config.active_source = TrackSource::Navidrome;
                            shell.persist_config();
                            shell.route = Route::Home;
                            if let Some(client) = shell.client.clone() {
                                spawn_home_fetch(
                                    client,
                                    shell.home_state.clone(),
                                    cx.entity(),
                                    shell.cover_cache.clone(),
                                    cx,
                                );
                            }
                        }
                        Err(e) => {
                            shell.setup_error =
                                Some(format!("Connection failed. {}", error_text(&e)));
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

    /// First-run path for users who do not have a Navidrome server. The setup
    /// screen stays available from Settings, so this does not lock anyone out
    /// of the server-backed library later.
    fn use_youtube_setup(&mut self, cx: &mut gpui::Context<Self>) {
        self.config.youtube_enabled = true;
        self.persist_config();
        if self.youtube.is_none() {
            let cookies = {
                let path = self.config.youtube_cookies.trim();
                (!path.is_empty()).then(|| std::path::PathBuf::from(path))
            };
            self.youtube = Some(Arc::new(YouTubeProvider::new(
                self.paths.cache.join("streams"),
                self.config.youtube_quality,
                cookies,
                self.config.stream_cache_mb,
            )));
        }
        self.youtube_cache_bytes = self
            .youtube
            .as_ref()
            .map(|provider| provider.cache_bytes())
            .unwrap_or(0);
        self.rebuild_services();
        self.active_source = TrackSource::YouTube;
        self.config.active_source = TrackSource::YouTube;
        self.persist_config();
        self.route = Route::Home;
        self.setup_focus_requested = false;
        self.youtube_home = None;
        self.refresh_youtube_home(cx);
        self.toast = Some(if YouTubeProvider::helper_installed() {
            "YouTube Music is ready. Browse Home or search for a song.".to_string()
        } else {
            "YouTube support is not installed yet. Run ./scripts/setup-youtube.sh.".to_string()
        });
        cx.notify();
    }

    pub fn cycle_quality(&mut self) {
        self.config.quality = match self.config.quality {
            crate::config::StreamQuality::Original => crate::config::StreamQuality::Kbps320,
            crate::config::StreamQuality::Kbps320 => crate::config::StreamQuality::Kbps192,
            crate::config::StreamQuality::Kbps192 => crate::config::StreamQuality::Original,
        };
        self.persist_config();
        self.rebuild_services();
    }

    fn rebuild_youtube_provider(&mut self) {
        self.youtube = if self.config.youtube_enabled {
            let cookies = {
                let path = self.config.youtube_cookies.trim();
                (!path.is_empty()).then(|| std::path::PathBuf::from(path))
            };
            Some(Arc::new(YouTubeProvider::new(
                self.paths.cache.join("streams"),
                self.config.youtube_quality,
                cookies,
                self.config.stream_cache_mb,
            )))
        } else {
            None
        };
        self.youtube_cache_bytes = self
            .youtube
            .as_ref()
            .map(|provider| provider.cache_bytes())
            .unwrap_or(0);
        self.youtube_home = None;
        self.youtube_home_error = None;
        self.mood_categories = None;
        self.mood_playlists.clear();
        self.mood_title = None;
        self.rebuild_services();
    }

    fn rebuild_services(&mut self) {
        let navidrome = self.client.as_ref().map(|client| {
            Arc::new(NavidromeService::new(
                (**client).clone(),
                self.config.quality,
            )) as Arc<dyn MusicService>
        });
        let youtube = self.youtube.as_ref().map(|provider| {
            Arc::new(YouTubeService::new(provider.clone())) as Arc<dyn MusicService>
        });
        self.services = SourceRegistry::new(navidrome, youtube);
    }

    fn toggle_youtube(&mut self, cx: &mut gpui::Context<Self>) {
        self.config.youtube_enabled = !self.config.youtube_enabled;
        self.persist_config();
        self.rebuild_youtube_provider();
        if !self.config.youtube_enabled {
            self.cancel_youtube_search();
            if let Some(cancel) = self.youtube_buffer_cancel.take() {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            self.youtube_loading = false;
            self.youtube_error = None;
            self.search_results = None;
            self.search_query.clear();
            self.search_generation
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.cancel_youtube_search();
            if self.active_source == TrackSource::YouTube {
                if self.client.is_some() {
                    self.active_source = TrackSource::Navidrome;
                    self.config.active_source = TrackSource::Navidrome;
                    self.persist_config();
                    self.route = Route::Home;
                } else {
                    self.route = Route::Setup;
                }
            }
        } else if self.client.is_none() {
            // YouTube is the only configured source.
            self.active_source = TrackSource::YouTube;
            self.config.active_source = TrackSource::YouTube;
            self.persist_config();
            self.route = Route::Home;
            self.refresh_youtube_home(cx);
        }
        cx.notify();
    }

    fn cycle_youtube_quality(&mut self, cx: &mut gpui::Context<Self>) {
        self.config.youtube_quality = match self.config.youtube_quality {
            YouTubeQuality::Standard => YouTubeQuality::Saver,
            YouTubeQuality::Saver => YouTubeQuality::Standard,
        };
        self.persist_config();
        if let Some(cancel) = self.youtube_buffer_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.rebuild_youtube_provider();
        cx.notify();
    }

    fn clear_youtube_cache(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(provider) = &self.youtube else {
            self.show_toast("YouTube Music is disabled.", cx);
            return;
        };
        match provider.clear_cache() {
            Ok(()) => {
                self.youtube_cache_bytes = 0;
                self.show_toast("YouTube stream cache cleared.", cx);
            }
            Err(error) => self.show_toast(format!("Could not clear the cache: {error}"), cx),
        }
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

    pub fn open_album(&mut self, source: TrackSource, id: String, cx: &mut gpui::Context<Self>) {
        self.route = Route::Album(SourceId::new(source, id.clone()));
        self.selected_album = Some(id.clone());
        self.album_detail = None;
        self.artist_detail = None;
        self.playlist_detail = None;
        self.detail_error = None;
        self.detail_loading = true;
        self.album_loading = true;
        let Some(service) = self.services.service(source).cloned() else {
            self.detail_loading = false;
            self.album_loading = false;
            self.detail_error = Some("This music source is not enabled.".to_string());
            return;
        };
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let album_id = id.clone();
            let res = cx
                .background_executor()
                .spawn(async move { service.album(&album_id) })
                .await;
            cx.update(|app| {
                entity.update(app, |shell, cx| {
                    shell.detail_loading = false;
                    shell.album_loading = false;
                    match res {
                        Ok(detail) => {
                            if let Some(url) = detail.album.cover_url.clone() {
                                shell.cache_remote_cover(url, cx);
                            }
                            if source == TrackSource::Navidrome
                                && let Some(cover_id) = detail.album.cover_art_id.clone()
                            {
                                shell.cache_cover_id(source, cover_id, cx);
                            }
                            if let Some(first) = detail.songs.first().cloned() {
                                shell.cache_track_cover(&first, cx);
                            }
                            shell.album_detail = Some(detail);
                        }
                        Err(error) => {
                            shell.detail_error =
                                Some(format!("Could not open this album. {error}"));
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    pub fn open_artist(&mut self, source: TrackSource, id: String, cx: &mut gpui::Context<Self>) {
        self.route = Route::Artist(SourceId::new(source, id.clone()));
        self.artist_detail = None;
        self.album_detail = None;
        self.playlist_detail = None;
        self.detail_error = None;
        self.detail_loading = true;
        let Some(service) = self.services.service(source).cloned() else {
            self.detail_loading = false;
            self.detail_error = Some("This music source is not enabled.".to_string());
            return;
        };
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let artist_id = id.clone();
            let res = cx
                .background_executor()
                .spawn(async move { service.artist(&artist_id) })
                .await;
            cx.update(|app| {
                entity.update(app, |shell, cx| {
                    shell.detail_loading = false;
                    match res {
                        Ok(detail) => {
                            if let Some(url) = detail.artist.cover_url.clone() {
                                shell.cache_remote_cover(url, cx);
                            } else if let Some(cover_id) = detail.artist.cover_art_id.clone() {
                                shell.cache_cover_id(source, cover_id, cx);
                            }
                            for album in &detail.albums {
                                if let Some(url) = album.cover_url.clone() {
                                    shell.cache_remote_cover(url, cx);
                                } else if let Some(cover_id) = album.cover_art_id.clone() {
                                    shell.cache_cover_id(source, cover_id, cx);
                                }
                            }
                            shell.artist_detail = Some(detail);
                        }
                        Err(error) => {
                            shell.detail_error =
                                Some(format!("Could not open this artist. {error}"));
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    pub fn open_playlist(&mut self, source: TrackSource, id: String, cx: &mut gpui::Context<Self>) {
        self.route = Route::Playlist(SourceId::new(source, id.clone()));
        self.playlist_detail = None;
        self.album_detail = None;
        self.artist_detail = None;
        self.detail_error = None;
        self.detail_loading = true;
        let Some(service) = self.services.service(source).cloned() else {
            self.detail_loading = false;
            self.detail_error = Some("This music source is not enabled.".to_string());
            return;
        };
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let playlist_id = id.clone();
            let res = cx
                .background_executor()
                .spawn(async move { service.playlist(&playlist_id) })
                .await;
            cx.update(|app| {
                entity.update(app, |shell, cx| {
                    shell.detail_loading = false;
                    match res {
                        Ok(detail) => {
                            if let Some(url) = detail.playlist.cover_url.clone() {
                                shell.cache_remote_cover(url, cx);
                            }
                            if let Some(first) = detail.songs.first().cloned() {
                                shell.cache_track_cover(&first, cx);
                            }
                            shell.playlist_detail = Some(detail);
                        }
                        Err(error) => {
                            shell.detail_error =
                                Some(format!("Could not open this playlist. {error}"));
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    /// Load and immediately play a playlist without navigating to its page.
    pub fn play_playlist(&mut self, source: TrackSource, id: String, cx: &mut gpui::Context<Self>) {
        let Some(service) = self.services.service(source).cloned() else {
            return;
        };
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let detail = cx
                .background_executor()
                .spawn(async move { service.playlist(&id) })
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

    /// The YouTube Music front page. Navidrome installs use the snapshot-backed
    /// home state instead.
    pub fn refresh_youtube_home(&mut self, cx: &mut gpui::Context<Self>) {
        if self.youtube_home_loading {
            return;
        }
        let Some(service) = self.services.service(TrackSource::YouTube).cloned() else {
            self.youtube_home_error = Some("YouTube Music is disabled in Settings.".to_string());
            return;
        };
        self.youtube_home_loading = true;
        self.youtube_home_error = None;
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let res = cx
                .background_executor()
                .spawn(async move { service.home(10, &cancel) })
                .await;
            cx.update(|app| {
                entity.update(app, |shell, cx| {
                    shell.youtube_home_loading = false;
                    match res {
                        Ok(sections) => {
                            for section in &sections {
                                for card in &section.items {
                                    if let Some(url) = card.cover_url() {
                                        shell.cache_remote_cover(url.to_string(), cx);
                                    }
                                }
                            }
                            shell.youtube_home = Some(sections);
                            shell.youtube_home_error = None;
                        }
                        Err(error) => {
                            shell.youtube_home_error =
                                Some(format!("Could not load YouTube home. {error}"));
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    pub fn open_moods(&mut self, cx: &mut gpui::Context<Self>) {
        self.route = Route::Moods;
        self.detail_error = None;
        self.detail_loading = false;
        if self.mood_categories.is_none() && !self.mood_loading {
            self.load_moods(cx);
        }
    }

    fn load_moods(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(service) = self.services.service(TrackSource::YouTube).cloned() else {
            self.mood_loading = false;
            return;
        };
        self.mood_loading = true;
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let res = cx
                .background_executor()
                .spawn(async move { service.moods(&cancel) })
                .await;
            cx.update(|app| {
                entity.update(app, |shell, _cx| {
                    shell.mood_loading = false;
                    if let Ok(categories) = res {
                        shell.mood_categories = Some(categories);
                    }
                    shell.detail_error = None;
                    _cx.notify();
                });
            });
        })
        .detach();
    }

    pub fn open_mood(&mut self, title: String, params: String, cx: &mut gpui::Context<Self>) {
        self.route = Route::Moods;
        self.mood_title = Some(title);
        self.mood_playlists.clear();
        self.mood_loading = true;
        let Some(service) = self.services.service(TrackSource::YouTube).cloned() else {
            self.mood_loading = false;
            return;
        };
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let res = cx
                .background_executor()
                .spawn(async move { service.mood_playlists(&params, &cancel) })
                .await;
            cx.update(|app| {
                entity.update(app, |shell, cx| {
                    shell.mood_loading = false;
                    match res {
                        Ok(playlists) => {
                            for playlist in &playlists {
                                if let Some(url) = playlist.cover_url.clone() {
                                    shell.cache_remote_cover(url, cx);
                                }
                            }
                            shell.mood_playlists = playlists;
                        }
                        Err(error) => {
                            shell.detail_error = Some(format!("Could not load this mood. {error}"));
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    pub fn toggle_album_star(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(detail) = &self.album_detail else {
            return;
        };
        let source = detail.album.source;
        let Some(service) = self.services.service(source).cloned() else {
            return;
        };
        let id = detail.album.id.clone();
        let next = !detail.album.starred;
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { service.set_starred(&id, next) })
                .await;
            cx.update(|app| {
                entity.update(app, |shell, cx| match res {
                    Ok(()) => {
                        if let Some(d) = &mut shell.album_detail {
                            d.album.starred = next;
                        }
                    }
                    Err(error) => {
                        shell.show_toast(format!("Could not update the favorite: {error}"), cx)
                    }
                });
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

    /// Which source the sidebar is currently browsing.
    pub fn active_source(&self) -> TrackSource {
        self.active_source
    }

    pub fn source_available(&self, source: TrackSource) -> bool {
        match source {
            TrackSource::Navidrome => self.client.is_some(),
            TrackSource::YouTube => self.youtube.is_some(),
        }
    }

    /// Switch the browsing source. Playback and the queue are intentionally
    /// untouched: a track from the other source keeps playing.
    pub fn set_active_source(&mut self, source: TrackSource, cx: &mut gpui::Context<Self>) -> bool {
        if !self.source_available(source) {
            self.show_toast("That music source is not configured.", cx);
            return false;
        }
        if self.active_source == source {
            if self.config.active_source != source {
                self.config.active_source = source;
                self.persist_config();
            }
            return true;
        }
        self.active_source = source;
        self.config.active_source = source;
        self.persist_config();

        // Do not leave a detail page or a scoped search from the old source on
        // screen. Browse starts again from Home.
        self.selected_album = None;
        self.album_detail = None;
        self.artist_detail = None;
        self.playlist_detail = None;
        self.detail_loading = false;
        self.detail_error = None;
        self.search_results = None;
        self.search_query.clear();
        self.search_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.cancel_youtube_search();
        self.youtube_loading = false;
        self.youtube_error = None;
        self.route = Route::Home;

        if source == TrackSource::YouTube
            && (self.youtube_home.is_none() || self.youtube_home_error.is_some())
        {
            self.refresh_youtube_home(cx);
        }
        cx.notify();
        true
    }

    pub fn loaded_album_count(&self, cx: &gpui::App) -> usize {
        self.home_state.read(cx).albums.len()
    }

    /// Whether the YouTube home feed has at least one shelf.
    pub fn youtube_home_loaded(&self) -> bool {
        self.youtube_home
            .as_ref()
            .is_some_and(|sections| !sections.is_empty())
    }

    /// Number of tracks in the queue (used by the autoplay tests and MPRIS).
    pub fn queue_len(&self, cx: &gpui::App) -> usize {
        self.player_state.read(cx).queue.len()
    }

    /// Snapshot of queue ids, for tests that need to prove what was queued
    /// rather than only how many items there are.
    pub fn queue_ids(&self, cx: &gpui::App) -> Vec<String> {
        self.player_state.read(cx).queue.ids().into_iter().collect()
    }

    /// Queue order (current position is visible via `queue_current_index`).
    pub fn queue_track_ids(&self, cx: &gpui::App) -> Vec<String> {
        self.player_state
            .read(cx)
            .queue
            .tracks()
            .into_iter()
            .map(|track| track.id)
            .collect()
    }

    pub fn queue_current_index(&self, cx: &gpui::App) -> Option<usize> {
        self.player_state.read(cx).queue.current_index()
    }

    /// Tracks still to play before the queue runs out.
    pub fn queue_remaining(&self, cx: &gpui::App) -> usize {
        self.player_state.read(cx).queue.remaining()
    }

    /// Queue drawer visibility, exposed for UI tests.
    pub fn queue_panel_open(&self) -> bool {
        self.queue_panel_open
    }

    /// Single entry point for UI intents. Screens will use this instead of
    /// reaching into `PlayerState`/queue internals once they are extracted.
    pub fn dispatch(&mut self, action: AppAction, cx: &mut gpui::Context<Self>) {
        match action {
            AppAction::Queue(action) => self.dispatch_queue(action, cx),
            AppAction::OpenAlbum(id) => self.open_album(id.source, id.id, cx),
            AppAction::OpenArtist(id) => self.open_artist(id.source, id.id, cx),
            AppAction::OpenPlaylist(id) => self.open_playlist(id.source, id.id, cx),
            AppAction::OpenMoods => self.open_moods(cx),
            AppAction::RefreshYouTubeHome => self.refresh_youtube_home(cx),
        }
    }

    fn dispatch_queue(&mut self, action: QueueAction, cx: &mut gpui::Context<Self>) {
        match action {
            QueueAction::TogglePanel => {
                self.queue_panel_open = !self.queue_panel_open;
            }
            QueueAction::PlayAt(index) => {
                let track = self.player_state.update(cx, |ps, _cx| ps.play_at(index));
                if let Some(track) = track {
                    self.sync_mpris_queue(cx);
                    self.load_and_play(&track, cx);
                }
            }
            QueueAction::Remove(index) => {
                let removed = self
                    .player_state
                    .update(cx, |ps, _cx| ps.queue.remove(index));
                if removed.is_some() {
                    self.sync_mpris_queue(cx);
                }
            }
            QueueAction::Move { from, to } => {
                let moved = self
                    .player_state
                    .update(cx, |ps, _cx| ps.queue.move_item(from, to));
                if moved {
                    self.sync_mpris_queue(cx);
                }
            }
            QueueAction::ClearUpcoming => {
                let removed = self
                    .player_state
                    .update(cx, |ps, _cx| ps.queue.clear_upcoming());
                if removed > 0 {
                    self.sync_mpris_queue(cx);
                }
            }
        }
        cx.notify();
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
            self.open_artist(TrackSource::Navidrome, id, cx);
        }
    }

    pub fn open_first_artist_album(&mut self, cx: &mut gpui::Context<Self>) {
        if let Some((source, id)) = self
            .artist_detail
            .as_ref()
            .and_then(|d| d.albums.first())
            .map(|a| (a.source, a.id.clone()))
        {
            self.open_album(source, id, cx);
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

    /// True when the pinned overlay has artwork to draw.
    pub fn overlay_cover_present(&self, cx: &gpui::App) -> bool {
        self.overlay_state.read(cx).cover.is_some()
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

    /// Report a failure to the user.
    ///
    /// Storing the message is not enough: without `cx.notify()` no repaint
    /// happens, which is how every asynchronous failure became invisible.
    fn show_toast(&mut self, message: impl Into<String>, cx: &mut gpui::Context<Self>) {
        self.toast = Some(message.into());
        cx.notify();
    }

    /// The same surface for callers that hold `App` (the player dispatch path)
    /// rather than a `Context<Shell>`.
    fn toast_from_app(&mut self, message: impl Into<String>, cx: &mut gpui::App) {
        self.toast = Some(message.into());
        cx.refresh_windows();
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
            .map(|r| r.songs.len() + r.albums.len() + r.artists.len() + r.youtube.len())
            .unwrap_or(0)
    }

    /// Results from the active source, excluding the other provider's section.
    pub fn library_result_count(&self) -> usize {
        self.search_results
            .as_ref()
            .map(|r| r.songs.len() + r.albums.len() + r.artists.len())
            .unwrap_or(0)
    }

    pub fn youtube_result_count(&self) -> usize {
        self.search_results
            .as_ref()
            .map(|r| r.youtube.len())
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
        let accent = self.accent_color;

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
            let f_setup_youtube = self.focus_for("setup-youtube", Self::TAB_CONTENT, cx);
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
                                    "delete" => {
                                        this.active_setup_mut().clear();
                                    }
                                    // These fields are hand-built text inputs, so
                                    // paste is not free: without this a password
                                    // manager cannot fill the first screen.
                                    "v" if ev.keystroke.modifiers.control => {
                                        if let Some(text) =
                                            cx.read_from_clipboard().and_then(|item| item.text())
                                        {
                                            // A copied URL or password often carries a
                                            // trailing newline; keep the field single-line.
                                            let cleaned: String = text
                                                .chars()
                                                .filter(|c| *c != '\n' && *c != '\r')
                                                .collect();
                                            this.active_setup_mut().push_str(&cleaned);
                                        }
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
                        )
                        .child(
                            div()
                                .id("setup-youtube")
                                .debug_selector(|| "setup-youtube".to_string())
                                .track_focus(&f_setup_youtube)
                                .tab_index(Self::TAB_CONTENT)
                                .role(gpui::Role::Button)
                                .aria_label("Use YouTube Music instead")
                                .border_2()
                                .border_color(self.focus_border(
                                    &f_setup_youtube,
                                    window,
                                    t.glass_border,
                                    t,
                                ))
                                .cursor_pointer()
                                .px(gpui::px(12.0))
                                .py(gpui::px(8.0))
                                .rounded(gpui::px(999.0))
                                .bg(t.glass_tint)
                                .border_1()
                                .border_color(t.glass_border)
                                .text_color(t.text_secondary)
                                .text_size(gpui::px(10.0))
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.use_youtube_setup(cx);
                                }))
                                .child("Use YouTube Music instead — no server needed"),
                        ),
                )
                .into_any_element();
        }

        // Keyboard-focus handles for the persistent chrome. The sidebar, the
        // player bar and the window controls are always rendered, so their
        // handles exist up front; content controls create theirs in the branch
        // that renders them.
        let f_np_open = self.focus_for("player-now-playing", Self::TAB_PLAYER, cx);
        let f_seek_back = self.focus_for("player-seek-back", Self::TAB_PLAYER, cx);
        let f_prev = self.focus_for("player-prev", Self::TAB_PLAYER, cx);
        let f_play = self.focus_for("player-play", Self::TAB_PLAYER, cx);
        let f_next = self.focus_for("player-next", Self::TAB_PLAYER, cx);
        let f_shuffle = self.focus_for("player-shuffle", Self::TAB_PLAYER, cx);
        let f_repeat = self.focus_for("player-repeat", Self::TAB_PLAYER, cx);
        let f_queue = self.focus_for("player-queue", Self::TAB_PLAYER, cx);
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
        let f_sync = self.focus_for("content-sync", Self::TAB_CONTENT, cx);

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
        let current_source = self
            .player_state
            .read(cx)
            .current
            .as_ref()
            .map(|t| t.source);
        let current_cover = self
            .player_state
            .read(cx)
            .current
            .as_ref()
            .and_then(|t| t.cover_key())
            .and_then(|key| self.home_state.read(cx).covers.get(&key).cloned());
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

        let queue_panel = self.render_queue_panel(window, cx);

        // Player bar
        let player = crate::theme::glass_surface(
            div()
                .absolute()
                .left(shadow_size + gpui::px(18.0))
                .right(shadow_size + gpui::px(18.0))
                .bottom(shadow_size + gpui::px(18.0))
                .h(gpui::px(64.0))
                .rounded(gpui::px(32.0)),
            t,
            32.0,
        )
        .bg(gpui::linear_gradient(
            145.0,
            gpui::linear_color_stop(t.glass_gradient_start, 0.0),
            gpui::linear_color_stop(t.glass_gradient_end, 1.0),
        ))
        .border_color(t.glass_border)
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
                                .w(gpui::px(48.0))
                                .h(gpui::px(48.0))
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
                                .flex()
                                .items_center()
                                .gap(gpui::px(6.0))
                                .child(
                                    div()
                                        .text_size(gpui::px(13.0))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(t.clay_ink)
                                        // A long title used to wrap to three
                                        // lines inside the 88 px player bar
                                        // and push the artist out of it.
                                        .line_clamp(1)
                                        .text_ellipsis()
                                        .child(if current_title.is_empty() {
                                            "Nothing playing".to_string()
                                        } else {
                                            current_title.clone()
                                        }),
                                )
                                .child(match current_source {
                                    Some(TrackSource::YouTube) => div()
                                        .px(gpui::px(5.0))
                                        .py(gpui::px(1.0))
                                        .rounded(gpui::px(999.0))
                                        .bg(t.gold_tint)
                                        .border_1()
                                        .border_color(t.gold_tint_border)
                                        .text_size(gpui::px(8.0))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(t.clay_ink)
                                        .child("YouTube")
                                        .into_any_element(),
                                    Some(TrackSource::Navidrome) => div()
                                        .px(gpui::px(5.0))
                                        .py(gpui::px(1.0))
                                        .rounded(gpui::px(999.0))
                                        .bg(t.glass_highlight_soft)
                                        .border_1()
                                        .border_color(t.glass_border)
                                        .text_size(gpui::px(8.0))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(t.text_secondary)
                                        .child("Library")
                                        .into_any_element(),
                                    None => div().into_any_element(),
                                }),
                        )
                        .child(
                            div()
                                .text_size(gpui::px(10.0))
                                .text_color(t.clay_ink)
                                .opacity(0.7)
                                .line_clamp(1)
                                .text_ellipsis()
                                .child(current_artist.clone()),
                        ),
                )
                .child(if is_playing {
                    div()
                        .flex()
                        .items_end()
                        .gap(gpui::px(2.0))
                        .h(gpui::px(14.0))
                        .child(eq_bar("eq1", 620, 4.0, 12.0, accent))
                        .child(eq_bar("eq2", 780, 6.0, 14.0, accent))
                        .child(eq_bar("eq3", 700, 3.0, 10.0, accent))
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
                                    let back =
                                        position.saturating_sub(std::time::Duration::from_secs(10));
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
                                        color: t.shadow_warm.into(),
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
                        // Shuffle and repeat had no reachable control at all
                        // (only Ctrl+S / Ctrl+R), so the feature was invisible.
                        .child({
                            let on = self.shuffle_on(cx);
                            div()
                                .w(gpui::px(24.0))
                                .h(gpui::px(24.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(gpui::px(12.0))
                                .text_color(if on { t.gold_deep } else { t.clay_ink })
                                .opacity(if on { 1.0 } else { 0.55 })
                                .cursor_pointer()
                                .id("shuffle-btn")
                                .debug_selector(|| "shuffle-btn".to_string())
                                .track_focus(&f_shuffle)
                                .tab_index(Self::TAB_PLAYER)
                                .role(gpui::Role::Button)
                                .border_2()
                                .border_color(self.focus_border(
                                    &f_shuffle,
                                    window,
                                    gpui::rgba(0x00000000),
                                    t,
                                ))
                                .aria_label(if on { "Shuffle on" } else { "Shuffle off" })
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.toggle_shuffle(cx);
                                }))
                                .child("⇄")
                        })
                        .child({
                            let repeat = self.repeat_label(cx);
                            let active = repeat != "Repeat: off";
                            div()
                                .w(gpui::px(24.0))
                                .h(gpui::px(24.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(gpui::px(12.0))
                                .text_color(if active { t.gold_deep } else { t.clay_ink })
                                .opacity(if active { 1.0 } else { 0.55 })
                                .cursor_pointer()
                                .id("repeat-btn")
                                .debug_selector(|| "repeat-btn".to_string())
                                .track_focus(&f_repeat)
                                .tab_index(Self::TAB_PLAYER)
                                .role(gpui::Role::Button)
                                .border_2()
                                .border_color(self.focus_border(
                                    &f_repeat,
                                    window,
                                    gpui::rgba(0x00000000),
                                    t,
                                ))
                                .aria_label(repeat)
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.toggle_repeat(cx);
                                }))
                                .child(if repeat == "Repeat: one" {
                                    "↻1"
                                } else {
                                    "↻"
                                })
                        })
                        .child(
                            div()
                                .w(gpui::px(30.0))
                                .h(gpui::px(24.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(gpui::px(12.0))
                                .text_color(t.clay_ink)
                                .opacity(0.7)
                                .cursor_pointer()
                                .id("queue-btn")
                                .debug_selector(|| "queue-btn".to_string())
                                .track_focus(&f_queue)
                                .tab_index(Self::TAB_PLAYER)
                                .role(gpui::Role::Button)
                                .border_2()
                                .border_color(self.focus_border(
                                    &f_queue,
                                    window,
                                    gpui::rgba(0x00000000),
                                    t,
                                ))
                                .aria_label(if self.queue_panel_open {
                                    "Close queue"
                                } else {
                                    "Open queue"
                                })
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.dispatch(AppAction::Queue(QueueAction::TogglePanel), cx);
                                }))
                                .child("☰"),
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
                                    let fwd =
                                        position.saturating_add(std::time::Duration::from_secs(10));
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
                                .text_size(gpui::px(10.0))
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
                                        .bg(accent),
                                ),
                        )
                        .child(
                            div()
                                .text_size(gpui::px(10.0))
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
                                .w(gpui::px(self.player_state.read(cx).volume * 60.0))
                                .h_full()
                                .rounded(gpui::px(999.0))
                                .bg(t.gold),
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
                        .text_size(gpui::px(10.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .cursor_pointer()
                        .id("pin-btn")
                        .track_focus(&f_pin)
                        .tab_index(Self::TAB_PLAYER)
                        .role(gpui::Role::Button)
                        .aria_label("Pin or unpin the lyrics overlay")
                        .border_2()
                        .border_color(self.focus_border(&f_pin, window, gpui::rgba(0x00000000), t))
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
                    // Enter activates the focused control. Space stays the global
                    // play/pause shortcut (documented in docs/design.md and
                    // covered by tests), so a keyboard user gets one unambiguous
                    // activation key for every control in the ring.
                    if key == "enter"
                        && !mods.control
                        && !mods.alt
                        && let Some(focused) = this.focused_control(window)
                    {
                        this.activate_focused(&focused, window, cx);
                        return;
                    }
                    match key {
                        // Shuffle and repeat had no reachable control at all.
                        // Global shortcuts keep them usable without touching the
                        // player bar layout.
                        "s" if mods.control => this.toggle_shuffle(cx),
                        "r" if mods.control => this.toggle_repeat(cx),
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
                                color: t.ambient_teal_deep.into(),
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
                                color: t.ambient_rose_deep.into(),
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
                            .w(gpui::px(220.0))
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .gap(gpui::px(12.0))
                            .px(gpui::px(16.0))
                            .pt(gpui::px(18.0))
                            .pb(gpui::px(92.0))
                            .bg(t.glass_tint)
                            .border_r_1()
                            .border_color(t.glass_border)
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
                            .child(self.render_sidebar_nav(window, cx, t))
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
                            .pb(gpui::px(86.0))
                            .id("main-scroll")
                            .overflow_y_scroll()
                            .child(if matches!(
                                self.route,
                                Route::Album(_) | Route::Artist(_) | Route::Playlist(_) | Route::Moods
                            ) || (self.route == Route::Home && self.selected_album.is_some())
                            {
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
                                                        crate::app::Route::Home => greeting_for_now(),                                                        crate::app::Route::Search => "Search",
                                                        crate::app::Route::NowPlaying => "Now Playing",
                                                        crate::app::Route::Settings => "Settings",
                                                        crate::app::Route::Setup => "Setup",
                                                        crate::app::Route::Albums => "Albums",
                                                        crate::app::Route::Artists => "Artists",
                                                        crate::app::Route::Playlists => "Playlists",
                                                        crate::app::Route::Favorites => "Favorites",
                                                        crate::app::Route::Album(_) => "Album",
                                                        crate::app::Route::Artist(_) => "Artist",
                                                        crate::app::Route::Playlist(_) => "Playlist",
                                                        crate::app::Route::Moods => "Moods & genres",
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .text_size(gpui::px(10.0))
                                                    .text_color(t.text_tertiary)
                                                    .debug_selector(|| "library-status".to_string())
                                                    .child(
                                                        self.library_status(cx).unwrap_or_default(),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("home-search")
                                            .track_focus(&f_search_shortcut)
                                            .tab_index(Self::TAB_CONTENT)
                                            .role(gpui::Role::Button)
                                            .aria_label("Search music (Ctrl K)")
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
                                                    color: t.glass_border_on_clay.into(),
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
                                            .child("Search music…")
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
                                || (self.route == Route::Home && self.active_source == TrackSource::YouTube)
                                || matches!(self.route, Route::NowPlaying | Route::Search | Route::Settings | Route::Artists | Route::Playlists | Route::Favorites | Route::Album(_) | Route::Artist(_) | Route::Playlist(_) | Route::Moods) {
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
                                    .child(
                                        // One button for the whole cache: pull the
                                        // library from the server into the snapshot
                                        // that makes lists and search instant.
                                        div()
                                            .id("sync-library")
                                            .debug_selector(|| "sync-library".to_string())
                                            .track_focus(&f_sync)
                                            .tab_index(Self::TAB_CONTENT)
                                            .role(gpui::Role::Button)
                                            .aria_label("Sync library")
                                            .border_2()
                                            .border_color(self.focus_border(&f_sync, window, gpui::rgba(0x00000000), t))
                                            .cursor_pointer()
                                            .text_size(gpui::px(10.0))
                                            .text_color(t.text_secondary)
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .on_click(cx.listener(|this, _event, _window, cx| {
                                                this.sync_library(cx);
                                            }))
                                            .child("Sync"),
                                    )
                            })
                            .child(if self.route == Route::NowPlaying {
                                div().child(self.now_playing.clone())
                            } else if self.route == Route::Search {
                                let mut col = div().flex().flex_col().gap(gpui::px(12.0));
                                let search_entity = cx.entity();
                                let searching_youtube =
                                    self.active_source == TrackSource::YouTube;
                                col = col.child(
                                    div()
                                        .text_size(gpui::px(9.0))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(t.gold)
                                        .debug_selector(|| "search-source".to_string())
                                        .child(if searching_youtube {
                                            "YOUTUBE MUSIC"
                                        } else {
                                            "YOUR LIBRARY"
                                        }),
                                );
                                col = col.child(
                                    div()
                                        .id("search-box")
                                        .track_focus(&self.search_focus)
                                        .tab_index(Self::TAB_CONTENT)
                                        .role(gpui::Role::TextInput)
                                        .aria_label(if self.active_source == TrackSource::YouTube {
                                            "Search YouTube Music"
                                        } else {
                                            "Search your library"
                                        })
                                        .focusable()
                                        .on_key_down(move |ev, _window, app| {
                                            let entity = search_entity.clone();
                                            app.update_entity(&entity, |this, cx| {
                                                let k = ev.keystroke.key.as_str();
                                                if k == "backspace" {
                                                    this.search_query.pop();
                                                    this.schedule_search(false, cx);
                                                } else if k == "escape" {
                                                    // Esc clears the query; on an
                                                    // already-empty field it also
                                                    // leaves the search page, as
                                                    // docs/design.md promises.
                                                    let was_empty = this.search_query.is_empty();
                                                    this.search_query.clear();
                                                    this.search_results = None;
                                                    this.search_generation
                                                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                                    if was_empty {
                                                        this.go(Route::Home);
                                                    }
                                                } else if k == "enter" {
                                                    this.schedule_search(true, cx);
                                                } else if !ev.keystroke.modifiers.control && !ev.keystroke.modifiers.alt {
                                                    let before = this.search_query.len();
                                                    if let Some(ch) = &ev.keystroke.key_char {
                                                        this.search_query.push_str(ch);
                                                    } else if k.len() == 1 && k.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
                                                        this.search_query.push_str(k);
                                                    }
                                                    if this.search_query.len() != before {
                                                        this.schedule_search(false, cx);
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
                                                    if self.active_source == TrackSource::YouTube {
                                                        "Search YouTube Music…".to_string()
                                                    } else {
                                                        "Search artists, albums, songs…".to_string()
                                                    }
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
                                            .child("Results update as you type. Esc clears."),
                                    );
                                }
                                if let Some(results) = &self.search_results {
                                    // One row style for every result group. The page
                                    // used to render only SONGS, so a query that
                                    // matched an album or an artist showed nothing.
                                    let result_row =
                                        |kind: &'static str,
                                         idx: usize,
                                         id: String,
                                         source: TrackSource,
                                         label: String,
                                         aria: String,
                                         window: &gpui::Window,
                                         cx: &mut gpui::Context<Self>| {
                                            let key = format!("search-{kind}-{idx}");
                                            let handle = self.focus_for(&key, Self::TAB_CONTENT, cx);
                                            let selector = key.clone();
                                            div()
                                                .id(key.clone())
                                                .debug_selector(move || selector.clone())
                                                .track_focus(&handle)
                                                .tab_index(Self::TAB_CONTENT)
                                                .role(gpui::Role::Button)
                                                .border_2()
                                                .border_color(self.focus_border(
                                                    &handle,
                                                    window,
                                                    t.glass_border,
                                                    t,
                                                ))
                                                .aria_label(aria)
                                                .cursor_pointer()
                                                .px(gpui::px(12.0))
                                                .py(gpui::px(8.0))
                                                .rounded(gpui::px(10.0))
                                                .bg(t.glass_tint)
                                                .border_1()
                                                .border_color(t.glass_border)
                                                .text_size(gpui::px(12.0))
                                                .line_clamp(1)
                                                .text_ellipsis()
                                                .on_click(cx.listener(move |this, _event, _window, cx| {
                                                    match kind {
                                                        "album" => this.open_album(source, id.clone(), cx),
                                                        "artist" => this.open_artist(source, id.clone(), cx),
                                                        _ => this.open_playlist(source, id.clone(), cx),
                                                    }
                                                }))
                                                .child(label)
                                        };
                                    if !results.artists.is_empty() {
                                        col = col.child(section_label("ARTISTS", t));
                                        for (i, artist) in results.artists.iter().take(10).enumerate() {
                                            let album_count = artist
                                                .album_count
                                                .map(|count| format!(" · {count} albums"))
                                                .unwrap_or_default();
                                            col = col.child(result_row(
                                                "artist",
                                                i,
                                                artist.id.clone(),
                                                artist.source,
                                                format!("{}{album_count}", artist.name),
                                                format!("Open artist {}", artist.name),
                                                window,
                                                cx,
                                            ));
                                        }
                                    }
                                    if !results.albums.is_empty() {
                                        col = col.child(section_label("ALBUMS", t));
                                        for (i, album) in results.albums.iter().take(10).enumerate() {
                                            col = col.child(result_row(
                                                "album",
                                                i,
                                                album.id.clone(),
                                                album.source,
                                                format!("{} — {}", album.name, album.artist),
                                                format!("Open album {}", album.name),
                                                window,
                                                cx,
                                            ));
                                        }
                                    }
                                    if !results.playlists.is_empty() {
                                        col = col.child(section_label("PLAYLISTS", t));
                                        for (i, playlist) in results.playlists.iter().take(10).enumerate() {
                                            col = col.child(result_row(
                                                "playlist",
                                                i,
                                                playlist.id.clone(),
                                                playlist.source,
                                                playlist.name.clone(),
                                                format!("Play playlist {}", playlist.name),
                                                window,
                                                cx,
                                            ));
                                        }
                                    }
                                    if !results.songs.is_empty() {
                                        col = col.child(section_label("SONGS", t));
                                        let songs = std::sync::Arc::new(results.songs.clone());
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
                                                        this.play_tracks((*songs2).clone(), i, cx);
                                                    }))
                                                    .child(format!("{} — {}", tr.title, tr.artist)),
                                            );
                                        }
                                    }
                                    // YouTube Music: a separate source, so a
                                    // separate section. Results are already
                                    // merged into `results.youtube` by the
                                    // generation-guarded search task.
                                    if self.youtube_loading {
                                        col = col.child(
                                            div()
                                                .text_size(gpui::px(11.0))
                                                .text_color(t.text_secondary)
                                                .child("Searching YouTube Music…"),
                                        );
                                    }
                                    if let Some(error) = &self.youtube_error {
                                        let f_retry = self.focus_for(
                                            "search-youtube-retry",
                                            Self::TAB_CONTENT,
                                            cx,
                                        );
                                        col = col.child(
                                            div()
                                                .id("search-youtube-error")
                                                .debug_selector(|| "search-youtube-error".to_string())
                                                .flex()
                                                .items_center()
                                                .gap(gpui::px(8.0))
                                                .text_size(gpui::px(11.0))
                                                .text_color(t.text_secondary)
                                                .child(error.clone())
                                                .child(
                                                    div()
                                                        .id("search-youtube-retry")
                                                        .track_focus(&f_retry)
                                                        .tab_index(Self::TAB_CONTENT)
                                                        .role(gpui::Role::Button)
                                                        .aria_label("Retry YouTube search")
                                                        .border_2()
                                                        .border_color(self.focus_border(
                                                            &f_retry,
                                                            window,
                                                            t.glass_border,
                                                            t,
                                                        ))
                                                        .cursor_pointer()
                                                        .px(gpui::px(8.0))
                                                        .py(gpui::px(4.0))
                                                        .rounded(gpui::px(8.0))
                                                        .bg(t.glass_tint)
                                                        .on_click(cx.listener(
                                                            |this, _event, _window, cx| {
                                                                this.schedule_search(true, cx);
                                                            },
                                                        ))
                                                        .child("Retry"),
                                                ),
                                        );
                                    }
                                    if !results.youtube.is_empty() {
                                        col = col.child(section_label("YOUTUBE MUSIC", t));
                                        for (i, tr) in results.youtube.iter().take(20).enumerate() {
                                            let track = tr.clone();
                                            let f_search_youtube = self.focus_for(
                                                &format!("search-youtube-{i}"),
                                                Self::TAB_CONTENT,
                                                cx,
                                            );
                                            let duration = if tr.duration.as_secs() > 0 {
                                                format!(" · {}", format_time(tr.duration.as_secs_f64()))
                                            } else {
                                                String::new()
                                            };
                                            col = col.child(
                                                div()
                                                    .id(format!("search-youtube-{}", i))
                                                    .debug_selector(move || {
                                                        format!("search-youtube-{}", i)
                                                    })
                                                    .track_focus(&f_search_youtube)
                                                    .tab_index(Self::TAB_CONTENT)
                                                    .role(gpui::Role::Button)
                                                    .border_2()
                                                    .border_color(self.focus_border(
                                                        &f_search_youtube,
                                                        window,
                                                        t.glass_border,
                                                        t,
                                                    ))
                                                    .aria_label(format!(
                                                        "Play {} by {} on YouTube Music",
                                                        tr.title, tr.artist
                                                    ))
                                                    .cursor_pointer()
                                                    .px(gpui::px(12.0))
                                                    .py(gpui::px(8.0))
                                                    .rounded(gpui::px(10.0))
                                                    .bg(t.glass_tint)
                                                    .border_1()
                                                    .border_color(t.glass_border)
                                                    .text_size(gpui::px(12.0))
                                                    .line_clamp(1)
                                                    .text_ellipsis()
                                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                                        // A search result starts its
                                                        // song radio; it does not
                                                        // queue the search page.
                                                        this.play_youtube_result(track.clone(), cx);
                                                    }))
                                                    .child(format!(
                                                        "{} — {}{}",
                                                        tr.title, tr.artist, duration
                                                    )),
                                            );
                                        }
                                    }
                                    if results.songs.is_empty()
                                        && results.albums.is_empty()
                                        && results.artists.is_empty()
                                        && results.playlists.is_empty()
                                        && results.youtube.is_empty()
                                        && !self.youtube_loading
                                        && self.youtube_error.is_none()
                                    {
                                        col = col.child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .items_center()
                                                .gap(gpui::px(8.0))
                                                .py(gpui::px(40.0))
                                                .child(
                                                    div()
                                                        .text_size(gpui::px(32.0))
                                                        .text_color(t.text_tertiary)
                                                        .child("⌕"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(gpui::px(13.0))
                                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                                        .text_color(t.text_secondary)
                                                        .child("No results"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(gpui::px(11.0))
                                                        .text_color(t.text_tertiary)
                                                        .child(if self.active_source == TrackSource::YouTube {
                                                            "Try a different query or browse Moods & genres."
                                                        } else {
                                                            "Try a different query or sync your library."
                                                        }),
                                                ),
                                        );
                                    }
                                }
                                col
                            } else if matches!(
                                self.route,
                                Route::Album(_) | Route::Artist(_) | Route::Playlist(_) | Route::Moods
                            ) {
                                div().w_full().child(self.render_detail_route(window, cx))
                            } else if self.route == Route::Home && self.active_source == TrackSource::YouTube {
                                div().w_full().child(self.render_youtube_home(window, cx))
                            } else if self.route == Route::Settings {
                                let mut col = div().flex().flex_col().gap(gpui::px(8.0));
                                let f_set_quality = self.focus_for("settings-quality", Self::TAB_CONTENT, cx);
                                let f_set_scrobble = self.focus_for("settings-scrobble", Self::TAB_CONTENT, cx);
                                let f_clear_cache = self.focus_for("settings-clear-cache", Self::TAB_CONTENT, cx);
                                let f_set_lyrics_source =
                                    self.focus_for("settings-lyrics-source", Self::TAB_CONTENT, cx);
                                let f_set_reduce_motion =
                                    self.focus_for("settings-reduce-motion", Self::TAB_CONTENT, cx);
                                let f_set_autoplay =
                                    self.focus_for("settings-autoplay", Self::TAB_CONTENT, cx);
                                let f_set_close_to_tray =
                                    self.focus_for("settings-close-to-tray", Self::TAB_CONTENT, cx);
                                let f_youtube =
                                    self.focus_for("settings-youtube", Self::TAB_CONTENT, cx);
                                let f_youtube_quality =
                                    self.focus_for("settings-youtube-quality", Self::TAB_CONTENT, cx);
                                let f_youtube_cache =
                                    self.focus_for("settings-youtube-cache", Self::TAB_CONTENT, cx);
                                let f_server =
                                    self.focus_for("settings-server", Self::TAB_CONTENT, cx);
                                let f_offset_down = self.focus_for("settings-offset-down", Self::TAB_CONTENT, cx);
                                let f_offset_up = self.focus_for("settings-offset-up", Self::TAB_CONTENT, cx);
                                col = col
                                    .child(settings_section("Account", t))
                                    .child(setting_row("Server URL", &self.config.server_url, t))
                                    .child(setting_row("Username", &self.config.username, t))
                                    .child(
                                        setting_row(
                                            "Music server",
                                            if self.client.is_some() {
                                                "Connected"
                                            } else {
                                                "Not connected"
                                            },
                                            t,
                                        )
                                        .id("set-server")
                                        .debug_selector(|| "set-server".to_string())
                                        .track_focus(&f_server)
                                        .tab_index(Self::TAB_CONTENT)
                                        .role(gpui::Role::Button)
                                        .aria_label(if self.client.is_some() {
                                            "Music server connected; open connection settings"
                                        } else {
                                            "Connect a music server"
                                        })
                                        .border_2()
                                        .border_color(self.focus_border(
                                            &f_server,
                                            window,
                                            t.glass_border,
                                            t,
                                        ))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _event, _window, cx| {
                                            this.route = Route::Setup;
                                            this.setup_error = None;
                                            this.setup_focus_requested = false;
                                            cx.notify();
                                        })),
                                    )
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
                                        setting_row("Cached library", "Clear", t)
                                            .id("set-clear-cache")
                                            .debug_selector(|| "set-clear-cache".to_string())
                                            .track_focus(&f_clear_cache).tab_index(Self::TAB_CONTENT).role(gpui::Role::Button).aria_label("Clear the cached library and artwork").border_2().border_color(self.focus_border(&f_clear_cache, window, t.glass_border, t))
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _event, _window, cx| {
                                                this.clear_library_cache(cx);
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
                                    .child(setting_row("License", "MIT", t))
                                    .child(settings_section("Sources", t))
                                    .child(
                                        setting_row(
                                            "YouTube Music",
                                            if self.config.youtube_enabled {
                                                "On"
                                            } else {
                                                "Off"
                                            },
                                            t,
                                        )
                                        .id("set-youtube")
                                        .debug_selector(|| "set-youtube".to_string())
                                        .track_focus(&f_youtube)
                                        .tab_index(Self::TAB_CONTENT)
                                        .role(gpui::Role::Button)
                                        .aria_label(format!(
                                            "YouTube Music, {}",
                                            if self.config.youtube_enabled {
                                                "on"
                                            } else {
                                                "off"
                                            }
                                        ))
                                        .border_2()
                                        .border_color(self.focus_border(
                                            &f_youtube,
                                            window,
                                            t.glass_border,
                                            t,
                                        ))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _event, _window, cx| {
                                            this.toggle_youtube(cx);
                                        })),
                                    )
                                    .child(
                                        setting_row(
                                            "YouTube quality",
                                            self.config.youtube_quality.label(),
                                            t,
                                        )
                                        .id("set-youtube-quality")
                                        .debug_selector(|| "set-youtube-quality".to_string())
                                        .track_focus(&f_youtube_quality)
                                        .tab_index(Self::TAB_CONTENT)
                                        .role(gpui::Role::Button)
                                        .aria_label(format!(
                                            "YouTube quality, {}",
                                            self.config.youtube_quality.label()
                                        ))
                                        .border_2()
                                        .border_color(self.focus_border(
                                            &f_youtube_quality,
                                            window,
                                            t.glass_border,
                                            t,
                                        ))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _event, _window, cx| {
                                            this.cycle_youtube_quality(cx);
                                        })),
                                    )
                                    .child(setting_row(
                                        "YouTube helper",
                                        if YouTubeProvider::helper_installed() {
                                            "Installed"
                                        } else {
                                            "Not installed"
                                        },
                                        t,
                                    ))
                                    .child(
                                        setting_row(
                                            "Stream cache",
                                            &format!(
                                                "{} MB",
                                                self.youtube_cache_bytes / (1024 * 1024)
                                            ),
                                            t,
                                        )
                                        .id("set-youtube-cache")
                                        .debug_selector(|| "set-youtube-cache".to_string())
                                        .track_focus(&f_youtube_cache)
                                        .tab_index(Self::TAB_CONTENT)
                                        .role(gpui::Role::Button)
                                        .aria_label("Clear the YouTube stream cache")
                                        .border_2()
                                        .border_color(self.focus_border(
                                            &f_youtube_cache,
                                            window,
                                            t.glass_border,
                                            t,
                                        ))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _event, _window, cx| {
                                            this.clear_youtube_cache(cx);
                                        })),
                                    );
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
                                        let album_source = a.source;
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
                                                    this.open_album(album_source, aid.clone(), cx);
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
                                        let artist_source = a.source;
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
                                                    this.open_artist(artist_source, aid.clone(), cx);
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
                                    let playlist_source = p.source;
                                    let f_playlist =
                                        self.focus_for(&format!("playlist:{}", pid), Self::TAB_CONTENT, cx);
                                    col = col.child(
                                        div()
                                            .id(format!("playlist-{}", pid))
                                            .track_focus(&f_playlist)
                                            .tab_index(Self::TAB_CONTENT)
                                            .role(gpui::Role::ListItem)
                                            .aria_label(format!(
                                                "Open playlist {}",
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
                                                this.open_playlist(playlist_source, pid.clone(), cx);
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
                                    for _ in 0..8 {
                                        grid = grid.child(crate::ui::components::skeleton_card(t));
                                    }
                                } else if let Some(error) = hs.error.clone() {
                                    // A network failure used to be rendered as
                                    // "No albums — Connect a server", which sends
                                    // the user to fix the wrong thing.
                                    let retry = self.focus_for("content-library-retry", Self::TAB_CONTENT, cx);
                                    grid = grid.child(
                                        div()
                                            .id("library-error")
                                            .debug_selector(|| "library-error".to_string())
                                            // The panel lives inside a 4-column grid;
                                            // without a span it would be a quarter wide.
                                            .col_span_full()
                                            .role(gpui::Role::Alert)
                                            .aria_label(error.clone())
                                            .flex()
                                            .flex_col()
                                            .gap(gpui::px(10.0))
                                            .p(gpui::px(20.0))
                                            .rounded(gpui::px(16.0))
                                            .bg(t.glass_tint)
                                            .border_1()
                                            .border_color(t.danger)
                                            .child(
                                                div()
                                                    .text_size(gpui::px(12.0))
                                                    .text_color(t.text_primary)
                                                    .child(error),
                                            )
                                            .child(
                                                div()
                                                    .id("library-retry")
                                                    .debug_selector(|| "library-retry".to_string())
                                                    .track_focus(&retry)
                                                    .tab_index(Self::TAB_CONTENT)
                                                    .role(gpui::Role::Button)
                                                    .aria_label("Try loading the library again")
                                                    .border_2()
                                                    .border_color(self.focus_border(&retry, window, t.glass_border, t))
                                                    .cursor_pointer()
                                                    .px(gpui::px(12.0))
                                                    .py(gpui::px(6.0))
                                                    .rounded(gpui::px(10.0))
                                                    .text_size(gpui::px(11.0))
                                                    .text_color(t.gold_light)
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                                        this.retry_home_fetch(cx);
                                                    }))
                                                    .child("Retry"),
                                            ),
                                    );
                                } else if hs.albums.is_empty() {
                                    // Genuinely empty: no server problem, just no music yet.
                                    grid = grid.child(
                                        div()
                                            .id("library-empty")
                                            .debug_selector(|| "library-empty".to_string())
                                            .col_span_full()
                                            .role(gpui::Role::Label)
                                            .aria_label("No albums yet")
                                            .flex()
                                            .flex_col()
                                            .gap(gpui::px(6.0))
                                            .p(gpui::px(20.0))
                                            .rounded(gpui::px(16.0))
                                            .bg(t.glass_tint)
                                            .border_1()
                                            .border_color(t.glass_border)
                                            .child(
                                                div()
                                                    .text_size(gpui::px(12.0))
                                                    .text_color(t.text_primary)
                                                    .child("No albums yet"),
                                            )
                                            .child(
                                                div()
                                                    .text_size(gpui::px(11.0))
                                                    .text_color(t.text_secondary)
                                                    .child("Press Sync to pull your library."),
                                            ),
                                    );
                                } else {
                                    let source = if self.route == Route::Favorites { &hs.favorites } else { &hs.albums };
                                let limit = if matches!(self.route, Route::Albums | Route::Favorites) { source.len() } else { 8 };
                                    for a in source.iter().take(limit) {
                                        let aid = a.id.clone();
                                        let album_source = a.source;
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
                                                    this.open_album(album_source, aid.clone(), cx);
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
                                let f_album_enqueue =
                                    self.focus_for("album-enqueue", Self::TAB_CONTENT, cx);
                                let songs_all = detail.songs.clone();
                                let songs_all_queue = songs_all.clone();
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
                                    let computed_duration: Option<std::time::Duration> =
                                        if detail.songs.is_empty() {
                                            detail.album.duration
                                        } else {
                                            let total: std::time::Duration =
                                                detail.songs.iter().map(|t| t.duration).sum();
                                            if total.as_secs() > 0 {
                                                Some(total)
                                            } else {
                                                detail.album.duration
                                            }
                                        };
                                    if let Some(duration) = computed_duration {
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
                                                        )
                                                        .child(
                                                            div()
                                                                .id("album-enqueue")
                                                                .debug_selector(|| "album-enqueue".to_string())
                                                                .track_focus(&f_album_enqueue)
                                                                .tab_index(Self::TAB_CONTENT)
                                                                .role(gpui::Role::Button)
                                                                .aria_label("Add all tracks to the queue")
                                                                .border_2()
                                                                .border_color(self.focus_border(
                                                                    &f_album_enqueue,
                                                                    window,
                                                                    t.glass_border,
                                                                    t,
                                                                ))
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
                                                                .on_click(cx.listener(move |this, _event, _window, cx| {
                                                                    this.enqueue_tracks(songs_all_queue.clone(), cx);
                                                                }))
                                                                .child("Add to queue"),
                                                        ),
                                                ),
                                        ),
                                );
                                let songs = std::sync::Arc::new(detail.songs.clone());
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
                                            None,
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
                                            this.play_tracks((*songs2).clone(), i, cx);
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
            .child(queue_panel)
            // Failure surface. Playback, autoplay and library errors are all
            // stored in `self.toast`; without this element they were stored and
            // thrown away, so every async failure was silent.
            .child(match self.toast.clone() {
                Some(message) => {
                    let entity = cx.entity();
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(shadow_size + gpui::px(118.0))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .id("toast")
                                .debug_selector(|| "toast".to_string())
                                .role(gpui::Role::Alert)
                                .aria_label(message.clone())
                                .px(gpui::px(14.0))
                                .py(gpui::px(9.0))
                                .rounded(gpui::px(999.0))
                                .bg(t.clay_surface)
                                .border_1()
                                .border_color(t.danger)
                                .text_size(gpui::px(12.0))
                                .text_color(t.clay_ink)
                                .cursor_pointer()
                                .on_click(move |_event, _window, app| {
                                    app.update_entity(&entity, |this, cx| {
                                        this.toast = None;
                                        cx.notify();
                                    });
                                })
                                .child(message),
                        )
                        .into_any_element()
                }
                None => div().into_any_element(),
            })
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

/// "3 days ago" style age, for the offline-copy hint.
fn humanise_age(seconds: u64) -> String {
    let minutes = seconds / 60;
    match minutes {
        0 => "just now".to_string(),
        1..=59 => format!("{minutes} min ago"),
        _ => {
            let hours = minutes / 60;
            match hours {
                1..=23 => format!("{hours} h ago"),
                _ => {
                    let days = hours / 24;
                    if days == 1 {
                        "1 day ago".to_string()
                    } else {
                        format!("{days} days ago")
                    }
                }
            }
        }
    }
}

/// Submit a scrobble, retrying the failures that could plausibly succeed.
///
/// The result used to be discarded with `let _ =`, so a rate limit or a blip lost
/// the scrobble silently and permanently.
fn submit_scrobble(client: &NavidromeClient, track_id: &str, submission: bool, timestamp_ms: i64) {
    let mut attempt = 0u8;
    loop {
        attempt = attempt.saturating_add(1);
        match client.scrobble(track_id, submission, timestamp_ms) {
            Ok(()) => return,
            Err(error) if error.is_retryable() && attempt < 3 => {
                std::thread::sleep(error.retry_delay(attempt));
            }
            Err(error) => {
                tracing::warn!("scrobble for {track_id} was dropped: {error}");
                return;
            }
        }
    }
}

/// Page size when walking the album list during a sync.
const SYNC_ALBUM_PAGE: u32 = 500;
/// A hard stop so a misbehaving server cannot make a sync run forever.
const SYNC_MAX_ALBUMS: usize = 50_000;

/// Fetch the whole library: every album page, every album's tracks, the artist
/// list and the playlists.
///
/// One request per album is the only way the Subsonic API exposes a complete
/// song list, so this is deliberately an explicit, progress-reported action
/// rather than something that happens on startup.
fn fetch_full_library(
    client: &NavidromeClient,
    progress: &std::sync::atomic::AtomicUsize,
) -> Result<crate::library::cache::LibrarySnapshot, crate::navidrome::error::NavidromeError> {
    use crate::navidrome::client::AlbumListKind;
    use std::sync::atomic::Ordering;

    let mut albums = Vec::new();
    let mut offset = 0u32;
    loop {
        let page = client.album_list(AlbumListKind::Newest, SYNC_ALBUM_PAGE, offset)?;
        let fetched = page.len();
        albums.extend(page);
        if fetched < SYNC_ALBUM_PAGE as usize || albums.len() >= SYNC_MAX_ALBUMS {
            break;
        }
        offset = offset.saturating_add(fetched as u32);
    }

    let artists = client.artists()?;
    let playlists = client.playlists()?;

    let mut songs = Vec::new();
    for album in &albums {
        progress.fetch_add(1, Ordering::Relaxed);
        // A single unreadable album must not lose the whole sync.
        match client.album(&album.id) {
            Ok(detail) => songs.extend(detail.songs),
            Err(error) => tracing::warn!("sync: skipping album {}: {error}", album.id),
        }
    }

    Ok(crate::library::cache::LibrarySnapshot {
        version: crate::library::cache::SNAPSHOT_VERSION,
        fetched_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        albums,
        artists,
        playlists,
        songs,
    })
}

/// A section heading inside a results page (SONGS, ALBUMS, …).
fn section_label(label: &'static str, t: &crate::theme::Colors) -> impl IntoElement {
    div()
        .text_size(gpui::px(11.0))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(t.text_secondary)
        .child(label)
}

/// Turn a library/server failure into something a user can act on.
///
/// The UI used to print `Debug` (`Server("HTTP 500")`), which tells the user
/// nothing about whether the password, the network or the server is at fault.
fn error_text(error: &crate::navidrome::error::NavidromeError) -> String {
    match error.hint() {
        Some(hint) => format!("{} {}", error.user_message(), hint),
        None => error.user_message().to_string(),
    }
}

/// Fetch a remote cover and return bytes GPUI can decode.
///
/// YouTube serves WebP for some thumbnails. GPUI's image loader at the pinned
/// revision expects PNG/JPEG, so anything else is decoded and re-encoded once,
/// on the background executor. The 5 MiB cap keeps a hostile URL from becoming
/// a memory problem.
/// Legacy dynamic keys carry only an id. Provider ids are namespaced (`yt:`),
/// so the source can be recovered without threading it through the key.
fn source_for_id(id: &str) -> TrackSource {
    if id.starts_with("yt:") {
        TrackSource::YouTube
    } else {
        TrackSource::Navidrome
    }
}

fn fetch_remote_cover(url: &str) -> Option<Vec<u8>> {
    const MAX_COVER_BYTES: usize = 5 * 1024 * 1024;
    let parsed = url::Url::parse(url).ok()?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return None;
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .user_agent("Myuzik/0.1 (https://github.com/SamVivan1/myuzik)")
        .timeout_connect(Some(std::time::Duration::from_secs(8)))
        .timeout_recv_response(Some(std::time::Duration::from_secs(15)))
        .timeout_global(Some(std::time::Duration::from_secs(25)))
        .build()
        .into();
    let bytes = agent
        .get(url)
        .header("Accept", "image/jpeg,image/png")
        .call()
        .ok()?
        .into_body()
        .read_to_vec()
        .ok()?;
    if bytes.is_empty() || bytes.len() > MAX_COVER_BYTES {
        return None;
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) || bytes.starts_with(&[0xFF, 0xD8]) {
        return Some(bytes);
    }
    let image = image::load_from_memory(&bytes).ok()?;
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    (!png.is_empty()).then_some(png)
}

fn provider_search_error_text(error: &crate::provider::ProviderError) -> String {
    use crate::provider::ProviderError;
    match error {
        ProviderError::Unavailable(_) => {
            "YouTube search failed. Try a different query.".to_string()
        }
        other => provider_error_text(other),
    }
}

fn provider_error_text(error: &crate::provider::ProviderError) -> String {
    use crate::provider::ProviderError;
    match error {
        ProviderError::HelperMissing => {
            "YouTube support is not installed. Run ./install.sh --with-youtube.".to_string()
        }
        ProviderError::PythonMissing => {
            "Python 3 was not found. Install python3 and the YouTube helper.".to_string()
        }
        ProviderError::NodeMissing => "Node.js 20+ is required for YouTube playback.".to_string(),
        ProviderError::Timeout => "YouTube took too long to respond. Try again.".to_string(),
        ProviderError::Cancelled => "YouTube search cancelled.".to_string(),
        ProviderError::Unavailable(_) => {
            "This track isn't available on YouTube. Try another result.".to_string()
        }
        ProviderError::SignInRequired => {
            "This track needs sign-in. Set youtube_cookies in config.toml.".to_string()
        }
        ProviderError::TooLarge => "This track is too large to buffer (limit 32 MB).".to_string(),
        ProviderError::Network(_) => {
            "Couldn't reach YouTube. Check your connection and try again.".to_string()
        }
        ProviderError::Protocol(message) => {
            format!("YouTube returned an unexpected response: {message}")
        }
    }
}

fn spawn_home_fetch(
    client: Arc<NavidromeClient>,
    hs: gpui::Entity<HomeState>,
    entity: gpui::Entity<Shell>,
    covers: Arc<crate::library::cover_cache::CoverCache>,
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
                let msg = format!("Library error. {}", error_text(&e));
                cx.update(|app| {
                    app.update_entity(&hs, |h, _| {
                        h.loading = false;
                        h.error = Some(msg.clone());
                    });
                    entity.update(app, |shell, cx| {
                        shell.show_toast(msg, cx);
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
        // PF-07: fetch the (up to 24) cover thumbnails concurrently instead of
        // serially awaiting them one by one. Each task still honours the shared
        // cover cache (read-through + in-flight claim), so dedup is preserved.
        let covers_dir_for_grid = covers.clone();
        let mut futures_list: Vec<_> = Vec::new();
        for album in albums.iter().take(24) {
            if let Some(cover_id) = &album.cover_art_id {
                let cid_bg = cover_id.clone();
                let client_bg = client.clone();
                let covers_bg = covers_dir_for_grid.clone();
                let fut = cx.background_executor().spawn(async move {
                    if let Some(bytes) = covers_bg.read(&cid_bg) {
                        return (
                            cid_bg,
                            Ok::<Vec<u8>, crate::navidrome::error::NavidromeError>(bytes),
                        );
                    }
                    let Some(_claim) = covers_bg.begin_fetch(&cid_bg) else {
                        return (
                            cid_bg,
                            Err(crate::navidrome::error::NavidromeError::Protocol(
                                "cover already downloading".to_string(),
                            )),
                        );
                    };
                    let res = client_bg.cover_bytes(&cid_bg, 512).inspect(|bytes| {
                        let _ = covers_bg.write(&cid_bg, bytes);
                    });
                    (cid_bg, res)
                });
                futures_list.push(fut);
            }
        }
        let results = futures::future::join_all(futures_list).await;
        for (cover_id, fetched) in results {
            if let Ok(bytes) = fetched {
                let format = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
                    gpui::ImageFormat::Png
                } else {
                    gpui::ImageFormat::Jpeg
                };
                let image = std::sync::Arc::new(gpui::Image {
                    format,
                    bytes,
                    id: fxhash_id(&cover_id),
                });
                let _ = covers_dir_for_grid.write(&cover_id, &image.bytes);
                let cid = cover_id.clone();
                cx.update(|app| {
                    app.update_entity(&hs, |h, _| {
                        h.insert_cover(cid, image);
                    });
                });
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
        .text_size(gpui::px(13.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(t.text_secondary)
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

/// Maximum number of decoded cover images kept in memory (`HomeState::covers`).
///
/// Exported so the stress test asserts the real bound instead of repeating it.
pub const COVER_CACHE_CAP: usize = 200;

#[derive(Clone, Default)]
pub struct HomeState {
    /// True while the lists came from the disk snapshot rather than the server.
    pub from_cache: bool,
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

/// Time-aware greeting for the Home screen (UI-30). Uses the local clock, not UTC.
fn greeting_for_now() -> &'static str {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&now, &mut tm);
    }
    match tm.tm_hour {
        0..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    }
}
