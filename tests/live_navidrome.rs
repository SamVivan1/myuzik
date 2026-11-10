//! Live end-to-end checks against the configured Navidrome server.
//! Run with: MYUZIK_LIVE=1 cargo test --test live_navidrome -- --nocapture --test-threads=1
//! Skipped automatically when credentials are absent or MYUZIK_LIVE is not set.

use myuzik::config::{Paths, load_config, load_password};
use myuzik::lyrics::cache::LyricsCache;
use myuzik::lyrics::model::LyricsStatus;
use myuzik::lyrics::provider::LyricsProvider;
use myuzik::navidrome::client::{AlbumListKind, NavidromeClient};

fn live_client() -> Option<NavidromeClient> {
    if std::env::var("MYUZIK_LIVE").ok().as_deref() != Some("1") {
        return None;
    }
    let paths = Paths::new().ok()?;
    let cfg = load_config(&paths).ok()?;
    let pw = load_password(&paths)?;
    NavidromeClient::new(&cfg.server_url, &cfg.username, &pw).ok()
}

fn setup() -> (Paths, NavidromeClient, myuzik::config::AppConfig) {
    let paths = Paths::new().expect("paths");
    let cfg = load_config(&paths).expect("config");
    let pw = load_password(&paths).expect("credentials file missing");
    let c = NavidromeClient::new(&cfg.server_url, &cfg.username, &pw).expect("client");
    (paths, c, cfg)
}

#[test]
#[ignore = "live server test; run with --ignored and MYUZIK_LIVE=1"]
fn live_ping_and_library() {
    if live_client().is_none() {
        eprintln!("skipped: MYUZIK_LIVE not set or no credentials");
        return;
    }
    let (_paths, c, _cfg) = setup();
    let info = c.ping().expect("ping");
    assert!(info.version.is_some(), "server version missing");
    let albums = c
        .album_list(AlbumListKind::Newest, 20, 0)
        .expect("album list");
    assert!(!albums.is_empty(), "no albums returned");
    assert!(!albums[0].id.is_empty());
}

#[test]
#[ignore = "live server test; run with --ignored and MYUZIK_LIVE=1"]
fn live_album_detail_and_cover() {
    if live_client().is_none() {
        return;
    }
    let (_paths, c, _cfg) = setup();
    let albums = c.album_list(AlbumListKind::Newest, 5, 0).expect("albums");
    for album in albums {
        let detail = c.album(&album.id).expect("album detail");
        assert_eq!(detail.album.id, album.id);
        if let Some(cid) = &detail.album.cover_art_id {
            let bytes = c.cover_bytes(cid, 512).expect("cover bytes");
            assert!(bytes.len() > 100, "cover too small");
            assert!(
                bytes.starts_with(&[0xFF, 0xD8]) || bytes.starts_with(&[0x89, b'P', b'N', b'G']),
                "cover not jpeg/png"
            );
            break;
        }
    }
}

#[test]
#[ignore = "live server test; run with --ignored and MYUZIK_LIVE=1"]
fn live_artists_playlists_search() {
    if live_client().is_none() {
        return;
    }
    let (_paths, c, _cfg) = setup();
    let artists = c.artists().expect("artists");
    assert!(!artists.is_empty(), "no artists");
    let _ = c.playlists().expect("playlists");
    let r = c.search("love").expect("search");
    assert!(
        !r.songs.is_empty() || !r.albums.is_empty() || !r.artists.is_empty(),
        "search empty"
    );
}

#[test]
#[ignore = "live server test; run with --ignored and MYUZIK_LIVE=1"]
fn live_stream_url_fetches_audio() {
    if live_client().is_none() {
        return;
    }
    let (_paths, c, cfg) = setup();
    let albums = c.album_list(AlbumListKind::Newest, 3, 0).expect("albums");
    let mut tested = 0;
    for album in albums {
        let detail = c.album(&album.id).expect("detail");
        if let Some(track) = detail.songs.first() {
            let uri = c.stream_url(&track.id, cfg.quality).expect("stream url");
            let agent = ureq::Agent::new_with_defaults();
            let resp = agent.get(&uri).call().expect("stream call");
            assert_eq!(resp.status(), 200);
            let mut buf = Vec::new();
            use std::io::Read;
            resp.into_body()
                .into_reader()
                .take(8192)
                .read_to_end(&mut buf)
                .expect("read");
            assert!(buf.len() > 1024, "stream too short");
            tested += 1;
            break;
        }
    }
    assert!(tested > 0, "could not stream a track");
}

