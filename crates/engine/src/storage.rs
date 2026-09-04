//! Collision-safe partial-file storage with assignment-bounded random access.
//!
//! Ranges are half-open (`start..end`). A [`SegmentWriter`] owns one disjoint
//! assignment and advances sequentially within it. Only fully written
//! assignments become completed coverage.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

const MAX_FILENAME_UTF16_UNITS: usize = 180;
const PART_CREATE_ATTEMPTS: u64 = 64;
const FINAL_NAME_ATTEMPTS: u32 = 10_000;
static NEXT_PART_ID: AtomicU64 = AtomicU64::new(1);

/// A validated half-open byte range in a partial file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileRange {
    start: u64,
    end: u64,
}

impl FileRange {
    /// Creates a non-empty half-open range.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidRange`] when `start >= end`.
    pub const fn new(start: u64, end: u64) -> Result<Self, StorageError> {
        if start >= end {
            return Err(StorageError::InvalidRange { start, end });
        }
        Ok(Self { start, end })
    }

    /// First byte in the range.
    #[must_use]
    pub const fn start(self) -> u64 {
        self.start
    }

    /// Exclusive upper bound.
    #[must_use]
    pub const fn end(self) -> u64 {
        self.end
    }

    /// Number of bytes in the range.
    #[must_use]
    pub const fn len(self) -> u64 {
        self.end - self.start
    }

    /// A validated range is never empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        false
    }

    const fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

impl fmt::Display for FileRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "[{}, {})", self.start, self.end)
    }
}

/// A filename that contains one bounded, Windows-safe path component.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SanitizedFilename(String);

impl fmt::Debug for SanitizedFilename {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SanitizedFilename(<redacted>)")
    }
}

impl SanitizedFilename {
    /// Returns the sanitized component as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SanitizedFilename {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Converts untrusted filename metadata into one safe Windows path component.
///
/// Separators, alternate-data-stream colons, wildcards, control characters,
/// and other Win32-invalid characters become underscores. Trailing dots and
/// spaces are removed, reserved device names are prefixed, and the result is
/// bounded by UTF-16 code units. Empty results become `download`.
#[must_use]
pub fn sanitize_filename(input: &str) -> SanitizedFilename {
    let mut output = String::new();
    let mut utf16_units = 0usize;
    let mut previous_was_replacement = false;

    for character in input.chars() {
        let replacement = character.is_control() || is_windows_invalid(character);
        let candidate = if replacement { '_' } else { character };
        if replacement && previous_was_replacement {
            continue;
        }
        let units = candidate.len_utf16();
        if utf16_units + units > MAX_FILENAME_UTF16_UNITS {
            break;
        }
        output.push(candidate);
        utf16_units += units;
        previous_was_replacement = replacement;
    }

    output = output
        .trim_matches(' ')
        .trim_end_matches([' ', '.'])
        .to_owned();
    if output.is_empty() || output == "." || output == ".." {
        "download".clone_into(&mut output);
    }

    if is_windows_reserved(&output) {
        output = format!("_{}", truncate_utf16(&output, MAX_FILENAME_UTF16_UNITS - 1));
    }
    output = truncate_utf16(&output, MAX_FILENAME_UTF16_UNITS);
    output = output.trim_end_matches([' ', '.']).to_owned();
    if output.is_empty() {
        "download".clone_into(&mut output);
    }

    SanitizedFilename(output)
}

/// Filesystem operation associated with a structured I/O failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageOperation {
    /// Inspect or canonicalize the destination directory.
    InspectDestination,
    /// Create a collision-safe partial file.
    CreatePartial,
    /// Establish the partial file's expected logical length.
    Preallocate,
    /// Write assigned bytes.
    Write,
    /// Read file metadata or flush file contents.
    Flush,
    /// Atomically publish a completed file.
    Publish,
}

