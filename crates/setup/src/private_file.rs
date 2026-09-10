//! Opt-in protected runtime-record primitive; not installed bridge authority.
//! The caller supplies an independently owned, ACL-protected directory. Leases
//! prevent path replacement; ACL checks exclude other unprivileged writers. Same-user code,
//! administrators and SYSTEM are outside this confidentiality boundary.
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    SetupError,
    paths::{DirectoryLease, validate_text},
};

const SCRIPT: &str = include_str!("private_file.ps1");
// Establish the script's dedicated raw UTF-8 reader BEFORE accepting a request.
// Process creation alone does not establish script initialization.
const BOOTSTRAP: &str = r#"
$ErrorActionPreference = 'Stop'
$dmInput = [IO.StreamReader]::new([Console]::OpenStandardInput(), [Text.UTF8Encoding]::new($false, $true), $false)
[Console]::Out.Write("start`n")
[Console]::Out.Flush()
$dmOperation = $dmInput.ReadLine()
$dmPath = $dmInput.ReadLine()
if ($dmOperation -cnotin @('create', 'verify') -or $null -eq $dmPath -or $dmPath.Length -gt 240 -or
    $dmPath -cnotmatch '^[A-Za-z]:\\' -or $dmPath -match '[\x00-\x1f\x7f]' -or $dmPath.Contains('/')) { exit 1 }
[Console]::Out.Write("input`n")
[Console]::Out.Flush()
"#;
pub(crate) const NAME: &str = "companion-runtime.json";
pub(crate) const LIMIT: usize = 4096;
const EXECUTION_LIMIT: Duration = Duration::from_secs(5);
const ERROR: SetupError = SetupError::PrivateFile;

#[cfg(test)]
fn trace_phase(phase: &'static str) {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let elapsed = START.get_or_init(Instant::now).elapsed().as_millis();
    eprintln!("private adapter: {phase} at +{elapsed} ms");
}

/// Verified, bounded bytes and a retained read-only file/ancestor lease.
/// Intentionally not Debug/Serialize. This is not a generation or engine proof.
pub struct PrivateFile {
    directory: DirectoryLease,
    file: File,
    bytes: Vec<u8>,
}

impl PrivateFile {
    /// Verify parent/owner/DACL before reading any bytes; deny writes/deletion.
    /// # Errors
    /// Refuses unsafe paths, sharing conflicts, unexpected permissions or size.
    pub fn open(directory: DirectoryLease) -> Result<Self, SetupError> {
        let path = record_path(&directory)?;
        let mut file = open_file(&path, false)?;
        Adapter::start("verify", &path)?.finish()?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(4097)
            .read_to_end(&mut bytes)
            .map_err(|_| ERROR)?;
        if bytes.is_empty() || bytes.len() > LIMIT {
            return Err(ERROR);
        }
        Ok(Self {
            directory,
            file,
            bytes,
        })
    }

    /// Sensitive contents: never put these in diagnostics or ordinary reports.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Only the successful create-new publisher receives removal authority.
pub struct PublishedFile(PrivateFile);

impl PublishedFile {
    /// Create an empty protected file, independently verify its ACL while the
    /// creator denies deletion, then write bytes in Rust only. No secrets enter
    /// the auxiliary process, its command line or `PowerShell` parameter logging.
    /// # Errors
    /// Existing entries are never adopted/replaced. Failed creations are left
    /// for caller-owned recovery, including partial or already complete files.
    pub fn create(directory: DirectoryLease, bytes: &[u8]) -> Result<Self, SetupError> {
        if bytes.is_empty() || bytes.len() > LIMIT {
            return Err(ERROR);
        }
        let path = record_path(&directory)?;
        let creator = Adapter::start("create", &path)?;
        let mut writer = open_file(&path, true)?;
        if writer.metadata().map_err(|_| ERROR)?.len() != 0 {
            return Err(ERROR);
        }
        Adapter::start("verify", &path)?.finish()?;
        writer.write_all(bytes).map_err(|_| ERROR)?;
        writer.sync_all().map_err(|_| ERROR)?;
        drop(writer);
        creator.finish()?;
        let verified = PrivateFile::open(directory)?;
        if verified.bytes() != bytes {
            return Err(ERROR);
        }
        Ok(Self(verified))
    }

