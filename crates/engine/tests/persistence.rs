use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use download_manager_engine::network::{EntityTag, Validators};
use download_manager_engine::persistence::{
    CheckpointOutcome, CheckpointPolicy, CheckpointUrgency, CleanupOutcome, LoadFailureReason,
    MAX_STATE_BYTES, PartialCleanup, PersistenceError, ResourceIdentity, StateValidationError,
    TaskId, TaskMetadata, TaskState, TaskStore, TimestampMillis, TransferMode,
};
use download_manager_engine::storage::{FileRange, PartialFile};
use serde_json::{Value, json};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

#[test]
fn task_ids_are_canonical_unique_v4_uuids() {
    let first = TaskId::new();
    let second = TaskId::new();
    let text = first.to_string();

    assert_ne!(first, second);
    assert_eq!(text.len(), 36);
    assert_eq!(text.as_bytes()[14], b'4');
    assert_eq!(TaskId::parse(&text), Ok(first));
    assert_eq!(
        TaskId::parse(&text.to_ascii_uppercase()),
        Err(StateValidationError::InvalidTaskId)
    );
    assert_eq!(
        TaskId::parse(&text.replace('-', "")),
        Err(StateValidationError::InvalidTaskId)
    );
    assert_eq!(
        TaskId::parse("00000000-0000-0000-0000-000000000000"),
        Err(StateValidationError::InvalidTaskId)
    );
}

#[test]
fn lifecycle_edges_are_explicit_and_failed_mutations_roll_back() {
    let fixture = TestDirectories::new("transitions");
    let mut task = TaskMetadata::new_at(
        "https://origin.example.test/file.bin",
        fixture.destination(),
        "file.bin",
        timestamp(1),
    )
    .expect("create task");
    let initial_revision = task.revision();

    assert_eq!(
        task.transition(TaskState::Downloading, timestamp(2)),
        Err(StateValidationError::InvalidTransition)
    );
    assert_eq!(task.state(), TaskState::Queued);
    assert_eq!(task.revision(), initial_revision);

    task.transition(TaskState::Probing, timestamp(2))
        .expect("start probe");
    let probing_revision = task.revision();
    assert_eq!(
        task.transition(TaskState::Downloading, timestamp(3)),
        Err(StateValidationError::InconsistentState)
    );
    assert_eq!(task.state(), TaskState::Probing);
    assert_eq!(task.revision(), probing_revision);

    assert!(TaskState::Downloading.allows(TaskState::Paused));
    assert!(TaskState::Downloading.allows(TaskState::Cancelled));
    assert!(TaskState::Failed.allows(TaskState::Queued));
    assert!(!TaskState::Completed.allows(TaskState::Queued));
    assert!(!TaskState::Cancelled.allows(TaskState::Cancelled));
    assert!(!TaskState::Failed.allows(TaskState::Failed));
}

#[test]
fn retry_revalidates_retained_partial_identity_before_resuming() {
    let fixture = TestDirectories::new("retry-identity");
    let (mut task, _partial) = downloading_task(fixture.destination(), 8);
    let retained_identity = task.resource().expect("resource identity").clone();
    task.transition(TaskState::Failed, timestamp(6))
        .expect("fail transfer");
    task.transition(TaskState::Queued, timestamp(7))
        .expect("explicit retry");
    task.transition(TaskState::Probing, timestamp(8))
        .expect("reprobe retry");

    let changed_identity = ResourceIdentity::new(
        "https://cdn.example.test/changed.bin",
        Some(8),
        Validators::default(),
        TransferMode::Segmented,
    )
    .expect("create changed identity");
    assert_eq!(
        task.apply_resource(changed_identity, timestamp(9)),
        Err(StateValidationError::InvalidResource)
    );
    assert_eq!(task.state(), TaskState::Probing);

    task.apply_resource(retained_identity, timestamp(10))
        .expect("accept matching identity");
    task.transition(TaskState::Downloading, timestamp(11))
        .expect("resume retained partial");
}

