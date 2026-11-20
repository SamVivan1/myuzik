# Engine lifecycle + lyrics provider — audit fixes (FL-19, PF-08)

Branch: `audit-fixes`. Files changed (only these, within my scope):

- `src/player/engine.rs`
- `src/lyrics/provider.rs`
- `tests/engine_lifecycle.rs` (new)

Out of scope, still **required** for FL-19 to cover "on normal app exit": one call site in
`src/main.rs` (or `src/ui/shell.rs`). The exact patch is in
[Required out-of-scope wiring](#required-out-of-scope-wiring).

Method: strict TDD per finding — test written and run first (red), minimal fix, green. Every
red/green quoted below was run with `cargo test --offline` on the shared working tree; other
agents were editing `src/ui/shell.rs`, `src/library/mod.rs`, `src/player/scrobble.rs` and
`tests/ui_shell.rs` at the same time, so the lib test count moves for reasons outside this
work (see [Unverified / notes](#unverified--notes)).

---

## FL-19 (P2) — the player thread is never shut down, and its channels are unbounded

**Status: fixed for everything inside `src/player/engine.rs`; the "normal app exit" half needs
the one-line call site listed below (out of my scope).**

### What changed (with line ranges)

`src/player/engine.rs`

| Lines | Change |
|---|---|
| 9–16 | `COMMAND_QUEUE = 256`, documented: the command channel is now `flume::bounded` |
| 24–39 | `PlayerCommand::Shutdown` (doc comment points at what the thread does with it) |
| 61–100 | `impl PlayerHandle`: `new(tx, rx)` constructor, `shutdown(&self)`, `shutdown_and_join(&self, timeout) -> bool` |
| 103 | `flume::bounded::<PlayerCommand>(COMMAND_QUEUE)` instead of `unbounded` |
| 126–178 | `bus()` / `add_watch()` are no longer `.expect(...)`: a failure posts `PlayerEvent::Error`, releases the pipeline and returns instead of panicking the thread |
| 179–215 | the 100 ms tick: `handle_command` now returns `After`; `After::Quit` and `TryRecvError::Disconnected` both `main_loop.quit()` **and** `return glib::ControlFlow::Break` |
| 217–223 | after `main_loop.run()`: `release_pipeline(&pipeline)` then `drop(watch)` |
| 234–238 | `fn release_pipeline`: `set_state(Null)` + `state()` to wait for it |
| 240–246 | `enum After { Continue, Quit }` |
| 270–279 | `handle_command(...) -> After`, `PlayerCommand::Shutdown => return After::Quit` before any pipeline use |
| 369–391 | new unit test `shutting_down_releases_the_pipeline` |

`tests/engine_lifecycle.rs` (new, 105 lines)

| Lines | Contents |
|---|---|
| 1–26 | module doc + `serial()` (see the GLib hazard note) |
| 36–50 | `wait_for_disconnect`: a disconnected event channel is the observable proof the thread dropped its senders |
| 54–64 | `assert_engine_is_live`: drains a `Stop` first, so a broken GStreamer install cannot make the test pass vacuously |
| 66–86 | `shutdown_stops_the_engine_and_refuses_further_commands` |
| 89–105 | `dropping_the_handle_stops_the_player_thread` |

The four FL-19 sub-findings:

1. **Pipeline never set to Null / loop never quit.** The tick now quits the loop on `Shutdown`
   and on a closed command channel, and the thread body always runs
   `release_pipeline(&pipeline)` (set to `Null` + wait for the state change) before it returns.
   The old unreachable "// cleanup" comment is now the guaranteed path. The GLib source is
   stopped with `ControlFlow::Break`, not just `quit()` — this matters: a source left attached
   to the default main context keeps its closure (and therefore `cmd_rx_tick`, `evt_tx_tick`
   and a `pipeline` clone) alive after the loop stops, which would have defeated both the join
   signal and the pipeline release. Verified by
   `dropping_the_handle_stops_the_player_thread` and `shutdown_stops_the_engine_and_refuses_further_commands`.
2. **Nothing drains `cmd_rx` when `gst::init` fails.** A thread that returns early drops
   `cmd_rx`, so the channel disconnects and `PlayerHandle::tx.send` fails immediately instead of
   queueing forever; and with a bounded queue a dead consumer can no longer grow the queue
   without limit. `shutdown_stops_the_engine_and_refuses_further_commands` asserts
   `tx.send(...).is_err()` after the thread is gone.
3. **`evt_tx` held by bus watch + tick keeps the event loop alive.** Fixed on the command side,
   which is the side that actually says "the consumer is gone": the tick breaks when
   `try_recv` reports `Disconnected` (all `PlayerHandle`s dropped), independent of how many
   event senders exist. `dropping_the_handle_stops_the_player_thread` is precisely this case.
4. **Unbounded channels.** Commands are bounded (256) and events stay unbounded **deliberately**:
   bounding `PlayerEvent` would make `evt_tx.send` block inside the GLib callback and inside the
   bus watch, i.e. a stalled UI would stall the player thread — a playback behaviour change the
   brief forbids. The event producer is per-state/per-track, not per-frame, so the growth risk
   the finding describes is on the command side.

### Why there is no `JoinHandle` field (and no API break)

`PlayerHandle` keeps exactly `pub tx` and `pub rx`. Adding a third field breaks every
`PlayerHandle { tx, rx }` struct literal, and `tests/` has 24 `PlayerHandle {` occurrences across 7 files outside my scope
(`tests/ui_flow.rs` 11, `tests/a11y.rs` 2, `tests/library_cache_flow.rs` 2, `tests/ui_shell.rs` 2,
`tests/screenshots.rs` 1, `tests/overlay_lyrics.rs` 1, `tests/autoplay_flow.rs` 1; a few of the
24 are `-> PlayerHandle` return types, the rest are struct literals) — that would have
left the whole test suite uncompilable until the Lead edited 10 files, colliding with the agents
editing them now. Instead the thread's exit is observed through a channel it already owns:

- The player thread owns **both** command receivers (`cmd_rx`, `cmd_rx_tick`) and drops them as
  it returns, so `Sender::is_disconnected()` becomes true only after the loop has stopped and
  `release_pipeline` has run.
- `shutdown_and_join(&self, timeout) -> bool` sends `Shutdown`, then polls
  `self.tx.is_disconnected()` with a 2 ms sleep up to `timeout` and returns `false` on timeout.
  No `unwrap`/`expect`, bounded, and it is only meant for process exit — documented as blocking.

This is a join in behaviour (caller learns the thread is gone, within a bound) without a struct
change. `PlayerHandle::new(tx, rx)` is offered as the constructor for anyone who prefers it; the
struct literals keep working. Verified: `cargo clippy --offline --all-targets -- -D warnings`
compiles every test binary in the repo (exit 0), and the full suite passes without touching one
existing test file.

### Tests added (red first)

| Test | File | First run |
|---|---|---|
| `dropping_the_handle_stops_the_player_thread` | `tests/engine_lifecycle.rs` | **red**, behaviour |
| `shutdown_stops_the_engine_and_refuses_further_commands` | `tests/engine_lifecycle.rs` | **red**, missing API |
| `shutting_down_releases_the_pipeline` | `src/player/engine.rs` (unit) | **red**, missing helper |

**Red 1 — the thread outlived its consumer (behavioural, not a compile error):**

```
$ cargo test --offline --test engine_lifecycle
running 1 test
test dropping_the_handle_stops_the_player_thread ... FAILED

---- dropping_the_handle_stops_the_player_thread stdout ----
thread 'dropping_the_handle_stops_the_player_thread' panicked at tests/engine_lifecycle.rs:78:5:
the player thread outlived its handle: the command channel is closed but the GLib main loop keeps running and still owns the pipeline

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.24s
```

Failing for the right reason: `assert_engine_is_live` had already received
`PlayerEvent::StateChanged(Stopped)` (so GStreamer and the tick loop were really running), and
the 5 s wait then proved the thread kept its event senders — the loop was still running after
the only `PlayerHandle` was dropped.

**Red 2 — the shutdown API did not exist:**

```
$ cargo test --offline --test engine_lifecycle
error[E0599]: no method named `shutdown` found for struct `PlayerHandle` in the current scope
   --> tests/engine_lifecycle.rs:73:12
error[E0599]: no method named `shutdown_and_join` found for struct `PlayerHandle` in the current scope
   --> tests/engine_lifecycle.rs:76:16
error: could not compile `myuzik` (test "engine_lifecycle") due to 2 previous errors
```

**Red 3 — the pipeline release helper did not exist:**

```
$ cargo test --offline --lib player::engine
error[E0425]: cannot find function `release_pipeline` in this scope
error: could not compile `myuzik` (lib test) due to 1 previous error
```

**Green:**

```
$ cargo test --offline --test engine_lifecycle
running 2 tests
test dropping_the_handle_stops_the_player_thread ... ok
test shutdown_stops_the_engine_and_refuses_further_commands ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.42s
```

```
$ cargo test --offline --lib player::engine
running 3 tests
test player::engine::tests::a_successful_engine_command_reports_nothing ... ok
test player::engine::tests::a_failed_engine_command_is_reported_as_a_player_error ... ok
test player::engine::tests::shutting_down_releases_the_pipeline ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 110 filtered out; finished in 0.02s
```

`shutting_down_releases_the_pipeline` was also run with `--nocapture` to confirm it did not take
the "gstreamer is not available" skip path: no skip message printed, and it asserts a real
`playbin` goes `Ready → Null`.

### Note on the GLib main context (hazard, not restructured)

`glib::timeout_add_local` and `Bus::add_watch` both attach to the **process-global default**
main context, and `timeout_add_local` does
`MainContext::default().acquire().expect("default main context already acquired by another
thread")` (`glib-0.22.10/src/source.rs:459-463`). Consequences:

- Safe today in production: the player thread is the only GLib loop owner in the process
  (`src/platform/mpris.rs` is zbus/tokio, the tray is its own thread with its own loop — per the
  performance audit's thread map).
- Not safe in general: a second player thread, or any other component that iterates the default
  context, makes `timeout_add_local` panic on the `expect` above (or makes two threads run the
  same context). That is why `tests/engine_lifecycle.rs` serialises its two tests on a mutex
  instead of running two players concurrently, and why I did **not** restructure the loop without
  evidence — no runtime failure was observed in this task.
- If a follow-up wants to remove the hazard, the change is `let ctx = glib::MainContext::new();`
  + `MainLoop::new(Some(&ctx), false)` + building the bus watch and timeout inside
  `ctx.with_thread_default(|| ...)`. Not done here: it is a structural change to a working
  playback path, outside the two findings.

### Required out-of-scope wiring

Nothing breaks without it (everything compiles and the thread exits when its last handle is
dropped), but "on **normal** app exit the pipeline is set to Null" needs a call before the
process goes away.

**Preferred — `src/ui/shell.rs` (Lead's file), inside `Shell::new`:**

```rust
// FL-19: stop the player thread (pipeline → Null, GLib loop quit) when the app quits.
cx.on_app_quit(|this, _cx| {
    let player = this.player_handle.clone();
    async move {
        if !player.shutdown_and_join(std::time::Duration::from_secs(2)) {
            tracing::warn!("the player thread did not stop within 2 s");
        }
    }
})
.detach(); // `Subscription::detach` exists in this gpui revision
```

**Alternative — `src/main.rs` only (no shell change).** The `app.run(move |cx| ...)` closure is
`move`, so a handle created inside it cannot escape; keep a slot outside:

```rust
// before `app.run(...)`, next to `let app = application();`
let player_shutdown: std::sync::Arc<std::sync::Mutex<Option<crate::player::engine::PlayerHandle>>> =
    std::sync::Arc::new(std::sync::Mutex::new(None));
let shutdown_slot = player_shutdown.clone();
```

```rust
// inside the closure, right after `let player_handle = spawn_player();`
*shutdown_slot
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(player_handle.clone());
```

```rust
// after `app.run(...)` returns (end of `fn main`, line ~402 in main.rs)
if let Some(player) = player_shutdown
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
    .take()
{
    if !player.shutdown_and_join(std::time::Duration::from_secs(2)) {
        tracing::warn!("the player thread did not stop within 2 s");
    }
}
```

---

## PF-08 (P1) — a new `ureq::Agent` is built for every track's LRCLIB lookup

**Status: fixed.** The agent is built once in `LyricsProvider::new` and reused for every lookup;
the timeouts are unchanged (8 s connect / 12 s receive / 20 s global). `LyricsProvider::new`'s
signature is unchanged, so `src/ui/shell.rs:232` and `:1620` and `tests/live_navidrome.rs` need
no edit.

### What changed (with line ranges)

`src/lyrics/provider.rs`

| Lines | Change |
|---|---|
| 36–38 | `const LRCLIB_BASE` — the origin, so a test can point lookups at a local stub |
| 46–63 | `LyricsProvider` gains `agent: ureq::Agent` and `lrclib_base: String` (private) |
| 65–70 | `#[cfg(test)] static AGENT_BUILDS` + `agent_builds()` — test instrumentation only |
| 127–141 | `fn build_agent()`: the one place an agent is built (same three timeouts as before) |
| 143–152 | `new()`: `agent: build_agent()` — **once per provider** |
| 154–161 | `#[cfg(test)] fn with_lrclib_base(...)`: the stub seam, test-only |
| 244–266 | `fetch_lrclib`: the inline agent construction is gone; `let agent = &self.agent;` |
| 269–298 | `lrclib_get` / `lrclib_search` build their URLs from `self.lrclib_base` |
| 484–604 | test stub: `LrclibStub` (counts TCP accepts and requests), `serve`, `candidate_json`, `lrclib_track` |
| 606–662 | new unit test `one_agent_serves_every_lrclib_lookup` |

The `base_url` field is production code (one `String`), not `#[cfg(test)]`: `fetch_lrclib` must
read it in every build. Its only non-default value comes from the test-only
`with_lrclib_base`, so production behaviour is byte-identical to before
(`https://lrclib.net`).

### The test proves reuse, not just "a field exists"

`one_agent_serves_every_lrclib_lookup` starts a real loopback HTTP server (`std::net`, no new
dependency, HTTP/1.1 keep-alive), points the provider at it, and fetches **two different tracks**
(different `album`, so the on-disk cache key differs and the second fetch really does a lookup).
It then asserts three things:

- the stub served **2 requests** (so the two lookups happened),
- the stub accepted **1 TCP connection** (the second lookup reused the pooled connection instead
  of paying a new handshake — the exact cost PF-08 describes),
- **0** additional agents were built between the two fetches (`AGENT_BUILDS` delta).

### Tests added (red first)

| Test | File |
|---|---|
| `one_agent_serves_every_lrclib_lookup` | `src/lyrics/provider.rs` (unit) |

**Red** — with only the seam + counter in place (no fix yet), two tracks opened two connections:

```
$ cargo test --offline --lib lyrics::provider::tests::one_agent
running 1 test
test lyrics::provider::tests::one_agent_serves_every_lrclib_lookup ... FAILED

---- lyrics::provider::tests::one_agent_serves_every_lrclib_lookup stdout ----
thread 'lyrics::provider::tests::one_agent_serves_every_lrclib_lookup' panicked at src/lyrics/provider.rs:630:9:
assertion `left == right` failed: the second track opened a new TCP connection: the agent was rebuilt for the lookup instead of being reused
  left: 2
 right: 1

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 113 filtered out; finished in 0.01s
```

The `stub.requests == 2` assertion above it had already passed, which is what makes the
connection count meaningful: the failure is the per-track agent, not a missing request.

**Green:**

```
$ cargo test --offline --lib lyrics::provider
running 9 tests
test lyrics::provider::tests::instrumental_is_reported_when_flagged ... ok
test lyrics::provider::tests::normalization_ignores_punctuation_and_decorations ... ok
test lyrics::provider::tests::names_must_agree_to_accept_a_candidate ... ok
test lyrics::provider::tests::falls_back_to_plain_when_the_synced_match_is_too_far ... ok
test lyrics::provider::tests::refuses_a_synced_match_that_is_a_different_recording ... ok
test lyrics::provider::tests::picks_the_recording_whose_duration_matches ... ok
test lyrics::provider::tests::rejects_synced_payloads_without_real_timings ... ok
test lyrics::provider::tests::unknown_duration_still_accepts_a_name_match ... ok
test lyrics::provider::tests::one_agent_serves_every_lrclib_lookup ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 105 filtered out; finished in 0.01s
```

---

## Gates (re-run, not claimed)

```
$ cargo clippy --offline --all-targets -- -D warnings
clippy exit: 0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.39s
```

```
$ cargo test --offline            # full suite, exit code 0
unittests src/lib.rs          113 passed; 0 failed; 1 ignored
unittests src/main.rs           7 passed; 0 failed
tests/engine_lifecycle.rs       2 passed; 0 failed
tests/ui_flow.rs               11 passed; 0 failed
tests/a11y.rs                  11 passed; 0 failed
tests/ui_shell.rs              12 passed; 0 failed
tests/autoplay_flow.rs          3 passed; 0 failed
tests/overlay_lyrics.rs         3 passed; 0 failed
tests/screenshots.rs            0 passed; 0 failed; 1 ignored
tests/playback_error.rs        23 passed; 0 failed
tests/queue.rs                 18 passed; 0 failed
tests/radio.rs                 11 passed; 0 failed
tests/stress.rs                 6 passed; 0 failed
... (28 binaries total, all ok)
```

Formatting: `rustfmt --edition 2024 src/player/engine.rs src/lyrics/provider.rs
tests/engine_lifecycle.rs` (my files only — `cargo fmt` would rewrite files other agents are
editing).

## Unverified / notes

- **The UI does not yet call `shutdown`.** Until the snippet above lands, normal app exit still
  relies on the process ending; the engine-side capability and its tests are complete. This is a
  genuine gap in the end-to-end fix, not a claim of "done".
- **Lib test count moved 114 → 113** across my runs while my two unit tests were added. Other
  agents are editing `src/player/scrobble.rs`, `src/library/mod.rs`, `src/ui/shell.rs` and
  `tests/ui_shell.rs` concurrently; the count is theirs, not a test of mine disappearing (both of
  mine are in the 113 and are named in the run above).
- **`MainLoop::new(None, false)` + `timeout_add_local` on a non-main thread** is *not*
  restructured. It is safe as long as the player thread is the only user of the process-global
  default main context; the panic path in glib's `acquire().expect(...)` is quoted above. No
  runtime failure was observed. `tests/engine_lifecycle.rs` serialises on a mutex for this reason.
- **`shutdown_and_join` polls** (`is_disconnected` every 2 ms until the timeout) because
  `PlayerHandle` could not gain a `JoinHandle` field without breaking 24 out-of-scope struct
  literals. Bounded and deterministic, but it does not reap the thread's stack the way a real
  `join()` would — the thread detaches once it returns.
- **Bounded command channel (256)**: all existing senders use blocking `send` (e.g.
  `src/ui/shell.rs:612`), so a player thread that is alive but wedged could block a UI call once
  256 commands are queued. Today that requires a wedged GLib loop; the previous behaviour was no
  backpressure at all. If the Lead prefers zero risk on the UI path, the senders should move to
  `send_timeout(.., 0ms)`/`try_send` — out of my scope (`src/ui/**`).
- **Not measured**: the actual latency/CPU saved per track by connection reuse (no network to
  `lrclib.net` in this environment; only the loopback reuse proof above). The TLS-session saving
  is inferred from the pooled connection, not measured against the real service.
