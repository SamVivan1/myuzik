use crate::theme::{midnight_brass, Theme};
use gpui::{Context, Render, Window, div, prelude::*, px};

pub struct Shell;

impl Shell {
    pub fn new() -> Self {
        Self
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = midnight_brass();
        let t = &theme.colors;
        let r = &theme.radii;

        // Sidebar
        let sidebar = div()
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
                // brand
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
                // nav
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
                    .mt_auto()
                    .flex()
                    .flex_col()
                    .gap(gpui::px(8.0))
                    .child(
                        div()
                            .px(gpui::px(14.0))
                            .py(gpui::px(10.0))
                            .rounded(gpui::px(r.button))
                            .bg(gpui::rgb(0x2A241C))
                            .text_color(t.gold_light)
                            .font_family(theme.ui_font)
                            .text_size(gpui::px(9.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("Pin lyrics"),
                    ),
            );

        // Main content
        let main = div()
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
                            .text_size(gpui::px(38.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Good evening"),
                    ),
            )
            .child(
                div()
                    .font_family(theme.display_font)
                    .text_size(gpui::px(18.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Recently added"),
            )
            .child(
                div()
                    .text_size(gpui::px(11.0))
                    .text_color(t.text_secondary)
                    .child("Albums will appear here once you connect to Navidrome."),
            );

        // Player bar (clay)
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
                color: gpui::rgba(0x00000070).into(),
                offset: gpui::point(gpui::px(0.0), gpui::px(24.0)),
                blur_radius: gpui::px(52.0),
                spread_radius: gpui::px(0.0),
                inset: false,
            }])
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_color(t.clay_ink)
                    .font_family(theme.ui_font)
                    .text_size(gpui::px(12.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Player bar (clay) — Midnight Brass"),
            );

        // Glass shell
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
                        color: gpui::rgba(0x000000A0).into(),
                        offset: gpui::point(gpui::px(0.0), gpui::px(40.0)),
                        blur_radius: gpui::px(100.0),
                        spread_radius: gpui::px(0.0),
                        inset: false,
                    }])
                    .flex()
                    .overflow_hidden()
                    .child(sidebar)
                    .child(main),
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
            .font_weight(gpui::FontWeight::BOLD)
            .text_size(gpui::px(12.0))
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
