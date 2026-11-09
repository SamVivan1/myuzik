use crate::library::Track;
use crate::lyrics::cache::LyricsCache;
use crate::lyrics::lrc::parse_lrc;
use crate::lyrics::model::LyricsStatus;
use crate::navidrome::client::NavidromeClient;
use crate::navidrome::error::NavidromeError;
use std::time::Duration;

pub struct LyricsProvider {
    client: NavidromeClient,
    cache: LyricsCache,
    prefer_navidrome: bool,
    user_agent: String,
}

impl LyricsProvider {
    pub fn new(client: NavidromeClient, cache: LyricsCache, prefer_navidrome: bool) -> Self {
        Self {
            client,
            cache,
            prefer_navidrome,
            user_agent: "Myuzik/0.1 (https://github.com/samvivan1/myuzik)".to_string(),
        }
    }

    pub fn fetch_cached(&self, track: &Track) -> Option<LyricsStatus> {
        let key = LyricsCache::key(track);
        self.cache.read(&key)
    }

    /// Blocking fetch. Call from a background task. Returns the resolved status.
    pub fn fetch(&self, track: &Track) -> Result<LyricsStatus, NavidromeError> {
        let key = LyricsCache::key(track);

        if let Some(cached) = self.cache.read(&key) {
            return Ok(cached);
        }

        // 1. Navidrome (OpenSubsonic)
        if self.prefer_navidrome {
            match self.client.lyrics(&track.id) {
                Ok(Some(status)) => {
                    // Cache raw text if possible.
                    match &status {
                        LyricsStatus::Synced(lines) => {
                            let text = lines.iter().map(|l| format!("[{}] {}", format_time(l.time), l.text)).collect::<Vec<_>>().join("\n");
                            let _ = self.cache.write_raw(&key, false, &text, "");
                        }
                        LyricsStatus::Plain(t) => {
                            let _ = self.cache.write_raw(&key, false, "", t);
                        }
                        LyricsStatus::Instrumental => {
                            let _ = self.cache.write_raw(&key, true, "", "");
                        }
                        _ => {}
                    }
                    return Ok(status);
                }
                Ok(None) => {}
                Err(NavidromeError::Unsupported) => {}
                Err(e) => return Err(e),
            }
        }

        // 2. LRCLIB
        if let Some(status) = self.fetch_lrclib(track) {
            // cache
            match &status {
                LyricsStatus::Synced(lines) => {
                    let text = lines.iter().map(|l| format!("[{}] {}", format_time(l.time), l.text)).collect::<Vec<_>>().join("\n");
                    let _ = self.cache.write_raw(&key, false, &text, "");
                }
                LyricsStatus::Plain(t) => {
                    let _ = self.cache.write_raw(&key, false, "", t);
                }
                LyricsStatus::Instrumental => {
                    let _ = self.cache.write_raw(&key, true, "", "");
                }
                _ => {}
            }
            return Ok(status);
        }

        let _ = self.cache.write_raw(&key, false, "", "");
        Ok(LyricsStatus::NotFound)
    }

    fn fetch_lrclib(&self, track: &Track) -> Option<LyricsStatus> {
        let agent = ureq::Agent::new_with_defaults();
        let duration = track.duration.as_secs();
        // /api/get
        let url = format!(
            "https://lrclib.net/api/get?artist_name={}&track_name={}&album_name={}&duration={}",
            urlencoding::encode(&track.artist),
            urlencoding::encode(&track.title),
            urlencoding::encode(&track.album),
            duration
        );
        if let Ok(resp) = agent.get(&url).header("User-Agent", &self.user_agent).call() {
            if let Ok(json) = resp.into_body().read_json::<serde_json::Value>() {
                if let Some(status) = build_result(&json) {
                    return Some(status);
                }
            }
        }
        // fallback search
        let search_url = format!(
            "https://lrclib.net/api/search?artist_name={}&track_name={}",
            urlencoding::encode(&track.artist),
            urlencoding::encode(&track.title)
        );
        if let Ok(resp) = agent.get(&search_url).header("User-Agent", &self.user_agent).call() {
            if let Ok(json) = resp.into_body().read_json::<serde_json::Value>() {
                if let Some(arr) = json.as_array() {
                    let mut plain = None;
                    for entry in arr {
                        if let Some(status) = build_result(entry) {
                            if matches!(status, LyricsStatus::Synced(_)) {
                                return Some(status);
                            }
                            if plain.is_none() {
                                plain = Some(status);
                            }
                        }
                    }
                    if let Some(p) = plain {
                        return Some(p);
                    }
                }
            }
        }
        None
    }
}

fn build_result(payload: &serde_json::Value) -> Option<LyricsStatus> {
    let instrumental = payload.get("instrumental").and_then(|v| v.as_bool()).unwrap_or(false);
    if instrumental {
        return Some(LyricsStatus::Instrumental);
    }
    let synced_text = payload.get("syncedLyrics").and_then(|v| v.as_str()).unwrap_or("");
    let plain_text = payload.get("plainLyrics").and_then(|v| v.as_str()).unwrap_or("");
    let synced = parse_lrc(synced_text);
    if !synced.is_empty() {
        return Some(LyricsStatus::Synced(synced));
    }
    if !plain_text.trim().is_empty() {
        return Some(LyricsStatus::Plain(plain_text.to_string()));
    }
    None
}

fn format_time(d: Duration) -> String {
    let total_ms = d.as_millis() as u64;
    let mins = total_ms / 60000;
    let secs = (total_ms % 60000) / 1000;
    let millis = total_ms % 1000;
    format!("{:02}:{:02}.{:03}", mins, secs, millis)
}
