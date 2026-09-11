use std::{fs, io::Write, path::PathBuf, time::Duration};

use download_manager_engine::{
    persistence::{HandoffPhase, PROTECTED_HANDOFF_FORMAT_VERSION, TaskId, TaskState, TaskStore},
    scheduler::{DownloadScheduler, WorkerCount},
    task::{
        CancelPartialPolicy, HandoffRequest, ProtectionDecision, ProtectionGate,
        ProtectionReceiver, ProtectionRequest, TaskEngine, TaskEngineError, TaskEngineOptions,
        TaskFailureKind, TaskSnapshot,
    },
};
use download_manager_test_server::{
    Fault, FaultRule, Fixture, RequestSelector, ServerConfig, TestServer,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const FIXTURE: Fixture = Fixture {
    len: 64 * 1024,
    seed: 37,
};
struct Domain {
    root: PathBuf,
    state: PathBuf,
    downloads: PathBuf,
}
impl Domain {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("dm-protection-{}", TaskId::new()));
        fs::create_dir(&root).unwrap();
        let downloads = root.join("downloads");
        fs::create_dir(&downloads).unwrap();
        Self {
            state: root.join("state"),
            root,
            downloads,
        }
    }
    fn engine(&self) -> (TaskEngine, ProtectionReceiver) {
        let (gate, receiver) = ProtectionGate::channel();
        let engine = TaskEngine::open_with_protection(
            &self.state,
            TaskEngineOptions::default(),
            DownloadScheduler::new().unwrap(),
            gate,
        )
        .unwrap();
        (engine, receiver)
    }
    fn request(&self, id: TaskId, url: &str) -> HandoffRequest {
        HandoffRequest::new(
            id,
            url,
            &self.downloads,
            "protected.txt",
            WorkerCount::Four,
            None,
        )
        .unwrap()
        .require_protection()
    }
    fn record(&self, id: TaskId) -> PathBuf {
        self.state.join("tasks").join(format!("{id}.task.json"))
    }
    fn output(&self) -> PathBuf {
        self.downloads.join("protected.txt")
    }
}
impl Drop for Domain {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
}
fn server() -> TestServer {
    TestServer::start(ServerConfig {
        fixture: FIXTURE,
        rules: vec![],
    })
    .unwrap()
}
async fn request(receiver: &mut ProtectionReceiver) -> Option<ProtectionRequest> {
    tokio::time::timeout(Duration::from_secs(5), receiver.recv())
        .await
        .ok()
        .flatten()
}
async fn inactive(engine: &TaskEngine, id: TaskId) -> Option<TaskSnapshot> {
    tokio::time::timeout(Duration::from_secs(5), engine.wait_until_inactive(id))
        .await
        .ok()
        .and_then(Result::ok)
}
fn failure(snapshot: Option<&TaskSnapshot>) -> Option<TaskFailureKind> {
    snapshot
        .and_then(TaskSnapshot::failure)
        .map(download_manager_engine::task::TaskFailure::kind)
}

