//! Versioned, crash-safe task metadata and conservative restart recovery.
//!
//! Persisted JSON is an internal format independent of the Native Messaging
//! protocol. Exact URLs and paths are intentionally available to recovery but
//! are redacted from ordinary `Debug` and error output.

use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::{Uuid, Variant};

use crate::network::{EntityTag, ProbeMode, ResourceProbe, Validators};
use crate::progress::MAX_SAFE_INTEGER;
use crate::storage::{
    FileRange, IoFailure, PartialFile, Promotion, StorageError, is_managed_partial_filename,
    sanitize_filename,
};

/// Current internal task-state format version.
pub const STATE_FORMAT_VERSION: u64 = 3;
/// Maximum bytes accepted for one task-state file.
pub const MAX_STATE_BYTES: usize = 256 * 1024;
/// Maximum canonical completed ranges accepted per task.
pub const MAX_COMPLETED_RANGES: usize = 8_192;
/// Default maximum frequency for routine progress checkpoints.
pub const DEFAULT_CHECKPOINT_INTERVAL: Duration = Duration::from_secs(1);

const STATE_FORMAT_NAME: &str = "firefox-download-manager-task";
const TASK_FILE_SUFFIX: &str = ".task.json";
const TEMP_FILE_MARKER: &str = ".task.json.tmp-";
const STORE_LOCK_NAME: &str = ".task-store.lock";
const TASK_DIRECTORY_NAME: &str = "tasks";
const MAX_TASK_FILES: usize = 10_000;
const MAX_DIRECTORY_ENTRIES: usize = 20_000;
const MAX_URL_BYTES: usize = 16 * 1024;
const DEFAULT_TASK_WORKERS: u8 = 4;
const MAX_TIMESTAMP_MILLIS: u64 = 253_402_300_799_999;
const MIN_CHECKPOINT_INTERVAL: Duration = Duration::from_millis(100);
const MAX_CHECKPOINT_INTERVAL: Duration = Duration::from_secs(60);
const TEMP_CREATE_ATTEMPTS: u64 = 32;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

/// Stable opaque task identifier represented canonically as a lowercase UUID.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(Uuid);

impl TaskId {
    /// Creates a random RFC 4122 version-4 identifier using the operating
    /// system randomness source selected by `uuid`/`getrandom`.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parses one canonical lowercase hyphenated UUID.
    ///
    /// # Errors
    ///
    /// Rejects alternate UUID text forms and invalid identifiers.
    pub fn parse(value: &str) -> Result<Self, StateValidationError> {
        let parsed = Uuid::from_str(value).map_err(|_| StateValidationError::InvalidTaskId)?;
        if parsed.hyphenated().to_string() != value
            || parsed.get_version_num() != 4
            || parsed.get_variant() != Variant::RFC4122
        {
            return Err(StateValidationError::InvalidTaskId);
        }
        Ok(Self(parsed))
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0.hyphenated())
    }
}

impl fmt::Debug for TaskId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "TaskId({})", self.0.hyphenated())
    }
}

/// Milliseconds since the Unix epoch, bounded to an RFC 3339-compatible year.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimestampMillis(u64);

impl TimestampMillis {
    /// Infallible internal fallback for nonpersistent event timestamps.
    pub(crate) const fn unix_epoch() -> Self {
        Self(0)
    }

    /// Creates a bounded timestamp.
    ///
    /// # Errors
    ///
    /// Rejects values beyond year 9999.
    pub const fn new(value: u64) -> Result<Self, StateValidationError> {
        if value > MAX_TIMESTAMP_MILLIS {
            return Err(StateValidationError::InvalidTimestamp);
        }
        Ok(Self(value))
    }

    /// Captures the current system time.
    ///
    /// # Errors
    ///
    /// Returns an error when the clock predates the Unix epoch or exceeds the
    /// persisted timestamp bound.
    pub fn now() -> Result<Self, StateValidationError> {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StateValidationError::InvalidTimestamp)?;
        let millis = u64::try_from(duration.as_millis())
            .map_err(|_| StateValidationError::InvalidTimestamp)?;
        Self::new(millis)
    }

    /// Integer milliseconds persisted on disk.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Explicit native task lifecycle states, aligned with protocol v2 names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    /// Accepted locally but not yet probing.
    Queued,
    /// Characterizing the HTTP resource.
    Probing,
    /// Network workers may write validated bytes.
    Downloading,
    /// No worker may write; completed coverage remains recoverable.
    Paused,
    /// Exact coverage/size/checksum validation is in progress.
    Validating,
    /// Atomic final publication is in progress.
    Promoting,
    /// Final publication succeeded.
    Completed,
    /// A fatal operation failed; explicit retry may requeue.
    Failed,
    /// User cancellation reached a safe checkpoint.
    Cancelled,
}

impl TaskState {
    /// Whether no ordinary forward transfer transition leaves this state.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Whether this exact transition is part of the accepted lifecycle graph.
    #[must_use]
    pub const fn allows(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Probing)
                | (Self::Probing | Self::Paused, Self::Downloading)
                | (Self::Downloading, Self::Paused | Self::Validating)
                | (Self::Validating, Self::Promoting)
                | (Self::Promoting, Self::Completed | Self::Failed)
                | (Self::Failed, Self::Queued)
                | (
                    Self::Queued
                        | Self::Probing
                        | Self::Downloading
                        | Self::Paused
                        | Self::Validating,
                    Self::Failed | Self::Cancelled
                )
        )
    }
}

/// Persisted transfer strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferMode {
    /// Probe has not selected a mode.
    Pending,
    /// One sequential response owns all bytes.
    Single,
    /// Multiple exact ranged responses may own disjoint assignments.
    Segmented,
}

/// Validation failures for task metadata and state mutations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum StateValidationError {
    /// Task identifier is not canonical.
    #[error("task identifier is invalid")]
    InvalidTaskId,
    /// Timestamp is out of range or moved backwards.
    #[error("task timestamp is invalid")]
    InvalidTimestamp,
    /// Revision arithmetic overflowed.
    #[error("task revision is invalid")]
    InvalidRevision,
    /// URL syntax, scheme, user-info, or length bound is invalid.
    #[error("persisted URL is invalid")]
    InvalidUrl,
    /// Destination is not a bounded absolute ordinary directory path.
    #[error("persisted destination is invalid")]
    InvalidDestination,
    /// Display/final filename is not already sanitized and bounded.
    #[error("persisted filename is invalid")]
    InvalidFilename,
    /// Resource mode, size, or final URL is inconsistent.
    #[error("persisted resource identity is invalid")]
    InvalidResource,
    /// `ETag` or `Last-Modified` data is malformed.
    #[error("persisted resource validator is invalid")]
    InvalidValidator,
    /// Partial file path is not confined to the destination.
    #[error("persisted partial path is invalid")]
    InvalidPartialPath,
    /// Final file path is not confined to the destination.
    #[error("persisted final path is invalid")]
    InvalidFinalPath,
    /// Completed ranges are malformed, overlapping, adjacent, or out of bounds.
    #[error("persisted completed ranges are invalid")]
    InvalidCompletedRanges,
    /// Persisted worker selection is not exactly 1, 2, 4, or 8.
    #[error("persisted worker count is invalid")]
    InvalidWorkerCount,
    /// State and stored resource/file data disagree.
    #[error("persisted task fields are inconsistent with lifecycle state")]
    InconsistentState,
    /// Requested lifecycle edge is not allowed.
    #[error("task state transition is invalid")]
    InvalidTransition,
}

/// Failure while creating a bytes-first durable progress snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TaskUpdateError {
    /// Task metadata or lifecycle is inconsistent.
    #[error("task update is invalid: {0}")]
    State(#[from] StateValidationError),
    /// Completed file bytes could not be flushed before metadata capture.
    #[error("task bytes could not be checkpointed: {0}")]
    Storage(#[from] StorageError),
}

/// Resource identity retained for strict resume validation.
#[derive(Clone, PartialEq, Eq)]
pub struct ResourceIdentity {
    final_url: String,
    expected_size: Option<u64>,
    validators: Validators,
    transfer_mode: TransferMode,
}

impl ResourceIdentity {
    /// Creates a validated resumable resource identity.
    ///
    /// # Errors
    ///
    /// Rejects unsupported URLs and inconsistent transfer mode/size values.
    pub fn new(
        final_url: &str,
        expected_size: Option<u64>,
        validators: Validators,
        transfer_mode: TransferMode,
    ) -> Result<Self, StateValidationError> {
        let final_url = normalize_url(final_url)?;
        validate_validators(&validators)?;
        if transfer_mode == TransferMode::Pending
            || expected_size.is_some_and(|size| size > MAX_SAFE_INTEGER)
            || transfer_mode == TransferMode::Segmented
                && expected_size.is_none_or(|size| size == 0)
        {
            return Err(StateValidationError::InvalidResource);
        }
        Ok(Self {
            final_url,
            expected_size,
            validators,
            transfer_mode,
        })
    }

    /// Converts a validated HTTP probe into persistent identity data.
    ///
    /// # Errors
    ///
    /// Returns an error only if an internal probe value violates persistence
    /// bounds.
    pub fn from_probe(probe: &ResourceProbe) -> Result<Self, StateValidationError> {
        let mode = match probe.mode() {
            ProbeMode::Segmented => TransferMode::Segmented,
            ProbeMode::SingleStream(_) | ProbeMode::Empty => TransferMode::Single,
        };
        Self::new(
            probe.final_url().as_str(),
            probe.size(),
            probe.validators().clone(),
            mode,
        )
    }

    /// Final URL after accepted redirects. This exact value can be sensitive.
    #[must_use]
    pub fn final_url(&self) -> &str {
        &self.final_url
    }

    /// Validated resource size, if known.
    #[must_use]
    pub const fn expected_size(&self) -> Option<u64> {
        self.expected_size
    }

    /// HTTP validators retained for future range/resume checks.
    #[must_use]
    pub const fn validators(&self) -> &Validators {
        &self.validators
    }

    /// Selected transfer strategy.
    #[must_use]
    pub const fn transfer_mode(&self) -> TransferMode {
        self.transfer_mode
    }
}

impl fmt::Debug for ResourceIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResourceIdentity")
            .field("final_url", &"<redacted>")
            .field("expected_size", &self.expected_size)
            .field("validators", &self.validators)
            .field("transfer_mode", &self.transfer_mode)
            .finish()
    }
}

