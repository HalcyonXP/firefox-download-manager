//! Opt-in native publication ownership. This is not a Firefox policy implementation.
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot, watch};

use super::{
    ManagedState, ManagedTask, TaskEngineError, TaskEngineInner, TaskFailure, TaskFailureKind,
    TaskId, TaskState, enter_run_state, failure_from_storage, lock,
};
use crate::scheduler::TransferCancellation;
use crate::storage::{NamedValidatedPartial, Promotion, ValidatedFingerprint, ValidatedPartial};

const CAPACITY: usize = 32;

/// A complete trusted policy decision, not a reputation callback alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtectionDecision {
    /// Every applicable policy permitted these exact bytes, name and contexts.
    PermitPublication,
    /// A policy blocked the transfer. There is no override command here.
    Blocked,
    /// Missing context, unsupported policy, uncertainty or failed observation.
    Unavailable,
}

struct Lifetime {
    id: TaskId,
    live: AtomicBool,
    publication: Mutex<()>,
    slots: Arc<Semaphore>,
}

/// Engine-side endpoint consumed by one engine, for one trusted native adapter
/// lifetime. Never reconnects or accepts caller-supplied fingerprints. Default
/// engines do not select it.
///
/// ```compile_fail
/// use download_manager_engine::task::ProtectionGate;
/// let (gate, _receiver) = ProtectionGate::channel();
/// let _duplicate = gate.clone();
/// ```
pub struct ProtectionGate {
    lifetime: Arc<Lifetime>,
    requests: mpsc::Sender<ProtectionRequest>,
    closed: watch::Receiver<bool>,
}

/// Sole noncloneable receiving owner. Closing/dropping revokes pending decisions;
/// an already-entered synchronous publication is ordered before that close.
/// This object owns no processes and its closure is not a process join.
pub struct ProtectionReceiver {
    lifetime: Arc<Lifetime>,
    requests: mpsc::Receiver<ProtectionRequest>,
    closed: watch::Sender<bool>,
}

/// Native-origin challenge with one consuming response route. Not serializable,
/// cloneable, or constructible by an extension; URL/name/hash are sensitive.
/// The adapter must independently bind the handoff ID to actual browser context
/// and enforce every applicable policy before permitting publication.
///
/// ```compile_fail
/// use download_manager_engine::task::{ProtectionDecision, ProtectionRequest};
/// fn replay(request: ProtectionRequest) {
///     let _ = request.decide(ProtectionDecision::Blocked);
///     let _ = request.decide(ProtectionDecision::PermitPublication);
/// }
/// ```
pub struct ProtectionRequest {
    lifetime: Arc<Lifetime>,
    id: TaskId,
    task_id: TaskId,
    generation: u64,
    source_url: String,
    file_name: String,
    fingerprint: ValidatedFingerprint,
    response: oneshot::Sender<ProtectionDecision>,
}

impl fmt::Debug for ProtectionRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProtectionRequest(<redacted>)")
    }
}

impl ProtectionRequest {
    /// Fresh UUID for this single decision attempt, including explicit retries.
    #[must_use]
    pub const fn id(&self) -> TaskId {
        self.id
    }

    /// Immutable handoff ID; also the memory-only browser-context association.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    /// Run counter within this engine lifetime, not a restart-stable identity.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Fresh native receiver lifetime identity, never reused on reconnection.
    #[must_use]
    pub fn connection_id(&self) -> TaskId {
        self.lifetime.id
    }

    /// Actual initial native URL. Protected probing and transfer refuse redirects.
    /// Browser history must come from a separately bound native browser snapshot.
    #[must_use]
    pub fn source_url(&self) -> &str {
        &self.source_url
    }

    /// Exact intended final filename; publication will not choose an alternate.
    #[must_use]
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Computed full-width identity under the engine's still-retained file lease.
    #[must_use]
    pub const fn fingerprint(&self) -> ValidatedFingerprint {
        self.fingerprint
    }

    /// A past metadata read is not liveness. This check grants no authority.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        !self.lifetime.live.load(Ordering::Acquire) || self.response.is_closed()
    }

    /// Consumes exactly one result route. Successful queuing is not publication,
    /// transport delivery, policy correctness, or task completion.
    /// # Errors
    /// Refuses retired owners/cancelled attempts; no result can be replayed.
    pub fn decide(self, decision: ProtectionDecision) -> Result<(), TaskFailureKind> {
        if self.is_cancelled() {
            return Err(TaskFailureKind::ProtectionUnavailable);
        }
        self.response
            .send(decision)
            .map_err(|_| TaskFailureKind::ProtectionUnavailable)
    }
}

impl ProtectionGate {
    /// Creates one bounded, in-memory adapter lifetime. No native host is opened.
    /// Holding the receiver is not evidence of Firefox policy readiness.
    #[must_use]
    pub fn channel() -> (Self, ProtectionReceiver) {
        let lifetime = Arc::new(Lifetime {
            id: TaskId::new(),
            live: AtomicBool::new(true),
            publication: Mutex::new(()),
            slots: Arc::new(Semaphore::new(CAPACITY)),
        });
        let (requests, receiver) = mpsc::channel(CAPACITY);
        let (closed, observation) = watch::channel(false);
        (
            Self {
                lifetime: lifetime.clone(),
                requests,
                closed: observation,
            },
            ProtectionReceiver {
                lifetime,
                requests: receiver,
                closed,
            },
        )
    }

