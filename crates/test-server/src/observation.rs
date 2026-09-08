//! Test-only barrier between complete request reception and ledger observation.
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Debug, Default)]
struct State {
    paused: bool,
    pending: usize,
}

#[derive(Debug, Default)]
pub(crate) struct ObservationGate {
    state: Mutex<State>,
    changed: Condvar,
}

impl ObservationGate {
    pub(crate) fn pause(self: &Arc<Self>) -> ObservationPause {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .paused = true;
        ObservationPause {
            gate: Arc::clone(self),
        }
    }

    pub(crate) fn arrive(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.paused {
            return;
        }
        state.pending += 1;
        self.changed.notify_all();
        state = self
            .changed
            .wait_while(state, |state| state.paused)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.pending -= 1;
    }

    pub(crate) fn release(&self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .paused = false;
        self.changed.notify_all();
    }
}

/// Holds fully received requests before they enter the observation ledger.
/// Dropping this guard releases the barrier. Use one pause at a time per server.
#[derive(Debug)]
pub struct ObservationPause {
    gate: Arc<ObservationGate>,
}

impl ObservationPause {
    /// Waits for at least `count` complete requests to reach the barrier.
    /// Returns false on timeout; does not guess readiness from a sleep.
    #[must_use]
    pub fn wait_for_pending(&self, count: usize, timeout: Duration) -> bool {
        let state = self
            .gate
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (state, _) = self
            .gate
            .changed
            .wait_timeout_while(state, timeout, |state| state.pending < count)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.pending >= count
    }
}

impl Drop for ObservationPause {
    fn drop(&mut self) {
        self.gate.release();
    }
}
