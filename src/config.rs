use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StreamQuality {
    #[default]
    Original,
    Kbps320,
    Kbps192,
}

/// Current schema version of `config.toml`.
///
/// INT-08: the file had no version at all, so a renamed or removed key simply
/// disappeared. Version 1 is the first versioned schema; a file written before
/// it has no `version` key and deserializes as 0 (see [`unversioned`]), which
/// [`migrate_config`] brings up to date.
pub const CONFIG_VERSION: u32 = 1;

/// The version of a file written before the schema carried one.
fn unversioned() -> u32 {
    0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Schema version of the file this came from; see [`CONFIG_VERSION`].
    ///
    /// A missing key means "written before versioning" (0), not "current": the
    /// container-level `#[serde(default)]` would otherwise fill it with
    /// [`AppConfig::default`]'s version and the migration could not tell.
    #[serde(default = "unversioned")]
    pub version: u32,
    pub server_url: String,
    pub username: String,
    pub quality: StreamQuality,
    pub scrobble: bool,
    pub volume: f32,
    /// Keep playing similar tracks when the queue runs out (autoplay radio).
    pub autoplay: bool,
    /// Closing the main window hides it to the tray instead of quitting.
    pub close_to_tray: bool,
    pub lyrics_offset_ms: i64,
    pub prefer_navidrome_lyrics: bool,
    pub overlay_x: i32,
    pub overlay_y: i32,
    pub overlay_width: u32,
    pub overlay_height: u32,
    pub overlay_opacity: f32,
    pub reduce_motion: bool,
    pub follow_os_accent: bool,
    /// Keys this build does not know: a typo, or a key a newer build wrote.
    ///
    /// INT-08b: `#[serde(default)]` made an unknown key vanish without a trace.
    /// Collecting them means the loader can name them, and a later save writes
    /// them back instead of destroying what it did not understand.
    #[serde(flatten)]
    pub unknown: std::collections::BTreeMap<String, toml::Value>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            server_url: String::new(),
            username: String::new(),
            quality: StreamQuality::Original,
            scrobble: true,
            volume: 0.8,
            autoplay: true,
            close_to_tray: true,
            lyrics_offset_ms: 0,
            prefer_navidrome_lyrics: true,
            overlay_x: 0,
            overlay_y: 0,
            overlay_width: 620,
            overlay_height: 330,
            overlay_opacity: 0.74,
            reduce_motion: false,
            follow_os_accent: true,
            unknown: std::collections::BTreeMap::new(),
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

/// The service and account the password is filed under in the Secret Service.
/// One Navidrome account per install; the username itself lives in `config.toml`.
const KEYRING_SERVICE: &str = "io.github.samvivan1.Myuzik";
const KEYRING_ACCOUNT: &str = "navidrome";

/// Where the password ended up.
///
/// The setup form tells the user which of the two it was: docs used to promise a
/// Secret Service while the code always wrote plaintext (audit INT-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialStore {
    /// The desktop's Secret Service (`org.freedesktop.secrets`: GNOME Keyring,
    /// KWallet, ...) holds the password.
    SecretService,
    /// No Secret Service answered, so the password sits in a `0600` file next to
    /// `config.toml`.
    File,
}

impl CredentialStore {
    /// A sentence for the setup form saying where the password was kept.
    pub fn describe(self) -> &'static str {
        match self {
            CredentialStore::SecretService => "Your password is stored in the system keyring.",
            CredentialStore::File => {
                "Your password is stored in a private file: no system keyring was available."
            }
        }
    }
}

/// A Secret Service the password can be kept in.
///
/// The trait lets the fallback and the "which store was used" report be tested
/// without a session bus; the production implementation is [`KeyringBackend`].
pub trait SecretServiceBackend: Send + Sync {
    fn set(&self, password: &str) -> anyhow::Result<()>;
    fn get(&self) -> anyhow::Result<Option<String>>;
    fn delete(&self) -> anyhow::Result<()>;
}

