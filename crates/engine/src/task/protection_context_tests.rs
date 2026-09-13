//! Real-file component tests; decisions are modeled, not Firefox policy.
use super::super::{
    ProtectionContext, ProtectionDecision, ProtectionGate, Subject, TaskFailureKind,
    TransferCancellation, unavailable,
};
use super::{Domain, subject};
use std::time::Duration;

fn scoped(gate: &ProtectionGate, context: &ProtectionContext) -> Subject {
    let mut subject = subject(gate);
    subject.binding = context.binding();
    subject
}

#[test]
fn context_replacement_keeps_the_engine_receiver_but_never_revives_old_contexts() {
    let (gate, mut receiver) = ProtectionGate::channel();
    let mut first = receiver.open_context().unwrap();
    let overlap = receiver.open_context().err();
    let original = first.id();
    first.close();
    let second = receiver.open_context().unwrap();
    let fresh = second.id();
    let old_live = first.is_available();
    let new_live = second.is_available();
    let gate_live = gate.is_available();
    receiver.close();
    let root_closed = !second.is_available() && !gate.is_available();
    let reopen = receiver.open_context().err();
    drop(second);
    drop(first);
    assert_eq!(overlap, Some(TaskFailureKind::ProtectionUnavailable));
    assert_ne!(original, fresh);
    assert_ne!(fresh, gate.lifetime.id);
    assert!(!old_live && new_live && gate_live && root_closed);
    assert_eq!(reopen, Some(TaskFailureKind::ProtectionUnavailable));
}

#[tokio::test]
async fn context_close_wakes_a_taken_request_without_a_reply() {
    let domain = Domain::new();
    let (gate, mut receiver) = ProtectionGate::channel();
    let mut context = receiver.open_context().unwrap();
    let subject = scoped(&gate, &context);
    let signal = TransferCancellation::default();
    let (authorization, request) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            gate.authorize(subject, domain.named("unanswered.txt"), &signal),
            async {
                let request = receiver.recv().await.unwrap();
                context.close();
                request // Retain the live sender: context notification must wake the wait.
            }
        )
    })
    .await
    .expect("owned in-process authorization futures retired on timeout");
    let cancelled = request.is_cancelled();
    let late = request.decide(ProtectionDecision::PermitPublication);
    let root_live = gate.is_available();
    drop(context);
    drop(receiver);
    assert_eq!(authorization.err(), Some(unavailable()));
    assert!(cancelled && root_live);
    assert_eq!(late, Err(TaskFailureKind::ProtectionUnavailable));
    assert!(!domain.0.join("unanswered.txt").exists());
}

#[tokio::test]
async fn context_replacement_cannot_publish_an_earlier_approved_file() {
    let domain = Domain::new();
    let (gate, mut receiver) = ProtectionGate::channel();
    let mut first = receiver.open_context().unwrap();
    let signal = TransferCancellation::default();
    let (authorization, identity) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            gate.authorize(scoped(&gate, &first), domain.named("stale.txt"), &signal),
            async {
                let request = receiver.recv().await.unwrap();
                let identity = (request.context_id(), request.connection_id());
                request
                    .decide(ProtectionDecision::PermitPublication)
                    .unwrap();
                identity
            }
        )
    })
    .await
    .expect("owned in-process authorization futures retired on timeout");
    first.close();
    let second = receiver.open_context().unwrap();
    let mut old_entered = false;
    let old = authorization.and_then(|authorized| {
        authorized.publish(&signal, || {
            old_entered = true;
            Ok(())
        })
    });
    let (fresh, reply) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            gate.authorize(scoped(&gate, &second), domain.named("fresh.txt"), &signal),
            async {
                let request = receiver.recv().await.unwrap();
                let identity = (request.context_id(), request.connection_id());
                request
                    .decide(ProtectionDecision::PermitPublication)
                    .unwrap();
                identity
            }
        )
    })
    .await
    .expect("owned in-process authorization futures retired on timeout");
    let mut new_entered = false;
    let new = fresh.and_then(|authorized| {
        authorized.publish(&signal, || {
            new_entered = true;
            Ok(())
        })
    });
    let expected = (Some(second.id()), gate.lifetime.id);
    drop(second);
    drop(first);
    drop(receiver);
    assert_eq!(identity.1, reply.1);
    assert_ne!(identity.0, reply.0);
    assert_eq!(reply, expected);
    assert_eq!(old.err(), Some(unavailable()));
    assert!(!old_entered && new_entered);
    assert!(new.is_ok());
    assert!(!domain.0.join("stale.txt").exists());
    assert_eq!(std::fs::read(domain.0.join("fresh.txt")).unwrap(), b"abc");
}

#[tokio::test]
async fn foreign_receiver_context_cannot_dispatch_a_challenge() {
    let domain = Domain::new();
    let (gate, mut receiver) = ProtectionGate::channel();
    let (_other_gate, mut other_receiver) = ProtectionGate::channel();
    let foreign = other_receiver.open_context().unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        gate.authorize(
            scoped(&gate, &foreign),
            domain.named("foreign.txt"),
            &TransferCancellation::default(),
        ),
    )
    .await;
    let queued = receiver.requests.try_recv();
    drop(foreign);
    drop(other_receiver);
    drop(receiver);
    assert_eq!(result.ok().and_then(Result::err), Some(unavailable()));
    assert!(queued.is_err());
    assert!(!domain.0.join("foreign.txt").exists());
}

