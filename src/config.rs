use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StreamQuality {
    #[default]
    Original,
    Kbps320,
    Kbps192,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub server_url: String,
    pub username: String,
    pub quality: StreamQuality,
    pub scrobble: bool,
    pub volume: f32,
    pub lyrics_offset_ms: i64,
    pub prefer_navidrome_lyrics: bool,
    pub overlay_x: i32,
    pub overlay_y: i32,
    pub overlay_width: u32,
    pub overlay_height: u32,
    pub overlay_opacity: f32,
    pub reduce_motion: bool,
    pub follow_os_accent: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            username: String::new(),
            quality: StreamQuality::Original,
            scrobble: true,
            volume: 0.8,
            lyrics_offset_ms: 0,
            prefer_navidrome_lyrics: true,
            overlay_x: 0,
            overlay_y: 0,
            overlay_width: 392,
            overlay_height: 180,
            overlay_opacity: 0.74,
            reduce_motion: false,
            follow_os_accent: true,
        }
    }
}

#[derive(Clone)]
pub struct Paths {
    pub config: PathBuf,
    pub cache: PathBuf,
    pub covers: PathBuf,
    pub lyrics: PathBuf,
    pub logs: PathBuf,
}

impl Paths {
    pub fn new() -> Result<Self> {
        let dirs = ProjectDirs::from("io.github", "samvivan1", "Myuzik")
            .ok_or_else(|| anyhow::anyhow!("could not determine config/cache directories"))?;
        Ok(Self {
            config: dirs.config_dir().join("config.toml"),
            cache: dirs.cache_dir().to_path_buf(),
            covers: dirs.cache_dir().join("covers"),
            lyrics: dirs.cache_dir().join("lyrics"),
            logs: dirs.cache_dir().join("logs"),
        })
    }

    pub fn ensure(&self) -> Result<()> {
        for p in [&self.cache, &self.covers, &self.lyrics, &self.logs] {
            fs::create_dir_all(p)?;
        }
        if let Some(parent) = self.config.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }
}

pub fn credentials_path(paths: &Paths) -> PathBuf {
    paths
        .config
        .parent()
        .unwrap_or(&paths.cache)
        .join("credentials")
}

pub fn load_password(paths: &Paths) -> Option<String> {
    let p = credentials_path(paths);
    if !p.exists() {
        return None;
    }
    std::fs::read_to_string(&p)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn save_password(paths: &Paths, password: &str) -> anyhow::Result<()> {
    paths.ensure()?;
    let p = credentials_path(paths);
    std::fs::write(&p, password)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&p)?.permissions();
        perm.set_mode(0o600);
        std::fs::set_permissions(&p, perm)?;
    }
    Ok(())
}

pub fn delete_password(paths: &Paths) {
    let _ = std::fs::remove_file(credentials_path(paths));
}

pub fn load_config(paths: &Paths) -> Result<AppConfig> {
    if !paths.config.exists() {
        return Ok(AppConfig::default());
    }
    let text = fs::read_to_string(&paths.config)?;
    Ok(toml::from_str(&text)?)
}

pub fn save_config(paths: &Paths, cfg: &AppConfig) -> Result<()> {
    paths.ensure()?;
    let text = toml::to_string_pretty(cfg)?;
    let tmp = paths.config.with_extension("toml.tmp");
    fs::write(&tmp, text)?;
    // Best-effort atomic rename; on Unix rename is atomic.
    fs::rename(&tmp, &paths.config)?;
    Ok(())
}

pub fn config_exists(paths: &Paths) -> bool {
    paths.config.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let c = AppConfig::default();
        assert_eq!(c.quality, StreamQuality::Original);
        assert_eq!(c.lyrics_offset_ms, 0);
        assert!(c.follow_os_accent);
        assert!(c.scrobble);
    }

    #[test]
    fn toml_roundtrip() {
        let c = AppConfig::default();
        let text = toml::to_string_pretty(&c).unwrap();
        let back: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(back, c);
    }
}
