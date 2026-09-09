use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use download_manager_engine::network::{ProbeClient, Validators};
use download_manager_engine::persistence::{
    CheckpointUrgency, ResourceIdentity, TaskMetadata, TaskState, TaskStore, TimestampMillis,
    TransferMode,
};
use download_manager_engine::progress::ProgressPolicy;
use download_manager_engine::scheduler::{
    ConcurrencyLimits, DownloadScheduler, SchedulerOptions, WorkerCount,
};
use download_manager_engine::storage::{FileRange, PartialFile};
use download_manager_engine::task::{
    CancelPartialPolicy, RetryPolicy, TaskEngine, TaskEngineError, TaskEngineOptions,
    TaskEventKind, TaskFailureKind, TaskSubscription,
};
use download_manager_test_server::{
    BadRange, ByteRange, Fault, FaultRule, Fixture, RequestSelector, ServerConfig, TestServer,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);
const MIB: u64 = 1024 * 1024;
// Deadlock containment, not a durable-control latency SLO. Retry-wait readiness
// has a deterministic never-expiring-timer unit test independent of disk I/O.
const CONTROL_TEST_DEADLINE: Duration = Duration::from_secs(15);

#[test]
fn start_without_a_tokio_runtime_fails_without_mutating_queued_state() {
    let directories = TestDirectories::new("runtime-unavailable");
    let engine = TaskEngine::open(directories.state(), TaskEngineOptions::default())
        .expect("open task engine");
    let task = engine
        .create_task_default(
            "https://origin.example.test/file.bin",
            directories.destination(),
            "runtime.bin",
        )
        .expect("create task");
    assert_eq!(
        engine.start(task.task_id()),
        Err(TaskEngineError::RuntimeUnavailable)
    );
    assert_eq!(
        engine
            .snapshot(task.task_id())
            .expect("queued snapshot")
            .state(),
        TaskState::Queued
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn task_completes_with_full_snapshot_and_collision_free_publication() {
    let fixture = Fixture {
        len: 2 * MIB,
        seed: 101,
    };
    let server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: Vec::new(),
    })
    .expect("start server");
    let directories = TestDirectories::new("complete");
    let existing = directories.destination().join("managed.bin");
    fs::write(&existing, b"existing final").expect("create existing final");
    let engine = TaskEngine::open(directories.state(), TaskEngineOptions::default())
        .expect("open task engine");
    let queued = engine
        .create_task(
            &server.url("/fixture"),
            directories.destination(),
            "managed.bin",
            WorkerCount::Four,
        )
        .expect("create task");
    assert_eq!(queued.state(), TaskState::Queued);
    let debug = format!("{queued:?} {engine:?}");
    assert!(!debug.contains("managed.bin"));
    assert!(!debug.contains(&directories.root.to_string_lossy().into_owned()));
    assert!(!debug.contains("/fixture"));

    let probing = engine.start(queued.task_id()).expect("start task");
    assert_eq!(probing.state(), TaskState::Probing);
    let completed = tokio::time::timeout(
        Duration::from_secs(10),
        engine.wait_until_inactive(queued.task_id()),
    )
    .await
    .expect("task timeout")
    .expect("wait for task");
    assert_eq!(completed.state(), TaskState::Completed);
    assert_eq!(completed.bytes_completed(), fixture.len);
    assert_eq!(completed.expected_size(), Some(fixture.len));
    assert_eq!(completed.eta_seconds(), Some(0));
    assert_eq!(completed.active_workers(), 0);

    let metadata = engine.metadata(queued.task_id()).expect("metadata");
    assert!(metadata.partial_path().is_none());
    let final_path = metadata.final_path().expect("published final");
    assert_ne!(final_path, existing);
    assert_eq!(
        fs::read(&existing).expect("read existing final"),
        b"existing final"
    );
    assert_eq!(
        fs::read(final_path).expect("read final"),
        fixture.bytes(0, usize::try_from(fixture.len).expect("fixture fits"), 0)
    );
    assert_eq!(
        engine.snapshot(queued.task_id()).expect("snapshot"),
        completed
    );
    assert_eq!(
        engine
            .cancel(queued.task_id(), CancelPartialPolicy::Delete)
            .await,
        Err(TaskEngineError::InvalidTaskState)
    );
    assert!(final_path.exists());

    let mut saw_completed = false;
    while let Some(event) = engine.try_next_event().expect("next event") {
        if matches!(event.kind(), TaskEventKind::Completed(task) if task.task_id() == queued.task_id())
        {
            saw_completed = true;
        }
    }
    assert!(saw_completed);
    assert!(!engine.events_require_snapshot());
    assert_eq!(engine.remove(queued.task_id(), false), Ok(queued.task_id()));
    assert_eq!(
        engine.snapshot(queued.task_id()),
        Err(TaskEngineError::TaskNotFound)
    );
    assert!(final_path.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn startup_normalizes_interrupted_downloading_to_durable_pause() {
    let fixture = Fixture {
        len: 2 * MIB,
        seed: 111,
    };
    let server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: Vec::new(),
    })
    .expect("start server");
    let directories = TestDirectories::new("startup-normalization");
    let probe = ProbeClient::new()
        .expect("probe client")
        .probe(&server.url("/fixture"))
        .await
        .expect("probe fixture");
    let store = TaskStore::open(directories.state()).expect("open task store");
    let mut metadata = TaskMetadata::new_with_workers(
        &server.url("/fixture"),
        directories.destination(),
        "interrupted.bin",
        2,
    )
    .expect("create metadata");
    let timestamp = TimestampMillis::now().expect("current timestamp");
    metadata
        .transition(TaskState::Probing, timestamp)
        .expect("start probing");
    metadata
        .apply_resource(
            ResourceIdentity::from_probe(&probe).expect("resource identity"),
            timestamp,
        )
        .expect("apply resource");
    let partial = PartialFile::create(directories.destination(), "interrupted.bin", fixture.len)
        .expect("create partial");
    metadata
        .attach_partial(&partial, timestamp)
        .expect("attach partial");
    metadata
        .transition(TaskState::Downloading, timestamp)
        .expect("start downloading");
    let mut writer = partial
        .assign(FileRange::new(0, MIB).expect("prefix range"))
        .expect("assign prefix");
    writer
        .write(&fixture.bytes(0, usize::try_from(MIB).expect("prefix fits"), 0))
        .expect("write prefix");
    writer.finish().expect("finish prefix");
    metadata
        .refresh_completed(&partial, timestamp)
        .expect("refresh prefix");
    store
        .checkpoint(&metadata, CheckpointUrgency::Critical)
        .expect("checkpoint interrupted state");
    let task_id = metadata.task_id();
    drop(partial);
    drop(store);

    let request_boundary = server.requests().len();
    let engine = TaskEngine::open(directories.state(), TaskEngineOptions::default())
        .expect("open recovered engine");
    assert_eq!(engine.recovery_report().normalized_tasks(), &[task_id]);
    let paused = engine.snapshot(task_id).expect("paused snapshot");
    assert_eq!(paused.state(), TaskState::Paused);
    assert_eq!(paused.bytes_completed(), MIB);
    assert_eq!(paused.workers(), WorkerCount::Two);
    engine.resume(task_id).await.expect("resume recovered task");
    assert_eq!(
        engine
            .wait_until_inactive(task_id)
            .await
            .expect("wait recovered task")
            .state(),
        TaskState::Completed
    );
    assert!(
        server
            .requests()
            .into_iter()
            .skip(request_boundary)
            .filter_map(|request| request.range)
            .filter(|range| range.start != range.end)
            .all(|range| range.start >= MIB)
    );
}