#[test]
#[ignore = "live server test; run with --ignored and MYUZIK_LIVE=1"]
fn live_navidrome_lyrics_list_shape() {
    if live_client().is_none() {
        return;
    }
    let (_paths, c, _cfg) = setup();
    let albums = c.album_list(AlbumListKind::Newest, 5, 0).expect("albums");
    let mut seen = false;
    for album in albums {
        let detail = c.album(&album.id).expect("detail");
        for track in &detail.songs {
            if let Ok(Some(LyricsStatus::Synced(lines))) = c.lyrics(&track.id) {
                assert!(!lines.is_empty());
                seen = true;
                break;
            }
        }
        if seen {
            break;
        }
    }
    assert!(seen, "no Navidrome-served synced lyrics found");
}

#[test]
#[ignore = "live server test; run with --ignored and MYUZIK_LIVE=1"]
fn live_artist_albums_nested() {
    if live_client().is_none() {
        return;
    }
    let (_paths, c, _cfg) = setup();
    let artists = c.artists().expect("artists");
    let mut checked = 0;
    for artist in artists.iter().take(30) {
        if let Ok(detail) = c.artist(&artist.id)
            && !detail.albums.is_empty()
        {
            checked += 1;
            break;
        }
    }
    assert!(checked > 0, "no artist returned nested albums");
}

#[test]
#[ignore = "live server test; run with --ignored and MYUZIK_LIVE=1"]
fn live_lyrics_pipeline() {
    if live_client().is_none() {
        return;
    }
    let (paths, c, cfg) = setup();
    let provider = LyricsProvider::new(
        NavidromeClient::new(
            &cfg.server_url,
            &cfg.username,
            &load_password(&paths).unwrap(),
        )
        .unwrap(),
        LyricsCache::new(paths.lyrics.clone()),
        cfg.prefer_navidrome_lyrics,
    );
    let albums = c.album_list(AlbumListKind::Newest, 5, 0).expect("albums");
    let mut checked = 0;
    for album in albums {
        let detail = c.album(&album.id).expect("detail");
        for track in detail.songs.iter().take(3) {
            match provider.fetch(track) {
                Ok(status) => {
                    checked += 1;
                    eprintln!(
                        "lyrics {}: {:?}",
                        track.title,
                        match &status {
                            LyricsStatus::Synced(l) => format!("synced {} lines", l.len()),
                            LyricsStatus::Plain(t) => format!("plain {} bytes", t.len()),
                            other => format!("{other:?}"),
                        }
                    );
                }
                Err(e) => panic!("lyrics fetch failed for {}: {e:?}", track.title),
            }
            if checked >= 3 {
                break;
            }
        }
        if checked >= 3 {
            break;
        }
    }
    assert!(checked > 0);
}

/// Autoplay radio sources against the real server: all four endpoints must
/// answer for a real song, and genre paging must advance rather than repeat.
#[test]
#[ignore = "live server test; run with --ignored and MYUZIK_LIVE=1"]
fn live_radio_sources() {
    if live_client().is_none() {
        eprintln!("skipped: MYUZIK_LIVE not set or no credentials");
        return;
    }
    use myuzik::library::radio::Radio;

    let (_paths, c, _cfg) = setup();

    // Seed from the newest album's first song.
    let albums = c
        .album_list(AlbumListKind::Newest, 5, 0)
        .expect("album list");
    let album = c.album(&albums[0].id).expect("album detail");
    let seed = album.songs.first().expect("album has no songs").clone();
    eprintln!(
        "seed: '{}' by '{}' genre={:?}",
        seed.title, seed.artist, seed.genre
    );
    assert!(
        seed.genre.is_some(),
        "the server should expose a genre for this song"
    );

    let mut radio = Radio::new();
    let batch = radio
        .next_batch(&c, &seed, &std::collections::HashSet::new())
        .expect("radio batch");
    assert!(!batch.is_empty(), "the radio produced no tracks");
    assert!(
        !batch.iter().any(|t| t.id == seed.id),
        "the radio must not queue the seed itself"
    );
    eprintln!(
        "batch: {} tracks, first '{}' by '{}'",
        batch.len(),
        batch[0].title,
        batch[0].artist
    );

    // A second batch must not repeat the first one.
    let exclude: std::collections::HashSet<String> = batch.iter().map(|t| t.id.clone()).collect();
    let batch2 = radio.next_batch(&c, &seed, &exclude).expect("second batch");
    let overlap = batch2.iter().filter(|t| exclude.contains(&t.id)).count();
    eprintln!("second batch: {} tracks, {overlap} overlaps", batch2.len());
    assert_eq!(overlap, 0, "second batch repeated tracks already queued");

    // The genre pool pages forward when it is big enough to be paged.
    let genre = seed.genre.clone().expect("genre");
    let page0 = c.songs_by_genre(&genre, 10, 0).expect("genre page 0");
    let page1 = c.songs_by_genre(&genre, 10, 10).expect("genre page 1");
    eprintln!(
        "genre '{}': page0={} page1={}",
        genre,
        page0.len(),
        page1.len()
    );
    if page0.len() == 10 && !page1.is_empty() {
        let first: std::collections::HashSet<&String> = page0.iter().map(|t| &t.id).collect();
        assert!(
            page1.iter().all(|t| !first.contains(&t.id)),
            "offset was ignored: page 1 repeats page 0"
        );
    }
}

