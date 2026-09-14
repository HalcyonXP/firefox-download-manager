//! Receipt-bound Start Menu entries. No startup hooks or persisted path authority.
use crate::{
    HELPER_FILE, SetupError, files,
    package::hash_bytes,
    paths::DirectoryLease,
    receipt::{Generation, Receipt},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Fixed generation copy and visible launch entry.
pub const LINK: &str = "Download Manager.lnk";
const LIMIT: usize = 8192;

/// Independently resolved Programs anchor retained through setup mutations.
pub struct ShortcutLocation {
    directory: DirectoryLease,
}
impl ShortcutLocation {
    /// Tests must supply an independently owned directory, not the normal Start Menu.
    /// # Errors
    /// Refuses unavailable, aliased or unsafe directories without creating them.
    pub fn open(path: &Path) -> Result<Self, SetupError> {
        Ok(Self {
            directory: DirectoryLease::open(path)?,
        })
    }
    /// Current-user Windows Programs folder; no caller/persisted path override.
    /// # Errors
    /// Refuses unavailable/redirected unsafe folders, without creating them.
    #[cfg(feature = "installed-runtime")]
    pub fn current() -> Result<Self, SetupError> {
        use winsafe::{self as w, co};
        let path = w::SHGetKnownFolderPath(&co::KNOWNFOLDERID::Programs, co::KF::default(), None)
            .map_err(|_| SetupError::Path)?;
        Self::open(Path::new(&path))
    }
    pub(crate) fn path(&self) -> &Path {
        self.directory.path()
    }
    pub(crate) fn scope(&self) -> Result<String, SetupError> {
        Ok(hash_bytes(
            self.path().to_str().ok_or(SetupError::Path)?.as_bytes(),
        ))
    }
    fn folder(&self, receipt: &Receipt) -> Result<PathBuf, SetupError> {
        if !receipt.is_valid()
            || receipt.version != 2
            || receipt.shortcut_scope.as_ref() != Some(&self.scope()?)
        {
            return Err(SetupError::Ownership);
        }
        Ok(self
            .path()
            .join(format!("Download Manager {}", receipt.installation_id)))
    }
    pub(crate) fn prepare(
        &self,
        previous: Option<&Receipt>,
        next: &Receipt,
    ) -> Result<DirectoryLease, SetupError> {
        let folder = self.folder(next)?;
        if previous.is_none_or(|r| r.version != 2) {
            // No existence check followed by adoption: successful creation owns this folder.
            fs::create_dir(&folder).map_err(|_| SetupError::Ownership)?;
        }
        DirectoryLease::open(&folder)
    }
    pub(crate) fn check(
        &self,
        root: &Path,
        receipt: &Receipt,
        missing_ok: bool,
    ) -> Result<(), SetupError> {
        let folder = self.folder(receipt)?;
        if !files::exists(&folder)? {
            return if missing_ok {
                Ok(())
            } else {
                Err(SetupError::Ownership)
            };
        }
        let _lease = DirectoryLease::open(&folder)?;
        let expected = generation_bytes(root, receipt.current_generation()?, false)?
            .ok_or(SetupError::Ownership)?;
        let actual = files::read_record(&folder.join(LINK))?;
        if actual.as_ref().is_some_and(|b| b != &expected) || (actual.is_none() && !missing_ok) {
            return Err(SetupError::Ownership);
        }
        Ok(())
    }
    pub(crate) fn activate(
        &self,
        root: &Path,
        previous: Option<&Receipt>,
        next: &Receipt,
    ) -> Result<(), SetupError> {
        let folder = self.folder(next)?;
        let _lease = DirectoryLease::open(&folder)?;
        let old = previous
            .filter(|r| r.version == 2)
            .map(|r| generation_bytes(root, r.current_generation()?, false))
            .transpose()?
            .flatten();
        let new = generation_bytes(root, next.current_generation()?, false)?
            .ok_or(SetupError::Ownership)?;
        files::replace_record(&folder.join(LINK), old.as_deref(), Some(&new))
    }
    pub(crate) fn rollback(
        &self,
        root: &Path,
        previous: Option<&Receipt>,
        next: &Receipt,
    ) -> Result<(), SetupError> {
        let folder = self.folder(next)?;
        if !files::exists(&folder)? {
            return if previous.is_some_and(|r| r.version == 2) {
                Err(SetupError::Recovery)
            } else {
                Ok(())
            };
        }
        let lease = DirectoryLease::open(&folder)?;
        let old = previous
            .filter(|r| r.version == 2)
            .map(|r| generation_bytes(root, r.current_generation()?, false))
            .transpose()?
            .flatten();
        let actual = files::read_record(&folder.join(LINK))?;
        if actual != old {
            // The immutable generation copy may not have been staged yet. In that
            // case only absence/previous bytes are legal; a digest alone cannot bless content.
            let new = generation_bytes(root, next.current_generation()?, true)?;
            if actual.is_none() || actual != new {
                return Err(SetupError::Recovery);
            }
            files::replace_record(&folder.join(LINK), actual.as_deref(), old.as_deref())?;
        }
        drop(lease);
        if previous.is_none_or(|r| r.version == 1) {
            files::remove_empty(&folder)?;
        }
        Ok(())
    }
    pub(crate) fn remove(&self, root: &Path, receipt: &Receipt) -> Result<(), SetupError> {
        let folder = self.folder(receipt)?;
        if !files::exists(&folder)? {
            return Ok(());
        }
        let lease = DirectoryLease::open(&folder)?;
        let path = folder.join(LINK);
        if let Some(actual) = files::read_record(&path)? {
            let expected = generation_bytes(root, receipt.current_generation()?, false)?
                .ok_or(SetupError::Ownership)?;
            if actual != expected {
                return Err(SetupError::Ownership);
            }
            files::replace_record(&path, Some(&actual), None)?;
        }
        drop(lease);
        files::remove_empty(&folder)?;
        Ok(())
    }
}

pub(crate) fn generation_bytes(
    root: &Path,
    generation: &Generation,
    missing_ok: bool,
) -> Result<Option<Vec<u8>>, SetupError> {
    let Some(expected) = &generation.shortcut_sha256 else {
        return Ok(None);
    };
    if !generation.is_valid() {
        return Err(SetupError::Ownership);
    }
    let directory = root.join(&generation.id);
    if !files::exists(&directory)? {
        return if missing_ok {
            Ok(None)
        } else {
            Err(SetupError::Ownership)
        };
    }
    let _lease = DirectoryLease::open(&directory)?;
    let bytes = files::read_record(&directory.join(LINK))?;
    match bytes {
        Some(bytes) if bytes.len() <= LIMIT && hash_bytes(&bytes) == *expected => {
            validate(&bytes, &directory.join(HELPER_FILE))?;
            Ok(Some(bytes))
        }
        None if missing_ok => Ok(None),
        _ => Err(SetupError::Ownership),
    }
}

#[cfg(feature = "installed-runtime")]
mod shell {
    use super::{LIMIT, Path, SetupError};
    use winsafe::{self as w, co, prelude::*};
    fn link() -> Result<w::IShellLink, SetupError> {
        w::CoCreateInstance(
            &co::CLSID::ShellLink,
            None::<&w::IUnknown>,
            co::CLSCTX::INPROC_SERVER,
        )
        .map_err(|_| SetupError::Ownership)
    }
    pub(crate) fn encode(executable: &Path) -> Result<Vec<u8>, SetupError> {
        let _com = w::CoInitializeEx(co::COINIT::APARTMENTTHREADED | co::COINIT::DISABLE_OLE1DDE)
            .map_err(|_| SetupError::Ownership)?;
        let link = link()?;
        let path = executable.to_str().ok_or(SetupError::Path)?;
        let parent = executable
            .parent()
            .and_then(Path::to_str)
            .ok_or(SetupError::Path)?;
        link.SetPath(path)
            .and_then(|()| link.SetArguments("--companion"))
            .and_then(|()| link.SetWorkingDirectory(parent))
            .and_then(|()| link.SetDescription("Download Manager"))
            .and_then(|()| link.SetShowCmd(co::SW::SHOWNORMAL))
            .map_err(|_| SetupError::Ownership)?;
        let stream = w::SHCreateMemStream(&[]).map_err(|_| SetupError::Ownership)?;
        link.QueryInterface::<w::IPersistStream>()
            .and_then(|p| p.Save(&stream, false))
            .map_err(|_| SetupError::Ownership)?;
        let length = stream
            .Seek(0, co::STREAM_SEEK::END)
            .map_err(|_| SetupError::Ownership)?;
        if length == 0 || length > LIMIT as u64 {
            return Err(SetupError::Ownership);
        }
        stream
            .Seek(0, co::STREAM_SEEK::SET)
            .map_err(|_| SetupError::Ownership)?;
        let mut bytes = vec![0; usize::try_from(length).map_err(|_| SetupError::Ownership)?];
        if u64::from(stream.Read(&mut bytes).map_err(|_| SetupError::Ownership)?) != length {
            return Err(SetupError::Ownership);
        }
        validate(&bytes, executable)?;
        Ok(bytes)
    }
    pub(crate) fn validate(bytes: &[u8], executable: &Path) -> Result<(), SetupError> {
        if bytes.len() < 76
            || bytes.len() > LIMIT
            || bytes[..4] != 76_u32.to_le_bytes()
            || bytes[4..20] != [1, 20, 2, 0, 0, 0, 0, 0, 192, 0, 0, 0, 0, 0, 0, 70]
        {
            return Err(SetupError::Ownership);
        }
        let flags = u32::from_le_bytes(
            bytes[20..24]
                .try_into()
                .map_err(|_| SetupError::Ownership)?,
        );
        // Only local target ID/list info, description, working directory, arguments
        // and Unicode. Refuse elevation, environment expansion, Darwin and shim modes.
        if flags & !0xb7 != 0 || flags & 0xb4 != 0xb4 || flags.trailing_zeros() >= 2 {
            return Err(SetupError::Ownership);
        }
        let _com = w::CoInitializeEx(co::COINIT::APARTMENTTHREADED | co::COINIT::DISABLE_OLE1DDE)
            .map_err(|_| SetupError::Ownership)?;
        let link = link()?;
        let stream = w::SHCreateMemStream(bytes).map_err(|_| SetupError::Ownership)?;
        link.QueryInterface::<w::IPersistStream>()
            .and_then(|p| p.Load(&stream))
            .map_err(|_| SetupError::Ownership)?;
        // Never Resolve or ShellExecute during validation; no relocation/search authority.
        if link
            .GetPath(None, co::SLGP::RAWPATH)
            .map_err(|_| SetupError::Ownership)?
            != executable.to_str().ok_or(SetupError::Path)?
            || link.GetArguments().map_err(|_| SetupError::Ownership)? != "--companion"
            || link
                .GetWorkingDirectory()
                .map_err(|_| SetupError::Ownership)?
                != executable
                    .parent()
                    .and_then(Path::to_str)
                    .ok_or(SetupError::Path)?
            || link.GetShowCmd().map_err(|_| SetupError::Ownership)? != co::SW::SHOWNORMAL
        {
            return Err(SetupError::Ownership);
        }
        Ok(())
    }
}
#[cfg(feature = "installed-runtime")]
pub(crate) use shell::{encode, validate};
#[cfg(not(feature = "installed-runtime"))]
pub(crate) fn encode(_: &Path) -> Result<Vec<u8>, SetupError> {
    Err(SetupError::Unsupported)
}
#[cfg(not(feature = "installed-runtime"))]
fn validate(_: &[u8], _: &Path) -> Result<(), SetupError> {
    Err(SetupError::Ownership)
}
