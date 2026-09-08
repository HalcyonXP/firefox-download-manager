//! Immutable generation activation and conservative journal recovery.
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;
use uuid::Uuid;

use crate::files::{self, JOURNAL, MANIFEST, RECEIPT, SetupLock};
use crate::package::{VerifiedPackage, hash_bytes};
use crate::paths::{DirectoryLease, InstallationPath};
use crate::receipt::{Action, Generation, Journal, MAX_GENERATIONS, Receipt};
use crate::registry::{RegistrationStore, RegistrationValue};
use crate::{EXTENSION_FILE, EXTENSION_ID, HELPER_FILE, HOST_NAME, SetupError};

/// Testable failure boundaries; no caller-controlled script or command strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Journal,
    Directory,
    Helper,
    Extension,
    Manifest,
    Probe,
    Registration,
    Receipt,
}
/// Only production's bounded helper launch verifier is wired to the command line.
pub trait InstallProbe {
    /// Verify the staged executable with fresh application state, never live state.
    /// # Errors
    /// Refuse incompatible, failed or timed-out launches.
    fn verify(&mut self, executable: &Path, application_data: &Path) -> Result<(), SetupError>;
    /// Fault-injection seam for tests; ordinary setup performs no action here.
    /// # Errors
    /// Injected errors exercise rollback; interrupted processes leave the journal.
    fn checkpoint(&mut self, _phase: Phase) -> Result<(), SetupError> {
        Ok(())
    }
}

/// All filesystem and registry changes require this per-user cooperative lock.
pub struct SetupSession {
    location: InstallationPath,
    _lock: SetupLock,
    application_data: DirectoryLease,
}
impl SetupSession {
    /// Resolve roots and acquire exclusive setup ownership. Does not register a host.
    /// # Errors
    /// Rejects unsafe roots, state overlap and concurrent setup.
    pub fn open(root: &Path, application_data: &Path) -> Result<Self, SetupError> {
        let application_data = DirectoryLease::open(application_data)?;
        let location = InstallationPath::resolve(root, application_data.path())?;
        let lock = SetupLock::acquire(application_data.path())?;
        Ok(Self {
            location,
            _lock: lock,
            application_data,
        })
    }
    /// Canonical installation root, not a task/download destination.
    #[must_use]
    pub fn root(&self) -> &Path {
        self.location.path()
    }

    fn ready(&self) -> Result<(DirectoryLease, Option<Receipt>), SetupError> {
        if files::exists(&self.root().join(JOURNAL))? {
            return Err(SetupError::Recovery);
        }
        let directory = files::create_tree(self.root())?;
        let receipt = load_receipt(self.root())?;
        if receipt.is_none()
            && fs::read_dir(self.root())
                .map_err(|_| SetupError::Io)?
                .next()
                .is_some()
        {
            return Err(SetupError::Ownership);
        }
        Ok((directory, receipt))
    }

    /// Stage a new generation, probe it, then activate registration and receipt.
    /// # Errors
    /// Failure rolls back only proven content; ambiguity preserves the journal.
    pub fn install(
        &self,
        package: &VerifiedPackage,
        registry: &mut impl RegistrationStore,
        probe: &mut impl InstallProbe,
    ) -> Result<String, SetupError> {
        let observed = registry.current()?;
        let (_root, previous) = self.ready()?;
        let old_value = expected_registration(self.root(), previous.as_ref())?;
        if observed.is_some() && observed != old_value {
            return Err(SetupError::Registration);
        }
        if let Some(old) = &previous {
            if old.generations.len() >= MAX_GENERATIONS {
                return Err(SetupError::HistoryFull);
            }
            verify_generation(self.root(), old.current_generation()?, false)?;
        }
        let generation = new_generation(self.root(), package)?;
        let mut next = previous.clone().unwrap_or_else(|| Receipt {
            format: "firefox-download-manager-installation".into(),
            version: 1,
            installation_id: Uuid::new_v4().to_string(),
            current: generation.id.clone(),
            generations: Vec::new(),
        });
        next.current.clone_from(&generation.id);
        next.generations.push(generation.clone());
        let journal = new_journal(Action::Install, previous, Some(next), observed.is_some());
        // A journal may claim only a directory we successfully created. A UUID
        // collision must never turn pre-existing bytes into rollback authority.
        let path = files::generation_path(self.root(), &generation.id);
        fs::create_dir(&path).map_err(|_| SetupError::Ownership)?;
        let directory = DirectoryLease::open(&path)?;
        persist_journal(self.root(), &journal)?;
        let result = self.stage_activate(package, &journal, &generation, registry, probe);
        drop(directory);
        if let Err(error) = result {
            if self.rollback_install(&journal, registry).is_err() {
                return Err(SetupError::Recovery);
            }
            return Err(error);
        }
        delete_journal(self.root(), &journal)?;
        Ok(generation.id)
    }

