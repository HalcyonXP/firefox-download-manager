use super::{Domain, FIXTURE, failure, inactive, request, server};
use download_manager_engine::{
    persistence::{HandoffPhase, TaskId, TaskState},
    task::{ProtectionDecision, ProtectionGate, TaskEngineError, TaskFailureKind},
};

#[tokio::test]
async fn scoped_preparation_refuses_foreign_or_closed_context_before_persistence() {
    let domain = Domain::new();
    let server = server();
    let url = server.url("/fixture");
    let (engine, mut receiver) = domain.engine();
    let (_foreign_gate, mut foreign_receiver) = ProtectionGate::channel();
    let foreign = foreign_receiver.open_context().unwrap();
    let foreign_id = TaskId::new();
    let foreign_result = engine.prepare_handoff(
        domain
            .request(foreign_id, &url)
            .require_protection_in(&foreign),
    );
    let mut context = receiver.open_context().unwrap();
    let closed_id = TaskId::new();
    let prepared = domain
        .request(closed_id, &url)
        .require_protection_in(&context)
        .require_protection();
    context.close();
    let closed_result = engine.prepare_handoff(prepared);
    let empty = engine.snapshots().is_empty();
    let joined = engine.shutdown().await;
    drop(context);
    drop(foreign);
    drop(foreign_receiver);
    drop(receiver);
    drop(engine);
    drop(server);
    assert!(joined.is_ok() && empty);
    for result in [foreign_result, closed_result] {
        assert!(matches!(
            result,
            Err(TaskEngineError::ControlFailed(
                TaskFailureKind::ProtectionUnavailable
            ))
        ));
    }
    assert!(!domain.record(foreign_id).exists() && !domain.record(closed_id).exists());
    assert!(!domain.output().exists());
}

#[tokio::test]
async fn replacing_context_cannot_commit_or_rebind_an_old_preparation() {
    let domain = Domain::new();
    let server = server();
    let url = server.url("/fixture");
    let (engine, mut receiver) = domain.engine();
    let mut first = receiver.open_context().unwrap();
    let id = TaskId::new();
    let prepared = engine.prepare_handoff(domain.request(id, &url).require_protection_in(&first));
    first.close();
    let second = receiver.open_context().unwrap();
    let replay = engine.prepare_handoff(domain.request(id, &url).require_protection_in(&second));
    let downgrade = engine.prepare_handoff(domain.request(id, &url));
    let repeated = engine.prepare_handoff(domain.request(id, &url).require_protection_in(&first));
    let commit = engine.commit_handoff(id);
    let status = engine.handoff_status(id);
    let joined = engine.shutdown().await;
    drop(second);
    drop(first);
    drop(receiver);
    drop(engine);
    drop(server);
    assert!(joined.is_ok());
    assert!(prepared.is_ok() && repeated.is_ok()); // Same-owner repetition is only status.
    assert!(matches!(replay, Err(TaskEngineError::InvalidTaskState)));
    assert!(matches!(downgrade, Err(TaskEngineError::InvalidTaskState)));
    assert!(matches!(
        commit,
        Err(TaskEngineError::ControlFailed(
            TaskFailureKind::ProtectionUnavailable
        ))
    ));
    assert_eq!(status.unwrap().phase(), HandoffPhase::Prepared);
    assert!(!domain.output().exists());
}

