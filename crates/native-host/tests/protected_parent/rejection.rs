use super::{
    Arc, CancellationStatus, Capability, Domain, Endpoint, EngineOwner, HostError, Reader, Server,
    TaskFailureKind, TaskId, TaskState, Value, WAIT, challenge, fixture, handshake, inactive, json,
    oneshot, pairs, private, read, resolution, write,
};
use download_manager_local_ipc::Error;

pub(super) async fn closed(reader: &mut Reader) -> Result<(), &'static str> {
    for _ in 0..128 {
        match tokio::time::timeout(WAIT, reader.read()).await {
            Ok(Err(Error::Transport)) => return Ok(()),
            Ok(Ok(raw)) => {
                let value: Value = serde_json::from_slice(&raw).map_err(|_| "retirement JSON")?;
                if value["kind"] != "event" {
                    return Err("malformed request was not refused");
                }
            }
            _ => return Err("actual transport closure required"),
        }
    }
    Err("retirement bound")
}
fn corrupt(case: u8, offer: &Value, request: &Value) -> Vec<u8> {
    let mut message = resolution(offer, request);
    match case {
        0 => message["context_id"] = json!(TaskId::new().to_string()),
        1 => message["body"]["challenge_id"] = json!(TaskId::new().to_string()),
        2 => message["body"]["task_id"] = json!(TaskId::new().to_string()),
        3 => message["body"]["generation"] = json!("01"),
        4 => message["body"]["generation"] = json!(1),
        5 => message["body"]["decision"] = json!(true),
        6 => message["body"]["decision"] = json!("safe"),
        7 => message["protection_bridge"] = json!(true),
        8 => message["body"]["sha256"] = json!("caller hash refused"),
        9 => message["protocol_version"] = json!(2),
        10 => message["body"]["kind"] = json!("scan"),
        11..=13 => {}
        _ => panic!("closed case set"),
    }
    let text = serde_json::to_string(&message).unwrap();
    match case {
        11 => text
            .replace(
                "\"decision\":\"permit_publication\"",
                "\"decision\":\"permit_publication\",\"decision\":\"permit_publication\"",
            )
            .into_bytes(),
        12 => text
            .replace(
                "\"protection_bridge\":1",
                "\"protection_bridge\":1,\"protection_bridge\":1",
            )
            .into_bytes(),
        13 => (" ".repeat(32768) + &text).into_bytes(),
        _ => text.into_bytes(),
    }
}
#[tokio::test]
async fn malformed_or_mismatched_private_resolutions_revoke_without_publication() {
    for case in 0..14 {
        let domain = Domain::new();
        let http = fixture();
        let url = http.url("/fixture");
        let (mut owner, mut receiver) =
            EngineOwner::open_with_protection(&domain.config()).unwrap();
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
                challenge(&mut reader, &mut writer, &offer, id, &url, "refused.txt").await?;
            writer
                .write(&corrupt(case, &offer, &request))
                .await
                .map_err(|_| "invalid write")?;
            closed(&mut reader).await
        };
        let (ended, result) = tokio::join!(
            owner.serve_protected_parent(native, &mut stopped, &mut receiver),
            peer
        );
        let retired = inactive(&owner, id).await;
        let joined = owner.shutdown().await;
        drop(receiver);
        drop(owner);
        let failed = server.cancellation_failed();
        drop(server);
        drop(http);
        assert!(joined.is_ok() && !failed, "case {case}");
        assert!(matches!(ended, Err(HostError::LocalSession)), "case {case}");
        assert_eq!(
            cancellation.status(),
            CancellationStatus::Requested,
            "case {case}"
        );
        result.unwrap();
        let retired = retired.unwrap();
        assert_eq!(retired.state(), TaskState::Failed, "case {case}");
        assert_eq!(
            retired.failure().unwrap().kind(),
            TaskFailureKind::ProtectionUnavailable,
            "case {case}"
        );
        assert!(
            !domain.0.join("downloads/refused.txt").exists(),
            "case {case}"
        );
    }
}
#[tokio::test]
async fn ordinary_peer_and_stop_before_admission_never_dispatch() {
    for ordinary in [true, false] {
        let domain = Domain::new();
        let (mut owner, mut receiver) =
            EngineOwner::open_with_protection(&domain.config()).unwrap();
        let endpoint = Endpoint::generate().unwrap();
        let key = Arc::new(Capability::generate().unwrap());
        let server = Server::bind(endpoint, key.clone()).unwrap();
        let (native, client) = pairs(&server, endpoint, &key, ordinary).await;
        let cancellation = native.cancellation();
        let (stop, mut stopped) = oneshot::channel();
        let peer = async {
            let (mut reader, _writer) = client.split();
            if !ordinary {
                let offer = read(&mut reader).await?;
                if offer["parent_transport"] != 2 {
                    return Err("offer");
                }
                stop.send(()).map_err(|()| "original stop")?;
            } // Ordinary peer is refused before even the offer.
            closed(&mut reader).await
        };
        let (ended, result) = tokio::join!(
            owner.serve_protected_parent(native, &mut stopped, &mut receiver),
            peer
        );
        let empty = owner.engine().snapshots().is_empty();
        let replacement = receiver.open_context().is_ok();
        let joined = owner.shutdown().await;
        drop(receiver);
        drop(owner);
        let failed = server.cancellation_failed();
        drop(server);
        assert!(joined.is_ok() && !failed && empty && replacement);
        result.unwrap();
        assert_eq!(cancellation.status(), CancellationStatus::Requested);
        if ordinary {
            assert!(matches!(ended, Err(HostError::LocalSession)));
        } else {
            assert!(matches!(
                ended,
                Ok(download_manager_native_host::LocalSessionEnd::StopRequested)
            ));
        }
    }
}
#[tokio::test]
async fn blocked_and_unavailable_decisions_cannot_publish() {
    for (decision, expected) in [
        ("blocked", TaskFailureKind::ProtectionBlocked),
        ("unavailable", TaskFailureKind::ProtectionUnavailable),
    ] {
        let domain = Domain::new();
        let http = fixture();
        let url = http.url("/fixture");
        let (mut owner, mut receiver) =
            EngineOwner::open_with_protection(&domain.config()).unwrap();
        let endpoint = Endpoint::generate().unwrap();
        let key = Arc::new(Capability::generate().unwrap());
        let server = Server::bind(endpoint, key.clone()).unwrap();
        let (native, client) = pairs(&server, endpoint, &key, false).await;
        let (_stop, mut stopped) = oneshot::channel();
        let id = TaskId::new();
        let peer = async {
            let (mut reader, mut writer) = client.split();
            let offer = handshake(&mut reader, &mut writer).await?;
            let request =
                challenge(&mut reader, &mut writer, &offer, id, &url, "denied.txt").await?;
            let mut response = resolution(&offer, &request);
            response["body"]["decision"] = json!(decision);
            write(&mut writer, &response).await?;
            if private(&mut reader).await?["body"]["accepted"] != true {
                return Err("consumed refusal");
            }
            for _ in 0..128 {
                let event = read(&mut reader).await?;
                if event["event"] == "failed" {
                    return Ok(());
                }
            }
            Err("failed event bound")
        };
        let (ended, result) = tokio::join!(
            owner.serve_protected_parent(native, &mut stopped, &mut receiver),
            peer
        );
        let state = owner.engine().snapshot(id);
        let joined = owner.shutdown().await;
        drop(receiver);
        drop(owner);
        let failed = server.cancellation_failed();
        drop(server);
        drop(http);
        assert!(joined.is_ok() && !failed);
        assert!(matches!(ended, Err(HostError::LocalSession)));
        result.unwrap();
        assert_eq!(state.unwrap().failure().unwrap().kind(), expected);
        assert!(!domain.0.join("downloads/denied.txt").exists());
    }
}

