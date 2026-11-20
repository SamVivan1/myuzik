//! On-disk snapshot of the whole library.
//!
//! Myuzik keeps the catalogue in one JSON file and searches it in memory, so a
//! restart can show the library before the server answers. This module is the
//! disk half of that: it reads and writes [`LibrarySnapshot`], and owns the
//! rules that keep a broken file from breaking startup.
//!
//! Two properties matter here:
//!
//! - **A write is atomic.** The snapshot is written to `library.json.tmp`, is
//!   fsynced, and only then renamed over `library.json`. A crash therefore
//!   leaves either the old snapshot or the new one, never a truncated file.
//! - **A read never panics and never deletes.** Anything that is not a
//!   snapshot this build understands (bad JSON, bad UTF-8, a version from
//!   another release) is an `Err` with a reason; the caller refreshes from the
//!   server and overwrites it, and the file stays on disk until it does.

use crate::library::{Album, Artist, Playlist, Track};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Bumped whenever the shape of [`LibrarySnapshot`] or the DTOs it holds
/// changes. A file written by another version is rejected, not guessed at.
pub const SNAPSHOT_VERSION: u32 = 1;

/// File name of the snapshot inside the cache directory.
const SNAPSHOT_FILE: &str = "library.json";

/// Suffix of the temporary file used to make a save atomic.
const TMP_SUFFIX: &str = ".tmp";

/// The whole library as it was when the server last answered.
///
/// `fetched_at` is what makes the file useful offline *and* refreshable: the
/// UI can show a stale snapshot immediately and still know when to refetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibrarySnapshot {
    pub version: u32,
    /// Unix seconds when the snapshot was taken.
    pub fetched_at: u64,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
    pub songs: Vec<Track>,
}

impl LibrarySnapshot {
    /// An empty snapshot stamped with the current version.
    pub fn empty() -> Self {
        Self {
            version: SNAPSHOT_VERSION,
            fetched_at: 0,
            albums: Vec::new(),
            artists: Vec::new(),
            playlists: Vec::new(),
            songs: Vec::new(),
        }
    }

    /// True when there is nothing at all to show from this snapshot.
    pub fn is_empty(&self) -> bool {
        self.albums.is_empty()
            && self.artists.is_empty()
            && self.playlists.is_empty()
            && self.songs.is_empty()
    }

    pub fn song_count(&self) -> usize {
        self.songs.len()
    }

    /// Age in seconds relative to `now` (unix seconds); saturates at 0.
    ///
    /// Saturating rather than wrapping keeps a clock that moved backwards from
    /// reporting a snapshot as decades old.
    pub fn age_seconds(&self, now: u64) -> u64 {
        now.saturating_sub(self.fetched_at)
    }

    /// Whether the snapshot is at least `max_age` old, i.e. due for a refetch.
    ///
    /// The comparison is done on the exact age, so a sub-second `max_age` is
    /// not silently truncated to zero.
    pub fn is_stale(&self, now: u64, max_age: Duration) -> bool {
        Duration::from_secs(self.age_seconds(now)) >= max_age
    }
}

/// Reads and writes `library.json` inside one directory.
#[derive(Debug, Clone)]
pub struct LibraryCache {
    /// Directory holding `library.json`.
    dir: PathBuf,
}

impl LibraryCache {
    /// `dir` is where `library.json` lives; it is created by the first save.
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The snapshot file itself.
    pub fn path(&self) -> PathBuf {
        self.dir.join(SNAPSHOT_FILE)
    }

    /// The temporary file a save goes through before the rename.
    fn tmp_path(&self) -> PathBuf {
        self.dir.join(format!("{SNAPSHOT_FILE}{TMP_SUFFIX}"))
    }

    /// Read the snapshot. Returns `Ok(None)` when there is no file yet.
    ///
    /// An unreadable file, something that is not JSON, or a snapshot written
    /// by a different [`SNAPSHOT_VERSION`] is an error: the caller then
    /// refreshes from the server. The rejected file is left where it is.
    pub fn load(&self) -> Result<Option<LibrarySnapshot>> {
        let path = self.path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            // No cache yet is the normal first-start case, not a failure.
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(anyhow::Error::new(error)
                    .context(format!("reading library cache {}", path.display())));
            }
        };

        let snapshot: LibrarySnapshot = serde_json::from_slice(&bytes)
            .with_context(|| format!("library cache {} is not a valid snapshot", path.display()))?;

        // The app's DTOs may have changed since this file was written; the
        // JSON can still deserialize while meaning something else entirely.
        if snapshot.version != SNAPSHOT_VERSION {
            bail!(
                "library cache {} was written by snapshot version {} but this build reads version {}",
                path.display(),
                snapshot.version,
                SNAPSHOT_VERSION
            );
        }

        Ok(Some(snapshot))
    }

    /// Write the snapshot atomically, creating the directory if needed.
    ///
    /// The bytes go to `<file>.tmp`, are fsynced, and the temp file is renamed
    /// over the real one, so an interrupted write can never leave a
    /// half-written cache behind. The directory is fsynced too, so the rename
    /// itself survives a power loss where the filesystem supports it.
    pub fn save(&self, snapshot: &LibrarySnapshot) -> Result<()> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating library cache directory {}", self.dir.display()))?;

        let bytes = serde_json::to_vec(snapshot).context("encoding the library snapshot")?;
        let path = self.path();
        let tmp = self.tmp_path();

        let written = self.write_tmp_and_replace(&tmp, &path, &bytes);
        if written.is_err() {
            // Best effort: a failed save must not leave its scratch file
            // behind for the next start to misread.
            let _ = fs::remove_file(&tmp);
        }
        written
    }

    fn write_tmp_and_replace(&self, tmp: &Path, path: &Path, bytes: &[u8]) -> Result<()> {
        {
            let mut file =
                File::create(tmp).with_context(|| format!("creating {}", tmp.display()))?;
            file.write_all(bytes)
                .with_context(|| format!("writing {}", tmp.display()))?;
            // Flush to the disk before the rename, so the renamed file cannot
            // be empty or short after a crash.
            file.sync_all()
                .with_context(|| format!("flushing {}", tmp.display()))?;
        }

        fs::rename(tmp, path).with_context(|| format!("replacing {}", path.display()))?;

        // The rename is a directory change; syncing the directory makes it
        // durable. Best effort: not every platform allows opening a directory.
        if let Ok(dir) = File::open(&self.dir) {
            let _ = dir.sync_all();
        }

        Ok(())
    }

    /// Delete the snapshot (and any leftover temporary file) — "clear cache".
    ///
    /// A missing file is not an error, so this is safe to call twice.
    pub fn clear(&self) -> Result<()> {
        let path = self.path();
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(anyhow::Error::new(error)
                    .context(format!("removing library cache {}", path.display())));
            }
        }

        // A temp file left by a crashed save is part of the cache too.
        let _ = fs::remove_file(self.tmp_path());
        Ok(())
    }
}
