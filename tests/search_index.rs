//! Tests for the in-memory local search index over a library snapshot.

use std::time::Duration;

use myuzik::library::search_index::{MAX_PER_GROUP, SearchIndex};
use myuzik::library::{Album, Artist, Playlist, SearchResults, Track, TrackSource};

fn track(id: &str, title: &str, artist: &str, album: &str, genre: Option<&str>) -> Track {
    Track {
        id: id.to_string(),
        title: title.to_string(),
        artist: artist.to_string(),
        artist_id: None,
        album: album.to_string(),
        album_id: None,
        cover_art_id: None,
        duration: Duration::from_secs(180),
        track_number: None,
        disc_number: None,
        year: None,
        starred: false,
        genre: genre.map(str::to_string),
        source: Default::default(),
        cover_url: None,
    }
}

fn album(id: &str, name: &str, artist: &str) -> Album {
    Album {
        id: id.to_string(),
        name: name.to_string(),
        artist: artist.to_string(),
        artist_id: None,
        cover_art_id: None,
        song_count: None,
        duration: None,
        year: None,
        starred: false,
        source: TrackSource::Navidrome,
        cover_url: None,
    }
}

fn artist(id: &str, name: &str) -> Artist {
    Artist {
        id: id.to_string(),
        name: name.to_string(),
        album_count: None,
        cover_art_id: None,
        source: TrackSource::Navidrome,
        cover_url: None,
    }
}

fn playlist(id: &str, name: &str) -> Playlist {
    Playlist {
        id: id.to_string(),
        name: name.to_string(),
        song_count: 3,
        duration: Duration::from_secs(600),
        owner: "sam".to_string(),
        source: TrackSource::Navidrome,
        cover_url: None,
    }
}

fn song_ids(results: &SearchResults) -> Vec<&str> {
    results.songs.iter().map(|s| s.id.as_str()).collect()
}

fn album_ids(results: &SearchResults) -> Vec<&str> {
    results.albums.iter().map(|a| a.id.as_str()).collect()
}

fn all_empty(results: &SearchResults) -> bool {
    results.artists.is_empty()
        && results.albums.is_empty()
        && results.songs.is_empty()
        && results.playlists.is_empty()
}

#[test]
fn empty_and_whitespace_queries_return_nothing() {
    let index = SearchIndex::build(
        &[album("al1", "Blue", "Miles Davis")],
        &[artist("ar1", "Miles Davis")],
        &[playlist("p1", "Blue")],
        &[track("s1", "Blue in Green", "Miles Davis", "Blue", None)],
    );

    assert!(
        all_empty(&index.search("")),
        "empty query must return nothing"
    );
    assert!(
        all_empty(&index.search("   \t\n ")),
        "whitespace-only query must return nothing"
    );
    assert!(
        all_empty(&index.search("zzzznothing")),
        "unmatched query must return nothing"
    );
}

#[test]
fn case_insensitive_match_finds_accented_artist() {
    let index = SearchIndex::build(
        &[],
        &[artist("ar1", "Björk")],
        &[],
        &[track("s1", "Jóga", "Björk", "Homogenic", None)],
    );

    let results = index.search("bjork");
    assert_eq!(
        results.artists.len(),
        1,
        "artist should match case-insensitively"
    );
    assert_eq!(results.artists[0].id, "ar1");
    assert_eq!(
        song_ids(&results),
        vec!["s1"],
        "song matches by artist name"
    );
}

#[test]
fn diacritic_folding_works_in_both_directions() {
    let index = SearchIndex::build(
        &[],
        &[artist("ar1", "Bjork"), artist("ar2", "Sigur Rós")],
        &[],
        &[
            track("s1", "Human Behaviour", "Bjork", "Debut", None),
            track("s2", "Hoppípolla", "Sigur Rós", "Takk...", None),
        ],
    );

    // Accented query against an unaccented entry.
    let results = index.search("Björk");
    assert_eq!(results.artists.len(), 1);
    assert_eq!(results.artists[0].id, "ar1");
    assert_eq!(song_ids(&results), vec!["s1"]);

    // Unaccented query against an unaccented entry still works.
    let results = index.search("bjork");
    assert_eq!(results.artists.len(), 1);
    assert_eq!(results.artists[0].id, "ar1");

    // Accented item, unaccented two-token query: both tokens must fold.
    let results = index.search("sigur ros");
    assert_eq!(results.artists.len(), 1, "'ros' must fold to match 'Rós'");
    assert_eq!(results.artists[0].id, "ar2");
    assert_eq!(song_ids(&results), vec!["s2"]);

    let results = index.search("sigur");
    assert_eq!(results.artists.len(), 1);
    assert_eq!(results.artists[0].id, "ar2");
    assert_eq!(song_ids(&results), vec!["s2"]);
}

#[test]
fn multi_word_query_requires_every_token() {
    let index = SearchIndex::build(
        &[album("al1", "Kind of Blue", "Miles Davis")],
        &[artist("ar1", "Miles Davis")],
        &[],
        &[track("s1", "So What", "Miles Davis", "Kind of Blue", None)],
    );

    let results = index.search("miles kind");
    assert_eq!(song_ids(&results), vec!["s1"]);
    assert_eq!(album_ids(&results), vec!["al1"]);

    let results = index.search("miles zzz");
    assert!(all_empty(&results), "an unmatched token excludes the item");
}

