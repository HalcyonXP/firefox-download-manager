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
    assert_eq!(first.expected_len(), Some(32));
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
    let mut promotion = storage.promote().expect("publish complete file");
    assert_eq!(promotion.partial_cleanup_failure(), None);
    assert_eq!(promotion.partial_path(), Some(partial_path.as_path()));
    assert!(partial_path.exists());
    assert_eq!(
        fs::read(promotion.final_path()).expect("read final"),
        b"ABCDEFGHIJKL"
    );
    promotion
        .cleanup_partial()
        .expect("remove checkpointed partial link");
    assert_eq!(promotion.partial_path(), None);
    assert!(!partial_path.exists());
}

#[test]
fn unknown_length_stream_seals_exact_coverage_before_publication() {
    let directory = TestDirectory::new("streaming");
    let storage = PartialFile::create_streaming(directory.path(), "stream.bin")
        .expect("create streaming partial");
    assert_eq!(storage.expected_len(), None);
    assert!(matches!(
        storage.assign(range(0, 1)),
        Err(StorageError::LengthUnknown)
    ));

    let mut writer = storage.begin_stream(8).expect("begin bounded stream");
    writer.write(b"ABC").expect("write first stream chunk");
    writer.write(b"DEFGH").expect("write second stream chunk");
    assert_eq!(writer.written_len(), 8);
    assert_eq!(
        writer.write(b"I"),
        Err(StorageError::StreamLimitExceeded { limit: 8 })
    );
    assert_eq!(writer.finish().expect("seal stream"), 8);
    assert_eq!(storage.expected_len(), Some(8));
    assert_eq!(storage.completed_ranges(), vec![range(0, 8)]);

    let mut promotion = storage.promote().expect("publish sealed stream");
    assert_eq!(
        fs::read(promotion.final_path()).expect("read streamed output"),
        b"ABCDEFGH"
    );
    promotion.cleanup_partial().expect("clean stream partial");
}

#[test]
fn unknown_length_stream_can_seal_a_clean_empty_eof() {
    let directory = TestDirectory::new("empty-stream");
    let storage = PartialFile::create_streaming(directory.path(), "empty-stream.bin")
        .expect("create streaming partial");
    let mut writer = storage.begin_stream(1).expect("begin empty response");
    assert_eq!(writer.finish().expect("seal empty EOF"), 0);
    assert_eq!(storage.expected_len(), Some(0));
    assert!(storage.completed_ranges().is_empty());
    let output = storage.promote().expect("publish empty stream");
    assert!(
        fs::read(output.final_path())
            .expect("read empty stream")
            .is_empty()
    );
}

#[test]
fn interrupted_unknown_stream_restarts_from_zero() {
    let directory = TestDirectory::new("stream-retry");
    let storage = PartialFile::create_streaming(directory.path(), "stream.bin")
        .expect("create streaming partial");
    {
        let mut first = storage.begin_stream(16).expect("begin first stream");
        first.write(b"STALE").expect("write stale attempt");
    }
    assert_eq!(storage.expected_len(), None);
    assert!(storage.completed_ranges().is_empty());

    let mut retry = storage.begin_stream(16).expect("restart stream");
    retry.write(b"GOOD").expect("write replacement stream");
    retry.finish().expect("seal replacement stream");
    assert_eq!(
        fs::metadata(storage.partial_path())
            .expect("metadata")
            .len(),
        4
    );
    let promotion = storage.promote().expect("publish replacement stream");
    assert_eq!(
        fs::read(promotion.final_path()).expect("read final"),
        b"GOOD"
    );
}