#[tokio::test]
async fn actual_bytes_remain_leased_and_unpublished_until_one_current_decision() {
    let domain = Domain::new();
    let server = server();
    let (engine, mut receiver) = domain.engine();
    let id = TaskId::new();
    let url = server.url("/fixture?opaque=owned-protection-canary");
    engine.prepare_handoff(domain.request(id, &url)).unwrap();
    let commit = engine.commit_handoff(id);
    let challenge = request(&mut receiver).await;
    let pending_state = engine.snapshot(id).ok().map(|snapshot| snapshot.state());
    let unpublished = !domain.output().exists();
    let mut evidence = None;
    let mut write_refused = true;
    if let Some(challenge) = challenge {
        #[cfg(windows)]
        {
            write_refused = engine
                .metadata(id)
                .ok()
                .and_then(|metadata| metadata.partial_path().map(PathBuf::from))
                .and_then(|path| fs::OpenOptions::new().write(true).open(path).ok())
                .is_some_and(|mut file| file.write_all(b"bad").is_err());
        }
        evidence = Some((
            challenge.task_id(),
            challenge.generation(),
            challenge.source_url() == url,
            challenge.file_name().to_owned(),
            challenge.fingerprint(),
            format!("{challenge:?}"),
        ));
        let _ = challenge.decide(ProtectionDecision::PermitPublication);
    }
    let completed = inactive(&engine, id).await;
    let joined = engine.shutdown().await;
    drop(receiver);
    drop(engine);
    drop(server);
    assert!(joined.is_ok());
    assert!(commit.is_ok());
    assert_eq!(pending_state, Some(TaskState::Validating));
    assert!(unpublished);
    assert!(
        write_refused,
        "the engine must retain writer exclusion while waiting"
    );
    let (task, generation, source, name, fingerprint, debug) =
        evidence.expect("actual native challenge");
    assert_eq!(task, id);
    assert_eq!(generation, 1);
    assert!(source);
    assert_eq!(name, "protected.txt");
    assert_eq!(debug, "ProtectionRequest(<redacted>)");
    let bytes = FIXTURE.bytes(0, 64 * 1024, 0);
    assert_eq!(fingerprint.length(), FIXTURE.len);
    assert_eq!(
        fingerprint.sha256(),
        <[u8; 32]>::from(Sha256::digest(&bytes))
    );
    assert_eq!(completed.unwrap().state(), TaskState::Completed);
    assert_eq!(fs::read(domain.output()).unwrap(), bytes);
    assert_eq!(fs::read_dir(&domain.downloads).unwrap().count(), 1);
    #[cfg(windows)]
    assert_eq!(
        fs::read(format!("{}:Zone.Identifier", domain.output().display())).unwrap(),
        b"[ZoneTransfer]\r\nZoneId=3\r\n"
    );
    let disk: Value = serde_json::from_slice(&fs::read(domain.record(id)).unwrap()).unwrap();
    assert_eq!(disk["version"], PROTECTED_HANDOFF_FORMAT_VERSION);
    assert_eq!(disk["protection"], "browser-bound-v1");
    assert!(disk.get("verdict").is_none());
    assert!(disk.get("fingerprint").is_none());
}

#[tokio::test]
async fn refused_missing_and_disconnected_decisions_never_publish() {
    for mode in 0..5 {
        let domain = Domain::new();
        let server = server();
        let (engine, mut receiver) = domain.engine();
        let id = TaskId::new();
        engine
            .prepare_handoff(domain.request(id, &server.url("/fixture")))
            .unwrap();
        let commit = engine.commit_handoff(id);
        let challenge = request(&mut receiver).await;
        let observed = challenge.is_some();
        if let Some(challenge) = challenge {
            match mode {
                0 => {
                    let _ = challenge.decide(ProtectionDecision::Blocked);
                }
                1 => {
                    let _ = challenge.decide(ProtectionDecision::Unavailable);
                }
                2 => drop(challenge),
                3 => {
                    receiver.close();
                    let _ = challenge.decide(ProtectionDecision::PermitPublication);
                }
                _ => {
                    let _ = challenge.decide(ProtectionDecision::PermitPublication);
                    receiver.close();
                }
            }
        }
        let stopped = inactive(&engine, id).await;
        let joined = engine.shutdown().await;
        drop(receiver);
        drop(engine);
        drop(server);
        assert!(joined.is_ok());
        assert!(commit.is_ok());
        assert!(observed);
        assert_eq!(
            failure(stopped.as_ref()),
            Some(if mode == 0 {
                TaskFailureKind::ProtectionBlocked
            } else {
                TaskFailureKind::ProtectionUnavailable
            })
        );
        assert!(!domain.output().exists());
    }
}