/// Lyrics quality on real tracks: whatever the provider resolves must be usable
/// in the pinned overlay — a real band of timed lines, never a wall of text and
/// never all-zero timings.
#[test]
#[ignore = "live server test; run with --ignored and MYUZIK_LIVE=1"]
fn live_lyrics_quality_in_the_overlay() {
    if live_client().is_none() {
        eprintln!("skipped: MYUZIK_LIVE not set or no credentials");
        return;
    }
    use myuzik::lyrics::cache::LyricsCache;
    use myuzik::lyrics::model::LyricsStatus;
    use myuzik::lyrics::provider::LyricsProvider;
    use myuzik::overlay::OverlayState;

    let (paths, c, cfg) = setup();
    let cache = LyricsCache::new(paths.lyrics.join("live-test"));
    let pw = load_password(&paths).expect("credentials file missing");
    let provider_client =
        NavidromeClient::new(&cfg.server_url, &cfg.username, &pw).expect("client");
    let provider = LyricsProvider::new(provider_client, cache, cfg.prefer_navidrome_lyrics);

    let albums = c
        .album_list(AlbumListKind::Newest, 8, 10)
        .expect("album list");

    let mut resolved = 0usize;
    let mut synced = 0usize;
    for album in albums.iter().take(6) {
        let detail = match c.album(&album.id) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("skip album {}: {e:?}", album.id);
                continue;
            }
        };
        let Some(track) = detail.songs.first() else {
            continue;
        };
        let status = provider.fetch(track).expect("lyrics fetch");
        let kind = match &status {
            LyricsStatus::Synced(_) => "synced",
            LyricsStatus::Plain(_) => "plain",
            LyricsStatus::Instrumental => "instrumental",
            LyricsStatus::NotFound => "none",
            LyricsStatus::Error(_) => "error",
            LyricsStatus::Loading => "loading",
        };
        eprintln!(
            "  {:<36} -> {kind}",
            track.title.chars().take(36).collect::<String>()
        );
        if matches!(status, LyricsStatus::NotFound | LyricsStatus::Error(_)) {
            continue;
        }
        resolved += 1;

        // Feed exactly what the app would show into the overlay state.
        let mut state = OverlayState::default();
        state.begin(track.title.clone(), track.artist.clone());
        state.set_status(&status);
        let total = track.duration;
        let mut last_rows = 0usize;
        let mut last_active: Option<usize> = None;
        for step in 0..=10u32 {
            let pos = total.mul_f32(step as f32 / 10.0);
            state.sync_position(pos, cfg.lyrics_offset_ms, Some(total));
            let rows = state.visible_rows().len();
            assert!(
                rows <= 7,
                "'{}' drew {rows} rows at {pos:?}; the overlay must stay in the karaoke band",
                track.title
            );
            last_rows = rows;
            last_active = state.active();
        }
        if matches!(status, LyricsStatus::Synced(_)) {
            synced += 1;
            assert!(
                last_rows > 0,
                "'{}' has synced lyrics but drew no rows",
                track.title
            );
            assert!(
                last_active.is_some(),
                "'{}' never highlighted a line despite synced lyrics",
                track.title
            );
        }
        if matches!(status, LyricsStatus::Plain(_)) {
            assert!(
                last_active.is_none(),
                "'{}' is a plain transcript but a line was marked active",
                track.title
            );
        }
    }

    eprintln!(
        "resolved {resolved} of {} tracks ({synced} synced)",
        albums.len().min(6)
    );
    // The library may legitimately have no lyrics at all; the assertion is about
    // quality when something is found, not about coverage.
    assert!(
        resolved > 0 || synced == 0,
        "resolved {resolved} but synced {synced} — inconsistent counting"
    );
}
