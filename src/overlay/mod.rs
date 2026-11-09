use crate::theme::midnight_brass;
use gpui::{
    Context, Render, Window, WindowBackgroundAppearance, WindowOptions, div, prelude::*,
};
pub const DEFAULT_WIDTH: u32 = 392;
pub const DEFAULT_HEIGHT: u32 = 180;
pub const MIN_WIDTH: u32 = 320;
pub const MIN_HEIGHT: u32 = 140;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Default for OverlayGeometry {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
        }
    }
}

/// Clamp overlay geometry to a monitor work area.
pub fn clamp_geometry(geom: OverlayGeometry, monitor_x: i32, monitor_y: i32, monitor_w: u32, monitor_h: u32) -> OverlayGeometry {
    let w = geom.width.clamp(MIN_WIDTH, monitor_w);
    let h = geom.height.clamp(MIN_HEIGHT, monitor_h);
    let x = geom.x.clamp(monitor_x, monitor_x + (monitor_w as i32) - (w as i32));
    let y = geom.y.clamp(monitor_y, monitor_y + (monitor_h as i32) - (h as i32));
    OverlayGeometry { x, y, width: w, height: h }
}

#[cfg(test)]
mod geometry_tests {
    use super::*;

    #[test]
    fn default_geometry() {
        let g = OverlayGeometry::default();
        assert_eq!(g.width, DEFAULT_WIDTH);
        assert_eq!(g.height, DEFAULT_HEIGHT);
    }

    #[test]
    fn clamp_size_to_monitor() {
        let g = OverlayGeometry { x: 0, y: 0, width: 10000, height: 10000 };
        let c = clamp_geometry(g, 0, 0, 1920, 1080);
        assert_eq!(c.width, 1920);
        assert_eq!(c.height, 1080);
    }

    #[test]
    fn clamp_position_to_monitor() {
        let g = OverlayGeometry { x: -100, y: -50, width: 392, height: 180 };
        let c = clamp_geometry(g, 0, 0, 1920, 1080);
        assert_eq!(c.x, 0);
        assert_eq!(c.y, 0);
    }

    #[test]
    fn clamp_min_size() {
        let g = OverlayGeometry { x: 0, y: 0, width: 100, height: 50 };
        let c = clamp_geometry(g, 0, 0, 1920, 1080);
        assert_eq!(c.width, MIN_WIDTH);
        assert_eq!(c.height, MIN_HEIGHT);
    }
}

pub const OVERLAY_TITLE: &str = "Myuzik Lyrics";
pub const APP_ID: &str = "io.github.samvivan1.Myuzik";

pub struct OverlayWindow {
    pub geometry: OverlayGeometry,
    pub open: bool,
}

impl OverlayWindow {
    pub fn new() -> Self {
        Self {
            geometry: OverlayGeometry::default(),
            open: false,
        }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn close(&mut self) {
        self.open = false;
    }
}

pub struct PinnedLyricsView {
    title: String,
    artist: String,
    lines: Vec<String>,
    active: Option<usize>,
}

impl PinnedLyricsView {
    pub fn new() -> Self {
        Self {
            title: "Myuzik".to_string(),
            artist: "No track playing".to_string(),
            lines: vec!["Lyrics will appear here.".to_string()],
            active: None,
        }
    }

    pub fn set_track(&mut self, title: String, artist: String) {
        self.title = title;
        self.artist = artist;
    }

    pub fn set_lines(&mut self, lines: Vec<String>, active: Option<usize>) {
        self.lines = lines;
        self.active = active;
    }
}

impl Render for PinnedLyricsView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = midnight_brass();
        let t = &theme.colors;

        let mut lines_container = div()
            .flex()
            .flex_col()
            .gap(gpui::px(4.0))
            .px(gpui::px(16.0))
            .py(gpui::px(12.0));

        for (i, line) in self.lines.iter().enumerate() {
            let is_active = self.active == Some(i);
            let line_color = if is_active { t.gold_light } else { t.text_secondary };
            let line_size = if is_active { 13.0 } else { 11.0 };
            let line_weight = if is_active {
                gpui::FontWeight::BOLD
            } else {
                gpui::FontWeight::NORMAL
            };
            let mut line_div = div()
                .text_size(gpui::px(line_size))
                .font_family(theme.ui_font)
                .font_weight(line_weight)
                .text_color(line_color)
                .child(line.clone());
            if is_active {
                line_div = line_div.child(
                    div()
                        .mt(gpui::px(2.0))
                        .w(gpui::px(40.0))
                        .h(gpui::px(2.0))
                        .rounded(gpui::px(999.0))
                        .bg(t.gold),
                );
            }
            lines_container = lines_container.child(line_div);
        }

        div()
            .size_full()
            .bg(t.glass_tint)
            .text_color(t.text_primary)
            .flex()
            .flex_col()
            .child(
                // header
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(gpui::px(14.0))
                    .py(gpui::px(10.0))
                    .border_b_1()
                    .border_color(t.glass_border)
                    .child(
                        div()
                            .px(gpui::px(8.0))
                            .py(gpui::px(4.0))
                            .rounded(gpui::px(999.0))
                            .bg(t.gold)
                            .text_color(t.clay_ink)
                            .font_family(theme.ui_font)
                            .text_size(gpui::px(9.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("Pinned lyrics"),
                    )
                    .child(
                        div()
                            .w(gpui::px(24.0))
                            .h(gpui::px(24.0))
                            .rounded(gpui::px(999.0))
                            .bg(gpui::rgba(0xFFFFFFFF))
                            .text_color(t.text_secondary)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(gpui::px(12.0))
                            .child("×"),
                    ),
            )
            .child(lines_container)
            .child(
                div()
                    .px(gpui::px(16.0))
                    .py(gpui::px(8.0))
                    .border_t_1()
                    .border_color(t.glass_border)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(gpui::px(9.0))
                            .text_color(t.text_secondary)
                            .child(format!("{} — {}", self.title, self.artist)),
                    )
                    .child(
                        div()
                            .w(gpui::px(60.0))
                            .h(gpui::px(3.0))
                            .rounded(gpui::px(999.0))
                            .bg(t.gold)
                            .opacity(0.6),
                    ),
            )
    }
}

/// Open the overlay window. Returns the window handle.
pub fn open_overlay(cx: &mut gpui::App) -> gpui::WindowHandle<PinnedLyricsView> {
    let mut opts = WindowOptions::default();
    opts.window_background = WindowBackgroundAppearance::Blurred;
    opts.window_decorations = None;
    opts.focus = false;
    opts.show = true;
    opts.app_id = Some(APP_ID.to_string());
    opts.titlebar = None;
    cx.open_window(opts, |_window, cx| {
        cx.new(|_cx| PinnedLyricsView::new())
    })
    .expect("failed to open overlay window")
}

pub fn set_overlay_title(window: &mut gpui::Window) {
    window.set_window_title(OVERLAY_TITLE);
}
