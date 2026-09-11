use super::{
    AuthorizedPartial, CAPACITY, ProtectionDecision, ProtectionGate, Subject, TaskId,
    TransferCancellation, unavailable,
};
use crate::storage::{FileRange, NamedValidatedPartial, PartialFile};
use std::{fs, path::PathBuf, time::Duration};

struct Domain(PathBuf);
impl Domain {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("dm-protection-owner-{}", TaskId::new()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn named(&self, name: &str) -> NamedValidatedPartial {
        let partial = PartialFile::create(&self.0, name, 3).unwrap();
        let mut writer = partial.assign(FileRange::new(0, 3).unwrap()).unwrap();
        writer.write(b"abc").unwrap();
        writer.finish().unwrap();
        partial
            .validate_with_fingerprint(None, || false)
            .unwrap()
            .bind_final_name(0)
            .unwrap()
    }
}
impl Drop for Domain {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn subject() -> Subject {
    Subject {
        task_id: TaskId::new(),
        generation: 1,
        source_url: "http://127.0.0.1/fixture".to_owned(),
    }
}

#[tokio::test]
async fn protection_result_does_not_survive_close_between_receipt_and_publication() {
    let domain = Domain::new();
    let named = domain.named("closed.txt");
    let (gate, mut receiver) = ProtectionGate::channel();
    let signal = TransferCancellation::default();
    let (authorization, response) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(gate.authorize(subject(), named, &signal), async {
            let request = receiver.recv().await.unwrap();
            request.decide(ProtectionDecision::PermitPublication)
        })
    })
    .await
    .expect("owned in-process decision futures retired on timeout");
    receiver.close();
    let mut entered = false;
    let publication = authorization.and_then(|authorized| {
        authorized.publish(&signal, || {
            entered = true;
            Ok(())
        })
    });
    drop(receiver);
    drop(gate);
    assert!(response.is_ok());
    assert_eq!(publication.err(), Some(unavailable()));
    assert!(!entered);
    assert!(!domain.0.join("closed.txt").exists());
}

#[tokio::test]
async fn protection_result_does_not_survive_cancellation_between_receipt_and_publication() {
    let domain = Domain::new();
    let named = domain.named("cancelled.txt");
    let (gate, mut receiver) = ProtectionGate::channel();
    let signal = TransferCancellation::default();
    let (authorization, response) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(gate.authorize(subject(), named, &signal), async {
            let request = receiver.recv().await.unwrap();
            request.decide(ProtectionDecision::PermitPublication)
        })
    })
    .await
    .expect("owned in-process decision futures retired on timeout");
    signal.cancel();
    let mut entered = false;
    let publication = authorization.and_then(|authorized| {
        authorized.publish(&signal, || {
            entered = true;
            Ok(())
        })
    });
    drop(receiver);
    drop(gate);
    assert!(response.is_ok());
    assert_eq!(publication.err(), Some(super::cancelled()));
    assert!(!entered);
    assert!(!domain.0.join("cancelled.txt").exists());
}

#[tokio::test]
async fn protection_poisoned_publication_owner_stays_unavailable() {
    let domain = Domain::new();
    let named = domain.named("poisoned.txt");
    let (gate, mut receiver) = ProtectionGate::channel();
    let signal = TransferCancellation::default();
    let (authorization, response) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(gate.authorize(subject(), named, &signal), async {
            let request = receiver.recv().await.unwrap();
            request.decide(ProtectionDecision::PermitPublication)
        })
    })
    .await
    .expect("owned in-process decision futures retired on timeout");
    let lifetime = gate.lifetime.clone();
    let child = std::thread::spawn(move || {
        let _owner = lifetime.publication.lock().unwrap();
        panic!("owned publication failure fixture");
    });
    let joined = child.join();
    let available = gate.is_available();
    let mut entered = false;
    let publication = authorization.and_then(|authorized| {
        authorized.publish(&signal, || {
            entered = true;
            Ok(())
        })
    });
    drop(receiver);
    drop(gate);
    assert!(joined.is_err());
    assert!(response.is_ok());
    assert!(!available);
    assert_eq!(publication.err(), Some(unavailable()));
    assert!(!entered);
    assert!(!domain.0.join("poisoned.txt").exists());
}

#[tokio::test]
async fn protection_capacity_includes_taken_requests_and_close_retires_them_without_replies() {
    let domain = Domain::new();
    let (gate, mut receiver) = ProtectionGate::channel();
    let gate = std::sync::Arc::new(gate);
    let fixtures: Vec<_> = (0..CAPACITY)
        .map(|index| domain.named(&format!("bounded-{index}.txt")))
        .collect();
    let extra = domain.named("overflow.txt");
    let mut tasks = tokio::task::JoinSet::new();
    let mut requests = vec![];
    for named in fixtures {
        let gate = gate.clone();
        tasks.spawn(async move {
            gate.authorize(subject(), named, &TransferCancellation::default())
                .await
        });
        if let Ok(Some(request)) =
            tokio::time::timeout(Duration::from_secs(5), receiver.recv()).await
        {
            requests.push(request);
        } else {
            break;
        }
    }
    let signal = TransferCancellation::default();
    let overflow = gate.authorize(subject(), extra, &signal);
    let overflow = tokio::time::timeout(Duration::from_secs(5), overflow).await;
    receiver.close(); // Taken request senders remain held; receiver loss must independently wake owners.
    let mut results = vec![];
    while let Some(result) = tasks.join_next().await {
        results.push(result.map(Result::err));
    }
    let stale = requests.iter().all(super::ProtectionRequest::is_cancelled);
    let replies: Vec<_> = requests
        .drain(..)
        .map(|request| request.decide(ProtectionDecision::PermitPublication))
        .collect();
    drop(receiver);
    drop(gate);
    assert_eq!(results.len(), CAPACITY);
    assert!(
        results
            .into_iter()
            .all(|result| result.is_ok_and(|failure| failure == Some(unavailable())))
    );
    assert_eq!(overflow.unwrap().err(), Some(unavailable()));
    assert!(stale);
    assert_eq!(replies.len(), CAPACITY);
    assert!(replies.into_iter().all(|result| result.is_err()));
    assert!(!domain.0.join("overflow.txt").exists());
}

#[test]
fn protection_authorized_owner_consumes_one_slot_until_publication_or_drop() {
    let domain = Domain::new();
    let (gate, receiver) = ProtectionGate::channel();
    let authorized = AuthorizedPartial {
        lifetime: gate.lifetime.clone(),
        named: domain.named("held.txt"),
        _slot: gate.lifetime.slots.clone().try_acquire_owned().unwrap(),
    };
    let held = gate.lifetime.slots.available_permits();
    drop(authorized);
    let released = gate.lifetime.slots.available_permits();
    drop(receiver);
    drop(gate);
    assert_eq!(held, CAPACITY - 1);
    assert_eq!(released, CAPACITY);
    assert!(!domain.0.join("held.txt").exists());
}
