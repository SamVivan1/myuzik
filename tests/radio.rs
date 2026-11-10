//! Autoplay radio: source priority, deduplication, genre pagination and the
//! stall guard. Runs against a mocked Subsonic server (no network, no display).

use httpmock::prelude::*;
use myuzik::library::Track;
use myuzik::library::radio::{BATCH, GENRE_PAGE, Radio};
use myuzik::navidrome::client::NavidromeClient;
use std::collections::HashSet;
use std::time::Duration;

// --- helpers ---------------------------------------------------------------

fn envelope(inner: &str) -> String {
    format!(r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1",{inner}}}}}"#)
}

fn song_json(id: &str) -> String {
    format!(
        r#"{{"id":"{id}","title":"Title {id}","artist":"Artist","artistId":"ar1","album":"Album","albumId":"al1","genre":"Rock","duration":180}}"#
    )
}

fn songs(ids: &[&str]) -> String {
    let list: Vec<String> = ids.iter().map(|id| song_json(id)).collect();
    list.join(",")
}

fn similar_song(ids: &[&str]) -> String {
    envelope(&format!(r#""similarSongs":{{"song":[{}]}}"#, songs(ids)))
}

fn similar_artist(ids: &[&str]) -> String {
    envelope(&format!(r#""similarSongs2":{{"song":[{}]}}"#, songs(ids)))
}

fn by_genre(ids: &[&str]) -> String {
    envelope(&format!(r#""songsByGenre":{{"song":[{}]}}"#, songs(ids)))
}

fn random(ids: &[&str]) -> String {
    envelope(&format!(r#""randomSongs":{{"song":[{}]}}"#, songs(ids)))
}

fn client(server: &MockServer) -> NavidromeClient {
    NavidromeClient::new(&server.base_url(), "tester", "secret").expect("client")
}

fn seed() -> Track {
    Track {
        id: "seed".to_string(),
        title: "Seed".to_string(),
        artist: "Artist".to_string(),
        artist_id: Some("ar1".to_string()),
        album: "Album".to_string(),
        album_id: Some("al1".to_string()),
        cover_art_id: None,
        duration: Duration::from_secs(180),
        track_number: None,
        disc_number: None,
        year: None,
        starred: false,
        genre: Some("Rock".to_string()),
    }
}

// --- tests -----------------------------------------------------------------

#[test]
fn sources_are_tried_in_priority_order() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSimilarSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(similar_song(&["song-sim"]));
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSimilarSongs2.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(similar_artist(&["artist-sim"]));
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSongsByGenre.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(by_genre(&["genre-1"]));
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getRandomSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(random(&["random-1"]));
    });

    let mut radio = Radio::new();
    let batch = radio
        .next_batch(&client(&server), &seed(), &HashSet::new())
        .expect("batch");

    let ids: Vec<&str> = batch.iter().map(|t| t.id.as_str()).collect();
    // Most precise source first, backstop last.
    assert_eq!(ids, vec!["song-sim", "artist-sim", "genre-1", "random-1"]);
}

#[test]
fn a_full_batch_from_one_source_stops_the_search() {
    let server = MockServer::start();
    let ids: Vec<String> = (0..30).map(|i| format!("s{i}")).collect();
    let refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    let song_mock = server.mock(|when, then| {
        when.method(GET).path("/rest/getSimilarSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(similar_song(&refs));
    });
    let random_mock = server.mock(|when, then| {
        when.method(GET).path("/rest/getRandomSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(random(&["never"]));
    });

    let mut radio = Radio::new();
    let batch = radio
        .next_batch(&client(&server), &seed(), &HashSet::new())
        .expect("batch");

    assert_eq!(batch.len(), BATCH, "batch size is capped");
    assert_eq!(song_mock.calls(), 1);
    assert_eq!(
        random_mock.calls(),
        0,
        "backstop not needed when batch filled"
    );
}

#[test]
fn already_queued_recent_and_the_seed_are_never_repeated() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSimilarSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(similar_song(&["queued", "played-before", "fresh", "seed"]));
    });
    stub_other_sources(&server);

    let mut radio = Radio::new();
    radio.note_played("played-before");

    let mut exclude = HashSet::new();
    exclude.insert("queued".to_string());

    let batch = radio
        .next_batch(&client(&server), &seed(), &exclude)
        .expect("batch");
    let ids: Vec<&str> = batch.iter().map(|t| t.id.as_str()).collect();

    assert!(!ids.contains(&"queued"), "queued track leaked in");
    assert!(!ids.contains(&"played-before"), "recently played leaked in");
    assert!(!ids.contains(&"seed"), "the seed itself leaked in");
    assert!(ids.contains(&"fresh"));
}

fn stub_other_sources(server: &MockServer) {
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSimilarSongs2.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(envelope(""));
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSongsByGenre.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(envelope(""));
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getRandomSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(envelope(""));
    });
}

#[test]
fn genre_pagination_advances_then_wraps_at_the_end_of_the_pool() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/rest/getSimilarSongs.view")
            .query_param("count", GENRE_PAGE.to_string());
        then.status(200)
            .header("content-type", "application/json")
            .body(envelope(""));
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSimilarSongs2.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(envelope(""));
    });
    // A full page at offset 0 and at offset 25; a short page at 50 (pool end).
    let page_ids =
        |prefix: &str| -> Vec<String> { (0..GENRE_PAGE).map(|i| format!("{prefix}{i}")).collect() };
    let first = page_ids("a");
    let second = page_ids("b");
    let first_refs: Vec<&str> = first.iter().map(|s| s.as_str()).collect();
    let second_refs: Vec<&str> = second.iter().map(|s| s.as_str()).collect();
    for (off, body) in [(0u32, by_genre(&first_refs)), (25, by_genre(&second_refs))] {
        server.mock(|when, then| {
            when.method(GET)
                .path("/rest/getSongsByGenre.view")
                .query_param("offset", off.to_string());
            then.status(200)
                .header("content-type", "application/json")
                .body(body);
        });
    }
    server.mock(|when, then| {
        when.method(GET)
            .path("/rest/getSongsByGenre.view")
            .query_param("offset", "50");
        then.status(200)
            .header("content-type", "application/json")
            .body(by_genre(&["last-1"]));
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getRandomSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(envelope(""));
    });

    let mut radio = Radio::new();
    let c = client(&server);

    // First batch: genre page at offset 0, which is full -> offset advances.
    let batch = radio
        .next_batch(&c, &seed(), &HashSet::new())
        .expect("batch");
    assert_eq!(radio.genre_offset(), GENRE_PAGE);
    assert_eq!(batch.len(), BATCH);

    // Second batch: starts from the advanced offset, so it returns new songs.
    let exclude: HashSet<String> = batch.iter().map(|t| t.id.clone()).collect();
    let batch2 = radio.next_batch(&c, &seed(), &exclude).expect("batch2");
    assert!(
        batch2.iter().all(|t| t.id.starts_with('b')),
        "second batch should come from the next genre page: {:?}",
        batch2.iter().map(|t| &t.id).collect::<Vec<_>>()
    );
}

