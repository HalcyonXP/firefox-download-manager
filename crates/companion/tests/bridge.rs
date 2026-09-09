#![cfg(windows)]
use download_manager_companion::worker::Worker;
use download_manager_engine::{
    persistence::{PersistenceError, TaskState},
    task::TaskEngineError,
};
use download_manager_local_ipc::{
    Capability, Channel, Endpoint, FrameReader, FrameWriter, LocalPipe, Server, connect,
};
use download_manager_native_host::{EngineOwner, HostConfig, HostError};
use download_manager_test_server::{ByteRange, Fixture, RequestSelector, ServerConfig, TestServer};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc, time::Duration};

struct Domain(PathBuf);
impl Domain {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("dm-engine-ipc-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("downloads")).unwrap();
        Self(path)
    }
    fn config(&self) -> HostConfig {
        HostConfig::new(self.0.join("state"), Some(self.0.join("downloads")))
    }
}
impl Drop for Domain {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
async fn until(stage: &str, mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(15), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("owned engine/IPC observation deadline: {stage}"));
}
fn locked(domain: &Domain) {
    assert!(matches!(
        EngineOwner::open(&domain.config()),
        Err(HostError::Engine(TaskEngineError::Persistence(
            PersistenceError::StoreLocked
        )))
    ));
}
async fn joined(worker: &mut Worker) {
    worker.request_stop();
    let mut result = None;
    until("worker join", || {
        result = worker.try_join();
        result.is_some()
    })
    .await;
    assert_eq!(
        result,
        Some(Ok(())),
        "retained companion worker must join successfully"
    );
}
struct Peer {
    reader: FrameReader<tokio::io::ReadHalf<LocalPipe>>,
    writer: FrameWriter<tokio::io::WriteHalf<LocalPipe>>,
}
impl Peer {
    fn new(channel: Channel<LocalPipe>) -> Self {
        let (reader, writer) = channel.split();
        Self { reader, writer }
    }
    async fn read(&mut self) -> Value {
        let body = tokio::time::timeout(Duration::from_secs(5), self.reader.read())
            .await
            .expect("owned response deadline")
            .expect("local frame");
        serde_json::from_slice(&body).unwrap()
    }
    async fn send(&mut self, name: &str, payload: Value, id: &str) {
        self.writer.write(&serde_json::to_vec(&json!({"protocol_version": 2, "kind":"command", "command": name, "correlation_id":id, "payload":payload})).unwrap()).await.unwrap();
    }
    async fn hello(&mut self) -> Vec<Value> {
        self.send("hello", json!({"supported_versions":[2],"client_name":"owned-engine-ipc-test","client_version":"0.1.0"}), "hello").await;
        let response = self.read().await;
        assert_eq!(response["ok"], true);
        assert_eq!(response["command"], "hello");
        let mut tasks = Vec::new();
        let mut sequence = 0;
        loop {
            let page = self.read().await;
            assert_eq!(page["event"], "snapshot");
            assert_eq!(page["sequence"], sequence);
            sequence += 1;
            tasks.extend(page["data"]["tasks"].as_array().unwrap().iter().cloned());
            if page["data"]["complete"] == true {
                return tasks;
            }
        }
    }
    async fn response(&mut self, id: &str) -> Value {
        loop {
            let value = self.read().await;
            if value["kind"] == "response" && value["correlation_id"] == id {
                return value;
            }
        }
    }
}

