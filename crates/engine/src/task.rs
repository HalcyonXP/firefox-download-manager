//! Persistent task lifecycle, cooperative controls, bounded retries, and events.
//!
//! This module is the native engine boundary consumed by the Native Messaging
//! host in issue #18. It serializes commands per task, keeps persisted metadata
//! authoritative, and exposes full latest-value snapshots independently from a
//! bounded coalescing event queue.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use reqwest::Url;
use thiserror::Error;
use tokio::runtime::Handle;
use tokio::sync::{Notify, watch};
use tokio::time::MissedTickBehavior;

use crate::auth::{ContextError, RequestContext};
use crate::network::{ProbeClient, ProbeError, RangeValidationError, ResourceProbe};
use crate::persistence::{
    CheckpointOutcome, CheckpointUrgency, CleanupOutcome, LoadFailure, PartialCleanup,
    PersistenceError, ResourceIdentity, StateValidationError, TaskId, TaskMetadata, TaskState,
    TaskStore, TimestampMillis, TransferMode,
};
use crate::progress::{
    MAX_SAFE_INTEGER, ProgressConfigError, ProgressEstimate, ProgressPolicy, SpeedEstimator,
};
use crate::scheduler::{
    DownloadScheduler, SchedulerError, TransferCancellation, TransferProgress, WorkerCount,
    transfer_progress_channel,
};
use crate::storage::{IoFailure, PartialFile, StorageError};

/// Maximum automatic retries accepted by protocol-v2 settings.
pub const MAX_RETRIES: u8 = 20;

const DEFAULT_RETRIES: u8 = 5;
const DEFAULT_RETRY_BASE: Duration = Duration::from_millis(250);
const DEFAULT_RETRY_CEILING: Duration = Duration::from_secs(30);
const DEFAULT_MAX_RETRY_AFTER: Duration = Duration::from_secs(5 * 60);
const MIN_RETRY_BASE: Duration = Duration::from_millis(10);
const MAX_RETRY_BASE: Duration = Duration::from_secs(60);
const MAX_RETRY_CEILING: Duration = Duration::from_secs(10 * 60);
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60 * 60);
const MIN_EVENT_CAPACITY: usize = 64;
const MAX_EVENT_CAPACITY: usize = 16_384;
const DEFAULT_EVENT_CAPACITY: usize = 4096;
const MAX_MANAGED_TASKS: usize = 10_000;

/// Invalid retry or task-engine configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TaskConfigError {
    /// Retry count exceeded protocol-v2's maximum of 20.
    #[error("retry count exceeds its supported bound")]
    InvalidRetryCount,
    /// Exponential retry delays were zero, reversed, or above ten minutes.
    #[error("retry delay bounds are invalid")]
    InvalidRetryDelay,
    /// Accepted server Retry-After bound was zero or above one hour.
    #[error("Retry-After bound is invalid")]
    InvalidRetryAfter,
    /// Progress sampling configuration was invalid.
    #[error("progress policy is invalid: {0}")]
    Progress(#[from] ProgressConfigError),
    /// Event queue capacity was below 64 or above 16,384.
    #[error("event queue capacity is outside supported bounds")]
    InvalidEventCapacity,
}

/// Bounded exponential backoff with caller-seeded equal jitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    maximum_retries: u8,
    base_delay: Duration,
    maximum_delay: Duration,
    maximum_retry_after: Duration,
}

impl RetryPolicy {
    /// Constructs a bounded retry policy.
    ///
    /// `maximum_retries` counts attempts after the initial request. Base delay
    /// must be 10 ms through 60 seconds, the exponential ceiling must be at
    /// least the base and no more than ten minutes, and accepted Retry-After
    /// guidance must be nonzero and no more than one hour.
    ///
    /// # Errors
    ///
    /// Rejects values outside those bounds.
    pub fn new(
        maximum_retries: u8,
        base_delay: Duration,
        maximum_delay: Duration,
        maximum_retry_after: Duration,
    ) -> Result<Self, TaskConfigError> {
        if maximum_retries > MAX_RETRIES {
            return Err(TaskConfigError::InvalidRetryCount);
        }
        if base_delay < MIN_RETRY_BASE
            || base_delay > MAX_RETRY_BASE
            || maximum_delay < base_delay
            || maximum_delay > MAX_RETRY_CEILING
        {
            return Err(TaskConfigError::InvalidRetryDelay);
        }
        if maximum_retry_after.is_zero() || maximum_retry_after > MAX_RETRY_AFTER {
            return Err(TaskConfigError::InvalidRetryAfter);
        }
        Ok(Self {
            maximum_retries,
            base_delay,
            maximum_delay,
            maximum_retry_after,
        })
    }

    /// Number of retries available after an initial request.
    #[must_use]
    pub const fn maximum_retries(self) -> u8 {
        self.maximum_retries
    }

    /// Computes a delay for a one-based retry number and caller-provided
    /// entropy. Returns `None` when the budget is exhausted or Retry-After
    /// exceeds the accepted bound, so the caller never retries early.
    #[must_use]
    pub fn delay_for(
        self,
        retry_number: u8,
        retry_after_seconds: Option<u64>,
        entropy: u64,
    ) -> Option<Duration> {
        if retry_number == 0 || retry_number > self.maximum_retries {
            return None;
        }
        let multiplier = 1_u32
            .checked_shl(u32::from(retry_number - 1))
            .unwrap_or(u32::MAX);
        let ceiling = self
            .base_delay
            .saturating_mul(multiplier)
            .min(self.maximum_delay);
        let ceiling_millis = u64::try_from(ceiling.as_millis()).ok()?;
        let floor_millis = ceiling_millis / 2;
        let span = ceiling_millis.saturating_sub(floor_millis);
        let jittered = floor_millis.saturating_add(entropy % span.saturating_add(1));
        let mut delay = Duration::from_millis(jittered);
        if let Some(seconds) = retry_after_seconds {
            let retry_after = Duration::from_secs(seconds);
            if retry_after > self.maximum_retry_after {
                return None;
            }
            delay = delay.max(retry_after);
        }
        Some(delay)
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            maximum_retries: DEFAULT_RETRIES,
            base_delay: DEFAULT_RETRY_BASE,
            maximum_delay: DEFAULT_RETRY_CEILING,
            maximum_retry_after: DEFAULT_MAX_RETRY_AFTER,
        }
    }
}

/// Validated task-engine behavior not supplied per download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskEngineOptions {
    default_workers: WorkerCount,
    retry: RetryPolicy,
    progress: ProgressPolicy,
    event_capacity: usize,
    keep_partial_on_failure: bool,
}

impl TaskEngineOptions {
    /// Constructs engine options.
    ///
    /// # Errors
    ///
    /// Rejects event capacities outside 64 through 16,384. Other arguments are
    /// already validated bounded types.
    pub const fn new(
        default_workers: WorkerCount,
        retry: RetryPolicy,
        progress: ProgressPolicy,
        event_capacity: usize,
    ) -> Result<Self, TaskConfigError> {
        if event_capacity < MIN_EVENT_CAPACITY || event_capacity > MAX_EVENT_CAPACITY {
            return Err(TaskConfigError::InvalidEventCapacity);
        }
        Ok(Self {
            default_workers,
            retry,
            progress,
            event_capacity,
            keep_partial_on_failure: true,
        })
    }

    /// Selects whether terminal failures retain recoverable partial storage.
    #[must_use]
    pub const fn with_failure_retention(mut self, keep: bool) -> Self {
        self.keep_partial_on_failure = keep;
        self
    }

    /// Default per-task worker selection.
    #[must_use]
    pub const fn default_workers(self) -> WorkerCount {
        self.default_workers
    }

    /// Automatic retry policy.
    #[must_use]
    pub const fn retry(self) -> RetryPolicy {
        self.retry
    }

    /// Progress event and speed policy.
    #[must_use]
    pub const fn progress(self) -> ProgressPolicy {
        self.progress
    }

    /// Bounded pending event capacity.
    #[must_use]
    pub const fn event_capacity(self) -> usize {
        self.event_capacity
    }
}

impl Default for TaskEngineOptions {
    fn default() -> Self {
        Self {
            default_workers: WorkerCount::default(),
            retry: RetryPolicy::default(),
            progress: ProgressPolicy::default(),
            event_capacity: DEFAULT_EVENT_CAPACITY,
            keep_partial_on_failure: true,
        }
    }
}

/// Explicit treatment of a partial after cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelPartialPolicy {
    /// Retain durable completed ranges and the managed partial.
    Keep,
    /// Delete the managed partial and retain only cancellation history.
    Delete,
}

/// Stable task failure category for later protocol-v2 error mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskFailureKind {
    /// Recovery lost the memory-only session.
    AuthRequired,
    /// Server denied access or a transferred cookie expired.
    AuthExpired,
    /// A redirect was forbidden before contacting its next origin.
    RedirectRejected,
    /// User-requested cancellation reached a safe checkpoint.
    Cancelled,
    /// HTTP probing could not safely characterize the resource.
    ProbeFailed,
    /// A terminal HTTP status was returned.
    HttpStatus,
    /// A ranged or sequential response violated accepted protocol metadata.
    RangeResponseInvalid,
    /// Resource URL, size, or validator identity changed.
    ResourceChanged,
    /// All bounded transient retries were consumed or server delay was unsafe.
    RetryExhausted,
    /// Generic storage failure.
    Storage,
    /// Destination capacity was exhausted.
    DiskFull,
    /// Filesystem access was denied.
    AccessDenied,
    /// Another process holds an incompatible file lock.
    FileLocked,
    /// Collision-free publication could not be completed.
    FileExists,
    /// Persistent state could not be safely updated or recovered.
    State,
    /// A bounded internal invariant or worker failed.
    Internal,
}

/// Path- and URL-free terminal task failure data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskFailure {
    kind: TaskFailureKind,
    http_status: Option<u16>,
    retry_after_seconds: Option<u64>,
}

impl TaskFailure {
    /// Stable failure category.
    #[must_use]
    pub const fn kind(self) -> TaskFailureKind {
        self.kind
    }

    /// HTTP status when relevant.
    #[must_use]
    pub const fn http_status(self) -> Option<u16> {
        self.http_status
    }

    /// Bounded server retry guidance when relevant.
    #[must_use]
    pub const fn retry_after_seconds(self) -> Option<u64> {
        self.retry_after_seconds
    }

    const fn new(kind: TaskFailureKind) -> Self {
        Self {
            kind,
            http_status: None,
            retry_after_seconds: None,
        }
    }

    const fn http(kind: TaskFailureKind, status: u16, retry_after_seconds: Option<u64>) -> Self {
        Self {
            kind,
            http_status: Some(status),
            retry_after_seconds: safe_retry_after(retry_after_seconds),
        }
    }
}

/// Absolute task progress used both by events and full snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskProgress {
    task_id: TaskId,
    bytes_completed: u64,
    expected_size: Option<u64>,
    speed_bytes_per_second: Option<u64>,
    eta_seconds: Option<u64>,
    active_workers: u8,
    sampled_at: TimestampMillis,
}

impl TaskProgress {
    /// Task receiving bytes.
    #[must_use]
    pub const fn task_id(self) -> TaskId {
        self.task_id
    }

    /// Absolute completed/in-process-safe byte count, never a delta.
    #[must_use]
    pub const fn bytes_completed(self) -> u64 {
        self.bytes_completed
    }

    /// Expected size, or unknown before sequential EOF.
    #[must_use]
    pub const fn expected_size(self) -> Option<u64> {
        self.expected_size
    }

    /// Smoothed speed after enough observations.
    #[must_use]
    pub const fn speed_bytes_per_second(self) -> Option<u64> {
        self.speed_bytes_per_second
    }

    /// Conservative ETA for known size and stable positive speed.
    #[must_use]
    pub const fn eta_seconds(self) -> Option<u64> {
        self.eta_seconds
    }

    /// Currently active worker requests.
    #[must_use]
    pub const fn active_workers(self) -> u8 {
        self.active_workers
    }

    /// Wall-clock timestamp for protocol rendering.
    #[must_use]
    pub const fn sampled_at(self) -> TimestampMillis {
        self.sampled_at
    }
}