/// Complete authoritative internal task metadata.
#[derive(Clone, PartialEq, Eq)]
pub struct TaskMetadata {
    task_id: TaskId,
    revision: u64,
    state: TaskState,
    original_url: String,
    needs_session: bool,
    resource: Option<ResourceIdentity>,
    destination: PathBuf,
    display_name: String,
    workers: u8,
    partial_path: Option<PathBuf>,
    final_path: Option<PathBuf>,
    completed_ranges: Vec<FileRange>,
    created_at: TimestampMillis,
    updated_at: TimestampMillis,
}

impl TaskMetadata {
    /// Creates a queued task with a new stable ID and current timestamp.
    ///
    /// # Errors
    ///
    /// Rejects an unsafe URL or destination.
    pub fn new(
        original_url: &str,
        destination: &Path,
        suggested_filename: &str,
    ) -> Result<Self, StateValidationError> {
        Self::new_with_workers(
            original_url,
            destination,
            suggested_filename,
            DEFAULT_TASK_WORKERS,
        )
    }

    /// Creates a queued task with a persisted fixed worker selection.
    ///
    /// # Errors
    ///
    /// Rejects unsafe task input or a worker count other than 1, 2, 4, or 8.
    pub fn new_with_workers(
        original_url: &str,
        destination: &Path,
        suggested_filename: &str,
        workers: u8,
    ) -> Result<Self, StateValidationError> {
        Self::new_at_with_workers(
            original_url,
            destination,
            suggested_filename,
            workers,
            TimestampMillis::now()?,
        )
    }

    /// Creates a queued task at an explicit bounded timestamp.
    ///
    /// # Errors
    ///
    /// Rejects an unsafe URL or destination.
    pub fn new_at(
        original_url: &str,
        destination: &Path,
        suggested_filename: &str,
        created_at: TimestampMillis,
    ) -> Result<Self, StateValidationError> {
        Self::new_at_with_workers(
            original_url,
            destination,
            suggested_filename,
            DEFAULT_TASK_WORKERS,
            created_at,
        )
    }

    fn new_at_with_workers(
        original_url: &str,
        destination: &Path,
        suggested_filename: &str,
        workers: u8,
        created_at: TimestampMillis,
    ) -> Result<Self, StateValidationError> {
        validate_worker_count(workers)?;
        let original_url = normalize_url(original_url)?;
        let destination = validate_new_destination(destination)?;
        let display_name = sanitize_filename(suggested_filename).as_str().to_owned();
        Ok(Self {
            task_id: TaskId::new(),
            revision: 1,
            state: TaskState::Queued,
            original_url,
            needs_session: false,
            resource: None,
            destination,
            display_name,
            workers,
            partial_path: None,
            final_path: None,
            completed_ranges: Vec::new(),
            created_at,
            updated_at: created_at,
        })
    }

    /// Whether recovery requires a session deliberately excluded from state.
    #[must_use]
    pub const fn needs_session(&self) -> bool {
        self.needs_session
    }

    pub(crate) fn require_session(&mut self) {
        self.needs_session = true;
    }

    /// Stable task ID.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    /// Monotonically increasing metadata revision.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Current lifecycle state.
    #[must_use]
    pub const fn state(&self) -> TaskState {
        self.state
    }

    /// Exact original URL required for retry/resume. This can be sensitive.
    #[must_use]
    pub fn original_url(&self) -> &str {
        &self.original_url
    }

    /// Accepted resource identity after probing.
    #[must_use]
    pub const fn resource(&self) -> Option<&ResourceIdentity> {
        self.resource.as_ref()
    }

    /// Canonical destination directory. This can be sensitive.
    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// Safe display/final filename.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Persisted fixed transfer worker count.
    #[must_use]
    pub const fn workers(&self) -> u8 {
        self.workers
    }

    /// Active or redundant partial path. This can be sensitive.
    #[must_use]
    pub fn partial_path(&self) -> Option<&Path> {
        self.partial_path.as_deref()
    }

    /// Published final path for completed work. This can be sensitive.
    #[must_use]
    pub fn final_path(&self) -> Option<&Path> {
        self.final_path.as_deref()
    }

    /// Reopens the recorded partial with only its durable completed coverage.
    ///
    /// # Errors
    ///
    /// Rejects tasks without a consistent resource/partial and propagates
    /// storage validation, file-lock, access, and length failures. An
    /// interrupted unknown-length stream reopens with no completed coverage so
    /// its next streaming writer restarts at byte zero.
    pub fn reopen_partial(&self) -> Result<PartialFile, TaskUpdateError> {
        let partial_path = self
            .partial_path
            .as_deref()
            .ok_or(StateValidationError::InvalidPartialPath)?;
        let resource = self
            .resource
            .as_ref()
            .ok_or(StateValidationError::InvalidResource)?;
        let partial = if let Some(expected_size) = resource.expected_size() {
            PartialFile::recover(
                partial_path,
                &self.display_name,
                expected_size,
                &self.completed_ranges,
            )?
        } else if resource.transfer_mode() == TransferMode::Single
            && self.completed_ranges.is_empty()
        {
            PartialFile::recover_streaming(partial_path, &self.display_name)?
        } else {
            return Err(StateValidationError::InvalidResource.into());
        };
        if partial.partial_path() != partial_path {
            return Err(StateValidationError::InvalidPartialPath.into());
        }
        Ok(partial)
    }

    /// Canonical completed coverage.
    #[must_use]
    pub fn completed_ranges(&self) -> &[FileRange] {
        &self.completed_ranges
    }

    /// Sum of completed range lengths.
    #[must_use]
    pub fn bytes_completed(&self) -> u64 {
        self.completed_ranges.iter().map(|range| range.len()).sum()
    }

    /// Creation timestamp.
    #[must_use]
    pub const fn created_at(&self) -> TimestampMillis {
        self.created_at
    }

    /// Last semantic mutation timestamp.
    #[must_use]
    pub const fn updated_at(&self) -> TimestampMillis {
        self.updated_at
    }

    /// Applies one explicit lifecycle transition.
    ///
    /// # Errors
    ///
    /// Rejects edges outside [`TaskState::allows`] and backwards timestamps.
    pub fn transition(
        &mut self,
        next: TaskState,
        updated_at: TimestampMillis,
    ) -> Result<(), StateValidationError> {
        if !self.state.allows(next) {
            return Err(StateValidationError::InvalidTransition);
        }
        let previous_state = self.state;
        let previous_revision = self.revision;
        let previous_updated_at = self.updated_at;
        self.bump_revision(updated_at)?;
        self.state = next;
        if let Err(error) = validate_task(self) {
            self.state = previous_state;
            self.revision = previous_revision;
            self.updated_at = previous_updated_at;
            return Err(error);
        }
        Ok(())
    }

    /// Stores the final URL, size, validators, and transfer mode accepted by a
    /// probe while the task is in `probing`.
    ///
    /// # Errors
    ///
    /// Rejects use in another state, a backwards timestamp, or a retry whose
    /// accepted identity conflicts with retained partial coverage.
    pub fn apply_resource(
        &mut self,
        resource: ResourceIdentity,
        updated_at: TimestampMillis,
    ) -> Result<(), StateValidationError> {
        if self.state != TaskState::Probing {
            return Err(StateValidationError::InconsistentState);
        }
        if self.partial_path.is_some()
            && self
                .resource
                .as_ref()
                .is_some_and(|known| known != &resource)
        {
            return Err(StateValidationError::InvalidResource);
        }
        self.bump_revision(updated_at)?;
        self.resource = Some(resource);
        Ok(())
    }

    /// Associates the unique storage partial with this probed task.
    ///
    /// # Errors
    ///
    /// Rejects a different destination, size, filename, lifecycle, or existing
    /// partial association.
    pub fn attach_partial(
        &mut self,
        partial: &PartialFile,
        updated_at: TimestampMillis,
    ) -> Result<(), StateValidationError> {
        let Some(resource) = self.resource.as_ref() else {
            return Err(StateValidationError::InconsistentState);
        };
        if self.state != TaskState::Probing
            || self.partial_path.is_some()
            || resource.expected_size != partial.expected_len()
            || partial.partial_path().parent() != Some(self.destination.as_path())
        {
            return Err(StateValidationError::InvalidPartialPath);
        }
        validate_confined_path(partial.partial_path(), &self.destination, PathKind::Partial)?;
        self.bump_revision(updated_at)?;
        partial
            .final_name()
            .as_str()
            .clone_into(&mut self.display_name);
        self.partial_path = Some(partial.partial_path().to_owned());
        Ok(())
    }

