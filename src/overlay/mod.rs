#![allow(clippy::items_after_test_module)]
use crate::theme::midnight_brass;
use gpui::{Context, Render, Window, WindowBackgroundAppearance, WindowOptions, div, prelude::*};
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
pub fn clamp_geometry(
    geom: OverlayGeometry,
    monitor_x: i32,
    monitor_y: i32,
    monitor_w: u32,
    monitor_h: u32,
) -> OverlayGeometry {
    let w = geom.width.clamp(MIN_WIDTH, monitor_w);
    let h = geom.height.clamp(MIN_HEIGHT, monitor_h);
    let x = geom
        .x
        .clamp(monitor_x, monitor_x + (monitor_w as i32) - (w as i32));
    let y = geom
        .y
        .clamp(monitor_y, monitor_y + (monitor_h as i32) - (h as i32));
    OverlayGeometry {
        x,
        y,
        width: w,
        height: h,
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
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
        let g = OverlayGeometry {
            x: 0,
            y: 0,
            width: 10000,
            height: 10000,
        };
        let c = clamp_geometry(g, 0, 0, 1920, 1080);
        assert_eq!(c.width, 1920);
        assert_eq!(c.height, 1080);
    }

    #[test]
    fn clamp_position_to_monitor() {
        let g = OverlayGeometry {
            x: -100,
            y: -50,
            width: 392,
            height: 180,
        };
        let c = clamp_geometry(g, 0, 0, 1920, 1080);
        assert_eq!(c.x, 0);
        assert_eq!(c.y, 0);
    }

    #[test]
    fn clamp_min_size() {
        let g = OverlayGeometry {
            x: 0,
            y: 0,
            width: 100,
            height: 50,
        };
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

/// How many lines before / after the active one the pinned overlay shows.
/// Together with the active line this is the whole band (6 rows), which fits the
/// default 392x180 window; a song with 70 lines is therefore never crammed in.
pub const BAND_BEFORE: usize = 2;
pub const BAND_AFTER: usize = 3;

/// What the overlay is currently able to show.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum OverlayLyrics {
    /// Nothing to show yet; `OverlayState::message` explains why.
    #[default]
    Message,
    /// Timed lines, with the index of the one that is currently playing.
    Synced {
        lines: Vec<crate::lyrics::model::LyricLine>,
        active: Option<usize>,
    },
    /// Untimed transcript. The band follows the song's progress proportionally
    /// instead of pretending to be synchronised.
    Plain { lines: Vec<String>, index: usize },
}

#[derive(Debug, Clone, Default)]
pub struct OverlayState {
    pub title: String,
    pub artist: String,
    pub lyrics: OverlayLyrics,
    /// Shown when `lyrics` is `Message`.
    pub message: String,
    /// True when the text on screen comes from an unsynchronised transcript.
    pub unsynced: bool,
}

impl OverlayState {
    /// Start a track with nothing resolved yet.
    pub fn begin(&mut self, title: String, artist: String) {
        self.title = title;
        self.artist = artist;
        self.lyrics = OverlayLyrics::Message;
        self.message = "Loading lyrics…".to_string();
        self.unsynced = false;
    }

    /// Show a resolved lyrics status.
    pub fn set_status(&mut self, status: &crate::lyrics::model::LyricsStatus) {
        use crate::lyrics::model::LyricsStatus;
        self.unsynced = false;
        match status {
            LyricsStatus::Synced(lines) if !lines.is_empty() => {
                self.lyrics = OverlayLyrics::Synced {
                    lines: lines.clone(),
                    active: None,
                };
            }
            // A payload with no usable timings is a transcript, not a synced file.
            LyricsStatus::Synced(_) => self.set_message("No lyrics found"),
            LyricsStatus::Plain(text) => {
                let lines: Vec<String> = text
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                if lines.is_empty() {
                    self.set_message("No lyrics found");
                } else {
                    self.lyrics = OverlayLyrics::Plain { lines, index: 0 };
                    self.unsynced = true;
                }
            }
            LyricsStatus::Instrumental => self.set_message("♪ Instrumental"),
            LyricsStatus::Loading => self.set_message("Loading lyrics…"),
            LyricsStatus::NotFound => self.set_message("No lyrics found"),
            LyricsStatus::Error(_) => self.set_message("Lyrics unavailable"),
        }
    }

    pub fn set_message(&mut self, message: &str) {
        self.lyrics = OverlayLyrics::Message;
        self.message = message.to_string();
        self.unsynced = false;
    }

    /// Advance the band for a playback position, honouring the lyric offset.
    pub fn sync_position(
        &mut self,
        position: std::time::Duration,
        offset_ms: i64,
        duration: Option<std::time::Duration>,
    ) {
        use crate::lyrics::lrc::{active_line, apply_offset};
        match &mut self.lyrics {
            OverlayLyrics::Synced { lines, active } => {
                let pos = apply_offset(position, offset_ms);
                *active = active_line(lines, pos);
            }
            OverlayLyrics::Plain { lines, index } => {
                *index = plain_index(position, duration, lines.len());
            }
            OverlayLyrics::Message => {}
        }
    }

    /// Total number of lines, 0 in the message state.
    pub fn line_count(&self) -> usize {
        match &self.lyrics {
            OverlayLyrics::Synced { lines, .. } => lines.len(),
            OverlayLyrics::Plain { lines, .. } => lines.len(),
            OverlayLyrics::Message => 0,
        }
    }

    /// The active line index, when there is one.
    pub fn active(&self) -> Option<usize> {
        match &self.lyrics {
            OverlayLyrics::Synced { active, .. } => *active,
            OverlayLyrics::Plain { .. } => None,
            OverlayLyrics::Message => None,
        }
    }

    /// The text of the first visible line, for tests and diagnostics.
    pub fn first_visible_line(&self) -> Option<String> {
        self.visible_rows().first().map(|(text, _)| text.clone())
    }

    /// The band of rows to draw: at most `BAND_BEFORE + 1 + BAND_AFTER` of them.
    pub fn visible_rows(&self) -> Vec<(String, bool)> {
        match &self.lyrics {
            OverlayLyrics::Message => Vec::new(),
            OverlayLyrics::Synced { lines, active } => {
                let Some(active) = *active else {
                    // Before the first timestamp: show the opening lines, none active.
                    let end = lines.len().min(BAND_BEFORE + 1 + BAND_AFTER);
                    return lines[..end]
                        .iter()
                        .map(|l| (l.text.clone(), false))
                        .collect();
                };
                let range = band_range(active, lines.len(), BAND_BEFORE, BAND_AFTER);
                range
                    .map(|i| (lines[i].text.clone(), i == active))
                    .collect()
            }
            OverlayLyrics::Plain { lines, index } => {
                let active = (*index).min(lines.len().saturating_sub(1));
                let range = band_range(active, lines.len(), BAND_BEFORE, BAND_AFTER);
                range.map(|i| (lines[i].clone(), i == active)).collect()
            }
        }
    }
}

/// The slice of lines to show around `active`, full whenever the list allows it.
pub fn band_range(
    active: usize,
    len: usize,
    before: usize,
    after: usize,
) -> std::ops::Range<usize> {
    if len == 0 {
        return 0..0;
    }
    let active = active.min(len - 1);
    let want = (before + 1 + after).min(len);
    let mut start = active.saturating_sub(before);
    let mut end = (active + 1 + after).min(len);
    // Near the end, slide the band back so it stays full instead of shrinking.
    if end - start < want {
        start = end.saturating_sub(want);
        end = (start + want).min(len);
    }
    start..end
}

/// Which line of an unsynchronised transcript to centre, by progress through the
/// track. Without a duration there is nothing to follow, so it stays at the top.
pub fn plain_index(
    position: std::time::Duration,
    duration: Option<std::time::Duration>,
    len: usize,
) -> usize {
    if len == 0 {
        return 0;
    }
    let Some(duration) = duration else {
        return 0;
    };
    let total = duration.as_secs_f64();
    if total <= 0.0 {
        return 0;
    }
    let frac = (position.as_secs_f64() / total).clamp(0.0, 1.0);
    ((frac * len as f64) as usize).min(len - 1)
}

pub struct PinnedLyricsView {
    state: gpui::Entity<OverlayState>,
    geometry: std::sync::Arc<std::sync::Mutex<OverlayGeometry>>,
    /// Focus handle for the overlay's close control.
    close_focus: gpui::FocusHandle,
}

impl PinnedLyricsView {
    pub fn new(
        state: gpui::Entity<OverlayState>,
        geometry: std::sync::Arc<std::sync::Mutex<OverlayGeometry>>,
        cx: &mut gpui::App,
    ) -> Self {
        Self {
            state,
            geometry,
            close_focus: cx.focus_handle().tab_stop(true).tab_index(0),
        }
    }
}

impl Render for PinnedLyricsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = midnight_brass();
        let t = &theme.colors;
        let state = self.state.read(cx).clone();

        // Karaoke band: only the lines around the active one. Drawing the whole
        // song here is what made a 70-line lyric sheet unreadable in a 180 px
        // window.
        let rows = state.visible_rows();
        let mut lines_container = div()
            .flex()
            .flex_col()
            .justify_center()
            .gap(gpui::px(5.0))
            .px(gpui::px(16.0))
            .py(gpui::px(10.0))
            .flex_1()
            .overflow_hidden();

        if rows.is_empty() {
            lines_container = lines_container.items_center().child(
                div()
                    .text_size(gpui::px(12.0))
                    .font_family(theme.ui_font)
                    .text_color(t.text_secondary)
                    .child(state.message.clone()),
            );
        } else {
            for (text, is_active) in rows {
                let (line_size, line_color, line_weight, opacity) = if is_active {
                    (16.0, t.gold_light, gpui::FontWeight::BOLD, 1.0)
                } else {
                    (11.5, t.text_secondary, gpui::FontWeight::NORMAL, 0.72)
                };
                let mut line_div = div()
                    .w_full()
                    .text_size(gpui::px(line_size))
                    .font_family(theme.ui_font)
                    .font_weight(line_weight)
                    .text_color(line_color)
                    .opacity(opacity)
                    // Long lines wrap instead of overflowing the pinned window.
                    .child(text);
                if is_active {
                    line_div = line_div.child(
                        div()
                            .mt(gpui::px(3.0))
                            .w(gpui::px(40.0))
                            .h(gpui::px(2.0))
                            .rounded(gpui::px(999.0))
                            .bg(t.gold),
                    );
                }
                lines_container = lines_container.child(line_div);
            }
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
                    .id("overlay-header")
                    .aria_label("Drag lyrics overlay")
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|_this, _event, window, _cx| window.start_window_move()),
                    )
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
                            .id("overlay-close")
                            .track_focus(&self.close_focus)
                            .tab_index(0)
                            .role(gpui::Role::Button)
                            .border_2()
                            .border_color(if self.close_focus.is_focused(window) {
                                t.os_accent
                            } else {
                                gpui::rgba(0x00000000)
                            })
                            .aria_label("Close lyrics overlay")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _event, window, _cx| {
                                let bounds = window.bounds();
                                if let Ok(mut g) = this.geometry.lock() {
                                    *g = OverlayGeometry {
                                        x: f32::from(bounds.origin.x) as i32,
                                        y: f32::from(bounds.origin.y) as i32,
                                        width: f32::from(bounds.size.width) as u32,
                                        height: f32::from(bounds.size.height) as u32,
                                    };
                                }
                                window.remove_window();
                            }))
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
                            .flex()
                            .items_center()
                            .gap(gpui::px(6.0))
                            .child(
                                div()
                                    .text_size(gpui::px(9.0))
                                    .text_color(t.text_secondary)
                                    .child(format!("{} — {}", state.title, state.artist)),
                            )
                            .when(state.unsynced, |el| {
                                el.child(
                                    div()
                                        .px(gpui::px(5.0))
                                        .py(gpui::px(1.0))
                                        .rounded(gpui::px(999.0))
                                        .border_1()
                                        .border_color(t.glass_border)
                                        .text_size(gpui::px(8.0))
                                        .text_color(t.text_secondary)
                                        .child("unsynced"),
                                )
                            }),
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
pub fn open_overlay(
    cx: &mut gpui::App,
    state: gpui::Entity<OverlayState>,
    geometry: std::sync::Arc<std::sync::Mutex<OverlayGeometry>>,
    bounds: gpui::Bounds<gpui::Pixels>,
) -> gpui::WindowHandle<PinnedLyricsView> {
    let opts = WindowOptions {
        window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
        window_background: WindowBackgroundAppearance::Blurred,
        window_decorations: None,
        focus: false,
        show: true,
        app_id: Some(APP_ID.to_string()),
        titlebar: None,
        ..Default::default()
    };
    cx.open_window(opts, move |_window, cx| {
        cx.new(|cx| PinnedLyricsView::new(state, geometry, cx))
    })
    .expect("failed to open overlay window")
}

