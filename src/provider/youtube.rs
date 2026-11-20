//! YouTube Music search and buffered playback.
//!
//! The provider never talks to YouTube itself. It runs a Python helper once per
//! request (`helper/ytmusic.py`), which uses ytmusicapi for the catalog and
//! yt-dlp for audio. The helper downloads a track to a private cache directory;
//! the player receives a normal local file URI.
//!
//! Why buffer instead of streaming the signed URL directly: the URL expires,
//! seeking depends on range/header behaviour, and both MPRIS position and the
//! lyric overlay need a stable clock. AudioTube streams directly and disables
//! seeking for this reason; Sung buffers and so do we.

use crate::config::YouTubeQuality;
use crate::library::{
    Album, AlbumDetail, Artist, ArtistDetail, Playlist, PlaylistDetail, Track, TrackSource,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

const SEARCH_TIMEOUT: Duration = Duration::from_secs(30);
const BROWSE_TIMEOUT: Duration = Duration::from_secs(45);
const BUFFER_TIMEOUT: Duration = Duration::from_secs(120);
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HELPER_OUTPUT: usize = 2 * 1024 * 1024;
const MAX_TRACK_BYTES: u64 = 32 * 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("YouTube support is not installed")]
    HelperMissing,
    #[error("Python 3 was not found")]
    PythonMissing,
    #[error("Node.js 20+ is required for YouTube playback")]
    NodeMissing,
    #[error("YouTube took too long to respond")]
    Timeout,
    #[error("YouTube request cancelled")]
    Cancelled,
    #[error("{0}")]
    Unavailable(String),
    #[error("This track requires sign-in")]
    SignInRequired,
    #[error("This track is too large to buffer")]
    TooLarge,
    #[error("{0}")]
    Network(String),
    #[error("{0}")]
    Protocol(String),
}

/// One row in a YouTube home shelf or a browse grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowseCard {
    Song(Track),
    Album(Album),
    Playlist(Playlist),
    Artist(Artist),
}

impl BrowseCard {
    pub fn title(&self) -> &str {
        match self {
            Self::Song(track) => &track.title,
            Self::Album(album) => &album.name,
            Self::Playlist(playlist) => &playlist.name,
            Self::Artist(artist) => &artist.name,
        }
    }

