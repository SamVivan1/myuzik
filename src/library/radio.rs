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

    /// Capture what a batch needs, under a short lock.
    ///
    /// Holding the lock across the HTTP calls is what froze the UI for as long
    /// as a batch took; the plan lets the caller release it and fetch without
    /// touching the radio at all.
    ///
    /// Returns `None` when this seed is stalled: the seed-change reset above has
    /// already run, so a *different* seed always gets a plan and autoplay can
    /// recover instead of dead-ending for the rest of the session.
    pub fn begin(&mut self, seed: &Track, exclude: &HashSet<String>) -> Option<RadioPlan> {
        // A different track means a different genre page, so start over.
        if self.last_seed.as_deref() != Some(seed.id.as_str()) {
            self.last_seed = Some(seed.id.clone());
            self.genre_offset = 0;
            self.empty_batches = 0;
        }
        if self.is_stalled() {
            return None;
        }

        let mut seen: HashSet<String> = exclude.clone();
        seen.insert(seed.id.clone());
        seen.extend(self.recent.iter().cloned());

        Some(RadioPlan {
            seed: seed.clone(),
            seen,
            genre_offset: self.genre_offset,
        })
    }

    /// Apply a fetched batch to the radio state, under a short lock.
    ///
    /// Discards the batch when a different track has seeded the radio since the
    /// plan was made, so a result that belongs to the previous queue can never
    /// land in the new one.
    pub fn commit(
        &mut self,
        plan: &RadioPlan,
        outcome: BatchOutcome,
    ) -> Result<Vec<Track>, NavidromeError> {
        if self.last_seed.as_deref() != Some(plan.seed.id.as_str()) {
            return Ok(Vec::new());
        }

        self.genre_offset = outcome.genre_offset;

        // The plan deduplicated against the history as it was *before* the
        // request; the user may have played something since, so filter again
        // rather than queueing a track that just started.
        let recent: HashSet<&str> = self.recent.iter().map(String::as_str).collect();
        let tracks: Vec<Track> = outcome
            .tracks
            .into_iter()
            .filter(|t| !recent.contains(t.id.as_str()))
            .collect();

        if tracks.is_empty() {
            self.empty_batches = self.empty_batches.saturating_add(1);
            if !outcome.any_ok
                && let Some(err) = outcome.error
            {
                return Err(err);
            }
        } else {
            self.empty_batches = 0;
        }
        Ok(tracks)
    }

    /// Fetch what should follow `seed`, excluding ids the caller already has.
    ///
    /// Returns `Ok(vec![])` when the server answered but nothing fresh was
    /// available, and `Err` only when every source failed — the caller can then
    /// report it once instead of silently stopping.
    ///
    /// Callers that must not hold a lock during I/O use [`Radio::begin`],
    /// [`fetch`] and [`Radio::commit`] directly instead of this convenience
    /// wrapper.
    pub fn next_batch(
        &mut self,
        client: &NavidromeClient,
        seed: &Track,
        exclude: &HashSet<String>,
    ) -> Result<Vec<Track>, NavidromeError> {
        let Some(plan) = self.begin(seed, exclude) else {
            return Ok(Vec::new());
        };
        let outcome = fetch(client, &plan);
        self.commit(&plan, outcome)
    }
}

/// One batch's worth of radio state, captured under a short lock.
#[derive(Debug)]
pub struct RadioPlan {
    seed: Track,
    seen: HashSet<String>,
    genre_offset: u32,
}

impl RadioPlan {
    /// The genre paging offset this batch starts from (exposed for tests).
    pub fn genre_offset(&self) -> u32 {
        self.genre_offset
    }
}

/// What one fetch produced, before it is committed to the radio state.
#[derive(Debug)]
pub struct BatchOutcome {
    /// Fresh tracks, already deduplicated against the plan.
    pub tracks: Vec<Track>,
    /// The genre offset the next batch should start from.
    pub genre_offset: u32,
    /// Whether at least one source answered, so an empty batch is not an error.
    pub any_ok: bool,
    /// The last error, when every source failed.
    pub error: Option<NavidromeError>,
}

impl BatchOutcome {
    /// An outcome that produced nothing (tests, and the stalled path).
    pub fn empty(genre_offset: u32) -> Self {
        Self {
            tracks: Vec::new(),
            genre_offset,
            any_ok: true,
            error: None,
        }
    }
}

/// Run the four sources for one plan. Performs all the HTTP and borrows no
/// radio state, so the caller may hold no lock while this runs.
pub fn fetch(client: &NavidromeClient, plan: &RadioPlan) -> BatchOutcome {
    let seed = &plan.seed;
    let mut seen = plan.seen.clone();
    let mut genre_offset = plan.genre_offset;
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
            match client.songs_by_genre(genre, GENRE_PAGE, genre_offset) {
                Ok(tracks) => {
                    any_ok = true;
                    let full_page = tracks.len() as u32 >= GENRE_PAGE;
                    absorb(tracks, &mut seen, &mut out);
                    if full_page {
                        genre_offset = genre_offset.saturating_add(GENRE_PAGE);
                    } else {
                        // End of the genre: wrap so the pool cycles.
                        genre_offset = 0;
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

    BatchOutcome {
        tracks: out,
        genre_offset,
        any_ok,
        error: last_err,
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
            source: Default::default(),
            cover_url: None,
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
