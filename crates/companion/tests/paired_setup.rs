#![cfg(all(windows, feature = "installed"))]
//! Actual application launch + filesystem installation; registration is memory-only.
//! No browser, shared registration, normal state or real XPI installation is used.
use download_manager_setup::{
    HELPER_FILE, SetupError,
    package::{Descriptor, PACKAGE_FILES, VerifiedPackage, file_hash},
    process::probe_application,
    registry::{RegistrationStore, RegistrationValue},
    transaction::{InstallProbe, SetupSession},
};
use std::{collections::BTreeMap, fs, path::Path};
const APP: &str = env!("CARGO_BIN_EXE_download-manager-app");
#[derive(Default)]
struct Registry(Option<RegistrationValue>);
impl RegistrationStore for Registry {
    fn current(&self) -> Result<Option<RegistrationValue>, SetupError> {
        Ok(self.0.clone())
    }
    fn replace_if_unchanged(
        &mut self,
        expected: Option<&RegistrationValue>,
        desired: Option<&RegistrationValue>,
    ) -> Result<(), SetupError> {
        if self.0.as_ref() != expected {
            return Err(SetupError::Registration);
        }
        self.0 = desired.cloned();
        Ok(())
    }
}
// Deliberately not production ApplicationProbe: no shared-registration preflight
// is bypassed because this test never calls a real registration store.
struct OwnedProbe;
impl InstallProbe for OwnedProbe {
    fn verify(&mut self, executable: &Path, local: &Path) -> Result<(), SetupError> {
        probe_application(executable, local)
    }
}
#[test]
fn actual_application_probes_before_registration_without_starting_an_engine() {
    let root = std::env::temp_dir().join(format!("dm-paired-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let local = root.join("Local");
    fs::create_dir(&local).unwrap();
    probe_application(Path::new(APP), &local).unwrap();
    assert!(
        fs::read_dir(&local).unwrap().next().is_none(),
        "metadata probe must not leave application state"
    );
    let source = root.join("Package");
    fs::create_dir(&source).unwrap();
    let mut files = BTreeMap::new();
    for name in PACKAGE_FILES {
        if name == HELPER_FILE {
            fs::copy(APP, source.join(name)).unwrap();
        } else {
            fs::write(
                source.join(name),
                b"public non-executable fixture, not a signed XPI",
            )
            .unwrap();
        }
        files.insert(name.to_owned(), file_hash(&source.join(name)).unwrap());
    }
    let descriptor = Descriptor {
        format: "firefox-download-manager-package".into(),
        version: 1,
        package_version: env!("CARGO_PKG_VERSION").into(),
        repository: "HalcyonXP/firefox-download-manager".into(),
        commit: "a".repeat(40),
        target: "x86_64-pc-windows-gnullvm".into(),
        files,
    };
    fs::write(
        source.join("package.json"),
        serde_json::to_vec(&descriptor).unwrap(),
    )
    .unwrap();
    let package = VerifiedPackage::open(&source).unwrap();
    let install = local.join("Host");
    let programs = root.join("Owned Programs");
    fs::create_dir(&programs).unwrap();
    let session = SetupSession::open(&install, &local)
        .unwrap()
        .with_shortcuts(
            download_manager_setup::shortcuts::ShortcutLocation::open(&programs).unwrap(),
        )
        .unwrap();
    let mut registry = Registry::default();
    let first = session
        .install(&package, &mut registry, &mut OwnedProbe)
        .unwrap();
    assert!(registry.0.is_some());
    assert!(session.root().join(&first).exists());
    let receipt = download_manager_setup::receipt::Receipt::decode(
        &fs::read(session.root().join("installation.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt.version, 2);
    let link = programs
        .join(format!("Download Manager {}", receipt.installation_id))
        .join(download_manager_setup::shortcuts::LINK);
    assert_eq!(
        fs::read(&link).unwrap(),
        fs::read(
            session
                .root()
                .join(&first)
                .join(download_manager_setup::shortcuts::LINK)
        )
        .unwrap()
    );
    session.repair(&mut registry, &mut OwnedProbe).unwrap();
    let second = session
        .install(&package, &mut registry, &mut OwnedProbe)
        .unwrap();
    assert_ne!(first, second);
    session.cleanup(&mut registry).unwrap();
    session.uninstall(&mut registry).unwrap();
    assert!(registry.0.is_none());
    assert!(!link.exists());
    assert!(fs::read_dir(&programs).unwrap().next().is_none());
    assert!(
        !local
            .join("HalcyonXP/FirefoxDownloadManager/state")
            .exists()
    );
    drop(session);
    drop(package);
    fs::remove_dir_all(root).unwrap(); // Only this exact successful fixture.
}
