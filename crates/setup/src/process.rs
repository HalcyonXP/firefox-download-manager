//! Bounded owned-child checks. No process termination by name or profile inspection.
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use download_manager_protocol::{read_frame, write_frame};
use serde_json::{Value, json};

use crate::files;
use crate::paths::DirectoryLease;
use crate::transaction::InstallProbe;
use crate::{HELPER_FILE, SetupError};

const DEADLINE: Duration = Duration::from_secs(15);
const OUTPUT_LIMIT: u64 = 64 * 1024;
struct OwnedChild {
    process: Child,
    readers: Vec<JoinHandle<()>>,
}
impl OwnedChild {
    fn reader(
        &mut self,
        stream: impl Read + Send + 'static,
        limit: u64,
    ) -> Result<Receiver<std::io::Result<Vec<u8>>>, SetupError> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let handle = std::thread::Builder::new()
            .name("setup-owned-output".into())
            .spawn(move || {
                let mut bytes = Vec::new();
                let result = stream
                    .take(limit + 1)
                    .read_to_end(&mut bytes)
                    .map(|_| bytes);
                let _ = sender.send(result);
            })
            .map_err(|_| SetupError::Launch)?;
        self.readers.push(handle);
        Ok(receiver)
    }
    fn retire(&mut self) -> Result<(), SetupError> {
        let mut failed = false;
        match self.process.try_wait() {
            Ok(Some(_)) => {}
            _ => {
                if self.process.kill().is_err() && !matches!(self.process.try_wait(), Ok(Some(_))) {
                    failed = true;
                }
            }
        }
        if self.process.wait().is_err() {
            failed = true;
        }
        for reader in self.readers.drain(..) {
            if reader.join().is_err() {
                failed = true;
            }
        }
        if failed {
            Err(SetupError::Launch)
        } else {
            Ok(())
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.retire();
    }
}
fn run(command: Command, input: Option<&[u8]>) -> Result<Vec<u8>, SetupError> {
    run_with_deadline(command, input, DEADLINE)
}
fn run_with_deadline(
    mut command: Command,
    input: Option<&[u8]>,
    limit: Duration,
) -> Result<Vec<u8>, SetupError> {
    // All callers use a fixed small hello or no input. Never let arbitrary input
    // turn the initial synchronous write into an unbounded request-body queue.
    if input.is_some_and(|bytes| bytes.len() > 1024) {
        return Err(SetupError::Launch);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x0800_0000);
    let end = Instant::now() + limit;
    let mut child = OwnedChild {
        process: command.spawn().map_err(|_| SetupError::Launch)?,
        readers: Vec::new(),
    };
    let result = (|| {
        let stdout = child.process.stdout.take().ok_or(SetupError::Launch)?;
        let output = child.reader(stdout, OUTPUT_LIMIT)?;
        let stderr = child.process.stderr.take().ok_or(SetupError::Launch)?;
        let errors = child.reader(stderr, 4096)?;
        let mut stdin = child.process.stdin.take().ok_or(SetupError::Launch)?;
        if let Some(input) = input {
            std::io::Write::write_all(&mut stdin, input).map_err(|_| SetupError::Launch)?;
        }
        drop(stdin);
        let status = loop {
            if let Some(status) = child.process.try_wait().map_err(|_| SetupError::Launch)? {
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
    })();
    child.retire().and(result)
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
    probe_in(executable, &sandbox)?; // Failed probe domains are preserved, not reused.
    // This unique root is entirely created by this process and its trusted owned
    // helper. No live profile or task state is used. std removal does not follow
    // junctions/symlinks; the parent and root are leased until the child is joined.
    drop(lease);
    std::fs::remove_dir_all(&sandbox).map_err(|_| SetupError::Recovery)
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

/// Paired application's probe does not create an engine or rely on registration.
/// It does not qualify tray readiness, signatures or installed browser behavior.
pub struct ApplicationProbe;
impl InstallProbe for ApplicationProbe {
    fn verify(&mut self, executable: &Path, application_data: &Path) -> Result<(), SetupError> {
        require_apps_closed()?;
        probe_application(executable, application_data)?;
        require_apps_closed()
    }
}

/// Probe a verified application in fresh owned state with the explicit metadata mode.
/// # Errors
/// Refuses incompatible/failed output and joined cleanup failures. Failed owned
/// probe domains are preserved; normal profiles and registration are not read.
pub fn probe_application(executable: &Path, application_data: &Path) -> Result<(), SetupError> {
    let parent = DirectoryLease::open(application_data)?;
    let sandbox = parent
        .path()
        .join(format!("dm-app-probe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&sandbox).map_err(|_| SetupError::Io)?;
    let lease = DirectoryLease::open(&sandbox)?;
    let mut command = Command::new(executable);
    command
        .arg(crate::application_probe::ARGUMENT)
        .env("LOCALAPPDATA", sandbox.join("Local"))
        .env("APPDATA", sandbox.join("Roaming"))
        .env("USERPROFILE", sandbox.join("Profile"))
        .env("HOME", sandbox.join("Profile"))
        .current_dir(&sandbox);
    let bytes = run(command, None)?;
    crate::application_probe::verify(&bytes)?;
    // A metadata-only probe must not have created state or arbitrary files.
    if std::fs::read_dir(&sandbox)
        .map_err(|_| SetupError::Io)?
        .next()
        .is_some()
    {
        return Err(SetupError::Launch);
    }
    drop(lease);
    std::fs::remove_dir(&sandbox).map_err(|_| SetupError::Recovery)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn shell(script: &str) -> Command {
        let mut command = Command::new(
            PathBuf::from(std::env::var_os("WINDIR").unwrap())
                .join("System32/WindowsPowerShell/v1.0/powershell.exe"),
        );
        command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        command
    }
    #[test]
    fn retained_process_readers_join_for_success_error_and_observation_timeout() {
        assert_eq!(
            run(shell("[Console]::Out.Write('fixture')"), None).unwrap(),
            b"fixture"
        );
        assert!(run(shell("[Console]::Error.Write('fixture'); exit 1"), None).is_err());
        assert!(
            run_with_deadline(
                shell("[Threading.Thread]::Sleep(30000)"),
                None,
                Duration::from_millis(100)
            )
            .is_err()
        );
    }
}
