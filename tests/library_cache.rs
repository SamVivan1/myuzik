//! Disk-cache behaviour for the offline library snapshot.
//!
//! The app keeps the whole library in one `library.json` and searches it in
//! memory, so the file has to survive save/load unchanged, reject anything it
//! cannot trust, and never be left half-written.

use myuzik::library::cache::{LibraryCache, LibrarySnapshot, SNAPSHOT_VERSION};
use myuzik::library::{Album, Artist, Playlist, Track, TrackSource};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

/// A throwaway directory that cleans itself up, without a tempfile dependency.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let unique = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "myuzik-library-cache-{}-{unique}-{label}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn cache(&self) -> LibraryCache {
        LibraryCache::new(self.path.clone())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn track(id: &str) -> Track {
    Track {
        id: id.to_string(),
        title: format!("Title {id}"),
        artist: format!("Artist {id}"),
        artist_id: Some(format!("artist-{id}")),
        album: format!("Album {id}"),
        album_id: Some(format!("album-{id}")),
        cover_art_id: Some(format!("cover-{id}")),
        duration: Duration::from_secs(215),
        track_number: Some(7),
        disc_number: Some(2),
        year: Some(1998),
        starred: true,
        genre: Some(format!("Genre {id}")),
        source: Default::default(),
        cover_url: None,
    }
}

fn album(id: &str) -> Album {
    Album {
        id: id.to_string(),
        name: format!("Album {id}"),
        artist: format!("Artist {id}"),
        artist_id: Some(format!("artist-{id}")),
        cover_art_id: Some(format!("cover-{id}")),
        song_count: Some(11),
        duration: Some(Duration::from_secs(2_580)),
        year: Some(2001),
        starred: false,
        source: TrackSource::Navidrome,
        cover_url: None,
    }
}

fn artist(id: &str) -> Artist {
    Artist {
        id: id.to_string(),
        name: format!("Artist {id}"),
        album_count: Some(4),
        cover_art_id: Some(format!("artist-cover-{id}")),
        source: TrackSource::Navidrome,
        cover_url: None,
    }
}

fn playlist(id: &str) -> Playlist {
    Playlist {
        id: id.to_string(),
        name: format!("Playlist {id}"),
        song_count: 42,
        duration: Duration::from_secs(9_000),
        owner: format!("owner-{id}"),
        source: TrackSource::Navidrome,
        cover_url: None,
    }
}

/// A snapshot that exercises every field, both `Some` and `None`.
fn full_snapshot(fetched_at: u64) -> LibrarySnapshot {
    let mut bare = track("bare");
    bare.artist_id = None;
    bare.album_id = None;
    bare.cover_art_id = None;
    bare.track_number = None;
    bare.disc_number = None;
    bare.year = None;
    bare.starred = false;
    bare.genre = None;
    bare.duration = Duration::from_millis(1_234);

    let mut sparse_album = album("sparse");
    sparse_album.artist_id = None;
    sparse_album.cover_art_id = None;
    sparse_album.song_count = None;
    sparse_album.duration = None;
    sparse_album.year = None;

    let mut sparse_artist = artist("sparse");
    sparse_artist.album_count = None;
    sparse_artist.cover_art_id = None;

    LibrarySnapshot {
        version: SNAPSHOT_VERSION,
        fetched_at,
        albums: vec![album("a1"), sparse_album],
        artists: vec![artist("ar1"), sparse_artist],
        playlists: vec![playlist("p1")],
        songs: vec![track("s1"), bare],
    }
}

fn write_file(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).expect("write fixture file");
}

fn directory_entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read temp dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn a_snapshot_round_trips_through_save_and_load_with_every_field_intact() {
    let dir = TempDir::new("round-trip");
    let cache = dir.cache();
    let snapshot = full_snapshot(1_700_000_000);

    cache.save(&snapshot).expect("save snapshot");
    let loaded = cache
        .load()
        .expect("load snapshot")
        .expect("snapshot exists");

    assert_eq!(loaded, snapshot, "snapshot must survive a disk round trip");
    assert_eq!(loaded.version, SNAPSHOT_VERSION);
    assert_eq!(loaded.fetched_at, 1_700_000_000);
    assert_eq!(loaded.albums.len(), 2);
    assert_eq!(loaded.artists.len(), 2);
    assert_eq!(loaded.playlists.len(), 1);
    assert_eq!(loaded.songs.len(), 2);
    // Sub-second durations and the `None` fields are the easy things to lose.
    assert_eq!(loaded.songs[1].duration, Duration::from_millis(1_234));
    assert_eq!(loaded.songs[1].artist_id, None);
    assert_eq!(loaded.songs[1].genre, None);
    assert_eq!(loaded.songs[0].duration, Duration::from_secs(215));
    assert_eq!(loaded.albums[1].duration, None);
    assert_eq!(loaded.artists[1].cover_art_id, None);
    assert_eq!(loaded.playlists[0].duration, Duration::from_secs(9_000));
}

