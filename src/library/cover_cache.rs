//! Cover art on disk: reads, atomic writes, pruning and in-flight bookkeeping.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Files kept in the cover cache (and bytes). Matches the lyrics cache bound.
pub const MAX_COVER_FILES: usize = 2_000;
pub const MAX_COVER_BYTES: u64 = 200 * 1024 * 1024;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub removed_files: usize,
    pub removed_bytes: u64,
    pub kept_files: usize,
    pub kept_bytes: u64,
}

/// Owns the covers directory: on-disk reads, atomic writes, pruning, and
/// in-flight bookkeeping so the same cover is never downloaded twice at once.
pub struct CoverCache {
    dir: PathBuf,
    in_flight: Arc<Mutex<HashSet<String>>>,
}

impl CoverCache {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            in_flight: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// The path an existing cover for `cover_id` lives at, checking `.png` then
    /// `.jpg`. Returns None when neither exists.
    pub fn existing_path(&self, cover_id: &str) -> Option<PathBuf> {
        let cover_id = valid_cover_id(cover_id)?;
        for ext in ["png", "jpg"] {
            let path = self.dir.join(format!("{cover_id}.{ext}"));
            if path.is_file() {
                return Some(path);
            }
        }
        None
    }

    /// Read an existing cover. None when absent or empty.
    pub fn read(&self, cover_id: &str) -> Option<Vec<u8>> {
        let cover_id = valid_cover_id(cover_id)?;
        for ext in ["png", "jpg"] {
            let path = self.dir.join(format!("{cover_id}.{ext}"));
            if let Ok(bytes) = fs::read(path)
                && !bytes.is_empty()
            {
                return Some(bytes);
            }
        }
        None
    }

    /// Write bytes atomically (tmp + rename) with the extension that matches the
    /// bytes (PNG magic -> .png, otherwise .jpg). Creates the directory.
    pub fn write(&self, cover_id: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
        let cover_id = valid_cover_id(cover_id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cover id must be a single path component",
            )
        })?;
        let is_png = bytes.starts_with(&[0x89, b'P', b'N', b'G']);
        let (ext, other_ext) = if is_png {
            ("png", "jpg")
        } else {
            ("jpg", "png")
        };
        fs::create_dir_all(&self.dir)?;

        let path = self.dir.join(format!("{cover_id}.{ext}"));
        let tmp = self.temp_path();
        if let Err(error) = fs::write(&tmp, bytes) {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        if let Err(error) = fs::rename(&tmp, &path) {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        // A previous write may have stored this cover with the other format's
        // extension. Leaving it behind would make `existing_path` return stale
        // bytes because it checks `.png` first.
        let _ = fs::remove_file(self.dir.join(format!("{cover_id}.{other_ext}")));
        Ok(path)
    }

    /// Delete the oldest files until both bounds hold. Returns what happened.
    /// A missing directory is not an error. Never deletes a file that is
    /// currently in flight.
    pub fn prune(&self, max_files: usize, max_bytes: u64) -> PruneReport {
        struct Candidate {
            path: PathBuf,
            cover_id: String,
            bytes: u64,
            modified: Option<std::time::SystemTime>,
        }

        let Ok(entries) = fs::read_dir(&self.dir) else {
            return PruneReport::default();
        };
        let mut candidates = Vec::new();
        let mut total_bytes = 0u64;
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_cover_path(&path) {
                continue;
            }
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let Some(cover_id) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned)
            else {
                continue;
            };
            total_bytes = total_bytes.saturating_add(metadata.len());
            candidates.push(Candidate {
                path,
                cover_id,
                bytes: metadata.len(),
                // An unreadable mtime is treated as the oldest possible file.
                modified: metadata.modified().ok(),
            });
        }

        // `None` sorts before every `Some`, so an unreadable mtime is oldest.
        candidates.sort_by(|left, right| {
            left.modified
                .cmp(&right.modified)
                .then_with(|| left.path.cmp(&right.path))
        });

        let mut kept_files = candidates.len();
        let mut kept_bytes = total_bytes;
        let mut report = PruneReport::default();
        let in_flight = self.in_flight();
        for candidate in &candidates {
            if kept_files <= max_files && kept_bytes <= max_bytes {
                break;
            }
            if in_flight.contains(&candidate.cover_id) {
                continue;
            }
            if fs::remove_file(&candidate.path).is_ok() {
                report.removed_files += 1;
                report.removed_bytes = report.removed_bytes.saturating_add(candidate.bytes);
                kept_files -= 1;
                kept_bytes = kept_bytes.saturating_sub(candidate.bytes);
            }
        }
        report.kept_files = kept_files;
        report.kept_bytes = kept_bytes;
        report
    }

    /// Claim the right to fetch `cover_id`. Returns None when another caller
    /// already holds it, so the caller can skip the download.
    /// The claim is released when the returned guard is dropped.
    pub fn begin_fetch(&self, cover_id: &str) -> Option<CoverFetchGuard> {
        let cover_id = valid_cover_id(cover_id)?;
        let mut in_flight = self.in_flight();
        if !in_flight.insert(cover_id.to_owned()) {
            return None;
        }
        Some(CoverFetchGuard {
            in_flight: Arc::clone(&self.in_flight),
            cover_id: cover_id.to_owned(),
        })
    }

    /// How many files are on disk (0 when the directory does not exist).
    pub fn file_count(&self) -> usize {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return 0;
        };
        entries
            .flatten()
            .filter(|entry| entry.metadata().is_ok_and(|meta| meta.is_file()))
            .count()
    }

    /// Total bytes on disk.
    pub fn total_bytes(&self) -> u64 {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return 0;
        };
        entries
            .flatten()
            .filter_map(|entry| entry.metadata().ok())
            .filter(|meta| meta.is_file())
            .map(|meta| meta.len())
            .sum()
    }

    fn temp_path(&self) -> PathBuf {
        static NEXT_TMP: AtomicU64 = AtomicU64::new(0);
        let seq = NEXT_TMP.fetch_add(1, Ordering::Relaxed);
        self.dir
            .join(format!(".cover-{}-{seq}.tmp", std::process::id()))
    }

    fn in_flight(&self) -> MutexGuard<'_, HashSet<String>> {
        match self.in_flight.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// Releases the in-flight claim on drop, including on panic.
pub struct CoverFetchGuard {
    in_flight: Arc<Mutex<HashSet<String>>>,
    cover_id: String,
}

impl Drop for CoverFetchGuard {
    fn drop(&mut self) {
        let mut in_flight = match self.in_flight.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        in_flight.remove(&self.cover_id);
    }
}

/// Cover ids come from the server and must never be able to address a path
/// outside the cache directory.
fn valid_cover_id(cover_id: &str) -> Option<&str> {
    if cover_id.is_empty()
        || cover_id == "."
        || cover_id == ".."
        || cover_id.contains('/')
        || cover_id.contains('\\')
        || cover_id.contains('\0')
    {
        None
    } else {
        Some(cover_id)
    }
}

/// Only cover images take part in pruning.
fn is_cover_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("png" | "jpg")
    )
}
