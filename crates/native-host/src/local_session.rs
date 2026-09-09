//! One active authenticated controller; transport loss never owns engine lifetime.
use super::{
    EngineOwner, HostError, Inbound, Session, SessionOutput, negotiate, run_active_session,
};
use download_manager_local_ipc::{CancellationStatus, Channel, LocalPipe};
use tokio::sync::{mpsc, oneshot};

/// Normal local-session completion, separate from engine shutdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalSessionEnd {
    Disconnected,
    StopRequested,
}

impl EngineOwner {
    /// Serve one already-authenticated local controller against this owner.
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
        session.writer = SessionOutput::Local(tokio::sync::Mutex::new(writer));
        session.settings = Some(&mut self.settings);
        let outcome = tokio::select! {
            biased;
            _ = stop => Ok(LocalSessionEnd::StopRequested),
            outcome = async {
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
