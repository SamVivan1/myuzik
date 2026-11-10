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
        genre: None,
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
            genre: None,
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

// --- autoplay radio support ------------------------------------------------
// The radio appends batches while a track plays (prefetch) and, when the queue
// already ran out, has to resume playback on the first new track.

fn tracks(n: usize) -> Vec<Track> {
    (0..n).map(|i| track(&format!("t{i}"))).collect()
}

#[test]
fn remaining_counts_tracks_after_the_current_one() {
    let mut q = Queue::new();
    assert_eq!(q.remaining(), 0, "empty queue has nothing remaining");
    q.play_now(tracks(3), 0);
    assert_eq!(q.remaining(), 2);
    q.next();
    assert_eq!(q.remaining(), 1);
    q.next();
    assert_eq!(q.remaining(), 0);
}

#[test]
fn append_does_not_move_the_current_track() {
    let mut q = Queue::new();
    q.play_now(tracks(2), 0);
    q.append(vec![track("added-1"), track("added-2")]);
    assert_eq!(q.len(), 4);
    assert_eq!(q.current().map(|t| t.id.clone()), Some("t0".to_string()));
    assert_eq!(q.remaining(), 3, "the appended tracks are pending");
}

#[test]
fn append_continue_resumes_after_the_queue_ran_out() {
    let mut q = Queue::new();
    q.play_now(tracks(2), 0);
    assert_eq!(q.next().map(|t| t.id), Some("t1".to_string()));
    // End of the queue: next() gives None and the position is cleared, which is
    // exactly the state autoplay has to recover from.
    assert!(q.next().is_none());
    assert!(q.current().is_none());
    assert_eq!(q.remaining(), 0);

    let resumed = q.append_continue(vec![track("radio-1"), track("radio-2")]);
    assert_eq!(resumed.map(|t| t.id), Some("radio-1".to_string()));
    assert_eq!(
        q.current().map(|t| t.id.clone()),
        Some("radio-1".to_string())
    );
    assert_eq!(q.next().map(|t| t.id), Some("radio-2".to_string()));
}

#[test]
fn append_continue_with_nothing_to_add_is_a_no_op() {
    let mut q = Queue::new();
    q.play_now(tracks(1), 0);
    assert!(q.append_continue(Vec::new()).is_none());
    assert_eq!(q.current().map(|t| t.id.clone()), Some("t0".to_string()));
}

#[test]
fn trim_front_to_bounds_the_queue_and_keeps_the_current_track() {
    let mut q = Queue::new();
    q.play_now(tracks(10), 0);
    q.next();
    q.next();
    q.next(); // current = t3
    q.trim_front_to(6);
    // Only played tracks may be dropped, so the queue stops at current + pending.
    assert_eq!(q.len(), 7);
    assert_eq!(
        q.current().map(|t| t.id.clone()),
        Some("t3".to_string()),
        "trimming must not change which track is playing"
    );

    // Once enough has been played, the cap is reached exactly.
    for _ in 0..5 {
        q.next();
    }
    q.trim_front_to(6);
    assert_eq!(q.len(), 6);
    assert_eq!(q.current().map(|t| t.id.clone()), Some("t8".to_string()));
}

#[test]
fn trim_leaves_the_queue_untouched_when_under_the_cap() {
    let mut q = Queue::new();
    q.play_now(tracks(4), 0);
    let before = q.ids();
    q.trim_front_to(500);
    assert_eq!(q.len(), 4);
    assert_eq!(q.ids(), before);
}

#[test]
fn trim_under_shuffle_keeps_the_order_valid() {
    let mut q = Queue::new();
    q.play_now(tracks(12), 0);
    q.set_shuffle(true);
    q.next();
    q.next();
    q.trim_front_to(6);
    // Every remaining id is still from the original set, and the shuffle order
    // still walks exactly the pending tracks before ending cleanly.
    let seen: Vec<String> = q.tracks().iter().map(|t| t.id.clone()).collect();
    assert!(seen.iter().all(|id| id.starts_with('t')), "{seen:?}");
    let pending = q.remaining();
    assert!(pending > 0, "trimming left nothing to play");
    for _ in 0..pending {
        assert!(q.next().is_some(), "shuffle walk broke after trimming");
    }
    assert!(q.next().is_none(), "the shuffle order should end cleanly");
}

#[test]
fn ids_reflects_the_queue_for_deduplication() {
    let mut q = Queue::new();
    q.play_now(tracks(3), 0);
    q.append(vec![track("extra")]);
    let ids = q.ids();
    assert_eq!(ids.len(), 4);
    assert!(ids.contains("extra"));
    assert!(ids.contains("t0"));
}
