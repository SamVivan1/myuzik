use crate::library::Track;
use crate::player::state::PlaybackStatus;
use flume::Sender;
use mpris_server::Player;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum MprisCommand {
    UpdateMetadata(Track, Option<String>),
    UpdateStatus(PlaybackStatus),
    UpdatePosition(Duration),
    UpdateVolume(f32),
    Quit,
}

pub struct MprisHandle {
    pub cmd_tx: Sender<MprisCommand>,
}

pub fn start_mpris() -> std::result::Result<MprisHandle, String> {
    let (cmd_tx, cmd_rx) = flume::unbounded::<MprisCommand>();

    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("mpris: failed to create runtime: {e}");
                return;
            }
        };

        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async move {
            let player = match Player::builder("Myuzik")
                .identity("Myuzik")
                .desktop_entry("io.github.samvivan1.Myuzik")
                .can_play(true)
                .can_pause(true)
                .can_go_next(true)
                .can_go_previous(true)
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

            // Handle control callbacks
            player.connect_play_pause(|_p| {
                // In v1, MPRIS play/pause is handled by the player thread.
                // The UI should observe MPRIS commands; for now, log.
            });
            player.connect_next(|_p| {});
            player.connect_previous(|_p| {});
            player.connect_seek(|_p, _time| {});
            player.connect_set_position(|_p, _track_id, _time| {});

            // Spawn the player event loop
            tokio::task::spawn_local(player.run());

            // Process commands
            while let Ok(cmd) = cmd_rx.recv_async().await {
                match cmd {
                    MprisCommand::UpdateMetadata(track, art_url) => {
                        let mut meta = mpris_server::Metadata::new();
                        meta.set_title(Some(track.title.clone()));
                        meta.set_artist(Some(vec![track.artist.clone()]));
                        meta.set_album(Some(track.album.clone()));
                        meta.set_album_artist(Some(vec![track.artist.clone()]));
                        meta.set_length(Some(mpris_server::Time::from_micros(track.duration.as_micros() as i64)));
                        if let Some(url) = art_url {
                            meta.set_art_url(Some(url));
                        }
                        if let Ok(tid) = mpris_server::TrackId::try_from(format!("/org/mpris/MediaPlayer2/Track/{}", track.id)) {
                            meta.set_trackid(Some(tid));
                        }
                        let _ = player.set_metadata(meta).await;
                        let _ = player.set_can_go_next(true).await;
                        let _ = player.set_can_go_previous(true).await;
                    }
                    MprisCommand::UpdateStatus(status) => {
                        let ps = match status {
                            crate::player::state::PlaybackStatus::Playing => mpris_server::PlaybackStatus::Playing,
                            crate::player::state::PlaybackStatus::Paused => mpris_server::PlaybackStatus::Paused,
                            _ => mpris_server::PlaybackStatus::Stopped,
                        };
                        let _ = player.set_playback_status(ps).await;
                    }
                    MprisCommand::UpdatePosition(pos) => {
                        player.set_position(mpris_server::Time::from_micros(pos.as_micros() as i64));
                    }
                    MprisCommand::UpdateVolume(v) => {
                        let _ = player.set_volume(v as f64).await;
                    }
                    MprisCommand::Quit => break,
                }
            }
        });
    });

    Ok(MprisHandle { cmd_tx })
}

impl MprisHandle {
    pub fn update_metadata(&self, track: &Track, art_url: Option<String>) {
        let _ = self.cmd_tx.send(MprisCommand::UpdateMetadata(track.clone(), art_url));
    }

    pub fn update_status(&self, status: PlaybackStatus) {
        let _ = self.cmd_tx.send(MprisCommand::UpdateStatus(status));
    }

    pub fn update_position(&self, pos: Duration) {
        let _ = self.cmd_tx.send(MprisCommand::UpdatePosition(pos));
    }

    pub fn update_volume(&self, v: f32) {
        let _ = self.cmd_tx.send(MprisCommand::UpdateVolume(v));
    }

    pub fn quit(&self) {
        let _ = self.cmd_tx.send(MprisCommand::Quit);
    }
}
