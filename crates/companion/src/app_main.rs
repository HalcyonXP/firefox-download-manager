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
        HOST_NAME,
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
    // Class is pinned before discovery/authentication or caller-frame forwarding.
    let class = bridge_class(args, &manifest).ok_or(())?;
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
                connect_for_class(record.endpoint(), record.capability(), class),
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

#[cfg(windows)]
async fn connect_for_class(
    endpoint: download_manager_local_ipc::Endpoint,
    capability: &download_manager_local_ipc::Capability,
    class: download_manager_local_ipc::PeerClass,
) -> Result<
    download_manager_local_ipc::Channel<download_manager_local_ipc::LocalPipe>,
    download_manager_local_ipc::Error,
> {
    use download_manager_local_ipc::{PeerClass, connect, connect_browser_parent};
    match class {
        PeerClass::NativeBridge => connect(endpoint, capability).await,
        PeerClass::BrowserParent => connect_browser_parent(endpoint, capability).await,
    }
}

/// No caller-controlled native message or advertised capability selects this
/// class. The private parent launcher must supply this fixed argument shape.
#[cfg(windows)]
fn bridge_class(
    args: &[std::ffi::OsString],
    manifest: &std::path::Path,
) -> Option<download_manager_local_ipc::PeerClass> {
    use download_manager_local_ipc::PeerClass;
    use download_manager_setup::EXTENSION_ID;
    match args {
        [] => Some(PeerClass::NativeBridge),
        [id] if id == EXTENSION_ID => Some(PeerClass::NativeBridge),
        [path, id] if path == manifest.as_os_str() && id == EXTENSION_ID => {
            Some(PeerClass::NativeBridge)
        }
        [mode, path, id]
            if mode == "--browser-parent" && path == manifest.as_os_str() && id == EXTENSION_ID =>
        {
            Some(PeerClass::BrowserParent)
        }
        _ => None,
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::bridge_class;
    use download_manager_local_ipc::PeerClass;
    use download_manager_setup::EXTENSION_ID;
    use std::{ffi::OsString, path::Path};

    fn classify(args: &[&str]) -> Option<PeerClass> {
        let args: Vec<_> = args.iter().map(OsString::from).collect();
        bridge_class(&args, Path::new(r"C:\fixture\host.json"))
    }

    #[test]
    fn ordinary_firefox_and_redirected_test_arguments_never_select_parent_class() {
        for args in [
            vec![],
            vec![EXTENSION_ID],
            vec![r"C:\fixture\host.json", EXTENSION_ID],
        ] {
            assert_eq!(classify(&args), Some(PeerClass::NativeBridge));
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fixed_entry_routes_to_matching_authenticated_domain() {
        use download_manager_local_ipc::{Capability, Channel, Endpoint, Server};
        use std::sync::Arc;
        for (args, expected) in [
            (
                vec![r"C:\fixture\host.json", EXTENSION_ID],
                PeerClass::NativeBridge,
            ),
            (
                vec!["--browser-parent", r"C:\fixture\host.json", EXTENSION_ID],
                PeerClass::BrowserParent,
            ),
        ] {
            let endpoint = Endpoint::generate().unwrap();
            let key = Arc::new(Capability::generate().unwrap());
            let server = Server::bind(endpoint, Arc::clone(&key)).unwrap();
            let (s, c) = tokio::join!(
                server.accept_with_browser_parent(),
                super::connect_for_class(endpoint, &key, classify(&args).unwrap())
            );
            let observations = (
                s.as_ref().map(Channel::peer_class).ok(),
                c.as_ref().map(Channel::peer_class).ok(),
            );
            drop((s, c));
            let failed = server.cancellation_failed();
            drop(server);
            drop(Server::bind(endpoint, key).unwrap());
            assert!(!failed);
            assert_eq!(observations, (Some(expected), Some(expected)));
        }
    }

    #[test]
    fn parent_class_requires_exact_fixed_mode_manifest_and_extension() {
        assert_eq!(
            classify(&["--browser-parent", r"C:\fixture\host.json", EXTENSION_ID]),
            Some(PeerClass::BrowserParent)
        );
        for args in [
            vec!["--browser-parent"],
            vec!["--browser-parent", EXTENSION_ID],
            vec!["--browser-parent", r"C:\fixture\other.json", EXTENSION_ID],
            vec![
                "--browser-parent",
                r"C:\fixture\host.json",
                "foreign-extension",
            ],
            vec![
                "--browser-parent=true",
                r"C:\fixture\host.json",
                EXTENSION_ID,
            ],
            vec![r"C:\fixture\host.json", EXTENSION_ID, "--browser-parent"],
            vec![
                "--browser-parent",
                r"C:\fixture\host.json",
                EXTENSION_ID,
                "extra",
            ],
            vec![r#"{"peer_class":"browser_parent"}"#],
            vec![r"C:\fixture\host.json", "foreign-extension"],
        ] {
            assert_eq!(classify(&args), None);
        }
    }
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    ExitCode::FAILURE
}