#[tokio::test]
async fn cancellation_and_shutdown_retire_the_lease_and_reject_late_results() {
    for explicit_cancel in [false, true] {
        let domain = Domain::new();
        let server = server();
        let (engine, mut receiver) = domain.engine();
        let id = TaskId::new();
        engine
            .prepare_handoff(domain.request(id, &server.url("/fixture")))
            .unwrap();
        let commit = engine.commit_handoff(id);
        let challenge = request(&mut receiver).await;
        let cancelled = if explicit_cancel {
            Some(
                tokio::time::timeout(
                    Duration::from_secs(5),
                    engine.cancel(id, CancelPartialPolicy::Keep),
                )
                .await,
            )
        } else {
            None
        };
        let joined = engine.shutdown().await;
        let stale = challenge.map(|challenge| {
            (
                challenge.is_cancelled(),
                challenge.decide(ProtectionDecision::PermitPublication),
            )
        });
        drop(receiver);
        drop(engine);
        drop(server);
        let store = TaskStore::open(&domain.state); // Requires all coordinator/engine owners gone.
        assert!(joined.is_ok());
        assert!(commit.is_ok());
        assert!(store.is_ok());
        if let Some(cancelled) = cancelled {
            assert_eq!(cancelled.unwrap().unwrap().state(), TaskState::Cancelled);
        }
        assert_eq!(
            stale,
            Some((true, Err(TaskFailureKind::ProtectionUnavailable)))
        );
        assert!(!domain.output().exists());
    }
}

#[tokio::test]
async fn explicit_retry_requires_new_generation_and_decision_not_cached_permission() {
    let domain = Domain::new();
    let server = server();
    let (engine, mut receiver) = domain.engine();
    let id = TaskId::new();
    engine
        .prepare_handoff(domain.request(id, &server.url("/fixture")))
        .unwrap();
    let commit = engine.commit_handoff(id);
    let first = request(&mut receiver).await;
    let first_identity = first.as_ref().map(|value| {
        (
            value.id(),
            value.generation(),
            value.connection_id(),
            value.fingerprint(),
        )
    });
    if let Some(first) = first {
        let _ = first.decide(ProtectionDecision::Blocked);
    }
    let failed = inactive(&engine, id).await;
    let retry = engine.retry(id);
    let second = request(&mut receiver).await;
    let second_identity = second.as_ref().map(|value| {
        (
            value.id(),
            value.generation(),
            value.connection_id(),
            value.fingerprint(),
        )
    });
    let still_unpublished = !domain.output().exists();
    if let Some(second) = second {
        let _ = second.decide(ProtectionDecision::PermitPublication);
    }
    let complete = inactive(&engine, id).await;
    let joined = engine.shutdown().await;
    drop(receiver);
    drop(engine);
    drop(server);
    assert!(joined.is_ok());
    assert!(commit.is_ok());
    assert!(retry.is_ok());
    assert_eq!(
        failure(failed.as_ref()),
        Some(TaskFailureKind::ProtectionBlocked)
    );
    assert!(still_unpublished);
    let first = first_identity.unwrap();
    let second = second_identity.unwrap();
    assert_ne!(first.0, second.0);
    assert!(first.1 < second.1);
    assert_eq!(first.2, second.2);
    assert_eq!(first.3, second.3);
    assert_eq!(complete.unwrap().state(), TaskState::Completed);
    assert_eq!(
        fs::read(domain.output()).unwrap(),
        FIXTURE.bytes(0, 64 * 1024, 0)
    );
}

#[tokio::test]
async fn a_late_name_collision_refuses_without_reselection_or_overwrite() {
    let domain = Domain::new();
    let server = server();
    let (engine, mut receiver) = domain.engine();
    let id = TaskId::new();
    engine
        .prepare_handoff(domain.request(id, &server.url("/fixture")))
        .unwrap();
    let commit = engine.commit_handoff(id);
    let challenge = request(&mut receiver).await;
    let observed = challenge.is_some();
    let collision = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(domain.output())
        .and_then(|mut file| file.write_all(b"existing owned bytes"));
    if let Some(challenge) = challenge {
        let _ = challenge.decide(ProtectionDecision::PermitPublication);
    }
    let failed = inactive(&engine, id).await;
    let joined = engine.shutdown().await;
    drop(receiver);
    drop(engine);
    drop(server);
    assert!(joined.is_ok());
    assert!(commit.is_ok());
    assert!(observed);
    assert!(collision.is_ok());
    assert_eq!(failure(failed.as_ref()), Some(TaskFailureKind::FileExists));
    assert_eq!(fs::read(domain.output()).unwrap(), b"existing owned bytes");
    assert!(!domain.downloads.join("protected (1).txt").exists());
}

