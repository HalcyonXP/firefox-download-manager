//! One active authenticated controller; transport loss never owns engine lifetime.
use super::{
    EngineOwner, HostError, Inbound, Session, SessionOutput, negotiate, run_active_session,
};
use download_manager_local_ipc::{CancellationStatus, Channel, LocalPipe, PeerClass};
use tokio::sync::{mpsc, oneshot};

/// Normal local-session completion, separate from engine shutdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalSessionEnd {
    Disconnected,
    StopRequested,
}

impl EngineOwner {
    /// Serve one already-authenticated ordinary local controller against this owner.
    /// Parent-class channels require a separate entry; they cannot silently enter
    /// this handler or select protection authority through JSON claims.
    /// The mutable borrow prevents concurrent serving through this API. Installed
    /// generation/capability authority must be checked before constructing it.
    /// Signal `stop` and await completion for joined teardown; do not externally
    /// cancel this future and claim successful cleanup. An abandoned future aborts
    /// its retained reader via `JoinSet`, but provides no joined-success result.
    ///
    /// # Errors
    /// Returns sanitized protocol/transport/reader-join/cancellation failures.
    /// None of these shuts down the engine or automatically retries a command.
    pub async fn serve_local(
        &mut self,
        channel: Channel<LocalPipe>,
        stop: &mut oneshot::Receiver<()>,
    ) -> Result<LocalSessionEnd, HostError> {
        if channel.peer_class() != PeerClass::NativeBridge {
            // No application read/write or task dispatch has started. Dropping
            // the exact channel requests cancellation; the caller still owns
            // shutdown and the server's sticky cancellation-failure checks.
            drop(channel);
            return Err(HostError::LocalSession);
        }
        self.serve_channel(channel, stop).await
    }

    /// Explicit parent-class transport admission followed by ordinary wire2.
    /// This does not enable captured handoffs, protection decisions or browser
    /// policy authority. Default listeners and `serve_local` remain ordinary-only.
    /// The caller must retain this future through stop and joined completion,
    /// exactly as for `serve_local`; transport loss never shuts down the engine.
    ///
    /// # Errors
    /// Refuses ordinary peers before I/O, invalid admission, protocol/transport
    /// failures and incomplete reader retirement. No command is replayed.
    pub async fn serve_parent_transport(
        &mut self,
        channel: Channel<LocalPipe>,
        stop: &mut oneshot::Receiver<()>,
    ) -> Result<LocalSessionEnd, HostError> {
        if channel.peer_class() != PeerClass::BrowserParent {
            drop(channel);
            return Err(HostError::LocalSession);
        }
        self.serve_channel(channel, stop).await
    }

    async fn serve_channel(
        &mut self,
        channel: Channel<LocalPipe>,
        stop: &mut oneshot::Receiver<()>,
    ) -> Result<LocalSessionEnd, HostError> {
        let parent = channel.peer_class() == PeerClass::BrowserParent;
        let cancellation = channel.cancellation();
        let (mut reader, writer) = channel.split();
        let (inbound, mut received) = mpsc::channel(8);
        let (retire, mut retired) = oneshot::channel();
        let mut readers = tokio::task::JoinSet::new();
        readers.spawn(async move {
            loop {
                let message = tokio::select! {
                    biased;
                    _ = &mut retired => break,
                    message = reader.read() => message,
                };
                let ended = message.is_err();
                let message = match message {
                    Ok(body) => Inbound::Body(body),
                    Err(_) => Inbound::LocalEnd,
                };
                let sent = tokio::select! {
                    biased;
                    _ = &mut retired => break,
                    sent = inbound.send(message) => sent.is_ok(),
                };
                if !sent || ended {
                    break;
                }
            }
        });
        let mut session = Session::<std::io::Sink>::new(
            std::io::sink(),
            Some(self.settings.current.destination.clone().into()),
        );
        // Existing ordinary handoffs are not a protected-capture dispatcher.
        session.handoff_enabled = !parent;
        session.writer = SessionOutput::Local(tokio::sync::Mutex::new(writer));
        session.settings = Some(&mut self.settings);
        let outcome = tokio::select! {
            biased;
            _ = stop => Ok(LocalSessionEnd::StopRequested),
            outcome = async {
                if parent {
                    super::parent_transport::admit(&session, &mut received).await?;
                }
                if negotiate(&mut received, &mut session, &self.engine).await? {
                    run_active_session(&mut received, &mut session, &mut self.engine).await.map(|()| LocalSessionEnd::Disconnected)
                } else { Ok(LocalSessionEnd::Disconnected) }
            } => outcome,
        };
        let _ = retire.send(());
        drop(received);
        drop(session);
        // No reader detach or successful report before joined resource retirement.
        let joined = readers
            .join_next()
            .await
            .is_some_and(|result| result.is_ok());
        if !joined || cancellation.status() != CancellationStatus::Requested {
            return Err(HostError::LocalRetirement);
        }
        outcome
    }
}
