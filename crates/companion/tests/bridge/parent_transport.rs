//! Real owned pipes; no browser, registration or installed-state operations.
use super::{Domain, Duration, EngineOwner, HostError, Server, json};
use download_manager_local_ipc::{
    CancellationStatus, Capability, Channel, Endpoint, Error, LocalPipe, connect,
    connect_browser_parent,
};
use download_manager_native_host::LocalSessionEnd;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::oneshot;

enum Case {
    Nominal,
    Manual(String),
    Invalid(Value),
    Duplicate,
    Old(String),
    Stop,
    Ordinary,
}

struct Observed {
    id: String,
    refused: bool,
    task_id: Option<String>,
}

type Reader = download_manager_local_ipc::FrameReader<tokio::io::ReadHalf<LocalPipe>>;
type Writer = download_manager_local_ipc::FrameWriter<tokio::io::WriteHalf<LocalPipe>>;

async fn read(reader: &mut Reader) -> Result<Value, &'static str> {
    let body = tokio::time::timeout(Duration::from_secs(5), reader.read())
        .await
        .map_err(|_| "frame deadline")?
        .map_err(|_| "frame transport")?;
    serde_json::from_slice(&body).map_err(|_| "frame JSON")
}

async fn write(writer: &mut Writer, value: &Value) -> Result<(), &'static str> {
    writer
        .write(&serde_json::to_vec(value).map_err(|_| "encode")?)
        .await
        .map_err(|_| "write")
}

async fn closed(reader: &mut Reader) -> Result<(), &'static str> {
    if matches!(
        tokio::time::timeout(Duration::from_secs(5), reader.read()).await,
        Ok(Err(Error::Transport))
    ) {
        Ok(())
    } else {
        Err("actual peer closure required")
    }
}

async fn accepted(reader: &mut Reader, writer: &mut Writer, id: &str) -> Result<(), &'static str> {
    write(
        writer,
        &json!({"parent_transport":1,"admission_id":id,"kind":"accept"}),
    )
    .await?;
    if read(reader).await?
        != json!({"parent_transport":1,"admission_id":id,"kind":"ready","capture_ready":false})
    {
        return Err("matching ready receipt required");
    }
    write(writer, &json!({"protocol_version":2,"kind":"command","command":"hello","correlation_id":"hello","payload":{"supported_versions":[2],"client_name":"owned-parent-test","client_version":"0.1.0"}})).await?;
    let hello = read(reader).await?;
    let caps = hello["result"]["capabilities"]
        .as_array()
        .ok_or("capabilities")?;
    if hello["ok"] != true
        || hello["command"] != "hello"
        || caps.contains(&json!("prepared_handoff"))
        || !caps.contains(&json!("task_handoff_phase"))
    {
        return Err("ordinary-only Hello required");
    }
    let snapshot = read(reader).await?;
    if snapshot["event"] != "snapshot"
        || snapshot["data"]["tasks"] != json!([])
        || snapshot["data"]["complete"] != true
    {
        return Err("empty snapshot required");
    }
    Ok(())
}

