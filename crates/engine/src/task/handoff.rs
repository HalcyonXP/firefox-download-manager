//! Durable preparation before browser cancellation; no wire capability is implied.
use super::{
    CheckpointUrgency, Handle, HandoffPhase, MAX_MANAGED_TASKS, ManagedState, Path, RunKind,
    TaskEngine, TaskEngineError, TaskFailure, TaskFailureKind, TaskId, TaskMetadata, TaskSnapshot,
    TaskState, WorkerCount, begin_run, ensure_present, lock, managed_task, next_timestamp, publish,
    reset_progress,
};
use crate::integrity::ExpectedSha256;

/// Validated immutable anonymous-GET preparation. Exact URLs/paths have no Debug output.
pub struct HandoffRequest {
    metadata: TaskMetadata,
    protection: Option<super::ProtectionBinding>,
}

impl HandoffRequest {
    /// Validates a fresh client-chosen ID and immutable transfer inputs, without I/O
    /// to the server or download files. The destination must already be ordinary.
    ///
    /// # Errors
    /// Rejects unsafe URL/destination inputs before any preparation is persisted.
    pub fn new(
        id: TaskId,
        url: &str,
        destination: &Path,
        filename: &str,
        workers: WorkerCount,
        expected: Option<ExpectedSha256>,
    ) -> Result<Self, TaskEngineError> {
        let mut metadata =
            TaskMetadata::new_with_workers(url, destination, filename, workers.get())?
                .for_handoff(id);
        if let Some(expected) = expected {
            metadata.require_checksum(expected);
        }
        Ok(Self {
            metadata,
            protection: None,
        })
    }

    /// Requires a fresh live native protection owner before execution and publication.
    /// The handoff ID also identifies the memory-only browser context binding.
    /// No browser metadata or verdict is accepted by this constructor.
    #[must_use]
    pub fn require_protection(mut self) -> Self {
        self.metadata.require_protection();
        self
    }

    /// Pins this new handoff to the exact native browser-adapter context. Closure
    /// cannot be repaired by repeating preparation with a replacement context.
    /// No context identity or authority is persisted; recovered work stays refused.
    #[must_use]
    pub fn require_protection_in(mut self, context: &super::ProtectionContext) -> Self {
        self.metadata.require_protection();
        self.protection = Some(context.binding());
        self
    }
}

/// Authoritative durable handoff phase plus the latest transfer snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffSnapshot {
    phase: HandoffPhase,
    task: TaskSnapshot,
}

impl HandoffSnapshot {
    #[must_use]
    pub const fn phase(&self) -> HandoffPhase {
        self.phase
    }

    #[must_use]
    pub const fn task(&self) -> &TaskSnapshot {
        &self.task
    }

    fn from_state(state: &ManagedState) -> Result<Self, TaskEngineError> {
        ensure_present(state)?;
        Ok(Self {
            phase: state
                .metadata
                .handoff_phase()
                .ok_or(TaskEngineError::InvalidTaskState)?,
            task: state.snapshot(),
        })
    }
}

impl TaskEngine {
    /// Exclusively records preparation with no probe, worker or download-file I/O.
    /// Repeated identical preparation returns its current phase, never a new task.
    ///
    /// # Errors
    /// Refuses reused IDs with different inputs, normal-task collisions, exhausted
    /// capacity and uncertain/corrupt persisted entries. Never adopts an existing file.
    pub fn prepare_handoff(
        &self,
        request: HandoffRequest,
    ) -> Result<HandoffSnapshot, TaskEngineError> {
        let metadata = request.metadata;
        let mut tasks = lock(&self.inner.tasks);
        if let Some(task) = tasks.get(&metadata.task_id()) {
            let state = lock(&task.state);
            let known = &state.metadata;
            if known.handoff_phase().is_none()
                || known.original_url() != metadata.original_url()
                || known.destination() != metadata.destination()
                || known.display_name() != metadata.display_name()
                || known.workers() != metadata.workers()
                || known.expected_sha256() != metadata.expected_sha256()
                || known.requires_protection() != metadata.requires_protection()
                || match (&request.protection, &state.protection_binding) {
                    (Some(requested), Some(bound)) => !requested.matches(bound),
                    (Some(_), None) => true,
                    (None, Some(bound)) => bound.context_id().is_some(),
                    (None, None) => false,
                }
            {
                return Err(TaskEngineError::InvalidTaskState);
            }
            return HandoffSnapshot::from_state(&state);
        }
        if tasks.len() >= MAX_MANAGED_TASKS {
            return Err(TaskEngineError::TooManyTasks);
        }
        let task = managed_task(metadata, self.inner.options.progress, None)?;
        let snapshot = {
            let mut state = lock(&task.state);
            if state.metadata.requires_protection() {
                state.protection_binding = request.protection.or_else(|| {
                    self.inner
                        .protection
                        .as_ref()
                        .map(super::ProtectionGate::binding)
                });
                // Explicit contexts must belong to this gate and remain live
                // BEFORE exclusive persistence; no foreign/retired adoption.
                if state
                    .protection_binding
                    .as_ref()
                    .is_some_and(|binding| binding.context_id().is_some())
                {
                    super::require_live_protection(&self.inner, &state)?;
                }
            }
            self.inner.store.create(&state.metadata)?;
            HandoffSnapshot::from_state(&state)?
        };
        tasks.insert(snapshot.task.task_id(), task);
        Ok(snapshot)
    }

