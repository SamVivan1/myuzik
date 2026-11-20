use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrackSource {
    #[default]
    #[serde(rename = "navidrome")]
    Navidrome,
    #[serde(rename = "youtube")]
    YouTube,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    /// Which provider produced this track. Defaults to Navidrome so caches
    /// written before the provider layer still load.
    #[serde(default)]
    pub source: TrackSource,
    /// Cover URL for providers that do not use Navidrome cover IDs.
    #[serde(default)]
    pub cover_url: Option<String>,
}

/// Stable cache key for a remote cover URL. YouTube thumbnails and any future
/// URL-based provider use the same hashing so the disk cache is shared.
pub fn url_cover_key(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    format!("yt-{}", hex::encode(hasher.finalize()))
}

impl Track {
    pub fn is_youtube(&self) -> bool {
        self.source == TrackSource::YouTube
    }

    /// The 11-character YouTube video id, when this is a YouTube track.
    pub fn video_id(&self) -> Option<&str> {
        let id = self.id.strip_prefix("yt:")?;
        (id.len() == 11
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
        .then_some(id)
    }

    /// The key the cover cache uses. YouTube thumbnails are addressed by URL,
    /// so they get a stable hash instead of a Navidrome cover id.
    pub fn cover_key(&self) -> Option<String> {
        if let Some(url) = &self.cover_url {
            return Some(url_cover_key(url));
        }
        self.cover_art_id.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    /// Which provider produced this album. Defaults to Navidrome so caches
    /// written before the provider layer still load.
    #[serde(default)]
    pub source: TrackSource,
    /// Remote cover URL for providers that do not use Navidrome cover IDs.
    #[serde(default)]
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AlbumDetail {
    pub album: Album,
    pub songs: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub album_count: Option<u32>,
    pub cover_art_id: Option<String>,
    #[serde(default)]
    pub source: TrackSource,
    #[serde(default)]
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ArtistDetail {
    pub artist: Artist,
    pub albums: Vec<Album>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub song_count: u32,
    pub duration: std::time::Duration,
    pub owner: String,
    #[serde(default)]
    pub source: TrackSource,
    #[serde(default)]
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlaylistDetail {
    pub playlist: Playlist,
    pub songs: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct SearchResults {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Track>,
    pub playlists: Vec<Playlist>,
    /// Results from the optional YouTube Music provider. Kept separate so the
    /// UI can label the source and never confuses it with the user's library.
    #[serde(default)]
    pub youtube: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct HomeData {
    pub recently_added: Vec<Album>,
    pub recently_played: Vec<Album>,
    pub playlists: Vec<Playlist>,
    pub random: Vec<Album>,
}

pub mod cache;
pub mod cover_cache;
pub mod radio;
pub mod search_index;

pub use radio::Radio;
