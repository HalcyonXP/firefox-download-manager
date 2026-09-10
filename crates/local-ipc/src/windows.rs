use std::{
    io,
    num::NonZeroU8,
    os::windows::io::AsHandle,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    task::{Context, Poll},
};

use interprocess::os::windows::{
    named_pipe::{
        PipeListenerOptions, pipe_mode,
        tokio::{DuplexPipeStream, PipeListener},
    },
    security_descriptor::SecurityDescriptor,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::windows::named_pipe::{ClientOptions, NamedPipeClient},
    sync::{OwnedSemaphorePermit, Semaphore},
};
use widestring::U16CString;

use crate::{Capability, Channel, Endpoint, Error, auth, identity};

/// Includes waiting accepts, unauthenticated handshakes and live sessions.
pub const MAX_CLIENTS: u8 = 4;

/// Exclusive local pipe binding, NOT a per-user installed-engine singleton.
/// The caller must hold independently verified state/generation authority.
pub struct Server {
    listener: PipeListener<pipe_mode::Bytes, pipe_mode::Bytes>,
    permits: Arc<Semaphore>,
    key: Arc<Capability>,
    endpoint: Endpoint,
    cancellation_failure: Arc<AtomicBool>,
}

impl Server {
    /// Bound public address; not installation or engine authority.
    #[must_use]
    pub const fn endpoint(&self) -> Endpoint {
        self.endpoint
    }

    /// Explicit secret-storage access after independent directory/receipt checks.
    #[must_use]
    pub fn capability_for_private_storage(&self) -> &Capability {
        &self.key
    }

