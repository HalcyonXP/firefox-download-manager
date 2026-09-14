use std::io::{self, Read, Write};

use serde::Serialize;
use thiserror::Error;

use crate::MAX_MESSAGE_BYTES;

/// Path- and payload-free Native Messaging frame read failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FrameReadError {
    /// The input stream failed.
    #[error("native message input failed")]
    Io {
        /// Stable standard-library I/O classification.
        kind: io::ErrorKind,
    },
    /// EOF occurred after only part of the four-byte prefix.
    #[error("native message length prefix was truncated")]
    TruncatedPrefix,
    /// The declared body exceeds the project limit and was not allocated.
    #[error("native message exceeds the configured size limit")]
    MessageTooLarge {
        /// Untrusted declared body length.
        declared: u32,
    },
    /// EOF occurred before the complete declared body arrived.
    #[error("native message body was truncated")]
    TruncatedBody {
        /// Declared body bytes.
        declared: u32,
        /// Body bytes actually read.
        received: u32,
    },
}

/// Path- and payload-free Native Messaging frame write failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FrameWriteError {
    /// JSON serialization failed.
    #[error("native message serialization failed")]
    Serialization,
    /// Native Messaging bodies must be JSON objects.
    #[error("native message root must be an object")]
    NonObject,
    /// The serialized body exceeds the project limit.
    #[error("native message exceeds the configured size limit")]
    MessageTooLarge,
    /// The output stream failed.
    #[error("native message output failed")]
    Io {
        /// Stable standard-library I/O classification.
        kind: io::ErrorKind,
    },
}

/// Reads one little-endian length-prefixed Native Messaging body.
///
/// `Ok(None)` is returned only for clean EOF before any prefix byte. A declared
/// body over one MiB is rejected before allocating or reading that body.
///
/// # Errors
///
/// Returns a bounded, payload-free error for truncated, oversized, or failed
/// input.
pub fn read_frame(reader: &mut impl Read) -> Result<Option<Vec<u8>>, FrameReadError> {
    let mut prefix = [0_u8; 4];
    let prefix_bytes = read_until_full(reader, &mut prefix)
        .map_err(|error| FrameReadError::Io { kind: error.kind() })?;
    if prefix_bytes == 0 {
        return Ok(None);
    }
    if prefix_bytes != prefix.len() {
        return Err(FrameReadError::TruncatedPrefix);
    }

    let declared = u32::from_le_bytes(prefix);
    let declared_usize = usize::try_from(declared).unwrap_or(usize::MAX);
    if declared_usize > MAX_MESSAGE_BYTES {
        return Err(FrameReadError::MessageTooLarge { declared });
    }

    let mut body = vec![0_u8; declared_usize];
    let received = read_until_full(reader, &mut body)
        .map_err(|error| FrameReadError::Io { kind: error.kind() })?;
    if received != declared_usize {
        return Err(FrameReadError::TruncatedBody {
            declared,
            received: u32::try_from(received).unwrap_or(u32::MAX),
        });
    }
    Ok(Some(body))
}

/// Serializes and completely writes one little-endian Native Messaging frame.
///
/// The writer is flushed after the complete prefix and body are written.
///
/// # Errors
///
/// Rejects serialization or bodies above one MiB and returns a payload-free
/// output classification for write/flush failures.
pub fn write_frame(
    writer: &mut impl Write,
    message: &impl Serialize,
) -> Result<(), FrameWriteError> {
    let body = encode_frame_body(message)?;
    let length = u32::try_from(body.len()).map_err(|_| FrameWriteError::MessageTooLarge)?;
    writer
        .write_all(&length.to_le_bytes())
        .and_then(|()| writer.write_all(&body))
        .and_then(|()| writer.flush())
        .map_err(|error| FrameWriteError::Io { kind: error.kind() })
}

/// Encodes the same bounded object body for native stdio and local transport.
/// No transport flush, delivery or command-acceptance semantics are implied.
/// # Errors
/// Returns only fixed serialization, non-object or oversized-body failures.
pub fn encode_frame_body(message: &impl Serialize) -> Result<Vec<u8>, FrameWriteError> {
    let body = serde_json::to_vec(message).map_err(|_| FrameWriteError::Serialization)?;
    if body.first() != Some(&b'{') {
        return Err(FrameWriteError::NonObject);
    }
    if body.len() > MAX_MESSAGE_BYTES {
        return Err(FrameWriteError::MessageTooLarge);
    }
    Ok(body)
}

fn read_until_full(reader: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor, Read, Write};

    use serde_json::json;

    use super::{FrameReadError, FrameWriteError, read_frame, write_frame};
    use crate::MAX_MESSAGE_BYTES;

    struct OneByteReader<R>(R);

    impl<R: Read> Read for OneByteReader<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let amount = buffer.len().min(1);
            self.0.read(&mut buffer[..amount])
        }
    }

    #[derive(Default)]
    struct ShortWriter {
        bytes: Vec<u8>,
    }

    impl Write for ShortWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            let amount = buffer.len().min(2);
            self.bytes.extend_from_slice(&buffer[..amount]);
            Ok(amount)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn partial_reads_and_writes_preserve_exact_frames() {
        let message = json!({"protocol_version": 2, "kind": "test"});
        let mut writer = ShortWriter::default();
        write_frame(&mut writer, &message).expect("write frame");

        let mut reader = OneByteReader(Cursor::new(writer.bytes));
        let body = read_frame(&mut reader)
            .expect("read frame")
            .expect("message body");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).expect("decode body"),
            message
        );
        assert_eq!(read_frame(&mut reader), Ok(None));
    }

    #[test]
    fn writer_rejects_non_object_json() {
        assert_eq!(
            write_frame(&mut Vec::new(), &"not an object"),
            Err(FrameWriteError::NonObject)
        );
    }

    #[test]
    fn clean_and_truncated_eof_are_distinct() {
        assert_eq!(read_frame(&mut Cursor::new(Vec::<u8>::new())), Ok(None));
        assert_eq!(
            read_frame(&mut Cursor::new(vec![1, 0, 0])),
            Err(FrameReadError::TruncatedPrefix)
        );

        let mut bytes = 4_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(b"ab");
        assert_eq!(
            read_frame(&mut Cursor::new(bytes)),
            Err(FrameReadError::TruncatedBody {
                declared: 4,
                received: 2,
            })
        );
    }

    #[test]
    fn oversized_declaration_is_rejected_before_body_read() {
        let declared = u32::try_from(MAX_MESSAGE_BYTES + 1).expect("limit fits");
        let mut reader = Cursor::new(declared.to_le_bytes());
        assert_eq!(
            read_frame(&mut reader),
            Err(FrameReadError::MessageTooLarge { declared })
        );
        assert_eq!(reader.position(), 4);
    }
}