async fn peer(
    channel: Channel<LocalPipe>,
    case: Case,
    stop: &mut Option<oneshot::Sender<()>>,
) -> Result<Observed, &'static str> {
    let (mut reader, mut writer) = channel.split();
    if matches!(case, Case::Ordinary) {
        closed(&mut reader).await?;
        return Ok(Observed {
            id: String::new(),
            refused: true,
            task_id: None,
        });
    }
    let offer = read(&mut reader).await?;
    let id = offer["admission_id"]
        .as_str()
        .ok_or("missing admission")?
        .to_owned();
    download_manager_engine::persistence::TaskId::parse(&id).map_err(|_| "admission UUID")?;
    if offer != json!({"parent_transport":1,"admission_id":id,"kind":"offer","capture_ready":false})
    {
        return Err("closed offer required");
    }
    match case {
        Case::Nominal => {
            accepted(&mut reader, &mut writer, &id).await?;
            write(&mut writer, &json!({"protocol_version":2,"kind":"command","command":"prepare_handoff","correlation_id":"prepare","payload":{"task_id":"11111111-1111-4111-8111-111111111111","download":{"url":"http://127.0.0.1:1/never-requested","suggested_filename":"not-created.bin"}}})).await?;
            let rejected = read(&mut reader).await?;
            if rejected["ok"] != false || rejected["error"]["code"] != "PROTOCOL_UNKNOWN_COMMAND" {
                return Err("unprotected handoff must refuse");
            }
            Ok(Observed {
                id,
                refused: false,
                task_id: None,
            })
        }
        Case::Manual(url) => {
            accepted(&mut reader, &mut writer, &id).await?;
            write(&mut writer, &json!({"protocol_version":2,"kind":"command","command":"add","correlation_id":"manual","payload":{"url":url,"suggested_filename":"parent-manual.bin"}})).await?;
            for _ in 0..64 {
                let message = read(&mut reader).await?;
                if message["correlation_id"] == "manual" && message["ok"] == true {
                    let task = message["result"]["task_id"]
                        .as_str()
                        .ok_or("manual task ID")?
                        .to_owned();
                    download_manager_engine::persistence::TaskId::parse(&task)
                        .map_err(|_| "manual task UUID")?;
                    return Ok(Observed {
                        id,
                        refused: false,
                        task_id: Some(task),
                    });
                }
                if message["kind"] != "event" {
                    return Err("manual response refused");
                }
            }
            Err("manual response bound")
        }
        Case::Stop => {
            stop.take()
                .ok_or("stop owner")?
                .send(())
                .map_err(|()| "stop receiver")?;
            closed(&mut reader).await?;
            Ok(Observed {
                id,
                refused: true,
                task_id: None,
            })
        }
        other => {
            let bytes = match other {
                Case::Invalid(mut value) => {
                    if value.get("admission_id").is_some() { value["admission_id"] = json!(id); }
                    serde_json::to_vec(&value).map_err(|_| "invalid encode")?
                }
                Case::Duplicate => format!(r#"{{"parent_transport":1,"admission_id":"{id}","kind":"accept","kind":"accept"}}"#).into_bytes(),
                Case::Old(old) => {
                    if old == id { return Err("admission reused across connections"); }
                    serde_json::to_vec(&json!({"parent_transport":1,"admission_id":old,"kind":"accept"})).map_err(|_| "old encode")?
                }
                _ => return Err("test case"),
            };
            // Write success is deliberately not receipt evidence.
            let _sent = writer.write(&bytes).await;
            closed(&mut reader).await?;
            Ok(Observed {
                id,
                refused: true,
                task_id: None,
            })
        }
    }
}

