//! HTTP probing and strict byte-range response validation.

use crate::admission::{Admission, AdmissionError, RequestPermit};
use crate::auth::{ContextError, RequestContext};
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use reqwest::header::{
    ACCEPT_ENCODING, CONTENT_DISPOSITION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE, ETAG,
    HeaderMap, HeaderName, HeaderValue, LAST_MODIFIED, RANGE, RETRY_AFTER,
};
use reqwest::redirect::Policy;
use reqwest::{Client, StatusCode, Url};
use thiserror::Error;

const MAX_REDIRECTS: usize = 10;
const MAX_METADATA_BYTES: usize = 8 * 1024;
const MAX_FILENAME_BYTES: usize = 1024;

/// A validated inclusive assignment within a known resource.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeAssignment {
    start: u64,
    end: u64,
    total: u64,
}

impl RangeAssignment {
    /// Creates an in-bounds assignment.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty resource, reversed range, or endpoint at
    /// or beyond the expected total.
    pub fn new(start: u64, end: u64, total: u64) -> Result<Self, RangeValidationError> {
        if total == 0 {
            return Err(RangeValidationError::InvalidAssignment);
        }
        if start > end || end >= total {
            return Err(RangeValidationError::InvalidAssignment);
        }
        Ok(Self { start, end, total })
    }

    /// First assigned byte.
    #[must_use]
    pub const fn start(self) -> u64 {
        self.start
    }

    /// Last assigned byte, inclusive.
    #[must_use]
    pub const fn end(self) -> u64 {
        self.end
    }

    /// Expected complete-resource size.
    #[must_use]
    pub const fn total(self) -> u64 {
        self.total
    }

    /// Number of assigned bytes.
    #[must_use]
    pub const fn len(self) -> u64 {
        self.end - self.start + 1
    }

    /// Assignments are never empty after construction.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        false
    }
}

/// Parsed strong or weak HTTP entity tag.
#[derive(Clone, Eq, PartialEq)]
pub struct EntityTag {
    weak: bool,
    opaque: String,
}

impl fmt::Debug for EntityTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EntityTag")
            .field("weak", &self.weak)
            .field("opaque", &"<redacted>")
            .finish()
    }
}

impl EntityTag {
    /// Parses one complete `ETag` header value without normalizing identity.
    ///
    /// # Errors
    ///
    /// Returns [`RangeValidationError::InvalidHeader`] for malformed, empty, or
    /// unsafe values.
    pub fn parse(value: &str) -> Result<Self, RangeValidationError> {
        parse_etag(value)
    }

    /// Whether this is a weak validator.
    #[must_use]
    pub const fn is_weak(&self) -> bool {
        self.weak
    }

    /// Opaque tag contents without quotes or the weak prefix.
    #[must_use]
    pub fn opaque(&self) -> &str {
        &self.opaque
    }
}

/// Resource validators accepted during probing.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Validators {
    /// Parsed `ETag`, when valid and present.
    pub etag: Option<EntityTag>,
    /// Canonical header text for a valid `Last-Modified` date.
    pub last_modified: Option<String>,
}

impl Validators {
    /// Only a strong `ETag` establishes byte identity for segmentation/reuse.
    #[must_use]
    pub fn has_strong_identity(&self) -> bool {
        self.etag.as_ref().is_some_and(|tag| !tag.is_weak())
    }
}

/// Why the helper selected a safe single stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FallbackReason {
    /// The server ignored the small ranged `GET` and returned `200`.
    RangeIgnored,
    /// The ignored-range response did not declare a resource length.
    UnknownLength,
    /// No strong `ETag` proves byte identity across independent requests.
    InsufficientIdentity,
}

/// Transfer mode established by a probe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeMode {
    /// A known-size resource proved exact byte-range behavior.
    Segmented,
    /// The resource must use one sequential stream.
    SingleStream(FallbackReason),
    /// A valid zero-length resource.
    Empty,
}