#[test]
fn metadata_boundary_rejects_credentials_invalid_validators_and_time_rollback() {
    let fixture = TestDirectories::new("metadata-validation");
    assert_eq!(
        TaskMetadata::new_at(
            "https://user:password@origin.example.test/file.bin",
            fixture.destination(),
            "file.bin",
            timestamp(10),
        ),
        Err(StateValidationError::InvalidUrl)
    );
    assert_eq!(
        ResourceIdentity::new(
            "https://cdn.example.test/file.bin",
            Some(8),
            Validators::default(),
            TransferMode::Pending,
        ),
        Err(StateValidationError::InvalidResource)
    );
    assert_eq!(
        ResourceIdentity::new(
            "https://cdn.example.test/file.bin",
            Some(8),
            Validators {
                etag: None,
                last_modified: Some("not an HTTP date".to_owned()),
            },
            TransferMode::Segmented,
        ),
        Err(StateValidationError::InvalidValidator)
    );

    let mut task = TaskMetadata::new_at(
        "https://origin.example.test/file.bin#browser-fragment",
        fixture.destination(),
        r"..\CON.txt",
        timestamp(10),
    )
    .expect("create sanitized task");
    assert_eq!(task.original_url(), "https://origin.example.test/file.bin");
    assert_eq!(task.display_name(), ".._CON.txt");
    task.transition(TaskState::Probing, timestamp(11))
        .expect("advance state");
    let revision = task.revision();
    assert_eq!(
        task.transition(TaskState::Failed, timestamp(9)),
        Err(StateValidationError::InvalidTimestamp)
    );
    assert_eq!(task.state(), TaskState::Probing);
    assert_eq!(task.revision(), revision);

    let mut unknown_length = TaskMetadata::new_at(
        "https://origin.example.test/stream",
        fixture.destination(),
        "stream.bin",
        timestamp(20),
    )
    .expect("create unknown-length task");
    unknown_length
        .transition(TaskState::Probing, timestamp(21))
        .expect("start unknown-length probe");
    unknown_length
        .apply_resource(
            ResourceIdentity::new(
                "https://cdn.example.test/stream",
                None,
                Validators::default(),
                TransferMode::Single,
            )
            .expect("record unknown-length identity"),
            timestamp(22),
        )
        .expect("apply unknown-length identity");
    assert_eq!(
        unknown_length.transition(TaskState::Downloading, timestamp(23)),
        Err(StateValidationError::InconsistentState)
    );
    assert_eq!(unknown_length.state(), TaskState::Probing);
}

#[cfg(windows)]
#[test]
fn windows_device_namespace_is_rejected_before_filesystem_creation() {
    let device_path = Path::new(r"\\.\GLOBALROOT\Device\HarddiskVolumeShadowCopy1");
    assert!(matches!(
        TaskStore::open(device_path),
        Err(PersistenceError::InvalidTask(
            StateValidationError::InvalidDestination
        ))
    ));
}

#[test]
fn round_trip_persists_identity_paths_validators_and_completed_ranges_only() {
    let fixture = TestDirectories::new("round-trip");
    let store = TaskStore::open(fixture.state()).expect("open store");
    let (mut task, partial) = downloading_task(fixture.destination(), 8);
    let mut writer = partial.assign(range(0, 4)).expect("assign prefix");
    writer.write(b"ABCD").expect("write prefix");
    writer.finish().expect("finish prefix");
    task.refresh_completed(&partial, timestamp(6))
        .expect("capture coverage");

    assert_eq!(
        store
            .checkpoint(&task, CheckpointUrgency::Critical)
            .expect("checkpoint"),
        CheckpointOutcome::Written
    );
    let bytes = fs::read(state_path(&store, task.task_id())).expect("read state");
    let text = String::from_utf8(bytes).expect("state is UTF-8 JSON");
    assert!(text.contains("https://origin.example.test/file.bin?signature=required"));
    assert!(text.contains("https://cdn.example.test/final.bin?signature=required"));
    assert!(text.contains("W/\\\"resource-v1\\\""));
    assert!(text.contains("Thu, 04 Sep 2025 10:00:00 GMT"));
    assert!(text.contains("completed_ranges"));
    assert!(!text.contains("cookies"));
    assert!(!text.contains("authorization"));
    assert!(!text.contains("headers"));
    assert!(!text.contains("referrer"));

    let report = store.load_all().expect("load state");
    assert!(report.failures().is_empty());
    assert_eq!(report.tasks(), &[task.clone()]);
    let loaded = &report.tasks()[0];
    assert_eq!(
        loaded.resource().expect("resource").expected_size(),
        Some(8)
    );
    assert_eq!(loaded.completed_ranges(), &[range(0, 4)]);
    assert_eq!(loaded.bytes_completed(), 4);

    let debug = format!("{loaded:?}");
    assert!(!debug.contains("signature=required"));
    assert!(!debug.contains("round-trip"));
    assert!(!debug.contains("payload.bin"));
    assert!(!format!("{store:?}").contains("round-trip"));
}

