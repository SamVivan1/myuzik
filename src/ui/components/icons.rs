//! Inline SVG icons for navigation (and other small UI affordances).
//!
//! GPUI renders an SVG as a monochrome alpha mask tinted by `text_color`, so the
//! actual stroke/fill colour in the markup does not matter — only the alpha.

use gpui::{IntoElement, Styled, px};

macro_rules! svg {
    ($body:expr) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox=""#,
            "0 0 16 16",
            r#"" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">"#,
            $body,
            "</svg>"
        )
    };
}

pub const ICON_HOME: &str =
    svg!("<path d=\"M2.5 7.5 L8 3 L13.5 7.5 V14 H9.5 V10.5 H6.5 V14 H2.5 Z\"/>");
pub const ICON_MUSIC: &str = svg!(
    "<path d=\"M12 2.5 V11\"/><path d=\"M12 2.5 L6.5 4.5 V13\"/><circle cx=\"4.5\" cy=\"13\" r=\"1.6\" fill=\"currentColor\" stroke=\"none\"/><circle cx=\"10\" cy=\"11\" r=\"1.6\" fill=\"currentColor\" stroke=\"none\"/>"
);
pub const ICON_SEARCH: &str =
    svg!("<circle cx=\"7\" cy=\"7\" r=\"4\"/><line x1=\"10\" y1=\"10\" x2=\"13.5\" y2=\"13.5\"/>");
pub const ICON_ALBUM: &str =
    svg!("<circle cx=\"8\" cy=\"8\" r=\"6\"/><circle cx=\"8\" cy=\"8\" r=\"2\"/>");
pub const ICON_PERSON: &str =
    svg!("<circle cx=\"8\" cy=\"5\" r=\"2.5\"/><path d=\"M3 14 C3 10.5 13 10.5 13 14\"/>");
pub const ICON_LIST: &str = svg!(
    "<line x1=\"5.5\" y1=\"4\" x2=\"14\" y2=\"4\"/><line x1=\"5.5\" y1=\"8\" x2=\"14\" y2=\"8\"/><line x1=\"5.5\" y1=\"12\" x2=\"14\" y2=\"12\"/><circle cx=\"2.5\" cy=\"4\" r=\"1\" fill=\"currentColor\" stroke=\"none\"/><circle cx=\"2.5\" cy=\"8\" r=\"1\" fill=\"currentColor\" stroke=\"none\"/><circle cx=\"2.5\" cy=\"12\" r=\"1\" fill=\"currentColor\" stroke=\"none\"/>"
);
pub const ICON_STAR: &str = svg!(
    "<path d=\"M8 2 L9.6 6 L14 6.4 L10.8 9.2 L11.6 13.6 L8 11.6 L4.4 13.6 L5.2 9.2 L2 6.4 L6.4 6 Z\"/>"
);
pub const ICON_HALF: &str = svg!(
    "<circle cx=\"8\" cy=\"8\" r=\"6\"/><path d=\"M8 2 A6 6 0 0 1 8 14 Z\" fill=\"currentColor\" stroke=\"none\"/>"
);
pub const ICON_GEAR: &str = svg!(
    "<circle cx=\"8\" cy=\"8\" r=\"2.5\"/><path d=\"M8 1.5 V3.2 M8 12.8 V14.5 M1.5 8 H3.2 M12.8 8 H14.5 M3.3 3.3 L4.5 4.5 M11.5 11.5 L12.7 12.7 M3.3 12.7 L4.5 11.5 M11.5 4.5 L12.7 3.3\"/>"
);
pub const ICON_DOT: &str =
    svg!("<circle cx=\"8\" cy=\"8\" r=\"3\" fill=\"currentColor\" stroke=\"none\"/>");

/// Map a legacy glyph string to its SVG bytes.
fn bytes_for(icon: &str) -> &'static [u8] {
    let s = match icon {
        "⌂" => ICON_HOME,
        "♫" => ICON_MUSIC,
        "⌕" => ICON_SEARCH,
        " " => ICON_ALBUM,
        "◎" => ICON_PERSON,
        "≡" => ICON_LIST,
        "★" => ICON_STAR,
        "◐" => ICON_HALF,
        "⚙" => ICON_GEAR,
        _ => ICON_DOT,
    };
    s.as_bytes()
}

pub fn nav_icon(icon: &str, color: gpui::Rgba) -> gpui::AnyElement {
    gpui::svg()
        .data(bytes_for(icon))
        .w(px(15.0))
        .h(px(15.0))
        .text_color(color)
        .into_any_element()
}
