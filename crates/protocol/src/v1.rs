use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::strict_json::parse_object;
use crate::{PROTOCOL_VERSION, is_valid_correlation_id};

const MAX_URL_CHARS: usize = 16_384;
const MAX_PATH_CHARS: usize = 32_767;
const MAX_NAME_CHARS: usize = 255;

/// Decoded protocol-v1 command with all envelope and payload fields validated.
pub struct CommandMessage {
    correlation_id: String,
    command: Command,
}

impl CommandMessage {
    /// Returns the safe correlation token and decoded command.
    #[must_use]
    pub fn into_parts(self) -> (String, Command) {
        (self.correlation_id, self.command)
    }
}

/// A supported protocol-v1 command.
pub enum Command {
    Hello(HelloPayload),
    Add(AddPayload),
    Pause(TaskIdPayload),
    Resume(TaskIdPayload),
    Cancel(CancelPayload),
    Remove(RemovePayload),
    List(ListPayload),
    Get(TaskIdPayload),
    UpdateSettings(UpdateSettingsPayload),
}

/// Safe error classification returned by strict command decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandDecodeFailure {
    InvalidMessage,
    UnsupportedVersion,
    UnknownCommand,
}

/// Path-, URL-, credential-, and payload-free command decoding error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("native protocol command is invalid")]
pub struct CommandDecodeError {
    correlation_id: Option<String>,
    command: Option<ResponseCommand>,
    failure: CommandDecodeFailure,
}

impl CommandDecodeError {
    /// A validated correlation ID that can safely be echoed, if recoverable.
    #[must_use]
    pub fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    /// A recognized command discriminator that can safely be echoed.
    #[must_use]
    pub const fn command(&self) -> Option<ResponseCommand> {
        self.command
    }

    /// Stable reason category without parser details or source data.
    #[must_use]
    pub const fn failure(&self) -> CommandDecodeFailure {
        self.failure
    }
}

/// Strictly decodes one UTF-8 JSON object as a protocol-v1 command.
///
/// Duplicate object members are rejected at every depth before Serde field
/// decoding. Unknown fields, invalid bounds, and unsupported nested shapes are
/// also rejected.
///
/// # Errors
///
/// Returns a sanitized category and only an independently validated
/// correlation ID / recognized command when recoverable.
pub fn decode_command(body: &[u8]) -> Result<CommandMessage, CommandDecodeError> {
    let object = parse_object(body)
        .map_err(|_| decode_error(None, None, CommandDecodeFailure::InvalidMessage))?;
    let correlation_id = object
        .get("correlation_id")
        .and_then(Value::as_str)
        .filter(|value| is_valid_correlation_id(value))
        .map(str::to_owned);

    match object.get("protocol_version").and_then(Value::as_u64) {
        Some(version) if version != u64::from(PROTOCOL_VERSION) => {
            return Err(decode_error(
                correlation_id,
                None,
                CommandDecodeFailure::UnsupportedVersion,
            ));
        }
        Some(_) => {}
        None => {
            return Err(decode_error(
                correlation_id,
                None,
                CommandDecodeFailure::InvalidMessage,
            ));
        }
    }

    let command_name = object.get("command").and_then(Value::as_str);
    let recognized = command_name.and_then(ResponseCommand::parse);
    let envelope: CommandEnvelope =
        serde_json::from_value(Value::Object(object)).map_err(|_| {
            decode_error(
                correlation_id.clone(),
                recognized,
                CommandDecodeFailure::InvalidMessage,
            )
        })?;
    if !is_valid_correlation_id(&envelope.correlation_id) {
        return Err(decode_error(
            None,
            recognized,
            CommandDecodeFailure::InvalidMessage,
        ));
    }
    let Some(command_name) = CommandName::parse(&envelope.command) else {
        return Err(decode_error(
            Some(envelope.correlation_id),
            None,
            CommandDecodeFailure::UnknownCommand,
        ));
    };
    let command = decode_payload(command_name, envelope.payload).map_err(|()| {
        decode_error(
            Some(envelope.correlation_id.clone()),
            Some(command_name.response()),
            CommandDecodeFailure::InvalidMessage,
        )
    })?;
    Ok(CommandMessage {
        correlation_id: envelope.correlation_id,
        command,
    })
}