async fn run(case: Case) -> Observed {
    let ordinary = matches!(case, Case::Ordinary);
    let stopped_case = matches!(case, Case::Stop);
    let domain = Domain::new();
    let mut owner = EngineOwner::open(&domain.config()).unwrap();
    let endpoint = Endpoint::generate().unwrap();
    let key = Arc::new(Capability::generate().unwrap());
    let server = Server::bind(endpoint, Arc::clone(&key)).unwrap();
    let client = async {
        if ordinary {
            connect(endpoint, &key).await
        } else {
            connect_browser_parent(endpoint, &key).await
        }
    };
    let (s, c) = tokio::join!(server.accept_with_browser_parent(), client);
    let channel = s.unwrap();
    let watch = channel.cancellation();
    let (stop, mut stopped) = oneshot::channel();
    let mut stop = Some(stop);
    // Peer returns no pipe handles, including on error. Both futures settle
    // before assertions; no join result can retain a channel and block EOF.
    let (outcome, observation) = tokio::join!(
        owner.serve_parent_transport(channel, &mut stopped),
        peer(c.unwrap(), case, &mut stop)
    );
    super::locked(&domain);
    let tasks = owner.engine().snapshots().len();
    let shutdown = owner.shutdown().await;
    drop(owner);
    let cancellation_failed = server.cancellation_failed();
    drop(server);
    let reopened = EngineOwner::open(&domain.config()).unwrap();
    let recovered_tasks = reopened.engine().snapshots().len();
    let reopened_shutdown = reopened.shutdown().await;
    drop(reopened);
    drop(Server::bind(endpoint, key).unwrap());
    assert!(shutdown.is_ok() && reopened_shutdown.is_ok());
    assert!(!cancellation_failed);
    assert_eq!(watch.status(), CancellationStatus::Requested);
    assert_eq!((tasks, recovered_tasks), (0, 0));
    assert_eq!(
        std::fs::read_dir(domain.0.join("downloads"))
            .unwrap()
            .count(),
        0
    );
    if stopped_case {
        assert!(matches!(outcome, Ok(LocalSessionEnd::StopRequested)));
    } else {
        // The stop sender remains retained: this is actual EOF/refusal.
        assert!(matches!(outcome, Err(HostError::LocalSession)));
    }
    observation.expect("owned parent observation failed after joined retirement")
}

#[tokio::test]
async fn parent_admission_correlates_fresh_native_ids_without_capture_authority() {
    let first = run(Case::Nominal).await;
    assert!(!first.refused);
    let second = run(Case::Nominal).await;
    assert!(!second.refused);
    assert_ne!(first.id, second.id);
    assert!(run(Case::Old(first.id)).await.refused);
}

#[tokio::test]
async fn parent_admission_refuses_ordinary_class_before_offer() {
    assert!(run(Case::Ordinary).await.refused);
}

#[tokio::test]
async fn parent_admission_refuses_unnegotiated_or_malformed_application_input() {
    for value in [
        json!({"protocol_version":2,"kind":"command","command":"hello","correlation_id":"early","payload":{}}),
        json!({"parent_transport":true,"admission_id":"replace","kind":"accept"}),
        json!({"parent_transport":1.0,"admission_id":"replace","kind":"accept"}),
        json!({"parent_transport":1,"admission_id":"replace","kind":"ready"}),
        json!({"parent_transport":1,"admission_id":"replace","kind":"accept","capture_ready":true}),
    ] {
        assert!(run(Case::Invalid(value)).await.refused);
    }
    assert!(run(Case::Duplicate).await.refused);
}

#[tokio::test]
async fn parent_admission_idle_stop_retires_reader_and_preserves_state_ownership() {
    assert!(run(Case::Stop).await.refused);
}