/// How long a Secret Service call may take before the caller gives up on it.
const KEYRING_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1500);

/// Run a blocking Secret Service call with a deadline.
///
/// `keyring` talks D-Bus synchronously and offers no timeout, so a hung keyring
/// daemon would block whoever called it — and the two callers here are both on
/// the UI thread: reading the password during startup, and saving it when the
/// setup form connects. The call runs on its own thread and we wait at most
/// [`KEYRING_TIMEOUT`]; past that the caller falls back to the private file.
fn bounded_secret_service_call<T, F>(timeout: std::time::Duration, work: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("myuzik-keyring".to_string())
        .spawn(move || {
            let _ = sender.send(work());
        })
        .ok()?;
    receiver.recv_timeout(timeout).ok()
}

fn timed_out() -> anyhow::Error {
    anyhow::anyhow!("the secret service did not answer within {KEYRING_TIMEOUT:?}")
}

/// The real backend: `keyring`'s Secret Service store.
struct KeyringBackend;

impl KeyringBackend {
    fn entry() -> anyhow::Result<keyring::Entry> {
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
            .map_err(|error| anyhow::anyhow!("keyring entry unavailable: {error}"))
    }

    fn set_blocking(password: &str) -> anyhow::Result<()> {
        Self::entry()?
            .set_password(password)
            .map_err(|error| anyhow::anyhow!("keyring refused the password: {error}"))
    }

    fn get_blocking() -> anyhow::Result<Option<String>> {
        match Self::entry()?.get_password() {
            Ok(password) => Ok(Some(password)),
            // No entry is not a failure: there is simply no password yet.
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(anyhow::anyhow!("keyring could not be read: {error}")),
        }
    }

    fn delete_blocking() -> anyhow::Result<()> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(anyhow::anyhow!(
                "keyring entry could not be deleted: {error}"
            )),
        }
    }
}

impl SecretServiceBackend for KeyringBackend {
    fn set(&self, password: &str) -> anyhow::Result<()> {
        let password = password.to_string();
        match bounded_secret_service_call(KEYRING_TIMEOUT, move || Self::set_blocking(&password)) {
            Some(result) => result,
            None => Err(timed_out()),
        }
    }

    fn get(&self) -> anyhow::Result<Option<String>> {
        match bounded_secret_service_call(KEYRING_TIMEOUT, Self::get_blocking) {
            Some(result) => result,
            None => Err(timed_out()),
        }
    }

    fn delete(&self) -> anyhow::Result<()> {
        match bounded_secret_service_call(KEYRING_TIMEOUT, Self::delete_blocking) {
            Some(result) => result,
            None => Err(timed_out()),
        }
    }
}

/// Whether the Secret Service may be used, from `MYUZIK_KEYRING`.
///
/// On by default: a password belongs in the keyring, and docs promised it for a
/// whole release while the code only ever wrote plaintext (audit INT-01). A
/// desktop without a keyring simply fails the call and falls back to the `0600`
/// file, which costs at most [`KEYRING_TIMEOUT`] once per read.
///
/// `MYUZIK_KEYRING=0` (`false`, `no`, `off`) forces the file-only path, for a
/// desktop with no keyring daemon at all or for testing.
pub fn secret_service_enabled(value: Option<&str>) -> bool {
    !matches!(
        value
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("0" | "false" | "no" | "off")
    )
}

const STORE_UNSET: u8 = 0;
const STORE_FILE: u8 = 1;
const STORE_KEYRING: u8 = 2;

/// The store decision for this process: unset, file-only, or the keyring.
static STORE_DECISION: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(STORE_UNSET);

