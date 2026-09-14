//! Windows provenance metadata, not a malware verdict or Firefox reputation check.
//! Fixed Internet/Restricted markers contain no source URL, referrer or credentials.

use super::{StorageError, StorageOperation, is_reparse_point, map_io};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const INTERNET: &[u8] = b"[ZoneTransfer]\r\nZoneId=3\r\n";
const RESTRICTED: &[u8] = b"[ZoneTransfer]\r\nZoneId=4\r\n";
const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const BACKUP_SEMANTICS: u32 = 0x0200_0000;
const SHARE_READ: u32 = 1;
const SHARE_WRITE: u32 = 2;
const MAX_ANCESTORS: usize = 128;

pub(super) struct InternetZoneLease {
    _ancestors: Vec<File>,
    _base: File,
    stream: File,
    marker: Vec<u8>,
}

fn io(error: &std::io::Error) -> StorageError {
    map_io(StorageOperation::ProtectDownload, error)
}

fn stream_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(":Zone.Identifier");
    PathBuf::from(name)
}

fn same_file(left: &File, right: &File) -> Result<bool, StorageError> {
    let left =
        same_file::Handle::from_file(left.try_clone().map_err(|e| io(&e))?).map_err(|e| io(&e))?;
    let right =
        same_file::Handle::from_file(right.try_clone().map_err(|e| io(&e))?).map_err(|e| io(&e))?;
    Ok(left == right)
}

fn read_marker(stream: &mut File) -> Result<Vec<u8>, StorageError> {
    stream.seek(SeekFrom::Start(0)).map_err(|e| io(&e))?;
    let mut bytes = Vec::new();
    stream
        .take(65)
        .read_to_end(&mut bytes)
        .map_err(|e| io(&e))?;
    if bytes != INTERNET && bytes != RESTRICTED {
        return Err(StorageError::InvalidDownloadZone);
    }
    Ok(bytes)
}

impl InternetZoneLease {
    pub(super) fn establish(file: &File, path: &Path) -> Result<Self, StorageError> {
        // Retain ordinary directory handles root-first. Denying delete only on
        // the leaf would still permit an ancestor rename and path replacement.
        let parents: Vec<_> = path.ancestors().skip(1).take(MAX_ANCESTORS + 1).collect();
        if parents.len() > MAX_ANCESTORS {
            return Err(StorageError::InvalidDestination);
        }
        let mut ancestors = Vec::with_capacity(parents.len());
        for parent in parents.into_iter().rev() {
            let handle = OpenOptions::new()
                .read(true)
                .share_mode(SHARE_READ | SHARE_WRITE)
                .custom_flags(OPEN_REPARSE_POINT | BACKUP_SEMANTICS)
                .open(parent)
                .map_err(|e| io(&e))?;
            let metadata = handle.metadata().map_err(|e| io(&e))?;
            if !metadata.is_dir() || is_reparse_point(&metadata) {
                return Err(StorageError::InvalidDestination);
            }
            ancestors.push(handle);
        }
        let base = OpenOptions::new()
            .read(true)
            .share_mode(SHARE_READ | SHARE_WRITE)
            .custom_flags(OPEN_REPARSE_POINT)
            .open(path)
            .map_err(|e| io(&e))?;
        let metadata = base.metadata().map_err(|e| io(&e))?;
        if !metadata.is_file() || is_reparse_point(&metadata) || !same_file(file, &base)? {
            return Err(StorageError::InvalidDestination);
        }
        let name = stream_path(path);
        let (mut stream, created) = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .share_mode(SHARE_READ)
            .custom_flags(OPEN_REPARSE_POINT)
            .open(&name)
        {
            Ok(handle) => (handle, true),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (
                OpenOptions::new()
                    .read(true)
                    .share_mode(SHARE_READ)
                    .custom_flags(OPEN_REPARSE_POINT)
                    .open(&name)
                    .map_err(|e| io(&e))?,
                false,
            ),
            Err(error) => return Err(io(&error)),
        };
        if !same_file(&base, &stream)? {
            return Err(StorageError::InvalidDownloadZone);
        }
        if created {
            stream.write_all(INTERNET).map_err(|e| io(&e))?;
            stream.sync_all().map_err(|e| io(&e))?;
        }
        // Existing data is never overwritten: reject unknown/weaker metadata,
        // and retain Restricted instead of silently lowering it to Internet.
        let marker = read_marker(&mut stream)?;
        Ok(Self {
            _ancestors: ancestors,
            _base: base,
            stream,
            marker,
        })
    }

    pub(super) fn verify_link(&mut self, file: &File, path: &Path) -> Result<(), StorageError> {
        // The retained marker denies competing stream writes/deletion through
        // readback; a new reader must allow this lease's existing writer.
        let mut linked = OpenOptions::new()
            .read(true)
            .share_mode(SHARE_READ | SHARE_WRITE)
            .custom_flags(OPEN_REPARSE_POINT)
            .open(stream_path(path))
            .map_err(|e| io(&e))?;
        if !same_file(file, &linked)?
            || !same_file(&self.stream, &linked)?
            || read_marker(&mut linked)? != self.marker
        {
            return Err(StorageError::InvalidDownloadZone);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn zone_lease_pins_ancestors_base_and_stream_until_release() {
        let root = std::env::temp_dir().join(format!("dm-zone-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).expect("owned root");
        let root = root.canonicalize().expect("canonical root");
        let parent = root.join("owned");
        fs::create_dir(&parent).expect("owned parent");
        let path = parent.join("owned.part");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("owned file");
        let mut lease = InternetZoneLease::establish(&file, &path).expect("lease");
        assert!(fs::rename(&parent, root.join("moved")).is_err());
        assert!(fs::rename(&path, parent.join("moved.part")).is_err());
        assert!(
            OpenOptions::new()
                .write(true)
                .open(stream_path(&path))
                .is_err()
        );
        let linked = parent.join("final.bin");
        fs::hard_link(&path, &linked).expect("new link");
        lease
            .verify_link(&file, &linked)
            .expect("linked stream identity");
        drop(lease);
        drop(file);
        fs::rename(&parent, root.join("moved")).expect("leases retired");
        fs::remove_dir_all(&root).expect("owned cleanup");
    }
}
