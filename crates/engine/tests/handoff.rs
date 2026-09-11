use std::{fs, path::PathBuf, time::Duration};

use download_manager_engine::{
    integrity::ExpectedSha256,
    persistence::{HandoffPhase, TaskId, TaskState},
    scheduler::WorkerCount,
    task::{CancelPartialPolicy, HandoffRequest, TaskEngine, TaskEngineError, TaskEngineOptions},
};
use download_manager_test_server::{Fixture, ServerConfig, TestServer};
use serde_json::{Value, json};

struct Domain {
    root: PathBuf,
    state: PathBuf,
    downloads: PathBuf,
}
impl Domain {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("dm-handoff-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let downloads = root.join("downloads");
        fs::create_dir(&downloads).unwrap();
        Self {
            state: root.join("state"),
            downloads,
            root,
        }
    }
    fn engine(&self) -> TaskEngine {
        TaskEngine::open(&self.state, TaskEngineOptions::default()).unwrap()
    }
    fn request(&self, id: TaskId, url: &str) -> HandoffRequest {
        HandoffRequest::new(
            id,
            url,
            &self.downloads,
            "capture.bin",
            WorkerCount::Four,
            None,
        )
        .unwrap()
    }
    fn record(&self, id: TaskId) -> PathBuf {
        self.state.join("tasks").join(format!("{id}.task.json"))
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
        fixture: Fixture {
            len: 128 * 1024,
            seed: 29,
        },
        rules: Vec::new(),
    })
    .unwrap()
}