    /// Reads a preparation/commit/abort outcome without creating or replaying work.
    /// # Errors
    /// Refuses unknown IDs and ordinary tasks that are not handoffs.
    pub fn handoff_status(&self, id: TaskId) -> Result<HandoffSnapshot, TaskEngineError> {
        let task = self.task(id)?;
        HandoffSnapshot::from_state(&lock(&task.state))
    }

    /// Commits after the caller positively observes browser cancellation.
    /// Durable committed/probing state precedes worker creation. A repeated commit
    /// only reads current state: failed/paused/completed tasks are never restarted.
    /// # Errors
    /// Refuses unknown/aborted/ordinary tasks, missing runtime and failed persistence.
    pub fn commit_handoff(&self, id: TaskId) -> Result<HandoffSnapshot, TaskEngineError> {
        let task = self.task(id)?;
        let admission = match self.inner.coordinators.admit() {
            Ok(admission) => admission,
            Err(error) => {
                let state = lock(&task.state);
                if state.metadata.handoff_phase() == Some(HandoffPhase::Committed) {
                    return HandoffSnapshot::from_state(&state);
                }
                return Err(error);
            }
        };
        let (runtime, snapshot, generation, cancellation) = {
            let mut state = lock(&task.state);
            ensure_present(&state)?;
            match state.metadata.handoff_phase() {
                Some(HandoffPhase::Committed) => return HandoffSnapshot::from_state(&state),
                Some(HandoffPhase::Prepared) => {}
                _ => return Err(TaskEngineError::InvalidTaskState),
            }
            super::require_live_protection(&self.inner, &state)?;
            let runtime = Handle::try_current().map_err(|_| TaskEngineError::RuntimeUnavailable)?;
            let timestamp = next_timestamp(&state.metadata)?;
            let before = state.metadata.clone();
            state
                .metadata
                .transition_handoff(HandoffPhase::Committed, timestamp)?;
            if let Err(error) = self
                .inner
                .store
                .checkpoint(&state.metadata, CheckpointUrgency::Critical)
            {
                state.metadata = before;
                return Err(error.into());
            }
            state.failure = None;
            reset_progress(&mut state, timestamp);
            let (generation, cancellation) = begin_run(&mut state)?;
            publish(&task, &state);
            (
                runtime,
                HandoffSnapshot::from_state(&state)?,
                generation,
                cancellation,
            )
        };
        self.emit_state_changed(snapshot.task.clone(), TaskState::Queued);
        self.spawn_run(
            &runtime,
            task,
            generation,
            cancellation,
            RunKind::Initial,
            admission,
        );
        Ok(snapshot)
    }

    /// Durably abandons an uncommitted preparation; retains the ID against replay.
    /// # Errors
    /// Refuses committed/ordinary/unknown tasks and failed persistence. No worker
    /// cancellation is needed: Prepared never authorized a worker in the first place.
    pub fn abort_handoff(&self, id: TaskId) -> Result<HandoffSnapshot, TaskEngineError> {
        let task = self.task(id)?;
        let snapshot = {
            let mut state = lock(&task.state);
            ensure_present(&state)?;
            match state.metadata.handoff_phase() {
                Some(HandoffPhase::Aborted) => return HandoffSnapshot::from_state(&state),
                Some(HandoffPhase::Prepared) => {}
                _ => return Err(TaskEngineError::InvalidTaskState),
            }
            let timestamp = next_timestamp(&state.metadata)?;
            let before = state.metadata.clone();
            state
                .metadata
                .transition_handoff(HandoffPhase::Aborted, timestamp)?;
            if let Err(error) = self
                .inner
                .store
                .checkpoint(&state.metadata, CheckpointUrgency::Critical)
            {
                state.metadata = before;
                return Err(error.into());
            }
            state.failure = Some(TaskFailure::new(TaskFailureKind::Cancelled));
            publish(&task, &state);
            HandoffSnapshot::from_state(&state)?
        };
        self.emit_state_changed(snapshot.task.clone(), TaskState::Queued);
        Ok(snapshot)
    }
}