    /// Remove only this publisher's file, not its directory. Other retained
    /// readers make removal fail; never terminate them or retry by PID.
    /// # Errors
    /// A sharing/removal failure preserves the record and refuses success.
    pub fn remove(self) -> Result<(), SetupError> {
        let PrivateFile {
            directory,
            file,
            bytes: _,
        } = self.0;
        let path = record_path(&directory)?;
        drop(file);
        fs::remove_file(path).map_err(|_| ERROR)
    }
}

fn record_path(directory: &DirectoryLease) -> Result<PathBuf, SetupError> {
    let path = directory.path().join(NAME);
    validate_text(&path)?;
    Ok(path)
}

fn open_file(path: &Path, writing: bool) -> Result<File, SetupError> {
    let file = OpenOptions::new()
        .read(true)
        .write(writing)
        .share_mode(if writing { 3 } else { 1 })
        .custom_flags(0x0020_0000) // OPEN_REPARSE_POINT, never traverse a leaf link.
        .open(path)
        .map_err(|_| ERROR)?;
    let meta = file.metadata().map_err(|_| ERROR)?;
    if !meta.is_file() || meta.file_attributes() & 0x400 != 0 || meta.len() > 4096 {
        return Err(ERROR);
    }
    Ok(file)
}

// Owns every process/thread even on an early error. Deadlines bound waiting for
// helper work, not an unconditional bound on Windows disk or joined cleanup.
pub(crate) struct Adapter {
    child: Child,
    worker: Option<JoinHandle<Result<(), SetupError>>>,
    close: Option<Sender<()>>,
    deadline: Instant,
}

impl Adapter {
    fn start(operation: &str, path: &Path) -> Result<Self, SetupError> {
        Self::start_script(operation, path, SCRIPT)
    }

    pub(crate) fn start_script(
        operation: &str,
        path: &Path,
        script: &str,
    ) -> Result<Self, SetupError> {
        let script = format!("{BOOTSTRAP}\ntry {{\n{script}\n}} finally {{ $dmInput.Dispose() }}");
        Self::start_program(operation, path, &script)
    }

    fn start_program(operation: &str, path: &Path, script: &str) -> Result<Self, SetupError> {
        if !matches!(operation, "create" | "verify") {
            return Err(ERROR);
        }
        validate_text(path)?;
        let input = format!("{operation}\n{}\n", path.to_str().ok_or(ERROR)?).into_bytes();
        if input.len() > 2048 {
            return Err(ERROR);
        }
        let executable = PathBuf::from(winsafe::GetSystemDirectory().map_err(|_| ERROR)?)
            .join(r"WindowsPowerShell\v1.0\powershell.exe");
        if !executable.is_absolute() {
            return Err(ERROR);
        }
        #[cfg(test)]
        trace_phase("launch requested");
        let child = Command::new(executable)
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW; no execution-policy changes.
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| ERROR)?;
        #[cfg(test)]
        trace_phase("process handle retained");
        let mut adapter = Self {
            child,
            worker: None,
            close: None,
            deadline: Instant::now() + EXECUTION_LIMIT,
        };
        let mut stdin = adapter.child.stdin.take().ok_or(ERROR)?;
        let mut stdout = adapter.child.stdout.take().ok_or(ERROR)?;
        let (ready_tx, ready_rx) = mpsc::channel();
        let (close_tx, close_rx) = mpsc::channel();
        adapter.close = Some(close_tx);
        adapter.worker = Some(
            thread::Builder::new()
                .name("private-file-adapter".into())
                .spawn(move || {
                    #[cfg(test)]
                    trace_phase("worker entered");
                    let mut start = [0; 6];
                    stdout.read_exact(&mut start).map_err(|_| ERROR)?;
                    #[cfg(test)]
                    trace_phase("startup bytes read");
                    if &start != b"start\n" {
                        return Err(ERROR);
                    }
                    stdin.write_all(&input).map_err(|_| ERROR)?;
                    #[cfg(test)]
                    trace_phase("request written");
                    let mut consumed = [0; 6];
                    stdout.read_exact(&mut consumed).map_err(|_| ERROR)?;
                    #[cfg(test)]
                    trace_phase("request receipt bytes read");
                    if &consumed != b"input\n" {
                        return Err(ERROR);
                    }
                    let mut ready = [0; 6];
                    stdout.read_exact(&mut ready).map_err(|_| ERROR)?;
                    #[cfg(test)]
                    trace_phase("readiness bytes read");
                    if &ready != b"ready\n" {
                        return Err(ERROR);
                    }
                    ready_tx.send(()).map_err(|_| ERROR)?;
                    close_rx.recv().map_err(|_| ERROR)?;
                    stdin.write_all(b"close\n").map_err(|_| ERROR)?;
                    #[cfg(test)]
                    trace_phase("close request written");
                    drop(stdin);
                    let mut output = Vec::new();
                    stdout.take(3).read_to_end(&mut output).map_err(|_| ERROR)?;
                    #[cfg(test)]
                    trace_phase("completion bytes read");
                    if output != b"ok" {
                        return Err(ERROR);
                    }
                    Ok(())
                })
                .map_err(|_| ERROR)?,
        );
        adapter.wait_ready(&ready_rx)?;
        Ok(adapter)
    }

