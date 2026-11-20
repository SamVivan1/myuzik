# Player queue / state / MPRIS — audit fixes (FL-09, FL-15, FL-17, INT-03, INT-04)

Branch: `audit-fixes`. Files changed (only these):

- `src/player/queue.rs`
- `src/player/state.rs`
- `src/platform/mpris.rs`
- `tests/queue.rs`
- `tests/mpris_metadata.rs`

Method: strict TDD — test written and run first (red), then the minimal fix, then green.
Build cache was shared with other agents, so some runs waited on the cargo lock;
the lib was transiently uncompilable at one point because of another agent's in-flight
`src/platform/tray.rs` edit (not my change) — the run that produced the FL-17 red was
taken after the tree compiled again.

---

## FL-17 (P2) — `previous()` was dead after the queue was exhausted

**Status: fixed.**

`Queue::previous` fell through to `self.current.map(...)`, which is `None` once the queue
ran to its end, so Previous / Shift+Left / tray Previous did nothing exactly at the end of
the queue.

### Test added (red first)

`tests/queue.rs`

- `previous_after_exhaustion_restarts_last_track` (required name)
- `previous_after_exhaustion_restarts_the_last_shuffle_track` (shuffle variant of the same
  rule: restart on the last index of the shuffle order)

Red, before the fix:

```
$ cargo test --offline --test queue previous_after_exhaustion
running 2 tests
test previous_after_exhaustion_restarts_the_last_shuffle_track ... ok
test previous_after_exhaustion_restarts_last_track ... FAILED

thread 'previous_after_exhaustion_restarts_last_track' panicked at tests/queue.rs:69:5:
assertion `left == right` failed: Previous on an exhausted queue should restart the last track
  left: None
 right: Some("t2")

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 16 filtered out
```

The shuffle test already passed before the fix because `shuffle_pos > 0` still pointed into
the order after exhaustion; it is kept as a regression guard for the new branch.

### Change

`src/player/queue.rs:131-169` (`previous`): when `items` is non-empty and `current` is
`None`, the queue now re-enters playback on the last track instead of returning `None` —
`len - 1` in linear mode, `shuffle_order.len() - 1` (with `shuffle_pos` set to match) in
shuffle mode. In linear mode `i == 0` with repeat off now returns the current track
explicitly rather than relying on the `current.map` fallback.

### Green

```
$ cargo test --offline --test queue
running 18 tests
test previous_after_exhaustion_restarts_last_track ... ok
test previous_after_exhaustion_restarts_the_last_shuffle_track ... ok
... (16 pre-existing)
test result: ok. 18 passed; 0 failed
```

No `PlayerState`/shell change was needed: `PlayerState::previous` already consumes
`queue.previous()` and moves playback onto the returned track.

---

## FL-09 (P1) — MPRIS `Seek(offset)` was treated as an absolute position

**Status: fixed** (relative offset, no wrap, clamped to `[0, duration]`; `SetPosition`
stays absolute).

The handler converted the *relative* `Seek` offset with
`Duration::from_micros(time.as_micros() as u64)` and sent `PlayerCommand::Seek(pos)`, which
the engine applies as an absolute position. `Seek(-5s)` cast a negative `i64` to `u64`,
wrapping to an enormous value.

### Test added (red first)

`tests/mpris_metadata.rs`

- `seek_offset_is_relative_to_the_current_position` — `Seek(+10s)` at 120 s ⇒ 130 s
- `negative_seek_offset_moves_back_without_wrapping` — `Seek(-5s)` at 120 s ⇒ 115 s, and
  past the start ⇒ `0` (never a wrapped huge value)
- `seek_offset_is_clamped_to_the_track_duration` — past the end ⇒ duration; with no known
  duration only the lower bound applies

Red, before the fix:

```
$ cargo test --offline --test mpris_metadata
error[E0432]: unresolved import `myuzik::platform::mpris::resolve_relative_seek`
 --> tests/mpris_metadata.rs:2:58
  | ... no `resolve_relative_seek` in `platform::mpris`
error: could not compile `myuzik` (test "mpris_metadata") due to 1 previous error
```