/// Full latest-value task view reconstructible independently of event history.
#[derive(Clone, PartialEq, Eq)]
pub struct TaskSnapshot {
    task_id: TaskId,
    display_name: String,
    destination: PathBuf,
    source_origin: String,
    state: TaskState,
    transfer_mode: TransferMode,
    expected_size: Option<u64>,
    bytes_completed: u64,
    workers: WorkerCount,
    speed_bytes_per_second: Option<u64>,
    eta_seconds: Option<u64>,
    active_workers: u8,
    created_at: TimestampMillis,
    updated_at: TimestampMillis,
    failure: Option<TaskFailure>,
}

impl TaskSnapshot {
    /// Stable task ID.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    /// Sanitized user-facing name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Canonical destination intended for privileged protocol responses.
    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// Source HTTP(S) origin without path, query, fragment, or user-info.
    #[must_use]
    pub fn source_origin(&self) -> &str {
        &self.source_origin
    }

    /// Persistent lifecycle state.
    #[must_use]
    pub const fn state(&self) -> TaskState {
        self.state
    }

    /// Selected transfer mode.
    #[must_use]
    pub const fn transfer_mode(&self) -> TransferMode {
        self.transfer_mode
    }

    /// Known expected size.
    #[must_use]
    pub const fn expected_size(&self) -> Option<u64> {
        self.expected_size
    }

    /// Latest absolute byte count.
    #[must_use]
    pub const fn bytes_completed(&self) -> u64 {
        self.bytes_completed
    }

    /// Selected fixed worker count.
    #[must_use]
    pub const fn workers(&self) -> WorkerCount {
        self.workers
    }

    /// Latest smoothed speed.
    #[must_use]
    pub const fn speed_bytes_per_second(&self) -> Option<u64> {
        self.speed_bytes_per_second
    }

    /// Latest conservative ETA.
    #[must_use]
    pub const fn eta_seconds(&self) -> Option<u64> {
        self.eta_seconds
    }

    /// Latest active worker count.
    #[must_use]
    pub const fn active_workers(&self) -> u8 {
        self.active_workers
    }

    /// Creation timestamp.
    #[must_use]
    pub const fn created_at(&self) -> TimestampMillis {
        self.created_at
    }

    /// Last persistent semantic mutation timestamp.
    #[must_use]
    pub const fn updated_at(&self) -> TimestampMillis {
        self.updated_at
    }

    /// Latest terminal error, if any.
    #[must_use]
    pub const fn failure(&self) -> Option<TaskFailure> {
        self.failure
    }
}

impl fmt::Debug for TaskSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TaskSnapshot")
            .field("task_id", &self.task_id)
            .field("display_name", &"<redacted>")
            .field("destination", &"<redacted>")
            .field("source_origin", &self.source_origin)
            .field("state", &self.state)
            .field("transfer_mode", &self.transfer_mode)
            .field("expected_size", &self.expected_size)
            .field("bytes_completed", &self.bytes_completed)
            .field("workers", &self.workers)
            .field("speed_bytes_per_second", &self.speed_bytes_per_second)
            .field("eta_seconds", &self.eta_seconds)
            .field("active_workers", &self.active_workers)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("failure", &self.failure)
            .finish()
    }
}

/// A bounded automatic retry notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryScheduled {
    task_id: TaskId,
    retry_number: u8,
    maximum_retries: u8,
    delay_millis: u64,
    http_status: Option<u16>,
    next_workers: Option<WorkerCount>,
}

impl RetryScheduled {
    /// Effective next transfer width, not a persisted user setting. None during initial probing.
    #[must_use]
    pub const fn next_workers(self) -> Option<WorkerCount> {
        self.next_workers
    }

    /// Affected task.
    #[must_use]
    pub const fn task_id(self) -> TaskId {
        self.task_id
    }

    /// One-based retry number.
    #[must_use]
    pub const fn retry_number(self) -> u8 {
        self.retry_number
    }

    /// Configured retry budget.
    #[must_use]
    pub const fn maximum_retries(self) -> u8 {
        self.maximum_retries
    }

    /// Actual bounded delay after jitter and Retry-After.
    #[must_use]
    pub const fn delay_millis(self) -> u64 {
        self.delay_millis
    }

    /// Triggering HTTP status, when available.
    #[must_use]
    pub const fn http_status(self) -> Option<u16> {
        self.http_status
    }
}

/// Event payload before Native Messaging envelope serialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskEventKind {
    /// Persisted lifecycle changed.
    StateChanged {
        /// Complete replacement snapshot.
        task: TaskSnapshot,
        /// State before the transition.
        previous_state: TaskState,
    },
    /// Coalescible absolute progress sample.
    Progress(TaskProgress),
    /// Engine-local bounded retry bookkeeping. Protocol v2 has no direct
    /// retry-scheduled event shape, so the connection adapter consumes this
    /// replaceable bookkeeping internally rather than serializing a new
    /// discriminator or field.
    RetryScheduled(RetryScheduled),
    /// Task completed publication.
    Completed(TaskSnapshot),
    /// Task reached failed state.
    Failed {
        /// Complete replacement snapshot.
        task: TaskSnapshot,
        /// Stable failure data.
        failure: TaskFailure,
    },
}

/// One connection-neutral event with a monotonically assigned dequeue order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskEvent {
    sequence: u64,
    emitted_at: TimestampMillis,
    kind: TaskEventKind,
}

impl TaskEvent {
    /// Monotonic engine dequeue order bounded for JavaScript. The connection
    /// adapter assigns its own protocol-v2 sequence only to serialized events.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Event wall-clock timestamp.
    #[must_use]
    pub const fn emitted_at(&self) -> TimestampMillis {
        self.emitted_at
    }

    /// Typed payload.
    #[must_use]
    pub const fn kind(&self) -> &TaskEventKind {
        &self.kind
    }
}

/// Task lifecycle operation failure without sensitive URL/path text.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TaskEngineError {
    /// Engine options were invalid.
    #[error("task engine configuration is invalid: {0}")]
    Config(#[from] TaskConfigError),
    /// Task identifier does not exist.
    #[error("task was not found")]
    TaskNotFound,
    /// Command does not apply to the current lifecycle state.
    #[error("task command is invalid for the current state")]
    InvalidTaskState,
    /// Maximum managed task count was reached.
    #[error("managed task count exceeds its bound")]
    TooManyTasks,
    /// A retained partial prevented non-destructive history removal.
    #[error("task retains a partial that requires explicit deletion")]
    PartialRetained,
    /// Persistent metadata validation failed.
    #[error("task metadata is invalid: {0}")]
    State(#[from] StateValidationError),
    /// Persistent state operation failed.
    #[error("task persistence failed: {0}")]
    Persistence(#[from] PersistenceError),
    /// Probe-client construction failed.
    #[error("task probe setup failed: {0}")]
    ProbeSetup(ProbeError),
    /// Scheduler construction failed.
    #[error("task scheduler setup failed: {0}")]
    SchedulerSetup(SchedulerError),
    /// Event sequence exhausted protocol-safe integer space.
    #[error("task event sequence space is exhausted")]
    EventSequenceExhausted,
    /// Waiting command was superseded by another terminal transition.
    #[error("task operation ended in another state")]
    OperationSuperseded,
    /// Workers stopped, but the requested durable control policy failed.
    #[error("task control failed safely: {0:?}")]
    ControlFailed(TaskFailureKind),
    /// No Tokio runtime is available to own network workers.
    #[error("task runtime is unavailable")]
    RuntimeUnavailable,
    /// A bounded internal runtime invariant failed.
    #[error("task runtime invariant failed")]
    Internal,
}

#[derive(Debug, Clone)]
struct ManagedUpdate {
    snapshot: TaskSnapshot,
    running: bool,
}

/// Latest-value subscription for one task. Slow consumers receive the newest
/// complete snapshot rather than an unbounded delta backlog.
pub struct TaskSubscription {
    receiver: watch::Receiver<ManagedUpdate>,
}

impl fmt::Debug for TaskSubscription {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TaskSubscription")
            .field("snapshot", &self.receiver.borrow().snapshot)
            .finish()
    }
}

impl TaskSubscription {
    /// Current complete task snapshot.
    #[must_use]
    pub fn latest(&self) -> TaskSnapshot {
        self.receiver.borrow().snapshot.clone()
    }

    /// Waits for the next coalesced snapshot, or returns `None` if the task
    /// engine was dropped.
    pub async fn changed(&mut self) -> Option<TaskSnapshot> {
        self.receiver.changed().await.ok()?;
        Some(self.receiver.borrow_and_update().snapshot.clone())
    }
}

#[derive(Debug)]
struct PendingEvent {
    emitted_at: TimestampMillis,
    kind: TaskEventKind,
}

#[derive(Debug)]
struct EventState {
    queue: VecDeque<PendingEvent>,
    next_sequence: u64,
}

#[derive(Debug)]
struct EventBuffer {
    capacity: usize,
    state: Mutex<EventState>,
    available: Notify,
    overflowed: AtomicBool,
}

impl EventBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: Mutex::new(EventState {
                queue: VecDeque::with_capacity(capacity),
                next_sequence: 0,
            }),
            available: Notify::new(),
            overflowed: AtomicBool::new(false),
        }
    }

    fn emit(&self, emitted_at: TimestampMillis, kind: TaskEventKind) {
        let progress_task = progress_task_id(&kind);
        let mut state = lock(&self.state);
        if let Some(task_id) = progress_task
            && let Some(index) = state.queue.iter().position(|pending| {
                progress_task_id(&pending.kind).is_some_and(|known| known == task_id)
            })
        {
            state.queue.remove(index);
        }
        if state.queue.len() >= self.capacity {
            if let Some(index) = state
                .queue
                .iter()
                .position(|pending| event_is_replaceable(&pending.kind))
            {
                state.queue.remove(index);
            } else if event_is_replaceable(&kind) {
                return;
            } else {
                self.overflowed.store(true, Ordering::Release);
                return;
            }
        }
        state.queue.push_back(PendingEvent { emitted_at, kind });
        drop(state);
        self.available.notify_one();
    }

    fn remove_task(&self, task_id: TaskId) {
        lock(&self.state)
            .queue
            .retain(|pending| event_task_id(&pending.kind) != task_id);
    }

    fn reset_after_overflow(&self) -> bool {
        let mut state = lock(&self.state);
        if !self.overflowed.swap(false, Ordering::AcqRel) {
            return false;
        }
        state.queue.clear();
        true
    }

    fn try_next(&self) -> Result<Option<TaskEvent>, TaskEngineError> {
        let mut state = lock(&self.state);
        let Some(pending) = state.queue.pop_front() else {
            return Ok(None);
        };
        if state.next_sequence > MAX_SAFE_INTEGER {
            state.queue.push_front(pending);
            return Err(TaskEngineError::EventSequenceExhausted);
        }
        let sequence = state.next_sequence;
        state.next_sequence = sequence + 1;
        Ok(Some(TaskEvent {
            sequence,
            emitted_at: pending.emitted_at,
            kind: pending.kind,
        }))
    }
}

fn progress_task_id(kind: &TaskEventKind) -> Option<TaskId> {
    match kind {
        TaskEventKind::Progress(progress) => Some(progress.task_id),
        _ => None,
    }
}

fn event_task_id(kind: &TaskEventKind) -> TaskId {
    match kind {
        TaskEventKind::StateChanged { task, .. }
        | TaskEventKind::Completed(task)
        | TaskEventKind::Failed { task, .. } => task.task_id(),
        TaskEventKind::Progress(progress) => progress.task_id(),
        TaskEventKind::RetryScheduled(retry) => retry.task_id(),
    }
}

fn event_is_replaceable(kind: &TaskEventKind) -> bool {
    matches!(
        kind,
        TaskEventKind::Progress(_) | TaskEventKind::RetryScheduled(_)
    )
}

/// Startup diagnoses retained separately from validated full snapshots.
#[derive(Debug, Clone, Default)]
pub struct TaskRecoveryReport {
    failures: Vec<LoadFailure>,
    normalized_tasks: Vec<TaskId>,
}

