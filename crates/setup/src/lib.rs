//! Local setup boundaries. No networking, elevation or browser-profile API.
use thiserror::Error;

pub mod package;
#[cfg(windows)]
pub mod paths;
pub mod receipt;
#[cfg(windows)]
pub mod registry;

/// Stable package and registration identity.
pub const HOST_NAME: &str = "com.halcyonxp.firefox_download_manager";
/// The only extension allowed to contact this native host.
pub const EXTENSION_ID: &str = "download-manager@halcyonxp.local";
/// Fixed installed/source helper filename.
pub const HELPER_FILE: &str = "download-manager-native-host.exe";
/// Fixed packaged setup filename.
pub const SETUP_FILE: &str = "download-manager-setup.exe";
/// Fixed extension filename; setup never loads it into a live profile.
pub const EXTENSION_FILE: &str = "firefox-download-manager.xpi";

/// No supplied paths, metadata, registry values or error chains are displayed.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub enum SetupError {
    #[error("setup supports Windows 11 x64 only")]
    Unsupported,
    #[error("invalid setup command; use --help")]
    Arguments,
    #[error("package is incomplete, incompatible or failed checksum verification")]
    Package,
    #[error("installation path is unsafe, unavailable or outside local application data")]
    Path,
    #[error("an entry is not owned by this installation; no replacement is authorized")]
    Ownership,
    #[error("setup is already running, or an installation file is in use")]
    Busy,
    #[error("current-user registration belongs to another installation or is unavailable")]
    Registration,
    #[error("verified helper did not complete its isolated launch check")]
    Launch,
    #[error("installation needs explicit recovery; unknown content was preserved")]
    Recovery,
    #[error("setup filesystem operation failed")]
    Io,
}
