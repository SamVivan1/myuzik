use crate::app::Route;
use crate::config::{AppConfig, Paths};
use crate::navidrome::client::NavidromeClient;
use crate::lyrics::provider::LyricsProvider;
use std::sync::Arc;
use crate::platform::mpris::MprisHandle;
use crate::player::engine::{PlayerCommand, PlayerEvent, PlayerHandle};
use crate::ui::now_playing::NowPlayingView;
use crate::ui::search::SearchView;
use crate::ui::settings::SettingsView;
use crate::player::state::{PlayerState, PlaybackStatus};
use crate::theme::midnight_brass;
use gpui::{AnimationExt, Context, InteractiveElement, Render, StatefulInteractiveElement, Window, div, prelude::*};

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
    search_query: String,
    search_results: Option<crate::library::SearchResults>,
    search_focus: gpui::FocusHandle,
    now_playing: gpui::Entity<NowPlayingView>,
    search_view: gpui::Entity<SearchView>,
    settings_view: gpui::Entity<SettingsView>,
    mpris_handle: Option<MprisHandle>,
    overlay_state: gpui::Entity<crate::overlay::OverlayState>,
    overlay_handle: Option<gpui::WindowHandle<crate::overlay::PinnedLyricsView>>,
}

impl Shell {
    pub fn new(cx: &mut gpui::App, player_handle: PlayerHandle, config: AppConfig, paths: Paths, mpris_handle: Option<MprisHandle>) -> Self {
        let player_state = cx.new(|_cx| PlayerState::new());
        let home_state = cx.new(|_cx| HomeState::default());
        let selected_album = None;
        let album_detail = None;
        let album_loading = false;
        let search_query = String::new();
        let search_results = None;
        let search_focus = cx.focus_handle();
        let now_playing = cx.new(|cx| NowPlayingView::new(cx));
        let search_view = cx.new(|_cx| SearchView::new());
        let settings_view = cx.new(|_cx| SettingsView::new(config.clone()));
        let overlay_state = cx.new(|_cx| crate::overlay::OverlayState::default());
        let overlay_handle = None;
        let password = crate::config::load_password(&paths);
        let client = if !config.server_url.is_empty() {
            password.and_then(|pw| NavidromeClient::new(&config.server_url, &config.username, &pw).ok()).map(Arc::new)
        } else { None };
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
        let route = if client.is_none() { Route::Setup } else { Route::Home };
        // Poll player events in background
        let state = player_state.clone();
        let rx = player_handle.rx.clone();
        let mpris_for_loop = mpris_handle.clone();
        let now_playing_for_loop = now_playing.clone();
        let overlay_for_loop = overlay_state.clone();
        // Fetch home data asynchronously
        if let Some(client) = client.clone() {
            let hs = home_state.clone();
            cx.spawn(async move |cx| {
                cx.update(|app| {
                    app.update_entity(&hs, |h, _| { h.loading = true; });
                });
                let client_bg = client.clone();
                let res = cx.background_executor().spawn(async move {
                    client_bg.album_list(crate::navidrome::client::AlbumListKind::Newest, 100, 0)
                }).await;
                let albums = match res {
                    Ok(a) => a,
                    Err(e) => {
                        cx.update(|app| {
                            app.update_entity(&hs, |h, _| {
                                h.loading = false;
                                h.error = Some(format!("{:?}", e));
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
                let artists = cx.background_executor().spawn(async move { client_bg.artists() }).await.unwrap_or_default();
                let client_bg = client.clone();
                let playlists = cx.background_executor().spawn(async move { client_bg.playlists() }).await.unwrap_or_default();
                cx.update(|app| {
                    app.update_entity(&hs, |h, _| {
                        h.artists = artists;
                        h.playlists = playlists;
                    });
                });
                // Fetch covers for the first 8 albums
                for album in albums.iter().take(24) {
                    if let Some(cover_id) = &album.cover_art_id {
                        let cid_bg = cover_id.clone();
                        let client_bg = client.clone();
                        let fetched = cx.background_executor().spawn(async move {
                            client_bg.cover_bytes(&cid_bg, 512)
                        }).await;
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
                            let cid = cover_id.clone();
                            cx.update(|app| {
                                app.update_entity(&hs, |h, _| {
                                    h.covers.insert(cid, image);
                                });
                            });
                        }
                    }
                }
            }).detach();
        }
        cx.spawn(async move |cx| {
            loop {
                match rx.recv_async().await {
                    Ok(PlayerEvent::StateChanged(s)) => {
                        cx.update(|app| {
                            state.update(app, |ps, _cx| {
                                ps.set_status(s);
                            });
                        });
                        if let Some(mh) = &mpris_for_loop { mh.update_status(s); }
                    }
                    Ok(PlayerEvent::Position(pos)) => {
                        cx.update(|app| {
                            state.update(app, |ps, _cx| {
                                ps.set_position(pos);
                            });
                        });
                        if let Some(mh) = &mpris_for_loop { mh.update_position(pos); }
                        let np = now_playing_for_loop.clone();
                        let dur = cx.update(|app| state.read(app).duration.map(|d| d.as_secs_f64()).unwrap_or(0.0));
                        cx.update(|app| {
                            app.update_entity(&np, |v, cx| {
                                v.set_position(cx, pos.as_secs_f64(), dur);
                            });
                        });
                        let os = overlay_for_loop.clone();
                        cx.update(|app| {
                            let lines = os.read(app).lines.clone();
                            let idx = crate::lyrics::lrc::active_line(&lines, pos);
                            app.update_entity(&os, |st, _cx| {
                                st.active = idx;
                            });
                        });
                    }
                    Ok(PlayerEvent::Duration(dur)) => {
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
                        });
                    }
                    Ok(PlayerEvent::Error(e)) => {
                        tracing::error!("player error: {:?}", e);
                    }
                    Ok(PlayerEvent::TrackStarted(id)) => {
                        tracing::info!("track started: {}", id);
                    }
                    Ok(PlayerEvent::Buffering(p)) => {
                        tracing::debug!("buffering: {}%", p);
                    }
                    Err(_) => break,
                }
            }
        }).detach();

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
            search_query,
            search_results,
            search_focus,
            now_playing,
            search_view,
            settings_view,
            overlay_state,
            overlay_handle,
            mpris_handle,
        }
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

    pub fn open_overlay(&mut self, cx: &mut gpui::App) {
        let opts = gpui::WindowOptions {
            window_background: gpui::WindowBackgroundAppearance::Blurred,
            window_decorations: None,
            focus: false,
            show: true,
            app_id: Some(crate::overlay::APP_ID.to_string()),
            titlebar: None,
            ..Default::default()
        };
        if let Some(handle) = self.overlay_handle {
            if handle.update(cx, |_view, _window, _cx| {}).is_err() {
                self.overlay_handle = None;
            } else {
                return;
            }
        }
        let state = self.overlay_state.clone();
        let handle = cx.open_window(opts, move |_window, cx| {
            cx.new(|_cx| crate::overlay::PinnedLyricsView::new(state))
        });
        if let Ok(h) = handle {
            h.update(cx, |_view, window, _cx| {
                window.set_window_title(crate::overlay::OVERLAY_TITLE);
            })
            .ok();
            self.overlay_handle = Some(h);
        }
    }

    pub fn reload_config(&mut self, cx: &mut gpui::App) {
        if let Ok(cfg) = crate::config::load_config(&self.paths) {
            self.config = cfg.clone();
            let password = crate::config::load_password(&self.paths);
            self.client = if !cfg.server_url.is_empty() {
                password.and_then(|pw| NavidromeClient::new(&cfg.server_url, &cfg.username, &pw).ok()).map(Arc::new)
            } else { None };
            self.route = if cfg.server_url.is_empty() { Route::Setup } else { Route::Home };
            if let Some(client) = self.client.clone() {
                let hs = self.home_state.clone();
                cx.spawn(async move |cx| {
                    let client_bg = client.clone();
                let res = cx.background_executor().spawn(async move {
                    client_bg.album_list(crate::navidrome::client::AlbumListKind::Newest, 100, 0)
                }).await;
                    cx.update(|app| {
                        app.update_entity(&hs, |h, _| {
                            h.loading = false;
                            match res {
                                Ok(albums) => h.albums = albums,
                                Err(e) => h.error = Some(format!("{:?}", e)),
                            }
                        });
                    });
                }).detach();
            }
        }
    }

    pub fn play_track(&mut self, track: crate::library::Track, cx: &mut gpui::App) {
        self.player_state.update(cx, |ps, _cx| {
            // play_now with single track
            ps.play_now(vec![track.clone()], 0);
        });
        // send load+play to engine
        if let Some(client) = &self.client
            && let Ok(uri) = client.stream_url(&track.id, self.config.quality) {
            self.player_handle.tx.send(PlayerCommand::Load { uri, track_id: track.id.clone() }).ok();
            self.player_handle.tx.send(PlayerCommand::Play).ok();
        }
        // fetch lyrics
        if let Some(client) = &self.client {
            let c = client.clone();
            let t = track.clone();
            let _ps = self.player_state.clone();
            let np = self.now_playing.clone();
            cx.spawn(async move |cx| {
                let t_bg = t.clone();
                let c_bg = c.clone();
                let status = cx.background_executor().spawn(async move {
                    c_bg.lyrics(&t_bg.id).ok().flatten().unwrap_or(crate::lyrics::model::LyricsStatus::NotFound)
                }).await;
                cx.update(|app| {
                    app.update_entity(&np, |v, cx| {
                        v.set_track(t.title.clone(), t.artist.clone(), t.album.clone());
                        v.set_lyrics(cx, status);
                    });
                });
            }).detach();
        }
    }

    pub fn play_tracks(&mut self, tracks: Vec<crate::library::Track>, start: usize, cx: &mut gpui::App) {
        if tracks.is_empty() || start >= tracks.len() {
            return;
        }
        self.player_state.update(cx, |ps, _cx| {
            ps.play_now(tracks.clone(), start);
        });
        self.load_and_play(&tracks[start], cx);
    }

    pub fn play_next(&mut self, cx: &mut gpui::App) {
        let next = self.player_state.update(cx, |ps, _cx| ps.next());
        if let Some(track) = next {
            self.load_and_play(&track, cx);
        }
    }

    pub fn play_previous(&mut self, cx: &mut gpui::App) {
        let prev = self.player_state.update(cx, |ps, _cx| ps.previous());
        if let Some(track) = prev {
            self.load_and_play(&track, cx);
        }
    }

    fn load_and_play(&mut self, track: &crate::library::Track, cx: &mut gpui::App) {
        if let Some(client) = &self.client
            && let Ok(uri) = client.stream_url(&track.id, self.config.quality) {
            self.player_handle.tx.send(PlayerCommand::Load { uri, track_id: track.id.clone() }).ok();
            self.player_handle.tx.send(PlayerCommand::Play).ok();
        }
        if let Some(provider) = &self.lyrics_provider {
            let p = provider.clone();
            let t = track.clone();
            let np = self.now_playing.clone();
            let os = self.overlay_state.clone();
            let cover = track.cover_art_id.as_ref()
                .and_then(|cid| self.home_state.read(cx).covers.get(cid).cloned());
            cx.spawn(async move |cx| {
                let t_bg = t.clone();
                let p_bg = p.clone();
                let status = cx.background_executor().spawn(async move {
                    p_bg.fetch(&t_bg).unwrap_or(crate::lyrics::model::LyricsStatus::NotFound)
                }).await;
                cx.update(|app| {
                    app.update_entity(&np, |v, cx| {
                        v.set_track(t.title.clone(), t.artist.clone(), t.album.clone());
                        v.set_cover(cover.clone());
                        v.set_lyrics(cx, status.clone());
                    });
                    app.update_entity(&os, |st, _cx| {
                        st.title = t.title.clone();
                        st.artist = t.artist.clone();
                        st.active = None;
                        st.lines = match status {
                            crate::lyrics::model::LyricsStatus::Synced(lines) => lines,
                            crate::lyrics::model::LyricsStatus::Plain(text) => text
                                .lines()
                                .map(|l| crate::lyrics::model::LyricLine {
                                    time: std::time::Duration::ZERO,
                                    text: l.to_string(),
                                })
                                .collect(),
                            _ => Vec::new(),
                        };
                    });
                });
            }).detach();
        }
    }

    pub fn close_overlay(&mut self, cx: &mut gpui::App) {
        if let Some(handle) = self.overlay_handle.take() {
            let _ = handle.update(cx, |_view, window, _cx| window.remove_window());
        }
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = midnight_brass();
        let t = &theme.colors;

        if self.route == Route::Setup {
            let cfg_path = self.paths.config.display().to_string();
            let cred_path = crate::config::credentials_path(&self.paths).display().to_string();
            return div()
                .size_full()
                .bg(t.bg_base)
                .text_color(t.text_primary)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
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
                                .text_size(gpui::px(12.0))
                                .text_color(t.text_secondary)
                                .child("Edit the config file below, then press Reload. Restart the app after changing the server URL."),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(gpui::px(2.0))
                                .p(gpui::px(12.0))
                                .rounded(gpui::px(12.0))
                                .bg(gpui::rgba(0xFFFFFF05))
                                .child(div().text_size(gpui::px(11.0)).child(cfg_path))
                                .child(div().text_size(gpui::px(9.0)).text_color(t.text_tertiary).child("[server_url] [username] [quality]"))
                                .child(div().text_size(gpui::px(11.0)).child(cred_path))
                                .child(div().text_size(gpui::px(9.0)).text_color(t.text_tertiary).child("password only (chmod 600)")),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(gpui::px(10.0))
                                .child(
                                    div()
                                        .px(gpui::px(18.0))
                                        .py(gpui::px(10.0))
                                        .rounded(gpui::px(999.0))
                                        .bg(t.gold)
                                        .text_color(t.clay_ink)
                                        .text_size(gpui::px(11.0))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .cursor_pointer()
                                        .id("setup-reload")
                                        .on_click(cx.listener(|this, _event, _window, cx| {
                                            this.reload_config(cx);
                                        }))
                                        .child("Reload config"),
                                ),
                        ),
                )
                .into_any_element();
        }

        let status = self.player_state.read(cx).status;
        let is_playing = status == PlaybackStatus::Playing;
        let current_title = self.player_state.read(cx).current.as_ref().map(|t| t.title.clone()).unwrap_or_default();
        let current_artist = self.player_state.read(cx).current.as_ref().map(|t| t.artist.clone()).unwrap_or_default();
        let current_cover = self.player_state.read(cx).current.as_ref()
            .and_then(|t| t.cover_art_id.clone())
            .and_then(|cid| self.home_state.read(cx).covers.get(&cid).cloned());
        let position = self.player_state.read(cx).position;
        let duration = self.player_state.read(cx).duration.unwrap_or(std::time::Duration::ZERO);

        let progress_pct = if duration.as_secs_f64() > 0.0 {
            (position.as_secs_f64() / duration.as_secs_f64() * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        // Player bar
        let player = div()
            .absolute()
            .left(gpui::px(18.0))
            .right(gpui::px(18.0))
            .bottom(gpui::px(16.0))
            .h(gpui::px(88.0))
            .rounded(gpui::px(30.0))
            .bg(gpui::linear_gradient(
                145.0,
                gpui::linear_color_stop(gpui::rgb(0xF7F0E4), 0.0),
                gpui::linear_color_stop(gpui::rgb(0xEADCC4), 1.0),
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
                    color: gpui::rgba(0x8C6D3F33).into(),
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
                    .on_click(cx.listener(|this, _event, _window, _cx| {
                        this.route = Route::NowPlaying;
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
                                    .text_size(gpui::px(12.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(t.clay_ink)
                                    .child(if current_title.is_empty() { "Nothing playing".to_string() } else { current_title.clone() }),
                            )
                            .child(
                                div()
                                    .text_size(gpui::px(9.0))
                                    .text_color(t.clay_ink)
                                    .opacity(0.7)
                                    .child(current_artist.clone()),
                            ),
                    ),
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
                                    .w(gpui::px(24.0))
                                    .h(gpui::px(24.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(t.clay_ink)
                                    .cursor_pointer()
                                    .id("prev-btn")
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
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.play_next(cx);
                                    }))
                                    .child("⏭"),
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
                            .w(gpui::px(60.0))
                            .h(gpui::px(5.0))
                            .rounded(gpui::px(999.0))
                            .bg(t.clay_shade)
                            .child(
                                div()
                                    .w(gpui::px(37.0))
                                    .h_full()
                                    .rounded(gpui::px(999.0))
                                    .bg(t.os_accent),
                            ),
                    )
                    .child(
                        div()
                            .px(gpui::px(10.0))
                            .py(gpui::px(6.0))
                            .rounded(gpui::px(999.0))
                            .bg(gpui::rgb(0x2A241C))
                            .text_color(t.gold_light)
                            .text_size(gpui::px(8.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .cursor_pointer()
                            .id("pin-btn")
                            .on_click(cx.listener(|this, _event, _window, _cx| {
                                if this.overlay_handle.is_some() { this.close_overlay(_cx); } else { this.open_overlay(_cx); }
                            }))
                            .child("Pin lyrics"),
                    ),
            );

        // Glass shell with home content
        div()
            .size_full()
            .bg(t.bg_base)
            .text_color(t.text_primary)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(gpui::linear_gradient(
                        135.0,
                        gpui::linear_color_stop(gpui::rgba(0x1C140BFF), 0.0),
                        gpui::linear_color_stop(gpui::rgba(0x0B0B0EFF), 1.0),
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
                    .bg(gpui::rgba(0xC9A86A3A))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::rgba(0xC9A86A55).into(),
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
                    .bg(gpui::rgba(0x2F6B6330))
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
                    .bg(gpui::rgba(0x7A4A5528))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::rgba(0x7A4A5540).into(),
                        offset: gpui::point(gpui::px(0.0), gpui::px(0.0)),
                        blur_radius: gpui::px(220.0),
                        spread_radius: gpui::px(0.0),
                        inset: false,
                    }]),
            )
            .child(
                div()
                    .absolute()
                    .inset(gpui::px(24.0))
                    .rounded(gpui::px(24.0))
                    .bg(gpui::linear_gradient(
                        135.0,
                        gpui::linear_color_stop(gpui::rgba(0x0A0A0DA8), 0.0),
                        gpui::linear_color_stop(gpui::rgba(0x14100A96), 1.0),
                    ))
                    .border_1()
                    .border_color(t.glass_border)
                    .shadow(vec![
                        gpui::BoxShadow {
                            color: gpui::hsla(0.0, 0.0, 0.0, 0.6),
                            offset: gpui::point(gpui::px(0.0), gpui::px(40.0)),
                            blur_radius: gpui::px(100.0),
                            spread_radius: gpui::px(0.0),
                            inset: false,
                        },
                        gpui::BoxShadow {
                            color: gpui::rgba(0xFFFFFF0D).into(),
                            offset: gpui::point(gpui::px(0.0), gpui::px(1.0)),
                            blur_radius: gpui::px(0.0),
                            spread_radius: gpui::px(0.0),
                            inset: true,
                        },
                    ])
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(
                        // Titlebar
                        div()
                            .id("titlebar")
                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|_this, _event, window, _cx| window.start_window_move()))
                            .h(gpui::px(38.0))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(gpui::px(10.0))
                            .px(gpui::px(15.0))
                            .bg(gpui::rgba(0xFFFFFF06))
                            .border_b_1()
                            .border_color(gpui::rgba(0xE8D3A314))
                            .child(
                                div()
                                    .flex()
                                    .gap(gpui::px(8.0))
                                    .child(
                                        div()
                                            .id("win-close")
                                            .cursor_pointer()
                                            .w(gpui::px(12.0))
                                            .h(gpui::px(12.0))
                                            .rounded(gpui::px(999.0))
                                            .bg(gpui::rgb(0xFF5F57))
                                            .border_1()
                                            .border_color(gpui::rgba(0x00000033))
                                            .hover(|style| style.bg(gpui::rgb(0xFF7B73)))
                                            .on_click(cx.listener(|_this, _event, window, _cx| window.remove_window())),
                                    )
                                    .child(
                                        div()
                                            .id("win-min")
                                            .cursor_pointer()
                                            .w(gpui::px(12.0))
                                            .h(gpui::px(12.0))
                                            .rounded(gpui::px(999.0))
                                            .bg(gpui::rgb(0xFEBC2E))
                                            .border_1()
                                            .border_color(gpui::rgba(0x00000033))
                                            .hover(|style| style.bg(gpui::rgb(0xFFD04A)))
                                            .on_click(cx.listener(|_this, _event, window, _cx| window.minimize_window())),
                                    )
                                    .child(
                                        div()
                                            .id("win-max")
                                            .cursor_pointer()
                                            .w(gpui::px(12.0))
                                            .h(gpui::px(12.0))
                                            .rounded(gpui::px(999.0))
                                            .bg(gpui::rgb(0x28C840))
                                            .border_1()
                                            .border_color(gpui::rgba(0x00000033))
                                            .hover(|style| style.bg(gpui::rgb(0x4AE05F)))
                                            .on_click(cx.listener(|_this, _event, window, _cx| window.zoom_window())),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .justify_center()
                                    .text_size(gpui::px(10.0))
                                    .text_color(t.text_tertiary)
                                    .child("myuzik"),
                            )
                            .child(div().w(gpui::px(52.0))),
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
                            .py(gpui::px(18.0))
                            .bg(gpui::rgba(0xFFFFFF05))
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
                                    .child(nav_item("Home", self.route == Route::Home, t).id("nav-home").on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::Home; this.selected_album = None; this.album_detail = None; })))
                                    .child(nav_item("Now Playing", self.route == Route::NowPlaying, t).id("nav-nowplaying").on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::NowPlaying; })))
                                    .child(nav_item("Search", self.route == Route::Search, t).id("nav-search").on_click(cx.listener(|this, _event, window, cx| { this.route = Route::Search; window.focus(&this.search_focus, cx); })))
                                    .child(nav_item("Albums", self.route == Route::Albums, t).id("nav-albums").on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::Albums; })))
                                    .child(nav_item("Artists", self.route == Route::Artists, t).id("nav-artists").on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::Artists; })))
                                    .child(nav_item("Playlists", self.route == Route::Playlists, t).id("nav-playlists").on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::Playlists; })))
                                    .child(nav_item("Favorites", self.route == Route::Favorites, t).id("nav-favorites").on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::Favorites; })))
                                    .child(nav_item("Settings", self.route == Route::Settings, t).id("nav-settings").on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::Settings; }))),
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
                                            .bg(gpui::rgb(0x2A241C))
                                            .text_color(t.gold_light)
                                            .font_family(theme.ui_font)
                                            .text_size(gpui::px(9.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .cursor_pointer()
                                            .id("pin-btn")
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
                            .child(
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
                                                    .text_size(gpui::px(9.0))
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(t.gold)
                                                    .child(match self.route {
                                                        crate::app::Route::Home => "YOUR LIBRARY",
                                                        crate::app::Route::Search => "SEARCH",
                                                        crate::app::Route::NowPlaying => "NOW PLAYING",
                                                        crate::app::Route::Settings => "SETTINGS",
                                                        _ => "MYUZIK",
                                                    }),
                                            )
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
                                            .text_color(t.clay_ink)
                                            .text_size(gpui::px(11.0))
                                            .on_click(cx.listener(|this, _event, window, cx| {
                                                this.route = Route::Search;
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
                                    ),
                            )
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
                                            .cursor_pointer()
                                            .text_size(gpui::px(10.0))
                                            .text_color(t.text_secondary)
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .on_click(cx.listener(|this, _event, _window, _cx| { this.route = Route::Albums; }))
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
                                                } else if k == "enter" {
                                                    let q = this.search_query.trim().to_string();
                                                    if !q.is_empty()
                                                        && let Some(client) = this.client.clone() {
                                                        let entity2 = cx.entity();
                                                        cx.spawn(async move |_, cx| {
                                                            let res = cx.background_executor().spawn(async move { client.search(&q) }).await;
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
                                        .border_color(t.os_accent)
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
                                            col = col.child(
                                                div()
                                                    .id(format!("search-song-{}", i))
                                                    .cursor_pointer()
                                                    .px(gpui::px(12.0))
                                                    .py(gpui::px(8.0))
                                                    .rounded(gpui::px(10.0))
                                                    .bg(t.glass_tint)
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
                                div().child(self.settings_view.clone())
                            } else if self.route == Route::Artists {
                                let hs = self.home_state.read(cx);
                                let mut col = div().flex().flex_col().gap(gpui::px(4.0));
                                for a in &hs.artists {
                                    col = col.child(
                                        div()
                                            .px(gpui::px(12.0))
                                            .py(gpui::px(9.0))
                                            .rounded(gpui::px(10.0))
                                            .bg(t.glass_tint)
                                            .border_1()
                                            .border_color(t.glass_border)
                                            .text_size(gpui::px(12.0))
                                            .child(a.name.clone()),
                                    );
                                }
                                col
                            } else if self.route == Route::Playlists {
                                let hs = self.home_state.read(cx);
                                let mut col = div().flex().flex_col().gap(gpui::px(4.0));
                                for p in &hs.playlists {
                                    col = col.child(
                                        div()
                                            .px(gpui::px(12.0))
                                            .py(gpui::px(9.0))
                                            .rounded(gpui::px(10.0))
                                            .bg(t.glass_tint)
                                            .border_1()
                                            .border_color(t.glass_border)
                                            .text_size(gpui::px(12.0))
                                            .child(format!("{} · {} tracks", p.name, p.song_count)),
                                    );
                                }
                                col
                            } else if self.selected_album.is_some() {
                                div()
                            } else {
                                let hs = self.home_state.read(cx);
                                let mut grid = div().grid().grid_cols(4).gap(gpui::px(14.0));
                                if hs.albums.is_empty() {
                                    grid = grid.child(album_card("No albums", "Connect a server", None, t));
                                } else {
                                    let limit = if self.route == Route::Albums { hs.albums.len() } else { 8 };
                                    for a in hs.albums.iter().take(limit) {
                                        let aid = a.id.clone();
                                        let cover = a.cover_art_id.as_ref().and_then(|cid| hs.covers.get(cid)).cloned();
                                        grid = grid.child(
                                            album_card(&a.name, &a.artist, cover, t)
                                                .id(format!("album-{}", aid))
                                                .cursor_pointer()
                                                .on_click(cx.listener(move |this, _event, _window, cx| {
                                                    let aid2 = aid.clone();
                                                    this.selected_album = Some(aid2.clone());
                                                    this.album_detail = None;
                                                    this.album_loading = true;
                                                    if let Some(client) = this.client.clone() {
                                                        let entity = cx.entity();
                                                        let aid3 = aid2.clone();
                                                        cx.spawn(async move |_, cx| {
                                                            let client_bg = client.clone();
                                                            let res = cx.background_executor().spawn(async move {
                                                                client_bg.album(&aid3)
                                                            }).await;
                                                            cx.update(|app| {
                                                                app.update_entity(&entity, |shell, _cx| {
                                                                    shell.album_loading = false;
                                                                    if let Ok(detail) = res {
                                                                        shell.album_detail = Some(detail);
                                                                    }
                                                                });
                                                            });
                                                        }).detach();
                                                    }
                                                })),
                                        );
                                    }
                                }
                                grid
                            })
                            .child(if let Some(detail) = &self.album_detail {
                                let mut col = div().flex().flex_col().gap(gpui::px(6.0));
                                let songs_all = detail.songs.clone();
                                let album_name = detail.album.name.clone();
                                let album_artist = detail.album.artist.clone();
                                col = col.child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(gpui::px(8.0))
                                        .child(
                                            div()
                                                .id("album-back")
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
                                                .text_size(gpui::px(26.0))
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .child(album_name),
                                        )
                                        .child(
                                            div()
                                                .text_size(gpui::px(11.0))
                                                .text_color(t.text_secondary)
                                                .child(album_artist),
                                        )
                                        .child(
                                            div()
                                                .id("album-play-all")
                                                .cursor_pointer()
                                                .px(gpui::px(16.0))
                                                .py(gpui::px(8.0))
                                                .rounded(gpui::px(999.0))
                                                .bg(t.gold)
                                                .text_color(t.clay_ink)
                                                .text_size(gpui::px(10.0))
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .on_click(cx.listener(move |this, _event, _window, cx| {
                                                    this.play_tracks(songs_all.clone(), 0, cx);
                                                }))
                                                .child("Play all"),
                                        ),
                                );
                                let songs = detail.songs.clone();
                                let current_id = self.player_state.read(cx).current.as_ref().map(|c| c.id.clone());
                                for (i, t_) in detail.songs.iter().enumerate() {
                                    let songs2 = songs.clone();
                                    let is_current = current_id.as_deref() == Some(t_.id.as_str());
                                    col = col.child(
                                        track_row(
                                            &format!("{:02}", i + 1),
                                            &t_.title,
                                            &t_.artist,
                                            &format_time(t_.duration.as_secs_f64()),
                                            is_current,
                                            t,
                                        )
                                        .id(format!("track-{}", i))
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _event, _window, cx| {
                                            this.play_tracks(songs2.clone(), i, cx);
                                        })),
                                    );
                                }
                                col
                            } else {
                                div()
                            }),
                    ),
                ),
            )
            .child(player)
            .with_animation(
                format!("route-{:?}", self.route),
                gpui::Animation::new(std::time::Duration::from_millis(180))
                    .with_easing(gpui::ease_out_quint()),
                |el, delta| el.opacity(0.35 + 0.65 * delta),
            )
            .into_any_element()
    }
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
            .text_size(gpui::px(12.0))
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
                    color: gpui::rgba(0x8C6D3F30).into(),
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
            .text_size(gpui::px(12.0))
            .hover(|style| style.bg(gpui::rgba(0xFFFFFF08)))
            .child(label.to_string())
    }
}


