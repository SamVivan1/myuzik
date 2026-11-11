# Cover cache (disk ownership, pruning, in-flight dedupe) — status

**Status: module and tests done and verified. Shell wiring is recommended below
and is not applied — `src/ui/**` belongs to the Lead.** Branch `audit-fixes`;
no branch, commit, reset, stash or checkout was run.

| File | Lines | What |
|---|---|---|
| `src/library/cover_cache.rs` | 263 | `CoverCache`, `CoverFetchGuard`, `PruneReport`, the two cache bounds |
| `tests/cover_cache.rs` | 386 | 13 integration tests covering every requested behaviour |
| `src/library/mod.rs` | +1 line (`pub mod cover_cache;`, line 84) | registration only |
| `docs/audit/fixes/cover-cache.md` | this file | status, evidence, wiring |

The Lead had placed a `PLACEHOLDER` module so the crate would compile. That
placeholder was replaced after the first red run; no placeholder code remains.

## API

| Item | Line in `src/library/cover_cache.rs` |
|---|---|
| `MAX_COVER_FILES` = 2 000 / `MAX_COVER_BYTES` = 200 MiB | 10–11 |
| `PruneReport` | 14 |
| `CoverCache` (`dir` + `Arc<Mutex<HashSet<String>>>` in-flight set) | 23 |
| `CoverCache::new` | 29 |
| `existing_path` (`.png` then `.jpg`, file must exist) | 38 |
| `read` (absent or empty ⇒ `None`; skips an empty `.png` for a valid `.jpg`) | 50 |
| `write` (validated id, magic-derived extension, unique `.tmp` + rename) | 65 |
| `prune` (mtime oldest first, `None` mtime oldest, PNG/JPG only, skips in-flight) | 100 |
| `begin_fetch` (atomic claim, `None` if already held) | 174 |
| `file_count` / `total_bytes` | 187 / 198 |
| `CoverFetchGuard` + `Drop` (releases on unwind; recovers a poisoned mutex) | 226 / 231 |
| `valid_cover_id` (rejects empty, `.`, `..`, `/`, `\`, NUL) | 243 |
| `is_cover_path` (prune candidates only) | 258 |

Behaviour notes:

- `write` writes `dir/.cover-<pid>-<seq>.tmp`, renames it over
  `<id>.<png|jpg>`, and removes the scratch file if either step fails. A
  successfully written PNG also removes a stale `.jpg` for the same id (and
  vice versa), because `existing_path`/`read` prefer `.png` and would otherwise
  return the old format.
- Cover ids are server-controlled. Every public id-taking method validates the
  id before joining it; `write` returns `InvalidInput` rather than escaping the
  directory. This is a rejection policy, not sanitisation.
- `prune` counts only `png`/`jpg` regular files. A file whose `modified()` is
  unreadable has `None`, which sorts before every `Some`, i.e. oldest. The
  in-flight mutex is held while candidates are removed, so a claim cannot be
  taken between the check and the delete. `kept_files`/`kept_bytes` include an
  in-flight file that the bounds would otherwise have evicted.
- `file_count`/`total_bytes` describe every regular file on disk (including a
  crash leftover), not only images; `prune` is the image-only operation.
- No dependencies were added. Production code has no `unsafe`, `panic!`,
  `unwrap`, or `expect`.

## Tests (`cargo test --offline --test cover_cache`)

13 tests, all green:

| Test (line) | Covers |
|---|---|
| `write_then_read_round_trips_and_extension_follows_bytes` (64) | PNG magic ⇒ `.png`, JPEG magic ⇒ `.jpg`, bytes round-trip |
| `existing_path_finds_either_extension_and_none_when_absent` (84) | lookup by both extensions, `None` otherwise |
| `writing_twice_replaces_the_file_and_leaves_no_tmp_file` (97) | second write wins, exactly one file, no `.tmp` |
| `file_count_and_total_bytes_reflect_the_directory` (116) | counts/bytes after writes and for a missing directory |
| `cover_ids_cannot_escape_the_cache_directory` (136) | `../`, nested slash, absolute, `..\`, `..`, `.`, empty all rejected; nothing written outside |
| `read_returns_none_for_an_empty_file` (196) | empty file is not a cover |
| `prune_keeps_the_newest_files_and_honours_max_files` (205) | 5 files, bound 2 ⇒ newest two; report values exact |
| `prune_honours_max_bytes_and_keeps_the_newest_until_the_budget_fits` (230) | 4×100 B, budget 250 B ⇒ newest two |
| `prune_ignores_non_images_and_a_missing_directory` (250) | `.txt`/`.tmp` untouched, empty report; missing dir harmless |
| `prune_never_deletes_a_file_that_is_in_flight` (268) | claimed oldest survives, unclaimed newer is pruned, claim drop releases |
| `begin_fetch_is_exclusive_and_releases_when_the_guard_drops` (304) | second claim `None`, unrelated id independent, `Some` after drop |
| `begin_fetch_allows_exactly_one_winner_across_threads` (327) | 8 threads, one `Some`, exactly one downloader |
| `the_guard_releases_the_claim_even_when_the_caller_panics` (371) | guard released while unwinding |

## Evidence (command + output)

### Red 1 — core API, before implementation

The placeholder provided basic reads/writes, so three core tests passed; the
missing counting and id validation failed for the right reasons:

```
$ cargo test --offline --test cover_cache
running 5 tests
test file_count_and_total_bytes_reflect_the_directory ... FAILED
thread 'file_count_and_total_bytes_reflect_the_directory' panicked at tests/cover_cache.rs:118:5:
assertion `left == right` failed
  left: 0
 right: 2
test cover_ids_cannot_escape_the_cache_directory ... FAILED
thread 'cover_ids_cannot_escape_the_cache_directory' panicked at tests/cover_cache.rs:142:9:
cover id "../escaped" must be rejected
test result: FAILED. 3 passed; 2 failed; 0 ignored
```

### Red 2 — pruning and in-flight, after adding those tests

`prune` was still a no-op and `begin_fetch` always returned `None`:

```
$ cargo test --offline --test cover_cache
running 13 tests
test prune_keeps_the_newest_files_and_honours_max_files ... FAILED
  left: ["cover-000.jpg", ..., "cover-004.jpg"]
 right: ["cover-003.jpg", "cover-004.jpg"]
test prune_honours_max_bytes_and_keeps_the_newest_until_the_budget_fits ... FAILED
test prune_never_deletes_a_file_that_is_in_flight ... FAILED
  claim the old cover
test begin_fetch_is_exclusive_and_releases_when_the_guard_drops ... FAILED
  first claim
test begin_fetch_allows_exactly_one_winner_across_threads ... FAILED
  left: 0, right: 1
test the_guard_releases_the_claim_even_when_the_caller_panics ... FAILED
  claim before panic
test result: FAILED. 7 passed; 6 failed; 0 ignored
```

### Green — implementation

```
$ cargo test --offline --test cover_cache
running 13 tests
test begin_fetch_allows_exactly_one_winner_across_threads ... ok
test begin_fetch_is_exclusive_and_releases_when_the_guard_drops ... ok
test cover_ids_cannot_escape_the_cache_directory ... ok
test existing_path_finds_either_extension_and_none_when_absent ... ok
test file_count_and_total_bytes_reflect_the_directory ... ok
test prune_honours_max_bytes_and_keeps_the_newest_until_the_budget_fits ... ok
test prune_ignores_non_images_and_a_missing_directory ... ok
test prune_keeps_the_newest_files_and_honours_max_files ... ok
test prune_never_deletes_a_file_that_is_in_flight ... ok
test read_returns_none_for_an_empty_file ... ok
test the_guard_releases_the_claim_even_when_the_caller_panics ... ok
test write_then_read_round_trips_and_extension_follows_bytes ... ok
test writing_twice_replaces_the_file_and_leaves_no_tmp_file ... ok

test result: ok. 13 passed; 0 failed; 0 ignored
```

### Full verification

```
$ cargo test --offline --lib
test result: ok. 114 passed; 0 failed; 1 ignored

$ cargo clippy --offline --all-targets -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.38s

$ cargo fmt -- src/library/cover_cache.rs tests/cover_cache.rs
(no output)
```

## Shell wiring for the Lead (`src/ui/shell.rs`)

The current working tree already has a startup prune and a cache-clear path
that construct `CoverCache` locally (lines 195 and 824). Keep those calls, but
store one shared cache on `Shell`; otherwise every track constructs a new
in-flight set and PF-10 is not fixed.

### 1. Field and one shared instance

Add to `struct Shell` (near line 49):

```rust
    /// Owns `<cache>/covers`: disk reads/writes, pruning, and the shared
    /// in-flight set that dedupes concurrent downloads of the same cover.
    cover_cache: Arc<crate::library::cover_cache::CoverCache>,
```

In `Shell::new`, before the existing prune block at line 195:

```rust
        let cover_cache = Arc::new(crate::library::cover_cache::CoverCache::new(
            paths.covers.clone(),
        ));
```

Replace line 195's local `let covers = ...` with `let covers = cover_cache.clone();`,
add `cover_cache: cover_cache.clone(),` to the struct literal near line 596, and
replace `let covers_for_loop = paths.covers.clone();` (line 289) with
`let covers_for_loop = cover_cache.clone();`.

### 2. `clear_library_cache` — lines 822–847

Replace the local construction at 824 and `covers.prune(0, 0)` at 840 with:

```rust
let report = self.cover_cache.prune(0, 0);
```

An in-flight cover is deliberately skipped; it will be pruned on the next pass.

### 3. `cache_track_cover` — lines 1420–1502 (PF-10 + atomic write)

Replace `read_cover_from_disk` at 1448 and the direct
`create_dir_all`/`std::fs::write` at 1462–1464:

```rust
let cover_cache = self.cover_cache.clone();
// ... existing in-memory check and client check ...
let Some(claim) = cover_cache.begin_fetch(&cid) else {
    // Another task is already fetching/decoding this exact cover. Its
    // completion inserts the image into HomeState and the overlay.
    return;
};
// inside the background closure:
let _claim = claim; // held until decode + write + UI update finish
let (bytes, path) = match cover_cache.read(&cid2) {
    Some(bytes) => {
        let path = cover_cache.existing_path(&cid2)?;
        (bytes, path)
    }
    None => {
        let bytes = c.cover_bytes(&cid2, 512).ok()?;
        let path = cover_cache.write(&cid2, &bytes).ok()?;
        (bytes, path)
    }
};
let format = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
    gpui::ImageFormat::Png
} else {
    gpui::ImageFormat::Jpeg
};
```

`begin_fetch` must be called before the `cx.spawn`, and the guard moved into the
spawned future (as `_claim`) so a fast second track sees `None` and skips the
download. The guard is released on return, `?`, cancellation, or panic.

### 4. `spawn_home_fetch` — signature 4853–4858 and loop 4923–4965

- Change `covers_dir: std::path::PathBuf` to
  `covers: Arc<crate::library::cover_cache::CoverCache>`.
- Change its three call sites (296, 910, 1794) from `paths.covers.clone()` /
  `shell.paths.covers.clone()` to `cover_cache.clone()` /
  `shell.cover_cache.clone()`.
- In the grid loop, replace `read_cover_from_disk` at 4932 and the UI-thread
  `create_dir_all` + `write` at 4954–4956 with this inside the existing
  background task, and return `Option<Vec<u8>>`:

```rust
if let Some(bytes) = covers.read(&cid_bg) {
    return Some(bytes);
}
let _claim = covers.begin_fetch(&cid_bg)?;
// Re-check after winning the claim: the previous holder may have just written.
if let Some(bytes) = covers.read(&cid_bg) {
    return Some(bytes);
}
let bytes = client_bg.cover_bytes(&cid_bg, 512).ok()?;
let _ = covers.write(&cid_bg, &bytes);
Some(bytes)
```

This also moves the write off the gpui main thread (the old write was inside
`cx.update`).

### 5. `open_album` — lines 1907, 1921–1929

Replace `let covers_for_album = self.paths.covers.clone();` with
`let covers_for_album = self.cover_cache.clone();`. In the background task:

```rust
if let Some(bytes) = covers_for_album.read(&cid_for_fetch) {
    return Some(bytes);
}
let _claim = covers_for_album.begin_fetch(&cid_for_fetch)?;
if let Some(bytes) = covers_for_album.read(&cid_for_fetch) {
    return Some(bytes);
}
let bytes = client.cover_bytes(&cid_for_fetch, 512).ok()?;
let _ = covers_for_album.write(&cid_for_fetch, &bytes);
Some(bytes)
```

The current path downloads album art but never persists it — this fixes that
and dedupes it against the track/grid fetches.

### 6. MPRIS art — lines 509–519

With `covers_for_loop` now an `Arc<CoverCache>`:

```rust
let art = track
    .cover_art_id
    .as_deref()
    .and_then(|cid| covers_for_loop.existing_path(cid))
    .map(|path| format!("file://{}", path.display()));
```

This removes the duplicated jpg/png existence checks.

### 7. Delete the old helper

After the three call sites are gone, delete `read_cover_from_disk` at
**lines 4754–4775**. There is no remaining caller.

### Does any call site call prune automatically?

No. `CoverCache::new` only stores the directory. The Lead-owned startup block
(line 195) and `clear_library_cache` (line 840) are the prune calls; keep the
startup one. The bounds are `MAX_COVER_FILES` and `MAX_COVER_BYTES`.
