//! Card components: album cards and skeletons.

use gpui::AnimationExt;
use gpui::prelude::*;

pub fn album_skeleton(index: usize, t: &crate::theme::Colors) -> gpui::Div {
    let pulse = gpui::div()
        .w_full()
        .aspect_ratio(1.0)
        .rounded(gpui::px(16.0))
        .bg(t.glass_tint)
        .border_1()
        .border_color(t.glass_border)
        .with_animation(
            format!("skeleton-{index}"),
            gpui::Animation::new(std::time::Duration::from_millis(900))
                .repeat()
                .with_easing(gpui::ease_in_out),
            |el, delta| el.opacity(0.35 + 0.45 * delta),
        );
    gpui::div()
        .flex()
        .flex_col()
        .gap(gpui::px(7.0))
        .w_full()
        .child(pulse)
        .child(
            gpui::div()
                .h(gpui::px(10.0))
                .w_full()
                .rounded(gpui::px(4.0))
                .bg(t.glass_tint),
        )
        .child(
            gpui::div()
                .h(gpui::px(8.0))
                .w(gpui::px(90.0))
                .rounded(gpui::px(4.0))
                .bg(t.glass_tint),
        )
}

#[allow(clippy::too_many_arguments)]
pub fn album_card(
    title: &str,
    artist: &str,
    cover: Option<std::sync::Arc<gpui::Image>>,
    t: &crate::theme::Colors,
) -> gpui::Div {
    let initial = title
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());
    gpui::div()
        .flex()
        .flex_col()
        .gap(gpui::px(7.0))
        .w_full()
        .hover(|style| style.bg(t.glass_hover).rounded(gpui::px(20.0)))
        .child(
            gpui::div()
                .w_full()
                .aspect_ratio(1.0)
                .rounded(gpui::px(20.0))
                .bg(gpui::linear_gradient(
                    135.0,
                    gpui::linear_color_stop(t.gold_deep, 0.0),
                    gpui::linear_color_stop(t.clay_surface, 1.0),
                ))
                .border_1()
                .border_color(t.glass_border)
                .shadow(vec![gpui::BoxShadow {
                    color: t.shadow_deep.into(),
                    offset: gpui::point(gpui::px(0.0), gpui::px(14.0)),
                    blur_radius: gpui::px(32.0),
                    spread_radius: gpui::px(0.0),
                    inset: false,
                }])
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .child(if let Some(image) = cover {
                    gpui::img(gpui::ImageSource::Image(image))
                        .w_full()
                        .h_full()
                        .rounded(gpui::px(20.0))
                        .object_fit(gpui::ObjectFit::Cover)
                        .into_any_element()
                } else {
                    gpui::div()
                        .font_family("Fraunces")
                        .text_size(gpui::px(54.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(t.clay_ink)
                        .opacity(0.85)
                        .child(initial)
                        .into_any_element()
                })
                .child(
                    gpui::div()
                        .absolute()
                        .right(gpui::px(8.0))
                        .bottom(gpui::px(8.0))
                        .w(gpui::px(33.0))
                        .h(gpui::px(33.0))
                        .rounded(gpui::px(999.0))
                        .bg(gpui::linear_gradient(
                            145.0,
                            gpui::linear_color_stop(t.gold_light, 0.0),
                            gpui::linear_color_stop(t.gold, 1.0),
                        ))
                        .text_color(t.clay_ink)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(gpui::px(11.0))
                        .child("▶"),
                ),
        )
        // Long album titles wrap by default, which made the artist baseline
        // differ per card and the grid rows ragged. One clamped line instead.
        .child(
            gpui::div()
                .w_full()
                .text_size(gpui::px(12.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(t.text_primary)
                .line_clamp(1)
                .text_ellipsis()
                .child(title.to_string()),
        )
        .child(
            gpui::div()
                .w_full()
                .text_size(gpui::px(10.0))
                .text_color(t.text_secondary)
                .line_clamp(1)
                .text_ellipsis()
                .child(artist.to_string()),
        )
}
