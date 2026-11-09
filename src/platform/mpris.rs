use crate::library::Track;
use crate::player::engine::PlayerCommand;
use crate::player::state::PlaybackStatus;
use flume::Sender;
use mpris_server::Player;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum MprisCommand {
    UpdateMetadata(Box<Track>, Option<String>),
    UpdateStatus(PlaybackStatus),
    UpdatePosition(Duration),
    UpdateVolume(f32),
    Quit,
}

#[derive(Clone)]
pub struct MprisHandle {
    pub cmd_tx: Sender<MprisCommand>,
}

pub fn start_mpris(player_tx: Sender<PlayerCommand>) -> std::result::Result<MprisHandle, String> {
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
            let player = match Player::builder("io.github.samvivan1.Myuzik")
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

            // Handle control callbacks — forward to player thread
            let player_tx_clone = player_tx.clone();
            player.connect_play_pause(move |_p| {
                let _ = player_tx_clone.send(PlayerCommand::Toggle);
            });
            let player_tx_clone = player_tx.clone();
            player.connect_next(move |_p| {
                let _ = player_tx_clone.send(PlayerCommand::Stop); // placeholder: next
            });
            let player_tx_clone = player_tx.clone();
            player.connect_previous(move |_p| {
                let _ = player_tx_clone.send(PlayerCommand::Stop); // placeholder: previous
            });
            let player_tx_clone = player_tx.clone();
            player.connect_seek(move |_p, time| {
                let pos = Duration::from_micros(time.as_micros() as u64);
                let _ = player_tx_clone.send(PlayerCommand::Seek(pos));
            });
            let player_tx_clone = player_tx.clone();
            player.connect_set_position(move |_p, _track_id, time| {
                let pos = Duration::from_micros(time.as_micros() as u64);
                let _ = player_tx_clone.send(PlayerCommand::Seek(pos));
            });

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
                        meta.set_length(Some(mpris_server::Time::from_micros(
                            track.duration.as_micros() as i64,
                        )));
                        if let Some(url) = art_url {
                            meta.set_art_url(Some(url));
                        }
                        if let Ok(tid) = mpris_server::TrackId::try_from(format!(
                            "/org/mpris/MediaPlayer2/Track/{}",
                            track.id
                        )) {
                            meta.set_trackid(Some(tid));
                        }
                        if let Some(n) = track.track_number {
                            meta.set_track_number(Some(n as i32));
                        }
                        if let Some(n) = track.disc_number {
                            meta.set_disc_number(Some(n as i32));
                        }
                        let _ = player.set_metadata(meta).await;
                        let _ = player.set_can_go_next(true).await;
                        let _ = player.set_can_go_previous(true).await;
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
        let _ = self.cmd_tx.send(MprisCommand::UpdateMetadata(Box::new(track.clone()), art_url));
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