/// Pin this process to the `0600` file, ignoring the environment.
///
/// Tests that drive the real setup flow must not write to the developer's own
/// keyring, and `std::env::set_var` is unsafe once other threads exist. The
/// override wins over an earlier environment decision, so it does not matter
/// which test ran first.
#[doc(hidden)]
pub fn use_file_store_only_for_tests() {
    STORE_DECISION.store(STORE_FILE, std::sync::atomic::Ordering::Relaxed);
}

fn default_backend() -> Option<KeyringBackend> {
    use std::sync::atomic::Ordering;
    if STORE_DECISION.load(Ordering::Relaxed) == STORE_UNSET {
        let configured = std::env::var("MYUZIK_KEYRING").ok();
        let decided = if secret_service_enabled(configured.as_deref()) {
            STORE_KEYRING
        } else {
            STORE_FILE
        };
        let _ = STORE_DECISION.compare_exchange(
            STORE_UNSET,
            decided,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }
    (STORE_DECISION.load(Ordering::Relaxed) == STORE_KEYRING).then_some(KeyringBackend)
}

/// Open `path` for writing with mode `0600` set *at creation*.
///
/// INT-09: the old code created the file with the umask (0644 on most systems)
/// and `chmod`ed it afterwards, so the password was briefly world-readable. Here
/// the mode is part of `open(2)`, and the permissions are also enforced before a
/// single byte is written, so a file left over from an older version is tightened
/// before it is rewritten.
pub fn create_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn save_password_to_file(path: &Path, password: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = create_private_file(path)?;
    file.write_all(password.as_bytes())?;
    file.flush()?;
    Ok(())
}

fn load_password_from_file(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    fs::read_to_string(path)
        .ok()
        .map(|password| password.trim().to_string())
        .filter(|password| !password.is_empty())
}

/// Save the password, preferring the Secret Service and reporting which store
/// was used.
///
/// A Secret Service failure is not fatal: the password is written to the private
/// file instead and [`CredentialStore::File`] is returned so the user can be told.
pub fn save_password_with(
    paths: &Paths,
    password: &str,
    backend: Option<&dyn SecretServiceBackend>,
) -> anyhow::Result<CredentialStore> {
    paths.ensure()?;
    let path = credentials_path(paths);
    if let Some(backend) = backend {
        match backend.set(password) {
            Ok(()) => {
                // A plaintext copy from an earlier fallback would defeat the
                // point of keeping the password in the keyring.
                match fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => {
                        tracing::warn!(%error, "could not remove the old plaintext password file");
                    }
                }
                return Ok(CredentialStore::SecretService);
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    "keyring unavailable, keeping the password in a private file"
                );
            }
        }
    }
    save_password_to_file(&path, password)?;
    Ok(CredentialStore::File)
}

/// Read the password from the Secret Service, falling back to the private file.
pub fn load_password_with(
    paths: &Paths,
    backend: Option<&dyn SecretServiceBackend>,
) -> Option<String> {
    if let Some(backend) = backend {
        match backend.get() {
            Ok(Some(password)) if !password.trim().is_empty() => {
                return Some(password.trim().to_string());
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "could not read the password from the keyring"),
        }
    }
    load_password_from_file(&credentials_path(paths))
}

/// Remove the password from both stores. A password that is gone is not an error.
pub fn delete_password_with(
    paths: &Paths,
    backend: Option<&dyn SecretServiceBackend>,
) -> anyhow::Result<()> {
    match fs::remove_file(credentials_path(paths)) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    if let Some(backend) = backend {
        // Reported, not swallowed: a keyring copy that survives "disconnect"
        // would keep working after the user thought they had signed out.
        backend.delete()?;
    }
    Ok(())
}

pub fn load_password(paths: &Paths) -> Option<String> {
    match default_backend() {
        Some(backend) => load_password_with(paths, Some(&backend)),
        None => load_password_with(paths, None),
    }
}

pub fn save_password(paths: &Paths, password: &str) -> anyhow::Result<CredentialStore> {
    match default_backend() {
        Some(backend) => save_password_with(paths, password, Some(&backend)),
        None => save_password_with(paths, password, None),
    }
}