#[test]
fn startup_forgets_terminal_coverage_when_its_partial_was_already_deleted() {
    let directories = TestDirectories::new("terminal-missing-partial");
    let store = TaskStore::open(directories.state()).expect("open task store");
    let mut metadata = TaskMetadata::new_at(
        "https://origin.example.test/file.bin",
        directories.destination(),
        "missing.bin",
        TimestampMillis::now().expect("timestamp"),
    )
    .expect("create metadata");
    let timestamp = TimestampMillis::now().expect("timestamp");
    metadata
        .transition(TaskState::Probing, timestamp)
        .expect("start probing");
    metadata
        .apply_resource(
            ResourceIdentity::new(
                "https://origin.example.test/file.bin",
                Some(4),
                Validators::default(),
                TransferMode::Segmented,
            )
            .expect("resource identity"),
            timestamp,
        )
        .expect("apply resource");
    let partial =
        PartialFile::create(directories.destination(), "missing.bin", 4).expect("create partial");
    metadata
        .attach_partial(&partial, timestamp)
        .expect("attach partial");
    metadata
        .transition(TaskState::Downloading, timestamp)
        .expect("start transfer");
    let mut writer = partial
        .assign(FileRange::new(0, 4).expect("range"))
        .expect("assign bytes");
    writer.write(b"data").expect("write bytes");
    writer.finish().expect("finish bytes");
    metadata
        .refresh_completed(&partial, timestamp)
        .expect("refresh bytes");
    metadata
        .transition(TaskState::Failed, timestamp)
        .expect("fail task");
    store
        .checkpoint(&metadata, CheckpointUrgency::Critical)
        .expect("checkpoint failed task");
    let task_id = metadata.task_id();
    let partial_path = metadata.partial_path().expect("partial path").to_path_buf();
    drop(partial);
    fs::remove_file(partial_path).expect("simulate completed deletion");
    drop(store);

    let engine = TaskEngine::open(directories.state(), TaskEngineOptions::default())
        .expect("open recovered engine");
    let snapshot = engine.snapshot(task_id).expect("recovered snapshot");
    assert_eq!(snapshot.state(), TaskState::Failed);
    assert_eq!(snapshot.bytes_completed(), 0);
    assert!(
        engine
            .metadata(task_id)
            .expect("recovered metadata")
            .partial_path()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn pause_checkpoints_retained_ranges_and_resume_requests_only_missing_bytes() {
    let fixture = Fixture {
        len: 8 * MIB,
        seed: 102,
    };
    let server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: None,
                range: None,
            },
            fault: Fault::Stall(Duration::from_millis(100)),
        }],
    })
    .expect("start server");
    let directories = TestDirectories::new("pause-resume");
    let engine = TaskEngine::open(directories.state(), TaskEngineOptions::default())
        .expect("open task engine");
    let task = engine
        .create_task(
            &server.url("/fixture"),
            directories.destination(),
            "paused.bin",
            WorkerCount::Two,
        )
        .expect("create task");
    engine.start(task.task_id()).expect("start task");

    wait_for_progress(&engine, task.task_id(), MIB).await;
    let paused = engine.pause(task.task_id()).await.expect("pause task");
    assert_eq!(paused.state(), TaskState::Paused);
    assert_eq!(paused.active_workers(), 0);
    assert!(paused.bytes_completed() >= MIB);
    assert!(paused.bytes_completed() < fixture.len);
    let retained_ranges = engine
        .metadata(task.task_id())
        .expect("paused metadata")
        .completed_ranges()
        .to_vec();
    assert!(!retained_ranges.is_empty());
    let request_boundary = server.requests().len();
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(server.requests().len(), request_boundary);

    drop(engine);
    let engine = TaskEngine::open(directories.state(), TaskEngineOptions::default())
        .expect("reopen paused task engine");
    let recovered = engine.snapshot(task.task_id()).expect("recovered snapshot");
    assert_eq!(recovered.state(), TaskState::Paused);
    assert_eq!(recovered.bytes_completed(), paused.bytes_completed());
    assert_eq!(recovered.workers(), WorkerCount::Two);
    assert!(engine.recovery_report().normalized_tasks().is_empty());

    let resumed = engine.resume(task.task_id()).await.expect("resume task");
    assert_ne!(resumed.state(), TaskState::Paused);
    let completed = tokio::time::timeout(
        Duration::from_secs(10),
        engine.wait_until_inactive(task.task_id()),
    )
    .await
    .expect("resume timeout")
    .expect("wait resumed task");
    assert_eq!(completed.state(), TaskState::Completed);
    for request in server.requests().into_iter().skip(request_boundary) {
        if let Some(range) = request.range
            && range.start != range.end
        {
            assert!(
                retained_ranges
                    .iter()
                    .all(|retained| range.end < retained.start() || range.start >= retained.end())
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_stops_network_and_persists_recoverable_state() {
    let fixture = Fixture {
        len: 8 * MIB,
        seed: 115,
    };
    let server = TestServer::start(ServerConfig {
        fixture,
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: None,
                range: None,
            },
            fault: Fault::Stall(Duration::from_millis(100)),
        }],
    })
    .expect("start shutdown server");
    let directories = TestDirectories::new("shutdown");
    let engine = TaskEngine::open(directories.state(), TaskEngineOptions::default())
        .expect("open task engine");
    let task = engine
        .create_task(
            &server.url("/fixture"),
            directories.destination(),
            "shutdown.bin",
            WorkerCount::Four,
        )
        .expect("create task");
    engine.start(task.task_id()).expect("start task");
    wait_for_progress(&engine, task.task_id(), MIB).await;

    let snapshots = tokio::time::timeout(Duration::from_secs(2), engine.shutdown())
        .await
        .expect("shutdown timeout")
        .expect("shutdown engine");
    let paused = snapshots
        .iter()
        .find(|snapshot| snapshot.task_id() == task.task_id())
        .expect("shutdown snapshot");
    assert_eq!(paused.state(), TaskState::Paused);
    assert!(paused.bytes_completed() >= MIB);
    let request_boundary = server.requests().len();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(server.requests().len(), request_boundary);

    drop(engine);
    let recovered = TaskEngine::open(directories.state(), TaskEngineOptions::default())
        .expect("reopen task engine")
        .snapshot(task.task_id())
        .expect("recovered snapshot");
    assert_eq!(recovered.state(), TaskState::Paused);
    assert_eq!(recovered.bytes_completed(), paused.bytes_completed());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_interrupts_probe_backoff_without_marking_user_cancellation() {
    let fixture = Fixture {
        len: 128 * 1024,
        seed: 116,
    };
    let server = TestServer::start(ServerConfig {
        fixture,
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: None,
                range: Some(ByteRange::new(0, 0).expect("probe range")),
            },
            fault: Fault::Status {
                code: 503,
                retry_after_seconds: Some(10),
            },
        }],
    })
    .expect("start retry server");
    let directories = TestDirectories::new("shutdown-probe");
    let engine =
        TaskEngine::open(directories.state(), long_retry_options()).expect("open task engine");
    let task = engine
        .create_task_default(
            &server.url("/fixture"),
            directories.destination(),
            "shutdown-probe.bin",
        )
        .expect("create task");
    engine.start(task.task_id()).expect("start task");
    wait_for_retry_event(&engine, task.task_id()).await;

    let snapshots = tokio::time::timeout(CONTROL_TEST_DEADLINE, engine.shutdown())
        .await
        .expect("shutdown acknowledgement deadline")
        .expect("shutdown engine");
    let failed = snapshots
        .iter()
        .find(|snapshot| snapshot.task_id() == task.task_id())
        .expect("failed snapshot");
    assert_eq!(failed.state(), TaskState::Failed);
    assert_eq!(
        failed.failure().expect("shutdown reason").kind(),
        TaskFailureKind::State
    );
    assert_eq!(server.requests().len(), 1);
    assert_checkpoint_state(directories.state(), task.task_id(), "failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bounded_event_overflow_requires_but_does_not_replace_full_snapshots() {
    let directories = TestDirectories::new("event-overflow");
    let options = TaskEngineOptions::new(
        WorkerCount::Four,
        RetryPolicy::default(),
        ProgressPolicy::default(),
        64,
    )
    .expect("task options");
    let engine = TaskEngine::open(directories.state(), options).expect("open task engine");
    let mut task_ids = Vec::new();
    for index in 0..70 {
        let task = engine
            .create_task_default(
                "https://origin.example.test/file.bin",
                directories.destination(),
                &format!("queued-{index}.bin"),
            )
            .expect("create queued task");
        task_ids.push(task.task_id());
        engine
            .cancel(task.task_id(), CancelPartialPolicy::Keep)
            .await
            .expect("cancel queued task");
    }

    assert!(engine.events_require_snapshot());
    let snapshots = engine.snapshots();
    assert_eq!(snapshots.len(), task_ids.len());
    assert!(
        snapshots
            .iter()
            .all(|snapshot| snapshot.state() == TaskState::Cancelled)
    );
    assert_eq!(drain_events(&engine).len(), 64);
    assert_eq!(
        engine
            .take_overflow_snapshot()
            .expect("overflow snapshot")
            .len(),
        task_ids.len()
    );
    assert!(!engine.events_require_snapshot());
    assert!(engine.take_overflow_snapshot().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn cancellation_applies_explicit_keep_and_delete_partial_policies() {
    for (label, policy) in [
        ("keep", CancelPartialPolicy::Keep),
        ("delete", CancelPartialPolicy::Delete),
    ] {
        let fixture = Fixture {
            len: 8 * MIB,
            seed: 103,
        };
        let server = TestServer::start(ServerConfig {
            fixture,
            rules: vec![FaultRule {
                selector: RequestSelector {
                    path: Some("/fixture".to_owned()),
                    request_number: None,
                    range: None,
                },
                fault: Fault::Stall(Duration::from_millis(100)),
            }],
        })
        .expect("start server");
        let directories = TestDirectories::new(label);
        let engine = TaskEngine::open(directories.state(), TaskEngineOptions::default())
            .expect("open task engine");
        let task = engine
            .create_task(
                &server.url("/fixture"),
                directories.destination(),
                "cancelled.bin",
                WorkerCount::Four,
            )
            .expect("create task");
        engine.start(task.task_id()).expect("start task");
        wait_for_progress(&engine, task.task_id(), MIB).await;
        let partial_before = engine
            .metadata(task.task_id())
            .expect("metadata before cancel")
            .partial_path()
            .expect("partial before cancel")
            .to_owned();

        let cancelled = engine
            .cancel(task.task_id(), policy)
            .await
            .expect("cancel task");
        assert_eq!(cancelled.state(), TaskState::Cancelled);
        assert_eq!(
            cancelled.failure().expect("cancel reason").kind(),
            TaskFailureKind::Cancelled
        );
        assert_eq!(cancelled.active_workers(), 0);
        let metadata = engine.metadata(task.task_id()).expect("cancel metadata");
        match policy {
            CancelPartialPolicy::Keep => {
                assert_eq!(metadata.partial_path(), Some(partial_before.as_path()));
                assert!(partial_before.exists());
                assert!(!metadata.completed_ranges().is_empty());
            }
            CancelPartialPolicy::Delete => {
                assert!(metadata.partial_path().is_none());
                assert!(!partial_before.exists());
                assert!(metadata.completed_ranges().is_empty());
                assert_eq!(cancelled.bytes_completed(), 0);
            }
        }
        assert!(metadata.final_path().is_none());
        if policy == CancelPartialPolicy::Keep {
            assert_eq!(
                engine.remove(task.task_id(), false),
                Err(TaskEngineError::PartialRetained)
            );
            assert_eq!(engine.remove(task.task_id(), true), Ok(task.task_id()));
            assert!(!partial_before.exists());
        } else {
            assert_eq!(engine.remove(task.task_id(), false), Ok(task.task_id()));
        }
        assert_eq!(
            engine.snapshot(task.task_id()),
            Err(TaskEngineError::TaskNotFound)
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn routine_progress_eventually_flushes_deferred_ranges_during_a_stall() {
    let fixture = Fixture {
        len: 8 * 1024 * 1024,
        seed: 113,
    };
    let server = TestServer::start(ServerConfig {
        fixture,
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: None,
                range: Some(ByteRange {
                    start: 2 * MIB,
                    end: 4 * MIB - 1,
                }),
            },
            fault: Fault::Stall(Duration::from_secs(3)),
        }],
    })
    .expect("start stalled transfer server");
    let directories = TestDirectories::new("periodic-checkpoint");
    let engine = TaskEngine::open(
        directories.state(),
        TaskEngineOptions::new(
            WorkerCount::One,
            RetryPolicy::default(),
            ProgressPolicy::new(Duration::from_millis(100), Duration::from_secs(2))
                .expect("progress policy"),
            256,
        )
        .expect("task options"),
    )
    .expect("open task engine");
    let task = engine
        .create_task(
            &server.url("/fixture"),
            directories.destination(),
            "checkpointed.bin",
            WorkerCount::One,
        )
        .expect("create task");
    engine.start(task.task_id()).expect("start task");

    let progress_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if engine
            .snapshot(task.task_id())
            .expect("snapshot")
            .bytes_completed()
            >= 2 * 1024 * 1024
        {
            break;
        }
        assert!(
            Instant::now() < progress_deadline,
            "first range never completed: snapshot={:?}, requests={:?}",
            engine
                .snapshot(task.task_id())
                .expect("diagnostic snapshot"),
            server.requests()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let record = directories
        .state()
        .join("tasks")
        .join(format!("{}.task.json", task.task_id()));
    let checkpoint_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&record).expect("read task record"))
                .expect("parse task record");
        if persisted["task"]["completed_ranges"]
            .as_array()
            .is_some_and(|ranges| !ranges.is_empty())
        {
            break;
        }
        assert!(
            Instant::now() < checkpoint_deadline,
            "deferred completed range was not checkpointed"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        engine.snapshot(task.task_id()).expect("snapshot").state(),
        TaskState::Downloading
    );
    let paused = engine.pause(task.task_id()).await.expect("pause transfer");
    assert_eq!(paused.state(), TaskState::Paused);
}

fn progress_fixture(seed: u8) -> (Fixture, TestServer) {
    let fixture = Fixture { len: 8 * MIB, seed };
    let server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: Vec::new(),
    })
    .expect("start progress fixture");
    (fixture, server)
}