#[test]
fn unknown_length_recovery_restarts_at_zero_then_persists_discovered_size() {
    let fixture = TestDirectories::new("unknown-stream-recovery");
    let store = TaskStore::open(fixture.state()).expect("open store");
    let mut task = TaskMetadata::new_at(
        "https://origin.example.test/stream",
        fixture.destination(),
        "stream.bin",
        timestamp(1),
    )
    .expect("create task");
    task.transition(TaskState::Probing, timestamp(2))
        .expect("begin probe");
    task.apply_resource(
        ResourceIdentity::new(
            "https://cdn.example.test/stream",
            None,
            Validators::default(),
            TransferMode::Single,
        )
        .expect("unknown-length identity"),
        timestamp(3),
    )
    .expect("apply resource");
    let partial = PartialFile::create_streaming(fixture.destination(), "stream.bin")
        .expect("create streaming partial");
    task.attach_partial(&partial, timestamp(4))
        .expect("attach unknown-length partial");
    task.transition(TaskState::Downloading, timestamp(5))
        .expect("begin unknown-length transfer");
    let revision = task.revision();
    assert!(
        !task
            .refresh_completed(&partial, timestamp(5))
            .expect("unknown stream has no durable range before EOF")
    );
    assert_eq!(task.revision(), revision);
    store
        .checkpoint(&task, CheckpointUrgency::Critical)
        .expect("persist restartable stream");

    let mut interrupted = partial.begin_stream(16).expect("start first response");
    interrupted.write(b"stale").expect("write unproven bytes");
    drop(interrupted);
    drop(partial);

    let report = store.load_all().expect("load interrupted stream");
    assert!(report.failures().is_empty());
    let mut recovered_task = report.tasks()[0].clone();
    assert_eq!(recovered_task.bytes_completed(), 0);
    let recovered = recovered_task
        .reopen_partial()
        .expect("reopen restartable stream");
    assert_eq!(recovered.expected_len(), None);
    assert_eq!(
        fs::metadata(recovered.partial_path())
            .expect("metadata")
            .len(),
        5
    );

    let mut restarted = recovered.begin_stream(16).expect("restart from zero");
    restarted.write(b"fresh-data").expect("write replacement");
    let discovered = restarted.finish().expect("seal validated EOF");
    assert_eq!(discovered, 10);
    assert_eq!(
        fs::read(recovered.partial_path()).expect("read restarted bytes"),
        b"fresh-data"
    );
    assert!(
        recovered_task
            .refresh_completed(&recovered, timestamp(6))
            .expect("persist discovered length")
    );
    assert_eq!(
        recovered_task
            .resource()
            .expect("resolved resource")
            .expected_size(),
        Some(10)
    );
    assert_eq!(recovered_task.completed_ranges(), &[range(0, 10)]);
    recovered_task
        .transition(TaskState::Validating, timestamp(7))
        .expect("resolved stream is complete");
}

#[test]
fn recovered_partial_preserves_coverage_and_accepts_only_missing_bytes() {
    let fixture = TestDirectories::new("resume-partial");
    let store = TaskStore::open(fixture.state()).expect("open store");
    let (mut task, partial) = downloading_task(fixture.destination(), 8);
    let mut prefix = partial.assign(range(0, 4)).expect("assign prefix");
    prefix.write(b"ABCD").expect("write prefix");
    prefix.finish().expect("finish prefix");
    task.refresh_completed(&partial, timestamp(6))
        .expect("durably capture prefix");
    store
        .checkpoint(&task, CheckpointUrgency::Critical)
        .expect("save resumable state");
    drop(partial);

    let report = store.load_all().expect("recover state");
    let mut recovered_task = report.tasks()[0].clone();
    let recovered_partial = recovered_task
        .reopen_partial()
        .expect("reopen validated partial");
    assert_eq!(recovered_partial.completed_ranges(), vec![range(0, 4)]);
    assert!(recovered_partial.assign(range(0, 4)).is_err());
    let mut suffix = recovered_partial
        .assign(range(4, 8))
        .expect("assign only missing suffix");
    suffix.write(b"EFGH").expect("write suffix");
    suffix.finish().expect("finish suffix");
    recovered_task
        .refresh_completed(&recovered_partial, timestamp(7))
        .expect("durably capture full coverage");
    recovered_task
        .transition(TaskState::Validating, timestamp(8))
        .expect("validate complete coverage");
    recovered_task
        .transition(TaskState::Promoting, timestamp(9))
        .expect("start promotion");
    let partial_path = recovered_partial.partial_path().to_owned();
    let mut promotion = recovered_partial.promote().expect("publish recovered file");
    recovered_task
        .record_promotion(&promotion, timestamp(10))
        .expect("record recoverable promotion");
    store
        .checkpoint(&recovered_task, CheckpointUrgency::Critical)
        .expect("checkpoint both publication links");
    assert!(partial_path.exists());
    promotion
        .cleanup_partial()
        .expect("remove checkpointed partial link");
    recovered_task
        .record_promotion(&promotion, timestamp(11))
        .expect("record partial cleanup");
    store
        .checkpoint(&recovered_task, CheckpointUrgency::Critical)
        .expect("checkpoint publication cleanup");
    assert!(!partial_path.exists());
    recovered_task
        .transition(TaskState::Completed, timestamp(12))
        .expect("complete recovered task");
    assert_eq!(
        fs::read(promotion.final_path()).expect("read recovered output"),
        b"ABCDEFGH"
    );
}

