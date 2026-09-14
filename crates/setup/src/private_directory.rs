//! Exclusive protected runtime-directory creation. Not installation/engine authority.
use std::{fs, path::Path};

use crate::{
    SetupError,
    paths::{DirectoryLease, validate_text},
    private_file::Adapter,
};
use download_manager_local_ipc::{CurrentUser, Endpoint};

const SCRIPT: &str = include_str!("private_directory.ps1");
const NAME: &str = "companion-runtime";
const ERROR: SetupError = SetupError::PrivateFile;
const BOOTSTRAP_ACCESS: u32 = 0x0012_0089; // FILE_GENERIC_READ; directory-list access participates in share checks

// Documented Windows ACL + ACCESS_ALLOWED_ACE + five-subauthority SID layout.
// repr(C) keeps ACL first and the SID subauthorities DWORD-aligned. The complete
// allocation remains live while CreateDirectory copies its AclSize bytes.
#[repr(C)]
struct ReadAcl {
    header: winsafe::ACL,
    ace_type: u8,
    ace_flags: u8,
    ace_size: u16,
    mask: u32,
    sid_revision: u8,
    subauthority_count: u8,
    authority: [u8; 6],
    subauthorities: [u32; 5],
}

/// Only a successful exclusive creator gets removal authority. Parent ownership
/// must be established independently by the caller; a path/ACL is not a receipt.
pub struct PrivateDirectory {
    parent: DirectoryLease,
    directory: DirectoryLease,
}

impl PrivateDirectory {
    /// Create a new protected directory below an independently owned parent.
    /// Verify the pinned ancestor namespace, create with a current-user read-only protected DACL,
    /// retain the new object, then independently verify/grant/read back access.
    /// # Errors
    /// Existing entries are refused, never adopted. Failure preserves any newly
    /// created directory, including a current-user read-only one, for owned recovery.
    pub fn create(parent: DirectoryLease) -> Result<Self, SetupError> {
        Self::create_with_script(parent, SCRIPT)
    }

    /// Create a fresh session directory for a canonical, already generated endpoint.
    /// # Errors
    /// Existing entries and unsafe/overlong namespace paths are preserved/refused.
    pub fn create_for_endpoint(
        parent: DirectoryLease,
        endpoint: Endpoint,
    ) -> Result<Self, SetupError> {
        Self::create_at(parent, &session_name(endpoint), SCRIPT)
    }

    fn create_with_script(parent: DirectoryLease, script: &str) -> Result<Self, SetupError> {
        Self::create_at(parent, NAME, script)
    }

    fn create_at(parent: DirectoryLease, name: &str, script: &str) -> Result<Self, SetupError> {
        let path = parent.path().join(name);
        validate_text(&path.join(crate::private_file::NAME))?;
        Adapter::start_script("verify", &path, script)?.finish()?;
        let user = CurrentUser::observe().map_err(|_| ERROR)?;
        create_readonly(&path, &user)?;
        let retained = DirectoryLease::open(&path)?;
        Adapter::start_script("create", &path, script)?.finish()?;
        let directory = DirectoryLease::open(&path)?;
        // The ordinary lease is acquired before releasing the bootstrap read-only one.
        drop(retained);
        Ok(Self { parent, directory })
    }

    /// Acquire a separate no-delete lease for protected record operations.
    /// # Errors
    /// Refuses path/handle acquisition errors. This is not installation authority.
    pub fn lease(&self) -> Result<DirectoryLease, SetupError> {
        DirectoryLease::open(self.directory.path())
    }

    /// Remove only this creator's empty directory while retaining its parent.
    /// # Errors
    /// Nonempty directories and competing leases refuse removal; no recursive
    /// cleanup, process termination, adoption or replacement is attempted.
    pub fn remove(self) -> Result<(), SetupError> {
        let Self { parent, directory } = self;
        let path = directory.path().to_owned();
        drop(directory);
        let result = fs::remove_dir(path).map_err(|_| ERROR);
        drop(parent);
        result
    }
}

pub(crate) fn session_name(endpoint: Endpoint) -> String {
    format!("{NAME}.{}", endpoint.id())
}

