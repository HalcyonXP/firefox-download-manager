use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use download_manager_engine::storage::{FileRange, PartialFile, StorageError, sanitize_filename};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

#[test]
fn creates_unique_preallocated_part_files_in_paths_with_spaces() {
    let directory = TestDirectory::new("destination with spaces");
    let first =
        PartialFile::create(directory.path(), "payload.bin", 32).expect("create first part");
    let second =
        PartialFile::create(directory.path(), "payload.bin", 32).expect("create second part");

    assert_ne!(first.partial_path(), second.partial_path());
    assert_eq!(
        first
            .partial_path()
            .extension()
            .and_then(|value| value.to_str()),
        Some("part")
    );
    assert_eq!(
        fs::metadata(first.partial_path())
            .expect("first metadata")
            .len(),
        32
    );
    assert_eq!(
        fs::metadata(second.partial_path())
            .expect("second metadata")
            .len(),
        32
    );
    assert_eq!(first.final_name().as_str(), "payload.bin");
    assert_eq!(first.expected_len(), 32);
}

#[test]
fn assignments_reject_active_and_completed_overlap() {
    let directory = TestDirectory::new("overlap");
    let storage = PartialFile::create(directory.path(), "ranges.bin", 8).expect("create part");
    let first_range = range(0, 4);
    let mut first = storage.assign(first_range).expect("assign first half");

    assert!(matches!(
        storage.assign(range(3, 6)),
        Err(StorageError::OverlappingAssignment { range: rejected }) if rejected == range(3, 6)
    ));

    first.write(b"ABCD").expect("write first half");
    first.finish().expect("finish first half");
    assert_eq!(storage.completed_ranges(), vec![first_range]);
    assert!(matches!(
        storage.assign(first_range),
        Err(StorageError::OverlappingAssignment { .. })
    ));
}

#[test]
fn short_assignment_remains_incomplete_until_all_bytes_arrive() {
    let directory = TestDirectory::new("short");
    let storage = PartialFile::create(directory.path(), "short.bin", 4).expect("create part");
    let mut writer = storage.assign(range(0, 4)).expect("assign file");

    writer.write(b"AB").expect("write prefix");
    assert_eq!(
        writer.finish(),
        Err(StorageError::IncompleteAssignment {
            expected: 4,
            written: 2,
        })
    );
    assert!(storage.completed_ranges().is_empty());

    writer.write(b"CD").expect("write suffix");
    writer.finish().expect("finish exact segment");
    assert_eq!(storage.completed_ranges(), vec![range(0, 4)]);
    assert_eq!(writer.write(b"E"), Err(StorageError::AssignmentClosed));
}

#[test]
fn assignment_and_write_bounds_are_checked_before_io() {
    let directory = TestDirectory::new("bounds");
    let storage = PartialFile::create(directory.path(), "bounds.bin", 8).expect("create part");

    assert_eq!(
        FileRange::new(4, 4),
        Err(StorageError::InvalidRange { start: 4, end: 4 })
    );
    assert!(matches!(
        storage.assign(range(7, 9)),
        Err(StorageError::RangeOutOfBounds {
            expected_len: 8,
            ..
        })
    ));

    let mut writer = storage.assign(range(2, 6)).expect("assign bounded range");
    assert_eq!(
        writer.write(b"12345"),
        Err(StorageError::WriteOutOfBounds {
            range: range(2, 6),
            offset: 2,
            bytes: 5,
        })
    );
    assert_eq!(writer.written_len(), 0);
    assert_eq!(
        fs::metadata(storage.partial_path())
            .expect("metadata")
            .len(),
        8
    );
}

