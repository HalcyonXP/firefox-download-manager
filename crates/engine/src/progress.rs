//! Bounded progress sampling, smoothed speed, and conservative ETA estimates.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use thiserror::Error;

/// Largest exact integer representable by protocol-v2 JavaScript consumers.
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

const MIN_EVENT_INTERVAL: Duration = Duration::from_millis(100);
const MAX_EVENT_INTERVAL: Duration = Duration::from_secs(60);
const MIN_SPEED_WINDOW: Duration = Duration::from_secs(1);
const MAX_SPEED_WINDOW: Duration = Duration::from_secs(60);
const MIN_RATE_SPAN: Duration = Duration::from_millis(500);
const MAX_RATE_SAMPLES: usize = 256;
const STABILITY_RATIO: u64 = 4;

/// Invalid progress sampling configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProgressConfigError {
    /// Event interval was below 100 ms or above 60 seconds.
    #[error("progress event interval is outside supported bounds")]
    InvalidEventInterval,
    /// Speed window was below one second, above 60 seconds, or shorter than
    /// the event interval.
    #[error("progress speed window is outside supported bounds")]
    InvalidSpeedWindow,
}

/// Rate-limited event cadence and smoothed-rate window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressPolicy {
    event_interval: Duration,
    speed_window: Duration,
}

impl ProgressPolicy {
    /// Constructs bounded progress behavior.
    ///
    /// # Errors
    ///
    /// Rejects event intervals outside 100 ms through 60 seconds and speed
    /// windows outside one through 60 seconds or shorter than the event
    /// interval.
    pub fn new(
        event_interval: Duration,
        speed_window: Duration,
    ) -> Result<Self, ProgressConfigError> {
        if event_interval < MIN_EVENT_INTERVAL || event_interval > MAX_EVENT_INTERVAL {
            return Err(ProgressConfigError::InvalidEventInterval);
        }
        if speed_window < MIN_SPEED_WINDOW
            || speed_window > MAX_SPEED_WINDOW
            || speed_window < event_interval
        {
            return Err(ProgressConfigError::InvalidSpeedWindow);
        }
        Ok(Self {
            event_interval,
            speed_window,
        })
    }

    /// Minimum interval between ordinary progress events.
    #[must_use]
    pub const fn event_interval(self) -> Duration {
        self.event_interval
    }

    /// Sliding time window used for speed smoothing.
    #[must_use]
    pub const fn speed_window(self) -> Duration {
        self.speed_window
    }
}

impl Default for ProgressPolicy {
    fn default() -> Self {
        Self {
            event_interval: Duration::from_millis(250),
            speed_window: Duration::from_secs(5),
        }
    }
}

/// Smoothed speed and conservative ETA derived from an absolute sample.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ProgressEstimate {
    speed_bytes_per_second: Option<u64>,
    eta_seconds: Option<u64>,
    stable: bool,
}

impl ProgressEstimate {
    /// Smoothed byte rate after enough observation time, including zero when a
    /// sufficiently long window made no progress.
    #[must_use]
    pub const fn speed_bytes_per_second(self) -> Option<u64> {
        self.speed_bytes_per_second
    }

    /// Ceiling ETA for a known remaining size and stable positive rate.
    #[must_use]
    pub const fn eta_seconds(self) -> Option<u64> {
        self.eta_seconds
    }

    /// Whether interval rates were stable enough to expose an ETA.
    #[must_use]
    pub const fn is_stable(self) -> bool {
        self.stable
    }
}

#[derive(Debug, Clone, Copy)]
struct RateSample {
    at: Instant,
    bytes: u64,
}

/// Bounded sliding-window estimator over absolute byte counters.
#[derive(Debug)]
pub struct SpeedEstimator {
    window: Duration,
    samples: VecDeque<RateSample>,
}