impl fmt::Display for StorageOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::InspectDestination => "inspect destination",
            Self::CreatePartial => "create partial file",
            Self::Preallocate => "preallocate partial file",
            Self::Write => "write partial file",
            Self::Flush => "flush partial file",
            Self::Publish => "publish final file",
        };
        formatter.write_str(text)
    }
}

/// Stable, path-free classification of a filesystem failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoFailure {
    /// The target volume has no usable capacity.
    DiskFull,
    /// Access control rejected the operation.
    AccessDenied,
    /// Another process holds an incompatible sharing or byte-range lock.
    FileLocked,
    /// A create-new name already exists.
    AlreadyExists,
    /// A required path no longer exists.
    NotFound,
    /// The filesystem cannot provide the required primitive.
    Unsupported,
    /// An I/O failure without a more specific stable classification.
    Other,
}

impl fmt::Display for IoFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::DiskFull => "disk full",
            Self::AccessDenied => "access denied",
            Self::FileLocked => "file locked",
            Self::AlreadyExists => "name already exists",
            Self::NotFound => "path not found",
            Self::Unsupported => "operation unsupported by filesystem",
            Self::Other => "filesystem I/O error",
        };
        formatter.write_str(text)
    }
}

/// Safe storage-layer failures. Display text intentionally contains no path.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StorageError {
    /// A half-open range was empty or reversed.
    #[error("invalid byte range [{start}, {end})")]
    InvalidRange {
        /// Requested start.
        start: u64,
        /// Requested exclusive end.
        end: u64,
    },
    /// An assignment exceeds the expected file length.
    #[error("assignment {range} exceeds expected file length {expected_len}")]
    RangeOutOfBounds {
        /// Rejected range.
        range: FileRange,
        /// Expected complete file length.
        expected_len: u64,
    },
    /// An assignment intersects active or completed ownership.
    #[error("assignment {range} overlaps existing ownership")]
    OverlappingAssignment {
        /// Rejected range.
        range: FileRange,
    },
    /// An assignment identifier exhausted its bounded counter.
    #[error("assignment identifier space exhausted")]
    AssignmentIdExhausted,
    /// A writer was used after completion or after its assignment disappeared.
    #[error("segment assignment is no longer active")]
    AssignmentClosed,
    /// A write would exceed the writer's assignment.
    #[error("write of {bytes} bytes at offset {offset} exceeds assignment {range}")]
    WriteOutOfBounds {
        /// Assignment range.
        range: FileRange,
        /// Attempted write offset.
        offset: u64,
        /// Attempted byte count.
        bytes: u64,
    },
    /// Completion was requested before all assigned bytes were written.
    #[error("segment is short: wrote {written} of {expected} assigned bytes")]
    IncompleteAssignment {
        /// Required bytes.
        expected: u64,
        /// Successfully written bytes.
        written: u64,
    },
    /// The file cannot be published while assignments are active.
    #[error("cannot publish while {count} segment assignments are active")]
    ActiveAssignments {
        /// Number of active assignments.
        count: usize,
    },
    /// Completed ranges do not cover the expected file exactly.
    #[error("completed ranges do not provide exact file coverage")]
    IncompleteCoverage,
    /// The partial file's logical length changed unexpectedly.
    #[error("partial file length changed: expected {expected}, found {actual}")]
    FileLengthChanged {
        /// Expected size.
        expected: u64,
        /// Observed size.
        actual: u64,
    },
    /// The destination path is not an ordinary directory.
    #[error("destination must be an ordinary directory")]
    InvalidDestination,
    /// No bounded create-new partial name remained available.
    #[error("could not allocate a unique partial filename")]
    PartialNameExhausted,
    /// No bounded final-name candidate remained available.
    #[error("could not allocate a non-existing final filename")]
    FinalNameExhausted,
    /// The storage object has already begun or completed publication.
    #[error("partial file is not active")]
    NotActive,
    /// A path-free, classified operating-system failure.
    #[error("{operation} failed: {failure}")]
    Io {
        /// Failed operation.
        operation: StorageOperation,
        /// Stable classification.
        failure: IoFailure,
        /// Optional operating-system error code for local diagnosis.
        os_code: Option<i32>,
    },
}

