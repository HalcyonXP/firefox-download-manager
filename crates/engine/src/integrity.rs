//! Immutable user-supplied SHA-256 expectations. Hashing lives at owned storage.

use std::fmt;

/// A syntactically valid SHA-256 digest; deliberately not generic serializable data.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ExpectedSha256([u8; 32]);

impl ExpectedSha256 {
    /// Accepts exactly 64 hexadecimal characters, case-insensitively.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let mut bytes = [0; 32];
        for (index, output) in bytes.iter_mut().enumerate() {
            *output = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
        }
        Some(Self(bytes))
    }

    /// Canonical lowercase representation for protected task metadata.
    #[must_use]
    pub fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        self.0
            .iter()
            .flat_map(|byte| {
                [
                    char::from(HEX[usize::from(byte >> 4)]),
                    char::from(HEX[usize::from(byte & 15)]),
                ]
            })
            .collect()
    }

    pub(crate) const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

impl fmt::Debug for ExpectedSha256 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ExpectedSha256(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::ExpectedSha256;
    #[test]
    fn digest_is_strict_and_canonical_without_debug_disclosure() {
        let input = "A1".repeat(32);
        let digest = ExpectedSha256::parse(&input).expect("digest");
        assert_eq!(digest.to_hex(), input.to_lowercase());
        assert!(!format!("{digest:?}").contains("a1"));
        for invalid in [
            "g".repeat(64),
            "a".repeat(63),
            "a".repeat(65),
            "é".repeat(32),
        ] {
            assert!(ExpectedSha256::parse(&invalid).is_none());
        }
    }
}
