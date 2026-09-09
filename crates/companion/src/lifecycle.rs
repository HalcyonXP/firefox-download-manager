//! Pure visibility/quit policy; OS registration and worker joins supply facts.

/// Companion lifetime, separate from any browser transport.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    /// No engine may start before tray registration is confirmed.
    WaitingForTray,
    /// An owned worker is starting.
    Starting,
    /// An owned engine is running.
    Running,
    /// Quit has been requested; the worker must still be joined.
    Stopping,
    /// A failure is visible and requires deliberate acknowledgement.
    Failed,
    /// All owned workers have been joined; the shell may close.
    Stopped,
}

/// Model facts, not a claim of OS visibility. No URLs, paths or task data.
#[derive(Debug)]
pub struct Lifecycle {
    phase: Phase,
    tray_confirmed: bool,
    failed: bool,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self {
            phase: Phase::WaitingForTray,
            tray_confirmed: false,
            failed: false,
        }
    }
}

impl Lifecycle {
    /// Records the actual registration/confirmation result; returns true only
    /// once when an engine is now allowed to start.
    pub fn tray_observed(&mut self, confirmed: bool) -> bool {
        self.tray_confirmed = confirmed;
        if confirmed && self.phase == Phase::WaitingForTray {
            self.phase = Phase::Starting;
            return true;
        }
        false
    }

    /// Records worker startup, but never reverses a pending Quit.
    pub fn started(&mut self) {
        if self.phase == Phase::Starting {
            self.phase = Phase::Running;
        }
    }

    /// Requests shutdown exactly once; never implies that joining is complete.
    pub fn quit(&mut self) {
        if self.phase != Phase::Stopped {
            self.phase = Phase::Stopping;
        }
    }

    /// Records a completed join, not merely a sent cancellation request.
    pub fn joined(&mut self, successful: bool) {
        if successful && self.phase == Phase::Stopping {
            self.phase = Phase::Stopped;
        } else {
            self.phase = Phase::Failed;
            self.failed = true;
        }
    }

    /// Whether hiding the status window still leaves a confirmed tray surface.
    #[must_use]
    pub fn can_hide(&self) -> bool {
        self.tray_confirmed && self.phase == Phase::Running
    }

    /// Current lifetime phase.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// Failure is sticky even after the user acknowledges it and quits.
    #[must_use]
    pub const fn failed(&self) -> bool {
        self.failed
    }

    /// Fixed user-facing text; never derived from error strings or input data.
    #[must_use]
    pub fn status(&self) -> &'static str {
        match self.phase {
            Phase::WaitingForTray => "Tray unavailable. The engine has not started.",
            Phase::Starting => "Starting the isolated preview engine...",
            Phase::Running if !self.tray_confirmed => {
                "Tray unavailable. This status window stays visible."
            }
            Phase::Running => "Engine running. Firefox bridge is not connected in this preview.",
            Phase::Stopping => "Stopping and joining engine work. Please wait...",
            Phase::Failed => "The preview failed. Private state is retained; you may quit.",
            Phase::Stopped => "Engine joined. Preview stopped.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_gates_start_and_loss_never_starts_a_second_owner() {
        let mut state = Lifecycle::default();
        assert!(!state.tray_observed(false));
        assert_eq!(state.phase(), Phase::WaitingForTray);
        assert!(!state.can_hide());
        assert!(state.tray_observed(true));
        assert!(!state.can_hide());
        state.started();
        assert!(state.can_hide());
        assert!(!state.tray_observed(false));
        assert!(!state.can_hide());
        assert!(!state.tray_observed(true));
        assert!(state.can_hide());
    }

    #[test]
    fn quit_is_sticky_and_join_is_required() {
        let mut state = Lifecycle::default();
        assert!(state.tray_observed(true));
        state.quit();
        state.started();
        assert_eq!(state.phase(), Phase::Stopping);
        assert!(!state.tray_observed(true));
        assert!(!state.can_hide());
        state.quit();
        assert_eq!(state.phase(), Phase::Stopping);
        state.joined(true);
        assert_eq!(state.phase(), Phase::Stopped);
        assert!(!state.tray_observed(true));
    }

    #[test]
    fn unexpected_exit_and_failed_join_remain_visible_until_acknowledged() {
        let mut state = Lifecycle::default();
        state.tray_observed(true);
        state.started();
        state.joined(true); // unexpected worker exit is not a successful Quit
        assert_eq!(state.phase(), Phase::Failed);
        assert!(state.failed());
        assert!(!state.can_hide());
        state.quit();
        state.joined(false);
        assert_eq!(state.phase(), Phase::Failed);
        state.quit();
        state.joined(true); // already joined; explicit acknowledgement
        assert_eq!(state.phase(), Phase::Stopped);
        assert!(state.failed());
    }
}
