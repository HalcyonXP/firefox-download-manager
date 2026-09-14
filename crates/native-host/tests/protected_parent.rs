//! Real owned loopback/Windows pipes; policy peer is modeled, never Firefox.
#![cfg(all(windows, feature = "local-bridge"))]
use download_manager_engine::{
    persistence::{TaskId, TaskState},
    task::TaskFailureKind,
};
use download_manager_local_ipc::{
    CancellationStatus, Capability, Channel, Endpoint, LocalPipe, Server, connect,
    connect_browser_parent,
};
use download_manager_native_host::{EngineOwner, HostConfig, HostError};
use download_manager_test_server::{Fixture, ServerConfig, TestServer};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::oneshot;
const FIXTURE: Fixture = Fixture {
    len: 64 * 1024,
    seed: 37,
};
const WAIT: Duration = Duration::from_secs(5);
type Reader = download_manager_local_ipc::FrameReader<tokio::io::ReadHalf<LocalPipe>>;
type Writer = download_manager_local_ipc::FrameWriter<tokio::io::WriteHalf<LocalPipe>>;
struct Domain(PathBuf);
impl Domain {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("dm-protected-parent-{}", TaskId::new()));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("downloads")).unwrap();
        Self(root)
    }
    fn config(&self) -> HostConfig {
        HostConfig::new(self.0.join("state"), Some(self.0.join("downloads")))
    }
}
impl Drop for Domain {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn fixture() -> TestServer {
    TestServer::start(ServerConfig {
        fixture: FIXTURE,
        rules: vec![],
    })
    .unwrap()
}
async fn read(reader: &mut Reader) -> Result<Value, &'static str> {
    let raw = tokio::time::timeout(WAIT, reader.read())
        .await
        .map_err(|_| "read deadline")?
        .map_err(|_| "read transport")?;
    serde_json::from_slice(&raw).map_err(|_| "read JSON")
}
async fn write(writer: &mut Writer, value: &Value) -> Result<(), &'static str> {
    writer
        .write(&serde_json::to_vec(value).map_err(|_| "encode")?)
        .await
        .map_err(|_| "write transport")
}
async fn private(reader: &mut Reader) -> Result<Value, &'static str> {
    for _ in 0..128 {
        let message = read(reader).await?;
        if message.get("protection_bridge").is_some() {
            return Ok(message);
        }
        if message["kind"] != "event" {
            return Err("unexpected public response");
        }
    }
    Err("bounded private response")
}
async fn pairs(
    server: &Server,
    endpoint: Endpoint,
    key: &Capability,
    ordinary: bool,
) -> (Channel<LocalPipe>, Channel<LocalPipe>) {
    let client = async {
        if ordinary {
            connect(endpoint, key).await
        } else {
            connect_browser_parent(endpoint, key).await
        }
    };
    let (accepted, connected) = tokio::join!(server.accept_with_browser_parent(), client);
    (accepted.unwrap(), connected.unwrap())
}
async fn handshake(reader: &mut Reader, writer: &mut Writer) -> Result<Value, &'static str> {
    let offer = read(reader).await?;
    for field in ["admission_id", "context_id", "receiver_id"] {
        let raw = offer[field].as_str().ok_or("native identity missing")?;
        if TaskId::parse(raw).map_err(|_| "UUID")?.to_string() != raw {
            return Err("noncanonical UUID");
        }
    }
    if offer
        != json!({"parent_transport":2,"admission_id":offer["admission_id"],"context_id":offer["context_id"],
        "receiver_id":offer["receiver_id"],"kind":"offer","capture_ready":false})
    {
        return Err("closed offer");
    }
    write(writer,&json!({"parent_transport":2,"admission_id":offer["admission_id"],"context_id":offer["context_id"],
        "receiver_id":offer["receiver_id"],"kind":"accept"})).await?;
    let mut ready = offer.clone();
    ready["kind"] = json!("ready");
    if read(reader).await? != ready {
        return Err("exact ready");
    }
    write(writer,&json!({"protocol_version":2,"kind":"command","command":"hello","correlation_id":"hello",
        "payload":{"supported_versions":[2],"client_name":"owned-private-peer","client_version":"0.1.0"}})).await?;
    let hello = read(reader).await?;
    if hello["ok"] != true
        || hello["result"]["capabilities"]
            .as_array()
            .ok_or("caps")?
            .contains(&json!("prepared_handoff"))
    {
        return Err("no unprotected capability");
    }
    for _ in 0..32 {
        let event = read(reader).await?;
        if event["event"] == "snapshot" && event["data"]["complete"] == true {
            return Ok(offer);
        }
    }
    Err("snapshot bound")
}
fn frame(offer: &Value, body: Value) -> Value {
    let mut message = json!({"protection_bridge":1,"context_id":offer["context_id"],"body":null});
    message["body"] = body;
    message
}
async fn challenge(
    reader: &mut Reader,
    writer: &mut Writer,
    offer: &Value,
    id: TaskId,
    url: &str,
    name: &str,
) -> Result<Value, &'static str> {
    write(
        writer,
        &frame(
            offer,
            json!({"kind":"prepare","task_id":id.to_string(),"source_url":url,"file_name":name}),
        ),
    )
    .await?;
    let prepare = private(reader).await?;
    if prepare["body"]["ok"] != true || prepare["body"]["phase"] != "prepared" {
        return Err("scoped prepare");
    }
    write(
        writer,
        &frame(offer, json!({"kind":"commit","task_id":id.to_string()})),
    )
    .await?;
    let commit = private(reader).await?;
    if commit["body"]["ok"] != true || commit["body"]["phase"] != "committed" {
        return Err("scoped commit");
    }
    let request = private(reader).await?;
    let body = &request["body"];
    if request["context_id"] != offer["context_id"]
        || body["receiver_id"] != offer["receiver_id"]
        || body["kind"] != "challenge"
        || body["task_id"] != id.to_string()
        || body["source_url"] != url
        || body["file_name"] != name
        || body["length"] != "65536"
        || body["sha256"].as_str().map(str::len) != Some(64)
        || body["generation"].as_str().is_none()
    {
        return Err("native challenge binding");
    }
    Ok(request)
}
fn resolution(offer: &Value, request: &Value) -> Value {
    frame(
        offer,
        json!({"kind":"resolution","challenge_id":request["body"]["challenge_id"],
        "task_id":request["body"]["task_id"],"generation":request["body"]["generation"],"decision":"permit_publication"}),
    )
}
async fn inactive(
    owner: &EngineOwner,
    id: TaskId,
) -> Result<download_manager_engine::task::TaskSnapshot, &'static str> {
    tokio::time::timeout(WAIT, async {
        loop {
            let task = owner.engine().snapshot(id).map_err(|_| "task")?;
            if !matches!(
                task.state(),
                TaskState::Probing
                    | TaskState::Queued
                    | TaskState::Downloading
                    | TaskState::Validating
                    | TaskState::Promoting
            ) {
                return Ok(task);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .map_err(|_| "task deadline")?
}

#[tokio::test]
async fn private_native_challenge_consumes_one_reply_and_preserves_manual_engine() {
    let domain = Domain::new();
    let http = fixture();
    let url = http.url("/fixture");
    let (mut owner, mut receiver) = EngineOwner::open_with_protection(&domain.config()).unwrap();
    let endpoint = Endpoint::generate().unwrap();
    let key = Arc::new(Capability::generate().unwrap());
    let server = Server::bind(endpoint, key.clone()).unwrap();
    let (native, client) = pairs(&server, endpoint, &key, false).await;
    let cancellation = native.cancellation();
    let (_stop, mut stopped) = oneshot::channel();
    let id = TaskId::new();
    let peer = async {
        let (mut reader, mut writer) = client.split();
        let offer = handshake(&mut reader, &mut writer).await?;
        let request =
            challenge(&mut reader, &mut writer, &offer, id, &url, "protected.txt").await?;
        if domain.0.join("downloads/protected.txt").exists() {
            return Err("unapproved publication");
        }
        write(&mut writer, &resolution(&offer, &request)).await?;
        let answer = private(&mut reader).await?;
        if answer["body"]["accepted"] != true {
            return Err("consumed reply");
        }
        for _ in 0..128 {
            let event = read(&mut reader).await?;
            if event["event"] == "completed" {
                write(&mut writer, &resolution(&offer, &request)).await?;
                rejection::closed(&mut reader).await?;
                return Ok(request);
            }
            if event["kind"] != "event" {
                return Err("completion event");
            }
        }
        Err("completion bound")
    };
    let (ended, observed) = tokio::join!(
        owner.serve_protected_parent(native, &mut stopped, &mut receiver),
        peer
    );
    let protected_count = owner.engine().snapshots().len();
    let manual = owner
        .engine()
        .create_task_default(&url, &domain.0.join("downloads"), "manual.txt")
        .unwrap();
    let started = owner.engine().start(manual.task_id());
    let completed = inactive(&owner, manual.task_id()).await;
    let joined = owner.shutdown().await;
    drop(receiver);
    drop(owner);
    let failed = server.cancellation_failed();
    drop(server);
    drop(http);
    assert!(joined.is_ok() && started.is_ok() && !failed);
    assert!(matches!(ended, Err(HostError::LocalSession)));
    assert_eq!(cancellation.status(), CancellationStatus::Requested);
    assert_eq!(protected_count, 1);
    let request = observed.unwrap();
    assert_eq!(completed.unwrap().state(), TaskState::Completed);
    let bytes = FIXTURE.bytes(0, 64 * 1024, 0);
    assert_eq!(
        std::fs::read(domain.0.join("downloads/protected.txt")).unwrap(),
        bytes
    );
    assert_eq!(
        std::fs::read(domain.0.join("downloads/manual.txt")).unwrap(),
        bytes
    );
    // SHA-256 independently computed with Python hashlib over the documented
    // fixture formula (seed37, generation0,65536 bytes), not the engine hash.
    assert_eq!(
        request["body"]["sha256"],
        "7d06a7d1b14b17a6656fd8f5e1a47257f7b62a11a1d92265631abbf1cd89b05a"
    );
}

#[tokio::test]
async fn disconnect_revokes_taken_challenge_and_reconnect_cannot_adopt_old_tasks() {
    let domain = Domain::new();
    let http = fixture();
    let url = http.url("/fixture");
    let (mut owner, mut receiver) = EngineOwner::open_with_protection(&domain.config()).unwrap();
    let endpoint = Endpoint::generate().unwrap();
    let key = Arc::new(Capability::generate().unwrap());
    let server = Server::bind(endpoint, key.clone()).unwrap();
    let old = TaskId::new();
    let unused = TaskId::new();
    let fresh = TaskId::new();
    let (_stop, mut stopped) = oneshot::channel();
    let (native, client) = pairs(&server, endpoint, &key, false).await;
    let peer = async {
        let (mut reader, mut writer) = client.split();
        let offer = handshake(&mut reader, &mut writer).await?;
        write(&mut writer, &frame(&offer, json!({"kind":"prepare","task_id":unused.to_string(),"source_url":url,"file_name":"unused.txt"}))).await?;
        if private(&mut reader).await?["body"]["phase"] != "prepared" {
            return Err("unused preparation");
        }

        let request = challenge(&mut reader, &mut writer, &offer, old, &url, "old.txt").await?;
        Ok::<_, &'static str>((offer, request))
    };
    let (first, observation) = tokio::join!(
        owner.serve_protected_parent(native, &mut stopped, &mut receiver),
        peer
    );
    let retired = inactive(&owner, old).await;
    let (native, client) = pairs(&server, endpoint, &key, false).await;
    let peer = async {
        let (mut reader, mut writer) = client.split();
        let offer = handshake(&mut reader, &mut writer).await?;
        if let Ok((prior, _)) = &observation {
            if offer["context_id"] == prior["context_id"]
                || offer["receiver_id"] != prior["receiver_id"]
            {
                return Err("fresh context same receiver");
            }
        } else {
            return Err("first observation");
        }
        for (previous, name) in [(old, "old.txt"), (unused, "unused.txt")] {
            for body in [
                json!({"kind":"prepare","task_id":previous.to_string(),"source_url":url,"file_name":name}),
                json!({"kind":"commit","task_id":previous.to_string()}),
                json!({"kind":"abort","task_id":previous.to_string()}),
            ] {
                write(&mut writer, &frame(&offer, body)).await?;
                if private(&mut reader).await?["body"]["ok"] != false {
                    return Err("old task adopted");
                }
            }
        }
        let request = challenge(&mut reader, &mut writer, &offer, fresh, &url, "fresh.txt").await?;
        write(&mut writer, &resolution(&offer, &request)).await?;
        if private(&mut reader).await?["body"]["accepted"] != true {
            return Err("fresh response");
        }
        for _ in 0..128 {
            let message = read(&mut reader).await?;
            if message["event"] == "completed" {
                return Ok(());
            }
        }
        Err("fresh completion")
    };
    let (second, result) = tokio::join!(
        owner.serve_protected_parent(native, &mut stopped, &mut receiver),
        peer
    );
    let joined = owner.shutdown().await;
    drop(receiver);
    drop(owner);
    let failed = server.cancellation_failed();
    drop(server);
    drop(http);
    assert!(joined.is_ok() && !failed);
    assert!(
        matches!(first, Err(HostError::LocalSession))
            && matches!(second, Err(HostError::LocalSession))
    );
    let retired = retired.unwrap();
    assert_eq!(retired.state(), TaskState::Failed);
    assert_eq!(
        retired.failure().unwrap().kind(),
        TaskFailureKind::ProtectionUnavailable
    );
    result.unwrap();
    assert!(!domain.0.join("downloads/old.txt").exists());
    assert_eq!(
        std::fs::read(domain.0.join("downloads/fresh.txt")).unwrap(),
        FIXTURE.bytes(0, 64 * 1024, 0)
    );
}

#[path = "protected_parent/rejection.rs"]
mod rejection;
