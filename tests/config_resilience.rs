//! Config resilience (audit INT-08 / INT-08b).
//!
//! Three properties are locked here:
//!
//! 1. A config file that cannot be read or parsed is **kept**, not deleted and
//!    not silently replaced: the original bytes end up in
//!    `config.toml.corrupt-<timestamp>` and the loader says so, so the UI can
//!    tell the user instead of showing an empty setup form.
//! 2. The app still starts on defaults — a broken file never blocks startup.
//! 3. The schema carries a version, an unversioned (pre-versioning) file is
//!    migrated on load, and keys this build does not know are reported instead
//!    of vanishing.

use myuzik::config::{
    AppConfig, CONFIG_VERSION, LoadOutcome, Paths, load_config, load_config_recovering,
};

fn temp_paths(tag: &str) -> Paths {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "myuzik-config-{tag}-{}-{nanos}",
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

fn write_config(paths: &Paths, contents: &str) {
    std::fs::create_dir_all(paths.config.parent().expect("parent")).expect("mkdir");
    std::fs::write(&paths.config, contents).expect("write config");
}

fn write_raw_config(paths: &Paths, contents: &[u8]) {
    std::fs::create_dir_all(paths.config.parent().expect("parent")).expect("mkdir");
    std::fs::write(&paths.config, contents).expect("write config");
}

/// Corrupt backups next to `config.toml`: `config.toml.corrupt-<timestamp>`.
fn backups(paths: &Paths) -> Vec<std::path::PathBuf> {
    let dir = paths.config.parent().expect("parent");
    let mut found: Vec<_> = std::fs::read_dir(dir)
        .expect("read config dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(".corrupt-"))
        })
        .collect();
    found.sort();
    found
}

fn cleanup(paths: &Paths) {
    if let Some(parent) = paths.config.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }
}

/// A half-written file (the classic `config.toml` corruption: the process died
/// mid-write, or an editor truncated it) must be moved aside with its bytes
/// intact, and the loader must return defaults *and say so*.
#[test]
fn a_corrupt_config_is_kept_aside_with_its_bytes() {
    let paths = temp_paths("corrupt");
    let original = "server_url = \"http://music.local:4533\"\nvolume = 0.";
    write_config(&paths, original);

    let outcome = load_config_recovering(&paths);

    assert_eq!(
        outcome.config,
        AppConfig::default(),
        "the app must still start on defaults"
    );
    let recovery = outcome
        .recovery
        .as_ref()
        .expect("a parse failure must be reported, not swallowed");
    let backup = recovery
        .backup
        .as_ref()
        .expect("the unreadable file must be moved aside, not deleted");
    assert_eq!(
        std::fs::read_to_string(backup).expect("read backup"),
        original,
        "the backup must hold the original bytes"
    );
    assert!(
        !paths.config.exists(),
        "the corrupt original must no longer shadow the file the app writes"
    );
    let name = backup.file_name().and_then(|n| n.to_str()).unwrap_or("");
    assert!(
        name.starts_with("config.toml.corrupt-"),
        "unexpected backup name: {name}"
    );
    let notice = outcome
        .notice()
        .expect("the caller needs a sentence for the user");
    assert!(
        notice.contains(name),
        "the notice must name the kept file so the user can find it: {notice}"
    );

    cleanup(&paths);
}

/// A file that is not even UTF-8 cannot be parsed either; it must take the same
/// recovery path rather than being read as "no config".
#[test]
fn a_config_that_is_not_utf8_is_kept_aside() {
    let paths = temp_paths("binary");
    write_raw_config(&paths, &[0xff, 0xfe, 0x00, b'v']);

    let outcome = load_config_recovering(&paths);

    assert_eq!(outcome.config, AppConfig::default());
    let backup = outcome
        .recovery
        .as_ref()
        .and_then(|recovery| recovery.backup.clone())
        .expect("an unreadable file must still be kept");
    assert_eq!(
        std::fs::read(backup).expect("read backup"),
        vec![0xff, 0xfe, 0x00, b'v']
    );

    cleanup(&paths);
}

/// A good file is read as it is, no backup appears, and a file written before
/// this version existed is migrated to the current schema version.
#[test]
fn a_valid_config_is_read_and_migrated_without_a_backup() {
    let paths = temp_paths("valid");
    write_config(
        &paths,
        "server_url = \"http://music.local:4533\"\nusername = \"sam\"\nvolume = 0.3\n",
    );

    let outcome = load_config_recovering(&paths);

    assert!(
        outcome.recovery.is_none(),
        "a readable file must not be recovered: {:?}",
        outcome.recovery
    );
    assert_eq!(outcome.config.server_url, "http://music.local:4533");
    assert_eq!(outcome.config.username, "sam");
    assert_eq!(outcome.config.volume, 0.3);
    assert_eq!(
        outcome.config.version, CONFIG_VERSION,
        "an unversioned file must be migrated to the current version"
    );
    assert!(
        backups(&paths).is_empty(),
        "a valid file must not be moved aside"
    );

    cleanup(&paths);
}

