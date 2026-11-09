use crate::app::Route;
use crate::config::{AppConfig, Paths};
use crate::navidrome::client::NavidromeClient;
use crate::platform::mpris::MprisHandle;
use crate::player::engine::{PlayerCommand, PlayerEvent, PlayerHandle};
use crate::player::state::{PlayerState, PlaybackStatus};
use crate::theme::midnight_brass;
use gpui::{Context, Render, Window, div, prelude::*};

#[allow(dead_code)]
pub struct Shell {
    player_state: gpui::Entity<PlayerState>,
    player_handle: PlayerHandle,
    config: AppConfig,
    paths: Paths,
    route: Route,
    #[allow(dead_code)]
    client: Option<NavidromeClient>,
    mpris_handle: Option<MprisHandle>,
}

impl Shell {
    pub fn new(cx: &mut gpui::App, player_handle: PlayerHandle, config: AppConfig, paths: Paths, mpris_handle: Option<MprisHandle>) -> Self {
        let player_state = cx.new(|_cx| PlayerState::new());
        let route = if config.server_url.is_empty() { Route::Setup } else { Route::Home };
        let password = crate::config::load_password(&paths);
        let client = if !config.server_url.is_empty() {
            password.and_then(|pw| NavidromeClient::new(&config.server_url, &config.username, &pw).ok())
        } else { None };
        // Poll player events in background
        let state = player_state.clone();
        let rx = player_handle.rx.clone();
        let mpris_for_loop = mpris_handle.clone();
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
        let handle = cx.open_window(opts, |_window, cx| {
            cx.new(|_cx| crate::overlay::PinnedLyricsView::new())
        });
        if let Ok(h) = handle {
            h.update(cx, |_view, window, _cx| {
                window.set_window_title(crate::overlay::OVERLAY_TITLE);
            })
            .ok();
        }
    }

