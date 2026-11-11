//! Player thread lifecycle (audit FL-19).
//!
//! The engine owns a thread running a GLib `MainLoop`; these tests pin the
//! properties the audit found missing:
//!
//! * the thread stops (and gives back its command receivers) once no
//!   [`PlayerHandle`] is left, instead of running until the process exits;
//! * an explicit shutdown quits the loop, releases the pipeline and lets the
//!   caller wait for the thread to be gone.
//!
//! One test at a time: `glib::timeout_add_local` and `Bus::add_watch` attach to
//! the *default* GLib main context, which is global to the process, so two
//! player threads sharing it concurrently would be unsafe. `serial()` keeps the
//! tests in this file from overlapping for that reason (see the FL-19 note in
//! `docs/audit/fixes/engine-provider.md`).

use myuzik::player::engine::{PlayerCommand, PlayerEvent, spawn_player};
use myuzik::player::state::PlaybackStatus;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

static ENGINE: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    // A panicking test must not poison the lock for the next one.
    ENGINE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Wait until every sender on the event channel is gone.
///
/// The player thread holds the only remaining senders (the bus watch and the
/// tick source), so a disconnected channel is the observable proof that the
/// thread dropped them, i.e. that it exited or is exiting.
fn wait_for_disconnect(events: &flume::Receiver<PlayerEvent>, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match events.recv_timeout(Duration::from_millis(20)) {
            Err(flume::RecvTimeoutError::Disconnected) => return true,
            Err(flume::RecvTimeoutError::Timeout) => {
                if Instant::now() >= deadline {
                    return false;
                }
            }
            // Events still flowing while the engine winds down.
            Ok(_) => {}
        }
    }
}

/// Prove the engine is really up before testing what happens when it goes
/// away, so a broken GStreamer install cannot make the test pass vacuously.
fn assert_engine_is_live(tx: &flume::Sender<PlayerCommand>, events: &flume::Receiver<PlayerEvent>) {
    tx.send(PlayerCommand::Stop).expect("send Stop");
    match events.recv_timeout(Duration::from_secs(10)) {
        Ok(PlayerEvent::StateChanged(PlaybackStatus::Stopped)) => {}
        Ok(PlayerEvent::Error(err)) => {
            panic!("gstreamer is not usable here, so the player loop cannot be tested: {err:?}")
        }
        other => panic!("the engine never drained a command (expected Stopped): {other:?}"),
    }
}

#[test]
fn shutdown_stops_the_engine_and_refuses_further_commands() {
    let _serial = serial();
    let handle = spawn_player();
    let events = handle.rx.clone();
    assert_engine_is_live(&handle.tx, &events);

    handle.shutdown();

    assert!(
        handle.shutdown_and_join(Duration::from_secs(5)),
        "shutdown did not stop the player thread"
    );
    assert!(
        handle.tx.send(PlayerCommand::Stop).is_err(),
        "a stopped engine must refuse commands instead of queueing them forever"
    );
    assert!(
        wait_for_disconnect(&events, Duration::from_millis(500)),
        "the stopped engine still holds its event senders"
    );
}

#[test]
fn dropping_the_handle_stops_the_player_thread() {
    let _serial = serial();
    let handle = spawn_player();
    let events = handle.rx.clone();
    assert_engine_is_live(&handle.tx, &events);

    // Losing the last handle closes the command channel. Nothing is left to
    // drain, so the thread must give up its loop, its pipeline and its senders
    // rather than lingering until process exit.
    drop(handle);

    assert!(
        wait_for_disconnect(&events, Duration::from_secs(5)),
        "the player thread outlived its handle: the command channel is closed \
         but the GLib main loop keeps running and still owns the pipeline"
    );
}
