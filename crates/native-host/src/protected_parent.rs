//! Opt-in private dispatcher; transport class/metadata are NOT Firefox policy.
//! No ordinary stdio/parent1 entry selects this module. The future fixed parent
//! API must keep these frames out of its generic extension-caller command API.
use super::{HostError, Inbound, Session, task_description};
use download_manager_engine::{
    persistence::TaskId,
    scheduler::WorkerCount,
    task::{
        HandoffRequest, HandoffSnapshot, ProtectionContext, ProtectionDecision, ProtectionReceiver,
        ProtectionRequest, TaskEngine, TaskEngineError,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    io::Write,
};
use tokio::sync::mpsc;

const MAX_INPUT: usize = 32 * 1024;
const MAX_OUTPUT: usize = 64 * 1024;
const CAPACITY: usize = 32;
const HISTORY: usize = 10_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    protection_bridge: u8,
    context_id: String,
    body: Input,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    Prepare {
        task_id: String,
        source_url: String,
        file_name: String,
    },
    Commit {
        task_id: String,
    },
    Abort {
        task_id: String,
    },
    Status {
        task_id: String,
    },
    Resolution {
        challenge_id: String,
        task_id: String,
        generation: String,
        decision: Decision,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Decision {
    PermitPublication,
    Blocked,
    Unavailable,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Acceptance {
    parent_transport: u8,
    admission_id: String,
    receiver_id: String,
    context_id: String,
    kind: String,
}

pub(super) struct Scope {
    context: ProtectionContext,
    pending: HashMap<TaskId, ProtectionRequest>,
    seen: HashSet<TaskId>,
    tasks: HashSet<TaskId>,
    intents: HashSet<TaskId>,
}
impl Scope {
    pub(super) fn open(receiver: &mut ProtectionReceiver) -> Result<Self, HostError> {
        Ok(Self {
            context: receiver
                .open_context()
                .map_err(|_| HostError::LocalSession)?,
            pending: HashMap::new(),
            seen: HashSet::new(),
            tasks: HashSet::new(),
            intents: HashSet::new(),
        })
    }
    fn guard(&self) -> Result<(), HostError> {
        if self.context.is_available() {
            Ok(())
        } else {
            Err(HostError::LocalSession)
        }
    }
    pub(super) fn close(&mut self) {
        // Revoke even already-consumed approvals BEFORE any I/O retirement await.
        self.context.close();
        self.pending.clear();
    }
    pub(super) async fn admit<W: Write>(
        &self,
        session: &Session<'_, W>,
        inbound: &mut mpsc::Receiver<Inbound>,
    ) -> Result<(), HostError> {
        self.guard()?;
        let admission = TaskId::new().to_string();
        let receiver = self.context.receiver_id().to_string();
        let context = self.context.id().to_string();
        let announcement = |kind| {
            json!({"parent_transport":2,"admission_id":admission,
            "receiver_id":receiver,"context_id":context,"kind":kind,"capture_ready":false})
        };
        session.write(&announcement("offer")).await?;
        let Some(Inbound::Body(body)) = inbound.recv().await else {
            return Err(HostError::LocalSession);
        };
        if body.len() > 512 {
            return Err(HostError::LocalSession);
        }
        let accepted: Acceptance =
            serde_json::from_slice(&body).map_err(|_| HostError::LocalSession)?;
        if accepted.parent_transport != 2
            || accepted.admission_id != admission
            || accepted.receiver_id != receiver
            || accepted.context_id != context
            || accepted.kind != "accept"
        {
            return Err(HostError::LocalSession);
        }
        self.guard()?;
        session.write(&announcement("ready")).await
    }
    async fn write<W: Write>(
        &self,
        session: &Session<'_, W>,
        body: impl Serialize,
    ) -> Result<(), HostError> {
        self.guard()?;
        let message =
            json!({"protection_bridge":1,"context_id":self.context.id().to_string(),"body":body});
        let bytes = serde_json::to_vec(&message).map_err(|_| HostError::Projection)?;
        if bytes.len() > MAX_OUTPUT {
            return Err(HostError::Projection);
        }
        self.guard()?;
        session.write(&message).await
    }
    async fn challenge<W: Write>(
        &mut self,
        session: &Session<'_, W>,
        request: ProtectionRequest,
    ) -> Result<(), HostError> {
        self.guard()?;
        if request.context_id() != Some(self.context.id())
            || request.connection_id() != self.context.receiver_id()
            || !self.tasks.contains(&request.task_id())
        {
            return Err(HostError::LocalSession);
        }
        if request.is_cancelled() {
            return Ok(());
        }
        // Native-origin UUIDs are consumed even if serialization/write later fails.
        if self.pending.len() >= CAPACITY
            || self.seen.len() >= HISTORY
            || !self.seen.insert(request.id())
        {
            return Err(HostError::LocalSession);
        }
        let id = request.id();
        self.pending.insert(id, request); // Retain the one response route before effects.
        let request = self.pending.get(&id).ok_or(HostError::Projection)?;
        let mut sha256 = String::with_capacity(64);
        for byte in request.fingerprint().sha256() {
            write!(sha256, "{byte:02x}").map_err(|_| HostError::Projection)?;
        }
        let body = json!({"kind":"challenge","receiver_id":request.connection_id().to_string(),
            "challenge_id":id.to_string(),"task_id":request.task_id().to_string(),
            "generation":request.generation().to_string(),"source_url":request.source_url(),
            "file_name":request.file_name(),"length":request.fingerprint().length().to_string(),"sha256":sha256});
        self.write(session, body).await
    }
    async fn handoff<W: Write>(
        &self,
        session: &Session<'_, W>,
        id: TaskId,
        outcome: Result<HandoffSnapshot, TaskEngineError>,
    ) -> Result<(), HostError> {
        let body = match outcome {
            Ok(receipt) => json!({"kind":"handoff","task_id":id.to_string(),"ok":true,
                "phase":receipt.phase(),"task":task_description(receipt.task())?}),
            Err(_) => {
                json!({"kind":"handoff","task_id":id.to_string(),"ok":false,"error":"refused"})
            }
        };
        self.write(session, body).await
    }
    async fn input<W: Write>(
        &mut self,
        session: &Session<'_, W>,
        engine: &TaskEngine,
        bytes: &[u8],
    ) -> Result<(), HostError> {
        self.guard()?;
        if bytes.len() > MAX_INPUT {
            return Err(HostError::LocalSession);
        }
        let value: Envelope = serde_json::from_slice(bytes).map_err(|_| HostError::LocalSession)?;
        if value.protection_bridge != 1 || value.context_id != self.context.id().to_string() {
            return Err(HostError::LocalSession);
        }
        match value.body {
            Input::Prepare {
                task_id,
                source_url,
                file_name,
            } => {
                let id = exact_id(&task_id)?;
                if self.intents.len() >= HISTORY && !self.intents.contains(&id) {
                    return Err(HostError::LocalSession);
                }
                let destination = session
                    .default_destination
                    .as_ref()
                    .ok_or(HostError::Configuration)?;
                let workers = WorkerCount::try_from(
                    session
                        .settings
                        .as_ref()
                        .map_or(4, |settings| settings.current.default_workers),
                )
                .map_err(|_| HostError::Configuration)?;
                self.intents.insert(id); // Retain intent before exclusive persistence.
                let outcome =
                    HandoffRequest::new(id, &source_url, destination, &file_name, workers, None)
                        .and_then(|request| {
                            engine.prepare_handoff(request.require_protection_in(&self.context))
                        });
                // A failed/uncertain prepare is not authority to mutate a
                // former context's task. Keep intent separate from admission.
                if outcome.is_ok() {
                    self.tasks.insert(id);
                }
                self.handoff(session, id, outcome).await
            }
            Input::Commit { ref task_id }
            | Input::Abort { ref task_id }
            | Input::Status { ref task_id } => {
                let id = exact_id(task_id)?;
                // Status is metadata-only across reconnect. Mutating controls
                // cannot adopt a former session's task just by presenting its ID.
                let outcome = match value.body {
                    Input::Status { .. } => engine.handoff_status(id),
                    Input::Commit { .. } if self.tasks.contains(&id) => engine.commit_handoff(id),
                    Input::Abort { .. } if self.tasks.contains(&id) => engine.abort_handoff(id),
                    _ => Err(TaskEngineError::InvalidTaskState),
                };
                self.handoff(session, id, outcome).await
            }
            Input::Resolution {
                challenge_id,
                task_id,
                generation,
                decision,
            } => {
                let id = exact_id(&challenge_id)?;
                let task = exact_id(&task_id)?;
                let request = self.pending.get(&id).ok_or(HostError::LocalSession)?;
                if request.task_id() != task || request.generation().to_string() != generation {
                    return Err(HostError::LocalSession);
                }
                // Consume before resolving. Neither write completion nor this
                // acknowledgment claims policy correctness or publication.
                let request = self.pending.remove(&id).ok_or(HostError::LocalSession)?;
                let decision = match decision {
                    Decision::PermitPublication => ProtectionDecision::PermitPublication,
                    Decision::Blocked => ProtectionDecision::Blocked,
                    Decision::Unavailable => ProtectionDecision::Unavailable,
                };
                let accepted = request.decide(decision).is_ok();
                self.write(
                    session,
                    json!({"kind":"resolution","challenge_id":id.to_string(),"accepted":accepted}),
                )
                .await
            }
        }
    }
    pub(super) async fn run<W: Write>(
        &mut self,
        receiver: &mut ProtectionReceiver,
        session: &mut Session<'_, W>,
        engine: &mut TaskEngine,
        inbound: &mut mpsc::Receiver<Inbound>,
    ) -> Result<(), HostError> {
        loop {
            self.guard()?;
            if let Some(snapshots) = engine.take_overflow_snapshot() {
                session.send_snapshot_events(&snapshots).await?;
            }
            tokio::select! {
                item = inbound.recv() => {
                    let Some(Inbound::Body(body)) = item else { return Err(HostError::LocalSession); };
                    let value: Value = serde_json::from_slice(&body).map_err(|_| HostError::LocalSession)?;
                    if value.get("protection_bridge").is_some() {
                        // Reparse ORIGINAL bytes, never the duplicate-collapsing Value.
                        self.input(session, engine, &body).await?;
                    } else {
                        let message = download_manager_protocol::decode_command(&body).map_err(|_| HostError::LocalSession)?;
                        let (correlation, command) = message.into_parts();
                        if matches!(command, download_manager_protocol::Command::Hello(_)) { return Err(HostError::LocalSession); }
                        session.dispatch(engine, correlation, command).await?;
                    }
                }
                request = receiver.recv() => {
                    self.challenge(session, request.ok_or(HostError::LocalSession)?).await?;
                }
                event = engine.next_event() => { session.send_engine_event(&event?).await?; }
            }
        }
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        self.close();
    }
}
fn exact_id(value: &str) -> Result<TaskId, HostError> {
    TaskId::parse(value)
        .ok()
        .filter(|id| id.to_string() == value)
        .ok_or(HostError::LocalSession)
}
