use crate::library::Track;
use crate::player::queue::{Queue, RepeatMode};
use std::time::Duration;

/// Ceiling for the retry backoff: a stream that has already failed several
/// times is not worth waiting minutes for, and the caller is better served by
/// advancing to the next track.
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Stopped,
    Playing,
    Paused,
    Buffering,
    Error,
}

/// What the shell should do after a stream failed (audit FL-07).
///
/// The policy is pure: it only looks at the number of consecutive failures for
/// the current track, so the event loop keeps ownership of the reload and of
/// the queue advance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    /// Reload the failed track once `delay` has passed.
    Retry { delay: Duration },
    /// Stop retrying this track. The caller advances to the next track (unless
    /// [`StreamFailureGuard`] says the whole queue is dead) or stops.
    GiveUp,
}

/// Bounded exponential backoff for a stream that keeps failing (audit FL-07).
///
/// `attempt` is the number of consecutive failures of the *current* track,
/// 1-based; `0` is treated as the first attempt so an off-by-one in the caller
/// retries instead of silently skipping the track. `max_attempts` counts the
/// total attempts per track, so `RetryPolicy::new(3, base)` means "play it,
/// retry twice, then give up".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    max_attempts: u8,
    base_delay: Duration,
}

impl RetryPolicy {
    /// `max_attempts` is clamped to at least 1 ("try once, then give up").
    pub fn new(max_attempts: u8, base_delay: Duration) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            base_delay,
        }
    }

    /// Total attempts allowed per track, retries included.
    pub fn max_attempts(&self) -> u8 {
        self.max_attempts
    }

    /// Decide what to do after failure number `attempt` (1-based).
    pub fn decide(&self, attempt: u8) -> RetryDecision {
        let attempt = attempt.max(1);
        if attempt >= self.max_attempts {
            return RetryDecision::GiveUp;
        }
        RetryDecision::Retry {
            delay: self.delay_for(attempt),
        }
    }

    /// `base_delay` on the first failure, doubling after that, capped at
    /// [`MAX_RETRY_DELAY`] so a large attempt count cannot overflow.
    fn delay_for(&self, attempt: u8) -> Duration {
        let factor = 1u32 << u32::from(attempt - 1).min(6);
        self.base_delay.saturating_mul(factor).min(MAX_RETRY_DELAY)
    }
}

impl Default for RetryPolicy {
    /// Three attempts per track: play, retry after 500 ms, retry after 1 s,
    /// then give up and advance.
    fn default() -> Self {
        Self::new(3, Duration::from_millis(500))
    }
}

/// Stops the "give up and advance" reaction when a whole run of tracks fails
/// (audit FL-07): a queue where every entry is dead must not be skipped
/// through in a loop, since the stream source, not the track, is broken.
///
/// The shell calls [`record_failure`](Self::record_failure) when it gives up on
/// a track and [`record_success`](Self::record_success) when a track actually
/// plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamFailureGuard {
    stop_after: u8,
    consecutive: u8,
}

impl StreamFailureGuard {
    /// `stop_after` is clamped to at least 1.
    pub fn new(stop_after: u8) -> Self {
        Self {
            stop_after: stop_after.max(1),
            consecutive: 0,
        }
    }

    /// Record one track that gave up. Returns `true` when playback must stop
    /// instead of advancing to yet another track.
    pub fn record_failure(&mut self) -> bool {
        self.consecutive = self.consecutive.saturating_add(1);
        self.consecutive >= self.stop_after
    }

    /// Record a track that really played, ending the current run of failures.
    pub fn record_success(&mut self) {
        self.consecutive = 0;
    }

    /// Length of the current run of given-up tracks.
    pub fn consecutive_failures(&self) -> u8 {
        self.consecutive
    }
}

impl Default for StreamFailureGuard {
    /// Three dead tracks in a row means the source is broken, not the track.
    fn default() -> Self {
        Self::new(3)
    }
}

/// Map a GStreamer buffering percentage onto a status (audit FL-12).
///
/// Below 100 % the stream is still filling: `Some(Buffering)`. At 100 % there
/// is no status to report — the caller keeps what it had, and the engine's next
/// `StateChanged` stays authoritative. [`PlayerState::set_buffering`] does that
/// and additionally clears its own buffering indicator.
pub fn status_for_buffering(percent: u8) -> Option<PlaybackStatus> {
    (percent < 100).then_some(PlaybackStatus::Buffering)
}

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub current: Option<Track>,
    pub status: PlaybackStatus,
    pub position: Duration,
    pub duration: Option<Duration>,
    pub volume: f32,
    pub queue: Queue,
    /// True from the moment a `Load` is issued until the engine confirms
    /// `Playing`. GStreamer posts a transient `Stopped` while a freshly loaded
    /// pipeline goes `Ready -> Paused`; that report is ignored while this is
    /// set, which is the FL-13 fix for the icon flickering back to stop.
    load_pending: bool,
}

impl PlayerState {
    pub fn new() -> Self {
        Self {
            current: None,
            status: PlaybackStatus::Stopped,
            position: Duration::ZERO,
            duration: None,
            volume: 0.8,
            queue: Queue::new(),
            load_pending: false,
        }
    }

    pub fn play_now(&mut self, tracks: Vec<Track>, start: usize) {
        if tracks.is_empty() {
            return;
        }
        self.queue.play_now(tracks, start);
        self.current = self.queue.current().cloned();
        self.status = PlaybackStatus::Playing;
        self.load_pending = true;
        self.position = Duration::ZERO;
        self.duration = self.current.as_ref().map(|t| t.duration);
    }