/// Safe metadata learned from a probe.
#[derive(Clone, Eq, PartialEq)]
pub struct ResourceProbe {
    final_url: Url,
    size: Option<u64>,
    filename: Option<String>,
    validators: Validators,
    mode: ProbeMode,
    context: Option<Arc<RequestContext>>,
}

impl ResourceProbe {
    pub(crate) fn check_session_status(&self, status: u16) -> Result<(), ContextError> {
        if self.context.is_some() && matches!(status, 401 | 403) {
            Err(ContextError::Expired)
        } else {
            Ok(())
        }
    }

    pub(crate) fn request_headers(&self) -> Result<HeaderMap, ContextError> {
        self.context.as_ref().map_or_else(
            || Ok(HeaderMap::new()),
            |context| context.headers(&self.final_url),
        )
    }

    /// Final URL after the bounded redirect policy.
    #[must_use]
    pub const fn final_url(&self) -> &Url {
        &self.final_url
    }

    /// Known resource size, or `None` for an unknown-length single stream.
    #[must_use]
    pub const fn size(&self) -> Option<u64> {
        self.size
    }

    /// Untrusted filename candidate from `Content-Disposition` or URL path.
    #[must_use]
    pub fn filename(&self) -> Option<&str> {
        self.filename.as_deref()
    }

    /// Parsed resource validators.
    #[must_use]
    pub const fn validators(&self) -> &Validators {
        &self.validators
    }

    /// Validated transfer capability.
    #[must_use]
    pub const fn mode(&self) -> ProbeMode {
        self.mode
    }
}

impl fmt::Debug for ResourceProbe {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResourceProbe")
            .field("origin", &origin(&self.final_url))
            .field("size", &self.size)
            .field("has_filename", &self.filename.is_some())
            .field("validators", &self.validators)
            .field("mode", &self.mode)
            .field("has_context", &self.context.is_some())
            .finish()
    }
}

/// Successful validation details for a worker response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedRange {
    /// The exact assignment proven by status and `Content-Range`.
    pub assignment: RangeAssignment,
    /// Validators returned with this response.
    pub validators: Validators,
}

/// Strict ranged-response failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RangeValidationError {
    /// Assignment itself is reversed, empty, or out of bounds.
    #[error("range assignment is invalid")]
    InvalidAssignment,
    /// Status must be exactly `206` for a ranged assignment.
    #[error("ranged response status is not 206")]
    WrongStatus,
    /// A required header is absent.
    #[error("required ranged-response header is missing")]
    MissingHeader,
    /// A singleton header occurred more than once.
    #[error("ranged-response header is duplicated")]
    DuplicateHeader,
    /// A header could not be represented as bounded visible text.
    #[error("ranged-response header is invalid")]
    InvalidHeader,
    /// `Content-Range` syntax is invalid or uses an unknown total.
    #[error("Content-Range is malformed")]
    MalformedContentRange,
    /// Parsed range does not exactly equal the assignment.
    #[error("Content-Range does not match the assignment")]
    ContentRangeMismatch,
    /// Parsed complete length conflicts with the accepted resource.
    #[error("Content-Range total does not match the resource")]
    TotalMismatch,
    /// Declared body length does not equal the assignment length.
    #[error("Content-Length does not match the assignment")]
    ContentLengthMismatch,
    /// The server transformed bytes despite requesting identity encoding.
    #[error("ranged response uses a non-identity content encoding")]
    UnexpectedContentEncoding,
    /// An expected validator is absent.
    #[error("resource validator is missing")]
    ValidatorMissing,
    /// A returned validator conflicts with accepted resource identity.
    #[error("resource validator changed")]
    ValidatorChanged,
}

