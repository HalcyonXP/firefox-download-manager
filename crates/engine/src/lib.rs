//! Native download engine boundary.

pub mod network;
pub mod persistence;
pub mod progress;
pub mod scheduler;
pub mod storage;
pub mod task;

use download_manager_protocol::PROTOCOL_VERSION;

/// Engine facade owned by the native host.
#[derive(Debug, Default)]
pub struct Engine;

impl Engine {
    /// Creates an engine facade without starting network or disk work.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Reports the wire major this engine was compiled against.
    #[must_use]
    pub const fn protocol_version(&self) -> u16 {
        PROTOCOL_VERSION
    }
}

#[cfg(test)]
mod tests {
    use super::Engine;

    #[test]
    fn engine_uses_protocol_v2() {
        assert_eq!(Engine::new().protocol_version(), 2);
    }
}
