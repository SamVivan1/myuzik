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
