#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: Option<String>,
    pub album: String,
    pub album_id: Option<String>,
    pub cover_art_id: Option<String>,
    pub duration: std::time::Duration,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<u32>,
    pub starred: bool,
    /// Genre from the server, used to seed autoplay radio.
    pub genre: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Album {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub artist_id: Option<String>,
    pub cover_art_id: Option<String>,
    pub song_count: Option<u32>,
    pub duration: Option<std::time::Duration>,
    pub year: Option<u32>,
    pub starred: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumDetail {
    pub album: Album,
    pub songs: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub album_count: Option<u32>,
    pub cover_art_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistDetail {
    pub artist: Artist,
    pub albums: Vec<Album>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub song_count: u32,
    pub duration: std::time::Duration,
    pub owner: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistDetail {
    pub playlist: Playlist,
    pub songs: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResults {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Track>,
    pub playlists: Vec<Playlist>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HomeData {
    pub recently_added: Vec<Album>,
    pub recently_played: Vec<Album>,
    pub playlists: Vec<Playlist>,
    pub random: Vec<Album>,
}

pub mod radio;

pub use radio::Radio;