/// Probe operation failure with no URL or response text in its display form.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProbeError {
    /// Shared request admission rejected this operation.
    #[error("probe admission failed: {0}")]
    Admission(#[from] AdmissionError),
    /// Invalid or expired memory-only session.
    #[error("request session is unavailable: {0}")]
    Context(#[from] ContextError),
    /// URL syntax is invalid.
    #[error("URL is invalid")]
    InvalidUrl,
    /// Only direct HTTP and HTTPS are accepted.
    #[error("URL scheme is unsupported")]
    UnsupportedScheme,
    /// URL user-info could expose credentials.
    #[error("URL user-info is forbidden")]
    UserInfoForbidden,
    /// The HTTP client could not be constructed.
    #[error("HTTP client setup failed")]
    ClientSetup,
    /// Network request failed.
    #[error("HTTP request failed")]
    Request,
    /// Redirect policy rejected a target, downgrade, loop, or depth.
    #[error("redirect policy rejected the request")]
    RedirectRejected,
    /// Server returned a non-probe status.
    #[error("server returned HTTP status {status}")]
    HttpStatus {
        /// Numeric status without server-provided reason text.
        status: u16,
        /// Parsed bounded retry guidance where available.
        retry_after_seconds: Option<u64>,
    },
    /// Strict ranged-response metadata failed validation.
    #[error("ranged probe response is invalid: {0}")]
    InvalidRange(#[from] RangeValidationError),
    /// Probe body was shorter or longer than its exact assignment.
    #[error("probe body length does not match its assignment")]
    BodyLengthMismatch,
}

/// Reusable, conservatively configured HTTP probe client.
#[derive(Clone, Debug)]
pub struct ProbeClient {
    client: Client,
    admission: Admission,
}

impl ProbeClient {
    /// Constructs a client with maintained TLS validation, no transparent
    /// decompression, bounded timeouts, and explicit redirect policy.
    ///
    /// # Errors
    ///
    /// Returns [`ProbeError::ClientSetup`] if the HTTP client cannot be built.
    pub fn new() -> Result<Self, ProbeError> {
        Self::with_admission(Admission::new(
            crate::scheduler::ConcurrencyLimits::default(),
        ))
    }

    /// Shares admission with the transfer scheduler, including redirect hops.
    /// # Errors
    /// Returns client setup failure without exposing request context.
    pub fn with_admission(admission: Admission) -> Result<Self, ProbeError> {
        let client = Client::builder()
            .redirect(Policy::none())
            .referer(false)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent("FirefoxDownloadManager/0.1")
            .build()
            .map_err(|_| ProbeError::ClientSetup)?;
        Ok(Self { client, admission })
    }

    /// Probes a direct HTTP(S) resource with `Range: bytes=0-0`.
    ///
    /// # Errors
    ///
    /// Returns a bounded error when URL, redirect, status, metadata, encoding,
    /// or probe body behavior cannot be accepted safely.
    pub async fn probe(&self, input: &str) -> Result<ResourceProbe, ProbeError> {
        self.probe_with_context(input, None).await
    }

    /// Probes with a validated, origin-confined memory-only session.
    /// # Errors
    /// Returns the same conservative probe failures, plus invalid/expired context.
    pub async fn probe_with_context(
        &self,
        input: &str,
        context: Option<Arc<RequestContext>>,
    ) -> Result<ResourceProbe, ProbeError> {
        let mut url = Url::parse(input).map_err(|_| ProbeError::InvalidUrl)?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ProbeError::UnsupportedScheme);
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(ProbeError::UserInfoForbidden);
        }
        url.set_fragment(None);

        let response = self
            .probe_request(url, "bytes=0-0", context.as_deref(), true)
            .await?;

        let status = response.status();
        let final_url = response.url().clone();
        let headers = response.headers().clone();
        if status.is_redirection() {
            return Err(ProbeError::RedirectRejected);
        }
        reject_unexpected_encoding(&headers)?;
        let filename = filename_from_headers_or_url(&headers, &final_url);
        let validators = parse_validators(&headers)?;

        if status == StatusCode::PARTIAL_CONTENT {
            let parsed = required_content_range(&headers)?;
            let assignment = RangeAssignment::new(0, 0, parsed.total)?;
            validate_range_response(status, &headers, assignment, &Validators::default())?;
            read_exact_body(response, assignment.len()).await?;

            if parsed.total > 1 {
                let last = parsed.total - 1;
                let verification = self
                    .probe_request(
                        final_url.clone(),
                        &format!("bytes={last}-{last}"),
                        context.as_deref(),
                        false,
                    )
                    .await?;
                if verification.url() != &final_url {
                    return Err(ProbeError::RedirectRejected);
                }
                let verification_assignment = RangeAssignment::new(last, last, parsed.total)?;
                validate_range_response(
                    verification.status(),
                    verification.headers(),
                    verification_assignment,
                    &validators,
                )?;
                read_exact_body(verification, verification_assignment.len()).await?;
            }

            let mode = if validators.has_strong_identity() {
                ProbeMode::Segmented
            } else {
                ProbeMode::SingleStream(FallbackReason::InsufficientIdentity)
            };
            return Ok(ResourceProbe {
                final_url,
                size: Some(parsed.total),
                filename,
                validators,
                mode,
                context,
            });
        }

        if status == StatusCode::OK {
            let size = optional_u64_header(&headers, CONTENT_LENGTH)?;
            let mode = if size.is_some() {
                ProbeMode::SingleStream(FallbackReason::RangeIgnored)
            } else {
                ProbeMode::SingleStream(FallbackReason::UnknownLength)
            };
            return Ok(ResourceProbe {
                final_url,
                size,
                filename,
                validators,
                mode,
                context,
            });
        }

        if status == StatusCode::RANGE_NOT_SATISFIABLE && unsatisfied_total(&headers)? == Some(0) {
            return Ok(ResourceProbe {
                final_url,
                size: Some(0),
                filename,
                validators,
                mode: ProbeMode::Empty,
                context,
            });
        }

        Err(ProbeError::HttpStatus {
            status: status.as_u16(),
            retry_after_seconds: retry_after_seconds(&headers),
        })
    }
}