    fn stage_activate(
        &self,
        package: &VerifiedPackage,
        journal: &Journal,
        generation: &Generation,
        registry: &mut impl RegistrationStore,
        probe: &mut impl InstallProbe,
    ) -> Result<(), SetupError> {
        probe.checkpoint(Phase::Journal)?;
        let path = files::generation_path(self.root(), &generation.id);
        let _directory = DirectoryLease::open(&path)?;
        probe.checkpoint(Phase::Directory)?;
        files::copy_new(
            &package.payload(HELPER_FILE)?,
            &path.join(HELPER_FILE),
            &generation.helper_sha256,
        )?;
        probe.checkpoint(Phase::Helper)?;
        files::copy_new(
            &package.payload(EXTENSION_FILE)?,
            &path.join(EXTENSION_FILE),
            &generation.extension_sha256,
        )?;
        probe.checkpoint(Phase::Extension)?;
        files::write_new(&path.join(MANIFEST), &manifest_bytes(&path)?)?;
        verify_generation(self.root(), generation, false)?;
        probe.checkpoint(Phase::Manifest)?;
        probe.verify(&path.join(HELPER_FILE), self.application_data.path())?;
        probe.checkpoint(Phase::Probe)?;
        // Recheck owned bytes after the child exits, immediately before activation.
        verify_generation(self.root(), generation, false)?;
        let old = if journal.had_registration {
            expected_registration(self.root(), journal.previous.as_ref())?
        } else {
            None
        };
        let new = expected_registration(self.root(), journal.next.as_ref())?;
        registry.replace_if_unchanged(old.as_ref(), new.as_ref())?;
        probe.checkpoint(Phase::Registration)?;
        replace_receipt(
            self.root(),
            journal.previous.as_ref(),
            journal.next.as_ref(),
        )?;
        probe.checkpoint(Phase::Receipt)
    }

    fn rollback_install(
        &self,
        journal: &Journal,
        registry: &mut impl RegistrationStore,
    ) -> Result<(), SetupError> {
        if journal.had_registration {
            verify_generation(
                self.root(),
                journal
                    .previous
                    .as_ref()
                    .ok_or(SetupError::Recovery)?
                    .current_generation()?,
                false,
            )?;
        }
        let old = if journal.had_registration {
            expected_registration(self.root(), journal.previous.as_ref())?
        } else {
            None
        };
        let new = expected_registration(self.root(), journal.next.as_ref())?;
        let current = registry.current()?;
        if current == new {
            registry.replace_if_unchanged(new.as_ref(), old.as_ref())?;
        } else if current != old {
            return Err(SetupError::Recovery);
        }
        let actual = load_receipt(self.root())?;
        if actual == journal.next {
            replace_receipt(self.root(), actual.as_ref(), journal.previous.as_ref())?;
        } else if actual != journal.previous {
            return Err(SetupError::Recovery);
        }
        let next = journal.next.as_ref().ok_or(SetupError::Recovery)?;
        remove_generation(self.root(), next.current_generation()?)?;
        delete_journal(self.root(), journal)
    }

    /// Complete a conservative interrupted operation; never guess unknown bytes.
    /// # Errors
    /// Malformed state, changed registration/content or failed cleanup is retained.
    pub fn recover(&self, registry: &mut impl RegistrationStore) -> Result<(), SetupError> {
        let _root = DirectoryLease::open(self.root())?;
        let bytes = files::read_record(&self.root().join(JOURNAL))?.ok_or(SetupError::Recovery)?;
        let journal = Journal::decode(&bytes)?;
        match journal.action {
            Action::Install => self.rollback_install(&journal, registry),
            Action::Uninstall | Action::Cleanup => self.finish_removal(&journal, registry),
        }
    }