Red for the right reason as far as a unit test can reach this bug: the faulty arithmetic
lived inside the `connect_seek` DBus closure, which needs a session bus to call, so the
resolution had to be extracted into a pure function before it could be tested at all. The
Red is the extraction being absent; the pre-fix behaviour is visible in the diff of the
handler (`Duration::from_micros(time.as_micros() as u64)` → absolute `Seek`).

### Change

- `src/platform/mpris.rs:190-226` — new `resolve_relative_seek(current_position,
  offset_micros, duration) -> Duration`: saturating signed add (never wraps), clamped to
  `>= 0` and `<= duration` when the duration is known. Helpers `duration_of`/`time_of`
  convert an MPRIS `Time` without an `as u64` cast on a possibly negative value.
- `src/platform/mpris.rs:94-105` (`connect_seek`) — computes the absolute target from
  `player.position()` (the position MPRIS itself reports, fed by the 10 Hz engine tick),
  the offset and the track length from the current metadata; sends the resulting
  `PlayerCommand::Seek`. It also calls `player.set_position(...)` with the resolved value so
  a second relative seek arriving before the next engine tick is applied to the right base.
- `src/platform/mpris.rs:113-117` (`connect_set_position`) — unchanged semantics (absolute),
  but a negative `Time` now clamps to zero instead of wrapping through `as u64`.

### Green

```
$ cargo test --offline --test mpris_metadata
running 10 tests
test negative_seek_offset_moves_back_without_wrapping ... ok
test seek_offset_is_clamped_to_the_track_duration ... ok
test seek_offset_is_relative_to_the_current_position ... ok
test result: ok. 10 passed; 0 failed
```

---

## INT-03 (P1) — MPRIS `Volume` was stale at startup

**Status: fixed** (MPRIS side + call site; the one-line call-site change was applied by the
Lead in `src/main.rs:78`).

`start_mpris` did `set_volume(0.8)` once, hardcoded, and never learned `config.volume`.

### Test added (red first)

`tests/mpris_metadata.rs`

- `mpris_volume_follows_the_configured_value` — a configured 30 % is reported as 30 %
  (not 80 %), and out-of-range values are clamped into MPRIS's `0.0..=1.0`.

Red, before the fix:

```
$ cargo test --offline --test mpris_metadata
error[E0432]: unresolved import `myuzik::platform::mpris::mpris_volume`
 --> tests/mpris_metadata.rs:2:45
  | ... no `mpris_volume` in `platform::mpris`
```

(Same shape as FL-09: the hardcode lived in the DBus thread, so only the converted pure
part is unit-testable; the wiring is proven by the signature change and the call site.)

### Change

- `src/platform/mpris.rs:34-42` — `start_mpris(player_tx, initial_volume: f32)`; the
  hardcoded `0.8` is gone.
- `src/platform/mpris.rs:119` — publishes `mpris_volume(initial_volume)` as soon as the
  player is built.
- `src/platform/mpris.rs:159-164` — `mpris_volume(f32) -> f64` clamps to `0.0..=1.0`; also
  used for `MprisCommand::UpdateVolume` (`:148`) so a desktop slider request cannot push an
  out-of-range value onto the bus.
- Call site (out of my file scope, applied by the Lead):

```rust
// src/main.rs:78
let mpris_handle = start_mpris(player_handle.tx.clone(), cfg.volume).ok();
```

Subsequent volume changes were already synced by `Shell` (`mh.update_volume(v)` at
`src/ui/shell.rs:605`, `:2278`, `:2323`); only the initial value was wrong.

### Green

```
$ cargo test --offline --test mpris_metadata
test mpris_volume_follows_the_configured_value ... ok
test result: ok. 10 passed; 0 failed
```

(The first green attempt failed on my own assertion (`mpris_volume(0.3)` is
`0.30000001192…` because `f32 → f64` is not exact); the test now compares numerically.
That was a test-precision mistake, not an implementation defect.)

---

## INT-04 (P1) — MPRIS did not expose `LoopStatus`/`Shuffle`, and Next/Previous were forced `true`

