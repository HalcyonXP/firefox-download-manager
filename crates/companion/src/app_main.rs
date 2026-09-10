//! Paired application entry. Not selected by the legacy package builder yet.
#![cfg_attr(windows, windows_subsystem = "windows")]
use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    use download_manager_companion::{relay, windows};
    let args: Vec<_> = std::env::args_os().skip(1).take(4).collect();
    let result = match args.as_slice() {
        [mode] if mode == "--companion" => {
            let result = windows::run_installed();
            if result.is_err() {
                use winsafe::{co, prelude::*};
                let _ = winsafe::HWND::NULL.MessageBox(
                    "Manager could not complete startup or shutdown. State was preserved.",
                    "Download Manager",
                    co::MB::ICONERROR,
                );
            }
            result.map_err(|_| ())
        }
        [mode] if mode == download_manager_setup::application_probe::ARGUMENT => {
            use std::io::Write;
            download_manager_setup::application_probe::report()
                .map_err(|_| ())
                .and_then(|bytes| std::io::stdout().write_all(&bytes).map_err(|_| ()))
        }
        [mode] if mode == "--stdio-input" => relay::pump(false).map_err(|_| ()),
        [mode] if mode == "--stdio-output" => relay::pump(true).map_err(|_| ()),
        _ => bridge(&args),
    };
    if result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(windows)]
fn bridge(args: &[std::ffi::OsString]) -> Result<(), ()> {
    use download_manager_companion::relay;
    use download_manager_setup::{
        EXTENSION_ID, HOST_NAME,
        installed_image::InstalledImage,
        runtime_record::{RuntimeConnection, candidates},
    };
    use std::{
        io::IsTerminal,
        process::Stdio,
        time::{Duration, Instant},
    };
    if std::io::stdin().is_terminal() || std::io::stdout().is_terminal() {
        return Err(());
    }
    let executable = std::env::current_exe().map_err(|_| ())?;
    let manifest = executable
        .parent()
        .ok_or(())?
        .join(format!("{HOST_NAME}.json"));
    // Firefox platform arguments, or explicit redirected native-test stdio.
    let valid = args.is_empty()
        || (args.len() == 1 && args[0] == EXTENSION_ID)
        || (args.len() == 2 && args[0] == manifest.as_os_str() && args[1] == EXTENSION_ID);
    if !valid {
        return Err(());
    }
    let image = InstalledImage::open_current().map_err(|_| ())?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| ())?;
    let result = runtime.block_on(async {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut selected = None;
        for endpoint in candidates(&image).map_err(|_| ())? {
            if Instant::now() >= deadline {
                return Err(());
            }
            let Ok(record) = RuntimeConnection::open(&image, endpoint) else {
                continue;
            };
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(());
            }
            if let Ok(Ok(channel)) = tokio::time::timeout(
                remaining,
                download_manager_local_ipc::connect(record.endpoint(), record.capability()),
            )
            .await
            {
                selected = Some(channel);
                break;
            }
        }
        // Candidate attempts precede all native input dispatch. Once forwarding
        // starts there is no candidate retry, engine launch or uncertain replay.
        relay::forward(
            selected.ok_or(())?,
            &executable,
            Stdio::inherit(),
            Stdio::inherit(),
        )
        .await
        .map_err(|_| ())
    });
    drop(runtime); // Join runtime workers before reporting bridge completion.
    result
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    ExitCode::FAILURE
}
