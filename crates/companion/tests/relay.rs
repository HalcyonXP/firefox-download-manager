#![cfg(all(windows, feature = "installed"))]
use download_manager_companion::{
    relay::{self, RelayError},
    worker::Worker,
};
use download_manager_local_ipc::{Capability, Endpoint, Server, connect};
use download_manager_native_host::{EngineOwner, HostConfig};
use download_manager_protocol::{MAX_MESSAGE_BYTES, read_frame};
use download_manager_test_server::{ByteRange, Fixture, RequestSelector, ServerConfig, TestServer};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    thread::JoinHandle,
    time::Duration,
};
const APP: &str = env!("CARGO_BIN_EXE_download-manager-app");

struct Peer {
    input: Option<std::io::PipeWriter>,
    received: Option<tokio::sync::mpsc::Receiver<Value>>,
    reader: Option<JoinHandle<()>>,
    relay: Option<tokio::task::JoinHandle<Result<(), RelayError>>>,
}
impl Peer {
    async fn start(endpoint: Endpoint, key: &Capability) -> Self {
        let channel = connect(endpoint, key).await.unwrap();
        let (input, writer) = std::io::pipe().unwrap();
        let (mut reader, output) = std::io::pipe().unwrap();
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        let thread = std::thread::spawn(move || {
            while let Ok(Some(body)) = read_frame(&mut reader) {
                let Ok(value) = serde_json::from_slice(&body) else {
                    break;
                };
                if sender.blocking_send(value).is_err() {
                    break;
                }
            }
        });
        let relay = tokio::spawn(relay::forward(
            channel,
            Path::new(APP),
            Stdio::from(input),
            Stdio::from(output),
        ));
        Self {
            input: Some(writer),
            received: Some(receiver),
            reader: Some(thread),
            relay: Some(relay),
        }
    }
    fn send(&mut self, name: &str, payload: &Value, id: &str) {
        let body = serde_json::to_vec(&json!({"protocol_version":2,"kind":"command","command":name,"correlation_id":id,"payload":payload})).unwrap();
        let writer = self.input.as_mut().unwrap();
        writer
            .write_all(&u32::try_from(body.len()).unwrap().to_le_bytes())
            .unwrap();
        writer.write_all(&body).unwrap();
    }
    async fn read(&mut self) -> Value {
        tokio::time::timeout(
            Duration::from_secs(5),
            self.received.as_mut().unwrap().recv(),
        )
        .await
        .unwrap()
        .unwrap()
    }
    async fn response(&mut self, id: &str) -> Value {
        loop {
            let value = self.read().await;
            if value["kind"] == "response" && value["correlation_id"] == id {
                return value;
            }
        }
    }
    async fn hello(&mut self) -> Vec<Value> {
        self.send("hello", &json!({"supported_versions":[2],"client_name":"owned-relay-test","client_version":"0.1.0"}), "hello");
        assert_eq!(self.response("hello").await["ok"], true);
        let mut tasks = Vec::new();
        loop {
            let value = self.read().await;
            assert_eq!(value["event"], "snapshot");
            tasks.extend(value["data"]["tasks"].as_array().unwrap().iter().cloned());
            if value["data"]["complete"] == true {
                return tasks;
            }
        }
    }
    async fn finish(mut self) {
        self.input.take();
        let mut task = self.relay.take().unwrap();
        let result = tokio::time::timeout(Duration::from_secs(10), &mut task).await;
        if result.is_err() {
            task.abort();
            let _ = task.await;
        }
        self.received.take();
        self.reader.take().unwrap().join().unwrap();
        assert!(
            matches!(result, Ok(Ok(Ok(())))),
            "native relay must close and join cleanly on input EOF"
        );
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.input.take();
        self.received.take();
        if let Some(task) = self.relay.take() {
            task.abort();
        }
        // Multi-threaded test runtime can retire the aborted relay's exact
        // children while this failure-path reader waits for actual pipe EOF.
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
struct Domain(PathBuf);
impl Domain {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("dm-relay-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("downloads")).unwrap();
        Self(root)
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
async fn until(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(15), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("owned integration observation deadline");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_stdio_disconnect_and_reconnect_preserve_one_correct_engine_task() {
    let domain = Domain::new();
    let fixture = Fixture {
        len: 64 * 1024,
        seed: 57,
    };
    let http = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: vec![],
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
    until(|| worker.take_ready()).await;
    let mut first = Peer::start(endpoint, &key).await;
    assert!(first.hello().await.is_empty());
    first.send(
        "add",
        &json!({"url":http.url("/fixture"),"suggested_filename":"relay.bin"}),
        "add",
    );
    let response = first.response("add").await;
    assert_eq!(response["ok"], true);
    let id = response["result"]["task_id"].as_str().unwrap().to_owned();
    until(|| !http.requests().is_empty()).await;
    first.finish().await;
    assert!(EngineOwner::open(&domain.config()).is_err());
    drop(gate);
    until(|| domain.0.join("downloads/relay.bin").exists()).await;
    let mut second = Peer::start(endpoint, &key).await;
    let tasks = second.hello().await;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["task_id"], id);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            second.send("get", &json!({"task_id":id}), "terminal");
            let value = second.response("terminal").await;
            if value["result"]["state"] == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("independent durable Completed receipt");
    assert_eq!(
        fs::read(domain.0.join("downloads/relay.bin")).unwrap(),
        fixture.bytes(0, usize::try_from(fixture.len).unwrap(), 0)
    );
    second.finish().await;
    worker.request_stop();
    let mut joined = None;
    until(|| {
        joined = worker.try_join();
        joined.is_some()
    })
    .await;
    assert_eq!(joined, Some(Ok(())));
    let owner = EngineOwner::open(&domain.config()).unwrap();
    owner.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partial_native_input_and_nonreading_output_retire_exact_pumps_and_join() {
    let endpoint = Endpoint::generate().unwrap();
    let key = Arc::new(Capability::generate().unwrap());
    let server = Server::bind(endpoint, Arc::clone(&key)).unwrap();
    let (accepted, connected) = tokio::join!(server.accept(), connect(endpoint, &key));
    let (_, mut writer) = accepted.unwrap().split();
    let (input, mut partial) = std::io::pipe().unwrap();
    let (mut unread, output) = std::io::pipe().unwrap();
    partial.write_all(&[0x80]).unwrap(); // The input child must block on the remaining prefix.
    let mut task = tokio::spawn(relay::forward(
        connected.unwrap(),
        Path::new(APP),
        input.into(),
        output.into(),
    ));
    writer.write(&vec![9; MAX_MESSAGE_BYTES]).await.unwrap();
    let mut prefix = [0; 4];
    unread.read_exact(&mut prefix).unwrap();
    assert_eq!(
        u32::from_le_bytes(prefix),
        u32::try_from(MAX_MESSAGE_BYTES).unwrap()
    );
    // Observed a real output attempt; deliberately do not consume its body.
    let result = tokio::time::timeout(Duration::from_secs(10), &mut task).await;
    if result.is_err() {
        task.abort();
        let _ = task.await;
    }
    assert!(
        matches!(result, Ok(Ok(Err(RelayError::Output)))),
        "stalled output must fail only after exact pump retirement/join"
    );
    let mut tail = Vec::new();
    unread
        .take(u64::try_from(MAX_MESSAGE_BYTES + 1).unwrap())
        .read_to_end(&mut tail)
        .unwrap();
    assert!(
        tail.len() < MAX_MESSAGE_BYTES,
        "nonreader must establish incomplete output and actual EOF"
    );
    drop(partial);
    drop(writer);
    assert!(!server.cancellation_failed());
}

#[test]
fn output_pump_retires_when_its_parent_pipe_dies_with_an_unread_consumer() {
    use std::{
        os::windows::process::CommandExt,
        process::{Child, Command},
    };
    struct Owned(Child);
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let (child_input, parent_output) = std::io::pipe().unwrap();
    let (mut unread, child_output) = std::io::pipe().unwrap();
    let mut child = Owned(
        Command::new(APP)
            .arg("--stdio-output")
            .stdin(child_input)
            .stdout(child_output)
            .stderr(Stdio::piped())
            .creation_flags(0x0800_0000)
            .spawn()
            .unwrap(),
    );
    // Both processes are directly retained by this fixture. Their pipe topology
    // models loss of the sole parent writer without relying on PID/tree lookup.
    let mut parent = Owned(
        Command::new(APP)
            .arg("--stdio-input")
            .stdin(Stdio::piped())
            .stdout(parent_output)
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
            .unwrap(),
    );
    let mut sender = parent.0.stdin.take().unwrap();
    let (ready, observed_ready) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        let result = (|| -> std::io::Result<_> {
            sender.write_all(&u32::try_from(MAX_MESSAGE_BYTES).unwrap().to_le_bytes())?;
            sender.write_all(&vec![7; MAX_MESSAGE_BYTES])?;
            let mut prefix = [0; 4];
            unread.read_exact(&mut prefix)?;
            Ok((sender, unread, prefix))
        })();
        let _ = ready.send(result);
    });
    let started = observed_ready.recv_timeout(Duration::from_secs(5));
    if !matches!(&started, Ok(Ok(_))) {
        let _ = parent.0.kill();
        let _ = parent.0.wait();
        let _ = child.0.kill();
        let _ = child.0.wait();
    }
    writer.join().unwrap();
    let (sender, unread, prefix) = started
        .expect("owned output startup observation")
        .expect("owned output prefix");
    assert_eq!(
        u32::from_le_bytes(prefix),
        u32::try_from(MAX_MESSAGE_BYTES).unwrap()
    );
    parent.0.kill().unwrap();
    parent.0.wait().unwrap();
    drop(sender);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let observed = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break Some(status);
        }
        if std::time::Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    if observed.is_none() {
        child.0.kill().unwrap();
    }
    child.0.wait().unwrap(); // All failure assertions follow exact owned cleanup.
    let mut tail = Vec::new();
    unread
        .take(u64::try_from(MAX_MESSAGE_BYTES + 1).unwrap())
        .read_to_end(&mut tail)
        .unwrap();
    assert!(
        observed.is_some_and(|status| !status.success()),
        "output pump must retire itself on parent-pipe loss, not survive behind an unread consumer"
    );
    assert!(tail.len() < MAX_MESSAGE_BYTES);
}

#[test]
fn input_pump_retires_on_private_parent_pipe_loss_with_native_input_held() {
    use std::{
        os::windows::process::CommandExt,
        process::{Child, Command},
    };
    struct Owned(Child);
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let (parent_reader, liveness) = std::io::pipe().unwrap();
    let mut child = Owned(
        Command::new(APP)
            .arg("--stdio-input")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(liveness)
            .creation_flags(0x0800_0000)
            .spawn()
            .unwrap(),
    );
    let mut native_input = child.0.stdin.take().unwrap();
    native_input.write_all(&[4, 0]).unwrap();
    drop(parent_reader); // Sole private parent reader; native input remains held.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let observed = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break Some(status);
        }
        if std::time::Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    if observed.is_none() {
        child.0.kill().unwrap();
    }
    child.0.wait().unwrap();
    assert!(
        observed.is_some_and(|status| !status.success()),
        "input pump must retire on private parent-pipe loss despite held native input"
    );
    assert!(native_input.write_all(&[0, 0]).is_err());
}