/// A config written by a newer build is not "corrupt": keep its version number
/// (writing it back as an older version would lose information) and load it.
#[test]
fn a_config_from_a_newer_version_is_loaded_as_it_is() {
    let paths = temp_paths("newer");
    write_config(
        &paths,
        &format!("version = {}\nvolume = 0.42\n", CONFIG_VERSION + 1),
    );

    let outcome = load_config_recovering(&paths);

    assert!(outcome.recovery.is_none(), "must not be treated as corrupt");
    assert_eq!(outcome.config.volume, 0.42);
    assert_eq!(
        outcome.config.version,
        CONFIG_VERSION + 1,
        "downgrading the version would rewrite the user's file as older than it is"
    );

    cleanup(&paths);
}

/// INT-08b: `#[serde(default)]` used to make a typo invisible. The key is
/// reported by name, the known keys keep their values, and nothing is reset.
#[test]
fn an_unknown_key_is_reported_by_name() {
    let paths = temp_paths("unknown");
    write_config(
        &paths,
        "volume = 0.3\nvolum = 0.9\nprefer_navidrome_lirics = false\n",
    );

    let outcome = load_config_recovering(&paths);

    assert!(outcome.recovery.is_none());
    assert_eq!(
        outcome.unknown_keys,
        vec!["prefer_navidrome_lirics".to_string(), "volum".to_string()],
        "both typos must be named, in a stable order"
    );
    assert_eq!(
        outcome.config.volume, 0.3,
        "a typo must not change the value of the key it was meant to be"
    );

    cleanup(&paths);
}

/// No file is not a failure: a fresh install has no `config.toml`.
#[test]
fn a_missing_config_is_not_a_recovery() {
    let paths = temp_paths("missing");

    let outcome: LoadOutcome = load_config_recovering(&paths);

    assert_eq!(outcome.config, AppConfig::default());
    assert!(outcome.recovery.is_none());
    assert!(outcome.unknown_keys.is_empty());
    assert!(outcome.notice().is_none());

    cleanup(&paths);
}

/// Two corrupt configs in the same second must not overwrite each other: every
/// unreadable file the user ever had stays recoverable.
#[test]
fn a_second_broken_config_never_overwrites_the_first_backup() {
    let paths = temp_paths("twice");
    write_config(&paths, "first = [broken");
    let first = load_config_recovering(&paths);
    let first_backup = first
        .recovery
        .and_then(|recovery| recovery.backup)
        .expect("first backup");

    write_config(&paths, "second = [broken");
    let second = load_config_recovering(&paths);
    let second_backup = second
        .recovery
        .and_then(|recovery| recovery.backup)
        .expect("second backup");

    assert_ne!(
        first_backup, second_backup,
        "the second recovery reused the first backup's name"
    );
    assert_eq!(
        std::fs::read_to_string(&first_backup).expect("first"),
        "first = [broken"
    );
    assert_eq!(
        std::fs::read_to_string(&second_backup).expect("second"),
        "second = [broken"
    );

    cleanup(&paths);
}

/// `load_config` is the older, `Result`-shaped entry point other code still
/// uses; it must keep its contract (an unreadable file is an `Err`) *and* keep
/// the bytes, so a later save cannot destroy the only copy.
#[test]
fn load_config_reports_an_unreadable_file_and_still_keeps_it() {
    let paths = temp_paths("legacy");
    write_config(&paths, "volume = ");

    let error = load_config(&paths).expect_err("an unreadable file must be an error");

    assert!(
        error.to_string().contains("could not be parsed"),
        "the error must say what was wrong: {error}"
    );
    assert_eq!(
        backups(&paths).len(),
        1,
        "the older entry point must not lose the user's file"
    );

    cleanup(&paths);
}

/// Saving a config that carried unknown keys must not drop them on the floor:
/// they were reported, but the file is also not silently rewritten without them.
#[test]
fn saving_keeps_the_keys_the_loader_did_not_understand() {
    let paths = temp_paths("keep");
    write_config(&paths, "volum = 0.9\nvolume = 0.3\n");

    let outcome = load_config_recovering(&paths);
    assert_eq!(outcome.unknown_keys, vec!["volum".to_string()]);

    myuzik::config::save_config(&paths, &outcome.config).expect("save");
    let written = std::fs::read_to_string(&paths.config).expect("read");
    assert!(
        written.contains("volum = 0.9"),
        "the unknown key was destroyed by a save: {written}"
    );
    assert!(
        written.contains(&format!("version = {CONFIG_VERSION}")),
        "a saved config must carry the schema version: {written}"
    );

    cleanup(&paths);
}
