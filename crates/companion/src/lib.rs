//! Visible shell and retained engine worker, with opt-in installed application
//! and native stdio forwarding. Package/browser qualification remains separate.
pub mod lifecycle;
#[cfg(windows)]
pub mod windows;
pub mod worker;

#[cfg(all(windows, feature = "installed"))]
pub mod relay;
