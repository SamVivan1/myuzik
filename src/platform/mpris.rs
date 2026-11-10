use crate::library::Track;
use crate::player::engine::PlayerCommand;
use crate::player::queue::{Queue, RepeatMode};
use crate::player::state::PlaybackStatus;
use flume::Sender;
use mpris_server::{LoopStatus, Player};
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum MprisCommand {
    UpdateMetadata(Box<Track>, Option<String>),
    UpdateStatus(PlaybackStatus),
    UpdatePosition(Duration),
    UpdateVolume(f32),
    UpdateQueue(QueueState),
    Quit,
}

/// What MPRIS mirrors about the queue: the transport capabilities it may
/// advertise and the `LoopStatus`/`Shuffle` properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueState {
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub loop_status: LoopStatus,
    pub shuffle: bool,
}

#[derive(Clone)]
pub struct MprisHandle {
    pub cmd_tx: Sender<MprisCommand>,
}

/// Start the MPRIS service.
///
/// `initial_volume` is the volume playback actually starts at (the configured
/// one); it is published as soon as the player exists so desktop media widgets
/// do not show a stale/hardcoded value until the user touches the slider.
pub fn start_mpris(
    player_tx: Sender<PlayerCommand>,
    initial_volume: f32,
) -> std::result::Result<MprisHandle, String> {
    let (cmd_tx, cmd_rx) = flume::unbounded::<MprisCommand>();
    let handle_tx = cmd_tx.clone();

    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("mpris: failed to create runtime: {e}");
                return;
            }
        };

        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async move {
            let player = match Player::builder("io.github.samvivan1.Myuzik")
                .identity("Myuzik")
                .desktop_entry("io.github.samvivan1.Myuzik")
                .can_play(true)
                .can_pause(true)
                // Next/Previous and LoopStatus/Shuffle follow the queue, so
                // they are published by MprisCommand::UpdateQueue instead of
                // being forced on at startup.
                .can_seek(true)
                .can_control(true)
                .build()
                .await
            {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("mpris: failed to build player: {e}");
                    return;
                }
            };

            // Handle control callbacks — forward to player thread
            let player_tx_clone = player_tx.clone();
            player.connect_play_pause(move |_p| {
                let _ = player_tx_clone.send(PlayerCommand::Toggle);
            });
            let player_tx_clone = player_tx.clone();
            player.connect_next(move |_p| {
                let _ = player_tx_clone.send(PlayerCommand::Next);
            });
            let player_tx_clone = player_tx.clone();
            player.connect_previous(move |_p| {
                let _ = player_tx_clone.send(PlayerCommand::Previous);
            });
            let player_tx_clone = player_tx.clone();
            player.connect_seek(move |player, offset| {
                let position = resolve_relative_seek(
                    player_position(player),
                    offset.as_micros(),
                    player_duration(player),
                );
                let _ = player_tx_clone.send(PlayerCommand::Seek(position));
                // Keep the reported position in step, so a second relative seek
                // arriving before the next engine position tick is applied to
                // the right base instead of the stale one.
                player.set_position(time_of(position));
            });
            let player_tx_clone = player_tx.clone();
            let volume_cmd_tx = cmd_tx.clone();
            player.connect_set_volume(move |_p, volume| {
                let _ = player_tx_clone.send(PlayerCommand::SetVolume(volume as f32));
                let _ = volume_cmd_tx.send(MprisCommand::UpdateVolume(volume as f32));
            });
            let player_tx_clone = player_tx.clone();
            player.connect_set_position(move |_p, _track_id, time| {
                // SetPosition is absolute; a negative value must not wrap.
                let pos = duration_of(time);
                let _ = player_tx_clone.send(PlayerCommand::Seek(pos));
            });

            let _ = player.set_volume(mpris_volume(initial_volume)).await;
            tokio::task::spawn_local(player.run());

            // Process commands
            while let Ok(cmd) = cmd_rx.recv_async().await {
                match cmd {
                    MprisCommand::UpdateMetadata(track, art_url) => {
                        let meta = metadata_for(&track, art_url);
                        let _ = player.set_metadata(meta).await;
                    }
                    MprisCommand::UpdateQueue(state) => {
                        let _ = player.set_can_go_next(state.can_go_next).await;
                        let _ = player.set_can_go_previous(state.can_go_previous).await;
                        let _ = player.set_loop_status(state.loop_status).await;
                        let _ = player.set_shuffle(state.shuffle).await;
                    }
                    MprisCommand::UpdateStatus(status) => {
                        use crate::player::state::PlaybackStatus;
                        let ps = match status {
                            PlaybackStatus::Playing => mpris_server::PlaybackStatus::Playing,
                            PlaybackStatus::Paused => mpris_server::PlaybackStatus::Paused,
                            _ => mpris_server::PlaybackStatus::Stopped,
                        };
                        let _ = player.set_playback_status(ps).await;
                    }
                    MprisCommand::UpdatePosition(pos) => {
                        player.set_position(time_of(pos));
                    }
                    MprisCommand::UpdateVolume(v) => {
                        let _ = player.set_volume(mpris_volume(v)).await;
                    }
                    MprisCommand::Quit => break,
                }
            }
        });
    });

    Ok(MprisHandle { cmd_tx: handle_tx })
}