fn decode_error(
    correlation_id: Option<String>,
    command: Option<ResponseCommand>,
    failure: CommandDecodeFailure,
) -> CommandDecodeError {
    CommandDecodeError {
        correlation_id,
        command,
        failure,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandEnvelope {
    #[serde(rename = "protocol_version")]
    _protocol_version: u16,
    correlation_id: String,
    #[serde(rename = "kind")]
    _kind: CommandKind,
    command: String,
    payload: Value,
}

#[derive(Deserialize)]
enum CommandKind {
    #[serde(rename = "command")]
    Command,
}

#[derive(Clone, Copy)]
enum CommandName {
    Hello,
    Add,
    Pause,
    Resume,
    Cancel,
    Remove,
    List,
    Get,
    UpdateSettings,
}

impl CommandName {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "hello" => Some(Self::Hello),
            "add" => Some(Self::Add),
            "pause" => Some(Self::Pause),
            "resume" => Some(Self::Resume),
            "cancel" => Some(Self::Cancel),
            "remove" => Some(Self::Remove),
            "list" => Some(Self::List),
            "get" => Some(Self::Get),
            "update_settings" => Some(Self::UpdateSettings),
            _ => None,
        }
    }

    const fn response(self) -> ResponseCommand {
        match self {
            Self::Hello => ResponseCommand::Hello,
            Self::Add => ResponseCommand::Add,
            Self::Pause => ResponseCommand::Pause,
            Self::Resume => ResponseCommand::Resume,
            Self::Cancel => ResponseCommand::Cancel,
            Self::Remove => ResponseCommand::Remove,
            Self::List => ResponseCommand::List,
            Self::Get => ResponseCommand::Get,
            Self::UpdateSettings => ResponseCommand::UpdateSettings,
        }
    }
}

fn decode_payload(command: CommandName, payload: Value) -> Result<Command, ()> {
    match command {
        CommandName::Hello => payload_as::<HelloPayload>(payload).map(Command::Hello),
        CommandName::Add => payload_as::<AddPayload>(payload).map(Command::Add),
        CommandName::Pause => payload_as::<TaskIdPayload>(payload).map(Command::Pause),
        CommandName::Resume => payload_as::<TaskIdPayload>(payload).map(Command::Resume),
        CommandName::Cancel => payload_as::<CancelPayload>(payload).map(Command::Cancel),
        CommandName::Remove => payload_as::<RemovePayload>(payload).map(Command::Remove),
        CommandName::List => payload_as::<ListPayload>(payload).map(Command::List),
        CommandName::Get => payload_as::<TaskIdPayload>(payload).map(Command::Get),
        CommandName::UpdateSettings => {
            payload_as::<UpdateSettingsPayload>(payload).map(Command::UpdateSettings)
        }
    }
}

trait Validate {
    fn validate(&self) -> bool;
}

fn payload_as<T>(payload: Value) -> Result<T, ()>
where
    T: for<'de> Deserialize<'de> + Validate,
{
    let parsed: T = serde_json::from_value(payload).map_err(|_| ())?;
    parsed.validate().then_some(parsed).ok_or(())
}

/// Validated hello payload.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelloPayload {
    client_name: String,
    client_version: String,
    supported_versions: Vec<u16>,
}

impl HelloPayload {
    #[must_use]
    pub fn client_name(&self) -> &str {
        &self.client_name
    }

    #[must_use]
    pub fn client_version(&self) -> &str {
        &self.client_version
    }

    #[must_use]
    pub fn supported_versions(&self) -> &[u16] {
        &self.supported_versions
    }
}

impl Validate for HelloPayload {
    fn validate(&self) -> bool {
        bounded_nonempty(&self.client_name, 128)
            && bounded_nonempty(&self.client_version, 128)
            && !self.supported_versions.is_empty()
            && self.supported_versions.len() <= 16
            && self.supported_versions.iter().all(|version| *version > 0)
            && self
                .supported_versions
                .iter()
                .copied()
                .collect::<HashSet<_>>()
                .len()
                == self.supported_versions.len()
    }
}

/// Validated add payload. Sensitive values intentionally have no `Debug`
/// implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddPayload {
    url: String,
    destination: Option<String>,
    suggested_filename: Option<String>,
    workers: Option<u8>,
    checksum: Option<ChecksumInput>,
    request_context: Option<RequestContextInput>,
}

impl AddPayload {
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    #[must_use]
    pub fn destination(&self) -> Option<&str> {
        self.destination.as_deref()
    }

    #[must_use]
    pub fn suggested_filename(&self) -> Option<&str> {
        self.suggested_filename.as_deref()
    }

    #[must_use]
    pub const fn workers(&self) -> Option<u8> {
        self.workers
    }

    #[must_use]
    pub const fn has_checksum(&self) -> bool {
        self.checksum.is_some()
    }

    #[must_use]
    pub const fn has_request_context(&self) -> bool {
        self.request_context.is_some()
    }
}