impl TaskRecoveryReport {
    /// Corrupt/unsafe records excluded by conservative persistence recovery.
    #[must_use]
    pub fn failures(&self) -> &[LoadFailure] {
        &self.failures
    }

    /// Tasks moved from interrupted active phases to a safe durable state.
    #[must_use]
    pub fn normalized_tasks(&self) -> &[TaskId] {
        &self.normalized_tasks
    }
}

/// Cloneable persistent native task engine.
#[derive(Clone)]
pub struct TaskEngine {
    inner: Arc<TaskEngineInner>,
}

struct TaskEngineInner {
    store: Arc<TaskStore>,
    scheduler: DownloadScheduler,
    probe_client: ProbeClient,
    options: TaskEngineOptions,
    tasks: Mutex<HashMap<TaskId, Arc<ManagedTask>>>,
    events: EventBuffer,
    recovery: TaskRecoveryReport,
    jitter_counter: AtomicU64,
}

impl fmt::Debug for TaskEngine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TaskEngine")
            .field("options", &self.inner.options)
            .field("task_count", &lock(&self.inner.tasks).len())
            .field("state_root", &"<redacted>")
            .field("event_overflowed", &self.events_require_snapshot())
            .finish_non_exhaustive()
    }
}

struct ManagedTask {
    state: Mutex<ManagedState>,
    updates: watch::Sender<ManagedUpdate>,
}

struct ManagedState {
    metadata: TaskMetadata,
    source_origin: String,
    workers: WorkerCount,
    partial: Option<PartialFile>,
    probe: Option<ResourceProbe>,
    context: Option<Arc<RequestContext>>,
    failure: Option<TaskFailure>,
    progress: TaskProgress,
    estimator: SpeedEstimator,
    last_progress_event: Option<Instant>,
    running: bool,
    removed: bool,
    generation: u64,
    cancellation: Option<TransferCancellation>,
    stop_request: Option<StopRequest>,
}

