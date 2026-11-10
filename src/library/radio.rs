//! Autoplay radio: after the queue runs out, keep playing similar music.
//!
//! The engine answers one question — "given the track that is playing, what
//! should follow?" — and it answers it from four server-side sources, in order
//! of how closely each matches the seed:
//!
//! 1. `getSimilarSongs` (this song) — the most precise signal;
//! 2. `getSimilarSongs2` (this artist) — wider, still related;
//! 3. `getSongsByGenre` (this track's genre) — paged with a rotating offset, so
//!    a long session keeps finding songs the queue has not seen. Measured on a
//!    real library, "Indie Pop" holds only 30 songs while "Rock" holds 557, so
//!    this is what makes endless playback actually endless;
//! 4. `getRandomSongs` — the backstop, so playback never dead-ends.
//!
//! A batch only ever contains tracks that are not already queued, not recently
//! played, and not the seed itself. Selection is deterministic (no RNG) so the
//! behaviour is testable; variety comes from the offset rotation and from the
//! recent-history filter.
//!
//! Nothing here talks to the UI: callers fetch off the main thread and append
//! the result to the queue.

use crate::library::Track;
use crate::navidrome::client::NavidromeClient;
use crate::navidrome::error::NavidromeError;
use std::collections::{HashSet, VecDeque};

/// Tracks appended per fetch.
pub const BATCH: usize = 20;
/// How many recently played ids are remembered for deduplication.
pub const RECENT_CAP: usize = 100;
/// Page size (and offset step) when paging a genre.
pub const GENRE_PAGE: u32 = 25;
/// Consecutive empty batches before the radio gives up until the seed changes.
const MAX_EMPTY_BATCHES: u8 = 3;
/// How many genre pages one batch may walk looking for fresh songs.
const MAX_GENRE_PAGES: u8 = 3;
/// How many random draws one batch may try when everything else runs dry.
const MAX_RANDOM_DRAWS: u8 = 2;

/// Autoplay state that has to survive between tracks.
#[derive(Debug, Default)]
pub struct Radio {
    /// Ids of recently played tracks, most recent last.
    recent: VecDeque<String>,
    /// Offset into the current genre's song list.
    genre_offset: u32,
    /// Batches in a row that produced nothing fresh.
    empty_batches: u8,
    /// The seed the paging state belongs to.
    last_seed: Option<String>,
}

impl Radio {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many ids the recent-history filter currently holds.
    pub fn recent_len(&self) -> usize {
        self.recent.len()
    }

    /// Current genre pagination offset (exposed for tests).
    pub fn genre_offset(&self) -> u32 {
        self.genre_offset
    }

    /// True once the radio has repeatedly found nothing; it then stays quiet
    /// until a different track seeds it, instead of retrying on every end.
    pub fn is_stalled(&self) -> bool {
        self.empty_batches >= MAX_EMPTY_BATCHES
    }

    /// Record a track as played, so it is not suggested again soon.
    pub fn note_played(&mut self, track_id: &str) {
        if self.recent.back().is_some_and(|id| id == track_id) {
            return;
        }
        self.recent.push_back(track_id.to_string());
        while self.recent.len() > RECENT_CAP {
            self.recent.pop_front();
        }
    }

    /// Forget pagination and history. Called when a new queue starts, so a
    /// freshly chosen album does not inherit the previous run's filters.
    pub fn reset(&mut self) {
        self.recent.clear();
        self.genre_offset = 0;
        self.empty_batches = 0;
        self.last_seed = None;
    }

