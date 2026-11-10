fn main() {
    let paths = myuzik::config::Paths::new().unwrap();
    let cfg = myuzik::config::load_config(&paths).unwrap();
    let pw = myuzik::config::load_password(&paths).expect("no password");
    let c = myuzik::navidrome::client::NavidromeClient::new(&cfg.server_url, &cfg.username, &pw).unwrap();
    println!("ping: {:?}", c.ping());
    println!("artists: {:?}", c.artists().map(|a| a.len()));
    println!("playlists: {:?}", c.playlists().map(|p| p.len()));
    match c.search("billie") {
        Ok(r) => println!("search: {} songs, {} albums, {} artists", r.songs.len(), r.albums.len(), r.artists.len()),
        Err(e) => println!("search err: {e:?}"),
    }
    let albums = match c.album_list(myuzik::navidrome::client::AlbumListKind::Newest, 20, 0) {
        Ok(a) => a,
        Err(e) => { println!("album err: {e:?}"); return; }
    };
    println!("albums: {}", albums.len());
    let first = &albums[0];
    println!(" - {} / {} ({})", first.name, first.artist, first.id);
    match c.album(&first.id) {
        Ok(d) => {
            println!("detail {}: {} songs", d.album.name, d.songs.len());
            if let Some(song) = d.songs.first() {
                match c.lyrics(&song.id) {
                    Ok(l) => println!("lyrics for {}: {:?}", song.title, l.map(|x| format!("{:?}", x).chars().take(80).collect::<String>())),
                    Err(e) => println!("lyrics err: {e:?}"),
                }
            }
        }
        Err(e) => println!("detail err: {e:?}"),
    }
    if let Some(cid) = &first.cover_art_id {
        match c.cover_bytes(cid, 512) {
            Ok(b) => println!("cover bytes: {} magic: {:02x?}", b.len(), &b[..4.min(b.len())]),
            Err(e) => println!("cover err: {e:?}"),
        }
    }
}
