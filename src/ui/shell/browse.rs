//! Source-aware browse screens: YouTube home shelves, album/artist/playlist
//! details, and moods/genres. Navidrome detail pages use the same screens, so a
//! design overhaul can restyle this file without touching providers.
//!
//! Part of the `Shell` impl split out of `shell.rs`; child modules can use the
//! shell's private helpers and state.

use super::*;
use crate::ui::components::{album_card, format_time, track_row};

impl Shell {
    fn cover_element(
        &self,
        url: Option<&str>,
        cover_id: Option<&str>,
        initial: &str,
        size: f32,
        t: &crate::theme::Colors,
        cx: &gpui::App,
    ) -> gpui::AnyElement {
        let key = url
            .map(crate::library::url_cover_key)
            .or_else(|| cover_id.map(str::to_string));
        let image = key.and_then(|key| self.home_state.read(cx).covers.get(&key).cloned());
        // The image is painted into a sized wrapper: a bare `img` does not
        // contribute its height to a flex row at this GPUI revision, which made
        // detail headers collapse and browse cards overlap.
        div()
            .w(gpui::px(size))
            .h(gpui::px(size))
            .flex_shrink_0()
            .rounded(gpui::px(18.0))
            .overflow_hidden()
            .border_1()
            .border_color(t.glass_border)
            .bg(gpui::linear_gradient(
                135.0,
                gpui::linear_color_stop(t.gold_deep, 0.0),
                gpui::linear_color_stop(t.clay_surface, 1.0),
            ))
            .flex()
            .items_center()
            .justify_center()
            .child(match image {
                Some(image) => gpui::img(gpui::ImageSource::Image(image))
                    .w_full()
                    .h_full()
                    .object_fit(gpui::ObjectFit::Cover)
                    .into_any_element(),
                None => div()
                    .font_family(midnight_brass().display_font)
                    .text_size(gpui::px(size * 0.32))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(t.clay_ink)
                    .opacity(0.85)
                    .child(initial.to_string())
                    .into_any_element(),
            })
            .into_any_element()
    }

    pub(super) fn detail_back(&mut self, cx: &mut gpui::Context<Self>) {
        let route = match &self.route {
            Route::Album(source) | Route::Playlist(source) => {
                if source.source == TrackSource::YouTube {
                    Route::Home
                } else {
                    Route::Albums
                }
            }
            Route::Artist(source) => {
                if source.source == TrackSource::YouTube {
                    Route::Home
                } else {
                    Route::Artists
                }
            }
            Route::Moods => Route::Home,
            _ => Route::Home,
        };
        self.selected_album = None;
        self.album_detail = None;
        self.artist_detail = None;
        self.playlist_detail = None;
        self.detail_error = None;
        self.go(route);
        cx.notify();
    }

