//! Local, in-memory search over a library snapshot.
//!
//! This module is a pure index: no IO, no threads, no globals. Every searchable
//! field is folded once at build time, so a query is a linear scan over owned
//! strings and matching never allocates per field.
//!
//! Matching rules:
//! - case- and diacritic-insensitive (see [`fold`]);
//! - every whitespace-separated token must match somewhere in the item (AND);
//! - all results are ranked by tier, lower is better:
//!   0 exact primary field, 1 prefix of it, 2 substring of it,
//!   3 exact secondary field, 4 prefix of it, 5 substring of it.
//!   For songs the primary field is the title, so an exact title always beats
//!   an artist match; an artist/album/genre match ranks after a title substring.
//! - the sort is stable, so equal tiers keep the input order, and results are
//!   truncated to [`MAX_PER_GROUP`] per group.

use crate::library::{Album, Artist, Playlist, SearchResults, Track};

/// Per-result-group cap, so a two-letter query cannot return the whole library.
pub const MAX_PER_GROUP: usize = 20;

/// Tier of a match against a secondary field (artist, album, genre).
const SECONDARY_BASE: u8 = 3;

/// One indexed item with its fields already folded.
#[derive(Debug)]
struct Entry<T> {
    item: T,
    primary: String,
    secondary: Vec<String>,
}

impl<T> Entry<T> {
    fn new(item: T, primary: &str, secondary: &[&str]) -> Self {
        Self {
            item,
            primary: fold(primary),
            secondary: secondary.iter().map(|field| fold(field)).collect(),
        }
    }
}

/// A searchable snapshot of the library.
#[derive(Debug, Default)]
pub struct SearchIndex {
    songs: Vec<Entry<Track>>,
    albums: Vec<Entry<Album>>,
    artists: Vec<Entry<Artist>>,
    playlists: Vec<Entry<Playlist>>,
}

impl SearchIndex {
    /// Build from the current library contents. Cheap enough to rebuild on sync.
    pub fn build(
        albums: &[Album],
        artists: &[Artist],
        playlists: &[Playlist],
        songs: &[Track],
    ) -> Self {
        Self {
            songs: songs
                .iter()
                .map(|song| {
                    let secondary = [
                        song.artist.as_str(),
                        song.album.as_str(),
                        song.genre.as_deref().unwrap_or_default(),
                    ];
                    Entry::new(song.clone(), &song.title, &secondary)
                })
                .collect(),
            albums: albums
                .iter()
                .map(|album| Entry::new(album.clone(), &album.name, &[album.artist.as_str()]))
                .collect(),
            artists: artists
                .iter()
                .map(|artist| Entry::new(artist.clone(), &artist.name, &[]))
                .collect(),
            playlists: playlists
                .iter()
                .map(|playlist| Entry::new(playlist.clone(), &playlist.name, &[]))
                .collect(),
        }
    }

    /// Local search. An empty/whitespace query returns an empty result set.
    pub fn search(&self, query: &str) -> SearchResults {
        let tokens: Vec<String> = query.split_whitespace().map(fold).collect();
        if tokens.is_empty() {
            return empty_results();
        }

        SearchResults {
            artists: ranked(&self.artists, &tokens),
            albums: ranked(&self.albums, &tokens),
            songs: ranked(&self.songs, &tokens),
            playlists: ranked(&self.playlists, &tokens),
            youtube: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.songs.is_empty()
            && self.albums.is_empty()
            && self.artists.is_empty()
            && self.playlists.is_empty()
    }

    pub fn song_count(&self) -> usize {
        self.songs.len()
    }
}

fn empty_results() -> SearchResults {
    SearchResults {
        artists: Vec::new(),
        albums: Vec::new(),
        songs: Vec::new(),
        playlists: Vec::new(),
        youtube: Vec::new(),
    }
}

/// Best-ranked matching items, truncated, in input order for equal tiers.
fn ranked<T: Clone>(entries: &[Entry<T>], tokens: &[String]) -> Vec<T> {
    let mut matched: Vec<(u8, &T)> = entries
        .iter()
        .filter_map(|entry| rank(entry, tokens).map(|tier| (tier, &entry.item)))
        .collect();
    // Stable: equal tiers stay in input order.
    matched.sort_by_key(|(tier, _)| *tier);
    matched.truncate(MAX_PER_GROUP);
    matched.into_iter().map(|(_, item)| item.clone()).collect()
}

/// The best tier over all tokens, or `None` when any token matches nothing.
fn rank<T>(entry: &Entry<T>, tokens: &[String]) -> Option<u8> {
    let mut best = u8::MAX;
    for token in tokens {
        let mut token_best = tier(&entry.primary, token, 0);
        for field in &entry.secondary {
            token_best = token_best.min(tier(field, token, SECONDARY_BASE));
        }
        if token_best == u8::MAX {
            return None;
        }
        best = best.min(token_best);
    }
    Some(best)
}

/// Exact, then prefix, then substring; `u8::MAX` when the field does not match.
fn tier(field: &str, token: &str, base: u8) -> u8 {
    if field == token {
        base
    } else if field.starts_with(token) {
        base + 1
    } else if field.contains(token) {
        base + 2
    } else {
        u8::MAX
    }
}

/// Lowercase and strip the common Latin diacritics, so "bjork" finds "Björk"
/// and a "Björk" query finds a "Bjork" entry. Decomposed accents lose their
/// combining mark. No dependencies: the table only covers the requested marks.
fn fold(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        for lower in ch.to_lowercase() {
            let replacement = match lower {
                'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
                'ç' | 'ć' | 'č' => "c",
                'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
                'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' => "i",
                'ñ' | 'ń' | 'ň' => "n",
                'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
                'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => "u",
                'ý' | 'ÿ' => "y",
                'š' | 'ş' => "s",
                'ž' | 'ź' | 'ż' => "z",
                'æ' => "ae",
                'œ' => "oe",
                'ß' => "ss",
                'đ' => "d",
                'ł' => "l",
                // Combining diacritical marks (decomposed input): drop them.
                '\u{0300}'..='\u{036F}' => "",
                other => {
                    out.push(other);
                    continue;
                }
            };
            out.push_str(replacement);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::fold;

    #[test]
    fn folding_lowercases_and_strips_accents() {
        assert_eq!(fold("Björk"), "bjork");
        assert_eq!(fold("BJÖRK"), "bjork");
        assert_eq!(fold("Sigur Rós"), "sigur ros");
        assert_eq!(fold("Motörhead"), "motorhead");
        assert_eq!(fold("AC/DC"), "ac/dc");
    }

    #[test]
    fn folding_handles_ligatures_and_decomposed_input() {
        assert_eq!(fold("Ærø"), "aero");
        assert_eq!(fold("Straße"), "strasse");
        assert_eq!(fold("Bjo\u{0308}rk"), "bjork");
    }
}