    /// Replaces persistent completed coverage from the authoritative storage
    /// object. An unchanged snapshot does not advance the task revision.
    ///
    /// # Errors
    ///
    /// Rejects a different partial/size, invalid state, invalid ranges, or a
    /// backwards timestamp.
    pub fn refresh_completed(
        &mut self,
        partial: &PartialFile,
        updated_at: TimestampMillis,
    ) -> Result<bool, TaskUpdateError> {
        if !matches!(
            self.state,
            TaskState::Downloading | TaskState::Paused | TaskState::Validating
        ) || self.partial_path.as_deref() != Some(partial.partial_path())
        {
            return Err(StateValidationError::InconsistentState.into());
        }
        let resource = self
            .resource
            .as_ref()
            .ok_or(StateValidationError::InvalidResource)?;
        let stored_size = resource.expected_size();
        let partial_size = partial.expected_len();
        let discovered_size = match (stored_size, partial_size) {
            (Some(expected), Some(actual)) if expected == actual => None,
            (None, None) if resource.transfer_mode() == TransferMode::Single => None,
            (None, Some(actual)) if resource.transfer_mode() == TransferMode::Single => {
                Some(actual)
            }
            _ => return Err(StateValidationError::InvalidResource.into()),
        };
        let completed = partial.durable_completed_ranges()?;
        validate_completed_ranges(&completed, partial_size)?;
        if completed == self.completed_ranges && discovered_size.is_none() {
            return Ok(false);
        }
        self.bump_revision(updated_at)?;
        if let Some(discovered_size) = discovered_size {
            self.resource
                .as_mut()
                .ok_or(StateValidationError::InvalidResource)?
                .expected_size = Some(discovered_size);
        }
        self.completed_ranges = completed;
        Ok(true)
    }

    /// Forgets a retained partial only after its explicit external deletion.
    ///
    /// # Errors
    ///
    /// Rejects non-cancelled/non-failed tasks, published final state, absent
    /// partial metadata, or backwards timestamps.
    pub fn record_partial_deletion(
        &mut self,
        updated_at: TimestampMillis,
    ) -> Result<(), StateValidationError> {
        if !matches!(self.state, TaskState::Cancelled | TaskState::Failed)
            || self.final_path.is_some()
            || self.partial_path.is_none()
        {
            return Err(StateValidationError::InconsistentState);
        }
        self.bump_revision(updated_at)?;
        self.partial_path = None;
        self.completed_ranges.clear();
        Ok(())
    }

    /// Records the collision-safe final pathname after storage publication.
    ///
    /// The first call retains the redundant partial link so both names are
    /// recoverable at the metadata boundary. After a durable critical
    /// checkpoint, the caller may invoke [`Promotion::cleanup_partial`] and
    /// call this method again to persist that cleanup.
    ///
    /// # Errors
    ///
    /// Rejects use outside `promoting`, inconsistent publication paths, an
    /// unconstrained final path, or a backwards timestamp.
    pub fn record_promotion(
        &mut self,
        promotion: &Promotion,
        updated_at: TimestampMillis,
    ) -> Result<(), StateValidationError> {
        if self.state != TaskState::Promoting {
            return Err(StateValidationError::InconsistentState);
        }
        let final_path = promotion.final_path();
        validate_confined_path(final_path, &self.destination, PathKind::Final)?;
        if self
            .final_path
            .as_deref()
            .is_some_and(|known| known != final_path)
        {
            return Err(StateValidationError::InconsistentState);
        }
        let retained_partial = promotion.partial_path();
        if retained_partial.is_some() && self.partial_path.as_deref() != retained_partial {
            return Err(StateValidationError::InvalidPartialPath);
        }

        self.bump_revision(updated_at)?;
        self.final_path = Some(final_path.to_owned());
        if retained_partial.is_none() {
            self.partial_path = None;
        }
        Ok(())
    }

    fn bump_revision(&mut self, updated_at: TimestampMillis) -> Result<(), StateValidationError> {
        if updated_at < self.updated_at {
            return Err(StateValidationError::InvalidTimestamp);
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(StateValidationError::InvalidRevision)?;
        self.updated_at = updated_at;
        Ok(())
    }
}

impl fmt::Debug for TaskMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TaskMetadata")
            .field("task_id", &self.task_id)
            .field("revision", &self.revision)
            .field("state", &self.state)
            .field("urls", &"<redacted>")
            .field("paths_and_filename", &"<redacted>")
            .field(
                "transfer_mode",
                &self
                    .resource
                    .as_ref()
                    .map_or(TransferMode::Pending, ResourceIdentity::transfer_mode),
            )
            .field(
                "expected_size",
                &self
                    .resource
                    .as_ref()
                    .and_then(ResourceIdentity::expected_size),
            )
            .field("bytes_completed", &self.bytes_completed())
            .field("workers", &self.workers)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish_non_exhaustive()
    }
}

/// Routine versus immediate metadata persistence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointUrgency {
    /// Progress-only update subject to the configured maximum frequency.
    Progress,
    /// Creation, state transition, shutdown, or other durability boundary.
    Critical,
}

/// Result of a checkpoint request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointOutcome {
    /// A complete state file was flushed and atomically installed.
    Written,
    /// The same revision was already durable.
    Unchanged,
    /// A progress revision remains dirty until the minimum interval elapses.
    Deferred,
}

/// Bounded routine checkpoint cadence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointPolicy {
    minimum_interval: Duration,
}

impl CheckpointPolicy {
    /// Creates a policy between 100 ms and 60 seconds, inclusive.
    ///
    /// # Errors
    ///
    /// Rejects a cadence that would permit unbounded write frequency or defer
    /// routine state excessively.
    pub fn new(minimum_interval: Duration) -> Result<Self, PersistenceError> {
        if minimum_interval < MIN_CHECKPOINT_INTERVAL || minimum_interval > MAX_CHECKPOINT_INTERVAL
        {
            return Err(PersistenceError::InvalidCheckpointPolicy);
        }
        Ok(Self { minimum_interval })
    }

    /// Configured minimum interval between routine writes for one task.
    #[must_use]
    pub const fn minimum_interval(self) -> Duration {
        self.minimum_interval
    }
}

impl Default for CheckpointPolicy {
    fn default() -> Self {
        Self {
            minimum_interval: DEFAULT_CHECKPOINT_INTERVAL,
        }
    }
}

/// Filesystem operation associated with persistence I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceOperation {
    /// Create or inspect the state root.
    OpenStore,
    /// Enumerate task records.
    ListState,
    /// Read one task record.
    ReadState,
    /// Write and flush a temporary record.
    WriteState,
    /// Atomically replace a task record.
    ReplaceState,
    /// Remove a state or explicitly selected partial file.
    Cleanup,
}

impl fmt::Display for PersistenceOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::OpenStore => "open state store",
            Self::ListState => "list task state",
            Self::ReadState => "read task state",
            Self::WriteState => "write task state",
            Self::ReplaceState => "replace task state",
            Self::Cleanup => "clean task state",
        };
        formatter.write_str(text)
    }
}

/// Persistence operation failure without sensitive paths or JSON text.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PersistenceError {
    /// The configured routine checkpoint cadence is unsafe.
    #[error("checkpoint policy is outside supported bounds")]
    InvalidCheckpointPolicy,
    /// Another helper process owns the state directory.
    #[error("task state store is locked by another helper")]
    StoreLocked,
    /// A reserved state-store entry has an unsafe type or changed identity.
    #[error("task state store layout is unsafe")]
    UnsafeStoreLayout,
    /// Task metadata failed semantic validation.
    #[error("task state is invalid: {0}")]
    InvalidTask(#[from] StateValidationError),
    /// Caller attempted to replace a newer durable revision.
    #[error("task state revision is stale")]
    StaleRevision,
    /// Existing durable state is malformed or incompatible and is preserved.
    #[error("existing task state must be recovered before replacement")]
    ExistingStateInvalid,
    /// Serialized state exceeded the per-task bound.
    #[error("task state exceeds the size limit")]
    StateTooLarge,
    /// JSON serialization failed without exposing its input.
    #[error("task state serialization failed")]
    Serialization,
    /// State directory contains more records than the recovery bound.
    #[error("task state file count exceeds the recovery limit")]
    TooManyTaskFiles,
    /// Cleanup applies only to terminal tasks.
    #[error("only terminal task state can be cleaned")]
    CleanupRequiresTerminal,
    /// Classified path-free filesystem failure.
    #[error("{operation} failed: {failure}")]
    Io {
        /// Failed persistence operation.
        operation: PersistenceOperation,
        /// Stable I/O classification shared with storage.
        failure: IoFailure,
        /// Optional operating-system error number.
        os_code: Option<i32>,
    },
}

