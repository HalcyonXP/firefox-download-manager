//! Fixed-concurrency HTTP transfer scheduling over validated storage ranges.
//!
//! The scheduler never gives response bodies to storage until ranged status,
//! `Content-Range`, validators, encoding, and exact body length are proven.
//! Work is pulled from a shared missing-range queue so fast workers continue
//! making progress. When only one slow chunk remains, one bounded hedged
//! request may complete that tail; only the first fully validated body can own
//! the corresponding storage range.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT_ENCODING, CONTENT_LENGTH, IF_RANGE, RANGE};
use reqwest::redirect::Policy;
use reqwest::{Client, Response, StatusCode, Url};
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

use crate::network::{
    ProbeMode, RangeAssignment, RangeValidationError, ResourceProbe, Validators, if_range_value,
    optional_u64_header, parse_validators, reject_unexpected_encoding, retry_after_seconds,
    validate_expected_validators, validate_range_response,
};
use crate::persistence::MAX_COMPLETED_RANGES;
use crate::storage::{FileRange, PartialFile, StorageError};

/// Default fixed worker count for a segmented task.
pub const DEFAULT_WORKERS: u8 = 4;
/// Initial per-task worker cap.
pub const MAX_WORKERS: u8 = 8;
/// Smallest ordinary ranged request produced by the planner.
pub const MIN_REQUEST_BYTES: u64 = 1024 * 1024;
/// Largest ranged response buffered before it can claim storage ownership.
pub const MAX_REQUEST_BYTES: u64 = 8 * 1024 * 1024;
/// Default safety bound for a response with no declared length.
pub const DEFAULT_UNKNOWN_STREAM_LIMIT: u64 = 64 * 1024 * 1024 * 1024;

const CHUNKS_PER_WORKER: u64 = 4;
const DEFAULT_PER_HOST_LIMIT: usize = 8;
const DEFAULT_GLOBAL_LIMIT: usize = 16;
const MAX_PER_HOST_LIMIT: usize = 8;
const MAX_GLOBAL_LIMIT: usize = 32;
const MAX_HOST_LIMITERS: usize = 1024;
const DEFAULT_TAIL_HEDGE_DELAY: Duration = Duration::from_millis(250);
const MIN_TAIL_HEDGE_DELAY: Duration = Duration::from_millis(10);
const MAX_TAIL_HEDGE_DELAY: Duration = Duration::from_secs(30);
const MAX_UNKNOWN_STREAM_LIMIT: u64 = 16 * 1024 * 1024 * 1024 * 1024;
const MAX_RANGE_REQUESTS: u64 = 1_000_000;

/// Supported fixed worker counts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WorkerCount {
    /// One worker.
    One = 1,
    /// Two workers.
    Two = 2,
    /// Four workers (the default).
    #[default]
    Four = 4,
    /// Eight workers (the initial maximum).
    Eight = 8,
}

impl WorkerCount {
    /// Number of workers represented by this setting.
    #[must_use]
    pub const fn get(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for WorkerCount {
    type Error = SchedulerConfigError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            4 => Ok(Self::Four),
            8 => Ok(Self::Eight),
            _ => Err(SchedulerConfigError::InvalidWorkerCount),
        }
    }
}

/// Independently bounded host and process request concurrency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConcurrencyLimits {
    per_host: usize,
    global: usize,
}

impl ConcurrencyLimits {
    /// Constructs bounded nonzero request limits.
    ///
    /// # Errors
    ///
    /// Rejects zero, a per-host limit above 8, or a global limit above 32.
    pub const fn new(per_host: usize, global: usize) -> Result<Self, SchedulerConfigError> {
        if per_host == 0
            || global == 0
            || per_host > MAX_PER_HOST_LIMIT
            || global > MAX_GLOBAL_LIMIT
        {
            return Err(SchedulerConfigError::InvalidConcurrencyLimits);
        }
        Ok(Self { per_host, global })
    }

    /// Maximum simultaneous requests to one URL origin.
    #[must_use]
    pub const fn per_host(self) -> usize {
        self.per_host
    }

    /// Maximum simultaneous requests across this scheduler.
    #[must_use]
    pub const fn global(self) -> usize {
        self.global
    }
}

impl Default for ConcurrencyLimits {
    fn default() -> Self {
        Self {
            per_host: DEFAULT_PER_HOST_LIMIT,
            global: DEFAULT_GLOBAL_LIMIT,
        }
    }
}

/// Validated scheduler construction options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerOptions {
    limits: ConcurrencyLimits,
    tail_hedge_delay: Duration,
    unknown_stream_limit: u64,
}

impl SchedulerOptions {
    /// Creates scheduler options with explicit bounds.
    ///
    /// # Errors
    ///
    /// Rejects hedge delays outside 10 ms through 30 seconds and unknown-body
    /// limits outside 1 byte through 16 TiB.
    pub fn new(
        limits: ConcurrencyLimits,
        tail_hedge_delay: Duration,
        unknown_stream_limit: u64,
    ) -> Result<Self, SchedulerConfigError> {
        if tail_hedge_delay < MIN_TAIL_HEDGE_DELAY || tail_hedge_delay > MAX_TAIL_HEDGE_DELAY {
            return Err(SchedulerConfigError::InvalidHedgeDelay);
        }
        if unknown_stream_limit == 0 || unknown_stream_limit > MAX_UNKNOWN_STREAM_LIMIT {
            return Err(SchedulerConfigError::InvalidUnknownStreamLimit);
        }
        Ok(Self {
            limits,
            tail_hedge_delay,
            unknown_stream_limit,
        })
    }

    /// Independent request concurrency limits.
    #[must_use]
    pub const fn limits(self) -> ConcurrencyLimits {
        self.limits
    }

    /// Delay before an idle worker may hedge the sole slow tail.
    #[must_use]
    pub const fn tail_hedge_delay(self) -> Duration {
        self.tail_hedge_delay
    }

    /// Maximum bytes accepted from an undeclared-length response.
    #[must_use]
    pub const fn unknown_stream_limit(self) -> u64 {
        self.unknown_stream_limit
    }
}

impl Default for SchedulerOptions {
    fn default() -> Self {
        Self {
            limits: ConcurrencyLimits::default(),
            tail_hedge_delay: DEFAULT_TAIL_HEDGE_DELAY,
            unknown_stream_limit: DEFAULT_UNKNOWN_STREAM_LIMIT,
        }
    }
}

