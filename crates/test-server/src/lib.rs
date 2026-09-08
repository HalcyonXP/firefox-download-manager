//! Deterministic, local-only HTTP fixtures for downloader integration tests.
//!
//! The server intentionally implements only the bounded HTTP/1.1 surface needed
//! by this project. It must never be included in production packages.

mod observation;
use observation::ObservationGate;
pub use observation::ObservationPause;
mod response;
use response::ResponseGate;
pub use response::ResponsePause;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MAX_REQUEST_HEADER_BYTES: usize = 16 * 1024;
const BODY_CHUNK_BYTES: usize = 16 * 1024;
const DEFAULT_STALL: Duration = Duration::from_millis(100);

/// An inclusive HTTP byte range.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ByteRange {
    /// First requested byte.
    pub start: u64,
    /// Last requested byte, inclusive.
    pub end: u64,
}

impl ByteRange {
    /// Constructs a valid inclusive range.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::InvalidInput`] when `start` is greater than
    /// `end`.
    pub fn new(start: u64, end: u64) -> io::Result<Self> {
        if start > end {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "range start exceeds range end",
            ));
        }
        Ok(Self { start, end })
    }

    fn len(self) -> u64 {
        self.end - self.start + 1
    }
}

/// Deterministic resource description.
#[derive(Clone, Debug)]
pub struct Fixture {
    /// Resource length in bytes.
    pub len: u64,
    /// Small seed used by the public byte-generation formula.
    pub seed: u8,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            len: 64 * 1024,
            seed: 73,
        }
    }
}

impl Fixture {
    /// Returns one reproducible byte for an offset and resource generation.
    ///
    /// Independent implementations can use this formula:
    /// `(offset * 31 + (offset >> 8) * 17 + seed + generation * 13) % 251`.
    #[must_use]
    pub fn byte_at(&self, offset: u64, generation: u64) -> u8 {
        let value = offset
            .wrapping_mul(31)
            .wrapping_add((offset >> 8).wrapping_mul(17))
            .wrapping_add(u64::from(self.seed))
            .wrapping_add(generation.wrapping_mul(13))
            % 251;
        value.to_le_bytes()[0]
    }

    /// Materializes a bounded portion of the deterministic resource.
    #[must_use]
    pub fn bytes(&self, start: u64, len: usize, generation: u64) -> Vec<u8> {
        let mut offset = start;
        std::iter::repeat_with(|| {
            let byte = self.byte_at(offset, generation);
            offset = offset.wrapping_add(1);
            byte
        })
        .take(len)
        .collect()
    }

    fn etag(&self, generation: u64) -> String {
        format!("\"fixture-{}-{}-g{generation}\"", self.seed, self.len)
    }

    fn last_modified(generation: u64) -> String {
        format!("Mon, 01 Jan 2024 00:00:{:02} GMT", generation % 60)
    }
}

/// Deliberately malformed `Content-Range` variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BadRange {
    /// Advertise a start one byte later than requested.
    Start,
    /// Advertise an end one byte later than requested.
    End,
    /// Advertise a total one byte larger than the fixture.
    Total,
}

/// A deterministic fault applied by a matching rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Fault {
    /// Ignore `Range` and return the complete body with `200`.
    IgnoreRange,
    /// Represent a zero-length resource with an exact `416` range response.
    EmptyResource,
    /// Return a `206` with deliberately incorrect range metadata.
    BadContentRange(BadRange),
    /// Omit `ETag` and `Last-Modified`.
    OmitValidators,
    /// Return only a weak `ETag` plus Last-Modified.
    WeakValidators,
    /// Serve a selected deterministic resource generation and validators.
    Generation(u64),
    /// Change validators while retaining generation-zero body bytes.
    ValidatorGeneration(u64),
    /// Return a redirect to this location.
    Redirect(String),
    /// Return an HTTP status and optional `Retry-After` seconds.
    Status {
        /// HTTP status code.
        code: u16,
        /// Optional retry delay.
        retry_after_seconds: Option<u64>,
    },
    /// Declare the full body length but disconnect after this many bytes.
    DisconnectAfter(usize),
    /// Wait before sending every matching response body.
    Stall(Duration),
    /// Wait only for the first request of each matching path/range pair.
    StallFirst(Duration),
    /// Send a body despite this unexpected content encoding.
    UnexpectedEncoding(String),
    /// Omit both `Content-Length` and transfer encoding and delimit by close.
    UnknownLength,
}

