//! Versioned Native Messaging protocol and strict framing boundaries.
//!
//! Wire bodies are UTF-8 JSON objects prefixed by Firefox Native Messaging's
//! four-byte little-endian body length. Input is rejected if it is oversized,
//! truncated, duplicated, malformed, or outside the exact protocol-v2 shape.

mod framing;
mod strict_json;
mod v2;

pub use framing::{FrameReadError, FrameWriteError, read_frame, write_frame};
pub use v2::{
    AddPayload, CancelPartial, CancelPayload, Command, CommandDecodeError, CommandDecodeFailure,
    CommandMessage, ErrorCode, ErrorContext, EventMessage, EventName, FailedData, HelloPayload,
    HelloResult, ListPayload, MessageBuildError, ProgressData, ProtocolError, RemovePayload,
    RemoveResult, ResponseCommand, ResponseMessage, SettingsDescription, SettingsPatchInput,
    SnapshotPage, StateChangedData, TaskDescription, TaskIdPayload, TaskStateName,
    TransferModeName, UpdateSettingsPayload, WarningData, decode_command,
};

/// Current wire-protocol major version.
pub const PROTOCOL_VERSION: u16 = 2;
/// Maximum JSON body accepted or emitted by the native host.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// Maximum correlation identifier length.
pub const MAX_CORRELATION_ID_CHARS: usize = 128;

/// Returns whether a correlation identifier satisfies the v1 wire grammar.
#[must_use]
pub fn is_valid_correlation_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() <= MAX_CORRELATION_ID_CHARS
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::is_valid_correlation_id;

    #[test]
    fn correlation_ids_are_bounded_non_secret_tokens() {
        for value in ["a", "request-1", "ui.start:4", &"a".repeat(128)] {
            assert!(is_valid_correlation_id(value));
        }
        for value in [
            "",
            "-starts-with-punctuation",
            "contains space",
            "url?secret=1",
            &"a".repeat(129),
        ] {
            assert!(!is_valid_correlation_id(value));
        }
    }
}
