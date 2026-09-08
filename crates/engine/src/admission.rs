//! Shared HTTP admission, including probes and redirected probe hops.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use reqwest::Url;
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

use crate::scheduler::ConcurrencyLimits;

const MAX_ORIGINS: usize = 1024;
const MAX_SERVER_DELAY: u64 = 3600;
const PRESSURE_MEMORY: Duration = Duration::from_secs(60);

/// Bounded, origin-free admission failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum AdmissionError {
    /// Origin was invalid or the bounded origin table was exhausted.
    #[error("HTTP admission capacity is unavailable")]
    Unavailable,
    /// Server guidance cannot be honored within the supported bound.
    #[error("server retry guidance exceeds the safe wait bound")]
    ServerDelay,
}

/// One helper-wide global cap and exact-origin pressure state.
#[derive(Clone)]
pub struct Admission {
    inner: Arc<Inner>,
}

struct Inner {
    limits: ConcurrencyLimits,
    global: Arc<Semaphore>,
    origins: Mutex<HashMap<String, Arc<Origin>>>,
    active: AtomicUsize,
    peak: AtomicUsize,
}

struct Origin {
    state: Mutex<OriginState>,
    changed: watch::Sender<u64>,
}

struct OriginState {
    active: usize,
    limit: usize,
    not_before: Option<Instant>,
    retain_until: Instant,
    blocked: bool,
}

/// Ownership of an admitted request until its response is consumed or dropped.
/// Dropping an acquisition future or permit is the cancellation mechanism.
pub struct RequestPermit {
    inner: Arc<Inner>,
    origin: Arc<Origin>,
    _global: OwnedSemaphorePermit,
}

impl fmt::Debug for Admission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Admission")
            .field("limits", &self.inner.limits)
            .field("active", &self.active())
            .field("peak", &self.peak())
            .finish_non_exhaustive()
    }
}

impl Admission {
    /// Creates a bounded admission domain shared by all client paths.
    #[must_use]
    pub fn new(limits: ConcurrencyLimits) -> Self {
        Self {
            inner: Arc::new(Inner {
                limits,
                global: Arc::new(Semaphore::new(limits.global())),
                origins: Mutex::new(HashMap::new()),
                active: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
            }),
        }
    }

    /// Current locally admitted HTTP requests, including probes.
    #[must_use]
    pub fn active(&self) -> usize {
        self.inner.active.load(Ordering::Acquire)
    }

    /// Peak locally admitted HTTP requests, including probes.
    #[must_use]
    pub fn peak(&self) -> usize {
        self.inner.peak.load(Ordering::Acquire)
    }

