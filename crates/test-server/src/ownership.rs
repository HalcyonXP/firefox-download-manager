//! Retained fixture threads/sockets. A stop request is not a joined retirement.
use std::io;
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::observation::ObservationGate;
use crate::response::ResponseGate;
use crate::{SharedState, lock_state};

#[derive(Debug, Default)]
pub(crate) struct Stop {
    requested: AtomicBool,
    lock: Mutex<()>,
    changed: Condvar,
}

impl Stop {
    pub(crate) fn requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    pub(crate) fn request(&self) {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.requested.store(true, Ordering::Release);
        self.changed.notify_all();
    }

    // Preserve the configured stall while live; retirement wakes the fixture,
    // rather than extending deadlines or leaving a sleeping detached handler.
    pub(crate) fn wait(&self, duration: Duration) -> bool {
        let guard = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (_guard, _) = self
            .changed
            .wait_timeout_while(guard, duration, |()| !self.requested())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.requested()
    }
}

struct Connection {
    socket: TcpStream,
    thread: Option<JoinHandle<()>>,
}

pub(crate) struct Connections {
    entries: Vec<Connection>,
    state: Arc<Mutex<SharedState>>,
    stop: Arc<Stop>,
    observation: Arc<ObservationGate>,
    response: Arc<ResponseGate>,
    failed: bool,
    finished: bool,
}

impl Connections {
    pub(crate) fn new(
        state: Arc<Mutex<SharedState>>,
        stop: Arc<Stop>,
        observation: Arc<ObservationGate>,
        response: Arc<ResponseGate>,
    ) -> Self {
        Self {
            entries: Vec::new(),
            state,
            stop,
            observation,
            response,
            failed: false,
            finished: false,
        }
    }

    pub(crate) fn spawn(
        &mut self,
        socket: TcpStream,
        action: impl FnOnce(TcpStream) + Send + 'static,
    ) -> io::Result<()> {
        // The independent owned socket is retained BEFORE thread creation. No
        // PID, borrowed native handle or discovered connection is adopted.
        self.entries.push(Connection {
            socket: socket.try_clone()?,
            thread: None,
        });
        let entry = self.entries.last_mut().expect("retained connection slot");
        entry.thread = Some(
            thread::Builder::new()
                .name("adversarial-http-connection".to_owned())
                .spawn(move || action(socket))?,
        );
        lock_state(&self.state).connections_started += 1;
        Ok(())
    }

    fn join(&mut self, mut entry: Connection) {
        if let Some(handle) = entry.thread.take() {
            self.failed |= handle.join().is_err();
            lock_state(&self.state).connections_joined += 1;
        }
    }

    pub(crate) fn reap(&mut self) -> io::Result<()> {
        let mut index = 0;
        while index < self.entries.len() {
            if self.entries[index]
                .thread
                .as_ref()
                .is_some_and(JoinHandle::is_finished)
            {
                let entry = self.entries.swap_remove(index);
                self.join(entry);
            } else {
                index += 1;
            }
        }
        self.result().map(|_| ())
    }

    fn result(&self) -> io::Result<usize> {
        if self.failed {
            Err(io::Error::other("owned fixture handler panicked"))
        } else {
            Ok(lock_state(&self.state).connections_joined)
        }
    }

    pub(crate) fn finish(&mut self) -> io::Result<usize> {
        if !self.finished {
            self.finished = true;
            self.stop.request();
            self.observation.release();
            self.response.release();
            for entry in &self.entries {
                // Shutdown only interrupts I/O; success still requires joining.
                // Each socket remains owned until its corresponding join.
                let _ = entry.socket.shutdown(Shutdown::Both);
            }
            while let Some(entry) = self.entries.pop() {
                self.join(entry);
            }
        }
        self.result()
    }
}

impl Drop for Connections {
    fn drop(&mut self) {
        // Also runs when listener code unwinds after retaining partial slots.
        let _ = self.finish();
    }
}

#[cfg(test)]
#[path = "ownership_unit_tests.rs"]
mod tests;