#[tokio::test]
async fn context_drop_revokes_already_authorized_publication() {
    let domain = Domain::new();
    let (gate, mut receiver) = ProtectionGate::channel();
    let context = receiver.open_context().unwrap();
    let signal = TransferCancellation::default();
    let (authorization, response) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            gate.authorize(
                scoped(&gate, &context),
                domain.named("dropped.txt"),
                &signal
            ),
            async {
                receiver
                    .recv()
                    .await
                    .unwrap()
                    .decide(ProtectionDecision::PermitPublication)
            }
        )
    })
    .await
    .expect("owned in-process authorization futures retired on timeout");
    drop(context);
    let replacement = receiver.open_context().unwrap();
    let mut entered = false;
    let publication = authorization.and_then(|authorized| {
        authorized.publish(&signal, || {
            entered = true;
            Ok(())
        })
    });
    drop(replacement);
    drop(receiver);
    assert!(response.is_ok());
    assert_eq!(publication.err(), Some(unavailable()));
    assert!(!entered && !domain.0.join("dropped.txt").exists());
}

#[tokio::test]
async fn an_entered_publication_is_ordered_before_context_close() {
    let domain = Domain::new();
    let (gate, mut receiver) = ProtectionGate::channel();
    let mut context = receiver.open_context().unwrap();
    let signal = TransferCancellation::default();
    let (authorization, response) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            gate.authorize(
                scoped(&gate, &context),
                domain.named("ordered.txt"),
                &signal
            ),
            async {
                receiver
                    .recv()
                    .await
                    .unwrap()
                    .decide(ProtectionDecision::PermitPublication)
            }
        )
    })
    .await
    .expect("owned in-process authorization futures retired on timeout");
    let authorized = authorization.unwrap();
    let (entered, observed) = std::sync::mpsc::sync_channel(1);
    let (release, released) = std::sync::mpsc::sync_channel(1);
    let publisher = std::thread::spawn(move || {
        authorized.publish(&signal, || {
            entered.send(()).map_err(|_| unavailable())?;
            released.recv().map_err(|_| unavailable())?;
            Ok(())
        })
    });
    let entered = observed.recv_timeout(Duration::from_secs(5));
    let excluded = matches!(
        gate.lifetime.publication.try_lock(),
        Err(std::sync::TryLockError::WouldBlock)
    );
    let closer = std::thread::spawn(move || {
        context.close();
        context.is_available()
    });
    let released = release.send(());
    drop(release);
    let publication_result = publisher.join();
    let retirement_result = closer.join();
    drop(receiver);
    assert!(response.is_ok() && entered.is_ok() && excluded && released.is_ok());
    assert!(publication_result.is_ok_and(|result| result.is_ok()));
    assert!(!retirement_result.unwrap());
    assert_eq!(std::fs::read(domain.0.join("ordered.txt")).unwrap(), b"abc");
}

#[test]
fn poisoned_publication_lock_cannot_issue_a_replacement_context() {
    let (gate, mut receiver) = ProtectionGate::channel();
    let mut first = receiver.open_context().unwrap();
    let lifetime = gate.lifetime.clone();
    let owner = std::thread::spawn(move || {
        let _guard = lifetime.publication.lock().unwrap();
        panic!("owned poisoned publication fixture");
    });
    let joined = owner.join();
    first.close();
    let result = receiver.open_context().err();
    let live = first.is_available();
    drop(first);
    drop(receiver);
    assert!(joined.is_err());
    assert!(!live);
    assert_eq!(result, Some(TaskFailureKind::ProtectionUnavailable));
}

#[tokio::test]
async fn receiver_skips_closed_context_before_its_waiter_is_repolled() {
    use std::future::Future;
    use std::task::Poll;
    let domain = Domain::new();
    let (gate, mut receiver) = ProtectionGate::channel();
    let mut first = receiver.open_context().unwrap();
    let signal = TransferCancellation::default();
    let mut old = Box::pin(gate.authorize(
        scoped(&gate, &first),
        domain.named("queued-old.txt"),
        &signal,
    ));
    std::future::poll_fn(|cx| {
        assert!(old.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    first.close(); // Keep old waiter/sender alive and deliberately unpolled.
    let second = receiver.open_context().unwrap();
    let mut new = Box::pin(gate.authorize(
        scoped(&gate, &second),
        domain.named("queued-new.txt"),
        &signal,
    ));
    std::future::poll_fn(|cx| {
        assert!(new.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let request = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
        .await
        .ok()
        .flatten();
    let identity = request
        .as_ref()
        .and_then(super::super::ProtectionRequest::context_id);
    let correct = identity == Some(second.id());
    let replied = request.map(|request| request.decide(ProtectionDecision::PermitPublication));
    if !correct {
        receiver.close();
    }
    let outcomes = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(&mut old, &mut new)
    })
    .await;
    receiver.close();
    drop(old);
    drop(new);
    drop(first);
    drop(second);
    assert!(
        correct,
        "receiver must skip an unpolled retired-context request"
    );
    assert_eq!(replied, Some(Ok(())));
    let (stale, fresh) = outcomes.expect("owned in-process futures retired before assertions");
    assert_eq!(stale.err(), Some(unavailable()));
    assert!(fresh.is_ok());
}
