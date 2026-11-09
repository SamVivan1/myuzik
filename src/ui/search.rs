use crate::theme::midnight_brass;
use gpui::{Context, Render, Window, div, prelude::*, px};

pub struct SearchView {
    pub query: String,
    pub results: Vec<String>,
}

impl SearchView {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
        }
    }
}

impl Render for SearchView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = midnight_brass();
        let t = &theme.colors;

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(gpui::px(16.0))
            .px(gpui::px(26.0))
            .py(gpui::px(20.0))
            .child(
                div()
                    .font_family(theme.display_font)
                    .text_size(gpui::px(24.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Search"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(gpui::px(10.0))
                    .px(gpui::px(16.0))
                    .py(gpui::px(12.0))
                    .rounded(gpui::px(999.0))
                    .bg(t.clay_surface)
                    .border_2()
                    .border_color(t.os_accent)
                    .child(
                        div()
                            .text_size(gpui::px(12.0))
                            .text_color(t.clay_ink)
                            .child("Ctrl K"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(gpui::px(12.0))
                            .text_color(t.clay_ink)
                            .child("Search your library…"),
                    ),
            )
            .child(
                div()
                    .text_size(gpui::px(11.0))
                    .text_color(t.text_secondary)
                    .child("Type to search artists, albums, songs, and playlists."),
            )
    }
}
