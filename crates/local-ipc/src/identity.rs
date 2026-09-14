//! Read-only current-process identity adapter. No profile/registry/credential
//! inspection. The Windows-supplied whoami child is retained, bounded and joined;
//! only its SID is retained in memory, never its account name or raw diagnostics.
use std::{
    io::Read,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::Error;

const OUTPUT_LIMIT: u64 = 8 * 1024;
const EXECUTION_LIMIT: Duration = Duration::from_secs(2);

/// An observed process-user SID, not installation or engine authority.
/// Raw account-name output is discarded. Deliberately not Debug/Serialize.
pub struct CurrentUser(String);

impl CurrentUser {
    /// Observe the current user using the bounded, joined OS identity adapter.
    /// # Errors
    /// Refuses unsupported identities, malformed output and failed cleanup.
    pub fn observe() -> Result<Self, Error> {
        observed_sid().map(Self)
    }

    /// SID for reviewed OS security-descriptor construction; do not log it.
    #[must_use]
    pub fn sid(&self) -> &str {
        &self.0
    }

    fn pipe_descriptor(&self) -> String {
        let sid = self.sid();
        format!("O:{sid}D:P(A;;GA;;;{sid})")
    }
}

pub(crate) fn current_user_descriptor() -> Result<String, Error> {
    CurrentUser::observe().map(|user| user.pipe_descriptor())
}

fn observed_sid() -> Result<String, Error> {
    // An OS API, NOT PATH/SystemRoot/environment executable discovery.
    let directory = winsafe::GetSystemDirectory().map_err(|_| Error::Identity)?;
    let executable = PathBuf::from(directory).join("whoami.exe");
    if !executable.is_absolute() {
        return Err(Error::Identity);
    }
    let mut child = Command::new(executable)
        .args(["/user", "/fo", "csv", "/nh"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW: no flashing auxiliary console.
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| Error::Identity)?;
    let Some(stdout) = child.stdout.take() else {
        retire(&mut child)?;
        return Err(Error::Identity);
    };
    let Ok(reader) = thread::Builder::new()
        .name("local-identity-reader".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .take(OUTPUT_LIMIT + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        })
    else {
        retire(&mut child)?;
        return Err(Error::Identity);
    };
    let deadline = Instant::now() + EXECUTION_LIMIT;
    let succeeded = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            _ => break false,
        }
    };
    // Cleanup before every outcome. kill/wait uses only Child's retained handle,
    // never a PID lookup. Windows whoami does not delegate stdout to descendants.
    let cleanup = retire(&mut child);
    let output = reader.join().map_err(|_| Error::Identity)?;
    cleanup?;
    if !succeeded {
        return Err(Error::Identity);
    }
    parse_sid(&output.map_err(|_| Error::Identity)?)
}

fn retire(child: &mut Child) -> Result<(), Error> {
    if !matches!(child.try_wait(), Ok(Some(_))) {
        // A simultaneous normal exit can make kill fail; the exact wait, not a
        // name/PID fallback, establishes whether the retained process is joined.
        let _ = child.kill();
    }
    child.wait().map(|_| ()).map_err(|_| Error::Identity)
}

fn parse_sid(output: &[u8]) -> Result<String, Error> {
    if output.len() > usize::try_from(OUTPUT_LIMIT).expect("small output bound") {
        return Err(Error::Identity);
    }
    let line = output.trim_ascii();
    if line.first() != Some(&b'"') || line.iter().any(u8::is_ascii_control) {
        return Err(Error::Identity);
    }
    // Parse exactly two CSV fields. The first may be non-UTF8 OEM text and escaped
    // quotes; it is discarded, not decoded, printed, cached or used as identity.
    let mut offset = 1;
    while offset < line.len() {
        if line[offset] != b'"' {
            offset += 1;
        } else if line.get(offset + 1) == Some(&b'"') {
            offset += 2;
        } else {
            break;
        }
    }
    let sid = line
        .get(offset..)
        .and_then(|rest| rest.strip_prefix(b"\",\""))
        .and_then(|rest| rest.strip_suffix(b"\""))
        .ok_or(Error::Identity)?;
    let sid = std::str::from_utf8(sid).map_err(|_| Error::Identity)?;
    let fields: Vec<_> = sid.split('-').collect();
    // Per-user local/domain or Microsoft Entra identity, not SYSTEM/service SIDs.
    if fields.len() != 8
        || (fields[..4] != ["S", "1", "5", "21"] && fields[..4] != ["S", "1", "12", "1"])
        || fields[4..].iter().any(|part| {
            part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.bytes().all(|b| b.is_ascii_digit())
                || part.parse::<u32>().is_err()
        })
    {
        return Err(Error::Identity);
    }
    Ok(sid.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_canonical_current_user_sid_enters_protected_descriptor() {
        for name in [b"user".as_slice(), b"a,b", b"a\"\"b", b"\xff\xfe"] {
            let mut bytes = b"\"".to_vec();
            bytes.extend_from_slice(name);
            bytes.extend_from_slice(b"\",\"S-1-5-21-1-2-3-1001\"\r\n");
            let user = CurrentUser(parse_sid(&bytes).unwrap());
            assert_eq!(user.sid(), "S-1-5-21-1-2-3-1001");
            assert_eq!(
                user.pipe_descriptor(),
                "O:S-1-5-21-1-2-3-1001D:P(A;;GA;;;S-1-5-21-1-2-3-1001)"
            );
        }
        for bad in [
            b"\"user\",\"S-1-5-18\"".as_slice(),
            b"\"user\",\"S-1-5-21-1-2-3-01\"",
            b"\"user\",\"S-1-5-21-1-2-3-4294967296\"",
            b"\"user\",\"S-1-5-21-1-2-3-1)(A;;GA;;;WD)\"",
            b"\"user\",\"S-1-5-21-1-2-3-1\",\"extra\"",
            b"\"user\",\"S-1-5-21-1-2-3-1\"\nother",
        ] {
            assert_eq!(parse_sid(bad), Err(Error::Identity));
        }
        assert_eq!(parse_sid(&vec![b'x'; 8193]), Err(Error::Identity));
    }

    #[test]
    fn windows_current_identity_lookup_is_read_only_and_joined() {
        // Do not print the returned descriptor or include it in assertion output.
        assert!(current_user_descriptor().is_ok());
    }
}