**Status: partially fixed.** The MPRIS reporting side is implemented and tested (queue →
`LoopStatus`/`Shuffle`/`can_go_next`/`can_go_previous`). Two pieces remain outside my file
scope:

1. `src/ui/shell.rs` must push the queue once (snippet below); until it does, MPRIS keeps
   the builder defaults (`can_go_next = false`, `can_go_previous = false`,
   `LoopStatus = None`, `Shuffle = false`) — which is honest but means Next/Previous appear
   disabled even when they work. **This is the one remaining wiring change needed.**
2. Controlling repeat/shuffle *from* a client (`Set LoopStatus` / `Set Shuffle`) is still
   not possible: it needs new `PlayerCommand` variants in `src/player/engine.rs` plus
   handling in `src/ui/shell.rs` (both out of scope). Because of that I deliberately left
   `can_set_loop_status` / `can_set_shuffle` at their `false` defaults rather than
   advertising a capability that would silently do nothing — MPRIS clients are then
   required not to offer those controls.

### Tests added (red first)

`tests/mpris_metadata.rs`

- `repeat_mode_maps_to_mpris_loop_status` — `Off → None`, `One → Track`, `All → Playlist`
- `queue_state_reports_next_and_previous_from_the_queue` — empty queue ⇒ both false; first
  of three ⇒ both true; last track with repeat off ⇒ next false, previous true; repeat all
  ⇒ next true and `LoopStatus::Playlist`; repeat one ⇒ next true and `LoopStatus::Track`
- `queue_state_advertises_the_whole_shuffle_round` — shuffle is reported and Next stays
  available for exactly the pending tracks of the shuffle order, then stops
- `exhausted_queue_still_advertises_previous` — after `next()` ran the queue out, Next is
  false and Previous is true (pairs with FL-17)

Red, before the fix:

```
$ cargo check --offline --test mpris_metadata
error[E0432]: unresolved imports `myuzik::platform::mpris::queue_state`, `myuzik::platform::mpris::repeat_mode_to_loop_status`
 --> tests/mpris_metadata.rs:4:46
```

### Change

- `src/player/queue.rs:222-261` — `Queue::can_go_next()` and `Queue::can_go_previous()`,
  mirroring the branches of `next()`/`previous()` (shuffle order and repeat included).
- `src/platform/mpris.rs:19-27` — `QueueState { can_go_next, can_go_previous, loop_status,
  shuffle }`.
- `src/platform/mpris.rs:168-183` — `queue_state(&Queue) -> QueueState` and
  `repeat_mode_to_loop_status(RepeatMode) -> LoopStatus`.
- `src/platform/mpris.rs:15, 129-134` — `MprisCommand::UpdateQueue(QueueState)` sets
  `can_go_next`, `can_go_previous`, `loop_status` and `shuffle` on the player.
- `src/platform/mpris.rs:125-128` — the forced `set_can_go_next(true)` /
  `set_can_go_previous(true)` in the metadata handler are gone; `can_go_next(true)` /
  `can_go_previous(true)` are gone from the builder (`:60-70`).
- `src/platform/mpris.rs:269-277` — `MprisHandle::update_queue(&Queue)`.

### Required call site (out of my scope — for the Lead)

`src/ui/shell.rs` (queue mutations live at `:814`, `:842`, `:1103-1105`, `play_next`
`:848`, `play_previous` `:862`):

```rust
/// Keep MPRIS's Next/Previous, LoopStatus and Shuffle in step with the queue.
fn sync_mpris_queue(&self, cx: &mut gpui::App) {
    if let Some(mh) = &self.mpris_handle {
        let queue = self.player_state.read(cx).queue.clone();
        mh.update_queue(&queue);
    }
}
```

Call `self.sync_mpris_queue(cx);` after the `player_state.update(...)` that changes the
queue in `play_now`/`play_tracks` (after `ps.play_now(...)`, `:814` and `:842`),
`play_next` (`:848`) and `play_previous` (`:862`), and after the radio resume in the
autoplay handler (`:1103-1105`). `Shell::play_next`/`play_previous` matter even when
nothing starts playing (queue ran out) — that is exactly where `can_go_next` has to flip to
`false`. (`Queue` is `Clone`, so the snapshot is cheap.)