/// Successful final publication information.
#[derive(Clone, PartialEq, Eq)]
pub struct Promotion {
    final_path: PathBuf,
    partial_cleanup_failure: Option<IoFailure>,
}

impl fmt::Debug for Promotion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Promotion")
            .field("final_path", &"<redacted>")
            .field("partial_cleanup_failure", &self.partial_cleanup_failure)
            .finish()
    }
}

impl Promotion {
    /// Collision-safe final path selected during atomic publication.
    #[must_use]
    pub fn final_path(&self) -> &Path {
        &self.final_path
    }

    /// A non-fatal cleanup failure after the final name became visible.
    ///
    /// The final file is complete when this is present; recovery may remove the
    /// now-redundant `.part` hard link later.
    #[must_use]
    pub const fn partial_cleanup_failure(&self) -> Option<IoFailure> {
        self.partial_cleanup_failure
    }
}

/// An active preallocated partial file.
pub struct PartialFile {
    inner: Arc<Inner>,
}

impl fmt::Debug for PartialFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PartialFile")
            .field("paths", &"<redacted>")
            .field("expected_len", &self.inner.expected_len)
            .finish_non_exhaustive()
    }
}

struct Inner {
    partial_path: PathBuf,
    destination: PathBuf,
    final_name: SanitizedFilename,
    expected_len: u64,
    file: Mutex<Option<File>>,
    state: Mutex<WriteState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    Active,
    Publishing,
    Published,
}

#[derive(Debug)]
struct WriteState {
    lifecycle: Lifecycle,
    next_assignment_id: u64,
    active: BTreeMap<u64, ActiveAssignment>,
    completed: Vec<FileRange>,
}

#[derive(Debug, Clone, Copy)]
struct ActiveAssignment {
    range: FileRange,
    next_offset: u64,
}

impl PartialFile {
    /// Creates a unique `.part` file in `destination` and sets its logical
    /// length to `expected_len` before any segment can be assigned.
    ///
    /// # Errors
    ///
    /// Returns a classified, path-free error if the destination is invalid,
    /// create-new allocation fails, or preallocation/flush fails.
    pub fn create(
        destination: &Path,
        suggested_filename: &str,
        expected_len: u64,
    ) -> Result<Self, StorageError> {
        let metadata = fs::symlink_metadata(destination)
            .map_err(|error| map_io(StorageOperation::InspectDestination, &error))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
            return Err(StorageError::InvalidDestination);
        }
        let destination = fs::canonicalize(destination)
            .map_err(|error| map_io(StorageOperation::InspectDestination, &error))?;
        let final_name = sanitize_filename(suggested_filename);
        let nonce = part_nonce();

        for attempt in 0..PART_CREATE_ATTEMPTS {
            let partial_name = part_filename(final_name.as_str(), nonce, attempt);
            let partial_path = destination.join(partial_name);
            let file = match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&partial_path)
            {
                Ok(file) => file,
                Err(error) if is_already_exists(&error) => continue,
                Err(error) => return Err(map_io(StorageOperation::CreatePartial, &error)),
            };

            if let Err(error) = file.set_len(expected_len) {
                drop(file);
                let _ = fs::remove_file(&partial_path);
                return Err(map_io(StorageOperation::Preallocate, &error));
            }
            if let Err(error) = file.sync_all() {
                drop(file);
                let _ = fs::remove_file(&partial_path);
                return Err(map_io(StorageOperation::Flush, &error));
            }

