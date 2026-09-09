//! Visible-shell policy and retained engine worker. No browser IPC or installer
//! integration is exposed by this initial development preview.
pub mod lifecycle;
#[cfg(windows)]
pub mod windows;
pub mod worker;
