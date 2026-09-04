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
    TaskEventKind, TaskFailureKind,
};
use download_manager_test_server::{
    BadRange, ByteRange, Fault, FaultRule, Fixture, RequestSelector, ServerConfig, TestServer,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);
const MIB: u64 = 1024 * 1024;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_events_are_rate_limited_while_snapshots_remain_complete() {
    let fixture = Fixture {
        len: 8 * MIB,
        seed: 105,
    };
    let server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: None,
                range: None,
            },
            fault: Fault::Stall(Duration::from_millis(130)),
        }],
    })
    .expect("start progress server");
    let directories = TestDirectories::new("progress-events");
    let progress = ProgressPolicy::new(Duration::from_millis(100), Duration::from_secs(1))
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
    let started = std::time::Instant::now();
    engine.start(task.task_id()).expect("start task");

    let mut progress_events = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = engine.next_event().await.expect("next task event");
            match event.kind() {
                TaskEventKind::Progress(progress) => {
                    progress_events.push((event.emitted_at(), *progress));
                }
                TaskEventKind::Completed(_) => break,
                _ => {}
            }
        }
    })
    .await
    .expect("event completion timeout");

    assert!(progress_events.len() >= 4);
    assert!(
        progress_events
            .windows(2)
            .all(|samples| samples[0].1.bytes_completed() <= samples[1].1.bytes_completed())
    );
    let ordinary: Vec<_> = progress_events
        .iter()
        .filter(|(_, sample)| sample.bytes_completed() < fixture.len)
        .collect();
    assert!(
        ordinary
            .windows(2)
            .all(|samples| { samples[1].0.get().saturating_sub(samples[0].0.get()) >= 80 })
    );
    let generous_maximum =
        usize::try_from(started.elapsed().as_millis() / 100).expect("test duration fits usize") + 3;
    assert!(progress_events.len() <= generous_maximum);

    let snapshot = engine.snapshot(task.task_id()).expect("latest snapshot");
    assert_eq!(snapshot.state(), TaskState::Completed);
    assert_eq!(snapshot.bytes_completed(), fixture.len);
    assert_eq!(snapshot.expected_size(), Some(fixture.len));
    assert!(snapshot.speed_bytes_per_second().is_some());
    assert_eq!(snapshot.eta_seconds(), Some(0));
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
    let started = std::time::Instant::now();
    let paused = tokio::time::timeout(
        Duration::from_secs(1),
        pause_engine.pause(pause_task.task_id()),
    )
    .await
    .expect("pause did not interrupt retry sleep")
    .expect("pause task");
    assert_eq!(paused.state(), TaskState::Paused);
    assert!(started.elapsed() < Duration::from_secs(1));
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
        Duration::from_secs(1),
        cancel_engine.cancel(cancel_task.task_id(), CancelPartialPolicy::Keep),
    )
    .await
    .expect("cancel did not interrupt probe retry sleep")
    .expect("cancel probing task");
    assert_eq!(cancelled.state(), TaskState::Cancelled);
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
