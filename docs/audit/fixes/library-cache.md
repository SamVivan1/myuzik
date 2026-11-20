# Library cache (offline snapshot on disk) — status

**Status: done and verified.** Branch `audit-fixes`, no branch/commit/reset/checkout
was run. Only the two files in scope were created:

| File | Lines | What |
|---|---|---|
| `src/library/cache.rs` | 214 | `LibrarySnapshot`, `LibraryCache`: atomic save, guarded load, staleness helpers |
| `tests/library_cache.rs` | 537 | 18 integration tests (every behaviour below) |

No other file was edited. `src/library/mod.rs` already contains the module line
(see "Registration" at the bottom), and its DTOs already derive serde — both were
done by the Lead, not by this change.

## API (as specified)

| Item | Line in `src/library/cache.rs` |
|---|---|
| `pub const SNAPSHOT_VERSION: u32 = 1` | 28 |
| `LibrarySnapshot { version, fetched_at, albums, artists, playlists, songs }` | 41 |
| `LibrarySnapshot::empty` | 53 |
| `LibrarySnapshot::is_empty` | 65 |
| `LibrarySnapshot::song_count` | 72 |
| `LibrarySnapshot::age_seconds` (saturating) | 80 |
| `LibrarySnapshot::is_stale` | 88 |
| `LibraryCache` (holds the directory) | 95 |
| `LibraryCache::new` | 102 |
| `LibraryCache::path` → `<dir>/library.json` | 107 |
| `LibraryCache::load` → `Result<Option<LibrarySnapshot>>` | 121 |
| `LibraryCache::save` (tmp + fsync + rename) | 156 |
| `LibraryCache::write_tmp_and_replace` (private) | 173 |
| `LibraryCache::clear` | 199 |

Behaviour notes:

- `load` returns `Ok(None)` only for `NotFound`. Anything else — not UTF-8, not
  JSON, a directory at the path, a different `version` — is `Err` with a reason
  that names the file; the file is **not** deleted.
- `save` serialises to `library.json.tmp` in the same directory, `sync_all`s it,
  renames it over `library.json`, then best-effort fsyncs the directory. On a
  failed write the scratch file is removed, so a failed save leaves the previous
  snapshot and no junk.
- `is_stale` is true when `age_seconds >= max_age`, compared as exact
  `Duration`s (a sub-second `max_age` is not truncated). Boundary semantics are
  pinned by the test below; callers that expect "strictly older than" should
  pass `max_age + 1s`.
- `clear` treats a missing file as success and also removes a leftover `.tmp`.

## Tests (`cargo test --offline --test library_cache`)

18 tests, all green:

| Test | Covers |
|---|---|
| `a_snapshot_round_trips_through_save_and_load_with_every_field_intact` | save/load keeps every field, incl. `Some`/`None` and sub-second `Duration` |
| `loading_a_missing_file_is_ok_none` | missing file ⇒ `Ok(None)` |
| `a_corrupt_json_file_returns_err_and_is_left_in_place` | truncation ⇒ `Err` naming the file, file kept |
| `a_file_that_is_not_utf8_returns_err_and_is_left_in_place` | invalid UTF-8 ⇒ `Err`, file kept |
| `a_directory_where_the_snapshot_should_be_is_an_error_not_a_panic` | no panic on an odd filesystem shape |
| `a_snapshot_from_a_different_version_returns_err_and_is_not_deleted` | version mismatch and a differently shaped file ⇒ `Err`, kept |
| `saving_is_atomic_and_leaves_no_tmp_file_behind` | after save the directory holds exactly `library.json` |
| `a_reader_never_sees_a_partial_snapshot_while_saves_are_running` | concurrent reader parses a complete snapshot on every read during repeated saves |
| `a_stale_tmp_file_from_a_crashed_save_is_replaced` | crash leftover `.tmp` is overwritten and gone |
| `a_failed_save_leaves_the_previous_snapshot_intact` | blocked temp path ⇒ `Err`, old snapshot still loads, no junk |
| `saving_creates_the_directory_when_it_is_missing` | `create_dir_all` on first save |
| `saving_over_an_existing_snapshot_replaces_it_completely` | second save replaces, never merges |
| `empty_and_song_count_describe_the_snapshot_contents` | `is_empty`/`song_count` across empty/albums-only/songs-only |
| `age_seconds_saturates_at_zero_for_a_snapshot_from_the_future` | saturation, incl. `now == 0` |
| `is_stale_follows_max_age_exactly_at_the_boundary` | fresh before, stale exactly at `max_age`, sub-second `max_age`, `max_age == 0` |
| `clear_removes_the_snapshot_and_is_idempotent` | clear existing + clear missing |
| `the_snapshot_lives_at_library_json_in_the_given_directory` | file name/location |
| `a_snapshot_with_twenty_thousand_songs_round_trips` | 20 000 songs + 500 albums + 100 artists + 20 playlists; count and edge ids |

