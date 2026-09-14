use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use download_manager_companion::worker::Worker;
use download_manager_engine::persistence::{PersistenceError, TaskState};
use download_manager_engine::scheduler::WorkerCount;
use download_manager_engine::task::TaskEngineError;
use download_manager_native_host::{EngineOwner, HostConfig, HostError};
use download_manager_test_server::{ByteRange, Fixture, RequestSelector, ServerConfig, TestServer};

struct Domain(PathBuf);
impl Domain {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("dm-companion-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).expect("exclusive test domain");
        fs::create_dir(root.join("downloads")).expect("owned destination");
        Self(root)
    }
    fn config(&self) -> HostConfig {
        HostConfig::new(self.0.join("state"), Some(self.0.join("downloads")))
    }
}
impl Drop for Domain {
    fn drop(&mut self) {
        // Exact test-created domain only, after owners/fixtures have been joined.
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).expect("owned domain should be unlocked");
        }
    }
}

fn assert_locked(config: &HostConfig) {
    assert!(matches!(
        EngineOwner::open(config),
        Err(HostError::Engine(TaskEngineError::Persistence(
            PersistenceError::StoreLocked
        )))
    ));
}

async fn until(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(15), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("owned test containment deadline");
}

#[tokio::test]
async fn dropped_in_process_observer_does_not_own_engine_lifetime() {
    let domain = Domain::new();
    let fixture = Fixture {
        len: 64 * 1024,
        seed: 50,
    };
    let server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: Vec::new(),
    })
    .unwrap();
    let gate = server
        .pause_responses(RequestSelector {
            range: Some(ByteRange { start: 0, end: 0 }),
            ..Default::default()
        })
        .unwrap();
    let config = domain.config();
    let owner = EngineOwner::open(&config).unwrap();
    assert_locked(&config);
    let task = owner
        .engine()
        .create_task_default(
            &server.url("/fixture"),
            &domain.0.join("downloads"),
            "owned.bin",
        )
        .unwrap();
    let id = task.task_id();
    let observer = owner.engine().subscribe(id).unwrap();
    owner.engine().start(id).unwrap();
    until(|| !server.requests().is_empty()).await;
    drop(observer); // This is an in-process observer, NOT actual Firefox IPC.
    assert!(!domain.0.join("downloads/owned.bin").exists());
    assert_locked(&config);
    drop(gate);
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        owner.engine().wait_until_inactive(id),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.state(), TaskState::Completed);
    let bytes = fs::read(domain.0.join("downloads/owned.bin")).unwrap();
    assert_eq!(bytes, fixture.bytes(0, 64 * 1024, 0));
    let reconnected = owner.engine().subscribe(id).unwrap();
    drop(reconnected);
    assert_eq!(owner.engine().snapshots().len(), 1);
    owner.shutdown().await.unwrap();
    assert_locked(&config); // shutdown acknowledgement is not lock release
    drop(owner);
    let reopened = EngineOwner::open(&config).unwrap();
    assert_eq!(
        reopened.engine().snapshot(id).unwrap().state(),
        TaskState::Completed
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_checkpoints_waiting_transfer_before_unlock_and_recovery() {
    let domain = Domain::new();
    let fixture = Fixture {
        len: 64 * 1024,
        seed: 51,
    };
    let server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: Vec::new(),
    })
    .unwrap();
    let assignment = ByteRange {
        start: 0,
        end: fixture.len - 1,
    };
    let gate = server
        .pause_responses(RequestSelector {
            range: Some(assignment),
            ..Default::default()
        })
        .unwrap();
    let config = domain.config();
    let owner = EngineOwner::open(&config).unwrap();
    let task = owner
        .engine()
        .create_task(
            &server.url("/fixture"),
            &domain.0.join("downloads"),
            "recovery.bin",
            WorkerCount::One,
        )
        .unwrap();
    let id = task.task_id();
    owner.engine().start(id).unwrap();
    until(|| {
        server
            .requests()
            .iter()
            .any(|r| r.range == Some(assignment))
    })
    .await;
    assert_eq!(
        owner.engine().snapshot(id).unwrap().state(),
        TaskState::Downloading
    );
    tokio::time::timeout(Duration::from_secs(15), owner.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        owner.engine().snapshot(id).unwrap().state(),
        TaskState::Paused
    );
    assert!(!domain.0.join("downloads/recovery.bin").exists());
    assert_locked(&config);
    drop(owner);
    drop(gate);
    let recovered = EngineOwner::open(&config).unwrap();
    assert_eq!(
        recovered.engine().snapshot(id).unwrap().state(),
        TaskState::Paused
    );
    assert!(
        recovered
            .engine()
            .metadata(id)
            .unwrap()
            .completed_ranges()
            .is_empty()
    );
    recovered.engine().resume(id).await.unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        recovered.engine().wait_until_inactive(id),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.state(), TaskState::Completed);
    assert_eq!(
        fs::read(domain.0.join("downloads/recovery.bin")).unwrap(),
        fixture.bytes(0, 64 * 1024, 0)
    );
    recovered.shutdown().await.unwrap();
}

#[tokio::test]
async fn retained_worker_joins_on_early_quit_and_releases_its_state_lock() {
    let domain = Domain::new();
    let mut worker = Worker::start(domain.config()).unwrap();
    worker.request_stop(); // before consuming any startup acknowledgement
    let mut joined = None;
    until(|| {
        joined = worker.try_join();
        joined.is_some()
    })
    .await;
    assert_eq!(joined, Some(Ok(())));
    assert!(worker.try_join().is_none());
    let owner = EngineOwner::open(&domain.config()).unwrap();
    owner.shutdown().await.unwrap();
}