pub fn set_overlay_title(window: &mut gpui::Window) {
    window.set_window_title(OVERLAY_TITLE);
}

impl Default for OverlayWindow {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod band_tests {
    use super::*;
    use crate::lyrics::model::{LyricLine, LyricsStatus};
    use std::time::Duration;

    fn synced(n: usize) -> Vec<LyricLine> {
        (0..n)
            .map(|i| LyricLine {
                time: Duration::from_secs(i as u64 * 5),
                text: format!("line {i}"),
            })
            .collect()
    }

    #[test]
    fn band_keeps_six_rows_around_the_active_line() {
        let range = band_range(10, 70, BAND_BEFORE, BAND_AFTER);
        assert_eq!(range, 8..14);
        assert_eq!(range.len(), BAND_BEFORE + 1 + BAND_AFTER);
    }

    #[test]
    fn band_stays_full_at_the_start_and_the_end() {
        assert_eq!(band_range(0, 70, BAND_BEFORE, BAND_AFTER), 0..6);
        let end = band_range(69, 70, BAND_BEFORE, BAND_AFTER);
        assert_eq!(end, 64..70);
        assert_eq!(end.len(), 6);
    }

    #[test]
    fn band_never_exceeds_the_line_count() {
        assert_eq!(band_range(1, 3, BAND_BEFORE, BAND_AFTER), 0..3);
        assert_eq!(band_range(0, 0, BAND_BEFORE, BAND_AFTER), 0..0);
        assert_eq!(band_range(99, 4, BAND_BEFORE, BAND_AFTER), 0..4);
    }

