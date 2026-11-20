# Conventions

Coding conventions for Myuzik. Follow these so changes stay consistent and
reviewable. Evidence: `docs/audit/*` and `AGENTS.md`.

## 1. General
- Rust 2024 edition, rust-version 1.99. `cargo fmt` + `cargo clippy --all-targets
  -- -D warnings` must pass (CI enforces).
- No `unwrap()`/`expect()` on fallible paths that can hit users; use typed errors
  (`NavidromeError`, `PlayerError`) or `anyhow::Result` at boundaries.
- Logging via `tracing`; redact URLs/tokens. Default filter is ERROR — use
  `error!` for user-relevant failures, not `warn!`.
- Never block the UI thread; all I/O on `background_executor`.

## 2. GPUI
- GPUI revision is pinned (`14b7b0ef…`); do not bump without checking APIs.
- `wayland` feature on `gpui_platform` is mandatory.
- Reserve focus border via `border_2()` always (no layout shift).
- Use OS-accent focus rings; respect `reduce_motion`.
- Async results must be generation-guarded (see search/lyrics patterns) to avoid
  stale responses overwriting newer state.
- Window min size 960x640; app id `io.github.samvivan1.Myuzik`.

## 3. Async / network
- All HTTP via `background_executor`; never in render/update.
- Explicit timeouts: connect 8s, recv 15s, global 25s; LRCLIB 8/12/20s.
- Do not hold `Mutex<Radio>` across HTTP (FL-01) — lock, clone, drop, then fetch.

## 4. Theme / UI
- Use `theme::midnight_brass()` tokens only; no raw colors.
- Tab order: sidebar → content → player bar → window chrome.
- Accesskit roles/labels on every icon-only control.
- Toast is the intended error surface (currently not rendered — UI-02); wire it
  when adding new failure paths.

## 5. Errors & state
- Surface failures to the user; do not swallow. `PlayerEvent::Error` should move
  status to `PlaybackStatus::Error` + retry/auto-advance (FL-07).
- Generation guards for async: search, lyrics, and (should add) cover/radio/album.

## 6. Testing
- Run serially: `cargo test -- --test-threads=1` with `RUST_MIN_STACK=33554432`
  (set in `.cargo/config.toml`).
- Live/network/GStreamer tests are `#[ignore]`d; gate with `MYUZIK_LIVE=1`.
- Stress tests use 20k tracks / 200 searches / 500 covers.
- Prefer testing behavior through the real code path, not just isolated helpers.

## 7. Memory & performance
- Idle ≤ 200 MB, playing+overlay ≤ 250 MB, startup ≤ 1.5 s.
- Decode/encode covers off UI thread (PF-01).
- Measure with `scripts/measure-ram.sh`.

## 8. Documentation discipline
- Do not claim a feature works unless verified in code. Audit docs (`docs/audit/`)
  are the evidence base; many README/progress claims are aspirational.
- When changing behavior, update `docs/progress.md` and relevant audit fix notes.