            return Ok(Self {
                inner: Arc::new(Inner {
                    partial_path,
                    destination,
                    final_name,
                    expected_len,
                    file: Mutex::new(Some(file)),
                    state: Mutex::new(WriteState {
                        lifecycle: Lifecycle::Active,
                        next_assignment_id: 1,
                        active: BTreeMap::new(),
                        completed: Vec::new(),
                    }),
                }),
            });
        }

        Err(StorageError::PartialNameExhausted)
    }

    /// Path of the unique partial file. This is intended for trusted recovery
    /// state, not routine logs.
    #[must_use]
    pub fn partial_path(&self) -> &Path {
        &self.inner.partial_path
    }

    /// Sanitized final filename requested for publication.
    #[must_use]
    pub fn final_name(&self) -> &SanitizedFilename {
        &self.inner.final_name
    }

    /// Expected complete file length.
    #[must_use]
    pub fn expected_len(&self) -> u64 {
        self.inner.expected_len
    }

    /// Returns canonical ordered, non-overlapping completed coverage.
    #[must_use]
    pub fn completed_ranges(&self) -> Vec<FileRange> {
        lock(&self.inner.state).completed.clone()
    }

    /// Reserves a disjoint range and returns its sole writer.
    ///
    /// # Errors
    ///
    /// Rejects out-of-file, overlapping, or post-publication assignments.
    pub fn assign(&self, range: FileRange) -> Result<SegmentWriter, StorageError> {
        if range.end > self.inner.expected_len {
            return Err(StorageError::RangeOutOfBounds {
                range,
                expected_len: self.inner.expected_len,
            });
        }

        let mut state = lock(&self.inner.state);
        if state.lifecycle != Lifecycle::Active {
            return Err(StorageError::NotActive);
        }
        if state.completed.iter().any(|known| range.overlaps(*known))
            || state
                .active
                .values()
                .any(|known| range.overlaps(known.range))
        {
            return Err(StorageError::OverlappingAssignment { range });
        }

        let assignment_id = state.next_assignment_id;
        state.next_assignment_id = state
            .next_assignment_id
            .checked_add(1)
            .ok_or(StorageError::AssignmentIdExhausted)?;
        state.active.insert(
            assignment_id,
            ActiveAssignment {
                range,
                next_offset: range.start,
            },
        );

        Ok(SegmentWriter {
            inner: Arc::clone(&self.inner),
            assignment_id,
            range,
            written: 0,
            finished: false,
        })
    }

    /// Flushes validated complete bytes and atomically publishes a create-new
    /// final directory entry on filesystems that support hard links.
    ///
    /// Existing final files are never replaced: collisions select `name (n)`
    /// candidates. Publication fails closed when exact coverage, file length,
    /// flushing, or atomic create-new linking cannot be proven.
    ///
    /// # Errors
    ///
    /// Returns a structured error when assignments remain active, coverage is
    /// incomplete, file identity changed, flushing fails, or the filesystem
    /// cannot create the final link.
    pub fn promote(&self) -> Result<Promotion, StorageError> {
        let mut state = lock(&self.inner.state);
        if state.lifecycle != Lifecycle::Active {
            return Err(StorageError::NotActive);
        }
        if !state.active.is_empty() {
            return Err(StorageError::ActiveAssignments {
                count: state.active.len(),
            });
        }
        if !has_exact_coverage(&state.completed, self.inner.expected_len) {
            return Err(StorageError::IncompleteCoverage);
        }
        state.lifecycle = Lifecycle::Publishing;

        let publication = self.publish_create_new();
        let final_path = match publication {
            Ok(path) => path,
            Err(error) => {
                state.lifecycle = Lifecycle::Active;
                return Err(error);
            }
        };
        state.lifecycle = Lifecycle::Published;

        let file = lock(&self.inner.file).take();
        drop(file);
        drop(state);
        let cleanup_failure = fs::remove_file(&self.inner.partial_path)
            .err()
            .map(|error| classify_io(&error));

        Ok(Promotion {
            final_path,
            partial_cleanup_failure: cleanup_failure,
        })
    }

    fn publish_create_new(&self) -> Result<PathBuf, StorageError> {
        let file_guard = lock(&self.inner.file);
        let file = file_guard.as_ref().ok_or(StorageError::NotActive)?;
        let actual_len = file
            .metadata()
            .map_err(|error| map_io(StorageOperation::Flush, &error))?
            .len();
        if actual_len != self.inner.expected_len {
            return Err(StorageError::FileLengthChanged {
                expected: self.inner.expected_len,
                actual: actual_len,
            });
        }
        file.sync_all()
            .map_err(|error| map_io(StorageOperation::Flush, &error))?;

        for index in 0..FINAL_NAME_ATTEMPTS {
            let candidate_name = numbered_filename(self.inner.final_name.as_str(), index);
            let candidate_path = self.inner.destination.join(candidate_name);
            match fs::hard_link(&self.inner.partial_path, &candidate_path) {
                Ok(()) => return Ok(candidate_path),
                Err(error) if is_already_exists(&error) => {}
                Err(error) => return Err(map_io(StorageOperation::Publish, &error)),
            }
        }

        Err(StorageError::FinalNameExhausted)
    }
}