impl Validate for AddPayload {
    fn validate(&self) -> bool {
        valid_http_url(&self.url)
            && self
                .destination
                .as_ref()
                .is_none_or(|value| bounded_nonempty(value, MAX_PATH_CHARS))
            && self
                .suggested_filename
                .as_ref()
                .is_none_or(|value| bounded_nonempty(value, MAX_NAME_CHARS))
            && self
                .workers
                .is_none_or(|value| matches!(value, 1 | 2 | 4 | 8))
            && self.checksum.as_ref().is_none_or(Validate::validate)
            && self.request_context.as_ref().is_none_or(Validate::validate)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChecksumInput {
    algorithm: String,
    digest: String,
}

impl Validate for ChecksumInput {
    fn validate(&self) -> bool {
        self.algorithm == "sha256"
            && self.digest.len() == 64
            && self.digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestContextInput {
    referrer: Option<String>,
    credentials: Option<CredentialsInput>,
}

impl Validate for RequestContextInput {
    fn validate(&self) -> bool {
        (self.referrer.is_some() || self.credentials.is_some())
            && self
                .referrer
                .as_ref()
                .is_none_or(|value| valid_http_url(value))
            && self.credentials.as_ref().is_none_or(Validate::validate)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialsInput {
    cookies: Option<Vec<CookieInput>>,
    authorization: Option<AuthorizationInput>,
}

impl Validate for CredentialsInput {
    fn validate(&self) -> bool {
        (self.cookies.is_some() || self.authorization.is_some())
            && self.cookies.as_ref().is_none_or(|cookies| {
                cookies.len() <= 256 && cookies.iter().all(Validate::validate)
            })
            && self.authorization.as_ref().is_none_or(Validate::validate)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CookieInput {
    name: String,
    value: String,
    domain: String,
    path: String,
    secure: bool,
    http_only: bool,
    #[serde(default)]
    expires_at: RequiredNullableString,
}

impl Validate for CookieInput {
    fn validate(&self) -> bool {
        let _ = (self.secure, self.http_only);
        bounded_nonempty(&self.name, 4096)
            && char_count_at_most(&self.value, 16_384)
            && bounded_nonempty(&self.domain, 253)
            && bounded_nonempty(&self.path, 4096)
            && match &self.expires_at {
                RequiredNullableString::Missing => false,
                RequiredNullableString::Null => true,
                RequiredNullableString::String(value) => valid_rfc3339(value),
            }
    }
}

#[derive(Default)]
enum RequiredNullableString {
    #[default]
    Missing,
    Null,
    String(String),
}

impl<'de> Deserialize<'de> for RequiredNullableString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Option::<String>::deserialize(deserializer).map(|value| match value {
            Some(value) => Self::String(value),
            None => Self::Null,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorizationInput {
    scheme: String,
    value: String,
}

impl Validate for AuthorizationInput {
    fn validate(&self) -> bool {
        bounded_nonempty(&self.scheme, 64)
            && self.scheme.bytes().all(valid_token_byte)
            && bounded_nonempty(&self.value, 16_384)
    }
}

/// Payload containing one task ID.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskIdPayload {
    task_id: String,
}

impl TaskIdPayload {
    #[must_use]
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
}

impl Validate for TaskIdPayload {
    fn validate(&self) -> bool {
        valid_task_id(&self.task_id)
    }
}

/// Validated cancel payload.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelPayload {
    task_id: String,
    partial_policy: CancelPartial,
}

impl CancelPayload {
    #[must_use]
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    #[must_use]
    pub const fn partial_policy(&self) -> CancelPartial {
        self.partial_policy
    }
}

impl Validate for CancelPayload {
    fn validate(&self) -> bool {
        valid_task_id(&self.task_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelPartial {
    Keep,
    Delete,
}

/// Validated remove payload.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovePayload {
    task_id: String,
    delete_partial: bool,
}

impl RemovePayload {
    #[must_use]
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    #[must_use]
    pub const fn delete_partial(&self) -> bool {
        self.delete_partial
    }
}

impl Validate for RemovePayload {
    fn validate(&self) -> bool {
        valid_task_id(&self.task_id)
    }
}

/// Validated list page request.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListPayload {
    #[serde(default)]
    cursor: RequiredNullableString,
    limit: u16,
    include_terminal: bool,
}

impl ListPayload {
    #[must_use]
    pub fn cursor(&self) -> Option<&str> {
        match &self.cursor {
            RequiredNullableString::String(value) => Some(value),
            RequiredNullableString::Missing | RequiredNullableString::Null => None,
        }
    }

    #[must_use]
    pub const fn limit(&self) -> u16 {
        self.limit
    }

    #[must_use]
    pub const fn include_terminal(&self) -> bool {
        self.include_terminal
    }
}

impl Validate for ListPayload {
    fn validate(&self) -> bool {
        (1..=200).contains(&self.limit)
            && match &self.cursor {
                RequiredNullableString::Missing => false,
                RequiredNullableString::Null => true,
                RequiredNullableString::String(value) => bounded_nonempty(value, 1024),
            }
    }
}

/// Validated settings update payload.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateSettingsPayload {
    settings: SettingsPatchInput,
}

impl UpdateSettingsPayload {
    #[must_use]
    pub const fn settings(&self) -> &SettingsPatchInput {
        &self.settings
    }
}

impl Validate for UpdateSettingsPayload {
    fn validate(&self) -> bool {
        self.settings.validate()
    }
}

/// Syntactically valid protocol-v1 settings patch. Persistence is introduced
/// by the settings work item; the native host may currently reject it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsPatchInput {
    destination: Option<String>,
    default_workers: Option<u8>,
    global_concurrency: Option<u8>,
    per_host_concurrency: Option<u8>,
    retry_limit: Option<u8>,
    keep_partial_on_cancel: Option<bool>,
    keep_partial_on_failure: Option<bool>,
}

impl Validate for SettingsPatchInput {
    fn validate(&self) -> bool {
        let populated = self.destination.is_some()
            || self.default_workers.is_some()
            || self.global_concurrency.is_some()
            || self.per_host_concurrency.is_some()
            || self.retry_limit.is_some()
            || self.keep_partial_on_cancel.is_some()
            || self.keep_partial_on_failure.is_some();
        populated
            && self
                .destination
                .as_ref()
                .is_none_or(|value| bounded_nonempty(value, MAX_PATH_CHARS))
            && self
                .default_workers
                .is_none_or(|value| matches!(value, 1 | 2 | 4 | 8))
            && self
                .global_concurrency
                .is_none_or(|value| (1..=32).contains(&value))
            && self
                .per_host_concurrency
                .is_none_or(|value| (1..=8).contains(&value))
            && self.retry_limit.is_none_or(|value| value <= 20)
    }
}

fn valid_task_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.get(8) == Some(&b'-')
        && bytes.get(13) == Some(&b'-')
        && bytes.get(14) == Some(&b'4')
        && bytes.get(18) == Some(&b'-')
        && bytes
            .get(19)
            .is_some_and(|byte| matches!(byte, b'8' | b'9' | b'a' | b'b'))
        && bytes.get(23) == Some(&b'-')
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23)
                || byte.is_ascii_digit()
                || matches!(byte, b'a'..=b'f')
        })
}

fn valid_http_url(value: &str) -> bool {
    bounded_nonempty(value, MAX_URL_CHARS)
        && (value.starts_with("http://") || value.starts_with("https://"))
}

fn bounded_nonempty(value: &str, maximum: usize) -> bool {
    !value.is_empty() && char_count_at_most(value, maximum)
}

fn char_count_at_most(value: &str, maximum: usize) -> bool {
    value.chars().take(maximum.saturating_add(1)).count() <= maximum
}

fn valid_rfc3339(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || !matches!(bytes.get(4), Some(b'-'))
        || !matches!(bytes.get(7), Some(b'-'))
        || !matches!(bytes.get(10), Some(b'T'))
        || !matches!(bytes.get(13), Some(b':'))
        || !matches!(bytes.get(16), Some(b':'))
    {
        return false;
    }
    let Some(year) = decimal(bytes, 0, 4) else {
        return false;
    };
    let Some(month) = decimal(bytes, 5, 7) else {
        return false;
    };
    let Some(day) = decimal(bytes, 8, 10) else {
        return false;
    };
    let Some(hour) = decimal(bytes, 11, 13) else {
        return false;
    };
    let Some(minute) = decimal(bytes, 14, 16) else {
        return false;
    };
    let Some(second) = decimal(bytes, 17, 19) else {
        return false;
    };
    let leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => return false,
    };
    if day == 0 || day > month_days || hour > 23 || minute > 59 || second > 59 {
        return false;
    }

    let mut zone = 19;
    if bytes.get(zone) == Some(&b'.') {
        zone += 1;
        let fraction_start = zone;
        while bytes.get(zone).is_some_and(u8::is_ascii_digit) {
            zone += 1;
        }
        if zone == fraction_start {
            return false;
        }
    }
    if bytes.get(zone) == Some(&b'Z') {
        return zone + 1 == bytes.len();
    }
    if !matches!(bytes.get(zone), Some(b'+' | b'-')) || zone + 6 != bytes.len() {
        return false;
    }
    matches!(bytes.get(zone + 3), Some(b':'))
        && decimal(bytes, zone + 1, zone + 3).is_some_and(|offset| offset <= 23)
        && decimal(bytes, zone + 4, zone + 6).is_some_and(|offset| offset <= 59)
}

fn decimal(bytes: &[u8], start: usize, end: usize) -> Option<u32> {
    bytes
        .get(start..end)?
        .iter()
        .try_fold(0_u32, |value, byte| {
            byte.is_ascii_digit()
                .then(|| value * 10 + u32::from(*byte - b'0'))
        })
}

fn valid_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// Command discriminator echoed by protocol responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseCommand {
    Hello,
    Add,
    Pause,
    Resume,
    Cancel,
    Remove,
    List,
    Get,
    UpdateSettings,
    Protocol,
}

impl ResponseCommand {
    fn parse(value: &str) -> Option<Self> {
        CommandName::parse(value).map(CommandName::response)
    }
}

/// Stable protocol-v1 error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ProtocolUnsupportedVersion,
    ProtocolUnknownCommand,
    ProtocolInvalidMessage,
    ProtocolMessageTooLarge,
    InvalidUrl,
    UnsupportedScheme,
    InvalidDestination,
    InvalidFilename,
    InvalidSettings,
    TaskNotFound,
    InvalidTaskState,
    AuthRequired,
    AuthExpired,
    RedirectRejected,
    ProbeFailed,
    RangeUnsupported,
    RangeResponseInvalid,
    ResourceChanged,
    HttpStatus,
    RetryExhausted,
    StorageError,
    DiskFull,
    AccessDenied,
    FileLocked,
    FileExists,
    StateCorrupt,
    ChecksumMismatch,
    Cancelled,
    InternalError,
}

