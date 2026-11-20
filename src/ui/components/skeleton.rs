//! Skeleton loading placeholders.

use crate::theme::Colors;
use gpui::prelude::*;
use gpui::{AnimationExt, Div, div};

/// A pulsing skeleton card matching `album_card` dimensions.
pub fn skeleton_card(t: &Colors) -> Div {
    let pulse = div()
        .w_full()
        .aspect_ratio(1.0)
        .rounded(gpui::px(20.0))
        .bg(t.glass_tint)
        .border_1()
        .border_color(t.glass_border)
        .with_animation(
            "skeleton-pulse",
            gpui::Animation::new(std::time::Duration::from_millis(900))
                .repeat()
                .with_easing(gpui::ease_in_out),
            |el, delta| el.opacity(0.35 + 0.45 * delta),
        );
    div()
        .flex()
        .flex_col()
        .gap(gpui::px(7.0))
        .w_full()
        .child(pulse)
        .child(
            div()
                .h(gpui::px(10.0))
                .w_full()
                .rounded(gpui::px(4.0))
                .bg(t.glass_tint),
        )
        .child(
            div()
                .h(gpui::px(8.0))
                .w(gpui::px(90.0))
                .rounded(gpui::px(4.0))
                .bg(t.glass_tint),
        )
}

/// A pulsing skeleton row matching `track_row` dimensions.
pub fn skeleton_row(t: &Colors) -> Div {
    div()
        .flex()
        .items_center()
        .gap(gpui::px(10.0))
        .px(gpui::px(10.0))
        .py(gpui::px(7.0))
        .rounded(gpui::px(12.0))
        .bg(t.glass_tint)
        .child(
            div()
                .w(gpui::px(15.0))
                .h(gpui::px(8.0))
                .rounded(gpui::px(4.0))
                .bg(t.glass_highlight),
        )
        .child(
            div()
                .w(gpui::px(34.0))
                .h(gpui::px(34.0))
                .rounded(gpui::px(9.0))
                .bg(t.glass_highlight),
        )
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(gpui::px(4.0))
                .child(
                    div()
                        .h(gpui::px(10.0))
                        .w(gpui::px(120.0))
                        .rounded(gpui::px(4.0))
                        .bg(t.glass_highlight),
                )
                .child(
                    div()
                        .h(gpui::px(8.0))
                        .w(gpui::px(80.0))
                        .rounded(gpui::px(4.0))
                        .bg(t.glass_highlight),
                ),
        )
        .child(
            div()
                .w(gpui::px(30.0))
                .h(gpui::px(8.0))
                .rounded(gpui::px(4.0))
                .bg(t.glass_highlight),
        )
}