// Read the independently published watch snapshot, not the managed-state mutex:
// a slow critical checkpoint may still own that mutex when a deadline fires.
// No URL, filename, path, validator, task ID or credential is formatted here.
fn progress_observation(subscription: &TaskSubscription, started: Instant) -> String {
    let snapshot = subscription.latest();
    format!(
        "elapsed_ms={} published_state={:?} bytes={} expected={:?} active={} speed={:?} failure={:?}",
        started.elapsed().as_millis(),
        snapshot.state(),
        snapshot.bytes_completed(),
        snapshot.expected_size(),
        snapshot.active_workers(),
        snapshot.speed_bytes_per_second(),
        snapshot
            .failure()
            .map(download_manager_engine::task::TaskFailure::kind),
    )
}

#[test]
fn progress_diagnostics_omit_sensitive_snapshot_fields() {
    let directories = TestDirectories::new("diagnostic-canary44");
    let engine = TaskEngine::open(directories.state(), TaskEngineOptions::default())
        .expect("open diagnostic fixture");
    let task = engine
        .create_task_default(
            "https://diagnostic44.example.test/private-path?token=synthetic44",
            directories.destination(),
            "private-name44.bin",
        )
        .expect("create diagnostic task without starting network");
    let subscription = engine.subscribe(task.task_id()).expect("subscribe");
    let text = progress_observation(&subscription, Instant::now());
    assert!(text.len() <= 256);
    assert!(
        text.contains(
            "published_state=Queued bytes=0 expected=None active=0 speed=None failure=None"
        )
    );
    for forbidden in [
        "diagnostic44",
        "private-path",
        "synthetic44",
        "private-name44",
        "diagnostic-canary44",
        &task.task_id().to_string(),
        &directories.root.to_string_lossy(),
    ] {
        assert!(!text.contains(forbidden));
    }
}

fn assert_progress_samples(
    progress_events: &[(TimestampMillis, download_manager_engine::task::TaskProgress)],
    size: u64,
    elapsed: Duration,
) {
    assert!(
        progress_events
            .windows(2)
            .all(|samples| samples[0].1.bytes_completed() <= samples[1].1.bytes_completed())
    );
    let ordinary: Vec<_> = progress_events
        .iter()
        .filter(|(_, sample)| sample.bytes_completed() < size)
        .collect();
    assert!(
        ordinary
            .windows(2)
            .all(|samples| { samples[1].0.get().saturating_sub(samples[0].0.get()) >= 80 })
    );
    let generous_maximum =
        usize::try_from(elapsed.as_millis() / 100).expect("test duration fits usize") + 3;
    assert!(progress_events.len() <= generous_maximum);
}

// Workflow containment is not a progress-frequency or transfer-latency SLO.
// A single accepted request may take 30s; preparation, joined disk validation and
// promotion do not belong to the active-cadence observation window. The latter
// retains its 10s missing-events watchdog after independent prefix/gate readiness.
const PROGRESS_WORKFLOW_LIMIT: Duration = Duration::from_secs(60);
const CADENCE_OBSERVATION_LIMIT: Duration = Duration::from_secs(10);

