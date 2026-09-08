use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use download_manager_engine::network::{ProbeClient, ProbeMode, RangeValidationError};
use download_manager_engine::scheduler::{
    ConcurrencyLimits, DownloadScheduler, SchedulerError, SchedulerOptions, TransferCancellation,
    TransferKind, WorkerCount, transfer_progress_channel,
};
use download_manager_engine::storage::{FileRange, PartialFile, StorageError};
use download_manager_test_server::{
    BadRange, ByteRange, Fault, FaultRule, Fixture, RequestSelector, ServerConfig, TestServer,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);
const MIB: u64 = 1024 * 1024;

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn all_supported_worker_counts_cover_once_and_produce_exact_output() {
    for workers in [
        WorkerCount::One,
        WorkerCount::Two,
        WorkerCount::Four,
        WorkerCount::Eight,
    ] {
        let fixture = Fixture {
            len: 8 * MIB,
            seed: 41 + workers.get(),
        };
        let server = TestServer::start(ServerConfig {
            fixture: fixture.clone(),
            rules: vec![FaultRule {
                selector: RequestSelector {
                    path: Some("/fixture".to_owned()),
                    request_number: None,
                    range: None,
                },
                fault: Fault::Stall(Duration::from_millis(40)),
            }],
        })
        .expect("start server");
        let probe = ProbeClient::new()
            .expect("probe client")
            .probe(&server.url("/fixture"))
            .await
            .expect("probe ranged fixture");
        assert_eq!(probe.mode(), ProbeMode::Segmented);

        let directory = TestDirectory::new(&format!("workers-{}", workers.get()));
        let partial = PartialFile::create(directory.path(), "worker output.bin", fixture.len)
            .expect("create partial");
        let scheduler = scheduler(8, 8, Duration::from_secs(1), 64 * MIB);
        let summary = scheduler
            .transfer(&probe, &partial, workers)
            .await
            .expect("complete segmented transfer");
        assert_eq!(summary.kind(), TransferKind::Segmented);
        assert_eq!(summary.bytes_written(), fixture.len);
        assert_eq!(summary.workers_used(), workers.get());
        assert_eq!(summary.hedged_requests(), 0);
        assert!(summary.ranges_completed() >= u64::from(workers.get().min(4)));
        assert!(server.max_concurrent_requests() <= usize::from(workers.get()));
        if workers != WorkerCount::One {
            assert_eq!(server.max_concurrent_requests(), usize::from(workers.get()));
        }

        assert_transfer_ranges_cover_once(&server, fixture.len);
        let expected_if_range = format!(
            "\"{}\"",
            probe
                .validators()
                .etag
                .as_ref()
                .expect("fixture strong ETag")
                .opaque()
        );
        for request in server
            .requests()
            .into_iter()
            .filter(|request| request.range.is_some_and(|range| range.start != range.end))
        {
            assert_eq!(
                request.if_range.as_deref(),
                Some(expected_if_range.as_str())
            );
        }
        let mut promotion = partial.promote().expect("publish output");
        assert_eq!(
            fs::read(promotion.final_path()).expect("read output"),
            fixture.bytes(
                0,
                usize::try_from(fixture.len).expect("fixture fits memory"),
                0,
            )
        );
        promotion.cleanup_partial().expect("remove partial link");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn idle_worker_hedges_one_slow_tail_without_overlapping_storage() {
    let fixture = Fixture {
        len: 4 * MIB,
        seed: 91,
    };
    let slow_range = ByteRange::new(0, MIB - 1).expect("slow range");
    let server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: None,
                range: Some(slow_range),
            },
            fault: Fault::StallFirst(Duration::from_secs(2)),
        }],
    })
    .expect("start server");
    let probe = ProbeClient::new()
        .expect("probe client")
        .probe(&server.url("/fixture"))
        .await
        .expect("probe fixture");
    let directory = TestDirectory::new("tail-hedge");
    let partial =
        PartialFile::create(directory.path(), "tail.bin", fixture.len).expect("create partial");
    let scheduler = scheduler(8, 8, Duration::from_millis(50), 64 * MIB);

    let started = Instant::now();
    let summary = scheduler
        .transfer(&probe, &partial, WorkerCount::Four)
        .await
        .expect("hedged transfer succeeds");
    assert!(started.elapsed() < Duration::from_millis(1500));
    assert_eq!(summary.hedged_requests(), 1);
    assert_eq!(summary.requests_started(), 5);
    assert_eq!(partial.completed_ranges(), vec![range(0, fixture.len)]);

    let transfer_requests = transfer_ranges(&server);
    assert_eq!(transfer_requests.len(), 5);
    assert_eq!(
        transfer_requests
            .iter()
            .filter(|range| **range == slow_range)
            .count(),
        2
    );
    let promotion = partial.promote().expect("publish hedged output");
    assert_eq!(
        fs::read(promotion.final_path()).expect("read output"),
        fixture.bytes(
            0,
            usize::try_from(fixture.len).expect("fixture fits memory"),
            0,
        )
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn controlled_stop_waits_for_workers_and_resume_skips_completed_ranges() {
    let fixture = Fixture {
        len: 8 * MIB,
        seed: 94,
    };
    let server = stalled_server(fixture.clone(), Duration::from_millis(80));
    let probe = ProbeClient::new()
        .expect("probe client")
        .probe(&server.url("/fixture"))
        .await
        .expect("probe fixture");
    let directory = TestDirectory::new("controlled-stop");
    let partial = PartialFile::create(directory.path(), "controlled.bin", fixture.len)
        .expect("create partial");
    let scheduler = DownloadScheduler::new().expect("scheduler");
    let cancellation = TransferCancellation::new();
    let (reporter, mut progress) = transfer_progress_channel();
    let running_scheduler = scheduler.clone();
    let running_probe = probe.clone();
    let running_partial = partial.clone();
    let running_cancellation = cancellation.clone();
    let transfer = tokio::spawn(async move {
        running_scheduler
            .transfer_controlled(
                &running_probe,
                &running_partial,
                WorkerCount::Four,
                &running_cancellation,
                reporter,
            )
            .await
    });

    loop {
        let sample = progress.changed().await.expect("progress remains open");
        if sample.bytes_completed() >= MIB {
            assert!(sample.active_workers() <= 4);
            break;
        }
    }
    cancellation.cancel();
    assert_eq!(
        transfer.await.expect("controlled worker joins"),
        Err(SchedulerError::Cancelled)
    );
    assert_eq!(progress.latest().active_workers(), 0);
    let retained = partial.completed_ranges();
    assert!(!retained.is_empty());
    // Remote observation can lag already-sent requests. Local reporter closure,
    // stable coverage and unchanged bytes are the cancellation contract.
    let stopped = progress.latest();
    let bytes = fs::read(partial.partial_path()).expect("read stopped partial");
    tokio::time::timeout(Duration::from_secs(5), async {
        while progress.changed().await.is_some() {}
    })
    .await
    .expect("all worker reporters close");
    assert_eq!(progress.latest(), stopped);
    assert_eq!(partial.completed_ranges(), retained);
    assert_eq!(
        fs::read(partial.partial_path()).expect("read stable partial"),
        bytes
    );

    scheduler
        .transfer(&probe, &partial, WorkerCount::Four)
        .await
        .expect("resume missing work");
    assert_eq!(partial.completed_ranges(), vec![range(0, fixture.len)]);
    let output = partial.promote().expect("publish resumed output");
    assert_eq!(
        fs::read(output.final_path()).expect("read output"),
        fixture.bytes(
            0,
            usize::try_from(fixture.len).expect("fixture fits memory"),
            0,
        )
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn received_requests_can_be_observed_after_join_without_post_stop_workers_or_writes() {
    let fixture = Fixture {
        len: 8 * MIB,
        seed: 95,
    };
    let server = stalled_server(fixture.clone(), Duration::from_millis(20));
    let probe = ProbeClient::new()
        .expect("client")
        .probe(&server.url("/fixture"))
        .await
        .expect("probe before gate");
    let directory = TestDirectory::new("late-observation");
    let partial = PartialFile::create(directory.path(), "late.bin", fixture.len).expect("partial");
    let mut writer = partial
        .assign(range(0, MIB))
        .expect("seed retained coverage");
    writer
        .write(&fixture.bytes(0, usize::try_from(MIB).expect("bounded"), 0))
        .expect("seed bytes");
    writer.finish().expect("seed completed range");
    let gate = server.pause_observation();
    let observed_before = server.requests().len();
    let cancellation = TransferCancellation::new();
    let (reporter, mut progress) = transfer_progress_channel();
    let scheduler = DownloadScheduler::new().expect("scheduler");
    let running_scheduler = scheduler.clone();
    let running_probe = probe.clone();
    let running_partial = partial.clone();
    let running_cancellation = cancellation.clone();
    let transfer = tokio::spawn(async move {
        running_scheduler
            .transfer_controlled(
                &running_probe,
                &running_partial,
                WorkerCount::Four,
                &running_cancellation,
                reporter,
            )
            .await
    });
    assert!(
        gate.wait_for_pending(4, Duration::from_secs(5)),
        "headers received before cancellation"
    );
    assert_eq!(server.requests().len(), observed_before);
    cancellation.cancel();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), transfer)
            .await
            .expect("bounded join")
            .expect("worker join"),
        Err(SchedulerError::Cancelled)
    );
    while progress.changed().await.is_some() {}
    let stopped = progress.latest();
    assert_eq!(stopped.active_workers(), 0);
    assert_eq!(stopped.requests_started(), 4);
    assert_eq!(partial.completed_ranges(), vec![range(0, MIB)]);
    let stopped_bytes = fs::read(partial.partial_path()).expect("stopped bytes");
    drop(gate);
    tokio::time::timeout(Duration::from_secs(5), async {
        while server.requests().len() < observed_before + 4 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("already-received requests enter ledger");
    assert_eq!(server.requests().len(), observed_before + 4);
    assert_eq!(progress.latest(), stopped);
    assert_eq!(
        fs::read(partial.partial_path()).expect("no late writes"),
        stopped_bytes
    );
    let resume_start = server.requests().len();
    scheduler
        .transfer(&probe, &partial, WorkerCount::Four)
        .await
        .expect("resume only missing work");
    for request in &server.requests()[resume_start..] {
        assert!(
            request.range.expect("ranged resume").start >= MIB,
            "retained bytes never requested"
        );
    }
    let result = partial.promote().expect("validated promotion");
    assert_eq!(
        fs::read(result.final_path()).expect("final bytes"),
        fixture.bytes(0, usize::try_from(fixture.len).expect("bounded fixture"), 0)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn per_host_and_global_request_limits_are_independent() {
    let fixture = Fixture {
        len: 8 * MIB,
        seed: 17,
    };
    let per_host_server = stalled_server(fixture.clone(), Duration::from_millis(50));
    let probe = ProbeClient::new()
        .expect("probe client")
        .probe(&per_host_server.url("/fixture"))
        .await
        .expect("probe fixture");
    let directory = TestDirectory::new("per-host-limit");
    let partial =
        PartialFile::create(directory.path(), "limited.bin", fixture.len).expect("create partial");
    let limited = scheduler(2, 8, Duration::from_secs(1), 64 * MIB);
    limited
        .transfer(&probe, &partial, WorkerCount::Eight)
        .await
        .expect("complete host-limited transfer");
    assert_eq!(per_host_server.max_concurrent_requests(), 2);
    assert!(limited.peak_global_requests() <= 2);

    let first_server = stalled_server(fixture.clone(), Duration::from_millis(60));
    let second_server = stalled_server(fixture.clone(), Duration::from_millis(60));
    let client = ProbeClient::new().expect("probe client");
    let first_probe = client
        .probe(&first_server.url("/fixture"))
        .await
        .expect("probe first host");
    let second_probe = client
        .probe(&second_server.url("/fixture"))
        .await
        .expect("probe second host");
    let first_directory = TestDirectory::new("global-first");
    let second_directory = TestDirectory::new("global-second");
    let first_partial = PartialFile::create(first_directory.path(), "first.bin", fixture.len)
        .expect("first partial");
    let second_partial = PartialFile::create(second_directory.path(), "second.bin", fixture.len)
        .expect("second partial");
    let global = scheduler(4, 3, Duration::from_secs(1), 64 * MIB);
    let (first, second) = tokio::join!(
        global.transfer(&first_probe, &first_partial, WorkerCount::Four),
        global.transfer(&second_probe, &second_partial, WorkerCount::Four),
    );
    first.expect("first globally limited transfer");
    second.expect("second globally limited transfer");
    assert_eq!(global.peak_global_requests(), 3);
    assert!(first_server.max_concurrent_requests() <= 3);
    assert!(second_server.max_concurrent_requests() <= 3);
    assert_eq!(
        first_partial.completed_ranges(),
        vec![range(0, fixture.len)]
    );
    assert_eq!(
        second_partial.completed_ranges(),
        vec![range(0, fixture.len)]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn known_and_unknown_single_stream_fallback_share_storage_publication() {
    let fixture = Fixture {
        len: 256 * 1024,
        seed: 66,
    };
    let server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: Vec::new(),
    })
    .expect("start server");
    let client = ProbeClient::new().expect("probe client");
    let scheduler = DownloadScheduler::new().expect("scheduler");

    let known_probe = client
        .probe(&server.url("/ignore-range"))
        .await
        .expect("probe known single stream");
    assert!(matches!(known_probe.mode(), ProbeMode::SingleStream(_)));
    assert_eq!(known_probe.size(), Some(fixture.len));
    let known_directory = TestDirectory::new("known-stream");
    let known = PartialFile::create(known_directory.path(), "known.bin", fixture.len)
        .expect("known partial");
    let known_summary = scheduler
        .transfer(&known_probe, &known, WorkerCount::Eight)
        .await
        .expect("known stream transfer");
    assert_eq!(known_summary.kind(), TransferKind::Single);
    assert_eq!(known_summary.requests_started(), 1);
    assert_eq!(known_summary.workers_used(), 1);
    let known_output = known.promote().expect("publish known stream");
    assert_eq!(
        fs::read(known_output.final_path()).expect("read known stream"),
        fixture.bytes(
            0,
            usize::try_from(fixture.len).expect("fixture fits memory"),
            0,
        )
    );

    let unknown_probe = client
        .probe(&server.url("/unknown-length"))
        .await
        .expect("probe unknown single stream");
    assert!(matches!(unknown_probe.mode(), ProbeMode::SingleStream(_)));
    assert_eq!(unknown_probe.size(), None);
    let unknown_directory = TestDirectory::new("unknown-stream");
    let unknown = PartialFile::create_streaming(unknown_directory.path(), "unknown.bin")
        .expect("streaming partial");
    let unknown_summary = scheduler
        .transfer(&unknown_probe, &unknown, WorkerCount::Four)
        .await
        .expect("unknown stream transfer");
    assert_eq!(unknown_summary.kind(), TransferKind::Single);
    assert_eq!(unknown_summary.bytes_written(), fixture.len);
    assert_eq!(unknown.expected_len(), Some(fixture.len));
    assert_eq!(unknown.completed_ranges(), vec![range(0, fixture.len)]);
    let unknown_output = unknown.promote().expect("publish unknown stream");
    assert_eq!(
        fs::read(unknown_output.final_path()).expect("read unknown stream"),
        fixture.bytes(
            0,
            usize::try_from(fixture.len).expect("fixture fits memory"),
            0,
        )
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn proven_empty_resource_completes_without_a_body_request() {
    let server = TestServer::start(ServerConfig::default()).expect("start server");
    let probe = ProbeClient::new()
        .expect("probe client")
        .probe(&server.url("/empty"))
        .await
        .expect("probe empty resource");
    assert_eq!(probe.mode(), ProbeMode::Empty);
    let request_count = server.requests().len();
    let directory = TestDirectory::new("empty");
    let partial = PartialFile::create(directory.path(), "empty.bin", 0).expect("empty partial");
    let summary = DownloadScheduler::new()
        .expect("scheduler")
        .transfer(&probe, &partial, WorkerCount::Eight)
        .await
        .expect("complete empty transfer");
    assert_eq!(summary.kind(), TransferKind::Empty);
    assert_eq!(summary.requests_started(), 0);
    assert_eq!(server.requests().len(), request_count);
    let output = partial.promote().expect("publish empty output");
    assert!(
        fs::read(output.final_path())
            .expect("read empty output")
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_stream_response_is_revalidated_before_completion() {
    let fixture = Fixture {
        len: 128 * 1024,
        seed: 67,
    };
    for (index, fault) in [
        Fault::ValidatorGeneration(1),
        Fault::UnexpectedEncoding("gzip".to_owned()),
        Fault::DisconnectAfter(11),
    ]
    .into_iter()
    .enumerate()
    {
        let server = TestServer::start(ServerConfig {
            fixture: fixture.clone(),
            rules: vec![FaultRule {
                selector: RequestSelector {
                    path: Some("/ignore-range".to_owned()),
                    request_number: Some(2),
                    range: None,
                },
                fault,
            }],
        })
        .expect("start server");
        let probe = ProbeClient::new()
            .expect("probe client")
            .probe(&server.url("/ignore-range"))
            .await
            .expect("probe fallback");
        let directory = TestDirectory::new(&format!("single-validation-{index}"));
        let partial = PartialFile::create(directory.path(), "single.bin", fixture.len)
            .expect("create partial");
        assert!(
            DownloadScheduler::new()
                .expect("scheduler")
                .transfer(&probe, &partial, WorkerCount::Four)
                .await
                .is_err()
        );
        assert!(partial.completed_ranges().is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resumed_transfer_requests_only_missing_ranges() {
    let fixture = Fixture {
        len: 4 * MIB,
        seed: 29,
    };
    let server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: Vec::new(),
    })
    .expect("start server");
    let probe = ProbeClient::new()
        .expect("probe client")
        .probe(&server.url("/fixture"))
        .await
        .expect("probe fixture");
    let directory = TestDirectory::new("resume-missing");
    let partial =
        PartialFile::create(directory.path(), "resume.bin", fixture.len).expect("create partial");
    let completed_end = MIB;
    let mut prefix = partial
        .assign(range(0, completed_end))
        .expect("assign prefix");
    prefix
        .write(&fixture.bytes(
            0,
            usize::try_from(completed_end).expect("prefix fits memory"),
            0,
        ))
        .expect("write prefix");
    prefix.finish().expect("finish prefix");
    partial
        .durable_completed_ranges()
        .expect("flush prefix before resume");

    let scheduler = DownloadScheduler::new().expect("scheduler");
    let summary = scheduler
        .transfer(&probe, &partial, WorkerCount::Four)
        .await
        .expect("resume missing ranges");
    assert_eq!(summary.bytes_written(), fixture.len - completed_end);
    for requested in transfer_ranges(&server) {
        assert!(requested.start >= completed_end);
    }
    let output = partial.promote().expect("publish resumed output");
    assert_eq!(
        fs::read(output.final_path()).expect("read resumed output"),
        fixture.bytes(
            0,
            usize::try_from(fixture.len).expect("fixture fits memory"),
            0,
        )
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_segment_revalidates_status_range_encoding_validator_and_body() {
    let fixture = Fixture {
        len: 128 * 1024,
        seed: 53,
    };
    let full_range = ByteRange::new(0, fixture.len - 1).expect("full range");
    let faults = [
        Fault::IgnoreRange,
        Fault::BadContentRange(BadRange::Total),
        Fault::ValidatorGeneration(1),
        Fault::OmitValidators,
        Fault::UnexpectedEncoding("gzip".to_owned()),
        Fault::DisconnectAfter(7),
        Fault::Status {
            code: 429,
            retry_after_seconds: Some(3),
        },
        Fault::Redirect("/fixture".to_owned()),
    ];

    for (index, fault) in faults.into_iter().enumerate() {
        let server = TestServer::start(ServerConfig {
            fixture: fixture.clone(),
            rules: vec![FaultRule {
                selector: RequestSelector {
                    path: Some("/fixture".to_owned()),
                    request_number: None,
                    range: Some(full_range),
                },
                fault,
            }],
        })
        .expect("start adversarial server");
        let probe = ProbeClient::new()
            .expect("probe client")
            .probe(&server.url("/fixture"))
            .await
            .expect("probe before worker-only fault");
        let directory = TestDirectory::new(&format!("worker-validation-{index}"));
        let partial = PartialFile::create(directory.path(), "invalid.bin", fixture.len)
            .expect("create partial");
        let result = DownloadScheduler::new()
            .expect("scheduler")
            .transfer(&probe, &partial, WorkerCount::Four)
            .await;
        assert!(result.is_err(), "fault {index} was unexpectedly accepted");
        if index == 6 {
            assert_eq!(
                result,
                Err(SchedulerError::HttpStatus {
                    status: 429,
                    retry_after_seconds: Some(3),
                })
            );
        }
        assert!(partial.completed_ranges().is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invalid_worker_response_and_unknown_body_limit_fail_without_claiming_bytes() {
    let fixture = Fixture {
        len: 128 * 1024,
        seed: 88,
    };
    let full_range = ByteRange::new(0, fixture.len - 1).expect("full range");
    let server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: None,
                range: Some(full_range),
            },
            fault: Fault::BadContentRange(BadRange::Start),
        }],
    })
    .expect("start server");
    let client = ProbeClient::new().expect("probe client");
    let probe = client
        .probe(&server.url("/fixture"))
        .await
        .expect("probe before selected worker fault");
    let directory = TestDirectory::new("invalid-range");
    let partial =
        PartialFile::create(directory.path(), "invalid.bin", fixture.len).expect("create partial");
    let default_scheduler = DownloadScheduler::new().expect("scheduler");
    assert!(matches!(
        default_scheduler
            .transfer(&probe, &partial, WorkerCount::Four)
            .await,
        Err(SchedulerError::InvalidRange(
            RangeValidationError::ContentRangeMismatch
        ))
    ));
    assert!(partial.completed_ranges().is_empty());

    let unknown_probe = client
        .probe(&server.url("/unknown-length"))
        .await
        .expect("probe unknown stream");
    let unknown_directory = TestDirectory::new("unknown-limit");
    let unknown = PartialFile::create_streaming(unknown_directory.path(), "limited.bin")
        .expect("create streaming partial");
    let bounded = scheduler(4, 8, Duration::from_secs(1), 1024);
    assert_eq!(
        bounded
            .transfer(&unknown_probe, &unknown, WorkerCount::Four)
            .await,
        Err(SchedulerError::Storage(StorageError::StreamLimitExceeded {
            limit: 1024
        }))
    );
    assert_eq!(unknown.expected_len(), None);
    assert!(unknown.completed_ranges().is_empty());
}

fn scheduler(
    per_host: usize,
    global: usize,
    hedge_delay: Duration,
    unknown_limit: u64,
) -> DownloadScheduler {
    let limits = ConcurrencyLimits::new(per_host, global).expect("valid limits");
    let options = SchedulerOptions::new(limits, hedge_delay, unknown_limit).expect("valid options");
    DownloadScheduler::with_options(options).expect("scheduler")
}

fn stalled_server(fixture: Fixture, delay: Duration) -> TestServer {
    TestServer::start(ServerConfig {
        fixture,
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: None,
                range: None,
            },
            fault: Fault::Stall(delay),
        }],
    })
    .expect("start stalled server")
}

fn transfer_ranges(server: &TestServer) -> Vec<ByteRange> {
    server
        .requests()
        .into_iter()
        .filter_map(|request| request.range)
        .filter(|range| range.start != range.end)
        .collect()
}

fn assert_transfer_ranges_cover_once(server: &TestServer, total: u64) {
    let mut ranges = transfer_ranges(server);
    ranges.sort_unstable_by_key(|range| range.start);
    let mut cursor = 0;
    for range in ranges {
        assert_eq!(range.start, cursor);
        assert!(range.end < total);
        assert!(range.end - range.start + 1 >= MIB);
        cursor = range.end + 1;
    }
    assert_eq!(cursor, total);
}

fn range(start: u64, end: u64) -> FileRange {
    FileRange::new(start, end).expect("valid test range")
}

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(label: &str) -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "firefox-download-manager-scheduler-{label}-{}-{nanos}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create test directory");
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

#[tokio::test]
async fn unproven_identity_uses_one_stream_and_never_reuses_completed_storage() {
    for path in ["/validators/missing", "/validators/weak"] {
        let fixture = Fixture {
            len: 8192,
            seed: 77,
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
        let directory = TestDirectory::new("weak-stream");
        let partial =
            PartialFile::create(directory.path(), "single.bin", fixture.len).expect("partial");
        let scheduler = DownloadScheduler::new().expect("scheduler");
        let result = scheduler
            .transfer(&probe, &partial, WorkerCount::Eight)
            .await
            .expect("one stream");
        assert_eq!(result.kind(), TransferKind::Single);
        assert_eq!(result.workers_used(), 1);
        assert!(server.requests().last().expect("GET").range.is_none());
        assert_eq!(
            fs::read(partial.partial_path()).expect("read"),
            fixture.bytes(0, 8192, 0)
        );
        let requests = server.requests().len();
        assert_eq!(
            scheduler
                .transfer(&probe, &partial, WorkerCount::Eight)
                .await,
            Err(SchedulerError::InvalidCompletedCoverage)
        );
        assert_eq!(server.requests().len(), requests);
    }
}
