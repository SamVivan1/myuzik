//! YouTube provider tests against a standard-library fixture helper.
//!
//! The fixture speaks the same JSON protocol as the real helper, so these tests
//! exercise the process boundary, timeout/cancel logic, path validation and
//! mapping without network access or Python packages.

use myuzik::config::YouTubeQuality;
use myuzik::library::{Track, TrackSource};
use myuzik::provider::{ProviderError, YouTubeProvider};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "myuzik-youtube-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn fixture_helper() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ytmusic_fixture.py")
}

fn provider(tag: &str) -> (YouTubeProvider, PathBuf) {
    let cache = temp_dir(tag);
    (
        YouTubeProvider::with_paths(
            fixture_helper(),
            PathBuf::from("python3"),
            cache.join("streams"),
            YouTubeQuality::Standard,
            None,
            64,
        ),
        cache,
    )
}

fn track(id: &str) -> Track {
    Track {
        id: format!("yt:{id}"),
        title: "Fixture One".to_string(),
        artist: "Fixture Artist".to_string(),
        artist_id: None,
        album: "Fixture Album".to_string(),
        album_id: None,
        cover_art_id: None,
        cover_url: Some("https://example.com/fixture.jpg".to_string()),
        duration: Duration::from_secs(120),
        track_number: None,
        disc_number: None,
        year: None,
        starred: false,
        genre: None,
        source: TrackSource::YouTube,
    }
}

#[test]
fn search_maps_fixture_tracks_to_youtube_source() {
    let (provider, _cache) = provider("search");
    let cancel = AtomicBool::new(false);
    let tracks = provider.search("lofi", 10, &cancel).expect("search");

    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].id, "yt:abcdefghijk");
    assert_eq!(tracks[0].source, TrackSource::YouTube);
    assert_eq!(tracks[0].video_id(), Some("abcdefghijk"));
    assert_eq!(tracks[0].duration, Duration::from_secs(120));
    assert_eq!(
        tracks[0].cover_url.as_deref(),
        Some("https://example.com/fixture.jpg")
    );
    assert!(
        tracks[0]
            .cover_key()
            .is_some_and(|key| key.starts_with("yt-"))
    );
}

#[test]
fn radio_maps_fixture_tracks_to_youtube_source() {
    let (provider, _cache) = provider("radio");
    let cancel = AtomicBool::new(false);
    let tracks = provider
        .radio(&track("abcdefghijk"), 10, &cancel)
        .expect("radio");
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].id, "yt:radiotrack1");
    assert_eq!(tracks[1].id, "yt:radiotrack2");
    assert!(tracks.iter().all(|track| track.is_youtube()));
}

#[test]
fn search_error_is_reported_as_unavailable() {
    let (provider, _cache) = provider("search-error");
    let cancel = AtomicBool::new(false);
    let error = provider
        .search("error", 10, &cancel)
        .expect_err("fixture should fail");
    assert!(matches!(error, ProviderError::Unavailable(_)));
}

#[test]
fn cancelled_search_returns_cancelled() {
    let (provider, _cache) = provider("cancel");
    let cancel = AtomicBool::new(true);
    let error = provider
        .search("lofi", 10, &cancel)
        .expect_err("cancel should win");
    assert!(matches!(error, ProviderError::Cancelled));
}

#[test]
fn buffer_returns_a_local_file_and_duration() {
    let (provider, _cache) = provider("buffer");
    let cancel = AtomicBool::new(false);
    let stream = provider
        .buffer(&track("abcdefghijk"), &cancel)
        .expect("buffer");

    assert!(stream.file.is_file());
    assert!(std::fs::metadata(&stream.file).expect("metadata").len() > 0);
    assert_eq!(stream.duration, Duration::from_secs(120));
    assert!(stream.file.starts_with(provider.cache_dir()));
}

#[test]
fn second_buffer_reuses_the_cache_without_a_helper_process() {
    let (provider, cache) = provider("cache-reuse");
    let log = cache.join("requests.log");
    // Tests run serially (`--test-threads=1`); the fixture appends one line per
    // request, so a second spawn would be visible here.
    unsafe {
        std::env::set_var("MYUZIK_YT_REQUEST_LOG", &log);
    }
    let cancel = AtomicBool::new(false);
    let first = provider
        .buffer(&track("abcdefghijk"), &cancel)
        .expect("first buffer");
    let second = provider
        .buffer(&track("abcdefghijk"), &cancel)
        .expect("cached buffer");
    unsafe {
        std::env::remove_var("MYUZIK_YT_REQUEST_LOG");
    }

    assert_eq!(first.file, second.file);
    let requests = std::fs::read_to_string(&log).expect("request log");
    assert_eq!(
        requests.matches("\"op\": \"buffer\"").count(),
        1,
        "the second buffer must not spawn the helper: {requests}"
    );
}