/// Why one state file was excluded from recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadFailureReason {
    /// Filename is not a canonical task UUID.
    InvalidFilename,
    /// Entry is a link, directory, reparse point, or other unsafe type.
    UnsafeFileType,
    /// File exceeded [`MAX_STATE_BYTES`].
    TooLarge,
    /// JSON was malformed, duplicated a field, or had unknown/missing fields.
    Malformed,
    /// Top-level format marker was not recognized.
    UnknownFormat,
    /// A future or unsupported internal schema version was found.
    IncompatibleVersion {
        /// Version found without interpreting its task body.
        found: u64,
    },
    /// A valid legacy record could not be rewritten atomically.
    MigrationFailed,
    /// Filename and record task IDs conflict.
    TaskIdMismatch,
    /// Typed task data violated a semantic invariant.
    InvalidTask(StateValidationError),
    /// Persisted destination is absent, replaced, or no longer an ordinary directory.
    DestinationUnavailable,
    /// A resumable partial file is absent.
    PartialMissing,
    /// A partial path resolves to an unsafe file type.
    UnsafePartial,
    /// Partial length conflicts with expected resource size.
    PartialLengthMismatch {
        /// Expected size.
        expected: u64,
        /// Observed size.
        actual: u64,
    },
    /// A recorded published final file is absent.
    FinalMissing,
    /// A recorded final path resolves to an unsafe file type.
    UnsafeFinal,
    /// Final length conflicts with expected resource size.
    FinalLengthMismatch {
        /// Expected size.
        expected: u64,
        /// Observed size.
        actual: u64,
    },
    /// Persisted partial and final paths do not identify the same file.
    PublicationIdentityMismatch,
    /// Final-file or hard-link identity inspection failed.
    FinalInspectionFailed(IoFailure),
    /// Reading this bounded record failed.
    ReadFailed(IoFailure),
}

impl fmt::Display for LoadFailureReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFilename => formatter.write_str("invalid state filename"),
            Self::UnsafeFileType => formatter.write_str("unsafe state file type"),
            Self::TooLarge => formatter.write_str("state file exceeds size limit"),
            Self::Malformed => formatter.write_str("state file is malformed"),
            Self::UnknownFormat => formatter.write_str("state format marker is unknown"),
            Self::IncompatibleVersion { found } => {
                write!(formatter, "state version {found} is incompatible")
            }
            Self::MigrationFailed => formatter.write_str("legacy state migration failed"),
            Self::TaskIdMismatch => formatter.write_str("state filename and task ID disagree"),
            Self::InvalidTask(error) => write!(formatter, "invalid task state: {error}"),
            Self::DestinationUnavailable => {
                formatter.write_str("task destination is unavailable or unsafe")
            }
            Self::PartialMissing => formatter.write_str("recoverable partial file is missing"),
            Self::UnsafePartial => formatter.write_str("recoverable partial file type is unsafe"),
            Self::PartialLengthMismatch { expected, actual } => write!(
                formatter,
                "partial length mismatch: expected {expected}, found {actual}"
            ),
            Self::FinalMissing => formatter.write_str("published final file is missing"),
            Self::UnsafeFinal => formatter.write_str("published final file type is unsafe"),
            Self::FinalLengthMismatch { expected, actual } => write!(
                formatter,
                "final length mismatch: expected {expected}, found {actual}"
            ),
            Self::PublicationIdentityMismatch => {
                formatter.write_str("partial and final publication identities disagree")
            }
            Self::FinalInspectionFailed(failure) => {
                write!(formatter, "final file inspection failed: {failure}")
            }
            Self::ReadFailed(failure) => write!(formatter, "state read failed: {failure}"),
        }
    }
}

/// One excluded record with no sensitive path or content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadFailure {
    task_id: Option<TaskId>,
    reason: LoadFailureReason,
}

impl LoadFailure {
    /// Task ID recovered safely from the filename, when available.
    #[must_use]
    pub const fn task_id(&self) -> Option<TaskId> {
        self.task_id
    }

    /// Stable reason this record cannot be resumed.
    #[must_use]
    pub const fn reason(&self) -> &LoadFailureReason {
        &self.reason
    }
}

/// Bounded recovery result. Invalid records remain on disk for diagnosis or
/// explicit cleanup but are never returned as resumable tasks.
#[derive(Debug, Default)]
pub struct LoadReport {
    tasks: Vec<TaskMetadata>,
    failures: Vec<LoadFailure>,
}

impl LoadReport {
    /// Validated recoverable/history tasks.
    #[must_use]
    pub fn tasks(&self) -> &[TaskMetadata] {
        &self.tasks
    }

    /// Records excluded with safe diagnoses.
    #[must_use]
    pub fn failures(&self) -> &[LoadFailure] {
        &self.failures
    }

    /// Takes ownership of validated tasks.
    #[must_use]
    pub fn into_tasks(self) -> Vec<TaskMetadata> {
        self.tasks
    }
}

/// Explicit treatment of a terminal task's partial file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialCleanup {
    /// Keep a retained partial and its metadata for later user action.
    Keep,
    /// Delete a confined ordinary partial, then remove task metadata.
    Delete,
}

/// Result of explicit terminal cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupOutcome {
    /// A retained partial prevented metadata removal by explicit policy.
    Retained,
    /// Task metadata was removed; any requested partial deletion completed.
    Removed,
}

/// Exclusive, user-scoped task metadata store.
pub struct TaskStore {
    root: PathBuf,
    tasks_directory: PathBuf,
    _lock_file: File,
    checkpoint_policy: CheckpointPolicy,
    checkpoints: Mutex<HashMap<TaskId, SavedCheckpoint>>,
    io_gate: Mutex<()>,
}

#[derive(Debug, Clone, Copy)]
struct SavedCheckpoint {
    revision: u64,
    written_at: Instant,
}

impl TaskStore {
    /// Opens or creates a state root using the default checkpoint cadence.
    ///
    /// # Errors
    ///
    /// Rejects unsafe directories, I/O failures, or another helper's lock.
    pub fn open(root: &Path) -> Result<Self, PersistenceError> {
        Self::open_with_policy(root, CheckpointPolicy::default())
    }

    /// Opens or creates a state root with an explicit bounded cadence.
    ///
    /// # Errors
    ///
    /// Rejects unsafe directories, I/O failures, or another helper's lock.
    pub fn open_with_policy(
        root: &Path,
        checkpoint_policy: CheckpointPolicy,
    ) -> Result<Self, PersistenceError> {
        validate_path_syntax(root).map_err(|()| StateValidationError::InvalidDestination)?;
        fs::create_dir_all(root)
            .map_err(|error| map_persistence_io(PersistenceOperation::OpenStore, &error))?;
        validate_ordinary_directory(root)?;
        let root = fs::canonicalize(root)
            .map_err(|error| map_persistence_io(PersistenceOperation::OpenStore, &error))?;
        let tasks_directory = root.join(TASK_DIRECTORY_NAME);
        fs::create_dir_all(&tasks_directory)
            .map_err(|error| map_persistence_io(PersistenceOperation::OpenStore, &error))?;
        validate_ordinary_directory(&tasks_directory)?;

        let lock_path = root.join(STORE_LOCK_NAME);
        match fs::symlink_metadata(&lock_path) {
            Ok(metadata)
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || is_reparse_point(&metadata) =>
            {
                return Err(PersistenceError::UnsafeStoreLayout);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(map_persistence_io(PersistenceOperation::OpenStore, &error));
            }
        }
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| map_persistence_io(PersistenceOperation::OpenStore, &error))?;
        let lock_metadata = fs::symlink_metadata(&lock_path)
            .map_err(|error| map_persistence_io(PersistenceOperation::OpenStore, &error))?;
        if !lock_metadata.is_file()
            || lock_metadata.file_type().is_symlink()
            || is_reparse_point(&lock_metadata)
            || !opened_file_matches_path(&lock_file, &lock_path)
                .map_err(|error| map_persistence_io(PersistenceOperation::OpenStore, &error))?
        {
            return Err(PersistenceError::UnsafeStoreLayout);
        }
        if let Err(error) = File::try_lock(&lock_file) {
            match error {
                TryLockError::WouldBlock => return Err(PersistenceError::StoreLocked),
                TryLockError::Error(error) => {
                    return Err(map_persistence_io(PersistenceOperation::OpenStore, &error));
                }
            }
        }