/// Sole sequential writer for one disjoint byte assignment.
pub struct SegmentWriter {
    inner: Arc<Inner>,
    assignment_id: u64,
    range: FileRange,
    written: u64,
    finished: bool,
}

impl fmt::Debug for SegmentWriter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SegmentWriter")
            .field("assignment_id", &self.assignment_id)
            .field("range", &self.range)
            .field("written", &self.written)
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}

impl SegmentWriter {
    /// Assigned half-open range.
    #[must_use]
    pub const fn range(&self) -> FileRange {
        self.range
    }

    /// Bytes successfully written by this assignment.
    #[must_use]
    pub const fn written_len(&self) -> u64 {
        self.written
    }

    /// Writes the next sequential chunk at the assignment's current offset.
    ///
    /// # Errors
    ///
    /// Rejects bytes beyond the assignment, closed assignments, and classified
    /// seek/write failures. A failed OS write never advances completed state.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), StorageError> {
        if self.finished {
            return Err(StorageError::AssignmentClosed);
        }
        if bytes.is_empty() {
            return Ok(());
        }
        let byte_count =
            u64::try_from(bytes.len()).map_err(|_| StorageError::WriteOutOfBounds {
                range: self.range,
                offset: self.range.end,
                bytes: u64::MAX,
            })?;

        let mut state = lock(&self.inner.state);
        if state.lifecycle != Lifecycle::Active {
            return Err(StorageError::NotActive);
        }
        let assignment = state
            .active
            .get_mut(&self.assignment_id)
            .ok_or(StorageError::AssignmentClosed)?;
        let offset = assignment.next_offset;
        let write_end = offset
            .checked_add(byte_count)
            .ok_or(StorageError::WriteOutOfBounds {
                range: self.range,
                offset,
                bytes: byte_count,
            })?;
        if write_end > assignment.range.end {
            return Err(StorageError::WriteOutOfBounds {
                range: assignment.range,
                offset,
                bytes: byte_count,
            });
        }

        let mut file_guard = lock(&self.inner.file);
        let file = file_guard.as_mut().ok_or(StorageError::NotActive)?;
        file.seek(SeekFrom::Start(offset))
            .and_then(|_| file.write_all(bytes))
            .map_err(|error| map_io(StorageOperation::Write, &error))?;
        assignment.next_offset = write_end;
        self.written += byte_count;
        Ok(())
    }

    /// Marks the assignment complete only after every assigned byte was
    /// successfully written.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::IncompleteAssignment`] for a short segment.
    pub fn finish(&mut self) -> Result<(), StorageError> {
        if self.finished {
            return Err(StorageError::AssignmentClosed);
        }
        let mut state = lock(&self.inner.state);
        if state.lifecycle != Lifecycle::Active {
            return Err(StorageError::NotActive);
        }
        let assignment = state
            .active
            .get(&self.assignment_id)
            .copied()
            .ok_or(StorageError::AssignmentClosed)?;
        if assignment.next_offset != assignment.range.end {
            return Err(StorageError::IncompleteAssignment {
                expected: assignment.range.len(),
                written: assignment.next_offset - assignment.range.start,
            });
        }

        state.active.remove(&self.assignment_id);
        insert_completed(&mut state.completed, assignment.range);
        self.finished = true;
        Ok(())
    }
}

