use crate::config::AppConfig;
use crate::library::HomeData;
use crate::theme::midnight_brass;
use gpui::{Context, Render, Window, div, prelude::*};

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
    pub home_data: Option<HomeData>,
    pub loading: bool,
    pub error: Option<String>,
}

impl MyuzikApp {
    pub fn new(config: AppConfig) -> Self {
        Self {
            route: if config.server_url.is_empty() {
                Route::Setup
            } else {
                Route::Home
            },
            config,
            home_data: None,
            loading: false,
            error: None,
        }
    }

    pub fn go(&mut self, route: Route) {
        self.route = route;
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
                    .child(
                        div()
                            .font_family(theme.display_font)
                            .text_size(gpui::px(24.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Myuzik"),
                    )
                    .child(
                        div()
                            .text_size(gpui::px(12.0))
                            .text_color(t.text_secondary)
                            .child(format!("Route: {:?}", self.route)),
                    ),
            )
    }
}