fn album_card(title: &str, artist: &str, cover: Option<std::sync::Arc<gpui::Image>>, t: &crate::theme::Colors) -> gpui::Div {
    let initial = title.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".to_string());
    gpui::div()
        .flex()
        .flex_col()
        .gap(gpui::px(7.0))
        .w_full()
        .hover(|style| style.bg(gpui::rgba(0xFFFFFF08)).rounded(gpui::px(16.0)))
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
                .text_size(gpui::px(11.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(t.text_primary)
                .child(title.to_string()),
        )
        .child(
            gpui::div()
                .w_full()
                .text_size(gpui::px(9.0))
                .text_color(t.text_secondary)
                .child(artist.to_string()),
        )
}

fn track_row(idx: &str, title: &str, artist: &str, dur: &str, active: bool, t: &crate::theme::Colors) -> gpui::Div {
    let bg = if active { gpui::rgba(0xC9A86A12) } else { t.glass_tint };
    let text_color = t.text_primary;
    gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(10.0))
        .px(gpui::px(10.0))
        .py(gpui::px(7.0))
        .rounded(gpui::px(12.0))
        .bg(bg)
        .border_1()
        .border_color(if active { gpui::rgba(0xC9A86A55) } else { t.glass_border })
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
                        .text_size(gpui::px(11.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(text_color)
                        .child(title.to_string()),
                )
                .child(
                    gpui::div()
                        .text_size(gpui::px(8.0))
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

#[derive(Clone, Default)]
pub struct HomeState {
    pub albums: Vec<crate::library::Album>,
    pub loading: bool,
    pub error: Option<String>,
    pub covers: std::collections::HashMap<String, std::sync::Arc<gpui::Image>>,
    pub artists: Vec<crate::library::Artist>,
    pub playlists: Vec<crate::library::Playlist>,
}


fn fxhash_id(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