#[tokio::test]
async fn context_loss_refuses_old_work_while_new_context_uses_the_same_engine() {
    let domain = Domain::new();
    let server = server();
    let (engine, mut receiver) = domain.engine();
    let mut first = receiver.open_context().unwrap();
    let old_id = TaskId::new();
    let url = server.url("/fixture");
    engine
        .prepare_handoff(domain.request(old_id, &url).require_protection_in(&first))
        .unwrap();
    let old_commit = engine.commit_handoff(old_id);
    let old_request = request(&mut receiver).await;
    let before = !domain.output().exists();
    first.close();
    let stopped = inactive(&engine, old_id).await;
    let second = receiver.open_context().unwrap();
    let new_id = TaskId::new();
    let new_prepare =
        engine.prepare_handoff(domain.request(new_id, &url).require_protection_in(&second));
    let new_commit = engine.commit_handoff(new_id);
    let new_request = request(&mut receiver).await;
    let identities = old_request
        .as_ref()
        .zip(new_request.as_ref())
        .map(|(old, new)| {
            (
                old.connection_id() == new.connection_id(),
                old.context_id() == Some(first.id()) && new.context_id() == Some(second.id()),
                old.context_id() != new.context_id() && old.id() != new.id(),
                old.task_id() == old_id && new.task_id() == new_id,
            )
        });
    let late = old_request.map(|request| request.decide(ProtectionDecision::PermitPublication));
    let fresh = new_request.map(|request| request.decide(ProtectionDecision::PermitPublication));
    let completed = inactive(&engine, new_id).await;
    let retry = engine.retry(old_id);
    let rebind =
        engine.prepare_handoff(domain.request(old_id, &url).require_protection_in(&second));
    let joined = engine.shutdown().await;
    drop(second);
    drop(first);
    drop(receiver);
    drop(engine);
    drop(server);
    assert!(joined.is_ok() && old_commit.is_ok() && new_prepare.is_ok() && new_commit.is_ok());
    assert!(before);
    assert_eq!(identities, Some((true, true, true, true)));
    assert_eq!(
        failure(stopped.as_ref()),
        Some(TaskFailureKind::ProtectionUnavailable)
    );
    assert_eq!(late, Some(Err(TaskFailureKind::ProtectionUnavailable)));
    assert_eq!(fresh, Some(Ok(())));
    assert_eq!(completed.unwrap().state(), TaskState::Completed);
    assert!(matches!(
        retry,
        Err(TaskEngineError::ControlFailed(
            TaskFailureKind::ProtectionUnavailable
        ))
    ));
    assert!(matches!(rebind, Err(TaskEngineError::InvalidTaskState)));
    assert_eq!(
        std::fs::read(domain.output()).unwrap(),
        FIXTURE.bytes(0, 64 * 1024, 0)
    );
}

#[tokio::test]
async fn reopening_store_cannot_reacquire_a_scoped_preparation() {
    let domain = Domain::new();
    let server = server();
    let url = server.url("/fixture");
    let (engine, mut receiver) = domain.engine();
    let context = receiver.open_context().unwrap();
    let context_id = context.id().to_string();
    let id = TaskId::new();
    let prepared = engine.prepare_handoff(domain.request(id, &url).require_protection_in(&context));
    let first_joined = engine.shutdown().await;
    drop(context);
    drop(receiver);
    drop(engine);
    let (engine, mut receiver) = domain.engine();
    let new_context = receiver.open_context().unwrap();
    let rebind =
        engine.prepare_handoff(domain.request(id, &url).require_protection_in(&new_context));
    let repeat = engine.prepare_handoff(domain.request(id, &url));
    let commit = engine.commit_handoff(id);
    let joined = engine.shutdown().await;
    drop(new_context);
    drop(receiver);
    drop(engine);
    drop(server);
    assert!(first_joined.is_ok() && joined.is_ok() && prepared.is_ok());
    assert!(matches!(rebind, Err(TaskEngineError::InvalidTaskState)));
    assert!(repeat.is_ok());
    assert!(matches!(
        commit,
        Err(TaskEngineError::ControlFailed(
            TaskFailureKind::ProtectionUnavailable
        ))
    ));
    let raw = std::fs::read_to_string(domain.record(id)).unwrap();
    assert!(!raw.contains(&context_id));
    let record: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(record["version"], super::PROTECTED_HANDOFF_FORMAT_VERSION);
    assert!(record.get("context_id").is_none() && record.get("protection_binding").is_none());
    assert!(!domain.output().exists());
}

#[tokio::test]
async fn context_loss_does_not_replace_or_disable_the_independent_manual_engine() {
    let domain = Domain::new();
    let server = server();
    let (engine, mut receiver) = domain.engine();
    let context = receiver.open_context().unwrap();
    let manual = engine
        .create_task_default(&server.url("/fixture"), &domain.downloads, "manual.txt")
        .unwrap();
    drop(context);
    let start = engine.start(manual.task_id());
    let completed = inactive(&engine, manual.task_id()).await;
    let replacement = receiver.open_context();
    let available = replacement.is_ok();
    let joined = engine.shutdown().await;
    drop(replacement);
    drop(receiver);
    drop(engine);
    drop(server);
    assert!(joined.is_ok() && start.is_ok() && available);
    assert_eq!(completed.unwrap().state(), TaskState::Completed);
    assert_eq!(
        std::fs::read(domain.downloads.join("manual.txt")).unwrap(),
        FIXTURE.bytes(0, 64 * 1024, 0)
    );
}
