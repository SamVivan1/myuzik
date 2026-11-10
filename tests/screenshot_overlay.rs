//! Screenshots of the pinned lyrics overlay, rendered headlessly and composited
//! over a desktop-like backdrop so the glass panel reads the way it does on screen.
//!
//! Run with:
//!   cargo test --test screenshot_overlay -- --ignored --test-threads=1 --nocapture
//!
//! Writes `docs/screenshots/overlay.png`, `overlay-plain.png` and `overlay-empty.png`.
//! It needs GPUI's headless wgpu renderer (no display, no server, no credentials).

use gpui::{AnyWindowHandle, AppContext, HeadlessAppContext};
use gpui_wgpu::CosmicTextSystem;
use image::ImageEncoder;
use myuzik::lyrics::model::{LyricLine, LyricsStatus};
use myuzik::overlay::{OverlayGeometry, OverlayState};
use std::sync::Arc;
use std::time::Duration;

const W: u32 = myuzik::overlay::DEFAULT_WIDTH;
const H: u32 = myuzik::overlay::DEFAULT_HEIGHT;
/// Backdrop margin around the panel.
const PAD: u32 = 44;

/// A believable album cover, so the artwork corner can be judged.
fn cover_png() -> Vec<u8> {
    let size = 320u32;
    let mut img = image::RgbaImage::new(size, size);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let fx = x as f32 / size as f32;
        let fy = y as f32 / size as f32;
        // Warm smoky gradient with a soft light behind the middle.
        let glow = (1.0 - ((fx - 0.5).powi(2) + (fy - 0.45).powi(2)).sqrt() * 1.8).max(0.0);
        let r = (26.0 + 150.0 * glow + 40.0 * fy) as u8;
        let g = (20.0 + 120.0 * glow + 30.0 * fy) as u8;
        let b = (24.0 + 70.0 * glow + 20.0 * fx) as u8;
        *px = image::Rgba([r, g, b, 255]);
    }
    // Two soft light pools, so the cover reads as artwork and not a test pattern.
    for (x, y, px) in img.enumerate_pixels_mut() {
        let fx = x as f32 / size as f32;
        let fy = y as f32 / size as f32;
        let pool = (1.0 - ((fx - 0.34).powi(2) + (fy - 0.62).powi(2)).sqrt() * 2.6).max(0.0);
        let pool2 = (1.0 - ((fx - 0.72).powi(2) + (fy - 0.24).powi(2)).sqrt() * 3.0).max(0.0);
        let base = px.0;
        *px = image::Rgba([
            (base[0] as f32 + 90.0 * pool2 + 20.0 * pool).min(255.0) as u8,
            (base[1] as f32 + 70.0 * pool2 + 40.0 * pool).min(255.0) as u8,
            (base[2] as f32 + 40.0 * pool2 + 30.0 * pool).min(255.0) as u8,
            255,
        ]);
    }
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&img, size, size, image::ExtendedColorType::Rgba8)
        .expect("encode cover");
    out
}

fn image_from_png(bytes: Vec<u8>, id: u64) -> Arc<gpui::Image> {
    Arc::new(gpui::Image {
        format: gpui::ImageFormat::Png,
        bytes,
        id,
    })
}

/// The lyric lines used for the screenshots: the first six mirror the approved
/// reference so the result can be compared against it directly.
fn synced_lines() -> Vec<LyricLine> {
    const SEED: [&str; 6] = [
        "bring another day, my bloody hoes",
        "Sleepin', you're on your tippy-toes",
        "Creepin' around like no one knows",
        "Think you're so criminal",
        "Bruises on both my knees for you",
        "Don't say thank you or please",
    ];
    (0..40)
        .map(|i| LyricLine {
            time: Duration::from_secs(i as u64 * 20),
            text: match i {
                0..=5 => SEED[i as usize].to_string(),
                _ => format!("another line of the song, number {i}"),
            },
        })
        .collect()
}

