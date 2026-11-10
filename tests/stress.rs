//! Stress and capacity tests: a 20 000-track library (mapping and windowed paging),
//! 200 searches through the generation guard, 500 cover-cache inserts against the real
//! LRU, and a 20 000-track queue under rapid next/previous churn.
//!
//! These are headless and offline: the only HTTP server is a local `httpmock` instance
//! bound to 127.0.0.1, so no Navidrome, display, keyring or internet is needed.
//!
//! They cover plan task 24, step 3 ("20k-track library, rapid next/prev, 200 searches,
//! 500 cover loads"). A 24 h idle soak is *not* automated here (it needs wall-clock time
//! and a real window); `scripts/measure-ram.sh` runs it by hand, see `docs/performance.md`.
//!
//! The time ceilings below are deliberately generous: they must stay green on a loaded
//! CI box. They exist to catch order-of-magnitude regressions (accidental O(n^2) mapping,
//! per-request full-library scans), not to police a few milliseconds.
//!
//! Run with: cargo test --test stress -- --test-threads=1 --nocapture

use httpmock::prelude::*;
use myuzik::library::{Album, SearchResults, Track};
use myuzik::navidrome::client::{AlbumListKind, NavidromeClient};
use myuzik::navidrome::types::{AlbumDto, SongDto, album_dto_to_album, song_dto_to_track};
use myuzik::player::queue::{Queue, RepeatMode};
use myuzik::player::state::PlayerState;
use myuzik::ui::shell::{COVER_CACHE_CAP, HomeState};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};