    /// Rebind a verified current generation after missing/stale owned registration.
    /// # Errors
    /// Never adopts another root or unknown registration; no journal is bypassed.
    pub fn repair(
        &self,
        registry: &mut impl RegistrationStore,
        probe: &mut impl InstallProbe,
    ) -> Result<(), SetupError> {
        let (_root, receipt) = self.ready()?;
        let receipt = receipt.ok_or(SetupError::Ownership)?;
        let current = receipt.current_generation()?;
        verify_generation(self.root(), current, false)?;
        let observed = registry.current()?;
        if let Some(value) = &observed {
            let known = receipt.generations.iter().any(|generation| {
                RegistrationValue::for_manifest(
                    &files::generation_path(self.root(), &generation.id).join(MANIFEST),
                )
                .is_ok_and(|expected| &expected == value)
            });
            if !known {
                return Err(SetupError::Registration);
            }
        }
        probe.verify(
            &files::generation_path(self.root(), &current.id).join(HELPER_FILE),
            self.application_data.path(),
        )?;
        verify_generation(self.root(), current, false)?;
        let desired = expected_registration(self.root(), Some(&receipt))?;
        registry.replace_if_unchanged(observed.as_ref(), desired.as_ref())
    }

    /// Remove matching registration and verified generated files, never task state.
    /// # Errors
    /// Refuses unknown registration/content; partial removal remains recoverable.
    pub fn uninstall(&self, registry: &mut impl RegistrationStore) -> Result<(), SetupError> {
        self.begin_removal(registry, false)
    }
    /// Remove only retired generations; keep the current installation registered.
    /// # Errors
    /// Unknown/in-use content is retained, not swept recursively.
    pub fn cleanup(&self, registry: &mut impl RegistrationStore) -> Result<(), SetupError> {
        self.begin_removal(registry, true)
    }
    fn begin_removal(
        &self,
        registry: &mut impl RegistrationStore,
        cleanup: bool,
    ) -> Result<(), SetupError> {
        let (_root, previous) = self.ready()?;
        let previous = previous.ok_or(SetupError::Ownership)?;
        let observed = registry.current()?;
        let expected = expected_registration(self.root(), Some(&previous))?;
        if observed.is_some() && observed != expected {
            return Err(SetupError::Registration);
        }
        let selected = previous
            .generations
            .iter()
            .filter(|generation| !cleanup || generation.id != previous.current);
        for generation in selected {
            verify_generation(self.root(), generation, true)?;
        }
        let next = if cleanup {
            let mut next = previous.clone();
            next.generations
                .retain(|generation| generation.id == next.current);
            Some(next)
        } else {
            None
        };
        let journal = new_journal(
            if cleanup {
                Action::Cleanup
            } else {
                Action::Uninstall
            },
            Some(previous),
            next,
            observed.is_some(),
        );
        persist_journal(self.root(), &journal)?;
        self.finish_removal(&journal, registry)
    }
    fn finish_removal(
        &self,
        journal: &Journal,
        registry: &mut impl RegistrationStore,
    ) -> Result<(), SetupError> {
        let previous = journal.previous.as_ref().ok_or(SetupError::Recovery)?;
        let old = expected_registration(self.root(), Some(previous))?;
        let observed = registry.current()?;
        if observed.is_some() && observed != old {
            return Err(SetupError::Registration);
        }
        let actual = load_receipt(self.root())?;
        if actual != journal.previous && actual != journal.next {
            return Err(SetupError::Recovery);
        }
        // Validate *all* remaining owned bytes before deleting any of them.
        let selected: Vec<_> = previous
            .generations
            .iter()
            .filter(|generation| {
                journal.action == Action::Uninstall || generation.id != previous.current
            })
            .collect();
        for generation in &selected {
            verify_generation(self.root(), generation, true)?;
        }
        if journal.action == Action::Uninstall {
            registry.replace_if_unchanged(observed.as_ref(), None)?;
        }
        for generation in selected {
            remove_generation(self.root(), generation)?;
        }
        replace_receipt(self.root(), actual.as_ref(), journal.next.as_ref())?;
        delete_journal(self.root(), journal)
    }
}

