use crate::library::{
    Album, AlbumDetail, Artist, ArtistDetail, Playlist, PlaylistDetail, SearchResults, Track,
};
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize, Debug, Default)]
pub struct SubsonicError {
    pub code: i32,
    pub message: String,
}

#[derive(Deserialize, Debug)]
pub struct SubsonicEnvelope<T> {
    #[serde(rename = "subsonic-response")]
    pub response: SubsonicResponse<T>,
}

#[derive(Deserialize, Debug)]
pub struct SubsonicResponse<T> {
    pub status: String,
    pub version: Option<String>,
    pub error: Option<SubsonicError>,
    #[serde(flatten)]
    pub data: T,
}

#[derive(Deserialize, Debug)]
pub struct PingData {}

#[derive(Deserialize, Debug, Default)]
pub struct AlbumList2 {
    #[serde(default)]
    pub album: Vec<AlbumDto>,
}

#[derive(Deserialize, Debug, Default)]
pub struct AlbumListData {
    #[serde(rename = "albumList2")]
    pub album_list2: Option<AlbumList2>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct AlbumDto {
    pub id: String,
    pub name: String,
    pub artist: Option<String>,
    #[serde(rename = "artistId")]
    pub artist_id: Option<String>,
    #[serde(rename = "coverArt")]
    pub cover_art: Option<String>,
    #[serde(rename = "songCount")]
    pub song_count: Option<u32>,
    pub duration: Option<i64>,
    pub year: Option<u32>,
    pub starred: Option<String>,
    #[serde(default)]
    pub song: Vec<SongDto>,
}

#[derive(Deserialize, Debug, Default)]
pub struct AlbumDetailData {
    pub album: Option<AlbumDto>,
    #[serde(default)]
    pub song: Vec<SongDto>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SongDto {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    #[serde(rename = "artistId")]
    pub artist_id: Option<String>,
    pub album: Option<String>,
    #[serde(rename = "albumId")]
    pub album_id: Option<String>,
    #[serde(rename = "coverArt")]
    pub cover_art: Option<String>,
    pub duration: Option<i64>,
    #[serde(rename = "track")]
    pub track_number: Option<u32>,
    #[serde(rename = "discNumber")]
    pub disc_number: Option<u32>,
    pub year: Option<u32>,
    pub starred: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
pub struct ArtistsData {
    pub artists: Option<ArtistsIndex>,
}

#[derive(Deserialize, Debug, Default)]
pub struct ArtistsIndex {
    #[serde(rename = "index")]
    pub indexes: Vec<ArtistIndex>,
}

#[derive(Deserialize, Debug, Default)]
pub struct ArtistIndex {
    #[serde(default)]
    pub artist: Vec<ArtistDto>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ArtistDto {
    pub id: String,
    pub name: String,
    #[serde(rename = "albumCount")]
    pub album_count: Option<u32>,
    #[serde(rename = "coverArt")]
    pub cover_art: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
pub struct ArtistDetailData {
    pub artist: Option<ArtistDto>,
    #[serde(default)]
    pub album: Vec<AlbumDto>,
}

#[derive(Deserialize, Debug, Default)]
pub struct PlaylistsData {
    pub playlists: Option<PlaylistIndex>,
}

#[derive(Deserialize, Debug, Default)]
pub struct PlaylistIndex {
    #[serde(default)]
    pub playlist: Vec<PlaylistDto>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PlaylistDto {
    pub id: String,
    pub name: String,
    #[serde(rename = "songCount")]
    pub song_count: Option<u32>,
    pub duration: Option<i64>,
    pub owner: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
pub struct PlaylistDetailData {
    pub playlist: Option<PlaylistDto>,
    #[serde(default)]
    pub entry: Vec<SongDto>,
}

#[derive(Deserialize, Debug, Default)]
pub struct Search3Data {
    #[serde(rename = "searchResult3")]
    pub search_result3: Option<SearchResult3>,
}

#[derive(Deserialize, Debug, Default)]
pub struct SearchResult3 {
    #[serde(default)]
    pub artist: Vec<ArtistDto>,
    #[serde(default)]
    pub album: Vec<AlbumDto>,
    #[serde(default)]
    pub song: Vec<SongDto>,
    #[serde(default)]
    pub playlist: Vec<PlaylistDto>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LyricsDto {
    #[serde(rename = "lyricsList")]
    pub lyrics_list: Option<LyricsList>,
    #[serde(rename = "structuredLyrics", default)]
    pub structured: Vec<StructuredLyrics>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct LyricsList {
    #[serde(rename = "structuredLyrics", default)]
    pub structured: Vec<StructuredLyrics>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct StructuredLyrics {
    #[serde(rename = "synced", default)]
    pub synced: bool,
    #[serde(rename = "line", default)]
    pub lines: Vec<LyricLineDto>,
    #[serde(rename = "displayArtist", default)]
    pub display_artist: Option<String>,
    #[serde(rename = "displayTitle", default)]
    pub display_title: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LyricLineDto {
    pub start: Option<i64>,
    pub value: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ServerInfo {
    pub version: Option<String>,
    pub server_version: Option<String>,
    pub open_subsonic: bool,
}

// ---- mapping helpers ----

pub fn song_dto_to_track(s: SongDto) -> Track {
    Track {
        id: s.id,
        title: s.title,
        artist: s.artist.unwrap_or_default(),
        artist_id: s.artist_id,
        album: s.album.unwrap_or_default(),
        album_id: s.album_id,
        cover_art_id: s.cover_art,
        duration: Duration::from_secs(s.duration.unwrap_or(0).max(0) as u64),
        track_number: s.track_number,
        disc_number: s.disc_number,
        year: s.year,
        starred: s.starred.is_some(),
    }
}

pub fn album_dto_to_album(a: AlbumDto) -> Album {
    Album {
        id: a.id,
        name: a.name,
        artist: a.artist.unwrap_or_default(),
        artist_id: a.artist_id,
        cover_art_id: a.cover_art,
        song_count: a.song_count,
        duration: a.duration.map(|d| Duration::from_secs(d.max(0) as u64)),
        year: a.year,
        starred: a.starred.is_some(),
    }
}

pub fn artist_dto_to_artist(a: ArtistDto) -> Artist {
    Artist {
        id: a.id,
        name: a.name,
        album_count: a.album_count,
        cover_art_id: a.cover_art,
    }
}

pub fn playlist_dto_to_playlist(p: PlaylistDto) -> Playlist {
    Playlist {
        id: p.id,
        name: p.name,
        song_count: p.song_count.unwrap_or(0),
        duration: Duration::from_secs(p.duration.unwrap_or(0).max(0) as u64),
        owner: p.owner.unwrap_or_default(),
    }
}

pub fn album_detail_from_dto(album: AlbumDto, songs: Vec<SongDto>) -> AlbumDetail {
    AlbumDetail {
        album: album_dto_to_album(album),
        songs: songs.into_iter().map(song_dto_to_track).collect(),
    }
}

pub fn artist_detail_from_dto(artist: ArtistDto, albums: Vec<AlbumDto>) -> ArtistDetail {
    ArtistDetail {
        artist: artist_dto_to_artist(artist),
        albums: albums.into_iter().map(album_dto_to_album).collect(),
    }
}

pub fn playlist_detail_from_dto(playlist: PlaylistDto, entries: Vec<SongDto>) -> PlaylistDetail {
    PlaylistDetail {
        playlist: playlist_dto_to_playlist(playlist),
        songs: entries.into_iter().map(song_dto_to_track).collect(),
    }
}

pub fn search_results_from_dto(s: SearchResult3) -> SearchResults {
    SearchResults {
        artists: s.artist.into_iter().map(artist_dto_to_artist).collect(),
        albums: s.album.into_iter().map(album_dto_to_album).collect(),
        songs: s.song.into_iter().map(song_dto_to_track).collect(),
        playlists: s
            .playlist
            .into_iter()
            .map(playlist_dto_to_playlist)
            .collect(),
    }
}

pub fn lyrics_from_dto(dto: LyricsDto) -> Option<crate::lyrics::model::LyricsStatus> {
    use crate::lyrics::model::LyricsStatus;
    let sl = dto
        .lyrics_list
        .map(|l| l.structured)
        .unwrap_or_default()
        .into_iter()
        .chain(dto.structured)
        .next()?;
    // OpenSubsonic lines: start is ms, value is text.
    let mut lines: Vec<crate::lyrics::model::LyricLine> = sl
        .lines
        .into_iter()
        .map(|l| crate::lyrics::model::LyricLine {
            time: Duration::from_millis(l.start.unwrap_or(0).max(0) as u64),
            text: l.value.unwrap_or_default(),
        })
        .collect();
    lines.sort_by_key(|l| l.time);
    if sl.synced && !lines.is_empty() {
        Some(LyricsStatus::Synced(lines))
    } else {
        // If there are plain lines, join as plain.
        let plain: String = lines
            .iter()
            .map(|l| l.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        if plain.trim().is_empty() {
            Some(LyricsStatus::NotFound)
        } else {
            Some(LyricsStatus::Plain(plain))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lyrics_list_wrapper() {
        let json = r#"{"lyricsList":{"structuredLyrics":[{"synced":true,"line":[{"start":1000,"value":"one"},{"start":2000,"value":"two"}]}]}}"#;
        let dto: LyricsDto = serde_json::from_str(json).unwrap();
        match lyrics_from_dto(dto) {
            Some(crate::lyrics::model::LyricsStatus::Synced(lines)) => {
                assert_eq!(lines.len(), 2);
                assert_eq!(lines[1].text, "two");
            }
            other => panic!("expected synced, got {other:?}"),
        }
    }

    #[test]
    fn parses_legacy_structured_lyrics() {
        let json = r#"{"structuredLyrics":[{"synced":false,"line":[{"start":0,"value":"plain a"},{"start":0,"value":"plain b"}]}]}"#;
        let dto: LyricsDto = serde_json::from_str(json).unwrap();
        match lyrics_from_dto(dto) {
            Some(crate::lyrics::model::LyricsStatus::Plain(text)) => {
                assert!(text.contains("plain a") && text.contains("plain b"));
            }
            other => panic!("expected plain, got {other:?}"),
        }
    }
}