#[tokio::test]
async fn private_admission_requires_exact_receiver_context_and_closed_schema() {
    for case in 0..7 {
        let domain = Domain::new();
        let (mut owner, mut receiver) =
            EngineOwner::open_with_protection(&domain.config()).unwrap();
        let endpoint = Endpoint::generate().unwrap();
        let key = Arc::new(Capability::generate().unwrap());
        let server = Server::bind(endpoint, key.clone()).unwrap();
        let (native, client) = pairs(&server, endpoint, &key, false).await;
        let (_stop, mut stopped) = oneshot::channel();
        let peer = async {
            let (mut reader, mut writer) = client.split();
            let offer = read(&mut reader).await?;
            let mut answer = json!({"parent_transport":2,"admission_id":offer["admission_id"],"receiver_id":offer["receiver_id"],"context_id":offer["context_id"],"kind":"accept"});
            match case {
                0 => answer["admission_id"] = json!(TaskId::new().to_string()),
                1 => answer["receiver_id"] = json!(TaskId::new().to_string()),
                2 => answer["context_id"] = json!(TaskId::new().to_string()),
                3 => answer["parent_transport"] = json!(true),
                4 => answer["capture_ready"] = json!(true),
                _ => {}
            }
            let mut text = serde_json::to_string(&answer).map_err(|_| "answer JSON")?;
            if case == 5 {
                text = text.replace(
                    "\"parent_transport\":2",
                    "\"parent_transport\":2,\"parent_transport\":2",
                );
            }
            if case == 6 {
                text = " ".repeat(513) + &text;
            }
            writer
                .write(text.as_bytes())
                .await
                .map_err(|_| "answer write")?;
            closed(&mut reader).await
        };
        let (ended, result) = tokio::join!(
            owner.serve_protected_parent(native, &mut stopped, &mut receiver),
            peer
        );
        let empty = owner.engine().snapshots().is_empty();
        let available = receiver.open_context().is_ok();
        let joined = owner.shutdown().await;
        drop(receiver);
        drop(owner);
        let failed = server.cancellation_failed();
        drop(server);
        assert!(
            joined.is_ok() && !failed && empty && available,
            "case {case}"
        );
        result.unwrap();
        assert!(matches!(ended, Err(HostError::LocalSession)), "case {case}");
    }
}

#[tokio::test]
async fn protected_factory_obtains_original_store_lock_before_settings_read() {
    use download_manager_engine::{persistence::PersistenceError, task::TaskEngineError};
    let domain = Domain::new();
    let owner = EngineOwner::open(&domain.config()).unwrap();
    std::fs::write(
        domain.0.join("state/settings.json"),
        b"not a settings document",
    )
    .unwrap();
    let refused = matches!(
        EngineOwner::open_with_protection(&domain.config()),
        Err(HostError::Engine(TaskEngineError::Persistence(
            PersistenceError::StoreLocked
        )))
    );
    let joined = owner.shutdown().await;
    drop(owner);
    assert!(joined.is_ok());
    assert!(refused, "state lock must precede settings inspection");
}
