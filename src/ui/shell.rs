use crate::player::engine::{PlayerCommand, PlayerHandle};
use crate::platform::mpris::MprisHandle;
use crate::theme::midnight_brass;
use gpui::{Context, Render, Window, div, prelude::*, px};

pub struct Shell {
    player_handle: PlayerHandle,
    mpris_handle: Option<MprisHandle>,
}

impl Shell {
    pub fn new(cx: &mut gpui::App, player_handle: PlayerHandle, mpris_handle: Option<MprisHandle>) -> Self {
        Self {
            player_handle,
            mpris_handle,
        }
    }

    pub fn open_overlay(&mut self, cx: &mut gpui::App) {
        let mut opts = gpui::WindowOptions::default();
        opts.window_background = gpui::WindowBackgroundAppearance::Blurred;
        opts.window_decorations = None;
        opts.focus = false;
        opts.show = true;
        opts.app_id = Some(crate::overlay::APP_ID.to_string());
        opts.titlebar = None;
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
            .justify_center()
            .gap(gpui::px(12.0))
            .child(
                div()
                    .text_color(t.clay_ink)
                    .font_family(theme.ui_font)
                    .text_size(gpui::px(12.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Player bar — Midnight Brass"),
            )
            .child(
                div()
                    .px(gpui::px(10.0))
                    .py(gpui::px(6.0))
                    .rounded(gpui::px(999.0))
                    .bg(t.gold)
                    .text_color(t.clay_ink)
                    .font_family(theme.ui_font)
                    .text_size(gpui::px(10.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .cursor_pointer()
                    .id("play-btn")
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.player_handle.tx.send(PlayerCommand::Toggle).ok();
                    }))
                    .child("▶"),
            );

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
                                            .on_click(cx.listener(|this, _event, _window, cx| {
                                                this.open_overlay(cx);
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
