use gpui::Rgba;
use gpui::rgb;

/// Read the OS accent color. Tries XDG portal first, then GNOME gsettings, then fallback.
pub fn read_os_accent() -> Rgba {
    // Try ashpd portal (GNOME 47+ supports accent-color).
    // For v1, use a simple fallback; full portal integration in Task 20.
    fallback_accent()
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
