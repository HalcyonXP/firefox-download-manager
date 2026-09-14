#![cfg(windows)]
//! Real filesystem/transaction tests with deliberately memory-only registration and launch adapters.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use download_manager_setup::package::{Descriptor, PACKAGE_FILES, VerifiedPackage, file_hash};
use download_manager_setup::registry::{RegistrationStore, RegistrationValue};
use download_manager_setup::transaction::{InstallProbe, Phase, SetupSession};
use download_manager_setup::{EXTENSION_FILE, HELPER_FILE, SetupError};

struct Fixture {
    root: PathBuf,
    local: PathBuf,
    install: PathBuf,
    source: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("dm27 {}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).expect("own root");
        let local = root.join("Local Data");
        let source = root.join("Package With Spaces");
        fs::create_dir(&local).expect("local");
        fs::create_dir(&source).expect("source");
        let install = local.join("Installed Host With Spaces");
        let mut files = BTreeMap::new();
        for name in PACKAGE_FILES {
            fs::write(source.join(name), b"synthetic payload, not an executable").expect("payload");
            files.insert(
                name.to_owned(),
                file_hash(&source.join(name)).expect("digest"),
            );
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
            serde_json::to_vec(&descriptor).expect("descriptor"),
        )
        .expect("write");
        Self {
            root,
            local,
            install,
            source,
        }
    }
    fn session(&self) -> SetupSession {
        SetupSession::open(&self.install, &self.local).expect("session")
    }
    fn package(&self) -> VerifiedPackage {
        VerifiedPackage::open(&self.source).expect("synthetic package")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
#[derive(Default)]
struct Registry {
    value: Rc<RefCell<Option<RegistrationValue>>>,
    fail_write: bool,
}
impl RegistrationStore for Registry {
    fn current(&self) -> Result<Option<RegistrationValue>, SetupError> {
        Ok(self.value.borrow().clone())
    }
    fn replace_if_unchanged(
        &mut self,
        expected: Option<&RegistrationValue>,
        desired: Option<&RegistrationValue>,
    ) -> Result<(), SetupError> {
        if self.value.borrow().as_ref() != expected || self.fail_write {
            return Err(SetupError::Registration);
        }
        *self.value.borrow_mut() = desired.cloned();
        Ok(())
    }
}
#[derive(Default)]
struct Probe {
    fail: Option<Phase>,
    crash: Option<Phase>,
    launch_fail: bool,
    stages: Vec<Phase>,
}
impl InstallProbe for Probe {
    fn verify(&mut self, _: &Path, _: &Path) -> Result<(), SetupError> {
        if self.launch_fail {
            Err(SetupError::Launch)
        } else {
            Ok(())
        }
    }
    fn checkpoint(&mut self, phase: Phase) -> Result<(), SetupError> {
        self.stages.push(phase);
        assert!(
            self.crash != Some(phase),
            "simulated abrupt coordinator loss"
        );
        if self.fail == Some(phase) {
            Err(SetupError::Io)
        } else {
            Ok(())
        }
    }
}

#[test]
fn install_upgrade_cleanup_uninstall_preserve_state_downloads_and_unknown_files() {
    let fixture = Fixture::new();
    let session = fixture.session();
    let package = fixture.package();
    let mut registry = Registry::default();
    let state = fixture
        .local
        .join(r"HalcyonXP\FirefoxDownloadManager\state");
    fs::create_dir(&state).expect("state");
    fs::write(
        state.join("task.json"),
        br#"{"format_version":3,"synthetic":"preserve verbatim"}"#,
    )
    .expect("state bytes");
    let download = fixture.root.join("Completed Download.bin");
    fs::write(&download, b"completed unrelated bytes").expect("download");
    let old = session
        .install(&package, &mut registry, &mut Probe::default())
        .expect("install");
    assert!(registry.current().expect("current").is_some());
    let old_registration = registry.current().expect("old");
    fs::write(
        session.root().join(&old).join("user-note.txt"),
        b"keep this unknown file",
    )
    .expect("unknown");
    let new = session
        .install(&package, &mut registry, &mut Probe::default())
        .expect("upgrade");
    assert_ne!(old, new);
    assert!(registry.current().expect("new") != old_registration);
    assert_eq!(
        fs::read(session.root().join(&new).join(HELPER_FILE)).expect("helper"),
        b"synthetic payload, not an executable"
    );
    session.cleanup(&mut registry).expect("cleanup");
    assert!(!session.root().join(&old).join(HELPER_FILE).exists());
    assert!(session.root().join(&old).join("user-note.txt").exists());
    session.uninstall(&mut registry).expect("uninstall");
    assert!(registry.current().expect("removed").is_none());
    assert!(!session.root().join(&new).join(EXTENSION_FILE).exists());
    assert!(!session.root().join("installation.json").exists());
    assert_eq!(
        fs::read(state.join("task.json")).expect("preserved"),
        br#"{"format_version":3,"synthetic":"preserve verbatim"}"#
    );
    assert_eq!(
        fs::read(download).expect("preserved"),
        b"completed unrelated bytes"
    );
}

#[test]
fn every_install_failure_boundary_rolls_back_without_exposing_new_registration() {
    for phase in [
        Phase::Journal,
        Phase::Directory,
        Phase::Helper,
        Phase::Extension,
        Phase::Manifest,
        Phase::Probe,
        Phase::Registration,
        Phase::Receipt,
    ] {
        let fixture = Fixture::new();
        let session = fixture.session();
        let package = fixture.package();
        let mut registry = Registry::default();
        let old = session
            .install(&package, &mut registry, &mut Probe::default())
            .expect("baseline");
        let old_registration = registry.current().expect("old");
        let old_receipt = fs::read(session.root().join("installation.json")).expect("old receipt");
        let mut probe = Probe {
            fail: Some(phase),
            ..Probe::default()
        };
        assert!(
            session
                .install(&package, &mut registry, &mut probe)
                .is_err()
        );
        assert!(registry.current().expect("rolled back") == old_registration);
        assert_eq!(
            fs::read(session.root().join("installation.json")).expect("receipt"),
            old_receipt
        );
        assert!(session.root().join(old).join(HELPER_FILE).exists());
        assert!(!session.root().join("transaction.json").exists());
    }
}

#[test]
fn interrupted_activation_recovers_prior_generation_not_unverified_new_authority() {
    for phase in [Phase::Helper, Phase::Registration, Phase::Receipt] {
        let fixture = Fixture::new();
        let package = fixture.package();
        let mut registry = Registry::default();
        let session = fixture.session();
        session
            .install(&package, &mut registry, &mut Probe::default())
            .expect("baseline");
        let old = registry.current().expect("old");
        let mut probe = Probe {
            crash: Some(phase),
            ..Probe::default()
        };
        assert!(
            catch_unwind(AssertUnwindSafe(|| session.install(
                &package,
                &mut registry,
                &mut probe
            )))
            .is_err()
        );
        assert!(session.root().join("transaction.json").exists());
        drop(session);
        fixture
            .session()
            .recover(&mut registry)
            .expect("recover from durable journal");
        assert!(registry.current().expect("restored") == old);
    }
}

#[test]
fn foreign_registration_unknown_roots_and_concurrent_setup_fail_closed() {
    let fixture = Fixture::new();
    let package = fixture.package();
    let session = fixture.session();
    assert!(matches!(
        SetupSession::open(&fixture.install, &fixture.local),
        Err(SetupError::Busy)
    ));
    let foreign =
        RegistrationValue::for_manifest(Path::new(r"C:\Foreign\manifest.json")).expect("foreign");
    let mut registry = Registry::default();
    *registry.value.borrow_mut() = Some(foreign.clone());
    assert!(matches!(
        session.install(&package, &mut registry, &mut Probe::default()),
        Err(SetupError::Registration)
    ));
    assert!(registry.current().expect("preserved") == Some(foreign));
    *registry.value.borrow_mut() = None;
    fs::write(session.root().join("unknown.txt"), b"do not overwrite").expect("unknown");
    assert!(matches!(
        session.install(&package, &mut registry, &mut Probe::default()),
        Err(SetupError::Ownership)
    ));
    assert_eq!(
        fs::read(session.root().join("unknown.txt")).expect("preserved"),
        b"do not overwrite"
    );
}

#[test]
fn failed_launch_registration_and_changed_owned_bytes_never_get_silent_cleanup() {
    let fixture = Fixture::new();
    let package = fixture.package();
    let session = fixture.session();
    let mut registry = Registry::default();
    let mut probe = Probe {
        launch_fail: true,
        ..Probe::default()
    };
    assert!(matches!(
        session.install(&package, &mut registry, &mut probe),
        Err(SetupError::Launch)
    ));
    assert!(registry.current().expect("no registration").is_none());
    assert!(!session.root().join("transaction.json").exists());
    registry.fail_write = true;
    assert!(
        session
            .install(&package, &mut registry, &mut Probe::default())
            .is_err()
    );
    assert!(!session.root().join("transaction.json").exists());
    registry.fail_write = false;
    let current = session
        .install(&package, &mut registry, &mut Probe::default())
        .expect("install");
    let before = registry.current().expect("registered");
    fs::write(
        session.root().join(current).join(HELPER_FILE),
        b"changed unknown bytes",
    )
    .expect("corrupt");
    assert!(session.uninstall(&mut registry).is_err());
    assert!(registry.current().expect("not unregistered") == before);
}

#[test]
fn repair_only_rebinds_verified_known_generations() {
    let fixture = Fixture::new();
    let session = fixture.session();
    let package = fixture.package();
    let mut registry = Registry::default();
    session
        .install(&package, &mut registry, &mut Probe::default())
        .expect("first");
    let old = registry.current().expect("old");
    session
        .install(&package, &mut registry, &mut Probe::default())
        .expect("second");
    let current = registry.current().expect("current");
    *registry.value.borrow_mut() = old;
    session
        .repair(&mut registry, &mut Probe::default())
        .expect("repair known stale generation");
    assert!(registry.current().expect("repaired") == current);
    *registry.value.borrow_mut() = Some(
        RegistrationValue::for_manifest(Path::new(r"C:\Other\manifest.json")).expect("foreign"),
    );
    assert!(
        session
            .repair(&mut registry, &mut Probe::default())
            .is_err()
    );
}

#[test]
fn namespace_changes_after_staging_are_not_overwritten_by_rollback() {
    struct Change {
        registry: Rc<RefCell<Option<RegistrationValue>>>,
        foreign: RegistrationValue,
    }
    impl InstallProbe for Change {
        fn verify(&mut self, _: &Path, _: &Path) -> Result<(), SetupError> {
            *self.registry.borrow_mut() = Some(self.foreign.clone());
            Ok(())
        }
    }
    let fixture = Fixture::new();
    let session = fixture.session();
    let package = fixture.package();
    let mut registry = Registry::default();
    let foreign =
        RegistrationValue::for_manifest(Path::new(r"C:\Other\manifest.json")).expect("foreign");
    let mut probe = Change {
        registry: registry.value.clone(),
        foreign: foreign.clone(),
    };
    assert!(matches!(
        session.install(&package, &mut registry, &mut probe),
        Err(SetupError::Recovery)
    ));
    assert!(registry.current().expect("preserved") == Some(foreign));
    assert!(session.root().join("transaction.json").exists());
}

#[test]
fn torn_staged_bytes_and_locked_installed_files_are_preserved() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let session = fixture.session();
    let package = fixture.package();
    let mut registry = Registry::default();
    let current = session
        .install(&package, &mut registry, &mut Probe::default())
        .expect("baseline");
    let before = registry.current().expect("registered");
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(session.root().join(&current).join(HELPER_FILE))
        .expect("exclusive competing I/O");
    assert!(session.uninstall(&mut registry).is_err());
    assert!(registry.current().expect("preserved") == before);
    drop(handle);
    let mut probe = Probe {
        crash: Some(Phase::Helper),
        ..Probe::default()
    };
    assert!(
        catch_unwind(AssertUnwindSafe(|| session.install(
            &package,
            &mut registry,
            &mut probe
        )))
        .is_err()
    );
    let journal: serde_json::Value = serde_json::from_slice(
        &fs::read(session.root().join("transaction.json")).expect("journal"),
    )
    .expect("JSON");
    let new = journal["next"]["current"].as_str().expect("new ID");
    let staged = session.root().join(new).join(HELPER_FILE);
    fs::write(&staged, b"torn or changed bytes").expect("torn fixture");
    assert!(session.recover(&mut registry).is_err());
    assert_eq!(
        fs::read(staged).expect("preserved"),
        b"torn or changed bytes"
    );
    assert!(registry.current().expect("old registration") == before);
    assert!(session.root().join("transaction.json").exists());
}

#[cfg(feature = "installed-runtime")]
mod paired_shortcuts {
    use super::*;
    use download_manager_setup::{
        receipt::Receipt,
        shortcuts::{LINK, ShortcutLocation},
    };
    impl Fixture {
        fn paired(&self) -> SetupSession {
            let programs = self.root.join("Programs Ω");
            if !programs.exists() {
                fs::create_dir(&programs).expect("owned Programs fixture");
            }
            self.session()
                .with_shortcuts(ShortcutLocation::open(&programs).expect("anchor"))
                .expect("disjoint anchor")
        }
        fn receipt(&self) -> Receipt {
            Receipt::decode(&fs::read(self.install.join("installation.json")).expect("receipt"))
                .expect("valid receipt")
        }
        fn link(&self) -> PathBuf {
            self.root
                .join("Programs Ω")
                .join(format!(
                    "Download Manager {}",
                    self.receipt().installation_id
                ))
                .join(LINK)
        }
    }
    #[test]
    fn shortcut_migrates_legacy_then_upgrades_cleans_and_uninstalls_only_recorded_bytes() {
        let f = Fixture::new();
        let package = f.package();
        let mut registry = Registry::default();
        let legacy = f.session();
        legacy
            .install(&package, &mut registry, &mut Probe::default())
            .expect("legacy baseline");
        assert_eq!(f.receipt().version, 1);
        drop(legacy);
        let session = f.paired();
        let first = session
            .install(&package, &mut registry, &mut Probe::default())
            .expect("migration");
        let receipt = f.receipt();
        assert_eq!(receipt.version, 2);
        let link = f.link();
        let original = fs::read(&link).expect("actual link");
        assert_eq!(
            original,
            fs::read(f.install.join(&first).join(LINK)).expect("immutable copy")
        );
        // Independent Windows decoder checks executable/arguments without launching it.
        {
            use winsafe::{self as w, co, prelude::*};
            let _com = w::CoInitializeEx(co::COINIT::APARTMENTTHREADED).expect("COM");
            let decoded: w::IShellLink = w::CoCreateInstance(
                &co::CLSID::ShellLink,
                None::<&w::IUnknown>,
                co::CLSCTX::INPROC_SERVER,
            )
            .expect("link");
            let stream = w::SHCreateMemStream(&original).expect("stream");
            decoded
                .QueryInterface::<w::IPersistStream>()
                .expect("persist")
                .Load(&stream)
                .expect("load");
            assert_eq!(decoded.GetArguments().expect("args"), "--companion");
            assert_eq!(
                PathBuf::from(decoded.GetPath(None, co::SLGP::RAWPATH).expect("path")),
                session.root().join(&first).join(HELPER_FILE)
            );
        }
        fs::write(
            link.parent().expect("parent").join("unrelated.txt"),
            b"preserve",
        )
        .expect("unknown");
        let second = session
            .install(&package, &mut registry, &mut Probe::default())
            .expect("paired upgrade");
        assert_eq!(f.receipt().shortcut_scope, receipt.shortcut_scope);
        assert_ne!(fs::read(&link).expect("updated"), original);
        assert_eq!(
            fs::read(&link).expect("updated"),
            fs::read(f.install.join(&second).join(LINK)).expect("copy")
        );
        session.cleanup(&mut registry).expect("cleanup");
        assert!(!f.install.join(first).exists());
        session.uninstall(&mut registry).expect("uninstall");
        assert!(!link.exists());
        assert_eq!(
            fs::read(link.parent().expect("parent").join("unrelated.txt")).expect("preserved"),
            b"preserve"
        );
        assert!(!f.install.join(second).exists());
        assert!(registry.current().expect("registry").is_none());
    }
    #[test]
    fn shortcut_failures_and_interrupted_activation_restore_exact_previous_entry() {
        for legacy in [false, true] {
            for phase in [
                Phase::Journal,
                Phase::Helper,
                Phase::Manifest,
                Phase::Registration,
                Phase::Shortcut,
                Phase::Receipt,
            ] {
                for crash in [false, true] {
                    let f = Fixture::new();
                    let package = f.package();
                    let mut registry = Registry::default();
                    let baseline = if legacy { f.session() } else { f.paired() };
                    baseline
                        .install(&package, &mut registry, &mut Probe::default())
                        .expect("baseline");
                    let receipt =
                        fs::read(f.install.join("installation.json")).expect("old receipt");
                    let old_registry = registry.current().expect("old registration");
                    let link = f.link();
                    let old = fs::read(&link).ok();
                    drop(baseline);
                    let session = f.paired();
                    let mut probe = Probe {
                        fail: (!crash).then_some(phase),
                        crash: crash.then_some(phase),
                        ..Probe::default()
                    };
                    if crash {
                        assert!(
                            catch_unwind(AssertUnwindSafe(|| session.install(
                                &package,
                                &mut registry,
                                &mut probe
                            )))
                            .is_err()
                        );
                        drop(session);
                        f.paired()
                            .recover(&mut registry)
                            .expect("recover exact old link");
                    } else {
                        assert_eq!(
                            session.install(&package, &mut registry, &mut probe),
                            Err(SetupError::Io)
                        );
                    }
                    assert_eq!(
                        fs::read(f.install.join("installation.json")).expect("restored"),
                        receipt
                    );
                    assert!(registry.current().expect("restored registry") == old_registry);
                    assert_eq!(fs::read(&link).ok(), old);
                    assert!(!f.install.join("transaction.json").exists());
                    if legacy {
                        assert!(!link.parent().expect("folder").exists());
                    }
                }
            }
        }
    }
    #[test]
    fn shortcut_missing_changed_locked_or_different_scope_never_grants_replacement() {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;
        let f = Fixture::new();
        let package = f.package();
        let mut registry = Registry::default();
        let session = f.paired();
        session
            .install(&package, &mut registry, &mut Probe::default())
            .expect("install");
        let link = f.link();
        let bytes = fs::read(&link).expect("link");
        let old_registry = registry.current().expect("registry");
        let lease = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&link)
            .expect("retained external reader");
        assert_eq!(
            session.install(&package, &mut registry, &mut Probe::default()),
            Err(SetupError::Io)
        );
        assert_eq!(fs::read(&link).expect("preserved"), bytes);
        drop(lease);
        fs::write(&link, b"unowned modified shortcut").expect("mutation");
        assert_eq!(session.uninstall(&mut registry), Err(SetupError::Ownership));
        assert!(registry.current().expect("unchanged") == old_registry);
        assert_eq!(
            fs::read(&link).expect("preserved"),
            b"unowned modified shortcut"
        );
        fs::write(&link, &bytes).expect("restore own fixture");
        drop(session);
        let legacy = f.session();
        assert_eq!(legacy.uninstall(&mut registry), Err(SetupError::Ownership));
        drop(legacy);
        let other = f.root.join("other Programs");
        fs::create_dir(&other).expect("other owned anchor");
        let other_session = f
            .session()
            .with_shortcuts(ShortcutLocation::open(&other).expect("anchor"))
            .expect("session");
        assert_eq!(
            other_session.uninstall(&mut registry),
            Err(SetupError::Ownership)
        );
        assert_eq!(fs::read(&link).expect("preserved"), bytes);
        drop(other_session);
        fs::remove_file(&link).expect("simulate missing shortcut");
        f.paired()
            .uninstall(&mut registry)
            .expect("missing link removal");
        assert!(!link.parent().expect("folder").exists());
    }
    #[test]
    fn preexisting_migration_folder_is_not_adopted_even_when_empty() {
        let f = Fixture::new();
        let mut registry = Registry::default();
        let legacy = f.session();
        legacy
            .install(&f.package(), &mut registry, &mut Probe::default())
            .expect("baseline");
        let receipt = fs::read(f.install.join("installation.json")).expect("receipt");
        let link = f.link();
        drop(legacy);
        let paired = f.paired();
        fs::create_dir(link.parent().expect("folder")).expect("unowned collision fixture");
        assert_eq!(
            paired.install(&f.package(), &mut registry, &mut Probe::default()),
            Err(SetupError::Ownership)
        );
        assert!(link.parent().expect("preserved").is_dir());
        assert_eq!(
            fs::read(f.install.join("installation.json")).expect("unchanged"),
            receipt
        );
        assert!(!f.install.join("transaction.json").exists());
    }
    #[test]
    fn interrupted_uninstall_finishes_recovery_and_changed_activation_preserves_journal() {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;
        let f = Fixture::new();
        let mut registry = Registry::default();
        let session = f.paired();
        let generation = session
            .install(&f.package(), &mut registry, &mut Probe::default())
            .expect("install");
        let link = f.link();
        let held = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(session.root().join(generation).join(HELPER_FILE))
            .expect("held image");
        assert!(session.uninstall(&mut registry).is_err());
        assert!(!link.exists());
        assert!(f.install.join("transaction.json").exists());
        drop(held);
        session
            .recover(&mut registry)
            .expect("finish partial uninstall");
        assert!(registry.current().expect("removed").is_none());
        assert!(!f.install.join("installation.json").exists());
        assert!(!f.install.join("transaction.json").exists());
        session
            .install(&f.package(), &mut registry, &mut Probe::default())
            .expect("new install");
        let link = f.link();
        let mut probe = Probe {
            crash: Some(Phase::Shortcut),
            ..Probe::default()
        };
        assert!(
            catch_unwind(AssertUnwindSafe(|| session.install(
                &f.package(),
                &mut registry,
                &mut probe
            )))
            .is_err()
        );
        let published = fs::read(&link).expect("new link");
        let registered = registry.current().expect("new registry");
        fs::write(&link, b"changed after interruption").expect("mutation");
        assert_eq!(session.recover(&mut registry), Err(SetupError::Recovery));
        assert!(f.install.join("transaction.json").exists());
        assert!(registry.current().expect("preserved registry") == registered);
        assert_eq!(
            fs::read(&link).expect("preserved"),
            b"changed after interruption"
        );
        fs::write(&link, published).expect("restore exact owned fixture");
        session
            .recover(&mut registry)
            .expect("rollback after fixture restoration");
        session.uninstall(&mut registry).expect("retire fixture");
    }
    #[test]
    fn shortcut_rollback_validates_old_generation_even_without_old_registration() {
        let f = Fixture::new();
        let mut registry = Registry::default();
        let session = f.paired();
        let old = session
            .install(&f.package(), &mut registry, &mut Probe::default())
            .expect("baseline");
        *registry.value.borrow_mut() = None;
        let link = f.link();
        let helper = session.root().join(old).join(HELPER_FILE);
        let original = fs::read(&helper).expect("old bytes");
        let mut probe = Probe {
            crash: Some(Phase::Shortcut),
            ..Probe::default()
        };
        assert!(
            catch_unwind(AssertUnwindSafe(|| session.install(
                &f.package(),
                &mut registry,
                &mut probe
            )))
            .is_err()
        );
        let new_link = fs::read(&link).expect("new link");
        fs::write(&helper, b"changed old image").expect("mutate owned fixture");
        assert!(session.recover(&mut registry).is_err());
        assert_eq!(
            fs::read(&link).expect("not rolled back to changed image"),
            new_link
        );
        assert!(f.install.join("transaction.json").exists());
        fs::write(helper, original).expect("restore fixture");
        session
            .recover(&mut registry)
            .expect("restored baseline without registration");
        assert!(registry.current().expect("absent").is_none());
        session.uninstall(&mut registry).expect("retire fixture");
    }
}
