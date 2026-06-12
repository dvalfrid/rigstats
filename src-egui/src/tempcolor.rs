use egui::Color32;

// Colours matching the JS dashboard constants.
pub fn color_ok() -> Color32 {
    Color32::from_rgb(0x39, 0xff, 0x88)
}
pub fn color_warn() -> Color32 {
    Color32::from_rgb(0xff, 0xb3, 0x00)
}
pub fn color_hot() -> Color32 {
    Color32::from_rgb(0xff, 0x3a, 0x1f)
}
pub fn color_unknown() -> Color32 {
    Color32::from_rgb(0x6f, 0x8d, 0xb7)
}

/// Maps a temperature (°C) to a colour given warn/crit thresholds.
/// `None` or `value <= 0` returns `color_unknown`.
pub fn temp_color(value: Option<f64>, warn: u8, crit: u8) -> Color32 {
    match value {
        None | Some(f64::NEG_INFINITY..=0.0) => color_unknown(),
        Some(v) if v >= crit as f64 => color_hot(),
        Some(v) if v >= warn as f64 => color_warn(),
        Some(_) => color_ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_returns_unknown() {
        assert_eq!(temp_color(None, 80, 90), color_unknown());
    }

    #[test]
    fn zero_returns_unknown() {
        assert_eq!(temp_color(Some(0.0), 80, 90), color_unknown());
    }

    #[test]
    fn below_warn_is_green() {
        assert_eq!(temp_color(Some(70.0), 80, 90), color_ok());
    }

    #[test]
    fn at_warn_is_yellow() {
        assert_eq!(temp_color(Some(80.0), 80, 90), color_warn());
    }

    #[test]
    fn between_warn_and_crit_is_yellow() {
        assert_eq!(temp_color(Some(85.0), 80, 90), color_warn());
    }

    #[test]
    fn at_crit_is_red() {
        assert_eq!(temp_color(Some(90.0), 80, 90), color_hot());
    }

    #[test]
    fn above_crit_is_red() {
        assert_eq!(temp_color(Some(100.0), 80, 90), color_hot());
    }
}