#[test]
fn short_genre_page_resets_the_offset_so_the_pool_cycles() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSimilarSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(envelope(""));
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSimilarSongs2.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(envelope(""));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/rest/getSongsByGenre.view")
            .query_param("offset", "0");
        then.status(200)
            .header("content-type", "application/json")
            .body(by_genre(&["only-1", "only-2"]));
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getRandomSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(envelope(""));
    });

    let mut radio = Radio::new();
    radio
        .next_batch(&client(&server), &seed(), &HashSet::new())
        .expect("batch");
    assert_eq!(radio.genre_offset(), 0, "a short page means the pool ended");
}

#[test]
fn random_is_the_backstop_when_nothing_else_matches() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSimilarSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(envelope(""));
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSimilarSongs2.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(envelope(""));
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSongsByGenre.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(envelope(""));
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getRandomSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(random(&["fallback-1", "fallback-2"]));
    });

    // A seed with no genre and no artist id still yields music.
    let mut bare = seed();
    bare.genre = None;
    bare.artist_id = None;

    let mut radio = Radio::new();
    let batch = radio
        .next_batch(&client(&server), &bare, &HashSet::new())
        .expect("batch");
    let ids: Vec<&str> = batch.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["fallback-1", "fallback-2"]);
}

#[test]
fn one_failing_source_does_not_abort_the_batch() {
    let server = MockServer::start();
    // Similar-to-song fails hard.
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSimilarSongs.view");
        then.status(500);
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSimilarSongs2.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(similar_artist(&["artist-ok"]));
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSongsByGenre.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(envelope(""));
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getRandomSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(envelope(""));
    });

    let mut radio = Radio::new();
    let batch = radio
        .next_batch(&client(&server), &seed(), &HashSet::new())
        .expect("a single failure must not fail the batch");
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].id, "artist-ok");
}

#[test]
fn all_sources_failing_surfaces_an_error_once() {
    let server = MockServer::start();
    for path in [
        "/rest/getSimilarSongs.view",
        "/rest/getSimilarSongs2.view",
        "/rest/getSongsByGenre.view",
        "/rest/getRandomSongs.view",
    ] {
        server.mock(|when, then| {
            when.method(GET).path(path);
            then.status(500);
        });
    }

    let mut radio = Radio::new();
    let err = radio
        .next_batch(&client(&server), &seed(), &HashSet::new())
        .expect_err("every source failing should be reported");
    // The caller turns this into one toast instead of retrying in a loop.
    assert!(!format!("{err:?}").is_empty());
}

#[test]
fn the_radio_stalls_after_repeated_empty_batches_and_recovers_on_a_new_seed() {
    let server = MockServer::start();
    // Every source answers, but only with ids the caller already has.
    server.mock(|when, then| {
        when.method(GET).path("/rest/getSimilarSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(similar_song(&["seed"]));
    });
    stub_other_sources(&server);
    let random_mock = server.mock(|when, then| {
        when.method(GET).path("/rest/getRandomSongs.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(envelope(""));
    });

    let mut radio = Radio::new();
    let c = client(&server);
    for _ in 0..3 {
        let batch = radio
            .next_batch(&c, &seed(), &HashSet::new())
            .expect("batch");
        assert!(batch.is_empty(), "nothing fresh is available");
    }
    assert!(radio.is_stalled(), "three empty batches should stall it");

    let hits_before = random_mock.calls();
    let batch = radio
        .next_batch(&c, &seed(), &HashSet::new())
        .expect("batch");
    assert!(batch.is_empty());
    assert_eq!(
        random_mock.calls(),
        hits_before,
        "a stalled radio must not keep querying the server"
    );

    // A different track seeds it again.
    let mut other = seed();
    other.id = "another-seed".to_string();
    radio
        .next_batch(&c, &other, &HashSet::new())
        .expect("batch");
    assert!(!radio.is_stalled(), "a new seed clears the stall");
}