/// Safe scheduler configuration failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SchedulerConfigError {
    /// Worker count was not 1, 2, 4, or 8.
    #[error("worker count must be 1, 2, 4, or 8")]
    InvalidWorkerCount,
    /// Host/global request limits were zero or above their caps.
    #[error("request concurrency limits are invalid")]
    InvalidConcurrencyLimits,
    /// Tail hedge delay was too short or too long.
    #[error("tail hedge delay is outside supported bounds")]
    InvalidHedgeDelay,
    /// Unknown-length response bound was zero or too large.
    #[error("unknown-length response limit is outside supported bounds")]
    InvalidUnknownStreamLimit,
}

/// Cooperative cancellation shared by one transfer invocation and its workers.
#[derive(Clone)]
pub struct TransferCancellation {
    signal: watch::Sender<bool>,
}

impl TransferCancellation {
    /// Creates an initially active cancellation signal.
    #[must_use]
    pub fn new() -> Self {
        let (signal, _) = watch::channel(false);
        Self { signal }
    }

    /// Requests cooperative stop. Repeated requests are idempotent.
    pub fn cancel(&self) {
        self.signal.send_replace(true);
    }

    /// Whether stop has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        *self.signal.borrow()
    }

    /// Waits until cancellation is requested.
    pub async fn cancelled(&self) {
        let mut receiver = self.signal.subscribe();
        if *receiver.borrow_and_update() {
            return;
        }
        while receiver.changed().await.is_ok() {
            if *receiver.borrow_and_update() {
                return;
            }
        }
    }

    fn subscribe(&self) -> watch::Receiver<bool> {
        self.signal.subscribe()
    }
}

impl Default for TransferCancellation {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for TransferCancellation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransferCancellation")
            .field("is_cancelled", &self.is_cancelled())
            .finish()
    }
}

/// Absolute transfer counters suitable for coalesced progress sampling.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TransferProgress {
    bytes_completed: u64,
    expected_size: Option<u64>,
    active_workers: u8,
}

impl TransferProgress {
    /// Bytes represented by this invocation's durable baseline plus safe
    /// in-process progress.
    #[must_use]
    pub const fn bytes_completed(self) -> u64 {
        self.bytes_completed
    }

    /// Expected complete length, or `None` until an unknown stream reaches EOF.
    #[must_use]
    pub const fn expected_size(self) -> Option<u64> {
        self.expected_size
    }

    /// Worker requests currently sending or receiving a response.
    #[must_use]
    pub const fn active_workers(self) -> u8 {
        self.active_workers
    }
}

/// Scheduler-owned endpoint for a bounded latest-value progress channel.
#[derive(Clone)]
pub struct TransferProgressReporter {
    sender: watch::Sender<TransferProgress>,
}

impl fmt::Debug for TransferProgressReporter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransferProgressReporter")
            .finish_non_exhaustive()
    }
}

impl TransferProgressReporter {
    fn publish(&self, progress: TransferProgress) {
        self.sender.send_replace(progress);
    }
}

/// Consumer endpoint retaining the newest absolute progress even when samples
/// are coalesced.
pub struct TransferProgressReceiver {
    receiver: watch::Receiver<TransferProgress>,
}

impl fmt::Debug for TransferProgressReceiver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransferProgressReceiver")
            .field("latest", &*self.receiver.borrow())
            .finish()
    }
}

impl TransferProgressReceiver {
    /// Returns the latest complete absolute sample without waiting.
    #[must_use]
    pub fn latest(&self) -> TransferProgress {
        *self.receiver.borrow()
    }

    /// Waits for a newer sample, returning `None` after all reporters close.
    pub async fn changed(&mut self) -> Option<TransferProgress> {
        self.receiver.changed().await.ok()?;
        Some(*self.receiver.borrow_and_update())
    }
}

/// Creates a bounded coalescing channel for one controlled transfer.
#[must_use]
pub fn transfer_progress_channel() -> (TransferProgressReporter, TransferProgressReceiver) {
    let (sender, receiver) = watch::channel(TransferProgress::default());
    (
        TransferProgressReporter { sender },
        TransferProgressReceiver { receiver },
    )
}

/// Transfer strategy actually executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    /// Validated ranged requests over a known-size resource.
    Segmented,
    /// One sequential response, with a known or EOF-discovered size.
    Single,
    /// A proven empty resource requiring no body request.
    Empty,
}

/// Bounded transfer statistics for integration with later progress events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferSummary {
    kind: TransferKind,
    bytes_written: u64,
    requests_started: u64,
    ranges_completed: u64,
    hedged_requests: u64,
    workers_used: u8,
}

impl TransferSummary {
    /// Transfer strategy that ran.
    #[must_use]
    pub const fn kind(self) -> TransferKind {
        self.kind
    }

    /// Bytes newly committed to storage by this invocation.
    #[must_use]
    pub const fn bytes_written(self) -> u64 {
        self.bytes_written
    }

    /// HTTP requests started, including one optional tail hedge.
    #[must_use]
    pub const fn requests_started(self) -> u64 {
        self.requests_started
    }

    /// Storage ranges completed by this invocation.
    #[must_use]
    pub const fn ranges_completed(self) -> u64 {
        self.ranges_completed
    }

    /// Duplicate tail requests started after the bounded hedge delay.
    #[must_use]
    pub const fn hedged_requests(self) -> u64 {
        self.hedged_requests
    }

    /// Configured workers used for this transfer mode.
    #[must_use]
    pub const fn workers_used(self) -> u8 {
        self.workers_used
    }
}

