//! Test-only retained coordinator tail; no process/profile or registration work.
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use tokio::sync::Notify;

use super::{TaskEngine, TaskEngineInner, TaskEngineOptions, lock};

type Retained = Pin<Box<dyn Future<Output = bool> + Send>>;

#[derive(Default)]
pub(super) struct Tail {
    entered: Notify,
    release: Notify,
    retained: Mutex<Vec<Retained>>,
}

impl Tail {
    pub(super) fn retain(&self, join: impl Future<Output = bool> + Send + 'static) {
        lock(&self.retained).push(Box::pin(join));
    }
}

pub(super) async fn at_tail(inner: &TaskEngineInner) {
    let tail = lock(&inner.coordinator_test).clone();
    if let Some(tail) = tail {
        tail.entered.notify_one();
        tail.release.notified().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_waits_for_the_retained_coordinator_not_only_inactive_state() {
    exercise_tail(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_joins_coordinators_after_their_task_was_removed() {
    exercise_tail(true).await;
}

async fn exercise_tail(remove: bool) {
    use crate::integrity::ExpectedSha256;
    use crate::persistence::{PersistenceError, TaskId};
    use crate::scheduler::WorkerCount;
    use download_manager_test_server::{ServerConfig, TestServer};

    let root = std::env::temp_dir().join(format!("dm-coordinator-{}", TaskId::new()));
    std::fs::create_dir(&root).expect("exclusive fixture root");
    let state = root.join("state");
    let destination = root.join("downloads");
    std::fs::create_dir(&state).expect("owned state");
    std::fs::create_dir(&destination).expect("owned destination");
    let server = TestServer::start(ServerConfig::default()).expect("owned HTTP fixture");
    let mut engine = Some(TaskEngine::open(&state, TaskEngineOptions::default()).expect("engine"));
    let tail = Arc::new(Tail::default());
    let owner = engine.as_ref().expect("owner");
    *lock(&owner.inner.coordinator_test) = Some(Arc::clone(&tail));
    let task = owner
        .create_task_with_integrity(
            &server.url("/fixture"),
            &destination,
            "mismatch.bin",
            WorkerCount::Four,
            None,
            ExpectedSha256::parse(&"f".repeat(64)),
        )
        .expect("task");
    owner
        .start(task.task_id())
        .expect("start owned coordinator");

    // After spawn, collect observations without assertions until exact cleanup.
    let entered = tokio::time::timeout(Duration::from_secs(5), tail.entered.notified())
        .await
        .is_ok();
    let mismatch = owner.snapshot(task.task_id()).is_ok_and(|snapshot| {
        snapshot
            .failure()
            .is_some_and(|failure| failure.kind() == super::TaskFailureKind::ChecksumMismatch)
    });
    let removal_ok = !remove || owner.remove(task.task_id(), true).is_ok();
    let first = {
        // Also initiate cancellation if the controlled tail was not reached.
        let mut shutdown = Box::pin(owner.shutdown());
        match shutdown
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(value) => Some(value),
            Poll::Pending => None,
        }
        // Dropping a pending shutdown must not lose its retained join handle.
    };
    let premature = first.is_some();
    let mut locked_after_acknowledgement = false;
    if first.as_ref().is_some_and(Result::is_ok) {
        drop(engine.take());
        match TaskEngine::open(&state, TaskEngineOptions::default()) {
            Err(super::TaskEngineError::Persistence(PersistenceError::StoreLocked)) => {
                locked_after_acknowledgement = true;
            }
            Ok(unexpected) => {
                let _ = unexpected.shutdown().await;
            }
            Err(_) => {}
        }
    }
    tail.release.notify_one();
    let retained = std::mem::take(&mut *lock(&tail.retained));
    let retained_count = retained.len();
    let mut joined = true;
    for handle in retained {
        joined &= handle.await;
    }
    let shutdown_ok = if let Some(owner) = engine.as_ref() {
        owner.shutdown().await.is_ok()
    } else {
        first.as_ref().is_some_and(Result::is_ok)
    };
    drop(engine);
    let reopened = match TaskEngine::open(&state, TaskEngineOptions::default()) {
        Ok(recovered) => recovered.shutdown().await.is_ok(),
        Err(_) => false,
    };
    drop(server);
    if entered && !premature && joined && shutdown_ok && reopened {
        std::fs::remove_dir_all(&root).expect("remove joined owned fixture");
    }
    assert!(
        entered && mismatch && removal_ok,
        "controlled checksum-failure tail was not reached"
    );
    assert_eq!(retained_count, 1, "exact coordinator retention");
    assert!(
        joined && shutdown_ok && reopened,
        "coordinator retirement/reopen failed"
    );
    assert!(
        !premature,
        "shutdown returned before coordinator retirement; StoreLocked after acknowledgement: {locked_after_acknowledgement}"
    );
}