## Evidence (command + output)

### Red — before the implementation

The Lead had already placed a minimal scaffold at `src/library/cache.rs` (marked
`PLACEHOLDER ... will replace this file`) so the crate would compile. The suite
was run against that scaffold first, and it failed for the right reason:

```
$ cargo test --offline --test library_cache
running 13 tests
...
test a_corrupt_json_file_returns_err_and_is_left_in_place ... FAILED
...
thread 'a_corrupt_json_file_returns_err_and_is_left_in_place' panicked at tests/library_cache.rs:197:5:
error should name the file, got: expected value at line 1 column 28
test result: FAILED. 12 passed; 1 failed; 0 ignored
```

The scaffold returned the bare `serde_json` message without the file path and
without the atomic-write/fsync/cleanup guarantees; the rest of the suite pinned
behaviour it happened to already satisfy, which is why only one test went red.
The full implementation was then written to replace the scaffold.

### Green — real implementation

```
$ cargo test --offline --test library_cache
running 18 tests
test a_directory_where_the_snapshot_should_be_is_an_error_not_a_panic ... ok
test a_corrupt_json_file_returns_err_and_is_left_in_place ... ok
test a_file_that_is_not_utf8_returns_err_and_is_left_in_place ... ok
test a_reader_never_sees_a_partial_snapshot_while_saves_are_running ... ok
test a_snapshot_round_trips_through_save_and_load_with_every_field_intact ... ok
test a_snapshot_from_a_different_version_returns_err_and_is_not_deleted ... ok
test a_snapshot_with_twenty_thousand_songs_round_trips ... ok
test age_seconds_saturates_at_zero_for_a_snapshot_from_the_future ... ok
test clear_removes_the_snapshot_and_is_idempotent ... ok
test empty_and_song_count_describe_the_snapshot_contents ... ok
test is_stale_follows_max_age_exactly_at_the_boundary ... ok
test loading_a_missing_file_is_ok_none ... ok
test saving_creates_the_directory_when_it_is_missing ... ok
test saving_is_atomic_and_leaves_no_tmp_file_behind ... ok
test saving_over_an_existing_snapshot_replaces_it_completely ... ok
test a_failed_save_leaves_the_previous_snapshot_intact ... ok
test a_stale_tmp_file_from_a_crashed_save_is_replaced ... ok
test the_snapshot_lives_at_library_json_in_the_given_directory ... ok

test result: ok. 18 passed; 0 failed; 0 ignored
```

### The tests bite (mutation checks, file restored after each)

| Mutation | Caught by | Result |
|---|---|---|
| `save` writes `library.json` directly (no tmp+rename) | `a_reader_never_sees_a_partial_snapshot_while_saves_are_running` (reader got `... is not a valid snapshot`), `a_failed_save_leaves_the_previous_snapshot_intact`, `a_stale_tmp_file_from_a_crashed_save_is_replaced` | 15 passed, **3 failed** |
| version check removed | `a_snapshot_from_a_different_version_returns_err_and_is_not_deleted` | 16 passed, **1 failed** |
| `is_stale` uses `>` instead of `>=` | `is_stale_follows_max_age_exactly_at_the_boundary` | 16 passed, **1 failed** |

### Gates

```
$ rustfmt --check --edition 2024 src/library/cache.rs tests/library_cache.rs
(no output)                                                  → clean
$ cargo clippy --offline --all-targets -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s)     → clean
$ cargo test --offline --lib
test result: ok. 111 passed; 0 failed; 1 ignored
```

## Registration

`src/library/mod.rs` already has the line, at **line 83**, so nothing is
missing; this is the exact line the cache needs:

```rust
pub mod cache;
```

The snapshot's `Serialize`/`Deserialize` also need the DTOs to derive serde.
The Lead already added that to `Track` (line 1), `Album` (line 19),
`Artist` (line 38) and `Playlist` (line 52):

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
```

`std::time::Duration` is serialised by serde as `{"secs":…, "nanos":…}`, so
track/album/playlist durations survive the round trip exactly — no field was
flattened to whole seconds.

## Not verified / caveats

- **Durability after a real power loss is not tested.** `sync_all` on the file
  and (best effort) on the directory is in the code, but no unit test can prove
  a crash. What *is* tested is that no reader, and no failed save, can see or
  leave a partial file.
- Not exercised on network filesystems (NFS/SMB), where `rename` and `fsync`
  guarantees vary.
- The `is_stale` boundary (`>=`) is a product decision; if the Lead wants
  "stale only after `max_age` has fully elapsed", change line 88–90 to `>` and
  update the one test.
- `load` does not attempt to repair or migrate older versions — by design it
  reports the mismatch and the caller refetches.