pub fn delete_password(paths: &Paths) -> anyhow::Result<()> {
    match default_backend() {
        Some(backend) => delete_password_with(paths, Some(&backend)),
        None => delete_password_with(paths, None),
    }
}

/// A `config.toml` that could not be used, and where it went.
///
/// INT-08: the old loader returned defaults and the first settings toggle wrote
/// them over the user's file, so a single bad byte destroyed every setting.
/// The original is moved aside instead, and this is the record of that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigRecovery {
    /// Where the unreadable file was moved (`None` when it could not be moved,
    /// e.g. a read-only config directory; the app still starts on defaults).
    pub backup: Option<PathBuf>,
    /// Why the file could not be used, for the log and the user.
    pub reason: String,
}

impl ConfigRecovery {
    /// One sentence for the user, naming the file that was kept.
    pub fn describe(&self) -> String {
        match &self.backup {
            Some(path) => format!(
                "Your settings could not be read ({}), so Myuzik started with defaults. \
                 The old file was kept as {}.",
                self.reason,
                path.display()
            ),
            None => format!(
                "Your settings could not be read ({}) and could not be moved aside, \
                 so Myuzik started with defaults.",
                self.reason
            ),
        }
    }
}

/// What [`load_config_recovering`] found.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadOutcome {
    /// The config to run with: the file's, or [`AppConfig::default`].
    pub config: AppConfig,
    /// Set when the file could not be used and had to be recovered from.
    pub recovery: Option<ConfigRecovery>,
    /// Top-level keys this build does not know, sorted by name (INT-08b).
    pub unknown_keys: Vec<String>,
}

impl LoadOutcome {
    fn healthy(config: AppConfig) -> Self {
        Self {
            config,
            recovery: None,
            unknown_keys: Vec::new(),
        }
    }

    /// Whether the file had to be recovered from (the user must be told).
    pub fn is_recovered(&self) -> bool {
        self.recovery.is_some()
    }

    /// A sentence for the user when the file had to be recovered from.
    pub fn notice(&self) -> Option<String> {
        self.recovery.as_ref().map(|recovery| recovery.describe())
    }
}

/// Load `config.toml`, recovering from a file that cannot be read.
///
/// Never fails: a missing, truncated, non-UTF-8 or unparseable file yields
/// [`AppConfig::default`] so the app starts, and the reason plus where the
/// original went is returned in the [`LoadOutcome`] so the caller can tell the
/// user. See audit INT-08.
pub fn load_config_recovering(paths: &Paths) -> LoadOutcome {
    let text = match fs::read_to_string(&paths.config) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return LoadOutcome::healthy(AppConfig::default());
        }
        Err(error) => return recover_config(paths, format!("could not be read: {error}")),
    };
    match toml::from_str::<AppConfig>(&text) {
        Ok(mut config) => {
            let from = config.version;
            migrate_config(&mut config, from);
            let unknown_keys = config.unknown.keys().cloned().collect::<Vec<_>>();
            if !unknown_keys.is_empty() {
                // INT-08b: an invisible typo used to change nothing and say
                // nothing. Named here, and carried in the outcome for the UI.
                tracing::warn!(
                    path = %paths.config.display(),
                    keys = %unknown_keys.join(", "),
                    "config has keys this version does not know, so they have no effect"
                );
            }
            LoadOutcome {
                config,
                recovery: None,
                unknown_keys,
            }
        }
        Err(error) => recover_config(paths, format!("could not be parsed: {error}")),
    }
}