#[test]
fn critical_checkpoints_atomically_replace_complete_records() {
    let fixture = TestDirectories::new("replacement");
    let store = TaskStore::open(fixture.state()).expect("open store");
    let mut task = TaskMetadata::new_at(
        "https://origin.example.test/replacement.bin",
        fixture.destination(),
        "replacement.bin",
        timestamp(1),
    )
    .expect("create task");

    assert_eq!(
        store
            .checkpoint(&task, CheckpointUrgency::Critical)
            .expect("initial checkpoint"),
        CheckpointOutcome::Written
    );
    task.transition(TaskState::Probing, timestamp(2))
        .expect("transition");
    assert_eq!(
        store
            .checkpoint(&task, CheckpointUrgency::Critical)
            .expect("replacement checkpoint"),
        CheckpointOutcome::Written
    );

    let raw = fs::read(state_path(&store, task.task_id())).expect("read replaced record");
    let value: Value = serde_json::from_slice(&raw).expect("record remains complete JSON");
    assert_eq!(value["task"]["state"], "probing");
    assert_eq!(value["task"]["revision"], task.revision());
    assert_eq!(count_temporary_files(&store), 0);
}

#[test]
fn routine_progress_is_coalesced_but_critical_state_is_immediate() {
    let fixture = TestDirectories::new("cadence");
    let policy = CheckpointPolicy::new(Duration::from_secs(60)).expect("valid policy");
    let store = TaskStore::open_with_policy(fixture.state(), policy).expect("open store");
    let mut task = TaskMetadata::new_at(
        "https://origin.example.test/cadence.bin",
        fixture.destination(),
        "cadence.bin",
        timestamp(1),
    )
    .expect("create task");

    assert_eq!(
        store
            .checkpoint(&task, CheckpointUrgency::Critical)
            .expect("initial checkpoint"),
        CheckpointOutcome::Written
    );
    task.transition(TaskState::Probing, timestamp(2))
        .expect("transition");
    assert_eq!(
        store
            .checkpoint(&task, CheckpointUrgency::Progress)
            .expect("coalesced checkpoint"),
        CheckpointOutcome::Deferred
    );
    assert_eq!(
        store.load_all().expect("load old state").tasks()[0].state(),
        TaskState::Queued
    );
    assert_eq!(
        store
            .checkpoint(&task, CheckpointUrgency::Critical)
            .expect("critical checkpoint"),
        CheckpointOutcome::Written
    );
    assert_eq!(
        store
            .checkpoint(&task, CheckpointUrgency::Critical)
            .expect("same revision"),
        CheckpointOutcome::Unchanged
    );
    assert_eq!(
        store.load_all().expect("load new state").tasks()[0].state(),
        TaskState::Probing
    );

    assert_eq!(
        CheckpointPolicy::new(Duration::from_millis(99)),
        Err(PersistenceError::InvalidCheckpointPolicy)
    );
    assert_eq!(
        CheckpointPolicy::new(Duration::from_secs(61)),
        Err(PersistenceError::InvalidCheckpointPolicy)
    );
}

#[test]
fn unopened_store_refuses_to_overwrite_newer_durable_revision() {
    let fixture = TestDirectories::new("stale-revision");
    let stale;
    let current;
    {
        let store = TaskStore::open(fixture.state()).expect("open first store");
        let mut task = TaskMetadata::new_at(
            "https://origin.example.test/stale.bin",
            fixture.destination(),
            "stale.bin",
            timestamp(1),
        )
        .expect("create task");
        store
            .checkpoint(&task, CheckpointUrgency::Critical)
            .expect("save revision one");
        stale = task.clone();
        task.transition(TaskState::Probing, timestamp(2))
            .expect("advance task");
        store
            .checkpoint(&task, CheckpointUrgency::Critical)
            .expect("save revision two");
        current = task;
    }

    let reopened = TaskStore::open(fixture.state()).expect("reopen without loading");
    assert_eq!(
        reopened.checkpoint(&stale, CheckpointUrgency::Critical),
        Err(PersistenceError::StaleRevision)
    );
    assert_eq!(
        reopened
            .checkpoint(&current, CheckpointUrgency::Critical)
            .expect("recognize current revision"),
        CheckpointOutcome::Unchanged
    );
}

