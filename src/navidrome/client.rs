use crate::config::StreamQuality;
use crate::library::{Album, AlbumDetail, Artist, ArtistDetail, Playlist, PlaylistDetail, SearchResults};
use crate::lyrics::model::LyricsStatus;
use crate::navidrome::auth::{auth_params, random_salt};
use crate::navidrome::error::NavidromeError;
use crate::navidrome::types::*;
use url::Url;

#[derive(Debug, Clone, Copy)]
pub enum AlbumListKind {
    Newest,
    Recent,
    Random,
    Frequent,
    Starred,
}

impl AlbumListKind {
    fn as_str(&self) -> &'static str {
        match self {
            AlbumListKind::Newest => "newest",
            AlbumListKind::Recent => "recent",
            AlbumListKind::Random => "random",
            AlbumListKind::Frequent => "frequent",
            AlbumListKind::Starred => "starred",
        }
    }
}

pub struct NavidromeClient {
    base_url: Url,
    username: String,
    password: String,
    agent: ureq::Agent,
}

impl NavidromeClient {
    pub fn new(base_url: &str, username: &str, password: &str) -> Result<Self, NavidromeError> {
        let mut base_url = Url::parse(base_url).map_err(|e| NavidromeError::Protocol(e.to_string()))?;
        if !base_url.path().ends_with('/') {
            base_url = base_url.join("/").map_err(|e| NavidromeError::Protocol(e.to_string()))?;
        }
        let config = ureq::Agent::config_builder()
            .user_agent("myuzik/0.1 (https://github.com/SamVivan1/myuzik)")
            .build();
        let agent = ureq::Agent::new_with_config(config);
        Ok(Self {
            base_url,
            username: username.to_string(),
            password: password.to_string(),
            agent,
        })
    }

    fn auth_params(&self) -> Vec<(&'static str, String)> {
        auth_params(&self.username, &self.password, &random_salt())
    }

    fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        extra: &[(&str, &str)],
    ) -> Result<T, NavidromeError> {
        let mut url = self
            .base_url
            .join(&format!("rest/{}.view", endpoint))
            .map_err(|e| NavidromeError::Protocol(e.to_string()))?;
        for (k, v) in self.auth_params() {
            url.query_pairs_mut().append_pair(k, &v);
        }
        for (k, v) in extra {
            url.query_pairs_mut().append_pair(k, v);
        }

        let resp = self.agent.get(url.as_str()).call().map_err(NavidromeError::from)?;
        let value: serde_json::Value = resp.into_body().read_json().map_err(NavidromeError::from)?;
        let inner = match value {
            serde_json::Value::Object(mut map) => map
                .remove("subsonic-response")
                .unwrap_or(serde_json::Value::Object(map)),
            other => other,
        };
        let parsed: T = serde_json::from_value(inner)?;
        Ok(parsed)
    }

    fn check_status<T>(resp: &SubsonicResponse<T>) -> Result<(), NavidromeError> {
        if resp.status == "ok" {
            Ok(())
        } else {
            let msg = resp
                .error
                .as_ref()
                .map(|e| format!("{}: {}", e.code, e.message))
                .unwrap_or_else(|| "unknown server error".to_string());
            let lower = msg.to_lowercase();
            if lower.contains("not found") || lower.contains("missing parameter") {
                Err(NavidromeError::NotFound)
            } else if lower.contains("wrong username") || lower.contains("password") || lower.contains("unauthorized") {
                Err(NavidromeError::Unauthorized)
            } else if lower.contains("not supported") || lower.contains("not implemented") {
                Err(NavidromeError::Unsupported)
            } else {
                Err(NavidromeError::Server(msg))
            }
        }
    }

    pub fn ping(&self) -> Result<crate::navidrome::types::ServerInfo, NavidromeError> {
        let resp: SubsonicResponse<PingData> = self.get_json("ping", &[])?;
        Self::check_status(&resp)?;
        Ok(crate::navidrome::types::ServerInfo {
            version: resp.version,
            server_version: None,
            open_subsonic: false,
        })
    }

    pub fn album_list(&self, kind: AlbumListKind, size: u32, offset: u32) -> Result<Vec<Album>, NavidromeError> {
        let kind_str = kind.as_str();
        let size_s = size.to_string();
        let offset_s = offset.to_string();
        let resp: SubsonicResponse<AlbumListData> =
            self.get_json("getAlbumList2", &[("type", kind_str), ("size", &size_s), ("offset", &offset_s)])?;
        Self::check_status(&resp)?;
        let albums = resp.data.album_list2.map(|a| a.album).unwrap_or_default();
        Ok(albums.into_iter().map(album_dto_to_album).collect())
    }

    pub fn artists(&self) -> Result<Vec<Artist>, NavidromeError> {
        let resp: SubsonicResponse<ArtistsData> = self.get_json("getArtists", &[])?;
        Self::check_status(&resp)?;
        let mut out = Vec::new();
        if let Some(idx) = resp.data.artists {
            for i in idx.indexes {
                out.extend(i.artist.into_iter().map(artist_dto_to_artist));
            }
        }
        Ok(out)
    }

    pub fn artist(&self, id: &str) -> Result<ArtistDetail, NavidromeError> {
        let resp: SubsonicResponse<ArtistDetailData> = self.get_json("getArtist", &[("id", id)])?;
        Self::check_status(&resp)?;
        let artist = resp.data.artist.ok_or(NavidromeError::NotFound)?;
        Ok(artist_detail_from_dto(artist, resp.data.album))
    }

    pub fn album(&self, id: &str) -> Result<AlbumDetail, NavidromeError> {
        let resp: SubsonicResponse<AlbumDetailData> = self.get_json("getAlbum", &[("id", id)])?;
        Self::check_status(&resp)?;
        let album = resp.data.album.ok_or(NavidromeError::NotFound)?;
        let songs = if album.song.is_empty() { resp.data.song } else { album.song.clone() };
        Ok(album_detail_from_dto(album, songs))
    }

    pub fn playlists(&self) -> Result<Vec<Playlist>, NavidromeError> {
        let resp: SubsonicResponse<PlaylistsData> = self.get_json("getPlaylists", &[])?;
        Self::check_status(&resp)?;
        let pls = resp.data.playlists.map(|p| p.playlist).unwrap_or_default();
        Ok(pls.into_iter().map(playlist_dto_to_playlist).collect())
    }

    pub fn playlist(&self, id: &str) -> Result<PlaylistDetail, NavidromeError> {
        let resp: SubsonicResponse<PlaylistDetailData> = self.get_json("getPlaylist", &[("id", id)])?;
        Self::check_status(&resp)?;
        let pl = resp.data.playlist.ok_or(NavidromeError::NotFound)?;
        Ok(playlist_detail_from_dto(pl, resp.data.entry))
    }

    pub fn search(&self, query: &str) -> Result<SearchResults, NavidromeError> {
        let resp: SubsonicResponse<Search3Data> = self.get_json(
            "search3",
            &[("query", query), ("artistCount", "20"), ("albumCount", "20"), ("songCount", "40")],
        )?;
        Self::check_status(&resp)?;
        Ok(search_results_from_dto(resp.data.search_result3.unwrap_or_default()))
    }

    pub fn stream_url(&self, track_id: &str, quality: StreamQuality) -> Result<String, NavidromeError> {
        let mut url = self
            .base_url
            .join("rest/stream.view")
            .map_err(|e| NavidromeError::Protocol(e.to_string()))?;
        for (k, v) in self.auth_params() {
            url.query_pairs_mut().append_pair(k, &v);
        }
        url.query_pairs_mut().append_pair("id", track_id);
        match quality {
            StreamQuality::Original => {}
            StreamQuality::Kbps320 => {
                url.query_pairs_mut().append_pair("format", "mp3");
                url.query_pairs_mut().append_pair("maxBitRate", "320");
            }
            StreamQuality::Kbps192 => {
                url.query_pairs_mut().append_pair("format", "mp3");
                url.query_pairs_mut().append_pair("maxBitRate", "192");
            }
        }
        Ok(url.to_string())
    }

    pub fn cover_url(&self, cover_id: &str, size: u32) -> Result<String, NavidromeError> {
        let mut url = self
            .base_url
            .join("rest/getCoverArt.view")
            .map_err(|e| NavidromeError::Protocol(e.to_string()))?;
        for (k, v) in self.auth_params() {
            url.query_pairs_mut().append_pair(k, &v);
        }
        url.query_pairs_mut().append_pair("id", cover_id);
        url.query_pairs_mut().append_pair("size", &size.to_string());
        Ok(url.to_string())
    }

    pub fn cover_bytes(&self, cover_id: &str, size: u32) -> Result<Vec<u8>, NavidromeError> {
        let url = self.cover_url(cover_id, size)?;
        let resp = self.agent.get(&url).call().map_err(NavidromeError::from)?;
        let mut bytes = Vec::new();
        use std::io::Read;
        resp.into_body()
            .into_reader()
            .take(8 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .map_err(NavidromeError::from)?;
        Ok(bytes)
    }

    pub fn set_starred(&self, id: &str, starred: bool) -> Result<(), NavidromeError> {
        let endpoint = if starred { "star" } else { "unstar" };
        let resp: SubsonicResponse<PingData> = self.get_json(endpoint, &[("id", id)])?;
        Self::check_status(&resp)?;
        Ok(())
    }

    pub fn scrobble(&self, track_id: &str, submission: bool, time_ms: i64) -> Result<(), NavidromeError> {
        let sub = if submission { "true" } else { "false" };
        let time = time_ms.to_string();
        let resp: SubsonicResponse<PingData> =
            self.get_json("scrobble", &[("id", track_id), ("submission", sub), ("time", &time)])?;
        Self::check_status(&resp)?;
        Ok(())
    }

    pub fn lyrics(&self, track_id: &str) -> Result<Option<LyricsStatus>, NavidromeError> {
        let resp: SubsonicResponse<LyricsDto> = match self.get_json("getLyricsBySongId", &[("id", track_id)]) {
            Ok(r) => r,
            Err(NavidromeError::NotFound) | Err(NavidromeError::Unsupported) => return Ok(None),
            Err(e) => return Err(e),
        };
        if resp.status != "ok" {
            return Ok(None);
        }
        // OpenSubsonic returns status ok even if no lyrics; check error.
        if let Some(err) = &resp.error {
            let lower = err.message.to_lowercase();
            if lower.contains("not found") || lower.contains("no lyrics") {
                return Ok(None);
            }
        }
        // If the endpoint is unsupported, Navidrome may return error code 70.
        if let Some(err) = &resp.error
            && err.code == 70 {
            return Err(NavidromeError::Unsupported);
        }
        Ok(crate::navidrome::types::lyrics_from_dto(resp.data))
    }
}
