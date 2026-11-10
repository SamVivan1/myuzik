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

use crate::library::Track;
use crate::lyrics::cache::LyricsCache;
use crate::lyrics::lrc::parse_lrc;
use crate::lyrics::model::LyricsStatus;
use crate::navidrome::client::NavidromeClient;
use crate::navidrome::error::NavidromeError;
use std::time::Duration;

/// A synced candidate is accepted only when its duration is within this many
/// seconds of the track's. Different edits of the same song differ by seconds.
const MAX_SYNCED_DELTA_SECS: f64 = 3.0;
/// Transcripts shorter than this are treated as junk rather than lyrics.
const MIN_SYNCED_LINES: usize = 3;

pub struct LyricsProvider {
    client: NavidromeClient,
    cache: LyricsCache,
    prefer_navidrome: bool,
    user_agent: String,
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

impl LyricsProvider {
    pub fn new(client: NavidromeClient, cache: LyricsCache, prefer_navidrome: bool) -> Self {
        Self {
            client,
            cache,
            prefer_navidrome,
            user_agent: "Myuzik/0.1 (https://github.com/SamVivan1/myuzik)".to_string(),
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

        // 1. Navidrome: authoritative, but only when it is actually synced. A
        //    plain transcript there must not shadow a synced version on LRCLIB.
        let mut from_server: Option<LyricsStatus> = None;
        if self.prefer_navidrome {
            match self.client.lyrics(&track.id) {
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

        // 2. LRCLIB.
        if let Some(status) = self.fetch_lrclib(track) {
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
        // LRCLIB can be slow or unreachable; without a bound the status stayed at
        // "Loading lyrics…" for the whole track. ureq has no default timeouts.
        let agent = ureq::Agent::config_builder()
            .user_agent("myuzik/0.1 (https://github.com/SamVivan1/myuzik)")
            .timeout_connect(Some(std::time::Duration::from_secs(8)))
            .timeout_recv_response(Some(std::time::Duration::from_secs(12)))
            .timeout_global(Some(std::time::Duration::from_secs(20)))
            .build()
            .into();
        let mut candidates: Vec<Candidate> = Vec::new();

        // Direct lookups first. The album name is deliberately empty: supplying
        // it makes LRCLIB demand an exact album match and 404 on correct songs.
        for with_duration in [true, false] {
            if let Some(found) = self.lrclib_get(&agent, track, with_duration) {
                let direct = found;
                if let Some(status) = decide(&[direct], track) {
                    return Some(status);
                }
                if let Some(extra) = candidates.pop() {
                    candidates.push(extra);
                }
                candidates.extend(self.lrclib_get(&agent, track, with_duration));
            }
        }

        // Search, ranked by duration proximity.
        candidates.extend(self.lrclib_search(&agent, track));
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
            "https://lrclib.net/api/get?artist_name={}&track_name={}&album_name=",
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
            "https://lrclib.net/api/search?artist_name={}&track_name={}",
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
        }
    }

    fn synced(n: usize) -> String {
        (0..n)
            .map(|i| format!("[00:{:02}.00] line {}", i, i))
            .collect::<Vec<_>>()
            .join("\n")
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