impl SpeedEstimator {
    /// Creates an estimator for a validated one-through-60-second window.
    ///
    /// # Errors
    ///
    /// Rejects windows outside the supported bounds.
    pub fn new(window: Duration) -> Result<Self, ProgressConfigError> {
        if window < MIN_SPEED_WINDOW || window > MAX_SPEED_WINDOW {
            return Err(ProgressConfigError::InvalidSpeedWindow);
        }
        Ok(Self {
            window,
            samples: VecDeque::new(),
        })
    }

    /// Discards previous timing history, such as after pause or a stream
    /// restart from byte zero.
    pub fn reset(&mut self) {
        self.samples.clear();
    }

    /// Adds an absolute sample at `Instant::now()`.
    #[must_use]
    pub fn sample(&mut self, bytes: u64, expected_size: Option<u64>) -> ProgressEstimate {
        self.sample_at(bytes, expected_size, Instant::now())
    }

    /// Adds an absolute sample at an explicit monotonic instant.
    ///
    /// A backwards timestamp or byte counter starts a new window rather than
    /// producing a negative or inflated rate. ETA remains absent for unknown
    /// sizes, zero rates, and interval rates differing by more than 4×.
    #[must_use]
    pub fn sample_at(
        &mut self,
        bytes: u64,
        expected_size: Option<u64>,
        now: Instant,
    ) -> ProgressEstimate {
        if self
            .samples
            .back()
            .is_some_and(|sample| now < sample.at || bytes < sample.bytes)
        {
            self.samples.clear();
        }
        if self.samples.back().is_some_and(|sample| sample.at == now) {
            self.samples.pop_back();
        }
        self.samples.push_back(RateSample { at: now, bytes });
        self.trim(now);
        self.estimate(expected_size)
    }

    fn trim(&mut self, now: Instant) {
        let cutoff = now.checked_sub(self.window);
        while self.samples.len() > 2 && cutoff.is_some_and(|cutoff| self.samples[1].at < cutoff) {
            self.samples.pop_front();
        }
        while self.samples.len() > MAX_RATE_SAMPLES {
            self.samples.pop_front();
        }
    }

    fn estimate(&self, expected_size: Option<u64>) -> ProgressEstimate {
        let (Some(first), Some(last)) = (self.samples.front(), self.samples.back()) else {
            return ProgressEstimate::default();
        };
        if self.samples.len() < 3 {
            return ProgressEstimate {
                eta_seconds: completed_eta(expected_size, last.bytes),
                ..ProgressEstimate::default()
            };
        }
        let span = last.at.saturating_duration_since(first.at);
        if span < MIN_RATE_SPAN || span.is_zero() {
            return ProgressEstimate {
                eta_seconds: completed_eta(expected_size, last.bytes),
                ..ProgressEstimate::default()
            };
        }
        let delta = last.bytes.saturating_sub(first.bytes);
        let speed = rate(delta, span);

        let mut interval_rates = Vec::with_capacity(self.samples.len().saturating_sub(1));
        for (previous, current) in self.samples.iter().zip(self.samples.iter().skip(1)) {
            let interval = current.at.saturating_duration_since(previous.at);
            if !interval.is_zero() {
                interval_rates.push(rate(current.bytes.saturating_sub(previous.bytes), interval));
            }
        }
        let stable = rates_are_stable(&interval_rates);
        let eta_seconds = match expected_size {
            Some(expected) if last.bytes == expected => Some(0),
            Some(expected) if last.bytes < expected && speed > 0 && stable => {
                Some(ceil_div(expected - last.bytes, speed).min(MAX_SAFE_INTEGER))
            }
            _ => None,
        };
        ProgressEstimate {
            speed_bytes_per_second: Some(speed),
            eta_seconds,
            stable,
        }
    }
}

fn rate(bytes: u64, elapsed: Duration) -> u64 {
    let nanos = elapsed.as_nanos();
    if nanos == 0 {
        return 0;
    }
    let value = u128::from(bytes)
        .saturating_mul(1_000_000_000)
        .checked_div(nanos)
        .unwrap_or(0);
    u64::try_from(value.min(u128::from(MAX_SAFE_INTEGER))).unwrap_or(MAX_SAFE_INTEGER)
}

