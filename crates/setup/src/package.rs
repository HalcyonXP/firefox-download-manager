//! Bounded local package verification, never a downloader or updater.
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{EXTENSION_FILE, HELPER_FILE, SETUP_FILE, SetupError};

/// Only these fixed leaf files can be consumed from a candidate package.
pub const PACKAGE_FILES: [&str; 7] = [
    HELPER_FILE,
    SETUP_FILE,
    EXTENSION_FILE,
    "INSTALL.md",
    "SECURITY.md",
    "THIRD-PARTY-NOTICES.txt",
    "BUILD-INFO.json",
];
const DESCRIPTOR_LIMIT: u64 = 64 * 1024;
const FILE_LIMIT: u64 = 256 * 1024 * 1024;
const HEX: &[u8; 16] = b"0123456789abcdef";

/// Bounded metadata: matching hashes establish content consistency, not publisher authenticity.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    /// Fixed type marker, not a path or URL.
    pub format: String,
    /// Descriptor schema version, separate from application version.
    pub version: u8,
    /// Paired helper/setup/extension version.
    pub package_version: String,
    /// Public canonical repository identifier.
    pub repository: String,
    /// Full public source commit identifier.
    pub commit: String,
    /// Fixed target identity.
    pub target: String,
    /// Exactly the fixed payload leaves and their SHA-256 digests.
    #[serde(deserialize_with = "unique_files")]
    pub files: BTreeMap<String, String>,
}

impl Descriptor {
    /// Rejects unknown authority, incomplete/mixed packages and path-like keys.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.format == "firefox-download-manager-package"
            && self.version == 1
            && self.package_version == env!("CARGO_PKG_VERSION")
            && self.repository == "HalcyonXP/firefox-download-manager"
            && self.target == "x86_64-pc-windows-msvc"
            && self.commit.len() == 40
            && self.commit.bytes().all(|byte| byte.is_ascii_hexdigit())
            && self.files.len() == PACKAGE_FILES.len()
            && PACKAGE_FILES.iter().all(|name| {
                self.files
                    .get(*name)
                    .is_some_and(|digest| valid_digest(digest))
            })
    }
}

/// Content-consistent local source, not a signature/provenance attestation.
/// Source paths are deliberately not Debug-renderable.
pub struct VerifiedPackage {
    root: PathBuf,
    descriptor: Descriptor,
    #[cfg(windows)]
    _directory: crate::paths::DirectoryLease,
}
impl VerifiedPackage {
    /// Checks every bounded payload before callers may stage installation.
    ///
    /// # Errors
    /// Rejects nonordinary entries, malformed descriptors and checksum mismatch.
    /// The package retains Windows directory ownership during staging; copied
    /// bytes must still be rechecked before publication to close a stale-proof gap.
    pub fn open(root: &Path) -> Result<Self, SetupError> {
        #[cfg(windows)]
        let directory = crate::paths::DirectoryLease::open(root)?;
        #[cfg(windows)]
        let root = directory.path();
        let mut bytes = Vec::new();
        ordinary_read(&root.join("package.json"))?
            .take(DESCRIPTOR_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| SetupError::Package)?;
        if u64::try_from(bytes.len()).map_err(|_| SetupError::Package)? > DESCRIPTOR_LIMIT {
            return Err(SetupError::Package);
        }
        let descriptor: Descriptor =
            serde_json::from_slice(&bytes).map_err(|_| SetupError::Package)?;
        if !descriptor.is_valid() {
            return Err(SetupError::Package);
        }
        for (name, expected) in &descriptor.files {
            if !file_hash(&root.join(name))?.eq_ignore_ascii_case(expected) {
                return Err(SetupError::Package);
            }
        }
        Ok(Self {
            root: root.to_owned(),
            descriptor,
            #[cfg(windows)]
            _directory: directory,
        })
    }

    /// Only a fixed verified package leaf is addressable.
    /// # Errors
    /// Rejects arbitrary names rather than joining them to a trusted directory.
    pub fn payload(&self, name: &str) -> Result<PathBuf, SetupError> {
        if !PACKAGE_FILES.contains(&name) {
            return Err(SetupError::Package);
        }
        Ok(self.root.join(name))
    }

    /// Expected hash to recheck after copying from a verified source.
    #[must_use]
    pub fn expected(&self, name: &str) -> Option<&str> {
        self.descriptor.files.get(name).map(String::as_str)
    }