#[test]
fn corrupt_unknown_and_future_records_fail_independently_and_remain_on_disk() {
    let fixture = TestDirectories::new("hostile-state");
    let store = TaskStore::open(fixture.state()).expect("open store");
    let task = TaskMetadata::new_at(
        "https://origin.example.test/valid.bin",
        fixture.destination(),
        "valid.bin",
        timestamp(1),
    )
    .expect("create task");
    store
        .checkpoint(&task, CheckpointUrgency::Critical)
        .expect("save valid task");
    let valid_bytes = fs::read(state_path(&store, task.task_id())).expect("read valid bytes");

    let malformed_id = TaskId::new();
    fs::write(state_path(&store, malformed_id), b"{").expect("write malformed");

    let duplicate_id = TaskId::new();
    let duplicate = String::from_utf8(valid_bytes.clone())
        .expect("valid UTF-8 state")
        .replace(&task.task_id().to_string(), &duplicate_id.to_string())
        .replacen("\"version\":1", "\"version\":1,\"version\":1", 1);
    fs::write(state_path(&store, duplicate_id), duplicate).expect("write duplicate field state");

    let future_id = TaskId::new();
    let mut future: Value = serde_json::from_slice(&valid_bytes).expect("parse valid state");
    future["version"] = json!(2);
    future["task"]["task_id"] = json!(future_id.to_string());
    fs::write(
        state_path(&store, future_id),
        serde_json::to_vec(&future).expect("encode future state"),
    )
    .expect("write future state");

    let unknown_id = TaskId::new();
    let mut unknown: Value = serde_json::from_slice(&valid_bytes).expect("parse valid state");
    unknown["task"]["task_id"] = json!(unknown_id.to_string());
    unknown["task"]["unexpected"] = json!(true);
    fs::write(
        state_path(&store, unknown_id),
        serde_json::to_vec(&unknown).expect("encode unknown state"),
    )
    .expect("write unknown state");

    let format_id = TaskId::new();
    let mut wrong_format: Value = serde_json::from_slice(&valid_bytes).expect("parse valid state");
    wrong_format["format"] = json!("some-other-application");
    wrong_format["task"]["task_id"] = json!(format_id.to_string());
    fs::write(
        state_path(&store, format_id),
        serde_json::to_vec(&wrong_format).expect("encode unknown format"),
    )
    .expect("write unknown format");

    let mismatch_id = TaskId::new();
    fs::write(state_path(&store, mismatch_id), &valid_bytes).expect("write ID mismatch");

    let oversized_id = TaskId::new();
    fs::write(
        state_path(&store, oversized_id),
        vec![b' '; MAX_STATE_BYTES + 1],
    )
    .expect("write oversized state");

    let unsafe_type_id = TaskId::new();
    fs::create_dir(state_path(&store, unsafe_type_id)).expect("write unsafe state entry");

    let report = store.load_all().expect("load mixed state");
    assert_eq!(report.tasks(), std::slice::from_ref(&task));
    assert_failure(&report, malformed_id, &LoadFailureReason::Malformed);
    assert_failure(&report, duplicate_id, &LoadFailureReason::Malformed);
    assert_failure(
        &report,
        future_id,
        &LoadFailureReason::IncompatibleVersion { found: 2 },
    );
    assert_failure(&report, unknown_id, &LoadFailureReason::Malformed);
    assert_failure(&report, format_id, &LoadFailureReason::UnknownFormat);
    assert_failure(&report, mismatch_id, &LoadFailureReason::TaskIdMismatch);
    assert_failure(&report, oversized_id, &LoadFailureReason::TooLarge);
    assert_failure(&report, unsafe_type_id, &LoadFailureReason::UnsafeFileType);
    assert!(state_path(&store, malformed_id).exists());
    assert!(state_path(&store, future_id).exists());

    let mut valid_replacement = task.clone();
    valid_replacement
        .transition(TaskState::Probing, timestamp(2))
        .expect("advance valid replacement");
    fs::write(
        state_path(&store, task.task_id()),
        b"corrupt current record",
    )
    .expect("corrupt current record");
    store.load_all().expect("observe current corruption");
    assert_eq!(
        store.checkpoint(&valid_replacement, CheckpointUrgency::Critical),
        Err(PersistenceError::ExistingStateInvalid)
    );
    assert_eq!(
        fs::read(state_path(&store, task.task_id())).expect("corrupt record retained"),
        b"corrupt current record"
    );
}

#[test]
fn missing_and_wrong_length_partials_are_not_returned_for_resume() {
    let fixture = TestDirectories::new("partial-validation");
    let store = TaskStore::open(fixture.state()).expect("open store");
    let (missing_task, missing_partial) = downloading_task(fixture.destination(), 8);
    let (short_task, short_partial) = downloading_task(fixture.destination(), 8);
    store
        .checkpoint(&missing_task, CheckpointUrgency::Critical)
        .expect("save missing candidate");
    store
        .checkpoint(&short_task, CheckpointUrgency::Critical)
        .expect("save short candidate");
    let missing_path = missing_partial.partial_path().to_owned();
    let short_path = short_partial.partial_path().to_owned();
    drop(missing_partial);
    drop(short_partial);
    fs::remove_file(missing_path).expect("remove partial");
    OpenOptions::new()
        .write(true)
        .open(short_path)
        .expect("open short partial")
        .set_len(3)
        .expect("truncate partial");

    let report = store.load_all().expect("load invalid partials");
    assert!(report.tasks().is_empty());
    assert_failure(
        &report,
        missing_task.task_id(),
        &LoadFailureReason::PartialMissing,
    );
    assert_failure(
        &report,
        short_task.task_id(),
        &LoadFailureReason::PartialLengthMismatch {
            expected: 8,
            actual: 3,
        },
    );
}