### Green

```
$ cargo test --offline --test mpris_metadata
running 10 tests
test repeat_mode_maps_to_mpris_loop_status ... ok
test queue_state_reports_next_and_previous_from_the_queue ... ok
test queue_state_advertises_the_whole_shuffle_round ... ok
test exhausted_queue_still_advertises_previous ... ok
test result: ok. 10 passed; 0 failed
```

---

## FL-15 (P2) — dead field `PlayerState::scrobble_submitted`

**Status: fixed.**

No behavioural test is possible for "a field that nobody reads": the evidence is the grep
plus the crate compiling and the whole suite staying green.

### Evidence before

```
$ grep -rn "scrobble_submitted" src/ tests/
src/player/state.rs:22:    pub scrobble_submitted: bool,
src/player/state.rs:34:            scrobble_submitted: false,
src/player/state.rs:47:        self.scrobble_submitted = false;
src/player/state.rs:57:        self.scrobble_submitted = false;
src/player/state.rs:69:        self.scrobble_submitted = false;
src/player/state.rs:78:        self.scrobble_submitted = false;
```

One declaration, five writes, zero reads — the real tracker is the local
`ScrobbleTracker` in the shell event loop.

### Change

`src/player/state.rs` — removed the field from the struct (`:15-22`), from the constructor (`:24-33`) and
the four reset writes in `play_now`, `next`, `adopt` and `previous`. No other file
mentioned it, so nothing else changed.

### Evidence after

```
$ grep -rn "scrobble_submitted" src/ tests/ || echo "FL-15: zero occurrences left"
FL-15: zero occurrences left
```

---

## Verification

Commands and results (all from the repo root, branch `audit-fixes`):

```
$ cargo test --offline --test queue
test result: ok. 18 passed; 0 failed; 0 ignored

$ cargo test --offline --test mpris_metadata
test result: ok. 10 passed; 0 failed; 0 ignored

$ cargo test --offline --lib
test result: ok. 103 passed; 0 failed; 1 ignored
```

Neighbouring suites that depend on the changed semantics:

```
$ cargo test --offline --test autoplay_flow
test result: ok. 3 passed; 0 failed        # autoplay/EOF resume still correct

$ cargo test --offline --test stress
test result: ok. 6 passed; 0 failed        # 20k-track queue churn (next/previous/shuffle)

$ cargo test --offline --test ui_flow
test result: ok. 9 passed; 0 failed        # UI → PlayerCommand incl. Seek
```

Lint and formatting (my files only; `cargo fmt` would have reformatted other agents' files):

```
$ rustfmt --edition 2024 --check src/player/queue.rs src/player/state.rs \
      src/platform/mpris.rs tests/queue.rs tests/mpris_metadata.rs
fmt clean

$ cargo clippy --offline --all-targets -- -D warnings
(no output: no warnings, no errors)
```

## Not verified

- **Real MPRIS clients / the D-Bus path.** All new coverage is pure-function level
  (`resolve_relative_seek`, `mpris_volume`, `queue_state`, `repeat_mode_to_loop_status`,
  `Queue::can_go_next/can_go_previous`). The `connect_seek` wiring, the `Volume` value a
  desktop widget actually reads, and `LoopStatus`/`Shuffle` reaching `playerctl` were not
  exercised: there is no session-bus test harness in the suite, and adding one would be a
  new CI surface (the audit tracks that separately as INT-40).
- **INT-04 end-to-end** is pending the `Shell` call site above. Until then the MPRIS
  properties stay at the builder defaults, so a desktop media widget shows Next/Previous as
  unavailable even though the app can play them.
- **Control of repeat/shuffle from MPRIS** is intentionally not advertised
  (`can_set_loop_status`/`can_set_shuffle` remain false) because there is no
  `PlayerCommand` path to the engine — that needs `src/player/engine.rs` and
  `src/ui/shell.rs`, both outside my scope.
- The engine-side of FL-18 (seek past duration / seek before preroll) is a separate finding
  and is not addressed here; this fix only clamps *MPRIS-sourced* positions.
