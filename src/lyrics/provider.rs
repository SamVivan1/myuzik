//! Lyrics resolution.
//!
//! Two sources, in this order of authority:
//!
//! 1. **Navidrome** (`getLyricsBySongId`, OpenSubsonic) — the server's own lyrics
//!    for the exact track id, so a synced result is used as-is.
//! 2. **LRCLIB** — the practical primary source: measured on the development
//!    library, Navidrome had lyrics for 0 of 6 sampled tracks.
//!
//! Getting LRCLIB *right* matters, because a wrong match is worse than no lyrics:
//! it puts another song's words in the overlay. Two things made matches wrong:
//!
//! * `GET /api/get` was called with `album_name`, which makes LRCLIB require an
//!   exact album match. Real tracks carry the album from Navidrome, which often
//!   differs from LRCLIB's record ("Music of the Spheres" vs "APT."), so the
//!   lookup 404'd even though the song was right there. The album is now left
//!   empty (verified: same request returns 200 with the synced lyrics).
//! * the search fallback took the **first** hit. For popular titles the hits
//!   span different edits — "APT." came back as 161 s, 170 s and 172 s against a
//!   170 s track — so the first hit could be a different recording and the
//!   timings were then out of step. Candidates are now ranked by how close their
//!   duration is to the track's, and only a close match is accepted as synced.
//!
//! Everything is validated before it is returned: the title and artist have to
//! agree after normalisation, a synced payload needs real timestamps, and if
//! nothing qualifies the answer is `NotFound` rather than someone else's lyrics.

use crate::library::{Track, TrackSource};
use crate::lyrics::cache::LyricsCache;
use crate::lyrics::lrc::parse_lrc;
use crate::lyrics::model::LyricsStatus;
use crate::navidrome::client::NavidromeClient;
use crate::navidrome::error::NavidromeError;
use std::time::Duration;

/// LRCLIB's origin. Kept apart from the request builders so a test can point the
/// lookups at a local stub instead of the real service.
const LRCLIB_BASE: &str = "https://lrclib.net";

/// A synced candidate is accepted only when its duration is within this many
/// seconds of the track's. Different edits of the same song differ by seconds.
const MAX_SYNCED_DELTA_SECS: f64 = 3.0;
/// Transcripts shorter than this are treated as junk rather than lyrics.
const MIN_SYNCED_LINES: usize = 3;

pub struct LyricsProvider {
    client: Option<NavidromeClient>,
    cache: LyricsCache,
    prefer_navidrome: bool,
    user_agent: String,
    /// One agent for every lookup.
    ///
    /// It used to be built inside `fetch_lrclib`, i.e. per track, which threw
    /// away the connection pool and the TLS session: each track then paid for a
    /// fresh TCP+TLS handshake on top of the ~5 sequential requests a lookup
    /// makes (audit PF-08).
    agent: ureq::Agent,
    /// Origin the LRCLIB lookups go to (`LRCLIB_BASE` unless a test overrides it).
    lrclib_base: String,
}

/// Number of HTTP agents built, so a test can prove one agent serves every
/// track instead of a fresh one per lookup (audit PF-08).
#[cfg(test)]
static AGENT_BUILDS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
fn agent_builds() -> usize {
    AGENT_BUILDS.load(std::sync::atomic::Ordering::Relaxed)
}

/// One LRCLIB record, normalised out of the JSON.
#[derive(Debug, Clone, Default)]
struct Candidate {
    artist: String,
    title: String,
    duration: Option<f64>,
    synced: Option<String>,
    plain: Option<String>,
    instrumental: bool,
}

impl Candidate {
    fn from_json(value: &serde_json::Value) -> Option<Self> {
        let text = |key: &str| {
            value
                .get(key)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .filter(|s| !s.trim().is_empty())
        };
        let candidate = Self {
            artist: text("artistName").unwrap_or_default(),
            title: text("trackName").unwrap_or_default(),
            duration: value.get("duration").and_then(|v| v.as_f64()),
            synced: text("syncedLyrics"),
            plain: text("plainLyrics"),
            instrumental: value
                .get("instrumental")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        };
        if candidate.artist.is_empty() && candidate.title.is_empty() && !candidate.instrumental {
            return None;
        }
        Some(candidate)
    }

    /// How far this candidate's duration is from the track's, in seconds.
    fn duration_delta(&self, track: &Track) -> Option<f64> {
        let theirs = self.duration?;
        let ours = track.duration.as_secs_f64();
        if ours <= 0.0 {
            return None;
        }
        Some((theirs - ours).abs())
    }