#[test]
fn destination_replacement_never_redirects_recovered_partial_work() {
    let fixture = TestDirectories::new("destination-replacement");
    let store = TaskStore::open(fixture.state()).expect("open store");
    let (task, partial) = downloading_task(fixture.destination(), 8);
    store
        .checkpoint(&task, CheckpointUrgency::Critical)
        .expect("save recoverable task");
    drop(partial);

    let displaced = fixture
        .destination()
        .parent()
        .expect("fixture parent")
        .join("displaced-destination");
    fs::rename(fixture.destination(), &displaced).expect("displace destination");
    let unavailable = store.load_all().expect("diagnose absent destination");
    assert_failure(
        &unavailable,
        task.task_id(),
        &LoadFailureReason::DestinationUnavailable,
    );

    fs::create_dir(fixture.destination()).expect("replace destination pathname");
    let replaced = store.load_all().expect("diagnose replaced destination");
    assert!(replaced.tasks().is_empty());
    assert_failure(
        &replaced,
        task.task_id(),
        &LoadFailureReason::PartialMissing,
    );
}

#[test]
fn reserved_store_lock_rejects_an_unsafe_filesystem_entry() {
    let fixture = TestDirectories::new("unsafe-store-lock");
    fs::create_dir_all(fixture.state()).expect("create state root");
    fs::create_dir(fixture.state().join(".task-store.lock")).expect("create unsafe lock directory");
    assert!(matches!(
        TaskStore::open(fixture.state()),
        Err(PersistenceError::UnsafeStoreLayout)
    ));
}

#[test]
fn store_lock_prevents_two_helper_owners_and_releases_on_drop() {
    let fixture = TestDirectories::new("store-lock");
    let first = TaskStore::open(fixture.state()).expect("open first owner");
    assert!(matches!(
        TaskStore::open(fixture.state()),
        Err(PersistenceError::StoreLocked)
    ));
    drop(first);
    TaskStore::open(fixture.state()).expect("lock released after drop");
}

#[test]
fn completed_cleanup_removes_only_metadata_and_never_final_output() {
    let fixture = TestDirectories::new("completed-cleanup");
    let store = TaskStore::open(fixture.state()).expect("open store");
    let (task, final_path) = completed_task(fixture.destination());
    store
        .checkpoint(&task, CheckpointUrgency::Critical)
        .expect("save completed task");

    assert_eq!(
        store
            .cleanup_terminal(&task, PartialCleanup::Keep)
            .expect("clean completed history"),
        CleanupOutcome::Removed
    );
    assert!(!state_path(&store, task.task_id()).exists());
    assert!(final_path.exists());
    assert_eq!(fs::read(final_path).expect("read final"), b"COMPLETE");

    let (mut retained, partial) = downloading_task(fixture.destination(), 8);
    let mut writer = partial.assign(range(0, 8)).expect("assign retained file");
    writer.write(b"RETAINED").expect("write retained file");
    writer.finish().expect("finish retained file");
    retained
        .refresh_completed(&partial, timestamp(6))
        .expect("capture retained coverage");
    retained
        .transition(TaskState::Validating, timestamp(7))
        .expect("validate retained file");
    retained
        .transition(TaskState::Promoting, timestamp(8))
        .expect("promote retained file");
    let promotion = partial.promote().expect("publish retained file");
    let retained_partial = promotion
        .partial_path()
        .expect("retained partial")
        .to_owned();
    let retained_final = promotion.final_path().to_owned();
    retained
        .record_promotion(&promotion, timestamp(9))
        .expect("record retained links");
    retained
        .transition(TaskState::Completed, timestamp(10))
        .expect("complete retained task");
    store
        .checkpoint(&retained, CheckpointUrgency::Critical)
        .expect("save completed task with redundant partial");
    assert_eq!(
        store
            .cleanup_terminal(&retained, PartialCleanup::Keep)
            .expect("keep redundant partial"),
        CleanupOutcome::Retained
    );
    assert!(retained_partial.exists());
    drop(partial);
    assert_eq!(
        store
            .cleanup_terminal(&retained, PartialCleanup::Delete)
            .expect("delete redundant partial and history"),
        CleanupOutcome::Removed
    );
    assert!(!retained_partial.exists());
    assert!(retained_final.exists());
    assert_eq!(
        fs::read(retained_final).expect("read retained final"),
        b"RETAINED"
    );
}

