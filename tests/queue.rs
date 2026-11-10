use myuzik::library::Track;
use myuzik::player::queue::{Queue, RepeatMode};
use std::time::Duration;

fn track(id: &str) -> Track {
    Track {
        id: id.to_string(),
        title: id.to_string(),
        artist: "artist".to_string(),
        artist_id: None,
        album: "album".to_string(),
        album_id: None,
        cover_art_id: None,
        duration: Duration::from_secs(180),
        track_number: None,
        disc_number: None,
        year: None,
        starred: false,
    }
}

#[test]
fn next_advances_and_wraps_with_repeat_all() {
    let mut q = Queue::new();
    q.play_now(vec![track("a"), track("b"), track("c")], 0);
    assert_eq!(q.next().unwrap().id, "b");
    assert_eq!(q.next().unwrap().id, "c");
    q.set_repeat(RepeatMode::All);
    assert_eq!(q.next().unwrap().id, "a");
}

#[test]
fn repeat_one_returns_same_track() {
    let mut q = Queue::new();
    q.play_now(vec![track("a"), track("b")], 0);
    q.set_repeat(RepeatMode::One);
    assert_eq!(q.next().unwrap().id, "a");
    assert_eq!(q.next().unwrap().id, "a");
}

#[test]
fn repeat_off_at_end_returns_none() {
    let mut q = Queue::new();
    q.play_now(vec![track("a"), track("b")], 0);
    q.next();
    assert!(q.next().is_none());
}

#[test]
fn previous_returns_previous_track() {
    let mut q = Queue::new();
    q.play_now(vec![track("a"), track("b"), track("c")], 1);
    assert_eq!(q.previous().unwrap().id, "a");
}

#[test]
fn shuffle_preserves_current_and_visits_all() {
    let mut q = Queue::new();
    q.play_now(vec![track("a"), track("b"), track("c"), track("d")], 0);
    q.set_shuffle(true);
    let mut seen = vec![q.current().unwrap().id.clone()];
    for _ in 0..3 {
        if let Some(t) = q.next() {
            seen.push(t.id);
        }
    }
    seen.sort();
    assert_eq!(seen, vec!["a", "b", "c", "d"]);
}

#[test]
fn add_to_queue_appends() {
    let mut q = Queue::new();
    q.play_now(vec![track("a")], 0);
    q.add_to_queue(track("b"));
    assert_eq!(q.len(), 2);
    assert_eq!(q.current().unwrap().id, "a");
    assert_eq!(q.next().unwrap().id, "b");
}

#[test]
fn play_next_inserts_after_current() {
    let mut q = Queue::new();
    q.play_now(vec![track("a"), track("c")], 0);
    q.play_next(track("b"));
    assert_eq!(q.next().unwrap().id, "b");
    assert_eq!(q.next().unwrap().id, "c");
}

#[test]
fn rapid_next_previous_stays_consistent() {
    let paths = myuzik::config::Paths::new().unwrap();
    let _ = paths;
    let tracks: Vec<myuzik::library::Track> = (0..50)
        .map(|i| myuzik::library::Track {
            id: format!("t{i}"),
            title: format!("Track {i}"),
            artist: "A".into(),
            artist_id: None,
            album: "X".into(),
            album_id: None,
            cover_art_id: None,
            duration: std::time::Duration::from_secs(180),
            track_number: Some(i + 1),
            disc_number: Some(1),
            year: None,
            starred: false,
        })
        .collect();
    let mut q = myuzik::player::queue::Queue::new();
    q.set_repeat(myuzik::player::queue::RepeatMode::All);
    q.play_now(tracks, 0);
    for _ in 0..100 {
        assert!(q.next().is_some());
    }
    for _ in 0..100 {
        assert!(q.previous().is_some());
    }
    assert!(q.current().is_some());
}
