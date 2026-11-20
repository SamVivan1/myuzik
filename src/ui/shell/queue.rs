//! Queue drawer. The queue model lives in `player::queue`; this file only
//! renders it and routes intents through `AppAction`/`QueueAction`.

use super::*;
use crate::ui::components::format_time;

impl Shell {
    pub(super) fn render_queue_panel(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        if !self.queue_panel_open {
            return div().into_any_element();
        }
        let theme = midnight_brass();
        let t = &theme.colors;
        let (queue, current) = {
            let ps = self.player_state.read(cx);
            (ps.queue.tracks(), ps.queue.current_index())
        };
        let f_close = self.focus_for("queue-close", Self::TAB_CONTENT, cx);
        let f_clear = self.focus_for("queue-clear-upcoming", Self::TAB_CONTENT, cx);

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(gpui::px(2.0))
                    .child(
                        div()
                            .font_family(theme.display_font)
                            .text_size(gpui::px(18.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(t.clay_ink)
                            .child("Queue"),
                    )
                    .child(
                        div()
                            .text_size(gpui::px(9.0))
                            .text_color(t.clay_ink)
                            .opacity(0.65)
                            .child(format!(
                                "{} tracks · {} left",
                                queue.len(),
                                queue.len().saturating_sub(current.map_or(0, |i| i + 1))
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(gpui::px(4.0))
                    .child(
                        div()
                            .id("queue-clear-upcoming")
                            .track_focus(&f_clear)
                            .tab_index(Self::TAB_CONTENT)
                            .role(gpui::Role::Button)
                            .border_2()
                            .border_color(self.focus_border(
                                &f_clear,
                                window,
                                gpui::rgba(0x00000000),
                                t,
                            ))
                            .aria_label("Clear upcoming tracks")
                            .cursor_pointer()
                            .px(gpui::px(7.0))
                            .py(gpui::px(4.0))
                            .rounded(gpui::px(7.0))
                            .text_size(gpui::px(9.0))
                            .text_color(t.clay_ink)
                            .opacity(0.7)
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.dispatch(AppAction::Queue(QueueAction::ClearUpcoming), cx);
                            }))
                            .child("Clear upcoming"),
                    )
                    .child(
                        div()
                            .id("queue-close")
                            .track_focus(&f_close)
                            .tab_index(Self::TAB_CONTENT)
                            .role(gpui::Role::Button)
                            .border_2()
                            .border_color(self.focus_border(
                                &f_close,
                                window,
                                gpui::rgba(0x00000000),
                                t,
                            ))
                            .aria_label("Close queue")
                            .cursor_pointer()
                            .px(gpui::px(7.0))
                            .py(gpui::px(4.0))
                            .rounded(gpui::px(7.0))
                            .text_size(gpui::px(12.0))
                            .text_color(t.clay_ink)
                            .opacity(0.7)
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.dispatch(AppAction::Queue(QueueAction::TogglePanel), cx);
                            }))
                            .child("×"),
                    ),
            );

        let mut list = div()
            .id("queue-list")
            .flex()
            .flex_1()
            .min_h(gpui::px(0.0))
            .flex_col()
            .gap(gpui::px(3.0))
            .overflow_y_scroll();
        if queue.is_empty() {
            list = list.child(
                div()
                    .py(gpui::px(18.0))
                    .text_size(gpui::px(11.0))
                    .text_color(t.clay_ink)
                    .opacity(0.7)
                    .child("Nothing in the queue yet."),
            );
        }
        for (i, track) in queue.iter().enumerate() {
            let is_current = current == Some(i);
            let f_play = self.focus_for(&format!("queue-play-{i}"), Self::TAB_CONTENT, cx);
            let f_up = self.focus_for(&format!("queue-up-{i}"), Self::TAB_CONTENT, cx);
            let f_down = self.focus_for(&format!("queue-down-{i}"), Self::TAB_CONTENT, cx);
            let f_remove = self.focus_for(&format!("queue-remove-{i}"), Self::TAB_CONTENT, cx);
            let can_remove = !is_current;
            let can_up = i > 0;
            let can_down = i + 1 < queue.len();
            list = list.child(
                div()
                    .id(format!("queue-row-{i}"))
                    .flex()
                    .items_center()
                    .gap(gpui::px(5.0))
                    .px(gpui::px(6.0))
                    .py(gpui::px(5.0))
                    .rounded(gpui::px(8.0))
                    .bg(if is_current {
                        t.glass_highlight_soft
                    } else {
                        gpui::rgba(0x00000000)
                    })
                    .border_1()
                    .border_color(if is_current {
                        t.gold_tint_border
                    } else {
                        gpui::rgba(0x00000000)
                    })
                    .child(
                        div()
                            .id(format!("queue-play-{i}"))
                            .debug_selector(move || format!("queue-play-{}", i))
                            .track_focus(&f_play)
                            .tab_index(Self::TAB_CONTENT)
                            .role(gpui::Role::Button)
                            .border_2()
                            .border_color(self.focus_border(
                                &f_play,
                                window,
                                gpui::rgba(0x00000000),
                                t,
                            ))
                            .aria_label(format!("Play queue item {}", track.title))
                            .cursor_pointer()
                            .w(gpui::px(22.0))
                            .text_size(gpui::px(9.0))
                            .text_color(t.clay_ink)
                            .opacity(if is_current { 1.0 } else { 0.6 })
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.dispatch(AppAction::Queue(QueueAction::PlayAt(i)), cx);
                            }))
                            .child(if is_current {
                                "▶".to_string()
                            } else {
                                format!("{:02}", i + 1)
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(gpui::px(1.0))
                            .child(
                                div()
                                    .text_size(gpui::px(10.0))
                                    .font_weight(if is_current {
                                        gpui::FontWeight::BOLD
                                    } else {
                                        gpui::FontWeight::NORMAL
                                    })
                                    .text_color(t.clay_ink)
                                    .line_clamp(1)
                                    .text_ellipsis()
                                    .child(track.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(gpui::px(8.0))
                                    .text_color(t.clay_ink)
                                    .opacity(0.65)
                                    .line_clamp(1)
                                    .text_ellipsis()
                                    .child(format!(
                                        "{} · {}",
                                        track.artist,
                                        format_time(track.duration.as_secs_f64())
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .id(format!("queue-up-{i}"))
                            .debug_selector(move || format!("queue-up-{}", i))
                            .track_focus(&f_up)
                            .tab_index(Self::TAB_CONTENT)
                            .role(gpui::Role::Button)
                            .border_2()
                            .border_color(self.focus_border(
                                &f_up,
                                window,
                                gpui::rgba(0x00000000),
                                t,
                            ))
                            .aria_label("Move queue item up")
                            .cursor_pointer()
                            .px(gpui::px(3.0))
                            .text_size(gpui::px(10.0))
                            .text_color(t.clay_ink)
                            .opacity(if can_up { 0.7 } else { 0.2 })
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                if can_up {
                                    this.dispatch(
                                        AppAction::Queue(QueueAction::Move { from: i, to: i - 1 }),
                                        cx,
                                    );
                                }
                            }))
                            .child("↑"),
                    )
                    .child(
                        div()
                            .id(format!("queue-down-{i}"))
                            .debug_selector(move || format!("queue-down-{}", i))
                            .track_focus(&f_down)
                            .tab_index(Self::TAB_CONTENT)
                            .role(gpui::Role::Button)
                            .border_2()
                            .border_color(self.focus_border(
                                &f_down,
                                window,
                                gpui::rgba(0x00000000),
                                t,
                            ))
                            .aria_label("Move queue item down")
                            .cursor_pointer()
                            .px(gpui::px(3.0))
                            .text_size(gpui::px(10.0))
                            .text_color(t.clay_ink)
                            .opacity(if can_down { 0.7 } else { 0.2 })
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                if can_down {
                                    this.dispatch(
                                        AppAction::Queue(QueueAction::Move { from: i, to: i + 1 }),
                                        cx,
                                    );
                                }
                            }))
                            .child("↓"),
                    )
                    .child(
                        div()
                            .id(format!("queue-remove-{i}"))
                            .debug_selector(move || format!("queue-remove-{}", i))
                            .track_focus(&f_remove)
                            .tab_index(Self::TAB_CONTENT)
                            .role(gpui::Role::Button)
                            .border_2()
                            .border_color(self.focus_border(
                                &f_remove,
                                window,
                                gpui::rgba(0x00000000),
                                t,
                            ))
                            .aria_label("Remove queue item")
                            .cursor_pointer()
                            .px(gpui::px(3.0))
                            .text_size(gpui::px(10.0))
                            .text_color(t.danger)
                            .opacity(if can_remove { 0.75 } else { 0.2 })
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                if can_remove {
                                    this.dispatch(AppAction::Queue(QueueAction::Remove(i)), cx);
                                }
                            }))
                            .child("×"),
                    ),
            );
        }

        div()
            .absolute()
            .right(gpui::px(24.0))
            .bottom(gpui::px(116.0))
            .w(gpui::px(390.0))
            .h(gpui::px(394.0))
            .rounded(gpui::px(20.0))
            .bg(t.clay_surface)
            .border_1()
            .border_color(t.glass_border)
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0.0, 0.0, 0.0, 0.45),
                offset: gpui::point(gpui::px(0.0), gpui::px(12.0)),
                blur_radius: gpui::px(30.0),
                spread_radius: gpui::px(0.0),
                inset: false,
            }])
            .flex()
            .flex_col()
            .gap(gpui::px(8.0))
            .p(gpui::px(12.0))
            .child(header)
            .child(list)
            .into_any_element()
    }
}