    pub fn close_overlay(&mut self, _cx: &mut gpui::App) {}
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = midnight_brass();
        let t = &theme.colors;
        let status = self.player_state.read(cx).status;
        let is_playing = status == PlaybackStatus::Playing;
        let current_title = self.player_state.read(cx).current.as_ref().map(|t| t.title.clone()).unwrap_or_default();
        let current_artist = self.player_state.read(cx).current.as_ref().map(|t| t.artist.clone()).unwrap_or_default();
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
            .bg(t.clay_surface)
            .border_1()
            .border_color(gpui::rgba(0xFFFFFFFF))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0.0, 0.0, 0.0, 0.45),
                offset: gpui::point(gpui::px(0.0), gpui::px(24.0)),
                blur_radius: gpui::px(52.0),
                spread_radius: gpui::px(0.0),
                inset: false,
            }])
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
                    .child(
                        div()
                            .w(gpui::px(52.0))
                            .h(gpui::px(52.0))
                            .rounded(gpui::px(14.0))
                            .bg(t.glass_tint)
                            .border_1()
                            .border_color(t.glass_border),
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
                                    .child("⏮"),
                            )
                            .child(
                                div()
                                    .w(gpui::px(44.0))
                                    .h(gpui::px(44.0))
                                    .rounded(gpui::px(999.0))
                                    .bg(t.gold)
                                    .text_color(t.clay_ink)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(gpui::px(16.0))
                                    .cursor_pointer()
                                    .id("play-btn")
                                    .on_click(cx.listener(|this, _event, _window, _cx| {
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
                                this.open_overlay(_cx);
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
                    .inset(gpui::px(24.0))
                    .rounded(gpui::px(theme.radii.glass))
                    .bg(t.glass_tint)
                    .border_1()
                    .border_color(t.glass_border)
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0.0, 0.0, 0.0, 0.6),
                        offset: gpui::point(gpui::px(0.0), gpui::px(40.0)),
                        blur_radius: gpui::px(100.0),
                        spread_radius: gpui::px(0.0),
                        inset: false,
                    }])
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
                            .bg(gpui::rgba(0x0C0C1014))
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
                                            .bg(t.gold)
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
                                    .child(nav_item("Home", true, t))
                                    .child(nav_item("Search", false, t))
                                    .child(nav_item("Albums", false, t))
                                    .child(nav_item("Artists", false, t))
                                    .child(nav_item("Playlists", false, t))
                                    .child(nav_item("Favorites", false, t)),
                            )
                            .child(
                                div()
                                    .mt(gpui::px(8.0))
                                    .text_size(gpui::px(9.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(t.text_tertiary)
                                    .child("YOUR PLAYLISTS"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(gpui::px(2.0))
                                    .child(playlist_item("Late night drive", t))
                                    .child(playlist_item("Focus / no vocals", t))
                                    .child(playlist_item("Sunday morning", t)),
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
                                                this.open_overlay(_cx);
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
                            .gap(gpui::px(18.0))
                            .px(gpui::px(26.0))
                            .py(gpui::px(20.0))
                            .pb(gpui::px(118.0))
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
                                            .child("YOUR LIBRARY"),
                                    )
                                    .child(
                                        div()
                                            .font_family(theme.display_font)
                                            .text_size(gpui::px(30.0))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child("Good evening"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .font_family(theme.display_font)
                                            .text_size(gpui::px(16.0))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child("Recently added"),
                                    )
                                    .child(
                                        div()
                                            .text_size(gpui::px(10.0))
                                            .text_color(t.text_secondary)
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child("See all"),
                                    ),
                            )
                            .child(
                                div()
                                    .grid()
                                    .grid_cols(4)
                                    .gap(gpui::px(14.0))
                                    .child(album_card("Neon Harbor", "Lumen Fields", t))
                                    .child(album_card("Moss & Static", "Kite Garden", t))
                                    .child(album_card("Golden Hour", "Vela", t))
                                    .child(album_card("Blue Terminal", "Nightshift", t)),
                            )
                            .child(
                                div()
                                    .font_family(theme.display_font)
                                    .text_size(gpui::px(16.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("Made for you"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(gpui::px(6.0))
                                    .child(track_row("01", "Midnight Static", "Lumen Fields", "3:42", true, t))
                                    .child(track_row("02", "Paper Satellites", "Kite Garden", "4:07", false, t))
                                    .child(track_row("03", "Golden Hour", "Vela", "3:18", false, t)),
                            ),
                    ),
            )
            .child(player)
    }
}

fn nav_item(label: &str, active: bool, t: &crate::theme::Colors) -> gpui::Div {
    if active {
        gpui::div()
            .px(gpui::px(10.0))
            .py(gpui::px(7.0))
            .rounded(gpui::px(12.0))
            .bg(t.clay_surface)
            .text_color(t.clay_ink)
            .text_size(gpui::px(12.0))
            .font_weight(gpui::FontWeight::BOLD)
            .child(label.to_string())
    } else {
        gpui::div()
            .px(gpui::px(10.0))
            .py(gpui::px(7.0))
            .rounded(gpui::px(12.0))
            .text_color(t.text_secondary)
            .text_size(gpui::px(12.0))
            .child(label.to_string())
    }
}

fn playlist_item(label: &str, t: &crate::theme::Colors) -> gpui::Div {
    gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(8.0))
        .text_size(gpui::px(11.0))
        .text_color(t.text_secondary)
        .child(
            gpui::div()
                .w(gpui::px(4.0))
                .h(gpui::px(4.0))
                .bg(t.gold_deep),
        )
        .child(label.to_string())
}

fn album_card(title: &str, artist: &str, t: &crate::theme::Colors) -> gpui::Div {
    gpui::div()
        .flex()
        .flex_col()
        .gap(gpui::px(6.0))
        .child(
            gpui::div()
                .aspect_ratio(1.0)
                .rounded(gpui::px(16.0))
                .bg(t.glass_tint)
                .border_1()
                .border_color(t.glass_border)
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0.0, 0.0, 0.0, 0.28),
                    offset: gpui::point(gpui::px(0.0), gpui::px(14.0)),
                    blur_radius: gpui::px(32.0),
                    spread_radius: gpui::px(0.0),
                    inset: false,
                }])
                .flex()
                .items_center()
                .justify_center()
                .child(
                    gpui::div()
                        .w(gpui::px(33.0))
                        .h(gpui::px(33.0))
                        .rounded(gpui::px(999.0))
                        .bg(t.gold)
                        .text_color(t.clay_ink)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child("▶"),
                ),
        )
        .child(
            gpui::div()
                .text_size(gpui::px(10.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title.to_string()),
        )
        .child(
            gpui::div()
                .text_size(gpui::px(8.0))
                .text_color(t.text_secondary)
                .child(artist.to_string()),
        )
}

fn track_row(idx: &str, title: &str, artist: &str, dur: &str, active: bool, t: &crate::theme::Colors) -> gpui::Div {
    let bg = if active { t.gold } else { t.glass_tint };
    let text_color = if active { t.clay_ink } else { t.text_primary };
    gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(10.0))
        .px(gpui::px(10.0))
        .py(gpui::px(7.0))
        .rounded(gpui::px(12.0))
        .bg(bg)
        .child(
            gpui::div()
                .w(gpui::px(15.0))
                .text_size(gpui::px(9.0))
                .text_color(if active { t.clay_ink } else { t.text_tertiary })
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
                        .text_color(if active { t.clay_ink } else { t.text_secondary })
                        .child(artist.to_string()),
                ),
        )
        .child(
            gpui::div()
                .text_size(gpui::px(9.0))
                .text_color(if active { t.clay_ink } else { t.text_tertiary })
                .child(dur.to_string()),
        )
}

fn format_time(seconds: f64) -> String {
    let mins = (seconds / 60.0) as u64;
    let secs = (seconds % 60.0) as u64;
    format!("{}:{:02}", mins, secs)
}