/// Selects requests for a custom fault rule.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RequestSelector {
    /// Optional exact path, excluding the query string.
    pub path: Option<String>,
    /// Optional one-based request number for that path.
    pub request_number: Option<u64>,
    /// Optional exact requested byte range.
    pub range: Option<ByteRange>,
}

impl RequestSelector {
    fn matches(&self, request: &ObservedRequest) -> bool {
        self.path.as_ref().is_none_or(|path| path == &request.path)
            && self
                .request_number
                .is_none_or(|number| number == request.request_number)
            && self.range.is_none_or(|range| Some(range) == request.range)
    }
}

/// A selector and the fault applied to matching requests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaultRule {
    /// Request matching criteria.
    pub selector: RequestSelector,
    /// Fault returned for a match.
    pub fault: Fault,
}

/// Non-sensitive assertions about session headers; raw values are never retained.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // Independent header assertions, not lifecycle flags.
pub struct SessionObservation {
    /// Cookie header was present.
    pub cookie_present: bool,
    /// Referrer header was present.
    pub referrer_present: bool,
    /// Authorization header was present.
    pub authorization_present: bool,
    /// Both session headers matched fixed synthetic fixture values.
    pub fixture_valid: bool,
    /// Signed query retained its exact encoding, order, and duplicated keys.
    pub signed_target_valid: bool,
}

/// One request observed by the fixture server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedRequest {
    /// Only boolean session assertions are exposed by the test ledger.
    pub session: SessionObservation,
    /// Request target path without a query.
    pub path: String,
    /// One-based ordinal among requests for the same path.
    pub request_number: u64,
    /// Parsed single byte range, when present and valid.
    pub range: Option<ByteRange>,
    /// Exact `If-Range` value, when supplied by a worker.
    pub if_range: Option<String>,
}

/// Configuration for a test-server instance.
#[derive(Clone, Debug, Default)]
pub struct ServerConfig {
    /// Deterministic resource shape.
    pub fixture: Fixture,
    /// Custom rules evaluated before built-in route behavior.
    pub rules: Vec<FaultRule>,
}

#[derive(Debug, Default)]
struct SharedState {
    path_counts: HashMap<String, u64>,
    range_counts: HashMap<(String, Option<ByteRange>), u64>,
    requests: Vec<ObservedRequest>,
    active_requests: usize,
    max_active_requests: usize,
}

/// A running server bound exclusively to an ephemeral IPv4 loopback port.
#[derive(Debug)]
pub struct TestServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<SharedState>>,
    listener_thread: Option<JoinHandle<()>>,
    observation: Arc<ObservationGate>,
    response: Arc<ResponseGate>,
}