/// Path-free transfer failures.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SchedulerError {
    /// The conservatively configured HTTP client could not be built.
    #[error("transfer HTTP client setup failed")]
    ClientSetup,
    /// Probe mode/size and storage shape disagree.
    #[error("probe and partial storage are inconsistent")]
    ProbeStorageMismatch,
    /// The final probe URL changed during a worker request.
    #[error("worker response URL changed")]
    ResponseUrlChanged,
    /// Sending or streaming an HTTP request failed.
    #[error("worker HTTP request failed")]
    Request,
    /// A worker received a status that cannot provide assigned bytes.
    #[error("worker returned HTTP status {status}")]
    HttpStatus {
        /// Numeric status without response reason text.
        status: u16,
        /// Bounded retry guidance consumed by the task policy.
        retry_after_seconds: Option<u64>,
    },
    /// Ranged response metadata did not prove the assignment.
    #[error("worker ranged response is invalid: {0}")]
    InvalidRange(#[from] RangeValidationError),
    /// A sequential response had invalid encoding, singleton headers, or validators.
    #[error("worker sequential response metadata is invalid: {0}")]
    InvalidSingleResponse(RangeValidationError),
    /// Response EOF did not occur at the exact accepted byte boundary.
    #[error("worker response body length is invalid")]
    BodyLengthMismatch,
    /// Missing-range metadata was not canonical or in bounds.
    #[error("completed storage coverage is invalid")]
    InvalidCompletedCoverage,
    /// A known-size transfer would require too many bounded requests.
    #[error("ranged request plan exceeds its safety bound")]
    RequestPlanTooLarge,
    /// Storage rejected assignment, writes, sealing, or publication state.
    #[error("worker storage operation failed: {0}")]
    Storage(#[from] StorageError),
    /// An internal worker task ended unexpectedly.
    #[error("transfer worker ended unexpectedly")]
    WorkerJoin,
    /// Cooperative pause/cancel control stopped network work safely.
    #[error("transfer was cancelled")]
    Cancelled,
    /// Internal bounded coordination state was exhausted.
    #[error("transfer coordination state is invalid")]
    Coordination,
}

/// Reusable scheduler whose semaphores apply across concurrent tasks.
#[derive(Clone)]
pub struct DownloadScheduler {
    inner: Arc<SchedulerInner>,
}

struct SchedulerInner {
    client: Client,
    options: SchedulerOptions,
    global: Arc<Semaphore>,
    hosts: Mutex<HashMap<String, Weak<Semaphore>>>,
    active_requests: AtomicUsize,
    peak_requests: AtomicUsize,
}

impl fmt::Debug for DownloadScheduler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DownloadScheduler")
            .field("options", &self.inner.options)
            .field(
                "active_requests",
                &self.inner.active_requests.load(Ordering::Relaxed),
            )
            .field(
                "peak_requests",
                &self.inner.peak_requests.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl DownloadScheduler {
    /// Builds a scheduler with four-worker task defaults and conservative
    /// request limits.
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::ClientSetup`] if Reqwest rejects static safe
    /// client configuration.
    pub fn new() -> Result<Self, SchedulerError> {
        Self::with_options(SchedulerOptions::default())
    }

    /// Builds a scheduler with validated limits and tail behavior.
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::ClientSetup`] if Reqwest rejects static safe
    /// client configuration.
    pub fn with_options(options: SchedulerOptions) -> Result<Self, SchedulerError> {
        let client = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent("FirefoxDownloadManager/0.1")
            .build()
            .map_err(|_| SchedulerError::ClientSetup)?;
        Ok(Self {
            inner: Arc::new(SchedulerInner {
                client,
                options,
                global: Arc::new(Semaphore::new(options.limits.global)),
                hosts: Mutex::new(HashMap::new()),
                active_requests: AtomicUsize::new(0),
                peak_requests: AtomicUsize::new(0),
            }),
        })
    }

    /// Highest simultaneous request count observed across this scheduler.
    #[must_use]
    pub fn peak_global_requests(&self) -> usize {
        self.inner.peak_requests.load(Ordering::Acquire)
    }

    /// Transfers all currently missing bytes proven by `probe` into `partial`.
    ///
    /// Known-size segmented tasks use the selected fixed worker count. Safe
    /// single-stream fallback uses one worker and the same partial/promote
    /// storage lifecycle. Existing durable completed ranges are skipped.
    ///
    /// # Errors
    ///
    /// Fails closed on probe/storage disagreement, redirects, status/header/
    /// validator/body mismatches, request failures, or storage failures. All
    /// started workers stop or reach a safe assignment boundary before return.
    pub async fn transfer(
        &self,
        probe: &ResourceProbe,
        partial: &PartialFile,
        workers: WorkerCount,
    ) -> Result<TransferSummary, SchedulerError> {
        let cancellation = TransferCancellation::new();
        let (progress, _) = transfer_progress_channel();
        self.transfer_controlled(probe, partial, workers, &cancellation, progress)
            .await
    }

    /// Transfers missing bytes with cooperative cancellation and absolute
    /// latest-value progress reporting.
    ///
    /// A cancellation result is returned only after all range workers have
    /// stopped or completed their current synchronous storage commit. The
    /// caller can then flush/checkpoint completed coverage before acknowledging
    /// pause or cancellation.
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::Cancelled`] after a requested safe stop, or
    /// the same strict network/storage errors as [`Self::transfer`].
    pub async fn transfer_controlled(
        &self,
        probe: &ResourceProbe,
        partial: &PartialFile,
        workers: WorkerCount,
        cancellation: &TransferCancellation,
        progress: TransferProgressReporter,
    ) -> Result<TransferSummary, SchedulerError> {
        if cancellation.is_cancelled() {
            return Err(SchedulerError::Cancelled);
        }
        let baseline = partial
            .completed_ranges()
            .iter()
            .try_fold(0_u64, |total, range| total.checked_add(range.len()))
            .ok_or(SchedulerError::InvalidCompletedCoverage)?;
        if baseline > 0 && !probe.validators().has_strong_identity() {
            return Err(SchedulerError::InvalidCompletedCoverage);
        }
        let metrics = Arc::new(TransferMetrics::new(baseline, probe.size(), progress));

        let result = match probe.mode() {
            ProbeMode::Segmented => {
                let total = probe.size().ok_or(SchedulerError::ProbeStorageMismatch)?;
                if total == 0 || partial.expected_len() != Some(total) || baseline > total {
                    return Err(SchedulerError::ProbeStorageMismatch);
                }
                self.transfer_segmented(
                    probe,
                    partial,
                    workers,
                    total,
                    cancellation,
                    Arc::clone(&metrics),
                )
                .await
            }
            ProbeMode::SingleStream(_) => {
                if partial.expected_len() != probe.size()
                    || probe.size().is_some_and(|total| baseline > total)
                {
                    return Err(SchedulerError::ProbeStorageMismatch);
                }
                self.transfer_single(probe, partial, cancellation, Arc::clone(&metrics))
                    .await
            }
            ProbeMode::Empty => {
                if probe.size() != Some(0) || partial.expected_len() != Some(0) || baseline != 0 {
                    return Err(SchedulerError::ProbeStorageMismatch);
                }
                Ok(metrics.summary(TransferKind::Empty, 0))
            }
        };
        if cancellation.is_cancelled() {
            return Err(SchedulerError::Cancelled);
        }
        result
    }

    async fn transfer_segmented(
        &self,
        probe: &ResourceProbe,
        partial: &PartialFile,
        workers: WorkerCount,
        total: u64,
        cancellation: &TransferCancellation,
        metrics: Arc<TransferMetrics>,
    ) -> Result<TransferSummary, SchedulerError> {
        let completed = partial.completed_ranges();
        let missing = missing_ranges(total, &completed)?;
        if missing.is_empty() {
            return Ok(metrics.summary(TransferKind::Segmented, workers.get()));
        }
        let host = self.host_semaphore(probe.final_url())?;
        let missing_bytes = missing.iter().try_fold(0_u64, |sum, range| {
            sum.checked_add(range.len())
                .ok_or(SchedulerError::InvalidCompletedCoverage)
        })?;
        let chunk_size = request_chunk_size(missing_bytes, workers);
        validate_request_plan(&missing, chunk_size)?;
        let coordinator = Arc::new(WorkCoordinator::new(
            missing,
            chunk_size,
            self.inner.options.tail_hedge_delay,
            cancellation.clone(),
        ));
        let mut handles = tokio::task::JoinSet::new();
        for _ in 0..workers.get() {
            let scheduler = self.clone();
            let probe = probe.clone();
            let partial = partial.clone();
            let host = Arc::clone(&host);
            let coordinator = Arc::clone(&coordinator);
            let metrics = Arc::clone(&metrics);
            handles.spawn(async move {
                scheduler
                    .range_worker(&probe, &partial, total, host, coordinator, metrics)
                    .await
            });
        }

        let mut first_error = None;
        while let Some(result) = handles.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    coordinator.abort();
                    first_error.get_or_insert(error);
                }
                Err(_) => {
                    coordinator.abort();
                    first_error.get_or_insert(SchedulerError::WorkerJoin);
                }
            }
        }
        if cancellation.is_cancelled() {
            return Err(SchedulerError::Cancelled);
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        if !has_exact_coverage(&partial.completed_ranges(), total) {
            return Err(SchedulerError::InvalidCompletedCoverage);
        }
        Ok(metrics.summary(TransferKind::Segmented, workers.get()))
    }

    async fn range_worker(
        &self,
        probe: &ResourceProbe,
        partial: &PartialFile,
        total: u64,
        host: Arc<Semaphore>,
        coordinator: Arc<WorkCoordinator>,
        metrics: Arc<TransferMetrics>,
    ) -> Result<(), SchedulerError> {
        while let Some(work) = coordinator.next_work().await? {
            match self
                .fetch_range(
                    probe.final_url(),
                    probe.validators(),
                    total,
                    work,
                    Arc::clone(&host),
                    &coordinator,
                    &metrics,
                )
                .await
            {
                Ok(FetchOutcome::Superseded) => coordinator.finish_superseded(work.id),
                Ok(FetchOutcome::Body(bytes)) => {
                    if !coordinator.claim_commit(work.id) {
                        continue;
                    }
                    let result = commit_range(partial, work.range, &bytes);
                    match result {
                        Ok(()) => {
                            metrics.record_range(work.range.len());
                            coordinator.finish_commit(work.id, true);
                        }
                        Err(error) => {
                            coordinator.finish_commit(work.id, false);
                            return Err(error);
                        }
                    }
                }
                Err(error) => {
                    if coordinator.finish_failed_attempt(work.id) {
                        return Err(error);
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn fetch_range(
        &self,
        url: &Url,
        validators: &Validators,
        total: u64,
        work: Work,
        host: Arc<Semaphore>,
        coordinator: &WorkCoordinator,
        metrics: &Arc<TransferMetrics>,
    ) -> Result<FetchOutcome, SchedulerError> {
        let Some(_host_permit) = acquire_or_cancel(host, coordinator, work.id).await? else {
            return Ok(FetchOutcome::Superseded);
        };
        let Some(_global_permit) =
            acquire_or_cancel(Arc::clone(&self.inner.global), coordinator, work.id).await?
        else {
            return Ok(FetchOutcome::Superseded);
        };
        if coordinator.should_cancel(work.id) {
            return Ok(FetchOutcome::Superseded);
        }
        let _activity = RequestActivity::begin(Arc::clone(&self.inner), Arc::clone(metrics));
        metrics.record_request(work.hedged);

        let assignment = RangeAssignment::new(work.range.start(), work.range.end() - 1, total)?;
        let mut request = self
            .inner
            .client
            .get(url.clone())
            .header(
                RANGE,
                format!("bytes={}-{}", assignment.start(), assignment.end()),
            )
            .header(ACCEPT_ENCODING, "identity");
        if let Some(value) = if_range_value(validators) {
            request = request.header(IF_RANGE, value);
        }
        let Some(mut response) = send_or_cancel(request, coordinator, work.id).await? else {
            return Ok(FetchOutcome::Superseded);
        };
        if response.url() != url {
            return Err(SchedulerError::ResponseUrlChanged);
        }
        if response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(SchedulerError::HttpStatus {
                status: response.status().as_u16(),
                retry_after_seconds: retry_after_seconds(response.headers()),
            });
        }
        validate_range_response(
            response.status(),
            response.headers(),
            assignment,
            validators,
        )?;

        let capacity =
            usize::try_from(assignment.len()).map_err(|_| SchedulerError::BodyLengthMismatch)?;
        let mut body = Vec::with_capacity(capacity);
        while let Some(chunk) = next_chunk_or_cancel(&mut response, coordinator, work.id).await? {
            if body.len().saturating_add(chunk.len()) > capacity {
                return Err(SchedulerError::BodyLengthMismatch);
            }
            body.extend_from_slice(&chunk);
        }
        if coordinator.should_cancel(work.id) {
            return Ok(FetchOutcome::Superseded);
        }
        if body.len() != capacity {
            return Err(SchedulerError::BodyLengthMismatch);
        }
        Ok(FetchOutcome::Body(body))
    }

    async fn transfer_single(
        &self,
        probe: &ResourceProbe,
        partial: &PartialFile,
        cancellation: &TransferCancellation,
        metrics: Arc<TransferMetrics>,
    ) -> Result<TransferSummary, SchedulerError> {
        if let Some(total) = probe.size()
            && has_exact_coverage(&partial.completed_ranges(), total)
        {
            return Ok(metrics.summary(TransferKind::Single, 1));
        }
        if !partial.completed_ranges().is_empty() {
            return Err(SchedulerError::ProbeStorageMismatch);
        }

        let host = self.host_semaphore(probe.final_url())?;
        let host_permit = acquire_with_cancellation(host, cancellation).await?;
        let global_permit =
            acquire_with_cancellation(Arc::clone(&self.inner.global), cancellation).await?;
        let activity = RequestActivity::begin(Arc::clone(&self.inner), Arc::clone(&metrics));
        metrics.record_request(false);
        let request = self
            .inner
            .client
            .get(probe.final_url().clone())
            .header(ACCEPT_ENCODING, "identity");
        let response = send_with_cancellation(request, cancellation).await?;
        let actual = self
            .consume_single_response(response, probe, partial, cancellation, &metrics)
            .await?;
        metrics.complete_single(actual);
        drop(activity);
        drop(global_permit);
        drop(host_permit);
        if cancellation.is_cancelled() {
            return Err(SchedulerError::Cancelled);
        }
        Ok(metrics.summary(TransferKind::Single, 1))
    }

    async fn consume_single_response(
        &self,
        mut response: Response,
        probe: &ResourceProbe,
        partial: &PartialFile,
        cancellation: &TransferCancellation,
        metrics: &TransferMetrics,
    ) -> Result<u64, SchedulerError> {
        if response.url() != probe.final_url() {
            return Err(SchedulerError::ResponseUrlChanged);
        }
        if response.status() != StatusCode::OK {
            return Err(SchedulerError::HttpStatus {
                status: response.status().as_u16(),
                retry_after_seconds: retry_after_seconds(response.headers()),
            });
        }
        reject_unexpected_encoding(response.headers())
            .map_err(SchedulerError::InvalidSingleResponse)?;
        let declared = optional_u64_header(response.headers(), CONTENT_LENGTH)
            .map_err(SchedulerError::InvalidSingleResponse)?;
        if let Some(expected) = probe.size()
            && declared.is_some_and(|length| length != expected)
        {
            return Err(SchedulerError::BodyLengthMismatch);
        }
        if probe.size().is_none()
            && declared.is_some_and(|length| length > self.inner.options.unknown_stream_limit)
        {
            return Err(StorageError::StreamLimitExceeded {
                limit: self.inner.options.unknown_stream_limit,
            }
            .into());
        }
        let actual_validators =
            parse_validators(response.headers()).map_err(SchedulerError::InvalidSingleResponse)?;
        validate_expected_validators(probe.validators(), &actual_validators)
            .map_err(SchedulerError::InvalidSingleResponse)?;

        let actual = if let Some(expected) = probe.size() {
            let range = FileRange::new(0, expected).map_err(SchedulerError::Storage)?;
            let mut writer = partial.assign(range)?;
            while let Some(chunk) = next_single_chunk(&mut response, cancellation).await? {
                writer.write(&chunk)?;
                metrics.record_stream_progress(writer.written_len());
            }
            if writer.written_len() != expected
                || declared.is_some_and(|length| length != writer.written_len())
            {
                return Err(SchedulerError::BodyLengthMismatch);
            }
            if cancellation.is_cancelled() {
                return Err(SchedulerError::Cancelled);
            }
            writer.finish()?;
            expected
        } else {
            let mut writer = partial.begin_stream(self.inner.options.unknown_stream_limit)?;
            while let Some(chunk) = next_single_chunk(&mut response, cancellation).await? {
                writer.write(&chunk)?;
                metrics.record_stream_progress(writer.written_len());
            }
            if declared.is_some_and(|length| length != writer.written_len()) {
                return Err(SchedulerError::BodyLengthMismatch);
            }
            if cancellation.is_cancelled() {
                return Err(SchedulerError::Cancelled);
            }
            let actual = writer.finish()?;
            metrics.set_expected_size(Some(actual));
            actual
        };

        Ok(actual)
    }

    fn host_semaphore(&self, url: &Url) -> Result<Arc<Semaphore>, SchedulerError> {
        let key = url.origin().ascii_serialization();
        let mut hosts = lock(&self.inner.hosts);
        hosts.retain(|_, semaphore| semaphore.strong_count() > 0);
        if let Some(existing) = hosts.get(&key).and_then(Weak::upgrade) {
            return Ok(existing);
        }
        if hosts.len() >= MAX_HOST_LIMITERS {
            return Err(SchedulerError::Coordination);
        }
        let semaphore = Arc::new(Semaphore::new(self.inner.options.limits.per_host));
        hosts.insert(key, Arc::downgrade(&semaphore));
        Ok(semaphore)
    }
}

#[derive(Debug)]
struct TransferMetrics {
    bytes_completed: AtomicU64,
    expected_size: Mutex<Option<u64>>,
    active_workers: AtomicUsize,
    bytes_written: AtomicU64,
    requests_started: AtomicU64,
    ranges_completed: AtomicU64,
    hedged_requests: AtomicU64,
    progress: TransferProgressReporter,
}

impl TransferMetrics {
    fn new(
        bytes_completed: u64,
        expected_size: Option<u64>,
        progress: TransferProgressReporter,
    ) -> Self {
        let metrics = Self {
            bytes_completed: AtomicU64::new(bytes_completed),
            expected_size: Mutex::new(expected_size),
            active_workers: AtomicUsize::new(0),
            bytes_written: AtomicU64::new(0),
            requests_started: AtomicU64::new(0),
            ranges_completed: AtomicU64::new(0),
            hedged_requests: AtomicU64::new(0),
            progress,
        };
        metrics.publish();
        metrics
    }

    fn record_request(&self, hedged: bool) {
        self.requests_started.fetch_add(1, Ordering::Relaxed);
        if hedged {
            self.hedged_requests.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn worker_started(&self) {
        self.active_workers.fetch_add(1, Ordering::AcqRel);
        self.publish();
    }

    fn worker_stopped(&self) {
        self.active_workers.fetch_sub(1, Ordering::AcqRel);
        self.publish();
    }

    fn record_range(&self, bytes: u64) {
        self.bytes_written.fetch_add(bytes, Ordering::Relaxed);
        self.ranges_completed.fetch_add(1, Ordering::Relaxed);
        self.bytes_completed.fetch_add(bytes, Ordering::AcqRel);
        self.publish();
    }

    fn record_stream_progress(&self, bytes: u64) {
        self.bytes_completed.store(bytes, Ordering::Release);
        self.publish();
    }

    fn complete_single(&self, bytes: u64) {
        self.bytes_written.store(bytes, Ordering::Release);
        self.ranges_completed
            .store(u64::from(bytes > 0), Ordering::Release);
        self.bytes_completed.store(bytes, Ordering::Release);
        self.publish();
    }

    fn set_expected_size(&self, expected_size: Option<u64>) {
        *lock(&self.expected_size) = expected_size;
        self.publish();
    }

    fn publish(&self) {
        let active = self
            .active_workers
            .load(Ordering::Acquire)
            .min(usize::from(MAX_WORKERS));
        self.progress.publish(TransferProgress {
            bytes_completed: self.bytes_completed.load(Ordering::Acquire),
            expected_size: *lock(&self.expected_size),
            active_workers: u8::try_from(active).unwrap_or(MAX_WORKERS),
        });
    }

    fn summary(&self, kind: TransferKind, workers_used: u8) -> TransferSummary {
        TransferSummary {
            kind,
            bytes_written: self.bytes_written.load(Ordering::Acquire),
            requests_started: self.requests_started.load(Ordering::Acquire),
            ranges_completed: self.ranges_completed.load(Ordering::Acquire),
            hedged_requests: self.hedged_requests.load(Ordering::Acquire),
            workers_used,
        }
    }
}

struct RequestActivity {
    scheduler: Arc<SchedulerInner>,
    transfer: Arc<TransferMetrics>,
}

impl RequestActivity {
    fn begin(scheduler: Arc<SchedulerInner>, transfer: Arc<TransferMetrics>) -> Self {
        let active = scheduler.active_requests.fetch_add(1, Ordering::AcqRel) + 1;
        update_peak(&scheduler.peak_requests, active);
        transfer.worker_started();
        Self {
            scheduler,
            transfer,
        }
    }
}

impl Drop for RequestActivity {
    fn drop(&mut self) {
        self.scheduler
            .active_requests
            .fetch_sub(1, Ordering::AcqRel);
        self.transfer.worker_stopped();
    }
}

fn update_peak(peak: &AtomicUsize, candidate: usize) {
    let mut current = peak.load(Ordering::Relaxed);
    while candidate > current {
        match peak.compare_exchange_weak(current, candidate, Ordering::AcqRel, Ordering::Relaxed) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Work {
    id: u64,
    range: FileRange,
    hedged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkPhase {
    InFlight,
    Committing,
}

#[derive(Debug)]
struct ChunkState {
    range: FileRange,
    phase: ChunkPhase,
    attempts_started: u8,
    active_attempts: u8,
    first_started: Instant,
}

#[derive(Debug)]
struct WorkState {
    pending: VecDeque<FileRange>,
    in_flight: BTreeMap<u64, ChunkState>,
    next_id: u64,
    aborted: bool,
}

#[derive(Debug)]
struct WorkCoordinator {
    state: Mutex<WorkState>,
    changes: watch::Sender<u64>,
    cancellation: TransferCancellation,
    chunk_size: u64,
    hedge_delay: Duration,
}

impl WorkCoordinator {
    fn new(
        missing: Vec<FileRange>,
        chunk_size: u64,
        hedge_delay: Duration,
        cancellation: TransferCancellation,
    ) -> Self {
        let (changes, _) = watch::channel(0);
        Self {
            state: Mutex::new(WorkState {
                pending: missing.into(),
                in_flight: BTreeMap::new(),
                next_id: 1,
                aborted: false,
            }),
            changes,
            cancellation,
            chunk_size,
            hedge_delay,
        }
    }

    async fn next_work(&self) -> Result<Option<Work>, SchedulerError> {
        let mut changes = self.changes.subscribe();
        let mut cancellation = self.cancellation.subscribe();
        loop {
            let wait = {
                let mut state = lock(&self.state);
                if state.aborted || self.cancellation.is_cancelled() {
                    return Ok(None);
                }
                if let Some(range) = pop_chunk(&mut state.pending, self.chunk_size)? {
                    let id = state.next_id;
                    state.next_id = state
                        .next_id
                        .checked_add(1)
                        .ok_or(SchedulerError::Coordination)?;
                    state.in_flight.insert(
                        id,
                        ChunkState {
                            range,
                            phase: ChunkPhase::InFlight,
                            attempts_started: 1,
                            active_attempts: 1,
                            first_started: Instant::now(),
                        },
                    );
                    return Ok(Some(Work {
                        id,
                        range,
                        hedged: false,
                    }));
                }
                if state.in_flight.is_empty() {
                    return Ok(None);
                }

                let now = Instant::now();
                let mut remaining = None;
                let mut hedge = None;
                let sole_tail = state.in_flight.len() == 1;
                for (&id, chunk) in &state.in_flight {
                    if !sole_tail {
                        break;
                    }
                    if chunk.phase != ChunkPhase::InFlight || chunk.attempts_started != 1 {
                        continue;
                    }
                    let elapsed = now.saturating_duration_since(chunk.first_started);
                    if elapsed >= self.hedge_delay {
                        hedge = Some(id);
                        break;
                    }
                    let delay = self.hedge_delay.saturating_sub(elapsed);
                    remaining = Some(remaining.map_or(delay, |known: Duration| known.min(delay)));
                }
                if let Some(id) = hedge {
                    let chunk = state
                        .in_flight
                        .get_mut(&id)
                        .ok_or(SchedulerError::Coordination)?;
                    chunk.attempts_started = 2;
                    chunk.active_attempts = chunk
                        .active_attempts
                        .checked_add(1)
                        .ok_or(SchedulerError::Coordination)?;
                    return Ok(Some(Work {
                        id,
                        range: chunk.range,
                        hedged: true,
                    }));
                }
                remaining
            };

            if let Some(delay) = wait {
                tokio::select! {
                    () = tokio::time::sleep(delay) => {}
                    result = changes.changed() => {
                        result.map_err(|_| SchedulerError::Coordination)?;
                    }
                    result = cancellation.changed() => {
                        result.map_err(|_| SchedulerError::Coordination)?;
                    }
                }
            } else {
                tokio::select! {
                    result = changes.changed() => {
                        result.map_err(|_| SchedulerError::Coordination)?;
                    }
                    result = cancellation.changed() => {
                        result.map_err(|_| SchedulerError::Coordination)?;
                    }
                }
            }
        }
    }

    fn should_cancel(&self, id: u64) -> bool {
        let state = lock(&self.state);
        state.aborted
            || self.cancellation.is_cancelled()
            || state
                .in_flight
                .get(&id)
                .is_none_or(|chunk| chunk.phase != ChunkPhase::InFlight)
    }

    fn claim_commit(&self, id: u64) -> bool {
        let mut state = lock(&self.state);
        if state.aborted || self.cancellation.is_cancelled() {
            return false;
        }
        let Some(chunk) = state.in_flight.get_mut(&id) else {
            return false;
        };
        chunk.active_attempts = chunk.active_attempts.saturating_sub(1);
        if chunk.phase != ChunkPhase::InFlight {
            return false;
        }
        chunk.phase = ChunkPhase::Committing;
        drop(state);
        self.signal();
        true
    }

    fn finish_superseded(&self, id: u64) {
        if let Some(chunk) = lock(&self.state).in_flight.get_mut(&id) {
            chunk.active_attempts = chunk.active_attempts.saturating_sub(1);
        }
        self.signal();
    }

    fn finish_failed_attempt(&self, id: u64) -> bool {
        let mut state = lock(&self.state);
        let Some(chunk) = state.in_flight.get_mut(&id) else {
            return false;
        };
        chunk.active_attempts = chunk.active_attempts.saturating_sub(1);
        if chunk.phase == ChunkPhase::Committing || chunk.active_attempts > 0 {
            drop(state);
            self.signal();
            return false;
        }
        state.in_flight.remove(&id);
        state.aborted = true;
        drop(state);
        self.signal();
        true
    }

    fn finish_commit(&self, id: u64, succeeded: bool) {
        let mut state = lock(&self.state);
        state.in_flight.remove(&id);
        if !succeeded {
            state.aborted = true;
        }
        drop(state);
        self.signal();
    }

    fn abort(&self) {
        lock(&self.state).aborted = true;
        self.signal();
    }

    fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    fn signal(&self) {
        self.changes.send_modify(|generation| {
            *generation = generation.wrapping_add(1);
        });
    }
}

#[derive(Debug)]
enum FetchOutcome {
    Body(Vec<u8>),
    Superseded,
}

async fn acquire_or_cancel(
    semaphore: Arc<Semaphore>,
    coordinator: &WorkCoordinator,
    id: u64,
) -> Result<Option<OwnedSemaphorePermit>, SchedulerError> {
    let acquire = semaphore.acquire_owned();
    tokio::pin!(acquire);
    let mut changes = coordinator.subscribe();
    let mut cancellation = coordinator.cancellation.subscribe();
    loop {
        if coordinator.should_cancel(id) {
            return Ok(None);
        }
        tokio::select! {
            result = &mut acquire => {
                return result
                    .map(Some)
                    .map_err(|_| SchedulerError::Coordination);
            }
            result = changes.changed() => {
                result.map_err(|_| SchedulerError::Coordination)?;
            }
            result = cancellation.changed() => {
                result.map_err(|_| SchedulerError::Coordination)?;
            }
        }
    }
}

async fn send_or_cancel(
    request: reqwest::RequestBuilder,
    coordinator: &WorkCoordinator,
    id: u64,
) -> Result<Option<Response>, SchedulerError> {
    let send = request.send();
    tokio::pin!(send);
    let mut changes = coordinator.subscribe();
    let mut cancellation = coordinator.cancellation.subscribe();
    loop {
        if coordinator.should_cancel(id) {
            return Ok(None);
        }
        tokio::select! {
            result = &mut send => {
                return result
                    .map(Some)
                    .map_err(|_| SchedulerError::Request);
            }
            result = changes.changed() => {
                result.map_err(|_| SchedulerError::Coordination)?;
            }
            result = cancellation.changed() => {
                result.map_err(|_| SchedulerError::Coordination)?;
            }
        }
    }
}

async fn next_chunk_or_cancel(
    response: &mut Response,
    coordinator: &WorkCoordinator,
    id: u64,
) -> Result<Option<bytes::Bytes>, SchedulerError> {
    let next = response.chunk();
    tokio::pin!(next);
    let mut changes = coordinator.subscribe();
    let mut cancellation = coordinator.cancellation.subscribe();
    loop {
        if coordinator.should_cancel(id) {
            return Ok(None);
        }
        tokio::select! {
            result = &mut next => {
                return result.map_err(|_| SchedulerError::Request);
            }
            result = changes.changed() => {
                result.map_err(|_| SchedulerError::Coordination)?;
            }
            result = cancellation.changed() => {
                result.map_err(|_| SchedulerError::Coordination)?;
            }
        }
    }
}

async fn acquire_with_cancellation(
    semaphore: Arc<Semaphore>,
    cancellation: &TransferCancellation,
) -> Result<OwnedSemaphorePermit, SchedulerError> {
    let permit = tokio::select! {
        result = semaphore.acquire_owned() => {
            result.map_err(|_| SchedulerError::Coordination)?
        }
        () = cancellation.cancelled() => return Err(SchedulerError::Cancelled),
    };
    if cancellation.is_cancelled() {
        return Err(SchedulerError::Cancelled);
    }
    Ok(permit)
}

async fn send_with_cancellation(
    request: reqwest::RequestBuilder,
    cancellation: &TransferCancellation,
) -> Result<Response, SchedulerError> {
    let response = tokio::select! {
        result = request.send() => result.map_err(|_| SchedulerError::Request)?,
        () = cancellation.cancelled() => return Err(SchedulerError::Cancelled),
    };
    if cancellation.is_cancelled() {
        return Err(SchedulerError::Cancelled);
    }
    Ok(response)
}

async fn next_single_chunk(
    response: &mut Response,
    cancellation: &TransferCancellation,
) -> Result<Option<bytes::Bytes>, SchedulerError> {
    let chunk = tokio::select! {
        result = response.chunk() => result.map_err(|_| SchedulerError::Request)?,
        () = cancellation.cancelled() => return Err(SchedulerError::Cancelled),
    };
    if cancellation.is_cancelled() {
        return Err(SchedulerError::Cancelled);
    }
    Ok(chunk)
}

fn commit_range(
    partial: &PartialFile,
    range: FileRange,
    bytes: &[u8],
) -> Result<(), SchedulerError> {
    let mut writer = partial.assign(range)?;
    writer.write(bytes)?;
    writer.finish()?;
    Ok(())
}

fn missing_ranges(total: u64, completed: &[FileRange]) -> Result<Vec<FileRange>, SchedulerError> {
    if completed.len() > MAX_COMPLETED_RANGES {
        return Err(SchedulerError::InvalidCompletedCoverage);
    }
    if total == 0 {
        return if completed.is_empty() {
            Ok(Vec::new())
        } else {
            Err(SchedulerError::InvalidCompletedCoverage)
        };
    }
    let mut missing = Vec::with_capacity(completed.len().saturating_add(1));
    let mut cursor = 0_u64;
    let mut previous_end = None;
    for range in completed {
        if previous_end.is_some_and(|end| range.start() <= end) || range.end() > total {
            return Err(SchedulerError::InvalidCompletedCoverage);
        }
        if range.start() > cursor {
            missing.push(
                FileRange::new(cursor, range.start())
                    .map_err(|_| SchedulerError::InvalidCompletedCoverage)?,
            );
        }
        cursor = range.end();
        previous_end = Some(cursor);
    }
    if cursor < total {
        missing.push(
            FileRange::new(cursor, total).map_err(|_| SchedulerError::InvalidCompletedCoverage)?,
        );
    }
    Ok(missing)
}

fn validate_request_plan(missing: &[FileRange], chunk_size: u64) -> Result<(), SchedulerError> {
    let request_count = missing.iter().try_fold(0_u64, |count, range| {
        let chunks = (range.len() - 1) / chunk_size + 1;
        count
            .checked_add(chunks)
            .ok_or(SchedulerError::RequestPlanTooLarge)
    })?;
    if request_count > MAX_RANGE_REQUESTS {
        return Err(SchedulerError::RequestPlanTooLarge);
    }
    Ok(())
}

fn request_chunk_size(missing_bytes: u64, workers: WorkerCount) -> u64 {
    let desired_chunks = u64::from(workers.get()) * CHUNKS_PER_WORKER;
    let ideal = missing_bytes
        .saturating_add(desired_chunks - 1)
        .checked_div(desired_chunks)
        .unwrap_or(MIN_REQUEST_BYTES);
    ideal.clamp(MIN_REQUEST_BYTES, MAX_REQUEST_BYTES)
}

fn pop_chunk(
    pending: &mut VecDeque<FileRange>,
    chunk_size: u64,
) -> Result<Option<FileRange>, SchedulerError> {
    let Some(gap) = pending.pop_front() else {
        return Ok(None);
    };
    let end = gap.start().saturating_add(chunk_size).min(gap.end());
    let chunk = FileRange::new(gap.start(), end).map_err(|_| SchedulerError::Coordination)?;
    if end < gap.end() {
        pending
            .push_front(FileRange::new(end, gap.end()).map_err(|_| SchedulerError::Coordination)?);
    }
    Ok(Some(chunk))
}

fn has_exact_coverage(completed: &[FileRange], total: u64) -> bool {
    if total == 0 {
        completed.is_empty()
    } else {
        completed.len() == 1 && completed[0].start() == 0 && completed[0].end() == total
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::{
        ConcurrencyLimits, DEFAULT_WORKERS, MAX_RANGE_REQUESTS, MAX_REQUEST_BYTES, MAX_WORKERS,
        MIN_REQUEST_BYTES, SchedulerConfigError, SchedulerError, SchedulerOptions, WorkerCount,
        missing_ranges, pop_chunk, request_chunk_size, validate_request_plan,
    };
    use crate::storage::FileRange;
    use std::collections::VecDeque;
    use std::time::Duration;

    fn range(start: u64, end: u64) -> FileRange {
        FileRange::new(start, end).expect("valid test range")
    }

    #[test]
    fn worker_counts_and_limits_are_strict() {
        assert_eq!(WorkerCount::default(), WorkerCount::Four);
        assert_eq!(WorkerCount::default().get(), DEFAULT_WORKERS);
        assert_eq!(WorkerCount::Eight.get(), MAX_WORKERS);
        assert_eq!(SchedulerOptions::default().limits().per_host(), 8);
        assert_eq!(SchedulerOptions::default().limits().global(), 16);
        for count in [1, 2, 4, 8] {
            assert_eq!(
                WorkerCount::try_from(count).expect("supported").get(),
                count
            );
        }
        for count in [0, 3, 5, 6, 7, 9, u8::MAX] {
            assert_eq!(
                WorkerCount::try_from(count),
                Err(SchedulerConfigError::InvalidWorkerCount)
            );
        }
        assert!(ConcurrencyLimits::new(1, 1).is_ok());
        assert_eq!(
            ConcurrencyLimits::new(0, 1),
            Err(SchedulerConfigError::InvalidConcurrencyLimits)
        );
        assert_eq!(
            ConcurrencyLimits::new(9, 16),
            Err(SchedulerConfigError::InvalidConcurrencyLimits)
        );
        assert_eq!(
            ConcurrencyLimits::new(8, 33),
            Err(SchedulerConfigError::InvalidConcurrencyLimits)
        );
        assert_eq!(
            SchedulerOptions::new(ConcurrencyLimits::default(), Duration::from_millis(9), 1,),
            Err(SchedulerConfigError::InvalidHedgeDelay)
        );
    }

    #[test]
    fn missing_ranges_and_lazy_chunks_cover_every_byte_once() {
        let missing = missing_ranges(100, &[range(10, 20), range(40, 60), range(90, 100)])
            .expect("plan gaps");
        assert_eq!(missing, [range(0, 10), range(20, 40), range(60, 90)]);
        assert_eq!(
            missing_ranges(100, &[range(0, 10), range(10, 20)]),
            Err(SchedulerError::InvalidCompletedCoverage)
        );
        let mut pending = VecDeque::from(missing);
        let mut chunks = Vec::new();
        while let Some(chunk) = pop_chunk(&mut pending, 7).expect("pop chunk") {
            chunks.push(chunk);
        }
        assert_eq!(
            chunks,
            [
                range(0, 7),
                range(7, 10),
                range(20, 27),
                range(27, 34),
                range(34, 40),
                range(60, 67),
                range(67, 74),
                range(74, 81),
                range(81, 88),
                range(88, 90),
            ]
        );
    }

    #[test]
    fn chunk_size_and_request_count_are_bounded() {
        assert_eq!(request_chunk_size(1, WorkerCount::Eight), MIN_REQUEST_BYTES);
        assert_eq!(
            request_chunk_size(u64::MAX, WorkerCount::One),
            MAX_REQUEST_BYTES
        );
        assert_eq!(
            request_chunk_size(64 * 1024 * 1024, WorkerCount::Four),
            4 * 1024 * 1024
        );
        assert!(validate_request_plan(&[range(0, MAX_REQUEST_BYTES)], MAX_REQUEST_BYTES).is_ok());
        assert_eq!(
            validate_request_plan(
                &[range(0, MAX_REQUEST_BYTES * (MAX_RANGE_REQUESTS + 1),)],
                MAX_REQUEST_BYTES,
            ),
            Err(SchedulerError::RequestPlanTooLarge)
        );
    }
}