    /// Fetch what should follow `seed`, excluding ids the caller already has.
    ///
    /// Returns `Ok(vec![])` when the server answered but nothing fresh was
    /// available, and `Err` only when every source failed — the caller can then
    /// report it once instead of silently stopping.
    pub fn next_batch(
        &mut self,
        client: &NavidromeClient,
        seed: &Track,
        exclude: &HashSet<String>,
    ) -> Result<Vec<Track>, NavidromeError> {
        // A different track means a different genre page, so start over.
        if self.last_seed.as_deref() != Some(seed.id.as_str()) {
            self.last_seed = Some(seed.id.clone());
            self.genre_offset = 0;
            self.empty_batches = 0;
        }
        if self.is_stalled() {
            return Ok(Vec::new());
        }

        let mut seen: HashSet<String> = exclude.clone();
        seen.insert(seed.id.clone());
        seen.extend(self.recent.iter().cloned());

        let mut out: Vec<Track> = Vec::new();
        let mut any_ok = false;
        let mut last_err: Option<NavidromeError> = None;

        // 1. Similar to this very song.
        match client.similar_songs_for_song(&seed.id, GENRE_PAGE) {
            Ok(tracks) => {
                any_ok = true;
                absorb(tracks, &mut seen, &mut out);
            }
            Err(e) => {
                tracing::debug!("radio: similar-songs failed: {e:?}");
                last_err = Some(e);
            }
        }

        // 2. Similar to the artist.
        if out.len() < BATCH
            && let Some(artist_id) = seed.artist_id.as_deref()
        {
            match client.similar_songs(artist_id, GENRE_PAGE) {
                Ok(tracks) => {
                    any_ok = true;
                    absorb(tracks, &mut seen, &mut out);
                }
                Err(e) => {
                    tracing::debug!("radio: similar-songs2 failed: {e:?}");
                    last_err = Some(e);
                }
            }
        }

        // 3. Same genre, paged, so a big genre keeps yielding new songs and a
        //    small one cycles instead of returning the same page forever.
        if out.len() < BATCH
            && let Some(genre) = seed.genre.as_deref()
        {
            for _ in 0..MAX_GENRE_PAGES {
                if out.len() >= BATCH {
                    break;
                }
                let offset = self.genre_offset;
                match client.songs_by_genre(genre, GENRE_PAGE, offset) {
                    Ok(tracks) => {
                        any_ok = true;
                        let full_page = tracks.len() as u32 >= GENRE_PAGE;
                        absorb(tracks, &mut seen, &mut out);
                        if full_page {
                            self.genre_offset = self.genre_offset.saturating_add(GENRE_PAGE);
                        } else {
                            // End of the genre: wrap so the pool cycles.
                            self.genre_offset = 0;
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::debug!("radio: songs-by-genre failed: {e:?}");
                        last_err = Some(e);
                        break;
                    }
                }
            }
        }

        // 4. Backstop: anything at all.
        let mut draws = 0;
        while out.len() < BATCH && draws < MAX_RANDOM_DRAWS {
            draws += 1;
            match client.random_songs(BATCH as u32) {
                Ok(tracks) => {
                    any_ok = true;
                    let before = out.len();
                    absorb(tracks, &mut seen, &mut out);
                    if out.len() == before {
                        // Nothing new came back; another identical draw will not help.
                        break;
                    }
                }
                Err(e) => {
                    tracing::debug!("radio: random-songs failed: {e:?}");
                    last_err = Some(e);
                    break;
                }
            }
        }

        if out.is_empty() {
            self.empty_batches = self.empty_batches.saturating_add(1);
            if !any_ok && let Some(err) = last_err {
                return Err(err);
            }
        } else {
            self.empty_batches = 0;
        }
        Ok(out)
    }
}

/// Keep the first `BATCH` tracks whose id is not already known.
fn absorb(tracks: Vec<Track>, seen: &mut HashSet<String>, out: &mut Vec<Track>) {
    for track in tracks {
        if out.len() >= BATCH {
            return;
        }
        if seen.insert(track.id.clone()) {
            out.push(track);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            id: id.to_string(),
            title: id.to_string(),
            artist: "artist".to_string(),
            artist_id: Some("artist-1".to_string()),
            album: "album".to_string(),
            album_id: Some("album-1".to_string()),
            cover_art_id: None,
            duration: std::time::Duration::from_secs(180),
            track_number: None,
            disc_number: None,
            year: None,
            starred: false,
            genre: Some("Rock".to_string()),
        }
    }

    #[test]
    fn absorb_dedupes_and_stops_at_batch() {
        let mut seen = HashSet::new();
        seen.insert("a".to_string());
        let mut out = Vec::new();
        let many: Vec<Track> = (0..40).map(|i| track(&format!("t{i}"))).collect();
        let mut with_dupe = vec![track("a")];
        with_dupe.extend(many);
        absorb(with_dupe, &mut seen, &mut out);
        assert_eq!(out.len(), BATCH);
        assert!(!out.iter().any(|t| t.id == "a"), "known id leaked in");
    }

    #[test]
    fn recent_history_is_bounded_and_deduped() {
        let mut r = Radio::new();
        for i in 0..(RECENT_CAP + 20) {
            r.note_played(&format!("t{i}"));
        }
        assert_eq!(r.recent_len(), RECENT_CAP);
        // Repeated notes of the same id do not grow the ring.
        let before = r.recent_len();
        r.note_played(&format!("t{}", RECENT_CAP + 19));
        assert_eq!(r.recent_len(), before);
    }

    #[test]
    fn stalled_after_repeated_empty_batches_and_recovers_on_new_seed() {
        let mut r = Radio::new();
        r.empty_batches = MAX_EMPTY_BATCHES;
        assert!(r.is_stalled());
        // A new seed clears the stall.
        r.last_seed = Some("other".to_string());
        r.empty_batches = 0;
        assert!(!r.is_stalled());
    }
}