#[test]
fn buffer_rejects_a_file_outside_the_cache_directory() {
    let (provider, _cache) = provider("outside");
    let cancel = AtomicBool::new(false);
    let error = provider
        .buffer(&track("outsidefile"), &cancel)
        .expect_err("outside path must be rejected");
    assert!(matches!(error, ProviderError::Protocol(_)));
}

#[test]
fn buffer_failure_is_reported() {
    let (provider, _cache) = provider("buffer-error");
    let cancel = AtomicBool::new(false);
    let error = provider
        .buffer(&track("failfailfai"), &cancel)
        .expect_err("fixture should fail");
    assert!(matches!(error, ProviderError::Unavailable(_)));
}

#[test]
fn invalid_video_id_is_rejected_before_spawning() {
    let (provider, _cache) = provider("bad-id");
    let cancel = AtomicBool::new(false);
    let error = provider
        .buffer(&track("too-short"), &cancel)
        .expect_err("bad id");
    assert!(matches!(error, ProviderError::Protocol(_)));
}

#[test]
fn old_track_json_loads_as_a_navidrome_track() {
    let value = serde_json::json!({
        "id": "legacy-1",
        "title": "Legacy",
        "artist": "Artist",
        "artist_id": null,
        "album": "Album",
        "album_id": null,
        "cover_art_id": "cover-1",
        "duration": {"secs": 180, "nanos": 0},
        "track_number": 1,
        "disc_number": 1,
        "year": 2020,
        "starred": false,
        "genre": null
    });
    let track: Track = serde_json::from_value(value).expect("legacy track");
    assert_eq!(track.source, TrackSource::Navidrome);
    assert_eq!(track.cover_url, None);
    assert_eq!(track.cover_key().as_deref(), Some("cover-1"));
    assert_eq!(
        serde_json::to_string(&TrackSource::YouTube).expect("serialize"),
        "\"youtube\""
    );
}

#[test]
fn missing_helper_is_reported_without_a_process() {
    let cache = temp_dir("missing-helper");
    let provider = YouTubeProvider::with_paths(
        cache.join("does-not-exist.py"),
        PathBuf::from("python3"),
        cache.join("streams"),
        YouTubeQuality::Standard,
        None,
        64,
    );
    let cancel = AtomicBool::new(false);
    let error = provider.version(&cancel).expect_err("helper missing");
    assert!(matches!(error, ProviderError::HelperMissing));
}

#[test]
fn version_reports_helper_versions() {
    let (provider, _cache) = provider("version");
    let cancel = AtomicBool::new(false);
    let version = provider.version(&cancel).expect("version");
    assert_eq!(version.ytmusicapi, "test");
    assert_eq!(version.yt_dlp, "test");
    assert!(!version.python.is_empty());
}

#[test]
#[ignore = "requires ytmusicapi, yt-dlp, Node and network access"]
fn live_search_and_buffer_a_track() {
    let cache = temp_dir("live");
    let provider = YouTubeProvider::new(cache.join("streams"), YouTubeQuality::Standard, None, 128);
    if !provider.helper_present() {
        eprintln!("skipped: install the YouTube helper first");
        return;
    }
    let cancel = AtomicBool::new(false);
    if let Err(error) = provider.version(&cancel) {
        eprintln!("skipped: helper runtime is not ready: {error}");
        return;
    }

    let tracks = provider.search("lofi", 5, &cancel).expect("live search");
    assert!(!tracks.is_empty(), "live search returned no tracks");
    let track = tracks
        .iter()
        .find(|track| track.duration.as_secs() > 0 && track.duration.as_secs() <= 300)
        .unwrap_or(&tracks[0]);

    let radio = provider.radio(track, 10, &cancel).expect("live radio");
    assert!(!radio.is_empty(), "live radio returned no tracks");
    assert!(
        radio.iter().any(|candidate| candidate.id == track.id),
        "the watch playlist should contain the seed track"
    );

    let stream = provider.buffer(track, &cancel).expect("live buffer");
    assert!(stream.file.is_file());
    assert!(std::fs::metadata(&stream.file).expect("metadata").len() > 0);
    assert!(stream.duration > Duration::ZERO);
}

