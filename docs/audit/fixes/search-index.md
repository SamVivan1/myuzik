# Local search index (`src/library/search_index.rs`)

Branch `audit-fixes`. Goal: replace per-keystroke Navidrome `search3` calls with an
in-memory index over the library snapshot — instant, works offline, pure (no IO, no threads,
no globals, no new dependencies).

**Status: complete and green.** 11 integration tests + 2 unit tests. Red before, green after,
plus a mutation check that proves the ranking test actually discriminates.

Files (the whole scope of this change):

| File | Lines | What |
|---|---|---|
| `src/library/search_index.rs` | 220 (new) | `SearchIndex`, `MAX_PER_GROUP`, fold/rank/match |
| `tests/search_index.rs` | 365 (new) | 11 behaviour tests |
| `docs/audit/fixes/search-index.md` | this file | report |

No other file was edited by this change. `src/library/mod.rs` is the Lead's file and already
carries the module declaration (see "Lead action" below).

---

## API

```rust
pub const MAX_PER_GROUP: usize = 20;

#[derive(Debug, Default)]
pub struct SearchIndex { /* private */ }

impl SearchIndex {
    pub fn build(albums: &[Album], artists: &[Artist], playlists: &[Playlist], songs: &[Track]) -> Self;
    pub fn search(&self, query: &str) -> SearchResults;
    pub fn is_empty(&self) -> bool;
    pub fn song_count(&self) -> usize;
}
```

`src/library/search_index.rs:21,46,55,89,103,110`.

This signature is exactly what `src/ui/shell.rs` already calls at `src/ui/shell.rs:117-122` and
`src/ui/shell.rs:1079-1084` (`build(&snapshot.albums, &snapshot.artists, &snapshot.playlists,
&snapshot.songs)`), so the shell wiring needs no change from me.

## How it works

- Each item is stored once with its fields folded at build time (`Entry`,
  `src/library/search_index.rs:28-41`): songs fold `title` as primary and
  `artist, album, genre` as secondary; albums fold `name` primary + `artist` secondary;
  artists and playlists fold `name` only. That is the "fields matched" requirement exactly.
- `fold` (`:168-196`) lowercases and maps Latin-1 marks: e/é/è/ê/ë…, a/á/à/â/ä/ã/å…,
  o/ö/ó/ò/ô/õ/ø…, u/ü/ú/ù/û…, i/í/ì/î/ï…, ç→c, ñ→n, ß→ss, æ→ae, œ→oe, and drops combining
  marks U+0300–U+036F so decomposed input folds too.
- `search` (`:89-101`) splits on whitespace and folds every token; an empty token list returns
  `SearchResults` with four empty vecs (there is no `Default` derive on `SearchResults`, so
  `empty_results()` builds it explicitly — `:115-122`).
- `rank` (`:137-150`): **every** token must match some field, otherwise the item is dropped
  (AND). The item's tier is the best tier over its tokens.
- `tier` (`:153-163`): exact = base, prefix = base+1, substring = base+2, no match = `u8::MAX`.
  Primary base 0, secondary base 3 (`SECONDARY_BASE`), so an exact title (0) beats an artist
  match (3), and a title substring (2) still beats a secondary-field match.
- `ranked` (`:125-134`) collects, then `sort_by_key` — Rust's sort is **stable**, so ties keep
  input order — then `truncate(MAX_PER_GROUP)`. Determinism therefore does not depend on hash
  order: everything is `Vec` built in input order.

## Tests

`tests/search_index.rs` (integration, public API):

| Test | Line | Requirement covered |
|---|---|---|
| `empty_and_whitespace_queries_return_nothing` | 75 | `""`, `"   \t\n "`, and an unmatched query all return four empty groups |
| `case_insensitive_match_finds_accented_artist` | 98 | `"bjork"` finds `"Björk"` (artist, and the song by that artist) |
| `diacritic_folding_works_in_both_directions` | 121 | `"Björk"` finds `"Bjork"`; `"sigur ros"` (unaccented, two tokens) finds `"Sigur Rós"` |
| `multi_word_query_requires_every_token` | 156 | `"miles kind"` → song + album; `"miles zzz"` → nothing |
| `songs_rank_exact_then_prefix_then_substring_then_artist` | 173 | id order `exact, prefix, substring, artist_exact` |
| `ties_keep_input_order` | 201 | equal tiers keep input order (`s2, s1, s3`) |
| `query_populates_all_four_groups` | 222 | one query fills artists + albums + songs + playlists |
| `per_group_cap_is_honoured` | 248 | 100 matching songs → exactly `MAX_PER_GROUP`, first is `s0` |
| `index_reports_its_size` | 272 | `is_empty` / `song_count` incl. an empty index |
| `repeated_builds_are_deterministic` | 292 | two builds of the same data give identical `Debug` output |
| `twenty_thousand_songs_build_and_search_quickly` | 340 | 20k songs → build+search under 2 s, first id `s0` |

Unit tests inside the module, `src/library/search_index.rs:202-219`:
`folding_lowercases_and_strips_accents`, `folding_handles_ligatures_and_decomposed_input`
(Ærø → aero, Straße → strasse, `Bjo\u{0308}rk` → bjork).

