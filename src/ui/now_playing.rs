use crate::lyrics::model::LyricsStatus;
use crate::player::engine::{PlayerCommand, PlayerHandle};
use crate::theme::midnight_brass;
use crate::ui::components::LyricsView;
use gpui::{Context, Render, Window, div, prelude::*};

#[derive(Clone, Copy, Debug)]
pub enum NowPlayingEvent {
    PinToggle,
    OffsetDelta(i64),
}

impl gpui::EventEmitter<NowPlayingEvent> for NowPlayingView {}

pub struct NowPlayingView {
    player: PlayerHandle,
    playing: bool,
    pub lyrics: gpui::Entity<LyricsView>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub position: f64,
    pub duration: f64,
    pub cover_art: Option<gpui::ImageSource>,
    source_label: String,
    offset_ms: i64,
}

impl NowPlayingView {
    pub fn new(cx: &mut gpui::App, player: PlayerHandle) -> Self {
        Self {
            player,
            playing: false,
            lyrics: cx.new(|_cx| LyricsView::new()),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            position: 0.0,
            duration: 0.0,
            cover_art: None,
            source_label: "Navidrome / LRCLIB".to_string(),
            offset_ms: 0,
        }
    }

    pub fn set_source_label(&mut self, label: String) {
        self.source_label = label;
    }

    pub fn set_offset_ms(&mut self, offset: i64) {
        self.offset_ms = offset;
    }

    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }

    pub fn set_cover(&mut self, image: Option<std::sync::Arc<gpui::Image>>) {
        self.cover_art = image.map(gpui::ImageSource::Image);
    }

    pub fn set_track(&mut self, title: String, artist: String, album: String) {
        self.title = title;
        self.artist = artist;
        self.album = album;
    }

    pub fn set_lyrics(&mut self, cx: &mut gpui::App, status: LyricsStatus) {
        self.lyrics.update(cx, |view, _cx| {
            view.set_status(status);
        });
    }

    pub fn set_position(&mut self, cx: &mut gpui::App, pos: f64, dur: f64) {
        self.position = pos;
        self.duration = dur;
        self.lyrics.update(cx, |view, _cx| {
            view.set_position(std::time::Duration::from_secs_f64(pos));
        });
    }
}

