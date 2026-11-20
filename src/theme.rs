//! Liquid Glass design tokens.
//!
//! Apple-inspired translucent surfaces over a deep ink base, with a dynamic
//! accent (default cool indigo) that is overridden at runtime from cover art.
//! Every component in Myuzik reads colors, radii, shadows, and fonts from
//! `MIDNIGHT_BRASS` (the function name is retained for compatibility; the
//! palette is Liquid Glass). No raw colors in view code.

use gpui::prelude::*;
use gpui::{Rgba, rgb, rgba};

#[derive(Clone, Copy, Debug)]
pub struct Colors {
    pub bg_base: Rgba,
    pub bg_raised: Rgba,
    pub glass_tint: Rgba,
    pub glass_border: Rgba,
    pub glass_border_on_clay: Rgba,
    pub text_primary: Rgba,
    pub text_secondary: Rgba,
    pub text_tertiary: Rgba,
    pub gold: Rgba,
    pub gold_light: Rgba,
    pub gold_deep: Rgba,
    pub clay_surface: Rgba,
    pub clay_shade: Rgba,
    pub clay_ink: Rgba,
    pub os_accent: Rgba,
    pub danger: Rgba,
    pub shadow_warm: Rgba,
    pub glass_highlight: Rgba,
    pub glass_hover: Rgba,
    pub clay_dark: Rgba,
    pub clay_dark_2: Rgba,
    pub gold_tint: Rgba,
    pub gold_tint_border: Rgba,
    pub glass_inner: Rgba,
    pub glass_inner_strong: Rgba,
    pub glass_border_soft: Rgba,
    pub glass_highlight_soft: Rgba,
    pub glass_highlight_strong: Rgba,
    pub shadow_deep: Rgba,
    pub scrim: Rgba,
    pub gold_glow: Rgba,
    pub gold_glow_border: Rgba,
    pub clay_gradient_start: Rgba,
    pub clay_gradient_end: Rgba,
    pub ambient_gold: Rgba,
    pub ambient_teal: Rgba,
    pub ambient_teal_deep: Rgba,
    pub ambient_rose: Rgba,
    pub ambient_rose_deep: Rgba,
    pub bg_gradient_start: Rgba,
    pub bg_gradient_end: Rgba,
    pub glass_gradient_start: Rgba,
    pub glass_gradient_end: Rgba,
    pub fade: Rgba,
}

#[derive(Clone, Copy, Debug)]
pub struct Radii {
    pub glass: f32,
    pub card: f32,
    pub clay: f32,
    pub button: f32,
    pub pill: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub colors: Colors,
    pub radii: Radii,
    pub display_font: &'static str,
    pub ui_font: &'static str,
}

pub fn midnight_brass() -> Theme {
    Theme {
        colors: Colors {
            // Deep ink base with a subtle aurora.
            bg_base: rgb(0x0A0B10),
            bg_raised: rgb(0x12131A),
            // Translucent glass surfaces.
            glass_tint: rgba(0xFFFFFF0D),
            glass_border: rgba(0xFFFFFF24),
            glass_border_on_clay: rgba(0xFFFFFFFF),
            text_primary: rgb(0xF5F3EE),
            text_secondary: rgb(0xB8B2A8),
            text_tertiary: rgb(0x9A948C),
            // Accent: cool indigo fallback (overridden at runtime from cover art).
            gold: rgb(0x6F7CFF),
            gold_light: rgb(0x9FA8FF),
            gold_deep: rgb(0x4A55CC),
            // Glass surfaces (replacing clay).
            clay_surface: rgba(0xFFFFFF14),
            clay_shade: rgba(0xFFFFFF0A),
            clay_ink: rgb(0xF5F3EE),
            // Follows the desktop's accent when `follow_os_accent` is on.
            os_accent: crate::platform::accent::current_accent(),
            danger: rgb(0xFF5A6A),
            shadow_warm: rgba(0x00000040),
            glass_highlight: rgba(0xFFFFFF1A),
            glass_hover: rgba(0xFFFFFF0D),
            clay_dark: rgb(0x1A1B22),
            clay_dark_2: rgb(0x12131A),
            gold_tint: rgba(0x6F7CFF1F),
            gold_tint_border: rgba(0x6F7CFF61),
            glass_inner: rgba(0xFFFFFF08),
            glass_inner_strong: rgba(0xFFFFFF0F),
            glass_border_soft: rgba(0xFFFFFF14),
            glass_highlight_soft: rgba(0xFFFFFF0A),
            glass_highlight_strong: rgba(0xFFFFFF24),
            shadow_deep: rgba(0x00000055),
            scrim: rgba(0x00000099),
            gold_glow: rgba(0x6F7CFF0D),
            gold_glow_border: rgba(0x6F7CFF1F),
            clay_gradient_start: rgba(0xFFFFFF14),
            clay_gradient_end: rgba(0xFFFFFF08),
            ambient_gold: rgba(0x6F7CFF14),
            ambient_teal: rgba(0x2F6B6318),
            ambient_teal_deep: rgba(0x2F6B6328),
            ambient_rose: rgba(0x7A4A5514),
            ambient_rose_deep: rgba(0x7A4A5524),
            bg_gradient_start: rgb(0x0E0F14),
            bg_gradient_end: rgb(0x0A0B10),
            glass_gradient_start: rgb(0x14141E),
            glass_gradient_end: rgb(0x0A0A12),
            fade: rgba(0x0A0B10F2),
        },
        radii: Radii {
            glass: 20.0,
            card: 20.0,
            clay: 24.0,
            button: 999.0,
            pill: 999.0,
        },
        display_font: "Inter",
        ui_font: "Inter",
    }
}

