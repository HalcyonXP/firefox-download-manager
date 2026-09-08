//! On-demand Firefox Native Messaging host.
//!
//! The helper owns the authoritative task engine and exposes only strict,
//! bounded protocol-v2 frames on standard output. Paths and URLs are accepted
//! only inside typed commands and never appear in ordinary diagnostics.

use std::env;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use download_manager_engine::persistence::{
    PersistenceError, StateValidationError, TaskId, TaskState, TimestampMillis, TransferMode,
};
use download_manager_engine::scheduler::WorkerCount;
use download_manager_engine::storage::IoFailure;
use download_manager_engine::task::{
    CancelPartialPolicy, TaskEngine, TaskEngineError, TaskEngineOptions, TaskEvent, TaskEventKind,
    TaskFailure, TaskFailureKind, TaskProgress, TaskSnapshot,
};
use download_manager_protocol::{
    CancelPartial, Command, CommandDecodeFailure, ErrorCode, ErrorContext, EventMessage, EventName,
    FailedData, FrameReadError, FrameWriteError, HelloResult, ListPayload, MAX_MESSAGE_BYTES,
    MessageBuildError, PROTOCOL_VERSION, ProgressData, ProtocolError, RemoveResult,
    ResponseCommand, ResponseMessage, SnapshotPage, StateChangedData, TaskDescription,
    TaskStateName, TransferModeName, WarningData, decode_command, read_frame, write_frame,
};
use thiserror::Error;
use tokio::runtime::Builder;
use tokio::sync::mpsc;

/// Registry/manifest name used by Firefox `runtime.connectNative`.
pub const NATIVE_HOST_NAME: &str = "com.halcyonxp.firefox_download_manager";
/// Fixed extension ID allowed by the Firefox native-host manifest.
pub const ALLOWED_EXTENSION_ID: &str = "download-manager@halcyonxp.local";

const HELPER_VERSION: &str = env!("CARGO_PKG_VERSION");
const PAGE_TASK_LIMIT: usize = 4;
const MAX_NEGOTIATION_ERRORS: u8 = 8;
const DEFAULT_FILENAME: &str = "download";