#[tokio::test]
async fn pipe_disconnect_preserves_transfer_and_reconnect_observes_one_correct_output() {
    let domain = Domain::new();
    let fixture = Fixture {
        len: 64 * 1024,
        seed: 51,
    };
    let http = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: Vec::new(),
    })
    .unwrap();
    let gate = http
        .pause_responses(RequestSelector {
            range: Some(ByteRange { start: 0, end: 0 }),
            ..Default::default()
        })
        .unwrap();
    let endpoint = Endpoint::generate().unwrap();
    let key = Arc::new(Capability::generate().unwrap());
    let mut worker = Worker::start_local(domain.config(), endpoint, Arc::clone(&key)).unwrap();
    until("worker ready", || worker.take_ready()).await;
    let mut first = Peer::new(connect(endpoint, &key).await.unwrap());
    assert!(first.hello().await.is_empty());
    first
        .send(
            "add",
            json!({"url":http.url("/fixture"), "suggested_filename":"owned.bin"}),
            "add",
        )
        .await;
    let add = first.response("add").await;
    assert_eq!(add["ok"], true);
    let id = add["result"]["task_id"].as_str().unwrap().to_owned();
    until("fixture request", || !http.requests().is_empty()).await;
    drop(first);
    locked(&domain);
    assert!(!domain.0.join("downloads/owned.bin").exists());
    drop(gate);
    until("final file promotion", || {
        domain.0.join("downloads/owned.bin").exists()
    })
    .await;
    assert_eq!(
        fs::read(domain.0.join("downloads/owned.bin")).unwrap(),
        fixture.bytes(0, 64 * 1024, 0)
    );
    locked(&domain);
    let mut second = Peer::new(connect(endpoint, &key).await.unwrap());
    let tasks = second.hello().await;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["task_id"], id);
    // Exclusive file promotion precedes the durable Completed transition.
    // Observe that separate receipt, rather than inferring it from file presence.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            second.send("get", json!({"task_id": id}), "terminal").await;
            let response = second.response("terminal").await;
            assert_eq!(response["ok"], true);
            if response["result"]["state"] == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("durable completed receipt");
    joined(&mut worker).await;
    assert!(
        second.reader.read().await.is_err(),
        "joined Quit must close the real peer"
    );
    drop(second);
    let reopened = EngineOwner::open(&domain.config()).unwrap();
    assert_eq!(reopened.engine().snapshots().len(), 1);
    assert_eq!(
        reopened.engine().snapshots()[0].state(),
        TaskState::Completed
    );
    reopened.shutdown().await.unwrap();
    drop(reopened);
    let rebound = Server::bind(endpoint, key).unwrap();
    drop(rebound);
}

