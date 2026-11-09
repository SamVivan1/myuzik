use crate::library::Track;
use crate::lyrics::lrc::parse_lrc;
use crate::lyrics::model::LyricsStatus;
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    instrumental: bool,
    synced: String,
    plain: String,
}

pub struct LyricsCache {
    dir: PathBuf,
}

impl LyricsCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)
    }

    pub fn key(track: &Track) -> String {
        let mut hasher = Sha256::new();
        hasher.update(track.artist.trim().to_lowercase().as_bytes());
        hasher.update(b"|");
        hasher.update(track.title.trim().to_lowercase().as_bytes());
        hasher.update(b"|");
        hasher.update(track.album.trim().to_lowercase().as_bytes());
        hasher.update(b"|");
        hasher.update((track.duration.as_secs()).to_string().as_bytes());
        hex::encode(hasher.finalize())
    }

    pub fn read(&self, key: &str) -> Option<LyricsStatus> {
        let path = self.dir.join(format!("{}.json", key));
        let text = fs::read_to_string(path).ok()?;
        let entry: CacheEntry = serde_json::from_str(&text).ok()?;
        if entry.instrumental {
            return Some(LyricsStatus::Instrumental);
        }
        let synced = parse_lrc(&entry.synced);
        if !synced.is_empty() {
            return Some(LyricsStatus::Synced(synced));
        }
        if !entry.plain.trim().is_empty() {
            return Some(LyricsStatus::Plain(entry.plain));
        }
        Some(LyricsStatus::NotFound)
    }

    pub fn write_raw(
        &self,
        key: &str,
        instrumental: bool,
        synced_text: &str,
        plain_text: &str,
    ) -> std::io::Result<()> {
        let _ = self.ensure();
        let path = self.dir.join(format!("{}.json", key));
        let entry = CacheEntry {
            instrumental,
            synced: synced_text.to_string(),
            plain: plain_text.to_string(),
        };
        let text = serde_json::to_string(&entry)
            .map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn prune(&self, max_files: usize, max_bytes: u64) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(std::path::PathBuf, u64, std::time::SystemTime)> = Vec::new();
        let mut total = 0u64;
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let meta = match fs::metadata(&p) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            files.push((p, meta.len(), mtime));
            total += meta.len();
        }
        if files.len() <= max_files && total <= max_bytes {
            return;
        }
        files.sort_by_key(|f| f.2);
        let mut i = 0;
        while (files.len() - i > max_files) || (total > max_bytes && i < files.len()) {
            let (p, len, _) = &files[i];
            let _ = fs::remove_file(p);
            total = total.saturating_sub(*len);
            i += 1;
            if i >= files.len() {
                break;
            }
        }
    }
}