impl TestServer {
    /// Starts a new local fixture server.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if loopback binding or listener configuration fails,
    /// or invalid input if the fixture is empty.
    pub fn start(config: ServerConfig) -> io::Result<Self> {
        if config.fixture.len == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fixture length must be positive",
            ));
        }

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(SharedState::default()));
        let thread_stop = Arc::clone(&stop);
        let thread_state = Arc::clone(&state);
        let observation = Arc::new(ObservationGate::default());
        let thread_observation = Arc::clone(&observation);
        let response = Arc::new(ResponseGate::default());
        let thread_response = Arc::clone(&response);

        let listener_thread = thread::Builder::new()
            .name("adversarial-http-listener".to_owned())
            .spawn(move || {
                accept_loop(
                    &listener,
                    &config,
                    &thread_stop,
                    &thread_state,
                    &thread_observation,
                    &thread_response,
                );
            })?;

        Ok(Self {
            address,
            stop,
            state,
            listener_thread: Some(listener_thread),
            observation,
            response,
        })
    }

    /// Returns the loopback socket address.
    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    /// Creates an HTTP URL for a root-relative path.
    ///
    /// # Panics
    ///
    /// Panics when `path` is not root-relative; test code should treat that as a
    /// programming error.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        assert!(path.starts_with('/'), "fixture path must start with '/'");
        format!("http://{}{path}", self.address)
    }

    /// Returns a point-in-time copy of observed request metadata.
    #[must_use]
    pub fn requests(&self) -> Vec<ObservedRequest> {
        lock_state(&self.state).requests.clone()
    }

    /// Pauses ledger observation after complete HTTP headers have arrived.
    /// The server and guard both release waiters on drop.
    #[must_use]
    pub fn pause_observation(&self) -> ObservationPause {
        self.observation.pause()
    }

    /// Pauses matching responses after ledger insertion, before headers/body.
    /// Unmatched requests proceed. Drop the guard to release selected responses.
    ///
    /// # Errors
    ///
    /// Returns `WouldBlock` if an earlier pause or its waiters are still active.
    pub fn pause_responses(&self, selector: RequestSelector) -> io::Result<ResponsePause> {
        self.response.pause(selector)
    }

    /// Highest number of response handlers active at the same time.
    #[must_use]
    pub fn max_concurrent_requests(&self) -> usize {
        lock_state(&self.state).max_active_requests
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.observation.release();
        self.response.release();
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_millis(100));
        if let Some(handle) = self.listener_thread.take() {
            let _ = handle.join();
        }
    }
}

struct ActiveRequestGuard<'a> {
    state: &'a Mutex<SharedState>,
}

impl Drop for ActiveRequestGuard<'_> {
    fn drop(&mut self) {
        let mut shared = lock_state(self.state);
        shared.active_requests = shared.active_requests.saturating_sub(1);
    }
}

fn lock_state(state: &Mutex<SharedState>) -> std::sync::MutexGuard<'_, SharedState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn accept_loop(
    listener: &TcpListener,
    config: &ServerConfig,
    stop: &AtomicBool,
    state: &Arc<Mutex<SharedState>>,
    observation: &Arc<ObservationGate>,
    response: &Arc<ResponseGate>,
) {
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, peer)) => {
                if peer.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
                    continue;
                }
                let connection_config = (*config).clone();
                let connection_state = Arc::clone(state);
                let connection_observation = Arc::clone(observation);
                let connection_response = Arc::clone(response);
                let _ = thread::Builder::new()
                    .name("adversarial-http-connection".to_owned())
                    .spawn(move || {
                        let _ = handle_connection(
                            stream,
                            &connection_config,
                            &connection_state,
                            &connection_observation,
                            &connection_response,
                        );
                    });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(_) => break,
        }
    }
}

#[derive(Debug)]
struct Request {
    session: SessionObservation,
    method: String,
    path: String,
    range: Option<ByteRange>,
    if_range: Option<String>,
    malformed_range: bool,
}