    pub fn cover_url(&self) -> Option<&str> {
        match self {
            Self::Song(track) => track.cover_url.as_deref(),
            Self::Album(album) => album.cover_url.as_deref(),
            Self::Playlist(playlist) => playlist.cover_url.as_deref(),
            Self::Artist(artist) => artist.cover_url.as_deref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BrowseSection {
    pub title: String,
    pub items: Vec<BrowseCard>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mood {
    pub title: String,
    pub params: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoodCategory {
    pub title: String,
    pub moods: Vec<Mood>,
}

#[derive(Debug, Clone)]
pub struct BufferedStream {
    pub file: PathBuf,
    pub duration: Duration,
    /// Size of the whole stream cache after this buffer was written and pruned.
    pub cache_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct HelperVersion {
    pub ytmusicapi: String,
    pub yt_dlp: String,
    pub python: String,
}

pub struct YouTubeProvider {
    helper: PathBuf,
    python: PathBuf,
    cache_dir: PathBuf,
    quality: YouTubeQuality,
    cookies: Option<PathBuf>,
    max_cache_bytes: u64,
}

impl YouTubeProvider {
    pub fn new(
        cache_dir: PathBuf,
        quality: YouTubeQuality,
        cookies: Option<PathBuf>,
        max_cache_mb: u64,
    ) -> Self {
        Self::with_paths(
            helper_path(),
            python_path(),
            cache_dir,
            quality,
            cookies,
            max_cache_mb,
        )
    }

    /// Explicit paths for tests and for a packaged helper installed elsewhere.
    pub fn with_paths(
        helper: PathBuf,
        python: PathBuf,
        cache_dir: PathBuf,
        quality: YouTubeQuality,
        cookies: Option<PathBuf>,
        max_cache_mb: u64,
    ) -> Self {
        let _ = fs::create_dir_all(&cache_dir);
        tracing::debug!(
            helper = %helper.display(),
            python = %python.display(),
            cache = %cache_dir.display(),
            "YouTube provider configured"
        );
        Self {
            helper,
            python,
            cache_dir,
            quality,
            cookies,
            max_cache_bytes: max_cache_mb.max(64) * 1024 * 1024,
        }
    }

    pub fn helper_present(&self) -> bool {
        self.helper.is_file()
    }

    /// Whether the default helper is installed, independent of whether the
    /// source is currently enabled. Settings uses this so turning YouTube off
    /// does not make a working install look missing.
    pub fn helper_installed() -> bool {
        helper_path().is_file()
    }

    pub fn python_present(&self) -> bool {
        self.python.is_file() || self.python == Path::new("python3")
    }

    pub fn helper_path(&self) -> &Path {
        &self.helper
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn search(
        &self,
        query: &str,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Vec<Track>, ProviderError> {
        let request = json!({
            "op": "search",
            "query": query,
            "limit": limit.clamp(1, 50),
            "filter": "songs",
        });
        let value = self.call_helper(&request, SEARCH_TIMEOUT, cancel)?;
        Ok(tracks_from_value(&value))
    }

    /// Fetch YouTube Music's radio/watch playlist for a seed song.
    ///
    /// The real client opens a song and queues the song's radio, not the search
    /// results that happened to contain it. AudioTube does this with
    /// `get_watch_playlist`; Sung's `radio` op uses `radio=True`.
    pub fn radio(
        &self,
        seed: &Track,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Vec<Track>, ProviderError> {
        let video_id = seed
            .video_id()
            .ok_or_else(|| ProviderError::Protocol("YouTube track has no video id".to_string()))?;
        let request = json!({
            "op": "radio",
            "id": video_id,
            "limit": limit.clamp(1, 50),
        });
        let value = self.call_helper(&request, SEARCH_TIMEOUT, cancel)?;
        Ok(tracks_from_value(&value))
    }

    /// The personalised YouTube Music home feed, grouped into shelves.
    pub fn home(
        &self,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Vec<BrowseSection>, ProviderError> {
        let request = json!({
            "op": "home",
            "limit": limit.clamp(1, 12),
        });
        let value = self.call_helper(&request, BROWSE_TIMEOUT, cancel)?;
        Ok(sections_from_value(&value))
    }

    pub fn album(&self, id: &str, cancel: &AtomicBool) -> Result<AlbumDetail, ProviderError> {
        let id = youtube_browse_id(id, "MPRE")
            .ok_or_else(|| ProviderError::Protocol("Invalid YouTube album id".to_string()))?;
        let value = self.call_helper(&json!({"op": "album", "id": id}), BROWSE_TIMEOUT, cancel)?;
        album_detail_from_value(&value)
    }

    pub fn artist(&self, id: &str, cancel: &AtomicBool) -> Result<ArtistDetail, ProviderError> {
        let id = youtube_browse_id(id, "UC")
            .ok_or_else(|| ProviderError::Protocol("Invalid YouTube artist id".to_string()))?;
        let value = self.call_helper(&json!({"op": "artist", "id": id}), BROWSE_TIMEOUT, cancel)?;
        artist_detail_from_value(&value)
    }

    pub fn playlist(&self, id: &str, cancel: &AtomicBool) -> Result<PlaylistDetail, ProviderError> {
        let id = playlist_id(id)
            .ok_or_else(|| ProviderError::Protocol("Invalid YouTube playlist id".to_string()))?;
        let value = self.call_helper(
            &json!({"op": "playlist", "id": id, "limit": 200}),
            BROWSE_TIMEOUT,
            cancel,
        )?;
        playlist_detail_from_value(&value)
    }

    pub fn moods(&self, cancel: &AtomicBool) -> Result<Vec<MoodCategory>, ProviderError> {
        let value = self.call_helper(&json!({"op": "moods"}), BROWSE_TIMEOUT, cancel)?;
        Ok(mood_categories_from_value(&value))
    }

    pub fn mood_playlists(
        &self,
        params: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<Playlist>, ProviderError> {
        let value = self.call_helper(
            &json!({"op": "mood", "params": params}),
            BROWSE_TIMEOUT,
            cancel,
        )?;
        Ok(playlists_from_value(&value))
    }

    /// Buffer a track, optionally swapping the preferred container. A failed
    /// WebM attempt can succeed as m4a; the caller retries once.
    pub fn buffer(
        &self,
        track: &Track,
        cancel: &AtomicBool,
    ) -> Result<BufferedStream, ProviderError> {
        self.buffer_with_fallback(track, cancel, false)
    }

    pub fn buffer_with_fallback(
        &self,
        track: &Track,
        cancel: &AtomicBool,
        fallback: bool,
    ) -> Result<BufferedStream, ProviderError> {
        let video_id = video_id(track)
            .ok_or_else(|| ProviderError::Protocol("YouTube track has no video id".to_string()))?;
        let directory = self.cache_dir.join(format!(
            "{}-{}",
            cache_key(video_id, self.quality),
            self.quality.as_str()
        ));
        create_private_dir(&directory)?;

        // Replaying a track should not spawn the helper or touch the network.
        // The directory is keyed by video id and quality, and only a completed
        // audio file (never a `.part`) is accepted.
        if !fallback && let Some(file) = cached_file(&directory) {
            return Ok(BufferedStream {
                file,
                duration: track.duration,
                cache_bytes: self.cache_bytes(),
            });
        }

        let request = json!({
            "op": "buffer",
            "id": video_id,
            "directory": directory,
            "root": self.cache_dir,
            "quality": self.quality.as_str(),
            "cookies": self.cookies.as_ref().map(|p| p.to_string_lossy()).unwrap_or_default(),
            "fallback": fallback,
        });
        let value = self.call_helper(&request, BUFFER_TIMEOUT, cancel)?;
        let file = value
            .get("file")
            .and_then(Value::as_str)
            .ok_or_else(|| ProviderError::Protocol("helper returned no file".to_string()))?;
        let seconds = value.get("seconds").and_then(Value::as_u64).unwrap_or(0);
        let path = PathBuf::from(file);
        let canonical = fs::canonicalize(&path).map_err(|error| {
            ProviderError::Protocol(format!("buffered file is missing: {error}"))
        })?;
        let root = fs::canonicalize(&directory)
            .map_err(|error| ProviderError::Protocol(format!("cache directory lost: {error}")))?;
        if !canonical.starts_with(&root) {
            return Err(ProviderError::Protocol(
                "helper returned a file outside the cache directory".to_string(),
            ));
        }
        let metadata = fs::metadata(&canonical)
            .map_err(|error| ProviderError::Protocol(format!("stat buffered file: {error}")))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(ProviderError::Protocol(
                "helper returned an empty or non-regular file".to_string(),
            ));
        }
        if metadata.len() > MAX_TRACK_BYTES {
            let _ = fs::remove_file(&canonical);
            return Err(ProviderError::TooLarge);
        }
        self.prune(Some(&directory));
        Ok(BufferedStream {
            file: canonical,
            duration: Duration::from_secs(seconds),
            cache_bytes: self.cache_bytes(),
        })
    }

    pub fn version(&self, cancel: &AtomicBool) -> Result<HelperVersion, ProviderError> {
        let value = self.call_helper(&json!({"op": "version"}), VERSION_TIMEOUT, cancel)?;
        Ok(HelperVersion {
            ytmusicapi: string_field(&value, "ytmusicapi"),
            yt_dlp: string_field(&value, "yt_dlp"),
            python: string_field(&value, "python"),
        })
    }

    pub fn cache_bytes(&self) -> u64 {
        directory_size(&self.cache_dir)
    }

    pub fn clear_cache(&self) -> std::io::Result<()> {
        match fs::read_dir(&self.cache_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let _ = fs::remove_dir_all(path);
                    } else {
                        let _ = fs::remove_file(path);
                    }
                }
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn call_helper(
        &self,
        request: &Value,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<Value, ProviderError> {
        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }
        if !self.helper.is_file() {
            return Err(ProviderError::HelperMissing);
        }
        let mut command = Command::new(&self.python);
        command
            .arg(&self.helper)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if std::env::var_os("MYUZIK_YT_DEBUG").is_some() {
                Stdio::inherit()
            } else {
                Stdio::null()
            });
        // The helper is a process tree: Python may spawn yt-dlp's own workers.
        // A new session lets us kill all of them at once, and PDEATHSIG makes
        // sure they do not outlive Myuzik.
        unsafe {
            use std::os::unix::process::CommandExt;
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let mut child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ProviderError::PythonMissing
            } else {
                ProviderError::Protocol(format!("could not start the YouTube helper: {error}"))
            }
        })?;
        let pid = child.id();
        let payload = serde_json::to_vec(request)
            .map_err(|error| ProviderError::Protocol(format!("request encoding: {error}")))?;
        if let Some(mut stdin) = child.stdin.take()
            && let Err(error) = stdin.write_all(&payload)
        {
            let _ = kill_group(pid);
            let _ = child.wait();
            return Err(ProviderError::Protocol(format!(
                "could not send request: {error}"
            )));
        }

        let started = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = kill_group(pid);
                let _ = child.wait();
                return Err(ProviderError::Cancelled);
            }
            if started.elapsed() > timeout {
                let _ = kill_group(pid);
                let _ = child.wait();
                return Err(ProviderError::Timeout);
            }
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => std::thread::sleep(POLL_INTERVAL),
                Err(error) => {
                    let _ = kill_group(pid);
                    let _ = child.wait();
                    return Err(ProviderError::Protocol(format!(
                        "could not wait for helper: {error}"
                    )));
                }
            }
        }

        let mut output = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            stdout
                .take((MAX_HELPER_OUTPUT + 1) as u64)
                .read_to_end(&mut output)
                .map_err(|error| {
                    ProviderError::Protocol(format!("could not read helper output: {error}"))
                })?;
        }
        if output.len() > MAX_HELPER_OUTPUT {
            return Err(ProviderError::Protocol(
                "helper response is too large".to_string(),
            ));
        }
        let value: Value = serde_json::from_slice(&output)
            .map_err(|error| ProviderError::Protocol(format!("helper JSON: {error}")))?;
        if value.get("ok").and_then(Value::as_bool) != Some(true) {
            let message = value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("YouTube helper failed");
            return Err(map_helper_error(message));
        }
        Ok(value)
    }

    fn prune(&self, keep: Option<&Path>) {
        let Ok(entries) = fs::read_dir(&self.cache_dir) else {
            return;
        };
        let mut dirs = Vec::new();
        let mut total = 0u64;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let size = directory_size(&path);
            total = total.saturating_add(size);
            let modified = fs::metadata(&path)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            dirs.push((path, size, modified));
        }
        if total <= self.max_cache_bytes {
            return;
        }
        dirs.sort_by_key(|(_, _, modified)| *modified);
        for (path, size, _) in dirs {
            if total <= self.max_cache_bytes {
                break;
            }
            if keep.is_some_and(|keep| keep == path) {
                continue;
            }
            if fs::remove_dir_all(&path).is_ok() {
                total = total.saturating_sub(size);
            }
        }
    }
}

fn helper_path() -> PathBuf {
    if let Some(path) = std::env::var_os("MYUZIK_YT_HELPER") {
        return PathBuf::from(path);
    }
    // An installed binary keeps the helper beside its own lib directory. Check
    // this before the checkout path so a prefix install never runs the source
    // tree's helper by accident.
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let beside = dir.join("../lib/myuzik/ytmusic.py");
        if beside.is_file() {
            return beside;
        }
    }
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("helper/ytmusic.py");
    if dev.is_file() {
        return dev;
    }
    if let Some(home) = std::env::var_os("HOME") {
        let installed = PathBuf::from(home).join(".local/lib/myuzik/ytmusic.py");
        if installed.is_file() {
            return installed;
        }
    }
    PathBuf::from("helper/ytmusic.py")
}

fn python_path() -> PathBuf {
    if let Some(path) = std::env::var_os("MYUZIK_YT_PYTHON") {
        return PathBuf::from(path);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let beside = dir.join("../lib/myuzik/runtime/bin/python");
        if beside.is_file() {
            return beside;
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let installed = PathBuf::from(home).join(".local/lib/myuzik/runtime/bin/python");
        if installed.is_file() {
            return installed;
        }
    }
    PathBuf::from("python3")
}

fn video_id(track: &Track) -> Option<&str> {
    let id = track.id.strip_prefix("yt:")?;
    if id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        Some(id)
    } else {
        None
    }
}

fn cache_key(video_id: &str, quality: YouTubeQuality) -> String {
    let mut hasher = Sha256::new();
    hasher.update(video_id.as_bytes());
    hasher.update(b"|");
    hasher.update(quality.as_str().as_bytes());
    hex::encode(hasher.finalize())
}

fn create_private_dir(path: &Path) -> Result<(), ProviderError> {
    fs::create_dir_all(path)
        .map_err(|error| ProviderError::Protocol(format!("could not create cache: {error}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn kill_group(pid: u32) -> std::io::Result<()> {
    if pid == 0 {
        return Ok(());
    }
    // Negative pid addresses the process group created by setsid().
    let result = unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn http_url(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
        .map(str::to_string)
}

fn youtube_browse_id<'a>(id: &'a str, prefix: &str) -> Option<&'a str> {
    let id = id.strip_prefix("yt:").unwrap_or(id);
    id.starts_with(prefix).then_some(id)
}

fn playlist_id(id: &str) -> Option<&str> {
    let id = id.strip_prefix("yt:").unwrap_or(id);
    let id = id.strip_prefix("VL").unwrap_or(id);
    (!id.is_empty()).then_some(id)
}

fn parse_count(value: Option<&Value>) -> u32 {
    match value {
        Some(Value::Number(number)) => number.as_u64().unwrap_or(0).min(u32::MAX as u64) as u32,
        Some(Value::String(text)) => text
            .split_whitespace()
            .find_map(|part| part.replace(',', "").parse::<u32>().ok())
            .unwrap_or(0),
        _ => 0,
    }
}

fn cards_from_value(value: &Value) -> Vec<BrowseCard> {
    value
        .get("items")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(card_from_json).collect())
        .unwrap_or_default()
}

fn sections_from_value(value: &Value) -> Vec<BrowseSection> {
    let Some(sections) = value.get("sections").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for section in sections {
        let title = section
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let items = cards_from_value(section);
        if !items.is_empty() {
            out.push(BrowseSection { title, items });
        }
    }
    out
}

fn card_from_json(item: &Value) -> Option<BrowseCard> {
    match item.get("kind").and_then(Value::as_str)? {
        "song" => track_from_json(item).map(BrowseCard::Song),
        "album" => album_from_json(item).map(BrowseCard::Album),
        "playlist" => playlist_from_json(item).map(BrowseCard::Playlist),
        "artist" => artist_from_json(item).map(BrowseCard::Artist),
        _ => None,
    }
}

fn album_from_json(item: &Value) -> Option<Album> {
    let id = youtube_browse_id(item.get("id")?.as_str()?, "MPRE")?;
    let name = item.get("title").and_then(Value::as_str)?.trim();
    if name.is_empty() {
        return None;
    }
    let artist_id = item
        .get("artistId")
        .and_then(Value::as_str)
        .filter(|id| id.starts_with("UC"))
        .map(|id| format!("yt:{id}"));
    Some(Album {
        id: format!("yt:{id}"),
        name: name.to_string(),
        artist: item
            .get("artist")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        artist_id,
        cover_art_id: None,
        cover_url: http_url(item.get("art")),
        song_count: None,
        duration: None,
        year: item
            .get("year")
            .and_then(Value::as_str)
            .and_then(|year| year.parse().ok()),
        starred: false,
        source: TrackSource::YouTube,
    })
}

fn playlist_from_json(item: &Value) -> Option<Playlist> {
    let id = playlist_id(item.get("id")?.as_str()?)?;
    let name = item.get("title").and_then(Value::as_str)?.trim();
    if name.is_empty() {
        return None;
    }
    let seconds = item.get("seconds").and_then(Value::as_u64).unwrap_or(0);
    Some(Playlist {
        id: format!("yt:{id}"),
        name: name.to_string(),
        song_count: parse_count(item.get("count")),
        duration: Duration::from_secs(seconds),
        owner: item
            .get("artist")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        source: TrackSource::YouTube,
        cover_url: http_url(item.get("art")),
    })
}

fn artist_from_json(item: &Value) -> Option<Artist> {
    let id = youtube_browse_id(item.get("id")?.as_str()?, "UC")?;
    let name = item.get("title").and_then(Value::as_str)?.trim();
    if name.is_empty() {
        return None;
    }
    Some(Artist {
        id: format!("yt:{id}"),
        name: name.to_string(),
        album_count: None,
        cover_art_id: None,
        source: TrackSource::YouTube,
        cover_url: http_url(item.get("art")),
    })
}

fn playlists_from_value(value: &Value) -> Vec<Playlist> {
    cards_from_value(value)
        .into_iter()
        .filter_map(|card| match card {
            BrowseCard::Playlist(playlist) => Some(playlist),
            _ => None,
        })
        .collect()
}

fn album_detail_from_value(value: &Value) -> Result<AlbumDetail, ProviderError> {
    let raw_id = value
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| youtube_browse_id(id, "MPRE"))
        .ok_or_else(|| ProviderError::Protocol("helper returned no album id".to_string()))?;
    let album_id = format!("yt:{raw_id}");
    let name = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let artist = value
        .get("artist")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let artist_id = value
        .get("artistId")
        .and_then(Value::as_str)
        .filter(|id| id.starts_with("UC"))
        .map(|id| format!("yt:{id}"));
    let cover_url = http_url(value.get("art"));
    let mut songs = tracks_from_value(value);
    for song in &mut songs {
        song.album = if song.album.is_empty() {
            name.clone()
        } else {
            song.album.clone()
        };
        song.album_id = Some(album_id.clone());
        if song.artist_id.is_none() {
            song.artist_id = artist_id.clone();
        }
        if song.cover_url.is_none() {
            song.cover_url = cover_url.clone();
        }
    }
    let duration: u64 = songs.iter().map(|track| track.duration.as_secs()).sum();
    let album = Album {
        id: album_id,
        name,
        artist,
        artist_id,
        cover_art_id: None,
        cover_url,
        song_count: Some(songs.len() as u32),
        duration: Some(Duration::from_secs(duration)),
        year: value
            .get("year")
            .and_then(Value::as_str)
            .and_then(|year| year.parse().ok()),
        starred: false,
        source: TrackSource::YouTube,
    };
    Ok(AlbumDetail { album, songs })
}

fn artist_detail_from_value(value: &Value) -> Result<ArtistDetail, ProviderError> {
    let raw_id = value
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| youtube_browse_id(id, "UC"))
        .ok_or_else(|| ProviderError::Protocol("helper returned no artist id".to_string()))?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let cover_url = http_url(value.get("art"));
    let mut albums = Vec::new();
    for key in ["albums", "singles"] {
        if let Some(items) = value.get(key).and_then(Value::as_array) {
            for item in items {
                if let Some(album) = album_from_json(item) {
                    albums.push(album);
                }
            }
        }
    }
    albums.sort_by_key(|album| std::cmp::Reverse(album.year));
    albums.dedup_by(|a, b| a.id == b.id);
    let artist = Artist {
        id: format!("yt:{raw_id}"),
        name,
        album_count: Some(albums.len() as u32),
        cover_art_id: None,
        source: TrackSource::YouTube,
        cover_url,
    };
    Ok(ArtistDetail { artist, albums })
}