impl ErrorCode {
    /// Static non-sensitive user-facing summary.
    #[must_use]
    pub const fn display_message(self) -> &'static str {
        match self {
            Self::ProtocolUnsupportedVersion => {
                "The extension and helper do not share a protocol version."
            }
            Self::ProtocolUnknownCommand => "The helper does not recognize that command.",
            Self::ProtocolInvalidMessage => "The helper rejected an invalid protocol message.",
            Self::ProtocolMessageTooLarge => "The protocol message is too large.",
            Self::InvalidUrl => "The download URL is invalid.",
            Self::UnsupportedScheme => "The URL scheme is unsupported.",
            Self::InvalidDestination => "The destination is invalid.",
            Self::InvalidFilename => "The filename is invalid.",
            Self::InvalidSettings => "The settings update is invalid or unsupported.",
            Self::TaskNotFound => "The requested download was not found.",
            Self::InvalidTaskState => {
                "The download cannot perform that action in its current state."
            }
            Self::AuthRequired => "The server requires authorization.",
            Self::AuthExpired => "The supplied authorization has expired.",
            Self::RedirectRejected => "The server redirect was rejected.",
            Self::ProbeFailed => "The helper could not establish a safe resource identity.",
            Self::RangeUnsupported => "The server does not support the required byte ranges.",
            Self::RangeResponseInvalid => "The server returned invalid byte-range data.",
            Self::ResourceChanged => {
                "The remote resource changed and retained bytes cannot be reused."
            }
            Self::HttpStatus => "The server returned an unsuccessful HTTP status.",
            Self::RetryExhausted => "The retry budget was exhausted.",
            Self::StorageError => "The helper could not safely store the download.",
            Self::DiskFull => "The destination volume is full.",
            Self::AccessDenied => "The destination denied access.",
            Self::FileLocked => "A required file is locked by another process.",
            Self::FileExists => "The final destination already exists.",
            Self::StateCorrupt => "The saved download state is not safe to use.",
            Self::ChecksumMismatch => "The completed download did not match its checksum.",
            Self::Cancelled => "The download was cancelled.",
            Self::InternalError => "The helper encountered an internal error.",
        }
    }
}

