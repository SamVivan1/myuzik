use crate::player::state::PlaybackStatus;
use flume::{Receiver, Sender};
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum PlayerError {
    Message(String),
}

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    Load { uri: String, track_id: String },
    Play,
    Pause,
    Toggle,
    Seek(Duration),
    SetVolume(f32),
    Stop,
    Next,
    Previous,
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

pub fn spawn_player() -> PlayerHandle {
    let (cmd_tx, cmd_rx) = flume::unbounded::<PlayerCommand>();
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

        let bus = pipeline.bus().expect("bus");
        let evt_tx_bus = evt_tx.clone();
        let _watch = bus
            .add_watch(move |_bus, msg| {
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
            })
            .expect("bus watch");

        let pipeline_clone = pipeline.clone();
        let evt_tx_tick = evt_tx.clone();
        let cmd_rx_tick = cmd_rx.clone();
        let main_loop = glib::MainLoop::new(None, false);

        // 100ms tick: drain commands, query position/duration
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            while let Ok(cmd) = cmd_rx_tick.try_recv() {
                handle_command(&pipeline_clone, cmd, &evt_tx_tick);
            }
            if let Some(pos) = pipeline_clone.query_position::<gst::ClockTime>() {
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

        // cleanup
        let _ = pipeline.set_state(gst::State::Null);
    });

    PlayerHandle {
        tx: cmd_tx,
        rx: evt_rx,
    }
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
) {
    match cmd {
        PlayerCommand::Load { uri, track_id } => {
            // A failed state change means the track cannot play: report it and
            // stop here, so the shell does not get `TrackStarted` (scrobble,
            // metadata, lyrics) for a track that never started.
            if report_if_failed(evt_tx, "load", pipeline.set_state(gst::State::Ready)) {
                return;
            }
            pipeline.set_property("uri", uri);
            if report_if_failed(evt_tx, "load", pipeline.set_state(gst::State::Playing)) {
                return;
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
}
