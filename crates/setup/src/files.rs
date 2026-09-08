//! Filesystem primitives for the setup coordinator (no recursive install cleanup).
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::SetupError;
use crate::package::{file_hash, ordinary_read};
use crate::paths::DirectoryLease;
use crate::receipt::RECORD_LIMIT;

pub(crate) const RECEIPT: &str = "installation.json";
pub(crate) const JOURNAL: &str = "transaction.json";
pub(crate) const MANIFEST: &str = "com.halcyonxp.firefox_download_manager.json";

pub(crate) fn exists(path: &Path) -> Result<bool, SetupError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(SetupError::Io),
    }
}
pub(crate) fn create_tree(path: &Path) -> Result<DirectoryLease, SetupError> {
    let mut ancestor = path.to_owned();
    let mut missing = Vec::new();
    while !exists(&ancestor)? {
        missing.push(ancestor.clone());
        ancestor = ancestor.parent().ok_or(SetupError::Path)?.to_owned();
    }
    let mut leases = vec![DirectoryLease::open(&ancestor)?];
    for directory in missing.iter().rev() {
        fs::create_dir(directory).map_err(|_| SetupError::Io)?;
        leases.push(DirectoryLease::open(directory)?);
    }
    leases.pop().ok_or(SetupError::Path)
}

/// Kept for the entire operation; persistent zero-byte lock is never removed.
pub(crate) struct SetupLock {
    _file: File,
    _directory: DirectoryLease,
}
impl SetupLock {
    pub(crate) fn acquire(application_data: &Path) -> Result<Self, SetupError> {
        let directory = create_tree(
            &application_data
                .join("HalcyonXP")
                .join("FirefoxDownloadManager"),
        )?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .custom_flags(0x0020_0000)
            .open(directory.path().join("setup.lock"))
            .map_err(|_| SetupError::Busy)?;
        let metadata = file.metadata().map_err(|_| SetupError::Io)?;
        if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 || metadata.len() != 0 {
            return Err(SetupError::Ownership);
        }
        Ok(Self {
            _file: file,
            _directory: directory,
        })
    }
}

pub(crate) fn read_record(path: &Path) -> Result<Option<Vec<u8>>, SetupError> {
    if !exists(path)? {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    ordinary_read(path)?
        .take(RECORD_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| SetupError::Io)?;
    if bytes.len() > RECORD_LIMIT {
        return Err(SetupError::Ownership);
    }
    Ok(Some(bytes))
}
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), SetupError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .open(path)
        .map_err(|_| SetupError::Ownership)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| SetupError::Io)
}

/// Caller owns parent directories and the cooperative lock. This is not a hostile-account CAS.
pub(crate) fn replace_record(
    path: &Path,
    expected: Option<&[u8]>,
    desired: Option<&[u8]>,
) -> Result<(), SetupError> {
    if read_record(path)?.as_deref() != expected {
        return Err(SetupError::Ownership);
    }
    match (expected, desired) {
        (None, Some(bytes)) => write_new(path, bytes),
        (Some(_), Some(bytes)) => {
            let temporary = path.with_extension(format!("{}.new", uuid::Uuid::new_v4()));
            write_new(&temporary, bytes)?;
            // Check again after staging; only a previously owned record is replaced.
            if read_record(path)?.as_deref() != expected {
                let _ = fs::remove_file(&temporary);
                return Err(SetupError::Ownership);
            }
            if fs::rename(&temporary, path).is_err() {
                let _ = fs::remove_file(&temporary);
                return Err(SetupError::Io);
            }
            Ok(())
        }
        (Some(_), None) => fs::remove_file(path).map_err(|_| SetupError::Io),
        (None, None) => Ok(()),
    }
}

pub(crate) fn copy_new(source: &Path, target: &Path, expected: &str) -> Result<(), SetupError> {
    let mut input = ordinary_read(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .open(target)
        .map_err(|_| SetupError::Ownership)?;
    // Bound even an unexpectedly growing source. A partial failed copy is retained
    // for explicit inspection, not reclassified as matching owned bytes.
    let copied = std::io::copy(
        &mut Read::by_ref(&mut input).take(256 * 1024 * 1024 + 1),
        &mut output,
    )
    .map_err(|_| SetupError::Io)?;
    if copied > 256 * 1024 * 1024 {
        return Err(SetupError::Package);
    }
    output.sync_all().map_err(|_| SetupError::Io)?;
    drop(output);
    if !file_hash(target)?.eq_ignore_ascii_case(expected) {
        return Err(SetupError::Package);
    }
    Ok(())
}
pub(crate) fn verify_file(path: &Path, expected: &str, missing_ok: bool) -> Result<(), SetupError> {
    if !exists(path)? {
        return if missing_ok {
            Ok(())
        } else {
            Err(SetupError::Ownership)
        };
    }
    if !file_hash(path)?.eq_ignore_ascii_case(expected) {
        return Err(SetupError::Ownership);
    }
    Ok(())
}
pub(crate) fn remove_verified(path: &Path, expected: &str) -> Result<(), SetupError> {
    verify_file(path, expected, true)?;
    if exists(path)? {
        fs::remove_file(path).map_err(|_| SetupError::Io)?;
    }
    Ok(())
}
pub(crate) fn remove_empty(path: &Path) -> Result<bool, SetupError> {
    if !exists(path)? {
        return Ok(true);
    }
    let lease = DirectoryLease::open(path)?;
    let empty = fs::read_dir(lease.path())
        .map_err(|_| SetupError::Io)?
        .next()
        .is_none();
    drop(lease);
    if empty {
        fs::remove_dir(path).map_err(|_| SetupError::Io)?;
    }
    Ok(empty)
}
pub(crate) fn generation_path(root: &Path, id: &str) -> PathBuf {
    root.join(id)
}