fn handle_connection(
    mut stream: TcpStream,
    config: &ServerConfig,
    state: &Mutex<SharedState>,
    observation: &ObservationGate,
    response: &ResponseGate,
) -> io::Result<()> {
    // Windows can inherit nonblocking mode from the listener. Connection
    // handlers need ordinary blocking semantics so transient WouldBlock errors
    // cannot truncate a deterministic fixture response.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.set_nodelay(true)?;

    let Some(request) = read_request(&mut stream)? else {
        return Ok(());
    };
    if request.method != "GET" && request.method != "HEAD" {
        return write_empty_status(&mut stream, 405, &[("Allow", "GET, HEAD".to_owned())]);
    }
    if request.malformed_range {
        return write_empty_status(&mut stream, 400, &[]);
    }

    observation.arrive();
    let (observed, range_request_number) = {
        let mut shared = lock_state(state);
        let count = shared.path_counts.entry(request.path.clone()).or_default();
        *count = count.saturating_add(1);
        let request_number = *count;
        let range_count = shared
            .range_counts
            .entry((request.path.clone(), request.range))
            .or_default();
        *range_count = range_count.saturating_add(1);
        let range_request_number = *range_count;
        let observed = ObservedRequest {
            session: request.session.clone(),
            path: request.path.clone(),
            request_number,
            range: request.range,
            if_range: request.if_range.clone(),
        };
        shared.requests.push(observed.clone());
        shared.active_requests = shared.active_requests.saturating_add(1);
        shared.max_active_requests = shared.max_active_requests.max(shared.active_requests);
        (observed, range_request_number)
    };
    let _activity = ActiveRequestGuard { state };
    response.arrive(&observed);

    if request.path == "/session/fixture" && !request.session.fixture_valid {
        return write_empty_status(&mut stream, 401, &[]);
    }
    if request.path == "/session/signed"
        && (!request.session.fixture_valid || !request.session.signed_target_valid)
    {
        return write_empty_status(&mut stream, 403, &[]);
    }
    let custom_fault = config
        .rules
        .iter()
        .find(|rule| {
            rule.selector.matches(&observed)
                && (!matches!(&rule.fault, Fault::StallFirst(_)) || range_request_number == 1)
        })
        .map(|rule| &rule.fault);
    let built_in = if custom_fault.is_none() {
        built_in_fault(&observed)
    } else {
        None
    };

    serve_fixture(
        &mut stream,
        &request,
        config,
        custom_fault.or(built_in.as_ref()),
    )
}

fn read_request(stream: &mut TcpStream) -> io::Result<Option<Request>> {
    let mut bytes = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Ok(None);
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if bytes.len() > MAX_REQUEST_HEADER_BYTES {
            write_empty_status(stream, 431, &[])?;
            return Ok(None);
        }
    }

    let text = std::str::from_utf8(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "request is not UTF-8"))?;
    let mut lines = text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing request line"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if parts.next().is_some() || target.is_empty() || !version.starts_with("HTTP/1.") {
        write_empty_status(stream, 400, &[])?;
        return Ok(None);
    }

    let path = target.split('?').next().unwrap_or(target).to_owned();
    let mut range = None;
    let mut if_range = None;
    let mut malformed_range = false;
    let mut session = SessionObservation {
        signed_target_valid: target.ends_with("?sig=a%2Fb%2BC&x=2&x=1"),
        ..SessionObservation::default()
    };
    let mut cookie_valid = false;
    let mut referrer_valid = false;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            malformed_range = true;
            continue;
        };
        if name.eq_ignore_ascii_case("cookie") {
            cookie_valid =
                !session.cookie_present && value.trim() == "fixture_session=not-a-real-session";
            session.cookie_present = true;
        } else if name.eq_ignore_ascii_case("referer") {
            referrer_valid = !session.referrer_present
                && value.trim() == format!("http://{}/session/page", stream.local_addr()?);
            session.referrer_present = true;
        } else if name.eq_ignore_ascii_case("authorization") {
            session.authorization_present = true;
        } else if name.eq_ignore_ascii_case("range") {
            match parse_range(value.trim()) {
                Some(parsed) if range.is_none() => range = Some(parsed),
                _ => malformed_range = true,
            }
        } else if name.eq_ignore_ascii_case("if-range") {
            let value = value.trim();
            if if_range.is_none() && !value.is_empty() && value.len() <= 8 * 1024 {
                if_range = Some(value.to_owned());
            } else {
                malformed_range = true;
            }
        }
    }

    session.fixture_valid = cookie_valid && referrer_valid;
    Ok(Some(Request {
        session,
        method,
        path,
        range,
        if_range,
        malformed_range,
    }))
}

fn parse_range(value: &str) -> Option<ByteRange> {
    let spec = value.strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (start, end) = spec.split_once('-')?;
    if start.is_empty() || end.is_empty() {
        return None;
    }
    ByteRange::new(start.parse().ok()?, end.parse().ok()?).ok()
}

