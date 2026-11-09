use crate::config::AppConfig;
use crate::theme::midnight_brass;
use gpui::{Context, Render, Window, div, prelude::*};

pub struct SettingsView {
    pub config: AppConfig,
}

impl SettingsView {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = midnight_brass();
        let t = &theme.colors;

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(gpui::px(20.0))
            .px(gpui::px(26.0))
            .py(gpui::px(20.0))
            .pb(gpui::px(40.0))
            .child(
                div()
                    .font_family(theme.display_font)
                    .text_size(gpui::px(24.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Settings"),
            )
            .child(section("Account", t, theme.ui_font, vec![
                ("Server URL", "https://navidrome.example.com"),
                ("Username", "samvivan"),
            ]))
            .child(section("Playback", t, theme.ui_font, vec![
                ("Stream quality", "Original"),
                ("Volume", "80%"),
                ("Scrobble", "On"),
            ]))
            .child(section("Lyrics", t, theme.ui_font, vec![
                ("Offset", "0 ms"),
                ("Source", "Navidrome first"),
            ]))
            .child(section("Overlay", t, theme.ui_font, vec![
                ("Status", "Installed"),
                ("Opacity", "74%"),
            ]))
            .child(section("Appearance", t, theme.ui_font, vec![
                ("Theme", "Midnight Brass"),
                ("Reduce motion", "Off"),
                ("OS accent", "Follow system"),
            ]))
            .child(section("About", t, theme.ui_font, vec![
                ("Version", "0.1.0"),
                ("License", "MIT"),
            ]))
    }
}

fn section(title: &str, t: &crate::theme::Colors, _font: &'static str, rows: Vec<(&str, &str)>) -> gpui::Div {
    let mut s = div()
        .flex()
        .flex_col()
        .gap(gpui::px(8.0))
        .child(
            div()
                .text_size(gpui::px(9.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(t.gold)
                .child(title.to_uppercase()),
        );

    for (label, value) in rows {
        s = s.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px(gpui::px(14.0))
                .py(gpui::px(10.0))
                .rounded(gpui::px(12.0))
                .bg(t.glass_tint)
                .border_1()
                .border_color(t.glass_border)
                .child(
                    div()
                        .text_size(gpui::px(11.0))
                        .text_color(t.text_secondary)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(gpui::px(11.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(value.to_string()),
                ),
        );
    }

    s
}
