//! Internal local transport, not Native Messaging or installed endpoint authority.
//! Successful transport authentication proves capability possession only. It does
//! not authorize an add-on, establish command delivery, or make replay safe.
#![cfg(any(windows, test))]

mod auth;
mod frame;
#[cfg(windows)]
mod identity;
#[cfg(windows)]
mod windows;

pub use auth::{Capability, Endpoint, PeerClass};
pub use frame::{Channel, FrameReader, FrameWriter, MAX_FRAME};
#[cfg(windows)]
pub use identity::CurrentUser;
#[cfg(windows)]
pub use windows::{
    CancellationStatus, CancellationWatch, LocalPipe, MAX_CLIENTS, Server, connect,
    connect_browser_parent,
};

/// Fixed classifications only: never embed pipe names, SID, keys or peer input.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("local transport randomness unavailable")]
    Randomness,
    #[error("local endpoint refused")]
    Endpoint,
    #[error("local user identity unavailable")]
    Identity,
    #[error("local transport unavailable")]
    Transport,
    #[error("local transport capacity reached")]
    Busy,
    #[error("local transport deadline")]
    Deadline,
    #[error("local transport authentication refused")]
    Authentication,
    #[error("local transport frame refused")]
    Frame,
    #[error("local transport closed")]
    Closed,
}
