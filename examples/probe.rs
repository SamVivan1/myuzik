fn main() {
    let paths = myuzik::config::Paths::new().unwrap();
    let cfg = myuzik::config::load_config(&paths).unwrap();
    let pw = myuzik::config::load_password(&paths).expect("no password");
    let c = myuzik::navidrome::client::NavidromeClient::new(&cfg.server_url, &cfg.username, &pw).unwrap();
    println!("ping: {:?}", c.ping());
    match c.album_list(myuzik::navidrome::client::AlbumListKind::Newest, 20, 0) {
        Ok(a) => {
            println!("albums: {}", a.len());
            for x in a.iter().take(3) { println!(" - {} / {} ({})", x.name, x.artist, x.id); }
            if let Some(first) = a.first() {
                match c.album(&first.id) {
                    Ok(d) => println!("detail {}: {} songs", d.album.name, d.songs.len()),
                    Err(e) => println!("detail err: {e:?}"),
                }
                if let Some(cid) = &first.cover_art_id {
                    match c.cover_bytes(cid, 512) {
                        Ok(b) => println!("cover bytes: {} magic: {:02x?}", b.len(), &b[..4.min(b.len())]),
                        Err(e) => println!("cover err: {e:?}"),
                    }
                }
            }
        }
        Err(e) => println!("album err: {e:?}"),
    }
}