#[test]
fn home_maps_fixture_shelves_and_card_kinds() {
    use myuzik::provider::youtube::BrowseCard;

    let (provider, _cache) = provider("home");
    let cancel = AtomicBool::new(false);
    let sections = provider.home(10, &cancel).expect("home");
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].title, "Quick picks");
    assert!(matches!(sections[0].items[0], BrowseCard::Song(_)));
    assert_eq!(sections[1].title, "New releases");
    match &sections[1].items[0] {
        BrowseCard::Album(album) => {
            assert_eq!(album.id, "yt:MPREb_fixture");
            assert_eq!(album.name, "Fixture Album");
            assert_eq!(album.source, TrackSource::YouTube);
            assert_eq!(album.year, Some(2026));
        }
        other => panic!("expected an album card, got {other:?}"),
    }
    match &sections[1].items[1] {
        BrowseCard::Playlist(playlist) => {
            assert_eq!(playlist.id, "yt:PLfixture");
            assert_eq!(playlist.song_count, 3);
            assert_eq!(playlist.source, TrackSource::YouTube);
        }
        other => panic!("expected a playlist card, got {other:?}"),
    }
    match &sections[1].items[2] {
        BrowseCard::Artist(artist) => {
            assert_eq!(artist.id, "yt:UCfixture");
            assert_eq!(artist.name, "Fixture Artist");
        }
        other => panic!("expected an artist card, got {other:?}"),
    }
}

#[test]
fn album_artist_and_playlist_details_map_fixture_items() {
    let (provider, _cache) = provider("browse");
    let cancel = AtomicBool::new(false);

    let album = provider.album("yt:MPREb_fixture", &cancel).expect("album");
    assert_eq!(album.album.id, "yt:MPREb_fixture");
    assert_eq!(album.album.source, TrackSource::YouTube);
    assert_eq!(album.album.artist_id.as_deref(), Some("yt:UCfixture"));
    assert_eq!(album.songs.len(), 2);
    assert_eq!(album.songs[0].album_id.as_deref(), Some("yt:MPREb_fixture"));
    assert_eq!(album.album.song_count, Some(2));

    let artist = provider.artist("yt:UCfixture", &cancel).expect("artist");
    assert_eq!(artist.artist.id, "yt:UCfixture");
    assert_eq!(artist.artist.name, "Fixture Artist");
    assert_eq!(artist.albums.len(), 1);
    assert_eq!(artist.albums[0].id, "yt:MPREb_fixture");

    let playlist = provider
        .playlist("yt:PLfixture", &cancel)
        .expect("playlist");
    assert_eq!(playlist.playlist.id, "yt:PLfixture");
    assert_eq!(playlist.playlist.song_count, 3);
    assert_eq!(playlist.songs.len(), 3);
    assert_eq!(playlist.songs[0].source, TrackSource::YouTube);
}

#[test]
fn moods_and_mood_playlists_map_fixture_items() {
    let (provider, _cache) = provider("moods");
    let cancel = AtomicBool::new(false);

    let categories = provider.moods(&cancel).expect("moods");
    assert_eq!(categories.len(), 1);
    assert_eq!(categories[0].title, "Moods & moments");
    assert_eq!(categories[0].moods[0].title, "Chill");

    let playlists = provider
        .mood_playlists("fixture-params", &cancel)
        .expect("mood playlists");
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].id, "yt:PLfixture");
}

#[test]
#[ignore = "requires ytmusicapi, yt-dlp, Node and network access"]
fn live_home_album_and_moods() {
    let cache = temp_dir("live-browse");
    let provider = YouTubeProvider::new(cache.join("streams"), YouTubeQuality::Standard, None, 128);
    if !provider.helper_present() {
        eprintln!("skipped: install the YouTube helper first");
        return;
    }
    let cancel = AtomicBool::new(false);
    if let Err(error) = provider.version(&cancel) {
        eprintln!("skipped: helper runtime is not ready: {error}");
        return;
    }
    let sections = match provider.home(3, &cancel) {
        Ok(sections) => sections,
        Err(error) => {
            eprintln!("skipped: YouTube home unavailable: {error}");
            return;
        }
    };
    assert!(!sections.is_empty(), "live home returned no shelves");
    assert!(
        sections.iter().any(|section| !section.items.is_empty()),
        "live home returned no cards"
    );

    let album_id = sections
        .iter()
        .flat_map(|section| section.items.iter())
        .find_map(|card| match card {
            myuzik::provider::youtube::BrowseCard::Album(album) => Some(album.id.clone()),
            _ => None,
        });
    if let Some(id) = album_id {
        let album = provider.album(&id, &cancel).expect("live album");
        assert!(!album.songs.is_empty(), "live album returned no songs");
    }

    let categories = provider.moods(&cancel).expect("live moods");
    assert!(!categories.is_empty(), "live moods returned no categories");
}
