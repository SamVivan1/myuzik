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

/// Pure helper: should we submit given listened time and duration?
pub fn should_submit(listened: Duration, duration: Duration) -> bool {
    listened >= scrobble_threshold(duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_half_for_short_tracks() {
        // 3 min track -> 1.5 min threshold
        let d = Duration::from_secs(180);
        assert_eq!(scrobble_threshold(d), Duration::from_secs(90));
    }

    #[test]
    fn threshold_four_min_for_long_tracks() {
        // 10 min track -> 4 min threshold (not 5 min)
        let d = Duration::from_secs(600);
        assert_eq!(scrobble_threshold(d), Duration::from_secs(240));
    }

    #[test]
    fn threshold_minimum_30s() {
        // 10s track -> 30s minimum
        let d = Duration::from_secs(10);
        assert_eq!(scrobble_threshold(d), Duration::from_secs(30));
    }

    #[test]
    fn tick_submits_once() {
        let mut t = ScrobbleTracker::new();
        let dur = Duration::from_secs(180);
        // 80s not enough (threshold 90s)
        assert!(!t.tick(Duration::from_secs(80), dur));
        // 20s more -> 100s >= 90s -> submit
        assert!(t.tick(Duration::from_secs(20), dur));
        // already submitted
        assert!(!t.tick(Duration::from_secs(10), dur));
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
