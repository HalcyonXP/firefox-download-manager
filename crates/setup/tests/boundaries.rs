#![cfg(windows)]
//! Filesystem-only setup regressions; no registry, helper launch or browser use.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use download_manager_setup::package::{Descriptor, PACKAGE_FILES, VerifiedPackage, file_hash};
use download_manager_setup::paths::{DirectoryLease, InstallationPath};
use download_manager_setup::{HELPER_FILE, SetupError};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("dm setup boundaries {}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).expect("create own root");
        Self(root)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn leases_confine_spaces_state_and_ancestor_replacement() {
    let root = Root::new();
    let local = root.path().join("Local App Data");
    fs::create_dir(&local).expect("local data");
    let target = local.join("Separate App With Spaces").join("host");
    let resolved = InstallationPath::resolve(&target, &local).expect("safe missing path");
    let expected = DirectoryLease::open(&local)
        .expect("canonical local path")
        .path()
        .join("Separate App With Spaces")
        .join("host");
    assert_eq!(resolved.path(), expected);
    assert!(!target.exists()); // validation alone must not write.
    assert!(InstallationPath::resolve(root.path(), &local).is_err());
    assert!(InstallationPath::resolve(&local, &local).is_err());
    for suffix in [
        r"HalcyonXP",
        r"HalcyonXP\FirefoxDownloadManager",
        r"HalcyonXP\FirefoxDownloadManager\state",
        r"HalcyonXP\FirefoxDownloadManager\state\nested",
    ] {
        assert!(InstallationPath::resolve(&local.join(suffix), &local).is_err());
    }
    assert!(
        InstallationPath::resolve(
            &local.join(r"HalcyonXP\FirefoxDownloadManager\host"),
            &local
        )
        .is_ok()
    );
    assert!(fs::rename(&local, root.path().join("moved")).is_err());
    drop(resolved);
    fs::rename(&local, root.path().join("moved")).expect("lease released");
}

#[test]
fn junction_roots_and_ancestors_are_rejected_without_touching_target() {
    let root = Root::new();
    let local = root.path().join("Local App Data");
    let other = root.path().join("Not Installation Data");
    fs::create_dir(&local).expect("local data");
    fs::create_dir(&other).expect("other data");
    fs::write(other.join("keep.txt"), b"synthetic unrelated bytes").expect("canary");
    let link = local.join("link");
    // Junction creation needs no symlink privilege or Developer Mode change.
    // Values are data in a child environment, never interpolated into script text.
    let shell = PathBuf::from(std::env::var_os("WINDIR").expect("Windows directory"))
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let result=std::process::Command::new(shell).args(["-NoProfile","-NonInteractive","-Command","$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:DM_TEST_LINK -Target $env:DM_TEST_TARGET | Out-Null"])
        .env("DM_TEST_LINK",&link).env("DM_TEST_TARGET",&other).output().expect("create owned junction");
    assert!(result.status.success(), "junction creation failed");
    assert!(DirectoryLease::open(&link).is_err());
    assert!(InstallationPath::resolve(&link.join("host"), &local).is_err());
    assert_eq!(
        fs::read(other.join("keep.txt")).expect("preserved"),
        b"synthetic unrelated bytes"
    );
    fs::remove_dir(&link).expect("remove only owned symlink");
}

#[test]
fn package_reads_are_bounded_verified_and_fixed_leaf_only() {
    let root = Root::new();
    let mut files = BTreeMap::new();
    for name in PACKAGE_FILES {
        fs::write(root.path().join(name), b"abc")
            .expect("synthetic payload, not executable qualification");
        let digest = file_hash(&root.path().join(name)).expect("digest");
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        files.insert(name.to_owned(), digest);
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
        root.path().join("package.json"),
        serde_json::to_vec(&descriptor).expect("descriptor"),
    )
    .expect("write descriptor");
    let package = VerifiedPackage::open(root.path()).expect("verified synthetic fixture");
    assert!(package.payload("../outside.exe").is_err());
    assert!(fs::rename(root.path(), root.path().with_extension("renamed")).is_err());
    drop(package);
    fs::write(root.path().join(HELPER_FILE), b"changed").expect("corrupt fixture");
    assert!(matches!(
        VerifiedPackage::open(root.path()),
        Err(SetupError::Package)
    ));
    fs::write(root.path().join("package.json"), vec![b' '; 64 * 1024 + 1])
        .expect("oversized descriptor");
    assert!(matches!(
        VerifiedPackage::open(root.path()),
        Err(SetupError::Package)
    ));
}
