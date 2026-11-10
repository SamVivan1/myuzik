use crate::config::{AppConfig, Paths};
use crate::library::{HomeData, Track};
use crate::lyrics::model::LyricsStatus;
use crate::player::engine::{PlayerCommand, PlayerHandle, spawn_player};
use crate::player::scrobble::ScrobbleTracker;
use crate::player::state::PlayerState;
use crate::theme::midnight_brass;
use gpui::{Context, Entity, Render, Window, div, prelude::*};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Setup,
    Home,
    Search,
    Albums,
    Artists,
    Playlists,
    Favorites,
    Album(String),
    Artist(String),
    Playlist(String),
    NowPlaying,
    Settings,
}

pub struct MyuzikApp {
    pub route: Route,
    pub config: AppConfig,
    pub paths: Paths,
    pub player_state: Entity<PlayerState>,
    pub player_handle: PlayerHandle,
    pub lyrics_status: LyricsStatus,
    pub scrobble: ScrobbleTracker,
    pub home_data: Option<HomeData>,
    pub loading: bool,
    pub error: Option<String>,
    pub search_query: String,
    pub search_results: Option<crate::library::SearchResults>,
}

impl MyuzikApp {
    pub fn new(cx: &mut gpui::App, config: AppConfig, paths: Paths) -> Self {
        let player_state = cx.new(|_cx| PlayerState::new());
        let player_handle = spawn_player();
        Self {
            route: if config.server_url.is_empty() {
                Route::Setup
            } else {
                Route::Home
            },
            config,
            paths,
            player_state,
            player_handle,
            lyrics_status: LyricsStatus::NotFound,
            scrobble: ScrobbleTracker::new(),
            home_data: None,
            loading: false,
            error: None,
            search_query: String::new(),
            search_results: None,
        }
    }

    pub fn go(&mut self, route: Route) {
        self.route = route;
    }

    pub fn play_track(&self, track: Track) {
        let uri = format!("track://{}", track.id); // placeholder; real URI from Navidrome
        self.player_handle
            .tx
            .send(PlayerCommand::Load {
                uri,
                track_id: track.id.clone(),
            })
            .ok();
        self.player_handle.tx.send(PlayerCommand::Play).ok();
    }

    pub fn toggle_playback(&self) {
        self.player_handle.tx.send(PlayerCommand::Toggle).ok();
    }

    pub fn next_track(&self) {
        self.player_handle.tx.send(PlayerCommand::Stop).ok();
        // In real impl, queue.next() then load
    }

    pub fn previous_track(&self) {
        self.player_handle.tx.send(PlayerCommand::Stop).ok();
    }

    pub fn seek(&self, pos: Duration) {
        self.player_handle.tx.send(PlayerCommand::Seek(pos)).ok();
    }

    pub fn set_volume(&self, v: f32) {
        self.player_handle.tx.send(PlayerCommand::SetVolume(v)).ok();
    }
}

impl Render for MyuzikApp {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = midnight_brass();
        let t = &theme.colors;

        div()
            .size_full()
            .bg(t.bg_base)
            .text_color(t.text_primary)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .size_full()
                    .gap_2()
                    .child(
                        div()
                            .font_family(theme.display_font)
                            .text_size(gpui::px(24.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Myuzik"),
                    )
                    .child(
                        div()
                            .text_size(gpui::px(11.0))
                            .text_color(t.text_secondary)
                            .child(format!("Route: {:?}", self.route)),
                    ),
            )
    }
}
