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
use gpui::{AnimationExt, Context, Render, Window, div, prelude::*};
use std::time::Duration;

const LYRIC_ROW_H: f32 = 40.0;

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
    last_position: Duration,
    anim_key: usize,
    from_offset: f32,
    to_offset: f32,
    current_offset: f32,
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
            last_position: Duration::ZERO,
            anim_key: 0,
            from_offset: 0.0,
            to_offset: 0.0,
            current_offset: 0.0,
        }
    }

    pub fn set_status(&mut self, status: LyricsStatus) {
        self.active = None;
        self.last_position = Duration::ZERO;
        self.anim_key = 0;
        self.from_offset = 0.0;
        self.to_offset = 0.0;
        self.current_offset = 0.0;
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
        if idx != self.active {
            let previous = self.active;
            self.active = idx;
            let active = idx.unwrap_or(0);
            let target = -(active as f32 * LYRIC_ROW_H);
            let seek_jump = position.abs_diff(self.last_position) > Duration::from_secs(2);
            let big_jump = previous.map(|p| p.abs_diff(active) > 2).unwrap_or(false);
            self.from_offset = if seek_jump || big_jump {
                target
            } else {
                self.current_offset
            };
            self.to_offset = target;
            self.current_offset = target;
            self.anim_key = self.anim_key.wrapping_add(1);
        }
        self.last_position = position;
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
                )
                .into_any_element(),
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
                )
                .into_any_element(),
            LyricsMode::Synced => {
                let pad = (self.viewport_height / 2.0 - LYRIC_ROW_H / 2.0).max(0.0);
                let mut container = div()
                    .flex()
                    .flex_col()
                    .px(gpui::px(24.0))
                    .pt(gpui::px(pad))
                    .pb(gpui::px(pad));

                for (i, line) in self.lines.iter().enumerate() {
                    let is_active = self.active == Some(i);
                    let color = if is_active {
                        t.gold_light
                    } else {
                        t.text_secondary
                    };
                    let size = if is_active { 21.0 } else { 15.0 };
                    let weight = if is_active {
                        gpui::FontWeight::BOLD
                    } else {
                        gpui::FontWeight::NORMAL
                    };

                    let text = div()
                        .text_size(gpui::px(size))
                        .font_family(theme.ui_font)
                        .font_weight(weight)
                        .text_color(color)
                        .child(line.text.clone());

                    let mut inner = div().flex().flex_col().gap(gpui::px(3.0)).child(text);
                    if is_active {
                        inner = inner.child(
                            div()
                                .w(gpui::px(40.0))
                                .h(gpui::px(2.0))
                                .rounded(gpui::px(999.0))
                                .bg(t.gold),
                        );
                    }

                    let line = div()
                        .flex()
                        .items_start()
                        .gap(gpui::px(9.0))
                        .min_h(gpui::px(LYRIC_ROW_H))
                        .child(
                            div()
                                .w(gpui::px(6.0))
                                .h(gpui::px(6.0))
                                .mt(gpui::px(if is_active { 11.0 } else { 8.0 }))
                                .rounded(gpui::px(999.0))
                                .bg(if is_active { t.gold } else { t.text_tertiary })
                                .opacity(if is_active { 1.0 } else { 0.35 }),
                        )
                        .child(inner);
                    container = container.child(
                        line.with_animation(
                            format!("lyric-{i}-{is_active}"),
                            gpui::Animation::new(std::time::Duration::from_millis(220))
                                .with_easing(gpui::ease_out_quint()),
                            |el, delta| el.opacity(0.45 + 0.55 * delta),
                        ),
                    );
                }

                let animated = if self.anim_key == 0 {
                    container.mt(gpui::px(self.to_offset)).into_any_element()
                } else {
                    let from = self.from_offset;
                    let to = self.to_offset;
                    container
                        .with_animation(
                            format!("lyrics-{}", self.anim_key),
                            gpui::Animation::new(Duration::from_millis(380))
                                .with_easing(gpui::ease_out_quint()),
                            move |el, delta| el.mt(gpui::px(from + (to - from) * delta)),
                        )
                        .into_any_element()
                };
                div()
                    .size_full()
                    .overflow_hidden()
                    .child(animated)
                    .into_any_element()
            }
        }
    }
}

impl Default for LyricsView {
    fn default() -> Self {
        Self::new()
    }
}
