# Audit fixes — tray, credentials, errors

Branch `audit-fixes`. Findings: INT-01, INT-02, INT-05, INT-09, INT-10 (docs/audit/integration.md)
and PF-09 (docs/audit/performance.md).

Files touched (all inside this agent's scope): `src/main.rs`, `src/platform/tray.rs`,
`src/config.rs`, `src/navidrome/error.rs`, plus two new test files
`tests/config_credentials.rs` and `tests/tray_reregistration.rs`.

Cross-agent note: while this work was in progress another agent changed
`start_mpris(player_tx)` into `start_mpris(player_tx, initial_volume: f32)` for INT-03.
`src/main.rs:78` (this agent's file) was adapted to pass `cfg.volume`, so the MPRIS `Volume`
starts at the configured value instead of the old hardcoded `0.8`.

Method: test first, watch it fail, then the minimal fix. Every test below was red before the
change (compile error naming the missing API, or the assertion) and green after.

| ID | Status | One line |
|---|---|---|
| INT-02 | **fixed** (decision logic unit-tested; window lifecycle not testable headless) | The tray follows the main window's handle, not the window count, and `Show` reopens a destroyed main window |
| INT-05 | **fixed**, verified against a real bus | `NameOwnerChanged` subscription + bounded re-registration (`100·2^n` ms, 5 attempts) on a worker thread |
| INT-09 | **fixed** | Credential file created with `open(2)` mode `0600` and the error is returned to the caller |
| INT-01 | **partially fixed** | Real `keyring` Secret Service path implemented and verified end to end, but **opt-in** (`MYUZIK_KEYRING=1`); docs still describe it as always on |
| INT-10 | **fixed** (library side) | `NavidromeError::user_message()` + `hint()`; `src/ui/shell.rs` still prints `Debug` — exact changes below |
| PF-09 | **fixed** | The registration wait no longer runs on the UI thread; `START_TIMEOUT` 10 s → 2 s |

---

## INT-02 (P0) — tray/overlay dead-end

**Status: fixed.** The window lifecycle itself cannot be driven headless, so the decision is a
pair of pure functions with unit tests, and `src/main.rs` is the thin adapter around them.

### Changes

`src/platform/tray.rs:84-112`

```rust
pub enum ShowAction { Focus, Reopen }
pub fn tray_visibility(main_window_alive: bool) -> bool   // !main_window_alive
pub fn show_action(main_window_alive: bool) -> ShowAction  // Focus / Reopen
```

`src/main.rs:31-46` — `main_window_alive(app, &slot)`: the main window's `AnyWindowHandle` is
alive iff it is present in `app.windows()`.

`src/main.rs:113-120` — the handle is kept in
`Arc<Mutex<Option<AnyWindowHandle>>>` instead of being dropped with the window.

`src/main.rs:145-172` — supervisor (400 ms) now:

```rust
let alive = main_window_alive(app, &main_window);
tray.set_visible(tray_visibility(alive));
if !alive && !close_to_tray { app.quit(); }   // was: windows().is_empty() && !close_to_tray
```

`src/main.rs:176-224` — `TrayCommand::Show` matches on `show_action(...)`:
`ShowAction::Focus` → `activate_window()` and **logs** a failure (`let _ =` removed);
`ShowAction::Reopen` → `app.open_window(main_window_options(bounds_for_tray), …)` and stores the
new handle. `Hide` likewise logs instead of discarding.

Note the deliberate semantic change in the supervisor: with `close_to_tray = false` the app now
quits when the *main window* is gone even if the lyrics overlay is still open. That is what the
setting promises and it removes the audit's "process alive with no window and no icon" case.

### Tests

`src/platform/tray.rs` (`cargo test --lib platform::tray`):

- `tray_visibility_follows_the_main_window_not_the_window_count`
- `show_focuses_an_open_window_and_reopens_a_closed_one`

Mapping to the old bug (for the reviewer): the old predicate was
`app.windows().is_empty()`; in the audit's scenario the main window is closed and the overlay is
open, so the window count is 1 → `is_empty() == false` → `set_visible(false)` → `Passive`. The
test asserts the opposite for exactly that state (`main_window_alive == false` ⇒ visible), so it
fails under the old predicate and passes under the new one. Red evidence is the pre-fix run:

```
error[E0425]: cannot find function `tray_visibility` in this scope
error[E0433]: cannot find type `ShowAction` in this scope
error[E0425]: cannot find function `show_action` in this scope
```

### Unverified

- The `open_window`/`activate_window` branch has no automated test (needs a compositor). Covered
  only by `cargo check` and by reading.
- `Show` on a main window that is *minimized* still only calls `activate_window()`; gpui has no
  `unminimize` (`crates/gpui/src/window.rs:6723-6735`), so Hide→Show may need a second click from
  the shell. Pre-existing, not introduced here.

---

## INT-05 (P1) — no re-registration with `StatusNotifierWatcher`

**Status: fixed, and verified against a real bus.**

### Changes — `src/platform/tray.rs`

| Lines | What |
|---|---|
| `48-52` | `WATCHER_MAX_ATTEMPTS = 5`, `WATCHER_FIRST_RETRY = 100 ms` |
| `860` | `spawn_watcher_watch(&connection, &own_name)` after the first `RegisterStatusNotifierItem` |
| `1009-1019` | `watcher_retry_delay(attempt)` — `100 · 2^attempt` ms, `None` after 5 attempts |
| `1021-1027` | `watcher_appeared(name, new_owner_present)` — only the watcher's own name gaining an owner counts |
| `1029-1038` | `spawn_watcher_watch` — dedicated `myuzik-tray-watcher` thread, off the UI thread and off the worker's `serve` loop |
| `1040-1082` | `watch_watcher` — one `NameHasOwner` check (covers "watcher already there but refused the first try"), then a `MessageIterator` on `MatchRule{type=signal, sender/interface=org.freedesktop.DBus, member=NameOwnerChanged, arg0=org.kde.StatusNotifierWatcher}` |
| `1084-1104` | `watcher_is_present` — `NameHasOwner` on `/org/freedesktop/DBus` |
| `1106-1130` | `register_when_the_watcher_returns` — bounded retries with backoff, `warn!` when it gives up |

The iteration ends when the connection closes in `Service::stop`, which is why the worker's join
still returns (see the smoke test below).

### Tests

`src/platform/tray.rs` (`cargo test --lib platform::tray`):

- `watcher_retries_are_bounded_and_back_off` — 5 delays, strictly growing, total ≤ 5 s, `None` at 5
- `only_the_watcher_appearing_is_a_reason_to_register` — watcher appearing yes, watcher leaving no, host names no

`tests/tray_reregistration.rs` (**ignored**, needs its own bus):

- `a_watcher_that_appears_late_is_registered_with` — no watcher at start, tray first, then a fake
  watcher claims `org.kde.StatusNotifierWatcher`; asserts `RegisterStatusNotifierItem` arrives.

### Evidence

```
$ dbus-run-session -- cargo test --offline --test tray_reregistration -- --ignored --nocapture
running 1 test
the late watcher was told about: ["org.kde.StatusNotifierItem-1141322-1", "org.kde.StatusNotifierItem-1141322-1"]
test a_watcher_that_appears_late_is_registered_with ... ok

$ cargo test --offline --lib platform::tray -- --ignored --test-threads=1
test platform::tray::tests::smoke_registers_answers_and_unregisters ... ok   (0.89 s, live session bus)
```

The second run is the pre-existing live smoke test (registration, menu, signals, `Activate` →
`Show`, shutdown releases the bus name) and it still passes with the watcher thread running, i.e.
the new thread does not hang shutdown.

### Unverified / noted

- The name is registered twice in the late-watcher test: once from the `NameHasOwner` retry and
  once from the `NameOwnerChanged` signal (race when the watcher appears during the initial
  check). `RegisterStatusNotifierItem` is idempotent, so this is harmless; it is left visible
  rather than hidden behind a "registered once" flag.
- A watcher that appears while the app is running is covered; a *host* registration
  (`StatusNotifierHostRegistered`) is not subscribed to — the audit asked for the watcher only.

---

## INT-09 (P1) — credential file created world-readable, then `chmod`ed

**Status: fixed.**

### Changes — `src/config.rs`

| Lines | What |
|---|---|
| `208-222` | `create_private_file(path)` — `OpenOptions::new().write(true).create(true).truncate(true).mode(0o600)`, then `set_permissions(0o600)` *before* any byte is written (also tightens a file left by an older version) |
| `225-233` | `save_password_to_file` — create parent, private file, `write_all`, `flush`; every step propagates |
| `250-282` | `save_password_with(...) -> anyhow::Result<CredentialStore>` — the error now reaches the caller instead of being dropped inside |
| `116-129` | `CredentialStore::describe()` — the sentence the setup form should show |

`std::fs::write` + post-hoc `chmod`, and the `let _ =` swallowing in the model of the old code, are
gone. The caller (`src/ui/shell.rs`) still throws the result away — that is on the Lead (below).

### Tests — `tests/config_credentials.rs`

- `the_fallback_file_is_only_readable_by_its_owner` (unix) — mode is `0600` after a save
- `private_file_is_created_with_mode_0600` (unix) — the creation primitive itself is `0600`
- `save_falls_back_to_the_file_when_the_secret_service_is_missing` — the write error path is real
- `save_without_a_secret_service_reports_the_file_store_but_succeeds`

Red evidence (before `config.rs` changed):

```
error[E0432]: unresolved import `myuzik::config::create_private_file`
  no `CredentialStore` in `config`, no `SecretServiceBackend` in `config`,
  no `save_password_with` in `config`, no `load_password_with` in `config`
```

### Unverified

The "no world-readable moment" property is guaranteed by construction (`open(2)` receives
`0600`), not observed: the intermediate mode of a file cannot be sampled without a filesystem
watcher, and `umask` cannot be read without `libc` (not a dependency). `private_file_is_created_with_mode_0600`
locks the primitive that provides the guarantee.

---

## INT-01 (P0) — docs promise Secret Service, code stores plaintext

**Status: partially fixed.** The Secret Service path is real, works, and is verified on this
machine; it is **opt-in**, so out of the box the password still goes to the `0600` file.

### What is implemented — `src/config.rs`

| Lines | What |
|---|---|
| `115-138` | `CredentialStore { SecretService, File }` + `describe()` |
| `140-183` | `trait SecretServiceBackend { set, get, delete }` and `KeyringBackend` using `keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)` (`keyring` 4.2 `v1` API, service `io.github.samvivan1.Myuzik`, account `navidrome`) |
| `187-194` | `secret_service_enabled(Option<&str>)` — `1/true/yes/on`, case-insensitive |
| `196-202` | `default_backend()` — from `MYUZIK_KEYRING` |
| `250-282` | `save_password_with` — keyring first; on failure `warn!` and fall back to the private file; on success **delete a stale plaintext file** |
| `284-299` | `load_password_with` — keyring first, then file (symmetric) |
| `301-317` | `delete_password_with` — both stores; a missing entry is success, a real failure is returned |
| `318-336` | `load_password` / `save_password` / `delete_password` wrappers keep the old shapes (plus the `CredentialStore` return) so `src/ui/shell.rs` still compiles |

### Verified here (better than expected)

This machine **does** have a session bus and a Secret Service (`org.freedesktop.secrets`, gnome-keyring),
so the keyring path was verified end to end, not just stubbed:

```
$ MYUZIK_KEYRING=1 cargo test --offline --test config_credentials -- --ignored
test real_secret_service_round_trip_leaves_no_plaintext_file ... ok
```

That test saves through the real `keyring` backend, asserts the store is `SecretService`, asserts
**no plaintext file exists**, reads it back, deletes it and asserts it is gone. A throwaway probe
(removed) also read `STORE=SecretService  FILE_EXISTS=false  LOAD=Some("probe-secret")`.

The `MYUZIK_KEYRING=1` run of `save_password_reports_the_store_it_used` therefore exercises the
real path too; the entry it wrote was deleted afterwards (`delete_password`).

### Tests — `tests/config_credentials.rs`

`save_uses_the_secret_service_when_it_works`,
`save_falls_back_to_the_file_when_the_secret_service_is_missing`,
`saving_to_the_secret_service_removes_a_stale_plaintext_file`,
`load_prefers_the_secret_service_over_the_file`,
`load_uses_the_file_when_the_service_has_no_entry`,
`load_returns_none_when_there_is_no_password_anywhere`,
`delete_removes_the_password_from_both_stores`,
`save_without_a_secret_service_reports_the_file_store_but_succeeds`,
`save_password_reports_the_store_it_used`,
`secret_service_is_opted_into_by_environment`,
`real_secret_service_round_trip_leaves_no_plaintext_file` (`#[ignore]`, writes to the user's keyring).

The stubs make the fallback deterministic and keep `cargo test` off the user's keyring: the
default `cargo test` run never touches a real Secret Service (mutation of the user's keyring only
happens in the `#[ignore]`d test).

### Why opt-in, and what remains

`MYUZIK_KEYRING=1` is required because:

1. `load_password` is called **twice on the main thread inside `Shell::new`**
   (`src/ui/shell.rs:180`, `:189`) and again at `:770`. Making the keyring the default would put a
   D-Bus round trip (and potentially a 25 s zbus method timeout against a broken keyring) on the
   startup path — a new PF-09-class hazard in a file outside this agent's scope.
2. It keeps the test suite deterministic and off the real keyring.

So the audit's core complaint stands until one of these is done:

- **The Lead's choice A (recommended):** make the keyring default-on
  (`secret_service_enabled` returning `true` when the variable is *unset*), after moving the
  `load_password` calls off the startup path (or accepting the cost). Docs then become true.
- **The Lead's choice B:** keep the opt-in and correct `docs/setup.md:25-26`,
  `docs/troubleshooting.md:84-85`, `README.md:67-68` to say "the password is kept in a `0600`
  file; set `MYUZIK_KEYRING=1` to use the system keyring". Those files are outside this agent's
  scope.

Additional caveat of the current backend: one keyring account per install
(`KEYRING_ACCOUNT = "navidrome"`, the username lives in `config.toml`), because `load_password`
does not receive the username. Fine for one Navidrome account; a second account would overwrite
the entry.

---

## INT-10 (P1) — users see raw Rust `Debug` of errors

**Status: fixed in `src/navidrome/error.rs`; the call sites need the Lead (below).**

### Changes — `src/navidrome/error.rs:22-100`

`NavidromeError::user_message() -> &'static str` and `NavidromeError::hint() -> Option<&'static str>`.

| Variant | Message | Hint |
|---|---|---|
| `Unauthorized` | The server rejected your username or password. | Check the username and password, or set them again in Setup. |
| `NotFound` | The server could not find that. | It may have been removed or renamed on the server. |
| `Unsupported` | The server does not support this feature. | …password instead of a token. |
| `Network` + timeout | The server took too long to answer. | Check that the server is running and reachable, then try again. |
| `Network` | Myuzik cannot reach the server. | Check the server address and that you are online. |
| `Protocol` | The server answered with something Myuzik cannot read. | Make sure the server address points at a Navidrome server. |
| `Tls` | The server's certificate could not be verified. | Trust a self-signed certificate in the system store. |
| `Server` + `429` | The server is busy and asked Myuzik to slow down. | Wait a few seconds and try again. |
| `Server` + `5xx` | The server ran into a problem handling the request. | Try again in a moment, and check the server's log… |
| `Server` | The server refused the request. | Try again, or check the server's settings. |

Rate limiting and `5xx` are classified from the payload (`429` / a `5xx` token) because
`NavidromeError` has no `RateLimited`/`Timeout` variant and the mapping lives in
`src/navidrome/client.rs` (INT-12, outside this agent's scope). When those variants exist,
`user_message` should match on them instead — see the note at the end.

### Tests — `src/navidrome/error.rs` (`cargo test --lib navidrome::error`)

- `every_variant_has_a_plain_sentence` — non-empty, ends with `.`, no `http`/`Some(`/`{`/`Err(`
- `common_failures_read_differently` — wrong password / unreachable / rate limited / not found / timeout all differ, and each says the right thing
- `server_errors_are_split_by_status` — 429, 5xx and an opaque server message read differently; the rate-limit hint says "again"
- `hints_are_short_sentences_or_absent`
- `user_message_never_exposes_the_url_or_secret`

Red evidence: `error: no method named 'user_message' found for enum 'NavidromeError'` (15 errors).

---

## PF-09 (P1) — `Tray::start` blocks the main thread up to 10 s

**Status: fixed.**

### Changes

`src/platform/tray.rs`:

- `48` — `START_TIMEOUT: Duration = Duration::from_secs(2)` (was 10 s).
- `154`, `198-232` — `Tray::start_background(icon, size) -> Result<TrayStart, TrayError>` spawns the
  worker and returns immediately; `TrayStart = (Tray, Receiver<TrayCommand>, Receiver<Result<(), TrayError>>)`.
- `169-186` — `Tray::start` is now `start_background` + `wait_ready(START_TIMEOUT)`; its failure
  path calls `abandon()` (`275-287`) instead of `shutdown()`, so a worker stuck inside a D-Bus
  call is detached rather than joined (joining there would reintroduce the stall).
- `980-995` — `wait_ready(ready, timeout)`: the bounded wait, as a free function so it is testable.
- `997-1007` — `report_registration(ready)`.

`src/main.rs:133-140`:

```rust
match Tray::start_background(pixels, icon_side) {
    Ok((tray, commands, ready)) => {
        cx.background_executor().spawn(async move { report_registration(ready) }).detach();
        …
```

The UI thread no longer waits at all: the only D-Bus work left in `main` is on the tray worker
thread, and the *reporting* of the result is a background-executor task. The 2 s budget only
bounds how long the reporter waits before logging "system tray unavailable".

### Tests

- `src/platform/tray.rs::the_startup_wait_is_short` — `START_TIMEOUT <= 2 s` (would have failed on the old 10 s) and `>= 200 ms`
- `src/platform/tray.rs::waiting_for_registration_is_bounded` — an unanswered channel returns `Err` within the 20 ms timeout, a late `Ok` is accepted
- `src/platform/tray.rs::a_worker_that_dies_before_registering_is_a_start_failure` — a dropped sender is a failure, not a success

### Unverified

The "first frame is not delayed" claim is not measured here (that needs a running app and
`docs/performance.md`'s startup budget). What is proven is that the waiting code path is no longer
reachable from the UI thread and that the bound is 2 s.

---

## Required changes outside this agent's scope

### `src/ui/shell.rs` — INT-10 (never show `Debug`)

Line numbers are as of this branch now (another agent is editing the file, so match on the code):

| Line | Now | Replace with |
|---|---|---|
| `802` | `Err(e) => h.error = Some(format!("{:?}", e)),` | `Err(e) => h.error = Some(error_text(&e)),` |
| `1214` | `shell.setup_error = Some(format!("Connection failed: {e}"));` | `shell.setup_error = Some(error_text(&e));` |
| `1125` | `let msg = format!("Autoplay unavailable: {e:?}");` | `let msg = format!("Autoplay unavailable. {}", error_text(&e));` |
| `3932` | `let msg = format!("Library error: {e:?}");` | `let msg = format!("Library error. {}", error_text(&e));` |
| `342` | `let msg = format!("Playback error: {e:?}");` | stays, unless `PlayerError::user_message()` is added in `src/player/engine.rs` (INT-10 also mentions it; `src/player/` is out of scope) |

with one helper next to `spawn_home_fetch`:

```rust
/// A Navidrome failure as the user should read it: sentence plus advice,
/// never `Debug` (audit INT-10).
fn error_text(error: &crate::navidrome::error::NavidromeError) -> String {
    match error.hint() {
        Some(hint) => format!("{} {}", error.user_message(), hint),
        None => error.user_message().to_string(),
    }
}
```

`src/ui/shell.rs:1176-1214` already has the right structure for a better setup form: put
`error_text(&e)` in `setup_error` and, on success, show the store:

```rust
// INT-09 / INT-01: do not swallow the result, and tell the user where the
// password went.
match crate::config::save_password(&shell.paths, &pass) {
    Ok(store) => shell.setup_error = None, // or a toast: store.describe()
    Err(error) => {
        shell.setup_error = Some(format!(
            "Could not save your password: {error}. You will be asked for it again."
        ));
        return; // do not enter Home with a password that will not survive a restart
    }
}
```

(`src/ui/shell.rs:1191` is the `let _ = crate::config::save_password(&shell.paths, &pass);` to
replace; `CredentialStore::describe()` is at `src/config.rs:128-136`.)

### Optional follow-ups this work suggests (not required by the findings)

- `src/navidrome/error.rs` — when INT-12 adds `RateLimited { retry_after }` and
  `Timeout { phase }` to `NavidromeError` (`src/navidrome/client.rs:86-92`, out of scope),
  replace the `is_rate_limited` / `is_server_fault` / `is_timeout` payload sniffing with matches on
  those variants and delete `is_timeout`, `is_rate_limited`, `is_server_fault`
  (`src/navidrome/error.rs:83-100`).
- `docs/setup.md`, `docs/troubleshooting.md`, `README.md` — INT-01 wording (choice A or B above).
- `src/ui/shell.rs:180`, `:189`, `:770` — `load_password` on the main thread; a single load in
  `main.rs` before `Shell::new` would remove two of the three calls and is a precondition for
  making the keyring default-on.

---

## Evidence summary (commands run)

```
cargo test --offline --lib                                  103 passed, 0 failed, 1 ignored
cargo test --offline --test config_credentials               12 passed, 0 failed, 1 ignored
cargo test --offline --test <each other target>              green (see below)
cargo check --offline --all-targets                          clean
MYUZIK_KEYRING=1 cargo test --offline --test config_credentials -- --ignored
                                                             real keyring round trip ok
dbus-run-session -- cargo test --offline --test tray_reregistration -- --ignored
                                                             late watcher registered ok
cargo test --offline --lib platform::tray -- --ignored       live tray smoke test ok
rustfmt --edition 2024 (the four src files + the two test files)
cargo clippy --offline --all-targets -- -D warnings          no findings in this agent's files
```

Per target, run separately because `cargo test` stops at the first failing target:

```
a11y 10 · autoplay_flow 3 · config_credentials 12 · cover_lru 2 · lyrics_view 6 · navidrome_auth 4
overlay_lyrics 3 · queue 18 · radio 11 · stress 6 · ui_flow 9 · ui_shell 9
live_navidrome 9 ignored · live_player 1 ignored · screenshot_overlay 3 ignored
screenshots 1 ignored · tray_reregistration 1 ignored
```

**Known failure not caused by this work:** `cargo test --offline` currently fails to compile
`tests/mpris_metadata.rs` (`no repeat_mode_to_loop_status / queue_state in platform::mpris`) and,
just before that, `mpris_volume_follows_the_configured_value` failed at `tests/mpris_metadata.rs:97`.
Both are another agent's in-flight INT-03/INT-04 work (`src/platform/mpris.rs` and that test file
are theirs and out of this agent's scope). This agent's only change in that area is
`src/main.rs:78` passing `cfg.volume` into the new `start_mpris(player_tx, initial_volume)`.

Clippy note: `cargo clippy -- -D warnings` currently fails on `src/platform/mpris.rs` (and
earlier on `src/ui/shell.rs`, `src/lyrics/cache.rs`), all files other agents are editing. Filtering
the output for the files in this report returns nothing.

---

## What is still unverified

1. **INT-01 default state.** The keyring path works and is tested, but it is opt-in. Until the
   default is flipped (choice A) or the docs are corrected (choice B), a default install still
   stores the password in a `0600` file — exactly what the audit complains about.
2. **INT-02 window lifecycle.** `open_window` / `activate_window` / `destroyed-handle detection`
   are not exercised by any test; only the pure decisions are.
3. **INT-09 intermediate permissions.** The absence of a world-readable window is by construction,
   not observed.
4. **PF-09 startup budget.** Nothing measures the first frame; only the bound and the code path.
5. **INT-10 in the UI.** The library side is tested; the UI still prints `Debug` until the
   `src/ui/shell.rs` change above lands.