#[test]
fn dropped_short_assignment_can_be_retried_without_claiming_coverage() {
    let directory = TestDirectory::new("retry");
    let storage = PartialFile::create(directory.path(), "retry.bin", 4).expect("create part");
    {
        let mut abandoned = storage.assign(range(0, 4)).expect("assign first attempt");
        abandoned.write(b"XX").expect("write uncommitted prefix");
    }

    assert!(storage.completed_ranges().is_empty());
    let mut retry = storage.assign(range(0, 4)).expect("reassign full range");
    retry.write(b"GOOD").expect("overwrite full range");
    retry.finish().expect("finish retry");
    let promotion = storage.promote().expect("publish retry");
    assert_eq!(
        fs::read(promotion.final_path()).expect("read final"),
        b"GOOD"
    );
}

#[test]
fn disjoint_threaded_writers_produce_exact_random_access_output() {
    let directory = TestDirectory::new("concurrent");
    let storage = PartialFile::create(directory.path(), "joined.bin", 12).expect("create part");
    let mut first = storage.assign(range(0, 6)).expect("assign first");
    let mut second = storage.assign(range(6, 12)).expect("assign second");

    let first_thread = thread::spawn(move || {
        first.write(b"ABC").expect("first chunk");
        first.write(b"DEF").expect("second chunk");
        first.finish().expect("finish first");
    });
    let second_thread = thread::spawn(move || {
        second.write(b"GHIJ").expect("first chunk");
        second.write(b"KL").expect("second chunk");
        second.finish().expect("finish second");
    });
    first_thread.join().expect("join first writer");
    second_thread.join().expect("join second writer");

    assert_eq!(storage.completed_ranges(), vec![range(0, 12)]);
    let partial_path = storage.partial_path().to_owned();
    let promotion = storage.promote().expect("publish complete file");
    assert_eq!(promotion.partial_cleanup_failure(), None);
    assert!(!partial_path.exists());
    assert_eq!(
        fs::read(promotion.final_path()).expect("read final"),
        b"ABCDEFGHIJKL"
    );
}

#[test]
fn promotion_never_overwrites_and_selects_a_create_new_name() {
    let directory = TestDirectory::new("collisions");
    fs::write(directory.path().join("report.txt"), b"original").expect("write original");
    fs::write(directory.path().join("report (1).txt"), b"other").expect("write collision");
    let storage = PartialFile::create(directory.path(), "report.txt", 3).expect("create part");
    let mut writer = storage.assign(range(0, 3)).expect("assign content");
    writer.write(b"new").expect("write content");
    writer.finish().expect("finish content");

    let promotion = storage.promote().expect("publish without overwrite");
    assert_eq!(
        promotion
            .final_path()
            .file_name()
            .and_then(|name| name.to_str()),
        Some("report (2).txt")
    );
    assert_eq!(
        fs::read(directory.path().join("report.txt")).expect("read original"),
        b"original"
    );
    assert_eq!(
        fs::read(directory.path().join("report (1).txt")).expect("read collision"),
        b"other"
    );
    assert_eq!(
        fs::read(promotion.final_path()).expect("read published"),
        b"new"
    );
}

#[test]
fn simultaneous_publication_uses_distinct_create_new_names() {
    let directory = TestDirectory::new("publication-race");
    let first = completed_storage(directory.path(), "race.bin", b"AAAA");
    let second = completed_storage(directory.path(), "race.bin", b"BBBB");

    let first_thread = thread::spawn(move || first.promote().expect("publish first"));
    let second_thread = thread::spawn(move || second.promote().expect("publish second"));
    let first_promotion = first_thread.join().expect("join first publication");
    let second_promotion = second_thread.join().expect("join second publication");

    assert_ne!(first_promotion.final_path(), second_promotion.final_path());
    let mut names = [
        first_promotion
            .final_path()
            .file_name()
            .and_then(|name| name.to_str())
            .expect("first filename"),
        second_promotion
            .final_path()
            .file_name()
            .and_then(|name| name.to_str())
            .expect("second filename"),
    ];
    names.sort_unstable();
    assert_eq!(names, ["race (1).bin", "race.bin"]);
    let mut contents = [
        fs::read(first_promotion.final_path()).expect("read first"),
        fs::read(second_promotion.final_path()).expect("read second"),
    ];
    contents.sort_unstable();
    assert_eq!(contents, [b"AAAA".to_vec(), b"BBBB".to_vec()]);
}