/// Bring a config written by an older schema up to [`CONFIG_VERSION`].
///
/// Version 1 is the first versioned schema, so there is nothing to rewrite yet.
/// The function exists — and is called on every load — so that the next renamed
/// or removed key has an obvious home here instead of disappearing silently.
fn migrate_config(config: &mut AppConfig, from: u32) {
    match from.cmp(&CONFIG_VERSION) {
        std::cmp::Ordering::Less => config.version = CONFIG_VERSION,
        // A file from a newer build: keep its version. Rewriting it as an older
        // version would make the newer build migrate a file it already migrated.
        std::cmp::Ordering::Greater => {
            tracing::warn!(
                from,
                current = CONFIG_VERSION,
                "config was written by a newer version of Myuzik"
            );
        }
        std::cmp::Ordering::Equal => {}
    }
}

/// Keep an unusable `config.toml` and run on defaults.
///
/// The file is moved, never deleted: the user's settings are often the only
/// copy. If it cannot be moved (read-only directory, or already gone) the app
/// still starts and the outcome says the file could not be kept.
fn recover_config(paths: &Paths, reason: String) -> LoadOutcome {
    tracing::error!(
        path = %paths.config.display(),
        reason = %reason,
        "config unusable, starting on defaults"
    );
    let backup = match move_config_aside(&paths.config) {
        Ok(backup) => backup,
        Err(error) => {
            tracing::error!(
                path = %paths.config.display(),
                %error,
                "could not move the unusable config aside"
            );
            None
        }
    };
    if let Some(backup) = &backup {
        tracing::error!(kept_as = %backup.display(), "the unusable config was kept");
    }
    LoadOutcome {
        config: AppConfig::default(),
        recovery: Some(ConfigRecovery { backup, reason }),
        unknown_keys: Vec::new(),
    }
}

/// Move `path` to `path.corrupt-<unix seconds>`, never overwriting an earlier
/// backup (a second broken file in the same second gets a `-1` suffix).
///
/// `Ok(None)` means there was nothing to keep any more.
fn move_config_aside(path: &Path) -> io::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let backup = available_backup_path(path, unix_now());
    fs::rename(path, &backup)?;
    Ok(Some(backup))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn available_backup_path(path: &Path, timestamp: u64) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.toml".to_string());
    let first = path.with_file_name(format!("{name}.corrupt-{timestamp}"));
    if !first.exists() {
        return first;
    }
    for suffix in 1u32.. {
        let candidate = path.with_file_name(format!("{name}.corrupt-{timestamp}-{suffix}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("the suffix loop always returns")
}

/// Load `config.toml`, treating a file that cannot be used as an error.
///
/// Kept for callers that want the `Result` (and for tests that skip when there
/// is no config); the recovery still happens, so the user's bytes are never
/// lost, and the error says where they are.
pub fn load_config(paths: &Paths) -> Result<AppConfig> {
    let outcome = load_config_recovering(paths);
    match outcome.recovery {
        Some(recovery) => Err(anyhow::anyhow!("{}", recovery.describe())),
        None => Ok(outcome.config),
    }
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

    /// A hung Secret Service must not hold the caller: the call is abandoned
    /// after the deadline and the caller falls back to the private file.
    #[test]
    fn a_slow_secret_service_call_is_abandoned() {
        let started = std::time::Instant::now();
        let result: Option<u8> =
            bounded_secret_service_call(std::time::Duration::from_millis(30), || {
                std::thread::sleep(std::time::Duration::from_millis(400));
                7
            });
        assert_eq!(result, None, "a call past the deadline must be given up on");
        assert!(
            started.elapsed() < std::time::Duration::from_millis(300),
            "the caller waited for the slow call instead of the deadline"
        );
    }

    /// A fast call still returns its value.
    #[test]
    fn a_prompt_secret_service_call_returns_its_value() {
        let result = bounded_secret_service_call(std::time::Duration::from_secs(2), || {
            Some("from-the-keyring".to_string())
        });
        assert_eq!(result, Some(Some("from-the-keyring".to_string())));
    }

    #[test]
    fn toml_roundtrip() {
        let c = AppConfig::default();
        let text = toml::to_string_pretty(&c).unwrap();
        let back: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(back, c);
    }
}