    fn wait_ready(&mut self, ready: &Receiver<()>) -> Result<(), SetupError> {
        loop {
            if Instant::now() >= self.deadline {
                #[cfg(test)]
                trace_phase("deadline before readiness");
                return Err(ERROR);
            }
            match ready.recv_timeout(Duration::from_millis(5)) {
                Ok(()) => return Ok(()),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(ERROR),
                Err(mpsc::RecvTimeoutError::Timeout) => (),
            }
            if self.child.try_wait().map_err(|_| ERROR)?.is_some() {
                #[cfg(test)]
                trace_phase("child exit before readiness");
                return Err(ERROR);
            }
        }
    }

    pub(crate) fn finish(mut self) -> Result<(), SetupError> {
        self.close
            .take()
            .ok_or(ERROR)?
            .send(())
            .map_err(|_| ERROR)?;
        let succeeded = loop {
            if Instant::now() >= self.deadline {
                break false;
            }
            match self.child.try_wait() {
                Ok(Some(status)) => break status.success(),
                Ok(None) => thread::sleep(Duration::from_millis(5)),
                Err(_) => break false,
            }
        };
        self.cleanup()?;
        if succeeded { Ok(()) } else { Err(ERROR) }
    }

    fn cleanup(&mut self) -> Result<(), SetupError> {
        self.close.take(); // Unblock a worker waiting for its Rust-side close signal.
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            #[cfg(test)]
            trace_phase("retiring retained process");
            let _ = self.child.kill();
        }
        let process = self.child.wait().map_err(|_| ERROR);
        #[cfg(test)]
        trace_phase("process wait returned");
        let worker = self.worker.take().map(|worker| {
            let result = worker.join().map_err(|_| ERROR);
            #[cfg(test)]
            trace_phase("worker join returned");
            result
        });
        process?;
        if let Some(result) = worker {
            result??;
        }
        Ok(())
    }
}

impl Drop for Adapter {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lease(root: &Path) -> DirectoryLease {
        DirectoryLease::open(root).unwrap()
    }

    fn mutate(path: &Path, change: &str) {
        // Fixture-only code; production never accepts caller-supplied scripts.
        let script = format!(
            r#"
$ErrorActionPreference = 'Stop'
# Input reader is established by the fixed Rust-side bootstrap.
# Opcode and literal path are provided by the fixed bootstrap.
{change}
[Console]::Out.Write("ready`n")
[Console]::Out.Flush()
if ($dmInput.ReadLine() -cne 'close') {{ exit 1 }}
[Console]::Out.Write('ok')
"#
        );
        Adapter::start_script("verify", path, &script)
            .unwrap()
            .finish()
            .unwrap();
    }

    fn protect_fixture(path: &Path, protect: bool) {
        let flag = if protect { "$true" } else { "$false" };
        mutate(
            path,
            &format!(
                r"
$p = [IO.Path]::GetDirectoryName($dmPath)
$a = [IO.Directory]::GetAccessControl($p)
$a.SetAccessRuleProtection({flag}, $true)
[IO.Directory]::SetAccessControl($p, $a)
"
            ),
        );
    }