fn create_readonly(path: &Path, user: &CurrentUser) -> Result<(), SetupError> {
    let sid = winsafe::ConvertStringSidToSid(user.sid()).map_err(|_| ERROR)?;
    let parts: Vec<_> = user.sid().split('-').collect();
    let authority = match parts.get(2).copied() {
        Some("5") => [0, 0, 0, 0, 0, 5],
        Some("12") => [0, 0, 0, 0, 0, 12],
        _ => return Err(ERROR),
    };
    let subauthorities: [u32; 5] = parts
        .get(3..)
        .ok_or(ERROR)?
        .iter()
        .map(|value| value.parse().map_err(|_| ERROR))
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| ERROR)?;
    let mut acl = ReadAcl {
        header: winsafe::ACL {
            AclRevision: 2,
            AclSize: u16::try_from(std::mem::size_of::<ReadAcl>()).map_err(|_| ERROR)?,
            AceCount: 1,
            ..Default::default()
        },
        ace_type: 0, // ACCESS_ALLOWED_ACE_TYPE
        ace_flags: 0,
        ace_size: 36, // ACE_HEADER(4) + mask(4) + SID header(8) + 5 DWORDs(20)
        mask: BOOTSTRAP_ACCESS,
        sid_revision: 1,
        subauthority_count: 5,
        authority,
        subauthorities,
    };
    let mut descriptor = winsafe::InitializeSecurityDescriptor().map_err(|_| ERROR)?;
    descriptor.Control = winsafe::co::SE::DACL_PRESENT | winsafe::co::SE::DACL_PROTECTED;
    // Reviewed synchronous SDK input, not a fabricated reference or transferred
    // allocation: SID's LocalFree guard and the complete current-user read-only ACL remain alive
    // through CreateDirectory. Windows copies the descriptor during creation.
    // Owner is declared mutable by the SDK ABI but CreateDirectory only reads it.
    descriptor.Owner = std::ptr::from_ref::<winsafe::SID>(&sid).cast_mut().cast();
    // Derive from the WHOLE allocation, not an eight-byte header-field borrow:
    // the OS reads all 44 bytes, including the ACE and variable-length SID.
    descriptor.Dacl = std::ptr::from_mut(&mut acl).cast::<winsafe::ACL>();
    let mut attributes = winsafe::SECURITY_ATTRIBUTES::default();
    attributes.set_lpSecurityDescriptor(Some(&mut descriptor));
    attributes.set_bInheritHandle(false);
    winsafe::CreateDirectory(path.to_str().ok_or(ERROR)?, Some(&attributes)).map_err(|_| ERROR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::private_file::{PrivateFile, PublishedFile};
    use std::path::PathBuf;

    fn root() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("dm-private-directory-{} ' Ω", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        path
    }

    fn set_fixture_acl(path: &Path, rights: &str) {
        let script = format!(
            r#"
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
try {{ $sid = $id.User.Value }} finally {{ $id.Dispose() }}
$a = [Security.AccessControl.DirectorySecurity]::new()
$a.SetSecurityDescriptorSddlForm(('D:P(A;OICI;FA;;;' + $sid + ')(A;;{rights};;;WD)'), [Security.AccessControl.AccessControlSections]::Access)
[IO.Directory]::SetAccessControl($dmPath, $a)
[Console]::Out.Write("ready`n")
[Console]::Out.Flush()
if ($dmInput.ReadLine() -cne 'close') {{ exit 1 }}
[Console]::Out.Write('ok')
"#
        );
        Adapter::start_script("verify", path, &script)
            .unwrap()
            .finish()
            .unwrap();
    }

    #[test]
    fn creator_retains_directory_during_grant_and_refuses_changed_descriptors() {
        let root = root();
        let parent = DirectoryLease::open(&root).unwrap();
        // No inherited entries: clearing only the child's protection control
        // bit must be distinguishable from changes to its ACE count/mask.
        let no_inherit = r#"
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
try { $sid = $id.User.Value } finally { $id.Dispose() }
$a = [Security.AccessControl.DirectorySecurity]::new()
$a.SetSecurityDescriptorSddlForm(('D:P(A;;FA;;;' + $sid + ')'), [Security.AccessControl.AccessControlSections]::Access)
[IO.Directory]::SetAccessControl($dmPath, $a)
[Console]::Out.Write("ready`n")
[Console]::Out.Flush()
if ($dmInput.ReadLine() -cne 'close') { exit 1 }
[Console]::Out.Write('ok')
"#;
        Adapter::start_script("verify", &root, no_inherit)
            .unwrap()
            .finish()
            .unwrap();
        let marker = "if ($dmOperation -ceq 'create') {";
        let retained = SCRIPT.replace(
            marker,
            &format!(
                r"{marker}
$refused = $false
try {{ [IO.Directory]::Delete($dmPath) }} catch [IO.IOException] {{ $refused = $true }}
if (!$refused) {{ throw 'retained directory was removed' }}
"
            ),
        );
        let directory =
            PrivateDirectory::create_with_script(DirectoryLease::open(&root).unwrap(), &retained)
                .unwrap();
        directory.remove().unwrap();
        let changed_initial = SCRIPT.replace(marker, &format!(r"{marker}
$bad = [Security.AccessControl.DirectorySecurity]::new()
$bad.SetSecurityDescriptorSddlForm(('D:P(A;;FA;;;' + $sid + ')'), [Security.AccessControl.AccessControlSections]::Access)
[IO.Directory]::SetAccessControl($dmPath, $bad)
"));
        assert!(
            PrivateDirectory::create_with_script(
                DirectoryLease::open(&root).unwrap(),
                &changed_initial
            )
            .is_err()
        );
        fs::remove_dir(root.join(NAME)).unwrap();
        let changed_protection = SCRIPT.replace(
            marker,
            &format!(
                r"{marker}
$bad = [IO.Directory]::GetAccessControl($dmPath)
$bad.SetAccessRuleProtection($false, $true)
[IO.Directory]::SetAccessControl($dmPath, $bad)
"
            ),
        );
        assert!(
            PrivateDirectory::create_with_script(
                DirectoryLease::open(&root).unwrap(),
                &changed_protection
            )
            .is_err()
        );
        fs::remove_dir(root.join(NAME)).unwrap();
        let changed_final = SCRIPT.replace("D:P(A;OICI;FA;;;", "D:P(A;CI;FA;;;");
        assert!(
            PrivateDirectory::create_with_script(
                DirectoryLease::open(&root).unwrap(),
                &changed_final
            )
            .is_err()
        );
        fs::remove_dir(root.join(NAME)).unwrap();
        drop(parent);
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn unsafe_parent_and_ancestor_are_refused_before_creation() {
        let root = root();
        let parent = DirectoryLease::open(&root).unwrap();
        // SDDL DC is a directory-service right (0x2), not FILE_DELETE_CHILD.
        set_fixture_acl(&root, "0x00000040");
        assert!(PrivateDirectory::create(DirectoryLease::open(&root).unwrap()).is_err());
        assert!(!root.join(NAME).exists());
        // FILE_WRITE_DATA must not be mistaken for harmless sibling creation:
        // retain the conservative refusal of potential reparse-control access.
        set_fixture_acl(&root, "0x00000002");
        assert!(PrivateDirectory::create(DirectoryLease::open(&root).unwrap()).is_err());
        assert!(!root.join(NAME).exists());
        // Restore only the owned fixture, then test a non-immediate ancestor.
        set_fixture_acl(&root, "RC");
        let inner = root.join("inner");
        fs::create_dir(&inner).unwrap();
        let inner_lease = DirectoryLease::open(&inner).unwrap();
        set_fixture_acl(&root, "WD");
        assert!(PrivateDirectory::create(DirectoryLease::open(&inner).unwrap()).is_err());
        assert!(!inner.join(NAME).exists());
        set_fixture_acl(&root, "0x00000002");
        assert!(PrivateDirectory::create(DirectoryLease::open(&inner).unwrap()).is_err());
        assert!(!inner.join(NAME).exists());
        set_fixture_acl(&root, "RC");
        drop(inner_lease);
        fs::remove_dir(inner).unwrap();
        drop(parent);
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn existing_file_and_competing_leases_do_not_authorize_replacement_or_removal() {
        let root = root();
        let parent = DirectoryLease::open(&root).unwrap();
        let path = root.join(NAME);
        fs::write(&path, b"public existing entry").unwrap();
        assert!(PrivateDirectory::create(DirectoryLease::open(&root).unwrap()).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"public existing entry");
        fs::remove_file(&path).unwrap();
        let directory = PrivateDirectory::create(DirectoryLease::open(&root).unwrap()).unwrap();
        let reader = directory.lease().unwrap();
        assert!(directory.remove().is_err());
        assert!(path.is_dir());
        drop(reader);
        // The test retains its original creation/parent ownership; no production
        // remove/recovery API adopts this failed publisher by name.
        fs::remove_dir(&path).unwrap();
        drop(parent);
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn owned_directory_enables_private_record_without_adopting_existing_entries() {
        let root = root();
        let directory = PrivateDirectory::create(DirectoryLease::open(&root).unwrap()).unwrap();
        assert!(PrivateDirectory::create(DirectoryLease::open(&root).unwrap()).is_err());
        let published =
            PublishedFile::create(directory.lease().unwrap(), b"public fixture").unwrap();
        let reader = PrivateFile::open(directory.lease().unwrap()).unwrap();
        assert_eq!(reader.bytes(), b"public fixture");
        drop(reader);
        published.remove().unwrap();
        directory.remove().unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn readonly_dacl_is_retained_before_write_access_is_granted() {
        assert_eq!(std::mem::size_of::<ReadAcl>(), 44);
        assert_eq!(std::mem::size_of::<winsafe::ACL>(), 8);
        assert_eq!(std::mem::offset_of!(ReadAcl, header), 0);
        assert_eq!(std::mem::offset_of!(ReadAcl, ace_type), 8);
        assert_eq!(std::mem::offset_of!(ReadAcl, mask), 12);
        assert_eq!(std::mem::offset_of!(ReadAcl, sid_revision), 16);
        assert_eq!(std::mem::offset_of!(ReadAcl, subauthorities), 24);
        let root = root();
        let parent = DirectoryLease::open(&root).unwrap();
        let path = root.join(NAME);
        Adapter::start_script("verify", &path, SCRIPT)
            .unwrap()
            .finish()
            .unwrap();
        let user = CurrentUser::observe().unwrap();
        create_readonly(&path, &user).unwrap();
        let retained = DirectoryLease::open(&path).unwrap();
        assert!(DirectoryLease::open(&path).is_ok());
        assert!(fs::create_dir(path.join("must-not-exist")).is_err());
        assert!(fs::remove_dir(&path).is_err());
        assert!(create_readonly(&path, &user).is_err());
        Adapter::start_script("create", &path, SCRIPT)
            .unwrap()
            .finish()
            .unwrap();
        let lease = DirectoryLease::open(&path).unwrap();
        drop(retained);
        drop(lease);
        fs::remove_dir(&path).unwrap();
        drop(parent);
        fs::remove_dir(root).unwrap();
    }
}
