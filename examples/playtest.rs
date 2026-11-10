use myuzik::player::engine::{PlayerCommand, spawn_player};
use std::time::Duration;

fn main() {
    let paths = myuzik::config::Paths::new().unwrap();
    let cfg = myuzik::config::load_config(&paths).unwrap();
    let pw = myuzik::config::load_password(&paths).expect("no password");
    let c = myuzik::navidrome::client::NavidromeClient::new(&cfg.server_url, &cfg.username, &pw)
        .unwrap();
    let albums = c
        .album_list(myuzik::navidrome::client::AlbumListKind::Newest, 1, 0)
        .unwrap();
    let detail = c.album(&albums[0].id).unwrap();
    let track = &detail.songs[0];
    let uri = c.stream_url(&track.id, cfg.quality).unwrap();
    println!(
        "playing: {} - {} ({}s)",
        track.title,
        track.artist,
        track.duration.as_secs()
    );
    let handle = spawn_player();
    handle
        .tx
        .send(PlayerCommand::Load {
            uri,
            track_id: track.id.clone(),
        })
        .unwrap();
    handle.tx.send(PlayerCommand::Play).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        if let Ok(ev) = handle.rx.recv_timeout(Duration::from_millis(500)) {
            println!("event: {ev:?}");
        }
    }
}