/// Size of the synthetic library used by the library and queue tests.
const LIBRARY_SIZE: usize = 20_000;
/// Albums per `getAlbumList2` window. Subsonic servers cap `size` at 500.
const PAGE_SIZE: u32 = 500;
/// Windows needed to cover `LIBRARY_SIZE` albums at `PAGE_SIZE`.
const PAGE_COUNT: usize = LIBRARY_SIZE / PAGE_SIZE as usize;
/// Sequential searches in the generation-guard test.
const SEARCH_COUNT: usize = 200;
/// Cover inserts in the LRU test (well past the 200-entry cap).
const COVER_INSERTS: usize = 500;
/// Concurrent, out-of-order searches in the stale-response test.
const RACY_SEARCHES: usize = 8;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Wrap a Subsonic payload in the envelope the client unwraps.
fn envelope(inner: &str) -> String {
    format!(r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1",{inner}}}}}"#)
}

fn song_dto(i: usize) -> SongDto {
    SongDto {
        id: format!("song-{i}"),
        title: format!("Song number {i}"),
        artist: Some(format!("Artist {}", i % 100)),
        artist_id: Some(format!("artist-{}", i % 100)),
        album: Some(format!("album-{}", i / 10)),
        album_id: Some(format!("album-{}", i / 10)),
        cover_art: Some(format!("cover-{}", i / 10)),
        duration: Some(180 + (i % 120) as i64),
        track_number: Some((i % 10) as u32 + 1),
        disc_number: Some(1),
        year: Some(2026),
        // Every fifth song is starred, so the starred mapping is exercised at scale too.
        starred: i
            .is_multiple_of(5)
            .then(|| "2026-11-10T00:00:00Z".to_string()),
    }
}

fn album_dto(i: usize) -> AlbumDto {
    AlbumDto {
        id: format!("album-{i}"),
        name: format!("Album {i}"),
        artist: Some(format!("Artist {}", i % 100)),
        artist_id: Some(format!("artist-{}", i % 100)),
        cover_art: Some(format!("cov-{i}")),
        song_count: Some(10),
        duration: Some(1800),
        year: Some(2026),
        starred: None,
        song: Vec::new(),
    }
}

/// One album as the JSON `getAlbumList2` returns it.
fn album_json(i: usize) -> String {
    format!(
        r#"{{"id":"album-{i}","name":"Album {i}","artist":"Artist {}","coverArt":"cov-{i}","songCount":10,"duration":1800,"year":2026}}"#,
        i % 100
    )
}

/// A `search3` payload with a single song, so responses are distinguishable per query.
fn search_json(hit_prefix: &str, i: usize) -> String {
    envelope(&format!(
        r#""searchResult3":{{"song":[{{"id":"{hit_prefix}{i}","title":"Hit {i}","artist":"Artist","album":"Album","duration":180}}],"album":[],"artist":[]}}"#
    ))
}

fn queue_track(i: usize) -> Track {
    Track {
        id: format!("t{i}"),
        title: format!("Track {i}"),
        artist: "Artist".to_string(),
        artist_id: None,
        album: "Album".to_string(),
        album_id: None,
        cover_art_id: None,
        duration: Duration::from_secs(180),
        track_number: Some((i % 20) as u32 + 1),
        disc_number: Some(1),
        year: None,
        starred: false,
    }
}

fn image(id: u64) -> Arc<gpui::Image> {
    Arc::new(gpui::Image {
        format: gpui::ImageFormat::Jpeg,
        bytes: vec![0; 4],
        id,
    })
}

/// Length of the strings a `Track` owns — a fair lower bound on its heap use.
fn track_heap_bytes(t: &Track) -> usize {
    fn opt(v: &Option<String>) -> usize {
        v.as_ref().map_or(0, String::len)
    }
    t.id.len()
        + t.title.len()
        + t.artist.len()
        + t.album.len()
        + opt(&t.artist_id)
        + opt(&t.album_id)
        + opt(&t.cover_art_id)
}

fn client_for(server: &MockServer) -> NavidromeClient {
    NavidromeClient::new(&server.base_url(), "tester", "secret").expect("client")
}

/// The generation guard the shell uses for search (`src/ui/shell.rs`: a counter is bumped
/// when a request starts and a response is only applied while its generation is still the
/// newest). Extracted here so 200 searches can exercise it without a GPUI window; the real
/// shell wiring is covered by `tests/ui_flow.rs::stale_search_response_is_discarded`.
struct SearchGate {
    newest: AtomicU64,
    applied: AtomicUsize,
    stale_after_apply: AtomicUsize,
    latest: Mutex<Option<SearchResults>>,
}

impl SearchGate {
    fn new() -> Self {
        Self {
            newest: AtomicU64::new(0),
            applied: AtomicUsize::new(0),
            stale_after_apply: AtomicUsize::new(0),
            latest: Mutex::new(None),
        }
    }

    /// Claim the next generation, exactly like `fetch_add(1, SeqCst) + 1` in the shell.
    fn begin(&self) -> u64 {
        self.newest.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Keep a response only while it is still the newest generation. Returns whether the
    /// response was applied.
    fn apply(&self, generation: u64, results: SearchResults) -> bool {
        if self.newest.load(Ordering::SeqCst) != generation {
            if self.applied.load(Ordering::SeqCst) > 0 {
                self.stale_after_apply.fetch_add(1, Ordering::SeqCst);
            }
            return false;
        }
        *self.latest.lock().expect("latest slot") = Some(results);
        self.applied.fetch_add(1, Ordering::SeqCst);
        true
    }

    fn applied(&self) -> usize {
        self.applied.load(Ordering::SeqCst)
    }

    fn stale_after_apply(&self) -> usize {
        self.stale_after_apply.load(Ordering::SeqCst)
    }

    fn latest(&self) -> Option<SearchResults> {
        self.latest.lock().expect("latest slot").clone()
    }
}

// ---------------------------------------------------------------------------
// 20k-track library: mapping and memory
// ---------------------------------------------------------------------------

#[test]
fn twenty_thousand_track_library_maps_within_budget() {
    let songs: Vec<SongDto> = (0..LIBRARY_SIZE).map(song_dto).collect();
    let albums: Vec<AlbumDto> = (0..LIBRARY_SIZE).map(album_dto).collect();

    let start = Instant::now();
    let tracks: Vec<Track> = songs.into_iter().map(song_dto_to_track).collect();
    let map_tracks = start.elapsed();

    let start = Instant::now();
    let mapped_albums: Vec<Album> = albums.into_iter().map(album_dto_to_album).collect();
    let map_albums = start.elapsed();

    println!(
        "mapped {LIBRARY_SIZE} songs in {map_tracks:?} and {LIBRARY_SIZE} albums in {map_albums:?}"
    );

    // The mapping did real work, in order, and did not drop or duplicate rows.
    assert_eq!(tracks.len(), LIBRARY_SIZE);
    assert_eq!(mapped_albums.len(), LIBRARY_SIZE);
    assert_eq!(tracks[0].id, "song-0");
    assert_eq!(
        tracks[LIBRARY_SIZE - 1].id,
        format!("song-{}", LIBRARY_SIZE - 1)
    );
    assert_eq!(tracks[12_345].album, "album-1234");
    assert_eq!(
        tracks[12_345].duration,
        Duration::from_secs(180 + (12_345 % 120) as u64)
    );
    assert!(tracks[0].starred, "every fifth song is starred");
    assert!(!tracks[1].starred);
    let last_cover = format!("cov-{}", LIBRARY_SIZE - 1);
    assert_eq!(
        mapped_albums[LIBRARY_SIZE - 1].cover_art_id.as_deref(),
        Some(last_cover.as_str())
    );

    // A domain Track must stay small and fixed-size: 20k tracks have to fit comfortably
    // inside the 200 MB idle budget, so refuse a pathological per-track footprint.
    let heap: usize = tracks.iter().map(track_heap_bytes).sum();
    let per_track = heap / LIBRARY_SIZE;
    println!(
        "≈{:.1} MiB of strings for {LIBRARY_SIZE} tracks ({per_track} B/track)",
        heap as f64 / (1024.0 * 1024.0)
    );
    assert!(
        per_track <= 1024,
        "{per_track} B/track for a 20k library is over the 1 KiB/track ceiling"
    );
    assert!(
        heap < 8 * 1024 * 1024,
        "20k tracks hold {heap} bytes of strings; expected < 8 MiB"
    );

    // Order-of-magnitude ceilings: a quadratic mapping would blow these.
    assert!(
        map_tracks < Duration::from_secs(5),
        "mapping 20k songs took {map_tracks:?}"
    );
    assert!(
        map_albums < Duration::from_secs(5),
        "mapping 20k albums took {map_albums:?}"
    );
}

// ---------------------------------------------------------------------------
// 20k-album library: windowed paging
// ---------------------------------------------------------------------------

#[test]
fn twenty_thousand_albums_page_in_windows_not_whole() {
    assert_eq!(PAGE_COUNT * PAGE_SIZE as usize, LIBRARY_SIZE);

    let server = MockServer::start();
    let mut mocks = Vec::with_capacity(PAGE_COUNT);
    for page in 0..PAGE_COUNT {
        let offset = page as u32 * PAGE_SIZE;
        let rows: Vec<String> = (offset..offset + PAGE_SIZE)
            .map(|i| album_json(i as usize))
            .collect();
        let body = envelope(&format!(r#""albumList2":{{"album":[{}]}}"#, rows.join(",")));
        let offset_s = offset.to_string();
        let size_s = PAGE_SIZE.to_string();
        // Every window is its own mock, matched on the exact `size`/`offset` the client
        // must send: a request for the whole library would match nothing and 404.
        mocks.push(server.mock(move |when, then| {
            when.method(GET)
                .path("/rest/getAlbumList2.view")
                .query_param("type", "newest")
                .query_param("size", size_s)
                .query_param("offset", offset_s);
            then.status(200)
                .header("content-type", "application/json")
                .body(body.clone());
        }));
    }

    let client = client_for(&server);
    let start = Instant::now();
    let mut albums: Vec<Album> = Vec::with_capacity(LIBRARY_SIZE);
    for page in 0..PAGE_COUNT {
        let result = client.album_list(AlbumListKind::Newest, PAGE_SIZE, page as u32 * PAGE_SIZE);
        let window = result.unwrap_or_else(|e| panic!("page {page} failed: {e}"));
        assert_eq!(
            window.len(),
            PAGE_SIZE as usize,
            "page {page} returned {} rows, expected a full window",
            window.len()
        );
        albums.extend(window);
    }
    let elapsed = start.elapsed();
    println!("{PAGE_COUNT} windows of {PAGE_SIZE} albums fetched in {elapsed:?}");

    // Right count, and every album exactly once, in order: no overlap and no gap between
    // windows means the client really walked the library page by page.
    assert_eq!(albums.len(), LIBRARY_SIZE);
    for (i, album) in albums.iter().enumerate() {
        assert_eq!(album.id, format!("album-{i}"), "gap or overlap at row {i}");
    }

    // Exactly one request per window: no retries, no re-fetch, and no request that asked
    // for the whole library (those would not match any mock and would have failed above).
    for mock in &mocks {
        mock.assert_calls(1);
    }
    let total_hits: usize = mocks.iter().map(|m| m.calls()).sum();
    assert_eq!(
        total_hits, PAGE_COUNT,
        "unexpected extra album-list requests"
    );

    // 40 localhost round trips; the ceiling only catches a per-request full scan.
    assert!(
        elapsed < Duration::from_secs(15),
        "paging 20k albums took {elapsed:?}"
    );
}

// ---------------------------------------------------------------------------
// 200 sequential searches through the generation guard
// ---------------------------------------------------------------------------

#[test]
fn two_hundred_searches_keep_only_the_newest_result() {
    let server = MockServer::start();
    let mut mocks = Vec::with_capacity(SEARCH_COUNT);
    for i in 0..SEARCH_COUNT {
        let body = search_json("hit-", i);
        let query = format!("q{i}");
        // Matching the counts too: the client must always bound how much a search can
        // bring back, otherwise one query can blow up the retained result set.
        mocks.push(server.mock(move |when, then| {
            when.method(GET)
                .path("/rest/search3.view")
                .query_param("query", query.clone())
                .query_param("artistCount", "20")
                .query_param("albumCount", "20")
                .query_param("songCount", "40");
            then.status(200)
                .header("content-type", "application/json")
                .body(body.clone());
        }));
    }

    let client = client_for(&server);
    let gate = SearchGate::new();
    // The real cover LRU, driven the way the shell drives it when results arrive.
    let mut home = HomeState::default();
    let mut covers_inserted = 0usize;

    let start = Instant::now();
    for i in 0..SEARCH_COUNT {
        let generation = gate.begin();
        let results = client
            .search(&format!("q{i}"))
            .unwrap_or_else(|e| panic!("search {i} failed: {e}"));
        // Mirror the shell: a response that is no longer the newest is dropped.
        if !gate.apply(generation, results) {
            continue;
        }
        let latest = gate.latest().expect("applied result");
        // Search results are capped server-side; keep that contract visible here.
        assert!(latest.songs.len() <= 40, "songCount exceeded");
        assert!(latest.albums.len() <= 20, "albumCount exceeded");
        assert!(latest.artists.len() <= 20, "artistCount exceeded");
        // Ten covers per search: 2000 inserts in total, ten times the cache cap.
        for n in 0..10 {
            home.insert_cover(format!("cover-{i}-{n}"), image((i * 10 + n) as u64));
            covers_inserted += 1;
        }
        assert!(
            home.covers.len() <= COVER_CACHE_CAP,
            "cover cache grew to {} entries at search {i}",
            home.covers.len()
        );
    }
    let elapsed = start.elapsed();
    println!("{SEARCH_COUNT} searches ({covers_inserted} cover inserts) in {elapsed:?}");

    // Sequential requests are always the newest when they land, so each one is applied...
    assert_eq!(gate.applied(), SEARCH_COUNT);
    // ...but they replace, never accumulate: exactly one result set survives.
    let latest = gate.latest().expect("latest results");
    assert_eq!(latest.songs.len(), 1);
    assert_eq!(latest.songs[0].id, format!("hit-{}", SEARCH_COUNT - 1));

    // A late stale response for an older generation must not overwrite the newest one.
    assert!(
        !gate.apply(SEARCH_COUNT as u64 - 1, latest.clone()),
        "a stale generation was accepted"
    );
    assert_eq!(
        gate.latest().expect("latest results").songs[0].id,
        format!("hit-{}", SEARCH_COUNT - 1),
        "stale search response overwrote the newest result"
    );

    // Caches stayed bounded while being inserted into ten times over their cap.
    assert_eq!(covers_inserted, SEARCH_COUNT * 10);
    assert_eq!(home.covers.len(), COVER_CACHE_CAP);
    assert_eq!(
        home.covers.len(),
        home.cover_order.len(),
        "cover recency queue drifted from the map"
    );
    assert!(
        !home.covers.contains_key("cover-0-0"),
        "the oldest cover should have been evicted"
    );

    for mock in &mocks {
        mock.assert_calls(1);
    }

    assert!(
        elapsed < Duration::from_secs(20),
        "{SEARCH_COUNT} searches took {elapsed:?}"
    );
}

// ---------------------------------------------------------------------------
// Out-of-order search responses
// ---------------------------------------------------------------------------

#[test]
fn stale_out_of_order_search_responses_are_discarded() {
    let server = MockServer::start();
    let mut mocks = Vec::with_capacity(RACY_SEARCHES);
    for i in 0..RACY_SEARCHES {
        let body = search_json("oom-", i);
        let query = format!("q{i}");
        // Later generations answer faster, so the newest response lands while older
        // requests are still in flight — the exact race the guard exists for.
        let delay = Duration::from_millis(30 + 30 * (RACY_SEARCHES - 1 - i) as u64);
        mocks.push(server.mock(move |when, then| {
            when.method(GET)
                .path("/rest/search3.view")
                .query_param("query", query.clone());
            then.delay(delay)
                .status(200)
                .header("content-type", "application/json")
                .body(body.clone());
        }));
    }

    let gate = SearchGate::new();
    // Generation N (the last query) is the newest from the start: every request carries a
    // generation fixed before it is sent, so the outcome does not depend on thread timing.
    gate.newest.store(RACY_SEARCHES as u64, Ordering::SeqCst);
    let base = server.base_url();
    let barrier = Barrier::new(RACY_SEARCHES);
    let start = Instant::now();
    std::thread::scope(|scope| {
        for i in 0..RACY_SEARCHES {
            let base = base.clone();
            let gate = &gate;
            let barrier = &barrier;
            scope.spawn(move || {
                let client = NavidromeClient::new(&base, "tester", "secret").expect("client");
                barrier.wait();
                let results = client
                    .search(&format!("q{i}"))
                    .unwrap_or_else(|e| panic!("racy search {i} failed: {e}"));
                gate.apply(i as u64 + 1, results);
            });
        }
    });
    let elapsed = start.elapsed();
    println!(
        "{RACY_SEARCHES} concurrent out-of-order searches in {elapsed:?}; {} stale responses \
         arrived after the winner, {} applied",
        gate.stale_after_apply(),
        gate.applied()
    );

    // Exactly one response — the newest generation — was kept.
    assert_eq!(
        gate.applied(),
        1,
        "the gate applied {} responses instead of 1",
        gate.applied()
    );
    let latest = gate.latest().expect("latest results");
    assert_eq!(
        latest.songs[0].id,
        format!("oom-{}", RACY_SEARCHES - 1),
        "an older response was applied over the newest one"
    );

    // All requests were answered: the stale ones were really in flight, not dropped.
    let total_hits: usize = mocks.iter().map(|m| m.calls()).sum();
    assert_eq!(
        total_hits, RACY_SEARCHES,
        "not every racy request reached the server"
    );
    // At least one stale response completed *after* the winner had been applied — that is
    // the race window, and it was closed.
    assert!(
        gate.stale_after_apply() >= 1,
        "no stale response arrived after the newest was applied; the test did not race"
    );
}

// ---------------------------------------------------------------------------
// Cover cache LRU
// ---------------------------------------------------------------------------

#[test]
fn five_hundred_cover_inserts_stay_bounded_and_lru() {
    let mut home = HomeState::default();
    let mut first_cover = None;

    for i in 0..COVER_INSERTS {
        let img = image(i as u64);
        if i == 0 {
            first_cover = Some(Arc::downgrade(&img));
        }
        home.insert_cover(format!("c{i}"), img);
        // Checked on every insert, not only at the end, so a temporary overflow is caught.
        assert!(
            home.covers.len() <= COVER_CACHE_CAP,
            "cover cache held {} entries after insert {i}",
            home.covers.len()
        );
        assert_eq!(
            home.covers.len(),
            home.cover_order.len(),
            "recency queue went out of sync with the map at insert {i}"
        );
    }

    assert_eq!(home.covers.len(), COVER_CACHE_CAP);
    let evicted = COVER_INSERTS - COVER_CACHE_CAP;
    for i in 0..evicted {
        assert!(
            !home.covers.contains_key(&format!("c{i}")),
            "c{i} should have been evicted (oldest first)"
        );
    }
    for i in evicted..COVER_INSERTS {
        assert!(
            home.covers.contains_key(&format!("c{i}")),
            "c{i} is the newest 200 and must still be cached"
        );
    }
    // Evicted entries are dropped, not leaked behind the map: the Arc would keep the
    // image bytes alive otherwise.
    assert!(
        first_cover
            .expect("weak ref to first cover")
            .upgrade()
            .is_none(),
        "an evicted cover is still alive behind a strong reference"
    );

    // Recency: re-inserting an existing cover refreshes it, so the oldest *untouched*
    // entry is the one evicted next.
    let mut churn = HomeState::default();
    for i in 0..COVER_CACHE_CAP {
        churn.insert_cover(format!("k{i}"), image(i as u64));
    }
    churn.insert_cover("k0".to_string(), image(9_999));
    assert_eq!(
        churn.covers.len(),
        COVER_CACHE_CAP,
        "re-inserting must not grow the cache"
    );
    churn.insert_cover("k200".to_string(), image(4_242));
    assert_eq!(churn.covers.len(), COVER_CACHE_CAP);
    assert!(
        churn.covers.contains_key("k0"),
        "the refreshed cover was evicted instead of the least recently added one"
    );
    assert!(
        !churn.covers.contains_key("k1"),
        "k1 is the least recently added and must be evicted"
    );
    assert_eq!(
        churn.covers["k0"].id, 9_999,
        "refresh must replace the image"
    );

    // Long mixed churn: 2000 more inserts over a 250-id working set, alternating between
    // hits and misses, must hold both invariants after every single insert.
    for step in 0..2_000usize {
        let id = format!("m{}", (step * 37) % 250);
        churn.insert_cover(id, image(step as u64));
        assert!(churn.covers.len() <= COVER_CACHE_CAP);
        assert_eq!(churn.covers.len(), churn.cover_order.len());
    }
}

// ---------------------------------------------------------------------------
// 20k-track queue under rapid churn
// ---------------------------------------------------------------------------

#[test]
fn twenty_thousand_track_queue_survives_rapid_churn() {
    let mut queue = Queue::new();
    queue.play_now((0..LIBRARY_SIZE).map(queue_track).collect(), 0);
    assert_eq!(queue.len(), LIBRARY_SIZE);
    assert_eq!(queue.current().expect("current").id, "t0");

    // Rapid skipping: 5000 next/previous pairs, with the current-track invariant checked
    // on every step (this is the "holds a real queue, not 50 tracks" upgrade of
    // tests/queue.rs::rapid_next_previous_stays_consistent).
    let start = Instant::now();
    let mut expected = 0usize;
    for step in 0..5_000usize {
        let next = queue.next().expect("next");
        expected += 1;
        assert_eq!(next.id, format!("t{expected}"));
        assert_eq!(queue.current().expect("current").id, format!("t{expected}"));
        // Bursts of three skips forward, then the same back: the boundary must stay exact.
        if step.is_multiple_of(3) {
            for _ in 0..2 {
                expected += 1;
                assert_eq!(queue.next().expect("next").id, format!("t{expected}"));
            }
            for _ in 0..3 {
                expected -= 1;
                assert_eq!(
                    queue.previous().expect("previous").id,
                    format!("t{expected}")
                );
            }
        }
        if step.is_multiple_of(250) {
            // The queue is fixed-size: skipping never appends or drops tracks.
            assert_eq!(
                queue.len(),
                LIBRARY_SIZE,
                "queue length changed at step {step}"
            );
        }
    }
    let churn = start.elapsed();
    println!("5000 next/prev pairs over {LIBRARY_SIZE} tracks in {churn:?}, ended at t{expected}");
    assert_eq!(queue.len(), LIBRARY_SIZE);

    // Repeat modes still hold at both ends of a 20k queue.
    queue.set_repeat(RepeatMode::All);
    queue.play_now(
        (0..LIBRARY_SIZE).map(queue_track).collect(),
        LIBRARY_SIZE - 1,
    );
    assert_eq!(queue.next().expect("wrap").id, "t0");
    assert_eq!(
        queue.previous().expect("wrap back").id,
        format!("t{}", LIBRARY_SIZE - 1)
    );

    queue.set_repeat(RepeatMode::One);
    for _ in 0..5 {
        assert_eq!(
            queue.next().expect("repeat one").id,
            format!("t{}", LIBRARY_SIZE - 1)
        );
    }

    queue.set_repeat(RepeatMode::Off);
    queue.play_now(
        (0..LIBRARY_SIZE).map(queue_track).collect(),
        LIBRARY_SIZE - 1,
    );
    assert!(queue.next().is_none(), "repeat off must stop at the end");
    assert!(queue.current().is_none());
    assert_eq!(
        queue.len(),
        LIBRARY_SIZE,
        "running off the end must not shrink the queue"
    );

    // Shuffle over 20k: a full pass must visit every track exactly once, and must not
    // duplicate the queue to do it.
    let start = Instant::now();
    queue.play_now((0..LIBRARY_SIZE).map(queue_track).collect(), 0);
    queue.set_shuffle(true);
    let mut visited = vec![false; LIBRARY_SIZE];
    let first = queue.current().expect("current").id.clone();
    let first_index: usize = first[1..].parse().expect("numeric id");
    visited[first_index] = true;
    let mut visited_count = 1usize;
    while let Some(track) = queue.next() {
        let index: usize = track.id[1..].parse().expect("numeric id");
        assert!(!visited[index], "shuffle visited {} twice", track.id);
        visited[index] = true;
        visited_count += 1;
        assert_eq!(
            queue.len(),
            LIBRARY_SIZE,
            "shuffle changed the queue length"
        );
    }
    let shuffle = start.elapsed();
    println!("shuffle walk of {LIBRARY_SIZE} tracks in {shuffle:?}");
    assert_eq!(visited_count, LIBRARY_SIZE, "shuffle skipped tracks");

    // The player state rides on top of the same queue; it must keep current/status in
    // sync through the same churn without growing the queue.
    let mut state = PlayerState::new();
    state.play_now((0..LIBRARY_SIZE).map(queue_track).collect(), 0);
    for _ in 0..5_000 {
        let next = state.next().expect("state next");
        assert_eq!(state.current.as_ref().expect("state current").id, next.id);
        assert_eq!(state.queue.current().expect("queue current").id, next.id);
        assert_eq!(state.queue.len(), LIBRARY_SIZE);
        let previous = state.previous().expect("state previous");
        assert_eq!(
            state.current.as_ref().expect("state current").id,
            previous.id
        );
        assert_eq!(state.queue.len(), LIBRARY_SIZE);
    }
    assert_eq!(state.status, myuzik::player::state::PlaybackStatus::Playing);

    let total = churn + shuffle;
    assert!(
        total < Duration::from_secs(15),
        "20k queue churn took {total:?}"
    );
}
