use crate::overlay::PinnedLyricsView;
use crate::theme::midnight_brass;
use gpui::{Context, Render, Window, WindowOptions, div, prelude::*};

pub struct Shell {
    overlay: Option<gpui::WindowHandle<PinnedLyricsView>>,
}

impl Shell {
    pub fn new() -> Self {
        Self { overlay: None }
    }

    pub fn open_overlay(&mut self, cx: &mut gpui::App) {
        if self.overlay.is_none() {
            let mut opts = WindowOptions::default();
            opts.window_background = gpui::WindowBackgroundAppearance::Blurred;
            opts.window_decorations = None;
            opts.focus = false;
            opts.show = true;
            opts.app_id = Some(crate::overlay::APP_ID.to_string());
            opts.titlebar = None;
            let handle = cx
                .open_window(opts, |_window, cx| {
                    cx.new(|_cx| PinnedLyricsView::new())
                })
                .expect("failed to open overlay");
            // Set title after creation
            handle
                .update(cx, |_view, window, _cx| {
                    window.set_window_title(crate::overlay::OVERLAY_TITLE);
                })
                .ok();
            self.overlay = Some(handle);
        }
    }

    pub fn close_overlay(&mut self, cx: &mut gpui::App) {
        if let Some(handle) = self.overlay.take() {
            handle.update(cx, |_view, window, _cx| {
                window.remove_window();
            }).ok();
        }
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = midnight_brass();
        let t = &theme.colors;

        div()
            .size_full()
            .bg(t.bg_base)
            .text_color(t.text_primary)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .size_full()
                    .gap_4()
                    .child(
                        div()
                            .font_family(theme.display_font)
                            .text_size(gpui::px(28.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Myuzik"),
                    )
                    .child(
                        div()
                            .text_size(gpui::px(12.0))
                            .text_color(t.text_secondary)
                            .child("Midnight Brass — walking skeleton"),
                    )
                    .child(
                        div()
                            .mt(gpui::px(16.0))
                            .flex()
                            .gap(gpui::px(12.0))
                            .child(
                                div()
                                    .px(gpui::px(16.0))
                                    .py(gpui::px(10.0))
                                    .rounded(gpui::px(theme.radii.button))
                                    .bg(t.clay_surface)
                                    .text_color(t.clay_ink)
                                    .font_family(theme.ui_font)
                                    .text_size(gpui::px(12.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .id("pin-btn")
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.open_overlay(cx);
                                    }))
                                    .child("Pin lyrics"),
                            )
                            .child(
                                div()
                                    .px(gpui::px(16.0))
                                    .py(gpui::px(10.0))
                                    .rounded(gpui::px(theme.radii.button))
                                    .bg(gpui::rgba(0x2A241CFF))
                                    .text_color(t.gold_light)
                                    .font_family(theme.ui_font)
                                    .text_size(gpui::px(12.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .id("unpin-btn")
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.close_overlay(cx);
                                    }))
                                    .child("Unpin"),
                            ),
                    ),
            )
    }
}
