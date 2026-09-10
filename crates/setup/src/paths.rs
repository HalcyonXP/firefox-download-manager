//! Windows directory confinement. Handles deny directory deletion while in use.
use std::fs::{self, File, OpenOptions};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::SetupError;

const REPARSE_POINT: u32 = 0x400;
const BACKUP_SEMANTICS: u32 = 0x0200_0000;
const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const SHARE_READ_WRITE: u32 = 3;
const MAX_ROOT_UNITS: usize = 240;
const MAX_INSTALL_ROOT_UNITS: usize = 160;

/// A canonical ordinary directory with its ancestors held against rename/delete.
/// No Debug implementation: paths can contain a private account name.
pub struct DirectoryLease {
    path: PathBuf,
    _handles: Vec<File>,
}
impl DirectoryLease {
    /// Opens each ancestor without following a reparse point, then canonicalizes.
    /// # Errors
    /// Rejects nonlocal/ambiguous paths, reparse entries, files and sharing errors.
    pub fn open(path: &Path) -> Result<Self, SetupError> {
        validate_text(path)?;
        let mut handles = Vec::new();
        for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
            let file = OpenOptions::new()
                .read(true)
                .share_mode(SHARE_READ_WRITE)
                .custom_flags(BACKUP_SEMANTICS | OPEN_REPARSE_POINT)
                .open(ancestor)
                .map_err(|_| SetupError::Path)?;
            let metadata = file.metadata().map_err(|_| SetupError::Path)?;
            if !metadata.is_dir() || metadata.file_attributes() & REPARSE_POINT != 0 {
                return Err(SetupError::Path);
            }
            handles.push(file);
        }
        let canonical = fs::canonicalize(path).map_err(|_| SetupError::Path)?;
        let text = canonical.to_str().ok_or(SetupError::Path)?;
        // Only a canonical drive prefix is accepted. Mapped/UNC results are not local roots.
        let plain = PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(text));
        validate_text(&plain)?;
        Ok(Self {
            path: plain,
            _handles: handles,
        })
    }
    /// Canonical drive-qualified path. The lease must outlive its filesystem use.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Installation path and nearest existing directory ownership. Does not create files.
pub struct InstallationPath {
    path: PathBuf,
    _existing: DirectoryLease,
    _application_data: DirectoryLease,
}
impl InstallationPath {
    /// Confines setup beneath local application data and disjoint from task state.
    /// # Errors
    /// Rejects traversal, drive-relative/device/UNC roots, unsafe ancestors, or state overlap.
    pub fn resolve(candidate: &Path, application_data: &Path) -> Result<Self, SetupError> {
        validate_text(candidate)?;
        if candidate
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .count()
            > MAX_INSTALL_ROOT_UNITS
        {
            return Err(SetupError::Path);
        }
        let application_data = DirectoryLease::open(application_data)?;
        let mut cursor = candidate.to_owned();
        let mut missing = Vec::new();
        loop {
            match fs::symlink_metadata(&cursor) {
                Ok(_) => break,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    missing.push(cursor.file_name().ok_or(SetupError::Path)?.to_owned());
                    cursor = cursor.parent().ok_or(SetupError::Path)?.to_owned();
                }
                Err(_) => return Err(SetupError::Path),
            }
        }
        let existing = DirectoryLease::open(&cursor)?;
        let mut path = existing.path().to_owned();
        for component in missing.into_iter().rev() {
            path.push(component);
        }
        if same_path(&path, application_data.path()) || !within(&path, application_data.path()) {
            return Err(SetupError::Path);
        }
        let state = application_data
            .path()
            .join("HalcyonXP")
            .join("FirefoxDownloadManager")
            .join("state");
        if within(&path, &state) || within(&state, &path) {
            return Err(SetupError::Path);
        }
        validate_text(&path)?;
        if path.as_os_str().to_string_lossy().encode_utf16().count() > MAX_INSTALL_ROOT_UNITS {
            return Err(SetupError::Path);
        }
        Ok(Self {
            path,
            _existing: existing,
            _application_data: application_data,
        })
    }
    /// Resolved path. Newly created directories must acquire their own lease.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.components().count() == b.components().count() && within(a, b)
}
pub(crate) fn within(candidate: &Path, parent: &Path) -> bool {
    let mut child = candidate.components();
    parent.components().all(|part| {
        child.next().is_some_and(|value| {
            value
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&part.as_os_str().to_string_lossy())
        })
    })
}
pub(crate) fn validate_text(path: &Path) -> Result<(), SetupError> {
    let text = path.to_str().ok_or(SetupError::Path)?;
    let bytes = text.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || bytes[2] != b'\\'
        || text.encode_utf16().count() > MAX_ROOT_UNITS
        || text.contains('/')
    {
        return Err(SetupError::Path);
    }
    if text.len() > 3 && !text[3..].split('\\').all(safe_component) {
        return Err(SetupError::Path);
    }
    Ok(())
}
fn safe_component(value: &str) -> bool {
    if value.is_empty() || value=="." || value==".." || value.ends_with(['.',' '])
        || value.chars().any(|ch|ch.is_control() || matches!(ch,'<'|'>'|':'|'"'|'/'|'\\'|'|'|'?'|'*'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')) {return false;}
    let base = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return false;
    }
    !["COM", "LPT"].iter().any(|prefix| {
        base.strip_prefix(prefix).is_some_and(|tail| {
            matches!(
                tail,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexical_roots_reject_ambiguous_windows_spellings() {
        for path in [
            r"C:relative",
            r"\rooted",
            r"\\server\share",
            r"\\?\C:\device",
            r"C:\a\..\b",
            r"C:\a\b.",
            r"C:\a\NUL.txt",
            r"C:\a\file:stream",
            r"C:\a\COM¹",
            r"C:\a\*",
            r"C:/a",
        ] {
            assert!(validate_text(Path::new(path)).is_err());
        }
        assert!(validate_text(Path::new(r"C:\Ordinary Directory With Spaces\host")).is_ok());
        assert!(!within(
            Path::new(r"C:\local-elsewhere\host"),
            Path::new(r"C:\local")
        ));
    }
}