/// Non-sensitive helper failure surfaced only on stderr/process status.
#[derive(Debug, Error)]
pub enum HostError {
    #[error("native host configuration is unavailable")]
    Configuration,
    #[error("native host runtime could not start")]
    Runtime,
    #[error("native host input failed: {0}")]
    Input(#[from] FrameReadError),
    #[error("native host output failed: {0}")]
    Output(#[from] FrameWriteError),
    #[error("native host protocol output could not be built")]
    MessageBuild(#[from] MessageBuildError),
    #[error("native host task engine failed: {0}")]
    Engine(#[from] TaskEngineError),
    #[error("native host task projection is invalid")]
    Projection,
    #[error("native host timestamp is invalid")]
    Timestamp(#[from] StateValidationError),
    #[error("native host protocol sequence is exhausted")]
    SequenceExhausted,
}

/// Trusted startup locations. `Debug` is intentionally omitted because these
/// values can contain a user name.
pub struct HostConfig {
    state_root: PathBuf,
    default_destination: Option<PathBuf>,
}

impl HostConfig {
    /// Creates an explicit configuration, primarily for tests and packaged
    /// launchers.
    #[must_use]
    pub fn new(state_root: PathBuf, default_destination: Option<PathBuf>) -> Self {
        Self {
            state_root,
            default_destination,
        }
    }

    /// Resolves the per-user Windows locations used by an installed helper.
    ///
    /// # Errors
    ///
    /// Rejects missing, empty, or non-absolute profile environment paths.
    pub fn for_current_user() -> Result<Self, HostError> {
        let local_app_data = absolute_environment_path("LOCALAPPDATA")?;
        let state_root = local_app_data
            .join("HalcyonXP")
            .join("FirefoxDownloadManager")
            .join("state");
        let default_destination = absolute_environment_path("USERPROFILE")
            .ok()
            .map(|profile| profile.join("Downloads"));
        Ok(Self::new(state_root, default_destination))
    }
}

/// Runs one helper process until Firefox closes stdin or a fatal framing /
/// negotiation failure occurs. Every exit path cooperatively stops active
/// engine work before returning.
///
/// # Errors
///
/// Returns only sanitized configuration, framing, engine, or runtime errors.
pub fn run_host<R, W>(reader: R, writer: W, config: HostConfig) -> Result<(), HostError>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    let runtime = Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("download-manager-host")
        .enable_all()
        .build()
        .map_err(|_| HostError::Runtime)?;
    runtime.block_on(async move {
        let engine = TaskEngine::open(&config.state_root, TaskEngineOptions::default())?;
        let session_result = run_session(reader, writer, &engine, config.default_destination).await;
        let shutdown_result = engine.shutdown().await.map(|_| ()).map_err(HostError::from);
        shutdown_result.and(session_result)
    })
}

async fn run_session<R, W>(
    reader: R,
    writer: W,
    engine: &TaskEngine,
    default_destination: Option<PathBuf>,
) -> Result<(), HostError>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    let (sender, mut inbound) = mpsc::channel(16);
    thread::Builder::new()
        .name("download-manager-native-input".to_owned())
        .spawn(move || read_input(reader, &sender))
        .map_err(|_| HostError::Runtime)?;

    let mut session = Session::new(writer, default_destination);
    if !negotiate(&mut inbound, &mut session, engine).await? {
        return Ok(());
    }
    run_active_session(&mut inbound, &mut session, engine).await
}

async fn negotiate<W: Write>(
    inbound: &mut mpsc::Receiver<Inbound>,
    session: &mut Session<W>,
    engine: &TaskEngine,
) -> Result<bool, HostError> {
    let mut errors = 0_u8;
    loop {
        let Some(item) = inbound.recv().await else {
            return Ok(false);
        };
        match item {
            Inbound::End(result) => {
                session.finish_input(result)?;
                return Ok(false);
            }
            Inbound::Body(body) => match decode_command(&body) {
                Ok(message) => {
                    let (correlation_id, command) = message.into_parts();
                    let Command::Hello(payload) = command else {
                        session.send_failure(
                            correlation_id,
                            response_command(&command),
                            ErrorCode::ProtocolInvalidMessage,
                        )?;
                        return Ok(false);
                    };
                    if !payload.supported_versions().contains(&PROTOCOL_VERSION) {
                        session.send_failure(
                            correlation_id,
                            ResponseCommand::Hello,
                            ErrorCode::ProtocolUnsupportedVersion,
                        )?;
                        return Ok(false);
                    }
                    let _ = (payload.client_name(), payload.client_version());
                    session.send_success(
                        correlation_id,
                        ResponseCommand::Hello,
                        &HelloResult {
                            selected_version: PROTOCOL_VERSION,
                            helper_version: HELPER_VERSION,
                            capabilities: vec!["snapshots", "coalesced_progress"],
                            max_message_bytes: MAX_MESSAGE_BYTES,
                        },
                    )?;
                    session.send_snapshot_events(&engine.snapshots())?;
                    session.send_recovery_warnings(engine)?;
                    return Ok(true);
                }
                Err(error) => {
                    let fatal = error.failure() == CommandDecodeFailure::UnsupportedVersion;
                    session.send_decode_error(&error)?;
                    errors = errors.saturating_add(1);
                    if fatal || errors >= MAX_NEGOTIATION_ERRORS {
                        return Ok(false);
                    }
                }
            },
        }
    }
}

async fn run_active_session<W: Write>(
    inbound: &mut mpsc::Receiver<Inbound>,
    session: &mut Session<W>,
    engine: &TaskEngine,
) -> Result<(), HostError> {
    loop {
        if let Some(snapshots) = engine.take_overflow_snapshot() {
            session.send_snapshot_events(&snapshots)?;
        }
        tokio::select! {
            item = inbound.recv() => {
                let Some(item) = item else {
                    return Ok(());
                };
                match item {
                    Inbound::End(result) => return session.finish_input(result),
                    Inbound::Body(body) => {
                        let message = match decode_command(&body) {
                            Ok(message) => message,
                            Err(error) => {
                                let fatal = error.failure() == CommandDecodeFailure::UnsupportedVersion;
                                session.send_decode_error(&error)?;
                                if fatal {
                                    return Ok(());
                                }
                                continue;
                            }
                        };
                        let (correlation_id, command) = message.into_parts();
                        if matches!(command, Command::Hello(_)) {
                            session.send_failure(
                                correlation_id,
                                ResponseCommand::Hello,
                                ErrorCode::ProtocolInvalidMessage,
                            )?;
                            continue;
                        }
                        session.dispatch(engine, correlation_id, command).await?;
                    }
                }
            }
            event = engine.next_event() => {
                session.send_engine_event(&event?)?;
            }
        }
    }
}

enum Inbound {
    Body(Vec<u8>),
    End(Result<(), FrameReadError>),
}

fn read_input(mut reader: impl Read, sender: &mpsc::Sender<Inbound>) {
    loop {
        match read_frame(&mut reader) {
            Ok(Some(body)) => {
                if sender.blocking_send(Inbound::Body(body)).is_err() {
                    return;
                }
            }
            Ok(None) => {
                let _ = sender.blocking_send(Inbound::End(Ok(())));
                return;
            }
            Err(error) => {
                let _ = sender.blocking_send(Inbound::End(Err(error)));
                return;
            }
        }
    }
}

struct Session<W> {
    writer: Arc<Mutex<W>>,
    default_destination: Option<PathBuf>,
    sequence: u64,
    token: u64,
    list: Option<ListSession>,
}

impl<W: Write> Session<W> {
    fn new(writer: W, default_destination: Option<PathBuf>) -> Self {
        Self {
            writer: Arc::new(Mutex::new(writer)),
            default_destination,
            sequence: 0,
            token: 0,
            list: None,
        }
    }

    // Keeping the exhaustive command-to-response table together makes it
    // auditable that every negotiated discriminator receives one response.
    #[allow(clippy::too_many_lines)]
    async fn dispatch(
        &mut self,
        engine: &TaskEngine,
        correlation_id: String,
        command: Command,
    ) -> Result<(), HostError> {
        match command {
            Command::Hello(_) => unreachable!("hello is handled before dispatch"),
            Command::Add(payload) => {
                if payload.has_checksum() || payload.has_request_context() {
                    return self.send_failure(
                        correlation_id,
                        ResponseCommand::Add,
                        ErrorCode::ProtocolInvalidMessage,
                    );
                }
                let destination = payload
                    .destination()
                    .map(PathBuf::from)
                    .or_else(|| self.default_destination.clone());
                let Some(destination) = destination else {
                    return self.send_failure(
                        correlation_id,
                        ResponseCommand::Add,
                        ErrorCode::InvalidDestination,
                    );
                };
                let workers = payload
                    .workers()
                    .map_or(Ok(WorkerCount::default()), WorkerCount::try_from)
                    .map_err(|_| HostError::Projection)?;
                let task = match engine.create_task(
                    payload.url(),
                    &destination,
                    payload.suggested_filename().unwrap_or(DEFAULT_FILENAME),
                    workers,
                ) {
                    Ok(task) => task,
                    Err(error) => {
                        return self.send_engine_failure(
                            correlation_id,
                            ResponseCommand::Add,
                            &error,
                            None,
                        );
                    }
                };
                let task_id = task.task_id();
                let task = match engine.start(task_id) {
                    Ok(task) => task,
                    Err(error) => {
                        return self.send_engine_failure(
                            correlation_id,
                            ResponseCommand::Add,
                            &error,
                            Some(task_id),
                        );
                    }
                };
                self.send_task(correlation_id, ResponseCommand::Add, &task)
            }
            Command::Pause(payload) => {
                let task_id = match parse_task_id(payload.task_id()) {
                    Ok(task_id) => task_id,
                    Err(error) => {
                        return self.send_error(correlation_id, ResponseCommand::Pause, error);
                    }
                };
                match engine.pause(task_id).await {
                    Ok(task) => self.send_task(correlation_id, ResponseCommand::Pause, &task),
                    Err(error) => self.send_engine_failure(
                        correlation_id,
                        ResponseCommand::Pause,
                        &error,
                        Some(task_id),
                    ),
                }
            }
            Command::Resume(payload) => {
                let task_id = match parse_task_id(payload.task_id()) {
                    Ok(task_id) => task_id,
                    Err(error) => {
                        return self.send_error(correlation_id, ResponseCommand::Resume, error);
                    }
                };
                let result = if engine
                    .snapshot(task_id)
                    .is_ok_and(|task| task.state() == TaskState::Queued)
                {
                    engine.start(task_id)
                } else {
                    engine.resume(task_id).await
                };
                match result {
                    Ok(task) => self.send_task(correlation_id, ResponseCommand::Resume, &task),
                    Err(error) => self.send_engine_failure(
                        correlation_id,
                        ResponseCommand::Resume,
                        &error,
                        Some(task_id),
                    ),
                }
            }
            Command::Cancel(payload) => {
                let task_id = match parse_task_id(payload.task_id()) {
                    Ok(task_id) => task_id,
                    Err(error) => {
                        return self.send_error(correlation_id, ResponseCommand::Cancel, error);
                    }
                };
                let policy = match payload.partial_policy() {
                    CancelPartial::Keep => CancelPartialPolicy::Keep,
                    CancelPartial::Delete => CancelPartialPolicy::Delete,
                };
                match engine.cancel(task_id, policy).await {
                    Ok(task) => self.send_task(correlation_id, ResponseCommand::Cancel, &task),
                    Err(error) => self.send_engine_failure(
                        correlation_id,
                        ResponseCommand::Cancel,
                        &error,
                        Some(task_id),
                    ),
                }
            }
            Command::Remove(payload) => {
                let task_id = match parse_task_id(payload.task_id()) {
                    Ok(task_id) => task_id,
                    Err(error) => {
                        return self.send_error(correlation_id, ResponseCommand::Remove, error);
                    }
                };
                match engine.remove(task_id, payload.delete_partial()) {
                    Ok(removed) => self.send_success(
                        correlation_id,
                        ResponseCommand::Remove,
                        &RemoveResult {
                            removed_task_id: removed.to_string(),
                        },
                    ),
                    Err(error) => self.send_engine_failure(
                        correlation_id,
                        ResponseCommand::Remove,
                        &error,
                        Some(task_id),
                    ),
                }
            }
            Command::List(payload) => self.send_list(engine, correlation_id, &payload),
            Command::Get(payload) => {
                let task_id = match parse_task_id(payload.task_id()) {
                    Ok(task_id) => task_id,
                    Err(error) => {
                        return self.send_error(correlation_id, ResponseCommand::Get, error);
                    }
                };
                match engine.snapshot(task_id) {
                    Ok(task) => self.send_task(correlation_id, ResponseCommand::Get, &task),
                    Err(error) => self.send_engine_failure(
                        correlation_id,
                        ResponseCommand::Get,
                        &error,
                        Some(task_id),
                    ),
                }
            }
            Command::OpenFolder(payload) => {
                let task_id = match parse_task_id(payload.task_id()) {
                    Ok(id) => id,
                    Err(error) => {
                        return self.send_error(correlation_id, ResponseCommand::OpenFolder, error);
                    }
                };
                let task = match engine.snapshot(task_id) {
                    Ok(task) => task,
                    Err(error) => {
                        return self.send_engine_failure(
                            correlation_id,
                            ResponseCommand::OpenFolder,
                            &error,
                            Some(task_id),
                        );
                    }
                };
                if open_folder(task.destination()).is_err() {
                    return self.send_failure(
                        correlation_id,
                        ResponseCommand::OpenFolder,
                        ErrorCode::InvalidDestination,
                    );
                }
                self.send_success(
                    correlation_id,
                    ResponseCommand::OpenFolder,
                    &FolderResult {
                        opened_task_id: task_id.to_string(),
                    },
                )
            }
            Command::GetSettings(_) => self.send_failure(
                correlation_id,
                ResponseCommand::GetSettings,
                ErrorCode::InvalidSettings,
            ),
            Command::UpdateSettings(_) => self.send_failure(
                correlation_id,
                ResponseCommand::UpdateSettings,
                ErrorCode::InvalidSettings,
            ),
        }
    }

    fn send_list(
        &mut self,
        engine: &TaskEngine,
        correlation_id: String,
        payload: &ListPayload,
    ) -> Result<(), HostError> {
        if payload.cursor().is_none() {
            let tasks = engine
                .snapshots()
                .into_iter()
                .filter(|task| payload.include_terminal() || !task.state().is_terminal())
                .map(|task| task_description(&task))
                .collect::<Result<Vec<_>, _>>()?;
            self.list = Some(ListSession {
                snapshot_id: self.next_token("snapshot")?,
                tasks,
                offset: 0,
                page_index: 0,
                expected_cursor: None,
                include_terminal: payload.include_terminal(),
            });
        }
        let requested_cursor = payload.cursor();
        let expected_cursor = self
            .list
            .as_ref()
            .and_then(|list| list.expected_cursor.as_deref());
        if requested_cursor != expected_cursor
            || self
                .list
                .as_ref()
                .is_some_and(|list| list.include_terminal != payload.include_terminal())
        {
            return self.send_failure(
                correlation_id,
                ResponseCommand::List,
                ErrorCode::ProtocolInvalidMessage,
            );
        }

        let limit = usize::from(payload.limit()).min(PAGE_TASK_LIMIT);
        let (snapshot_id, page_index, tasks, end, complete) = {
            let list = self.list.as_ref().ok_or(HostError::Projection)?;
            let end = list.offset.saturating_add(limit).min(list.tasks.len());
            (
                list.snapshot_id.clone(),
                list.page_index,
                list.tasks[list.offset..end].to_vec(),
                end,
                end == list.tasks.len(),
            )
        };
        let next_cursor = if complete {
            None
        } else {
            Some(self.next_token("cursor")?)
        };
        let page = SnapshotPage {
            snapshot_id,
            page_index,
            tasks,
            next_cursor: next_cursor.clone(),
            complete,
        };
        let list = self.list.as_mut().ok_or(HostError::Projection)?;
        list.offset = end;
        list.page_index = list.page_index.saturating_add(1);
        list.expected_cursor = next_cursor;
        self.send_success(correlation_id, ResponseCommand::List, &page)?;
        if complete {
            self.list = None;
        }
        Ok(())
    }

    fn send_task(
        &self,
        correlation_id: String,
        command: ResponseCommand,
        task: &TaskSnapshot,
    ) -> Result<(), HostError> {
        self.send_success(correlation_id, command, &task_description(task)?)
    }

    fn send_snapshot_events(&mut self, snapshots: &[TaskSnapshot]) -> Result<(), HostError> {
        let snapshot_id = self.next_token("snapshot")?;
        let tasks = snapshots
            .iter()
            .map(task_description)
            .collect::<Result<Vec<_>, _>>()?;
        let page_count = tasks.len().div_ceil(PAGE_TASK_LIMIT).max(1);
        for page_index in 0..page_count {
            let start = page_index * PAGE_TASK_LIMIT;
            let end = start.saturating_add(PAGE_TASK_LIMIT).min(tasks.len());
            let complete = page_index + 1 == page_count;
            let page = SnapshotPage {
                snapshot_id: snapshot_id.clone(),
                page_index: u64::try_from(page_index).map_err(|_| HostError::Projection)?,
                tasks: tasks[start..end].to_vec(),
                next_cursor: (!complete).then(|| format!("{snapshot_id}-{end}")),
                complete,
            };
            self.send_event(EventName::Snapshot, TimestampMillis::now()?, &page)?;
        }
        Ok(())
    }

    fn send_recovery_warnings(&mut self, engine: &TaskEngine) -> Result<(), HostError> {
        for failure in engine.recovery_report().failures() {
            let task_id = failure.task_id().map(|task_id| task_id.to_string());
            let context = task_id
                .as_ref()
                .map_or_else(ErrorContext::default, |task_id| {
                    ErrorContext::default().with_task_id(task_id.clone())
                });
            self.send_event(
                EventName::Warning,
                TimestampMillis::now()?,
                &WarningData {
                    task_id,
                    warning: ProtocolError::new(ErrorCode::StateCorrupt, context),
                },
            )?;
        }
        Ok(())
    }

    fn send_engine_event(&mut self, event: &TaskEvent) -> Result<(), HostError> {
        match event.kind() {
            TaskEventKind::RetryScheduled(_) => Ok(()),
            TaskEventKind::StateChanged {
                task,
                previous_state,
            } => self.send_event(
                EventName::StateChanged,
                event.emitted_at(),
                &StateChangedData {
                    task: task_description(task)?,
                    previous_state: task_state(*previous_state),
                },
            ),
            TaskEventKind::Progress(progress) => self.send_event(
                EventName::Progress,
                event.emitted_at(),
                &progress_description(*progress),
            ),
            TaskEventKind::Completed(task) => self.send_event(
                EventName::Completed,
                event.emitted_at(),
                &task_description(task)?,
            ),
            TaskEventKind::Failed { task, failure } => {
                let error = task_failure_error(*failure, Some(task.task_id()));
                self.send_event(
                    EventName::Failed,
                    event.emitted_at(),
                    &FailedData {
                        task: task_description(task)?,
                        error,
                    },
                )
            }
        }
    }

    fn finish_input(&mut self, result: Result<(), FrameReadError>) -> Result<(), HostError> {
        match result {
            Ok(()) => Ok(()),
            Err(FrameReadError::MessageTooLarge { .. }) => {
                let correlation_id = self.next_token("protocol")?;
                self.send_failure(
                    correlation_id,
                    ResponseCommand::Protocol,
                    ErrorCode::ProtocolMessageTooLarge,
                )
            }
            Err(error) => Err(error.into()),
        }
    }

    fn send_decode_error(
        &mut self,
        error: &download_manager_protocol::CommandDecodeError,
    ) -> Result<(), HostError> {
        let correlation_id = error
            .correlation_id()
            .map(str::to_owned)
            .unwrap_or(self.next_token("protocol")?);
        let command = error.command().unwrap_or(ResponseCommand::Protocol);
        let code = match error.failure() {
            CommandDecodeFailure::InvalidMessage => ErrorCode::ProtocolInvalidMessage,
            CommandDecodeFailure::UnsupportedVersion => ErrorCode::ProtocolUnsupportedVersion,
            CommandDecodeFailure::UnknownCommand => ErrorCode::ProtocolUnknownCommand,
        };
        self.send_failure(correlation_id, command, code)
    }

    fn send_engine_failure(
        &self,
        correlation_id: String,
        command: ResponseCommand,
        error: &TaskEngineError,
        task_id: Option<TaskId>,
    ) -> Result<(), HostError> {
        self.send_error(correlation_id, command, task_engine_error(error, task_id))
    }

    fn send_failure(
        &self,
        correlation_id: String,
        command: ResponseCommand,
        code: ErrorCode,
    ) -> Result<(), HostError> {
        self.send_error(
            correlation_id,
            command,
            ProtocolError::without_context(code),
        )
    }

    fn send_error(
        &self,
        correlation_id: String,
        command: ResponseCommand,
        error: ProtocolError,
    ) -> Result<(), HostError> {
        self.write(&ResponseMessage::failure(correlation_id, command, error))
    }

    fn send_success(
        &self,
        correlation_id: String,
        command: ResponseCommand,
        result: &impl serde::Serialize,
    ) -> Result<(), HostError> {
        self.write(&ResponseMessage::success(correlation_id, command, result)?)
    }

    fn send_event(
        &mut self,
        event: EventName,
        emitted_at: TimestampMillis,
        data: &impl serde::Serialize,
    ) -> Result<(), HostError> {
        let sequence = self.next_sequence()?;
        let correlation_id = self.next_token("event")?;
        let message = EventMessage::new(
            correlation_id,
            event,
            sequence,
            timestamp_rfc3339(emitted_at),
            data,
        )?;
        self.write(&message)
    }

    fn write(&self, message: &impl serde::Serialize) -> Result<(), HostError> {
        write_frame(&mut *lock(&self.writer), message).map_err(HostError::from)
    }

    fn next_sequence(&mut self) -> Result<u64, HostError> {
        if self.sequence > download_manager_engine::progress::MAX_SAFE_INTEGER {
            return Err(HostError::SequenceExhausted);
        }
        let current = self.sequence;
        self.sequence = self.sequence.saturating_add(1);
        Ok(current)
    }

    fn next_token(&mut self, prefix: &str) -> Result<String, HostError> {
        if self.token > download_manager_engine::progress::MAX_SAFE_INTEGER {
            return Err(HostError::SequenceExhausted);
        }
        let current = self.token;
        self.token = self.token.saturating_add(1);
        Ok(format!("{prefix}-{current}"))
    }
}

struct ListSession {
    snapshot_id: String,
    tasks: Vec<TaskDescription>,
    offset: usize,
    page_index: u64,
    expected_cursor: Option<String>,
    include_terminal: bool,
}

fn response_command(command: &Command) -> ResponseCommand {
    match command {
        Command::Hello(_) => ResponseCommand::Hello,
        Command::Add(_) => ResponseCommand::Add,
        Command::Pause(_) => ResponseCommand::Pause,
        Command::Resume(_) => ResponseCommand::Resume,
        Command::Cancel(_) => ResponseCommand::Cancel,
        Command::Remove(_) => ResponseCommand::Remove,
        Command::List(_) => ResponseCommand::List,
        Command::Get(_) => ResponseCommand::Get,
        Command::OpenFolder(_) => ResponseCommand::OpenFolder,
        Command::GetSettings(_) => ResponseCommand::GetSettings,
        Command::UpdateSettings(_) => ResponseCommand::UpdateSettings,
    }
}

#[derive(serde::Serialize)]
struct FolderResult {
    opened_task_id: String,
}

fn open_folder(destination: &std::path::Path) -> Result<(), HostError> {
    folder_command(destination)?
        .spawn()
        .map_err(|_| HostError::Configuration)?;
    Ok(())
}

fn folder_command(destination: &std::path::Path) -> Result<std::process::Command, HostError> {
    // Only an existing canonical task directory, never a UI-supplied command/path.
    let canonical = std::fs::canonicalize(destination).map_err(|_| HostError::Configuration)?;
    if !canonical.is_dir() || canonical != destination || !cfg!(windows) {
        return Err(HostError::Configuration);
    }
    let explorer = absolute_environment_path("SystemRoot")?.join("explorer.exe");
    let mut command = std::process::Command::new(explorer);
    command
        .arg(canonical)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    Ok(command)
}

fn parse_task_id(value: &str) -> Result<TaskId, ProtocolError> {
    TaskId::parse(value).map_err(|_| ProtocolError::without_context(ErrorCode::TaskNotFound))
}

fn task_description(snapshot: &TaskSnapshot) -> Result<TaskDescription, HostError> {
    let destination = snapshot
        .destination()
        .to_str()
        .ok_or(HostError::Projection)?
        .to_owned();
    Ok(TaskDescription {
        task_id: snapshot.task_id().to_string(),
        display_name: snapshot.display_name().to_owned(),
        destination,
        source_origin: snapshot.source_origin().to_owned(),
        state: task_state(snapshot.state()),
        transfer_mode: transfer_mode(snapshot.transfer_mode()),
        expected_size: snapshot.expected_size(),
        bytes_completed: snapshot.bytes_completed(),
        workers: snapshot.workers().get(),
        speed_bytes_per_second: snapshot.speed_bytes_per_second(),
        eta_seconds: snapshot.eta_seconds(),
        created_at: timestamp_rfc3339(snapshot.created_at()),
        updated_at: timestamp_rfc3339(snapshot.updated_at()),
        error: snapshot
            .failure()
            .map(|failure| task_failure_error(failure, Some(snapshot.task_id()))),
    })
}

fn progress_description(progress: TaskProgress) -> ProgressData {
    ProgressData {
        task_id: progress.task_id().to_string(),
        bytes_completed: progress.bytes_completed(),
        expected_size: progress.expected_size(),
        speed_bytes_per_second: progress.speed_bytes_per_second(),
        eta_seconds: progress.eta_seconds(),
        active_workers: progress.active_workers(),
        sampled_at: timestamp_rfc3339(progress.sampled_at()),
    }
}

const fn task_state(state: TaskState) -> TaskStateName {
    match state {
        TaskState::Queued => TaskStateName::Queued,
        TaskState::Probing => TaskStateName::Probing,
        TaskState::Downloading => TaskStateName::Downloading,
        TaskState::Paused => TaskStateName::Paused,
        TaskState::Validating => TaskStateName::Validating,
        TaskState::Promoting => TaskStateName::Promoting,
        TaskState::Completed => TaskStateName::Completed,
        TaskState::Failed => TaskStateName::Failed,
        TaskState::Cancelled => TaskStateName::Cancelled,
    }
}

const fn transfer_mode(mode: TransferMode) -> TransferModeName {
    match mode {
        TransferMode::Pending => TransferModeName::Pending,
        TransferMode::Single => TransferModeName::Single,
        TransferMode::Segmented => TransferModeName::Segmented,
    }
}

fn task_failure_error(failure: TaskFailure, task_id: Option<TaskId>) -> ProtocolError {
    let mut context = task_id.map_or_else(ErrorContext::default, |task_id| {
        ErrorContext::default().with_task_id(task_id.to_string())
    });
    if let Some(status) = failure.http_status() {
        context = context.with_status(status);
    }
    if let Some(seconds) = failure.retry_after_seconds() {
        context = context.with_retry_after_seconds(seconds);
    }
    ProtocolError::new(failure_code(failure.kind()), context)
}

const fn failure_code(kind: TaskFailureKind) -> ErrorCode {
    match kind {
        TaskFailureKind::Cancelled => ErrorCode::Cancelled,
        TaskFailureKind::ProbeFailed => ErrorCode::ProbeFailed,
        TaskFailureKind::HttpStatus => ErrorCode::HttpStatus,
        TaskFailureKind::RangeResponseInvalid => ErrorCode::RangeResponseInvalid,
        TaskFailureKind::ResourceChanged => ErrorCode::ResourceChanged,
        TaskFailureKind::RetryExhausted => ErrorCode::RetryExhausted,
        TaskFailureKind::Storage => ErrorCode::StorageError,
        TaskFailureKind::DiskFull => ErrorCode::DiskFull,
        TaskFailureKind::AccessDenied => ErrorCode::AccessDenied,
        TaskFailureKind::FileLocked => ErrorCode::FileLocked,
        TaskFailureKind::FileExists => ErrorCode::FileExists,
        TaskFailureKind::State => ErrorCode::StateCorrupt,
        TaskFailureKind::Internal => ErrorCode::InternalError,
    }
}

fn task_engine_error(error: &TaskEngineError, task_id: Option<TaskId>) -> ProtocolError {
    let code = match error {
        TaskEngineError::Config(_) => ErrorCode::InvalidSettings,
        TaskEngineError::TaskNotFound => ErrorCode::TaskNotFound,
        TaskEngineError::InvalidTaskState
        | TaskEngineError::PartialRetained
        | TaskEngineError::OperationSuperseded => ErrorCode::InvalidTaskState,
        TaskEngineError::TooManyTasks
        | TaskEngineError::EventSequenceExhausted
        | TaskEngineError::RuntimeUnavailable
        | TaskEngineError::Internal
        | TaskEngineError::SchedulerSetup(_) => ErrorCode::InternalError,
        TaskEngineError::State(error) => state_error_code(*error),
        TaskEngineError::Persistence(error) => persistence_error_code(error),
        TaskEngineError::ProbeSetup(_) => ErrorCode::ProbeFailed,
        TaskEngineError::ControlFailed(kind) => failure_code(*kind),
    };
    let context = task_id.map_or_else(ErrorContext::default, |task_id| {
        ErrorContext::default().with_task_id(task_id.to_string())
    });
    ProtocolError::new(code, context)
}

const fn state_error_code(error: StateValidationError) -> ErrorCode {
    match error {
        StateValidationError::InvalidUrl => ErrorCode::InvalidUrl,
        StateValidationError::InvalidDestination => ErrorCode::InvalidDestination,
        StateValidationError::InvalidFilename => ErrorCode::InvalidFilename,
        StateValidationError::InvalidWorkerCount => ErrorCode::InvalidSettings,
        StateValidationError::InvalidTaskId => ErrorCode::TaskNotFound,
        StateValidationError::InvalidTimestamp
        | StateValidationError::InvalidRevision
        | StateValidationError::InvalidResource
        | StateValidationError::InvalidValidator
        | StateValidationError::InvalidPartialPath
        | StateValidationError::InvalidFinalPath
        | StateValidationError::InvalidCompletedRanges
        | StateValidationError::InconsistentState
        | StateValidationError::InvalidTransition => ErrorCode::StateCorrupt,
    }
}

const fn persistence_error_code(error: &PersistenceError) -> ErrorCode {
    match error {
        PersistenceError::InvalidTask(error) => state_error_code(*error),
        PersistenceError::Io { failure, .. } => io_failure_code(*failure),
        PersistenceError::ExistingStateInvalid
        | PersistenceError::StaleRevision
        | PersistenceError::UnsafeStoreLayout => ErrorCode::StateCorrupt,
        PersistenceError::InvalidCheckpointPolicy => ErrorCode::InvalidSettings,
        PersistenceError::StoreLocked
        | PersistenceError::StateTooLarge
        | PersistenceError::Serialization
        | PersistenceError::TooManyTaskFiles
        | PersistenceError::CleanupRequiresTerminal => ErrorCode::StorageError,
    }
}

const fn io_failure_code(failure: IoFailure) -> ErrorCode {
    match failure {
        IoFailure::DiskFull => ErrorCode::DiskFull,
        IoFailure::AccessDenied => ErrorCode::AccessDenied,
        IoFailure::FileLocked => ErrorCode::FileLocked,
        IoFailure::AlreadyExists => ErrorCode::FileExists,
        IoFailure::NotFound | IoFailure::Unsupported | IoFailure::Other => ErrorCode::StorageError,
    }
}

fn timestamp_rfc3339(timestamp: TimestampMillis) -> String {
    let millis = timestamp.get();
    let total_seconds = millis / 1_000;
    let millisecond = millis % 1_000;
    let days = total_seconds / 86_400;
    let seconds_in_day = total_seconds % 86_400;
    let hour = seconds_in_day / 3_600;
    let minute = (seconds_in_day % 3_600) / 60;
    let second = seconds_in_day % 60;
    let (year, month, day) = civil_date(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millisecond:03}Z")
}

fn civil_date(days_since_epoch: u64) -> (i64, i64, i64) {
    let shifted = i64::try_from(days_since_epoch).unwrap_or(i64::MAX) + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

fn absolute_environment_path(name: &str) -> Result<PathBuf, HostError> {
    let value = env::var_os(name).ok_or(HostError::Configuration)?;
    if value.is_empty() {
        return Err(HostError::Configuration);
    }
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(HostError::Configuration);
    }
    Ok(path)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Cursor, Write};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use download_manager_engine::persistence::{TaskId, TaskState};
    use download_manager_engine::task::{TaskEngine, TaskEngineOptions};
    use download_manager_protocol::{read_frame, write_frame};
    use download_manager_test_server::{Fixture, ServerConfig, TestServer};
    use serde_json::{Value, json};

    use super::{HostConfig, Session, run_host, timestamp_rfc3339};
    use download_manager_engine::persistence::TimestampMillis;

    #[derive(Clone, Default)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("writer lock").write(buffer)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct Directories {
        root: std::path::PathBuf,
        state: std::path::PathBuf,
        destination: std::path::PathBuf,
    }

    impl Directories {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "download-manager-native-{label}-{}-{nonce}",
                std::process::id()
            ));
            let state = root.join("state");
            let destination = root.join("destination");
            fs::create_dir_all(&state).expect("state directory");
            fs::create_dir_all(&destination).expect("destination directory");
            Self {
                root,
                state,
                destination,
            }
        }
    }

    impl Drop for Directories {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn framed(values: &[Value]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in values {
            write_frame(&mut bytes, value).expect("encode input");
        }
        bytes
    }

    fn messages(writer: &SharedWriter) -> Vec<Value> {
        let bytes = writer.0.lock().expect("writer lock").clone();
        let mut cursor = Cursor::new(bytes);
        let mut messages = Vec::new();
        while let Some(body) = read_frame(&mut cursor).expect("decode output frame") {
            messages.push(serde_json::from_slice(&body).expect("decode output JSON"));
        }
        messages
    }

    fn hello(correlation: &str) -> Value {
        json!({
            "protocol_version": 2,
            "correlation_id": correlation,
            "kind": "command",
            "command": "hello",
            "payload": {
                "supported_versions": [2],
                "client_name": "native-host-test",
                "client_version": "0.1.0"
            }
        })
    }

    #[test]
    fn folder_open_uses_only_canonical_directory_and_absolute_explorer() {
        let directories = Directories::new("folder argument with spaces");
        let destination = fs::canonicalize(&directories.destination).expect("canonical directory");
        let command = super::folder_command(&destination).expect("folder command");
        assert!(std::path::Path::new(command.get_program()).is_absolute());
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![destination.as_os_str()]
        );
        assert!(super::folder_command(&destination.join("missing")).is_err());
        let file = destination.join("not-a-directory");
        fs::write(&file, b"fixture").expect("fixture file");
        assert!(super::folder_command(&file).is_err());
    }

    #[test]
    fn unix_milliseconds_render_as_bounded_rfc3339() {
        assert_eq!(
            timestamp_rfc3339(TimestampMillis::new(0).expect("epoch")),
            "1970-01-01T00:00:00.000Z"
        );
        assert_eq!(
            timestamp_rfc3339(TimestampMillis::new(951_827_696_789).expect("leap date")),
            "2000-02-29T12:34:56.789Z"
        );
        assert_eq!(
            timestamp_rfc3339(TimestampMillis::new(253_402_300_799_999).expect("maximum date")),
            "9999-12-31T23:59:59.999Z"
        );
    }

    #[test]
    fn hello_negotiates_then_emits_authoritative_snapshot() {
        let directories = Directories::new("hello");
        let writer = SharedWriter::default();
        run_host(
            Cursor::new(framed(&[hello("hello-1")])),
            writer.clone(),
            HostConfig::new(
                directories.state.clone(),
                Some(directories.destination.clone()),
            ),
        )
        .expect("run host");

        let output = messages(&writer);
        assert_eq!(output.len(), 2);
        assert_eq!(output[0]["kind"], "response");
        assert_eq!(output[0]["command"], "hello");
        assert_eq!(output[0]["ok"], true);
        assert_eq!(output[0]["result"]["max_message_bytes"], 1_048_576);
        assert_eq!(output[1]["kind"], "event");
        assert_eq!(output[1]["event"], "snapshot");
        assert_eq!(output[1]["sequence"], 0);
        assert_eq!(output[1]["data"]["tasks"], json!([]));
        assert_eq!(output[1]["data"]["complete"], true);
    }

    #[test]
    fn reconnect_rebuilds_queued_tasks_from_persistent_snapshot() {
        let directories = Directories::new("reconnect");
        let engine = TaskEngine::open(&directories.state, TaskEngineOptions::default())
            .expect("open setup engine");
        let task = engine
            .create_task_default(
                "https://example.invalid/file.bin",
                &directories.destination,
                "file.bin",
            )
            .expect("create queued task");
        drop(engine);

        for attempt in 0..2 {
            let writer = SharedWriter::default();
            run_host(
                Cursor::new(framed(&[hello(&format!("hello-{attempt}"))])),
                writer.clone(),
                HostConfig::new(
                    directories.state.clone(),
                    Some(directories.destination.clone()),
                ),
            )
            .expect("run reconnect host");
            let output = messages(&writer);
            let snapshot = output
                .iter()
                .find(|message| message["event"] == "snapshot")
                .expect("snapshot event");
            assert_eq!(
                snapshot["data"]["tasks"][0]["task_id"],
                task.task_id().to_string()
            );
            assert_eq!(snapshot["data"]["tasks"][0]["state"], "queued");
        }
    }

    #[test]
    fn unadvertised_sensitive_fields_are_rejected_without_echo() {
        let directories = Directories::new("sensitive-rejection");
        let add = json!({
            "protocol_version": 2,
            "correlation_id": "add-sensitive",
            "kind": "command",
            "command": "add",
            "payload": {
                "url": "https://example.invalid/private?token=never-echo",
                "destination": directories.destination.to_string_lossy(),
                "request_context": {
                    "credentials": {
                        "authorization": {
                            "scheme": "Bearer",
                            "value": "never-echo-authorization"
                        }
                    }
                }
            }
        });
        let writer = SharedWriter::default();
        run_host(
            Cursor::new(framed(&[hello("hello-sensitive"), add])),
            writer.clone(),
            HostConfig::new(
                directories.state.clone(),
                Some(directories.destination.clone()),
            ),
        )
        .expect("run sensitive rejection");
        let output = messages(&writer);
        let response = output
            .iter()
            .find(|message| message["command"] == "add")
            .expect("add rejection");
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "PROTOCOL_INVALID_MESSAGE");
        let serialized = serde_json::to_string(&output).expect("serialize output");
        assert!(!serialized.contains("never-echo"));
        assert!(
            TaskEngine::open(&directories.state, TaskEngineOptions::default())
                .expect("reopen rejected state")
                .snapshots()
                .is_empty()
        );
    }

    #[test]
    fn list_cursors_page_one_consistent_bounded_snapshot() {
        let directories = Directories::new("list-pages");
        let engine = TaskEngine::open(&directories.state, TaskEngineOptions::default())
            .expect("open list engine");
        for index in 0..5 {
            engine
                .create_task_default(
                    &format!("https://example.invalid/{index}.bin"),
                    &directories.destination,
                    &format!("{index}.bin"),
                )
                .expect("create list task");
        }
        let writer = SharedWriter::default();
        let mut session = Session::new(writer.clone(), Some(directories.destination.clone()));
        let first_json = json!({
            "protocol_version": 2,
            "correlation_id": "list-0",
            "kind": "command",
            "command": "list",
            "payload": {"cursor": null, "limit": 200, "include_terminal": true}
        });
        let (_, command) = download_manager_protocol::decode_command(
            &serde_json::to_vec(&first_json).expect("encode first list"),
        )
        .expect("decode first list")
        .into_parts();
        let download_manager_protocol::Command::List(first) = command else {
            panic!("expected list payload");
        };
        session
            .send_list(&engine, "list-0".to_owned(), &first)
            .expect("send first list page");
        let first_output = messages(&writer);
        assert_eq!(
            first_output[0]["result"]["tasks"]
                .as_array()
                .expect("tasks")
                .len(),
            4
        );
        assert_eq!(first_output[0]["result"]["page_index"], 0);
        assert_eq!(first_output[0]["result"]["complete"], false);
        let cursor = first_output[0]["result"]["next_cursor"]
            .as_str()
            .expect("next cursor");
        let snapshot_id = first_output[0]["result"]["snapshot_id"].clone();

        let second_json = json!({
            "protocol_version": 2,
            "correlation_id": "list-1",
            "kind": "command",
            "command": "list",
            "payload": {"cursor": cursor, "limit": 200, "include_terminal": true}
        });
        let (_, command) = download_manager_protocol::decode_command(
            &serde_json::to_vec(&second_json).expect("encode second list"),
        )
        .expect("decode second list")
        .into_parts();
        let download_manager_protocol::Command::List(second) = command else {
            panic!("expected list payload");
        };
        session
            .send_list(&engine, "list-1".to_owned(), &second)
            .expect("send second list page");
        let output = messages(&writer);
        assert_eq!(output[1]["result"]["snapshot_id"], snapshot_id);
        assert_eq!(output[1]["result"]["page_index"], 1);
        assert_eq!(
            output[1]["result"]["tasks"]
                .as_array()
                .expect("tasks")
                .len(),
            1
        );
        assert_eq!(output[1]["result"]["next_cursor"], Value::Null);
        assert_eq!(output[1]["result"]["complete"], true);
    }

    #[test]
    fn malformed_message_is_sanitized_and_does_not_prevent_later_hello() {
        let directories = Directories::new("malformed");
        let mut input = Vec::new();
        let malformed = br#"{"protocol_version":2,"correlation_id":"safe","correlation_id":"other","kind":"command","command":"hello","payload":{}}"#;
        input.extend_from_slice(
            &u32::try_from(malformed.len())
                .expect("length")
                .to_le_bytes(),
        );
        input.extend_from_slice(malformed);
        input.extend_from_slice(&framed(&[hello("hello-after-error")]));
        let writer = SharedWriter::default();
        run_host(
            Cursor::new(input),
            writer.clone(),
            HostConfig::new(
                directories.state.clone(),
                Some(directories.destination.clone()),
            ),
        )
        .expect("run host");

        let output = messages(&writer);
        assert_eq!(output[0]["command"], "protocol");
        assert_eq!(output[0]["error"]["code"], "PROTOCOL_INVALID_MESSAGE");
        assert_eq!(output[1]["command"], "hello");
        assert_eq!(output[2]["event"], "snapshot");
    }

    #[test]
    fn add_command_and_stdin_eof_leave_only_durable_inactive_state() {
        let server = TestServer::start(ServerConfig {
            fixture: Fixture {
                len: 2 * 1024 * 1024,
                seed: 41,
            },
            rules: Vec::new(),
        })
        .expect("start test server");
        let directories = Directories::new("add-eof");
        let add = json!({
            "protocol_version": 2,
            "correlation_id": "add-1",
            "kind": "command",
            "command": "add",
            "payload": {
                "url": server.url("/fixture"),
                "destination": directories.destination.to_string_lossy(),
                "suggested_filename": "native.bin",
                "workers": 2
            }
        });
        let writer = SharedWriter::default();
        run_host(
            Cursor::new(framed(&[hello("hello-add"), add])),
            writer.clone(),
            HostConfig::new(
                directories.state.clone(),
                Some(directories.destination.clone()),
            ),
        )
        .expect("run add host");

        let output = messages(&writer);
        let response = output
            .iter()
            .find(|message| message["kind"] == "response" && message["command"] == "add")
            .expect("add response");
        assert_eq!(response["ok"], true);
        let task_id = TaskId::parse(response["result"]["task_id"].as_str().expect("task id"))
            .expect("parse task id");
        let recovered = TaskEngine::open(&directories.state, TaskEngineOptions::default())
            .expect("reopen after EOF")
            .snapshot(task_id)
            .expect("recover task");
        assert!(matches!(
            recovered.state(),
            TaskState::Paused | TaskState::Failed | TaskState::Completed
        ));
        assert_eq!(recovered.active_workers(), 0);
    }

    #[test]
    fn oversized_frame_gets_bounded_protocol_error_without_reading_a_body() {
        let directories = Directories::new("oversized");
        let declared =
            u32::try_from(download_manager_protocol::MAX_MESSAGE_BYTES + 1).expect("message limit");
        let writer = SharedWriter::default();
        run_host(
            Cursor::new(declared.to_le_bytes()),
            writer.clone(),
            HostConfig::new(
                directories.state.clone(),
                Some(directories.destination.clone()),
            ),
        )
        .expect("run oversized host");
        let output = messages(&writer);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["command"], "protocol");
        assert_eq!(output[0]["error"]["code"], "PROTOCOL_MESSAGE_TOO_LARGE");
    }

    #[test]
    fn command_before_hello_and_unsupported_version_close_safely() {
        for (label, command, code) in [
            (
                "before-hello",
                json!({
                    "protocol_version": 2,
                    "correlation_id": "get-1",
                    "kind": "command",
                    "command": "get",
                    "payload": {"task_id": "00000000-0000-4000-8000-000000000000"}
                }),
                "PROTOCOL_INVALID_MESSAGE",
            ),
            (
                "unsupported",
                json!({
                    "protocol_version": 99,
                    "correlation_id": "hello-2",
                    "kind": "command",
                    "command": "hello",
                    "payload": {}
                }),
                "PROTOCOL_UNSUPPORTED_VERSION",
            ),
        ] {
            let directories = Directories::new(label);
            let writer = SharedWriter::default();
            run_host(
                Cursor::new(framed(&[command, hello("must-not-run")])),
                writer.clone(),
                HostConfig::new(
                    directories.state.clone(),
                    Some(directories.destination.clone()),
                ),
            )
            .expect("run rejected host");
            let output = messages(&writer);
            assert_eq!(output.len(), 1);
            assert_eq!(output[0]["error"]["code"], code);
        }
    }
}