impl ProbeClient {
    async fn probe_request(
        &self,
        mut url: Url,
        range: &str,
        context: Option<&RequestContext>,
        follow: bool,
    ) -> Result<AdmittedResponse, ProbeError> {
        for depth in 0..=MAX_REDIRECTS {
            let permit = self.admission.acquire(&url).await?;
            let headers =
                context.map_or_else(|| Ok(HeaderMap::new()), |context| context.headers(&url))?;
            let response = self
                .client
                .get(url.clone())
                .headers(headers)
                .header(RANGE, range)
                .header(ACCEPT_ENCODING, "identity")
                .send()
                .await
                .map_err(|error| classify_request_error(&error))?;
            permit.observe(
                response.status().as_u16(),
                retry_after_seconds(response.headers()),
            );
            if context.is_some() && matches!(response.status().as_u16(), 401 | 403) {
                return Err(ContextError::Expired.into());
            }
            if !response.status().is_redirection() {
                return Ok(AdmittedResponse {
                    response,
                    _permit: permit,
                });
            }
            if !follow
                || depth == MAX_REDIRECTS
                || !matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308)
            {
                return Err(ProbeError::RedirectRejected);
            }
            let location = single_header(response.headers(), reqwest::header::LOCATION)?
                .ok_or(ProbeError::RedirectRejected)?;
            let mut next = url
                .join(location)
                .map_err(|_| ProbeError::RedirectRejected)?;
            if !matches!(next.scheme(), "http" | "https")
                || !next.username().is_empty()
                || next.password().is_some()
                || url.scheme() == "https" && next.scheme() == "http"
                || context.is_some() && next.origin() != url.origin()
            {
                return Err(ProbeError::RedirectRejected);
            }
            next.set_fragment(None);
            url = next;
        }
        Err(ProbeError::RedirectRejected)
    }
}