    /// Jump to an item already in the queue. Used by the queue panel.
    pub fn play_at(&mut self, index: usize) -> Option<Track> {
        let track = self.queue.set_current(index)?;
        self.current = Some(track.clone());
        self.status = PlaybackStatus::Playing;
        self.load_pending = true;
        self.position = Duration::ZERO;
        self.duration = Some(track.duration);
        Some(track)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Track> {
        let t = self.queue.next()?;
        self.current = Some(t.clone());
        self.status = PlaybackStatus::Playing;
        self.load_pending = true;
        self.position = Duration::ZERO;
        self.duration = Some(t.duration);
        Some(t)
    }

    /// Move playback onto `track` without touching the queue. Used when the
    /// autoplay radio appends tracks after the queue ran out and playback has
    /// to resume on the first of them.
    pub fn adopt(&mut self, track: Track) {
        self.current = Some(track.clone());
        self.status = PlaybackStatus::Playing;
        self.load_pending = true;
        self.position = Duration::ZERO;
        self.duration = Some(track.duration);
    }

    pub fn previous(&mut self) -> Option<Track> {
        let t = self.queue.previous()?;
        self.current = Some(t.clone());
        self.status = PlaybackStatus::Playing;
        self.load_pending = true;
        self.position = Duration::ZERO;
        self.duration = Some(t.duration);
        Some(t)
    }

    /// Flip between [`Playing`](PlaybackStatus::Playing) and
    /// [`Paused`](PlaybackStatus::Paused).
    ///
    /// Every other status is left untouched on purpose:
    /// - `Stopped` has nothing playing to pause;
    /// - `Buffering` is decided by the engine, which reports the resumed state
    ///   once the buffer is full — flipping it here would claim a state the
    ///   pipeline is not in;
    /// - `Error` means the pipeline failed and cannot be resumed by a toggle;
    ///   recovery is a fresh `Load` (see [`set_error`](Self::set_error)).
    pub fn toggle(&mut self) {
        match self.status {
            PlaybackStatus::Playing => self.status = PlaybackStatus::Paused,
            PlaybackStatus::Paused => self.status = PlaybackStatus::Playing,
            _ => {}
        }
    }

    /// Set the status from a source that is authoritative by construction: the
    /// user's own transport action, or the queue running out. It is *not*
    /// filtered, so a stop the user asked for is never swallowed.
    ///
    /// A terminal status (`Playing`, `Stopped`, `Error`) also ends a pending
    /// load: whatever the engine still has to say about the old pipeline is no
    /// longer a transient. Engine reports should go through
    /// [`set_status_from_engine`](Self::set_status_from_engine) instead.
    pub fn set_status(&mut self, status: PlaybackStatus) {
        self.status = status;
        if matches!(
            status,
            PlaybackStatus::Stopped | PlaybackStatus::Playing | PlaybackStatus::Error
        ) {
            self.load_pending = false;
        }
    }

    /// Apply a status reported by the engine, filtering the transient states
    /// (audit FL-13).
    ///
    /// While a load is pending, `Stopped` is the `Ready -> Paused` hop
    /// GStreamer goes through before it can play, not a real stop, so it is
    /// dropped — otherwise the icon flickers to stop right after a track is
    /// picked. `Playing` is the confirmation that ends the pending load, and
    /// `Error` ends it too because the pipeline is unusable. `Paused` and
    /// `Buffering` are applied as reported and leave the load pending.
    pub fn set_status_from_engine(&mut self, status: PlaybackStatus) {
        if self.load_pending {
            match status {
                PlaybackStatus::Stopped => return,
                PlaybackStatus::Playing | PlaybackStatus::Error => self.load_pending = false,
                PlaybackStatus::Paused | PlaybackStatus::Buffering => {}
            }
        }
        self.status = status;
    }

    /// True while a `Load` is in flight: `set_status_from_engine` drops the
    /// transient `Stopped` until the engine confirms a state.
    pub fn is_load_pending(&self) -> bool {
        self.load_pending
    }

    /// Enter [`PlaybackStatus::Error`] after the engine reported a failure
    /// (audit FL-07). The current track is kept so the UI can name what failed;
    /// any pending load is over, so a later transient `Stopped` is no longer
    /// filtered. The only recovery is a new `Load` (the shell's retry or the
    /// next track).
    pub fn set_error(&mut self) {
        self.set_status(PlaybackStatus::Error);
    }

    /// Apply a GStreamer buffering percentage (audit FL-12).
    ///
    /// Below 100 % the status becomes [`Buffering`](PlaybackStatus::Buffering) so
    /// the UI can show why the position stopped moving. At 100 % the indicator
    /// this method itself put up is cleared back to
    /// [`Playing`](PlaybackStatus::Playing); any other status is left alone,
    /// because only the engine knows the real pipeline state — it reports the
    /// resumed state with `StateChanged`.
    pub fn set_buffering(&mut self, percent: u8) {
        match status_for_buffering(percent) {
            Some(status) => self.status = status,
            None if self.status == PlaybackStatus::Buffering => {
                self.status = PlaybackStatus::Playing;
            }
            None => {}
        }
    }

    pub fn set_position(&mut self, pos: Duration) {
        self.position = pos;
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.queue.set_repeat(mode);
    }

    pub fn set_shuffle(&mut self, on: bool) {
        self.queue.set_shuffle(on);
    }
}

impl Default for PlayerState {
    fn default() -> Self {
        Self::new()
    }
}