impl Render for NowPlayingView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = midnight_brass();
        let t = &theme.colors;

        let progress_pct = if self.duration > 0.0 {
            (self.position / self.duration * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        div()
            .size_full()
            .flex()
            .gap(gpui::px(24.0))
            .px(gpui::px(24.0))
            .py(gpui::px(20.0))
            .child(
                // Left: album art + metadata + controls
                div()
                    .w(gpui::px(360.0))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(gpui::px(16.0))
                    .child(
                        // Ivory clay frame around the cover
                        div()
                            .p(gpui::px(6.0))
                            .rounded(gpui::px(30.0))
                            .bg(gpui::linear_gradient(
                                145.0,
                                gpui::linear_color_stop(t.clay_surface, 0.0),
                                gpui::linear_color_stop(t.clay_shade, 1.0),
                            ))
                            .shadow(vec![
                                gpui::BoxShadow {
                                    color: gpui::hsla(0.0, 0.0, 0.0, 0.4),
                                    offset: gpui::point(gpui::px(0.0), gpui::px(26.0)),
                                    blur_radius: gpui::px(60.0),
                                    spread_radius: gpui::px(0.0),
                                    inset: false,
                                },
                                gpui::BoxShadow {
                                    color: gpui::rgba(0xFFFFFFCC).into(),
                                    offset: gpui::point(gpui::px(2.0), gpui::px(2.0)),
                                    blur_radius: gpui::px(4.0),
                                    spread_radius: gpui::px(0.0),
                                    inset: true,
                                },
                                gpui::BoxShadow {
                                    color: gpui::rgba(0x8C6D3F33).into(),
                                    offset: gpui::point(gpui::px(-3.0), gpui::px(-3.0)),
                                    blur_radius: gpui::px(8.0),
                                    spread_radius: gpui::px(0.0),
                                    inset: true,
                                },
                            ])
                            .child(
                                div()
                                    .w(gpui::px(288.0))
                                    .h(gpui::px(288.0))
                                    .rounded(gpui::px(24.0))
                                    .bg(t.glass_tint)
                                    .border_1()
                                    .border_color(t.glass_border)
                                    .overflow_hidden()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(if let Some(source) = self.cover_art.clone() {
                                        gpui::img(source)
                                            .w(gpui::px(288.0))
                                            .h(gpui::px(288.0))
                                            .rounded(gpui::px(24.0))
                                            .object_fit(gpui::ObjectFit::Cover)
                                            .into_any_element()
                                    } else {
                                        div()
                                            .text_size(gpui::px(48.0))
                                            .text_color(t.text_secondary)
                                            .child("♪")
                                            .into_any_element()
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(gpui::px(2.0))
                            .child(
                                div()
                                    .font_family(theme.display_font)
                                    .text_size(gpui::px(26.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(self.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(gpui::px(13.0))
                                    .text_color(t.text_secondary)
                                    .child(self.artist.clone()),
                            )
                            .child(
                                div()
                                    .text_size(gpui::px(9.0))
                                    .text_color(t.text_tertiary)
                                    .child(self.album.clone()),
                            ),
                    )
                    .child(
                        // Transport controls
                        div()
                            .flex()
                            .items_center()
                            .gap(gpui::px(16.0))
                            .child(
                                div()
                                    .id("np-prev")
                                    .debug_selector(|| "np-prev".to_string())
                                    .aria_label("Previous track")
                                    .cursor_pointer()
                                    .w(gpui::px(26.0))
                                    .h(gpui::px(26.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(t.text_secondary)
                                    .on_click(cx.listener(|this, _event, _window, _cx| {
                                        this.player.tx.send(PlayerCommand::Previous).ok();
                                    }))
                                    .child("⏮"),
                            )
                            .child(
                                div()
                                    .id("np-play")
                                    .debug_selector(|| "np-play".to_string())
                                    .aria_label(if self.playing { "Pause" } else { "Play" })
                                    .cursor_pointer()
                                    .w(gpui::px(46.0))
                                    .h(gpui::px(46.0))
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
                                    .text_size(gpui::px(18.0))
                                    .on_click(cx.listener(|this, _event, _window, _cx| {
                                        this.player.tx.send(PlayerCommand::Toggle).ok();
                                    }))
                                    .child(if self.playing { "⏸" } else { "▶" }),
                            )
                            .child(
                                div()
                                    .id("np-next")
                                    .debug_selector(|| "np-next".to_string())
                                    .aria_label("Next track")
                                    .cursor_pointer()
                                    .w(gpui::px(26.0))
                                    .h(gpui::px(26.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(t.text_secondary)
                                    .on_click(cx.listener(|this, _event, _window, _cx| {
                                        this.player.tx.send(PlayerCommand::Next).ok();
                                    }))
                                    .child("⏭"),
                            ),
                    )
                    .child(
                        // Progress
                        div()
                            .w(gpui::px(300.0))
                            .flex()
                            .flex_col()
                            .gap(gpui::px(4.0))
                            .child(
                                div()
                                    .w_full()
                                    .h(gpui::px(5.0))
                                    .rounded(gpui::px(999.0))
                                    .bg(t.clay_shade)
                                    .child(
                                        div()
                                            .w(gpui::px(progress_pct as f32 * 3.0))
                                            .h_full()
                                            .rounded(gpui::px(999.0))
                                            .bg(t.gold),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_size(gpui::px(8.0))
                                    .text_color(t.text_tertiary)
                                    .child(format_time(self.position))
                                    .child(format_time(self.duration)),
                            ),
                    ),
            )
            .child(
                // Right: lyrics panel
                div()
                    .flex_1()
                    .min_w_0()
                    .rounded(gpui::px(22.0))
                    .bg(t.glass_tint)
                    .border_1()
                    .border_color(t.glass_border)
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0.0, 0.0, 0.0, 0.18),
                        offset: gpui::point(gpui::px(0.0), gpui::px(18.0)),
                        blur_radius: gpui::px(40.0),
                        spread_radius: gpui::px(0.0),
                        inset: false,
                    }])
                    .flex()
                    .flex_col()
                    .child(
                        // header
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px(gpui::px(20.0))
                            .py(gpui::px(16.0))
                            .border_b_1()
                            .border_color(t.glass_border)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(gpui::px(2.0))
                                    .child(
                                        div()
                                            .text_size(gpui::px(9.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(t.gold)
                                            .child("SYNCED LYRICS"),
                                    )
                                    .child(
                                        div()
                                            .text_size(gpui::px(9.0))
                                            .text_color(t.text_tertiary)
                                            .child(format!(
                                                "{} · offset {} ms",
                                                self.source_label, self.offset_ms
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(gpui::px(6.0))
                                    .child(
                                        div()
                                            .id("np-offset-down")
                                            .debug_selector(|| "np-offset-down".to_string())
                                            .cursor_pointer()
                                            .px(gpui::px(9.0))
                                            .py(gpui::px(7.0))
                                            .rounded(gpui::px(9.0))
                                            .bg(t.clay_surface)
                                            .text_color(t.clay_ink)
                                            .text_size(gpui::px(9.0))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .on_click(cx.listener(|_this, _event, _window, cx| {
                                                cx.emit(NowPlayingEvent::OffsetDelta(-500));
                                            }))
                                            .child("-0.5s"),
                                    )
                                    .child(
                                        div()
                                            .id("np-offset-up")
                                            .debug_selector(|| "np-offset-up".to_string())
                                            .cursor_pointer()
                                            .px(gpui::px(9.0))
                                            .py(gpui::px(7.0))
                                            .rounded(gpui::px(9.0))
                                            .bg(t.clay_surface)
                                            .text_color(t.clay_ink)
                                            .text_size(gpui::px(9.0))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .on_click(cx.listener(|_this, _event, _window, cx| {
                                                cx.emit(NowPlayingEvent::OffsetDelta(500));
                                            }))
                                            .child("+0.5s"),
                                    )
                                    .child(
                                        div()
                                            .id("np-pin")
                                            .debug_selector(|| "np-pin".to_string())
                                            .cursor_pointer()
                                            .px(gpui::px(11.0))
                                            .py(gpui::px(7.0))
                                            .rounded(gpui::px(999.0))
                                            .bg(gpui::rgb(0x2A241C))
                                            .text_color(t.gold_light)
                                            .text_size(gpui::px(9.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .on_click(cx.listener(|_this, _event, _window, cx| {
                                                cx.emit(NowPlayingEvent::PinToggle);
                                            }))
                                            .child("Pin to desktop"),
                                    ),
                            ),
                    )
                    .child(
                        // lyrics list
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .px(gpui::px(24.0))
                            .py(gpui::px(16.0))
                            .child(self.lyrics.clone()),
                    )
                    .child(
                        // footer
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px(gpui::px(20.0))
                            .py(gpui::px(12.0))
                            .border_t_1()
                            .border_color(t.glass_border)
                            .text_size(gpui::px(8.0))
                            .text_color(t.text_tertiary)
                            .child(format!("{} — {}", self.title, self.artist))
                            .child(format!(
                                "{} / {}",
                                format_time(self.position),
                                format_time(self.duration)
                            )),
                    ),
            )
    }
}

fn format_time(seconds: f64) -> String {
    let mins = (seconds / 60.0) as u64;
    let secs = (seconds % 60.0) as u64;
    format!("{}:{:02}", mins, secs)
}
