use crate::player::state::PlaybackStatus;
use flume::{Receiver, Sender};
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use std::thread;
use std::time::{Duration, Instant};

/// How many commands may wait for the player thread.
///
/// The queue used to be unbounded, so a thread that stopped draining (or never
/// started, when `gst::init` failed) let commands pile up for the rest of the
/// process's life (audit FL-19). flume's bounded and unbounded channels share
/// the same `Sender`/`Receiver` types, so `PlayerHandle { tx, rx }` keeps
/// compiling for every existing caller, tests included.
const COMMAND_QUEUE: usize = 256;

#[derive(Debug, Clone)]
pub enum PlayerError {
    Message(String),
}

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    Load {
        uri: String,
        track_id: String,
    },
    Play,
    Pause,
    Toggle,
    Seek(Duration),
    SetVolume(f32),
    Stop,
    Next,
    Previous,
    /// Quit the engine: the player thread puts the pipeline back to `Null`,
    /// quits its GLib main loop and returns.
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    StateChanged(PlaybackStatus),
    Position(Duration),
    Duration(Duration),
    Eos,
    Error(PlayerError),
    Buffering(u8),
    TrackStarted(String),
    NextRequested,
    PreviousRequested,
}

#[derive(Clone)]
pub struct PlayerHandle {
    pub tx: Sender<PlayerCommand>,
    pub rx: Receiver<PlayerEvent>,
}

impl PlayerHandle {
    /// Build a handle from a pair of channels, for callers that talk to an
    /// engine stand-in (the UI tests do).
    pub fn new(tx: Sender<PlayerCommand>, rx: Receiver<PlayerEvent>) -> Self {
        Self { tx, rx }
    }

    /// Ask the player thread to stop.
    ///
    /// The thread answers by putting the pipeline back to `Null` (so the
    /// decoder and the audio device are released) and quitting its GLib main
    /// loop. This only sends; use [`shutdown_and_join`](Self::shutdown_and_join)
    /// when the caller has to know the thread is gone.
    pub fn shutdown(&self) {
        let _ = self.tx.send(PlayerCommand::Shutdown);
    }

    /// [`shutdown`](Self::shutdown), then wait for the thread to exit.
    ///
    /// Returns `false` if it is still running after `timeout`. The thread owns
    /// both command receivers and drops them as it returns, so a disconnected
    /// command channel is the thread's exit signal: by the time it is
    /// disconnected the pipeline has been released. This blocks the caller, so
    /// call it when the process is going away, not on the UI hot path.
    pub fn shutdown_and_join(&self, timeout: Duration) -> bool {
        self.shutdown();
        let deadline = Instant::now().checked_add(timeout);
        loop {
            if self.tx.is_disconnected() {
                return true;
            }
            match deadline {
                Some(deadline) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(2));
                }
                _ => return false,
            }
        }
    }
}