    /// Paired source package version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.descriptor.package_version
    }
}

/// Exact SHA-256 grammar; values are never interpreted as paths.
#[must_use]
pub fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Stream the actual ordinary file with a fixed buffer and a hard size ceiling.
/// # Errors
/// Rejects unsafe entries, unavailable/oversized reads and I/O failure.
pub fn file_hash(path: &Path) -> Result<String, SetupError> {
    let mut file = ordinary_read(path)?;
    if file.metadata().map_err(|_| SetupError::Package)?.len() > FILE_LIMIT {
        return Err(SetupError::Package);
    }
    let mut buffer = vec![0_u8; 256 * 1024];
    let mut total = 0_u64;
    let mut hasher = Sha256::new();
    loop {
        let count = file.read(&mut buffer).map_err(|_| SetupError::Package)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| SetupError::Package)?)
            .ok_or(SetupError::Package)?;
        if total > FILE_LIMIT {
            return Err(SetupError::Package);
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect())
}

fn ordinary_read(path: &Path) -> Result<File, SetupError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| SetupError::Package)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(SetupError::Package);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(SetupError::Package);
        }
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x0020_0000); // FILE_SHARE_READ: deny source mutation/deletion while reading.
    }
    let file = options.open(path).map_err(|_| SetupError::Package)?;
    let opened = file.metadata().map_err(|_| SetupError::Package)?;
    if !opened.is_file() {
        return Err(SetupError::Package);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if opened.file_attributes() & 0x400 != 0 {
            return Err(SetupError::Package);
        }
    }
    Ok(file)
}

fn unique_files<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    struct UniqueFiles;
    impl<'de> serde::de::Visitor<'de> for UniqueFiles {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("fixed unique package leaves")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut files = BTreeMap::new();
            while let Some((name, digest)) = map.next_entry::<String, String>()? {
                if files.len() >= PACKAGE_FILES.len()
                    || !PACKAGE_FILES.contains(&name.as_str())
                    || !valid_digest(&digest)
                    || files.insert(name, digest).is_some()
                {
                    return Err(serde::de::Error::custom(
                        "invalid or duplicate package leaf",
                    ));
                }
            }
            Ok(files)
        }
    }
    deserializer.deserialize_map(UniqueFiles)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn descriptor() -> Descriptor {
        Descriptor {
            format: "firefox-download-manager-package".into(),
            version: 1,
            package_version: env!("CARGO_PKG_VERSION").into(),
            repository: "HalcyonXP/firefox-download-manager".into(),
            commit: "a".repeat(40),
            target: "x86_64-pc-windows-msvc".into(),
            files: PACKAGE_FILES
                .iter()
                .map(|name| ((*name).into(), "a".repeat(64)))
                .collect(),
        }
    }
    #[test]
    fn duplicate_payload_members_are_rejected_before_interpretation() {
        let value = serde_json::to_string(&descriptor()).expect("descriptor");
        let duplicate = value.replace(
            "\"files\":{",
            &format!("\"files\":{{\"{HELPER_FILE}\":\"{}\",", "a".repeat(64)),
        );
        assert!(serde_json::from_str::<Descriptor>(&duplicate).is_err());
    }

    #[test]
    fn descriptor_is_closed_fixed_scope_and_paired_version() {
        let base = descriptor();
        assert!(base.is_valid());
        for (field, value) in [
            ("format", "other"),
            ("package_version", "99.0.0"),
            ("repository", "other/other"),
            ("target", "other"),
            ("commit", "bad"),
        ] {
            let mut json = serde_json::to_value(&base).expect("fixture");
            json[field] = value.into();
            assert!(
                !serde_json::from_value::<Descriptor>(json)
                    .expect("shape")
                    .is_valid()
            );
        }
        let mut bad = descriptor();
        bad.files.remove(HELPER_FILE);
        bad.files.insert("../outside.exe".into(), "a".repeat(64));
        assert!(!bad.is_valid());
        let mut bad = descriptor();
        bad.files.insert(HELPER_FILE.into(), "g".repeat(64));
        assert!(!bad.is_valid());
        let mut json = serde_json::to_value(base).expect("fixture");
        json["update_url"] = "https://example.test/update".into();
        assert!(serde_json::from_value::<Descriptor>(json).is_err());
    }
}