#[test]
fn loading_a_missing_file_is_ok_none() {
    let dir = TempDir::new("missing");
    let cache = dir.cache();

    assert!(!cache.path().exists(), "precondition: nothing on disk yet");
    assert_eq!(cache.load().expect("missing file is not an error"), None);
}

#[test]
fn a_corrupt_json_file_returns_err_and_is_left_in_place() {
    let dir = TempDir::new("corrupt");
    let cache = dir.cache();
    write_file(&cache.path(), br#"{"version": 1, "albums": [ oops"#);

    let error = cache.load().expect_err("corrupt JSON must be rejected");
    let message = format!("{error}");
    assert!(
        message.contains("library.json"),
        "error should name the file, got: {message}"
    );
    assert!(
        cache.path().exists(),
        "a rejected cache file must not be deleted"
    );
}

#[test]
fn a_file_that_is_not_utf8_returns_err_and_is_left_in_place() {
    let dir = TempDir::new("not-utf8");
    let cache = dir.cache();
    write_file(&cache.path(), &[0xff, 0xfe, 0x00, 0x9f]);

    assert!(cache.load().is_err(), "invalid UTF-8 must be rejected");
    assert!(cache.path().exists(), "rejected file must be left in place");
}

#[test]
fn a_directory_where_the_snapshot_should_be_is_an_error_not_a_panic() {
    let dir = TempDir::new("dir-instead-of-file");
    let cache = dir.cache();
    std::fs::create_dir(cache.path()).expect("create a directory at the snapshot path");

    let error = cache.load().expect_err("a directory cannot be a snapshot");
    let message = format!("{error}");
    assert!(
        message.contains("library.json"),
        "error should name the path, got: {message}"
    );
    assert!(cache.path().is_dir(), "the directory must be left alone");
}

#[test]
fn a_snapshot_from_a_different_version_returns_err_and_is_not_deleted() {
    let dir = TempDir::new("version");
    let cache = dir.cache();
    let mut future = full_snapshot(1_700_000_000);
    future.version = SNAPSHOT_VERSION + 1;
    cache.save(&future).expect("save future-version snapshot");

    let error = cache
        .load()
        .expect_err("a snapshot from another version must be rejected");
    let message = format!("{error}");
    assert!(
        message.contains(&future.version.to_string()),
        "error should name the unsupported version, got: {message}"
    );
    assert!(
        cache.path().exists(),
        "a version mismatch must not delete the file"
    );

    // A file too old/odd to even have the fields is rejected the same way.
    write_file(&cache.path(), br#"{"version": 2, "recent": []}"#);
    assert!(
        cache.load().is_err(),
        "a differently shaped snapshot must be rejected"
    );
    assert!(cache.path().exists(), "rejected file must stay on disk");
}

#[test]
fn saving_is_atomic_and_leaves_no_tmp_file_behind() {
    let dir = TempDir::new("atomic");
    let cache = dir.cache();

    cache.save(&full_snapshot(1)).expect("first save");
    assert_eq!(
        directory_entries(dir.path()),
        vec!["library.json".to_string()],
        "one save must leave exactly the snapshot file, no .tmp"
    );

    cache.save(&full_snapshot(2)).expect("second save");
    let entries = directory_entries(dir.path());
    assert_eq!(entries, vec!["library.json".to_string()]);
    assert!(
        entries.iter().all(|name| !name.ends_with(".tmp")),
        "no temporary file may survive a save, got {entries:?}"
    );
    assert!(!dir.path().join("library.json.tmp").exists());
}

/// The promise of the temp-then-rename write: while a save is in progress the
/// file on disk is always a complete snapshot, so the only thing a crash can
/// lose is the newest save.
#[test]
fn a_reader_never_sees_a_partial_snapshot_while_saves_are_running() {
    let dir = TempDir::new("concurrent-reader");
    let cache = dir.cache();
    cache.save(&full_snapshot(0)).expect("seed the snapshot");

    let reader_cache = cache.clone();
    let reader = std::thread::spawn(move || {
        let mut reads = 0usize;
        let until = Instant::now() + Duration::from_millis(600);
        while Instant::now() < until {
            let snapshot = reader_cache
                .load()
                .expect("a reader must never see a file that is mid-write")
                .expect("the snapshot file is never removed during a save");
            assert_eq!(
                snapshot.song_count(),
                2,
                "a reader saw a half-written snapshot"
            );
            reads += 1;
        }
        reads
    });

    let mut saves = 0usize;
    let until = Instant::now() + Duration::from_millis(500);
    while Instant::now() < until {
        cache.save(&full_snapshot(saves as u64)).expect("save");
        saves += 1;
    }
    let reads = reader.join().expect("reader thread");

    assert!(saves > 0, "the test must actually write something");
    assert!(reads > 0, "the reader must actually read something");
}

#[test]
fn a_stale_tmp_file_from_a_crashed_save_is_replaced() {
    let dir = TempDir::new("stale-tmp");
    let cache = dir.cache();
    // A previous run crashed after writing the temp file, before the rename.
    write_file(
        &dir.path().join("library.json.tmp"),
        b"half written garbage",
    );

    let snapshot = full_snapshot(42);
    cache.save(&snapshot).expect("save over a crash leftover");

    assert_eq!(
        directory_entries(dir.path()),
        vec!["library.json".to_string()],
        "the stale temp file must be gone after a successful save"
    );
    assert_eq!(
        cache.load().expect("load").expect("snapshot exists"),
        snapshot
    );
}

#[test]
fn a_failed_save_leaves_the_previous_snapshot_intact() {
    let dir = TempDir::new("failed-save");
    let cache = dir.cache();
    let snapshot = full_snapshot(7);
    cache.save(&snapshot).expect("save the good snapshot");

    // Block the temporary path with a directory so the temp write must fail.
    let blocked_tmp = dir.path().join("library.json.tmp");
    std::fs::create_dir(&blocked_tmp).expect("block the temp path");

    let result = cache.save(&full_snapshot(8));
    assert!(result.is_err(), "a blocked temp write must be an error");

    assert_eq!(
        cache.load().expect("load").expect("snapshot exists"),
        snapshot,
        "a failed save must not damage the snapshot already on disk"
    );

    std::fs::remove_dir(&blocked_tmp).expect("unblock the temp path");
    assert_eq!(
        directory_entries(dir.path()),
        vec!["library.json".to_string()],
        "a failed save must not leave junk behind"
    );
}

#[test]
fn saving_creates_the_directory_when_it_is_missing() {
    let dir = TempDir::new("create-dir");
    let nested = dir.path().join("nested").join("library");
    let cache = LibraryCache::new(nested.clone());

    cache.save(&full_snapshot(3)).expect("save into a new dir");

    assert!(nested.is_dir(), "save must create the directory");
    assert_eq!(
        cache.load().expect("load").expect("snapshot exists"),
        full_snapshot(3)
    );
}

#[test]
fn saving_over_an_existing_snapshot_replaces_it_completely() {
    let dir = TempDir::new("replace");
    let cache = dir.cache();
    let first = full_snapshot(1_000);
    cache.save(&first).expect("save first snapshot");

    let mut replacement = LibrarySnapshot::empty();
    replacement.fetched_at = 2_000;
    replacement.albums = vec![album("only")];
    cache.save(&replacement).expect("save replacement");

    let loaded = cache
        .load()
        .expect("load replacement")
        .expect("snapshot exists");
    assert_eq!(
        loaded, replacement,
        "second save must not merge with the first"
    );
    assert_eq!(loaded.songs.len(), 0);
    assert_eq!(loaded.artists.len(), 0);
    assert_eq!(loaded.playlists.len(), 0);
    assert_eq!(loaded.albums.len(), 1);
    assert_eq!(loaded.albums[0].id, "only");
}

#[test]
fn empty_and_song_count_describe_the_snapshot_contents() {
    let empty = LibrarySnapshot::empty();
    assert_eq!(empty.version, SNAPSHOT_VERSION);
    assert!(empty.is_empty());
    assert_eq!(empty.song_count(), 0);

    let snapshot = full_snapshot(5);
    assert!(!snapshot.is_empty());
    assert_eq!(snapshot.song_count(), 2);

    let mut albums_only = LibrarySnapshot::empty();
    albums_only.albums.push(album("a"));
    assert!(!albums_only.is_empty(), "albums alone are not empty");
    assert_eq!(albums_only.song_count(), 0);

    let mut songs_only = LibrarySnapshot::empty();
    songs_only.songs.push(track("s"));
    assert!(!songs_only.is_empty());
    assert_eq!(songs_only.song_count(), 1);
}

#[test]
fn age_seconds_saturates_at_zero_for_a_snapshot_from_the_future() {
    let mut snapshot = LibrarySnapshot::empty();
    snapshot.fetched_at = 1_000;

    assert_eq!(snapshot.age_seconds(1_000), 0);
    assert_eq!(snapshot.age_seconds(1_060), 60);
    assert_eq!(
        snapshot.age_seconds(999),
        0,
        "a clock that moved backwards must not underflow"
    );
    assert_eq!(snapshot.age_seconds(0), 0);
}

#[test]
fn is_stale_follows_max_age_exactly_at_the_boundary() {
    let mut snapshot = LibrarySnapshot::empty();
    snapshot.fetched_at = 1_000;
    let max_age = Duration::from_secs(300);

    assert!(!snapshot.is_stale(1_000, max_age), "brand new is fresh");
    assert!(
        !snapshot.is_stale(1_299, max_age),
        "one second before the boundary is still fresh"
    );
    assert!(
        snapshot.is_stale(1_300, max_age),
        "exactly at max_age the snapshot is stale"
    );
    assert!(snapshot.is_stale(1_301, max_age));
    assert!(snapshot.is_stale(4_000, max_age));

    // Sub-second max_age compares against the exact age, not a truncated one.
    assert!(!snapshot.is_stale(1_000, Duration::from_millis(1_500)));
    assert!(!snapshot.is_stale(1_001, Duration::from_millis(1_500)));
    assert!(snapshot.is_stale(1_002, Duration::from_millis(1_500)));
    // A zero max_age is stale immediately.
    assert!(snapshot.is_stale(1_000, Duration::ZERO));
}

#[test]
fn clear_removes_the_snapshot_and_is_idempotent() {
    let dir = TempDir::new("clear");
    let cache = dir.cache();
    cache.save(&full_snapshot(1)).expect("save snapshot");

    cache.clear().expect("clear existing snapshot");
    assert!(!cache.path().exists());
    assert_eq!(cache.load().expect("load after clear"), None);

    cache
        .clear()
        .expect("clearing a missing snapshot is not an error");
}

#[test]
fn the_snapshot_lives_at_library_json_in_the_given_directory() {
    let dir = TempDir::new("path");
    let cache = LibraryCache::new(dir.path().to_path_buf());

    assert_eq!(cache.path(), dir.path().join("library.json"));
}

#[test]
fn a_snapshot_with_twenty_thousand_songs_round_trips() {
    let dir = TempDir::new("large");
    let cache = dir.cache();

    let songs: Vec<Track> = (0..20_000)
        .map(|i| {
            let mut song = track(&format!("song-{i}"));
            song.title = format!("Large library song {i}");
            song
        })
        .collect();
    let snapshot = LibrarySnapshot {
        version: SNAPSHOT_VERSION,
        fetched_at: 1_700_000_000,
        albums: (0..500).map(|i| album(&format!("big-{i}"))).collect(),
        artists: (0..100).map(|i| artist(&format!("big-{i}"))).collect(),
        playlists: (0..20).map(|i| playlist(&format!("big-{i}"))).collect(),
        songs,
    };

    cache.save(&snapshot).expect("save large snapshot");
    let loaded = cache
        .load()
        .expect("load large snapshot")
        .expect("snapshot exists");

    assert_eq!(loaded.song_count(), 20_000);
    assert_eq!(loaded.songs[0].id, "song-0");
    assert_eq!(loaded.songs[19_999].id, "song-19999");
    assert_eq!(loaded.albums.len(), 500);
    assert_eq!(loaded.artists.len(), 100);
    assert_eq!(loaded.playlists.len(), 20);
}
