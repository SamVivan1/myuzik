# Single instance + config recovery — audit fixes (INT-06, INT-08, INT-08b)

Branch: `audit-fixes`. Files changed (only these, plus one new test file):

- `src/main.rs`
- `src/config.rs`
- `tests/config_resilience.rs` (new)

Method: strict TDD — the test was written and run first (red, with the failure output
quoted below), then the minimal fix, then green. No new dependency was added.
The build cache was shared with the other agents, so some runs waited on the cargo lock.

---

## INT-06 (P1) — no single-instance guard

**Status: Fixed.** A second process takes no app resources: it is refused the lock
*before* the config is read, the player is spawned, MPRIS is started or the tray is
registered. It logs the reason at `ERROR` (visible with the default filter), and it asks
the running instance to show its window through a request file beside the lock, then
exits cleanly (exit status 0).

**How the lock works.** `std::fs::File::try_lock` is `flock(2)` and is stable in this
toolchain (`rustc 1.99.1`), so the guard needs no `libc`/`nix`/`fs2` dependency. The lock
is advisory and tied to the open file description, so it is released when the process
exits or crashes: there is no stale-lock failure mode and nothing to clean up.

Path: `$XDG_RUNTIME_DIR/myuzik.lock`, falling back to `$XDG_CACHE_HOME/myuzik/myuzik.lock`
when `XDG_RUNTIME_DIR` is unset **or empty** (some minimal sessions set it empty).

Decision table (`instance_decision`, pure and unit-tested):

| `LockAttempt` | `InstanceDecision` |
|---|---|
| `Acquired(lock)` | `Run(Some(lock))` — lock held for the rest of `main` |
| `Held { pid }` | `Exit("Myuzik is already running (pid N); …")` |
| `Unavailable` (read-only dir, no permission) | `Run(None)` — fail open, logged |

Files and line ranges:

