use myuzik::ui::components::lyrics_view::target_offset;

#[test]
fn centers_the_active_line() {
    let tops = [0.0, 40.0, 80.0];
    let heights = [30.0, 30.0, 30.0];
    assert_eq!(target_offset(&tops, &heights, 200.0, 1), 45.0);
}

#[test]
fn empty_or_out_of_range_is_zero() {
    assert_eq!(target_offset(&[], &[], 200.0, 0), 0.0);
    assert_eq!(target_offset(&[0.0], &[10.0], 200.0, 5), 0.0);
}

// --- the view's own centering -------------------------------------------------
//
// `target_offset` is the correct arithmetic but used to be reachable only from
// tests: the render did `-(active * 40)` against a viewport height that nothing
// ever set. These tests pin the wiring.

use myuzik::lyrics::model::{LyricLine, LyricsStatus};
use myuzik::ui::components::lyrics_view::LyricsView;
use std::time::Duration;

fn synced(seconds: &[u64]) -> LyricsStatus {
    LyricsStatus::Synced(
        seconds
            .iter()
            .map(|s| LyricLine {
                time: Duration::from_secs(*s),
                text: format!("line {s}"),
            })
            .collect(),
    )
}

#[test]
fn a_wrapped_row_is_centered_from_its_measured_height() {
    // Row 2 wrapped onto a second line, so the rows are not a uniform grid.
    let tops = [0.0, 40.0, 80.0, 140.0];
    let heights = [40.0, 40.0, 60.0, 40.0];

    let centered = target_offset(&tops, &heights, 200.0, 2);

    assert_eq!(centered, 100.0 - (80.0 + 30.0));
    assert_ne!(
        centered,
        -(2.0 * 40.0),
        "index x row-height arithmetic ignores the wrapped row and drifts"
    );
}

#[test]
fn the_view_centers_the_active_line_through_target_offset() {
    let mut view = LyricsView::new();
    view.set_status(synced(&[0, 5, 10, 15, 20]));
    view.set_viewport_height(600.0);
    view.set_position(Duration::from_secs(16));

    assert_eq!(view.active(), Some(3));
    let (tops, heights) = view.row_geometry();
    assert_eq!(
        view.offset(),
        target_offset(tops, heights, 600.0, 3),
        "the render must use the shared centering arithmetic"
    );
}

#[test]
fn every_row_has_the_height_the_arithmetic_assumes() {
    let mut view = LyricsView::new();
    view.set_status(synced(&[0, 5, 10]));
    let (_, heights) = view.row_geometry();
    assert_eq!(
        heights,
        vec![40.0, 40.0, 40.0],
        "rows are laid out at a fixed height, so index x 40 matches the layout"
    );
}

#[test]
fn padding_is_half_the_real_viewport() {
    let mut view = LyricsView::new();
    view.set_viewport_height(600.0);
    assert_eq!(
        view.content_padding(),
        280.0,
        "half the viewport minus half a row, so the active line lands mid-panel"
    );
    view.set_viewport_height(320.0);
    assert_eq!(view.content_padding(), 140.0);
}

/// The panel header must describe the lyrics that are actually loaded. It was
/// hardcoded to "SYNCED LYRICS", which is wrong for unsynced, instrumental,
/// missing and failed lyrics — five of the six states.
#[test]
fn the_header_follows_the_lyrics_status() {
    let mut view = LyricsView::new();
    assert_eq!(view.header_label(), "LYRICS", "nothing loaded yet");

    view.set_status(LyricsStatus::Synced(vec![]));
    assert_eq!(view.header_label(), "SYNCED LYRICS");

    view.set_status(LyricsStatus::Plain("some words".to_string()));
    assert_eq!(view.header_label(), "UNSYNCED LYRICS");

    view.set_status(LyricsStatus::Instrumental);
    assert_eq!(view.header_label(), "INSTRUMENTAL");

    view.set_status(LyricsStatus::NotFound);
    assert_eq!(view.header_label(), "NO LYRICS");

    view.set_status(LyricsStatus::Error("boom".to_string()));
    assert_eq!(view.header_label(), "LYRICS ERROR");

    view.set_status(LyricsStatus::Loading);
    assert_eq!(view.header_label(), "LOADING LYRICS");
}