    #[test]
    fn malformed_receipts_and_stalled_creator_are_joined() {
        let root = std::env::temp_dir().join(format!("dm-private-helper-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join(NAME);
        let directory = lease(&root);
        protect_fixture(&path, true);
        assert!(Adapter::start_program("verify\ncreate", &path, "").is_err());
        assert!(Adapter::start_program("verify", &root.join("bad\nname"), "").is_err());
        // A wrong startup marker must be refused even if the peer could go on
        // to supply otherwise valid ready/completion markers. No filesystem IO.
        let bad_start = r#"
$r = [IO.StreamReader]::new([Console]::OpenStandardInput(), [Text.UTF8Encoding]::new($false, $true), $false)
[Console]::Out.Write("wrong`n")
[Console]::Out.Flush()
[void]$r.ReadLine()
[void]$r.ReadLine()
[Console]::Out.Write("input`n")
[Console]::Out.Write("ready`n")
[Console]::Out.Flush()
[void]$r.ReadLine()
[Console]::Out.Write('ok')
"#;
        assert!(Adapter::start_program("verify", &path, bad_start).is_err());
        let bad_input = bad_start
            .replace("Write(\"wrong`n\")", "Write(\"start`n\")")
            .replace("Write(\"input`n\")", "Write(\"wrong`n\")");
        assert!(Adapter::start_program("verify", &path, &bad_input).is_err());
        // Receipt failure after successful process exit must still refuse success.
        let bad_receipt =
            SCRIPT.replace("[Console]::Out.Write('ok')", "[Console]::Out.Write('no')");
        let creator = Adapter::start_script("create", &path, &bad_receipt).unwrap();
        assert!(creator.finish().is_err());
        fs::remove_file(&path).unwrap();
        // Observe real readiness first, then use a SHORTER test-only deadline.
        // A held file must become deletable after the exact child is retired.
        let stalled = SCRIPT.replace("    if ($null -ne $file) { $file.Dispose(); $file = $null }",
            "    [Threading.Thread]::Sleep(30000)\n    if ($null -ne $file) { $file.Dispose(); $file = $null }");
        let mut creator = Adapter::start_script("create", &path, &stalled).unwrap();
        assert!(fs::remove_file(&path).is_err());
        creator.deadline = Instant::now() + Duration::from_millis(100);
        assert!(creator.finish().is_err());
        fs::remove_file(&path).unwrap();
        drop(directory);
        fs::remove_dir(&root).unwrap();
    }

    #[test]
    fn owned_private_record_lifecycle_and_refusals() {
        let root =
            std::env::temp_dir().join(format!("dm-private-file-{} ' Ω", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        // Intentionally no cleanup-on-panic: failed owned domains are preserved.
        let path = root.join(NAME);
        let bytes = b"public deterministic fixture, not a credential";
        protect_fixture(&path, false);
        assert!(PublishedFile::create(lease(&root), bytes).is_err());
        assert!(!path.exists());
        protect_fixture(&path, true);
        assert!(PublishedFile::create(lease(&root), &[]).is_err());
        assert!(PublishedFile::create(lease(&root), &vec![0; LIMIT + 1]).is_err());
        assert!(!path.exists());

        // Refusal must preserve an existing unleased entry, not merely fail
        // because another reader happened to block an overwrite.
        fs::write(&path, bytes).unwrap();
        assert!(PublishedFile::create(lease(&root), b"replacement").is_err());
        assert!(fs::read(&path).unwrap() == bytes);
        fs::remove_file(&path).unwrap();

        let publisher = PublishedFile::create(lease(&root), bytes).unwrap();
        let reader = PrivateFile::open(lease(&root)).unwrap();
        assert!(reader.bytes() == bytes);
        assert!(PublishedFile::create(lease(&root), b"replacement").is_err());
        assert!(fs::remove_file(&path).is_err());
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        drop(reader);
        publisher.remove().unwrap();
        assert!(!path.exists());

        // While the empty creator is retained, ordinary readers must refuse;
        // aborting it joins the child/worker but preserves the failed empty file.
        let creator = Adapter::start("create", &path).unwrap();
        assert!(open_file(&path, false).is_err()); // Sharing, not the empty-body check.
        assert!(fs::remove_file(&path).is_err());
        assert!(PrivateFile::open(lease(&root)).is_err());
        drop(creator);
        assert!(path.exists());
        assert!(PrivateFile::open(lease(&root)).is_err());
        fs::remove_file(&path).unwrap();

        let publisher = PublishedFile::create(lease(&root), &vec![0x53; LIMIT]).unwrap();
        assert!(PrivateFile::open(lease(&root)).unwrap().bytes().len() == LIMIT);
        // Add a broad ACE on public fixture bytes only, not on any real secret.
        let mutator = r#"
$ErrorActionPreference = 'Stop'
# Input reader is established by the fixed Rust-side bootstrap.
# Opcode and literal path are provided by the fixed bootstrap.
$a = [IO.File]::GetAccessControl($dmPath)
$world = [Security.Principal.SecurityIdentifier]::new('S-1-1-0')
$rule = [Security.AccessControl.FileSystemAccessRule]::new($world, [Security.AccessControl.FileSystemRights]::Read, [Security.AccessControl.AccessControlType]::Allow)
$a.AddAccessRule($rule)
[IO.File]::SetAccessControl($dmPath, $a)
[Console]::Out.Write("ready`n")
[Console]::Out.Flush()
if ($dmInput.ReadLine() -cne 'close') { exit 1 }
[Console]::Out.Write('ok')
"#;
        Adapter::start_script("verify", &path, mutator)
            .unwrap()
            .finish()
            .unwrap();
        assert!(PrivateFile::open(lease(&root)).is_err());
        publisher.remove().unwrap();

        // Remove parent inheritance solely in this owned empty fixture so the
        // unprotected-file case differs ONLY in its protection control bit.
        mutate(
            &path,
            r"
$p = [IO.Path]::GetDirectoryName($dmPath)
$a = [IO.Directory]::GetAccessControl($p)
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
try { $sid = $id.User.Value } finally { $id.Dispose() }
$a.SetSecurityDescriptorSddlForm(('D:P(A;;FA;;;' + $sid + ')'), [Security.AccessControl.AccessControlSections]::Access)
[IO.Directory]::SetAccessControl($p, $a)
",
        );
        for change in [
            "$a.SetAccessRuleProtection($false, $true)",
            "$a.SetSecurityDescriptorSddlForm('D:P(A;;FA;;;WD)', [Security.AccessControl.AccessControlSections]::Access)",
            "$a.SetSecurityDescriptorSddlForm('D:NO_ACCESS_CONTROL', [Security.AccessControl.AccessControlSections]::Access)",
        ] {
            let publisher = PublishedFile::create(lease(&root), bytes).unwrap();
            mutate(
                &path,
                &format!(
                    "$a = [IO.File]::GetAccessControl($dmPath)\n{change}\n[IO.File]::SetAccessControl($dmPath, $a)"
                ),
            );
            assert!(PrivateFile::open(lease(&root)).is_err());
            publisher.remove().unwrap();
        }

        let publisher = PublishedFile::create(lease(&root), bytes).unwrap();
        drop(publisher); // Still this test's exact create-new file, no adoption.
        fs::write(&path, vec![0; LIMIT + 1]).unwrap();
        assert!(PrivateFile::open(lease(&root)).is_err());
        fs::remove_file(&path).unwrap();

        // Reject an otherwise owned parent if an unprivileged other principal
        // can mutate its namespace. No private file may be created on refusal.
        mutate(
            &path,
            r"
$p = [IO.Path]::GetDirectoryName($dmPath)
$a = [IO.Directory]::GetAccessControl($p)
$world = [Security.Principal.SecurityIdentifier]::new('S-1-1-0')
$a.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($world, [Security.AccessControl.FileSystemRights]::Write, [Security.AccessControl.AccessControlType]::Allow))
[IO.Directory]::SetAccessControl($p, $a)
",
        );
        assert!(PublishedFile::create(lease(&root), bytes).is_err());
        assert!(!path.exists());
        fs::remove_dir(&root).unwrap();
    }
}
