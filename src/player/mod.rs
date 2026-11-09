pub mod queue;
pub mod scrobble;
pub mod state;

#[cfg(feature = "audio")]
pub mod engine;
#[cfg(feature = "audio")]
pub use engine::{PlayerCommand, PlayerError, PlayerEvent, PlayerHandle, spawn_player};

pub use queue::{Queue, RepeatMode};
pub use scrobble::{ScrobbleTracker, scrobble_threshold, should_submit};
pub use state::{PlaybackStatus, PlayerState};
