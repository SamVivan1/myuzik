use crate::theme::midnight_brass;
use gpui::{Context, Render, Window, div, prelude::*};

pub struct HomeView {
    pub greeting: String,
}

impl HomeView {
    pub fn new() -> Self {
        Self {
            greeting: "Good evening".to_string(),
        }
    }
}

impl Render for HomeView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = midnight_brass();
        let t = &theme.colors;

        div()
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
                            .child(self.greeting.clone()),
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
                // Album grid placeholder (4 columns)
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
                // Track list
                div()
                    .flex()
                    .flex_col()
                    .gap(gpui::px(6.0))
                    .child(track_row(
                        "01",
                        "Midnight Static",
                        "Lumen Fields",
                        "3:42",
                        true,
                        t,
                    ))
                    .child(track_row(
                        "02",
                        "Paper Satellites",
                        "Kite Garden",
                        "4:07",
                        false,
                        t,
                    ))
                    .child(track_row("03", "Golden Hour", "Vela", "3:18", false, t)),
            )
    }
}

fn album_card(title: &str, artist: &str, t: &crate::theme::Colors) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(gpui::px(6.0))
        .child(
            div()
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
                    div()
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
            div()
                .text_size(gpui::px(10.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title.to_string()),
        )
        .child(
            div()
                .text_size(gpui::px(8.0))
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
    let bg = if active { t.gold } else { t.glass_tint };
    let text_color = if active { t.clay_ink } else { t.text_primary };
    div()
        .flex()
        .items_center()
        .gap(gpui::px(10.0))
        .px(gpui::px(10.0))
        .py(gpui::px(7.0))
        .rounded(gpui::px(12.0))
        .bg(bg)
        .child(
            div()
                .w(gpui::px(15.0))
                .text_size(gpui::px(9.0))
                .text_color(if active { t.clay_ink } else { t.text_tertiary })
                .child(idx.to_string()),
        )
        .child(
            div()
                .w(gpui::px(34.0))
                .h(gpui::px(34.0))
                .rounded(gpui::px(9.0))
                .bg(t.glass_tint)
                .border_1()
                .border_color(t.glass_border),
        )
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(gpui::px(11.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(text_color)
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .text_size(gpui::px(8.0))
                        .text_color(if active { t.clay_ink } else { t.text_secondary })
                        .child(artist.to_string()),
                ),
        )
        .child(
            div()
                .text_size(gpui::px(9.0))
                .text_color(if active { t.clay_ink } else { t.text_tertiary })
                .child(dur.to_string()),
        )
}

impl Default for HomeView {
    fn default() -> Self {
        Self::new()
    }
}
