//! Credential storage (audit INT-01 / INT-09).
//!
//! Two properties are locked here:
//!
//! 1. The plaintext fallback file is private *by construction*: it is created
//!    with mode `0600`, never written world-readable and then `chmod`ed.
//! 2. The Secret Service is used when it is available and the caller is told
//!    which store was used; when it is not available the file fallback takes
//!    over and the password still survives a restart.
//!
//! The Secret Service itself needs a session D-Bus, which the test environment
//! does not have, so the backend is stubbed through [`SecretServiceBackend`].
//! The real `keyring` backend is exercised only when `MYUZIK_KEYRING=1` is set
//! and a secret service is actually running.

use myuzik::config::{
    CredentialStore, Paths, SecretServiceBackend, create_private_file, credentials_path,
    delete_password_with, load_password, load_password_with, save_password, save_password_with,
    secret_service_enabled,
};
use std::sync::Mutex;

/// A `SecretServiceBackend` that keeps the password in memory.
#[derive(Default)]
struct StubBackend {
    stored: Mutex<Option<String>>,
    fail: bool,
}

impl StubBackend {
    fn working() -> Self {
        Self::default()
    }

    fn broken() -> Self {
        Self {
            stored: Mutex::new(None),
            fail: true,
        }
    }
}

impl SecretServiceBackend for StubBackend {
    fn set(&self, password: &str) -> anyhow::Result<()> {
        if self.fail {
            anyhow::bail!("no secret service on this bus");
        }
        *self.stored.lock().expect("stub lock") = Some(password.to_owned());
        Ok(())
    }

    fn get(&self) -> anyhow::Result<Option<String>> {
        if self.fail {
            anyhow::bail!("no secret service on this bus");
        }
        Ok(self.stored.lock().expect("stub lock").clone())
    }

    fn delete(&self) -> anyhow::Result<()> {
        if self.fail {
            anyhow::bail!("no secret service on this bus");
        }
        self.stored.lock().expect("stub lock").take();
        Ok(())
    }
}

fn temp_paths(tag: &str) -> Paths {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "myuzik-credentials-{tag}-{}-{nanos}",
        std::process::id()
    ));
    Paths {
        config: base.join("config.toml"),
        cache: base.join("cache"),
        covers: base.join("covers"),
        lyrics: base.join("lyrics"),
        logs: base.join("logs"),
    }
}

fn cleanup(paths: &Paths) {
    if let Some(parent) = paths.config.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }
}

#[cfg(unix)]
fn mode_of(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777
}

#[test]
fn save_uses_the_secret_service_when_it_works() {
    let paths = temp_paths("secret-service");
    let backend = StubBackend::working();

    let store = save_password_with(&paths, "hunter2", Some(&backend)).expect("save");

    assert_eq!(store, CredentialStore::SecretService);
    assert!(
        !credentials_path(&paths).exists(),
        "the password must not also be written to disk in plaintext"
    );
    assert_eq!(
        load_password_with(&paths, Some(&backend)).as_deref(),
        Some("hunter2")
    );
    cleanup(&paths);
}

#[test]
fn save_falls_back_to_the_file_when_the_secret_service_is_missing() {
    let paths = temp_paths("fallback");
    let backend = StubBackend::broken();

    let store = save_password_with(&paths, "hunter2", Some(&backend)).expect("save");

    assert_eq!(store, CredentialStore::File);
    assert!(
        credentials_path(&paths).exists(),
        "the fallback file is missing"
    );
    let on_disk = std::fs::read_to_string(credentials_path(&paths)).expect("read");
    assert_eq!(on_disk, "hunter2");
    // Loading through the same (broken) backend must still find the password.
    assert_eq!(
        load_password_with(&paths, Some(&backend)).as_deref(),
        Some("hunter2")
    );
    cleanup(&paths);
}

#[test]
fn save_without_a_secret_service_reports_the_file_store_but_succeeds() {
    let paths = temp_paths("none");

    let store = save_password_with(&paths, "hunter2", None).expect("save");

    assert_eq!(store, CredentialStore::File);
    assert_eq!(load_password_with(&paths, None).as_deref(), Some("hunter2"));
    cleanup(&paths);
}

#[cfg(unix)]
#[test]
fn the_fallback_file_is_only_readable_by_its_owner() {
    let paths = temp_paths("mode");
    save_password_with(&paths, "hunter2", None).expect("save");

    assert_eq!(
        mode_of(&credentials_path(&paths)),
        0o600,
        "the password file must be 0600"
    );
    cleanup(&paths);
}

/// The regression INT-09 reports: the file used to be created with the umask
/// (0644 on most systems) and only `chmod`ed afterwards. `create_private_file`
/// is the primitive that closes that window; it must hand back a `0600` file.
#[cfg(unix)]
#[test]
fn private_file_is_created_with_mode_0600() {
    let paths = temp_paths("create");
    std::fs::create_dir_all(paths.config.parent().expect("parent")).expect("mkdir");
    let path = credentials_path(&paths);

    let file = create_private_file(&path).expect("create");
    drop(file);

    assert_eq!(mode_of(&path), 0o600);
    cleanup(&paths);
}

/// A second save must not leave a stale plaintext copy behind when the secret
/// service starts working again (or vice versa).
#[test]
fn saving_to_the_secret_service_removes_a_stale_plaintext_file() {
    let paths = temp_paths("stale");
    save_password_with(&paths, "old", None).expect("file save");
    assert!(credentials_path(&paths).exists());

    let backend = StubBackend::working();
    let store = save_password_with(&paths, "new", Some(&backend)).expect("service save");

    assert_eq!(store, CredentialStore::SecretService);
    assert!(
        !credentials_path(&paths).exists(),
        "the old plaintext password is still on disk"
    );
    assert_eq!(
        load_password_with(&paths, Some(&backend)).as_deref(),
        Some("new")
    );
    cleanup(&paths);
}