#[test]
fn promotion_requires_no_active_writers_and_exact_coverage() {
    let directory = TestDirectory::new("coverage");
    let storage = PartialFile::create(directory.path(), "coverage.bin", 8).expect("create part");
    let active = storage.assign(range(0, 4)).expect("assign prefix");

    assert_eq!(
        storage.promote(),
        Err(StorageError::ActiveAssignments { count: 1 })
    );
    drop(active);
    assert_eq!(storage.promote(), Err(StorageError::IncompleteCoverage));
    assert!(!directory.path().join("coverage.bin").exists());

    let mut prefix = storage.assign(range(0, 4)).expect("reassign prefix");
    prefix.write(b"1234").expect("write prefix");
    prefix.finish().expect("finish prefix");
    assert_eq!(storage.promote(), Err(StorageError::IncompleteCoverage));
    assert!(!directory.path().join("coverage.bin").exists());
}

#[test]
fn promotion_rechecks_preallocated_length_before_publication() {
    let directory = TestDirectory::new("truncated");
    let storage = PartialFile::create(directory.path(), "truncated.bin", 4).expect("create part");
    let mut writer = storage.assign(range(0, 4)).expect("assign content");
    writer.write(b"DATA").expect("write content");
    writer.finish().expect("finish content");
    OpenOptions::new()
        .write(true)
        .open(storage.partial_path())
        .expect("open second handle")
        .set_len(2)
        .expect("truncate partial");

    assert_eq!(
        storage.promote(),
        Err(StorageError::FileLengthChanged {
            expected: 4,
            actual: 2,
        })
    );
    assert!(!directory.path().join("truncated.bin").exists());
}

#[test]
fn empty_file_can_be_flushed_and_published_without_assignments() {
    let directory = TestDirectory::new("empty");
    let storage = PartialFile::create(directory.path(), "empty.bin", 0).expect("create empty part");
    let promotion = storage.promote().expect("publish empty file");

    assert_eq!(
        fs::metadata(promotion.final_path())
            .expect("metadata")
            .len(),
        0
    );
}

#[test]
fn public_sanitizer_always_returns_one_non_reserved_component() {
    let sanitized = sanitize_filename(r"\\?\C:\outside\AUX.txt::$DATA");
    assert!(!sanitized.as_str().contains(['/', '\\', ':']));
    assert_ne!(sanitized.as_str().to_ascii_uppercase(), "AUX.TXT");
    assert_eq!(Path::new(sanitized.as_str()).components().count(), 1);
}

#[test]
fn ordinary_debug_output_redacts_paths_and_filenames() {
    let directory = TestDirectory::new("debug-secret-directory");
    let storage = PartialFile::create(directory.path(), "secret-name.bin", 0).expect("create part");
    let storage_debug = format!("{storage:?}");
    assert!(!storage_debug.contains("debug-secret-directory"));
    assert!(!storage_debug.contains("secret-name.bin"));
    assert!(!format!("{:?}", storage.final_name()).contains("secret-name.bin"));

    let promotion = storage.promote().expect("publish empty file");
    assert!(!format!("{promotion:?}").contains("secret-name.bin"));
}

fn completed_storage(destination: &Path, filename: &str, bytes: &[u8]) -> PartialFile {
    let expected_len = u64::try_from(bytes.len()).expect("test content length fits u64");
    let storage = PartialFile::create(destination, filename, expected_len).expect("create part");
    let mut writer = storage
        .assign(range(0, expected_len))
        .expect("assign complete content");
    writer.write(bytes).expect("write complete content");
    writer.finish().expect("finish complete content");
    storage
}

fn range(start: u64, end: u64) -> FileRange {
    FileRange::new(start, end).expect("valid test range")
}

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(label: &str) -> Self {
        let counter = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "download-manager-storage-{label}-{}-{timestamp}-{counter}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create test directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