#[test]
fn recovery_rejects_noncanonical_coverage_before_reopening() {
    let directory = TestDirectory::new("recover-coverage");
    let storage = PartialFile::create(directory.path(), "recover.bin", 8).expect("create part");
    let mut writer = storage.assign(range(0, 4)).expect("assign durable prefix");
    writer.write(b"DATA").expect("write durable prefix");
    writer.finish().expect("finish durable prefix");
    storage
        .durable_completed_ranges()
        .expect("flush durable prefix");
    let partial_path = storage.partial_path().to_owned();
    drop(writer);
    drop(storage);

    assert!(matches!(
        PartialFile::recover(&partial_path, "recover.bin", 8, &[range(0, 4), range(4, 8)]),
        Err(StorageError::InvalidCompletedCoverage)
    ));
    let recovered = PartialFile::recover(&partial_path, "recover.bin", 8, &[range(0, 4)])
        .expect("recover canonical coverage");
    assert_eq!(recovered.completed_ranges(), vec![range(0, 4)]);
    assert!(recovered.assign(range(0, 4)).is_err());
    recovered.assign(range(4, 8)).expect("assign missing bytes");
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

#[test]
fn sha256_known_vectors_are_streamed_and_publication_requires_the_owned_lease() {
    use download_manager_engine::integrity::ExpectedSha256;
    let directory = TestDirectory::new("sha256-vectors");
    for (index, (bytes, digest)) in [
        (
            Vec::new(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"abc".to_vec(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            vec![b'a'; 1_000_000],
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("vector-{index}.bin");
        let partial = if bytes.is_empty() {
            PartialFile::create(directory.path(), &name, 0).expect("empty")
        } else {
            completed_storage(directory.path(), &name, &bytes)
        };
        let mut checks = 0;
        let lease = partial
            .validate(ExpectedSha256::parse(digest), || {
                checks += 1;
                false
            })
            .expect("known hash");
        assert_eq!(
            checks,
            bytes.len().div_ceil(256 * 1024) + 2,
            "one bounded chunk per cancellation check, plus initial/EOF checks"
        );
        assert!(!directory.path().join(&name).exists());
        assert!(matches!(
            partial.validate(None, || false),
            Err(StorageError::NotActive)
        ));
        assert!(
            matches!(partial.promote(), Err(StorageError::NotActive)),
            "a clone cannot bypass the active validation lease"
        );
        let output = lease.promote().expect("publish validated file");
        assert_eq!(fs::read(output.final_path()).expect("output"), bytes);
    }
}

#[test]
fn sha256_detects_disk_corruption_and_cancelled_validation_can_be_repeated() {
    use download_manager_engine::integrity::ExpectedSha256;
    use std::io::Write;
    let directory = TestDirectory::new("sha256-failure");
    let partial = completed_storage(directory.path(), "million.bin", &vec![b'a'; 1_000_000]);
    let expected =
        ExpectedSha256::parse("cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    let mut checks = 0;
    assert!(matches!(
        partial.validate(expected, || {
            checks += 1;
            checks == 3
        }),
        Err(StorageError::ValidationCancelled)
    ));
    assert_eq!(checks, 3);
    assert!(!directory.path().join("million.bin").exists());
    let lease = partial
        .validate(expected, || false)
        .expect("retry full validation");
    drop(lease);
    let mut external = OpenOptions::new()
        .write(true)
        .open(partial.partial_path())
        .expect("open mutation handle");
    external.write_all(b"b").expect("same-length corruption");
    external.sync_all().expect("flush corruption");
    assert!(matches!(
        partial.validate(expected, || false),
        Err(StorageError::ChecksumMismatch)
    ));
    assert!(!directory.path().join("million.bin").exists());
}

#[test]
fn sha256_cannot_validate_active_assignments_gaps_or_changed_lengths() {
    use download_manager_engine::integrity::ExpectedSha256;
    let directory = TestDirectory::new("sha256-structure");
    let partial = PartialFile::create(directory.path(), "gap.bin", 4).expect("partial");
    let mut writer = partial.assign(range(0, 2)).expect("assignment");
    assert!(matches!(
        partial.validate(None, || false),
        Err(StorageError::ActiveAssignments { .. })
    ));
    writer.write(b"ab").expect("bytes");
    writer.finish().expect("finish");
    assert!(matches!(
        partial.validate(None, || false),
        Err(StorageError::IncompleteCoverage)
    ));
    let complete = completed_storage(directory.path(), "length.bin", b"abc");
    OpenOptions::new()
        .write(true)
        .open(complete.partial_path())
        .expect("file")
        .set_len(2)
        .expect("truncate");
    assert!(matches!(
        complete.validate(
            ExpectedSha256::parse(
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            ),
            || false
        ),
        Err(StorageError::FileLengthChanged { .. })
    ));
    assert!(!directory.path().join("length.bin").exists());
}

#[cfg(windows)]
#[test]
fn sha256_windows_lock_rejects_competing_io_and_releases_on_lease_drop() {
    use std::io::Write;
    let directory = TestDirectory::new("sha256-lock");
    let partial = completed_storage(directory.path(), "locked.bin", b"abc");
    let mut external = OpenOptions::new()
        .read(true)
        .write(true)
        .open(partial.partial_path())
        .expect("external handle");
    external.try_lock().expect("competing owner");
    assert!(matches!(
        partial.validate(None, || false),
        Err(StorageError::Io {
            failure: download_manager_engine::storage::IoFailure::FileLocked,
            ..
        })
    ));
    assert!(
        partial.validate(None, || false).is_err(),
        "failed acquisition never unlocks the other owner"
    );
    external.unlock().expect("release competitor");
    let lease = partial.validate(None, || false).expect("validation lease");
    assert!(
        external.write_all(b"x").is_err(),
        "ordinary competing I/O is locked out"
    );
    drop(lease);
    external.write_all(b"a").expect("lease released");
}

#[cfg(windows)]
fn zone_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(":Zone.Identifier");
    PathBuf::from(name)
}

#[cfg(windows)]
#[test]
fn windows_internet_zone_precedes_final_publication_and_survives_partial_cleanup() {
    let directory = TestDirectory::new("internet-zone");
    let partial = completed_storage(directory.path(), "internet.bin", b"abc");
    let mut promotion = partial.promote().expect("protected promotion");
    assert_eq!(
        fs::read(zone_path(promotion.final_path())).expect("final zone"),
        b"[ZoneTransfer]\r\nZoneId=3\r\n"
    );
    assert_eq!(fs::read(promotion.final_path()).expect("data"), b"abc");
    promotion.cleanup_partial().expect("partial cleanup");
    assert_eq!(
        fs::read(zone_path(&directory.path().join("internet.bin"))).expect("retained zone"),
        b"[ZoneTransfer]\r\nZoneId=3\r\n"
    );
}

#[cfg(windows)]
#[test]
fn windows_internet_zone_preserves_restricted_and_refuses_weaker_or_unknown_metadata() {
    let directory = TestDirectory::new("zone-policy");
    for (index, marker) in [
        b"[ZoneTransfer]\r\nZoneId=0\r\n".as_slice(),
        b"unknown",
        b"",
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("refused-{index}.bin");
        let partial = completed_storage(directory.path(), &name, b"abc");
        fs::write(zone_path(partial.partial_path()), marker).expect("fixture metadata");
        assert_eq!(
            partial.promote().err(),
            Some(StorageError::InvalidDownloadZone)
        );
        assert!(!directory.path().join(name).exists());
        assert_eq!(
            fs::read(zone_path(partial.partial_path())).expect("unchanged marker"),
            marker
        );
    }
    let partial = completed_storage(directory.path(), "restricted.bin", b"abc");
    let marker = b"[ZoneTransfer]\r\nZoneId=4\r\n";
    fs::write(zone_path(partial.partial_path()), marker).expect("restricted fixture");
    let promoted = partial.promote().expect("restricted promotion");
    assert_eq!(
        fs::read(zone_path(promoted.final_path())).expect("restricted readback"),
        marker
    );
}

#[cfg(windows)]
#[test]
fn windows_internet_zone_locked_stream_refuses_promotion_and_can_retry_after_release() {
    let directory = TestDirectory::new("zone-locked");
    let partial = completed_storage(directory.path(), "locked-zone.bin", b"abc");
    fs::write(
        zone_path(partial.partial_path()),
        b"[ZoneTransfer]\r\nZoneId=3\r\n",
    )
    .expect("zone fixture");
    let writer = OpenOptions::new()
        .write(true)
        .open(zone_path(partial.partial_path()))
        .expect("competing writer");
    assert!(partial.promote().is_err());
    assert!(!directory.path().join("locked-zone.bin").exists());
    drop(writer);
    partial
        .promote()
        .expect("fresh validation after retirement");
}

#[cfg(windows)]
#[test]
fn windows_internet_zone_collision_never_marks_or_changes_existing_final_file() {
    let directory = TestDirectory::new("zone-collision");
    let existing = directory.path().join("keep.bin");
    fs::write(&existing, b"existing").expect("existing final");
    let partial = completed_storage(directory.path(), "keep.bin", b"abc");
    let promoted = partial.promote().expect("numbered promotion");
    assert_ne!(promoted.final_path(), existing);
    assert_eq!(fs::read(&existing).expect("existing bytes"), b"existing");
    assert!(!zone_path(&existing).exists());
    assert_eq!(
        fs::read(zone_path(promoted.final_path())).expect("new marker"),
        b"[ZoneTransfer]\r\nZoneId=3\r\n"
    );
}

#[test]
fn computed_fingerprints_require_complete_hashing_and_keep_the_validation_lease() {
    use sha2::{Digest, Sha256};
    let directory = TestDirectory::new("computed-fingerprints");
    for (index, bytes) in [Vec::new(), b"abc".to_vec(), vec![b'a'; 1_000_000]]
        .into_iter()
        .enumerate()
    {
        let name = format!("fingerprint-{index}.bin");
        let partial = if bytes.is_empty() {
            PartialFile::create(directory.path(), &name, 0).expect("empty")
        } else {
            completed_storage(directory.path(), &name, &bytes)
        };
        let ordinary = partial
            .validate(None, || false)
            .expect("ordinary validation");
        assert!(ordinary.fingerprint().is_none());
        drop(ordinary);
        let mut checks = 0;
        let lease = partial
            .validate_with_fingerprint(None, || {
                checks += 1;
                false
            })
            .expect("computed fingerprint");
        let fingerprint = lease.fingerprint().expect("hashing was mandatory");
        let expected: [u8; 32] = Sha256::digest(&bytes).into();
        assert_eq!(fingerprint.sha256(), expected);
        assert_eq!(
            fingerprint.length(),
            u64::try_from(bytes.len()).expect("fixture size")
        );
        assert_eq!(checks, bytes.len().div_ceil(256 * 1024) + 2);
        assert_eq!(
            format!("{fingerprint:?}"),
            "ValidatedFingerprint(<redacted>)"
        );
        assert!(matches!(partial.promote(), Err(StorageError::NotActive)));
        assert!(matches!(
            partial.validate_with_fingerprint(None, || false),
            Err(StorageError::NotActive)
        ));
        assert!(!directory.path().join(&name).exists());
        let promotion = lease.promote().expect("same owned lease");
        assert_eq!(fs::read(promotion.final_path()).expect("output"), bytes);
    }
}

#[test]
fn fingerprint_cancellation_expectation_and_changed_bytes_do_not_reuse_old_evidence() {
    use download_manager_engine::integrity::ExpectedSha256;
    use std::io::Write;
    let directory = TestDirectory::new("fingerprint-retirement");
    let partial = completed_storage(directory.path(), "large.bin", &vec![b'a'; 1_000_000]);
    let mut checks = 0;
    assert!(matches!(
        partial.validate_with_fingerprint(None, || {
            checks += 1;
            checks == 3
        }),
        Err(StorageError::ValidationCancelled)
    ));
    let wrong = ExpectedSha256::parse(&"00".repeat(32)).expect("valid expected hash");
    assert!(matches!(
        partial.validate_with_fingerprint(Some(wrong), || false),
        Err(StorageError::ChecksumMismatch)
    ));
    let correct =
        ExpectedSha256::parse("cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    let lease = partial
        .validate_with_fingerprint(correct, || false)
        .expect("expected and computed");
    let before = lease.fingerprint().expect("computed");
    drop(lease);
    let mut writer = OpenOptions::new()
        .write(true)
        .open(partial.partial_path())
        .expect("owned fixture mutation");
    writer.write_all(b"b").expect("change one byte");
    writer.sync_all().expect("flush");
    drop(writer);
    let lease = partial
        .validate_with_fingerprint(None, || false)
        .expect("recompute");
    let after = lease.fingerprint().expect("computed again");
    assert_eq!(before.length(), after.length());
    assert_ne!(before.sha256(), after.sha256());
    drop(lease);
    assert!(matches!(
        partial.validate_with_fingerprint(correct, || false),
        Err(StorageError::ChecksumMismatch)
    ));
    assert!(
        partial
            .validate(None, || false)
            .expect("ordinary independent validation")
            .fingerprint()
            .is_none()
    );
    assert!(!directory.path().join("large.bin").exists());
}

#[test]
fn fingerprints_cannot_be_obtained_from_gaps_or_active_assignments() {
    let directory = TestDirectory::new("fingerprint-incomplete");
    let partial = PartialFile::create(directory.path(), "gap.bin", 4).expect("create");
    assert!(matches!(
        partial.validate_with_fingerprint(None, || false),
        Err(StorageError::IncompleteCoverage)
    ));
    let writer = partial.assign(range(0, 4)).expect("active assignment");
    assert!(matches!(
        partial.validate_with_fingerprint(None, || false),
        Err(StorageError::ActiveAssignments { .. })
    ));
    drop(writer);
    assert!(matches!(
        partial.validate_with_fingerprint(None, || false),
        Err(StorageError::IncompleteCoverage)
    ));
}
