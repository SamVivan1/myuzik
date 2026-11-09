/// Compute the vertical translation needed to center the active lyric line.
/// `line_tops`: top y of each line relative to the list content.
/// `line_heights`: height of each line.
/// `viewport_h`: visible height of the lyrics viewport.
/// `active`: index of the active line.
/// Returns the translation (negative to scroll down).
pub fn target_offset(
    line_tops: &[f32],
    line_heights: &[f32],
    viewport_h: f32,
    active: usize,
) -> f32 {
    if active >= line_tops.len() || line_tops.is_empty() {
        return 0.0;
    }
    let line_center = line_tops[active] + line_heights[active] / 2.0;
    let target = viewport_h / 2.0 - line_center;
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_first_line() {
        let tops = [0.0, 40.0, 80.0];
        let heights = [20.0, 20.0, 20.0];
        let offset = target_offset(&tops, &heights, 100.0, 0);
        // line 0 center = 10; target = 50 - 10 = 40
        assert!((offset - 40.0).abs() < 0.01);
    }

    #[test]
    fn centers_middle_line() {
        let tops = [0.0, 40.0, 80.0];
        let heights = [20.0, 20.0, 20.0];
        let offset = target_offset(&tops, &heights, 100.0, 1);
        // line 1 center = 50; target = 50 - 50 = 0
        assert!((offset - 0.0).abs() < 0.01);
    }

    #[test]
    fn centers_last_line() {
        let tops = [0.0, 40.0, 80.0];
        let heights = [20.0, 20.0, 20.0];
        let offset = target_offset(&tops, &heights, 100.0, 2);
        // line 2 center = 90; target = 50 - 90 = -40
        assert!((offset - (-40.0)).abs() < 0.01);
    }

    #[test]
    fn out_of_bounds_returns_zero() {
        let tops = [0.0, 40.0];
        let heights = [20.0, 20.0];
        assert_eq!(target_offset(&tops, &heights, 100.0, 5), 0.0);
    }

    #[test]
    fn empty_returns_zero() {
        assert_eq!(target_offset(&[], &[], 100.0, 0), 0.0);
    }
}
