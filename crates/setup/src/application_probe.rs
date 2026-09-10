//! Closed, non-engine application launch probe for paired setup candidates.
//! This reports compiled entry compatibility, not installed/tray/browser readiness.
use crate::SetupError;
use serde::{Deserialize, Serialize};

/// Fixed internal argument; never a native-message command or supplied path.
pub const ARGUMENT: &str = "--package-probe";
const LIMIT: usize = 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    format: String,
    version: u32,
    package_version: String,
    protocol_version: u16,
    entry_modes_version: u32,
}

/// Emit compiled compatibility only. Does not open a runtime, receipt or state.
/// # Errors
/// Refuses serialization failure.
pub fn report() -> Result<Vec<u8>, SetupError> {
    serde_json::to_vec(&Report {
        format: "firefox-download-manager-application-probe".into(),
        version: 1,
        package_version: env!("CARGO_PKG_VERSION").into(),
        protocol_version: download_manager_protocol::PROTOCOL_VERSION,
        entry_modes_version: 1,
    })
    .map_err(|_| SetupError::Launch)
}

/// Validate the complete bounded response, not just a selected JSON field.
/// # Errors
/// Refuses extra/duplicate fields, incompatible versions and oversized output.
pub fn verify(bytes: &[u8]) -> Result<(), SetupError> {
    if bytes.is_empty() || bytes.len() > LIMIT {
        return Err(SetupError::Launch);
    }
    let value: Report = serde_json::from_slice(bytes).map_err(|_| SetupError::Launch)?;
    if value.format != "firefox-download-manager-application-probe"
        || value.version != 1
        || value.package_version != env!("CARGO_PKG_VERSION")
        || value.protocol_version != download_manager_protocol::PROTOCOL_VERSION
        || value.entry_modes_version != 1
    {
        return Err(SetupError::Launch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn closed_compatibility_response_is_not_a_readiness_receipt() {
        let bytes = super::report().unwrap();
        super::verify(&bytes).unwrap();
        let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for (field, bad) in [
            ("format", serde_json::json!("other")),
            ("version", serde_json::json!(2)),
            ("package_version", serde_json::json!("0.0.0")),
            ("protocol_version", serde_json::json!(1)),
            ("entry_modes_version", serde_json::json!(0)),
            ("ready", serde_json::json!(true)),
        ] {
            let mut changed = original.clone();
            changed[field] = bad;
            assert!(super::verify(&serde_json::to_vec(&changed).unwrap()).is_err());
        }
        let mut padded = bytes.clone();
        padded.resize(super::LIMIT, b' ');
        super::verify(&padded).unwrap();
        padded.push(b' ');
        assert!(super::verify(&padded).is_err());
        let duplicate = String::from_utf8(bytes)
            .unwrap()
            .replacen('{', "{\"version\":1,", 1);
        assert!(super::verify(duplicate.as_bytes()).is_err());
    }
}