## Evidence

### RED (before implementation)

`src/library/search_index.rs` was a compile-only placeholder returning empty results, so the
tests ran and failed on assertions:

```
$ cargo test --offline --test search_index
test result: FAILED. 1 passed; 10 failed; 0 ignored; 0 measured; 0 filtered out

---- songs_rank_exact_then_prefix_then_substring_then_artist stdout ----
thread '...' panicked at tests/search_index.rs:177:5:
assertion `left == right` failed: exact title, then prefix, then substring, then a non-title field
  left: []
 right: ["exact", "prefix", "substring", "artist_exact"]

---- twenty_thousand_songs_build_and_search_quickly stdout ----
assertion `left == right` failed
  left: 0
 right: 20000
```

`empty_and_whitespace_queries_return_nothing` is the one that passed against the placeholder —
honest note: it is satisfied by "returns nothing", which a do-nothing stub also does. It stays in
the suite as the guard that whitespace never reaches the matching path.

One assertion of mine was wrong and was corrected **before** implementing (it contradicted the
two-way diacritic requirement): `"sigur ros"` must match `"Sigur Rós"`, so the test now asserts
that instead of asserting emptiness.

### GREEN (after implementation)

```
$ cargo test --offline --test search_index
running 11 tests
test case_insensitive_match_finds_accented_artist ... ok
test empty_and_whitespace_queries_return_nothing ... ok
test multi_word_query_requires_every_token ... ok
test diacritic_folding_works_in_both_directions ... ok
test index_reports_its_size ... ok
test query_populates_all_four_groups ... ok
test songs_rank_exact_then_prefix_then_substring_then_artist ... ok
test ties_keep_input_order ... ok
test per_group_cap_is_honoured ... ok
test repeated_builds_are_deterministic ... ok
test twenty_thousand_songs_build_and_search_quickly ... ok
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
```

The 20k build + search finishes inside the 0.10 s whole-suite run, well under the 2 s guard.

### Whole lib suite

```
$ cargo test --offline --lib
test result: ok. 111 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.04s
  (includes library::search_index::tests::folding_* — both ok)
```

### Lint

```
$ cargo clippy --offline --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.72s   (no warnings)

$ rustfmt --edition 2024 --check src/library/search_index.rs tests/search_index.rs
(clean)
```

Only my two files were formatted; `cargo fmt` was deliberately not run on the whole crate so no
other agent's file was touched.

### Mutation check (tests are not vacuous)

Temporarily changed `tier` so a prefix scored the same as an exact match
(`base + 1` → `base`), reran, then restored:

```
test result: FAILED. 10 passed; 1 failed
  test songs_rank_exact_then_prefix_then_substring_then_artist ... FAILED
```

Restored file, rerun: `11 passed; 0 failed`. So the ranking assertions really exercise the
tiering, not just "some result came back".

## Lead action

`src/library/mod.rs` **already contains** the line at line 85 — no edit needed:

```rust
pub mod search_index;
```

Exact line, if it ever has to be re-added after `pub mod cache;` (line 83) and
`pub mod radio;` (line 84): `pub mod search_index;`

While this work was in flight the Lead also added a compile-only placeholder
`src/library/search_index.rs` plus `pub mod cache;` and a placeholder `src/library/cache.rs`, so
the crate would build. That placeholder has been replaced by the real implementation; nothing of
the placeholder remains except the `MAX_PER_GROUP = 20` constant and the four method signatures,
both kept deliberately because `src/ui/shell.rs` and the task spec depend on them.

## Caveats / unverified

- **Not wired into the typing path by me.** `src/ui/shell.rs` is outside my scope; it already
  holds `library_index: Option<Arc<SearchIndex>>` and builds it after sync/cache load
  (`src/ui/shell.rs:51,117,1079`). Whether the search field actually queries it instead of
  `search3` must be verified by whoever owns the UI change; the index supports it but I did not
  trace or edit that path.
- **Concurrent-build interference.** Twice during this task the lib did not compile because of
  another agent's in-progress edits (`src/library/cache.rs` missing `clone_for_task`,
  `src/ui/shell.rs` missing `fetch_full_library` / a `String`/`&str` mismatch at
  `src/ui/shell.rs:1791`). Those were not mine; the recorded green runs above were taken when the
  tree compiled. If the tree is broken again, re-run `cargo test --offline --test search_index`
  before trusting the state.
- **Fold table is deliberately small**: the requested Latin-1 marks plus ligatures and combining
  marks. Non-Latin scripts are only lowercased (a Cyrillic/Greek query matches itself, not a
  transliteration).
- **Ranking is a design choice within the spec**: ties keep input order, and a multi-token item is
  ranked by its best token. The spec did not state the multi-token aggregation; `min` was chosen
  so `"miles kind"` ranks `"Kind of Blue"` by its title prefix, which the test locks in.
- **No `SearchResults::default()`**: the type does not derive `Default` in `src/library/mod.rs`
  and I did not add it (out of scope). `empty_results()` builds the empty value explicitly. If the
  Lead prefers `#[derive(Default)]` on `SearchResults`, that is a one-line mod.rs change and then
  `empty_results()` can be replaced by `SearchResults::default()`.