#[test]
fn abandoned_cleanup_requires_explicit_partial_deletion() {
    let fixture = TestDirectories::new("abandoned-cleanup");
    let store = TaskStore::open(fixture.state()).expect("open store");
    let (mut task, partial) = downloading_task(fixture.destination(), 8);
    task.transition(TaskState::Failed, timestamp(7))
        .expect("fail task");
    store
        .checkpoint(&task, CheckpointUrgency::Critical)
        .expect("save failed task");
    let partial_path = partial.partial_path().to_owned();

    assert_eq!(
        store
            .cleanup_terminal(&task, PartialCleanup::Keep)
            .expect("retain abandoned task"),
        CleanupOutcome::Retained
    );
    assert!(partial_path.exists());
    assert!(state_path(&store, task.task_id()).exists());
    drop(partial);
    assert_eq!(
        store
            .cleanup_terminal(&task, PartialCleanup::Delete)
            .expect("delete abandoned task"),
        CleanupOutcome::Removed
    );
    assert!(!partial_path.exists());
    assert!(!state_path(&store, task.task_id()).exists());

    let live = TaskMetadata::new_at(
        "https://origin.example.test/live.bin",
        fixture.destination(),
        "live.bin",
        timestamp(1),
    )
    .expect("create live task");
    assert_eq!(
        store.cleanup_terminal(&live, PartialCleanup::Delete),
        Err(PersistenceError::CleanupRequiresTerminal)
    );
}

#[test]
fn stale_temp_cleanup_is_narrow_and_does_not_touch_unknown_files() {
    let fixture = TestDirectories::new("temp-cleanup");
    let store = TaskStore::open(fixture.state()).expect("open store");
    let tasks = store.root().join("tasks");
    let stale = tasks.join(format!(
        "{}.task.json.tmp-deadbeef-0000000000000001-00",
        TaskId::new()
    ));
    let unknown = tasks.join(format!("{}.task.json.tmp-do-not-delete", TaskId::new()));
    fs::write(&stale, b"complete but uncommitted").expect("write stale temp");
    fs::write(&unknown, b"unknown file").expect("write unknown temp-like file");

    assert_eq!(store.cleanup_stale_temps().expect("clean stale temps"), 1);
    assert!(!stale.exists());
    assert!(unknown.exists());

    let startup_stale = tasks.join(format!(
        "{}.task.json.tmp-cafe-0000000000000002-01",
        TaskId::new()
    ));
    fs::write(&startup_stale, b"uncommitted").expect("write startup stale temp");
    drop(store);
    TaskStore::open(fixture.state()).expect("reopen and clean stale temp");
    assert!(!startup_stale.exists());
    assert!(unknown.exists());
}

#[test]
fn publication_recovery_requires_present_same_file_links() {
    let fixture = TestDirectories::new("publication-identity");
    let store = TaskStore::open(fixture.state()).expect("open store");
    let (mut task, partial) = downloading_task(fixture.destination(), 8);
    let mut writer = partial.assign(range(0, 8)).expect("assign full file");
    writer.write(b"ORIGINAL").expect("write full file");
    writer.finish().expect("finish full file");
    task.refresh_completed(&partial, timestamp(6))
        .expect("capture full coverage");
    task.transition(TaskState::Validating, timestamp(7))
        .expect("start validation");
    task.transition(TaskState::Promoting, timestamp(8))
        .expect("start promotion");
    let promotion = partial.promote().expect("publish final hard link");
    let final_path = promotion.final_path().to_owned();
    task.record_promotion(&promotion, timestamp(9))
        .expect("record both publication links");
    store
        .checkpoint(&task, CheckpointUrgency::Critical)
        .expect("checkpoint publication boundary");

    let intact = store.load_all().expect("load intact publication");
    assert_eq!(intact.tasks().len(), 1);
    assert!(intact.failures().is_empty());

    fs::remove_file(&final_path).expect("unlink published name");
    fs::write(&final_path, b"DIFFERNT").expect("replace with unrelated same-length file");
    let replaced = store.load_all().expect("diagnose replaced publication");
    assert!(replaced.tasks().is_empty());
    assert_failure(
        &replaced,
        task.task_id(),
        &LoadFailureReason::PublicationIdentityMismatch,
    );

    fs::remove_file(&final_path).expect("remove replacement");
    let missing = store.load_all().expect("diagnose missing final");
    assert!(missing.tasks().is_empty());
    assert_failure(&missing, task.task_id(), &LoadFailureReason::FinalMissing);
}

