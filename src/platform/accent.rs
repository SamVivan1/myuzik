use gpui::Rgba;
use gpui::rgb;

/// Read the OS accent color.
///
/// GNOME 47+ stores it in `org.gnome.desktop.interface accent-color` as a name
/// ("blue", "teal", …). This used to be a stub that always returned the fallback,
/// so `follow_os_accent` was a setting that could never take effect.
pub fn read_os_accent() -> Rgba {
    read_os_accent_with(|schema_key| {
        let output = std::process::Command::new("gsettings")
            .args(["get", schema_key.0, schema_key.1])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    })
}

/// The schema and key the accent lives in.
const ACCENT_KEY: (&str, &str) = ("org.gnome.desktop.interface", "accent-color");

/// The testable core: `probe` returns the raw `gsettings get` output (or None).
///
/// Split out so the parsing is covered without a desktop session.
pub fn read_os_accent_with(probe: impl FnOnce((&str, &str)) -> Option<String>) -> Rgba {
    let accent = probe(ACCENT_KEY)
        .as_deref()
        .map(parse_accent_output)
        .unwrap_or_else(fallback_accent);
    // A focus ring nobody can see is worse than the default one.
    if accent_is_visible(accent) {
        accent
    } else {
        fallback_accent()
    }
}

/// `gsettings get` prints a GVariant, so the name arrives quoted: `'teal'`.
pub fn parse_accent_output(raw: &str) -> Rgba {
    let name = raw.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    if name.is_empty() {
        return fallback_accent();
    }
    gnome_accent_name_to_rgb(name)
}

/// The darkest surface the focus ring has to be visible against (the glass).
const GLASS: u32 = 0x0C0C10;
/// WCAG 1.4.11: a non-text UI indicator needs 3:1 against its neighbour.
const MIN_FOCUS_CONTRAST: f64 = 3.0;

/// Relative luminance of an sRGB colour, per WCAG.
fn luminance(hex: u32) -> f64 {
    let channel = |value: u32| {
        let c = value as f64 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel((hex >> 16) & 0xFF)
        + 0.7152 * channel((hex >> 8) & 0xFF)
        + 0.0722 * channel(hex & 0xFF)
}

/// Contrast ratio between two sRGB colours.
fn contrast_ratio(a: u32, b: u32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Whether an accent can be seen as a focus ring on the glass.
///
/// GNOME's `slate` accent is only ~2.97:1 against the Midnight Brass glass, so
/// drawing it as the focus ring would make focus invisible — a regression for the
/// very users who rely on the ring. Such an accent falls back to the default.
pub fn accent_is_visible(accent: Rgba) -> bool {
    let hex = ((accent.r * 255.0).round() as u32) << 16
        | ((accent.g * 255.0).round() as u32) << 8
        | (accent.b * 255.0).round() as u32;
    contrast_ratio(hex, GLASS) >= MIN_FOCUS_CONTRAST
}

/// The accent the app should draw with: the OS one when `follow_os_accent` is on,
/// otherwise the Midnight Brass default. Cached so a render never shells out.
static CURRENT_ACCENT: std::sync::OnceLock<Rgba> = std::sync::OnceLock::new();

/// Publish the accent chosen at startup. First call wins; later calls are ignored.
pub fn set_current_accent(accent: Rgba) {
    let _ = CURRENT_ACCENT.set(accent);
}

/// The accent every theme build should use.
pub fn current_accent() -> Rgba {
    *CURRENT_ACCENT.get().unwrap_or(&fallback_accent())
}

pub fn accent_from_rgb(r: f64, g: f64, b: f64) -> Rgba {
    rgb(((r * 255.0).round() as u32) << 16
        | ((g * 255.0).round() as u32) << 8
        | ((b * 255.0).round() as u32))
}

pub fn fallback_accent() -> Rgba {
    rgb(0x3584E4)
}

/// Map a GNOME accent color name to RGB.
pub fn gnome_accent_name_to_rgb(name: &str) -> Rgba {
    match name.trim().to_lowercase().as_str() {
        "blue" => rgb(0x3584E4),
        "teal" => rgb(0x2EC27E),
        "green" => rgb(0x33D17A),
        "yellow" => rgb(0xF8E45C),
        "orange" => rgb(0xFF7800),
        "red" => rgb(0xE01B24),
        "pink" => rgb(0xF66151),
        "purple" => rgb(0x9141AC),
        "slate" => rgb(0x5E5C64),
        _ => fallback_accent(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quoted_gsettings_name_is_parsed() {
        // `gsettings get` prints a GVariant, so the value arrives quoted.
        assert_eq!(
            parse_accent_output("'teal'\n"),
            gnome_accent_name_to_rgb("teal")
        );
        assert_eq!(
            parse_accent_output("\"orange\""),
            gnome_accent_name_to_rgb("orange")
        );
        assert_eq!(
            parse_accent_output("  'purple'  "),
            gnome_accent_name_to_rgb("purple")
        );
    }

    #[test]
    fn an_unknown_or_empty_accent_falls_back() {
        assert_eq!(parse_accent_output("'chartreuse'"), fallback_accent());
        assert_eq!(parse_accent_output(""), fallback_accent());
        assert_eq!(parse_accent_output("''"), fallback_accent());
    }

    #[test]
    fn an_accent_that_cannot_be_seen_falls_back() {
        // GNOME's slate is ~2.97:1 on the glass, below the 3:1 a focus ring needs.
        assert!(!accent_is_visible(gnome_accent_name_to_rgb("slate")));
        assert_eq!(
            read_os_accent_with(|_| Some("'slate'".to_string())),
            fallback_accent(),
            "an invisible accent must not become the focus ring"
        );
        // The ones that pass stay.
        for name in [
            "blue", "teal", "green", "yellow", "orange", "red", "pink", "purple",
        ] {
            assert!(
                accent_is_visible(gnome_accent_name_to_rgb(name)),
                "{name} should be usable as a focus ring"
            );
        }
    }

    #[test]
    fn a_missing_probe_result_falls_back() {
        assert_eq!(read_os_accent_with(|_| None), fallback_accent());
    }

    #[test]
    fn a_present_probe_result_is_used() {
        let read = read_os_accent_with(|(schema, key)| {
            assert_eq!(schema, "org.gnome.desktop.interface");
            assert_eq!(key, "accent-color");
            Some("'green'".to_string())
        });
        assert_eq!(read, gnome_accent_name_to_rgb("green"));
    }

    #[test]
    fn maps_known_accent_names() {
        assert_eq!(
            accent_from_rgb(0.2078, 0.5176, 0.8941),
            gnome_accent_name_to_rgb("blue")
        );
    }

    #[test]
    fn unknown_accent_falls_back() {
        assert_eq!(gnome_accent_name_to_rgb("unknown"), fallback_accent());
    }

    #[test]
    fn accent_rgb_conversion() {
        let c = accent_from_rgb(1.0, 0.0, 0.0);
        // red
        assert_eq!(c.r, 1.0);
        assert_eq!(c.g, 0.0);
        assert_eq!(c.b, 0.0);
    }
}