impl Default for ProbeClient {
    fn default() -> Self {
        Self::new().expect("static HTTP client configuration should be valid")
    }
}

// Response ownership drops before its permit; no probe can release admission
// while its body is still being consumed.
struct AdmittedResponse {
    response: reqwest::Response,
    _permit: RequestPermit,
}
impl std::ops::Deref for AdmittedResponse {
    type Target = reqwest::Response;
    fn deref(&self) -> &Self::Target {
        &self.response
    }
}
impl std::ops::DerefMut for AdmittedResponse {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.response
    }
}

fn classify_request_error(error: &reqwest::Error) -> ProbeError {
    if error.is_redirect() {
        ProbeError::RedirectRejected
    } else {
        ProbeError::Request
    }
}

/// Validates all metadata required before a ranged body may be written.
///
/// Body streaming must separately enforce that exactly `assignment.len()` bytes
/// arrive; this function intentionally validates only status and headers.
///
/// # Errors
///
/// Returns a specific conservative failure for any status, range, length,
/// encoding, or known-validator inconsistency.
pub fn validate_range_response(
    status: StatusCode,
    headers: &HeaderMap,
    assignment: RangeAssignment,
    expected_validators: &Validators,
) -> Result<ValidatedRange, RangeValidationError> {
    if status != StatusCode::PARTIAL_CONTENT {
        return Err(RangeValidationError::WrongStatus);
    }
    reject_unexpected_encoding(headers)?;
    let parsed = required_content_range(headers)?;
    if parsed.start != assignment.start || parsed.end != assignment.end {
        return Err(RangeValidationError::ContentRangeMismatch);
    }
    if parsed.total != assignment.total {
        return Err(RangeValidationError::TotalMismatch);
    }
    if let Some(length) = optional_u64_header(headers, CONTENT_LENGTH)?
        && length != assignment.len()
    {
        return Err(RangeValidationError::ContentLengthMismatch);
    }
    let validators = parse_validators(headers)?;
    validate_expected_validators(expected_validators, &validators)?;
    Ok(ValidatedRange {
        assignment,
        validators,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ContentRange {
    start: u64,
    end: u64,
    total: u64,
}

fn required_content_range(headers: &HeaderMap) -> Result<ContentRange, RangeValidationError> {
    let value =
        single_header(headers, CONTENT_RANGE)?.ok_or(RangeValidationError::MissingHeader)?;
    parse_content_range(value)
}

fn parse_content_range(value: &str) -> Result<ContentRange, RangeValidationError> {
    let value = value
        .strip_prefix("bytes ")
        .ok_or(RangeValidationError::MalformedContentRange)?;
    let (bounds, total) = value
        .split_once('/')
        .ok_or(RangeValidationError::MalformedContentRange)?;
    if total == "*" || total.is_empty() || total.contains('/') {
        return Err(RangeValidationError::MalformedContentRange);
    }
    let (start, end) = bounds
        .split_once('-')
        .ok_or(RangeValidationError::MalformedContentRange)?;
    let start = parse_u64(start).ok_or(RangeValidationError::MalformedContentRange)?;
    let end = parse_u64(end).ok_or(RangeValidationError::MalformedContentRange)?;
    let total = parse_u64(total).ok_or(RangeValidationError::MalformedContentRange)?;
    if total == 0 || start > end || end >= total {
        return Err(RangeValidationError::MalformedContentRange);
    }
    Ok(ContentRange { start, end, total })
}

fn unsatisfied_total(headers: &HeaderMap) -> Result<Option<u64>, RangeValidationError> {
    let Some(value) = single_header(headers, CONTENT_RANGE)? else {
        return Ok(None);
    };
    let Some(total) = value.strip_prefix("bytes */") else {
        return Err(RangeValidationError::MalformedContentRange);
    };
    Ok(parse_u64(total))
}

pub(crate) fn optional_u64_header(
    headers: &HeaderMap,
    name: HeaderName,
) -> Result<Option<u64>, RangeValidationError> {
    single_header(headers, name)?
        .map(|value| parse_u64(value).ok_or(RangeValidationError::InvalidHeader))
        .transpose()
}

fn parse_u64(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn single_header(
    headers: &HeaderMap,
    name: HeaderName,
) -> Result<Option<&str>, RangeValidationError> {
    let mut values = headers.get_all(name).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(RangeValidationError::DuplicateHeader);
    }
    let value = value
        .to_str()
        .map_err(|_| RangeValidationError::InvalidHeader)?;
    if value.len() > MAX_METADATA_BYTES || value.chars().any(char::is_control) {
        return Err(RangeValidationError::InvalidHeader);
    }
    Ok(Some(value))
}

pub(crate) fn reject_unexpected_encoding(headers: &HeaderMap) -> Result<(), RangeValidationError> {
    if let Some(value) = single_header(headers, CONTENT_ENCODING)?
        && !value.eq_ignore_ascii_case("identity")
    {
        return Err(RangeValidationError::UnexpectedContentEncoding);
    }
    Ok(())
}

pub(crate) fn parse_validators(headers: &HeaderMap) -> Result<Validators, RangeValidationError> {
    let etag = single_header(headers, ETAG)?.map(parse_etag).transpose()?;
    let last_modified = single_header(headers, LAST_MODIFIED)?
        .map(|value| {
            httpdate::parse_http_date(value)
                .map(|_| value.to_owned())
                .map_err(|_| RangeValidationError::InvalidHeader)
        })
        .transpose()?;
    Ok(Validators {
        etag,
        last_modified,
    })
}

fn parse_etag(value: &str) -> Result<EntityTag, RangeValidationError> {
    let (weak, quoted) = value
        .strip_prefix("W/")
        .map_or((false, value), |rest| (true, rest));
    let opaque = quoted
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .ok_or(RangeValidationError::InvalidHeader)?;
    if opaque.is_empty()
        || opaque
            .bytes()
            .any(|byte| byte == b'"' || byte == 0x7f || byte < 0x21)
    {
        return Err(RangeValidationError::InvalidHeader);
    }
    Ok(EntityTag {
        weak,
        opaque: opaque.to_owned(),
    })
}

fn if_range_value(validators: &Validators) -> Option<String> {
    validators
        .etag
        .as_ref()
        .filter(|etag| !etag.weak)
        .map(|etag| format!("\"{}\"", etag.opaque))
        .or_else(|| validators.last_modified.clone())
}

// Remote validators can be sensitive or reflect request data. Never Debug raw values.
pub(crate) fn if_range_header(
    validators: &Validators,
) -> Result<Option<HeaderValue>, RangeValidationError> {
    if_range_value(validators)
        .map(|value| {
            let mut header =
                HeaderValue::from_str(&value).map_err(|_| RangeValidationError::InvalidHeader)?;
            header.set_sensitive(true);
            Ok(header)
        })
        .transpose()
}

pub(crate) fn validate_expected_validators(
    expected: &Validators,
    actual: &Validators,
) -> Result<(), RangeValidationError> {
    if let Some(expected_etag) = expected.etag.as_ref() {
        match actual.etag.as_ref() {
            Some(actual_etag) if actual_etag == expected_etag => {}
            Some(_) => return Err(RangeValidationError::ValidatorChanged),
            None => return Err(RangeValidationError::ValidatorMissing),
        }
    }
    if let Some(expected_date) = expected.last_modified.as_ref() {
        match actual.last_modified.as_ref() {
            Some(actual_date) if actual_date == expected_date => {}
            Some(_) => return Err(RangeValidationError::ValidatorChanged),
            None => return Err(RangeValidationError::ValidatorMissing),
        }
    }
    Ok(())
}

async fn read_exact_body(mut response: AdmittedResponse, expected: u64) -> Result<(), ProbeError> {
    let mut received = 0_u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ProbeError::BodyLengthMismatch)?
    {
        received = received
            .checked_add(u64::try_from(chunk.len()).map_err(|_| ProbeError::BodyLengthMismatch)?)
            .ok_or(ProbeError::BodyLengthMismatch)?;
        if received > expected {
            return Err(ProbeError::BodyLengthMismatch);
        }
    }
    if received != expected {
        return Err(ProbeError::BodyLengthMismatch);
    }
    Ok(())
}

pub(crate) fn retry_after_seconds(headers: &HeaderMap) -> Option<u64> {
    retry_after_at(headers, SystemTime::now())
}

fn retry_after_at(headers: &HeaderMap, now: SystemTime) -> Option<u64> {
    let value = single_header(headers, RETRY_AFTER).ok().flatten()?;
    if let Some(seconds) = parse_u64(value) {
        return Some(seconds);
    }
    let target = httpdate::parse_http_date(value).ok()?;
    let delay = target.duration_since(now).unwrap_or_default();
    Some(
        delay
            .as_secs()
            .saturating_add(u64::from(delay.subsec_nanos() != 0)),
    )
}

fn filename_from_headers_or_url(headers: &HeaderMap, url: &Url) -> Option<String> {
    single_header(headers, CONTENT_DISPOSITION)
        .ok()
        .flatten()
        .and_then(filename_from_content_disposition)
        .or_else(|| {
            url.path_segments()
                .and_then(Iterator::last)
                .and_then(percent_decode)
                .and_then(valid_filename_candidate)
        })
}

fn filename_from_content_disposition(value: &str) -> Option<String> {
    let parameters = split_parameters(value);
    for parameter in parameters.iter().skip(1) {
        let Some((name, raw)) = parameter.split_once('=') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("filename*") {
            let raw = raw.trim();
            let Some((charset, encoded)) = raw.split_once("''") else {
                continue;
            };
            if charset.eq_ignore_ascii_case("utf-8")
                && let Some(candidate) = percent_decode(encoded).and_then(valid_filename_candidate)
            {
                return Some(candidate);
            }
        }
    }
    for parameter in parameters.iter().skip(1) {
        let Some((name, raw)) = parameter.split_once('=') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("filename") {
            return unquote(raw.trim()).and_then(valid_filename_candidate);
        }
    }
    None
}

fn split_parameters(value: &str) -> Vec<&str> {
    let mut parameters = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
        } else if quoted && character == '\\' {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
        } else if character == ';' && !quoted {
            parameters.push(&value[start..index]);
            start = index + 1;
        }
    }
    parameters.push(&value[start..]);
    parameters
}

