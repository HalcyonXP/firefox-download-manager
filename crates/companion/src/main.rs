//! Development entry point, deliberately excluded from release package payloads.
#![cfg_attr(windows, windows_subsystem = "windows")]
use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    use winsafe::{co, prelude::*};
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--preview")) || args.next().is_some() {
        let _ = winsafe::HWND::NULL.MessageBox(
            "This is an unfinished development build, not an installer. No installed state was opened.",
            "Download Manager - not installed",
            co::MB::ICONINFORMATION,
        );
        return ExitCode::FAILURE;
    }
    if download_manager_companion::windows::run_preview().is_err() {
        let _ = winsafe::HWND::NULL.MessageBox(
            "The isolated companion preview stopped with an error. Private state was preserved; no success is claimed.",
            "Download Manager - preview error",
            co::MB::ICONERROR,
        );
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!("The companion preview requires Windows.");
    ExitCode::FAILURE
}