    pub(super) fn render_detail_route(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        if self.detail_loading {
            let theme = midnight_brass();
            let t = &theme.colors;
            return div()
                .id("detail-loading")
                .debug_selector(|| "detail-loading".to_string())
                .flex()
                .flex_col()
                .gap(gpui::px(12.0))
                .child(
                    div()
                        .text_size(gpui::px(13.0))
                        .text_color(t.text_secondary)
                        .child("Loading…"),
                )
                .child(
                    div()
                        .w_full()
                        .h(gpui::px(180.0))
                        .rounded(gpui::px(18.0))
                        .bg(t.glass_tint)
                        .border_1()
                        .border_color(t.glass_border),
                )
                .into_any_element();
        }
        if let Some(error) = self.detail_error.clone() {
            let theme = midnight_brass();
            let t = &theme.colors;
            let f_back = self.focus_for("detail-back", Self::TAB_CONTENT, cx);
            return div()
                .id("detail-error")
                .debug_selector(|| "detail-error".to_string())
                .role(gpui::Role::Alert)
                .aria_label(error.clone())
                .flex()
                .flex_col()
                .gap(gpui::px(10.0))
                .p(gpui::px(20.0))
                .rounded(gpui::px(16.0))
                .bg(t.glass_tint)
                .border_1()
                .border_color(t.danger)
                .child(
                    div()
                        .text_size(gpui::px(12.0))
                        .text_color(t.text_primary)
                        .child(error),
                )
                .child(
                    div()
                        .id("detail-back")
                        .debug_selector(|| "detail-back".to_string())
                        .track_focus(&f_back)
                        .tab_index(Self::TAB_CONTENT)
                        .role(gpui::Role::Button)
                        .aria_label("Go back")
                        .border_2()
                        .border_color(self.focus_border(&f_back, window, t.glass_border, t))
                        .cursor_pointer()
                        .px(gpui::px(12.0))
                        .py(gpui::px(6.0))
                        .rounded(gpui::px(10.0))
                        .text_size(gpui::px(11.0))
                        .text_color(t.gold_light)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.detail_back(cx);
                        }))
                        .child("Back"),
                )
                .into_any_element();
        }
        match self.route.clone() {
            Route::Album(_) => self.render_album_screen(window, cx),
            Route::Artist(_) => self.render_artist_screen(window, cx),
            Route::Playlist(_) => self.render_playlist_screen(window, cx),
            Route::Moods => self.render_moods_screen(window, cx),
            _ => div().into_any_element(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn detail_header(
        &mut self,
        kind: &str,
        title: &str,
        subtitle: &str,
        meta: &str,
        url: Option<&str>,
        cover_id: Option<&str>,
        source: TrackSource,
        artist_id: Option<String>,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
        t: &crate::theme::Colors,
    ) -> gpui::Div {
        let f_back = self.focus_for("detail-back", Self::TAB_CONTENT, cx);
        let initial = title
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string());
        let cover = self.cover_element(url, cover_id, &initial, 220.0, t, cx);
        let artist = match artist_id {
            Some(id) => div()
                .id("detail-artist")
                .debug_selector(|| "detail-artist".to_string())
                .cursor_pointer()
                .text_size(gpui::px(12.0))
                .text_color(t.gold_light)
                .child(subtitle.to_string())
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    this.open_artist(source, id.clone(), cx);
                }))
                .into_any_element(),
            None => div()
                .text_size(gpui::px(12.0))
                .text_color(t.text_secondary)
                .child(subtitle.to_string())
                .into_any_element(),
        };
        div()
            .flex()
            .items_start()
            .gap(gpui::px(20.0))
            .child(cover)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(gpui::px(8.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(gpui::px(6.0))
                            .text_size(gpui::px(10.0))
                            .text_color(t.text_tertiary)
                            .child(kind.to_uppercase())
                            .child(div().text_color(t.text_tertiary).child("›"))
                            .child(div().text_color(t.text_secondary).child(title.to_string())),
                    )
                    .child(
                        div()
                            .id("detail-back")
                            .debug_selector(|| "detail-back".to_string())
                            .track_focus(&f_back)
                            .tab_index(Self::TAB_CONTENT)
                            .role(gpui::Role::Link)
                            .aria_label("Go back")
                            .border_2()
                            .border_color(self.focus_border(
                                &f_back,
                                window,
                                t.glass_border_soft,
                                t,
                            ))
                            .cursor_pointer()
                            .text_size(gpui::px(10.0))
                            .text_color(t.text_secondary)
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.detail_back(cx);
                            }))
                            .child("← Back"),
                    )
                    .child(
                        div()
                            .text_size(gpui::px(9.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(t.gold)
                            .child(kind.to_uppercase()),
                    )
                    .child(
                        div()
                            .font_family(midnight_brass().display_font)
                            .text_size(gpui::px(30.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(t.text_primary)
                            .line_clamp(2)
                            .child(title.to_string()),
                    )
                    .child(artist)
                    .child(
                        div()
                            .text_size(gpui::px(10.0))
                            .text_color(t.text_tertiary)
                            .child(meta.to_string()),
                    ),
            )
    }

    fn render_album_screen(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let Some(detail) = self.album_detail.clone() else {
            return div()
                .text_size(gpui::px(12.0))
                .text_color(midnight_brass().colors.text_secondary)
                .child("This album is not loaded.")
                .into_any_element();
        };
        let theme = midnight_brass();
        let t = &theme.colors;
        let accent = self.accent_color;
        let source = detail.album.source;
        let songs = detail.songs.clone();
        let current = self.current_track_id(cx);
        let meta = {
            let mut parts = Vec::new();
            if let Some(year) = detail.album.year {
                parts.push(year.to_string());
            }
            parts.push(format!(
                "{} song{}",
                songs.len(),
                if songs.len() == 1 { "" } else { "s" }
            ));
            let seconds: u64 = songs.iter().map(|song| song.duration.as_secs()).sum();
            if seconds > 0 {
                parts.push(format!("{} min", seconds / 60));
            }
            if source == TrackSource::YouTube {
                parts.push("YouTube Music".to_string());
            }
            parts.join(" · ")
        };
        let header = self.detail_header(
            "Album",
            &detail.album.name,
            &detail.album.artist,
            &meta,
            detail.album.cover_url.as_deref(),
            detail.album.cover_art_id.as_deref(),
            source,
            detail.album.artist_id.clone(),
            window,
            cx,
            t,
        );
        let f_play = self.focus_for("detail-play-all", Self::TAB_CONTENT, cx);
        let f_enqueue = self.focus_for("detail-enqueue", Self::TAB_CONTENT, cx);
        let f_star = self.focus_for("detail-star", Self::TAB_CONTENT, cx);
        let songs_play = songs.clone();
        let songs_queue = songs.clone();
        let mut actions = div().flex().items_center().gap(gpui::px(8.0)).child(
            div()
                .id("detail-play-all")
                .debug_selector(|| "detail-play-all".to_string())
                .track_focus(&f_play)
                .tab_index(Self::TAB_CONTENT)
                .role(gpui::Role::Button)
                .aria_label("Play this album")
                .border_2()
                .border_color(self.focus_border(&f_play, window, t.gold_glow_border, t))
                .cursor_pointer()
                .px(gpui::px(16.0))
                .py(gpui::px(8.0))
                .rounded(gpui::px(999.0))
                .bg(accent)
                .text_color(t.clay_ink)
                .text_size(gpui::px(10.0))
                .font_weight(gpui::FontWeight::BOLD)
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    this.play_tracks(songs_play.clone(), 0, cx);
                }))
                .child("Play all"),
        );
        actions = actions.child(
            div()
                .id("detail-enqueue")
                .debug_selector(|| "detail-enqueue".to_string())
                .track_focus(&f_enqueue)
                .tab_index(Self::TAB_CONTENT)
                .role(gpui::Role::Button)
                .aria_label("Add this album to the queue")
                .border_2()
                .border_color(self.focus_border(&f_enqueue, window, t.glass_border, t))
                .cursor_pointer()
                .px(gpui::px(14.0))
                .py(gpui::px(8.0))
                .rounded(gpui::px(999.0))
                .bg(t.glass_highlight_soft)
                .border_1()
                .border_color(t.glass_border)
                .text_color(t.gold_light)
                .text_size(gpui::px(10.0))
                .font_weight(gpui::FontWeight::BOLD)
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    this.enqueue_tracks(songs_queue.clone(), cx);
                }))
                .child("Add to queue"),
        );
        if source == TrackSource::Navidrome {
            let starred = detail.album.starred;
            actions = actions.child(
                div()
                    .id("detail-star")
                    .debug_selector(|| "detail-star".to_string())
                    .track_focus(&f_star)
                    .tab_index(Self::TAB_CONTENT)
                    .role(gpui::Role::Button)
                    .aria_label(if starred {
                        "Remove this album from favorites"
                    } else {
                        "Add this album to favorites"
                    })
                    .border_2()
                    .border_color(self.focus_border(&f_star, window, t.glass_border, t))
                    .cursor_pointer()
                    .px(gpui::px(14.0))
                    .py(gpui::px(8.0))
                    .rounded(gpui::px(999.0))
                    .bg(t.glass_highlight_soft)
                    .border_1()
                    .border_color(t.glass_border)
                    .text_color(t.gold_light)
                    .text_size(gpui::px(10.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.toggle_album_star(cx);
                    }))
                    .child(if starred {
                        "★ Favorited"
                    } else {
                        "☆ Favorite"
                    }),
            );
        }

        let mut list = div().flex().flex_col().gap(gpui::px(3.0));
        for (i, song) in songs.iter().enumerate() {
            let f = self.focus_for(&format!("detail-track-{i}"), Self::TAB_CONTENT, cx);
            let active = current.as_deref() == Some(song.id.as_str());
            let cover = song
                .cover_key()
                .and_then(|key| self.home_state.read(cx).covers.get(&key).cloned());
            let mut row = track_row(
                &format!("{:02}", i + 1),
                &song.title,
                &song.artist,
                &format_time(song.duration.as_secs_f64()),
                active,
                cover,
                t,
            )
            .id(format!("detail-track-{i}"))
            .debug_selector(move || format!("track-{i}"))
            .track_focus(&f)
            .tab_index(Self::TAB_CONTENT)
            .role(gpui::Role::Button)
            .border_2()
            .border_color(self.focus_border(&f, window, t.glass_border_soft, t))
            .cursor_pointer()
            .on_click(cx.listener({
                let songs = songs.clone();
                move |this, _event, _window, cx| {
                    this.play_tracks(songs.clone(), i, cx);
                }
            }));
            let f_next = self.focus_for(&format!("detail-next-{i}"), Self::TAB_CONTENT, cx);
            let f_add = self.focus_for(&format!("detail-add-{i}"), Self::TAB_CONTENT, cx);
            let next_track = song.clone();
            let add_track = song.clone();
            row = row
                .child(
                    div()
                        .id(format!("detail-next-{i}"))
                        .debug_selector(move || format!("detail-next-{i}"))
                        .track_focus(&f_next)
                        .tab_index(Self::TAB_CONTENT)
                        .role(gpui::Role::Button)
                        .aria_label("Play next")
                        .border_2()
                        .border_color(self.focus_border(&f_next, window, gpui::rgba(0x00000000), t))
                        .cursor_pointer()
                        .w(gpui::px(22.0))
                        .h(gpui::px(22.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(gpui::px(10.0))
                        .text_color(t.text_secondary)
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            cx.stop_propagation();
                            this.enqueue_next(next_track.clone(), cx);
                        }))
                        .child("⏭"),
                )
                .child(
                    div()
                        .id(format!("detail-add-{i}"))
                        .debug_selector(move || format!("detail-add-{i}"))
                        .track_focus(&f_add)
                        .tab_index(Self::TAB_CONTENT)
                        .role(gpui::Role::Button)
                        .aria_label("Add to queue")
                        .border_2()
                        .border_color(self.focus_border(&f_add, window, gpui::rgba(0x00000000), t))
                        .cursor_pointer()
                        .w(gpui::px(22.0))
                        .h(gpui::px(22.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(gpui::px(12.0))
                        .text_color(t.text_secondary)
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            cx.stop_propagation();
                            this.enqueue_tracks(vec![add_track.clone()], cx);
                        }))
                        .child("+"),
                );
            list = list.child(row);
        }

        div()
            .flex()
            .flex_col()
            .gap(gpui::px(18.0))
            .child(header)
            .child(actions)
            .child(list)
            .into_any_element()
    }

    fn render_artist_screen(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let Some(detail) = self.artist_detail.clone() else {
            return div()
                .text_size(gpui::px(12.0))
                .text_color(midnight_brass().colors.text_secondary)
                .child("This artist is not loaded.")
                .into_any_element();
        };
        let theme = midnight_brass();
        let t = &theme.colors;
        let source = detail.artist.source;
        let meta = format!(
            "{} album{}",
            detail.albums.len(),
            if detail.albums.len() == 1 { "" } else { "s" }
        );
        let header = self.detail_header(
            "Artist",
            &detail.artist.name,
            "",
            &meta,
            detail.artist.cover_url.as_deref(),
            detail.artist.cover_art_id.as_deref(),
            source,
            None,
            window,
            cx,
            t,
        );
        let mut grid = div()
            .grid()
            .grid_cols(4)
            .gap(gpui::px(14.0))
            .id("detail-artist-grid")
            .role(gpui::Role::Grid)
            .aria_label(format!("Albums by {}", detail.artist.name));
        if detail.albums.is_empty() {
            grid = grid.child(
                div()
                    .col_span_full()
                    .text_size(gpui::px(12.0))
                    .text_color(t.text_secondary)
                    .child("No albums listed for this artist."),
            );
        }
        for (i, album) in detail.albums.iter().enumerate() {
            let album_id = album.id.clone();
            let album_source = album.source;
            let f = self.focus_for(&format!("detail-artist-album-{i}"), Self::TAB_CONTENT, cx);
            let cover = album
                .cover_url
                .as_deref()
                .map(crate::library::url_cover_key)
                .or_else(|| album.cover_art_id.clone())
                .and_then(|key| self.home_state.read(cx).covers.get(&key).cloned());
            let album_selector = album_id.clone();
            grid = grid.child(
                album_card(&album.name, &album.artist, cover, t)
                    .id(format!("detail-artist-album-{i}"))
                    .debug_selector(move || format!("artist-album-{album_selector}"))
                    .track_focus(&f)
                    .tab_index(Self::TAB_CONTENT)
                    .role(gpui::Role::Button)
                    .border_2()
                    .border_color(self.focus_border(&f, window, t.glass_border_soft, t))
                    .aria_label(format!("Open album {}", album.name))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.dispatch(
                            AppAction::OpenAlbum(SourceId::new(album_source, album_id.clone())),
                            cx,
                        );
                    })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(gpui::px(18.0))
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(gpui::px(8.0))
                    .child(
                        div()
                            .font_family(midnight_brass().display_font)
                            .text_size(gpui::px(18.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(t.text_primary)
                            .child("Albums"),
                    )
                    .child(grid),
            )
            .into_any_element()
    }

    fn render_playlist_screen(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let Some(detail) = self.playlist_detail.clone() else {
            return div()
                .text_size(gpui::px(12.0))
                .text_color(midnight_brass().colors.text_secondary)
                .child("This playlist is not loaded.")
                .into_any_element();
        };
        let theme = midnight_brass();
        let t = &theme.colors;
        let accent = self.accent_color;
        let source = detail.playlist.source;
        let songs = detail.songs.clone();
        let current = self.current_track_id(cx);
        let mut meta_parts = vec![format!(
            "{} song{}",
            songs.len(),
            if songs.len() == 1 { "" } else { "s" }
        )];
        if !detail.playlist.owner.is_empty() {
            meta_parts.push(format!("by {}", detail.playlist.owner));
        }
        if source == TrackSource::YouTube {
            meta_parts.push("YouTube Music".to_string());
        }
        let meta = meta_parts.join(" · ");
        let header = self.detail_header(
            "Playlist",
            &detail.playlist.name,
            "",
            &meta,
            detail.playlist.cover_url.as_deref(),
            None,
            source,
            None,
            window,
            cx,
            t,
        );
        let f_play = self.focus_for("playlist-play-all", Self::TAB_CONTENT, cx);
        let f_enqueue = self.focus_for("playlist-enqueue", Self::TAB_CONTENT, cx);
        let songs_play = songs.clone();
        let songs_queue = songs.clone();
        let actions = div()
            .flex()
            .items_center()
            .gap(gpui::px(8.0))
            .child(
                div()
                    .id("playlist-play-all")
                    .debug_selector(|| "playlist-play-all".to_string())
                    .track_focus(&f_play)
                    .tab_index(Self::TAB_CONTENT)
                    .role(gpui::Role::Button)
                    .aria_label("Play this playlist")
                    .border_2()
                    .border_color(self.focus_border(&f_play, window, t.gold_glow_border, t))
                    .cursor_pointer()
                    .px(gpui::px(16.0))
                    .py(gpui::px(8.0))
                    .rounded(gpui::px(999.0))
                    .bg(accent)
                    .text_color(t.clay_ink)
                    .text_size(gpui::px(10.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.play_tracks(songs_play.clone(), 0, cx);
                    }))
                    .child("Play all"),
            )
            .child(
                div()
                    .id("playlist-enqueue")
                    .debug_selector(|| "playlist-enqueue".to_string())
                    .track_focus(&f_enqueue)
                    .tab_index(Self::TAB_CONTENT)
                    .role(gpui::Role::Button)
                    .aria_label("Add this playlist to the queue")
                    .border_2()
                    .border_color(self.focus_border(&f_enqueue, window, t.glass_border, t))
                    .cursor_pointer()
                    .px(gpui::px(14.0))
                    .py(gpui::px(8.0))
                    .rounded(gpui::px(999.0))
                    .bg(t.glass_highlight_soft)
                    .border_1()
                    .border_color(t.glass_border)
                    .text_color(t.gold_light)
                    .text_size(gpui::px(10.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.enqueue_tracks(songs_queue.clone(), cx);
                    }))
                    .child("Add to queue"),
            );
        let mut list = div().flex().flex_col().gap(gpui::px(3.0));
        for (i, song) in songs.iter().enumerate() {
            let f = self.focus_for(&format!("playlist-track-{i}"), Self::TAB_CONTENT, cx);
            let active = current.as_deref() == Some(song.id.as_str());
            let cover = song
                .cover_key()
                .and_then(|key| self.home_state.read(cx).covers.get(&key).cloned());
            let mut row = track_row(
                &format!("{:02}", i + 1),
                &song.title,
                &song.artist,
                &format_time(song.duration.as_secs_f64()),
                active,
                cover,
                t,
            )
            .id(format!("playlist-track-{i}"))
            .debug_selector(move || format!("playlist-track-{i}"))
            .track_focus(&f)
            .tab_index(Self::TAB_CONTENT)
            .role(gpui::Role::Button)
            .border_2()
            .border_color(self.focus_border(&f, window, t.glass_border_soft, t))
            .cursor_pointer()
            .on_click(cx.listener({
                let songs = songs.clone();
                move |this, _event, _window, cx| {
                    this.play_tracks(songs.clone(), i, cx);
                }
            }));
            let f_next = self.focus_for(&format!("playlist-next-{i}"), Self::TAB_CONTENT, cx);
            let f_add = self.focus_for(&format!("playlist-add-{i}"), Self::TAB_CONTENT, cx);
            let next_track = song.clone();
            let add_track = song.clone();
            row = row
                .child(
                    div()
                        .id(format!("playlist-next-{i}"))
                        .debug_selector(move || format!("playlist-next-{i}"))
                        .track_focus(&f_next)
                        .tab_index(Self::TAB_CONTENT)
                        .role(gpui::Role::Button)
                        .aria_label("Play next")
                        .border_2()
                        .border_color(self.focus_border(&f_next, window, gpui::rgba(0x00000000), t))
                        .cursor_pointer()
                        .w(gpui::px(22.0))
                        .h(gpui::px(22.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(gpui::px(10.0))
                        .text_color(t.text_secondary)
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            cx.stop_propagation();
                            this.enqueue_next(next_track.clone(), cx);
                        }))
                        .child("⏭"),
                )
                .child(
                    div()
                        .id(format!("playlist-add-{i}"))
                        .debug_selector(move || format!("playlist-add-{i}"))
                        .track_focus(&f_add)
                        .tab_index(Self::TAB_CONTENT)
                        .role(gpui::Role::Button)
                        .aria_label("Add to queue")
                        .border_2()
                        .border_color(self.focus_border(&f_add, window, gpui::rgba(0x00000000), t))
                        .cursor_pointer()
                        .w(gpui::px(22.0))
                        .h(gpui::px(22.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(gpui::px(12.0))
                        .text_color(t.text_secondary)
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            cx.stop_propagation();
                            this.enqueue_tracks(vec![add_track.clone()], cx);
                        }))
                        .child("+"),
                );
            list = list.child(row);
        }
        div()
            .flex()
            .flex_col()
            .gap(gpui::px(18.0))
            .child(header)
            .child(actions)
            .child(list)
            .into_any_element()
    }

    fn render_moods_screen(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let theme = midnight_brass();
        let t = &theme.colors;
        let Some(categories) = self.mood_categories.clone() else {
            return div()
                .text_size(gpui::px(12.0))
                .text_color(t.text_secondary)
                .child(if self.mood_loading {
                    "Loading moods…"
                } else {
                    "Moods are unavailable."
                })
                .into_any_element();
        };
        let f_back = self.focus_for("detail-back", Self::TAB_CONTENT, cx);
        let mut chips = div().flex().flex_col().gap(gpui::px(12.0));
        let mut mood_index = 0;
        for category in &categories {
            let mut row = div().flex().items_center().gap(gpui::px(6.0)).flex_wrap();
            for mood in &category.moods {
                let idx = mood_index;
                mood_index += 1;
                let f = self.focus_for(&format!("mood-{idx}"), Self::TAB_CONTENT, cx);
                let title = mood.title.clone();
                let params = mood.params.clone();
                row = row.child(
                    div()
                        .id(format!("mood-{idx}"))
                        .debug_selector(move || format!("mood-{idx}"))
                        .track_focus(&f)
                        .tab_index(Self::TAB_CONTENT)
                        .role(gpui::Role::Button)
                        .aria_label(format!("Browse {} playlists", title))
                        .border_2()
                        .border_color(self.focus_border(&f, window, t.glass_border, t))
                        .cursor_pointer()
                        .px(gpui::px(12.0))
                        .py(gpui::px(6.0))
                        .rounded(gpui::px(999.0))
                        .bg(t.glass_tint)
                        .border_1()
                        .border_color(t.glass_border)
                        .text_size(gpui::px(11.0))
                        .text_color(t.text_primary)
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.open_mood(title.clone(), params.clone(), cx);
                        }))
                        .child(mood.title.clone()),
                );
            }
            chips = chips.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(gpui::px(6.0))
                    .child(
                        div()
                            .text_size(gpui::px(9.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(t.gold)
                            .child(category.title.to_uppercase()),
                    )
                    .child(row),
            );
        }
        let mut playlists = div()
            .grid()
            .grid_cols(4)
            .gap(gpui::px(14.0))
            .id("mood-playlists")
            .role(gpui::Role::Grid)
            .aria_label("Mood playlists");
        if self.mood_loading {
            playlists = playlists.child(
                div()
                    .col_span_full()
                    .text_size(gpui::px(12.0))
                    .text_color(t.text_secondary)
                    .child("Loading playlists…"),
            );
        } else if self.mood_playlists.is_empty() {
            playlists = playlists.child(
                div()
                    .col_span_full()
                    .text_size(gpui::px(12.0))
                    .text_color(t.text_secondary)
                    .child("Pick a mood to see playlists."),
            );
        }
        for (i, playlist) in self.mood_playlists.iter().cloned().enumerate() {
            let f = self.focus_for(&format!("mood-playlist-{i}"), Self::TAB_CONTENT, cx);
            let cover = playlist
                .cover_url
                .as_deref()
                .map(crate::library::url_cover_key)
                .and_then(|key| self.home_state.read(cx).covers.get(&key).cloned());
            let id = playlist.id.clone();
            playlists = playlists.child(
                album_card(&playlist.name, &playlist.owner, cover, t)
                    .id(format!("mood-playlist-{i}"))
                    .debug_selector(move || format!("mood-playlist-{i}"))
                    .track_focus(&f)
                    .tab_index(Self::TAB_CONTENT)
                    .role(gpui::Role::Button)
                    .border_2()
                    .border_color(self.focus_border(&f, window, t.glass_border_soft, t))
                    .aria_label(format!("Open playlist {}", playlist.name))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.dispatch(
                            AppAction::OpenPlaylist(SourceId::new(
                                TrackSource::YouTube,
                                id.clone(),
                            )),
                            cx,
                        );
                    })),
            );
        }
        let selected = self.mood_title.clone();
        div()
            .flex()
            .flex_col()
            .gap(gpui::px(18.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(gpui::px(4.0))
                            .child(
                                div()
                                    .font_family(theme.display_font)
                                    .text_size(gpui::px(30.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(t.text_primary)
                                    .child(
                                        selected.unwrap_or_else(|| "Moods & genres".to_string()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(gpui::px(10.0))
                                    .text_color(t.text_tertiary)
                                    .child("Curated by YouTube Music"),
                            ),
                    )
                    .child(
                        div()
                            .id("detail-back")
                            .debug_selector(|| "detail-back".to_string())
                            .track_focus(&f_back)
                            .tab_index(Self::TAB_CONTENT)
                            .role(gpui::Role::Link)
                            .aria_label("Go back")
                            .border_2()
                            .border_color(self.focus_border(
                                &f_back,
                                window,
                                t.glass_border_soft,
                                t,
                            ))
                            .cursor_pointer()
                            .text_size(gpui::px(10.0))
                            .text_color(t.text_secondary)
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.detail_back(cx);
                            }))
                            .child("← Back"),
                    ),
            )
            .child(chips)
            .child(playlists)
            .into_any_element()
    }

    pub(super) fn render_youtube_home(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let theme = midnight_brass();
        let t = &theme.colors;
        let f_retry = self.focus_for("youtube-home-retry", Self::TAB_CONTENT, cx);

        if self.youtube_home_loading && self.youtube_home.is_none() {
            let mut skeleton = div().flex().flex_col().gap(gpui::px(18.0));
            for s in 0..2 {
                let mut row = div().flex().items_center().gap(gpui::px(12.0));
                for i in 0..5 {
                    row = row.child(
                        div()
                            .id(format!("yt-skeleton-{s}-{i}"))
                            .w(gpui::px(150.0))
                            .h(gpui::px(190.0))
                            .rounded(gpui::px(16.0))
                            .bg(t.glass_tint)
                            .border_1()
                            .border_color(t.glass_border),
                    );
                }
                skeleton = skeleton.child(row);
            }
            return skeleton.into_any_element();
        }
        if let Some(error) = self.youtube_home_error.clone() {
            return div()
                .id("youtube-home-error")
                .debug_selector(|| "youtube-home-error".to_string())
                .role(gpui::Role::Alert)
                .aria_label(error.clone())
                .flex()
                .flex_col()
                .gap(gpui::px(10.0))
                .p(gpui::px(20.0))
                .rounded(gpui::px(16.0))
                .bg(t.glass_tint)
                .border_1()
                .border_color(t.danger)
                .child(
                    div()
                        .text_size(gpui::px(12.0))
                        .text_color(t.text_primary)
                        .child(error),
                )
                .child(
                    div()
                        .id("youtube-home-retry")
                        .debug_selector(|| "youtube-home-retry".to_string())
                        .track_focus(&f_retry)
                        .tab_index(Self::TAB_CONTENT)
                        .role(gpui::Role::Button)
                        .aria_label("Try loading YouTube home again")
                        .border_2()
                        .border_color(self.focus_border(&f_retry, window, t.glass_border, t))
                        .cursor_pointer()
                        .px(gpui::px(12.0))
                        .py(gpui::px(6.0))
                        .rounded(gpui::px(10.0))
                        .text_size(gpui::px(11.0))
                        .text_color(t.gold_light)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.refresh_youtube_home(cx);
                        }))
                        .child("Retry"),
                )
                .into_any_element();
        }
        let Some(sections) = self.youtube_home.clone() else {
            return div()
                .flex()
                .flex_col()
                .gap(gpui::px(12.0))
                .child(
                    div()
                        .text_size(gpui::px(12.0))
                        .text_color(t.text_secondary)
                        .child("YouTube Music returned no shelves."),
                )
                .into_any_element();
        };
        let mut body = div().flex().flex_col().gap(gpui::px(22.0));
        if sections.is_empty() {
            body = body.child(
                div()
                    .text_size(gpui::px(12.0))
                    .text_color(t.text_secondary)
                    .child("YouTube Music returned no shelves."),
            );
        }
        for (s, section) in sections.iter().enumerate() {
            let mut row = div()
                .id(format!("yt-shelf-{s}"))
                .flex()
                .items_start()
                .gap(gpui::px(12.0))
                .overflow_x_scroll()
                .pb(gpui::px(4.0));
            for (i, card) in section.items.iter().cloned().enumerate() {
                row = row.child(self.browse_card(s, i, card, window, cx, t));
            }
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(gpui::px(9.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .font_family(theme.display_font)
                                    .text_size(gpui::px(18.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(t.text_primary)
                                    .child(if section.title.is_empty() {
                                        "For you".to_string()
                                    } else {
                                        section.title.clone()
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(gpui::px(10.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(t.text_tertiary)
                                    .child("See all ›"),
                            ),
                    )
                    .child(
                        div().relative().child(row).child(
                            div()
                                .absolute()
                                .right(gpui::px(0.0))
                                .top(gpui::px(0.0))
                                .bottom(gpui::px(0.0))
                                .w(gpui::px(40.0))
                                .bg(gpui::linear_gradient(
                                    90.0,
                                    gpui::linear_color_stop(gpui::rgba(0x0A0B1000), 0.0),
                                    gpui::linear_color_stop(gpui::rgba(0x0A0B10CC), 1.0),
                                )),
                        ),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(gpui::px(16.0))
            .child(
                div()
                    .text_size(gpui::px(10.0))
                    .text_color(t.text_tertiary)
                    .child("YouTube Music"),
            )
            .child(body)
            .into_any_element()
    }

    fn browse_card(
        &mut self,
        section: usize,
        index: usize,
        card: crate::provider::youtube::BrowseCard,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
        t: &crate::theme::Colors,
    ) -> gpui::AnyElement {
        use crate::provider::youtube::BrowseCard;
        let key = format!("yt-card-{section}-{index}");
        let f = self.focus_for(&key, Self::TAB_CONTENT, cx);
        let title = match &card {
            BrowseCard::Song(track) => track.title.clone(),
            BrowseCard::Album(album) => album.name.clone(),
            BrowseCard::Playlist(playlist) => playlist.name.clone(),
            BrowseCard::Artist(artist) => artist.name.clone(),
        };
        let subtitle = match &card {
            BrowseCard::Song(track) => track.artist.clone(),
            BrowseCard::Album(album) => album.artist.clone(),
            BrowseCard::Playlist(playlist) => {
                if !playlist.owner.is_empty() {
                    playlist.owner.clone()
                } else if playlist.song_count > 0 {
                    format!("{} tracks", playlist.song_count)
                } else {
                    "Playlist".to_string()
                }
            }
            BrowseCard::Artist(_) => "Artist".to_string(),
        };
        let kind = match &card {
            BrowseCard::Song(_) => "Play song",
            BrowseCard::Album(_) => "Open album",
            BrowseCard::Playlist(_) => "Open playlist",
            BrowseCard::Artist(_) => "Open artist",
        };
        let cover_url = card.cover_url().map(str::to_string);
        let initial = title
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string());
        let cover = self.cover_element(cover_url.as_deref(), None, &initial, 150.0, t, cx);
        let click_card = card.clone();
        div()
            .id(key.clone())
            .debug_selector({
                let key = key.clone();
                move || key.clone()
            })
            .track_focus(&f)
            .tab_index(Self::TAB_CONTENT)
            .role(gpui::Role::Button)
            .aria_label(format!("{kind}: {title}"))
            .border_2()
            .border_color(self.focus_border(&f, window, gpui::rgba(0x00000000), t))
            .cursor_pointer()
            .w(gpui::px(150.0))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(gpui::px(6.0))
            .rounded(gpui::px(16.0))
            .p(gpui::px(2.0))
            .hover(|style| style.bg(t.glass_highlight_soft))
            .on_click(
                cx.listener(move |this, _event, _window, cx| match &click_card {
                    BrowseCard::Song(track) => this.play_youtube_result(track.clone(), cx),
                    BrowseCard::Album(album) => {
                        this.open_album(TrackSource::YouTube, album.id.clone(), cx)
                    }
                    BrowseCard::Playlist(playlist) => {
                        this.open_playlist(TrackSource::YouTube, playlist.id.clone(), cx)
                    }
                    BrowseCard::Artist(artist) => {
                        this.open_artist(TrackSource::YouTube, artist.id.clone(), cx)
                    }
                }),
            )
            .child(cover)
            .child(
                div()
                    .text_size(gpui::px(11.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(t.text_primary)
                    .line_clamp(2)
                    .child(title),
            )
            .child(
                div()
                    .text_size(gpui::px(9.0))
                    .text_color(t.text_secondary)
                    .line_clamp(1)
                    .text_ellipsis()
                    .child(subtitle),
            )
            .with_animation(
                format!("card-{}", key),
                gpui::Animation::new(std::time::Duration::from_millis(240))
                    .with_easing(gpui::ease_out_quint()),
                |el, delta| el.opacity(delta),
            )
            .into_any_element()
    }
}