/// The volume MPRIS reports for a player volume: normalized to MPRIS's
/// `0.0..=1.0` range, so neither a config value nor a desktop slider request
/// can push an out-of-range value onto the bus.
pub fn mpris_volume(volume: f32) -> f64 {
    (volume as f64).clamp(0.0, 1.0)
}

/// The MPRIS view of `queue`: Next/Previous capabilities, [`LoopStatus`] and
/// the shuffle flag.
pub fn queue_state(queue: &Queue) -> QueueState {
    QueueState {
        can_go_next: queue.can_go_next(),
        can_go_previous: queue.can_go_previous(),
        loop_status: repeat_mode_to_loop_status(queue.repeat()),
        shuffle: queue.shuffle(),
    }
}

/// MPRIS `LoopStatus` for a queue repeat mode.
pub fn repeat_mode_to_loop_status(mode: RepeatMode) -> LoopStatus {
    match mode {
        RepeatMode::Off => LoopStatus::None,
        RepeatMode::One => LoopStatus::Track,
        RepeatMode::All => LoopStatus::Playlist,
    }
}

pub fn position_us(position: Duration) -> i64 {
    position.as_micros() as i64
}

/// Resolve an MPRIS `Seek(offset)` into the absolute position to play.
///
/// `offset` is *relative* to the position MPRIS currently reports and may be
/// negative. The arithmetic saturates instead of wrapping, and the result is
/// clamped to `[0, duration]`; when the duration is unknown only the lower
/// bound applies. (A `Seek(-5s)` near the start used to cast the negative
/// offset to `u64` and jump the player to a random position or EOS.)
pub fn resolve_relative_seek(
    current_position: Duration,
    offset_micros: i64,
    duration: Option<Duration>,
) -> Duration {
    let base = i64::try_from(current_position.as_micros()).unwrap_or(i64::MAX);
    let mut target = base.saturating_add(offset_micros).max(0);
    if let Some(duration) = duration {
        target = target.min(i64::try_from(duration.as_micros()).unwrap_or(i64::MAX));
    }
    Duration::from_micros(target as u64)
}

/// An MPRIS [`Time`](mpris_server::Time) as a [`Duration`]; a negative time
/// (invalid on the wire) clamps to zero rather than wrapping.
fn duration_of(time: mpris_server::Time) -> Duration {
    Duration::from_micros(time.as_micros().max(0) as u64)
}

fn time_of(position: Duration) -> mpris_server::Time {
    mpris_server::Time::from_micros(i64::try_from(position.as_micros()).unwrap_or(i64::MAX))
}

fn player_position(player: &Player) -> Duration {
    duration_of(player.position())
}

/// Length of the track MPRIS is showing, when it knows it.
fn player_duration(player: &Player) -> Option<Duration> {
    player.metadata().length().map(duration_of)
}

pub fn metadata_for(track: &Track, art_url: Option<String>) -> mpris_server::Metadata {
    let mut meta = mpris_server::Metadata::new();
    meta.set_title(Some(track.title.clone()));
    meta.set_artist(Some(vec![track.artist.clone()]));
    meta.set_album(Some(track.album.clone()));
    meta.set_album_artist(Some(vec![track.artist.clone()]));
    meta.set_length(Some(mpris_server::Time::from_micros(position_us(
        track.duration,
    ))));
    if let Some(url) = art_url {
        meta.set_art_url(Some(url));
    }
    if let Ok(tid) =
        mpris_server::TrackId::try_from(format!("/org/mpris/MediaPlayer2/Track/{}", track.id))
    {
        meta.set_trackid(Some(tid));
    }
    if let Some(n) = track.track_number {
        meta.set_track_number(Some(n as i32));
    }
    if let Some(n) = track.disc_number {
        meta.set_disc_number(Some(n as i32));
    }
    meta
}

impl MprisHandle {
    pub fn update_metadata(&self, track: &Track, art_url: Option<String>) {
        let _ = self.cmd_tx.send(MprisCommand::UpdateMetadata(
            Box::new(track.clone()),
            art_url,
        ));
    }

    pub fn update_status(&self, status: PlaybackStatus) {
        let _ = self.cmd_tx.send(MprisCommand::UpdateStatus(status));
    }

    pub fn update_position(&self, pos: Duration) {
        let _ = self.cmd_tx.send(MprisCommand::UpdatePosition(pos));
    }

    /// Publish the queue's transport capabilities to MPRIS: whether Next and
    /// Previous lead anywhere, the repeat mode as `LoopStatus`, and `Shuffle`.
    pub fn update_queue(&self, queue: &Queue) {
        let _ = self
            .cmd_tx
            .send(MprisCommand::UpdateQueue(queue_state(queue)));
    }

    pub fn update_volume(&self, v: f32) {
        let _ = self.cmd_tx.send(MprisCommand::UpdateVolume(v));
    }

    pub fn quit(&self) {
        let _ = self.cmd_tx.send(MprisCommand::Quit);
    }
}
