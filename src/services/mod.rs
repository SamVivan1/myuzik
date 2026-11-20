//! Stable backend contract for the UI.
//!
//! Screens should ask a [`MusicService`] to search, browse, resolve playback and
//! fetch covers. They must not know whether the bytes come from Navidrome or a
//! one-shot YouTube helper. [`SourceRegistry`] dispatches by [`TrackSource`], so
//! a design overhaul can replace the screens without touching providers.

use crate::config::StreamQuality;
use crate::library::{
    AlbumDetail, ArtistDetail, Playlist, PlaylistDetail, SearchResults, Track, TrackSource,
};
use crate::navidrome::client::NavidromeClient;
use crate::navidrome::error::NavidromeError;
use crate::provider::youtube::{BrowseSection, MoodCategory};
use crate::provider::{ProviderError, YouTubeProvider};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error(transparent)]
    Navidrome(#[from] NavidromeError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("{0}")]
    Unsupported(&'static str),
    #[error("This source is disabled")]
    Disabled,
}

/// Where the player should load audio from. Navidrome hands over an HTTP URL;
/// YouTube downloads to a local buffer first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackSource {
    Url(String),
    LocalFile(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverSource {
    Bytes(Vec<u8>),
    Url(String),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Capabilities {
    pub browse: bool,
    pub search: bool,
    pub radio: bool,
    pub scrobble: bool,
    pub starred: bool,
}

/// A source the UI can talk to. Methods are blocking; call them from the
/// background executor, never from `render` or the UI thread.
pub trait MusicService: Send + Sync {
    fn source(&self) -> TrackSource;
    fn capabilities(&self) -> Capabilities;

    fn search(
        &self,
        query: &str,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<SearchResults, ServiceError>;
    fn album(&self, id: &str) -> Result<AlbumDetail, ServiceError>;
    fn artist(&self, id: &str) -> Result<ArtistDetail, ServiceError>;
    fn playlist(&self, id: &str) -> Result<PlaylistDetail, ServiceError>;
    fn radio(
        &self,
        seed: &Track,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Vec<Track>, ServiceError>;
    /// The source's front page, when it has one. Navidrome leaves this to the
    /// library snapshot fetcher; YouTube returns personalised shelves.
    fn home(
        &self,
        _limit: usize,
        _cancel: &AtomicBool,
    ) -> Result<Vec<BrowseSection>, ServiceError> {
        Err(ServiceError::Unsupported("this source has no home feed"))
    }
    /// Mood and genre categories, when the source exposes them.
    fn moods(&self, _cancel: &AtomicBool) -> Result<Vec<MoodCategory>, ServiceError> {
        Err(ServiceError::Unsupported("this source has no moods"))
    }
    fn mood_playlists(
        &self,
        _params: &str,
        _cancel: &AtomicBool,
    ) -> Result<Vec<Playlist>, ServiceError> {
        Err(ServiceError::Unsupported("this source has no moods"))
    }
    fn resolve(&self, track: &Track, cancel: &AtomicBool) -> Result<PlaybackSource, ServiceError>;
    fn cover(&self, track: &Track, size: u32) -> Result<CoverSource, ServiceError>;
    /// Cover by a provider-native cover id (Navidrome's `coverArt`).
    fn cover_id(&self, _cover_id: &str, _size: u32) -> Result<CoverSource, ServiceError> {
        Err(ServiceError::Unsupported(
            "cover ids are not supported by this source",
        ))
    }
    fn set_starred(&self, id: &str, starred: bool) -> Result<(), ServiceError>;
    fn scrobble(
        &self,
        track_id: &str,
        submission: bool,
        timestamp_ms: i64,
    ) -> Result<(), ServiceError>;
}

/// The current Navidrome implementation. Kept as a thin adapter so the UI
/// contract survives if the Subsonic client is rewritten.
pub struct NavidromeService {
    client: NavidromeClient,
    quality: StreamQuality,
}

impl NavidromeService {
    pub fn new(client: NavidromeClient, quality: StreamQuality) -> Self {
        Self { client, quality }
    }
}

impl MusicService for NavidromeService {
    fn source(&self) -> TrackSource {
        TrackSource::Navidrome
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            browse: true,
            search: true,
            radio: true,
            scrobble: true,
            starred: true,
        }
    }

    fn search(
        &self,
        query: &str,
        _limit: usize,
        _cancel: &AtomicBool,
    ) -> Result<SearchResults, ServiceError> {
        Ok(self.client.search(query)?)
    }

    fn album(&self, id: &str) -> Result<AlbumDetail, ServiceError> {
        Ok(self.client.album(id)?)
    }

    fn artist(&self, id: &str) -> Result<ArtistDetail, ServiceError> {
        Ok(self.client.artist(id)?)
    }

    fn playlist(&self, id: &str) -> Result<PlaylistDetail, ServiceError> {
        Ok(self.client.playlist(id)?)
    }

    fn radio(
        &self,
        _seed: &Track,
        _limit: usize,
        _cancel: &AtomicBool,
    ) -> Result<Vec<Track>, ServiceError> {
        // Navidrome radio is the multi-source `Radio` state machine; the
        // registry cannot express it as one call yet.
        Err(ServiceError::Unsupported(
            "Navidrome radio is handled by the radio engine",
        ))
    }

    fn resolve(&self, track: &Track, _cancel: &AtomicBool) -> Result<PlaybackSource, ServiceError> {
        Ok(PlaybackSource::Url(
            self.client.stream_url(&track.id, self.quality)?,
        ))
    }

    fn cover(&self, track: &Track, size: u32) -> Result<CoverSource, ServiceError> {
        let Some(cover_id) = track.cover_art_id.as_deref() else {
            return Err(ServiceError::Unsupported("track has no cover"));
        };
        Ok(CoverSource::Bytes(self.client.cover_bytes(cover_id, size)?))
    }

    fn cover_id(&self, cover_id: &str, size: u32) -> Result<CoverSource, ServiceError> {
        Ok(CoverSource::Bytes(self.client.cover_bytes(cover_id, size)?))
    }

    fn set_starred(&self, id: &str, starred: bool) -> Result<(), ServiceError> {
        Ok(self.client.set_starred(id, starred)?)
    }

    fn scrobble(
        &self,
        track_id: &str,
        submission: bool,
        timestamp_ms: i64,
    ) -> Result<(), ServiceError> {
        Ok(self.client.scrobble(track_id, submission, timestamp_ms)?)
    }
}

/// YouTube Music adapter. Search, home/browse, radio, buffering and remote
/// covers all go through the same one-shot helper.
pub struct YouTubeService {
    provider: Arc<YouTubeProvider>,
}

impl YouTubeService {
    pub fn new(provider: Arc<YouTubeProvider>) -> Self {
        Self { provider }
    }
}

impl MusicService for YouTubeService {
    fn source(&self) -> TrackSource {
        TrackSource::YouTube
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            browse: true,
            search: true,
            radio: true,
            scrobble: false,
            starred: false,
        }
    }

    fn search(
        &self,
        query: &str,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<SearchResults, ServiceError> {
        let tracks = self.provider.search(query, limit, cancel)?;
        Ok(SearchResults {
            youtube: tracks,
            ..Default::default()
        })
    }

    fn album(&self, id: &str) -> Result<AlbumDetail, ServiceError> {
        Ok(self.provider.album(id, &AtomicBool::new(false))?)
    }

    fn artist(&self, id: &str) -> Result<ArtistDetail, ServiceError> {
        Ok(self.provider.artist(id, &AtomicBool::new(false))?)
    }

    fn playlist(&self, id: &str) -> Result<PlaylistDetail, ServiceError> {
        Ok(self.provider.playlist(id, &AtomicBool::new(false))?)
    }

    fn radio(
        &self,
        seed: &Track,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Vec<Track>, ServiceError> {
        Ok(self.provider.radio(seed, limit, cancel)?)
    }

    fn home(&self, limit: usize, cancel: &AtomicBool) -> Result<Vec<BrowseSection>, ServiceError> {
        Ok(self.provider.home(limit, cancel)?)
    }

    fn moods(&self, cancel: &AtomicBool) -> Result<Vec<MoodCategory>, ServiceError> {
        Ok(self.provider.moods(cancel)?)
    }

    fn mood_playlists(
        &self,
        params: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<Playlist>, ServiceError> {
        Ok(self.provider.mood_playlists(params, cancel)?)
    }

    fn resolve(&self, track: &Track, cancel: &AtomicBool) -> Result<PlaybackSource, ServiceError> {
        let buffered = self.provider.buffer(track, cancel)?;
        Ok(PlaybackSource::LocalFile(buffered.file))
    }

    fn cover(&self, track: &Track, _size: u32) -> Result<CoverSource, ServiceError> {
        track
            .cover_url
            .clone()
            .map(CoverSource::Url)
            .ok_or(ServiceError::Unsupported("track has no cover URL"))
    }

    fn set_starred(&self, _id: &str, _starred: bool) -> Result<(), ServiceError> {
        Err(ServiceError::Unsupported(
            "YouTube likes are not implemented",
        ))
    }

    fn scrobble(
        &self,
        _track_id: &str,
        _submission: bool,
        _timestamp_ms: i64,
    ) -> Result<(), ServiceError> {
        Err(ServiceError::Unsupported(
            "YouTube scrobbling is not implemented",
        ))
    }
}

/// Dispatches calls to the enabled source. One registry is enough for the
/// whole app; adding Audius later is one more entry, not a UI change.
pub struct SourceRegistry {
    navidrome: Option<Arc<dyn MusicService>>,
    youtube: Option<Arc<dyn MusicService>>,
}

impl SourceRegistry {
    pub fn new(
        navidrome: Option<Arc<dyn MusicService>>,
        youtube: Option<Arc<dyn MusicService>>,
    ) -> Self {
        Self { navidrome, youtube }
    }

    pub fn service(&self, source: TrackSource) -> Option<&Arc<dyn MusicService>> {
        match source {
            TrackSource::Navidrome => self.navidrome.as_ref(),
            TrackSource::YouTube => self.youtube.as_ref(),
        }
    }

    pub fn enabled(&self) -> Vec<TrackSource> {
        let mut out = Vec::new();
        if self.navidrome.is_some() {
            out.push(TrackSource::Navidrome);
        }
        if self.youtube.is_some() {
            out.push(TrackSource::YouTube);
        }
        out
    }

    pub fn resolve(
        &self,
        track: &Track,
        cancel: &AtomicBool,
    ) -> Result<PlaybackSource, ServiceError> {
        self.service(track.source)
            .ok_or(ServiceError::Disabled)?
            .resolve(track, cancel)
    }

    pub fn cover(&self, track: &Track, size: u32) -> Result<CoverSource, ServiceError> {
        self.service(track.source)
            .ok_or(ServiceError::Disabled)?
            .cover(track, size)
    }

    pub fn search(
        &self,
        source: TrackSource,
        query: &str,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<SearchResults, ServiceError> {
        self.service(source)
            .ok_or(ServiceError::Disabled)?
            .search(query, limit, cancel)
    }
}
