//! FL-07 / FL-12 / FL-13 (`docs/audit/flow-logic.md`): a mid-stream failure must
//! be representable, retried a bounded number of times and then handed on to the
//! next track; buffering must be representable; and a transient GStreamer
//! `Stopped` during a load must not overwrite the real status.
//!
//! Everything here is pure state-machine logic: no GStreamer pipeline, no
//! Navidrome, no display, no wall-clock sleeping.

use myuzik::library::Track;
use myuzik::player::state::{
    PlaybackStatus, PlayerState, RetryDecision, RetryPolicy, StreamFailureGuard,
    status_for_buffering,
};
use std::time::Duration;

/// Minimal track fixture: only the fields playback logic reads carry meaning.
fn track(id: &str) -> Track {
    Track {
        id: id.to_string(),
        title: format!("Track {id}"),
        artist: "Artist".to_string(),
        artist_id: None,
        album: "Album".to_string(),
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

/// A player that has just been asked to play `ids`, i.e. mid-load.
fn playing_state(ids: &[&str]) -> PlayerState {
    let mut state = PlayerState::new();
    state.play_now(ids.iter().map(|id| track(id)).collect(), 0);
    state
}

// ---------------------------------------------------------------------------
// FL-07 — a failed stream must leave the Playing claim
// ---------------------------------------------------------------------------

#[test]
fn a_stream_failure_moves_the_state_to_error_and_keeps_the_track() {
    let mut state = playing_state(&["a", "b"]);
    state.set_error();

    assert_eq!(
        state.status,
        PlaybackStatus::Error,
        "a pipeline error must not leave the state claiming Playing"
    );
    assert_eq!(
        state.current.as_ref().map(|t| t.id.as_str()),
        Some("a"),
        "the track is kept so the UI can name what failed"
    );
}

#[test]
fn toggle_from_error_does_not_pretend_playback_resumed() {
    let mut state = playing_state(&["a"]);
    state.set_error();
    state.toggle();

    assert_eq!(
        state.status,
        PlaybackStatus::Error,
        "an errored pipeline cannot be resumed by toggle; recovery is a reload"
    );
}

#[test]
fn toggle_from_buffering_does_not_pretend_playback_paused() {
    let mut state = playing_state(&["a"]);
    state.set_status(PlaybackStatus::Buffering);
    state.toggle();

    assert_eq!(
        state.status,
        PlaybackStatus::Buffering,
        "only the engine decides when a buffering stream becomes Playing/Paused"
    );
}

#[test]
fn the_first_failure_retries_after_the_base_delay() {
    let policy = RetryPolicy::new(3, Duration::from_millis(500));

    assert_eq!(
        policy.decide(1),
        RetryDecision::Retry {
            delay: Duration::from_millis(500)
        },
        "the first failure must be retried, not given up on"
    );
}

#[test]
fn the_backoff_doubles_with_each_failed_attempt() {
    let policy = RetryPolicy::new(4, Duration::from_millis(250));

    assert_eq!(
        policy.decide(2),
        RetryDecision::Retry {
            delay: Duration::from_millis(500)
        }
    );
    assert_eq!(
        policy.decide(3),
        RetryDecision::Retry {
            delay: Duration::from_secs(1)
        }
    );
}

#[test]
fn the_policy_gives_up_after_the_configured_attempts() {
    let policy = RetryPolicy::new(3, Duration::from_millis(500));

    assert_eq!(
        policy.decide(3),
        RetryDecision::GiveUp,
        "the third failure of the same track must advance instead of retrying"
    );
    assert_eq!(
        policy.decide(4),
        RetryDecision::GiveUp,
        "attempts past the limit must stay given up, not restart the cycle"
    );
}

#[test]
fn a_policy_with_a_single_attempt_gives_up_immediately() {
    let policy = RetryPolicy::new(1, Duration::from_millis(500));

    assert_eq!(policy.decide(1), RetryDecision::GiveUp);
}

#[test]
fn attempt_zero_counts_as_the_first_failure() {
    let policy = RetryPolicy::new(2, Duration::from_millis(100));

    assert_eq!(
        policy.decide(0),
        RetryDecision::Retry {
            delay: Duration::from_millis(100)
        },
        "an off-by-one in the caller must not silently skip the track"
    );
}

#[test]
fn the_default_policy_retries_before_it_gives_up() {
    let policy = RetryPolicy::default();

    assert!(matches!(policy.decide(1), RetryDecision::Retry { .. }));
    assert_eq!(policy.decide(policy.max_attempts()), RetryDecision::GiveUp);
}

#[test]
fn a_huge_attempt_count_does_not_overflow_the_backoff() {
    let policy = RetryPolicy::new(255, Duration::from_secs(1));

    assert_eq!(
        policy.decide(254),
        RetryDecision::Retry {
            delay: Duration::from_secs(30)
        },
        "the backoff is capped instead of overflowing"
    );
}

#[test]
fn the_failure_guard_stops_advancing_once_the_queue_looks_dead() {
    let mut guard = StreamFailureGuard::new(3);

    assert!(!guard.record_failure(), "first dead track still advances");
    assert!(!guard.record_failure(), "second dead track still advances");
    assert!(
        guard.record_failure(),
        "a whole run of dead tracks must stop the advance instead of looping"
    );
    assert!(guard.record_failure(), "the guard stays stopped");
}

#[test]
fn a_track_that_actually_played_clears_the_failure_run() {
    let mut guard = StreamFailureGuard::new(2);

    assert!(!guard.record_failure());
    guard.record_success();
    assert!(
        !guard.record_failure(),
        "the run restarted after real playback"
    );
}

// ---------------------------------------------------------------------------
// FL-12 — a filling buffer must be visible
// ---------------------------------------------------------------------------

#[test]
fn a_stream_below_one_hundred_percent_says_buffering() {
    assert_eq!(status_for_buffering(0), Some(PlaybackStatus::Buffering));
    assert_eq!(status_for_buffering(99), Some(PlaybackStatus::Buffering));
}

#[test]
fn a_finished_buffer_leaves_the_status_to_the_engine() {
    assert_eq!(
        status_for_buffering(100),
        None,
        "a full buffer is not a status of its own"
    );
}

#[test]
fn the_player_shows_buffering_while_the_stream_fills() {
    let mut state = playing_state(&["a"]);

    state.set_buffering(37);

    assert_eq!(
        state.status,
        PlaybackStatus::Buffering,
        "a stalled stream must not keep claiming Playing"
    );
}

#[test]
fn buffering_that_completes_returns_to_playing_instead_of_sticking() {
    let mut state = playing_state(&["a"]);
    state.set_buffering(10);

    state.set_buffering(100);

    assert_eq!(
        state.status,
        PlaybackStatus::Playing,
        "the buffering indicator must clear when the buffer is full"
    );
}

#[test]
fn buffering_that_completes_does_not_overwrite_an_unrelated_status() {
    let mut state = playing_state(&["a"]);
    state.set_status(PlaybackStatus::Paused);

    state.set_buffering(100);

    assert_eq!(state.status, PlaybackStatus::Paused);
}

// ---------------------------------------------------------------------------
// FL-13 — one source of truth for the status
// ---------------------------------------------------------------------------

#[test]
fn a_transient_stopped_during_a_load_does_not_flip_the_status() {
    let mut state = playing_state(&["a", "b"]);
    assert!(state.is_load_pending(), "play_now issues a load");

    // GStreamer goes Ready -> Stopped -> Paused on the way to Playing.
    state.set_status_from_engine(PlaybackStatus::Stopped);

    assert_eq!(
        state.status,
        PlaybackStatus::Playing,
        "the icon must not flicker to stop mid-load"
    );
    assert!(state.is_load_pending(), "the load is still pending");
}

#[test]
fn the_engine_playing_report_ends_the_pending_load() {
    let mut state = playing_state(&["a", "b"]);

    state.set_status_from_engine(PlaybackStatus::Playing);

    assert!(!state.is_load_pending());
    state.set_status_from_engine(PlaybackStatus::Stopped);
    assert_eq!(
        state.status,
        PlaybackStatus::Stopped,
        "once the load is confirmed, a real stop is applied"
    );
}

#[test]
fn a_paused_report_during_a_load_is_applied() {
    let mut state = playing_state(&["a"]);

    state.set_status_from_engine(PlaybackStatus::Paused);

    assert_eq!(state.status, PlaybackStatus::Paused);
    assert!(
        state.is_load_pending(),
        "pausing does not confirm that the track started"
    );
}

#[test]
fn a_stop_the_user_asked_for_is_not_swallowed() {
    let mut state = playing_state(&["a"]);

    // The user's own stop must not be filtered as a transient engine report.
    state.set_status(PlaybackStatus::Stopped);

    assert_eq!(state.status, PlaybackStatus::Stopped);
    assert!(!state.is_load_pending(), "an explicit stop ends the load");
}

#[test]
fn an_engine_error_during_a_load_ends_the_load() {
    let mut state = playing_state(&["a"]);

    state.set_status_from_engine(PlaybackStatus::Error);

    assert_eq!(state.status, PlaybackStatus::Error);
    assert!(!state.is_load_pending(), "a failed load is over");
}

#[test]
fn moving_to_the_next_track_marks_a_new_load_pending() {
    let mut state = playing_state(&["a", "b"]);
    state.set_status_from_engine(PlaybackStatus::Playing);

    state.next();

    assert!(
        state.is_load_pending(),
        "next() is followed by a Load, so the transient stop comes again"
    );
}