#[tokio::test]
async fn preparation_survives_restart_without_network_or_ordinary_control_authority() {
    let domain = Domain::new();
    let server = server();
    let id = TaskId::new();
    let url = server.url("/fixture?opaque=synthetic-canary");
    let engine = domain.engine();
    let prepared = engine.prepare_handoff(domain.request(id, &url)).unwrap();
    assert_eq!(prepared.phase(), HandoffPhase::Prepared);
    assert_eq!(
        prepared.task().handoff_phase(),
        Some(HandoffPhase::Prepared)
    );
    assert_eq!(
        engine.snapshots()[0].handoff_phase(),
        Some(HandoffPhase::Prepared)
    );
    assert_eq!(prepared.task().state(), TaskState::Queued);
    assert_eq!(engine.start(id), Err(TaskEngineError::InvalidTaskState));
    assert_eq!(
        engine.resume(id).await,
        Err(TaskEngineError::InvalidTaskState)
    );
    assert_eq!(engine.retry(id), Err(TaskEngineError::InvalidTaskState));
    assert_eq!(
        engine.cancel(id, CancelPartialPolicy::Delete).await,
        Err(TaskEngineError::InvalidTaskState)
    );
    assert_eq!(
        engine.prepare_handoff(domain.request(id, &url)).unwrap(),
        prepared
    );
    assert!(
        !format!("{prepared:?} {:?}", engine.metadata(id).unwrap()).contains("synthetic-canary")
    );
    engine.shutdown().await.unwrap();
    drop(engine);
    let disk: Value = serde_json::from_slice(&fs::read(domain.record(id)).unwrap()).unwrap();
    assert_eq!(disk["version"], 5);
    assert_eq!(disk["handoff"], "prepared");
    let recovered = domain.engine();
    assert_eq!(recovered.handoff_status(id).unwrap(), prepared);
    assert_eq!(
        recovered.prepare_handoff(domain.request(id, &url)).unwrap(),
        prepared
    );
    recovered.shutdown().await.unwrap();
    assert_eq!(recovered.snapshots().len(), 1);
    assert!(server.requests().is_empty());
    assert_eq!(fs::read_dir(&domain.downloads).unwrap().count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_commits_and_restart_create_only_one_independently_correct_output() {
    let domain = Domain::new();
    let server = server();
    let id = TaskId::new();
    let url = server.url("/fixture");
    let engine = domain.engine();
    engine.prepare_handoff(domain.request(id, &url)).unwrap();
    assert!(server.requests().is_empty());
    let mut commits = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let engine = engine.clone();
        commits.spawn(async move { engine.commit_handoff(id) });
    }
    while let Some(result) = commits.join_next().await {
        assert_eq!(result.unwrap().unwrap().phase(), HandoffPhase::Committed);
    }
    let complete = tokio::time::timeout(Duration::from_secs(15), engine.wait_until_inactive(id))
        .await
        .unwrap()
        .unwrap();
    engine.shutdown().await.unwrap();
    assert_eq!(complete.state(), TaskState::Completed);
    assert_eq!(complete.handoff_phase(), Some(HandoffPhase::Committed));
    assert_eq!(engine.snapshots().len(), 1);
    let count = server.requests().len();
    assert!(count > 0);
    let expected = Fixture {
        len: 128 * 1024,
        seed: 29,
    }
    .bytes(0, 128 * 1024, 0);
    assert_eq!(
        fs::read(domain.downloads.join("capture.bin")).unwrap(),
        expected
    );
    assert_eq!(fs::read_dir(&domain.downloads).unwrap().count(), 1);
    assert_eq!(engine.commit_handoff(id).unwrap().task(), &complete);
    assert_eq!(
        engine.abort_handoff(id),
        Err(TaskEngineError::InvalidTaskState)
    );
    assert_eq!(
        engine.remove(id, true),
        Err(TaskEngineError::InvalidTaskState)
    );
    drop(engine);
    let recovered = domain.engine();
    assert_eq!(
        recovered.commit_handoff(id).unwrap().task().state(),
        TaskState::Completed
    );
    assert_eq!(
        recovered
            .prepare_handoff(domain.request(id, &url))
            .unwrap()
            .phase(),
        HandoffPhase::Committed
    );
    recovered.shutdown().await.unwrap();
    assert_eq!(server.requests().len(), count);
    assert_eq!(fs::read_dir(&domain.downloads).unwrap().count(), 1);
}

#[tokio::test]
async fn aborted_identity_and_immutable_inputs_cannot_be_reused() {
    let domain = Domain::new();
    let engine = domain.engine();
    let server = server();
    let id = TaskId::new();
    let url = server.url("/fixture");
    engine.prepare_handoff(domain.request(id, &url)).unwrap();
    let other = Domain::new();
    for (candidate_url, destination, filename, workers, expected) in [
        (
            server.url("/other"),
            &domain.downloads,
            "capture.bin",
            WorkerCount::Four,
            None,
        ),
        (
            url.clone(),
            &other.downloads,
            "capture.bin",
            WorkerCount::Four,
            None,
        ),
        (
            url.clone(),
            &domain.downloads,
            "other.bin",
            WorkerCount::Four,
            None,
        ),
        (
            url.clone(),
            &domain.downloads,
            "capture.bin",
            WorkerCount::One,
            None,
        ),
        (
            url.clone(),
            &domain.downloads,
            "capture.bin",
            WorkerCount::Four,
            ExpectedSha256::parse(&"a".repeat(64)),
        ),
    ] {
        let request =
            HandoffRequest::new(id, &candidate_url, destination, filename, workers, expected)
                .unwrap();
        assert_eq!(
            engine.prepare_handoff(request),
            Err(TaskEngineError::InvalidTaskState)
        );
    }
    let aborted = engine.abort_handoff(id).unwrap();
    assert_eq!(aborted.phase(), HandoffPhase::Aborted);
    assert_eq!(aborted.task().handoff_phase(), Some(HandoffPhase::Aborted));
    assert_eq!(
        engine.snapshots()[0].handoff_phase(),
        Some(HandoffPhase::Aborted)
    );
    assert_eq!(engine.abort_handoff(id).unwrap(), aborted);
    assert_eq!(
        engine.commit_handoff(id),
        Err(TaskEngineError::InvalidTaskState)
    );
    assert_eq!(
        engine.remove(id, true),
        Err(TaskEngineError::InvalidTaskState)
    );
    engine.shutdown().await.unwrap();
    drop(engine);
    let recovered = domain.engine();
    assert_eq!(
        recovered.abort_handoff(id).unwrap().phase(),
        HandoffPhase::Aborted
    );
    assert_eq!(
        recovered
            .prepare_handoff(domain.request(id, &url))
            .unwrap()
            .phase(),
        HandoffPhase::Aborted
    );
    assert_eq!(
        recovered.commit_handoff(id),
        Err(TaskEngineError::InvalidTaskState)
    );
    recovered.shutdown().await.unwrap();
    assert!(server.requests().is_empty());
}

#[test]
fn ordinary_tasks_and_existing_unloaded_files_are_not_adopted() {
    let domain = Domain::new();
    let engine = domain.engine();
    let url = "https://fixture.example.invalid/file";
    let ordinary = engine
        .create_task_default(url, &domain.downloads, "normal.bin")
        .unwrap();
    assert_eq!(ordinary.handoff_phase(), None);
    assert_eq!(
        engine.prepare_handoff(domain.request(ordinary.task_id(), url)),
        Err(TaskEngineError::InvalidTaskState)
    );
    assert_eq!(
        engine.commit_handoff(ordinary.task_id()),
        Err(TaskEngineError::InvalidTaskState)
    );
    let disk: Value =
        serde_json::from_slice(&fs::read(domain.record(ordinary.task_id())).unwrap()).unwrap();
    assert_eq!(disk["version"], 4);
    assert!(disk.get("handoff").is_none());
    let id = TaskId::new();
    fs::write(domain.record(id), b"uncertain initial write").unwrap();
    assert!(engine.prepare_handoff(domain.request(id, url)).is_err());
    assert_eq!(
        fs::read(domain.record(id)).unwrap(),
        b"uncertain initial write"
    );
    assert_eq!(
        engine.handoff_status(id),
        Err(TaskEngineError::TaskNotFound)
    );
}

#[test]
fn malformed_handoff_envelopes_remain_refused_and_preserved() {
    for mutation in 0..7 {
        let domain = Domain::new();
        let engine = domain.engine();
        let id = TaskId::new();
        let url = "https://fixture.example.invalid/file";
        engine.prepare_handoff(domain.request(id, url)).unwrap();
        drop(engine); // Prepared has no owned worker to retire.
        let mut disk: Value =
            serde_json::from_slice(&fs::read(domain.record(id)).unwrap()).unwrap();
        match mutation {
            0 => {
                disk.as_object_mut().unwrap().remove("handoff");
            }
            1 => disk["handoff"] = json!("unknown"),
            2 => disk["task"]["state"] = json!("probing"),
            3 => disk["task"]["needs_session"] = json!(true),
            4 => disk["version"] = json!(4),
            5 => disk["handoff"] = json!("committed"),
            _ => disk["task"]["revision"] = json!(2),
        }
        let bytes = serde_json::to_vec(&disk).unwrap();
        fs::write(domain.record(id), &bytes).unwrap();
        let recovered = domain.engine();
        assert_eq!(recovered.recovery_report().failures().len(), 1);
        assert_eq!(
            recovered.handoff_status(id),
            Err(TaskEngineError::TaskNotFound)
        );
        assert!(recovered.prepare_handoff(domain.request(id, url)).is_err());
        assert_eq!(fs::read(domain.record(id)).unwrap(), bytes);
    }
}

#[test]
fn commit_without_runtime_preserves_the_durable_preparation() {
    let domain = Domain::new();
    let engine = domain.engine();
    let id = TaskId::new();
    engine
        .prepare_handoff(domain.request(id, "https://fixture.example.invalid/file"))
        .unwrap();
    let before = fs::read(domain.record(id)).unwrap();
    assert_eq!(
        engine.commit_handoff(id),
        Err(TaskEngineError::RuntimeUnavailable)
    );
    assert_eq!(
        engine.handoff_status(id).unwrap().phase(),
        HandoffPhase::Prepared
    );
    assert_eq!(fs::read(domain.record(id)).unwrap(), before);
}

#[tokio::test]
async fn recovered_commit_intent_does_not_replay_a_failed_start() {
    let domain = Domain::new();
    let server = server();
    let engine = domain.engine();
    let id = TaskId::new();
    engine
        .prepare_handoff(domain.request(id, &server.url("/fixture")))
        .unwrap();
    drop(engine);
    let mut disk: Value = serde_json::from_slice(&fs::read(domain.record(id)).unwrap()).unwrap();
    // Metadata fixture for crash after durable commit/before worker dispatch;
    // this is not an observed real process crash.
    disk["handoff"] = json!("committed");
    disk["task"]["state"] = json!("probing");
    disk["task"]["revision"] = json!(2);
    fs::write(domain.record(id), serde_json::to_vec(&disk).unwrap()).unwrap();
    let recovered = domain.engine();
    assert_eq!(
        recovered.handoff_status(id).unwrap().phase(),
        HandoffPhase::Committed
    );
    assert_eq!(
        recovered.commit_handoff(id).unwrap().task().state(),
        TaskState::Failed
    );
    recovered.shutdown().await.unwrap();
    assert!(server.requests().is_empty());
}

#[cfg(windows)]
#[tokio::test]
async fn refused_commit_and_abort_checkpoints_cannot_authorize_network() {
    use std::os::windows::fs::OpenOptionsExt;
    for commit in [false, true] {
        let domain = Domain::new();
        let server = server();
        let engine = domain.engine();
        let id = TaskId::new();
        engine
            .prepare_handoff(domain.request(id, &server.url("/fixture")))
            .unwrap();
        let before = fs::read(domain.record(id)).unwrap();
        let lease = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(domain.record(id))
            .unwrap();
        let outcome = if commit {
            engine.commit_handoff(id)
        } else {
            engine.abort_handoff(id)
        };
        let observed_phase = engine.handoff_status(id).unwrap().phase();
        // Retire any accidentally authorized worker before the regression asserts.
        let shutdown = engine.shutdown().await;
        assert!(outcome.is_err());
        assert_eq!(observed_phase, HandoffPhase::Prepared);
        shutdown.unwrap();
        assert!(server.requests().is_empty());
        assert_eq!(fs::read(domain.record(id)).unwrap(), before);
        drop(lease);
        assert_eq!(
            engine.abort_handoff(id).unwrap().phase(),
            HandoffPhase::Aborted
        );
    }
}

#[test]
fn persistence_cleanup_cannot_forget_an_aborted_handoff() {
    use download_manager_engine::persistence::{PartialCleanup, PersistenceError, TaskStore};
    let domain = Domain::new();
    let engine = domain.engine();
    let id = TaskId::new();
    engine
        .prepare_handoff(domain.request(id, "https://fixture.example.invalid/file"))
        .unwrap();
    engine.abort_handoff(id).unwrap();
    drop(engine);
    let store = TaskStore::open(&domain.state).unwrap();
    let loaded = store.load_all().unwrap();
    assert_eq!(loaded.tasks().len(), 1);
    assert_eq!(
        store.cleanup_terminal(&loaded.tasks()[0], PartialCleanup::Delete),
        Err(PersistenceError::HandoffRetained)
    );
    assert!(domain.record(id).exists());
}
