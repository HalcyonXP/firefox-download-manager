//! Native Messaging protocol boundary.
//!
//! Full message types will be implemented from `protocol/schema/v1` as the
//! native host is built. These constants and boundary checks keep both project
//! halves aligned during scaffolding.

/// The only protocol major currently supported.
pub const PROTOCOL_VERSION: u16 = 1;

/// Project-level maximum JSON body size in either direction.
pub const MAX_MESSAGE_BYTES: usize = 1_048_576;

/// Returns whether a correlation identifier satisfies the v1 wire grammar.
#[must_use]
pub fn is_valid_correlation_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    let Some(first) = bytes.first() else {
        return false;
    };

    bytes.len() <= 128
        && first.is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::is_valid_correlation_id;

    #[test]
    fn accepts_v1_correlation_ids() {
        for value in ["a", "request-1", "ui.start:4", &"a".repeat(128)] {
            assert!(is_valid_correlation_id(value), "expected {value:?} to pass");
        }
    }

    #[test]
    fn rejects_unsafe_correlation_ids() {
        for value in [
            "",
            "-starts-with-punctuation",
            "contains space",
            "url?secret=1",
            &"a".repeat(129),
        ] {
            assert!(
                !is_valid_correlation_id(value),
                "expected {value:?} to fail"
            );
        }
    }
}
