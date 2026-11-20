use mpris_server::LoopStatus;
use myuzik::library::Track;
use myuzik::platform::mpris::{
    metadata_for, mpris_volume, position_us, queue_state, repeat_mode_to_loop_status,
    resolve_relative_seek,
};
use myuzik::player::queue::{Queue, RepeatMode};
use std::time::Duration;

fn sample_track() -> Track {
    Track {
        id: "abc".into(),
        title: "Paper Satellites".into(),
        artist: "Kite Garden".into(),
        artist_id: None,
        album: "Moss & Static".into(),
        album_id: None,
        cover_art_id: Some("cover1".into()),
        duration: Duration::from_secs(247),
        track_number: Some(2),
        disc_number: Some(1),
        year: Some(2026),
        starred: false,
        genre: None,
        source: Default::default(),
        cover_url: None,
    }
}

#[test]
fn metadata_maps_all_fields() {
    let meta = metadata_for(&sample_track(), Some("file:///tmp/cover.png".into()));
    assert_eq!(meta.title(), Some("Paper Satellites"));
    assert_eq!(meta.artist(), Some(vec!["Kite Garden".to_string()]));
    assert_eq!(meta.album(), Some("Moss & Static"));
    assert_eq!(
        meta.art_url().map(|u| u.to_string()),
        Some("file:///tmp/cover.png".to_string())
    );
    assert_eq!(meta.length().map(|t| t.as_micros()), Some(247_000_000));
    assert_eq!(meta.track_number(), Some(2));
    assert_eq!(meta.disc_number(), Some(1));
}

#[test]
fn position_is_microseconds() {
    assert_eq!(position_us(Duration::from_millis(1500)), 1_500_000);
}

// --- MPRIS Seek(offset) is relative (FL-09) --------------------------------
// `Seek(offset)` must move relative to the position MPRIS reports, a negative
// offset must move backwards (never wrap through the unsigned casting), and the
// result must stay inside the track: [0, duration].

#[test]
fn seek_offset_is_relative_to_the_current_position() {
    let duration = Some(Duration::from_secs(200));
    assert_eq!(
        resolve_relative_seek(Duration::from_secs(120), 10_000_000, duration),
        Duration::from_secs(130),
        "Seek(+10s) at 120s must land on 130s, not on 10s"
    );
}

#[test]
fn negative_seek_offset_moves_back_without_wrapping() {
    let duration = Some(Duration::from_secs(200));
    assert_eq!(
        resolve_relative_seek(Duration::from_secs(120), -5_000_000, duration),
        Duration::from_secs(115),
        "Seek(-5s) at 120s must land on 115s"
    );
    assert_eq!(
        resolve_relative_seek(Duration::from_secs(2), -5_000_000, duration),
        Duration::ZERO,
        "seeking past the start must clamp to zero, never wrap to a huge value"
    );
}

#[test]
fn seek_offset_is_clamped_to_the_track_duration() {
    assert_eq!(
        resolve_relative_seek(
            Duration::from_secs(195),
            60_000_000,
            Some(Duration::from_secs(200))
        ),
        Duration::from_secs(200),
        "seeking past the end must clamp to the duration"
    );
    assert_eq!(
        resolve_relative_seek(Duration::from_secs(195), 60_000_000, None),
        Duration::from_secs(255),
        "with no known duration only the lower bound applies"
    );
}

// --- MPRIS Volume follows the configured volume (INT-03) ------------------
// The base volume is whatever the user configured, not a value hardcoded in
// the MPRIS thread; values outside the MPRIS range are clamped.

#[test]
fn mpris_volume_follows_the_configured_value() {
    // f32 -> f64 is not exact (0.3f32 is 0.3000000119...), so compare as a
    // number rather than on the bit pattern.
    assert!(
        (mpris_volume(0.3) - 0.3).abs() < 1e-6,
        "a configured volume of 30% must be reported as 30%, got {}",
        mpris_volume(0.3)
    );
    assert_eq!(mpris_volume(0.0), 0.0);
    assert_eq!(mpris_volume(1.0), 1.0);
    assert_eq!(mpris_volume(1.5), 1.0, "out-of-range values are clamped");
    assert_eq!(mpris_volume(-0.2), 0.0);
}

// --- LoopStatus / Shuffle / can_go_next / can_go_previous (INT-04) --------
// MPRIS must mirror the queue: RepeatMode -> LoopStatus, the shuffle flag, and
// Next/Previous advertised only when the queue can actually move.

fn track(id: &str) -> Track {
    Track {
        id: id.into(),
        title: id.into(),
        artist: "artist".into(),
        ..sample_track()
    }
}

#[test]
fn repeat_mode_maps_to_mpris_loop_status() {
    assert_eq!(
        repeat_mode_to_loop_status(RepeatMode::Off),
        LoopStatus::None
    );
    assert_eq!(
        repeat_mode_to_loop_status(RepeatMode::One),
        LoopStatus::Track
    );
    assert_eq!(
        repeat_mode_to_loop_status(RepeatMode::All),
        LoopStatus::Playlist
    );
}

#[test]
fn queue_state_reports_next_and_previous_from_the_queue() {
    let empty = queue_state(&Queue::new());
    assert!(!empty.can_go_next, "an empty queue has no next track");
    assert!(
        !empty.can_go_previous,
        "an empty queue has no previous track"
    );
    assert_eq!(empty.loop_status, LoopStatus::None);
    assert!(!empty.shuffle);

    let mut q = Queue::new();
    q.play_now(vec![track("a"), track("b"), track("c")], 0);
    let playing = queue_state(&q);
    assert!(playing.can_go_next, "two tracks are pending");
    assert!(
        playing.can_go_previous,
        "previous restarts the current track"
    );

    q.next();
    q.next();
    let at_end = queue_state(&q);
    assert!(
        !at_end.can_go_next,
        "repeat off: nothing after the last track"
    );
    assert!(at_end.can_go_previous, "previous still reaches the queue");

    q.set_repeat(RepeatMode::All);
    assert!(queue_state(&q).can_go_next, "repeat all wraps around");
    assert_eq!(queue_state(&q).loop_status, LoopStatus::Playlist);

    q.set_repeat(RepeatMode::One);
    assert!(queue_state(&q).can_go_next, "repeat one replays the track");
    assert_eq!(queue_state(&q).loop_status, LoopStatus::Track);
}

#[test]
fn queue_state_advertises_the_whole_shuffle_round() {
    let mut q = Queue::new();
    q.play_now(vec![track("a"), track("b"), track("c")], 0);
    q.set_shuffle(true);
    assert!(queue_state(&q).shuffle, "shuffle must be reported");
    assert_eq!(queue_state(&q).loop_status, LoopStatus::None);

    let mut steps = 0;
    while queue_state(&q).can_go_next {
        assert!(q.next().is_some(), "next advertised but returned nothing");
        steps += 1;
        assert!(steps < 10, "shuffle advertised more tracks than it has");
    }
    assert_eq!(steps, 2, "the two pending shuffle tracks are advertised");
}

#[test]
fn exhausted_queue_still_advertises_previous() {
    let mut q = Queue::new();
    q.play_now(vec![track("a"), track("b")], 0);
    while q.next().is_some() {}
    assert!(q.current().is_none(), "precondition: exhausted queue");
    let state = queue_state(&q);
    assert!(!state.can_go_next);
    assert!(
        state.can_go_previous,
        "previous restarts the last track after exhaustion (FL-17)"
    );
}
