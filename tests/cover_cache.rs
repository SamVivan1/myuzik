use myuzik::library::cover_cache::{CoverCache, PruneReport};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\ncover-cache-test-png";
const JPEG: &[u8] = b"\xff\xd8\xff\xe0cover-cache-test-jpeg";

/// A unique temporary directory, removed when the test finishes.
struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(tag: &str) -> Self {
        Self::at(tag, true)
    }

    fn missing(tag: &str) -> Self {
        Self::at(tag, false)
    }

    fn at(tag: &str, create: bool) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let seq = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "myuzik-cover-cache-{}-{tag}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        if create {
            std::fs::create_dir_all(&path).expect("create test directory");
        }
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn cache(&self) -> CoverCache {
        CoverCache::new(self.path.clone())
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read test directory")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn write_then_read_round_trips_and_extension_follows_bytes() {
    let dir = TestDir::new("round-trip");
    let cache = dir.cache();

    let png_path = cache.write("png-cover", PNG).expect("write png cover");
    assert_eq!(
        png_path.extension().and_then(|ext| ext.to_str()),
        Some("png")
    );
    assert_eq!(cache.read("png-cover"), Some(PNG.to_vec()));

    let jpg_path = cache.write("jpg-cover", JPEG).expect("write jpeg cover");
    assert_eq!(
        jpg_path.extension().and_then(|ext| ext.to_str()),
        Some("jpg")
    );
    assert_eq!(cache.read("jpg-cover"), Some(JPEG.to_vec()));
}

#[test]
fn existing_path_finds_either_extension_and_none_when_absent() {
    let dir = TestDir::new("existing-path");
    let cache = dir.cache();

    let png_path = cache.write("with-png", PNG).expect("write png cover");
    let jpg_path = cache.write("with-jpg", JPEG).expect("write jpeg cover");

    assert_eq!(cache.existing_path("with-png"), Some(png_path));
    assert_eq!(cache.existing_path("with-jpg"), Some(jpg_path));
    assert_eq!(cache.existing_path("never-written"), None);
}

#[test]
fn writing_twice_replaces_the_file_and_leaves_no_tmp_file() {
    let dir = TestDir::new("replace");
    let cache = dir.cache();

    let first = [JPEG, b"first"].concat();
    let second = [JPEG, b"second"].concat();
    cache.write("replace-me", &first).expect("write first");
    cache.write("replace-me", &second).expect("write second");

    assert_eq!(cache.read("replace-me"), Some(second));
    let files = names(dir.path());
    assert_eq!(files, vec!["replace-me.jpg".to_string()]);
    assert!(
        files.iter().all(|name| !name.contains(".tmp")),
        "atomic writes must not leave temporary files: {files:?}"
    );
}

#[test]
fn file_count_and_total_bytes_reflect_the_directory() {
    let dir = TestDir::new("counts");
    let cache = dir.cache();

    assert_eq!(cache.file_count(), 0);
    assert_eq!(cache.total_bytes(), 0);

    cache.write("one", PNG).expect("write first");
    cache.write("two", JPEG).expect("write second");

    assert_eq!(cache.file_count(), 2);
    assert_eq!(cache.total_bytes(), (PNG.len() + JPEG.len()) as u64);

    let missing = TestDir::missing("counts-missing");
    let empty = missing.cache();
    assert_eq!(empty.file_count(), 0);
    assert_eq!(empty.total_bytes(), 0);
}

#[test]
fn cover_ids_cannot_escape_the_cache_directory() {
    let root = TestDir::new("traversal");
    let covers = root.path().join("covers");
    let cache = CoverCache::new(covers.clone());

    for id in [
        "../escaped",
        "nested/evil",
        "/absolute",
        "..\\escaped",
        "..",
        ".",
        "",
    ] {
        assert!(
            cache.write(id, PNG).is_err(),
            "cover id {id:?} must be rejected"
        );
        assert_eq!(
            cache.existing_path(id),
            None,
            "cover id {id:?} must not resolve"
        );
        assert_eq!(cache.read(id), None, "cover id {id:?} must not read");
    }

    assert!(
        !root.path().join("escaped.png").exists(),
        "a traversal id wrote outside the covers directory"
    );
    assert!(
        !covers.join("nested").exists(),
        "a slash id created a nested directory"
    );
    if covers.exists() {
        assert!(
            names(&covers).is_empty(),
            "rejected ids left files behind: {:?}",
            names(&covers)
        );
    }
}

