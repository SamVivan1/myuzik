//! The pinned lyrics overlay ("PiP"): a small always-on-top window that keeps the
//! current lyric line visible while you work elsewhere.
//!
//! Layout follows the approved reference: two columns. On the left, the artwork
//! with the track title, the artist and a real progress bar; on the right, a band
//! of six lyric lines centred vertically, with the line being sung emphasised.
//! Only the band is ever drawn, so a 70-line lyric sheet still fits (and stays
//! readable) in a 620x330 window.
//!
//! Cheap by construction: the state is read without cloning the whole lyric list,
//! the artwork is the same `Arc<Image>` the main window already caches, and there
//! is no timer or effect work when playback is paused.

#![allow(clippy::items_after_test_module)]

use crate::theme::midnight_brass;
use gpui::{Context, Render, Window, WindowBackgroundAppearance, WindowOptions, div, prelude::*};

pub const DEFAULT_WIDTH: u32 = 620;
pub const DEFAULT_HEIGHT: u32 = 330;
pub const MIN_WIDTH: u32 = 520;
pub const MIN_HEIGHT: u32 = 280;

/// Lyric rows drawn: three above the active line, three below. The outermost
/// rows sit under the edge fade, so the band reads as a continuous lyric stream
/// rather than a fixed list.
pub const BAND_BEFORE: usize = 3;
pub const BAND_AFTER: usize = 3;
/// Width of the left column (artwork, title, artist, progress).
const LEFT_COLUMN: f32 = 160.0;

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

impl Default for OverlayWindow {
    fn default() -> Self {
        Self::new()
    }
}

/// How prominent one lyric row is.
///
/// The reference shades four tiers: the line being sung, the line that follows,
/// the line just before, and everything further away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineTier {
    Active,
    Next,
    Far,
    Dim,
}

impl LineTier {
    pub fn is_active(self) -> bool {
        matches!(self, LineTier::Active)
    }

    fn of(index: usize, active: Option<usize>) -> Self {
        match active {
            Some(active) if index == active => LineTier::Active,
            Some(active) if index == active + 1 => LineTier::Next,
            Some(active) if index + 1 == active => LineTier::Far,
            Some(_) => LineTier::Dim,
            // Before the first timestamp there is no active line yet.
            None => LineTier::Dim,
        }
    }
}

/// One row to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleRow {
    pub text: String,
    pub tier: LineTier,
}

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
    /// Artwork for the left column; the same `Arc` the main window caches.
    pub cover: Option<std::sync::Arc<gpui::Image>>,
    /// Playback position, for the progress bar.
    pub position: std::time::Duration,
    /// Track length, when known.
    pub duration: Option<std::time::Duration>,
}