fn new_journal(
    action: Action,
    previous: Option<Receipt>,
    next: Option<Receipt>,
    had_registration: bool,
) -> Journal {
    Journal {
        format: "firefox-download-manager-setup-transaction".into(),
        version: 1,
        transaction_id: Uuid::new_v4().to_string(),
        action,
        previous,
        next,
        had_registration,
    }
}
fn persist_journal(root: &Path, journal: &Journal) -> Result<(), SetupError> {
    if !journal.is_valid() {
        return Err(SetupError::Recovery);
    }
    files::write_new(
        &root.join(JOURNAL),
        &serde_json::to_vec(journal).map_err(|_| SetupError::Recovery)?,
    )
}
fn delete_journal(root: &Path, journal: &Journal) -> Result<(), SetupError> {
    let path = root.join(JOURNAL);
    let bytes = files::read_record(&path)?.ok_or(SetupError::Recovery)?;
    if Journal::decode(&bytes)? != *journal {
        return Err(SetupError::Recovery);
    }
    files::replace_record(&path, Some(&bytes), None)
}
fn load_receipt(root: &Path) -> Result<Option<Receipt>, SetupError> {
    files::read_record(&root.join(RECEIPT))?
        .map(|bytes| Receipt::decode(&bytes))
        .transpose()
}
fn replace_receipt(
    root: &Path,
    expected: Option<&Receipt>,
    desired: Option<&Receipt>,
) -> Result<(), SetupError> {
    let path = root.join(RECEIPT);
    let bytes = files::read_record(&path)?;
    if bytes
        .as_ref()
        .map(|bytes| Receipt::decode(bytes))
        .transpose()?
        .as_ref()
        != expected
    {
        return Err(SetupError::Ownership);
    }
    let next = desired
        .map(serde_json::to_vec)
        .transpose()
        .map_err(|_| SetupError::Ownership)?;
    files::replace_record(&path, bytes.as_deref(), next.as_deref())
}
fn expected_registration(
    root: &Path,
    receipt: Option<&Receipt>,
) -> Result<Option<RegistrationValue>, SetupError> {
    receipt
        .map(|receipt| {
            RegistrationValue::for_manifest(
                &files::generation_path(root, &receipt.current_generation()?.id).join(MANIFEST),
            )
        })
        .transpose()
}
fn manifest_bytes(generation_path: &Path) -> Result<Vec<u8>, SetupError> {
    let executable = generation_path.join(HELPER_FILE);
    serde_json::to_vec(&json!({"name": HOST_NAME,"description":"Firefox Download Manager native host","path":executable.to_str().ok_or(SetupError::Path)?,"type":"stdio","allowed_extensions":[EXTENSION_ID]})).map_err(|_| SetupError::Package)
}
fn new_generation(root: &Path, package: &VerifiedPackage) -> Result<Generation, SetupError> {
    let id = Uuid::new_v4().to_string();
    Ok(Generation {
        manifest_sha256: hash_bytes(&manifest_bytes(&root.join(&id))?),
        id,
        package_version: package.version().into(),
        helper_sha256: package
            .expected(HELPER_FILE)
            .ok_or(SetupError::Package)?
            .into(),
        extension_sha256: package
            .expected(EXTENSION_FILE)
            .ok_or(SetupError::Package)?
            .into(),
    })
}
fn generation_files<'a>(
    root: &Path,
    generation: &'a Generation,
) -> Result<[(PathBuf, &'a str); 3], SetupError> {
    if !generation.is_valid() {
        return Err(SetupError::Ownership);
    }
    let path = files::generation_path(root, &generation.id);
    Ok([
        (path.join(HELPER_FILE), &generation.helper_sha256),
        (path.join(EXTENSION_FILE), &generation.extension_sha256),
        (path.join(MANIFEST), &generation.manifest_sha256),
    ])
}
fn verify_generation(
    root: &Path,
    generation: &Generation,
    missing_ok: bool,
) -> Result<(), SetupError> {
    let path = files::generation_path(root, &generation.id);
    if !files::exists(&path)? {
        return if missing_ok {
            Ok(())
        } else {
            Err(SetupError::Ownership)
        };
    }
    let _directory = DirectoryLease::open(&path)?;
    for (file, digest) in generation_files(root, generation)? {
        files::verify_file(&file, digest, missing_ok)?;
    }
    // A receipt cannot bless a manifest pointing outside its own generation.
    if files::exists(&path.join(MANIFEST))?
        && crate::package::file_hash(&path.join(MANIFEST))? != hash_bytes(&manifest_bytes(&path)?)
    {
        return Err(SetupError::Ownership);
    }
    Ok(())
}
fn remove_generation(root: &Path, generation: &Generation) -> Result<(), SetupError> {
    verify_generation(root, generation, true)?;
    let path = files::generation_path(root, &generation.id);
    if !files::exists(&path)? {
        return Ok(());
    }
    let directory = DirectoryLease::open(&path)?;
    for (file, digest) in generation_files(root, generation)? {
        files::remove_verified(&file, digest)?;
    }
    drop(directory);
    files::remove_empty(&path)?; // Unknown files/directories are intentionally retained.
    Ok(())
}