    /// Bind a fresh address, with first-instance protection, a protected exact
    /// current-user owner/DACL, remote clients refused and noninheritable handles.
    /// Call on a worker under an active Tokio runtime, not on the UI thread:
    /// the joined read-only Windows identity lookup has a two-second budget.
    /// # Errors
    /// Refuses failed identity/security setup and existing/unavailable addresses.
    pub fn bind(endpoint: Endpoint, key: Arc<Capability>) -> Result<Self, Error> {
        let sddl = U16CString::from_str(identity::current_user_descriptor()?)
            .map_err(|_| Error::Identity)?;
        let security = SecurityDescriptor::deserialize(&sddl).map_err(|_| Error::Identity)?;
        let listener = PipeListenerOptions::new()
            .path(endpoint.path())
            .security_descriptor(Some(security))
            .accept_remote(false)
            .inheritable(false)
            // One extra instance is the listener's replacement reservation.
            .instance_limit(NonZeroU8::new(MAX_CLIENTS + 1))
            .input_buffer_size_hint(4096)
            .output_buffer_size_hint(4096)
            .create_tokio_duplex::<pipe_mode::Bytes>()
            .map_err(|_| Error::Transport)?;
        Ok(Self {
            listener,
            permits: Arc::new(Semaphore::new(usize::from(MAX_CLIENTS))),
            key,
            endpoint,
            cancellation_failure: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Sticky cancellation failure, including failed/unauthenticated sessions.
    /// Inspect after closing/joining all session tasks, before successful shutdown.
    #[must_use]
    pub fn cancellation_failed(&self) -> bool {
        self.cancellation_failure.load(Ordering::Acquire)
    }

    /// Accept/authenticate at most `MAX_CLIENTS` retained sessions. No task or
    /// unbounded queue is spawned here. Cancellation closes owned I/O and releases
    /// its reservation. Pending accepts are idle waits; handshakes share a 2s limit.
    /// # Errors
    /// Refuses capacity, transport, authentication and handshake deadline failures.
    pub async fn accept(&self) -> Result<Channel<LocalPipe>, Error> {
        let permit = Arc::clone(&self.permits)
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let io = self.listener.accept().await.map_err(|_| Error::Transport)?;
        let io = LocalPipe {
            inner: Pipe::Server(io),
            _permit: Some(permit),
            cancellation: CancellationWatch::new(),
            server_failure: Some(Arc::clone(&self.cancellation_failure)),
        };
        auth::server(io, &self.key, self.endpoint).await
    }
}

/// Connect only to a derived local address and prove both peers know the key.
/// No implicit wait/retry/launch/adoption of a busy or absent companion. The
/// native bridge must resolve installed authority before calling this function.
/// # Errors
/// Returns fixed transport/authentication/deadline classifications.
pub async fn connect(endpoint: Endpoint, key: &Capability) -> Result<Channel<LocalPipe>, Error> {
    // Tokio explicitly sets SECURITY_IDENTIFICATION | SECURITY_SQOS_PRESENT.
    // Do not use Interprocess's client constructor, which does not set SQOS.
    // Opening a squatted endpoint must not grant server-side impersonation power.
    let io = ClientOptions::new()
        .open(endpoint.path())
        .map_err(|_| Error::Transport)?;
    auth::client(
        LocalPipe {
            inner: Pipe::Client(io),
            _permit: None,
            cancellation: CancellationWatch::new(),
            server_failure: None,
        },
        key,
        endpoint,
    )
    .await
}

/// Observation of the cancellation request, NOT actual I/O completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancellationStatus {
    Active,
    Requested,
    Failed,
}

/// Retain before splitting; inspect after BOTH directions' tasks are joined.
/// Requested is not a receipt, peer-closure proof or completed runtime shutdown.
#[derive(Clone)]
pub struct CancellationWatch(Arc<AtomicU8>);

impl CancellationWatch {
    fn new() -> Self {
        Self(Arc::new(AtomicU8::new(0)))
    }
    fn record(&self, failed: bool, server_failure: Option<&AtomicBool>) {
        if failed && let Some(flag) = server_failure {
            flag.store(true, Ordering::Release);
        }
        self.0
            .fetch_max(if failed { 2 } else { 1 }, Ordering::AcqRel);
    }

    #[must_use]
    pub fn status(&self) -> CancellationStatus {
        match self.0.load(Ordering::Acquire) {
            0 => CancellationStatus::Active,
            1 => CancellationStatus::Requested,
            _ => CancellationStatus::Failed,
        }
    }
}

impl Channel<LocalPipe> {
    /// Retain the cancellation-failure observation independently of the I/O.
    #[must_use]
    pub fn cancellation(&self) -> CancellationWatch {
        self.transport().cancellation.clone()
    }
}

enum Pipe {
    Server(DuplexPipeStream<pipe_mode::Bytes>),
    Client(NamedPipeClient),
}

/// Opaque owned pipe; no public constructor, raw handles or process-ID authority.
/// Its last half closes before the capacity reservation is released.
pub struct LocalPipe {
    inner: Pipe,
    _permit: Option<OwnedSemaphorePermit>,
    cancellation: CancellationWatch,
    server_failure: Option<Arc<AtomicBool>>,
}

impl Drop for LocalPipe {
    fn drop(&mut self) {
        let handle = match &self.inner {
            Pipe::Server(io) => io.as_handle(),
            Pipe::Client(io) => io.as_handle(),
        };
        let failed = download_manager_windows_io::request_cancellation(handle).is_err();
        self.cancellation
            .record(failed, self.server_failure.as_deref());
        if let Pipe::Server(io) = &self.inner {
            // Interprocess normally sends dirty pipes to a background flush/linger
            // pool. Never do that: a nonreading peer could retain an unjoined pipe.
            // We never start flush operations, so clearing the dirty marker before
            // field drop avoids that pool. Explicit CancelIoEx above ALSO cancels
            // Mio's preserved pending writes. Neither is a completion/receipt.
            io.assume_flushed();
        }
    }
}

impl AsyncRead for LocalPipe {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut self.get_mut().inner {
            Pipe::Server(io) => Pin::new(io).poll_read(cx, buf),
            Pipe::Client(io) => Pin::new(io).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for LocalPipe {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut self.get_mut().inner {
            Pipe::Server(io) => Pin::new(io).poll_write(cx, buf),
            Pipe::Client(io) => Pin::new(io).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        // No user-space buffering. No FlushFileBuffers, remote-reader wait or
        // transport-delivery promise. Public frame methods do not call this.
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::AsyncWriteExt;

    #[test]
    fn cancellation_failure_is_sticky_and_distinct_from_completion() {
        let watch = CancellationWatch::new();
        let failure = AtomicBool::new(false);
        assert_eq!(watch.status(), CancellationStatus::Active);
        watch.record(true, Some(&failure));
        watch.record(false, Some(&failure));
        assert_eq!(watch.status(), CancellationStatus::Failed);
        assert!(failure.load(Ordering::Acquire));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_local_pipe_auth_frames_exclusivity_and_rebind_after_join() {
        let endpoint = Endpoint::generate().unwrap();
        let key = Arc::new(Capability::generate().unwrap());
        let server = Server::bind(endpoint, Arc::clone(&key)).unwrap();
        assert!(matches!(
            Server::bind(endpoint, Arc::clone(&key)),
            Err(Error::Transport)
        ));
        let (s, c) = tokio::join!(server.accept(), connect(endpoint, &key));
        let (mut sr, mut sw) = s.unwrap().split();
        let (mut cr, mut cw) = c.unwrap().split();
        cw.write(b"bounded request").await.unwrap();
        assert_eq!(sr.read().await.unwrap(), b"bounded request");
        sw.write(b"application receipt").await.unwrap();
        assert_eq!(cr.read().await.unwrap(), b"application receipt");
        drop((sr, sw, cr, cw));
        assert_eq!(server.permits.available_permits(), usize::from(MAX_CLIENTS));
        drop(server);
        // No sleep/Explorer restart/PID cleanup: last owned handles must be gone.
        drop(Server::bind(endpoint, key).unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_wrong_key_and_silent_peer_refuse_without_reservation_leaks() {
        let endpoint = Endpoint::generate().unwrap();
        let key = Arc::new(Capability::generate().unwrap());
        let server = Server::bind(endpoint, Arc::clone(&key)).unwrap();
        let wrong = Capability::generate().unwrap();
        let (s, c) = tokio::join!(server.accept(), connect(endpoint, &wrong));
        assert!(matches!(s, Err(Error::Authentication)));
        assert!(c.is_err());
        assert_eq!(server.permits.available_permits(), usize::from(MAX_CLIENTS));
        let silent = ClientOptions::new().open(endpoint.path()).unwrap();
        assert!(matches!(server.accept().await, Err(Error::Deadline)));
        drop(silent);
        assert_eq!(server.permits.available_permits(), usize::from(MAX_CLIENTS));
        drop(server);
        drop(Server::bind(endpoint, key).unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_capacity_is_bounded_and_dropped_channels_release_it() {
        let endpoint = Endpoint::generate().unwrap();
        let key = Arc::new(Capability::generate().unwrap());
        let server = Server::bind(endpoint, Arc::clone(&key)).unwrap();
        let mut channels = Vec::new();
        for _ in 0..MAX_CLIENTS {
            let (s, c) = tokio::join!(server.accept(), connect(endpoint, &key));
            channels.push((s.unwrap(), c.unwrap()));
        }
        assert!(matches!(server.accept().await, Err(Error::Busy)));
        drop(channels);
        assert_eq!(server.permits.available_permits(), usize::from(MAX_CLIENTS));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn nonreading_peer_cannot_send_dropped_server_to_background_linger() {
        for server_writes in [true, false] {
            let endpoint = Endpoint::generate().unwrap();
            let key = Arc::new(Capability::generate().unwrap());
            let server = Server::bind(endpoint, Arc::clone(&key)).unwrap();
            let (s, c) = tokio::join!(server.accept(), connect(endpoint, &key));
            let (mut sender, mut peer) = if server_writes {
                (s.unwrap(), c.unwrap())
            } else {
                (c.unwrap(), s.unwrap())
            };
            let observation = sender.cancellation();
            // Mio can acknowledge an entire queued write, not just delivered
            // bytes. Queue one bounded test-only MiB, then require the NEXT write
            // to stall behind it. Do not drain the receiving wrapper's prefetch.
            let payload = vec![42; 1024 * 1024];
            let queued = tokio::time::timeout(
                Duration::from_secs(1),
                sender.io_for_test().write_all(&payload),
            )
            .await;
            let stalled = matches!(queued, Ok(Ok(())))
                && tokio::time::timeout(
                    Duration::from_millis(100),
                    sender.io_for_test().write_all(&[42]),
                )
                .await
                .is_err();
            drop(sender);
            let closed = tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    if peer.io_for_test().write_all(&[0]).await.is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .is_ok();
            drop(peer);
            let cancellation_failed = server.cancellation_failed();
            drop(server);
            assert!(stalled, "fixture did not establish stalled pending output");
            assert!(closed, "owned peer closure not observed with unread output");
            assert_eq!(observation.status(), CancellationStatus::Requested);
            assert!(!cancellation_failed);
            drop(Server::bind(endpoint, key).unwrap());
        }
    }
}
