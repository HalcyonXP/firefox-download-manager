//! Retained current-image/receipt binding. Not engine or publisher authority.
use std::{fs::File, io::Read, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    EXTENSION_FILE, HELPER_FILE, SetupError,
    files::{self, SetupLock},
    package::{file_hash, hash_bytes, ordinary_read},
    paths::{DirectoryLease, InstallationPath},
    receipt::{RECORD_LIMIT, Receipt},
    registry::{CurrentUserRegistration, RegistrationStore, RegistrationValue},
    transaction::manifest_bytes,
};

/// Public metadata only, but deliberately not Debug-renderable or caller-constructible.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImageBinding {
    installation_id: String,
    generation_id: String,
    package_version: String,
    helper_sha256: String,
    receipt_sha256: String,
}

/// Retains receipt and immutable generation files against writes/deletion.
/// This does not acquire the engine's state lock or prove XPI installation.
pub struct InstalledImage {
    root: DirectoryLease,
    generation: DirectoryLease,
    _application_data: DirectoryLease,
    _files: [File; 4],
    pub(crate) binding: ImageBinding,
}

impl InstalledImage {
    /// Verify this running image, never an executable named by a native message.
    /// This entry is opt-in and not called by the released host or preview.
    /// # Errors
    /// Missing/busy setup state, journals, non-current generations, mismatching
    /// files/registration and malformed metadata fail without writes or adoption.
    pub fn open_current() -> Result<Self, SetupError> {
        let executable = std::env::current_exe().map_err(|_| SetupError::Ownership)?;
        // Refuse the development/preview image before reading installed state.
        if executable.file_name() != Some(std::ffi::OsStr::new(HELPER_FILE)) {
            return Err(SetupError::Ownership);
        }
        let application_data = std::env::var_os("LOCALAPPDATA").ok_or(SetupError::Path)?;
        Self::inspect(
            &executable,
            Path::new(&application_data),
            &CurrentUserRegistration,
        )
    }

    fn inspect(
        executable: &Path,
        application_data: &Path,
        registration: &impl RegistrationStore,
    ) -> Result<Self, SetupError> {
        if executable.file_name() != Some(std::ffi::OsStr::new(HELPER_FILE)) {
            return Err(SetupError::Ownership);
        }
        let application_data = DirectoryLease::open(application_data)?;
        let setup = SetupLock::open_existing(application_data.path())?;
        Self::inspect_locked(executable, application_data, registration, &setup)
    }