/// Optional bounded and non-sensitive error context.
#[derive(Clone, Default, Serialize)]
pub struct ErrorContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after_seconds: Option<u64>,
}

impl ErrorContext {
    #[must_use]
    pub fn with_task_id(mut self, task_id: impl Into<String>) -> Self {
        self.task_id = Some(task_id.into());
        self
    }

    #[must_use]
    pub const fn with_status(mut self, status: u16) -> Self {
        self.status_code = Some(status);
        self
    }

    #[must_use]
    pub const fn with_retry_after_seconds(mut self, seconds: u64) -> Self {
        self.retry_after_seconds = Some(seconds);
        self
    }

    fn is_empty(&self) -> bool {
        self.task_id.is_none() && self.status_code.is_none() && self.retry_after_seconds.is_none()
    }
}

/// Protocol-v1 error object.
#[derive(Clone, Serialize)]
pub struct ProtocolError {
    code: ErrorCode,
    display_message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<ErrorContext>,
}

impl ProtocolError {
    #[must_use]
    pub fn new(code: ErrorCode, context: ErrorContext) -> Self {
        let context = (!context.is_empty()).then_some(context);
        Self {
            code,
            display_message: code.display_message(),
            context,
        }
    }

    #[must_use]
    pub fn without_context(code: ErrorCode) -> Self {
        Self::new(code, ErrorContext::default())
    }
}

/// Protocol response envelope.
#[derive(Serialize)]
pub struct ResponseMessage {
    protocol_version: u16,
    correlation_id: String,
    kind: &'static str,
    command: ResponseCommand,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ProtocolError>,
}

/// Infallible for current result structures, but kept explicit at the protocol
/// construction boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("protocol result could not be serialized")]
pub struct MessageBuildError;