fn rates_are_stable(rates: &[u64]) -> bool {
    if rates.len() < 2 {
        return false;
    }
    let minimum = rates.iter().copied().min().unwrap_or(0);
    let maximum = rates.iter().copied().max().unwrap_or(0);
    if maximum == 0 {
        return true;
    }
    minimum > 0 && maximum <= minimum.saturating_mul(STABILITY_RATIO)
}

const fn completed_eta(expected_size: Option<u64>, bytes: u64) -> Option<u64> {
    match expected_size {
        Some(expected) if expected == bytes => Some(0),
        _ => None,
    }
}

fn ceil_div(numerator: u64, denominator: u64) -> u64 {
    let quotient = numerator / denominator;
    if numerator.is_multiple_of(denominator) {
        quotient
    } else {
        quotient + 1
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{ProgressConfigError, ProgressPolicy, SpeedEstimator};

    #[test]
    fn policy_bounds_event_and_smoothing_cadence() {
        assert_eq!(
            ProgressPolicy::new(Duration::from_millis(99), Duration::from_secs(5)),
            Err(ProgressConfigError::InvalidEventInterval)
        );
        assert_eq!(
            ProgressPolicy::new(Duration::from_secs(2), Duration::from_secs(1)),
            Err(ProgressConfigError::InvalidSpeedWindow)
        );
        assert!(ProgressPolicy::new(Duration::from_millis(100), Duration::from_secs(1)).is_ok());
    }

    #[test]
    fn smoothed_speed_and_eta_require_stable_known_progress() {
        let start = Instant::now();
        let mut estimator = SpeedEstimator::new(Duration::from_secs(5)).expect("estimator");
        assert_eq!(
            estimator
                .sample_at(0, Some(5_000), start)
                .speed_bytes_per_second(),
            None
        );
        let _ = estimator.sample_at(1_000, Some(5_000), start + Duration::from_secs(1));
        let estimate = estimator.sample_at(2_000, Some(5_000), start + Duration::from_secs(2));
        assert_eq!(estimate.speed_bytes_per_second(), Some(1_000));
        assert_eq!(estimate.eta_seconds(), Some(3));
        assert!(estimate.is_stable());

        let unknown = estimator.sample_at(3_000, None, start + Duration::from_secs(3));
        assert_eq!(unknown.speed_bytes_per_second(), Some(1_000));
        assert_eq!(unknown.eta_seconds(), None);
    }

    #[test]
    fn unstable_stalled_and_regressed_samples_hide_eta() {
        let start = Instant::now();
        let mut estimator = SpeedEstimator::new(Duration::from_secs(10)).expect("estimator");
        let _ = estimator.sample_at(0, Some(10_000), start);
        let _ = estimator.sample_at(100, Some(10_000), start + Duration::from_secs(1));
        let unstable = estimator.sample_at(5_100, Some(10_000), start + Duration::from_secs(2));
        assert!(unstable.speed_bytes_per_second().is_some());
        assert_eq!(unstable.eta_seconds(), None);
        assert!(!unstable.is_stable());

        estimator.reset();
        let _ = estimator.sample_at(5_100, Some(10_000), start + Duration::from_secs(3));
        let _ = estimator.sample_at(5_100, Some(10_000), start + Duration::from_secs(4));
        let stalled = estimator.sample_at(5_100, Some(10_000), start + Duration::from_secs(5));
        assert_eq!(stalled.speed_bytes_per_second(), Some(0));
        assert_eq!(stalled.eta_seconds(), None);

        let regressed = estimator.sample_at(10, Some(10_000), start + Duration::from_secs(6));
        assert_eq!(regressed.speed_bytes_per_second(), None);
        assert_eq!(regressed.eta_seconds(), None);
    }

    #[test]
    fn completion_reports_zero_eta_without_inventing_a_rate() {
        let mut estimator = SpeedEstimator::new(Duration::from_secs(5)).expect("estimator");
        let complete = estimator.sample(0, Some(0));
        assert_eq!(complete.speed_bytes_per_second(), None);
        assert_eq!(complete.eta_seconds(), Some(0));
    }
}
