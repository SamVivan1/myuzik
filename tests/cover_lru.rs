use myuzik::ui::shell::HomeState;
use std::sync::Arc;

fn image(id: u64) -> Arc<gpui::Image> {
    Arc::new(gpui::Image {
        format: gpui::ImageFormat::Jpeg,
        bytes: vec![0; 4],
        id,
    })
}

#[test]
fn cover_cache_evicts_least_recently_added() {
    let mut state = HomeState::default();
    for i in 0..201u64 {
        state.insert_cover(format!("c{i}"), image(i));
    }
    assert_eq!(state.covers.len(), 200);
    assert!(
        !state.covers.contains_key("c0"),
        "oldest cover should be evicted"
    );
    assert!(state.covers.contains_key("c200"));
}

#[test]
fn reinserting_a_cover_does_not_grow_the_cache() {
    let mut state = HomeState::default();
    for i in 0..200u64 {
        state.insert_cover(format!("c{i}"), image(i));
    }
    state.insert_cover("c0".into(), image(999));
    assert_eq!(state.covers.len(), 200);
    assert_eq!(state.covers["c0"].id, 999);
}