fn built_in_fault(request: &ObservedRequest) -> Option<Fault> {
    match request.path.as_str() {
        "/fixture" | "/session/fixture" | "/session/signed" => None,
        "/ignore-range" => Some(Fault::IgnoreRange),
        "/empty" => Some(Fault::EmptyResource),
        "/bad-range/start" => Some(Fault::BadContentRange(BadRange::Start)),
        "/bad-range/end" => Some(Fault::BadContentRange(BadRange::End)),
        "/bad-range/total" => Some(Fault::BadContentRange(BadRange::Total)),
        "/validators/missing" => Some(Fault::OmitValidators),
        "/validators/weak" => Some(Fault::WeakValidators),
        "/validators/changing" => Some(Fault::ValidatorGeneration(
            request.request_number.saturating_sub(1),
        )),
        "/mutating" => Some(Fault::Generation(request.request_number.saturating_sub(1))),
        "/redirect/once" => Some(Fault::Redirect("/fixture".to_owned())),
        "/redirect/loop-a" => Some(Fault::Redirect("/redirect/loop-b".to_owned())),
        "/redirect/loop-b" => Some(Fault::Redirect("/redirect/loop-a".to_owned())),
        "/disconnect" => Some(Fault::DisconnectAfter(17)),
        "/stall" => Some(Fault::Stall(DEFAULT_STALL)),
        "/status/403" => Some(Fault::Status {
            code: 403,
            retry_after_seconds: None,
        }),
        "/status/416" => Some(Fault::Status {
            code: 416,
            retry_after_seconds: None,
        }),
        "/status/429" => Some(Fault::Status {
            code: 429,
            retry_after_seconds: Some(2),
        }),
        "/status/503" => Some(Fault::Status {
            code: 503,
            retry_after_seconds: Some(1),
        }),
        "/unknown-length" => Some(Fault::UnknownLength),
        "/encoded" => Some(Fault::UnexpectedEncoding("gzip".to_owned())),
        _ => Some(Fault::Status {
            code: 404,
            retry_after_seconds: None,
        }),
    }
}