/// WCAG relative luminance for an sRGB Rgba.
pub fn relative_luminance(c: Rgba) -> f32 {
    fn channel(v: f32) -> f32 {
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
}

/// WCAG contrast ratio between two colors (always >= 1.0).
pub fn contrast_ratio(a: Rgba, b: Rgba) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Extract a vibrant accent color from image bytes. Downsamples to 16x16 and
/// picks the most saturated pixel, then lightens if too dark. Returns None if
/// the bytes cannot be decoded.
pub fn accent_from_bytes(bytes: &[u8]) -> Option<Rgba> {
    let img = image::load_from_memory(bytes).ok()?;
    let small = img
        .resize_exact(16, 16, image::imageops::FilterType::Triangle)
        .to_rgba8();
    let mut best: Option<(f32, f32, f32)> = None;
    let mut best_score = -1.0f32;
    for px in small.pixels() {
        let r = px[0] as f32 / 255.0;
        let g = px[1] as f32 / 255.0;
        let b = px[2] as f32 / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
        let score = sat * (0.5 + 0.5 * max);
        if score > best_score {
            best_score = score;
            best = Some((r, g, b));
        }
    }
    let (mut r, mut g, mut b) = best?;
    let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    if lum < 0.3 {
        let factor = 0.3 / lum.max(0.01);
        r = (r * factor).min(1.0);
        g = (g * factor).min(1.0);
        b = (b * factor).min(1.0);
    }
    Some(rgb(((r * 255.0) as u32) << 16
        | ((g * 255.0) as u32) << 8
        | ((b * 255.0) as u32)))
}

/// Apply a Liquid Glass surface to a div: translucent background, subtle
/// border highlight, rounded corners, and a soft shadow. Returns the div so
/// callers can chain further styling.
pub fn glass_surface(div: gpui::Div, t: &Colors, radius: f32) -> gpui::Div {
    div.bg(t.glass_tint)
        .border_1()
        .border_color(t.glass_border)
        .rounded(gpui::px(radius))
        .shadow(vec![
            // Specular highlight at the top edge (simulates glass refraction).
            gpui::BoxShadow {
                color: t.glass_highlight_strong.into(),
                offset: gpui::point(gpui::px(0.0), gpui::px(1.0)),
                blur_radius: gpui::px(0.0),
                spread_radius: gpui::px(0.0),
                inset: true,
            },
            // Subtle inner shadow for depth.
            gpui::BoxShadow {
                color: t.glass_highlight_soft.into(),
                offset: gpui::point(gpui::px(0.0), gpui::px(2.0)),
                blur_radius: gpui::px(4.0),
                spread_radius: gpui::px(0.0),
                inset: true,
            },
            // Drop shadow for elevation.
            gpui::BoxShadow {
                color: t.shadow_deep.into(),
                offset: gpui::point(gpui::px(0.0), gpui::px(8.0)),
                blur_radius: gpui::px(24.0),
                spread_radius: gpui::px(0.0),
                inset: false,
            },
        ])
}

/// Glass surface color over a given base, simulating the dark glass tint.
pub fn glass_over(base: Rgba) -> Rgba {
    let t = midnight_brass().colors.glass_tint;
    let r = base.r * (1.0 - t.a) + t.r * t.a;
    let g = base.g * (1.0 - t.a) + t.g * t.a;
    let b = base.b * (1.0 - t.a) + t.b * t.a;
    rgb(((r * 255.0) as u32) << 16 | ((g * 255.0) as u32) << 8 | ((b * 255.0) as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_text_on_glass_passes_aa() {
        let glass = glass_over(midnight_brass().colors.bg_base);
        let ratio = contrast_ratio(midnight_brass().colors.text_primary, glass);
        assert!(ratio >= 4.5, "body text contrast {ratio:.2} < 4.5");
    }

    #[test]
    fn active_accent_on_glass_passes_aa() {
        let glass = glass_over(midnight_brass().colors.bg_base);
        let ratio = contrast_ratio(midnight_brass().colors.gold_light, glass);
        assert!(ratio >= 4.5, "accent text contrast {ratio:.2} < 4.5");
    }

    #[test]
    fn danger_text_on_glass_passes_aa() {
        let glass = glass_over(midnight_brass().colors.bg_base);
        let ratio = contrast_ratio(midnight_brass().colors.danger, glass);
        assert!(ratio >= 4.5, "danger text contrast {ratio:.2} < 4.5");
    }

    #[test]
    fn os_accent_focus_ring_passes_3_to_1() {
        let glass = glass_over(midnight_brass().colors.bg_base);
        assert!(contrast_ratio(midnight_brass().colors.os_accent, glass) >= 3.0);
    }

    #[test]
    fn tertiary_caption_text_on_glass_passes_aa() {
        let glass = glass_over(midnight_brass().colors.bg_base);
        let ratio = contrast_ratio(midnight_brass().colors.text_tertiary, glass);
        assert!(ratio >= 4.5, "tertiary text contrast {ratio:.2} < 4.5");
    }
}