    #[test]
    fn a_long_song_renders_at_most_six_rows() {
        let mut state = OverlayState::default();
        state.set_status(&LyricsStatus::Synced(synced(70)));
        state.sync_position(Duration::from_secs(300), 0, Some(Duration::from_secs(360)));
        let rows = state.visible_rows();
        assert_eq!(rows.len(), BAND_BEFORE + 1 + BAND_AFTER);
        assert_eq!(rows.iter().filter(|(_, active)| *active).count(), 1);
    }

    #[test]
    fn offset_shifts_which_line_is_active() {
        let mut state = OverlayState::default();
        state.set_status(&LyricsStatus::Synced(synced(10)));
        // 5 s lines: at 10 s line 2 is active, with no offset.
        state.sync_position(Duration::from_secs(10), 0, None);
        assert_eq!(state.active(), Some(2));
        // -5000 ms of offset moves playback back a whole line.
        state.sync_position(Duration::from_secs(10), -5_000, None);
        assert_eq!(state.active(), Some(1));
    }

    #[test]
    fn plain_lyrics_follow_progress_and_never_highlight_a_fake_line() {
        let mut state = OverlayState::default();
        state.set_status(&LyricsStatus::Plain(
            (0..10)
                .map(|i| format!("row {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ));
        assert!(state.unsynced);
        assert_eq!(
            state.active(),
            None,
            "plain lyrics must not fake an active line"
        );
        assert_eq!(
            plain_index(Duration::ZERO, Some(Duration::from_secs(100)), 10),
            0
        );
        assert_eq!(
            plain_index(Duration::from_secs(50), Some(Duration::from_secs(100)), 10),
            5
        );
        assert_eq!(
            plain_index(Duration::from_secs(100), Some(Duration::from_secs(100)), 10),
            9
        );
        // No duration: nothing to follow, stay at the top.
        assert_eq!(plain_index(Duration::from_secs(50), None, 10), 0);
    }

    #[test]
    fn states_show_a_message_instead_of_an_empty_box() {
        for (status, expected) in [
            (LyricsStatus::Instrumental, "♪ Instrumental"),
            (LyricsStatus::NotFound, "No lyrics found"),
            (LyricsStatus::Loading, "Loading lyrics…"),
            (LyricsStatus::Error("boom".into()), "Lyrics unavailable"),
        ] {
            let mut state = OverlayState::default();
            state.set_status(&status);
            assert_eq!(state.line_count(), 0);
            assert!(state.visible_rows().is_empty());
            assert_eq!(state.message, expected, "for {status:?}");
        }
    }

    #[test]
    fn blank_plain_lines_are_dropped() {
        let mut state = OverlayState::default();
        state.set_status(&LyricsStatus::Plain("a\n\n   \nb".to_string()));
        assert_eq!(state.line_count(), 2);
        assert_eq!(state.first_visible_line().as_deref(), Some("a"));
    }
}