async fn prove_preparation_outlasts_old_deadline(
    engine: &TaskEngine,
    subscription: &TaskSubscription,
    pause: Option<download_manager_test_server::ObservationPause>,
    started: Instant,
) {
    let Some(pause) = pause else { return };
    tokio::time::timeout(Duration::from_secs(5), async {
        while !pause.wait_for_pending(1, Duration::ZERO) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("controlled probe did not arrive");
    // The connected probe is allowed 30s by the real client. Keep it pending
    // across the original 10s whole-task budget; do not consume any task event.
    let task_id = subscription.latest().task_id();
    assert!(
        tokio::time::timeout(
            CADENCE_OBSERVATION_LIMIT,
            engine.wait_until_inactive(task_id),
        )
        .await
        .is_err(),
        "withheld preparation must outlast the old whole-task deadline",
    );
    let snapshot = subscription.latest();
    assert_eq!(snapshot.state(), TaskState::Probing);
    assert_eq!(snapshot.bytes_completed(), 0);
    assert_eq!(snapshot.failure(), None);
    eprintln!(
        "controlled preparation at old deadline: {}",
        progress_observation(subscription, started)
    );
    drop(pause);
}

async fn wait_for_cadence_readiness(
    subscription: &mut TaskSubscription,
    pause: &download_manager_test_server::ResponsePause,
    deadline: tokio::time::Instant,
    started: Instant,
) {
    let prepared = tokio::time::timeout_at(deadline, async {
        loop {
            let snapshot = subscription.latest();
            assert_ne!(
                snapshot.state(),
                TaskState::Failed,
                "preparation failed; {}",
                progress_observation(subscription, started)
            );
            if snapshot.bytes_completed() == 2 * MIB && pause.wait_for_pending(1, Duration::ZERO) {
                break;
            }
            subscription
                .changed()
                .await
                .expect("preparation watch closed");
        }
    })
    .await;
    assert!(
        prepared.is_ok(),
        "preparation workflow deadline; {}",
        progress_observation(subscription, started)
    );
}

async fn observe_active_cadence(
    engine: &TaskEngine,
    subscription: &TaskSubscription,
    pause: download_manager_test_server::ResponsePause,
    workflow_deadline: tokio::time::Instant,
    started: Instant,
) -> Option<u64> {
    let observation_started = Instant::now();
    let cadence_deadline = tokio::time::Instant::now() + CADENCE_OBSERVATION_LIMIT;
    let mut pause = Some(pause);
    let mut held_samples = 0;
    let mut first_prefix_ms = None;
    let mut released_ms = None;
    let mut last_transition = None;
    let mut validation_rate = None;
    let mut progress_events = Vec::new();
    let completion = tokio::time::timeout_at(workflow_deadline, async {
        loop {
            let next = engine.next_event();
            let event =
                if pause.is_some() {
                    tokio::time::timeout_at(cadence_deadline, next).await.unwrap_or_else(|_| panic!(
                    "active cadence observation deadline; held_samples={held_samples}; {}",
                    progress_observation(subscription, started),
                ))
                } else {
                    next.await
                }
                .expect("next task event");
            match event.kind() {
                TaskEventKind::Progress(progress) => {
                    progress_events.push((event.emitted_at(), *progress));
                    if pause.is_some() && progress.bytes_completed() == 2 * MIB {
                        first_prefix_ms.get_or_insert_with(|| started.elapsed().as_millis());
                        held_samples += 1;
                        if held_samples >= 4
                            && progress.speed_bytes_per_second().is_some()
                            && pause
                                .as_ref()
                                .is_some_and(|guard| guard.wait_for_pending(1, Duration::ZERO))
                        {
                            released_ms = Some(started.elapsed().as_millis());
                            drop(pause.take());
                        }
                    }
                }
                TaskEventKind::Completed(task) => {
                    assert_eq!(Some(task.speed_bytes_per_second()), validation_rate,
                        "validation/promotion must preserve the final transfer estimate, including None");
                    break;
                }
                TaskEventKind::StateChanged { task, .. } => {
                    if task.state() == TaskState::Validating {
                        validation_rate = Some(task.speed_bytes_per_second());
                    }
                    last_transition = Some((task.state(), started.elapsed().as_millis()));
                }
                TaskEventKind::Failed { failure, .. } => panic!(
                    "progress fixture failed: {:?}; {}",
                    failure.kind(),
                    progress_observation(subscription, started),
                ),
                TaskEventKind::RetryScheduled(_) => {}
            }
        }
    })
    .await;
    let gate_pending = pause
        .as_ref()
        .is_some_and(|guard| guard.wait_for_pending(1, Duration::ZERO));
    let observation = format!(
        "{} events={} held_samples={held_samples} first_prefix_ms={first_prefix_ms:?} \
         released_ms={released_ms:?} gate_pending={gate_pending} last_transition={last_transition:?}",
        progress_observation(subscription, started),
        progress_events.len(),
    );
    assert!(
        completion.is_ok(),
        "completion workflow deadline; {observation}"
    );
    eprintln!("progress cadence observation: {observation}");
    assert!(
        pause.is_none(),
        "completion must follow explicit response release"
    );
    assert!(held_samples >= 4);
    // Preparation time cannot inflate the permitted ordinary-event count.
    assert_progress_samples(&progress_events, 8 * MIB, observation_started.elapsed());
    validation_rate.expect("validation phase must be observed")
}

async fn assert_cadence_fixture(delay_preparation: bool) {
    let (fixture, server) = progress_fixture(105);
    let preparation = delay_preparation.then(|| server.pause_observation());
    let pause = server
        .pause_responses(RequestSelector {
            path: Some("/fixture".to_owned()),
            request_number: None,
            range: Some(ByteRange {
                start: 2 * MIB,
                end: 4 * MIB - 1,
            }),
        })
        .expect("pause second response");
    let directories = TestDirectories::new("progress-events");
    // Keep observed active samples eligible throughout this test's watchdog.
    // The one-second stale-window semantics have independent deterministic tests.
    let progress = ProgressPolicy::new(Duration::from_millis(100), CADENCE_OBSERVATION_LIMIT)
        .expect("progress policy");
    let options = TaskEngineOptions::new(WorkerCount::One, RetryPolicy::default(), progress, 256)
        .expect("task options");
    let engine = TaskEngine::open(directories.state(), options).expect("open task engine");
    let task = engine
        .create_task(
            &server.url("/fixture"),
            directories.destination(),
            "progress.bin",
            WorkerCount::One,
        )
        .expect("create task");
    let mut subscription = engine
        .subscribe(task.task_id())
        .expect("diagnostic subscription");
    let delay_subscription = engine
        .subscribe(task.task_id())
        .expect("delay subscription");
    let started = Instant::now();
    let deadline = tokio::time::Instant::now() + PROGRESS_WORKFLOW_LIMIT;
    engine.start(task.task_id()).expect("start task");
    let ((), final_rate) = tokio::join!(
        prove_preparation_outlasts_old_deadline(&engine, &delay_subscription, preparation, started),
        async {
            wait_for_cadence_readiness(&mut subscription, &pause, deadline, started).await;
            observe_active_cadence(&engine, &subscription, pause, deadline, started).await
        },
    );
    let snapshot = engine.snapshot(task.task_id()).expect("latest snapshot");
    assert_eq!(snapshot.state(), TaskState::Completed);
    assert_eq!(snapshot.bytes_completed(), fixture.len);
    assert_eq!(snapshot.expected_size(), Some(fixture.len));
    assert_eq!(snapshot.speed_bytes_per_second(), final_rate);
    assert_eq!(snapshot.active_workers(), 0);
    assert_eq!(snapshot.eta_seconds(), Some(0));
    assert_eq!(
        fs::read(directories.destination().join("progress.bin")).expect("read published bytes"),
        fixture.bytes(0, usize::try_from(fixture.len).expect("fixture fits"), 0),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_events_are_rate_limited_while_snapshots_remain_complete() {
    assert_cadence_fixture(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn preparation_can_outlast_old_whole_task_deadline_without_losing_cadence() {
    assert_cadence_fixture(true).await;
}

async fn assert_late_consumer_fixture(delay_preparation: bool) {
    let (fixture, server) = progress_fixture(139);
    let preparation = delay_preparation.then(|| server.pause_observation());
    let directories = TestDirectories::new("late-progress-consumer");
    let engine = TaskEngine::open(directories.state(), TaskEngineOptions::default())
        .expect("open task engine");
    let task = engine
        .create_task_default(
            &server.url("/fixture"),
            directories.destination(),
            "coalesced.bin",
        )
        .expect("create task");
    let subscription = engine
        .subscribe(task.task_id())
        .expect("diagnostic subscription");
    let started = Instant::now();
    let deadline = tokio::time::Instant::now() + PROGRESS_WORKFLOW_LIMIT;
    engine.start(task.task_id()).expect("start task");
    prove_preparation_outlasts_old_deadline(&engine, &subscription, preparation, started).await;
    // Consume no events until inactivity, including during controlled preparation.
    let completed = tokio::time::timeout_at(deadline, engine.wait_until_inactive(task.task_id()))
        .await
        .unwrap_or_else(|_| {
            panic!(
                "late consumer workflow deadline; {}",
                progress_observation(&subscription, started)
            )
        })
        .expect("wait for task");
    eprintln!(
        "late consumer completion: {}",
        progress_observation(&subscription, started)
    );
    assert_eq!(completed.state(), TaskState::Completed);
    assert_eq!(completed.bytes_completed(), fixture.len);
    let mut samples = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = engine.next_event().await.expect("next event");
            match event.kind() {
                TaskEventKind::Progress(progress) => samples.push(*progress),
                TaskEventKind::Completed(snapshot) => {
                    assert_eq!(snapshot.bytes_completed(), fixture.len);
                    break;
                }
                TaskEventKind::Failed { .. } => panic!("fixture unexpectedly failed"),
                _ => {}
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "late consumer event timeout; samples={}; {}",
            samples.len(),
            progress_observation(&subscription, started)
        )
    });
    assert!(samples.len() <= 1);
    for sample in samples {
        assert!(sample.bytes_completed() <= fixture.len);
        assert_eq!(sample.expected_size(), Some(fixture.len));
        assert!(sample.active_workers() <= 4);
    }
    assert_eq!(completed.expected_size(), Some(fixture.len));
    assert_eq!(completed.active_workers(), 0);
    assert_eq!(completed.eta_seconds(), Some(0));
    assert!(!engine.events_require_snapshot());
    assert_eq!(
        fs::read(directories.destination().join("coalesced.bin")).expect("read published bytes"),
        fixture.bytes(0, usize::try_from(fixture.len).expect("fixture fits"), 0),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_events_coalesce_for_a_late_consumer_without_losing_final_state() {
    assert_late_consumer_fixture(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn preparation_can_outlast_old_whole_task_deadline_without_losing_coalescing() {
    assert_late_consumer_fixture(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_size_progress_omits_eta_until_bounded_clean_eof() {
    let fixture = Fixture {
        len: 2 * MIB,
        seed: 106,
    };
    let server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: Vec::new(),
    })
    .expect("start unknown-size server");
    let directories = TestDirectories::new("unknown-progress");
    let progress = ProgressPolicy::new(Duration::from_millis(100), Duration::from_secs(1))
        .expect("progress policy");
    let options = TaskEngineOptions::new(WorkerCount::Four, RetryPolicy::default(), progress, 256)
        .expect("task options");
    let engine = TaskEngine::open(directories.state(), options).expect("open task engine");
    let task = engine
        .create_task(
            &server.url("/unknown-length"),
            directories.destination(),
            "unknown.bin",
            WorkerCount::Four,
        )
        .expect("create task");
    engine.start(task.task_id()).expect("start task");

    let mut saw_unknown_snapshot = false;
    let mut saw_unknown_progress = false;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = engine.next_event().await.expect("next task event");
            match event.kind() {
                TaskEventKind::StateChanged { task, .. }
                    if task.state() == TaskState::Downloading
                        && task.expected_size().is_none()
                        && task.eta_seconds().is_none() =>
                {
                    saw_unknown_snapshot = true;
                }
                TaskEventKind::Progress(progress)
                    if progress.expected_size().is_none() && progress.eta_seconds().is_none() =>
                {
                    saw_unknown_progress = true;
                }
                TaskEventKind::Completed(_) => break,
                _ => {}
            }
        }
    })
    .await
    .expect("unknown stream timeout");

    assert!(saw_unknown_snapshot);
    assert!(saw_unknown_progress);
    let completed = engine.snapshot(task.task_id()).expect("completed snapshot");
    assert_eq!(completed.state(), TaskState::Completed);
    assert_eq!(completed.expected_size(), Some(fixture.len));
    assert_eq!(completed.bytes_completed(), fixture.len);
    assert_eq!(completed.eta_seconds(), Some(0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_interrupts_transfer_retry_sleep() {
    let fixture = Fixture {
        len: 128 * 1024,
        seed: 107,
    };
    let full = ByteRange::new(0, fixture.len - 1).expect("full range");
    let options = long_retry_options();

    let transfer_server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: Some(3),
                range: Some(full),
            },
            fault: Fault::Status {
                code: 503,
                retry_after_seconds: Some(10),
            },
        }],
    })
    .expect("start transfer retry server");
    let pause_directories = TestDirectories::new("pause-retry-sleep");
    let pause_engine =
        TaskEngine::open(pause_directories.state(), options).expect("open pause engine");
    let pause_task = pause_engine
        .create_task(
            &transfer_server.url("/fixture"),
            pause_directories.destination(),
            "pause-retry.bin",
            WorkerCount::Four,
        )
        .expect("create pause task");
    pause_engine
        .start(pause_task.task_id())
        .expect("start pause task");
    wait_for_retry_event(&pause_engine, pause_task.task_id()).await;
    let paused = tokio::time::timeout(
        CONTROL_TEST_DEADLINE,
        pause_engine.pause(pause_task.task_id()),
    )
    .await
    .expect("pause acknowledgement deadline")
    .expect("pause task");
    assert_eq!(paused.state(), TaskState::Paused);
    assert_checkpoint_state(pause_directories.state(), pause_task.task_id(), "paused");
    assert_eq!(transfer_server.requests().len(), 3);
    pause_engine
        .resume(pause_task.task_id())
        .await
        .expect("resume after retry sleep");
    assert_eq!(
        pause_engine
            .wait_until_inactive(pause_task.task_id())
            .await
            .expect("wait resumed task")
            .state(),
        TaskState::Completed
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_interrupts_probe_retry_sleep() {
    let fixture = Fixture {
        len: 128 * 1024,
        seed: 109,
    };
    let options = long_retry_options();
    let probe_server = TestServer::start(ServerConfig {
        fixture,
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: Some(1),
                range: Some(ByteRange::new(0, 0).expect("probe range")),
            },
            fault: Fault::Status {
                code: 503,
                retry_after_seconds: Some(10),
            },
        }],
    })
    .expect("start probe retry server");
    let cancel_directories = TestDirectories::new("cancel-probe-sleep");
    let cancel_engine =
        TaskEngine::open(cancel_directories.state(), options).expect("open cancel engine");
    let cancel_task = cancel_engine
        .create_task(
            &probe_server.url("/fixture"),
            cancel_directories.destination(),
            "cancel-probe.bin",
            WorkerCount::Four,
        )
        .expect("create cancel task");
    cancel_engine
        .start(cancel_task.task_id())
        .expect("start cancel task");
    wait_for_retry_event(&cancel_engine, cancel_task.task_id()).await;
    let cancelled = tokio::time::timeout(
        CONTROL_TEST_DEADLINE,
        cancel_engine.cancel(cancel_task.task_id(), CancelPartialPolicy::Keep),
    )
    .await
    .expect("cancel acknowledgement deadline")
    .expect("cancel probing task");
    assert_eq!(cancelled.state(), TaskState::Cancelled);
    assert_checkpoint_state(
        cancel_directories.state(),
        cancel_task.task_id(),
        "cancelled",
    );
    assert_eq!(probe_server.requests().len(), 1);
    assert!(
        cancel_engine
            .metadata(cancel_task.task_id())
            .expect("cancelled metadata")
            .partial_path()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retry_budget_exhaustion_is_terminal_and_never_becomes_unbounded() {
    let fixture = Fixture {
        len: 128 * 1024,
        seed: 108,
    };
    let full = ByteRange::new(0, fixture.len - 1).expect("full range");
    let server = TestServer::start(ServerConfig {
        fixture,
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: None,
                range: Some(full),
            },
            fault: Fault::Status {
                code: 503,
                retry_after_seconds: None,
            },
        }],
    })
    .expect("start retry exhaustion server");
    let directories = TestDirectories::new("retry-exhaustion");
    let retry = RetryPolicy::new(
        2,
        Duration::from_millis(10),
        Duration::from_millis(10),
        Duration::from_secs(2),
    )
    .expect("retry policy");
    let options = TaskEngineOptions::new(WorkerCount::Four, retry, ProgressPolicy::default(), 256)
        .expect("task options");
    let engine = TaskEngine::open(directories.state(), options).expect("open task engine");
    let task = engine
        .create_task(
            &server.url("/fixture"),
            directories.destination(),
            "exhausted.bin",
            WorkerCount::Four,
        )
        .expect("create task");
    engine.start(task.task_id()).expect("start task");
    let failed = engine
        .wait_until_inactive(task.task_id())
        .await
        .expect("wait exhausted task");
    assert_eq!(failed.state(), TaskState::Failed);
    assert_eq!(
        failed.failure().expect("failure reason").kind(),
        TaskFailureKind::RetryExhausted
    );
    assert_eq!(server.requests().len(), 5);
    let retries = drain_events(&engine)
        .into_iter()
        .filter(|event| matches!(event.kind(), TaskEventKind::RetryScheduled(_)))
        .count();
    assert_eq!(retries, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probe_and_transfer_share_one_retry_budget() {
    let fixture = Fixture {
        len: 128 * 1024,
        seed: 114,
    };
    let first = ByteRange::new(0, 0).expect("first probe range");
    let full = ByteRange::new(0, fixture.len - 1).expect("full range");
    let server = TestServer::start(ServerConfig {
        fixture,
        rules: vec![
            FaultRule {
                selector: RequestSelector {
                    path: Some("/fixture".to_owned()),
                    request_number: Some(1),
                    range: Some(first),
                },
                fault: Fault::Status {
                    code: 503,
                    retry_after_seconds: None,
                },
            },
            FaultRule {
                selector: RequestSelector {
                    path: Some("/fixture".to_owned()),
                    request_number: Some(4),
                    range: Some(full),
                },
                fault: Fault::Status {
                    code: 503,
                    retry_after_seconds: None,
                },
            },
        ],
    })
    .expect("start shared-budget server");
    let directories = TestDirectories::new("shared-retry-budget");
    let retry = RetryPolicy::new(
        1,
        Duration::from_millis(10),
        Duration::from_millis(10),
        Duration::from_secs(1),
    )
    .expect("retry policy");
    let options = TaskEngineOptions::new(WorkerCount::Four, retry, ProgressPolicy::default(), 256)
        .expect("task options");
    let engine = TaskEngine::open(directories.state(), options).expect("open task engine");
    let task = engine
        .create_task_default(
            &server.url("/fixture"),
            directories.destination(),
            "shared-budget.bin",
        )
        .expect("create task");
    engine.start(task.task_id()).expect("start task");
    let failed = engine
        .wait_until_inactive(task.task_id())
        .await
        .expect("wait shared-budget failure");
    assert_eq!(failed.state(), TaskState::Failed);
    assert_eq!(
        failed.failure().expect("exhausted failure").kind(),
        TaskFailureKind::RetryExhausted
    );
    assert_eq!(server.requests().len(), 4);
    assert_eq!(
        drain_events(&engine)
            .iter()
            .filter(|event| matches!(event.kind(), TaskEventKind::RetryScheduled(_)))
            .count(),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn explicit_retry_rejects_changed_retained_resource_identity() {
    let fixture = Fixture {
        len: 128 * 1024,
        seed: 110,
    };
    let first = ByteRange::new(0, 0).expect("first probe range");
    let last = ByteRange::new(fixture.len - 1, fixture.len - 1).expect("last probe range");
    let full = ByteRange::new(0, fixture.len - 1).expect("full transfer range");
    let server = TestServer::start(ServerConfig {
        fixture,
        rules: vec![
            FaultRule {
                selector: RequestSelector {
                    path: Some("/fixture".to_owned()),
                    request_number: Some(3),
                    range: Some(full),
                },
                fault: Fault::Status {
                    code: 403,
                    retry_after_seconds: None,
                },
            },
            FaultRule {
                selector: RequestSelector {
                    path: Some("/fixture".to_owned()),
                    request_number: Some(4),
                    range: Some(first),
                },
                fault: Fault::Generation(1),
            },
            FaultRule {
                selector: RequestSelector {
                    path: Some("/fixture".to_owned()),
                    request_number: Some(5),
                    range: Some(last),
                },
                fault: Fault::Generation(1),
            },
        ],
    })
    .expect("start changing server");
    let directories = TestDirectories::new("changed-retry-identity");
    let engine = TaskEngine::open(directories.state(), TaskEngineOptions::default())
        .expect("open task engine");
    let task = engine
        .create_task(
            &server.url("/fixture"),
            directories.destination(),
            "changed.bin",
            WorkerCount::Four,
        )
        .expect("create task");
    engine.start(task.task_id()).expect("start task");
    let first_failure = engine
        .wait_until_inactive(task.task_id())
        .await
        .expect("wait first failure");
    assert_eq!(first_failure.state(), TaskState::Failed);
    assert_eq!(
        first_failure.failure().expect("HTTP failure").kind(),
        TaskFailureKind::HttpStatus
    );
    assert!(
        engine
            .metadata(task.task_id())
            .expect("failed metadata")
            .partial_path()
            .is_some()
    );

    engine
        .resume(task.task_id())
        .await
        .expect("explicit retry through resume control");
    let changed = engine
        .wait_until_inactive(task.task_id())
        .await
        .expect("wait changed retry");
    assert_eq!(changed.state(), TaskState::Failed);
    assert_eq!(
        changed.failure().expect("identity failure").kind(),
        TaskFailureKind::ResourceChanged
    );
    assert_eq!(server.requests().len(), 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fatal_storage_limits_do_not_enter_automatic_retry() {
    let fixture = Fixture {
        len: 4096,
        seed: 112,
    };
    let server = TestServer::start(ServerConfig {
        fixture,
        rules: Vec::new(),
    })
    .expect("start unknown stream server");
    let directories = TestDirectories::new("fatal-storage");
    let scheduler_options =
        SchedulerOptions::new(ConcurrencyLimits::default(), Duration::from_secs(1), 1024)
            .expect("scheduler options");
    let scheduler = DownloadScheduler::with_options(scheduler_options).expect("scheduler");
    let engine = TaskEngine::open_with_scheduler(
        directories.state(),
        TaskEngineOptions::default(),
        scheduler,
    )
    .expect("open task engine");
    let task = engine
        .create_task_default(
            &server.url("/unknown-length"),
            directories.destination(),
            "bounded-stream.bin",
        )
        .expect("create task");
    engine.start(task.task_id()).expect("start task");
    let failed = engine
        .wait_until_inactive(task.task_id())
        .await
        .expect("wait storage failure");
    assert_eq!(failed.state(), TaskState::Failed);
    assert_eq!(
        failed.failure().expect("storage failure").kind(),
        TaskFailureKind::Storage
    );
    assert_eq!(server.requests().len(), 2);
    assert!(
        drain_events(&engine)
            .iter()
            .all(|event| !matches!(event.kind(), TaskEventKind::RetryScheduled(_)))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retries_are_bounded_respect_retry_after_and_skip_fatal_protocol_errors() {
    let fixture = Fixture {
        len: 128 * 1024,
        seed: 104,
    };
    let full = ByteRange::new(0, fixture.len - 1).expect("full range");
    let transient = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: Some(3),
                range: Some(full),
            },
            fault: Fault::Status {
                code: 503,
                retry_after_seconds: Some(1),
            },
        }],
    })
    .expect("start transient server");
    let directories = TestDirectories::new("retry-after");
    let retry = RetryPolicy::new(
        2,
        Duration::from_millis(10),
        Duration::from_millis(20),
        Duration::from_secs(2),
    )
    .expect("retry policy");
    let options = TaskEngineOptions::new(WorkerCount::Four, retry, ProgressPolicy::default(), 256)
        .expect("task options");
    let engine = TaskEngine::open(directories.state(), options).expect("open task engine");
    let task = engine
        .create_task(
            &transient.url("/fixture"),
            directories.destination(),
            "retry.bin",
            WorkerCount::Four,
        )
        .expect("create retry task");
    let started = std::time::Instant::now();
    engine.start(task.task_id()).expect("start retry task");
    let completed = engine
        .wait_until_inactive(task.task_id())
        .await
        .expect("wait retry task");
    assert_eq!(completed.state(), TaskState::Completed);
    assert!(started.elapsed() >= Duration::from_millis(900));
    assert!(transient.requests().len() >= 4);
    let retry_events: Vec<_> = drain_events(&engine)
        .into_iter()
        .filter_map(|event| match event.kind() {
            TaskEventKind::RetryScheduled(retry) => Some(*retry),
            _ => None,
        })
        .collect();
    assert_eq!(retry_events.len(), 1);
    assert_eq!(retry_events[0].retry_number(), 1);
    assert_eq!(retry_events[0].delay_millis(), 1000);
    assert_eq!(retry_events[0].http_status(), Some(503));

    let fatal = TestServer::start(ServerConfig {
        fixture,
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: Some(3),
                range: Some(full),
            },
            fault: Fault::BadContentRange(BadRange::Start),
        }],
    })
    .expect("start fatal server");
    let fatal_directories = TestDirectories::new("fatal");
    let fatal_engine =
        TaskEngine::open(fatal_directories.state(), options).expect("open fatal task engine");
    let fatal_task = fatal_engine
        .create_task(
            &fatal.url("/fixture"),
            fatal_directories.destination(),
            "fatal.bin",
            WorkerCount::Four,
        )
        .expect("create fatal task");
    fatal_engine
        .start(fatal_task.task_id())
        .expect("start fatal task");
    let failed = fatal_engine
        .wait_until_inactive(fatal_task.task_id())
        .await
        .expect("wait fatal task");
    assert_eq!(failed.state(), TaskState::Failed);
    assert_eq!(
        failed.failure().expect("fatal reason").kind(),
        TaskFailureKind::RangeResponseInvalid
    );
    assert_eq!(fatal.requests().len(), 3);
    assert!(
        drain_events(&fatal_engine)
            .iter()
            .all(|event| !matches!(event.kind(), TaskEventKind::RetryScheduled(_)))
    );
}

fn assert_checkpoint_state(
    directory: &Path,
    task_id: download_manager_engine::persistence::TaskId,
    expected: &str,
) {
    let record = directory.join("tasks").join(format!("{task_id}.task.json"));
    let bytes = fs::read(record).expect("read acknowledged critical checkpoint");
    let stored: serde_json::Value = serde_json::from_slice(&bytes).expect("decode checkpoint");
    assert_eq!(stored["task"]["state"], expected);
}

fn long_retry_options() -> TaskEngineOptions {
    let retry = RetryPolicy::new(
        2,
        Duration::from_millis(10),
        Duration::from_millis(20),
        Duration::from_secs(20),
    )
    .expect("retry policy");
    TaskEngineOptions::new(WorkerCount::Four, retry, ProgressPolicy::default(), 256)
        .expect("task options")
}

async fn wait_for_retry_event(
    engine: &TaskEngine,
    task_id: download_manager_engine::persistence::TaskId,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = engine.next_event().await.expect("next task event");
            if matches!(
                event.kind(),
                TaskEventKind::RetryScheduled(retry) if retry.task_id() == task_id
            ) {
                break;
            }
        }
    })
    .await
    .expect("retry event timeout");
}