impl fmt::Debug for ManagedState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagedState")
            .field("metadata", &self.metadata)
            .field("source_origin", &self.source_origin)
            .field("workers", &self.workers)
            .field("has_partial", &self.partial.is_some())
            .field("has_probe", &self.probe.is_some())
            .field("has_context", &self.context.is_some())
            .field("failure", &self.failure)
            .field("progress", &self.progress)
            .field("estimator", &"<rate-window>")
            .field("has_progress_event", &self.last_progress_event.is_some())
            .field("running", &self.running)
            .field("removed", &self.removed)
            .field("generation", &self.generation)
            .field("has_cancellation", &self.cancellation.is_some())
            .field("stop_request", &self.stop_request)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopRequest {
    Pause,
    Cancel(CancelPartialPolicy),
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunKind {
    Initial,
    Resume,
}

#[derive(Debug)]
struct RetryBudget {
    used: u8,
    workers: Option<WorkerCount>,
}

impl RetryBudget {
    const fn new() -> Self {
        Self {
            used: 0,
            workers: None,
        }
    }
}

impl TaskEngine {
    /// Opens persistent state with default scheduler limits.
    ///
    /// # Errors
    ///
    /// Fails on unsafe store ownership/layout, client construction, or an
    /// interrupted task that cannot be normalized safely.
    pub fn open(state_root: &Path, options: TaskEngineOptions) -> Result<Self, TaskEngineError> {
        let scheduler = DownloadScheduler::new().map_err(TaskEngineError::SchedulerSetup)?;
        Self::open_with_scheduler(state_root, options, scheduler)
    }

    /// Opens persistent state with an explicitly configured shared scheduler.
    ///
    /// # Errors
    ///
    /// Fails on unsafe store ownership/layout, client construction, or an
    /// interrupted task that cannot be normalized safely.
    pub fn open_with_scheduler(
        state_root: &Path,
        options: TaskEngineOptions,
        scheduler: DownloadScheduler,
    ) -> Result<Self, TaskEngineError> {
        let store = TaskStore::open(state_root)?;
        let loaded = store.load_all()?;
        let probe_client = ProbeClient::with_admission(scheduler.admission())
            .map_err(TaskEngineError::ProbeSetup)?;
        let mut recovery = TaskRecoveryReport {
            failures: loaded.failures().to_vec(),
            normalized_tasks: Vec::new(),
        };
        let mut tasks = HashMap::with_capacity(loaded.tasks().len());
        for mut metadata in loaded.into_tasks() {
            let previous = metadata.state();
            let failure = normalize_recovered_task(&store, &mut metadata)?;
            if metadata.state() != previous {
                recovery.normalized_tasks.push(metadata.task_id());
            }
            let managed = managed_task(metadata, options.progress, failure)?;
            tasks.insert(managed_id(&managed), managed);
        }
        Ok(Self {
            inner: Arc::new(TaskEngineInner {
                store: Arc::new(store),
                scheduler,
                probe_client,
                options,
                tasks: Mutex::new(tasks),
                events: EventBuffer::new(options.event_capacity),
                recovery,
                jitter_counter: AtomicU64::new(1),
            }),
        })
    }

    /// Reconfigures an exclusively owned, inactive engine without changing task state.
    ///
    /// # Errors
    /// Rejects active runs or another owner; callers must pause work first.
    pub fn reconfigure(
        &mut self,
        options: TaskEngineOptions,
        mut scheduler: DownloadScheduler,
    ) -> Result<(), TaskEngineError> {
        let inner = Arc::get_mut(&mut self.inner).ok_or(TaskEngineError::InvalidTaskState)?;
        if lock(&inner.tasks)
            .values()
            .any(|task| lock(&task.state).running)
        {
            return Err(TaskEngineError::InvalidTaskState);
        }
        if options.event_capacity != inner.options.event_capacity
            || options.progress != inner.options.progress
        {
            return Err(TaskEngineError::InvalidTaskState);
        }
        scheduler
            .inherit_pressure(&inner.scheduler)
            .map_err(TaskEngineError::SchedulerSetup)?;
        let probe_client = ProbeClient::with_admission(scheduler.admission())
            .map_err(TaskEngineError::ProbeSetup)?;
        inner.options = options;
        inner.scheduler = scheduler;
        inner.probe_client = probe_client;
        Ok(())
    }

    /// Conservative startup report.
    #[must_use]
    pub fn recovery_report(&self) -> &TaskRecoveryReport {
        &self.inner.recovery
    }

    /// Creates a queued task using the configured default worker selection.
    ///
    /// # Errors
    ///
    /// Rejects unsafe URL/destination/name input, task-count exhaustion, or a
    /// persistence failure.
    pub fn create_task_default(
        &self,
        original_url: &str,
        destination: &Path,
        suggested_filename: &str,
    ) -> Result<TaskSnapshot, TaskEngineError> {
        self.create_task(
            original_url,
            destination,
            suggested_filename,
            self.inner.options.default_workers,
        )
    }

    /// Creates and critically checkpoints a queued task without starting I/O.
    ///
    /// # Errors
    ///
    /// Rejects unsafe URL/destination/name input, task-count exhaustion, or a
    /// persistence failure.
    pub fn create_task(
        &self,
        original_url: &str,
        destination: &Path,
        suggested_filename: &str,
        workers: WorkerCount,
    ) -> Result<TaskSnapshot, TaskEngineError> {
        self.create_task_with_context(original_url, destination, suggested_filename, workers, None)
    }

    /// Creates a task with memory-only request context and a durable recovery marker.
    /// # Errors
    /// Rejects invalid task input or persistence failure before starting network I/O.
    pub fn create_task_with_context(
        &self,
        original_url: &str,
        destination: &Path,
        suggested_filename: &str,
        workers: WorkerCount,
        context: Option<Arc<RequestContext>>,
    ) -> Result<TaskSnapshot, TaskEngineError> {
        let mut metadata = TaskMetadata::new_with_workers(
            original_url,
            destination,
            suggested_filename,
            workers.get(),
        )?;
        if context.is_some() {
            metadata.require_session();
        }
        let mut tasks = lock(&self.inner.tasks);
        if tasks.len() >= MAX_MANAGED_TASKS {
            return Err(TaskEngineError::TooManyTasks);
        }
        if tasks.contains_key(&metadata.task_id()) {
            return Err(TaskEngineError::Internal);
        }
        self.inner
            .store
            .checkpoint(&metadata, CheckpointUrgency::Critical)?;
        let managed = managed_task(metadata, self.inner.options.progress, None)?;
        lock(&managed.state).context = context;
        let snapshot = lock(&managed.state).snapshot();
        tasks.insert(snapshot.task_id(), managed);
        Ok(snapshot)
    }

    /// Starts one queued task and returns its persisted probing snapshot.
    ///
    /// # Errors
    ///
    /// Rejects missing, running, or non-queued tasks and persistence failures.
    pub fn start(&self, task_id: TaskId) -> Result<TaskSnapshot, TaskEngineError> {
        let task = self.task(task_id)?;
        let runtime = Handle::try_current().map_err(|_| TaskEngineError::RuntimeUnavailable)?;
        let (snapshot, previous, generation, cancellation) = {
            let mut state = lock(&task.state);
            ensure_present(&state)?;
            if state.running || state.metadata.state() != TaskState::Queued {
                return Err(TaskEngineError::InvalidTaskState);
            }
            let previous = state.metadata.state();
            let timestamp = next_timestamp(&state.metadata)?;
            let before = state.metadata.clone();
            state.metadata.transition(TaskState::Probing, timestamp)?;
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
            let snapshot = publish(&task, &state);
            (snapshot, previous, generation, cancellation)
        };
        self.emit_state_changed(snapshot.clone(), previous);
        self.spawn_run(&runtime, task, generation, cancellation, RunKind::Initial);
        Ok(snapshot)
    }

    /// Revalidates and resumes a safely paused task, or treats a failed task as
    /// an explicit user retry. A paused task returns its first
    /// downloading/completed/failed/cancelled state rather than an optimistic
    /// acknowledgement; a failed task returns its newly persisted probing
    /// snapshot.
    ///
    /// # Errors
    ///
    /// Rejects missing, running, or states other than paused/failed.
    pub async fn resume(&self, task_id: TaskId) -> Result<TaskSnapshot, TaskEngineError> {
        let task = self.task(task_id)?;
        let is_failed = {
            let state = lock(&task.state);
            ensure_present(&state)?;
            state.metadata.state() == TaskState::Failed
        };
        if is_failed {
            return self.retry(task_id);
        }
        let runtime = Handle::try_current().map_err(|_| TaskEngineError::RuntimeUnavailable)?;
        let mut subscription = task.updates.subscribe();
        let (generation, cancellation) = {
            let mut state = lock(&task.state);
            ensure_present(&state)?;
            if state.running || state.metadata.state() != TaskState::Paused {
                return Err(TaskEngineError::InvalidTaskState);
            }
            state.failure = None;
            state.estimator.reset();
            let run = begin_run(&mut state)?;
            publish(&task, &state);
            run
        };
        self.spawn_run(&runtime, task, generation, cancellation, RunKind::Resume);
        wait_until(&mut subscription, |update| {
            update.snapshot.state() != TaskState::Paused || !update.running
        })
        .await
    }

    /// Explicitly requeues a failed task, then starts a fresh probe with a new
    /// bounded retry budget.
    ///
    /// # Errors
    ///
    /// Rejects missing, running, or non-failed tasks and persistence failures.
    pub fn retry(&self, task_id: TaskId) -> Result<TaskSnapshot, TaskEngineError> {
        let task = self.task(task_id)?;
        let runtime = Handle::try_current().map_err(|_| TaskEngineError::RuntimeUnavailable)?;
        let (queued, previous) = {
            let mut state = lock(&task.state);
            ensure_present(&state)?;
            if state.running || state.metadata.state() != TaskState::Failed {
                return Err(TaskEngineError::InvalidTaskState);
            }
            let previous = state.metadata.state();
            let queued_at = next_timestamp(&state.metadata)?;
            let failed = state.metadata.clone();
            state.metadata.transition(TaskState::Queued, queued_at)?;
            if let Err(error) = self
                .inner
                .store
                .checkpoint(&state.metadata, CheckpointUrgency::Critical)
            {
                state.metadata = failed;
                return Err(error.into());
            }
            state.failure = None;
            (publish(&task, &state), previous)
        };
        self.emit_state_changed(queued, previous);

        let (probing, generation, cancellation) = {
            let mut state = lock(&task.state);
            ensure_present(&state)?;
            if state.running || state.metadata.state() != TaskState::Queued {
                return Err(TaskEngineError::InvalidTaskState);
            }
            let probing_at = next_timestamp(&state.metadata)?;
            let queued_metadata = state.metadata.clone();
            state.metadata.transition(TaskState::Probing, probing_at)?;
            if let Err(error) = self
                .inner
                .store
                .checkpoint(&state.metadata, CheckpointUrgency::Critical)
            {
                state.metadata = queued_metadata;
                publish(&task, &state);
                return Err(error.into());
            }
            reset_progress(&mut state, probing_at);
            let (generation, cancellation) = begin_run(&mut state)?;
            (publish(&task, &state), generation, cancellation)
        };
        self.emit_state_changed(probing.clone(), TaskState::Queued);
        self.spawn_run(&runtime, task, generation, cancellation, RunKind::Initial);
        Ok(probing)
    }

    /// Requests pause and waits until all network workers stop and completed
    /// bytes are critically checkpointed.
    ///
    /// # Errors
    ///
    /// Rejects missing tasks, non-downloading state, or a competing stop
    /// command. If another terminal transition wins the race, returns
    /// [`TaskEngineError::OperationSuperseded`].
    pub async fn pause(&self, task_id: TaskId) -> Result<TaskSnapshot, TaskEngineError> {
        let task = self.task(task_id)?;
        let mut subscription = task.updates.subscribe();
        {
            let mut state = lock(&task.state);
            ensure_present(&state)?;
            if !state.running
                || state.metadata.state() != TaskState::Downloading
                || state.stop_request.is_some()
            {
                return Err(TaskEngineError::InvalidTaskState);
            }
            state.stop_request = Some(StopRequest::Pause);
            state
                .cancellation
                .as_ref()
                .ok_or(TaskEngineError::Internal)?
                .cancel();
        }
        let snapshot = wait_until(&mut subscription, |update| !update.running).await?;
        if let Some(failure) = snapshot.failure() {
            return Err(TaskEngineError::ControlFailed(failure.kind()));
        }
        if snapshot.state() == TaskState::Paused {
            Ok(snapshot)
        } else {
            Err(TaskEngineError::OperationSuperseded)
        }
    }

    /// Cancels a task at a safe worker boundary and applies the explicit
    /// retained-partial policy before acknowledgement.
    ///
    /// # Errors
    ///
    /// Rejects terminal/promoting tasks or competing controls and propagates
    /// safe checkpoint/deletion failures.
    pub async fn cancel(
        &self,
        task_id: TaskId,
        partial_policy: CancelPartialPolicy,
    ) -> Result<TaskSnapshot, TaskEngineError> {
        let task = self.task(task_id)?;
        let mut subscription = task.updates.subscribe();
        let direct = {
            let mut state = lock(&task.state);
            ensure_present(&state)?;
            if !state.metadata.state().allows(TaskState::Cancelled) || state.stop_request.is_some()
            {
                return Err(TaskEngineError::InvalidTaskState);
            }
            if state.running {
                state.stop_request = Some(StopRequest::Cancel(partial_policy));
                state
                    .cancellation
                    .as_ref()
                    .ok_or(TaskEngineError::Internal)?
                    .cancel();
                None
            } else {
                Some(cancel_inactive(
                    &self.inner,
                    &task,
                    &mut state,
                    partial_policy,
                )?)
            }
        };
        if let Some(snapshot) = direct {
            return Ok(snapshot);
        }
        let snapshot = wait_until(&mut subscription, |update| !update.running).await?;
        if snapshot.state() != TaskState::Cancelled {
            return Err(TaskEngineError::OperationSuperseded);
        }
        if let Some(failure) = snapshot.failure()
            && failure.kind() != TaskFailureKind::Cancelled
        {
            return Err(TaskEngineError::ControlFailed(failure.kind()));
        }
        if partial_policy == CancelPartialPolicy::Delete
            && self.metadata(task_id)?.partial_path().is_some()
        {
            return Err(TaskEngineError::OperationSuperseded);
        }
        Ok(snapshot)
    }

    /// Removes inactive terminal history, optionally deleting a retained
    /// managed partial. Completed final output is never removed.
    ///
    /// # Errors
    ///
    /// Rejects missing, active, or nonterminal tasks. A retained partial must
    /// be explicitly deleted before its history can be removed.
    pub fn remove(&self, task_id: TaskId, delete_partial: bool) -> Result<TaskId, TaskEngineError> {
        let task = self.task(task_id)?;
        {
            let mut state = lock(&task.state);
            ensure_present(&state)?;
            if state.running || !state.metadata.state().is_terminal() {
                return Err(TaskEngineError::InvalidTaskState);
            }
            let cleanup = if delete_partial {
                PartialCleanup::Delete
            } else {
                PartialCleanup::Keep
            };
            match self
                .inner
                .store
                .cleanup_terminal(&state.metadata, cleanup)?
            {
                CleanupOutcome::Retained => return Err(TaskEngineError::PartialRetained),
                CleanupOutcome::Removed => {
                    state.removed = true;
                    publish(&task, &state);
                }
            }
        }
        let mut tasks = lock(&self.inner.tasks);
        if tasks
            .get(&task_id)
            .is_some_and(|known| Arc::ptr_eq(known, &task))
        {
            tasks.remove(&task_id);
        }
        drop(tasks);
        self.inner.events.remove_task(task_id);
        Ok(task_id)
    }

    /// Cooperatively stops every active run for Native Messaging EOF/shutdown.
    /// Downloading work reaches `paused`; interrupted probe/validation phases
    /// fail safely; promotion is allowed to finish its synchronous publication.
    ///
    /// # Errors
    ///
    /// Returns only if an internal task update channel disappears unexpectedly.
    pub async fn shutdown(&self) -> Result<Vec<TaskSnapshot>, TaskEngineError> {
        let tasks: Vec<_> = lock(&self.inner.tasks).values().cloned().collect();
        for task in &tasks {
            let mut state = lock(&task.state);
            if state.removed || !state.running || state.stop_request.is_some() {
                continue;
            }
            if state.metadata.state() == TaskState::Promoting {
                continue;
            }
            state.stop_request = Some(StopRequest::Shutdown);
            state
                .cancellation
                .as_ref()
                .ok_or(TaskEngineError::Internal)?
                .cancel();
        }
        for task in tasks {
            let mut receiver = task.updates.subscribe();
            if receiver.borrow().running {
                wait_until(&mut receiver, |update| !update.running).await?;
            }
        }
        Ok(self.snapshots())
    }

    /// Returns one full latest-value snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`TaskEngineError::TaskNotFound`] for an unknown ID.
    pub fn snapshot(&self, task_id: TaskId) -> Result<TaskSnapshot, TaskEngineError> {
        let task = self.task(task_id)?;
        let state = lock(&task.state);
        ensure_present(&state)?;
        Ok(state.snapshot())
    }

    /// Returns authoritative metadata for trusted native-host/recovery code.
    /// Exact URLs and paths in this value must not enter ordinary logs.
    ///
    /// # Errors
    ///
    /// Returns [`TaskEngineError::TaskNotFound`] for an unknown ID.
    pub fn metadata(&self, task_id: TaskId) -> Result<TaskMetadata, TaskEngineError> {
        let task = self.task(task_id)?;
        let state = lock(&task.state);
        ensure_present(&state)?;
        Ok(state.metadata.clone())
    }

    /// Returns all current snapshots ordered by task ID.
    #[must_use]
    pub fn snapshots(&self) -> Vec<TaskSnapshot> {
        let tasks: Vec<_> = lock(&self.inner.tasks).values().cloned().collect();
        let mut snapshots: Vec<_> = tasks
            .iter()
            .filter_map(|task| {
                let state = lock(&task.state);
                (!state.removed).then(|| state.snapshot())
            })
            .collect();
        snapshots.sort_by_key(TaskSnapshot::task_id);
        snapshots
    }

    /// Subscribes to one task's latest complete snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`TaskEngineError::TaskNotFound`] for an unknown ID.
    pub fn subscribe(&self, task_id: TaskId) -> Result<TaskSubscription, TaskEngineError> {
        let task = self.task(task_id)?;
        let state = lock(&task.state);
        ensure_present(&state)?;
        let receiver = task.updates.subscribe();
        drop(state);
        Ok(TaskSubscription { receiver })
    }

    /// Waits until the current run is no longer active.
    ///
    /// # Errors
    ///
    /// Returns [`TaskEngineError::TaskNotFound`] or if the engine disappears.
    pub async fn wait_until_inactive(
        &self,
        task_id: TaskId,
    ) -> Result<TaskSnapshot, TaskEngineError> {
        let task = self.task(task_id)?;
        let mut receiver = {
            let state = lock(&task.state);
            ensure_present(&state)?;
            task.updates.subscribe()
        };
        if !receiver.borrow().running {
            return Ok(receiver.borrow().snapshot.clone());
        }
        wait_until(&mut receiver, |update| !update.running).await
    }

    /// Pops one pending event without waiting.
    ///
    /// # Errors
    ///
    /// Fails only if protocol-safe sequence space is exhausted.
    pub fn try_next_event(&self) -> Result<Option<TaskEvent>, TaskEngineError> {
        self.inner.events.try_next()
    }

    /// Waits for the next pending event.
    ///
    /// # Errors
    ///
    /// Fails only if protocol-safe sequence space is exhausted.
    pub async fn next_event(&self) -> Result<TaskEvent, TaskEngineError> {
        loop {
            if let Some(event) = self.inner.events.try_next()? {
                return Ok(event);
            }
            self.inner.events.available.notified().await;
        }
    }

    /// Whether critical events overflowed and a consumer must request full
    /// snapshots rather than trusting event continuity.
    #[must_use]
    pub fn events_require_snapshot(&self) -> bool {
        self.inner.events.overflowed.load(Ordering::Acquire)
    }

    /// Clears an overflowed pending-event generation and returns an
    /// authoritative replacement snapshot. Events produced after the clear
    /// remain queued and can be applied after this snapshot.
    #[must_use]
    pub fn take_overflow_snapshot(&self) -> Option<Vec<TaskSnapshot>> {
        self.inner
            .events
            .reset_after_overflow()
            .then(|| self.snapshots())
    }

    fn task(&self, task_id: TaskId) -> Result<Arc<ManagedTask>, TaskEngineError> {
        lock(&self.inner.tasks)
            .get(&task_id)
            .cloned()
            .ok_or(TaskEngineError::TaskNotFound)
    }

    fn spawn_run(
        &self,
        runtime: &Handle,
        task: Arc<ManagedTask>,
        generation: u64,
        cancellation: TransferCancellation,
        kind: RunKind,
    ) {
        let inner = Arc::clone(&self.inner);
        runtime.spawn(async move {
            run_task(inner, task, generation, cancellation, kind).await;
        });
    }

    fn emit_state_changed(&self, snapshot: TaskSnapshot, previous_state: TaskState) {
        self.inner.emit(TaskEventKind::StateChanged {
            task: snapshot,
            previous_state,
        });
    }
}

impl TaskEngineInner {
    fn emit(&self, kind: TaskEventKind) {
        let timestamp = TimestampMillis::now().unwrap_or_else(|_| TimestampMillis::unix_epoch());
        self.events.emit(timestamp, kind);
    }

    fn entropy(&self, task_id: TaskId, retry_number: u8) -> u64 {
        let counter = self.jitter_counter.fetch_add(1, Ordering::Relaxed);
        let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ counter ^ u64::from(retry_number);
        for byte in task_id.to_string().bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }
}

impl ManagedState {
    fn snapshot(&self) -> TaskSnapshot {
        let resource = self.metadata.resource();
        TaskSnapshot {
            task_id: self.metadata.task_id(),
            display_name: self
                .metadata
                .final_path()
                .and_then(Path::file_name)
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_else(|| self.metadata.display_name())
                .to_owned(),
            destination: self.metadata.destination().to_owned(),
            source_origin: self.source_origin.clone(),
            state: self.metadata.state(),
            transfer_mode: resource.map_or(TransferMode::Pending, ResourceIdentity::transfer_mode),
            expected_size: self.progress.expected_size,
            bytes_completed: self.progress.bytes_completed,
            workers: self.workers,
            speed_bytes_per_second: self.progress.speed_bytes_per_second,
            eta_seconds: self.progress.eta_seconds,
            active_workers: self.progress.active_workers,
            created_at: self.metadata.created_at(),
            updated_at: self.metadata.updated_at(),
            failure: self.failure,
        }
    }
}

async fn run_task(
    inner: Arc<TaskEngineInner>,
    task: Arc<ManagedTask>,
    generation: u64,
    cancellation: TransferCancellation,
    kind: RunKind,
) {
    let mut budget = RetryBudget::new();
    let Some((mut probe, partial, mut workers)) =
        prepare_run(&inner, &task, generation, &cancellation, kind, &mut budget).await
    else {
        return;
    };
    budget.workers = Some(workers);
    let mut revalidated_416 = false;
    loop {
        let transfer = run_transfer_attempt(
            &inner,
            &task,
            generation,
            &probe,
            &partial,
            workers,
            &cancellation,
        )
        .await;
        match transfer {
            Ok(()) => {
                if let Err(failure) = checkpoint_boundary(&inner, &task, generation, &partial) {
                    fail_run(&inner, &task, generation, failure);
                } else if cancellation.is_cancelled() {
                    finish_stop(&inner, &task, generation);
                } else {
                    complete_run(&inner, &task, generation, &partial, &cancellation);
                }
                return;
            }
            Err(TransferAttemptError::Checkpoint(failure)) => {
                fail_run(&inner, &task, generation, failure);
                return;
            }
            Err(TransferAttemptError::Scheduler(SchedulerError::Cancelled)) => {
                finish_stop(&inner, &task, generation);
                return;
            }
            Err(TransferAttemptError::Scheduler(error)) => {
                if let Err(failure) = checkpoint_boundary(&inner, &task, generation, &partial) {
                    fail_run(&inner, &task, generation, failure);
                    return;
                }
                if cancellation.is_cancelled() {
                    finish_stop(&inner, &task, generation);
                    return;
                }
                let retry_data = if matches!(error, SchedulerError::HttpStatus { status: 416, .. })
                    && !revalidated_416
                {
                    revalidated_416 = true;
                    match revalidate_worker_range(&inner, &task, &probe, &cancellation, &mut budget)
                        .await
                    {
                        Ok(refreshed) => probe = refreshed,
                        Err(RunError::Cancelled) => {
                            finish_stop(&inner, &task, generation);
                            return;
                        }
                        Err(RunError::Failed(failure)) => {
                            fail_run(&inner, &task, generation, failure);
                            return;
                        }
                    }
                    Some((Some(416), None))
                } else {
                    scheduler_retry_data(&error)
                };
                if let Some((status, retry_after)) = retry_data {
                    workers = workers.reduced();
                    budget.workers = Some(workers);
                    if let Some(delay) =
                        schedule_retry(&inner, task_id(&task), &mut budget, retry_after, status)
                    {
                        mark_retry_wait(&task);
                        if wait_retry(delay, &cancellation).await {
                            continue;
                        }
                        finish_stop(&inner, &task, generation);
                        return;
                    }
                    fail_run(
                        &inner,
                        &task,
                        generation,
                        TaskFailure {
                            kind: TaskFailureKind::RetryExhausted,
                            http_status: status,
                            retry_after_seconds: safe_retry_after(retry_after),
                        },
                    );
                    return;
                }
                fail_run(&inner, &task, generation, failure_from_scheduler(&error));
                return;
            }
        }
    }
}

async fn prepare_run(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    generation: u64,
    cancellation: &TransferCancellation,
    kind: RunKind,
    budget: &mut RetryBudget,
) -> Option<(ResourceProbe, PartialFile, WorkerCount)> {
    let prepared = match kind {
        RunKind::Initial => prepare_initial(inner, task, generation, cancellation, budget).await,
        RunKind::Resume => prepare_resume(inner, task, generation, cancellation, budget).await,
    };
    match prepared {
        Ok(value) => Some(value),
        Err(RunError::Cancelled) => {
            finish_stop(inner, task, generation);
            None
        }
        Err(RunError::Failed(failure)) => {
            fail_run(inner, task, generation, failure);
            None
        }
    }
}

async fn revalidate_worker_range(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    known: &ResourceProbe,
    cancellation: &TransferCancellation,
    budget: &mut RetryBudget,
) -> Result<ResourceProbe, RunError> {
    let url = lock(&task.state).metadata.original_url().to_owned();
    let fresh = probe_with_retries(inner, task_id(task), &url, cancellation, budget).await?;
    if ResourceIdentity::from_probe(&fresh).map_err(run_probe_identity_error)?
        != ResourceIdentity::from_probe(known).map_err(run_probe_identity_error)?
    {
        return Err(RunError::Failed(TaskFailure::new(
            TaskFailureKind::ResourceChanged,
        )));
    }
    Ok(fresh)
}

async fn prepare_initial(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    generation: u64,
    cancellation: &TransferCancellation,
    budget: &mut RetryBudget,
) -> Result<(ResourceProbe, PartialFile, WorkerCount), RunError> {
    let url = {
        let state = lock(&task.state);
        ensure_generation(&state, generation)?;
        state.metadata.original_url().to_owned()
    };
    let probe = probe_with_retries(inner, task_id(task), &url, cancellation, budget).await?;
    if cancellation.is_cancelled() {
        return Err(RunError::Cancelled);
    }

    let (partial, workers, snapshot, previous) = {
        let mut state = lock(&task.state);
        ensure_generation(&state, generation)?;
        if state.metadata.state() != TaskState::Probing {
            return Err(RunError::Failed(TaskFailure::new(TaskFailureKind::State)));
        }
        let timestamp = next_timestamp(&state.metadata).map_err(run_state_error)?;
        let identity = ResourceIdentity::from_probe(&probe).map_err(run_probe_identity_error)?;
        if state.metadata.partial_path().is_some()
            && (state.metadata.resource() != Some(&identity)
                || state.metadata.bytes_completed() > 0
                    && !identity.validators().has_strong_identity())
        {
            return Err(RunError::Failed(TaskFailure::new(
                TaskFailureKind::ResourceChanged,
            )));
        }
        state
            .metadata
            .apply_resource(identity, timestamp)
            .map_err(run_state_error)?;
        let partial = if let Some(partial) = state.partial.clone() {
            partial
        } else if state.metadata.partial_path().is_some() {
            state.metadata.reopen_partial().map_err(run_update_error)?
        } else {
            let partial = match probe.size() {
                Some(size) => PartialFile::create(
                    state.metadata.destination(),
                    state.metadata.display_name(),
                    size,
                ),
                None => PartialFile::create_streaming(
                    state.metadata.destination(),
                    state.metadata.display_name(),
                ),
            }
            .map_err(|error| run_storage_error(&error))?;
            state
                .metadata
                .attach_partial(&partial, timestamp)
                .map_err(run_state_error)?;
            partial
        };
        let previous = state.metadata.state();
        let downloading_at = next_timestamp(&state.metadata).map_err(run_state_error)?;
        state
            .metadata
            .transition(TaskState::Downloading, downloading_at)
            .map_err(run_state_error)?;
        inner
            .store
            .checkpoint(&state.metadata, CheckpointUrgency::Critical)
            .map_err(|error| run_persistence_error(&error))?;
        state.partial = Some(partial.clone());
        state.probe = Some(probe.clone());
        reset_progress(&mut state, downloading_at);
        let workers = state.workers;
        let snapshot = publish(task, &state);
        (partial, workers, snapshot, previous)
    };
    inner.emit(TaskEventKind::StateChanged {
        task: snapshot,
        previous_state: previous,
    });
    Ok((probe, partial, workers))
}

async fn prepare_resume(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    generation: u64,
    cancellation: &TransferCancellation,
    budget: &mut RetryBudget,
) -> Result<(ResourceProbe, PartialFile, WorkerCount), RunError> {
    let url = {
        let state = lock(&task.state);
        ensure_generation(&state, generation)?;
        state.metadata.original_url().to_owned()
    };
    let probe = probe_with_retries(inner, task_id(task), &url, cancellation, budget).await?;
    if cancellation.is_cancelled() {
        return Err(RunError::Cancelled);
    }
    let identity = ResourceIdentity::from_probe(&probe).map_err(run_probe_identity_error)?;

    let (partial, workers, snapshot) = {
        let mut state = lock(&task.state);
        ensure_generation(&state, generation)?;
        if state.metadata.state() != TaskState::Paused
            || state.metadata.resource() != Some(&identity)
            || state.metadata.bytes_completed() > 0 && !identity.validators().has_strong_identity()
        {
            return Err(RunError::Failed(TaskFailure::new(
                TaskFailureKind::ResourceChanged,
            )));
        }
        let partial = if let Some(partial) = state.partial.clone() {
            partial
        } else {
            state.metadata.reopen_partial().map_err(run_update_error)?
        };
        let timestamp = next_timestamp(&state.metadata).map_err(run_state_error)?;
        state
            .metadata
            .transition(TaskState::Downloading, timestamp)
            .map_err(run_state_error)?;
        inner
            .store
            .checkpoint(&state.metadata, CheckpointUrgency::Critical)
            .map_err(|error| run_persistence_error(&error))?;
        state.partial = Some(partial.clone());
        state.probe = Some(probe.clone());
        reset_progress(&mut state, timestamp);
        let workers = state.workers;
        let snapshot = publish(task, &state);
        (partial, workers, snapshot)
    };
    inner.emit(TaskEventKind::StateChanged {
        task: snapshot,
        previous_state: TaskState::Paused,
    });
    Ok((probe, partial, workers))
}

async fn probe_with_retries(
    inner: &TaskEngineInner,
    task_id: TaskId,
    url: &str,
    cancellation: &TransferCancellation,
    budget: &mut RetryBudget,
) -> Result<ResourceProbe, RunError> {
    let context = {
        let tasks = lock(&inner.tasks);
        let task = tasks
            .get(&task_id)
            .ok_or(RunError::Failed(TaskFailure::new(
                TaskFailureKind::Internal,
            )))?;
        let state = lock(&task.state);
        if state.metadata.needs_session() && state.context.is_none() {
            return Err(RunError::Failed(TaskFailure::new(
                TaskFailureKind::AuthRequired,
            )));
        }
        state.context.clone()
    };
    loop {
        let result = tokio::select! {
            result = inner.probe_client.probe_with_context(url, context.clone()) => result,
            () = cancellation.cancelled() => return Err(RunError::Cancelled),
        };
        match result {
            Ok(probe) => return Ok(probe),
            Err(error) if probe_retry_data(&error).is_some() => {
                let data = probe_retry_data(&error);
                let retry_after = data.and_then(|value| value.1);
                let status = data.and_then(|value| value.0);
                let Some(delay) = schedule_retry(inner, task_id, budget, retry_after, status)
                else {
                    return Err(RunError::Failed(TaskFailure {
                        kind: TaskFailureKind::RetryExhausted,
                        http_status: status,
                        retry_after_seconds: safe_retry_after(retry_after),
                    }));
                };
                if !wait_retry(delay, cancellation).await {
                    return Err(RunError::Cancelled);
                }
            }
            Err(error) => return Err(RunError::Failed(failure_from_probe(&error))),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_transfer_attempt(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    generation: u64,
    probe: &ResourceProbe,
    partial: &PartialFile,
    workers: WorkerCount,
    cancellation: &TransferCancellation,
) -> Result<(), TransferAttemptError> {
    let (reporter, mut progress) = transfer_progress_channel();
    let transfer =
        inner
            .scheduler
            .transfer_controlled(probe, partial, workers, cancellation, reporter);
    tokio::pin!(transfer);
    let mut ticker = tokio::time::interval(inner.options.progress.event_interval());
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut dirty = true;
    let mut checkpoint_failure = None;

    loop {
        tokio::select! {
            result = &mut transfer => {
                update_progress(task, generation, progress.latest(), true);
                return match checkpoint_failure {
                    Some(failure) => Err(TransferAttemptError::Checkpoint(failure)),
                    None => result.map(|_| ()).map_err(TransferAttemptError::Scheduler),
                };
            }
            sample = progress.changed() => {
                if let Some(sample) = sample {
                    update_progress(task, generation, sample, false);
                    dirty = true;
                }
            }
            _ = ticker.tick(), if checkpoint_failure.is_none() => {
                let checkpointed = if dirty {
                    match checkpoint_progress(inner, task, generation, partial) {
                        Ok(checkpointed) => checkpointed,
                        Err(failure) => {
                            checkpoint_failure = Some(failure);
                            cancellation.cancel();
                            continue;
                        }
                    }
                } else {
                    true
                };
                sample_progress(task, generation);
                emit_progress(inner, task);
                dirty = !checkpointed;
            }
        }
    }
}

fn update_progress(
    task: &Arc<ManagedTask>,
    generation: u64,
    sample: TransferProgress,
    force_inactive: bool,
) {
    let mut state = lock(&task.state);
    if state.generation != generation || !state.running {
        return;
    }
    let sampled_at = wall_timestamp(&state.metadata);
    state.progress = TaskProgress {
        task_id: state.metadata.task_id(),
        bytes_completed: sample.bytes_completed().min(MAX_SAFE_INTEGER),
        expected_size: sample.expected_size(),
        speed_bytes_per_second: state.progress.speed_bytes_per_second,
        eta_seconds: state.progress.eta_seconds,
        active_workers: if force_inactive {
            0
        } else {
            sample.active_workers()
        },
        sampled_at,
    };
    publish(task, &state);
}

fn sample_progress(task: &Arc<ManagedTask>, generation: u64) {
    let mut state = lock(&task.state);
    if state.generation != generation || !state.running {
        return;
    }
    let bytes = state.progress.bytes_completed;
    let expected = state.progress.expected_size;
    let estimate = state.estimator.sample(bytes, expected);
    apply_estimate(&mut state.progress, estimate);
    state.progress.sampled_at = wall_timestamp(&state.metadata);
    publish(task, &state);
}

fn checkpoint_progress(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    generation: u64,
    partial: &PartialFile,
) -> Result<bool, TaskFailure> {
    let mut state = lock(&task.state);
    if state.generation != generation || !state.running {
        return Err(TaskFailure::new(TaskFailureKind::Internal));
    }
    let timestamp = next_timestamp(&state.metadata).map_err(failure_from_state)?;
    state
        .metadata
        .refresh_completed(partial, timestamp)
        .map_err(failure_from_update)?;
    let outcome = inner
        .store
        .checkpoint(&state.metadata, CheckpointUrgency::Progress)
        .map_err(|error| failure_from_persistence(&error))?;
    publish(task, &state);
    Ok(outcome != CheckpointOutcome::Deferred)
}

fn checkpoint_boundary(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    generation: u64,
    partial: &PartialFile,
) -> Result<(), TaskFailure> {
    let mut state = lock(&task.state);
    if state.generation != generation || !state.running {
        return Err(TaskFailure::new(TaskFailureKind::Internal));
    }
    let timestamp = next_timestamp(&state.metadata).map_err(failure_from_state)?;
    state
        .metadata
        .refresh_completed(partial, timestamp)
        .map_err(failure_from_update)?;
    inner
        .store
        .checkpoint(&state.metadata, CheckpointUrgency::Critical)
        .map_err(|error| failure_from_persistence(&error))?;
    state.progress.bytes_completed = state.metadata.bytes_completed().min(MAX_SAFE_INTEGER);
    state.progress.expected_size = state
        .metadata
        .resource()
        .and_then(ResourceIdentity::expected_size);
    state.progress.active_workers = 0;
    state.progress.sampled_at = timestamp;
    let completed = state.progress.bytes_completed;
    let expected = state.progress.expected_size;
    let estimate = state.estimator.sample(completed, expected);
    apply_estimate(&mut state.progress, estimate);
    publish(task, &state);
    Ok(())
}

fn complete_run(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    generation: u64,
    partial: &PartialFile,
    cancellation: &TransferCancellation,
) {
    if let Err(failure) = enter_run_state(inner, task, generation, TaskState::Validating) {
        finish_or_fail_completion(inner, task, generation, failure);
        return;
    }
    if cancellation.is_cancelled() {
        finish_stop(inner, task, generation);
        return;
    }
    if let Err(failure) = enter_run_state(inner, task, generation, TaskState::Promoting) {
        finish_or_fail_completion(inner, task, generation, failure);
        return;
    }

    let mut promotion = match partial.promote() {
        Ok(promotion) => promotion,
        Err(error) => {
            fail_run(inner, task, generation, failure_from_storage(&error));
            return;
        }
    };
    if let Err(failure) = record_promotion(inner, task, generation, &promotion) {
        leave_promoting_failure(inner, task, failure);
        return;
    }
    if promotion.cleanup_partial().is_ok()
        && let Err(failure) = record_promotion(inner, task, generation, &promotion)
    {
        leave_promoting_failure(inner, task, failure);
        return;
    }

    match transition_for_run(inner, task, generation, TaskState::Completed) {
        Ok((snapshot, previous)) => {
            inner.emit(TaskEventKind::StateChanged {
                task: snapshot.clone(),
                previous_state: previous,
            });
            inner.emit(TaskEventKind::Completed(snapshot));
            let mut state = lock(&task.state);
            finish_running(task, &mut state);
        }
        Err(failure) => leave_promoting_failure(inner, task, failure),
    }
}

fn finish_or_fail_completion(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    generation: u64,
    failure: TaskFailure,
) {
    if failure.kind == TaskFailureKind::Cancelled {
        finish_stop(inner, task, generation);
    } else {
        fail_run(inner, task, generation, failure);
    }
}

fn enter_run_state(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    generation: u64,
    next: TaskState,
) -> Result<(), TaskFailure> {
    let (snapshot, previous) = transition_for_run(inner, task, generation, next)?;
    inner.emit(TaskEventKind::StateChanged {
        task: snapshot,
        previous_state: previous,
    });
    Ok(())
}

fn record_promotion(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    generation: u64,
    promotion: &crate::storage::Promotion,
) -> Result<(), TaskFailure> {
    let mut state = lock(&task.state);
    ensure_generation(&state, generation).map_err(|error| match error {
        RunError::Failed(failure) => failure,
        RunError::Cancelled => TaskFailure::new(TaskFailureKind::Cancelled),
    })?;
    let timestamp = next_timestamp(&state.metadata).map_err(failure_from_state)?;
    state
        .metadata
        .record_promotion(promotion, timestamp)
        .map_err(failure_from_state)?;
    inner
        .store
        .checkpoint(&state.metadata, CheckpointUrgency::Critical)
        .map(|_| ())
        .map_err(|error| failure_from_persistence(&error))
}

fn transition_for_run(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    generation: u64,
    next: TaskState,
) -> Result<(TaskSnapshot, TaskState), TaskFailure> {
    let mut state = lock(&task.state);
    ensure_generation(&state, generation).map_err(|error| match error {
        RunError::Failed(failure) => failure,
        RunError::Cancelled => TaskFailure::new(TaskFailureKind::Cancelled),
    })?;
    let previous = state.metadata.state();
    let timestamp = next_timestamp(&state.metadata).map_err(failure_from_state)?;
    let before = state.metadata.clone();
    state
        .metadata
        .transition(next, timestamp)
        .map_err(failure_from_state)?;
    if let Err(error) = inner
        .store
        .checkpoint(&state.metadata, CheckpointUrgency::Critical)
    {
        state.metadata = before;
        return Err(failure_from_persistence(&error));
    }
    if next == TaskState::Completed {
        state.failure = None;
        state.progress.active_workers = 0;
        state.progress.bytes_completed = state.metadata.bytes_completed().min(MAX_SAFE_INTEGER);
        state.progress.expected_size = state
            .metadata
            .resource()
            .and_then(ResourceIdentity::expected_size);
        state.progress.eta_seconds = Some(0);
    }
    let snapshot = publish(task, &state);
    Ok((snapshot, previous))
}

fn finish_stop(inner: &TaskEngineInner, task: &Arc<ManagedTask>, generation: u64) {
    let partial = {
        let state = lock(&task.state);
        if state.generation != generation || !state.running {
            return;
        }
        matches!(
            state.metadata.state(),
            TaskState::Downloading | TaskState::Paused | TaskState::Validating
        )
        .then(|| state.partial.clone())
        .flatten()
    };
    if let Some(partial) = partial.as_ref()
        && let Err(failure) = checkpoint_boundary(inner, task, generation, partial)
    {
        fail_run(inner, task, generation, failure);
        return;
    }

    let outcome = {
        let mut state = lock(&task.state);
        if state.generation != generation || !state.running {
            return;
        }
        apply_stop(inner, task, &mut state)
    };
    if outcome.snapshot.state() != outcome.previous {
        inner.emit(TaskEventKind::StateChanged {
            task: outcome.snapshot.clone(),
            previous_state: outcome.previous,
        });
    }
    if let Some(failure) = outcome.failure
        && failure.kind != TaskFailureKind::Cancelled
    {
        inner.emit(TaskEventKind::Failed {
            task: outcome.snapshot,
            failure,
        });
    }
}

struct StopOutcome {
    snapshot: TaskSnapshot,
    previous: TaskState,
    failure: Option<TaskFailure>,
}

fn apply_stop(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    state: &mut ManagedState,
) -> StopOutcome {
    let request = requested_stop(state);
    let previous = state.metadata.state();
    let (next, requested_failure) = match (request, previous) {
        (StopRequest::Pause, _) | (StopRequest::Shutdown, TaskState::Downloading) => {
            (Some(TaskState::Paused), None)
        }
        (StopRequest::Cancel(_), _) => (
            Some(TaskState::Cancelled),
            Some(TaskFailure::new(TaskFailureKind::Cancelled)),
        ),
        (StopRequest::Shutdown, TaskState::Paused) => (None, None),
        (StopRequest::Shutdown, _) => (
            Some(TaskState::Failed),
            Some(TaskFailure::new(TaskFailureKind::State)),
        ),
    };
    let before = state.metadata.clone();
    let timestamp = next_timestamp(&state.metadata).unwrap_or_else(|_| state.metadata.updated_at());
    let persisted = if let Some(next) = next {
        state
            .metadata
            .transition(next, timestamp)
            .map_err(failure_from_state)
            .and_then(|()| {
                inner
                    .store
                    .checkpoint(&state.metadata, CheckpointUrgency::Critical)
                    .map(|_| ())
                    .map_err(|error| failure_from_persistence(&error))
            })
    } else {
        Ok(())
    };
    if let Err(failure) = persisted {
        state.metadata = before;
        mark_stop_failed(inner, state, timestamp, failure);
        let snapshot = publish(task, state);
        finish_running(task, state);
        return StopOutcome {
            snapshot,
            previous,
            failure: Some(failure),
        };
    }

    state.failure = requested_failure;
    if request == StopRequest::Cancel(CancelPartialPolicy::Delete)
        && let Err(error) = inner
            .store
            .discard_terminal_partial(&mut state.metadata, timestamp)
    {
        state.failure = Some(failure_from_persistence(&error));
    }
    if state.metadata.partial_path().is_none() {
        state.partial = None;
        state.progress.bytes_completed = 0;
    }
    finish_stop_progress(state, timestamp);
    let failure = state.failure;
    let snapshot = publish(task, state);
    finish_running(task, state);
    StopOutcome {
        snapshot,
        previous,
        failure,
    }
}

fn mark_stop_failed(
    inner: &TaskEngineInner,
    state: &mut ManagedState,
    timestamp: TimestampMillis,
    failure: TaskFailure,
) {
    if state.metadata.state().allows(TaskState::Failed)
        && state
            .metadata
            .transition(TaskState::Failed, timestamp)
            .is_ok()
    {
        let _ = inner
            .store
            .checkpoint(&state.metadata, CheckpointUrgency::Critical);
    }
    state.failure = Some(failure);
    finish_stop_progress(state, timestamp);
}

fn finish_stop_progress(state: &mut ManagedState, timestamp: TimestampMillis) {
    state.progress.active_workers = 0;
    state.progress.speed_bytes_per_second = None;
    state.progress.eta_seconds = None;
    state.progress.expected_size = state
        .metadata
        .resource()
        .and_then(ResourceIdentity::expected_size);
    state.progress.sampled_at = timestamp;
}

fn requested_stop(state: &ManagedState) -> StopRequest {
    state.stop_request.unwrap_or_else(|| {
        if state.metadata.state() == TaskState::Downloading {
            StopRequest::Pause
        } else {
            StopRequest::Cancel(CancelPartialPolicy::Keep)
        }
    })
}

fn cancel_inactive(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    state: &mut ManagedState,
    policy: CancelPartialPolicy,
) -> Result<TaskSnapshot, TaskEngineError> {
    let previous = state.metadata.state();
    let timestamp = next_timestamp(&state.metadata)?;
    let before = state.metadata.clone();
    state.metadata.transition(TaskState::Cancelled, timestamp)?;
    if let Err(error) = inner
        .store
        .checkpoint(&state.metadata, CheckpointUrgency::Critical)
    {
        state.metadata = before;
        return Err(error.into());
    }
    state.failure = Some(TaskFailure::new(TaskFailureKind::Cancelled));
    if policy == CancelPartialPolicy::Delete
        && let Err(error) = inner
            .store
            .discard_terminal_partial(&mut state.metadata, timestamp)
    {
        let failure = failure_from_persistence(&error);
        state.failure = Some(failure);
        if state.metadata.partial_path().is_none() {
            state.partial = None;
            state.progress.bytes_completed = 0;
        }
        finish_stop_progress(state, timestamp);
        let snapshot = publish(task, state);
        inner.emit(TaskEventKind::StateChanged {
            task: snapshot.clone(),
            previous_state: previous,
        });
        inner.emit(TaskEventKind::Failed {
            task: snapshot,
            failure,
        });
        return Err(error.into());
    }
    if policy == CancelPartialPolicy::Delete {
        state.partial = None;
        state.progress.bytes_completed = 0;
    }
    state.progress.active_workers = 0;
    state.progress.speed_bytes_per_second = None;
    state.progress.eta_seconds = None;
    state.progress.sampled_at = timestamp;
    let snapshot = publish(task, state);
    inner.emit(TaskEventKind::StateChanged {
        task: snapshot.clone(),
        previous_state: previous,
    });
    Ok(snapshot)
}

fn fail_run(
    inner: &TaskEngineInner,
    task: &Arc<ManagedTask>,
    generation: u64,
    failure: TaskFailure,
) {
    let event = {
        let mut state = lock(&task.state);
        if state.generation != generation || !state.running {
            return;
        }
        let previous = state.metadata.state();
        let mut terminal_failure = failure;
        if state.metadata.state().allows(TaskState::Failed) {
            match next_timestamp(&state.metadata) {
                Ok(timestamp) => {
                    if state
                        .metadata
                        .transition(TaskState::Failed, timestamp)
                        .is_err()
                    {
                        terminal_failure = TaskFailure::new(TaskFailureKind::State);
                    } else if let Err(error) = inner
                        .store
                        .checkpoint(&state.metadata, CheckpointUrgency::Critical)
                    {
                        terminal_failure = failure_from_persistence(&error);
                    }
                }
                Err(error) => terminal_failure = failure_from_state(error),
            }
        }
        if !inner.options.keep_partial_on_failure && state.metadata.state() == TaskState::Failed {
            let timestamp = wall_timestamp(&state.metadata);
            if let Err(error) = inner
                .store
                .discard_terminal_partial(&mut state.metadata, timestamp)
            {
                terminal_failure = failure_from_persistence(&error);
            }
            if state.metadata.partial_path().is_none() {
                state.partial = None;
                state.progress.bytes_completed = 0;
            }
        }
        state.failure = Some(terminal_failure);
        state.progress.active_workers = 0;
        state.progress.speed_bytes_per_second = None;
        state.progress.eta_seconds = None;
        state.progress.sampled_at = wall_timestamp(&state.metadata);
        let snapshot = publish(task, &state);
        finish_running(task, &mut state);
        (snapshot, previous, terminal_failure)
    };
    if event.0.state() != event.1 {
        inner.emit(TaskEventKind::StateChanged {
            task: event.0.clone(),
            previous_state: event.1,
        });
    }
    inner.emit(TaskEventKind::Failed {
        task: event.0,
        failure: event.2,
    });
}

fn leave_promoting_failure(inner: &TaskEngineInner, task: &Arc<ManagedTask>, failure: TaskFailure) {
    let snapshot = {
        let mut state = lock(&task.state);
        state.failure = Some(failure);
        state.progress.active_workers = 0;
        let snapshot = publish(task, &state);
        finish_running(task, &mut state);
        snapshot
    };
    inner.emit(TaskEventKind::Failed {
        task: snapshot,
        failure,
    });
}

fn finish_running(task: &Arc<ManagedTask>, state: &mut ManagedState) {
    state.running = false;
    if matches!(
        state.metadata.state(),
        TaskState::Completed | TaskState::Cancelled | TaskState::Failed
    ) {
        state.context = None;
        state.probe = None;
    }
    state.cancellation = None;
    state.stop_request = None;
    publish(task, state);
}

fn begin_run(state: &mut ManagedState) -> Result<(u64, TransferCancellation), TaskEngineError> {
    state.generation = state
        .generation
        .checked_add(1)
        .ok_or(TaskEngineError::Internal)?;
    let cancellation = TransferCancellation::new();
    state.running = true;
    state.cancellation = Some(cancellation.clone());
    state.last_progress_event = None;
    state.stop_request = None;
    Ok((state.generation, cancellation))
}

fn reset_progress(state: &mut ManagedState, sampled_at: TimestampMillis) {
    state.estimator.reset();
    let bytes = state.metadata.bytes_completed().min(MAX_SAFE_INTEGER);
    let expected = state
        .metadata
        .resource()
        .and_then(ResourceIdentity::expected_size);
    state.progress = TaskProgress {
        task_id: state.metadata.task_id(),
        bytes_completed: bytes,
        expected_size: expected,
        speed_bytes_per_second: None,
        eta_seconds: expected.filter(|expected| *expected == bytes).map(|_| 0),
        active_workers: 0,
        sampled_at,
    };
}

fn apply_estimate(progress: &mut TaskProgress, estimate: ProgressEstimate) {
    progress.speed_bytes_per_second = estimate.speed_bytes_per_second();
    progress.eta_seconds = estimate.eta_seconds();
}

fn emit_progress(inner: &TaskEngineInner, task: &Arc<ManagedTask>) {
    let progress = {
        let mut state = lock(&task.state);
        let now = Instant::now();
        if state.last_progress_event.is_some_and(|last| {
            now.saturating_duration_since(last) < inner.options.progress.event_interval()
        }) {
            return;
        }
        state.last_progress_event = Some(now);
        state.progress
    };
    inner.emit(TaskEventKind::Progress(progress));
}

fn publish(task: &Arc<ManagedTask>, state: &ManagedState) -> TaskSnapshot {
    let snapshot = state.snapshot();
    task.updates.send_replace(ManagedUpdate {
        snapshot: snapshot.clone(),
        running: state.running,
    });
    snapshot
}

fn managed_task(
    metadata: TaskMetadata,
    progress_policy: ProgressPolicy,
    failure: Option<TaskFailure>,
) -> Result<Arc<ManagedTask>, TaskEngineError> {
    let source_origin = source_origin(metadata.original_url())?;
    let workers =
        WorkerCount::try_from(metadata.workers()).map_err(|_| TaskEngineError::Internal)?;
    let sampled_at = metadata.updated_at();
    let bytes = metadata.bytes_completed().min(MAX_SAFE_INTEGER);
    let expected = metadata
        .resource()
        .and_then(ResourceIdentity::expected_size);
    let progress = TaskProgress {
        task_id: metadata.task_id(),
        bytes_completed: bytes,
        expected_size: expected,
        speed_bytes_per_second: None,
        eta_seconds: expected.filter(|expected| *expected == bytes).map(|_| 0),
        active_workers: 0,
        sampled_at,
    };
    let state = ManagedState {
        metadata,
        source_origin,
        workers,
        partial: None,
        probe: None,
        context: None,
        failure,
        progress,
        estimator: SpeedEstimator::new(progress_policy.speed_window())
            .map_err(TaskConfigError::Progress)?,
        last_progress_event: None,
        running: false,
        removed: false,
        generation: 0,
        cancellation: None,
        stop_request: None,
    };
    let update = ManagedUpdate {
        snapshot: state.snapshot(),
        running: false,
    };
    let (updates, _) = watch::channel(update);
    Ok(Arc::new(ManagedTask {
        state: Mutex::new(state),
        updates,
    }))
}

fn managed_id(task: &Arc<ManagedTask>) -> TaskId {
    lock(&task.state).metadata.task_id()
}

fn task_id(task: &Arc<ManagedTask>) -> TaskId {
    managed_id(task)
}

fn source_origin(url: &str) -> Result<String, TaskEngineError> {
    let parsed = Url::parse(url).map_err(|_| TaskEngineError::Internal)?;
    let origin = parsed.origin().ascii_serialization();
    if origin == "null" || origin.len() > 4096 {
        return Err(TaskEngineError::Internal);
    }
    Ok(origin)
}

fn normalize_recovered_task(
    store: &TaskStore,
    task: &mut TaskMetadata,
) -> Result<Option<TaskFailure>, TaskEngineError> {
    let interrupted = TaskFailure::new(TaskFailureKind::State);
    if matches!(task.state(), TaskState::Failed | TaskState::Cancelled)
        && task.partial_path().is_some_and(|path| {
            matches!(
                std::fs::symlink_metadata(path),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound
            )
        })
    {
        let timestamp = next_timestamp(task)?;
        task.record_partial_deletion(timestamp)?;
        store.checkpoint(task, CheckpointUrgency::Critical)?;
    }
    let target = match task.state() {
        TaskState::Downloading => Some(TaskState::Paused),
        TaskState::Promoting if task.final_path().is_some() => Some(TaskState::Completed),
        TaskState::Probing | TaskState::Validating | TaskState::Promoting => {
            Some(TaskState::Failed)
        }
        _ => None,
    };
    if let Some(target) = target {
        let timestamp = next_timestamp(task)?;
        task.transition(target, timestamp)?;
        store.checkpoint(task, CheckpointUrgency::Critical)?;
    }
    Ok(if task.state() == TaskState::Failed {
        Some(interrupted)
    } else {
        None
    })
}

fn next_timestamp(task: &TaskMetadata) -> Result<TimestampMillis, StateValidationError> {
    let now = TimestampMillis::now()?;
    if now < task.updated_at() {
        Ok(task.updated_at())
    } else {
        Ok(now)
    }
}

fn wall_timestamp(task: &TaskMetadata) -> TimestampMillis {
    next_timestamp(task).unwrap_or_else(|_| task.updated_at())
}

async fn wait_until(
    receiver: &mut watch::Receiver<ManagedUpdate>,
    predicate: impl Fn(&ManagedUpdate) -> bool,
) -> Result<TaskSnapshot, TaskEngineError> {
    if predicate(&receiver.borrow()) {
        return Ok(receiver.borrow().snapshot.clone());
    }
    loop {
        receiver
            .changed()
            .await
            .map_err(|_| TaskEngineError::Internal)?;
        let update = receiver.borrow_and_update();
        if predicate(&update) {
            return Ok(update.snapshot.clone());
        }
    }
}

fn schedule_retry(
    inner: &TaskEngineInner,
    task_id: TaskId,
    budget: &mut RetryBudget,
    retry_after_seconds: Option<u64>,
    http_status: Option<u16>,
) -> Option<Duration> {
    let retry_number = budget.used.checked_add(1)?;
    let delay = inner.options.retry.delay_for(
        retry_number,
        retry_after_seconds,
        inner.entropy(task_id, retry_number),
    )?;
    budget.used = retry_number;
    inner.emit(TaskEventKind::RetryScheduled(RetryScheduled {
        task_id,
        retry_number,
        maximum_retries: inner.options.retry.maximum_retries,
        next_workers: budget.workers,
        delay_millis: u64::try_from(delay.as_millis())
            .unwrap_or(MAX_SAFE_INTEGER)
            .min(MAX_SAFE_INTEGER),
        http_status,
    }));
    Some(delay)
}

fn mark_retry_wait(task: &Arc<ManagedTask>) {
    let mut state = lock(&task.state);
    state.estimator.reset();
    state.progress.speed_bytes_per_second = None;
    state.progress.eta_seconds = None;
    state.progress.active_workers = 0;
    state.progress.sampled_at = wall_timestamp(&state.metadata);
    publish(task, &state);
}

async fn wait_retry(delay: Duration, cancellation: &TransferCancellation) -> bool {
    tokio::select! {
        () = tokio::time::sleep(delay) => !cancellation.is_cancelled(),
        () = cancellation.cancelled() => false,
    }
}

fn probe_retry_data(error: &ProbeError) -> Option<(Option<u16>, Option<u64>)> {
    match error {
        ProbeError::Request => Some((None, None)),
        ProbeError::HttpStatus {
            status,
            retry_after_seconds,
        } if retryable_status(*status) => Some((Some(*status), *retry_after_seconds)),
        _ => None,
    }
}

fn scheduler_retry_data(error: &SchedulerError) -> Option<(Option<u16>, Option<u64>)> {
    match error {
        SchedulerError::Request => Some((None, None)),
        SchedulerError::HttpStatus {
            status,
            retry_after_seconds,
        } if retryable_status(*status) => Some((Some(*status), *retry_after_seconds)),
        _ => None,
    }
}

const fn safe_retry_after(value: Option<u64>) -> Option<u64> {
    match value {
        Some(seconds) if seconds <= MAX_SAFE_INTEGER => Some(seconds),
        _ => None,
    }
}

const fn retryable_status(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

fn failure_from_probe(error: &ProbeError) -> TaskFailure {
    match error {
        ProbeError::Admission(_) => TaskFailure::new(TaskFailureKind::RetryExhausted),
        ProbeError::HttpStatus {
            status,
            retry_after_seconds,
        } => TaskFailure::http(
            if *status == 401 {
                TaskFailureKind::AuthRequired
            } else {
                TaskFailureKind::HttpStatus
            },
            *status,
            *retry_after_seconds,
        ),
        ProbeError::InvalidRange(
            RangeValidationError::ValidatorChanged | RangeValidationError::ValidatorMissing,
        ) => TaskFailure::new(TaskFailureKind::ResourceChanged),
        ProbeError::RedirectRejected => TaskFailure::new(TaskFailureKind::RedirectRejected),
        ProbeError::Context(ContextError::Expired) => {
            TaskFailure::new(TaskFailureKind::AuthExpired)
        }
        ProbeError::Context(ContextError::Invalid) => {
            TaskFailure::new(TaskFailureKind::AuthRequired)
        }
        ProbeError::InvalidRange(_) | ProbeError::BodyLengthMismatch => {
            TaskFailure::new(TaskFailureKind::RangeResponseInvalid)
        }
        _ => TaskFailure::new(TaskFailureKind::ProbeFailed),
    }
}

fn failure_from_scheduler(error: &SchedulerError) -> TaskFailure {
    match error {
        SchedulerError::Context(ContextError::Expired) => {
            TaskFailure::new(TaskFailureKind::AuthExpired)
        }
        SchedulerError::Context(ContextError::Invalid) => {
            TaskFailure::new(TaskFailureKind::AuthRequired)
        }
        SchedulerError::HttpStatus {
            status,
            retry_after_seconds,
        } => TaskFailure::http(
            if *status == 401 {
                TaskFailureKind::AuthRequired
            } else {
                TaskFailureKind::HttpStatus
            },
            *status,
            *retry_after_seconds,
        ),
        SchedulerError::ResponseUrlChanged
        | SchedulerError::InvalidRange(
            RangeValidationError::ValidatorChanged | RangeValidationError::ValidatorMissing,
        )
        | SchedulerError::InvalidSingleResponse(
            RangeValidationError::ValidatorChanged | RangeValidationError::ValidatorMissing,
        ) => TaskFailure::new(TaskFailureKind::ResourceChanged),
        SchedulerError::InvalidRange(_)
        | SchedulerError::InvalidSingleResponse(_)
        | SchedulerError::BodyLengthMismatch => {
            TaskFailure::new(TaskFailureKind::RangeResponseInvalid)
        }
        SchedulerError::Storage(error) => failure_from_storage(error),
        SchedulerError::Cancelled => TaskFailure::new(TaskFailureKind::Cancelled),
        SchedulerError::Admission(_) | SchedulerError::Request => {
            TaskFailure::new(TaskFailureKind::RetryExhausted)
        }
        _ => TaskFailure::new(TaskFailureKind::Internal),
    }
}

fn failure_from_storage(error: &StorageError) -> TaskFailure {
    match error {
        StorageError::Io { failure, .. } => match failure {
            IoFailure::DiskFull => TaskFailure::new(TaskFailureKind::DiskFull),
            IoFailure::AccessDenied => TaskFailure::new(TaskFailureKind::AccessDenied),
            IoFailure::FileLocked => TaskFailure::new(TaskFailureKind::FileLocked),
            IoFailure::AlreadyExists => TaskFailure::new(TaskFailureKind::FileExists),
            IoFailure::NotFound | IoFailure::Unsupported | IoFailure::Other => {
                TaskFailure::new(TaskFailureKind::Storage)
            }
        },
        StorageError::PartialNameExhausted | StorageError::FinalNameExhausted => {
            TaskFailure::new(TaskFailureKind::FileExists)
        }
        _ => TaskFailure::new(TaskFailureKind::Storage),
    }
}

fn failure_from_state(_error: StateValidationError) -> TaskFailure {
    TaskFailure::new(TaskFailureKind::State)
}

fn failure_from_persistence(error: &PersistenceError) -> TaskFailure {
    match error {
        PersistenceError::Io { failure, .. } => match failure {
            IoFailure::DiskFull => TaskFailure::new(TaskFailureKind::DiskFull),
            IoFailure::AccessDenied => TaskFailure::new(TaskFailureKind::AccessDenied),
            IoFailure::FileLocked => TaskFailure::new(TaskFailureKind::FileLocked),
            _ => TaskFailure::new(TaskFailureKind::State),
        },
        _ => TaskFailure::new(TaskFailureKind::State),
    }
}

fn failure_from_update(error: crate::persistence::TaskUpdateError) -> TaskFailure {
    match error {
        crate::persistence::TaskUpdateError::State(error) => failure_from_state(error),
        crate::persistence::TaskUpdateError::Storage(error) => failure_from_storage(&error),
    }
}

#[derive(Debug)]
enum TransferAttemptError {
    Scheduler(SchedulerError),
    Checkpoint(TaskFailure),
}

#[derive(Debug)]
enum RunError {
    Cancelled,
    Failed(TaskFailure),
}

fn ensure_present(state: &ManagedState) -> Result<(), TaskEngineError> {
    if state.removed {
        Err(TaskEngineError::TaskNotFound)
    } else {
        Ok(())
    }
}

fn ensure_generation(state: &ManagedState, generation: u64) -> Result<(), RunError> {
    if state.generation != generation || !state.running {
        Err(RunError::Failed(TaskFailure::new(
            TaskFailureKind::Internal,
        )))
    } else if state
        .cancellation
        .as_ref()
        .is_some_and(TransferCancellation::is_cancelled)
    {
        Err(RunError::Cancelled)
    } else {
        Ok(())
    }
}

fn run_probe_identity_error(_error: StateValidationError) -> RunError {
    RunError::Failed(TaskFailure::new(TaskFailureKind::ProbeFailed))
}

fn run_state_error(error: StateValidationError) -> RunError {
    RunError::Failed(failure_from_state(error))
}

fn run_storage_error(error: &StorageError) -> RunError {
    RunError::Failed(failure_from_storage(error))
}

fn run_persistence_error(error: &PersistenceError) -> RunError {
    RunError::Failed(failure_from_persistence(error))
}

fn run_update_error(error: crate::persistence::TaskUpdateError) -> RunError {
    RunError::Failed(failure_from_update(error))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        EventBuffer, MAX_RETRIES, RetryPolicy, RetryScheduled, TaskConfigError, TaskEngineError,
        TaskEngineOptions, TaskEventKind, TaskFailure, TaskFailureKind, WorkerCount,
    };
    use crate::persistence::{TaskId, TimestampMillis};
    use crate::progress::{MAX_SAFE_INTEGER, ProgressPolicy};

    #[test]
    fn retry_policy_is_bounded_jittered_and_respects_server_delay() {
        assert_eq!(
            RetryPolicy::new(
                MAX_RETRIES + 1,
                Duration::from_millis(10),
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
            Err(TaskConfigError::InvalidRetryCount)
        );
        let policy = RetryPolicy::new(
            3,
            Duration::from_millis(100),
            Duration::from_millis(400),
            Duration::from_secs(2),
        )
        .expect("valid policy");
        let first_low = policy.delay_for(1, None, 0).expect("first retry");
        let first_high = policy.delay_for(1, None, u64::MAX).expect("first retry");
        assert!((Duration::from_millis(50)..=Duration::from_millis(100)).contains(&first_low));
        assert!((Duration::from_millis(50)..=Duration::from_millis(100)).contains(&first_high));
        assert!(
            (Duration::from_millis(100)..=Duration::from_millis(200))
                .contains(&policy.delay_for(2, None, 7).expect("second retry"))
        );
        assert!(
            (Duration::from_millis(200)..=Duration::from_millis(400))
                .contains(&policy.delay_for(3, None, 9).expect("third retry"))
        );
        assert_eq!(policy.delay_for(4, None, 0), None);
        assert_eq!(
            policy.delay_for(1, Some(1), 0),
            Some(Duration::from_secs(1))
        );
        assert_eq!(policy.delay_for(1, Some(3), 0), None);
        assert_eq!(
            RetryPolicy::new(
                1,
                Duration::from_millis(9),
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
            Err(TaskConfigError::InvalidRetryDelay)
        );
        assert_eq!(
            RetryPolicy::new(
                1,
                Duration::from_millis(10),
                Duration::from_secs(1),
                Duration::ZERO,
            ),
            Err(TaskConfigError::InvalidRetryAfter)
        );
    }

    #[test]
    fn protocol_failure_context_drops_inexact_retry_after_values() {
        let failure = TaskFailure::http(TaskFailureKind::HttpStatus, 503, Some(u64::MAX));
        assert_eq!(failure.retry_after_seconds(), None);
    }

    #[test]
    fn event_sequence_exhaustion_never_reuses_the_maximum_value() {
        let events = EventBuffer::new(64);
        super::lock(&events.state).next_sequence = MAX_SAFE_INTEGER;
        let kind = || {
            TaskEventKind::RetryScheduled(RetryScheduled {
                task_id: TaskId::new(),
                retry_number: 1,
                next_workers: None,
                maximum_retries: 1,
                delay_millis: 10,
                http_status: None,
            })
        };
        events.emit(TimestampMillis::unix_epoch(), kind());
        assert_eq!(
            events
                .try_next()
                .expect("last sequence")
                .expect("last event")
                .sequence(),
            MAX_SAFE_INTEGER
        );
        events.emit(TimestampMillis::unix_epoch(), kind());
        assert_eq!(
            events.try_next(),
            Err(TaskEngineError::EventSequenceExhausted)
        );
    }

    #[test]
    fn event_queue_capacity_is_strictly_bounded() {
        assert_eq!(
            TaskEngineOptions::new(
                WorkerCount::Four,
                RetryPolicy::default(),
                ProgressPolicy::default(),
                63,
            ),
            Err(TaskConfigError::InvalidEventCapacity)
        );
        assert!(
            TaskEngineOptions::new(
                WorkerCount::Four,
                RetryPolicy::default(),
                ProgressPolicy::default(),
                64,
            )
            .is_ok()
        );
    }
}