    /// Waits without retaining a global slot while an origin is blocked/full.
    /// Rechecks origin state after acquiring a global slot to close races.
    ///
    /// # Errors
    /// Fails closed on exhausted origin capacity or unsupported server delay.
    pub async fn acquire(&self, url: &Url) -> Result<RequestPermit, AdmissionError> {
        let origin = self.origin(url)?;
        let mut changed = origin.changed.subscribe();
        loop {
            let (ready, deadline) = {
                let state = lock(&origin.state);
                if state.blocked {
                    return Err(AdmissionError::ServerDelay);
                }
                (
                    state.active < state.limit
                        && state
                            .not_before
                            .is_none_or(|deadline| Instant::now() >= deadline),
                    state.not_before,
                )
            };
            if ready {
                let global = self
                    .inner
                    .global
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| AdmissionError::Unavailable)?;
                let mut state = lock(&origin.state);
                if state.blocked {
                    return Err(AdmissionError::ServerDelay);
                }
                if state.active < state.limit
                    && state
                        .not_before
                        .is_none_or(|deadline| Instant::now() >= deadline)
                {
                    state.active += 1;
                    let active = self.inner.active.fetch_add(1, Ordering::AcqRel) + 1;
                    self.inner.peak.fetch_max(active, Ordering::AcqRel);
                    drop(state);
                    return Ok(RequestPermit {
                        inner: Arc::clone(&self.inner),
                        origin,
                        _global: global,
                    });
                }
                drop(state);
                drop(global);
                continue;
            }
            if let Some(deadline) = deadline.filter(|deadline| *deadline > Instant::now()) {
                tokio::select! {
                    () = tokio::time::sleep_until(deadline.into()) => {},
                    result = changed.changed() => {result.map_err(|_| AdmissionError::Unavailable)?;},
                }
            } else {
                changed
                    .changed()
                    .await
                    .map_err(|_| AdmissionError::Unavailable)?;
            }
        }
    }

    pub(crate) fn inherit_pressure(&self, previous: &Self) -> Result<(), AdmissionError> {
        if Arc::ptr_eq(&self.inner, &previous.inner) || self.active() != 0 || previous.active() != 0
        {
            return Err(AdmissionError::Unavailable);
        }
        let source = lock(&previous.inner.origins);
        let mut target = lock(&self.inner.origins);
        if !target.is_empty() {
            return Err(AdmissionError::Unavailable);
        }
        for (key, origin) in source.iter() {
            let state = lock(&origin.state);
            if state.active != 0 {
                return Err(AdmissionError::Unavailable);
            }
            if state.blocked || state.retain_until > Instant::now() {
                let (changed, _) = watch::channel(0);
                target.insert(
                    key.clone(),
                    Arc::new(Origin {
                        state: Mutex::new(OriginState {
                            active: 0,
                            limit: state.limit.min(self.inner.limits.per_host()),
                            not_before: state.not_before,
                            retain_until: state.retain_until,
                            blocked: state.blocked,
                        }),
                        changed,
                    }),
                );
            }
        }
        Ok(())
    }

    fn origin(&self, url: &Url) -> Result<Arc<Origin>, AdmissionError> {
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(AdmissionError::Unavailable);
        }
        let mut origins = lock(&self.inner.origins);
        origins.retain(|_, origin| {
            Arc::strong_count(origin) > 1 || {
                let state = lock(&origin.state);
                state.blocked || state.retain_until > Instant::now()
            }
        });
        let key = url.origin().ascii_serialization();
        if let Some(origin) = origins.get(&key) {
            return Ok(Arc::clone(origin));
        }
        if origins.len() >= MAX_ORIGINS {
            return Err(AdmissionError::Unavailable);
        }
        let (changed, _) = watch::channel(0);
        let origin = Arc::new(Origin {
            state: Mutex::new(OriginState {
                active: 0,
                limit: self.inner.limits.per_host(),
                not_before: None,
                retain_until: Instant::now(),
                blocked: false,
            }),
            changed,
        });
        origins.insert(key, Arc::clone(&origin));
        Ok(origin)
    }
}

impl RequestPermit {
    /// Records guidance before releasing this response's admission ownership.
    /// Existing requests may finish; subsequent origin requests see the cooldown
    /// and halved cap. Oversized guidance blocks the origin for this admission
    /// domain's lifetime rather than retrying early.
    pub fn observe(&self, status: u16, retry_after_seconds: Option<u64>) {
        if !matches!(status, 429 | 503) {
            return;
        }
        let mut state = lock(&self.origin.state);
        state.limit = (state.limit / 2).max(1);
        let seconds = retry_after_seconds.unwrap_or(1);
        if seconds > MAX_SERVER_DELAY {
            state.blocked = true;
        } else {
            let deadline = Instant::now() + Duration::from_secs(seconds);
            state.not_before = Some(state.not_before.map_or(deadline, |old| old.max(deadline)));
            state.retain_until = state.not_before.unwrap_or(deadline) + PRESSURE_MEMORY;
        }
        drop(state);
        self.origin
            .changed
            .send_modify(|version| *version = version.wrapping_add(1));
    }
}

impl Drop for RequestPermit {
    fn drop(&mut self) {
        lock(&self.origin.state).active -= 1;
        self.inner.active.fetch_sub(1, Ordering::AcqRel);
        self.origin
            .changed
            .send_modify(|version| *version = version.wrapping_add(1));
    }
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bounded_origin_table_retains_guidance_and_reclaims_only_idle_unpenalized_entries() {
        let admission = Admission::new(ConcurrencyLimits::default());
        // Retained blocked entries must not be evicted just to send early to a
        // penalized origin. The cap remains explicit even under hostile peers.
        for index in 0..MAX_ORIGINS {
            let target = Url::parse(&format!("https://o{index}.example.test/file")).expect("URL");
            let request = admission.acquire(&target).await.expect("bounded origin");
            request.observe(503, Some(MAX_SERVER_DELAY + 1));
        }
        let next = Url::parse("https://extra.example.test/file").expect("URL");
        assert!(matches!(
            admission.acquire(&next).await,
            Err(AdmissionError::Unavailable)
        ));
        let fresh = Admission::new(ConcurrencyLimits::default());
        for index in 0..=MAX_ORIGINS {
            let target = Url::parse(&format!("https://o{index}.example.test/file")).expect("URL");
            drop(fresh.acquire(&target).await.expect("idle origin reclaimed"));
        }
        assert_eq!(fresh.active(), 0);
    }
}
