//! Contract tests for the source registry. The point is that the UI-facing
//! layer can dispatch playback/cover calls without knowing the provider.

use myuzik::library::{
    AlbumDetail, ArtistDetail, PlaylistDetail, SearchResults, Track, TrackSource,
};
use myuzik::services::{
    Capabilities, CoverSource, MusicService, PlaybackSource, ServiceError, SourceRegistry,
};
use std::sync::atomic::AtomicBool;

struct FakeYouTube;

impl MusicService for FakeYouTube {
    fn source(&self) -> TrackSource {
        TrackSource::YouTube
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            browse: false,
            search: true,
            radio: true,
            scrobble: false,
            starred: false,
        }
    }

    fn search(
        &self,
        _query: &str,
        _limit: usize,
        _cancel: &AtomicBool,
    ) -> Result<SearchResults, ServiceError> {
        Ok(SearchResults::default())
    }

    fn album(&self, _id: &str) -> Result<AlbumDetail, ServiceError> {
        Err(ServiceError::Unsupported("fake"))
    }

    fn artist(&self, _id: &str) -> Result<ArtistDetail, ServiceError> {
        Err(ServiceError::Unsupported("fake"))
    }

    fn playlist(&self, _id: &str) -> Result<PlaylistDetail, ServiceError> {
        Err(ServiceError::Unsupported("fake"))
    }

    fn radio(
        &self,
        _seed: &Track,
        _limit: usize,
        _cancel: &AtomicBool,
    ) -> Result<Vec<Track>, ServiceError> {
        Ok(Vec::new())
    }

    fn resolve(
        &self,
        _track: &Track,
        _cancel: &AtomicBool,
    ) -> Result<PlaybackSource, ServiceError> {
        Ok(PlaybackSource::Url("file:///tmp/fake".to_string()))
    }

    fn cover(&self, _track: &Track, _size: u32) -> Result<CoverSource, ServiceError> {
        Ok(CoverSource::Url(
            "https://example.com/cover.jpg".to_string(),
        ))
    }

    fn set_starred(&self, _id: &str, _starred: bool) -> Result<(), ServiceError> {
        Err(ServiceError::Unsupported("fake"))
    }

    fn scrobble(
        &self,
        _track_id: &str,
        _submission: bool,
        _timestamp_ms: i64,
    ) -> Result<(), ServiceError> {
        Err(ServiceError::Unsupported("fake"))
    }
}

fn youtube_track() -> Track {
    Track {
        id: "yt:abcdefghijk".to_string(),
        title: "Song".to_string(),
        artist: "Artist".to_string(),
        artist_id: None,
        album: "Album".to_string(),
        album_id: None,
        cover_art_id: None,
        cover_url: Some("https://example.com/cover.jpg".to_string()),
        duration: std::time::Duration::from_secs(120),
        track_number: None,
        disc_number: None,
        year: None,
        starred: false,
        genre: None,
        source: TrackSource::YouTube,
    }
}

#[test]
fn registry_dispatches_by_track_source() {
    let registry = SourceRegistry::new(None, Some(std::sync::Arc::new(FakeYouTube)));
    let track = youtube_track();
    let cancel = AtomicBool::new(false);

    assert_eq!(registry.enabled(), vec![TrackSource::YouTube]);
    assert!(matches!(
        registry.resolve(&track, &cancel).expect("resolve"),
        PlaybackSource::Url(_)
    ));
    assert!(matches!(
        registry.cover(&track, 512).expect("cover"),
        CoverSource::Url(_)
    ));
}

#[test]
fn disabled_source_is_reported() {
    let registry = SourceRegistry::new(None, None);
    let track = youtube_track();
    let cancel = AtomicBool::new(false);
    assert!(matches!(
        registry.resolve(&track, &cancel),
        Err(ServiceError::Disabled)
    ));
}