pub fn spawn_player() -> PlayerHandle {
    let (cmd_tx, cmd_rx) = flume::bounded::<PlayerCommand>(COMMAND_QUEUE);
    let (evt_tx, evt_rx) = flume::unbounded::<PlayerEvent>();

    thread::spawn(move || {
        if gst::init().is_err() {
            let _ = evt_tx.send(PlayerEvent::Error(PlayerError::Message(
                "gstreamer init failed".into(),
            )));
            return;
        }

        let pipeline = match gst::ElementFactory::make("playbin").build() {
            Ok(p) => p,
            Err(_) => {
                let _ = evt_tx.send(PlayerEvent::Error(PlayerError::Message(
                    "playbin not available".into(),
                )));
                return;
            }
        };

        pipeline.set_property("volume", 0.8_f64);

        let Some(bus) = pipeline.bus() else {
            let _ = evt_tx.send(PlayerEvent::Error(PlayerError::Message(
                "playbin has no bus".into(),
            )));
            release_pipeline(&pipeline);
            return;
        };
        let evt_tx_bus = evt_tx.clone();
        let watch = bus.add_watch(move |_bus, msg| {
            use gst::MessageView;
            match msg.view() {
                MessageView::Eos(_) => {
                    let _ = evt_tx_bus.send(PlayerEvent::Eos);
                }
                MessageView::Error(err) => {
                    let _ = evt_tx_bus.send(PlayerEvent::Error(PlayerError::Message(
                        err.error().to_string(),
                    )));
                }
                MessageView::StateChanged(sc) => {
                    if sc.src().is_some() {
                        let status = match sc.current() {
                            gst::State::Playing => PlaybackStatus::Playing,
                            gst::State::Paused => PlaybackStatus::Paused,
                            gst::State::Ready | gst::State::Null => PlaybackStatus::Stopped,
                            _ => PlaybackStatus::Buffering,
                        };
                        let _ = evt_tx_bus.send(PlayerEvent::StateChanged(status));
                    }
                }
                MessageView::Buffering(b) => {
                    let _ = evt_tx_bus.send(PlayerEvent::Buffering(b.percent() as u8));
                }
                _ => {}
            }
            glib::ControlFlow::Continue
        });
        let watch = match watch {
            Ok(watch) => watch,
            Err(err) => {
                let _ = evt_tx.send(PlayerEvent::Error(PlayerError::Message(format!(
                    "could not watch the pipeline bus: {err}"
                ))));
                release_pipeline(&pipeline);
                return;
            }
        };

        let pipeline_clone = pipeline.clone();
        let evt_tx_tick = evt_tx.clone();
        let cmd_rx_tick = cmd_rx.clone();
        let main_loop = glib::MainLoop::new(None, false);
        let main_loop_tick = main_loop.clone();

        // 100ms tick: drain commands, query position/duration.
        //
        // Returning `ControlFlow::Break` destroys the source and with it the
        // clones it holds, which is what lets the thread's senders drop on
        // shutdown; quitting the loop first and keeping the source alive would
        // leave the pipeline and the channels owned by a loop that never runs
        // again (audit FL-19).
        // Position is published every 5th tick (500 ms) to cut MPRIS/UI traffic
        // (PF-18); commands are still drained every 100 ms.
        let mut pos_counter: u8 = 0;
        glib::timeout_add_local(Duration::from_millis(100), move || {
            loop {
                match cmd_rx_tick.try_recv() {
                    Ok(cmd) => {
                        if let After::Quit = handle_command(&pipeline_clone, cmd, &evt_tx_tick) {
                            main_loop_tick.quit();
                            return glib::ControlFlow::Break;
                        }
                    }
                    Err(flume::TryRecvError::Empty) => break,
                    // Every `PlayerHandle` is gone, so no one can send another
                    // command and no one is listening: stop instead of keeping
                    // the loop and the pipeline alive until process exit.
                    Err(flume::TryRecvError::Disconnected) => {
                        main_loop_tick.quit();
                        return glib::ControlFlow::Break;
                    }
                }
            }
            pos_counter = pos_counter.wrapping_add(1);
            if pos_counter.is_multiple_of(5)
                && let Some(pos) = pipeline_clone.query_position::<gst::ClockTime>()
            {
                let _ =
                    evt_tx_tick.send(PlayerEvent::Position(Duration::from_nanos(pos.nseconds())));
            }
            if let Some(dur) = pipeline_clone.query_duration::<gst::ClockTime>() {
                let _ =
                    evt_tx_tick.send(PlayerEvent::Duration(Duration::from_nanos(dur.nseconds())));
            }
            glib::ControlFlow::Continue
        });

        main_loop.run();

        // Cleanup: the tick only quits the loop, so releasing the pipeline here
        // keeps the (possibly blocking) state change out of the GLib callback.
        release_pipeline(&pipeline);
        // Removes the bus watch and the sender clone it owns.
        drop(watch);
    });

    PlayerHandle {
        tx: cmd_tx,
        rx: evt_rx,
    }
}

/// Put the pipeline back to `Null`, releasing the decoder and the audio device,
/// and wait for that state change to land.
fn release_pipeline(pipeline: &gst::Element) {
    let _ = pipeline.set_state(gst::State::Null);
    let _ = pipeline.state(gst::ClockTime::ZERO);
}

/// What the tick loop should do once a command has run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum After {
    Continue,
    /// The engine was asked to stop: quit the main loop and return from the thread.
    Quit,
}

/// Report a failed engine command as [`PlayerEvent::Error`] instead of
/// swallowing it (audit FL-07/FL-18). Returns `true` when the command failed.
///
/// The shell reacts to the event by retrying with a
/// [`crate::player::state::RetryPolicy`] and eventually advancing, so a failure
/// here must never be silent: a swallowed `seek_simple`/`set_state` error is
/// exactly the "button does nothing, UI says Playing" state the audit found.
fn report_if_failed<T, E: std::fmt::Display>(
    evt_tx: &flume::Sender<PlayerEvent>,
    action: &str,
    result: Result<T, E>,
) -> bool {
    match result {
        Ok(_) => false,
        Err(err) => {
            let _ = evt_tx.send(PlayerEvent::Error(PlayerError::Message(format!(
                "{action} failed: {err}"
            ))));
            true
        }
    }
}