#[tokio::test]
async fn recovered_protected_preparation_cannot_reacquire_browser_context_by_replay() {
    let domain = Domain::new();
    let server = server();
    let id = TaskId::new();
    let url = server.url("/fixture");
    let (engine, receiver) = domain.engine();
    let prepared = engine.prepare_handoff(domain.request(id, &url)).unwrap();
    let downgrade = engine.prepare_handoff(
        HandoffRequest::new(
            id,
            &url,
            &domain.downloads,
            "protected.txt",
            WorkerCount::Four,
            None,
        )
        .unwrap(),
    );
    let joined = engine.shutdown().await;
    drop(receiver);
    drop(engine);
    let (recovered, receiver) = domain.engine();
    let replay = recovered.prepare_handoff(domain.request(id, &url));
    let commit = recovered.commit_handoff(id);
    let metadata = recovered.metadata(id);
    let joined_again = recovered.shutdown().await;
    let requests = server.requests();
    drop(receiver);
    drop(recovered);
    drop(server);
    assert!(joined.is_ok());
    assert!(joined_again.is_ok());
    assert_eq!(downgrade, Err(TaskEngineError::InvalidTaskState));
    assert_eq!(replay.unwrap(), prepared);
    assert_eq!(
        commit,
        Err(TaskEngineError::ControlFailed(
            TaskFailureKind::ProtectionUnavailable
        ))
    );
    assert!(metadata.unwrap().requires_protection());
    assert!(requests.is_empty());
    assert!(!domain.output().exists());
}

#[tokio::test]
async fn default_engine_and_recovered_failed_runs_do_not_silently_omit_protection() {
    let domain = Domain::new();
    let server = server();
    let id = TaskId::new();
    let (engine, mut receiver) = domain.engine();
    engine
        .prepare_handoff(domain.request(id, &server.url("/fixture")))
        .unwrap();
    let commit = engine.commit_handoff(id);
    let challenge = request(&mut receiver).await;
    let observed = challenge.is_some();
    if let Some(challenge) = challenge {
        let _ = challenge.decide(ProtectionDecision::Blocked);
    }
    let failed = inactive(&engine, id).await;
    let joined = engine.shutdown().await;
    drop(receiver);
    drop(engine);
    let before = server.requests().len();
    let (recovered, receiver) = domain.engine();
    let retry = recovered.retry(id);
    let resume = recovered.resume(id).await;
    let repeat_commit = recovered.commit_handoff(id);
    let joined_again = recovered.shutdown().await;
    drop(receiver);
    drop(recovered);
    let default = TaskEngine::open(&domain.state, TaskEngineOptions::default()).unwrap();
    let other = TaskId::new();
    let prepared = default.prepare_handoff(domain.request(other, &server.url("/fixture")));
    let refused = default.commit_handoff(other);
    let final_join = default.shutdown().await;
    drop(default);
    let after = server.requests().len();
    drop(server);
    assert!(joined.is_ok());
    assert!(joined_again.is_ok());
    assert!(final_join.is_ok());
    assert!(commit.is_ok());
    assert!(observed);
    assert_eq!(
        failure(failed.as_ref()),
        Some(TaskFailureKind::ProtectionBlocked)
    );
    assert_eq!(
        retry,
        Err(TaskEngineError::ControlFailed(
            TaskFailureKind::ProtectionUnavailable
        ))
    );
    assert_eq!(resume, retry);
    assert_eq!(repeat_commit.unwrap().phase(), HandoffPhase::Committed);
    assert!(prepared.is_ok());
    assert_eq!(
        refused,
        Err(TaskEngineError::ControlFailed(
            TaskFailureKind::ProtectionUnavailable
        ))
    );
    assert_eq!(before, after);
    assert!(!domain.output().exists());
}