/// Long lines that must wrap: the case where a fixed row height used to make
/// neighbouring lines overlap.
fn wrapping_lines() -> Vec<LyricLine> {
    const TEXT: [&str; 7] = [
        "bring another day, my bloody hoes and all the things I never said out loud",
        "Sleepin', you're on your tippy-toes and I am not the one to tell you so",
        "Creepin' around like no one knows exactly what you did behind the door",
        "Think you're so criminal",
        "Bruises on both my knees for you and every single promise that you broke",
        "Don't say thank you or please, I do what I want when I'm wanting to",
        "So cynical",
    ];
    (0..40)
        .map(|i| LyricLine {
            time: Duration::from_secs(i as u64 * 20),
            text: match i {
                0..=6 => TEXT[i as usize].to_string(),
                _ => format!("another line of the song, number {i}"),
            },
        })
        .collect()
}

fn backdrop(w: u32, h: u32, light: bool) -> image::RgbaImage {
    let (w, h) = (w + PAD * 2, h + PAD * 2);
    let mut img = image::RgbaImage::new(w, h);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let fx = x as f32 / w as f32;
        let fy = y as f32 / h as f32;
        *px = if light {
            // A bright desktop: the dark panel must still read as glass here.
            image::Rgba([
                (196.0 + 40.0 * fy) as u8,
                (206.0 + 34.0 * fx) as u8,
                (226.0 + 24.0 * (1.0 - fy)) as u8,
                255,
            ])
        } else {
            // A cool wallpaper, like the reference shot.
            image::Rgba([
                (18.0 + 40.0 * fy + 10.0 * fx) as u8,
                (26.0 + 55.0 * fy + 20.0 * fx) as u8,
                (58.0 + 90.0 * (1.0 - fy) + 30.0 * fx) as u8,
                255,
            ])
        };
    }
    img
}

fn capture(name: &str, cx: &mut HeadlessAppContext, window: AnyWindowHandle, light: bool) {
    cx.update(|app| app.refresh_windows());
    cx.run_until_parked();
    let _ = cx.update_window(window, |_view, window, cx| {
        let _ = window.draw(cx);
    });
    match cx.capture_screenshot(window) {
        Ok(shot) => {
            // Report the real alpha of the window's corner, before compositing:
            // this is what decides whether the frame looks rounded on the desktop.
            let corner = shot.get_pixel(1, 1);
            eprintln!(
                "raw corner pixel of {name}: rgba({}, {}, {}, {})",
                corner.0[0], corner.0[1], corner.0[2], corner.0[3]
            );
            // The headless renderer runs at 2x, so size the backdrop from the real
            // capture instead of the logical window size.
            let mut canvas = backdrop(shot.width(), shot.height(), light);
            image::imageops::overlay(&mut canvas, &shot, PAD as i64, PAD as i64);
            let path = format!("docs/screenshots/{name}.png");
            canvas.save(&path).expect("save");
            eprintln!(
                "wrote {path} ({}x{}, panel {}x{})",
                canvas.width(),
                canvas.height(),
                shot.width(),
                shot.height()
            );
        }
        Err(e) => eprintln!("capture {name} failed: {e:?}"),
    }
}