    pub(super) fn is_available(&self) -> bool {
        self.lifetime.live.load(Ordering::Acquire)
            && !self.lifetime.publication.is_poisoned()
            && !self.requests.is_closed()
    }

    async fn authorize(
        &self,
        subject: Subject,
        named: NamedValidatedPartial,
        cancellation: &TransferCancellation,
    ) -> Result<AuthorizedPartial, TaskFailure> {
        if !self.is_available() {
            return Err(unavailable());
        }
        let slot = self
            .lifetime
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| unavailable())?;
        let (response, result) = oneshot::channel();
        let request = ProtectionRequest {
            lifetime: self.lifetime.clone(),
            id: TaskId::new(),
            task_id: subject.task_id,
            generation: subject.generation,
            source_url: subject.source_url,
            file_name: named.file_name().as_str().to_owned(),
            fingerprint: named.fingerprint(),
            response,
        };
        self.requests.try_send(request).map_err(|_| unavailable())?;
        let mut closed = self.closed.clone();
        let decision = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(cancelled()),
            _ = closed.changed() => return Err(unavailable()),
            result = result => result.map_err(|_| unavailable())?,
        };
        if cancellation.is_cancelled() {
            return Err(cancelled());
        }
        if !self.is_available() {
            return Err(unavailable());
        }
        match decision {
            ProtectionDecision::PermitPublication => Ok(AuthorizedPartial {
                lifetime: self.lifetime.clone(),
                named,
                _slot: slot,
            }),
            ProtectionDecision::Blocked => {
                Err(TaskFailure::new(TaskFailureKind::ProtectionBlocked))
            }
            ProtectionDecision::Unavailable => Err(unavailable()),
        }
    }
}

impl ProtectionReceiver {
    /// Receives the next live challenge; discards already-cancelled queued records.
    /// Dropping a received challenge refuses that attempt, not a retry/replay.
    pub async fn recv(&mut self) -> Option<ProtectionRequest> {
        while let Some(request) = self.requests.recv().await {
            if !request.is_cancelled() {
                return Some(request);
            }
        }
        None
    }

    /// Permanently revokes this lifetime and all queued/in-flight decisions.
    /// Linearizes against the brief final checkpoint/publication section; never
    /// holds that lock while waiting for a policy service or transfer/hash work.
    pub fn close(&mut self) {
        let _publication = lock(&self.lifetime.publication);
        self.lifetime.live.store(false, Ordering::Release);
        self.closed.send_replace(true);
        self.requests.close();
        while self.requests.try_recv().is_ok() {}
    }
}

impl Drop for ProtectionReceiver {
    fn drop(&mut self) {
        self.close();
    }
}

struct Subject {
    task_id: TaskId,
    generation: u64,
    source_url: String,
}
struct AuthorizedPartial {
    lifetime: Arc<Lifetime>,
    named: NamedValidatedPartial,
    _slot: OwnedSemaphorePermit,
}
impl AuthorizedPartial {
    fn publish(
        self,
        cancellation: &TransferCancellation,
        enter: impl FnOnce() -> Result<(), TaskFailure>,
    ) -> Result<Promotion, TaskFailure> {
        let _publication = self
            .lifetime
            .publication
            .lock()
            .map_err(|_| unavailable())?;
        if !self.lifetime.live.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        if cancellation.is_cancelled() {
            return Err(cancelled());
        }
        enter()?;
        self.named
            .promote()
            .map_err(|error| failure_from_storage(&error))
    }
}

const fn unavailable() -> TaskFailure {
    TaskFailure::new(TaskFailureKind::ProtectionUnavailable)
}
const fn cancelled() -> TaskFailure {
    TaskFailure::new(TaskFailureKind::Cancelled)
}

pub(super) fn require_live_protection(
    inner: &TaskEngineInner,
    state: &ManagedState,
) -> Result<(), TaskEngineError> {
    if state.metadata.requires_protection()
        && (!state.fresh_protection_binding
            || state.context.is_some()
            || !inner
                .protection
                .as_ref()
                .is_some_and(ProtectionGate::is_available))
    {
        return Err(TaskEngineError::ControlFailed(
            TaskFailureKind::ProtectionUnavailable,
        ));
    }
    Ok(())
}

pub(super) async fn publish_validated(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    generation: u64,
    lease: ValidatedPartial,
    cancellation: &TransferCancellation,
) -> Result<Promotion, TaskFailure> {
    let subject = {
        let state = lock(&task.state);
        require_live_protection(inner, &state).map_err(|_| unavailable())?;
        state.metadata.requires_protection().then(|| Subject {
            task_id: state.metadata.task_id(),
            generation,
            source_url: state.metadata.original_url().to_owned(),
        })
    };
    let enter = || enter_run_state(inner, task, generation, TaskState::Promoting);
    if let Some(subject) = subject {
        let named = lease
            .bind_final_name(0)
            .map_err(|error| failure_from_storage(&error))?;
        let authorized = inner
            .protection
            .as_ref()
            .ok_or_else(unavailable)?
            .authorize(subject, named, cancellation)
            .await?;
        authorized.publish(cancellation, enter)
    } else {
        enter()?;
        lease
            .promote()
            .map_err(|error| failure_from_storage(&error))
    }
}

#[cfg(test)]
#[path = "protection_tests.rs"]
mod tests;