fn unquote(value: &str) -> Option<String> {
    if !value.starts_with('"') || !value.ends_with('"') || value.len() < 2 {
        return valid_filename_candidate(value.to_owned());
    }
    let mut result = String::new();
    let mut escaped = false;
    for character in value[1..value.len() - 1].chars() {
        if escaped {
            result.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            result.push(character);
        }
    }
    if escaped {
        return None;
    }
    valid_filename_candidate(result)
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1)?;
            let low = *bytes.get(index + 2)?;
            decoded.push(hex(high)?.checked_mul(16)?.checked_add(hex(low)?)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

const fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn valid_filename_candidate(value: String) -> Option<String> {
    if value.is_empty() || value.len() > MAX_FILENAME_BYTES || value.chars().any(char::is_control) {
        return None;
    }
    Some(value)
}

fn origin(url: &Url) -> String {
    url.origin().ascii_serialization()
}

#[cfg(test)]
mod tests {
    use reqwest::header::{CONTENT_DISPOSITION, HeaderMap, HeaderValue};

    use super::{
        ContentRange, EntityTag, Validators, filename_from_content_disposition, if_range_value,
        parse_content_range, parse_etag, percent_decode,
    };

    #[test]
    fn remote_validator_debug_and_outgoing_header_are_redacted() {
        let validators = Validators {
            etag: Some(EntityTag::parse("\"synthetic-reflected-secret\"").expect("ETag")),
            last_modified: None,
        };
        let header = super::if_range_header(&validators)
            .expect("header")
            .expect("value");
        assert_eq!(header, "\"synthetic-reflected-secret\"");
        assert!(header.is_sensitive());
        assert!(!format!("{validators:?} {header:?}").contains("synthetic-reflected-secret"));
    }

    #[test]
    fn parses_strict_content_range() {
        assert_eq!(
            parse_content_range("bytes 10-19/100"),
            Ok(ContentRange {
                start: 10,
                end: 19,
                total: 100,
            })
        );
        for invalid in [
            "bytes */100",
            "bytes 19-10/100",
            "bytes 10-100/100",
            "bytes 10-19/*",
            "bytes 10-19/100/2",
            "items 10-19/100",
            "bytes +10-19/100",
        ] {
            assert!(parse_content_range(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn if_range_prefers_only_strong_entity_tags() {
        let date = "Mon, 01 Jan 2024 00:00:00 GMT".to_owned();
        let strong = Validators {
            etag: Some(parse_etag("\"strong\"").expect("strong tag")),
            last_modified: Some(date.clone()),
        };
        assert_eq!(if_range_value(&strong).as_deref(), Some("\"strong\""));

        let weak = Validators {
            etag: Some(parse_etag("W/\"weak\"").expect("weak tag")),
            last_modified: Some(date.clone()),
        };
        assert_eq!(if_range_value(&weak), Some(date));
        assert_eq!(
            if_range_value(&Validators {
                etag: Some(parse_etag("W/\"weak\"").expect("weak tag")),
                last_modified: None,
            }),
            None
        );
    }

    #[test]
    fn parses_entity_tags_without_normalizing_identity() {
        assert_eq!(
            parse_etag("\"opaque-value\""),
            Ok(EntityTag {
                weak: false,
                opaque: "opaque-value".to_owned(),
            })
        );
        assert!(parse_etag("W/\"weak\"").is_ok());
        assert!(parse_etag("unquoted").is_err());
        assert!(parse_etag("\"contains space\"").is_err());
    }

    #[test]
    fn prefers_utf8_content_disposition_filename() {
        let value = "attachment; filename=plain.bin; filename*=UTF-8''report%20final.bin";
        assert_eq!(
            filename_from_content_disposition(value).as_deref(),
            Some("report final.bin")
        );
        assert_eq!(
            filename_from_content_disposition("attachment; filename=\"semi;colon.bin\"").as_deref(),
            Some("semi;colon.bin")
        );
        assert_eq!(percent_decode("bad%2"), None);
        assert_eq!(
            filename_from_content_disposition("attachment; filename=\"\""),
            None
        );
        assert_eq!(
            filename_from_content_disposition("attachment; odd; filename=fallback.bin").as_deref(),
            Some("fallback.bin")
        );
    }

    #[test]
    fn retry_after_http_date_rounds_up_instead_of_retrying_before_the_deadline() {
        let mut headers = HeaderMap::new();
        let target = std::time::UNIX_EPOCH + std::time::Duration::from_secs(2);
        headers.insert(
            reqwest::header::RETRY_AFTER,
            HeaderValue::from_str(&httpdate::fmt_http_date(target)).expect("date"),
        );
        for (millis, expected) in [(1000, 1), (1500, 1), (1999, 1), (2000, 0), (3000, 0)] {
            assert_eq!(
                super::retry_after_at(
                    &headers,
                    std::time::UNIX_EPOCH + std::time::Duration::from_millis(millis)
                ),
                Some(expected)
            );
        }
    }

    #[test]
    fn content_disposition_header_remains_bounded() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=fixture.bin"),
        );
        assert_eq!(
            super::filename_from_headers_or_url(
                &headers,
                &reqwest::Url::parse("https://example.test/fallback.bin").expect("valid test URL")
            )
            .as_deref(),
            Some("fixture.bin")
        );
    }
}
