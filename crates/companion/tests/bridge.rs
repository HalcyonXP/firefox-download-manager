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
#[tokio::test]
async fn ordinary_dispatch_refuses_parent_class_before_any_application_frame() {
    let domain = Domain::new();
    let mut owner = EngineOwner::open(&domain.config()).unwrap();
    let endpoint = Endpoint::generate().unwrap();
    let key = Arc::new(Capability::generate().unwrap());
    let server = Server::bind(endpoint, Arc::clone(&key)).unwrap();
    let (s, c) = tokio::join!(
        server.accept_with_browser_parent(),
        download_manager_local_ipc::connect_browser_parent(endpoint, &key)
    );
    let channel = s.unwrap();
    let watch = channel.cancellation();
    let (_stop, mut stopped) = tokio::sync::oneshot::channel();
    let peer = async {
        let (mut reader, mut writer) = c.unwrap().split();
        let _sent = writer
            .write(include_bytes!(
                "../../../protocol/schema/v2/examples/hello.command.json"
            ))
            .await;
        let received = tokio::time::timeout(Duration::from_secs(5), reader.read()).await;
        let observation = (
            matches!(received, Ok(Ok(_))),
            matches!(
                received,
                Ok(Err(download_manager_local_ipc::Error::Transport))
            ),
        );
        drop((reader, writer));
        observation
    };
    let (outcome, (frame_received, closed)) =
        tokio::join!(owner.serve_local(channel, &mut stopped), peer);
    let tasks = owner.engine().snapshots().len();
    let shutdown = owner.shutdown().await;
    drop(owner);
    let failed = server.cancellation_failed();
    drop(server);
    let reopened = EngineOwner::open(&domain.config()).unwrap();
    let reopened_tasks = reopened.engine().snapshots().len();
    let reopened_shutdown = reopened.shutdown().await;
    drop(reopened);
    drop(Server::bind(endpoint, key).unwrap());
    assert!(shutdown.is_ok() && reopened_shutdown.is_ok());
    assert!(!failed);
    assert_eq!(
        watch.status(),
        download_manager_local_ipc::CancellationStatus::Requested
    );
    assert_eq!((tasks, reopened_tasks), (0, 0));
    assert!(
        !frame_received,
        "ordinary dispatch accepted parent-class application input"
    );
    assert!(closed, "peer closure, not an idle deadline, is required");
    assert!(matches!(outcome, Err(HostError::LocalSession)));
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
    async fn closed_after_events(&mut self) -> Result<usize, &'static str> {
        // Closure does not erase already buffered frames. Still require actual
        // transport termination, not a frame/parser error or an idle timeout.
        tokio::time::timeout(Duration::from_secs(5), async {
            for count in 0..=128 {
                match self.reader.read().await {
                    Err(download_manager_local_ipc::Error::Transport) => return Ok(count),
                    Ok(body) if count < 128 => {
                        let value: Value =
                            serde_json::from_slice(&body).map_err(|_| "invalid terminal frame")?;
                        if value["kind"] != "event" {
                            return Err("unexpected terminal response");
                        }
                    }
                    _ => return Err("peer closure not established"),
                }
            }
            Err("terminal event bound exceeded")
        })
        .await
        .map_err(|_| "peer closure observation deadline")?
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
        assert!(
            response["result"]["capabilities"]
                .as_array()
                .unwrap()
                .contains(&json!("prepared_handoff"))
        );
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
    assert_eq!(
        fs::read(domain.0.join("downloads/owned.bin")).unwrap(),
        fixture.bytes(0, 64 * 1024, 0)
    );
    joined(&mut worker).await;
    let count = second
        .closed_after_events()
        .await
        .expect("joined Quit must close the real peer after buffered events");
    eprintln!("post-join event frames before EOF: {count}");
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

#[tokio::test]
async fn buffered_event_is_not_erased_by_sender_pipe_closure() {
    let endpoint = Endpoint::generate().unwrap();
    let key = Arc::new(Capability::generate().unwrap());
    let server = Server::bind(endpoint, Arc::clone(&key)).unwrap();
    let (accepted, connected) = tokio::join!(server.accept(), connect(endpoint, &key));
    let (read, mut write) = accepted.unwrap().split();
    let mut peer = Peer::new(connected.unwrap());
    write.write(&serde_json::to_vec(&json!({"protocol_version":2,"kind":"event","event":"snapshot","sequence":0,"data":{"tasks":[],"complete":true}})).unwrap()).await.unwrap();
    drop(read);
    drop(write);
    drop(server);
    assert_eq!(peer.closed_after_events().await, Ok(1));
}

#[tokio::test]
async fn an_idle_live_peer_is_not_misclassified_as_closed() {
    let endpoint = Endpoint::generate().unwrap();
    let key = Arc::new(Capability::generate().unwrap());
    let server = Server::bind(endpoint, Arc::clone(&key)).unwrap();
    let (accepted, connected) = tokio::join!(server.accept(), connect(endpoint, &key));
    let accepted = accepted.unwrap();
    let mut peer = Peer::new(connected.unwrap());
    assert_eq!(
        peer.closed_after_events().await,
        Err("peer closure observation deadline")
    );
    drop(peer);
    drop(accepted);
    drop(server);
}

#[derive(Debug, Default, PartialEq, Eq)]
struct HandoffObservation {
    responses: u64,
    waiting: &'static str,
    last_state: Option<TaskState>,
    last_bytes: Option<u64>,
    last_failure: Option<&'static str>,
}
impl HandoffObservation {
    fn record(&mut self, response: &Value) {
        self.responses = self.responses.saturating_add(1);
        let task = &response["result"]["task"];
        self.last_state = serde_json::from_value(task["state"].clone()).ok();
        self.last_bytes = task["bytes_completed"].as_u64();
        // Never format a wire response or accept an arbitrary diagnostic string.
        self.last_failure = if task["error"].is_null() {
            None
        } else {
            Some(
                [
                    "CHECKSUM_MISMATCH",
                    "AUTH_REQUIRED",
                    "AUTH_EXPIRED",
                    "REDIRECT_REJECTED",
                    "CANCELLED",
                    "PROBE_FAILED",
                    "HTTP_STATUS",
                    "RANGE_RESPONSE_INVALID",
                    "RESOURCE_CHANGED",
                    "RETRY_EXHAUSTED",
                    "STORAGE_ERROR",
                    "DISK_FULL",
                    "ACCESS_DENIED",
                    "FILE_LOCKED",
                    "FILE_EXISTS",
                    "STATE_CORRUPT",
                    "INTERNAL_ERROR",
                ]
                .into_iter()
                .find(|code| task["error"]["code"].as_str() == Some(*code))
                .unwrap_or("unrecognized"),
            )
        };
    }
}

#[test]
fn handoff_observation_retains_only_closed_classifications_and_counts() {
    let mut observation = HandoffObservation::default();
    observation.record(
        &json!({"result":{"task":{"state":"failed", "bytes_completed":12,
        "error":{"code":"PROBE_FAILED", "context":{"other":"synthetic-discarded"}}}}}),
    );
    assert_eq!(observation.last_state, Some(TaskState::Failed));
    assert_eq!(observation.last_bytes, Some(12));
    assert_eq!(observation.last_failure, Some("PROBE_FAILED"));
    observation.record(
        &json!({"result":{"task":{"state":"synthetic-discarded", "bytes_completed":-1,
        "error":{"code":"synthetic-discarded"}}}}),
    );
    assert_eq!(observation.responses, 2);
    assert_eq!(observation.last_state, None);
    assert_eq!(observation.last_bytes, None);
    assert_eq!(observation.last_failure, Some("unrecognized"));
    assert!(!format!("{observation:?}").contains("synthetic-discarded"));
}

async fn completed_handoff(peer: &mut Peer, id: &str) -> Result<(), HandoffObservation> {
    let mut observation = HandoffObservation::default();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            observation.waiting = "send";
            peer.send("get_handoff", json!({"task_id":id}), "complete")
                .await;
            observation.waiting = "response";
            let response = peer.response("complete").await;
            observation.record(&response);
            assert_eq!(response["ok"], true);
            assert_eq!(response["result"]["phase"], "committed");
            if response["result"]["task"]["state"] == "completed" {
                break;
            }
            observation.waiting = "poll delay";
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .map_err(|_| observation)
}

async fn complete_or_retire(
    mut peer: Peer,
    id: &str,
    worker: &mut Worker,
    http: TestServer,
) -> (Peer, TestServer) {
    if let Err(observation) = completed_handoff(&mut peer, id).await {
        eprintln!("handoff completion deadline: {observation:?}");
        drop(peer);
        joined(worker).await;
        drop(http);
        panic!("durable handoff completion not observed; worker joined and fixture retired");
    }
    (peer, http)
}

#[tokio::test]
async fn lost_prepare_and_commit_replies_recover_by_id_without_a_second_transfer() {
    let domain = Domain::new();
    let fixture = Fixture {
        len: 64 * 1024,
        seed: 61,
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
    until("handoff worker ready", || worker.take_ready()).await;
    let id = uuid::Uuid::new_v4().to_string();
    let payload = json!({"task_id":id, "download":{"url":http.url("/fixture"), "suggested_filename":"handoff.bin"}});
    let mut first = Peer::new(connect(endpoint, &key).await.unwrap());
    assert!(first.hello().await.is_empty());
    first
        .send("prepare_handoff", payload.clone(), "prepare")
        .await;
    let record = domain.0.join("state/tasks").join(format!("{id}.task.json"));
    until("owned preparation bytes", || {
        fs::read(&record)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|record| record["version"] == 5 && record["handoff"] == "prepared")
    })
    .await;
    drop(first); // No application receipt read, even if a reply reached the pipe.
    assert!(http.requests().is_empty());
    let mut second = Peer::new(connect(endpoint, &key).await.unwrap());
    let prepared_tasks = second.hello().await;
    assert_eq!(prepared_tasks.len(), 1);
    assert_eq!(prepared_tasks[0]["handoff_phase"], "prepared");
    second
        .send("prepare_handoff", payload, "repeat-prepare")
        .await;
    let prepared = second.response("repeat-prepare").await;
    assert_eq!(prepared["ok"], true);
    assert_eq!(prepared["result"]["phase"], "prepared");
    assert_eq!(prepared["result"]["task"]["handoff_phase"], "prepared");
    second
        .send("resume", json!({"task_id":id}), "forbidden")
        .await;
    assert_eq!(
        second.response("forbidden").await["error"]["code"],
        "INVALID_TASK_STATE"
    );
    assert!(http.requests().is_empty());
    second
        .send("commit_handoff", json!({"task_id":id}), "lost-commit")
        .await;
    until("committed network dispatch", || !http.requests().is_empty()).await;
    drop(second); // Uncertain commit reply is not replaced with Add.
    let mut third = Peer::new(connect(endpoint, &key).await.unwrap());
    let tasks = third.hello().await;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["task_id"], id);
    assert_eq!(tasks[0]["handoff_phase"], "committed");
    third
        .send("commit_handoff", json!({"task_id":id}), "repeat-commit")
        .await;
    assert_eq!(
        third.response("repeat-commit").await["result"]["phase"],
        "committed"
    );
    drop(gate);
    let (mut third, http) = complete_or_retire(third, &id, &mut worker, http).await;
    let count = http.requests().len();
    assert_eq!(
        fs::read(domain.0.join("downloads/handoff.bin")).unwrap(),
        fixture.bytes(0, 64 * 1024, 0)
    );
    assert_eq!(fs::read_dir(domain.0.join("downloads")).unwrap().count(), 1);
    third
        .send("commit_handoff", json!({"task_id":id}), "completed-commit")
        .await;
    assert_eq!(
        third.response("completed-commit").await["result"]["task"]["state"],
        "completed"
    );
    joined(&mut worker).await;
    third.closed_after_events().await.unwrap();
    drop(third);
    assert_eq!(http.requests().len(), count);
    assert_reopened_handoff(&domain, &id).await;
}

async fn assert_reopened_handoff(domain: &Domain, id: &str) {
    let reopened = EngineOwner::open(&domain.config()).unwrap();
    assert_eq!(reopened.engine().snapshots().len(), 1);
    assert_eq!(
        reopened
            .engine()
            .commit_handoff(download_manager_engine::persistence::TaskId::parse(id).unwrap())
            .unwrap()
            .task()
            .state(),
        TaskState::Completed
    );
    reopened.shutdown().await.unwrap();
}