    fn inspect_locked(
        executable: &Path,
        application_data: DirectoryLease,
        registration: &impl RegistrationStore,
        _setup: &SetupLock,
    ) -> Result<Self, SetupError> {
        let generation = DirectoryLease::open(executable.parent().ok_or(SetupError::Path)?)?;
        let location = InstallationPath::resolve(
            generation.path().parent().ok_or(SetupError::Path)?,
            application_data.path(),
        )?;
        let root = DirectoryLease::open(location.path())?;
        if files::exists(&root.path().join(files::JOURNAL))? {
            return Err(SetupError::Recovery);
        }
        let mut receipt_file = ordinary_read(&root.path().join(files::RECEIPT))?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut receipt_file)
            .take(RECORD_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| SetupError::Ownership)?;
        let receipt = Receipt::decode(&bytes)?;
        let current = receipt.current_generation()?;
        if current.package_version != env!("CARGO_PKG_VERSION")
            || generation.path() != root.path().join(&current.id)
            || executable != generation.path().join(HELPER_FILE)
        {
            return Err(SetupError::Ownership);
        }
        let helper_path = generation.path().join(HELPER_FILE);
        let extension_path = generation.path().join(EXTENSION_FILE);
        let manifest_path = generation.path().join(files::MANIFEST);
        // Acquire each lease BEFORE hashing. Hashing's transient second reader
        // is not itself lifetime protection against later replacement.
        let helper = ordinary_read(&helper_path)?;
        let extension = ordinary_read(&extension_path)?;
        let manifest = ordinary_read(&manifest_path)?;
        for (path, expected) in [
            (&helper_path, &current.helper_sha256),
            (&extension_path, &current.extension_sha256),
            (&manifest_path, &current.manifest_sha256),
        ] {
            if !file_hash(path)?.eq_ignore_ascii_case(expected) {
                return Err(SetupError::Ownership);
            }
        }
        if file_hash(&manifest_path)? != hash_bytes(&manifest_bytes(generation.path())?)
            || registration.current()? != Some(RegistrationValue::for_manifest(&manifest_path)?)
            || files::exists(&root.path().join(files::JOURNAL))?
        {
            return Err(SetupError::Ownership);
        }
        let binding = ImageBinding {
            installation_id: receipt.installation_id.clone(),
            generation_id: current.id.clone(),
            package_version: current.package_version.clone(),
            helper_sha256: current.helper_sha256.to_ascii_lowercase(),
            receipt_sha256: hash_bytes(&bytes),
        };
        Ok(Self {
            root,
            generation,
            _application_data: application_data,
            _files: [receipt_file, helper, extension, manifest],
            binding,
        })
    }

    /// Read the current installed target for an explicit setup UI launch. Unlike
    /// native bridge startup, this derives the executable from a confined receipt.
    /// # Errors
    /// Missing lock/receipt, journal, changed files/registration or unsafe root
    /// refuses without creating or adopting installation objects.
    pub fn open_installed(root: &Path, application_data: &Path) -> Result<Self, SetupError> {
        Self::inspect_root(root, application_data, &CurrentUserRegistration)
    }

    fn inspect_root(
        root: &Path,
        application_data: &Path,
        registration: &impl RegistrationStore,
    ) -> Result<Self, SetupError> {
        let application_data = DirectoryLease::open(application_data)?;
        let setup = SetupLock::open_existing(application_data.path())?;
        let location = InstallationPath::resolve(root, application_data.path())?;
        let root = DirectoryLease::open(location.path())?;
        if files::exists(&root.path().join(files::JOURNAL))? {
            return Err(SetupError::Recovery);
        }
        let mut bytes = Vec::new();
        ordinary_read(&root.path().join(files::RECEIPT))?
            .take(RECORD_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| SetupError::Ownership)?;
        let receipt = Receipt::decode(&bytes)?;
        let executable = root
            .path()
            .join(&receipt.current_generation()?.id)
            .join(HELPER_FILE);
        Self::inspect_locked(&executable, application_data, registration, &setup)
    }

    /// Fixed helper target covered by this retained image binding.
    #[must_use]
    pub fn executable(&self) -> std::path::PathBuf {
        self.generation.path().join(HELPER_FILE)
    }

    /// Lease the verified install root for separately gated runtime publication.
    /// No directory ACL or engine singleton authority is inferred here.
    /// # Errors
    /// Refuses unavailable/nonordinary path traversal.
    pub fn root_lease(&self) -> Result<DirectoryLease, SetupError> {
        DirectoryLease::open(self.root.path())
    }

    /// Non-secret installation identifier; not authority when copied elsewhere.
    #[must_use]
    pub fn installation_id(&self) -> &str {
        &self.binding.installation_id
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::receipt::Generation;
    use std::{fs, path::PathBuf};

    struct Registry(Option<RegistrationValue>);
    impl RegistrationStore for Registry {
        fn current(&self) -> Result<Option<RegistrationValue>, SetupError> {
            Ok(self.0.clone())
        }
        fn replace_if_unchanged(
            &mut self,
            _: Option<&RegistrationValue>,
            _: Option<&RegistrationValue>,
        ) -> Result<(), SetupError> {
            panic!("read-only binding must never change registration")
        }
    }
    pub(crate) struct Fixture {
        root: PathBuf,
        local: PathBuf,
        install: PathBuf,
        executable: PathBuf,
        receipt: Receipt,
        registry: Registry,
    }
    impl Fixture {
        pub(crate) fn new() -> Self {
            let root = std::env::temp_dir().join(format!("dm-image-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            let root = DirectoryLease::open(&root).unwrap().path().to_owned();
            let local = root.join("local");
            fs::create_dir(&local).unwrap();
            drop(SetupLock::acquire(&local).unwrap());
            let install = local.join("install");
            fs::create_dir(&install).unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            let path = install.join(&id);
            fs::create_dir(&path).unwrap();
            let executable = path.join(HELPER_FILE);
            assert!(
                executable
                    == DirectoryLease::open(&path)
                        .unwrap()
                        .path()
                        .join(HELPER_FILE),
                "fixture image path is not canonical"
            );
            fs::write(
                &executable,
                b"public image fixture, not executable qualification",
            )
            .unwrap();
            fs::write(
                path.join(EXTENSION_FILE),
                b"public XPI fixture, not signing qualification",
            )
            .unwrap();
            fs::write(path.join(files::MANIFEST), manifest_bytes(&path).unwrap()).unwrap();
            let generation = Generation {
                id: id.clone(),
                package_version: env!("CARGO_PKG_VERSION").into(),
                helper_sha256: file_hash(&executable).unwrap(),
                extension_sha256: file_hash(&path.join(EXTENSION_FILE)).unwrap(),
                manifest_sha256: file_hash(&path.join(files::MANIFEST)).unwrap(),
            };
            let receipt = Receipt {
                format: "firefox-download-manager-installation".into(),
                version: 1,
                installation_id: uuid::Uuid::new_v4().to_string(),
                current: id,
                generations: vec![generation],
            };
            fs::write(
                install.join(files::RECEIPT),
                serde_json::to_vec(&receipt).unwrap(),
            )
            .unwrap();
            let registry = Registry(Some(
                RegistrationValue::for_manifest(&path.join(files::MANIFEST)).unwrap(),
            ));
            Self {
                root,
                local,
                install,
                executable,
                receipt,
                registry,
            }
        }
        pub(crate) fn inspect(&self) -> Result<InstalledImage, SetupError> {
            InstalledImage::inspect(&self.executable, &self.local, &self.registry)
        }
        fn save_receipt(&self) {
            fs::write(
                self.install.join(files::RECEIPT),
                serde_json::to_vec(&self.receipt).unwrap(),
            )
            .unwrap();
        }
        pub(crate) fn remove(self) {
            fs::remove_dir_all(self.root).unwrap();
        }
    }

    #[test]
    fn current_binding_retains_files_and_setup_coordination_without_state_or_registry_writes() {
        let fixture = Fixture::new();
        let image = fixture.inspect().unwrap();
        assert_eq!(image.installation_id(), fixture.receipt.installation_id);
        for path in [
            fixture.executable.clone(),
            fixture.install.join(files::RECEIPT),
            fixture.executable.with_file_name(EXTENSION_FILE),
            fixture.executable.with_file_name(files::MANIFEST),
        ] {
            assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
            assert!(fs::remove_file(path).is_err());
        }
        assert!(
            fs::rename(
                fixture.executable.parent().unwrap(),
                fixture.install.join("moved")
            )
            .is_err()
        );
        // Short-lived setup inspection lock is released; file leases remain.
        let setup = SetupLock::acquire(&fixture.local).unwrap();
        assert!(fixture.inspect().is_err());
        drop(setup);
        assert!(
            !fixture
                .local
                .join("HalcyonXP/FirefoxDownloadManager/state")
                .exists()
        );
        drop(image);
        fixture.remove();
    }

    #[test]
    fn noncurrent_or_incompatible_generation_never_becomes_image_authority() {
        let mut fixture = Fixture::new();
        assert!(fixture.inspect().is_ok());
        fixture.receipt.generations[0].package_version = "999.0.0".into();
        fixture.save_receipt();
        assert!(fixture.inspect().is_err());
        fixture.receipt.generations[0].package_version = env!("CARGO_PKG_VERSION").into();
        fixture.save_receipt();
        assert!(fixture.inspect().is_ok());
        let mut other = fixture.receipt.generations[0].clone();
        other.id = uuid::Uuid::new_v4().to_string();
        fixture.receipt.current.clone_from(&other.id);
        fixture.receipt.generations.push(other);
        fixture.save_receipt();
        assert!(fixture.inspect().is_err());
        fixture.remove();
    }

    #[test]
    fn missing_journaled_and_changed_bindings_are_refused_without_adoption() {
        let mut fixture = Fixture::new();
        assert!(fixture.inspect().is_ok());
        fixture.registry.0 = None;
        assert!(fixture.inspect().is_err());
        fixture.registry.0 = Some(
            RegistrationValue::for_manifest(&fixture.executable.with_file_name(files::MANIFEST))
                .unwrap(),
        );
        assert!(fixture.inspect().is_ok());
        fs::write(
            fixture.install.join(files::JOURNAL),
            b"public incomplete transaction",
        )
        .unwrap();
        assert!(fixture.inspect().is_err());
        fs::remove_file(fixture.install.join(files::JOURNAL)).unwrap();
        assert!(fixture.inspect().is_ok());
        let lock = fixture
            .local
            .join("HalcyonXP/FirefoxDownloadManager/setup.lock");
        fs::remove_file(&lock).unwrap();
        assert!(fixture.inspect().is_err());
        assert!(!lock.exists());
        fs::write(&lock, b"").unwrap();
        assert!(fixture.inspect().is_ok());
        fs::write(&fixture.executable, b"changed public image").unwrap();
        assert!(fixture.inspect().is_err());
        fixture.receipt.generations[0].helper_sha256 = file_hash(&fixture.executable).unwrap();
        fixture.save_receipt();
        assert!(fixture.inspect().is_ok());
        let manifest = fixture.executable.with_file_name(files::MANIFEST);
        fs::write(&manifest, b"{}").unwrap();
        fixture.receipt.generations[0].manifest_sha256 = file_hash(&manifest).unwrap();
        fixture.save_receipt();
        // Hash agreement alone cannot bless a malformed/redirected manifest.
        assert!(fixture.inspect().is_err());
        fixture.remove();
    }

    #[test]
    fn launch_target_uses_existing_coordination_and_current_receipt_only() {
        let fixture = Fixture::new();
        let image =
            InstalledImage::inspect_root(&fixture.install, &fixture.local, &fixture.registry)
                .unwrap();
        assert_eq!(image.executable(), fixture.executable);
        assert!(
            fs::OpenOptions::new()
                .write(true)
                .open(&fixture.executable)
                .is_err()
        );
        drop(image);
        let lock = SetupLock::open_existing(&fixture.local).unwrap();
        assert!(
            InstalledImage::inspect_root(&fixture.install, &fixture.local, &fixture.registry)
                .is_err()
        );
        drop(lock);
        fs::remove_file(
            fixture
                .local
                .join("HalcyonXP/FirefoxDownloadManager/setup.lock"),
        )
        .unwrap();
        assert!(
            InstalledImage::inspect_root(&fixture.install, &fixture.local, &fixture.registry)
                .is_err()
        );
        assert!(
            !fixture
                .local
                .join("HalcyonXP/FirefoxDownloadManager/setup.lock")
                .exists()
        );
        fixture.remove();
    }
}