impl OverlayState {
    /// Start a track with nothing resolved yet.
    pub fn begin(&mut self, title: String, artist: String) {
        self.title = title;
        self.artist = artist;
        self.lyrics = OverlayLyrics::Message;
        self.message = "Loading lyrics…".to_string();
        self.unsynced = false;
        self.cover = None;
        self.position = std::time::Duration::ZERO;
        self.duration = None;
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

    /// Artwork for the left column.
    pub fn set_cover(&mut self, cover: Option<std::sync::Arc<gpui::Image>>) {
        self.cover = cover;
    }

    /// Advance the band for a playback position, honouring the lyric offset.
    pub fn sync_position(
        &mut self,
        position: std::time::Duration,
        offset_ms: i64,
        duration: Option<std::time::Duration>,
    ) {
        use crate::lyrics::lrc::{active_line, apply_offset};
        self.position = position;
        self.duration = duration;
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

    /// How far through the track we are, 0.0–1.0. Zero without a duration.
    pub fn progress_fraction(&self) -> f32 {
        let Some(total) = self.duration else {
            return 0.0;
        };
        let total = total.as_secs_f64();
        if total <= 0.0 {
            return 0.0;
        }
        ((self.position.as_secs_f64() / total) as f32).clamp(0.0, 1.0)
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
        self.visible_rows().first().map(|row| row.text.clone())
    }

    /// The band of rows to draw: at most `BAND_BEFORE + 1 + BAND_AFTER` of them.
    pub fn visible_rows(&self) -> Vec<VisibleRow> {
        match &self.lyrics {
            OverlayLyrics::Message => Vec::new(),
            OverlayLyrics::Synced { lines, active } => {
                let range = match *active {
                    Some(active) => band_range(active, lines.len(), BAND_BEFORE, BAND_AFTER),
                    // Before the first timestamp: show the opening lines, none active.
                    None => 0..lines.len().min(BAND_BEFORE + 1 + BAND_AFTER),
                };
                range
                    .map(|i| VisibleRow {
                        text: lines[i].text.clone(),
                        tier: LineTier::of(i, *active),
                    })
                    .collect()
            }
            OverlayLyrics::Plain { lines, index } => {
                let active = (*index).min(lines.len().saturating_sub(1));
                let range = band_range(active, lines.len(), BAND_BEFORE, BAND_AFTER);
                range
                    .map(|i| VisibleRow {
                        text: lines[i].clone(),
                        tier: LineTier::of(i, Some(active)),
                    })
                    .collect()
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

/// `m:ss` for the progress labels.
fn fmt_time(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
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

        // Read once, copy only what is drawn: the old code cloned every lyric
        // line about ten times a second just to draw six of them.
        let (title, artist, cover, unsynced, message, rows, progress, elapsed, total) = {
            let state = self.state.read(cx);
            (
                state.title.clone(),
                state.artist.clone(),
                state.cover.clone(),
                state.unsynced,
                state.message.clone(),
                state.visible_rows(),
                state.progress_fraction(),
                fmt_time(state.position),
                state
                    .duration
                    .map(fmt_time)
                    .unwrap_or_else(|| "--:--".to_string()),
            )
        };

        // ---- left column: artwork, title, artist, progress ----
        let art = div()
            .w(gpui::px(LEFT_COLUMN))
            .h(gpui::px(LEFT_COLUMN))
            .flex_none()
            .rounded(gpui::px(18.0))
            .overflow_hidden()
            .border_1()
            .border_color(gpui::rgba(0xFFFFFF40))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::rgba(0x00000066).into(),
                offset: gpui::point(gpui::px(0.0), gpui::px(10.0)),
                blur_radius: gpui::px(24.0),
                spread_radius: gpui::px(0.0),
                inset: false,
            }])
            .child(match &cover {
                Some(image) => div()
                    .size_full()
                    .child(
                        gpui::img(gpui::ImageSource::Image(image.clone()))
                            .w(gpui::px(LEFT_COLUMN))
                            .h(gpui::px(LEFT_COLUMN))
                            .object_fit(gpui::ObjectFit::Cover),
                    )
                    .into_any_element(),
                None => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui::linear_gradient(
                        135.0,
                        gpui::linear_color_stop(t.clay_gradient_start, 0.0),
                        gpui::linear_color_stop(t.gold_deep, 1.0),
                    ))
                    .text_size(gpui::px(34.0))
                    .text_color(t.clay_ink)
                    .font_family(theme.display_font)
                    .child("♪")
                    .into_any_element(),
            });

        let progress_fill = LEFT_COLUMN * progress;

        let left = div()
            .w(gpui::px(LEFT_COLUMN))
            .flex_none()
            .flex()
            .flex_col()
            .child(art)
            .child(
                div()
                    .mt(gpui::px(13.0))
                    .font_family(theme.display_font)
                    .text_size(gpui::px(20.0))
                    .line_height(gpui::px(25.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(t.text_primary)
                    .child(title),
            )
            .child(
                div()
                    .mt(gpui::px(4.0))
                    .flex()
                    .items_center()
                    .gap(gpui::px(6.0))
                    .child(
                        div()
                            .text_size(gpui::px(12.5))
                            .text_color(t.text_secondary)
                            .child(artist),
                    )
                    .when(unsynced, |el| {
                        el.child(
                            div()
                                .px(gpui::px(5.0))
                                .py(gpui::px(1.0))
                                .rounded(gpui::px(999.0))
                                .border_1()
                                .border_color(t.gold_tint_border)
                                .text_size(gpui::px(8.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(t.gold)
                                .child("UNSYNCED"),
                        )
                    }),
            )
            .child(
                // Real progress, not decoration.
                div()
                    .mt(gpui::px(16.0))
                    .w(gpui::px(LEFT_COLUMN))
                    .h(gpui::px(5.0))
                    .rounded(gpui::px(999.0))
                    .bg(gpui::rgba(0xFFFFFF2E))
                    .overflow_hidden()
                    .child(
                        div()
                            .h(gpui::px(5.0))
                            .w(gpui::px(progress_fill))
                            .rounded(gpui::px(999.0))
                            .bg(gpui::linear_gradient(
                                90.0,
                                gpui::linear_color_stop(t.gold_light, 0.0),
                                gpui::linear_color_stop(t.gold, 1.0),
                            )),
                    ),
            )
            .child(
                div()
                    .mt(gpui::px(8.0))
                    .w(gpui::px(LEFT_COLUMN))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(gpui::px(10.0))
                    .text_color(t.text_tertiary)
                    .child(elapsed)
                    .child(total),
            )
            .child(div().flex_1());

        // ---- right column: the lyric band ----
        let mut lines = div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .justify_center()
            .items_center()
            .gap(gpui::px(17.0))
            .px(gpui::px(6.0))
            // Room for the close control, so it never sits on a lyric.
            .pr(gpui::px(38.0))
            .overflow_hidden();

        if rows.is_empty() {
            lines = lines.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(gpui::px(4.0))
                    .child(
                        div()
                            .text_size(gpui::px(14.0))
                            .font_family(theme.ui_font)
                            .text_color(t.text_secondary)
                            .child(message),
                    )
                    .child(
                        div()
                            .text_size(gpui::px(10.5))
                            .font_family(theme.ui_font)
                            .text_color(t.text_tertiary)
                            .child("Lyrics come from Navidrome and LRCLIB"),
                    ),
            );
        } else {
            for row in rows {
                let (size, color, weight, opacity) = match row.tier {
                    LineTier::Active => (20.0, t.gold_light, gpui::FontWeight::SEMIBOLD, 1.0),
                    LineTier::Next => (16.5, t.text_primary, gpui::FontWeight::MEDIUM, 0.94),
                    LineTier::Far => (16.0, t.text_primary, gpui::FontWeight::NORMAL, 0.72),
                    LineTier::Dim => (15.5, t.text_secondary, gpui::FontWeight::NORMAL, 0.52),
                };
                lines = lines.child(
                    div()
                        .w_full()
                        .text_center()
                        .text_size(gpui::px(size))
                        .line_height(gpui::px(size * 1.34))
                        .font_family(theme.ui_font)
                        .font_weight(weight)
                        .text_color(color)
                        .opacity(opacity)
                        .child(row.text),
                );
            }
        }

        let right = div().flex_1().h_full().child(lines);

        let ambient = div()
            .absolute()
            .inset_0()
            .child(
                div()
                    .absolute()
                    .left(gpui::px(-70.0))
                    .top(gpui::px(-60.0))
                    .w(gpui::px(300.0))
                    .h(gpui::px(300.0))
                    .rounded(gpui::px(999.0))
                    .bg(gpui::linear_gradient(
                        150.0,
                        gpui::linear_color_stop(gpui::rgba(0xC9A86A5C), 0.0),
                        gpui::linear_color_stop(gpui::rgba(0x00000000), 0.62),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .right(gpui::px(-90.0))
                    .bottom(gpui::px(-80.0))
                    .w(gpui::px(320.0))
                    .h(gpui::px(320.0))
                    .rounded(gpui::px(999.0))
                    .bg(gpui::linear_gradient(
                        330.0,
                        gpui::linear_color_stop(gpui::rgba(0x2F6B6340), 0.0),
                        gpui::linear_color_stop(gpui::rgba(0x00000000), 0.58),
                    )),
            );

        div()
            .size_full()
            .rounded(gpui::px(22.0))
            .overflow_hidden()
            .bg(gpui::linear_gradient(
                155.0,
                gpui::linear_color_stop(gpui::rgb(0x1B1614), 0.0),
                gpui::linear_color_stop(gpui::rgb(0x0C0B0F), 1.0),
            ))
            .border_1()
            .border_color(gpui::rgba(0xE8D3A342))
            .shadow(vec![
                // Deep drop shadow: the panel floats above the desktop.
                gpui::BoxShadow {
                    color: gpui::rgba(0x000000A0).into(),
                    offset: gpui::point(gpui::px(0.0), gpui::px(20.0)),
                    blur_radius: gpui::px(52.0),
                    spread_radius: gpui::px(0.0),
                    inset: false,
                },
                // Inset top highlight: the glass edge catching the light.
                gpui::BoxShadow {
                    color: gpui::rgba(0xFFFFFF1A).into(),
                    offset: gpui::point(gpui::px(0.0), gpui::px(1.0)),
                    blur_radius: gpui::px(1.0),
                    spread_radius: gpui::px(0.0),
                    inset: true,
                },
                gpui::BoxShadow {
                    color: t.shadow_warm.into(),
                    offset: gpui::point(gpui::px(0.0), gpui::px(-2.0)),
                    blur_radius: gpui::px(10.0),
                    spread_radius: gpui::px(0.0),
                    inset: true,
                },
            ])
            .text_color(t.text_primary)
            // The whole panel drags the window.
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|_this, _event, window, _cx| window.start_window_move()),
            )
            .child(ambient)
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_row()
                    .gap(gpui::px(24.0))
                    .p(gpui::px(20.0))
                    .child(left)
                    .child(right),
            )
            .child(
                div()
                    .absolute()
                    .top(gpui::px(12.0))
                    .right(gpui::px(12.0))
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
                            .w(gpui::px(28.0))
                            .h(gpui::px(28.0))
                            .rounded(gpui::px(999.0))
                            .bg(gpui::rgba(0xFFFFFF1F))
                            .opacity(0.8)
                            .hover(|style| style.opacity(1.0).bg(gpui::rgba(0xFFFFFF1F)))
                            .text_color(t.text_secondary)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(gpui::px(13.0))
                            .child("×"),
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
            width: 620,
            height: 330,
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
    fn band_keeps_seven_rows_around_the_active_line() {
        let range = band_range(10, 70, BAND_BEFORE, BAND_AFTER);
        assert_eq!(range, 7..14);
        assert_eq!(range.len(), BAND_BEFORE + 1 + BAND_AFTER);
    }

    #[test]
    fn band_stays_full_at_the_start_and_the_end() {
        assert_eq!(band_range(0, 70, BAND_BEFORE, BAND_AFTER), 0..7);
        let end = band_range(69, 70, BAND_BEFORE, BAND_AFTER);
        assert_eq!(end, 63..70);
        assert_eq!(end.len(), 7);
    }

    #[test]
    fn band_never_exceeds_the_line_count() {
        assert_eq!(band_range(1, 3, BAND_BEFORE, BAND_AFTER), 0..3);
        assert_eq!(band_range(0, 0, BAND_BEFORE, BAND_AFTER), 0..0);
        assert_eq!(band_range(99, 4, BAND_BEFORE, BAND_AFTER), 0..4);
    }

    #[test]
    fn a_long_song_renders_at_most_seven_rows() {
        let mut state = OverlayState::default();
        state.set_status(&LyricsStatus::Synced(synced(70)));
        state.sync_position(Duration::from_secs(300), 0, Some(Duration::from_secs(360)));
        let rows = state.visible_rows();
        assert_eq!(rows.len(), 7);
        assert_eq!(rows.iter().filter(|r| r.tier.is_active()).count(), 1);
    }

    #[test]
    fn tiers_follow_the_active_line() {
        let mut state = OverlayState::default();
        state.set_status(&LyricsStatus::Synced(synced(70)));
        // 5 s per line: at 300 s line 60 is playing.
        state.sync_position(Duration::from_secs(300), 0, None);
        let rows = state.visible_rows();
        let tiers: Vec<LineTier> = rows.iter().map(|r| r.tier).collect();
        assert_eq!(
            tiers,
            vec![
                LineTier::Dim,
                LineTier::Dim,
                LineTier::Far,
                LineTier::Active,
                LineTier::Next,
                LineTier::Dim,
                LineTier::Dim,
            ]
        );
        assert_eq!(rows[3].text, "line 60");
    }

    #[test]
    fn offset_shifts_which_line_is_active() {
        let mut state = OverlayState::default();
        state.set_status(&LyricsStatus::Synced(synced(10)));
        state.sync_position(Duration::from_secs(10), 0, None);
        assert_eq!(state.active(), Some(2));
        state.sync_position(Duration::from_secs(10), -5_000, None);
        assert_eq!(state.active(), Some(1));
    }

    #[test]
    fn progress_fraction_is_clamped() {
        let mut state = OverlayState::default();
        assert_eq!(state.progress_fraction(), 0.0, "no duration yet");
        state.sync_position(Duration::from_secs(50), 0, Some(Duration::from_secs(100)));
        assert!((state.progress_fraction() - 0.5).abs() < f32::EPSILON);
        state.sync_position(Duration::from_secs(500), 0, Some(Duration::from_secs(100)));
        assert_eq!(state.progress_fraction(), 1.0, "clamped at the end");
        state.sync_position(Duration::ZERO, 0, Some(Duration::ZERO));
        assert_eq!(state.progress_fraction(), 0.0, "zero duration");
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
            state
                .visible_rows()
                .iter()
                .filter(|r| r.tier.is_active())
                .count(),
            1,
            "the band still centres the line the song has reached"
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

    #[test]
    fn begin_clears_the_previous_track_artwork_and_progress() {
        let mut state = OverlayState::default();
        state.sync_position(Duration::from_secs(30), 0, Some(Duration::from_secs(60)));
        state.begin("New".to_string(), "Artist".to_string());
        assert_eq!(state.cover, None);
        assert_eq!(state.progress_fraction(), 0.0);
        assert_eq!(state.line_count(), 0);
    }
}