#[test]
fn songs_rank_exact_then_prefix_then_substring_then_artist() {
    let index = SearchIndex::build(
        &[],
        &[],
        &[],
        &[
            track(
                "substring",
                "Kind of Blue",
                "Miles Davis",
                "Kind of Blue",
                None,
            ),
            track("artist_exact", "So What", "Blue", "Other", None),
            track("prefix", "Blue Moon", "Someone Else", "Other", None),
            track("exact", "Blue", "Someone Else", "Other", None),
        ],
    );

    let results = index.search("blue");
    assert_eq!(
        song_ids(&results),
        vec!["exact", "prefix", "substring", "artist_exact"],
        "exact title, then prefix, then substring, then a non-title field"
    );
}

#[test]
fn ties_keep_input_order() {
    let index = SearchIndex::build(
        &[],
        &[],
        &[],
        &[
            track("s2", "Blue Two", "A", "X", None),
            track("s1", "Blue One", "B", "X", None),
            track("s3", "Blue Three", "C", "X", None),
        ],
    );

    let results = index.search("blue");
    assert_eq!(
        song_ids(&results),
        vec!["s2", "s1", "s3"],
        "equal ranks keep the input order (stable sort)"
    );
}

#[test]
fn query_populates_all_four_groups() {
    let index = SearchIndex::build(
        &[album("al1", "OK Computer", "Radiohead")],
        &[artist("ar1", "Radiohead")],
        &[playlist("p1", "Radiohead favourites")],
        &[track(
            "s1",
            "Paranoid Android",
            "Radiohead",
            "OK Computer",
            None,
        )],
    );

    let results = index.search("radiohead");
    assert_eq!(results.artists.len(), 1);
    assert_eq!(results.albums.len(), 1);
    assert_eq!(results.songs.len(), 1);
    assert_eq!(results.playlists.len(), 1);
    assert_eq!(results.artists[0].id, "ar1");
    assert_eq!(results.albums[0].id, "al1");
    assert_eq!(results.songs[0].id, "s1");
    assert_eq!(results.playlists[0].id, "p1");
}

#[test]
fn per_group_cap_is_honoured() {
    let songs: Vec<Track> = (0..100)
        .map(|i| {
            track(
                &format!("s{i}"),
                &format!("Song {i}"),
                "Various",
                "Compilation",
                None,
            )
        })
        .collect();
    let index = SearchIndex::build(&[], &[], &[], &songs);

    let results = index.search("song");
    assert_eq!(results.songs.len(), MAX_PER_GROUP);
    assert_eq!(
        song_ids(&results)[0],
        "s0",
        "first candidate survives the cap"
    );
}

#[test]
fn index_reports_its_size() {
    let index = SearchIndex::build(
        &[album("al1", "Blue", "Miles Davis")],
        &[artist("ar1", "Miles Davis")],
        &[playlist("p1", "Blue")],
        &[
            track("s1", "So What", "Miles Davis", "Kind of Blue", None),
            track("s2", "Blue in Green", "Miles Davis", "Kind of Blue", None),
        ],
    );

    assert!(!index.is_empty());
    assert_eq!(index.song_count(), 2);

    let empty = SearchIndex::build(&[], &[], &[], &[]);
    assert!(empty.is_empty());
    assert_eq!(empty.song_count(), 0);
}

#[test]
fn repeated_builds_are_deterministic() {
    let build = || {
        let songs: Vec<Track> = (0..50)
            .map(|i| {
                track(
                    &format!("s{i}"),
                    &format!("Needle {i}"),
                    &format!("Artist {i}"),
                    &format!("Album {i}"),
                    Some("Rock"),
                )
            })
            .collect();
        let albums: Vec<Album> = (0..30)
            .map(|i| {
                album(
                    &format!("al{i}"),
                    &format!("Needle Album {i}"),
                    &format!("Artist {i}"),
                )
            })
            .collect();
        let artists: Vec<Artist> = (0..30)
            .map(|i| artist(&format!("ar{i}"), &format!("Needle Artist {i}")))
            .collect();
        let playlists: Vec<Playlist> = (0..10)
            .map(|i| playlist(&format!("p{i}"), &format!("Needle List {i}")))
            .collect();
        SearchIndex::build(&albums, &artists, &playlists, &songs)
    };

    let first = build();
    let second = build();

    let a = first.search("needle");
    let b = second.search("needle");
    assert_eq!(
        format!("{a:?}"),
        format!("{b:?}"),
        "same input, same output"
    );
    assert_eq!(a.songs.len(), MAX_PER_GROUP);
    assert_eq!(a.albums.len(), MAX_PER_GROUP);
    assert_eq!(a.artists.len(), MAX_PER_GROUP);
    assert_eq!(a.playlists.len(), 10);
}

#[test]
fn twenty_thousand_songs_build_and_search_quickly() {
    let songs: Vec<Track> = (0..20_000)
        .map(|i| {
            track(
                &format!("s{i}"),
                &format!("Needle Track {i}"),
                &format!("Artist {i}"),
                &format!("Album {}", i % 500),
                Some("Jazz"),
            )
        })
        .collect();

    let started = std::time::Instant::now();
    let index = SearchIndex::build(&[], &[], &[], &songs);
    let results = index.search("needle track");
    let elapsed = started.elapsed();

    assert_eq!(index.song_count(), 20_000);
    assert_eq!(results.songs.len(), MAX_PER_GROUP);
    assert_eq!(song_ids(&results)[0], "s0");
    assert!(
        elapsed < Duration::from_secs(2),
        "build + search over 20k songs took {elapsed:?}, expected well under 2 s"
    );
}
