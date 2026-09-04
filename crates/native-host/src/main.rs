//! Native Messaging host process entry point.

use download_manager_engine::Engine;

fn main() {
    // Standard output is reserved exclusively for framed Native Messaging.
    // The framing loop is introduced by issue #10; scaffolding must stay silent.
    let engine = Engine::new();
    debug_assert_eq!(engine.protocol_version(), 1);
}
