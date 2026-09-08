//! Closed installation ownership/journal shapes, separate from download state.
use std::collections::BTreeSet;

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::SetupError;
use crate::package::valid_digest;

/// Bounded history: cleanup must preserve unknown or still-used generations.
pub const MAX_GENERATIONS: usize = 4;
/// Installation records never contain payload bytes or task metadata.
pub const RECORD_LIMIT: usize = 64 * 1024;

/// Fixed-file digests for one immutable, locally owned generation.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generation {
    /// Canonical random directory identifier, never an arbitrary relative path.
    pub id: String,
    /// Application version, not a directory name or state version.
    pub package_version: String,
    /// Helper bytes in this generation.
    pub helper_sha256: String,
    /// Explicitly loaded unsigned XPI bytes.
    pub extension_sha256: String,
    /// Generated fixed-principal native manifest bytes.
    pub manifest_sha256: String,
}
impl Generation {
    /// Validates every persisted field before it can affect filesystem ownership.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        canonical_id(&self.id)
            && version_label(&self.package_version)
            && [
                &self.helper_sha256,
                &self.extension_sha256,
                &self.manifest_sha256,
            ]
            .into_iter()
            .all(|digest| valid_digest(digest))
    }
}

/// Current generation and known historical ownership; no task/download paths.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    /// Fixed record identity.
    pub format: String,
    /// Receipt schema, unrelated to task/settings/wire versions.
    pub version: u8,
    /// Stable identity of this install root across compatible upgrades.
    pub installation_id: String,
    /// One listed generation to which registration may point.
    pub current: String,
    /// Only proven generated directories are eligible for cleanup.
    pub generations: Vec<Generation>,
}
impl Receipt {
    /// Checks identity, bounds and unique/current generation coverage.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        if self.format != "firefox-download-manager-installation"
            || self.version != 1
            || !canonical_id(&self.installation_id)
            || self.generations.is_empty()
            || self.generations.len() > MAX_GENERATIONS
        {
            return false;
        }
        let mut ids = BTreeSet::new();
        self.generations
            .iter()
            .all(|generation| generation.is_valid() && ids.insert(&generation.id))
            && ids.contains(&self.current)
    }
    /// Bounded strict decoding; missing, duplicate and future fields fail closed.
    /// # Errors
    /// Returns ownership failure without exposing raw persisted content.
    pub fn decode(bytes: &[u8]) -> Result<Self, SetupError> {
        if bytes.len() > RECORD_LIMIT {
            return Err(SetupError::Ownership);
        }
        let receipt: Self = serde_json::from_slice(bytes).map_err(|_| SetupError::Ownership)?;
        if !receipt.is_valid() {
            return Err(SetupError::Ownership);
        }
        Ok(receipt)
    }
    /// Current metadata only after full record validation.
    /// # Errors
    /// Rejects invalid or inconsistent caller-constructed records too.
    pub fn current_generation(&self) -> Result<&Generation, SetupError> {
        if !self.is_valid() {
            return Err(SetupError::Ownership);
        }
        self.generations
            .iter()
            .find(|generation| generation.id == self.current)
            .ok_or(SetupError::Ownership)
    }
}

/// Recovery action is closed, never a command string or arbitrary path list.
#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Install,
    Uninstall,
}

