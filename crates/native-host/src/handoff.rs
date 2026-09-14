//! Additive `prepared_handoff` capability, selected only by the owned local bridge.
use std::{io::Write, path::PathBuf};

use download_manager_engine::{
    integrity::ExpectedSha256,
    persistence::HandoffPhase,
    scheduler::WorkerCount,
    task::{HandoffRequest, HandoffSnapshot, TaskEngine, TaskEngineError},
};
use download_manager_protocol::{
    ErrorCode, PrepareHandoffPayload, ProtocolError, ResponseCommand, TaskDescription,
    TaskIdPayload,
};

use super::{
    DEFAULT_FILENAME, HostError, Session, parse_task_id, task_description, task_engine_error,
};

#[derive(serde::Serialize)]
struct HandoffResult {
    phase: HandoffPhase,
    task: TaskDescription,
}

impl<W: Write> Session<'_, W> {
    pub(super) async fn prepare_handoff(
        &self,
        correlation: String,
        payload: &PrepareHandoffPayload,
        engine: &TaskEngine,
    ) -> Result<(), HostError> {
        let command = ResponseCommand::PrepareHandoff;
        if !self.handoff_enabled {
            return self
                .send_failure(correlation, command, ErrorCode::ProtocolUnknownCommand)
                .await;
        }
        let request = match self.handoff_request(payload) {
            Ok(request) => request,
            Err(error) => return self.send_error(correlation, command, error).await,
        };
        self.send_handoff(correlation, command, engine.prepare_handoff(request))
            .await
    }

    fn handoff_request(
        &self,
        payload: &PrepareHandoffPayload,
    ) -> Result<HandoffRequest, ProtocolError> {
        let id = parse_task_id(payload.task_id())?;
        let input = payload.download();
        // Typed decode excludes request_context; retain a local defense at dispatch.
        if input.request_context().is_some() {
            return Err(ProtocolError::without_context(
                ErrorCode::ProtocolInvalidMessage,
            ));
        }
        let destination = input
            .destination()
            .map(PathBuf::from)
            .or_else(|| self.default_destination.clone())
            .ok_or_else(|| ProtocolError::without_context(ErrorCode::InvalidDestination))?;
        let workers = WorkerCount::try_from(input.workers().unwrap_or_else(|| {
            self.settings
                .as_ref()
                .map_or(4, |settings| settings.current.default_workers)
        }))
        .map_err(|_| ProtocolError::without_context(ErrorCode::InvalidSettings))?;
        let expected = input
            .expected_sha256()
            .map(|value| {
                ExpectedSha256::parse(value).ok_or_else(|| {
                    ProtocolError::without_context(ErrorCode::ProtocolInvalidMessage)
                })
            })
            .transpose()?;
        HandoffRequest::new(
            id,
            input.url(),
            &destination,
            input.suggested_filename().unwrap_or(DEFAULT_FILENAME),
            workers,
            expected,
        )
        .map_err(|error| task_engine_error(&error, Some(id)))
    }

    pub(super) async fn control_handoff(
        &self,
        correlation: String,
        command: ResponseCommand,
        payload: &TaskIdPayload,
        engine: &TaskEngine,
    ) -> Result<(), HostError> {
        if !self.handoff_enabled {
            return self
                .send_failure(correlation, command, ErrorCode::ProtocolUnknownCommand)
                .await;
        }
        let id = match parse_task_id(payload.task_id()) {
            Ok(id) => id,
            Err(error) => return self.send_error(correlation, command, error).await,
        };
        let outcome = match command {
            ResponseCommand::CommitHandoff => engine.commit_handoff(id),
            ResponseCommand::AbortHandoff => engine.abort_handoff(id),
            ResponseCommand::GetHandoff => engine.handoff_status(id),
            _ => return Err(HostError::Projection),
        };
        self.send_handoff(correlation, command, outcome).await
    }

    async fn send_handoff(
        &self,
        correlation: String,
        command: ResponseCommand,
        outcome: Result<HandoffSnapshot, TaskEngineError>,
    ) -> Result<(), HostError> {
        match outcome {
            Ok(receipt) => {
                self.send_success(
                    correlation,
                    command,
                    &HandoffResult {
                        phase: receipt.phase(),
                        task: task_description(receipt.task())?,
                    },
                )
                .await
            }
            Err(error) => {
                self.send_engine_failure(correlation, command, &error, None)
                    .await
            }
        }
    }
}
