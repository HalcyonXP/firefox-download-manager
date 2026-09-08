//! Bounded owned-child checks. No process termination by name or profile inspection.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use download_manager_protocol::{read_frame, write_frame};
use serde_json::{Value, json};

use crate::files;
use crate::paths::DirectoryLease;
use crate::transaction::InstallProbe;
use crate::{HELPER_FILE, SetupError};

const DEADLINE: Duration = Duration::from_secs(15);
const OUTPUT_LIMIT: u64 = 64 * 1024;
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn reader(stream: impl Read + Send + 'static, limit: u64) -> Receiver<std::io::Result<Vec<u8>>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stream
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = sender.send(result);
    });
    receiver
}
fn run(mut command: Command, input: Option<&[u8]>) -> Result<Vec<u8>, SetupError> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = OwnedChild(command.spawn().map_err(|_| SetupError::Launch)?);
    let output = reader(
        child.0.stdout.take().ok_or(SetupError::Launch)?,
        OUTPUT_LIMIT,
    );
    let errors = reader(child.0.stderr.take().ok_or(SetupError::Launch)?, 4096);
    let mut stdin = child.0.stdin.take().ok_or(SetupError::Launch)?;
    if let Some(input) = input {
        std::io::Write::write_all(&mut stdin, input).map_err(|_| SetupError::Launch)?;
    }
    drop(stdin); // Empty owned profile: EOF shuts down after the hello, no tasks to replay.
    let end = Instant::now() + DEADLINE;
    let status = loop {
        if let Some(status) = child.0.try_wait().map_err(|_| SetupError::Launch)? {
            break status;
        }
        if Instant::now() >= end {
            return Err(SetupError::Launch);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let remaining = || end.saturating_duration_since(Instant::now());
    let output = output
        .recv_timeout(remaining())
        .map_err(|_| SetupError::Launch)?
        .map_err(|_| SetupError::Launch)?;
    let errors = errors
        .recv_timeout(remaining())
        .map_err(|_| SetupError::Launch)?
        .map_err(|_| SetupError::Launch)?;
    if !status.success()
        || !errors.is_empty()
        || u64::try_from(output.len()).map_err(|_| SetupError::Launch)? > OUTPUT_LIMIT
    {
        return Err(SetupError::Launch);
    }
    Ok(output)
}

/// Refuse—not terminate—Firefox or existing helpers before registry/file mutation.
/// # Errors
/// Missing process inspection or matching processes fail closed. No command lines,
/// browser profiles, URLs or unrelated process records are requested or logged.
pub fn require_apps_closed() -> Result<(), SetupError> {
    let windows = PathBuf::from(std::env::var_os("WINDIR").ok_or(SetupError::Busy)?);
    let _lease = DirectoryLease::open(&windows.join("System32"))?;
    for name in ["firefox.exe", HELPER_FILE] {
        let mut command = Command::new(windows.join("System32").join("tasklist.exe"));
        command.args(["/FI", &format!("IMAGENAME eq {name}"), "/FO", "CSV", "/NH"]);
        let output = run(command, None).map_err(|_| SetupError::Busy)?;
        let lower = output.to_ascii_lowercase();
        let needle = format!("\"{name}\",");
        if lower
            .windows(needle.len())
            .any(|part| part == needle.as_bytes())
        {
            return Err(SetupError::Busy);
        }
    }
    Ok(())
}

/// Production probe; lifecycle tests use their explicitly separate adapter.
pub struct NativeProbe;
impl InstallProbe for NativeProbe {
    fn verify(&mut self, executable: &Path, application_data: &Path) -> Result<(), SetupError> {
        require_apps_closed()?;
        probe_helper(executable, application_data)?;
        require_apps_closed()
    }
}

/// Run only the verified helper in a self-created, isolated application state root.
/// Does not register a host or load a browser; useful as an independent package test.
/// # Errors
/// Invalid/failed/time-limited hello, unexpected output or cleanup failure is refused.
pub fn probe_helper(executable: &Path, application_data: &Path) -> Result<(), SetupError> {
    let parent = DirectoryLease::open(application_data)?;
    let sandbox = parent
        .path()
        .join(format!("dm-probe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&sandbox).map_err(|_| SetupError::Io)?;
    let lease = DirectoryLease::open(&sandbox)?;
    let result = probe_in(executable, &sandbox);
    // This unique root is entirely created by this process and its trusted owned
    // helper. No live profile or task state is used. std removal does not follow
    // junctions/symlinks; the parent and root are leased until the child is joined.
    drop(lease);
    let cleanup = std::fs::remove_dir_all(&sandbox).map_err(|_| SetupError::Recovery);
    result.and(cleanup)
}
fn probe_in(executable: &Path, sandbox: &Path) -> Result<(), SetupError> {
    let local = sandbox.join("Local");
    let profile = sandbox.join("Profile");
    let _local = files::create_tree(&local)?;
    let _profile = files::create_tree(&profile.join("Downloads"))?;
    let mut command = Command::new(executable);
    command
        .env("LOCALAPPDATA", &local)
        .env("APPDATA", sandbox.join("Roaming"))
        .env("USERPROFILE", &profile)
        .env("HOME", &profile)
        .current_dir(sandbox);
    let mut input = Vec::new();
    write_frame(&mut input, &json!({"protocol_version":2,"correlation_id":"setup-hello","kind":"command","command":"hello","payload":{"supported_versions":[2],"client_name":"setup-probe","client_version":env!("CARGO_PKG_VERSION")}})).map_err(|_| SetupError::Launch)?;
    let output = run(command, Some(&input))?;
    let mut stream = std::io::Cursor::new(output);
    let mut hello = false;
    let mut count = 0;
    while let Some(body) = read_frame(&mut stream).map_err(|_| SetupError::Launch)? {
        count += 1;
        if count > 8 {
            return Err(SetupError::Launch);
        }
        let value: Value = serde_json::from_slice(&body).map_err(|_| SetupError::Launch)?;
        if value["correlation_id"] == "setup-hello" && value["command"] == "hello" {
            let result = &value["result"];
            let capabilities = result["capabilities"]
                .as_array()
                .ok_or(SetupError::Launch)?;
            if hello
                || result["selected_version"] != 2
                || result["helper_version"] != env!("CARGO_PKG_VERSION")
                || ![
                    "snapshots",
                    "coalesced_progress",
                    "authenticated_requests",
                    "sha256",
                ]
                .iter()
                .all(|capability| capabilities.iter().any(|value| value == capability))
            {
                return Err(SetupError::Launch);
            }
            hello = true;
        }
    }
    if !hello {
        return Err(SetupError::Launch);
    }
    Ok(())
}
