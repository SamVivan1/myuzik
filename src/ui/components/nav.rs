//! Navigation components.

use crate::ui::components::icons::nav_icon;
use gpui::prelude::*;

pub fn nav_item(icon: &str, label: &str, active: bool, t: &crate::theme::Colors) -> gpui::Div {
    if active {
        gpui::div()
            .px(gpui::px(10.0))
            .py(gpui::px(7.0))
            .rounded(gpui::px(12.0))
            .bg(t.gold_tint)
            .text_color(t.gold_light)
            .text_size(gpui::px(13.0))
            .font_weight(gpui::FontWeight::BOLD)
            .shadow(vec![
                gpui::BoxShadow {
                    color: t.glass_border_on_clay.into(),
                    offset: gpui::point(gpui::px(1.5), gpui::px(1.5)),
                    blur_radius: gpui::px(3.0),
                    spread_radius: gpui::px(0.0),
                    inset: true,
                },
                gpui::BoxShadow {
                    color: t.shadow_deep.into(),
                    offset: gpui::point(gpui::px(-2.0), gpui::px(-2.0)),
                    blur_radius: gpui::px(5.0),
                    spread_radius: gpui::px(0.0),
                    inset: true,
                },
            ])
            .flex()
            .items_center()
            .gap(gpui::px(8.0))
            .child(nav_icon(icon, t.gold_light))
            .child(label.to_string())
    } else {
        gpui::div()
            .px(gpui::px(10.0))
            .py(gpui::px(7.0))
            .rounded(gpui::px(12.0))
            .text_color(t.text_secondary)
            .text_size(gpui::px(13.0))
            .hover(|style| style.bg(t.glass_hover))
            .flex()
            .items_center()
            .gap(gpui::px(8.0))
            .child(nav_icon(icon, t.text_secondary))
            .child(label.to_string())
    }
}
