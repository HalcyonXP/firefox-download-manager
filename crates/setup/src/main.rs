//! Local Windows setup entry point. No elevation or browser profile modification.
#![cfg_attr(all(windows, feature = "application"), windows_subsystem = "windows")]
#[cfg(all(windows, target_arch = "x86_64"))]
mod windows {
    #[cfg(not(feature = "application"))]
    use download_manager_setup::EXTENSION_FILE;
    use download_manager_setup::package::VerifiedPackage;
    use download_manager_setup::process::require_apps_closed;
    #[cfg(feature = "application")]
    use download_manager_setup::process::{
        ApplicationProbe as SelectedProbe, probe_application as selected_probe,
    };
    #[cfg(not(feature = "application"))]
    use download_manager_setup::process::{
        NativeProbe as SelectedProbe, probe_helper as selected_probe,
    };
    use download_manager_setup::registry::CurrentUserRegistration;
    use download_manager_setup::transaction::SetupSession;
    use download_manager_setup::{HELPER_FILE, SetupError};
    use std::path::PathBuf;

    struct Options {
        action: String,
        local: PathBuf,
        root: PathBuf,
        source: PathBuf,
    }
    fn options() -> Result<Option<Options>, SetupError> {
        let mut args = std::env::args_os().skip(1);
        let action = args.next().unwrap_or_else(|| "--help".into());
        if (action == "--help" || action == "--version") && args.next().is_none() {
            if action == "--help" {
                help();
            } else {
                println!("{}", env!("CARGO_PKG_VERSION"));
            }
            return Ok(None);
        }
        let action = action.to_str().ok_or(SetupError::Arguments)?;
        if ![
            "verify",
            "probe",
            "install",
            "cleanup",
            "uninstall",
            "recover",
            "repair",
        ]
        .contains(&action)
        {
            return Err(SetupError::Arguments);
        }
        let local = PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or(SetupError::Path)?);
        let mut root = None;
        let mut package = None;
        while let Some(key) = args.next() {
            let value = PathBuf::from(args.next().ok_or(SetupError::Arguments)?);
            match key.to_str() {
                Some("--root") if root.is_none() && !["verify", "probe"].contains(&action) => {
                    root = Some(value);
                }
                Some("--package")
                    if package.is_none() && ["verify", "probe", "install"].contains(&action) =>
                {
                    package = Some(value);
                }
                _ => return Err(SetupError::Arguments),
            }
        }
        let source = if let Some(source) = package {
            source
        } else {
            std::env::current_exe()
                .map_err(|_| SetupError::Package)?
                .parent()
                .ok_or(SetupError::Package)?
                .to_owned()
        };
        let root = root.unwrap_or_else(|| {
            local
                .join("HalcyonXP")
                .join("FirefoxDownloadManager")
                .join("host")
        });
        Ok(Some(Options {
            action: action.into(),
            local,
            root,
            source,
        }))
    }
    pub(super) fn execute() -> Result<(), SetupError> {
        let Some(options) = options()? else {
            return Ok(());
        };
        if ["verify", "probe"].contains(&options.action.as_str()) {
            let package = VerifiedPackage::open(&options.source)?;
            if options.action == "probe" {
                selected_probe(&package.payload(HELPER_FILE)?, &options.local)?;
            }
            println!(
                "Local package checks passed; no registration or browser profile was changed."
            );
            return Ok(());
        }
        require_apps_closed()?;
        let verified = if options.action == "install" {
            Some(VerifiedPackage::open(&options.source)?)
        } else {
            None
        };
        let mut registry = CurrentUserRegistration;
        let session = SetupSession::open(&options.root, &options.local)?;
        #[cfg(feature = "application")]
        let session = session
            .with_shortcuts(download_manager_setup::shortcuts::ShortcutLocation::current()?)?;
        match options.action.as_str() {
            "install" => {
                let generation = session.install(
                    verified.as_ref().ok_or(SetupError::Package)?,
                    &mut registry,
                    &mut SelectedProbe,
                )?;
                #[cfg(feature = "application")]
                println!(
                    "Paired application installed in generation {generation}. This development candidate does not qualify persistent Firefox capture or ordinary setup UI."
                );
                #[cfg(not(feature = "application"))]
                println!(
                    "Native host installed. In the installation root, load {generation}\\{EXTENSION_FILE} using Firefox about:debugging. Temporary extensions must be reloaded after Firefox restarts."
                );
            }
            "cleanup" => {
                session.cleanup(&mut registry)?;
                println!(
                    "Verified retired generation files removed; unknown files were preserved."
                );
            }
            "uninstall" => {
                session.uninstall(&mut registry)?;
                println!(
                    "Matching registration and verified program files removed. Task state, downloads and unknown files were preserved."
                );
            }
            "recover" => {
                session.recover(&mut registry)?;
                println!("Journal recovery completed; no task state or downloads were removed.");
            }
            "repair" => {
                session.repair(&mut registry, &mut SelectedProbe)?;
                println!(
                    "Verified current generation registered; no task state or downloads were changed."
                );
            }
            _ => return Err(SetupError::Arguments),
        }
        Ok(())
    }
    fn help() {
        println!(
            "Firefox Download Manager local setup\nCommands: verify, probe, install, cleanup, uninstall, recover, repair\nOptions: --package <absolute extracted package directory>, --root <absolute installation directory>\nDefault root: %LOCALAPPDATA%\\HalcyonXP\\FirefoxDownloadManager\\host\nClose Firefox and native helpers before install/cleanup/uninstall/recover/repair.\nVerify checks file consistency, not publisher authenticity. No browser profile or security setting is changed."
        );
    }
}
#[cfg(all(windows, target_arch = "x86_64"))]
fn main() {
    #[cfg(feature = "application")]
    if std::env::args_os().len() == 1 {
        if download_manager_setup::application_ui::run().is_err() {
            use winsafe::{co, prelude::*};
            let _ = winsafe::HWND::NULL.MessageBox("Setup could not open or complete its window. No successful installation is asserted.", "Download Manager setup", co::MB::ICONERROR);
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = windows::execute() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() {
    eprintln!("{}", download_manager_setup::SetupError::Unsupported);
    std::process::exit(1);
}