impl ResponseMessage {
    /// Builds an `ok: true` response with exactly one serialized result.
    ///
    /// # Errors
    ///
    /// Returns a sanitized error if result serialization fails.
    pub fn success(
        correlation_id: impl Into<String>,
        command: ResponseCommand,
        result: &impl Serialize,
    ) -> Result<Self, MessageBuildError> {
        let result = serde_json::to_value(result).map_err(|_| MessageBuildError)?;
        Ok(Self {
            protocol_version: PROTOCOL_VERSION,
            correlation_id: correlation_id.into(),
            kind: "response",
            command,
            ok: true,
            result: Some(result),
            error: None,
        })
    }

    #[must_use]
    pub fn failure(
        correlation_id: impl Into<String>,
        command: ResponseCommand,
        error: ProtocolError,
    ) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            correlation_id: correlation_id.into(),
            kind: "response",
            command,
            ok: false,
            result: None,
            error: Some(error),
        }
    }
}

/// Hello result returned after version negotiation.
#[derive(Serialize)]
pub struct HelloResult {
    pub selected_version: u16,
    pub helper_version: &'static str,
    pub capabilities: Vec<&'static str>,
    pub max_message_bytes: usize,
}

/// Snapshot/list result page.
#[derive(Clone, Serialize)]
pub struct SnapshotPage {
    pub snapshot_id: String,
    pub page_index: u64,
    pub tasks: Vec<TaskDescription>,
    pub next_cursor: Option<String>,
    pub complete: bool,
}

/// Protocol-v1 settings projection.
#[derive(Clone, Serialize)]
pub struct SettingsDescription {
    pub destination: String,
    pub default_workers: u8,
    pub global_concurrency: u8,
    pub per_host_concurrency: u8,
    pub retry_limit: u8,
    pub keep_partial_on_cancel: bool,
    pub keep_partial_on_failure: bool,
}

/// Complete protocol-v1 task projection.
#[derive(Clone, Serialize)]
pub struct TaskDescription {
    pub task_id: String,
    pub source_origin: String,
    pub display_name: String,
    pub destination: String,
    pub state: TaskStateName,
    pub transfer_mode: TransferModeName,
    pub expected_size: Option<u64>,
    pub bytes_completed: u64,
    pub workers: u8,
    pub speed_bytes_per_second: Option<u64>,
    pub eta_seconds: Option<u64>,
    pub created_at: String,
    pub updated_at: String,
    pub error: Option<ProtocolError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStateName {
    Queued,
    Probing,
    Downloading,
    Paused,
    Validating,
    Promoting,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferModeName {
    Pending,
    Segmented,
    #[serde(rename = "single")]
    Single,
}

#[derive(Serialize)]
pub struct RemoveResult {
    pub removed_task_id: String,
}

/// Event discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventName {
    Snapshot,
    StateChanged,
    Progress,
    Warning,
    Completed,
    Failed,
}

/// Protocol event envelope.
#[derive(Serialize)]
pub struct EventMessage {
    protocol_version: u16,
    correlation_id: String,
    kind: &'static str,
    event: EventName,
    sequence: u64,
    emitted_at: String,
    data: Value,
}

impl EventMessage {
    /// Builds one event envelope.
    ///
    /// # Errors
    ///
    /// Returns a sanitized error if event data serialization fails.
    pub fn new(
        correlation_id: impl Into<String>,
        event: EventName,
        sequence: u64,
        emitted_at: impl Into<String>,
        data: &impl Serialize,
    ) -> Result<Self, MessageBuildError> {
        let data = serde_json::to_value(data).map_err(|_| MessageBuildError)?;
        Ok(Self {
            protocol_version: PROTOCOL_VERSION,
            correlation_id: correlation_id.into(),
            kind: "event",
            event,
            sequence,
            emitted_at: emitted_at.into(),
            data,
        })
    }
}

#[derive(Serialize)]
pub struct StateChangedData {
    pub task: TaskDescription,
    pub previous_state: TaskStateName,
}

#[derive(Serialize)]
pub struct ProgressData {
    pub task_id: String,
    pub bytes_completed: u64,
    pub expected_size: Option<u64>,
    pub speed_bytes_per_second: Option<u64>,
    pub eta_seconds: Option<u64>,
    pub active_workers: u8,
    pub sampled_at: String,
}

#[derive(Serialize)]
pub struct WarningData {
    pub task_id: Option<String>,
    pub warning: ProtocolError,
}

#[derive(Serialize)]
pub struct FailedData {
    pub task: TaskDescription,
    pub error: ProtocolError,
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{
        Command, CommandDecodeFailure, ErrorCode, ProtocolError, ResponseCommand, ResponseMessage,
        decode_command,
    };