async fn wait_for_progress(
    engine: &TaskEngine,
    task_id: download_manager_engine::persistence::TaskId,
    bytes: u64,
) {
    let mut subscription = engine.subscribe(task_id).expect("subscribe to task");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = subscription.latest();
            if snapshot.bytes_completed() >= bytes && snapshot.state() == TaskState::Downloading {
                break;
            }
            subscription
                .changed()
                .await
                .expect("task remains available");
        }
    })
    .await
    .expect("progress timeout");
}

fn drain_events(engine: &TaskEngine) -> Vec<download_manager_engine::task::TaskEvent> {
    let mut events = Vec::new();
    while let Some(event) = engine.try_next_event().expect("event dequeue") {
        events.push(event);
    }
    events
}

struct TestDirectories {
    root: PathBuf,
    state: PathBuf,
    destination: PathBuf,
}

impl TestDirectories {
    fn new(label: &str) -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "firefox-download-manager-task-{label}-{}-{nanos}-{sequence}",
            std::process::id()
        ));
        let state = root.join("state");
        let destination = root.join("downloads");
        fs::create_dir_all(&destination).expect("create test destination");
        Self {
            root,
            state,
            destination,
        }
    }

    fn state(&self) -> &Path {
        &self.state
    }

    fn destination(&self) -> &Path {
        &self.destination
    }
}

