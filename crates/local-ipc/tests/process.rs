#![cfg(windows)]

use std::{
    io::{Read, Write},
    os::windows::process::CommandExt,
    process::{Child, Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use download_manager_local_ipc::{Capability, Endpoint, Error, Server, connect};

const CHILD_FLAG: &str = "DOWNLOAD_MANAGER_OWNED_IPC_TEST_CHILD";

struct OwnedChild(Child);

impl OwnedChild {
    fn join(&mut self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match self.0.try_wait() {
                Ok(Some(status)) => return status.success(),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
                _ => return false,
            }
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            // Failure containment only, using the exact retained child handle.
            // No PID, process-name, descendant/tree or browser termination.
            let _ = self.0.kill();
        }
        assert!(self.0.wait().is_ok(), "owned IPC child could not be joined");
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

fn child() {
    let mut input = Vec::new();
    std::io::stdin().take(69).read_to_end(&mut input).unwrap();
    assert_eq!(input.len(), 68, "bounded owned child input required");
    let key = Capability::from_bytes(input[..32].try_into().unwrap());
    let endpoint = Endpoint::parse(std::str::from_utf8(&input[32..]).unwrap()).unwrap();
    runtime().block_on(async {
        let channel = connect(endpoint, &key).await.unwrap();
        let (mut reader, mut writer) = channel.split();
        writer.write(b"owned child request").await.unwrap();
        assert_eq!(reader.read().await.unwrap(), b"owned parent receipt");
        writer.write(b"owned child receipt observed").await.unwrap();
        assert_eq!(
            reader.read().await.unwrap(),
            b"owned parent quit permission"
        );
        drop((reader, writer));
    });
}

#[test]
fn real_cross_process_roundtrip_and_join_without_browser_or_registration() {
    if std::env::var(CHILD_FLAG).as_deref() == Ok("1") {
        child();
        return;
    }
    let endpoint = Endpoint::generate().unwrap();
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).unwrap();
    let key = Arc::new(Capability::from_bytes(bytes));
    let rt = runtime();
    let server = {
        let _entered = rt.enter();
        Server::bind(endpoint, Arc::clone(&key)).unwrap()
    };
    let mut child = OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "real_cross_process_roundtrip_and_join_without_browser_or_registration",
            ])
            .env(CHILD_FLAG, "1") // Test mode only; never a key or address in arguments/environment.
            .creation_flags(0x0800_0000)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let outcome = (|| {
        let mut stdin = child.0.stdin.take().ok_or(Error::Transport)?;
        stdin.write_all(&bytes).map_err(|_| Error::Transport)?;
        stdin
            .write_all(endpoint.id().as_bytes())
            .map_err(|_| Error::Transport)?;
        drop(stdin);
        rt.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let channel = server.accept().await?;
                let (mut reader, mut writer) = channel.split();
                if reader.read().await? != b"owned child request" {
                    return Err(Error::Frame);
                }
                writer.write(b"owned parent receipt").await?;
                if reader.read().await? != b"owned child receipt observed" {
                    return Err(Error::Frame);
                }
                writer.write(b"owned parent quit permission").await?;
                // Keep server I/O alive until the child has actually consumed the
                // final permission and exited, rather than assuming write == receipt.
                Ok((reader, writer))
            })
            .await
            .map_err(|_| Error::Deadline)?
        })
    })();
    let joined_success = child.join();
    drop(child); // containment/join before assertions, including failed cases.
    assert!(
        joined_success,
        "owned IPC child failed; no success authorized"
    );
    assert!(outcome.is_ok(), "owned cross-process exchange failed");
    drop(outcome);
    drop(server);
    let _entered = rt.enter();
    drop(Server::bind(endpoint, key).unwrap());
}
