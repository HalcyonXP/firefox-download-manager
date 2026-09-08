//! Test-only barrier after ledger insertion, before a selected response starts.
use std::io;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::{ObservedRequest, RequestSelector};

#[derive(Debug, Default)]
struct State {
    selector: Option<RequestSelector>,
    pending: usize,
}

#[derive(Debug, Default)]
pub(crate) struct ResponseGate {
    state: Mutex<State>,
    changed: Condvar,
}

impl ResponseGate {
    pub(crate) fn pause(self: &Arc<Self>, selector: RequestSelector) -> io::Result<ResponsePause> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.selector.is_some() || state.pending != 0 {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "response pause is still active",
            ));
        }
        state.selector = Some(selector);
        Ok(ResponsePause {
            gate: Arc::clone(self),
        })
    }

    pub(crate) fn arrive(&self, request: &ObservedRequest) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state
            .selector
            .as_ref()
            .is_some_and(|selector| selector.matches(request))
        {
            return;
        }
        state.pending += 1;
        self.changed.notify_all();
        state = self
            .changed
            .wait_while(state, |state| state.selector.is_some())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.pending -= 1;
        self.changed.notify_all();
    }

    pub(crate) fn release(&self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .selector = None;
        self.changed.notify_all();
    }
}

/// Holds matching requests after ledger insertion but before response headers/body.
/// Other requests proceed normally. Dropping this guard or the server releases it.
#[derive(Debug)]
pub struct ResponsePause {
    gate: Arc<ResponseGate>,
}

impl ResponsePause {
    /// Waits for at least `count` selected requests to reach the barrier.
    /// Returns false on timeout, rather than estimating readiness from a sleep.
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

impl Drop for ResponsePause {
    fn drop(&mut self) {
        self.gate.release();
    }
}
