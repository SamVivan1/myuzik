//! Application routing and action state.
//!
//! The running UI lives in [`crate::ui::shell::Shell`]; this module holds the
//! route enum and the small action vocabulary the UI uses to ask the shell to
//! change app state. Keeping actions here (rather than calling services from a
//! render function) is what lets the screens be replaced by a design overhaul
//! without touching playback, library or provider code.

use crate::library::TrackSource;

/// A provider-qualified entity id. Two sources can use the same id space, so
/// routes carry the source rather than guessing from the id string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceId {
    pub source: TrackSource,
    pub id: String,
}

impl SourceId {
    pub fn new(source: TrackSource, id: impl Into<String>) -> Self {
        Self {
            source,
            id: id.into(),
        }
    }
}

/// Pick the source to browse at startup. The saved choice wins when it is
/// available; otherwise fall back to the other configured source. `None` means
/// neither source is configured and the Setup screen should open.
pub fn resolve_active_source(
    preferred: TrackSource,
    navidrome_available: bool,
    youtube_available: bool,
) -> Option<TrackSource> {
    match preferred {
        TrackSource::Navidrome if navidrome_available => Some(TrackSource::Navidrome),
        TrackSource::YouTube if youtube_available => Some(TrackSource::YouTube),
        TrackSource::Navidrome if youtube_available => Some(TrackSource::YouTube),
        TrackSource::YouTube if navidrome_available => Some(TrackSource::Navidrome),
        _ => None,
    }
}

/// A top-level destination in the main window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Setup,
    Home,
    Search,
    Albums,
    Artists,
    Playlists,
    Favorites,
    Album(SourceId),
    Artist(SourceId),
    Playlist(SourceId),
    Moods,
    Shelf,
    NowPlaying,
    Settings,
}

/// Queue commands issued by the queue panel or a track-row action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueAction {
    TogglePanel,
    PlayAt(usize),
    Remove(usize),
    Move { from: usize, to: usize },
    ClearUpcoming,
}

/// UI intent. Screens send these; the shell owns the state transition and the
/// service call. More variants will be added as screens move out of the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAction {
    Queue(QueueAction),
    OpenAlbum(SourceId),
    OpenArtist(SourceId),
    OpenPlaylist(SourceId),
    OpenMoods,
    RefreshYouTubeHome,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_source_is_used_when_it_is_available() {
        assert_eq!(
            resolve_active_source(TrackSource::YouTube, true, true),
            Some(TrackSource::YouTube)
        );
        assert_eq!(
            resolve_active_source(TrackSource::Navidrome, true, true),
            Some(TrackSource::Navidrome)
        );
    }

    #[test]
    fn an_unavailable_saved_source_falls_back_to_the_other_one() {
        assert_eq!(
            resolve_active_source(TrackSource::YouTube, true, false),
            Some(TrackSource::Navidrome)
        );
        assert_eq!(
            resolve_active_source(TrackSource::Navidrome, false, true),
            Some(TrackSource::YouTube)
        );
    }

    #[test]
    fn no_configured_source_resolves_to_none() {
        assert_eq!(
            resolve_active_source(TrackSource::Navidrome, false, false),
            None
        );
        assert_eq!(
            resolve_active_source(TrackSource::YouTube, false, false),
            None
        );
    }
}