#[test]
fn load_prefers_the_secret_service_over_the_file() {
    let paths = temp_paths("prefer");
    save_password_with(&paths, "from-file", None).expect("file save");
    let backend = StubBackend::working();
    backend.set("from-service").expect("stub");

    assert_eq!(
        load_password_with(&paths, Some(&backend)).as_deref(),
        Some("from-service")
    );
    // Without the backend the file is still readable.
    assert_eq!(
        load_password_with(&paths, None).as_deref(),
        Some("from-file")
    );
    cleanup(&paths);
}

#[test]
fn load_uses_the_file_when_the_service_has_no_entry() {
    let paths = temp_paths("empty-service");
    save_password_with(&paths, "from-file", None).expect("file save");
    let backend = StubBackend::working();
    assert_eq!(backend.get().expect("stub"), None);

    assert_eq!(
        load_password_with(&paths, Some(&backend)).as_deref(),
        Some("from-file")
    );
    cleanup(&paths);
}

#[test]
fn load_returns_none_when_there_is_no_password_anywhere() {
    let paths = temp_paths("empty");
    assert_eq!(load_password_with(&paths, None), None);
    assert_eq!(
        load_password_with(&paths, Some(&StubBackend::working())),
        None
    );
    cleanup(&paths);
}

#[test]
fn delete_removes_the_password_from_both_stores() {
    let paths = temp_paths("delete");
    let backend = StubBackend::working();
    save_password_with(&paths, "hunter2", None).expect("file save");
    backend.set("hunter2").expect("stub");

    delete_password_with(&paths, Some(&backend)).expect("delete");

    assert!(!credentials_path(&paths).exists());
    assert_eq!(backend.get().expect("stub"), None);
    assert_eq!(load_password_with(&paths, Some(&backend)), None);
    cleanup(&paths);
}

/// INT-01: the Secret Service is the default store — docs promised it for a whole
/// release while the code only wrote plaintext. `MYUZIK_KEYRING=0` switches it
/// off for a desktop with no keyring daemon.
#[test]
fn secret_service_is_on_by_default_and_can_be_switched_off() {
    assert!(
        secret_service_enabled(None),
        "the keyring is the default store"
    );
    assert!(secret_service_enabled(Some("")));
    assert!(secret_service_enabled(Some("1")));
    assert!(secret_service_enabled(Some("true")));
    assert!(secret_service_enabled(Some("yes")));
    assert!(secret_service_enabled(Some("on")));
    assert!(secret_service_enabled(Some("TRUE")));
    assert!(!secret_service_enabled(Some("0")));
    assert!(!secret_service_enabled(Some("false")));
    assert!(!secret_service_enabled(Some("no")));
    assert!(!secret_service_enabled(Some("off")));
    assert!(!secret_service_enabled(Some(" OFF ")));
}

/// Both stores are reported honestly through the same entry point the UI calls.
///
/// This uses explicit backends so a `cargo test` run never touches the user's own
/// keyring; the real Secret Service is covered by the ignored test below.
#[test]
fn save_password_reports_the_store_it_used() {
    let paths = temp_paths("public");

    // No keyring: the private file, and the password is really there.
    let store = save_password_with(&paths, "hunter2", None).expect("save to file");
    assert_eq!(store, CredentialStore::File);
    assert!(
        credentials_path(&paths).exists(),
        "the file store must leave a password file behind"
    );
    assert_eq!(load_password_with(&paths, None).as_deref(), Some("hunter2"));

    // With a working keyring: reported as such, and no plaintext left behind.
    let backend = StubBackend::default();
    let store = save_password_with(&paths, "hunter2", Some(&backend)).expect("save to keyring");
    assert_eq!(store, CredentialStore::SecretService);
    assert!(
        !credentials_path(&paths).exists(),
        "the keyring store must not leave plaintext behind"
    );
    assert_eq!(
        load_password_with(&paths, Some(&backend)).as_deref(),
        Some("hunter2")
    );

    delete_password_with(&paths, Some(&backend)).expect("delete");
    assert_eq!(load_password_with(&paths, Some(&backend)), None);
    cleanup(&paths);
}

/// The real `keyring` backend, end to end — the default store, exercised against
/// a live Secret Service. Ignored by default because it writes to the user's own
/// keyring:
///
/// ```text
/// cargo test --offline --test config_credentials -- --ignored
/// ```
///
/// It cleans up after itself (the entry is deleted, not just the temp dir).
#[test]
#[ignore = "writes to the real keyring; needs a session Secret Service"]
fn real_secret_service_round_trip_leaves_no_plaintext_file() {
    let paths = temp_paths("real-keyring");
    // Start from a clean slate — in both stores — so the round trip is what gets
    // read back.
    myuzik::config::delete_password(&paths).expect("pre-clean");

    let store = save_password(&paths, "real-secret").expect("save");
    assert_eq!(
        store,
        CredentialStore::SecretService,
        "no Secret Service answered, so this test cannot verify the keyring path"
    );
    assert!(
        !credentials_path(&paths).exists(),
        "the password was also written to disk in plaintext"
    );
    assert_eq!(load_password(&paths).as_deref(), Some("real-secret"));

    myuzik::config::delete_password(&paths).expect("delete");
    assert_eq!(load_password(&paths), None);
    cleanup(&paths);
}