        let store = Self {
            root,
            tasks_directory,
            _lock_file: lock_file,
            checkpoint_policy,
            checkpoints: Mutex::new(HashMap::new()),
            io_gate: Mutex::new(()),
        };
        store.cleanup_stale_temps()?;
        Ok(store)
    }

    /// Canonical application state root. This is intended for trusted local
    /// management code, not routine logs.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Persists a complete task snapshot with routine coalescing or an
    /// immediate durability boundary.
    ///
    /// # Errors
    ///
    /// Returns a path-free validation, serialization, or I/O error. A failed
    /// write leaves the prior complete record authoritative.
    pub fn checkpoint(
        &self,
        task: &TaskMetadata,
        urgency: CheckpointUrgency,
    ) -> Result<CheckpointOutcome, PersistenceError> {
        validate_task(task)?;
        let _io_guard = lock(&self.io_gate);
        let mut checkpoints = lock(&self.checkpoints);
        let now = Instant::now();
        if let Some(previous) = checkpoints.get(&task.task_id) {
            if previous.revision > task.revision {
                return Err(PersistenceError::StaleRevision);
            }
            if previous.revision == task.revision {
                return Ok(CheckpointOutcome::Unchanged);
            }
            if urgency == CheckpointUrgency::Progress
                && now.duration_since(previous.written_at) < self.checkpoint_policy.minimum_interval
            {
                return Ok(CheckpointOutcome::Deferred);
            }
        } else if let Some(durable_revision) = self.durable_revision(task.task_id)? {
            if durable_revision > task.revision {
                return Err(PersistenceError::StaleRevision);
            }
            if durable_revision == task.revision {
                checkpoints.insert(
                    task.task_id,
                    SavedCheckpoint {
                        revision: durable_revision,
                        written_at: now,
                    },
                );
                return Ok(CheckpointOutcome::Unchanged);
            }
        }

        let bytes = serialize_task(task)?;
        self.write_atomic(task.task_id, &bytes)?;
        checkpoints.insert(
            task.task_id,
            SavedCheckpoint {
                revision: task.revision,
                written_at: now,
            },
        );
        Ok(CheckpointOutcome::Written)
    }

    /// Loads every bounded task record independently.
    ///
    /// Corrupt or incompatible records are reported and left untouched, while
    /// valid records continue to load. Unsafe or inconsistent tasks are never
    /// returned for resume.
    ///
    /// # Errors
    ///
    /// Returns only store-level enumeration failures or an excessive file
    /// count; per-record failures are captured in [`LoadReport`].
    pub fn load_all(&self) -> Result<LoadReport, PersistenceError> {
        let _io_guard = lock(&self.io_gate);
        let mut entries = fs::read_dir(&self.tasks_directory)
            .map_err(|error| map_persistence_io(PersistenceOperation::ListState, &error))?;
        let mut report = LoadReport::default();
        let mut task_file_count = 0usize;
        let mut directory_entry_count = 0usize;

        for entry_result in entries.by_ref() {
            directory_entry_count += 1;
            if directory_entry_count > MAX_DIRECTORY_ENTRIES {
                return Err(PersistenceError::TooManyTaskFiles);
            }
            let entry = entry_result
                .map_err(|error| map_persistence_io(PersistenceOperation::ListState, &error))?;
            let filename = entry.file_name();
            let Some(filename) = filename.to_str() else {
                continue;
            };
            let Some(id_text) = filename.strip_suffix(TASK_FILE_SUFFIX) else {
                continue;
            };
            task_file_count += 1;
            if task_file_count > MAX_TASK_FILES {
                return Err(PersistenceError::TooManyTaskFiles);
            }
            let task_id = TaskId::parse(id_text).ok();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    report.failures.push(LoadFailure {
                        task_id,
                        reason: LoadFailureReason::ReadFailed(classify_io(&error)),
                    });
                    continue;
                }
            };
            if !file_type.is_file() || file_type.is_symlink() {
                report.failures.push(LoadFailure {
                    task_id,
                    reason: LoadFailureReason::UnsafeFileType,
                });
                continue;
            }
            let Some(filename_id) = task_id else {
                report.failures.push(LoadFailure {
                    task_id: None,
                    reason: LoadFailureReason::InvalidFilename,
                });
                continue;
            };

            match load_task_file(&entry.path(), filename_id) {
                Ok(loaded) => {
                    if loaded.migrated {
                        let migration = serialize_task(&loaded.task)
                            .and_then(|bytes| self.write_atomic(loaded.task.task_id, &bytes));
                        if migration.is_err() {
                            report.failures.push(LoadFailure {
                                task_id: Some(filename_id),
                                reason: LoadFailureReason::MigrationFailed,
                            });
                            continue;
                        }
                    }
                    report.tasks.push(loaded.task);
                }
                Err(reason) => report.failures.push(LoadFailure {
                    task_id: Some(filename_id),
                    reason,
                }),
            }
        }

        report.tasks.sort_by_key(TaskMetadata::task_id);
        report.failures.sort_by_key(|failure| failure.task_id);
        let now = Instant::now();
        let mut checkpoints = lock(&self.checkpoints);
        checkpoints.clear();
        for task in &report.tasks {
            checkpoints.insert(
                task.task_id,
                SavedCheckpoint {
                    revision: task.revision,
                    written_at: now,
                },
            );
        }
        Ok(report)
    }

    /// Removes stale create-new temporary state files left before atomic
    /// replacement. Unknown files and links are never followed or deleted.
    ///
    /// # Errors
    ///
    /// Returns a classified enumeration or deletion error.
    pub fn cleanup_stale_temps(&self) -> Result<usize, PersistenceError> {
        let _io_guard = lock(&self.io_gate);
        let entries = fs::read_dir(&self.tasks_directory)
            .map_err(|error| map_persistence_io(PersistenceOperation::ListState, &error))?;
        let mut removed = 0usize;
        let mut directory_entry_count = 0usize;
        for entry_result in entries {
            directory_entry_count += 1;
            if directory_entry_count > MAX_DIRECTORY_ENTRIES {
                return Err(PersistenceError::TooManyTaskFiles);
            }
            let entry = entry_result
                .map_err(|error| map_persistence_io(PersistenceOperation::ListState, &error))?;
            let filename = entry.file_name();
            let Some(filename) = filename.to_str() else {
                continue;
            };
            let Some((task_id, suffix)) = filename.split_once(TEMP_FILE_MARKER) else {
                continue;
            };
            if TaskId::parse(task_id).is_err() || !is_temporary_suffix(suffix) {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|error| map_persistence_io(PersistenceOperation::Cleanup, &error))?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || is_reparse_point(&metadata)
            {
                continue;
            }
            fs::remove_file(entry.path())
                .map_err(|error| map_persistence_io(PersistenceOperation::Cleanup, &error))?;
            removed += 1;
        }
        Ok(removed)
    }

    /// Deletes and forgets a failed/cancelled task's retained partial while
    /// preserving its terminal history record.
    ///
    /// Deletion precedes metadata replacement. If interruption occurs between
    /// those operations, recovery accepts the old terminal record with a
    /// missing partial and a later call can safely finish the metadata update.
    ///
    /// # Errors
    ///
    /// Rejects other lifecycle states or unsafe partial entries and propagates
    /// deletion/checkpoint failures.
    pub fn discard_terminal_partial(
        &self,
        task: &mut TaskMetadata,
        updated_at: TimestampMillis,
    ) -> Result<bool, PersistenceError> {
        if !matches!(task.state, TaskState::Cancelled | TaskState::Failed) {
            return Err(PersistenceError::CleanupRequiresTerminal);
        }
        validate_task(task)?;
        let Some(partial_path) = task.partial_path.as_deref() else {
            return Ok(false);
        };
        {
            let _io_guard = lock(&self.io_gate);
            validate_confined_path(partial_path, &task.destination, PathKind::Partial)?;
            match fs::symlink_metadata(partial_path) {
                Ok(metadata) => {
                    if !metadata.is_file()
                        || metadata.file_type().is_symlink()
                        || is_reparse_point(&metadata)
                    {
                        return Err(StateValidationError::InvalidPartialPath.into());
                    }
                    fs::remove_file(partial_path).map_err(|error| {
                        map_persistence_io(PersistenceOperation::Cleanup, &error)
                    })?;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(map_persistence_io(PersistenceOperation::Cleanup, &error));
                }
            }
        }
        let retained_metadata = task.clone();
        task.record_partial_deletion(updated_at)?;
        if let Err(error) = self.checkpoint(task, CheckpointUrgency::Critical) {
            *task = retained_metadata;
            return Err(error);
        }
        Ok(true)
    }

    /// Applies explicit terminal cleanup without ever deleting a final file.
    ///
    /// Failed/cancelled/completed tasks with a retained partial remain in the
    /// store when `Keep` is selected. `Delete` removes only a validated partial
    /// in the task destination before deleting metadata. Completed final files
    /// are never touched.
    ///
    /// # Errors
    ///
    /// Rejects non-terminal tasks and surfaces partial/state deletion failures.
    pub fn cleanup_terminal(
        &self,
        task: &TaskMetadata,
        partial_cleanup: PartialCleanup,
    ) -> Result<CleanupOutcome, PersistenceError> {
        if !task.state.is_terminal() {
            return Err(PersistenceError::CleanupRequiresTerminal);
        }
        validate_task(task)?;
        let _io_guard = lock(&self.io_gate);
        if task.partial_path.is_some() && partial_cleanup == PartialCleanup::Keep {
            return Ok(CleanupOutcome::Retained);
        }
        if let Some(partial_path) = task.partial_path.as_deref() {
            validate_confined_path(partial_path, &task.destination, PathKind::Partial)?;
            match fs::symlink_metadata(partial_path) {
                Ok(metadata) => {
                    if !metadata.is_file()
                        || metadata.file_type().is_symlink()
                        || is_reparse_point(&metadata)
                    {
                        return Err(StateValidationError::InvalidPartialPath.into());
                    }
                    if task.state == TaskState::Completed {
                        let final_path = task
                            .final_path
                            .as_deref()
                            .ok_or(StateValidationError::InvalidFinalPath)?;
                        if !same_file::is_same_file(partial_path, final_path).map_err(|error| {
                            map_persistence_io(PersistenceOperation::Cleanup, &error)
                        })? {
                            return Err(StateValidationError::InvalidPartialPath.into());
                        }
                    }
                    fs::remove_file(partial_path).map_err(|error| {
                        map_persistence_io(PersistenceOperation::Cleanup, &error)
                    })?;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(map_persistence_io(PersistenceOperation::Cleanup, &error));
                }
            }
        }

        let state_path = self.task_path(task.task_id);
        match fs::remove_file(state_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(map_persistence_io(PersistenceOperation::Cleanup, &error));
            }
        }
        lock(&self.checkpoints).remove(&task.task_id);
        Ok(CleanupOutcome::Removed)
    }

    fn durable_revision(&self, task_id: TaskId) -> Result<Option<u64>, PersistenceError> {
        let path = self.task_path(task_id);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(map_persistence_io(PersistenceOperation::ReadState, &error));
            }
        };
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || is_reparse_point(&metadata)
            || metadata.len() > u64::try_from(MAX_STATE_BYTES).unwrap_or(u64::MAX)
        {
            return Err(PersistenceError::ExistingStateInvalid);
        }
        let file = File::open(&path)
            .map_err(|error| map_persistence_io(PersistenceOperation::ReadState, &error))?;
        if !opened_file_matches_path(&file, &path)
            .map_err(|error| map_persistence_io(PersistenceOperation::ReadState, &error))?
        {
            return Err(PersistenceError::ExistingStateInvalid);
        }
        let mut bytes =
            Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(MAX_STATE_BYTES));
        file.take(u64::try_from(MAX_STATE_BYTES + 1).unwrap_or(u64::MAX))
            .read_to_end(&mut bytes)
            .map_err(|error| map_persistence_io(PersistenceOperation::ReadState, &error))?;
        if bytes.len() > MAX_STATE_BYTES {
            return Err(PersistenceError::ExistingStateInvalid);
        }
        let envelope: PersistedEnvelope =
            serde_json::from_slice(&bytes).map_err(|_| PersistenceError::ExistingStateInvalid)?;
        if envelope.format != STATE_FORMAT_NAME || envelope.version != STATE_FORMAT_VERSION {
            return Err(PersistenceError::ExistingStateInvalid);
        }
        let durable_task = task_from_persisted(envelope.task)
            .map_err(|_| PersistenceError::ExistingStateInvalid)?;
        if durable_task.task_id != task_id {
            return Err(PersistenceError::ExistingStateInvalid);
        }
        Ok(Some(durable_task.revision))
    }

    fn write_atomic(&self, task_id: TaskId, bytes: &[u8]) -> Result<(), PersistenceError> {
        let target = self.task_path(task_id);
        for attempt in 0..TEMP_CREATE_ATTEMPTS {
            let temporary = self.temporary_path(task_id, attempt);
            let mut file = match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(map_persistence_io(PersistenceOperation::WriteState, &error));
                }
            };
            let write_result = file.write_all(bytes).and_then(|()| file.sync_all());
            if let Err(error) = write_result {
                drop(file);
                return Err(map_persistence_io(PersistenceOperation::WriteState, &error));
            }
            if !opened_file_matches_path(&file, &temporary)
                .map_err(|error| map_persistence_io(PersistenceOperation::WriteState, &error))?
            {
                return Err(PersistenceError::UnsafeStoreLayout);
            }
            drop(file);
            if let Err(error) = fs::rename(&temporary, &target) {
                let _ = fs::remove_file(&temporary);
                return Err(map_persistence_io(
                    PersistenceOperation::ReplaceState,
                    &error,
                ));
            }
            return Ok(());
        }
        Err(PersistenceError::Io {
            operation: PersistenceOperation::WriteState,
            failure: IoFailure::AlreadyExists,
            os_code: None,
        })
    }

    fn task_path(&self, task_id: TaskId) -> PathBuf {
        self.tasks_directory
            .join(format!("{task_id}{TASK_FILE_SUFFIX}"))
    }

    fn temporary_path(&self, task_id: TaskId, attempt: u64) -> PathBuf {
        let sequence = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        self.tasks_directory.join(format!(
            "{task_id}{TEMP_FILE_MARKER}{:x}-{sequence:016x}-{attempt:02x}",
            std::process::id()
        ))
    }
}