#[tokio::test]
async fn parent_admission_manual_transfer_finishes_after_its_controller_disconnects() {
    use super::{ByteRange, Fixture, RequestSelector, ServerConfig, TestServer};
    use download_manager_engine::persistence::TaskState;
    let domain = Domain::new();
    let fixture = Fixture {
        len: 64 * 1024,
        seed: 96,
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
    let mut owner = EngineOwner::open(&domain.config()).unwrap();
    let endpoint = Endpoint::generate().unwrap();
    let key = Arc::new(Capability::generate().unwrap());
    let server = Server::bind(endpoint, Arc::clone(&key)).unwrap();
    let (s, c) = tokio::join!(
        server.accept_with_browser_parent(),
        connect_browser_parent(endpoint, &key)
    );
    let channel = s.unwrap();
    let watch = channel.cancellation();
    let (stop, mut stopped) = oneshot::channel();
    let mut stop = Some(stop); // Do not turn client EOF into a sent stop.
    let (outcome, observed) = tokio::join!(
        owner.serve_parent_transport(channel, &mut stopped),
        peer(c.unwrap(), Case::Manual(http.url("/fixture")), &mut stop)
    );
    let before = owner.engine().snapshots();
    drop(gate); // Native controller already retired; engine must keep this task.
    let completed = if observed.is_ok() {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let snapshots = owner.engine().snapshots();
                if snapshots.len() == 1 && snapshots[0].state() == TaskState::Completed {
                    return snapshots;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .ok()
    } else {
        None
    };
    let shutdown = owner.shutdown().await;
    drop(owner);
    let failed = server.cancellation_failed();
    drop(server);
    drop(http); // Join the owned fixture before assertions, also on failure.
    assert!(shutdown.is_ok());
    assert!(!failed);
    assert!(matches!(outcome, Err(HostError::LocalSession)));
    assert_eq!(watch.status(), CancellationStatus::Requested);
    let observed = observed.expect("manual response after joined session");
    assert_eq!(before.len(), 1);
    assert_ne!(before[0].state(), TaskState::Completed);
    let completed = completed.expect("manual completion after controller retirement");
    assert_eq!(Some(completed[0].task_id().to_string()), observed.task_id);
    assert_eq!(
        std::fs::read(domain.0.join("downloads/parent-manual.bin")).unwrap(),
        fixture.bytes(0, 64 * 1024, 0)
    );
    assert_eq!(
        std::fs::read_dir(domain.0.join("downloads"))
            .unwrap()
            .count(),
        1
    );
    let reopened = EngineOwner::open(&domain.config()).unwrap();
    let recovered = reopened.engine().snapshots();
    let reopened_shutdown = reopened.shutdown().await;
    drop(reopened);
    drop(Server::bind(endpoint, key).unwrap());
    assert!(reopened_shutdown.is_ok());
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].state(), TaskState::Completed);
    assert_eq!(Some(recovered[0].task_id().to_string()), observed.task_id);
}

#[tokio::test]
async fn parent_admission_reconnect_on_same_engine_rejects_previous_acceptance() {
    type Observation = (
        Result<LocalSessionEnd, HostError>,
        Result<Observed, &'static str>,
        CancellationStatus,
    );
    let domain = Domain::new();
    let mut owner = EngineOwner::open(&domain.config()).unwrap();
    let endpoint = Endpoint::generate().unwrap();
    let key = Arc::new(Capability::generate().unwrap());
    let server = Server::bind(endpoint, Arc::clone(&key)).unwrap();
    let mut observations: Vec<Observation> = Vec::new();
    for index in 0..3 {
        let (s, c) = tokio::join!(
            server.accept_with_browser_parent(),
            connect_browser_parent(endpoint, &key)
        );
        let channel = s.unwrap();
        let watch = channel.cancellation();
        let (stop, mut stopped) = oneshot::channel();
        let mut stop = Some(stop);
        let case = if index == 2 {
            let Some((_, Ok(first), _)) = observations.first() else {
                break;
            };
            Case::Old(first.id.clone())
        } else {
            Case::Nominal
        };
        let (result, observed) = tokio::join!(
            owner.serve_parent_transport(channel, &mut stopped),
            peer(c.unwrap(), case, &mut stop)
        );
        observations.push((result, observed, watch.status()));
    }
    let tasks = owner.engine().snapshots().len();
    let shutdown = owner.shutdown().await;
    drop(owner);
    let failed = server.cancellation_failed();
    drop(server);
    drop(Server::bind(endpoint, key).unwrap());
    assert!(shutdown.is_ok());
    assert!(!failed);
    assert_eq!(tasks, 0);
    assert_eq!(observations.len(), 3);
    let mut ids = std::collections::HashSet::new();
    for (index, (result, observed, cancellation)) in observations.into_iter().enumerate() {
        assert!(matches!(result, Err(HostError::LocalSession)));
        assert_eq!(cancellation, CancellationStatus::Requested);
        let observed = observed.expect("same-engine parent observation after joined retirement");
        assert_eq!(observed.refused, index == 2);
        assert!(ids.insert(observed.id));
    }
}
