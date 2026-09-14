//! Minimal reviewed Windows cancellation boundary; see ADR0015.
#![cfg(windows)]

use std::{
    io,
    os::windows::io::{AsRawHandle, BorrowedHandle},
};
use windows_sys::Win32::{Foundation::ERROR_NOT_FOUND, System::IO::CancelIoEx};

/// Request cancellation of this process's pending I/O on the borrowed handle.
/// No pending request is a successful no-op. This is NOT a completion wait:
/// callers must keep I/O buffers owned until their runtime retires the requests,
/// join their workers, and independently observe failed cleanup.
///
/// # Errors
/// Returns the OS error if cancellation could not be requested. Callers must map
/// it to a fixed classification rather than attach paths/peer input to a log.
#[allow(unsafe_code)] // Sole reviewed FFI exception, ADR0015.
pub fn request_cancellation(handle: BorrowedHandle<'_>) -> io::Result<()> {
    // SAFETY: BorrowedHandle guarantees a valid, retained handle for this call.
    // NULL selects all operations for the handle; no OVERLAPPED/buffer pointers
    // are borrowed or dereferenced here, nor is handle ownership transferred.
    let succeeded = unsafe { CancelIoEx(handle.as_raw_handle(), std::ptr::null()) };
    if succeeded != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == i32::try_from(ERROR_NOT_FOUND).ok() {
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::windows::{io::AsHandle, process::CommandExt},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    #[test]
    fn valid_owned_non_io_handle_reports_cancellation_failure() {
        // An owned test-listing child, not an invalid/raw/fabricated handle or
        // another process discovered by PID. Observe failure without logging it.
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let refused = request_cancellation(child.as_handle()).is_err();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut normal = false;
        while Instant::now() < deadline {
            if let Ok(Some(status)) = child.try_wait() {
                normal = status.success();
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if !matches!(child.try_wait(), Ok(Some(_))) {
            let _ = child.kill();
        }
        let joined = child.wait().is_ok();
        assert!(joined && normal && refused);
    }
}
