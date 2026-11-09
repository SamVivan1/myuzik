use crate::library::Track;
use crate::player::queue::{Queue, RepeatMode};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Stopped,
    Playing,
    Paused,
    Buffering,
    Error,
}

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub current: Option<Track>,
    pub status: PlaybackStatus,
    pub position: Duration,
    pub duration: Option<Duration>,
    pub volume: f32,
    pub queue: Queue,
    pub scrobble_submitted: bool,
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
            scrobble_submitted: false,
        }
    }

    pub fn play_now(&mut self, tracks: Vec<Track>, start: usize) {
        if tracks.is_empty() {
            return;
        }
        self.queue.play_now(tracks, start);
        self.current = self.queue.current().cloned();
        self.status = PlaybackStatus::Playing;
        self.position = Duration::ZERO;
        self.duration = self.current.as_ref().map(|t| t.duration);
        self.scrobble_submitted = false;
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Track> {
        let t = self.queue.next()?;
        self.current = Some(t.clone());
        self.status = PlaybackStatus::Playing;
        self.position = Duration::ZERO;
        self.duration = Some(t.duration);
        self.scrobble_submitted = false;
        Some(t)
    }

    pub fn previous(&mut self) -> Option<Track> {
        let t = self.queue.previous()?;
        self.current = Some(t.clone());
        self.status = PlaybackStatus::Playing;
        self.position = Duration::ZERO;
        self.duration = Some(t.duration);
        self.scrobble_submitted = false;
        Some(t)
    }

    pub fn toggle(&mut self) {
        match self.status {
            PlaybackStatus::Playing => self.status = PlaybackStatus::Paused,
            PlaybackStatus::Paused => self.status = PlaybackStatus::Playing,
            _ => {}
        }
    }

    pub fn set_status(&mut self, status: PlaybackStatus) {
        self.status = status;
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
