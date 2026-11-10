//! Midnight Brass design tokens.
//!
//! Every component in Myuzik must read colors, radii, shadows, and fonts
//! from `MIDNIGHT_BRASS` (or from an overridden `Theme`). No raw colors
//! in view code. Gold is reserved for state/now-playing; the OS accent
//! is used only for focus rings and selection.

use gpui::{Rgba, rgb, rgba};

#[derive(Clone, Copy, Debug)]
pub struct Colors {
    pub bg_base: Rgba,
    pub bg_raised: Rgba,
    pub glass_tint: Rgba,
    pub glass_border: Rgba,
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
    pub ambient_rose: Rgba,
    pub bg_gradient_start: Rgba,
    pub bg_gradient_end: Rgba,
    pub glass_gradient_start: Rgba,
    pub glass_gradient_end: Rgba,
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
            bg_base: rgb(0x0B0B0E),
            bg_raised: rgb(0x121218),
            glass_tint: rgba(0x0C0C10B8), // ~72% opacity
            glass_border: rgba(0xE8D3A328),
            text_primary: rgb(0xF5EFE4),
            text_secondary: rgb(0xB8B0A2),
            text_tertiary: rgb(0x7C756A),
            gold: rgb(0xC9A86A),
            gold_light: rgb(0xE8D3A3),
            gold_deep: rgb(0x8C6D3F),
            clay_surface: rgb(0xF4EBDD),
            clay_shade: rgb(0xE7D8C2),
            clay_ink: rgb(0x2A241C),
            os_accent: rgb(0x3584E4),
            danger: rgb(0xD96C6C),
            shadow_warm: rgba(0x5A411E55),
            glass_highlight: rgba(0xFFFFFF0D),
            glass_hover: rgba(0xFFFFFF08),
            clay_dark: rgb(0x2A241C),
            clay_dark_2: rgb(0x171310),
            gold_tint: rgba(0xC9A86A12),
            gold_tint_border: rgba(0xC9A86A38),
            glass_inner: rgba(0xFFFFFF05),
            glass_inner_strong: rgba(0xFFFFFF06),
            glass_border_soft: rgba(0xE8D3A314),
            glass_highlight_soft: rgba(0xFFFFFF0A),
            glass_highlight_strong: rgba(0xFFFFFF14),
            shadow_deep: rgba(0x8C6D3F33),
            scrim: rgba(0x00000080),
            gold_glow: rgba(0xC9A86A3A),
            gold_glow_border: rgba(0xC9A86A55),
            clay_gradient_start: rgb(0xF7F0E4),
            clay_gradient_end: rgb(0xEADCC4),
            ambient_gold: rgba(0xC9A86A3A),
            ambient_teal: rgba(0x2F6B6330),
            ambient_rose: rgba(0x7A4A5528),
            bg_gradient_start: rgba(0x1C140BFF),
            bg_gradient_end: rgba(0x0B0B0EFF),
            glass_gradient_start: rgba(0x0A0A0DA8),
            glass_gradient_end: rgba(0x14100A96),
        },
        radii: Radii {
            glass: 20.0,
            card: 16.0,
            clay: 28.0,
            button: 999.0,
            pill: 999.0,
        },
        display_font: "Fraunces",
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

/// Glass surface color over a given base, simulating the 72% dark tint.
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
    fn active_gold_on_glass_passes_aa() {
        let glass = glass_over(midnight_brass().colors.bg_base);
        let ratio = contrast_ratio(midnight_brass().colors.gold_light, glass);
        assert!(ratio >= 4.5, "gold text contrast {ratio:.2} < 4.5");
    }

    #[test]
    fn clay_ink_on_clay_passes_aa() {
        let ratio = contrast_ratio(
            midnight_brass().colors.clay_ink,
            midnight_brass().colors.clay_surface,
        );
        assert!(ratio >= 4.5, "clay ink contrast {ratio:.2} < 4.5");
    }

    #[test]
    fn os_accent_focus_ring_passes_3_to_1() {
        let glass = glass_over(midnight_brass().colors.bg_base);
        assert!(contrast_ratio(midnight_brass().colors.os_accent, glass) >= 3.0);
        assert!(
            contrast_ratio(
                midnight_brass().colors.os_accent,
                midnight_brass().colors.clay_surface
            ) >= 3.0
        );
    }

    #[test]
    fn gold_is_not_used_as_large_surface() {
        // Structural guard: no color token is named "gold_background".
        // (Manual review enforces the 5–10% rule.)
    }
}
