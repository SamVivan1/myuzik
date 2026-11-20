# Player error / buffering / status — audit fixes (FL-07, FL-12, FL-13)

Branch: `audit-fixes`. Files changed (only these):

- `src/player/state.rs`
- `src/player/engine.rs`
- `tests/playback_error.rs` (new)

Out of scope, still **required** for FL-07/FL-12/FL-13 to reach the user: `src/ui/shell.rs`
(the event loop). The exact patch is in [Required shell wiring](#required-shell-wiring-for-the-lead-out-of-my-scope)
below. Until it lands, the new logic exists and is tested, but the UI still shows the old
behaviour.

Method: strict TDD per finding — test written and run first (red), minimal fix, green. The
build cache is shared with other agents, so several runs waited on the cargo lock, and several
runs died on *their* in-flight edits (`src/config.rs` borrow error, `src/main.rs`
`show_main_window` missing, `src/ui/shell.rs` closure arity). Every red/green quoted below
was taken on a tree where those files compiled; the failures shown are only mine.

---

## FL-07 (P1) — a failed stream left the UI claiming it was playing

**Status: partially fixed.**

Fixed in my scope: the state transition to `PlaybackStatus::Error`, a pure retry policy
(backoff → give up → advance) with a queue-level guard, and honest engine error reporting.
Not fixed here: the shell still shows a toast and keeps `Playing` — that is the wiring
below (`src/ui/shell.rs`, the Lead's file). `PlaybackStatus::Error` was constructed nowhere
in `src/` or `tests/` before this change (verified with
`grep -rn "PlaybackStatus::Error" src/ tests/` → no matches).

### Tests added (red first)

`tests/playback_error.rs`

- `a_stream_failure_moves_the_state_to_error_and_keeps_the_track`
- `toggle_from_error_does_not_pretend_playback_resumed`
- `toggle_from_buffering_does_not_pretend_playback_paused`
- `the_first_failure_retries_after_the_base_delay`
- `the_backoff_doubles_with_each_failed_attempt`
- `the_policy_gives_up_after_the_configured_attempts`
- `a_policy_with_a_single_attempt_gives_up_immediately`
- `attempt_zero_counts_as_the_first_failure`
- `the_default_policy_retries_before_it_gives_up`
- `a_huge_attempt_count_does_not_overflow_the_backoff`
- `the_failure_guard_stops_advancing_once_the_queue_looks_dead`
- `a_track_that_actually_played_clears_the_failure_run`

`src/player/engine.rs` (in-file unit tests, they exercise the private reporter)

- `a_failed_engine_command_is_reported_as_a_player_error`
- `a_successful_engine_command_reports_nothing`

Red, before either fix:

```
$ cargo test --offline --test playback_error
error[E0432]: unresolved imports `myuzik::player::state::RetryDecision`,
              `myuzik::player::state::RetryPolicy`, `myuzik::player::state::StreamFailureGuard`
error[E0599]: no method named `set_error` found for struct `PlayerState` ...
error: could not compile `myuzik` (test "playback_error") due to 3 previous errors

$ cargo test --offline --lib
error[E0425]: cannot find function `report_if_failed` in this scope
   --> src/player/engine.rs:194
error: could not compile `myuzik` (lib test) due to 2 previous errors
```

Green:

```
$ cargo test --offline --test playback_error
test result: ok. 12 passed; 0 failed; 0 ignored          # FL-07 stage (before FL-12/13 tests)

$ cargo test --offline --lib
test player::engine::tests::a_failed_engine_command_is_reported_as_a_player_error ... ok
test player::engine::tests::a_successful_engine_command_reports_nothing ... ok
test result: ok. 107 passed; 0 failed; 1 ignored
```

### Change

`src/player/state.rs`

| Lines | What |
|---|---|
| 5-8 | `MAX_RETRY_DELAY` (30 s ceiling for the backoff) |
| 19-31 | `RetryDecision { Retry { delay }, GiveUp }` |
| 33-84 | `RetryPolicy`: `new(max_attempts, base_delay)` (`46-77`), `max_attempts()`, `decide(attempt)` (1-based, `0` treated as the first attempt, doubling backoff, capped), `Default` (`79-84`) = 3 attempts / 500 ms base |
| 87-132 | `StreamFailureGuard`: `record_failure() -> bool` (true = stop advancing), `record_success()`, `consecutive_failures()`, `Default` = 3 |
| 216-232 | `toggle()` documentation: only Playing↔Paused flip; `Stopped`/`Buffering`/`Error` are no-ops and why |
| 278-285 | `set_error()` — enters `PlaybackStatus::Error`, keeps the current track, ends the pending load |

The policy is pure (no I/O, no clock): the shell owns the wait, the reload and the queue
advance. `decide` semantics: `max_attempts` counts *total* attempts per track, so
`RetryPolicy::default()` plays, retries after 500 ms, retries after 1 s, then gives up.

`src/player/engine.rs`

| Lines | What |
|---|---|
| 137-160 | `report_if_failed(evt_tx, action, result)` — sends `PlayerEvent::Error` for a failed command, returns whether it failed |
| 166-186 | `PlayerCommand::Load`: both `set_state` calls are reported; on failure it returns **before** `TrackStarted`, so a track that never started no longer triggers scrobble/metadata/lyrics |
| 193-207 | `PlayerCommand::Seek`: `seek_simple`'s `Result` is reported instead of `let _ =` |
| 221-253 | `#[cfg(test)] mod tests` for the reporter |

Deliberately unchanged: `PlayerCommand::Play`/`Pause`/`Stop`/`Toggle` still ignore
`set_state`'s `Result` (their failures are visible through the bus and are not part of this
finding), and the `pos.as_nanos() as u64` conversion in `Seek` is left alone (FL-18's clamp
is a separate finding).

---

## FL-12 (P2) — buffering was invisible

**Status: partially fixed.**

The status is now representable and tested; rendering it needs the shell (below) and,
for a real spinner, `src/ui/now_playing.rs` (Lead's file).

### Tests added (red first)

`tests/playback_error.rs`

- `a_stream_below_one_hundred_percent_says_buffering`
- `a_finished_buffer_leaves_the_status_to_the_engine`
- `the_player_shows_buffering_while_the_stream_fills`
- `buffering_that_completes_returns_to_playing_instead_of_sticking`
- `buffering_that_completes_does_not_overwrite_an_unrelated_status`

Red:

```
$ cargo test --offline --test playback_error
error[E0432]: unresolved import `myuzik::player::state::status_for_buffering`
error[E0599]: no method named `set_buffering` found for struct `PlayerState` ...
   --> tests/playback_error.rs:225,237,239,253
error: could not compile `myuzik` (test "playback_error") due to 5 previous errors
```

Green:

```
$ cargo test --offline --test playback_error
test result: ok. 17 passed; 0 failed; 0 ignored          # FL-07 + FL-12
```

### Change

`src/player/state.rs`

| Lines | What |
|---|---|
| 134-141 | `status_for_buffering(percent) -> Option<PlaybackStatus>`: `Some(Buffering)` below 100, `None` at 100 (the caller keeps its status; the engine's next `StateChanged` stays authoritative) |
| 287-303 | `PlayerState::set_buffering(percent)`: applies the helper; at 100 % it clears the indicator it itself put up (back to `Playing`) and leaves any other status alone, so a full buffer can never leave the UI stuck on "buffering" |

---

## FL-13 (P2) — two sources of truth for the status

**Status: partially fixed.**

`PlayerState` now filters the transient `Stopped` and exposes `is_load_pending()`. The
flicker only disappears once the shell (a) routes engine events through
`set_status_from_engine` and (b) derives its mirrors (`NowPlayingView.playing`, tray,
MPRIS) from the filtered status instead of the raw event — see the wiring below.

### Tests added (red first)

`tests/playback_error.rs`

- `a_transient_stopped_during_a_load_does_not_flip_the_status`
- `the_engine_playing_report_ends_the_pending_load`
- `a_paused_report_during_a_load_is_applied`
- `a_stop_the_user_asked_for_is_not_swallowed`
- `an_engine_error_during_a_load_ends_the_load`
- `moving_to_the_next_track_marks_a_new_load_pending`

Red (these were written **after** an earlier exploratory pass had already added
`load_pending`; that code was deleted first so the red is real):

```
$ cargo test --offline --test playback_error
error[E0599]: no method named `is_load_pending` found for struct `PlayerState` ...
error[E0599]: no method named `set_status_from_engine` found for struct `PlayerState` ...
(10 such errors in total: 6 call sites of the six FL-13 tests plus 4 repeats)
```

Green:

```
$ cargo test --offline --test playback_error
test result: ok. 23 passed; 0 failed; 0 ignored          # FL-07 + FL-12 + FL-13
```

### Change

`src/player/state.rs`

| Lines | What |
|---|---|
| 152-156 | private `load_pending` field (documented) |
| 179, 189, 201, 210 | `play_now` / `next` / `adopt` / `previous` mark the load pending — each is followed by a `Load` in the shell |
| 234-250 | `set_status` is the *authoritative* path (user transport action, queue exhausted). A terminal status (`Playing`/`Stopped`/`Error`) also ends the pending load, so a stop the user asked for is never filtered |
| 252-270 | `set_status_from_engine`: while a load is pending, `Stopped` is dropped (`Ready -> Paused` hop), `Playing`/`Error` end the load, `Paused`/`Buffering` are applied and keep it pending |
| 272-276 | `is_load_pending()` |

---

## Required shell wiring for the Lead (out of my scope)

`src/ui/shell.rs`. Line numbers are from the tree at the time of writing and will drift.

### 1. Locals in the player event loop (next to `let mut scrobble = ...`, ~line 234)

```rust
let mut retry_policy = crate::player::state::RetryPolicy::default();
let mut retry_attempts: u8 = 0;
let mut failure_guard = crate::player::state::StreamFailureGuard::default();
```

Add to the imports at the top of the file:

```rust
use crate::player::state::{PlaybackStatus, RetryDecision, RetryPolicy, StreamFailureGuard};
```

### 2. `Ok(PlayerEvent::StateChanged(s))` — filter, then mirror the filtered status (FL-13)

Replace the handler body (`~241-273`) with:

```rust
Ok(PlayerEvent::StateChanged(s)) => {
    // FL-13: one source of truth. `set_status_from_engine` drops the
    // transient `Stopped` GStreamer posts while a load is pending, and the
    // mirrors below follow the resulting status, not the raw event.
    let status = cx.update(|app| {
        state.update(app, |ps, _cx| {
            ps.set_status_from_engine(s);
            ps.status
        })
    });
    cx.update(|app| {
        let np = now_playing_for_loop.clone();
        app.update_entity(&np, |view, _cx| {
            view.set_playing(status == PlaybackStatus::Playing);
        });
        if let Some(tray) = tray_slot_for_loop.lock().ok().and_then(|slot| slot.clone()) {
            tray.set_playing(status == PlaybackStatus::Playing);
        }
    });
    if status == PlaybackStatus::Playing {
        // FL-07: the engine confirmed playback, so the retry budget restarts.
        retry_attempts = 0;
        failure_guard.record_success();
    }
    // MPRIS has no Buffering state (it maps to Stopped in
    // platform/mpris.rs:137-141), so keep the last real transport state while
    // buffering; Error maps to Stopped, which is honest.
    if status != PlaybackStatus::Buffering
        && let Some(mh) = &mpris_for_loop
    {
        mh.update_status(status);
    }
}
```

### 3. `Ok(PlayerEvent::Buffering(p))` — make it visible (FL-12), `~386`

```rust
Ok(PlayerEvent::Buffering(p)) => {
    // FL-12: a filling buffer is a status, not a debug line.
    let status = cx.update(|app| {
        state.update(app, |ps, _cx| {
            ps.set_buffering(p);
            ps.status
        })
    });
    // Keep the transport looking "on" while it stalls, so the button does not
    // flip to Play while audio is merely waiting for the network.
    let on = matches!(status, PlaybackStatus::Playing | PlaybackStatus::Buffering);
    cx.update(|app| {
        let np = now_playing_for_loop.clone();
        app.update_entity(&np, |view, _cx| view.set_playing(on));
        if let Some(tray) = tray_slot_for_loop.lock().ok().and_then(|slot| slot.clone()) {
            tray.set_playing(on);
        }
    });
    // A real spinner needs a `NowPlayingView::set_buffering(bool)` in
    // src/ui/now_playing.rs (Lead's file):
    //     pub fn set_buffering(&mut self, buffering: bool) { self.buffering = buffering; }
    // and a `cx.notify()` where the icon is rendered. Then call
    // `view.set_buffering(status == PlaybackStatus::Buffering)` above.
}
```

### 4. `Ok(PlayerEvent::Error(e))` — stop claiming Playing, retry, then advance (FL-07), `~339`

```rust
Ok(PlayerEvent::Error(e)) => {
    tracing::error!("player error: {:?}", e);
    // FL-07: never keep claiming Playing after a pipeline failure.
    cx.update(|app| {
        state.update(app, |ps, _cx| ps.set_error());
        let np = now_playing_for_loop.clone();
        app.update_entity(&np, |view, _cx| view.set_playing(false));
        if let Some(tray) = tray_slot_for_loop.lock().ok().and_then(|slot| slot.clone()) {
            tray.set_playing(false);
        }
    });
    if let Some(mh) = &mpris_for_loop {
        mh.update_status(PlaybackStatus::Error); // maps to Stopped: honest
    }
    // The listen so far is not a scrobble-worthy play.
    scrobble.reset();
    last_pos = std::time::Duration::ZERO;
    scrobble_track = None;

    match retry_policy.decide(retry_attempts.saturating_add(1)) {
        RetryDecision::Retry { delay } => {
            retry_attempts = retry_attempts.saturating_add(1);
            tracing::info!(?delay, attempt = retry_attempts, "retrying failed stream");
            let entity = self_entity.clone();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(delay).await;
                cx.update(|app| entity.update(app, |shell, cx| shell.retry_current_track(cx)));
            })
            .detach();
        }
        RetryDecision::GiveUp => {
            retry_attempts = 0;
            let dead_source = failure_guard.record_failure();
            let entity = self_entity.clone();
            cx.update(|app| {
                if dead_source {
                    // Every track in a row failed: the source is down, not the
                    // track. Stay on Error and say so instead of skipping the
                    // whole queue.
                    entity.update(app, |shell, cx| {
                        shell.show_toast(
                            "Playback failed repeatedly — check the server, then press play".into(),
                            cx,
                        );
                    });
                } else {
                    entity.update(app, |shell, cx| shell.play_next(cx));
                }
            });
        }
    }
}
```

`cx.spawn` inside the event loop is the same shape as the lyrics fetch
(`cx.spawn(async move |cx| ...)`, one argument — `AsyncApp::spawn(&self, AsyncFnOnce(&mut
AsyncApp))`), `BackgroundExecutor::timer(delay) -> Task<()>` is awaited directly, and
`AsyncApp::update`/`Entity::update` both return their closure's value (no `Result` to
unwrap), which is why the snippet above has no `.ok()`.

### 5. New `Shell` method — reload the failed track without touching the queue

Add next to `load_and_play` (`~1030`) and make it callable from the loop (it already calls
the private `load_and_play`, so the method itself can stay private):

```rust
/// Reload the current track after a retried stream failure (FL-07).
///
/// `adopt` marks a fresh load without touching the queue, which is what makes
/// the following transient `Stopped` filtered (FL-13).
fn retry_current_track(&mut self, cx: &mut gpui::App) {
    let Some(track) = self.player_state.read(cx).current.clone() else {
        return;
    };
    self.player_state.update(cx, |ps, _cx| ps.adopt(track.clone()));
    self.load_and_play(&track, cx);
}
```

### 6. `Ok(PlayerEvent::TrackStarted(id))` — reset the retry budget

At the top of that handler (`~345`), add `retry_attempts = 0;` (a successful load starts a
fresh budget for the new track).

### 7. `Ok(PlayerEvent::Eos)` — unchanged

It already uses `ps.set_status(PlaybackStatus::Stopped)`, which is the authoritative path and
also ends a pending load. Do **not** route it through `set_status_from_engine`.

### Optional, not required

Re-exporting the new types from `src/player/mod.rs` (out of my scope):

```rust
pub use state::{PlaybackStatus, PlayerState, RetryDecision, RetryPolicy, StreamFailureGuard};
pub use state::status_for_buffering;
```

Everything above works with the fully-qualified `crate::player::state::...` path, so this is
purely cosmetic. `tests/playback_error.rs` imports through `myuzik::player::state::...`.

---

## Verification

```
$ cargo test --offline --test playback_error
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

$ cargo test --offline --lib
test result: ok. 107 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.18s
```

Full suite (`cargo test --offline -- --test-threads=1`, every binary green, exit code 0):

```
unittests src/lib.rs            107 passed, 1 ignored
unittests src/main.rs             7 passed
tests/a11y.rs                    11 passed
tests/autoplay_flow.rs            3 passed
tests/config_credentials.rs      12 passed, 1 ignored
tests/config_resilience.rs        9 passed
tests/cover_lru.rs                2 passed
tests/live_navidrome.rs           0 passed, 9 ignored
tests/live_player.rs              0 passed, 1 ignored
tests/lyrics_view.rs              7 passed
tests/mpris_metadata.rs          10 passed
tests/navidrome_auth.rs           4 passed
tests/overlay_lyrics.rs           3 passed
tests/playback_error.rs          23 passed      <-- new file
tests/queue.rs                   18 passed
tests/radio.rs                   11 passed
tests/screenshot_overlay.rs       0 passed, 3 ignored
tests/screenshots.rs              0 passed, 1 ignored
tests/stress.rs                   6 passed
tests/tray_reregistration.rs      0 passed, 1 ignored
tests/ui_flow.rs                 11 passed
tests/ui_shell.rs                10 passed
Doc-tests myuzik                  0 passed, 1 ignored
```

Formatting. `rustfmt --check` on my three files is clean:

```
$ rustfmt --edition 2024 --check src/player/state.rs src/player/engine.rs tests/playback_error.rs
fmt clean
```

`cargo fmt --check` on the whole tree reports only `tests/a11y.rs:608`. That file is
byte-identical to HEAD (`git diff --quiet tests/a11y.rs` → clean), so the drift pre-dates my
work and I left it alone: it is outside my scope.

Clippy: with `-D warnings` the run stops on **one pre-existing lint in the Lead's
`src/ui/shell.rs:608`** (`collapsible_if`, in their in-flight edit). Allowing only that lint
proves the whole crate — my files included — is otherwise warning-free:

```
$ cargo clippy --offline --all-targets -- -D warnings
error: this `if` statement can be collapsed
   --> src/ui/shell.rs:608:24
   = note: `-D clippy::collapsible-if` implied by `-D warnings`

$ cargo clippy --offline --all-targets -- -D warnings -A clippy::collapsible-if
    Checking myuzik v0.1.0 (/home/samvivan/Documents/Myuzik)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.02s
```

No new dependency was added (`Cargo.toml` untouched).

## Not verified

- **No real GStreamer pipeline was run.** The engine change is covered by
  `report_if_failed` unit tests (Result → `PlayerEvent::Error`), not by a live stream that
  actually fails; `tests/live_player.rs` stays `#[ignore]` (it needs a real Navidrome).
- **The bus message order** that motivates FL-13 (`Ready -> Stopped -> Paused -> Playing` on
  load) is taken from the audit's reading of the code and GStreamer semantics, not from a
  captured log. `set_status_from_engine` only ever *filters* `Stopped`, so a wrong assumption
  about the order degrades to the current behaviour, not to a new bug.
- **`gst` posts `Buffering(100 %)`** — if it does not, the `set_buffering(100)` branch never
  runs and the status stays `Buffering` until the next `StateChanged`, which would leave the
  indicator on. This is the one assumption in FL-12 that real playback should confirm.
- **The UI/MPRIS wiring** (sections 2-6 above) is not applied: it is in `src/ui/shell.rs`
  (`now_playing.rs` for a spinner), outside my scope. Until the Lead applies it, a failed
  stream still leaves `PlaybackStatus::Playing` on screen, because the shell keeps calling
  `ps.set_status(s)` for engine events.
- **Retry policy tuning** (3 attempts, 500 ms base, 30 s cap, 3 dead tracks) is a judgement
  call, not a measured one.