impl fmt::Debug for TaskStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TaskStore")
            .field("root", &"<redacted>")
            .field("checkpoint_policy", &self.checkpoint_policy)
            .finish_non_exhaustive()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedEnvelope {
    format: String,
    version: u64,
    task: PersistedTask,
}

#[derive(Debug, Deserialize)]
struct VersionProbe {
    format: String,
    version: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedTask {
    needs_session: bool,
    task_id: String,
    revision: u64,
    state: TaskState,
    original_url: String,
    final_url: Option<String>,
    expected_size: Option<u64>,
    validators: PersistedValidators,
    transfer_mode: TransferMode,
    destination: String,
    display_name: String,
    workers: u8,
    partial_path: Option<String>,
    final_path: Option<String>,
    completed_ranges: Vec<PersistedRange>,
    created_at_ms: u64,
    updated_at_ms: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedEnvelopeV2 {
    format: String,
    version: u64,
    task: PersistedTaskV2,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedTaskV2 {
    task_id: String,
    revision: u64,
    state: TaskState,
    original_url: String,
    final_url: Option<String>,
    expected_size: Option<u64>,
    validators: PersistedValidators,
    transfer_mode: TransferMode,
    destination: String,
    display_name: String,
    workers: u8,
    partial_path: Option<String>,
    final_path: Option<String>,
    completed_ranges: Vec<PersistedRange>,
    created_at_ms: u64,
    updated_at_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedEnvelopeV1 {
    format: String,
    version: u64,
    task: PersistedTaskV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedTaskV1 {
    task_id: String,
    revision: u64,
    state: TaskState,
    original_url: String,
    final_url: Option<String>,
    expected_size: Option<u64>,
    validators: PersistedValidators,
    transfer_mode: TransferMode,
    destination: String,
    display_name: String,
    partial_path: Option<String>,
    final_path: Option<String>,
    completed_ranges: Vec<PersistedRange>,
    created_at_ms: u64,
    updated_at_ms: u64,
}

impl PersistedTaskV2 {
    fn migrate(self) -> PersistedTask {
        PersistedTask {
            needs_session: false,
            task_id: self.task_id,
            revision: self.revision,
            state: self.state,
            original_url: self.original_url,
            final_url: self.final_url,
            expected_size: self.expected_size,
            validators: self.validators,
            transfer_mode: self.transfer_mode,
            destination: self.destination,
            display_name: self.display_name,
            workers: self.workers,
            partial_path: self.partial_path,
            final_path: self.final_path,
            completed_ranges: self.completed_ranges,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

impl PersistedTaskV1 {
    fn migrate(self) -> PersistedTask {
        PersistedTask {
            task_id: self.task_id,
            revision: self.revision,
            state: self.state,
            original_url: self.original_url,
            needs_session: false,
            final_url: self.final_url,
            expected_size: self.expected_size,
            validators: self.validators,
            transfer_mode: self.transfer_mode,
            destination: self.destination,
            display_name: self.display_name,
            workers: DEFAULT_TASK_WORKERS,
            partial_path: self.partial_path,
            final_path: self.final_path,
            completed_ranges: self.completed_ranges,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedValidators {
    etag: Option<String>,
    last_modified: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedRange {
    start: u64,
    end: u64,
}

fn serialize_task(task: &TaskMetadata) -> Result<Vec<u8>, PersistenceError> {
    let persisted = PersistedEnvelope {
        format: STATE_FORMAT_NAME.to_owned(),
        version: STATE_FORMAT_VERSION,
        task: PersistedTask::from_task(task)?,
    };
    let bytes = serde_json::to_vec(&persisted).map_err(|_| PersistenceError::Serialization)?;
    if bytes.len() > MAX_STATE_BYTES {
        return Err(PersistenceError::StateTooLarge);
    }
    Ok(bytes)
}

impl PersistedTask {
    fn from_task(task: &TaskMetadata) -> Result<Self, PersistenceError> {
        let resource = task.resource.as_ref();
        Ok(Self {
            task_id: task.task_id.to_string(),
            revision: task.revision,
            state: task.state,
            original_url: task.original_url.clone(),
            needs_session: task.needs_session,
            final_url: resource.map(|identity| identity.final_url.clone()),
            expected_size: resource.and_then(ResourceIdentity::expected_size),
            validators: PersistedValidators::from_validators(
                resource.map_or(&Validators::default(), ResourceIdentity::validators),
            ),
            transfer_mode: resource.map_or(TransferMode::Pending, ResourceIdentity::transfer_mode),
            destination: path_to_string(&task.destination)?.to_owned(),
            display_name: task.display_name.clone(),
            workers: task.workers,
            partial_path: task
                .partial_path
                .as_deref()
                .map(path_to_string)
                .transpose()?
                .map(str::to_owned),
            final_path: task
                .final_path
                .as_deref()
                .map(path_to_string)
                .transpose()?
                .map(str::to_owned),
            completed_ranges: task
                .completed_ranges
                .iter()
                .map(|range| PersistedRange {
                    start: range.start(),
                    end: range.end(),
                })
                .collect(),
            created_at_ms: task.created_at.get(),
            updated_at_ms: task.updated_at.get(),
        })
    }
}

impl PersistedValidators {
    fn from_validators(validators: &Validators) -> Self {
        Self {
            etag: validators.etag.as_ref().map(format_entity_tag),
            last_modified: validators.last_modified.clone(),
        }
    }

    fn into_validators(self) -> Result<Validators, StateValidationError> {
        let etag = self
            .etag
            .as_deref()
            .map(EntityTag::parse)
            .transpose()
            .map_err(|_| StateValidationError::InvalidValidator)?;
        let validators = Validators {
            etag,
            last_modified: self.last_modified,
        };
        validate_validators(&validators)?;
        Ok(validators)
    }
}

struct LoadedTaskFile {
    task: TaskMetadata,
    migrated: bool,
}

fn load_task_file(path: &Path, filename_id: TaskId) -> Result<LoadedTaskFile, LoadFailureReason> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| LoadFailureReason::ReadFailed(classify_io(&error)))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
        return Err(LoadFailureReason::UnsafeFileType);
    }
    if metadata.len() > u64::try_from(MAX_STATE_BYTES).unwrap_or(u64::MAX) {
        return Err(LoadFailureReason::TooLarge);
    }

    let file =
        File::open(path).map_err(|error| LoadFailureReason::ReadFailed(classify_io(&error)))?;
    if !opened_file_matches_path(&file, path)
        .map_err(|error| LoadFailureReason::ReadFailed(classify_io(&error)))?
    {
        return Err(LoadFailureReason::UnsafeFileType);
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(MAX_STATE_BYTES));
    file.take(u64::try_from(MAX_STATE_BYTES + 1).unwrap_or(u64::MAX))
        .read_to_end(&mut bytes)
        .map_err(|error| LoadFailureReason::ReadFailed(classify_io(&error)))?;
    if bytes.len() > MAX_STATE_BYTES {
        return Err(LoadFailureReason::TooLarge);
    }

    let version: VersionProbe =
        serde_json::from_slice(&bytes).map_err(|_| LoadFailureReason::Malformed)?;
    if version.format != STATE_FORMAT_NAME {
        return Err(LoadFailureReason::UnknownFormat);
    }
    let (raw, migrated) = match version.version {
        STATE_FORMAT_VERSION => {
            let envelope: PersistedEnvelope =
                serde_json::from_slice(&bytes).map_err(|_| LoadFailureReason::Malformed)?;
            (envelope.task, false)
        }
        2 => {
            let envelope: PersistedEnvelopeV2 =
                serde_json::from_slice(&bytes).map_err(|_| LoadFailureReason::Malformed)?;
            (envelope.task.migrate(), true)
        }
        1 => {
            let envelope: PersistedEnvelopeV1 =
                serde_json::from_slice(&bytes).map_err(|_| LoadFailureReason::Malformed)?;
            if envelope.format != STATE_FORMAT_NAME || envelope.version != 1 {
                return Err(LoadFailureReason::Malformed);
            }
            (envelope.task.migrate(), true)
        }
        found => return Err(LoadFailureReason::IncompatibleVersion { found }),
    };
    let task = task_from_persisted(raw).map_err(LoadFailureReason::InvalidTask)?;
    if task.task_id != filename_id {
        return Err(LoadFailureReason::TaskIdMismatch);
    }
    validate_recovery_file(&task)?;
    Ok(LoadedTaskFile { task, migrated })
}

fn task_from_persisted(raw: PersistedTask) -> Result<TaskMetadata, StateValidationError> {
    let task_id = TaskId::parse(&raw.task_id)?;
    if raw.revision == 0 {
        return Err(StateValidationError::InvalidRevision);
    }
    let created_at = TimestampMillis::new(raw.created_at_ms)?;
    let updated_at = TimestampMillis::new(raw.updated_at_ms)?;
    if updated_at < created_at {
        return Err(StateValidationError::InvalidTimestamp);
    }
    let original_url = require_canonical_url(&raw.original_url)?;
    let destination = validate_persisted_directory(&raw.destination)?;
    if sanitize_filename(&raw.display_name).as_str() != raw.display_name {
        return Err(StateValidationError::InvalidFilename);
    }
    let validators = raw.validators.into_validators()?;
    let resource = match (raw.transfer_mode, raw.final_url) {
        (TransferMode::Pending, None) => {
            if raw.expected_size.is_some()
                || validators.etag.is_some()
                || validators.last_modified.is_some()
            {
                return Err(StateValidationError::InvalidResource);
            }
            None
        }
        (TransferMode::Pending, Some(_)) | (_, None) => {
            return Err(StateValidationError::InvalidResource);
        }
        (mode, Some(final_url)) => Some(ResourceIdentity::new(
            &require_canonical_url(&final_url)?,
            raw.expected_size,
            validators,
            mode,
        )?),
    };
    let partial_path = raw.partial_path.map(PathBuf::from);
    if let Some(path) = partial_path.as_deref() {
        validate_confined_path(path, &destination, PathKind::Partial)?;
    }
    let final_path = raw.final_path.map(PathBuf::from);
    if let Some(path) = final_path.as_deref() {
        validate_confined_path(path, &destination, PathKind::Final)?;
    }
    let completed_ranges = raw
        .completed_ranges
        .into_iter()
        .map(|range| {
            FileRange::new(range.start, range.end)
                .map_err(|_| StateValidationError::InvalidCompletedRanges)
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_completed_ranges(
        &completed_ranges,
        resource.as_ref().and_then(ResourceIdentity::expected_size),
    )?;

    let task = TaskMetadata {
        task_id,
        needs_session: raw.needs_session,
        revision: raw.revision,
        state: raw.state,
        original_url,
        resource,
        destination,
        display_name: raw.display_name,
        workers: raw.workers,
        partial_path,
        final_path,
        completed_ranges,
        created_at,
        updated_at,
    };
    validate_task(&task)?;
    Ok(task)
}

fn validate_task(task: &TaskMetadata) -> Result<(), StateValidationError> {
    if task.revision == 0 || task.updated_at < task.created_at {
        return Err(StateValidationError::InvalidRevision);
    }
    validate_worker_count(task.workers)?;
    require_canonical_url(&task.original_url)?;
    validate_path_syntax(&task.destination)
        .map_err(|()| StateValidationError::InvalidDestination)?;
    if sanitize_filename(&task.display_name).as_str() != task.display_name {
        return Err(StateValidationError::InvalidFilename);
    }
    if let Some(resource) = task.resource.as_ref() {
        ResourceIdentity::new(
            &resource.final_url,
            resource.expected_size,
            resource.validators.clone(),
            resource.transfer_mode,
        )?;
    }
    if let Some(path) = task.partial_path.as_deref() {
        validate_confined_path(path, &task.destination, PathKind::Partial)?;
    }
    if let Some(path) = task.final_path.as_deref() {
        validate_confined_path(path, &task.destination, PathKind::Final)?;
    }
    validate_completed_ranges(
        &task.completed_ranges,
        task.resource
            .as_ref()
            .and_then(ResourceIdentity::expected_size),
    )?;

    if task.resource.is_none()
        && (task.partial_path.is_some()
            || task.final_path.is_some()
            || !task.completed_ranges.is_empty())
    {
        return Err(StateValidationError::InconsistentState);
    }
    if task.resource.as_ref().is_some_and(|resource| {
        resource.expected_size.is_none()
            && (resource.transfer_mode != TransferMode::Single
                || task.final_path.is_some()
                || !task.completed_ranges.is_empty())
    }) {
        return Err(StateValidationError::InconsistentState);
    }
    if matches!(task.state, TaskState::Downloading | TaskState::Paused)
        && (task.resource.is_none() || task.partial_path.is_none())
    {
        return Err(StateValidationError::InconsistentState);
    }
    if matches!(
        task.state,
        TaskState::Validating | TaskState::Promoting | TaskState::Completed
    ) {
        let Some(resource_size) = task
            .resource
            .as_ref()
            .and_then(ResourceIdentity::expected_size)
        else {
            return Err(StateValidationError::InconsistentState);
        };
        if !has_exact_coverage(&task.completed_ranges, resource_size) {
            return Err(StateValidationError::InconsistentState);
        }
        if task.state == TaskState::Validating && task.partial_path.is_none() {
            return Err(StateValidationError::InconsistentState);
        }
        if task.state == TaskState::Promoting
            && task.partial_path.is_none()
            && task.final_path.is_none()
        {
            return Err(StateValidationError::InconsistentState);
        }
        if task.state == TaskState::Completed && task.final_path.is_none() {
            return Err(StateValidationError::InconsistentState);
        }
    }
    if task.final_path.is_some()
        && !matches!(task.state, TaskState::Promoting | TaskState::Completed)
    {
        return Err(StateValidationError::InconsistentState);
    }
    Ok(())
}

fn validate_recovery_file(task: &TaskMetadata) -> Result<(), LoadFailureReason> {
    let destination_metadata = fs::symlink_metadata(&task.destination)
        .map_err(|_| LoadFailureReason::DestinationUnavailable)?;
    if !destination_metadata.is_dir()
        || destination_metadata.file_type().is_symlink()
        || is_reparse_point(&destination_metadata)
        || fs::canonicalize(&task.destination).ok().as_deref() != Some(task.destination.as_path())
    {
        return Err(LoadFailureReason::DestinationUnavailable);
    }

    let expected_size = task
        .resource
        .as_ref()
        .and_then(ResourceIdentity::expected_size);
    let mut partial_exists = false;
    if let Some(partial_path) = task.partial_path.as_deref() {
        match fs::symlink_metadata(partial_path) {
            Ok(metadata) => {
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || is_reparse_point(&metadata)
                    || fs::canonicalize(partial_path).ok().as_deref() != Some(partial_path)
                {
                    return Err(LoadFailureReason::UnsafePartial);
                }
                if let Some(expected) = expected_size
                    && metadata.len() != expected
                {
                    return Err(LoadFailureReason::PartialLengthMismatch {
                        expected,
                        actual: metadata.len(),
                    });
                }
                partial_exists = true;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if matches!(
                    task.state,
                    TaskState::Downloading | TaskState::Paused | TaskState::Validating
                ) || task.state == TaskState::Promoting && task.final_path.is_none()
                {
                    return Err(LoadFailureReason::PartialMissing);
                }
            }
            Err(error) => return Err(LoadFailureReason::ReadFailed(classify_io(&error))),
        }
    }

    if let Some(final_path) = task.final_path.as_deref() {
        let metadata = match fs::symlink_metadata(final_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(LoadFailureReason::FinalMissing);
            }
            Err(error) => {
                return Err(LoadFailureReason::FinalInspectionFailed(classify_io(
                    &error,
                )));
            }
        };
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || is_reparse_point(&metadata)
            || fs::canonicalize(final_path).ok().as_deref() != Some(final_path)
        {
            return Err(LoadFailureReason::UnsafeFinal);
        }
        if let Some(expected) = expected_size
            && metadata.len() != expected
        {
            return Err(LoadFailureReason::FinalLengthMismatch {
                expected,
                actual: metadata.len(),
            });
        }
        if partial_exists
            && let Some(partial_path) = task.partial_path.as_deref()
            && !same_file::is_same_file(partial_path, final_path)
                .map_err(|error| LoadFailureReason::FinalInspectionFailed(classify_io(&error)))?
        {
            return Err(LoadFailureReason::PublicationIdentityMismatch);
        }
    }
    Ok(())
}

const fn validate_worker_count(workers: u8) -> Result<(), StateValidationError> {
    if matches!(workers, 1 | 2 | 4 | 8) {
        Ok(())
    } else {
        Err(StateValidationError::InvalidWorkerCount)
    }
}

fn validate_completed_ranges(
    ranges: &[FileRange],
    expected_size: Option<u64>,
) -> Result<(), StateValidationError> {
    if ranges.len() > MAX_COMPLETED_RANGES {
        return Err(StateValidationError::InvalidCompletedRanges);
    }
    let mut previous_end = None;
    for range in ranges {
        if previous_end.is_some_and(|end| range.start() <= end)
            || expected_size.is_some_and(|size| range.end() > size)
        {
            return Err(StateValidationError::InvalidCompletedRanges);
        }
        previous_end = Some(range.end());
    }
    ranges.iter().try_fold(0_u64, |total, range| {
        total
            .checked_add(range.len())
            .ok_or(StateValidationError::InvalidCompletedRanges)
    })?;
    Ok(())
}

fn has_exact_coverage(ranges: &[FileRange], expected_size: u64) -> bool {
    if expected_size == 0 {
        ranges.is_empty()
    } else {
        ranges.len() == 1 && ranges[0].start() == 0 && ranges[0].end() == expected_size
    }
}

fn normalize_url(value: &str) -> Result<String, StateValidationError> {
    if value.len() > MAX_URL_BYTES {
        return Err(StateValidationError::InvalidUrl);
    }
    let mut url = Url::parse(value).map_err(|_| StateValidationError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(StateValidationError::InvalidUrl);
    }
    url.set_fragment(None);
    let normalized = url.to_string();
    if normalized.len() > MAX_URL_BYTES {
        return Err(StateValidationError::InvalidUrl);
    }
    Ok(normalized)
}

fn require_canonical_url(value: &str) -> Result<String, StateValidationError> {
    let normalized = normalize_url(value)?;
    if normalized != value {
        return Err(StateValidationError::InvalidUrl);
    }
    Ok(normalized)
}

fn validate_validators(validators: &Validators) -> Result<(), StateValidationError> {
    if let Some(etag) = validators.etag.as_ref() {
        let formatted = format_entity_tag(etag);
        if formatted.len() > 8 * 1024 {
            return Err(StateValidationError::InvalidValidator);
        }
        EntityTag::parse(&formatted).map_err(|_| StateValidationError::InvalidValidator)?;
    }
    if let Some(last_modified) = validators.last_modified.as_ref()
        && (last_modified.len() > 128
            || httpdate::parse_http_date(last_modified).is_err()
            || last_modified.chars().any(char::is_control))
    {
        return Err(StateValidationError::InvalidValidator);
    }
    Ok(())
}

fn format_entity_tag(entity_tag: &EntityTag) -> String {
    let prefix = if entity_tag.is_weak() { "W/" } else { "" };
    format!("{prefix}\"{}\"", entity_tag.opaque())
}

fn validate_new_destination(path: &Path) -> Result<PathBuf, StateValidationError> {
    validate_path_syntax(path).map_err(|()| StateValidationError::InvalidDestination)?;
    let metadata =
        fs::symlink_metadata(path).map_err(|_| StateValidationError::InvalidDestination)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
        return Err(StateValidationError::InvalidDestination);
    }
    fs::canonicalize(path).map_err(|_| StateValidationError::InvalidDestination)
}

fn validate_persisted_directory(value: &str) -> Result<PathBuf, StateValidationError> {
    let path = PathBuf::from(value);
    validate_path_syntax(&path).map_err(|()| StateValidationError::InvalidDestination)?;
    Ok(path)
}

fn validate_path_syntax(path: &Path) -> Result<(), ()> {
    if !path.is_absolute()
        || has_forbidden_windows_prefix(path)
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(());
    }
    let text = path.to_str().ok_or(())?;
    if text.is_empty() || text.encode_utf16().count() > 32_767 || text.chars().any(|ch| ch == '\0')
    {
        return Err(());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum PathKind {
    Partial,
    Final,
}

fn validate_confined_path(
    path: &Path,
    destination: &Path,
    kind: PathKind,
) -> Result<(), StateValidationError> {
    validate_path_syntax(path).map_err(|()| match kind {
        PathKind::Partial => StateValidationError::InvalidPartialPath,
        PathKind::Final => StateValidationError::InvalidFinalPath,
    })?;
    if path.parent() != Some(destination) {
        return Err(match kind {
            PathKind::Partial => StateValidationError::InvalidPartialPath,
            PathKind::Final => StateValidationError::InvalidFinalPath,
        });
    }
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(match kind {
            PathKind::Partial => StateValidationError::InvalidPartialPath,
            PathKind::Final => StateValidationError::InvalidFinalPath,
        })?;
    match kind {
        PathKind::Partial
            if sanitize_filename(filename).as_str() != filename
                || !is_managed_partial_filename(filename) =>
        {
            Err(StateValidationError::InvalidPartialPath)
        }
        PathKind::Final if sanitize_filename(filename).as_str() != filename => {
            Err(StateValidationError::InvalidFinalPath)
        }
        _ => Ok(()),
    }
}

fn path_to_string(path: &Path) -> Result<&str, PersistenceError> {
    validate_path_syntax(path).map_err(|()| StateValidationError::InvalidDestination)?;
    path.to_str()
        .ok_or_else(|| StateValidationError::InvalidDestination.into())
}

fn validate_ordinary_directory(path: &Path) -> Result<(), PersistenceError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| map_persistence_io(PersistenceOperation::OpenStore, &error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
        return Err(StateValidationError::InvalidDestination.into());
    }
    Ok(())
}

fn is_temporary_suffix(value: &str) -> bool {
    let mut components = value.split('-');
    let (Some(process), Some(sequence), Some(attempt), None) = (
        components.next(),
        components.next(),
        components.next(),
        components.next(),
    ) else {
        return false;
    };
    !process.is_empty()
        && process.len() <= 8
        && sequence.len() == 16
        && attempt.len() == 2
        && process.bytes().all(is_lower_hex_digit)
        && sequence.bytes().all(is_lower_hex_digit)
        && attempt.bytes().all(is_lower_hex_digit)
}

const fn is_lower_hex_digit(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
}

fn opened_file_matches_path(file: &File, path: &Path) -> io::Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
        return Ok(false);
    }
    let file_handle = same_file::Handle::from_file(file.try_clone()?)?;
    let path_handle = same_file::Handle::from_path(path)?;
    Ok(file_handle == path_handle)
}

fn map_persistence_io(operation: PersistenceOperation, error: &io::Error) -> PersistenceError {
    PersistenceError::Io {
        operation,
        failure: classify_io(error),
        os_code: error.raw_os_error(),
    }
}

fn classify_io(error: &io::Error) -> IoFailure {
    #[cfg(windows)]
    if let Some(code) = error.raw_os_error() {
        match code {
            32 | 33 => return IoFailure::FileLocked,
            39 | 112 => return IoFailure::DiskFull,
            80 | 183 => return IoFailure::AlreadyExists,
            1 | 17 | 50 => return IoFailure::Unsupported,
            _ => {}
        }
    }
    match error.kind() {
        io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => IoFailure::DiskFull,
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => {
            IoFailure::AccessDenied
        }
        io::ErrorKind::AlreadyExists => IoFailure::AlreadyExists,
        io::ErrorKind::NotFound => IoFailure::NotFound,
        io::ErrorKind::Unsupported | io::ErrorKind::CrossesDevices => IoFailure::Unsupported,
        _ => IoFailure::Other,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(windows)]
fn has_forbidden_windows_prefix(path: &Path) -> bool {
    use std::path::Prefix;

    path.components().next().is_some_and(|component| {
        let Component::Prefix(prefix) = component else {
            return false;
        };
        matches!(prefix.kind(), Prefix::DeviceNS(_) | Prefix::Verbatim(_))
    })
}

#[cfg(not(windows))]
const fn has_forbidden_windows_prefix(_path: &Path) -> bool {
    false
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
const fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}