#[allow(clippy::too_many_lines)]
fn serve_fixture(
    stream: &mut TcpStream,
    request: &Request,
    config: &ServerConfig,
    fault: Option<&Fault>,
) -> io::Result<()> {
    if matches!(fault, Some(Fault::EmptyResource)) {
        if request.range.is_some() {
            return write_empty_status(stream, 416, &[("Content-Range", "bytes */0".to_owned())]);
        }
        return write_empty_status(stream, 200, &[]);
    }
    if let Some(Fault::Redirect(location)) = fault {
        if location.contains(['\r', '\n']) {
            return write_empty_status(stream, 500, &[]);
        }
        return write_empty_status(stream, 302, &[("Location", location.clone())]);
    }
    if let Some(Fault::Status {
        code,
        retry_after_seconds,
    }) = fault
    {
        let mut headers = Vec::new();
        if let Some(seconds) = retry_after_seconds {
            headers.push(("Retry-After", seconds.to_string()));
        }
        if *code == 416 {
            headers.push(("Content-Range", format!("bytes */{}", config.fixture.len)));
        }
        return write_empty_status(stream, *code, &headers);
    }

    let body_generation = match fault {
        Some(Fault::Generation(value)) => *value,
        _ => 0,
    };
    let validator_generation = match fault {
        Some(Fault::Generation(value) | Fault::ValidatorGeneration(value)) => *value,
        _ => 0,
    };
    let mut selected_range = request.range;
    let ignore_range = matches!(fault, Some(Fault::IgnoreRange | Fault::UnknownLength));
    if ignore_range {
        selected_range = None;
    }

    if let Some(range) = selected_range
        && (range.start >= config.fixture.len || range.end >= config.fixture.len)
    {
        return write_empty_status(
            stream,
            416,
            &[("Content-Range", format!("bytes */{}", config.fixture.len))],
        );
    }

    let body_range = selected_range.unwrap_or(ByteRange {
        start: 0,
        end: config.fixture.len - 1,
    });
    let status = if selected_range.is_some() { 206 } else { 200 };
    let mut headers = vec![
        ("Content-Type", "application/octet-stream".to_owned()),
        ("Accept-Ranges", "bytes".to_owned()),
    ];

    if !matches!(fault, Some(Fault::OmitValidators)) {
        let etag = config.fixture.etag(validator_generation);
        headers.push((
            "ETag",
            if matches!(fault, Some(Fault::WeakValidators)) {
                format!("W/{etag}")
            } else {
                etag
            },
        ));
        headers.push((
            "Last-Modified",
            Fixture::last_modified(validator_generation),
        ));
    }

    if let Some(range) = selected_range {
        let mut advertised = range;
        let mut total = config.fixture.len;
        match fault {
            Some(Fault::BadContentRange(BadRange::Start)) => {
                advertised.start = advertised.start.saturating_add(1);
            }
            Some(Fault::BadContentRange(BadRange::End)) => {
                advertised.end = advertised.end.saturating_add(1);
            }
            Some(Fault::BadContentRange(BadRange::Total)) => {
                total = total.saturating_add(1);
            }
            _ => {}
        }
        headers.push((
            "Content-Range",
            format!("bytes {}-{}/{total}", advertised.start, advertised.end),
        ));
    }

    let unknown_length = matches!(fault, Some(Fault::UnknownLength));
    if !unknown_length {
        headers.push(("Content-Length", body_range.len().to_string()));
    }
    if let Some(Fault::UnexpectedEncoding(encoding)) = fault
        && !encoding.contains(['\r', '\n'])
    {
        headers.push(("Content-Encoding", encoding.clone()));
    }

    write_head(stream, status, &headers)?;
    if request.method == "HEAD" {
        return finish_response(stream);
    }
    if let Some(Fault::Stall(duration) | Fault::StallFirst(duration)) = fault {
        thread::sleep(*duration);
    }

    let maximum = match fault {
        Some(Fault::DisconnectAfter(limit)) => Some(*limit),
        _ => None,
    };
    write_generated_body(
        stream,
        &config.fixture,
        body_range,
        body_generation,
        maximum,
    )
}

fn write_generated_body(
    stream: &mut TcpStream,
    fixture: &Fixture,
    range: ByteRange,
    generation: u64,
    maximum: Option<usize>,
) -> io::Result<()> {
    let allowed = maximum.map_or(range.len(), |limit| {
        range.len().min(u64::try_from(limit).unwrap_or(u64::MAX))
    });
    let mut written = 0_u64;
    while written < allowed {
        let remaining = allowed - written;
        let count_u64 = remaining.min(BODY_CHUNK_BYTES as u64);
        let count = usize::try_from(count_u64)
            .map_err(|_| io::Error::other("body chunk does not fit usize"))?;
        let bytes = fixture.bytes(range.start + written, count, generation);
        stream.write_all(&bytes)?;
        written += count_u64;
    }
    finish_response(stream)
}

fn finish_response(stream: &mut TcpStream) -> io::Result<()> {
    stream.flush()?;
    stream.shutdown(Shutdown::Write)
}

fn write_empty_status(
    stream: &mut TcpStream,
    status: u16,
    extra_headers: &[(&str, String)],
) -> io::Result<()> {
    let mut headers = extra_headers.to_vec();
    headers.push(("Content-Length", "0".to_owned()));
    write_head(stream, status, &headers)?;
    finish_response(stream)
}

fn write_head(stream: &mut TcpStream, status: u16, headers: &[(&str, String)]) -> io::Result<()> {
    let mut response = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    for (name, value) in headers {
        if name.contains(['\r', '\n']) || value.contains(['\r', '\n']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "response header contains a line break",
            ));
        }
        write!(response, "{name}: {value}\r\n")
            .map_err(|_| io::Error::other("formatting response failed"))?;
    }
    response.push_str("Connection: close\r\n\r\n");
    stream.write_all(response.as_bytes())
}

const fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        302 => "Found",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Fixture Status",
    }
}
