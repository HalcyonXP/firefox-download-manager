//! Native stdio forwarding through two exact owned I/O-only children.
//! No engine, discovery authority, command replay or global blocking-pool stdin.
use download_manager_local_ipc::{CancellationStatus, Channel, LocalPipe};
use download_manager_protocol::{MAX_MESSAGE_BYTES, read_frame};
use std::{
    io::{IsTerminal, Read, Write},
    os::windows::process::CommandExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread::{self, JoinHandle},
    time::Duration,
};
use tokio::sync::{mpsc as asynchronous, oneshot};

/// Fixed classification; never carries a frame, path, key or underlying exception.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayError {
    Start,
    Input,
    Output,
    Transport,
    Retirement,
}
const DEADLINE: Duration = Duration::from_secs(2);
type Delivery = (Vec<u8>, oneshot::Sender<Result<(), RelayError>>);

fn write_body(writer: &mut impl Write, body: &[u8]) -> Result<(), RelayError> {
    if body.is_empty() || body.len() > MAX_MESSAGE_BYTES {
        return Err(RelayError::Output);
    }
    let prefix = u32::try_from(body.len())
        .map_err(|_| RelayError::Output)?
        .to_le_bytes();
    writer
        .write_all(&prefix)
        .and_then(|()| writer.write_all(body))
        .and_then(|()| writer.flush())
        .map_err(|_| RelayError::Output)
}

/// I/O-only internal entry. Output completion and input-parent liveness use
/// separate private stderr bytes; no frame or credential is logged there.
/// # Errors
/// Rejects console handles, malformed frames and failed actual writes. Parent
/// loss during blocked I/O retires this I/O-only process with failure; it never
/// claims delivery or engine cleanup. Its owner must still wait for this process.
pub fn pump(output: bool) -> Result<(), RelayError> {
    if std::io::stdin().is_terminal()
        || std::io::stdout().is_terminal()
        || std::io::stderr().is_terminal()
    {
        return Err(RelayError::Start);
    }
    if output { output_pump() } else { input_pump() }
}

fn input_pump() -> Result<(), RelayError> {
    let (stop, stopped) = mpsc::channel();
    let monitor = thread::Builder::new()
        .name("manager-input-parent".into())
        .spawn(move || {
            loop {
                // The parent continuously drains this private pipe. Its read end is
                // distinct from native stdin, which can remain open after parent loss.
                if std::io::stderr()
                    .write_all(&[0])
                    .and_then(|()| std::io::stderr().flush())
                    .is_err()
                {
                    std::process::exit(1);
                }
                match stopped.recv_timeout(Duration::from_millis(250)) {
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    _ => break,
                }
            }
        })
        .map_err(|_| RelayError::Start)?;
    let result = (|| {
        let mut input = std::io::stdin();
        while let Some(body) = read_frame(&mut input).map_err(|_| RelayError::Input)? {
            write_body(&mut std::io::stdout(), &body)?;
        }
        Ok(())
    })();
    let _ = stop.send(());
    monitor.join().map_err(|_| RelayError::Retirement)?;
    result
}

fn output_pump() -> Result<(), RelayError> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let pending = Arc::new(AtomicBool::new(false));
    let receiving = Arc::clone(&pending);
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = thread::Builder::new()
        .name("manager-output-parent".into())
        .spawn(move || {
            let mut input = std::io::stdin();
            loop {
                match read_frame(&mut input) {
                    Ok(Some(body)) if !body.is_empty() => {
                        // Parent protocol permits exactly one outstanding frame;
                        // it waits for the actual stdout acknowledgement before more.
                        if receiving.swap(true, Ordering::SeqCst) || sender.send(body).is_err() {
                            std::process::exit(1);
                        }
                    }
                    Ok(None) if !receiving.load(Ordering::SeqCst) => break,
                    // No synchronous stdout cancellation claim: terminate this exact
                    // I/O-only process, including its blocked writer, with failure.
                    _ => std::process::exit(1),
                }
            }
        })
        .map_err(|_| RelayError::Start)?;
    for body in receiver {
        if write_body(&mut std::io::stdout(), &body).is_err() {
            std::process::exit(1);
        }
        // Clear before acknowledging: a valid parent cannot send the next frame
        // until it receives that acknowledgement, avoiding a lost pending flag.
        pending.store(false, Ordering::SeqCst);
        if std::io::stderr()
            .write_all(&[1])
            .and_then(|()| std::io::stderr().flush())
            .is_err()
        {
            std::process::exit(1);
        }
    }
    reader.join().map_err(|_| RelayError::Retirement)
}