#[test]
#[ignore = "needs GPUI's headless wgpu renderer; run with --ignored"]
fn capture_overlay_states() {
    let text_system = Arc::new(CosmicTextSystem::new("sans-serif"));
    let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(()), || {
        gpui_platform::current_headless_renderer()
    });
    // The real app loads these in main.rs; without them the screenshots would
    // judge the design in the wrong typeface.
    cx.update(|app| {
        let fonts: Vec<std::borrow::Cow<'static, [u8]>> = vec![
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Fraunces.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter.ttf")),
        ];
        if let Err(e) = app.text_system().add_fonts(fonts) {
            eprintln!("font load failed: {e:?}");
        }
        app.set_reduce_motion(true);
    });

    let state = cx.update(|app| app.new(|_cx| OverlayState::default()));
    let cover = image_from_png(cover_png(), 0x5eed);

    // ---- 1. synced lyrics, mid-song ----
    {
        let st = state.clone();
        let cover = cover.clone();
        cx.update(move |app| {
            st.update(app, |s, _| {
                s.begin("Bad Guy".to_string(), "Billie Eilish".to_string());
                s.set_status(&LyricsStatus::Synced(synced_lines()));
                s.set_cover(Some(cover.clone()));
                // 20 s per line, so 60 s puts line 3 ("Think you're so criminal") active.
                s.sync_position(Duration::from_secs(60), 0, Some(Duration::from_secs(214)));
            });
        });
        let geometry = Arc::new(std::sync::Mutex::new(OverlayGeometry::default()));
        let handle = cx.update(|app| {
            let bounds = gpui::Bounds::new(
                gpui::point(gpui::px(0.0), gpui::px(0.0)),
                gpui::size(gpui::px(W as f32), gpui::px(H as f32)),
            );
            myuzik::overlay::open_overlay(app, state.clone(), geometry, bounds)
        });
        let any: AnyWindowHandle = handle.into();
        capture("overlay", &mut cx, any, false);
        // Same frame on a bright desktop, to prove the glass works either way.
        capture("overlay-light", &mut cx, any, true);
    }

    // ---- 1b. the smallest window the clamp allows (footer must still fit) ----
    {
        let st = state.clone();
        let cover = cover.clone();
        cx.update(move |app| {
            st.update(app, |s, _| {
                s.begin("Bad Guy".to_string(), "Billie Eilish".to_string());
                s.set_status(&LyricsStatus::Synced(wrapping_lines()));
                s.set_cover(Some(cover.clone()));
                s.sync_position(Duration::from_secs(60), 0, Some(Duration::from_secs(214)));
            });
        });
        cx.update(|app| app.refresh_windows());
        let any = cx.update(|app| {
            app.windows()
                .iter()
                .find(|w| w.downcast::<myuzik::overlay::PinnedLyricsView>().is_some())
                .copied()
        });
        if let Some(any) = any {
            let _ = cx.update_window(any, |_view, window, _cx| {
                window.resize(gpui::size(
                    gpui::px(myuzik::overlay::MIN_WIDTH as f32),
                    gpui::px(myuzik::overlay::MIN_HEIGHT as f32),
                ));
            });
            capture("overlay-min", &mut cx, any, false);
            // Back to the default size for the remaining captures.
            let _ = cx.update_window(any, |_view, window, _cx| {
                window.resize(gpui::size(gpui::px(W as f32), gpui::px(H as f32)));
            });
        }
    }

    // ---- 1c. position at zero: the track must still be drawn ----
    {
        let st = state.clone();
        let cover = cover.clone();
        cx.update(move |app| {
            st.update(app, |s, _| {
                s.begin("Tarot".to_string(), ".Feast".to_string());
                s.set_status(&LyricsStatus::Synced(synced_lines()));
                s.set_cover(Some(cover.clone()));
                s.sync_position(Duration::ZERO, 0, Some(Duration::from_secs(306)));
            });
        });
        cx.update(|app| app.refresh_windows());
        let any = cx.update(|app| {
            app.windows()
                .iter()
                .find(|w| w.downcast::<myuzik::overlay::PinnedLyricsView>().is_some())
                .copied()
        });
        if let Some(any) = any {
            capture("overlay-zero", &mut cx, any, false);
        }
    }

    // ---- 1d. the reported case: minimum window, long title, no cover ----
    {
        let st = state.clone();
        cx.update(move |app| {
            st.update(app, |s, _| {
                s.begin("Kenangan Yang Hilang".to_string(), "Aryiion".to_string());
                s.set_status(&LyricsStatus::NotFound);
                s.sync_position(Duration::from_secs(28), 0, Some(Duration::from_secs(193)));
            });
        });
        cx.update(|app| app.refresh_windows());
        let any = cx.update(|app| {
            app.windows()
                .iter()
                .find(|w| w.downcast::<myuzik::overlay::PinnedLyricsView>().is_some())
                .copied()
        });
        if let Some(any) = any {
            let _ = cx.update_window(any, |_view, window, _cx| {
                window.resize(gpui::size(
                    gpui::px(myuzik::overlay::MIN_WIDTH as f32),
                    gpui::px(myuzik::overlay::MIN_HEIGHT as f32),
                ));
            });
            capture("overlay-title-wrap", &mut cx, any, false);
            let _ = cx.update_window(any, |_view, window, _cx| {
                window.resize(gpui::size(gpui::px(W as f32), gpui::px(H as f32)));
            });
        }
    }

    // ---- 2. unsynced transcript ----
    {
        let st = state.clone();
        let cover = cover.clone();
        cx.update(move |app| {
            st.update(app, |s, _| {
                s.begin("Golden".to_string(), "AUDREY NUNA".to_string());
                s.set_status(&LyricsStatus::Plain(
                    "I'm a golden child\nnever gonna fall\nwatch me while I glow\n\
                     keep the fire low\nrun until the morning\nnothing left to show"
                        .to_string(),
                ));
                s.set_cover(Some(cover.clone()));
                s.sync_position(Duration::from_secs(80), 0, Some(Duration::from_secs(194)));
            });
        });
        cx.update(|app| app.refresh_windows());
        let any = cx.update(|app| {
            app.windows()
                .iter()
                .find(|w| w.downcast::<myuzik::overlay::PinnedLyricsView>().is_some())
                .copied()
        });
        if let Some(any) = any {
            capture("overlay-plain", &mut cx, any, false);
        }
    }

    // ---- 3. no lyrics ----
    {
        let st = state.clone();
        let cover = cover.clone();
        cx.update(move |app| {
            st.update(app, |s, _| {
                s.begin("Soda Pop".to_string(), "Andrew Choi".to_string());
                s.set_status(&LyricsStatus::NotFound);
                s.set_cover(Some(cover.clone()));
                s.sync_position(Duration::from_secs(40), 0, Some(Duration::from_secs(170)));
            });
        });
        cx.update(|app| app.refresh_windows());
        let any = cx.update(|app| {
            app.windows()
                .iter()
                .find(|w| w.downcast::<myuzik::overlay::PinnedLyricsView>().is_some())
                .copied()
        });
        if let Some(any) = any {
            capture("overlay-empty", &mut cx, any, false);
        }
    }
}