#[tokio::test]
async fn protected_reprobe_refuses_redirect_before_contact_on_initial_and_retry_runs() {
    let domain = Domain::new();
    let target = server();
    let source = TestServer::start(ServerConfig {
        fixture: FIXTURE,
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: None,
                range: None,
            },
            fault: Fault::Redirect(target.url("/fixture")),
        }],
    })
    .unwrap();
    let (engine, receiver) = domain.engine();
    let id = TaskId::new();
    engine
        .prepare_handoff(domain.request(id, &source.url("/fixture")))
        .unwrap();
    let commit = engine.commit_handoff(id);
    let initial = inactive(&engine, id).await;
    let retry = engine.retry(id);
    let failed = inactive(&engine, id).await;
    let joined = engine.shutdown().await;
    let source_requests = source.requests();
    let target_requests = target.requests();
    drop(receiver);
    drop(engine);
    drop(source);
    drop(target);
    assert!(joined.is_ok());
    assert!(commit.is_ok());
    assert!(retry.is_ok());
    assert_eq!(
        failure(initial.as_ref()),
        Some(TaskFailureKind::RedirectRejected)
    );
    assert_eq!(
        failure(failed.as_ref()),
        Some(TaskFailureKind::RedirectRejected)
    );
    assert_eq!(source_requests.len(), 2);
    assert!(target_requests.is_empty());
    assert!(!domain.output().exists());
}

#[tokio::test]
async fn protected_envelope_is_closed_and_cannot_encode_an_optional_false_or_unknown_requirement() {
    let domain = Domain::new();
    let id = TaskId::new();
    let (engine, receiver) = domain.engine();
    engine
        .prepare_handoff(domain.request(id, "http://127.0.0.1/fixture"))
        .unwrap();
    let joined = engine.shutdown().await;
    drop(receiver);
    drop(engine);
    let original: Value = serde_json::from_slice(&fs::read(domain.record(id)).unwrap()).unwrap();
    let mut outcomes = vec![];
    for change in 0..8 {
        let mut raw = original.clone();
        match change {
            0 => {
                raw.as_object_mut().unwrap().remove("protection");
            }
            1 => raw["protection"] = Value::Null,
            2 => raw["protection"] = json!(false),
            3 => raw["protection"] = json!("optional"),
            4 => raw["unknown"] = json!(true),
            5 => raw["task"]["needs_session"] = json!(true),
            6 => raw["version"] = json!(5), // Older closed envelope rejects the extra requirement.
            _ => raw["protection"] = json!({"browser-bound-v1": null}),
        }
        fs::write(domain.record(id), serde_json::to_vec(&raw).unwrap()).unwrap();
        let store = TaskStore::open(&domain.state).unwrap();
        let report = store.load_all().unwrap();
        outcomes.push(report.tasks().is_empty() && report.failures().len() == 1);
        drop(store);
    }
    fs::write(domain.record(id), serde_json::to_vec(&original).unwrap()).unwrap();
    let store = TaskStore::open(&domain.state).unwrap();
    let loaded = store.load_all().unwrap();
    drop(store);
    assert!(joined.is_ok());
    assert!(outcomes.into_iter().all(|refused| refused));
    assert_eq!(loaded.tasks().len(), 1);
    assert!(loaded.tasks()[0].requires_protection());
    assert!(!domain.output().exists());
}

#[tokio::test]
async fn checksum_failure_precedes_any_protection_challenge() {
    let domain = Domain::new();
    let server = server();
    let (engine, mut receiver) = domain.engine();
    let id = TaskId::new();
    let expected =
        download_manager_engine::integrity::ExpectedSha256::parse(&"00".repeat(32)).unwrap();
    let request = HandoffRequest::new(
        id,
        &server.url("/fixture"),
        &domain.downloads,
        "protected.txt",
        WorkerCount::Four,
        Some(expected),
    )
    .unwrap()
    .require_protection();
    engine.prepare_handoff(request).unwrap();
    let commit = engine.commit_handoff(id);
    let observation = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::select! {
            request = receiver.recv() => (request, None),
            stopped = engine.wait_until_inactive(id) => (None, Some(stopped)),
        }
    })
    .await;
    let observed = if let Ok((request, stopped)) = observation {
        let observed = request.is_none()
            && stopped.is_some_and(|value| {
                value.is_ok_and(|snapshot| {
                    snapshot
                        .failure()
                        .is_some_and(|failure| failure.kind() == TaskFailureKind::ChecksumMismatch)
                })
            });
        if let Some(request) = request {
            let _ = request.decide(ProtectionDecision::Blocked);
        }
        observed
    } else {
        false
    };
    let stopped = inactive(&engine, id).await;
    let joined = engine.shutdown().await;
    drop(receiver);
    drop(engine);
    drop(server);
    assert!(joined.is_ok());
    assert!(commit.is_ok());
    assert!(observed);
    assert_eq!(
        failure(stopped.as_ref()),
        Some(TaskFailureKind::ChecksumMismatch)
    );
    assert!(!domain.output().exists());
}