    #[test]
    fn decodes_strict_hello_and_add_commands() {
        let hello = include_bytes!("../../../protocol/schema/v1/examples/hello.command.json");
        let (_, command) = decode_command(hello).expect("decode hello").into_parts();
        let Command::Hello(payload) = command else {
            panic!("expected hello");
        };
        assert_eq!(payload.supported_versions(), &[1]);

        let add_example = include_bytes!("../../../protocol/schema/v1/examples/add.command.json");
        let (_, command) = decode_command(add_example)
            .expect("decode add example")
            .into_parts();
        let Command::Add(payload) = command else {
            panic!("expected add");
        };
        assert!(payload.has_checksum());

        let add = br#"{
          "protocol_version": 1,
          "correlation_id": "add-1",
          "kind": "command",
          "command": "add",
          "payload": {
            "url": "https://example.invalid/file.bin",
            "destination": "C:\\Users\\Example User\\Downloads",
            "suggested_filename": "file.bin",
            "workers": 8
          }
        }"#;
        let (_, command) = decode_command(add).expect("decode add").into_parts();
        let Command::Add(payload) = command else {
            panic!("expected add");
        };
        assert_eq!(payload.workers(), Some(8));

        let authenticated_add = br#"{
          "protocol_version": 1,
          "correlation_id": "add-auth",
          "kind": "command",
          "command": "add",
          "payload": {
            "url": "https://example.invalid/private.bin",
            "request_context": {
              "referrer": "https://example.invalid/page",
              "credentials": {
                "cookies": [{
                  "name": "session",
                  "value": "fake",
                  "domain": "example.invalid",
                  "path": "/",
                  "secure": true,
                  "http_only": true,
                  "expires_at": null
                }],
                "authorization": {"scheme": "Bearer", "value": "fake"}
              }
            }
          }
        }"#;
        let (_, command) = decode_command(authenticated_add)
            .expect("decode reserved authenticated add")
            .into_parts();
        let Command::Add(payload) = command else {
            panic!("expected add");
        };
        assert!(payload.has_request_context());

