//! One fixed current-user Firefox registration. Never enumerate unrelated hosts.
//! Operations must run under setup's cooperative per-user filesystem lock.
use std::io::ErrorKind;
use std::path::Path;

use winreg::enums::{
    HKEY_CURRENT_USER, KEY_READ, KEY_WOW64_64KEY, KEY_WRITE, REG_CREATED_NEW_KEY, REG_SZ,
};
use winreg::{RegKey, RegValue};

use crate::SetupError;

const KEY: &str = r"Software\Mozilla\NativeMessagingHosts\com.halcyonxp.firefox_download_manager";
const MAX_VALUE_BYTES: u32 = 1024;

/// A valid, exact UTF-16 `REG_SZ` default value, without a Debug/Serialize surface.
#[derive(Clone, Eq, PartialEq)]
pub struct RegistrationValue(String);
impl RegistrationValue {
    /// The manifest must already have a validated absolute path and fixed identity.
    /// # Errors
    /// Rejects non-Unicode, NUL-containing, oversized or non-drive-qualified values.
    pub fn for_manifest(path: &Path) -> Result<Self, SetupError> {
        let value = path.to_str().ok_or(SetupError::Registration)?;
        let bytes = value.as_bytes();
        if bytes.len() < 3
            || !bytes[0].is_ascii_alphabetic()
            || bytes[1] != b':'
            || bytes[2] != b'\\'
            || value.contains('\0')
            || value.encode_utf16().count() > 255
        {
            return Err(SetupError::Registration);
        }
        Ok(Self(value.to_owned()))
    }
    fn raw(&self) -> RegValue<'static> {
        RegValue {
            vtype: REG_SZ,
            bytes: self
                .0
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect(),
        }
    }
    fn from_raw(raw: &RegValue<'_>) -> Result<Self, SetupError> {
        if raw.vtype != REG_SZ
            || raw.bytes.len() < 2
            || !raw.bytes.len().is_multiple_of(2)
            || raw.bytes.len() > MAX_VALUE_BYTES as usize
        {
            return Err(SetupError::Registration);
        }
        let mut words = raw
            .bytes
            .chunks_exact(2)
            .map(|part| u16::from_le_bytes([part[0], part[1]]))
            .collect::<Vec<_>>();
        if words.pop() != Some(0) || words.contains(&0) {
            return Err(SetupError::Registration);
        }
        let value = String::from_utf16(&words).map_err(|_| SetupError::Registration)?;
        Self::for_manifest(Path::new(&value))
    }
}

/// Adapter permits filesystem/transaction tests without touching real HKCU.
pub trait RegistrationStore {
    /// Returns only a well-formed default-only key, or absence.
    /// # Errors
    /// Unknown values/subkeys/types, unavailable access and malformed text fail closed.
    fn current(&self) -> Result<Option<RegistrationValue>, SetupError>;
    /// Checks the exact expected value again immediately before modification.
    /// # Errors
    /// Refuses detected foreign changes and failed registration operations.
    /// This is not an atomic CAS against malicious non-cooperating account actors.
    fn replace_if_unchanged(
        &mut self,
        expected: Option<&RegistrationValue>,
        desired: Option<&RegistrationValue>,
    ) -> Result<(), SetupError>;
}

/// Current-user, native 64-bit registry view only. No arbitrary key constructor.
pub struct CurrentUserRegistration;
impl CurrentUserRegistration {
    fn read_key(key: &RegKey) -> Result<RegistrationValue, SetupError> {
        let metadata = key.query_info().map_err(|_| SetupError::Registration)?;
        // Bound before asking winreg to allocate the raw value. Concurrent hostile
        // account mutation is outside the cooperative setup-lock boundary.
        if metadata.sub_keys != 0
            || metadata.values != 1
            || metadata.max_value_name_len != 0
            || metadata.max_value_len > MAX_VALUE_BYTES
        {
            return Err(SetupError::Registration);
        }
        RegistrationValue::from_raw(
            &key.get_raw_value("")
                .map_err(|_| SetupError::Registration)?,
        )
    }
    fn empty(key: &RegKey) -> bool {
        key.query_info()
            .is_ok_and(|info| info.values == 0 && info.sub_keys == 0)
    }
    fn delete_empty(key: &RegKey) -> Result<(), SetupError> {
        if !Self::empty(key) {
            return Err(SetupError::Registration);
        }
        // Non-recursive: never sweep other host registrations or subkeys.
        RegKey::predef(HKEY_CURRENT_USER)
            .delete_subkey_with_flags(KEY, KEY_WOW64_64KEY)
            .map_err(|_| SetupError::Registration)
    }
}
impl RegistrationStore for CurrentUserRegistration {
    fn current(&self) -> Result<Option<RegistrationValue>, SetupError> {
        match RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(KEY, KEY_READ | KEY_WOW64_64KEY)
        {
            Ok(key) => Ok(Some(Self::read_key(&key)?)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(_) => Err(SetupError::Registration),
        }
    }
    fn replace_if_unchanged(
        &mut self,
        expected: Option<&RegistrationValue>,
        desired: Option<&RegistrationValue>,
    ) -> Result<(), SetupError> {
        if self.current()?.as_ref() != expected {
            return Err(SetupError::Registration);
        }
        if desired == expected {
            return Ok(());
        }
        if let Some(desired) = desired {
            let (key, disposition) = RegKey::predef(HKEY_CURRENT_USER)
                .create_subkey_with_flags(KEY, KEY_READ | KEY_WRITE | KEY_WOW64_64KEY)
                .map_err(|_| SetupError::Registration)?;
            let created = disposition == REG_CREATED_NEW_KEY;
            let matches = if created {
                expected.is_none() && Self::empty(&key)
            } else {
                Self::read_key(&key).is_ok_and(|value| Some(&value) == expected)
            };
            if !matches {
                if created {
                    let _ = Self::delete_empty(&key);
                }
                return Err(SetupError::Registration);
            }
            if key.set_raw_value("", &desired.raw()).is_err() {
                if created {
                    let _ = Self::delete_empty(&key);
                }
                return Err(SetupError::Registration);
            }
            if Self::read_key(&key)? != *desired {
                return Err(SetupError::Registration);
            }
        } else if let Some(expected) = expected {
            let key = RegKey::predef(HKEY_CURRENT_USER)
                .open_subkey_with_flags(KEY, KEY_READ | KEY_WRITE | KEY_WOW64_64KEY)
                .map_err(|_| SetupError::Registration)?;
            if Self::read_key(&key)? != *expected {
                return Err(SetupError::Registration);
            }
            key.delete_value("").map_err(|_| SetupError::Registration)?;
            Self::delete_empty(&key)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_bytes_do_not_accept_lossy_or_ambiguous_strings() {
        let value =
            RegistrationValue::for_manifest(Path::new(r"C:\Local App Data\host\manifest.json"))
                .expect("path");
        assert!(RegistrationValue::from_raw(&value.raw()).is_ok_and(|parsed| parsed == value));
        for bytes in [
            vec![],
            vec![0],
            vec![0, 0, 0, 0],
            vec![0, 0xd8, 0, 0],
            vec![b'C', 0, b':', 0],
        ] {
            assert!(
                RegistrationValue::from_raw(&RegValue {
                    vtype: REG_SZ,
                    bytes: bytes.into(),
                })
                .is_err()
            );
        }
        let mut wrong = value.raw();
        wrong.vtype = winreg::enums::REG_EXPAND_SZ;
        assert!(RegistrationValue::from_raw(&wrong).is_err());
    }
}