    /// Does this record describe the requested track?
    fn matches(&self, track: &Track) -> bool {
        let artist_ok = strings_agree(&self.artist, &track.artist);
        let title_ok = strings_agree(&self.title, &track.title);
        artist_ok && title_ok
    }
}

/// The one agent every LRCLIB request goes through.
///
/// LRCLIB can be slow or unreachable; without a bound the status stayed at
/// "Loading lyrics…" for the whole track. ureq has no default timeouts.
fn build_agent() -> ureq::Agent {
    #[cfg(test)]
    AGENT_BUILDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ureq::Agent::config_builder()
        .user_agent("myuzik/0.1 (https://github.com/SamVivan1/myuzik)")
        .timeout_connect(Some(Duration::from_secs(8)))
        .timeout_recv_response(Some(Duration::from_secs(12)))
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into()
}

/// Strip video-specific suffixes from a YouTube title before an LRCLIB lookup.
///
/// Display keeps the original title; only the lyrics query is cleaned. The
/// existing LRCLIB matcher still requires the artist to agree and validates the
/// duration, so this cannot turn one song's lyrics into another's by itself.
fn clean_youtube_title(title: &str) -> String {
    const SUFFIXES: &[&str] = &[
        "(Official Music Video)",
        "[Official Music Video]",
        "(Official Video)",
        "[Official Video]",
        "(Official Audio)",
        "[Official Audio]",
        "(Official Lyric Video)",
        "[Official Lyric Video]",
        "(Lyric Video)",
        "[Lyric Video]",
        "(Lyrics)",
        "[Lyrics]",
        "(Visualizer)",
        "[Visualizer]",
        "(Audio)",
        "[Audio]",
        "(HD)",
        "[HD]",
        "(4K)",
        "[4K]",
        "(Remastered)",
        "[Remastered]",
        "(Remastered 2011)",
        "[Remastered 2011]",
    ];
    let mut text = title.to_string();
    for suffix in SUFFIXES {
        text = text.replace(suffix, "");
    }
    let lower = text.to_lowercase();
    let mut cut = text.len();
    for marker in [" feat. ", " ft. ", " featuring "] {
        if let Some(position) = lower.find(marker) {
            cut = cut.min(position);
        }
    }
    text.truncate(cut);
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl LyricsProvider {
    pub fn new(client: NavidromeClient, cache: LyricsCache, prefer_navidrome: bool) -> Self {
        Self::with_optional_client(Some(client), cache, prefer_navidrome)
    }

    /// Build a provider that can run without a Navidrome server. YouTube tracks
    /// use LRCLIB directly; the Navidrome step is skipped when `client` is
    /// `None` or the track came from another provider.
    pub fn with_optional_client(
        client: Option<NavidromeClient>,
        cache: LyricsCache,
        prefer_navidrome: bool,
    ) -> Self {
        Self {
            client,
            cache,
            prefer_navidrome,
            user_agent: "Myuzik/0.1 (https://github.com/SamVivan1/myuzik)".to_string(),
            agent: build_agent(),
            lrclib_base: LRCLIB_BASE.to_string(),
        }
    }

    /// Point the LRCLIB lookups at a local stub. Tests only: the production
    /// origin is [`LRCLIB_BASE`].
    #[cfg(test)]
    fn with_lrclib_base(mut self, base: String) -> Self {
        self.lrclib_base = base;
        self
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

        // 1. Navidrome: authoritative, but only when it is actually synced. A
        //    plain transcript there must not shadow a synced version on LRCLIB.
        //    YouTube and other providers have no Navidrome id, so they skip
        //    this step entirely.
        let mut from_server: Option<LyricsStatus> = None;
        if self.prefer_navidrome
            && track.source == TrackSource::Navidrome
            && let Some(client) = &self.client
        {
            match client.lyrics(&track.id) {
                Ok(Some(status)) => {
                    if matches!(status, LyricsStatus::Synced(_)) {
                        Self::store(&self.cache, &key, &status);
                        return Ok(status);
                    }
                    from_server = Some(status);
                }
                Ok(None) => {}
                Err(NavidromeError::Unsupported) => {}
                Err(e) => return Err(e),
            }
        }

        // 2. LRCLIB. YouTube titles often carry "(Official Video)" and similar
        //    suffixes; strip them for the lookup only, never for display.
        let lookup_track;
        let lrclib_track = if track.is_youtube() {
            lookup_track = Track {
                title: clean_youtube_title(&track.title),
                ..track.clone()
            };
            &lookup_track
        } else {
            track
        };
        if let Some(status) = self.fetch_lrclib(lrclib_track) {
            match (&status, &from_server) {
                // A synced match beats an unsynced transcript from the server.
                (LyricsStatus::Synced(_), _) => {
                    Self::store(&self.cache, &key, &status);
                    return Ok(status);
                }
                _ if from_server.is_some() => {
                    let server = from_server.take().expect("checked");
                    Self::store(&self.cache, &key, &server);
                    return Ok(server);
                }
                _ => {
                    Self::store(&self.cache, &key, &status);
                    return Ok(status);
                }
            }
        }

        if let Some(server) = from_server {
            Self::store(&self.cache, &key, &server);
            return Ok(server);
        }

        Self::store(&self.cache, &key, &LyricsStatus::NotFound);
        Ok(LyricsStatus::NotFound)
    }

    /// Persist a resolved status so the next play is instant.
    fn store(cache: &LyricsCache, key: &str, status: &LyricsStatus) {
        match status {
            LyricsStatus::Synced(lines) => {
                let text = lines
                    .iter()
                    .map(|l| format!("[{}] {}", format_time(l.time), l.text))
                    .collect::<Vec<_>>()
                    .join("\n");
                let _ = cache.write_raw(key, false, &text, "");
            }
            LyricsStatus::Plain(t) => {
                let _ = cache.write_raw(key, false, "", t);
            }
            LyricsStatus::Instrumental => {
                let _ = cache.write_raw(key, true, "", "");
            }
            _ => {}
        }
    }

    fn fetch_lrclib(&self, track: &Track) -> Option<LyricsStatus> {
        let agent = &self.agent;
        let mut candidates: Vec<Candidate> = Vec::new();

        // Direct lookups first. The album name is deliberately empty: supplying
        // it makes LRCLIB demand an exact album match and 404 on correct songs.
        for with_duration in [true, false] {
            if let Some(found) = self.lrclib_get(agent, track, with_duration) {
                let direct = found;
                if let Some(status) = decide(&[direct], track) {
                    return Some(status);
                }
                if let Some(extra) = candidates.pop() {
                    candidates.push(extra);
                }
                candidates.extend(self.lrclib_get(agent, track, with_duration));
            }
        }

        // Search, ranked by duration proximity.
        candidates.extend(self.lrclib_search(agent, track));
        decide(&candidates, track)
    }

    /// `GET /api/get` — an exact-ish lookup.
    fn lrclib_get(
        &self,
        agent: &ureq::Agent,
        track: &Track,
        with_duration: bool,
    ) -> Option<Candidate> {
        let mut url = format!(
            "{}/api/get?artist_name={}&track_name={}&album_name=",
            self.lrclib_base,
            urlencoding::encode(&track.artist),
            urlencoding::encode(&track.title),
        );
        if with_duration {
            url.push_str(&format!("&duration={}", track.duration.as_secs()));
        }
        let resp = agent
            .get(&url)
            .header("User-Agent", &self.user_agent)
            .call()
            .ok()?;
        let json = resp.into_body().read_json::<serde_json::Value>().ok()?;
        Candidate::from_json(&json)
    }

    /// `GET /api/search` — several records, ranked rather than first-wins.
    fn lrclib_search(&self, agent: &ureq::Agent, track: &Track) -> Vec<Candidate> {
        let url = format!(
            "{}/api/search?artist_name={}&track_name={}",
            self.lrclib_base,
            urlencoding::encode(&track.artist),
            urlencoding::encode(&track.title),
        );
        let Ok(resp) = agent
            .get(&url)
            .header("User-Agent", &self.user_agent)
            .call()
        else {
            return Vec::new();
        };
        let Ok(json) = resp.into_body().read_json::<serde_json::Value>() else {
            return Vec::new();
        };
        json.as_array()
            .map(|items| items.iter().filter_map(Candidate::from_json).collect())
            .unwrap_or_default()
    }
}

/// Choose the best candidate for `track`, or nothing at all.
///
/// A synced record is only accepted when its duration is close: a mismatched
/// recording would scroll the right words at the wrong times, which looks worse
/// than an unsynced transcript. Plain transcripts carry no timing, so they are
/// safe to accept once artist and title agree.
fn decide(candidates: &[Candidate], track: &Track) -> Option<LyricsStatus> {
    let matching: Vec<&Candidate> = candidates.iter().filter(|c| c.matches(track)).collect();
    if matching.is_empty() {
        return None;
    }

    // Best synced candidate within the duration window.
    let mut best_synced: Option<(&Candidate, f64, Vec<crate::lyrics::model::LyricLine>)> = None;
    for candidate in &matching {
        let Some(text) = candidate.synced.as_deref() else {
            continue;
        };
        let lines = parse_lrc(text);
        if !looks_like_synced_lyrics(&lines) {
            continue;
        }
        // Unknown durations are acceptable; the artist/title match already holds.
        let delta = candidate.duration_delta(track).unwrap_or(0.0);
        if candidate.duration.is_some() && delta > MAX_SYNCED_DELTA_SECS {
            continue;
        }
        let better = best_synced
            .as_ref()
            .is_none_or(|(_, best_delta, _)| delta < *best_delta);
        if better {
            best_synced = Some((candidate, delta, lines));
        }
    }
    if let Some((_, _, lines)) = best_synced {
        return Some(LyricsStatus::Synced(lines));
    }

    // Instrumental markers are useful even without text.
    if matching.iter().any(|c| c.instrumental) {
        return Some(LyricsStatus::Instrumental);
    }

    // Otherwise the closest plain transcript, if any.
    let mut best_plain: Option<(&Candidate, f64)> = None;
    for candidate in &matching {
        let Some(text) = candidate.plain.as_deref() else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        let delta = candidate.duration_delta(track).unwrap_or(f64::MAX);
        let better = best_plain
            .as_ref()
            .is_none_or(|(_, best_delta)| delta < *best_delta);
        if better {
            best_plain = Some((candidate, delta));
        }
    }
    if let Some((candidate, _)) = best_plain {
        return Some(LyricsStatus::Plain(
            candidate.plain.clone().unwrap_or_default(),
        ));
    }

    None
}

/// Reject payloads that parse but are not usable lyrics: too short, or every
/// line stamped at zero (which would make the overlay highlight the last line).
fn looks_like_synced_lyrics(lines: &[crate::lyrics::model::LyricLine]) -> bool {
    if lines.len() < MIN_SYNCED_LINES {
        return false;
    }
    lines.iter().any(|l| l.time > Duration::ZERO)
}

/// Compare two music names leniently: case, punctuation and decorative Unicode
/// differ constantly between a library and LRCLIB ("Bling‐Bang‐Bang‐Born" with
/// U+2010 hyphens, "APT." with a full stop, "(feat. X)" suffixes).
fn strings_agree(a: &str, b: &str) -> bool {
    let a = normalize(a);
    let b = normalize(b);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.contains(&b) || b.contains(&a)
}

fn normalize(text: &str) -> String {
    // Drop bracketed suffixes such as "(feat. X)", "[Live]", "- Remastered".
    let mut cleaned = String::with_capacity(text.len());
    let mut depth = 0usize;
    for ch in text.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => cleaned.push(ch),
            _ => {}
        }
    }
    let lowered = cleaned.to_lowercase();
    let without_feat = lowered
        .split(" feat.")
        .next()
        .unwrap_or(&lowered)
        .split(" ft.")
        .next()
        .unwrap_or(&lowered)
        .to_string();

    // Keep letters and digits only: unify every dash/quote/punctuation mark.
    let mut out = String::with_capacity(without_feat.len());
    let mut last_space = true;
    for ch in without_feat.chars() {
        if ch.is_alphanumeric() {
            out.push(ch);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim().to_string()
}

fn format_time(d: Duration) -> String {
    let total_ms = d.as_millis() as u64;
    let mins = total_ms / 60000;
    let secs = (total_ms % 60000) / 1000;
    let millis = total_ms % 1000;
    format!("{:02}:{:02}.{:03}", mins, secs, millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(title: &str, artist: &str, duration_secs: u64) -> Track {
        Track {
            id: "id".to_string(),
            title: title.to_string(),
            artist: artist.to_string(),
            artist_id: None,
            album: "Music of the Spheres".to_string(),
            album_id: None,
            cover_art_id: None,
            duration: Duration::from_secs(duration_secs),
            track_number: Some(1),
            disc_number: Some(1),
            year: None,
            starred: false,
            genre: None,
            source: Default::default(),
            cover_url: None,
        }
    }

    fn synced(n: usize) -> String {
        (0..n)
            .map(|i| format!("[00:{:02}.00] line {}", i, i))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A local stand-in for LRCLIB that counts TCP connections and requests, so
    /// a test can tell a reused, pooled connection (one agent) from a new
    /// connection per track (a new agent per lookup).
    struct LrclibStub {
        base_url: String,
        connections: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        requests: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        accept: Option<std::thread::JoinHandle<()>>,
    }

    impl LrclibStub {
        fn start() -> std::io::Result<Self> {
            use std::sync::Arc;
            use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
            let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
            listener.set_nonblocking(true)?;
            let address = listener.local_addr()?;
            let connections = Arc::new(AtomicUsize::new(0));
            let requests = Arc::new(AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let accept = {
                let connections = connections.clone();
                let requests = requests.clone();
                let stop = stop.clone();
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                connections.fetch_add(1, Ordering::Relaxed);
                                let requests = requests.clone();
                                std::thread::spawn(move || serve(stream, requests));
                            }
                            Err(_) => std::thread::sleep(Duration::from_millis(2)),
                        }
                    }
                })
            };
            Ok(Self {
                base_url: format!("http://{address}"),
                connections,
                requests,
                stop,
                accept: Some(accept),
            })
        }
    }

    impl Drop for LrclibStub {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(accept) = self.accept.take() {
                let _ = accept.join();
            }
        }
    }

    /// Answer every request that arrives on this connection. A client that
    /// reuses its pooled connection keeps sending on it; a client with a fresh
    /// agent opens a second connection instead.
    fn serve(
        mut stream: std::net::TcpStream,
        requests: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        use std::io::{Read, Write};
        use std::sync::atomic::Ordering;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut pending: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 2048];
        loop {
            if let Some(end) = pending
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|position| position + 4)
            {
                let head = String::from_utf8_lossy(&pending[..end]).to_string();
                pending.drain(..end);
                requests.fetch_add(1, Ordering::Relaxed);
                let body = if head.contains("/api/search") {
                    format!("[{}]", candidate_json())
                } else {
                    candidate_json()
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{}",
                    body.len(),
                    body
                );
                if stream.write_all(response.as_bytes()).is_err() {
                    return;
                }
                continue;
            }
            match stream.read(&mut chunk) {
                Ok(0) => return,
                Ok(read) => pending.extend_from_slice(&chunk[..read]),
                Err(_) => return,
            }
        }
    }

    /// What the stub returns: a synced record matching `track(...)`.
    fn candidate_json() -> String {
        serde_json::json!({
            "artistName": "Test Artist",
            "trackName": "Test Song",
            "duration": 100.0,
            "instrumental": false,
            "syncedLyrics": "[00:00.00] one\n[00:05.00] two\n[00:10.00] three\n[00:15.00] four\n[00:20.00] five",
            "plainLyrics": "one two three",
        })
        .to_string()
    }

    fn lrclib_track(album: &str) -> Track {
        let mut track = track("Test Song", "Test Artist", 100);
        track.album = album.to_string();
        track
    }

    /// Audit PF-08: the provider built a new `ureq::Agent` inside every LRCLIB
    /// lookup, which threw away the connection pool and the TLS session, so each
    /// track paid for a fresh handshake (and the lookup makes up to five
    /// requests). One agent must serve every lookup.
    #[test]
    fn one_agent_serves_every_lrclib_lookup() {
        use std::sync::atomic::Ordering;
        let Ok(stub) = LrclibStub::start() else {
            eprintln!("skipped: cannot open a local stub server");
            return;
        };
        let cache_dir = std::env::temp_dir().join(format!(
            "myuzik-provider-one-agent-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let Ok(client) = NavidromeClient::new("http://127.0.0.1:1/", "user", "pass") else {
            eprintln!("skipped: cannot build a client");
            return;
        };
        let provider = LyricsProvider::new(client, LyricsCache::new(cache_dir.clone()), false)
            .with_lrclib_base(stub.base_url.clone());
        let builds_before = agent_builds();

        assert!(
            matches!(
                provider.fetch(&lrclib_track("Album A")),
                Ok(LyricsStatus::Synced(_))
            ),
            "the first lookup must resolve through the stub"
        );
        assert!(
            matches!(
                provider.fetch(&lrclib_track("Album B")),
                Ok(LyricsStatus::Synced(_))
            ),
            "the second lookup must resolve through the stub"
        );

        assert_eq!(
            stub.requests.load(Ordering::Relaxed),
            2,
            "each track needs its own lookup; the stub only proves reuse if it was asked twice"
        );
        assert_eq!(
            stub.connections.load(Ordering::Relaxed),
            1,
            "the second track opened a new TCP connection: the agent was rebuilt \
             for the lookup instead of being reused"
        );
        assert_eq!(
            agent_builds() - builds_before,
            0,
            "an agent was built again for the second lookup"
        );

        let _ = std::fs::remove_dir_all(&cache_dir);
    }

    #[test]
    fn normalization_ignores_punctuation_and_decorations() {
        assert_eq!(
            normalize("Bling‐Bang‐Bang‐Born"),
            normalize("Bling-Bang-Bang-Born")
        );
        assert_eq!(normalize("APT."), normalize("APT"));
        assert_eq!(
            normalize("Golden (from the Netflix film)"),
            normalize("Golden")
        );
        assert_eq!(normalize("Song feat. Someone"), normalize("Song"));
        assert_eq!(normalize("  Multiple   Spaces "), "multiple spaces");
    }

    #[test]
    fn names_must_agree_to_accept_a_candidate() {
        assert!(strings_agree("Bruno Mars", "bruno mars"));
        assert!(strings_agree(
            "Bling‐Bang‐Bang‐Born",
            "Bling-Bang-Bang-Born"
        ));
        assert!(!strings_agree("Bruno Mars", "Lady Gaga"));
        assert!(!strings_agree("APT.", "Die With A Smile"));
    }

    #[test]
    fn rejects_synced_payloads_without_real_timings() {
        let flat = "[00:00.00] a\n[00:00.00] b\n[00:00.00] c";
        assert!(!looks_like_synced_lyrics(&parse_lrc(flat)));
        let short = "[00:00.00] a\n[00:05.00] b";
        assert!(!looks_like_synced_lyrics(&parse_lrc(short)));
        assert!(looks_like_synced_lyrics(&parse_lrc(&synced(5))));
    }

    #[test]
    fn picks_the_recording_whose_duration_matches() {
        // Real spread seen for "APT." against a 170 s track.
        let candidates = vec![
            Candidate {
                artist: "Bruno Mars".into(),
                title: "APT.".into(),
                duration: Some(161.0),
                synced: Some(synced(10)),
                ..Default::default()
            },
            Candidate {
                artist: "Bruno Mars".into(),
                title: "APT.".into(),
                duration: Some(170.0),
                synced: Some(synced(20)),
                ..Default::default()
            },
        ];
        let chosen = decide(&candidates, &track("APT.", "Bruno Mars", 169));
        match chosen {
            Some(LyricsStatus::Synced(lines)) => {
                assert_eq!(
                    lines.len(),
                    20,
                    "picked the 161 s edit instead of the 170 s one"
                );
            }
            other => panic!("expected synced lyrics for the matching edit, got {other:?}"),
        }
    }

    #[test]
    fn refuses_a_synced_match_that_is_a_different_recording() {
        // Every candidate is far from the track's duration: better no lyrics
        // than words scrolling at the wrong times.
        let candidates = vec![Candidate {
            artist: "Another Artist".into(),
            title: "Different Song".into(),
            duration: Some(200.0),
            synced: Some(synced(12)),
            ..Default::default()
        }];
        assert!(decide(&candidates, &track("APT.", "Bruno Mars", 169)).is_none());
    }

    #[test]
    fn falls_back_to_plain_when_the_synced_match_is_too_far() {
        let candidates = vec![Candidate {
            artist: "Bruno Mars".into(),
            title: "APT.".into(),
            duration: Some(230.0),
            synced: Some(synced(30)),
            plain: Some("plain transcript".into()),
            ..Default::default()
        }];
        match decide(&candidates, &track("APT.", "Bruno Mars", 169)) {
            Some(LyricsStatus::Plain(text)) => assert_eq!(text, "plain transcript"),
            other => panic!("expected the plain transcript, got {other:?}"),
        }
    }

    #[test]
    fn unknown_duration_still_accepts_a_name_match() {
        let candidates = vec![Candidate {
            artist: "Bruno Mars".into(),
            title: "APT.".into(),
            duration: None,
            synced: Some(synced(8)),
            ..Default::default()
        }];
        assert!(matches!(
            decide(&candidates, &track("APT.", "Bruno Mars", 169)),
            Some(LyricsStatus::Synced(_))
        ));
    }

    #[test]
    fn instrumental_is_reported_when_flagged() {
        let candidates = vec![Candidate {
            artist: "Some Artist".into(),
            title: "Some Song".into(),
            instrumental: true,
            ..Default::default()
        }];
        assert_eq!(
            decide(&candidates, &track("Some Song", "Some Artist", 100)),
            Some(LyricsStatus::Instrumental)
        );
    }
}