- `src/main.rs:400-577` — the single-instance module: `InstanceLock` (records its own pid),
  `LockAttempt`, `InstanceDecision`, `instance_lock_path`(`, `_for`), `show_request_path`,
  `request_show`, `take_show_request`, `read_lock_pid`, `acquire_instance_lock`,
  `instance_decision`.
- `src/main.rs:97-127` — taken in `main()` before `load_config_recovering`.
- `src/main.rs:55-95` — `show_main_window`, extracted from the tray's `Show` arm so both the
  tray (INT-02) and a second launch use one path (focus, or reopen a closed window).
- `src/main.rs:160-165`, `src/main.rs:204-228` — the show-request task (one `unlink` every
  400 ms; independent of the tray, so it works even when the tray cannot start).
- `src/main.rs:579-737` — unit tests.

Tests (all in `src/main.rs`, run with `cargo test --offline --bins`):

| Test | Locks |
|---|---|
| `the_lock_path_prefers_the_runtime_dir` | `$XDG_RUNTIME_DIR` wins; unset **and** empty fall back to the cache dir |
| `a_second_instance_is_told_to_exit_and_names_the_owner` | second acquire → `Exit`, reason names the pid; the lock file holds the owner's pid |
| `the_lock_is_released_when_its_owner_goes_away` | dropping the owner lets the next start run (no permanent lockout) |
| `a_lock_that_cannot_be_created_does_not_block_startup` | unusable lock location → `Run(None)` |
| `a_show_request_is_written_and_consumed_once` | request fires once, then is gone |
| `a_missing_show_request_is_not_an_error` | no request → no window raise |
| `the_show_request_lives_beside_the_lock` | request path derives from the lock path |

Red before the fix (compiled with stubs so the failures are behavioural, not compile errors):

```
test result: FAILED. 3 passed; 4 failed
  tests::a_second_instance_is_told_to_exit_and_names_the_owner
    assertion `left == right` failed: the lock file must name the owning process
      left: ""   right: "1236870"
  tests::the_lock_is_released_when_its_owner_goes_away
    assertion failed: matches!(instance_decision(acquire_instance_lock(&path)), InstanceDecision::Exit(_))
  tests::the_lock_path_prefers_the_runtime_dir
    left: "/home/samv/.cache/myuzik/myuzik.lock"  right: "/run/user/1000/myuzik.lock"
  tests::a_show_request_is_written_and_consumed_once
    the request must be on disk
```

Green after the fix:

```
running 7 tests
test result: ok. 7 passed; 0 failed; 0 ignored
```

**Not done (deliberately, reported instead):** the audit's first suggestion,
`request_name("io.github.samvivan1.Myuzik")`, needs a bus name that no process in my file
scope owns today (`src/platform/*` belongs to the Lead). The file-lock guard plus the show
request covers every effect the audit listed (two tray icons, double audio, double
scrobbling, two config writers, the doomed second MPRIS instance) without it.

---

## INT-08 (P1) — a corrupt config was reset silently and then overwritten

**Status: Fixed in the loader; the user-facing toast needs one `src/ui/shell.rs` method
(exact patch in the next section). Until that lands the notice is logged at `ERROR`, which
is visible with the default `EnvFilter` (the old `warn!` was not), and it is carried in
`LoadOutcome` for the shell.**

What the loader does now (`src/config.rs:519-560`):

1. File missing → defaults, no recovery (a fresh install is not a failure).
2. File unreadable (I/O, not UTF-8) or unparseable → the file is **moved, never deleted**,
   to `config.toml.corrupt-<unix seconds>` (a second broken file in the same second gets
   `-1`, `-2`, … so no backup is ever overwritten), defaults are returned, and the outcome
   says why and where the file went (`ConfigRecovery { backup, reason }`).
3. If even the move fails (read-only config dir), the app still starts and the outcome says
   the file could not be kept.
4. A readable file is migrated (`migrate_config`) and its unknown keys reported.

Schema version (`src/config.rs:18-40`):

- `pub const CONFIG_VERSION: u32 = 1` — the current value, documented.
- `AppConfig::version` serializes as `version = 1`. A file written before versioning has no
  key and deserializes as `0` (`#[serde(default = "unversioned")]` deliberately overrides
  the container-level `#[serde(default)]`, which would otherwise hide "legacy" behind
  `AppConfig::default().version`). `migrate_config` (called on every load,
  `src/config.rs:562-580`) sets 0 → `CONFIG_VERSION`, leaves an equal version alone, and
  **keeps** a version greater than `CONFIG_VERSION` (a downgrade must not rewrite a newer
  file as older) while warning.
- The migration body is a no-op today: it exists so the next renamed or removed key has one
  obvious place to live instead of disappearing.

`load_config` (`src/config.rs:648-659`) keeps its old `Result` contract — an unreadable file
is still `Err` — but the recovery happens inside it too, so `src/ui/shell.rs::reload_config`
cannot lose the user's bytes either. `main()` uses the richer
`load_config_recovering` (`src/config.rs:525-560`).

Tests (`tests/config_resilience.rs`, run with
`cargo test --offline --test config_resilience`):

| Test | Locks |
|---|---|
| `a_corrupt_config_is_kept_aside_with_its_bytes` | backup exists, bytes identical, original gone, notice names it, defaults returned |
| `a_config_that_is_not_utf8_is_kept_aside` | the read-error path recovers too (bytes kept, non-UTF-8) |
| `a_valid_config_is_read_and_migrated_without_a_backup` | values read, `version` migrated to `CONFIG_VERSION`, no backup |
| `a_config_from_a_newer_version_is_loaded_as_it_is` | version above current is preserved, not treated as corrupt |
| `a_missing_config_is_not_a_recovery` | fresh install: defaults, no recovery, no notice |
| `a_second_broken_config_never_overwrites_the_first_backup` | two corrupt files in the same second → two distinct backups with their own bytes |
| `load_config_reports_an_unreadable_file_and_still_keeps_it` | old entry point still `Err`, and the file was kept |
| `saving_keeps_the_keys_the_loader_did_not_understand` | a save writes `version` and does not destroy unknown keys |

Red before the fix (compiled with a skeleton loader so the failures are behavioural):

```
test result: FAILED. 2 passed; 7 failed
  a_corrupt_config_is_kept_aside_with_its_bytes
    a parse failure must be reported, not swallowed
  a_config_that_is_not_utf8_is_kept_aside
    an unreadable file must still be kept
  a_valid_config_is_read_and_migrated_without_a_backup
    left: 0  right: 1        (an unversioned file must be migrated)
  load_config_reports_an_unreadable_file_and_still_keeps_it
    an unreadable file must be an error: AppConfig { version: 1, ... }
  a_second_broken_config_never_overwrites_the_first_backup
    first backup
  an_unknown_key_is_reported_by_name
    left: []  right: ["prefer_navidrome_lirics", "volum"]
  saving_keeps_the_keys_the_loader_did_not_understand
    left: []  right: ["volum"]
```

Green after the fix:

```
running 9 tests
test result: ok. 9 passed; 0 failed; 0 ignored
```

---

## INT-08b (P2) — unknown keys were silently ignored

**Status: Fixed.**

`AppConfig` gained a `#[serde(flatten)] pub unknown: BTreeMap<String, toml::Value>`
catch-all (`src/config.rs:59-65`). Consequences:

- `load_config_recovering` returns the names in `LoadOutcome::unknown_keys`, sorted, and
  logs `WARN` naming them, the path, and that they have no effect
  (`src/config.rs:537-546`).
- A save writes them back (verified by `saving_keeps_the_keys_the_loader_did_not_understand`),
  so a typo is reported **and** not destroyed on the next settings toggle.
- Nested tables are caught too (one key per table), verified while prototyping the field.

Tests: `an_unknown_key_is_reported_by_name` (two typos, stable order, known keys keep their
values), `saving_keeps_the_keys_the_loader_did_not_understand`. Red/green above.

---

## The exact `src/ui/shell.rs` change the Lead must apply

`show_toast` is private and `Shell::new` has no notice parameter, so the recovered-config
sentence cannot reach the user from my files. `Shell::new`'s signature must **not** change
(existing tests in `tests/` call it with five arguments). Add one public method in the same
`impl Shell` block as `toast_text`/`show_toast` (they sit around `src/ui/shell.rs:1693`
today; the Lead's own edits move that number, so match on the names, not the lines):

```rust
    /// Tell the user something that happened before a window existed.
    ///
    /// INT-08: a corrupt `config.toml` is moved aside and the app starts on
    /// defaults; by the time this window exists the file is already gone, so a
    /// log line is the only other place that fact can live.
    pub fn notice(&mut self, message: impl Into<String>, cx: &mut gpui::Context<Self>) {
        self.show_toast(message, cx);
    }
```

and with it, one call in `src/main.rs` right after the shell is created
(`src/main.rs:189`, inside `app.run`'s closure) — I can add these two lines myself once the
method exists:

```rust
        // INT-08: a recovered config must reach the user, not only the log.
        if let Some(notice) = startup_notice.clone() {
            shell.update(cx, |shell, cx| shell.notice(notice, cx));
        }
```

`startup_notice` is already computed in `main()` (`src/main.rs:134-139`) from
`LoadOutcome::notice()`; today it is only logged at `ERROR`. If the Lead prefers a different
channel (a banner on the Setup route instead of a toast), `LoadOutcome` carries the structured
`recovery { backup, reason }` so the message can be built in the shell.

Nothing else in `src/ui/*` or `src/platform/*` needs to change for these findings.

---

## Verification gate (run, not claimed)

| Command | Result |
|---|---|
| `cargo test --offline --test config_resilience` | 9 passed, 0 failed |
| `cargo test --offline --bins` | 7 passed, 0 failed |
| `cargo test --offline --lib` | 107 passed, 0 failed, 1 ignored |
| `cargo test --offline` (all targets) | every target `ok`, 0 failed (run after the last code change) |
| `cargo check --offline --all-targets` | finished, 0 errors |
| `rustfmt --edition 2024 --check src/main.rs src/config.rs tests/config_resilience.rs` | clean |
| `cargo clippy --offline --all-targets -- -D warnings -A unused_mut -A clippy::collapsible_if -A refining_impl_trait_reachable` | clean |
| `cargo clippy --offline --all-targets -- -D warnings` | **fails on files outside my scope** — `src/ui/now_playing.rs:115` (`refining_impl_trait_reachable`), `src/ui/shell.rs:3166` (`unused_mut`), `src/ui/shell.rs:608` (`collapsible_if`). No warning comes from `src/main.rs`, `src/config.rs` or `tests/config_resilience.rs`; the three lints were excused only to prove that. |

Only my three files were formatted (`rustfmt` on them, not `cargo fmt`), because the other
agents' files are in flight in the same working tree.

---

## Not verified

- **A real second launch.** The conflict is exercised with two `try_lock` opens in one test
  process, which is the same `flock` conflict a second process hits, but launching the
  binary twice needs a display/session and was not run here.
- **The show-request pickup.** `take_show_request`/`request_show`/the paths are unit-tested;
  the 400 ms task that calls `show_main_window` is compile-checked only (gpui needs a window).
- **The toast.** Until the `Shell::notice` patch lands, the user only sees the `ERROR` log
  line in the journal/stderr; the sentence itself is tested via `LoadOutcome::notice()`.
- **Real `$XDG_RUNTIME_DIR` behaviour** under a desktop session (permissions, cleanup) —
  only the path selection and the lock semantics are tested.
- **A downgrade in practice**: "a newer version number is preserved" is tested for the
  loader, not for a real older binary running against a newer file.