impl Drop for TestDirectories {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settings_reconfiguration_rejects_running_work_and_failure_policy_deletes_only_partial() {
    let directories = TestDirectories::new("settings-retention");
    let server = TestServer::start(ServerConfig {
        fixture: Fixture {
            len: 2 * MIB,
            seed: 71,
        },
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/retention".to_owned()),
                request_number: Some(3),
                range: None,
            },
            fault: Fault::BadContentRange(BadRange::Start),
        }],
    })
    .expect("server");
    let mut engine =
        TaskEngine::open(directories.state(), TaskEngineOptions::default()).expect("engine");
    let task = engine
        .create_task_default(
            &server.url("/stall"),
            directories.destination(),
            "retain.bin",
        )
        .expect("task");
    engine.start(task.task_id()).expect("start");
    assert!(
        engine
            .reconfigure(
                TaskEngineOptions::default(),
                DownloadScheduler::new().expect("scheduler")
            )
            .is_err()
    );
    engine
        .cancel(task.task_id(), CancelPartialPolicy::Delete)
        .await
        .expect("stop");
    // Run futures may still be returning; reconfigure fails rather than racing them.
    tokio::time::sleep(Duration::from_millis(50)).await;
    engine
        .reconfigure(
            TaskEngineOptions::default().with_failure_retention(false),
            DownloadScheduler::new().expect("scheduler"),
        )
        .expect("inactive reconfiguration");
    let task = engine
        .create_task_default(
            &server.url("/retention"),
            directories.destination(),
            "failure.bin",
        )
        .expect("failed task");
    engine.start(task.task_id()).expect("start failure");
    let done = tokio::time::timeout(
        Duration::from_secs(10),
        engine.wait_until_inactive(task.task_id()),
    )
    .await
    .expect("timeout")
    .expect("done");
    assert_eq!(done.state(), TaskState::Failed);
    assert!(
        engine
            .metadata(task.task_id())
            .expect("metadata")
            .partial_path()
            .is_none()
    );
    assert!(!directories.destination().join("failure.bin").exists());
}

#[tokio::test]
async fn recovered_weak_identity_completed_bytes_cannot_resume_or_retry() {
    for path in ["/validators/missing", "/validators/weak"] {
        let directories = TestDirectories::new("weak-resume");
        let fixture = Fixture {
            len: 4096,
            seed: 59,
        };
        let server = TestServer::start(ServerConfig {
            fixture: fixture.clone(),
            rules: Vec::new(),
        })
        .expect("server");
        let probe = ProbeClient::new()
            .expect("client")
            .probe(&server.url(path))
            .await
            .expect("probe");
        let mut metadata =
            TaskMetadata::new(&server.url(path), directories.destination(), "weak.bin")
                .expect("metadata");
        let timestamp = TimestampMillis::now().expect("time");
        metadata
            .transition(TaskState::Probing, timestamp)
            .expect("probing");
        metadata
            .apply_resource(
                ResourceIdentity::from_probe(&probe).expect("identity"),
                timestamp,
            )
            .expect("resource");
        let partial = PartialFile::create(directories.destination(), "weak.bin", fixture.len)
            .expect("partial");
        metadata
            .attach_partial(&partial, timestamp)
            .expect("attach");
        metadata
            .transition(TaskState::Downloading, timestamp)
            .expect("downloading");
        let mut writer = partial
            .assign(FileRange::new(0, fixture.len).expect("range"))
            .expect("writer");
        writer.write(&fixture.bytes(0, 4096, 0)).expect("write");
        writer.finish().expect("finish");
        metadata
            .refresh_completed(&partial, timestamp)
            .expect("flush");
        let id = metadata.task_id();
        let store = TaskStore::open(directories.state()).expect("store");
        store
            .checkpoint(&metadata, CheckpointUrgency::Critical)
            .expect("checkpoint");
        drop(store);
        drop(partial);
        let engine =
            TaskEngine::open(directories.state(), TaskEngineOptions::default()).expect("recovery");
        let result = engine.resume(id).await.expect("resume response");
        assert_eq!(result.state(), TaskState::Failed);
        assert_eq!(
            result.failure().expect("failure").kind(),
            TaskFailureKind::ResourceChanged
        );
        engine.retry(id).expect("retry");
        let result = engine.wait_until_inactive(id).await.expect("retry result");
        assert_eq!(result.state(), TaskState::Failed);
        assert_eq!(
            result.failure().expect("failure").kind(),
            TaskFailureKind::ResourceChanged
        );
        assert!(!directories.destination().join("weak.bin").exists());
    }
}