fn handle_command(
    pipeline: &gst::Element,
    cmd: PlayerCommand,
    evt_tx: &flume::Sender<PlayerEvent>,
) -> After {
    match cmd {
        // The tick loop turns this into `ControlFlow::Break` + `main_loop.quit()`:
        // the pipeline is released by the thread body once the loop has stopped.
        PlayerCommand::Shutdown => return After::Quit,
        PlayerCommand::Load { uri, track_id } => {
            // A failed state change means the track cannot play: report it and
            // stop here, so the shell does not get `TrackStarted` (scrobble,
            // metadata, lyrics) for a track that never started.
            if report_if_failed(evt_tx, "load", pipeline.set_state(gst::State::Ready)) {
                return After::Continue;
            }
            pipeline.set_property("uri", uri);
            if report_if_failed(evt_tx, "load", pipeline.set_state(gst::State::Playing)) {
                return After::Continue;
            }
            let _ = evt_tx.send(PlayerEvent::TrackStarted(track_id));
        }
        PlayerCommand::Play => {
            let _ = pipeline.set_state(gst::State::Playing);
        }
        PlayerCommand::Pause => {
            let _ = pipeline.set_state(gst::State::Paused);
        }
        PlayerCommand::Toggle => {
            let (_, current, _) = pipeline.state(gst::ClockTime::ZERO);
            if current == gst::State::Playing {
                let _ = pipeline.set_state(gst::State::Paused);
            } else {
                let _ = pipeline.set_state(gst::State::Playing);
            }
        }
        PlayerCommand::Seek(pos) => {
            // Seeking before preroll (or past the end) fails; reporting it lets
            // the shell tell the user instead of the button doing nothing.
            report_if_failed(
                evt_tx,
                "seek",
                pipeline.seek_simple(
                    gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                    gst::ClockTime::from_nseconds(pos.as_nanos() as u64),
                ),
            );
        }
        PlayerCommand::SetVolume(v) => {
            pipeline.set_property("volume", v as f64);
        }
        PlayerCommand::Stop => {
            let _ = pipeline.set_state(gst::State::Null);
            let _ = evt_tx.send(PlayerEvent::StateChanged(PlaybackStatus::Stopped));
        }
        PlayerCommand::Next => {
            let _ = evt_tx.send(PlayerEvent::NextRequested);
        }
        PlayerCommand::Previous => {
            let _ = evt_tx.send(PlayerEvent::PreviousRequested);
        }
    }

    After::Continue
}

#[cfg(test)]
mod tests {
    use super::*;
    use flume::unbounded;

    #[test]
    fn a_failed_engine_command_is_reported_as_a_player_error() {
        let (tx, rx) = unbounded::<PlayerEvent>();

        let failed = report_if_failed(&tx, "seek", Err::<(), _>("pipeline not ready"));

        assert!(failed, "a failed command must be reported as failed");
        match rx.try_recv() {
            Ok(PlayerEvent::Error(PlayerError::Message(msg))) => {
                assert_eq!(msg, "seek failed: pipeline not ready");
            }
            other => panic!("expected PlayerEvent::Error, got {other:?}"),
        }
    }

    #[test]
    fn a_successful_engine_command_reports_nothing() {
        let (tx, rx) = unbounded::<PlayerEvent>();

        let failed = report_if_failed(&tx, "load", Ok::<u8, &str>(1));

        assert!(!failed);
        assert!(
            rx.try_recv().is_err(),
            "a command that worked must not post an error event"
        );
    }

    /// A player that leaves the pipeline in `Playing` keeps the audio device
    /// and the decoder alive until the process exits (audit FL-19); shutting
    /// down has to put it back to `Null`.
    #[test]
    fn shutting_down_releases_the_pipeline() {
        if gst::init().is_err() {
            eprintln!("skipped: gstreamer is not available");
            return;
        }
        let Ok(pipeline) = gst::ElementFactory::make("playbin").build() else {
            eprintln!("skipped: playbin is not available");
            return;
        };
        assert!(pipeline.set_state(gst::State::Ready).is_ok());
        assert_eq!(pipeline.current_state(), gst::State::Ready);

        release_pipeline(&pipeline);

        assert_eq!(
            pipeline.current_state(),
            gst::State::Null,
            "shutdown must put the pipeline back to Null"
        );
    }
}