impl Drop for SegmentWriter {
    fn drop(&mut self) {
        if !self.finished {
            lock(&self.inner.state).active.remove(&self.assignment_id);
        }
    }
}

fn insert_completed(completed: &mut Vec<FileRange>, range: FileRange) {
    completed.push(range);
    completed.sort_unstable();

    let mut merged: Vec<FileRange> = Vec::with_capacity(completed.len());
    for current in completed.drain(..) {
        if let Some(previous) = merged.last_mut()
            && previous.end == current.start
        {
            previous.end = current.end;
        } else {
            merged.push(current);
        }
    }
    *completed = merged;
}

fn has_exact_coverage(completed: &[FileRange], expected_len: u64) -> bool {
    if expected_len == 0 {
        completed.is_empty()
    } else {
        completed
            == [FileRange {
                start: 0,
                end: expected_len,
            }]
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn is_windows_invalid(character: char) -> bool {
    matches!(
        character,
        '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
    )
}

fn is_windows_reserved(filename: &str) -> bool {
    let stem = filename
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }

    let mut characters = stem.chars();
    let prefix: String = characters.by_ref().take(3).collect();
    let suffix: String = characters.collect();
    matches!(prefix.as_str(), "COM" | "LPT")
        && matches!(
            suffix.as_str(),
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
}

fn truncate_utf16(value: &str, maximum_units: usize) -> String {
    let mut units = 0usize;
    value
        .chars()
        .take_while(|character| {
            let next = units + character.len_utf16();
            if next > maximum_units {
                false
            } else {
                units = next;
                true
            }
        })
        .collect()
}

fn part_nonce() -> u128 {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = u128::from(NEXT_PART_ID.fetch_add(1, Ordering::Relaxed));
    (time << 32) ^ (u128::from(std::process::id()) << 16) ^ sequence
}

fn part_filename(final_name: &str, nonce: u128, attempt: u64) -> String {
    let suffix = format!(".dm-{nonce:032x}-{attempt:02x}.part");
    let available = MAX_FILENAME_UTF16_UNITS.saturating_sub(suffix.len());
    format!("{}{}", truncate_utf16(final_name, available), suffix)
}

fn numbered_filename(base: &str, index: u32) -> String {
    if index == 0 {
        return base.to_owned();
    }

    let suffix = format!(" ({index})");
    let (stem, extension) = split_extension(base);
    let extension_units = extension.encode_utf16().count();
    let suffix_units = suffix.len();
    if extension_units + suffix_units + 1 >= MAX_FILENAME_UTF16_UNITS {
        return format!(
            "{}{}",
            truncate_utf16(base, MAX_FILENAME_UTF16_UNITS - suffix_units),
            suffix
        );
    }
    let stem_limit = MAX_FILENAME_UTF16_UNITS - extension_units - suffix_units;
    format!(
        "{}{}{}",
        truncate_utf16(stem, stem_limit),
        suffix,
        extension
    )
}

fn split_extension(filename: &str) -> (&str, &str) {
    let Some(index) = filename.rfind('.') else {
        return (filename, "");
    };
    if index == 0 {
        (filename, "")
    } else {
        (&filename[..index], &filename[index..])
    }
}

fn is_already_exists(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::AlreadyExists
        || cfg!(windows) && matches!(error.raw_os_error(), Some(80 | 183))
}

fn map_io(operation: StorageOperation, error: &io::Error) -> StorageError {
    StorageError::Io {
        operation,
        failure: classify_io(error),
        os_code: error.raw_os_error(),
    }
}

fn classify_io(error: &io::Error) -> IoFailure {
    #[cfg(windows)]
    if let Some(code) = error.raw_os_error() {
        match code {
            32 | 33 => return IoFailure::FileLocked,
            39 | 112 => return IoFailure::DiskFull,
            80 | 183 => return IoFailure::AlreadyExists,
            1 | 17 | 50 => return IoFailure::Unsupported,
            _ => {}
        }
    }

    match error.kind() {
        io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => IoFailure::DiskFull,
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => {
            IoFailure::AccessDenied
        }
        io::ErrorKind::AlreadyExists => IoFailure::AlreadyExists,
        io::ErrorKind::NotFound => IoFailure::NotFound,
        io::ErrorKind::Unsupported | io::ErrorKind::CrossesDevices => IoFailure::Unsupported,
        _ => IoFailure::Other,
    }
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
const fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{
        IoFailure, MAX_FILENAME_UTF16_UNITS, classify_io, numbered_filename, sanitize_filename,
    };
    use std::io;

    #[test]
    fn sanitizer_blocks_windows_paths_devices_and_trailing_aliases() {
        let cases = [
            (r"..\..\secret.txt", ".._.._secret.txt"),
            ("../../secret.txt", ".._.._secret.txt"),
            (r"C:\temp\payload.exe", "C_temp_payload.exe"),
            (r"name:stream", "name_stream"),
            ("CON.txt", "_CON.txt"),
            ("com1", "_com1"),
            ("LPT².log", "_LPT².log"),
            ("report. ", "report"),
            ("...", "download"),
            ("\0bad\u{0001}name", "_bad_name"),
        ];

        for (input, expected) in cases {
            assert_eq!(
                sanitize_filename(input).as_str(),
                expected,
                "input {input:?}"
            );
        }
    }

    #[test]
    fn sanitizer_bounds_utf16_without_splitting_unicode() {
        let sanitized = sanitize_filename(&"🦀".repeat(200));
        assert!(sanitized.as_str().encode_utf16().count() <= MAX_FILENAME_UTF16_UNITS);
        assert!(!sanitized.as_str().ends_with(char::REPLACEMENT_CHARACTER));
    }

    #[test]
    fn numbered_names_preserve_normal_extensions_and_remain_bounded() {
        assert_eq!(numbered_filename("archive.tar.gz", 3), "archive.tar (3).gz");
        let long = sanitize_filename(&format!("{}.bin", "x".repeat(176)));
        let candidate = numbered_filename(long.as_str(), 9999);
        assert!(candidate.encode_utf16().count() <= MAX_FILENAME_UTF16_UNITS);
        assert!(candidate.ends_with(" (9999).bin"));
    }

    #[test]
    fn io_classification_is_stable_and_path_free() {
        assert_eq!(
            classify_io(&io::Error::from(io::ErrorKind::StorageFull)),
            IoFailure::DiskFull
        );
        assert_eq!(
            classify_io(&io::Error::from(io::ErrorKind::PermissionDenied)),
            IoFailure::AccessDenied
        );
        assert_eq!(
            classify_io(&io::Error::from(io::ErrorKind::Unsupported)),
            IoFailure::Unsupported
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_sharing_violations_are_reported_as_file_locks() {
        assert_eq!(
            classify_io(&io::Error::from_raw_os_error(32)),
            IoFailure::FileLocked
        );
        assert_eq!(
            classify_io(&io::Error::from_raw_os_error(33)),
            IoFailure::FileLocked
        );
        assert_eq!(
            classify_io(&io::Error::from_raw_os_error(112)),
            IoFailure::DiskFull
        );
        assert_eq!(
            classify_io(&io::Error::from_raw_os_error(50)),
            IoFailure::Unsupported
        );
    }
}