#[test]
fn completed_range_tampering_is_rejected_before_resume() {
    let fixture = TestDirectories::new("range-tamper");
    let store = TaskStore::open(fixture.state()).expect("open store");
    let (task, _partial) = downloading_task(fixture.destination(), 8);
    store
        .checkpoint(&task, CheckpointUrgency::Critical)
        .expect("save task");
    let path = state_path(&store, task.task_id());
    let mut value: Value =
        serde_json::from_slice(&fs::read(&path).expect("read state")).expect("parse state");
    value["task"]["completed_ranges"] = json!([
        {"start": 0, "end": 5},
        {"start": 4, "end": 8}
    ]);
    fs::write(&path, serde_json::to_vec(&value).expect("encode tamper")).expect("write tamper");

    let report = store.load_all().expect("load tampered state");
    assert!(report.tasks().is_empty());
    assert_failure(
        &report,
        task.task_id(),
        &LoadFailureReason::InvalidTask(StateValidationError::InvalidCompletedRanges),
    );
}

fn downloading_task(destination: &Path, size: u64) -> (TaskMetadata, PartialFile) {
    let mut task = TaskMetadata::new_at(
        "https://origin.example.test/file.bin?signature=required",
        destination,
        "payload.bin",
        timestamp(1),
    )
    .expect("create task");
    task.transition(TaskState::Probing, timestamp(2))
        .expect("start probing");
    let validators = Validators {
        etag: Some(EntityTag::parse("W/\"resource-v1\"").expect("valid etag")),
        last_modified: Some("Thu, 04 Sep 2025 10:00:00 GMT".to_owned()),
    };
    let resource = ResourceIdentity::new(
        "https://cdn.example.test/final.bin?signature=required",
        Some(size),
        validators,
        TransferMode::Segmented,
    )
    .expect("create resource identity");
    task.apply_resource(resource, timestamp(3))
        .expect("record resource");
    let partial = PartialFile::create(destination, "payload.bin", size).expect("create partial");
    task.attach_partial(&partial, timestamp(4))
        .expect("attach partial");
    task.transition(TaskState::Downloading, timestamp(5))
        .expect("start downloading");
    (task, partial)
}

fn completed_task(destination: &Path) -> (TaskMetadata, PathBuf) {
    let (mut task, partial) = downloading_task(destination, 8);
    let mut writer = partial.assign(range(0, 8)).expect("assign full file");
    writer.write(b"COMPLETE").expect("write full file");
    writer.finish().expect("finish full file");
    task.refresh_completed(&partial, timestamp(6))
        .expect("capture complete coverage");
    task.transition(TaskState::Validating, timestamp(7))
        .expect("start validation");
    task.transition(TaskState::Promoting, timestamp(8))
        .expect("start promotion");
    let mut promotion = partial.promote().expect("publish complete file");
    let final_path = promotion.final_path().to_owned();
    task.record_promotion(&promotion, timestamp(9))
        .expect("record recoverable publication");
    promotion
        .cleanup_partial()
        .expect("remove redundant partial in in-memory setup");
    task.record_promotion(&promotion, timestamp(10))
        .expect("record partial cleanup");
    task.transition(TaskState::Completed, timestamp(11))
        .expect("complete task");
    (task, final_path)
}

fn assert_failure(
    report: &download_manager_engine::persistence::LoadReport,
    task_id: TaskId,
    expected: &LoadFailureReason,
) {
    assert!(
        report
            .failures()
            .iter()
            .any(|failure| failure.task_id() == Some(task_id) && failure.reason() == expected),
        "missing failure {task_id}: {expected}; actual: {:?}",
        report.failures()
    );
}

fn count_temporary_files(store: &TaskStore) -> usize {
    fs::read_dir(store.root().join("tasks"))
        .expect("read task directory")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.contains(".task.json.tmp-"))
        })
        .count()
}

fn state_path(store: &TaskStore, task_id: TaskId) -> PathBuf {
    store
        .root()
        .join("tasks")
        .join(format!("{task_id}.task.json"))
}

fn range(start: u64, end: u64) -> FileRange {
    FileRange::new(start, end).expect("valid test range")
}

fn timestamp(value: u64) -> TimestampMillis {
    TimestampMillis::new(value).expect("valid test timestamp")
}

struct TestDirectories {
    root: PathBuf,
    destination: PathBuf,
    state: PathBuf,
}

impl TestDirectories {
    fn new(label: &str) -> Self {
        let counter = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "download-manager-state-{label}-{}-{timestamp}-{counter}",
            std::process::id()
        ));
        let destination = root.join("downloads with spaces");
        let state = root.join("private state");
        fs::create_dir_all(&destination).expect("create destination");
        Self {
            root,
            destination,
            state,
        }
    }

    fn destination(&self) -> &Path {
        &self.destination
    }

    fn state(&self) -> &Path {
        &self.state
    }
}

impl Drop for TestDirectories {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
