//! The shell retains the exact worker handle. A sent Quit is not a joined Quit.
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};

use download_manager_native_host::{EngineOwner, HostConfig};
use tokio::sync::oneshot;

/// Fixed classification, never underlying paths, URLs or diagnostic strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerError {
    /// Runtime, configuration or state ownership could not be established.
    Startup,
    /// Engine event processing or shutdown failed.
    Engine,
    /// Local transport startup or joined session retirement failed.
    Bridge,
    /// The retained worker panicked.
    Join,
}

/// One owned runtime thread; no detach, PID lookup or process termination.
pub struct Worker {
    handle: Option<JoinHandle<Result<(), WorkerError>>>,
    stop: Option<oneshot::Sender<()>>,
    ready: Receiver<()>,
}

impl Worker {
    /// Starts the real engine in the caller's explicitly selected domain.
    /// The shell must confirm tray registration before calling this method.
    ///
    /// # Errors
    /// Refuses when the owned worker thread cannot start.
    pub fn start(config: HostConfig) -> Result<Self, WorkerError> {
        Self::spawn(move |mut stopped, ready| async move {
            let owner = EngineOwner::open(&config).map_err(|_| WorkerError::Startup)?;
            let _ = ready.try_send(());
            let session = loop {
                let _ = owner.engine().take_overflow_snapshot();
                tokio::select! {
                    biased;
                    _ = &mut stopped => break Ok(()),
                    event = owner.engine().next_event() => {
                        if event.is_err() { break Err(WorkerError::Engine); }
                    }
                }
            };
            let shutdown = owner.shutdown().await.map_err(|_| WorkerError::Engine);
            shutdown.and(session)
        })
    }

    /// Start a visible-shell-owned engine with an explicit local test/launcher
    /// binding. This does not discover or publish installed generation authority.
    /// The shell must confirm tray registration before calling it.
    /// # Errors
    /// Returns a startup error if its retained worker cannot be created.
    #[cfg(windows)]
    pub fn start_local(
        config: HostConfig,
        endpoint: download_manager_local_ipc::Endpoint,
        key: std::sync::Arc<download_manager_local_ipc::Capability>,
    ) -> Result<Self, WorkerError> {
        Self::spawn(move |mut stopped, ready| async move {
            let mut owner = EngineOwner::open(&config).map_err(|_| WorkerError::Startup)?;
            let session = async {
                let server = download_manager_local_ipc::Server::bind(endpoint, key)
                    .map_err(|_| WorkerError::Bridge)?;
                let _ = ready.try_send(());
                serve_controller(&mut owner, &server, &mut stopped).await
            }
            .await;
            let shutdown = owner.shutdown().await.map_err(|_| WorkerError::Engine);
            shutdown.and(session)
        })
    }

    /// Start the installed image only after the shell's confirmed visibility gate.
    /// Metadata verification precedes state access; engine ownership precedes
    /// protected publication. Failed cleanup preserves the runtime domain.
    /// # Errors
    /// Refuses failed worker creation; startup errors are reported through join.
    #[cfg(all(windows, feature = "installed"))]
    pub fn start_installed() -> Result<Self, WorkerError> {
        use download_manager_local_ipc::{Capability, Endpoint};
        use download_manager_setup::{
            installed_image::InstalledImage, runtime_record::RuntimePublication,
        };
        Self::spawn(move |mut stopped, ready| async move {
            let image = InstalledImage::open_current().map_err(|_| WorkerError::Startup)?;
            let config = HostConfig::for_current_user().map_err(|_| WorkerError::Startup)?;
            let mut owner = EngineOwner::open(&config).map_err(|_| WorkerError::Startup)?;
            let publication = (|| {
                let endpoint = Endpoint::generate().map_err(|_| WorkerError::Bridge)?;
                let key =
                    std::sync::Arc::new(Capability::generate().map_err(|_| WorkerError::Bridge)?);
                RuntimePublication::bind(&image, endpoint, key).map_err(|_| WorkerError::Bridge)
            })();
            let publication = match publication {
                Ok(publication) => publication,
                Err(error) => {
                    owner.shutdown().await.map_err(|_| WorkerError::Engine)?;
                    return Err(error);
                }
            };
            let _ = ready.try_send(());
            let session = serve_controller(&mut owner, publication.server(), &mut stopped).await;
            let shutdown = owner.shutdown().await.map_err(|_| WorkerError::Engine);
            shutdown.and(session)?;
            publication.remove().map_err(|_| WorkerError::Bridge)
        })
    }

    fn spawn<F, Fut>(run: F) -> Result<Self, WorkerError>
    where
        F: FnOnce(oneshot::Receiver<()>, mpsc::SyncSender<()>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), WorkerError>>,
    {
        let (stop, stopped) = oneshot::channel();
        let (ready, received) = mpsc::sync_channel(1);
        let handle = thread::Builder::new()
            .name("download-manager-companion-engine".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .map_err(|_| WorkerError::Startup)?;
                runtime.block_on(run(stopped, ready))
            })
            .map_err(|_| WorkerError::Startup)?;
        Ok(Self {
            handle: Some(handle),
            stop: Some(stop),
            ready: received,
        })
    }

    /// Nonblocking startup observation. Thread completion remains authoritative
    /// for startup failure; a disconnected ready channel does not imply success.
    #[must_use]
    pub fn take_ready(&self) -> bool {
        match self.ready.try_recv() {
            Ok(()) => true,
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => false,
        }
    }

    /// Sticky, idempotent stop request; does not release the worker handle.
    pub fn request_stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }

    /// Joins only a finished worker, keeping the UI message loop responsive.
    pub fn try_join(&mut self) -> Option<Result<(), WorkerError>> {
        if !self.handle.as_ref().is_some_and(JoinHandle::is_finished) {
            return None;
        }
        self.handle
            .take()
            .map(|handle| handle.join().unwrap_or(Err(WorkerError::Join)))
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.request_stop();
        // Exceptional shell exit must not abandon a running engine. Normal
        // Quit polls and joins first, leaving no handle here. No kill fallback.
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(windows)]
async fn serve_controller(
    owner: &mut EngineOwner,
    server: &download_manager_local_ipc::Server,
    stopped: &mut oneshot::Receiver<()>,
) -> Result<(), WorkerError> {
    use download_manager_native_host::{HostError, LocalSessionEnd};
    let result = loop {
        if server.cancellation_failed() {
            break Err(WorkerError::Bridge);
        }
        let _ = owner.engine().take_overflow_snapshot();
        tokio::select! {
            biased;
            _ = &mut *stopped => break Ok(()),
            event = owner.engine().next_event() => {
                if event.is_err() { break Err(WorkerError::Engine); }
            }
            connection = server.accept() => {
                let Ok(channel) = connection else {
                    // Bound retries on a failed listener/unauthenticated peer;
                    // do not turn a peer refusal into a hot loop or engine stop.
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    continue;
                };
                // Deliberately one controller. Awaiting this session leaves
                // additional connections at bounded handshake timeout; no
                // second engine/settings writer or command dispatch exists.
                match owner.serve_local(channel, stopped).await {
                    Ok(LocalSessionEnd::StopRequested) => break Ok(()),
                    Err(HostError::LocalRetirement) => break Err(WorkerError::Bridge),
                    Err(HostError::Engine(_)) => break Err(WorkerError::Engine),
                    Ok(LocalSessionEnd::Disconnected) | Err(_) => {}
                }
            }
        }
    };
    // The final select may itself retire an unauthenticated pipe.
    // Observe that cancellation result before destroying the server.
    if server.cancellation_failed() {
        Err(WorkerError::Bridge)
    } else {
        result
    }
}
