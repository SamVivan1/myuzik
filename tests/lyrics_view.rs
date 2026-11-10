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