fn playlist_detail_from_value(value: &Value) -> Result<PlaylistDetail, ProviderError> {
    let raw_id = value
        .get("id")
        .and_then(Value::as_str)
        .and_then(playlist_id)
        .ok_or_else(|| ProviderError::Protocol("helper returned no playlist id".to_string()))?;
    let playlist_id = format!("yt:{raw_id}");
    let name = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let cover_url = http_url(value.get("art"));
    let mut songs = tracks_from_value(value);
    for song in &mut songs {
        if song.cover_url.is_none() {
            song.cover_url = cover_url.clone();
        }
    }
    let seconds = value.get("seconds").and_then(Value::as_u64).unwrap_or(0);
    let playlist = Playlist {
        id: playlist_id,
        name,
        song_count: value
            .get("count")
            .and_then(Value::as_u64)
            .map(|count| count.min(u32::MAX as u64) as u32)
            .unwrap_or(songs.len() as u32),
        duration: Duration::from_secs(seconds),
        owner: value
            .get("owner")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        source: TrackSource::YouTube,
        cover_url,
    };
    Ok(PlaylistDetail { playlist, songs })
}

fn mood_categories_from_value(value: &Value) -> Vec<MoodCategory> {
    let Some(categories) = value.get("categories").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for category in categories {
        let title = category
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let moods: Vec<Mood> = category
            .get("items")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        Some(Mood {
                            title: item.get("title")?.as_str()?.to_string(),
                            params: item.get("params")?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !moods.is_empty() {
            out.push(MoodCategory { title, moods });
        }
    }
    out
}

fn tracks_from_value(value: &Value) -> Vec<Track> {
    let Some(items) = value.get("items").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut tracks = Vec::with_capacity(items.len());
    let mut seen = std::collections::HashSet::new();
    for item in items {
        if let Some(track) = track_from_json(item)
            && seen.insert(track.id.clone())
        {
            tracks.push(track);
        }
    }
    tracks
}

fn track_from_json(item: &Value) -> Option<Track> {
    let video_id = item.get("videoId").or_else(|| item.get("id"))?.as_str()?;
    if video_id.len() != 11
        || !video_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return None;
    }
    let title = item.get("title")?.as_str()?.trim();
    if title.is_empty() {
        return None;
    }
    let artist = item.get("artist").and_then(Value::as_str).unwrap_or("");
    let album = item.get("album").and_then(Value::as_str).unwrap_or("");
    let cover_url = item
        .get("art")
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
        .map(str::to_string);
    let seconds = item.get("seconds").and_then(Value::as_u64).unwrap_or(0);
    Some(Track {
        id: format!("yt:{video_id}"),
        title: title.to_string(),
        artist: artist.to_string(),
        artist_id: None,
        album: album.to_string(),
        album_id: None,
        cover_art_id: None,
        cover_url,
        duration: Duration::from_secs(seconds),
        track_number: None,
        disc_number: None,
        year: None,
        starred: false,
        genre: None,
        source: TrackSource::YouTube,
    })
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string()
}

fn map_helper_error(message: &str) -> ProviderError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("no module named") {
        return ProviderError::Protocol(message.to_string());
    }
    if lower.contains("cookie file") {
        return ProviderError::Protocol(message.to_string());
    }
    if lower.contains("sign in")
        || lower.contains("login")
        || lower.contains("cookies")
        || lower.contains("cookie")
    {
        return ProviderError::SignInRequired;
    }
    if lower.contains("javascript runtime")
        || lower.contains("node")
        || lower.contains("js runtime")
    {
        return ProviderError::NodeMissing;
    }
    if lower.contains("too large")
        || lower.contains("max-filesize")
        || lower.contains("larger than")
    {
        return ProviderError::TooLarge;
    }
    if lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("connection")
        || lower.contains("network")
        || lower.contains("unable to download")
    {
        return ProviderError::Network(message.to_string());
    }
    if lower.contains("unavailable") || lower.contains("not available") {
        return ProviderError::Unavailable(message.to_string());
    }
    ProviderError::Unavailable(message.to_string())
}

/// A completed audio file from an earlier buffer attempt, if one is usable.
fn cached_file(directory: &Path) -> Option<PathBuf> {
    const AUDIO_EXTENSIONS: &[&str] = &["webm", "m4a", "mp4", "opus", "mp3", "ogg", "wav"];
    let entries = fs::read_dir(directory).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
            continue;
        }
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.is_file() && metadata.len() > 0 && metadata.len() <= MAX_TRACK_BYTES {
            return fs::canonicalize(path).ok();
        }
    }
    None
}

fn directory_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                directory_size(&path)
            } else {
                entry.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}
