use crate::lyrics::model::LyricLine;
use crate::lyrics::model::LyricsStatus;
use crate::theme::midnight_brass;

/// Pure: vertical translation to center the active lyric line.
pub fn target_offset(
    line_tops: &[f32],
    line_heights: &[f32],
    viewport_h: f32,
    active: usize,
) -> f32 {
    if active >= line_tops.len() || line_tops.is_empty() {
        return 0.0;
    }
    let line_center = line_tops[active] + line_heights[active] / 2.0;
    viewport_h / 2.0 - line_center
}
use gpui::{Context, Render, Window, div, prelude::*};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LyricsMode {
    Synced,
    Plain,
    Message,
}

pub struct LyricsView {
    lines: Vec<LyricLine>,
    active: Option<usize>,
    mode: LyricsMode,
    message: String,
    plain_text: String,
    offset_ms: i64,
    viewport_height: f32,
}

impl LyricsView {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            active: None,
            mode: LyricsMode::Message,
            message: "No lyrics".to_string(),
            plain_text: String::new(),
            offset_ms: 0,
            viewport_height: 400.0,
        }
    }

    pub fn set_status(&mut self, status: LyricsStatus) {
        match status {
            LyricsStatus::Loading => {
                self.mode = LyricsMode::Message;
                self.message = "Loading lyrics…".to_string();
                self.lines.clear();
                self.active = None;
            }
            LyricsStatus::Synced(lines) => {
                self.mode = LyricsMode::Synced;
                self.lines = lines;
                self.active = None;
            }
            LyricsStatus::Plain(text) => {
                self.mode = LyricsMode::Plain;
                self.plain_text = text;
                self.lines.clear();
                self.active = None;
            }
            LyricsStatus::Instrumental => {
                self.mode = LyricsMode::Message;
                self.message = "♪ Instrumental".to_string();
                self.lines.clear();
                self.active = None;
            }
            LyricsStatus::NotFound => {
                self.mode = LyricsMode::Message;
                self.message = "No lyrics found".to_string();
                self.lines.clear();
                self.active = None;
            }
            LyricsStatus::Error(e) => {
                self.mode = LyricsMode::Message;
                self.message = format!("Lyrics error: {}", e);
                self.lines.clear();
                self.active = None;
            }
        }
    }

    pub fn set_position(&mut self, position: Duration) {
        if self.mode != LyricsMode::Synced || self.lines.is_empty() {
            return;
        }
        let pos = position.as_secs_f64() + (self.offset_ms as f64 / 1000.0);
        let pos = if pos < 0.0 { 0.0 } else { pos };
        // binary search for active line
        let mut idx = None;
        for (i, line) in self.lines.iter().enumerate() {
            if line.time.as_secs_f64() <= pos {
                idx = Some(i);
            } else {
                break;
            }
        }
        self.active = idx;
    }

    pub fn set_offset_ms(&mut self, offset: i64) {
        self.offset_ms = offset;
    }

    pub fn set_viewport_height(&mut self, h: f32) {
        self.viewport_height = h;
    }

    pub fn active(&self) -> Option<usize> {
        self.active
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn mode(&self) -> LyricsMode {
        self.mode
    }
}

impl Render for LyricsView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = midnight_brass();
        let t = &theme.colors;

        match self.mode {
            LyricsMode::Message => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(gpui::px(14.0))
                        .text_color(t.text_secondary)
                        .child(self.message.clone()),
                ),
            LyricsMode::Plain => div()
                .size_full()
                .flex()
                .flex_col()
                .gap(gpui::px(4.0))
                .px(gpui::px(16.0))
                .py(gpui::px(12.0))
                .child(
                    div()
                        .text_size(gpui::px(13.0))
                        .text_color(t.text_secondary)
                        .child(self.plain_text.clone()),
                ),
            LyricsMode::Synced => {
                let _active = self.active.unwrap_or(0);
                let mut container = div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .gap(gpui::px(6.0))
                    .px(gpui::px(16.0))
                    .py(gpui::px(12.0));

                for (i, line) in self.lines.iter().enumerate() {
                    let is_active = self.active == Some(i);
                    let color = if is_active {
                        t.gold_light
                    } else {
                        t.text_secondary
                    };
                    let size = if is_active { 16.0 } else { 13.0 };
                    let weight = if is_active {
                        gpui::FontWeight::BOLD
                    } else {
                        gpui::FontWeight::NORMAL
                    };

                    let mut line_div = div()
                        .text_size(gpui::px(size))
                        .font_family(theme.ui_font)
                        .font_weight(weight)
                        .text_color(color)
                        .child(line.text.clone());

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

                    container = container.child(line_div);
                }

                container
            }
        }
    }
}

impl Default for LyricsView {
    fn default() -> Self {
        Self::new()
    }
}