fn fixture_context(
    server: &TestServer,
    url: &str,
) -> std::sync::Arc<download_manager_engine::auth::RequestContext> {
    let input = serde_json::from_value(serde_json::json!({
        "referrer":server.url("/session/page"),"credentials":{"cookies":[{
            "name":"fixture_session","value":"not-a-real-session","domain":"127.0.0.1", "path":"/session",
            "secure":false,"http_only":true,"expires_at":null
        }]}
    })).expect("fixture context");
    download_manager_engine::auth::RequestContext::new(url, &input)
        .expect("validated fixture session")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn session_downloads_validate_final_bytes_for_every_worker_count_and_single_fallback() {
    for (workers, fallback) in [
        (WorkerCount::One, false),
        (WorkerCount::Two, false),
        (WorkerCount::Four, false),
        (WorkerCount::Eight, false),
        (WorkerCount::Four, true),
    ] {
        let fixture = Fixture {
            len: 4 * MIB,
            seed: 42,
        };
        let server = TestServer::start(ServerConfig {
            fixture: fixture.clone(),
            rules: if fallback {
                vec![FaultRule {
                    selector: RequestSelector::default(),
                    fault: Fault::IgnoreRange,
                }]
            } else {
                vec![]
            },
        })
        .expect("server");
        let directories = TestDirectories::new("session-transfer");
        let engine =
            TaskEngine::open(directories.state(), TaskEngineOptions::default()).expect("engine");
        let url = server.url("/session/signed?sig=a%2Fb%2BC&x=2&x=1");
        let task = engine
            .create_task_with_context(
                &url,
                directories.destination(),
                "session.bin",
                workers,
                Some(fixture_context(&server, &url)),
            )
            .expect("create session task");
        engine.start(task.task_id()).expect("start");
        let final_task = tokio::time::timeout(
            Duration::from_secs(15),
            engine.wait_until_inactive(task.task_id()),
        )
        .await
        .expect("bounded completion")
        .expect("snapshot");
        assert_eq!(final_task.state(), TaskState::Completed);
        assert_eq!(
            fs::read(directories.destination().join("session.bin")).expect("output"),
            fixture.bytes(0, usize::try_from(fixture.len).expect("length"), 0)
        );
        assert!(
            server
                .requests()
                .iter()
                .all(|r| r.session.fixture_valid && r.session.signed_target_valid)
        );
        let metadata = engine.metadata(task.task_id()).expect("metadata");
        assert!(metadata.needs_session());
        let bytes = fs::read(
            directories
                .state()
                .join("tasks")
                .join(format!("{}.task.json", task.task_id())),
        )
        .expect("state bytes");
        let persisted = String::from_utf8(bytes).expect("UTF8");
        assert!(!persisted.contains("not-a-real-session"));
        assert!(!persisted.contains("/session/page"));
        assert!(!persisted.contains("cookies"));
        assert!(!format!("{engine:?} {final_task:?} {metadata:?}").contains("not-a-real-session"));
        engine.shutdown().await.expect("shutdown");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recovered_session_partial_cannot_send_or_resume_without_its_memory_only_context() {
    let fixture = Fixture {
        len: 16 * MIB,
        seed: 43,
    };
    let server = TestServer::start(ServerConfig {
        fixture,
        rules: vec![FaultRule {
            selector: RequestSelector::default(),
            fault: Fault::Stall(Duration::from_millis(80)),
        }],
    })
    .expect("server");
    let directories = TestDirectories::new("session-recovery");
    let engine =
        TaskEngine::open(directories.state(), TaskEngineOptions::default()).expect("engine");
    let url = server.url("/session/fixture");
    let task = engine
        .create_task_with_context(
            &url,
            directories.destination(),
            "session.bin",
            WorkerCount::Four,
            Some(fixture_context(&server, &url)),
        )
        .expect("create");
    engine.start(task.task_id()).expect("start");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let snapshot = engine.snapshot(task.task_id()).expect("snapshot");
        if snapshot.bytes_completed() > 0 {
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    engine
        .pause(task.task_id())
        .await
        .expect("pause with retained session");
    let before = engine.metadata(task.task_id()).expect("metadata");
    assert!(before.bytes_completed() > 0);
    engine.shutdown().await.expect("shutdown");
    drop(engine);
    let recovered = TaskEngine::open(directories.state(), TaskEngineOptions::default())
        .expect("recovered engine");
    assert!(
        recovered
            .metadata(task.task_id())
            .expect("recovered marker")
            .needs_session()
    );
    let barrier = server.pause_observation();
    let failure = recovered
        .resume(task.task_id())
        .await
        .expect("authoritative failed snapshot");
    assert_eq!(failure.state(), TaskState::Failed);
    assert_eq!(
        failure.failure().expect("session required").kind(),
        TaskFailureKind::AuthRequired
    );
    assert!(!barrier.wait_for_pending(1, Duration::from_millis(100)));
    let after = recovered.metadata(task.task_id()).expect("failed metadata");
    assert_eq!(before.completed_ranges(), after.completed_ranges());
    assert!(!directories.destination().join("session.bin").exists());
    recovered.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejected_session_is_actionable_nonretrying_and_cannot_be_replaced_on_retained_bytes() {
    let server = TestServer::start(ServerConfig {
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/session/fixture".into()),
                request_number: Some(3),
                range: None,
            },
            fault: Fault::Status {
                code: 403,
                retry_after_seconds: None,
            },
        }],
        ..ServerConfig::default()
    })
    .expect("server");
    let directories = TestDirectories::new("session-expired");
    let engine =
        TaskEngine::open(directories.state(), TaskEngineOptions::default()).expect("engine");
    let url = server.url("/session/fixture");
    let task = engine
        .create_task_with_context(
            &url,
            directories.destination(),
            "session.bin",
            WorkerCount::One,
            Some(fixture_context(&server, &url)),
        )
        .expect("create");
    engine.start(task.task_id()).expect("start");
    let failed = engine
        .wait_until_inactive(task.task_id())
        .await
        .expect("failure");
    assert_eq!(failed.state(), TaskState::Failed);
    assert_eq!(
        failed.failure().expect("reason").kind(),
        TaskFailureKind::AuthExpired
    );
    assert_eq!(server.requests().len(), 3);
    assert!(!directories.destination().join("session.bin").exists());
    engine.retry(task.task_id()).expect("explicit retry");
    let failed = engine
        .wait_until_inactive(task.task_id())
        .await
        .expect("failure");
    assert_eq!(
        failed.failure().expect("reason").kind(),
        TaskFailureKind::AuthRequired
    );
    assert_eq!(server.requests().len(), 3);
    let fresh = engine
        .create_task_with_context(
            &url,
            directories.destination(),
            "fresh.bin",
            WorkerCount::One,
            Some(fixture_context(&server, &url)),
        )
        .expect("fresh handoff");
    engine.start(fresh.task_id()).expect("start fresh");
    assert_eq!(
        engine
            .wait_until_inactive(fresh.task_id())
            .await
            .expect("fresh output")
            .state(),
        TaskState::Completed
    );
    engine.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn transfer_and_new_task_probe_share_caps_and_both_cancel_waiters_safely() {
    let server = TestServer::start(ServerConfig::default()).expect("server");
    let scheduler = DownloadScheduler::with_options(
        SchedulerOptions::new(
            ConcurrencyLimits::new(1, 1).expect("limits"),
            Duration::from_secs(30),
            64 * MIB,
        )
        .expect("options"),
    )
    .expect("scheduler");
    let admission = scheduler.admission();
    let directories = TestDirectories::new("shared-probe-transfer-cap");
    let engine = TaskEngine::open_with_scheduler(
        directories.state(),
        TaskEngineOptions::default(),
        scheduler,
    )
    .expect("engine");
    // Hold one local slot so both tasks can be started deterministically before
    // any remote handler sees a request. Requests, not delayed server ledger
    // insertion, are the configured admission boundary.
    let held = admission
        .acquire(&reqwest::Url::parse(&server.url("/fixture")).expect("URL"))
        .await
        .expect("held admission");
    let first = engine
        .create_task_default(
            &server.url("/fixture"),
            directories.destination(),
            "first.bin",
        )
        .expect("first");
    let second = engine
        .create_task_default(
            &server.url("/fixture"),
            directories.destination(),
            "second.bin",
        )
        .expect("second");
    engine.start(first.task_id()).expect("start first");
    engine.start(second.task_id()).expect("start second");
    assert!(server.requests().is_empty());
    engine
        .cancel(second.task_id(), CancelPartialPolicy::Delete)
        .await
        .expect("cancel admission waiter");
    drop(held);
    assert_eq!(
        engine
            .wait_until_inactive(first.task_id())
            .await
            .expect("first completion")
            .state(),
        TaskState::Completed
    );
    assert_eq!(admission.peak(), 1);
    assert_eq!(admission.active(), 0);
    assert_eq!(
        engine
            .snapshot(second.task_id())
            .expect("cancelled")
            .state(),
        TaskState::Cancelled
    );
    assert!(!directories.destination().join("second.bin").exists());
    engine.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn transient_transfer_failures_reduce_only_effective_workers_under_one_retry_budget() {
    let server = TestServer::start(ServerConfig {
        fixture: Fixture {
            len: 16 * MIB,
            seed: 51,
        },
        rules: (3..=50)
            .map(|number| FaultRule {
                selector: RequestSelector {
                    path: Some("/fixture".into()),
                    request_number: Some(number),
                    range: None,
                },
                fault: Fault::Status {
                    code: 500,
                    retry_after_seconds: Some(0),
                },
            })
            .collect(),
    })
    .expect("server");
    let directories = TestDirectories::new("adaptive-width");
    let options = TaskEngineOptions::new(
        WorkerCount::Four,
        RetryPolicy::new(
            4,
            Duration::from_millis(10),
            Duration::from_millis(20),
            Duration::from_secs(1),
        )
        .expect("retry policy"),
        ProgressPolicy::default(),
        4096,
    )
    .expect("options");
    let engine = TaskEngine::open(directories.state(), options).expect("engine");
    let task = engine
        .create_task(
            &server.url("/fixture"),
            directories.destination(),
            "width.bin",
            WorkerCount::Eight,
        )
        .expect("task");
    engine.start(task.task_id()).expect("start");
    let mut widths = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), engine.next_event())
            .await
            .expect("event timeout")
            .expect("event");
        match event.kind() {
            TaskEventKind::RetryScheduled(retry) => {
                widths.push(retry.next_workers().expect("transfer retry width").get());
            }
            TaskEventKind::Failed { failure, .. } => {
                assert_eq!(failure.kind(), TaskFailureKind::RetryExhausted);
                break;
            }
            _ => {}
        }
    }
    let final_task = engine
        .wait_until_inactive(task.task_id())
        .await
        .expect("final");
    assert_eq!(widths, vec![4, 2, 1, 1]);
    assert_eq!(final_task.workers(), WorkerCount::Eight);
    assert_eq!(
        engine.metadata(task.task_id()).expect("metadata").workers(),
        8
    );
    assert!(!directories.destination().join("width.bin").exists());
    engine.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn worker_416_revalidates_once_and_never_merges_changed_or_repeatedly_rejected_ranges() {
    for scenario in ["unchanged", "changed", "repeated"] {
        let fixture = Fixture {
            len: 128 * 1024,
            seed: 52,
        };
        let rule = |number, fault| FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".into()),
                request_number: Some(number),
                range: None,
            },
            fault,
        };
        let mut rules = vec![rule(
            3,
            Fault::Status {
                code: 416,
                retry_after_seconds: None,
            },
        )];
        if scenario == "changed" {
            rules.extend([rule(4, Fault::Generation(1)), rule(5, Fault::Generation(1))]);
        }
        if scenario == "repeated" {
            rules.push(rule(
                6,
                Fault::Status {
                    code: 416,
                    retry_after_seconds: None,
                },
            ));
        }
        let server = TestServer::start(ServerConfig {
            fixture: fixture.clone(),
            rules,
        })
        .expect("server");
        let directories = TestDirectories::new("worker-416");
        let engine =
            TaskEngine::open(directories.state(), TaskEngineOptions::default()).expect("engine");
        let task = engine
            .create_task(
                &server.url("/fixture"),
                directories.destination(),
                "416.bin",
                WorkerCount::One,
            )
            .expect("task");
        engine.start(task.task_id()).expect("start");
        let result = engine
            .wait_until_inactive(task.task_id())
            .await
            .expect("result");
        let requests = server.requests();
        assert_eq!(
            requests[3].range,
            Some(ByteRange::new(0, 0).expect("first byte"))
        );
        assert_eq!(
            requests[4].range,
            Some(ByteRange::new(fixture.len - 1, fixture.len - 1).expect("last byte"))
        );
        if scenario == "unchanged" {
            assert_eq!(result.state(), TaskState::Completed);
            assert_eq!(requests.len(), 6);
            assert_eq!(
                fs::read(directories.destination().join("416.bin")).expect("output"),
                fixture.bytes(0, usize::try_from(fixture.len).expect("length"), 0)
            );
        } else {
            assert_eq!(result.state(), TaskState::Failed);
            assert_eq!(requests.len(), if scenario == "changed" { 5 } else { 6 });
            assert!(!directories.destination().join("416.bin").exists());
            if scenario == "changed" {
                assert_eq!(
                    result.failure().expect("reason").kind(),
                    TaskFailureKind::ResourceChanged
                );
            }
        }
        engine.shutdown().await.expect("shutdown");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn settings_reconfiguration_preserves_known_origin_cooldown() {
    let directories = TestDirectories::new("pressure-settings");
    let scheduler = DownloadScheduler::new().expect("scheduler");
    let before = scheduler.admission();
    let origin = reqwest::Url::parse("https://example.test/fixture").expect("URL");
    let permit = before.acquire(&origin).await.expect("initial");
    permit.observe(429, Some(2));
    drop(permit);
    let mut engine = TaskEngine::open_with_scheduler(
        directories.state(),
        TaskEngineOptions::default(),
        scheduler,
    )
    .expect("engine");
    let replacement = DownloadScheduler::new().expect("replacement");
    let after = replacement.admission();
    engine
        .reconfigure(TaskEngineOptions::default(), replacement)
        .expect("reconfigure");
    assert!(
        tokio::time::timeout(Duration::from_millis(30), after.acquire(&origin))
            .await
            .is_err()
    );
    assert_eq!(after.active(), 0);
    engine.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sha256_completion_covers_all_worker_counts_single_fallback_empty_and_collision() {
    use download_manager_engine::integrity::ExpectedSha256;
    let fixture = Fixture {
        len: 2 * MIB + 53,
        seed: 33,
    };
    let bytes = fixture.bytes(0, usize::try_from(fixture.len).expect("size"), 0);
    // Independent Python hashlib digest of the documented fixture formula.
    let digest = "533406bc0c38f16af711c9eee95f57379ca1c285838b27f31dabfbe4d2e930de";
    let server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: Vec::new(),
    })
    .expect("server");
    for (workers, route) in [
        (WorkerCount::One, "/fixture"),
        (WorkerCount::Two, "/fixture"),
        (WorkerCount::Four, "/fixture"),
        (WorkerCount::Eight, "/fixture"),
        (WorkerCount::Eight, "/ignore-range"),
        (WorkerCount::Four, "/unknown-length"),
        (WorkerCount::One, "/empty"),
    ] {
        let directories = TestDirectories::new("sha256-success");
        fs::write(directories.destination().join("integrity.bin"), b"existing").expect("collision");
        let engine =
            TaskEngine::open(directories.state(), TaskEngineOptions::default()).expect("engine");
        let expected = if route == "/empty" {
            ExpectedSha256::parse(
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            )
        } else {
            ExpectedSha256::parse(digest)
        };
        let task = engine
            .create_task_with_integrity(
                &server.url(route),
                directories.destination(),
                "integrity.bin",
                workers,
                None,
                expected,
            )
            .expect("create");
        engine.start(task.task_id()).expect("start");
        let completed = engine
            .wait_until_inactive(task.task_id())
            .await
            .expect("completion");
        assert_eq!(completed.state(), TaskState::Completed);
        let output =
            fs::read(directories.destination().join(completed.display_name())).expect("output");
        assert_eq!(
            output,
            if route == "/empty" {
                Vec::new()
            } else {
                bytes.clone()
            }
        );
        assert_eq!(
            completed.bytes_completed(),
            u64::try_from(output.len()).expect("size")
        );
        assert_eq!(
            fs::read(directories.destination().join("integrity.bin")).expect("collision preserved"),
            b"existing"
        );
        let events = drain_events(&engine);
        let states = events
            .iter()
            .filter_map(|event| match event.kind() {
                TaskEventKind::StateChanged { task, .. } => Some(task.state()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(states.ends_with(&[
            TaskState::Validating,
            TaskState::Promoting,
            TaskState::Completed
        ]));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event.kind(), TaskEventKind::Completed(_)))
                .count(),
            1
        );
        engine.shutdown().await.expect("shutdown");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sha256_mismatch_retention_and_restart_never_drop_the_immutable_expected_digest() {
    use download_manager_engine::integrity::ExpectedSha256;
    let server = TestServer::start(ServerConfig::default()).expect("server");
    for keep in [true, false] {
        let directories = TestDirectories::new("sha256-mismatch");
        let options = TaskEngineOptions::default().with_failure_retention(keep);
        let engine = TaskEngine::open(directories.state(), options).expect("engine");
        let expected = ExpectedSha256::parse(&"f".repeat(64));
        let task = engine
            .create_task_with_integrity(
                &server.url("/fixture"),
                directories.destination(),
                "mismatch.bin",
                WorkerCount::Four,
                None,
                expected,
            )
            .expect("create");
        engine.start(task.task_id()).expect("start");
        let failed = engine
            .wait_until_inactive(task.task_id())
            .await
            .expect("failure");
        assert_eq!(failed.state(), TaskState::Failed);
        assert_eq!(
            failed.failure().expect("reason").kind(),
            TaskFailureKind::ChecksumMismatch
        );
        assert!(!directories.destination().join("mismatch.bin").exists());
        assert!(
            !drain_events(&engine)
                .iter()
                .any(|event| matches!(event.kind(), TaskEventKind::Completed(_)))
        );
        assert_eq!(
            fs::read_dir(directories.destination())
                .expect("directory")
                .count(),
            usize::from(keep)
        );
        engine.shutdown().await.expect("shutdown");
        drop(engine);
        let state_path = directories
            .state()
            .join("tasks")
            .join(format!("{}.task.json", task.task_id()));
        let record: serde_json::Value =
            serde_json::from_slice(&fs::read(state_path).expect("state")).expect("JSON");
        assert_eq!(record["version"], 4);
        assert_eq!(record["task"]["expected_sha256"], "f".repeat(64));
        let recovered = TaskEngine::open(directories.state(), options).expect("recover");
        let before = server.requests().len();
        recovered.resume(task.task_id()).await.expect("retry");
        let retried = recovered
            .wait_until_inactive(task.task_id())
            .await
            .expect("retried");
        assert_eq!(
            retried.failure().expect("still protected").kind(),
            TaskFailureKind::ChecksumMismatch
        );
        assert!(!directories.destination().join("mismatch.bin").exists());
        if keep {
            assert_eq!(
                server.requests().len() - before,
                2,
                "retained complete coverage is reprobed and rehashed, never trusted merely by size"
            );
        }
        recovered.shutdown().await.expect("shutdown");
    }
}
