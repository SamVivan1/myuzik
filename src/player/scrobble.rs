use std::time::Duration;

/// Tracks whether a scrobble has been submitted for the current track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrobbleTracker {
    pub submitted: bool,
    pub listened: Duration,
}

impl Default for ScrobbleTracker {
    fn default() -> Self {
        Self {
            submitted: false,
            listened: Duration::ZERO,
        }
    }
}

impl ScrobbleTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset for a new track.
    pub fn reset(&mut self) {
        self.submitted = false;
        self.listened = Duration::ZERO;
    }

    /// Record that `delta` seconds were listened. Returns true if a scrobble
    /// should be submitted now (threshold crossed and not yet submitted).
    pub fn tick(&mut self, delta: Duration, track_duration: Duration) -> bool {
        if self.submitted {
            return false;
        }
        self.listened += delta;
        let threshold = scrobble_threshold(track_duration);
        if self.listened >= threshold {
            self.submitted = true;
            true
        } else {
            false
        }
    }

    /// Should a "now playing" (submission=false) be sent?
    pub fn should_now_playing(&self) -> bool {
        !self.submitted && self.listened.is_zero()
    }
}

/// Scrobble threshold: 50% of track duration, or 4 minutes, whichever is shorter.
/// Minimum 30 seconds to avoid scrobbling skips.
pub fn scrobble_threshold(duration: Duration) -> Duration {
    let half = duration / 2;
    let four_min = Duration::from_secs(240);
    let threshold = half.min(four_min);
    threshold.max(Duration::from_secs(30))
}

/// How much of a position jump actually counts as listening.
///
/// The position delta and the wall-clock time since the previous event normally
/// agree, but they disagree in two ways:
///
/// - a seek jumps the position much further than the clock moved;
/// - a stalled UI delivers several position events in a burst, so the position
///   jumps while the clock between two of them barely moves.
///
/// Dropping "large" deltas, as the caller used to, silently lost real listening
/// time in the second case. Counting them whole would let a seek fake minutes.
/// So a tick is capped by what the clock allows, with a small floor so a burst is
/// never thrown away.
pub fn listened_delta(position_delta: Duration, wall_delta: Duration) -> Duration {
    /// A burst of position events may arrive inside one render stall; this is how
    /// much a single tick may claim regardless of the clock.
    const FLOOR: Duration = Duration::from_secs(2);
    position_delta.min(wall_delta.max(FLOOR))
}

/// Pure helper: should we submit given listened time and duration?
pub fn should_submit(listened: Duration, duration: Duration) -> bool {
    listened >= scrobble_threshold(duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seek_is_capped_instead_of_counted_whole() {
        // Jumped 60 s forward with 1 s of real time: count the 2 s floor, never 60.
        assert_eq!(
            listened_delta(Duration::from_secs(60), Duration::from_secs(1)),
            Duration::from_secs(2)
        );
        // A backwards jump has a saturating zero delta.
        assert_eq!(
            listened_delta(Duration::ZERO, Duration::from_secs(2)),
            Duration::ZERO
        );
    }

    #[test]
    fn a_burst_of_events_does_not_lose_listening_time() {
        // A render stall delivers 3 s of position inside 10 ms of clock.
        assert_eq!(
            listened_delta(Duration::from_secs(3), Duration::from_millis(10)),
            Duration::from_secs(2),
            "a burst must still count, capped at the floor"
        );
    }

    #[test]
    fn a_stalled_ui_does_not_lose_listening_time() {
        assert_eq!(
            listened_delta(Duration::from_secs(3), Duration::from_millis(3200)),
            Duration::from_secs(3)
        );
    }

    #[test]
    fn normal_playback_counts_the_position_delta() {
        assert_eq!(
            listened_delta(Duration::from_millis(100), Duration::from_millis(120)),
            Duration::from_millis(100)
        );
    }

    #[test]
    fn reset_clears_state() {
        let mut t = ScrobbleTracker::new();
        t.tick(Duration::from_secs(100), Duration::from_secs(180));
        t.reset();
        assert!(!t.submitted);
        assert_eq!(t.listened, Duration::ZERO);
    }

    #[test]
    fn should_submit_helper() {
        assert!(!should_submit(
            Duration::from_secs(80),
            Duration::from_secs(180)
        ));
        assert!(should_submit(
            Duration::from_secs(90),
            Duration::from_secs(180)
        ));
    }
}