#[tokio::test]
async fn incremental_history_and_busy_controller_do_not_create_another_owner() {
    let domain = Domain::new();
    let initial = EngineOwner::open(&domain.config()).unwrap();
    for index in 0..80 {
        initial
            .engine()
            .create_task_default(
                &format!("https://example.invalid/{index}"),
                &domain.0.join("downloads"),
                &format!("{index}.bin"),
            )
            .unwrap();
    }
    initial.shutdown().await.unwrap();
    drop(initial);
    let endpoint = Endpoint::generate().unwrap();
    let key = Arc::new(Capability::generate().unwrap());
    let mut worker = Worker::start_local(domain.config(), endpoint, Arc::clone(&key)).unwrap();
    until("worker ready", || worker.take_ready()).await;
    let mut first = Peer::new(connect(endpoint, &key).await.unwrap());
    assert_eq!(first.hello().await.len(), 80);
    assert!(
        matches!(
            connect(endpoint, &key).await,
            Err(download_manager_local_ipc::Error::Deadline)
        ),
        "second controller must not negotiate or dispatch"
    );
    locked(&domain);
    first.send("get_settings", json!({}), "settings").await;
    assert_eq!(first.response("settings").await["ok"], true);
    drop(first);
    // Only establishment is retried while the listener transitions; no command
    // has been sent, and no uncertain Add or other operation is replayed.
    let mut next = None;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(channel) = connect(endpoint, &key).await {
                next = Some(Peer::new(channel));
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("controller reconnect after refused peer");
    let mut next = next.unwrap();
    assert_eq!(next.hello().await.len(), 80);
    joined(&mut worker).await;
    assert!(next.reader.read().await.is_err());
    drop(next);
    let rebound = Server::bind(endpoint, key).unwrap();
    drop(rebound);
}

#[tokio::test]
async fn stalled_bidirectional_client_does_not_prevent_joined_quit() {
    let domain = Domain::new();
    let initial = EngineOwner::open(&domain.config()).unwrap();
    for index in 0..800 {
        initial
            .engine()
            .create_task_default(
                &format!("https://example.invalid/{index}"),
                &domain.0.join("downloads"),
                &format!("{index}.bin"),
            )
            .unwrap();
    }
    initial.shutdown().await.unwrap();
    drop(initial);
    let endpoint = Endpoint::generate().unwrap();
    let key = Arc::new(Capability::generate().unwrap());
    let mut worker = Worker::start_local(domain.config(), endpoint, Arc::clone(&key)).unwrap();
    until("worker ready", || worker.take_ready()).await;
    let mut peer = Peer::new(connect(endpoint, &key).await.unwrap());
    peer.send("hello", json!({"supported_versions":[2],"client_name":"owned-nonreading-test","client_version":"0.1.0"}), "hello").await;
    let mut command = serde_json::to_vec(&json!({"protocol_version":2,"kind":"command","command":"get_settings","correlation_id":"bounded-input","payload":{}})).unwrap();
    command.resize(download_manager_local_ipc::MAX_FRAME, b' ');
    let mut stalled = false;
    for _ in 0..32 {
        match tokio::time::timeout(Duration::from_millis(100), peer.writer.write(&command)).await {
            Err(_) => {
                stalled = true;
                break;
            }
            Ok(result) => result.expect("fixture transport must remain open until stall"),
        }
    }
    // This is observed pending client output, not an inferred kernel byte count.
    // Cancelling that write retires its direction; it is deliberately never reused.
    joined(&mut worker).await;
    let closure = tokio::time::timeout(Duration::from_secs(5), async {
        while peer.reader.read().await.is_ok() {}
    })
    .await;
    drop(peer);
    let reopened = EngineOwner::open(&domain.config()).unwrap();
    let count = reopened.engine().snapshots().len();
    reopened.shutdown().await.unwrap();
    drop(reopened);
    let rebound = Server::bind(endpoint, key).unwrap();
    drop(rebound);
    assert!(stalled, "fixture did not establish pending client output");
    assert!(closure.is_ok(), "joined quit did not close the real peer");
    assert_eq!(count, 800);
    assert_eq!(fs::read_dir(domain.0.join("downloads")).unwrap().count(), 0);
}

#[tokio::test]
async fn refused_command_and_reconnect_preserve_current_owner_settings() {
    let domain = Domain::new();
    let destination = domain.0.join("next");
    fs::create_dir(&destination).unwrap();
    let endpoint = Endpoint::generate().unwrap();
    let key = Arc::new(Capability::generate().unwrap());
    let mut worker = Worker::start_local(domain.config(), endpoint, Arc::clone(&key)).unwrap();
    until("worker ready", || worker.take_ready()).await;
    let mut first = Peer::new(connect(endpoint, &key).await.unwrap());
    assert!(first.hello().await.is_empty());
    first
        .send(
            "update_settings",
            json!({"settings":{"destination":destination.to_str().unwrap()}}),
            "update",
        )
        .await;
    assert_eq!(first.response("update").await["ok"], true);
    first
        .send(
            "add",
            json!({"url":"file:///not-a-download","suggested_filename":"refused.bin"}),
            "refused",
        )
        .await;
    assert_eq!(first.response("refused").await["ok"], false);
    locked(&domain);
    drop(first);
    let channel = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(channel) = connect(endpoint, &key).await {
                break channel;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned reconnect establishment, before any command");
    let mut next = Peer::new(channel);
    assert!(next.hello().await.is_empty());
    next.send("get_settings", json!({}), "current").await;
    let settings = next.response("current").await;
    assert_eq!(settings["ok"], true);
    assert_eq!(
        settings["result"]["destination"],
        destination.to_str().unwrap()
    );
    joined(&mut worker).await;
    assert!(next.reader.read().await.is_err());
    drop(next);
    let reopened = EngineOwner::open(&domain.config()).unwrap();
    assert!(reopened.engine().snapshots().is_empty());
    reopened.shutdown().await.unwrap();
    drop(reopened);
    assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
    let rebound = Server::bind(endpoint, key).unwrap();
    drop(rebound);
}

#[tokio::test]
async fn idle_controller_quit_retires_both_directions_before_success() {
    let domain = Domain::new();
    let endpoint = Endpoint::generate().unwrap();
    let key = Arc::new(Capability::generate().unwrap());
    let mut worker = Worker::start_local(domain.config(), endpoint, Arc::clone(&key)).unwrap();
    until("worker ready", || worker.take_ready()).await;
    let mut peer = Peer::new(connect(endpoint, &key).await.unwrap());
    assert!(peer.hello().await.is_empty());
    joined(&mut worker).await;
    assert!(peer.reader.read().await.is_err());
    drop(peer);
    let reopened = EngineOwner::open(&domain.config()).unwrap();
    reopened.shutdown().await.unwrap();
    drop(reopened);
    let rebound = Server::bind(endpoint, key).unwrap();
    drop(rebound);
}
