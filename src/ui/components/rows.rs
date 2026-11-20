//! Row components: track rows.

use gpui::prelude::*;

pub fn track_row(
    idx: &str,
    title: &str,
    artist: &str,
    dur: &str,
    active: bool,
    cover: Option<std::sync::Arc<gpui::Image>>,
    t: &crate::theme::Colors,
) -> gpui::Div {
    let bg = if active { t.gold_tint } else { t.glass_tint };
    let text_color = t.text_primary;
    gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(10.0))
        .px(gpui::px(10.0))
        .py(gpui::px(7.0))
        .rounded(gpui::px(12.0))
        .bg(bg)
        .hover(|style| style.bg(t.glass_highlight_soft))
        .border_1()
        .border_color(if active {
            t.gold_glow_border
        } else {
            t.glass_border
        })
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
                .flex_shrink_0()
                .rounded(gpui::px(9.0))
                .overflow_hidden()
                .bg(t.glass_tint)
                .border_1()
                .border_color(t.glass_border)
                .child(match cover {
                    Some(image) => gpui::img(gpui::ImageSource::Image(image))
                        .w_full()
                        .h_full()
                        .object_fit(gpui::ObjectFit::Cover)
                        .into_any_element(),
                    None => gpui::div().into_any_element(),
                }),
        )
        .child(
            gpui::div()
                .flex_1()
                .flex()
                .flex_col()
                .child(
                    gpui::div()
                        .text_size(gpui::px(12.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(text_color)
                        .line_clamp(1)
                        .text_ellipsis()
                        .child(title.to_string()),
                )
                .child(
                    gpui::div()
                        .text_size(gpui::px(10.0))
                        .text_color(t.text_secondary)
                        .line_clamp(1)
                        .text_ellipsis()
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

pub fn format_time(seconds: f64) -> String {
    let mins = (seconds / 60.0) as u64;
    let secs = (seconds % 60.0) as u64;
    format!("{}:{:02}", mins, secs)
}
