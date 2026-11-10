//! Live GStreamer playback check. Run with MYUZIK_LIVE=1 cargo test --test live_player -- --nocapture

use myuzik::config::{Paths, load_config, load_password};
use myuzik::navidrome::client::{AlbumListKind, NavidromeClient};
use myuzik::player::engine::{PlayerCommand, PlayerEvent, spawn_player};
use myuzik::player::state::PlaybackStatus;
use std::time::{Duration, Instant};

#[test]
#[ignore = "live server test; run with --ignored and MYUZIK_LIVE=1"]
fn live_player_plays_pauses_seeks() {
    if std::env::var("MYUZIK_LIVE").ok().as_deref() != Some("1") {
        eprintln!("skipped: MYUZIK_LIVE not set");
        return;
    }
    let paths = Paths::new().unwrap();
    let cfg = load_config(&paths).unwrap();
    let pw = load_password(&paths).expect("password");
    let c = NavidromeClient::new(&cfg.server_url, &cfg.username, &pw).unwrap();
    let albums = c.album_list(AlbumListKind::Newest, 3, 0).unwrap();
    let detail = c.album(&albums[0].id).unwrap();
    let track = detail.songs.first().expect("track").clone();
    let uri = c.stream_url(&track.id, cfg.quality).unwrap();

    let h = spawn_player();
    h.tx.send(PlayerCommand::Load {
        uri,
        track_id: track.id.clone(),
    })
    .unwrap();
    h.tx.send(PlayerCommand::Play).unwrap();

    let mut position = Duration::ZERO;
    let mut got_duration = false;
    let mut playing = false;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        match h.rx.recv_timeout(Duration::from_millis(300)) {
            Ok(PlayerEvent::Position(p)) => position = p,
            Ok(PlayerEvent::Duration(_)) => got_duration = true,
            Ok(PlayerEvent::StateChanged(PlaybackStatus::Playing)) => playing = true,
            Ok(PlayerEvent::Error(e)) => panic!("player error: {e:?}"),
            _ => {}
        }
        if position > Duration::from_millis(1500) && got_duration && playing {
            break;
        }
    }
    assert!(playing, "player never reported Playing");
    assert!(got_duration, "no duration reported");
    assert!(
        position > Duration::from_millis(1500),
        "position did not advance (got {position:?})"
    );

    h.tx.send(PlayerCommand::Seek(Duration::from_secs(30)))
        .unwrap();
    let mut seeked = false;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(PlayerEvent::Position(p)) = h.rx.recv_timeout(Duration::from_millis(300))
            && p >= Duration::from_secs(25)
        {
            seeked = true;
            break;
        }
    }
    assert!(seeked, "seek did not move position");

    h.tx.send(PlayerCommand::Pause).unwrap();
    let mut paused = false;
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if let Ok(PlayerEvent::StateChanged(PlaybackStatus::Paused)) =
            h.rx.recv_timeout(Duration::from_millis(300))
        {
            paused = true;
            break;
        }
    }
    assert!(paused, "pause did not report Paused");
    h.tx.send(PlayerCommand::Stop).unwrap();
}
