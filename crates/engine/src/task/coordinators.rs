//! Retained coordinator joins, separate from a task's published inactive flag.
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;

use super::{MAX_MANAGED_TASKS, TaskEngineError, lock};

pub(super) struct RunOwner(AsyncMutex<RunState>);

enum RunState {
    Running(JoinHandle<()>),
    Joined(bool),
}

impl RunOwner {
    pub(super) fn new(handle: JoinHandle<()>) -> Self {
        Self(AsyncMutex::new(RunState::Running(handle)))
    }

    // A borrowed handle stays inside the owner if an awaiting caller is cancelled.
    pub(super) async fn join(&self) -> bool {
        let mut state = self.0.lock().await;
        if let RunState::Running(handle) = &mut *state {
            let successful = handle.await.is_ok();
            *state = RunState::Joined(successful);
        }
        matches!(*state, RunState::Joined(true))
    }

    fn try_join(&self) -> Option<bool> {
        let mut state = self.0.try_lock().ok()?;
        if let RunState::Running(handle) = &mut *state {
            match Pin::new(handle).poll(&mut Context::from_waker(Waker::noop())) {
                Poll::Pending => return None,
                Poll::Ready(result) => *state = RunState::Joined(result.is_ok()),
            }
        }
        match *state {
            RunState::Joined(successful) => Some(successful),
            RunState::Running(_) => None,
        }
    }
}

#[derive(Default)]
pub(super) struct Coordinators(Mutex<Registry>);

#[derive(Default)]
struct Registry {
    closed: bool,
    failed: bool,
    runs: Vec<Arc<RunOwner>>,
}

// Held from BEFORE task activation through coordinator retention. Shutdown
// cannot miss a run in the activation/spawn interval.
pub(super) struct Admission<'a>(MutexGuard<'a, Registry>);

impl Coordinators {
    pub(super) fn admit(&self) -> Result<Admission<'_>, TaskEngineError> {
        let mut state = lock(&self.0);
        if state.closed {
            return Err(TaskEngineError::InvalidTaskState);
        }
        let mut failed = false;
        state.runs.retain(|run| match run.try_join() {
            None => true,
            Some(successful) => {
                failed |= !successful;
                false
            }
        });
        state.failed |= failed;
        if state.failed {
            return Err(TaskEngineError::Internal);
        }
        if state.runs.len() >= MAX_MANAGED_TASKS {
            return Err(TaskEngineError::TooManyTasks);
        }
        Ok(Admission(state))
    }

    pub(super) fn close(&self) -> (Vec<Arc<RunOwner>>, bool) {
        let mut state = lock(&self.0);
        state.closed = true;
        (state.runs.clone(), state.failed)
    }
}

impl Admission<'_> {
    pub(super) fn retain(mut self, run: Arc<RunOwner>) {
        self.0.runs.push(run);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn cancelled_join_waiter_keeps_the_handle_and_completed_joins_are_harvested() {
        let registry = Coordinators::default();
        let admission = registry.admit().expect("admission");
        let (release, wait) = oneshot::channel();
        let owner = Arc::new(RunOwner::new(tokio::spawn(async move {
            let _ = wait.await;
        })));
        admission.retain(Arc::clone(&owner));
        let pending = {
            let mut join = Box::pin(owner.join());
            join.as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        };
        let retained = owner.try_join().is_none();
        let _ = release.send(());
        let joined = owner.join().await;
        let repeated = owner.join().await;
        let next = registry.admit().expect("harvest joined result");
        let empty = next.0.runs.is_empty();
        drop(next);
        assert!(pending && retained && joined && repeated && empty);
    }

    #[tokio::test]
    async fn failed_join_is_memoized_and_cannot_be_harvested_into_success() {
        let registry = Coordinators::default();
        let admission = registry.admit().expect("admission");
        let owner = Arc::new(RunOwner::new(tokio::spawn(async {
            panic!("owned coordinator fixture");
        })));
        admission.retain(Arc::clone(&owner));
        let joined = owner.join().await;
        let repeated = owner.join().await;
        let refused = matches!(registry.admit(), Err(TaskEngineError::Internal));
        let (remaining, failed) = registry.close();
        assert!(!joined && !repeated && refused && failed && remaining.is_empty());
    }

    #[tokio::test]
    async fn retained_slot_capacity_refuses_new_admission_without_discarding_owners() {
        let registry = Coordinators::default();
        let (release, wait) = oneshot::channel();
        let owner = Arc::new(RunOwner::new(tokio::spawn(async move {
            let _ = wait.await;
        })));
        // Metadata fixture for a full registry, not a claim of this many live workers.
        lock(&registry.0).runs = vec![Arc::clone(&owner); MAX_MANAGED_TASKS];
        let refused = matches!(registry.admit(), Err(TaskEngineError::TooManyTasks));
        let (runs, failed) = registry.close();
        let closed = matches!(registry.admit(), Err(TaskEngineError::InvalidTaskState));
        let count = runs.len();
        let _ = release.send(());
        let mut joined = true;
        for run in runs {
            joined &= run.join().await;
        }
        assert!(refused && closed && !failed && joined);
        assert_eq!(count, MAX_MANAGED_TASKS);
    }
}