/// The overlay must repaint when the song moves while the window has no focus and
/// receives no input. Renders the same window twice with a position jump in
/// between and requires the pixels to change.
#[test]
#[ignore = "needs GPUI's headless wgpu renderer; run with --ignored"]
fn the_overlay_repaints_without_any_input() {
    let text_system = Arc::new(CosmicTextSystem::new("sans-serif"));
    let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(()), || {
        gpui_platform::current_headless_renderer()
    });
    cx.update(|app| {
        let fonts: Vec<std::borrow::Cow<'static, [u8]>> = vec![
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Fraunces.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter.ttf")),
        ];
        let _ = app.text_system().add_fonts(fonts);
    });

    let state = cx.update(|app| app.new(|_cx| OverlayState::default()));
    let st = state.clone();
    cx.update(move |app| {
        st.update(app, |s, _cx| {
            s.begin("Bad Guy".to_string(), "Billie Eilish".to_string());
            s.set_status(&LyricsStatus::Synced(synced_lines()));
            s.sync_position(Duration::from_secs(20), 0, Some(Duration::from_secs(214)));
        });
    });

    let geometry = Arc::new(std::sync::Mutex::new(OverlayGeometry::default()));
    let handle = cx.update(|app| {
        let bounds = gpui::Bounds::new(
            gpui::point(gpui::px(0.0), gpui::px(0.0)),
            gpui::size(gpui::px(W as f32), gpui::px(H as f32)),
        );
        myuzik::overlay::open_overlay(app, state.clone(), geometry, bounds)
    });
    let any: AnyWindowHandle = handle.into();

    // Give the overlay's own ticker a chance to run: no input event is sent.
    let shot = |cx: &mut HeadlessAppContext| -> image::RgbaImage {
        cx.update(|app| app.refresh_windows());
        cx.run_until_parked();
        let _ = cx.update_window(any, |_view, window, cx| {
            let _ = window.draw(cx);
        });
        cx.capture_screenshot(any).expect("capture")
    };

    let first = shot(&mut cx);

    // Jump to a different part of the song, the way a position tick does.
    let st = state.clone();
    cx.update(move |app| {
        st.update(app, |s, cx| {
            s.sync_position(Duration::from_secs(160), 0, Some(Duration::from_secs(214)));
            cx.notify();
        });
    });
    std::thread::sleep(Duration::from_millis(250));
    let second = shot(&mut cx);

    assert_ne!(
        first.as_raw(),
        second.as_raw(),
        "the overlay did not repaint after the position moved"
    );
    eprintln!("overlay repainted without input");
}