/// Write before mutation. Recovery must also verify actual files and registration.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    /// Fixed transaction record identity.
    pub format: String,
    /// Journal format version.
    pub version: u8,
    /// Unique transaction identifier.
    pub transaction_id: String,
    /// Install versus uninstall determines allowed state edges.
    pub action: Action,
    /// Presence is required, even for a null first-install predecessor.
    #[serde(deserialize_with = "nullable_receipt")]
    pub previous: Option<Receipt>,
    /// Presence is required, even for a null uninstall successor.
    #[serde(deserialize_with = "nullable_receipt")]
    pub next: Option<Receipt>,
    /// Whether rollback may restore the previous matching registration.
    pub had_registration: bool,
}
impl Journal {
    /// Never infer ownership from a malformed or cross-install transition.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        if self.format != "firefox-download-manager-setup-transaction"
            || self.version != 1
            || !canonical_id(&self.transaction_id)
            || self
                .previous
                .as_ref()
                .is_some_and(|receipt| !receipt.is_valid())
            || self
                .next
                .as_ref()
                .is_some_and(|receipt| !receipt.is_valid())
            || (self.had_registration && self.previous.is_none())
        {
            return false;
        }
        match self.action {
            Action::Uninstall => self.previous.is_some() && self.next.is_none(),
            Action::Install => self.next.as_ref().is_some_and(|next| {
                if let Some(previous) = &self.previous {
                    previous.installation_id == next.installation_id
                        && !previous
                            .generations
                            .iter()
                            .any(|old| old.id == next.current)
                        && next.generations.len() == previous.generations.len() + 1
                        && previous
                            .generations
                            .iter()
                            .all(|old| next.generations.contains(old))
                } else {
                    next.generations.len() == 1
                }
            }),
        }
    }
    /// Strict bounded recovery decoding, not authority to blindly replay writes.
    /// # Errors
    /// Returns a fixed recovery failure; callers preserve inconsistent content.
    pub fn decode(bytes: &[u8]) -> Result<Self, SetupError> {
        if bytes.len() > RECORD_LIMIT {
            return Err(SetupError::Recovery);
        }
        let journal: Self = serde_json::from_slice(bytes).map_err(|_| SetupError::Recovery)?;
        if !journal.is_valid() {
            return Err(SetupError::Recovery);
        }
        Ok(journal)
    }
}
fn nullable_receipt<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Receipt>, D::Error> {
    Option::<Receipt>::deserialize(deserializer)
}
fn canonical_id(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| {
        id.get_version_num() == 4
            && id.get_variant() == uuid::Variant::RFC4122
            && id.hyphenated().to_string() == value
    })
}
fn version_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value.as_bytes()[0].is_ascii_digit()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn generation() -> Generation {
        Generation {
            id: Uuid::new_v4().to_string(),
            package_version: "0.1.0".into(),
            helper_sha256: "a".repeat(64),
            extension_sha256: "b".repeat(64),
            manifest_sha256: "c".repeat(64),
        }
    }
    fn receipt() -> Receipt {
        let generation = generation();
        Receipt {
            format: "firefox-download-manager-installation".into(),
            version: 1,
            installation_id: Uuid::new_v4().to_string(),
            current: generation.id.clone(),
            generations: vec![generation],
        }
    }
    #[test]
    fn receipt_cannot_invent_duplicate_missing_or_path_like_ownership() {
        let good = receipt();
        assert!(good.is_valid());
        let mut bad = good.clone();
        bad.generations.push(bad.generations[0].clone());
        assert!(!bad.is_valid());
        let mut bad = good.clone();
        bad.current = Uuid::new_v4().to_string();
        assert!(!bad.is_valid());
        let mut bad = good.clone();
        bad.generations[0].id = "../state".into();
        assert!(!bad.is_valid());
        let mut json = serde_json::to_value(good).expect("fixture");
        json["unknown"] = true.into();
        assert!(Receipt::decode(&serde_json::to_vec(&json).expect("JSON")).is_err());
    }
    #[test]
    fn journal_requires_explicit_nullable_edges_and_same_install_ownership() {
        let previous = receipt();
        let mut next = previous.clone();
        let new = generation();
        next.current = new.id.clone();
        next.generations.push(new);
        let journal = Journal {
            format: "firefox-download-manager-setup-transaction".into(),
            version: 1,
            transaction_id: Uuid::new_v4().to_string(),
            action: Action::Install,
            previous: Some(previous),
            next: Some(next),
            had_registration: true,
        };
        assert!(journal.is_valid());
        let mut bad = journal.clone();
        bad.next.as_mut().expect("next").installation_id = Uuid::new_v4().to_string();
        assert!(!bad.is_valid());
        let mut json = serde_json::to_value(&journal).expect("fixture");
        json.as_object_mut().expect("object").remove("previous");
        assert!(Journal::decode(&serde_json::to_vec(&json).expect("JSON")).is_err());
        let mut uninstall = journal;
        uninstall.action = Action::Uninstall;
        uninstall.next = None;
        assert!(uninstall.is_valid());
        let mut json = serde_json::to_value(uninstall).expect("fixture");
        json.as_object_mut().expect("object").remove("next");
        assert!(Journal::decode(&serde_json::to_vec(&json).expect("JSON")).is_err());
    }
}