struct Pumps {
    children: Vec<Child>,
    threads: Vec<JoinHandle<()>>,
    input: Option<asynchronous::Receiver<Result<Vec<u8>, RelayError>>>,
    output: Option<mpsc::SyncSender<Delivery>>,
}
impl Pumps {
    fn start(executable: &Path, input: Stdio, output: Stdio) -> Result<Self, RelayError> {
        let mut pumps = Self {
            children: Vec::new(),
            threads: Vec::new(),
            input: None,
            output: None,
        };
        // Create input first; it cannot inherit the not-yet-created output pipe.
        let child = Command::new(executable)
            .arg("--stdio-input")
            .stdin(input)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|_| RelayError::Start)?;
        pumps.children.push(child);
        let mut source = pumps.children[0].stdout.take().ok_or(RelayError::Start)?;
        let (sender, receiver) = asynchronous::channel(1);
        pumps.input = Some(receiver);
        pumps.threads.push(
            thread::Builder::new()
                .name("manager-native-input".into())
                .spawn(move || {
                    loop {
                        let value = match read_frame(&mut source) {
                            Ok(Some(body)) if !body.is_empty() => Ok(body),
                            Ok(None) => break,
                            _ => Err(RelayError::Input),
                        };
                        let failed = value.is_err();
                        if sender.blocking_send(value).is_err() || failed {
                            break;
                        }
                    }
                })
                .map_err(|_| RelayError::Start)?,
        );
        let mut liveness = pumps.children[0].stderr.take().ok_or(RelayError::Start)?;
        pumps.threads.push(
            thread::Builder::new()
                .name("manager-native-parent-monitor".into())
                .spawn(move || {
                    let mut marker = [0];
                    while liveness.read_exact(&mut marker).is_ok() && marker == [0] {}
                })
                .map_err(|_| RelayError::Start)?,
        );
        let child = Command::new(executable)
            .arg("--stdio-output")
            .stdin(Stdio::piped())
            .stdout(output)
            .stderr(Stdio::piped())
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|_| RelayError::Start)?;
        pumps.children.push(child);
        let mut destination = pumps.children[1].stdin.take().ok_or(RelayError::Start)?;
        let mut acknowledgement = pumps.children[1].stderr.take().ok_or(RelayError::Start)?;
        let (sender, receiver) = mpsc::sync_channel::<Delivery>(1);
        pumps.output = Some(sender);
        pumps.threads.push(
            thread::Builder::new()
                .name("manager-native-output".into())
                .spawn(move || {
                    while let Ok((body, completed)) = receiver.recv() {
                        let result = write_body(&mut destination, &body).and_then(|()| {
                            let mut marker = [0];
                            acknowledgement
                                .read_exact(&mut marker)
                                .map_err(|_| RelayError::Output)?;
                            if marker == [1] {
                                Ok(())
                            } else {
                                Err(RelayError::Output)
                            }
                        });
                        let failed = result.is_err();
                        let _ = completed.send(result);
                        if failed {
                            break;
                        }
                    }
                })
                .map_err(|_| RelayError::Start)?,
        );
        Ok(pumps)
    }

    async fn exchange(&mut self, channel: Channel<LocalPipe>) -> Result<(), RelayError> {
        let cancellation = channel.cancellation();
        let mut input = self.input.take().ok_or(RelayError::Input)?;
        let output = self.output.as_ref().ok_or(RelayError::Output)?.clone();
        let (mut reader, mut writer) = channel.split();
        let result = {
            let incoming = async move {
                while let Some(body) = input.recv().await {
                    writer
                        .write(&body?)
                        .await
                        .map_err(|_| RelayError::Transport)?;
                }
                Ok(())
            };
            let outgoing = async move {
                loop {
                    let body = reader.read().await.map_err(|_| RelayError::Transport)?;
                    let (sent, received) = oneshot::channel();
                    output
                        .try_send((body, sent))
                        .map_err(|_| RelayError::Output)?;
                    // Actual child output completion, not a queued parent write.
                    tokio::time::timeout(DEADLINE, received)
                        .await
                        .map_err(|_| RelayError::Output)?
                        .map_err(|_| RelayError::Output)??;
                }
            };
            tokio::pin!(incoming, outgoing);
            tokio::select! { value = &mut incoming => value, value = &mut outgoing => value }
        };
        // Both cancelled futures/directions have been dropped, not merely signalled.
        if cancellation.status() != CancellationStatus::Requested {
            return Err(RelayError::Retirement);
        }
        result
    }

    fn retire(&mut self) -> Result<(), RelayError> {
        self.input.take();
        self.output.take();
        let mut failed = false;
        // Closing exact child counterparts releases pending synchronous pipe I/O.
        // CancelIoEx is not misrepresented as cancellation of these sync reads.
        let mut unjoined_child = false;
        for child in &mut self.children {
            match child.try_wait() {
                Ok(Some(status)) => {
                    failed |= !status.success();
                }
                Ok(None) => {
                    if child.kill().is_err() && !matches!(child.try_wait(), Ok(Some(_))) {
                        failed = true;
                    }
                }
                Err(_) => {
                    failed = true;
                    let _ = child.kill();
                }
            }
            if child.wait().is_err() {
                failed = true;
                unjoined_child = true;
            }
        }
        if !unjoined_child {
            self.children.clear();
        }
        for handle in self.threads.drain(..) {
            if handle.join().is_err() {
                failed = true;
            }
        }
        if failed {
            Err(RelayError::Retirement)
        } else {
            Ok(())
        }
    }
}
impl Drop for Pumps {
    fn drop(&mut self) {
        let _ = self.retire();
    }
}

/// Forward one authenticated connection; never reconnect/replay after dispatch.
/// The caller retains verified executable/installation leases. Stdio arguments
/// must be inherited native handles or explicitly owned test streams.
/// # Errors
/// Any forwarding/retirement failure refuses success. Exact children and worker
/// threads are retired/joined first; no PID/name lookup or detached worker exists.
pub async fn forward(
    channel: Channel<LocalPipe>,
    executable: &Path,
    input: Stdio,
    output: Stdio,
) -> Result<(), RelayError> {
    let mut pumps = Pumps::start(executable, input, output)?;
    let result = pumps.exchange(channel).await;
    pumps.retire().and(result)
}