        let settings =
            include_bytes!("../../../protocol/schema/v1/examples/update-settings.command.json");
        let (_, command) = decode_command(settings)
            .expect("decode settings")
            .into_parts();
        assert!(matches!(command, Command::UpdateSettings(_)));
    }

    #[test]
    fn rejects_duplicates_unknown_fields_and_invalid_nested_shapes() {
        for (index, body) in [
            br#"{"protocol_version":1,"correlation_id":"x","kind":"command","command":"get","payload":{"task_id":"x","task_id":"y"}}"#.as_slice(),
            br#"{"protocol_version":1,"correlation_id":"x","kind":"command","command":"get","payload":{"task_id":"x","extra":true}}"#.as_slice(),
            br#"{"protocol_version":1,"correlation_id":"x","kind":"command","command":"add","payload":{"url":"https://example.invalid","request_context":{"credentials":{}}}}"#.as_slice(),
            br#"{"protocol_version":1,"correlation_id":"x","kind":"command","command":"list","payload":{"limit":1,"include_terminal":true}}"#.as_slice(),
            br#"{"protocol_version":1,"correlation_id":"x","kind":"command","command":"add","payload":{"url":"https://example.invalid","request_context":{"credentials":{"cookies":[{"name":"a","value":"b","domain":"example.invalid","path":"/","secure":true,"http_only":true,"expires_at":"not-a-date"}]}}}}"#.as_slice(),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                decode_command(body)
                    .err()
                    .unwrap_or_else(|| panic!("case {index} must reject"))
                    .failure(),
                CommandDecodeFailure::InvalidMessage
            );
        }
    }

    #[test]
    fn decode_errors_never_debug_raw_sensitive_payloads() {
        let secret = "https://example.invalid/file?token=do-not-log";
        let body = format!(
            r#"{{"protocol_version":1,"correlation_id":"add-safe","kind":"command","command":"add","payload":{{"url":"{secret}","unexpected":true}}}}"#
        );
        let error = decode_command(body.as_bytes()).err().expect("invalid add");
        let debug = format!("{error:?}");
        assert!(!debug.contains(secret));
        assert!(!debug.contains("do-not-log"));
        assert!(debug.contains("add-safe"));
    }

    #[test]
    fn reports_only_validated_bootstrap_fields() {
        let unsupported = decode_command(
            br#"{"protocol_version":2,"correlation_id":"safe-2","kind":"command","command":"hello","payload":{}}"#,
        )
        .err()
        .expect("unsupported version");
        assert_eq!(
            unsupported.failure(),
            CommandDecodeFailure::UnsupportedVersion
        );
        assert_eq!(unsupported.correlation_id(), Some("safe-2"));
        assert_eq!(unsupported.command(), None);

        let unknown = decode_command(
            br#"{"protocol_version":1,"correlation_id":"safe-1","kind":"command","command":"future","payload":{}}"#,
        )
        .err()
        .expect("unknown command");
        assert_eq!(unknown.failure(), CommandDecodeFailure::UnknownCommand);
        assert_eq!(unknown.correlation_id(), Some("safe-1"));

        let invalid_correlation = decode_command(
            br#"{"protocol_version":1,"correlation_id":"secret/value","kind":"command","command":"get","payload":{}}"#,
        )
        .err()
        .expect("invalid correlation");
        assert_eq!(invalid_correlation.correlation_id(), None);
    }

    #[test]
    fn error_code_serialization_matches_the_v1_registry() {
        let cases = [
            (
                ErrorCode::ProtocolUnsupportedVersion,
                "PROTOCOL_UNSUPPORTED_VERSION",
            ),
            (
                ErrorCode::ProtocolUnknownCommand,
                "PROTOCOL_UNKNOWN_COMMAND",
            ),
            (
                ErrorCode::ProtocolInvalidMessage,
                "PROTOCOL_INVALID_MESSAGE",
            ),
            (
                ErrorCode::ProtocolMessageTooLarge,
                "PROTOCOL_MESSAGE_TOO_LARGE",
            ),
            (ErrorCode::InvalidUrl, "INVALID_URL"),
            (ErrorCode::UnsupportedScheme, "UNSUPPORTED_SCHEME"),
            (ErrorCode::InvalidDestination, "INVALID_DESTINATION"),
            (ErrorCode::InvalidFilename, "INVALID_FILENAME"),
            (ErrorCode::InvalidSettings, "INVALID_SETTINGS"),
            (ErrorCode::TaskNotFound, "TASK_NOT_FOUND"),
            (ErrorCode::InvalidTaskState, "INVALID_TASK_STATE"),
            (ErrorCode::AuthRequired, "AUTH_REQUIRED"),
            (ErrorCode::AuthExpired, "AUTH_EXPIRED"),
            (ErrorCode::RedirectRejected, "REDIRECT_REJECTED"),
            (ErrorCode::ProbeFailed, "PROBE_FAILED"),
            (ErrorCode::RangeUnsupported, "RANGE_UNSUPPORTED"),
            (ErrorCode::RangeResponseInvalid, "RANGE_RESPONSE_INVALID"),
            (ErrorCode::ResourceChanged, "RESOURCE_CHANGED"),
            (ErrorCode::HttpStatus, "HTTP_STATUS"),
            (ErrorCode::RetryExhausted, "RETRY_EXHAUSTED"),
            (ErrorCode::StorageError, "STORAGE_ERROR"),
            (ErrorCode::DiskFull, "DISK_FULL"),
            (ErrorCode::AccessDenied, "ACCESS_DENIED"),
            (ErrorCode::FileLocked, "FILE_LOCKED"),
            (ErrorCode::FileExists, "FILE_EXISTS"),
            (ErrorCode::StateCorrupt, "STATE_CORRUPT"),
            (ErrorCode::ChecksumMismatch, "CHECKSUM_MISMATCH"),
            (ErrorCode::Cancelled, "CANCELLED"),
            (ErrorCode::InternalError, "INTERNAL_ERROR"),
        ];
        let schema: Value = serde_json::from_str(include_str!(
            "../../../protocol/schema/v1/message.schema.json"
        ))
        .expect("parse protocol schema");
        let schema_codes = schema
            .pointer("/$defs/error/properties/code/enum")
            .and_then(Value::as_array)
            .expect("schema error codes")
            .iter()
            .map(|value| value.as_str().expect("string error code"))
            .collect::<Vec<_>>();
        assert_eq!(
            cases
                .iter()
                .map(|(_, expected)| *expected)
                .collect::<Vec<_>>(),
            schema_codes
        );
        for (code, expected) in cases {
            assert_eq!(
                serde_json::to_value(code).expect("serialize error code"),
                Value::String(expected.to_owned())
            );
        }
    }

    #[test]
    fn response_has_exactly_one_result_or_error_branch() {
        let success = ResponseMessage::success("x", ResponseCommand::Get, &json!({"task": {}}))
            .expect("build success");
        let success = serde_json::to_value(success).expect("serialize success");
        assert_eq!(success.get("ok"), Some(&Value::Bool(true)));
        assert!(success.get("result").is_some());
        assert!(success.get("error").is_none());

        let failure = ResponseMessage::failure(
            "x",
            ResponseCommand::Get,
            ProtocolError::without_context(ErrorCode::TaskNotFound),
        );
        let failure = serde_json::to_value(failure).expect("serialize failure");
        assert_eq!(failure.get("ok"), Some(&Value::Bool(false)));
        assert!(failure.get("result").is_none());
        assert_eq!(
            failure.pointer("/error/code").and_then(Value::as_str),
            Some("TASK_NOT_FOUND")
        );
    }
}