fn write_sequence(cache: &CoverCache, prefix: &str, count: usize, bytes: &[u8]) {
    for index in 0..count {
        cache
            .write(&format!("{prefix}-{index:03}"), bytes)
            .expect("write sequenced cover");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn cover_names(dir: &Path) -> Vec<String> {
    names(dir)
        .into_iter()
        .filter(|name| name.ends_with(".png") || name.ends_with(".jpg"))
        .collect()
}

#[test]
fn read_returns_none_for_an_empty_file() {
    let dir = TestDir::new("empty");
    let cache = dir.cache();
    std::fs::write(dir.path().join("empty.png"), b"").expect("write empty file");

    assert_eq!(cache.read("empty"), None);
}

#[test]
fn prune_keeps_the_newest_files_and_honours_max_files() {
    let dir = TestDir::new("prune-files");
    let cache = dir.cache();
    let bytes = [JPEG, b"0123456789"].concat();
    write_sequence(&cache, "cover", 5, &bytes);

    let report = cache.prune(2, u64::MAX);

    assert_eq!(
        cover_names(dir.path()),
        vec!["cover-003.jpg".to_string(), "cover-004.jpg".to_string()],
        "pruning must keep the newest files"
    );
    assert_eq!(
        report,
        PruneReport {
            removed_files: 3,
            removed_bytes: 3 * bytes.len() as u64,
            kept_files: 2,
            kept_bytes: 2 * bytes.len() as u64,
        }
    );
}

#[test]
fn prune_honours_max_bytes_and_keeps_the_newest_until_the_budget_fits() {
    let dir = TestDir::new("prune-bytes");
    let cache = dir.cache();
    let bytes = vec![b'x'; 100];
    write_sequence(&cache, "cover", 4, &bytes);

    let report = cache.prune(100, 250);

    assert_eq!(
        cover_names(dir.path()),
        vec!["cover-002.jpg".to_string(), "cover-003.jpg".to_string()],
        "a 250-byte budget can only keep the two newest 100-byte covers"
    );
    assert_eq!(report.removed_files, 2);
    assert_eq!(report.removed_bytes, 200);
    assert_eq!(report.kept_files, 2);
    assert_eq!(report.kept_bytes, 200);
}

#[test]
fn prune_ignores_non_images_and_a_missing_directory() {
    let dir = TestDir::new("prune-ignores");
    let cache = dir.cache();
    std::fs::write(dir.path().join("notes.txt"), b"not an image").expect("write notes");
    std::fs::write(dir.path().join("cover.tmp"), b"temporary").expect("write tmp file");

    let report = cache.prune(0, 0);

    assert_eq!(report, PruneReport::default());
    assert!(dir.path().join("notes.txt").is_file());
    assert!(dir.path().join("cover.tmp").is_file());

    let missing = TestDir::missing("prune-missing");
    let empty = missing.cache();
    assert_eq!(empty.prune(0, 0), PruneReport::default());
}

#[test]
fn prune_never_deletes_a_file_that_is_in_flight() {
    let dir = TestDir::new("prune-in-flight");
    let cache = dir.cache();
    cache.write("old", JPEG).expect("write old cover");
    std::thread::sleep(Duration::from_millis(5));
    cache.write("new", JPEG).expect("write new cover");

    let claim = cache.begin_fetch("old").expect("claim the old cover");
    let first = cache.prune(0, u64::MAX);

    assert!(
        dir.path().join("old.jpg").is_file(),
        "in-flight file deleted"
    );
    assert!(!dir.path().join("new.jpg").exists());
    assert_eq!(
        first,
        PruneReport {
            removed_files: 1,
            removed_bytes: JPEG.len() as u64,
            kept_files: 1,
            kept_bytes: JPEG.len() as u64,
        }
    );

    drop(claim);
    let second = cache.prune(0, u64::MAX);
    assert!(
        !dir.path().join("old.jpg").exists(),
        "claim was not released"
    );
    assert_eq!(second.removed_files, 1);
    assert_eq!(second.kept_files, 0);
}

#[test]
fn begin_fetch_is_exclusive_and_releases_when_the_guard_drops() {
    let dir = TestDir::new("begin-fetch");
    let cache = dir.cache();

    let first = cache.begin_fetch("same").expect("first claim");
    assert!(
        cache.begin_fetch("same").is_none(),
        "a second claim must skip the download"
    );

    let unrelated = cache.begin_fetch("other").expect("unrelated claim");
    assert!(cache.begin_fetch("same").is_none());
    drop(unrelated);

    assert!(cache.begin_fetch("same").is_none());
    drop(first);
    assert!(
        cache.begin_fetch("same").is_some(),
        "dropping the guard must release the claim"
    );
}

#[test]
fn begin_fetch_allows_exactly_one_winner_across_threads() {
    use std::sync::Barrier;
    use std::sync::atomic::AtomicUsize;

    const THREADS: usize = 8;
    let dir = TestDir::new("begin-fetch-threads");
    let cache = Arc::new(dir.cache());
    let start = Arc::new(Barrier::new(THREADS));
    let done = Arc::new(Barrier::new(THREADS));
    let winners = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();

    for _ in 0..THREADS {
        let cache = cache.clone();
        let start = start.clone();
        let done = done.clone();
        let winners = winners.clone();
        handles.push(std::thread::spawn(move || {
            start.wait();
            let claim = cache.begin_fetch("shared");
            if claim.is_some() {
                winners.fetch_add(1, Ordering::SeqCst);
                assert!(
                    cache.begin_fetch("shared").is_none(),
                    "the winner must remain exclusive until it drops the guard"
                );
            }
            done.wait();
            drop(claim);
        }));
    }
    for handle in handles {
        handle.join().expect("claim thread did not panic");
    }

    assert_eq!(
        winners.load(Ordering::SeqCst),
        1,
        "exactly one caller may download a cover"
    );
    assert!(cache.begin_fetch("shared").is_some());
}

#[test]
fn the_guard_releases_the_claim_even_when_the_caller_panics() {
    let dir = TestDir::new("guard-panic");
    let cache = dir.cache();
    let claim = cache.begin_fetch("panicking").expect("claim before panic");

    let handle = std::thread::spawn(move || {
        let _claim = claim;
        panic!("simulated download panic");
    });

    assert!(handle.join().is_err(), "the worker must have panicked");
    assert!(
        cache.begin_fetch("panicking").is_some(),
        "unwinding must release the in-flight claim"
    );
}