#[tokio::test]
async fn recovered_paused_metadata_does_not_authorize_resume_with_a_new_receiver() {
    let domain = Domain::new();
    let server = server();
    let id = TaskId::new();
    let (engine, mut receiver) = domain.engine();
    engine
        .prepare_handoff(domain.request(id, &server.url("/fixture")))
        .unwrap();
    let commit = engine.commit_handoff(id);
    let challenge = request(&mut receiver).await;
    let observed = challenge.is_some();
    if let Some(challenge) = challenge {
        let _ = challenge.decide(ProtectionDecision::Blocked);
    }
    let failed = inactive(&engine, id).await;
    let joined = engine.shutdown().await;
    drop(receiver);
    drop(engine);
    // Valid full-partial metadata fixture, not an observed interrupted download.
    let mut raw: Value = serde_json::from_slice(&fs::read(domain.record(id)).unwrap()).unwrap();
    raw["task"]["state"] = json!(TaskState::Paused);
    fs::write(domain.record(id), serde_json::to_vec(&raw).unwrap()).unwrap();
    let (recovered, receiver) = domain.engine();
    let before = recovered.snapshot(id);
    let resumed = recovered.resume(id).await;
    let after = recovered.snapshot(id);
    let joined_again = recovered.shutdown().await;
    drop(receiver);
    drop(recovered);
    drop(server);
    assert!(joined.is_ok());
    assert!(joined_again.is_ok());
    assert!(commit.is_ok());
    assert!(observed);
    assert_eq!(
        failure(failed.as_ref()),
        Some(TaskFailureKind::ProtectionBlocked)
    );
    assert_eq!(before.as_ref().unwrap().state(), TaskState::Paused);
    assert_eq!(
        resumed,
        Err(TaskEngineError::ControlFailed(
            TaskFailureKind::ProtectionUnavailable
        ))
    );
    assert_eq!(before, after);
    assert!(!domain.output().exists());
}

#[tokio::test]
async fn recovered_committed_queued_metadata_refuses_generic_start_before_mutation() {
    let domain = Domain::new();
    let server = server();
    let id = TaskId::new();
    let (engine, receiver) = domain.engine();
    engine
        .prepare_handoff(domain.request(id, &server.url("/fixture")))
        .unwrap();
    let joined = engine.shutdown().await;
    drop(receiver);
    drop(engine);
    // Metadata-only committed/probe-failed/requeued fixture; no crash is inferred.
    let mut raw: Value = serde_json::from_slice(&fs::read(domain.record(id)).unwrap()).unwrap();
    raw["handoff"] = json!(HandoffPhase::Committed);
    raw["task"]["revision"] = json!(4);
    fs::write(domain.record(id), serde_json::to_vec(&raw).unwrap()).unwrap();
    let (recovered, receiver) = domain.engine();
    let before = recovered.snapshot(id);
    let start = recovered.start(id);
    let after = recovered.snapshot(id);
    let joined_again = recovered.shutdown().await;
    let requests = server.requests();
    drop(receiver);
    drop(recovered);
    drop(server);
    assert!(joined.is_ok());
    assert!(joined_again.is_ok());
    assert_eq!(before.as_ref().unwrap().state(), TaskState::Queued);
    assert_eq!(
        start,
        Err(TaskEngineError::ControlFailed(
            TaskFailureKind::ProtectionUnavailable
        ))
    );
    assert_eq!(before, after);
    assert!(requests.is_empty());
    assert!(!domain.output().exists());
}
