//! Capture directories: `manifest.json` plus a streaming `timeline.jsonl`.
//!
//! Writing: the manifest goes down first as `open`, events are appended (flushed
//! per event by default), and `finish` syncs the timeline then atomically
//! rewrites the manifest. Reading derives [`Completeness`] from the file itself
//! and cross-checks the manifest; a cut-off file is reported, never accepted as
//! complete.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Component, Path, PathBuf};

use crate::model::*;

#[derive(Debug)]
pub enum ArchiveError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Json {
        path: PathBuf,
        line: Option<u64>,
        source: serde_json::Error,
    },
    Format {
        path: PathBuf,
        found: String,
    },
    Version {
        path: PathBuf,
        found: FormatVersion,
        supported: FormatVersion,
    },
    AlreadyExists {
        path: PathBuf,
    },
    InvalidTimelineFile {
        name: String,
    },
    SeqOrder {
        path: PathBuf,
        line: u64,
        previous: u64,
        found: u64,
    },
    EventAfterEnd {
        path: PathBuf,
        line: u64,
    },
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArchiveError::Io { path, source } => write!(f, "{}: {source}", path.display()),
            ArchiveError::Json {
                path,
                line: Some(line),
                source,
            } => {
                write!(f, "{} line {line}: {source}", path.display())
            }
            ArchiveError::Json {
                path,
                line: None,
                source,
            } => {
                write!(f, "{}: {source}", path.display())
            }
            ArchiveError::Format { path, found } => write!(
                f,
                "{}: format is {found:?}, expected {FORMAT_NAME:?}",
                path.display()
            ),
            ArchiveError::Version {
                path,
                found,
                supported,
            } => write!(
                f,
                "{}: format version {found} is not readable by {supported}",
                path.display()
            ),
            ArchiveError::AlreadyExists { path } => {
                write!(
                    f,
                    "{} already exists (set overwrite to replace it)",
                    path.display()
                )
            }
            ArchiveError::InvalidTimelineFile { name } => write!(
                f,
                "timeline_file {name:?} must be a plain filename distinct from the manifest"
            ),
            ArchiveError::SeqOrder {
                path,
                line,
                previous,
                found,
            } => write!(
                f,
                "{} line {line}: seq {found} does not increase after {previous}",
                path.display()
            ),
            ArchiveError::EventAfterEnd { path, line } => {
                write!(
                    f,
                    "{} line {line}: event after the end event",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ArchiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ArchiveError::Io { source, .. } => Some(source),
            ArchiveError::Json { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> ArchiveError + '_ {
    move |source| ArchiveError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn timeline_path(dir: &Path, name: &str) -> Result<PathBuf, ArchiveError> {
    let mut components = Path::new(name).components();
    let plain =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if !plain
        || name.contains(['/', '\\'])
        || name.eq_ignore_ascii_case(MANIFEST_FILE)
        || name.eq_ignore_ascii_case("manifest.json.tmp")
    {
        return Err(ArchiveError::InvalidTimelineFile { name: name.into() });
    }
    Ok(dir.join(name))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteOptions {
    /// Flush the timeline after every event so a crash loses at most one line.
    pub flush_each_event: bool,
    /// Replace an existing capture directory's files instead of failing.
    pub overwrite: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions {
            flush_each_event: true,
            overwrite: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/// Atomically write `manifest.json` (temp file + rename).
pub fn write_manifest(dir: &Path, manifest: &Manifest) -> Result<(), ArchiveError> {
    let path = dir.join(MANIFEST_FILE);
    let tmp = dir.join("manifest.json.tmp");
    let json = serde_json::to_vec_pretty(manifest).map_err(|source| ArchiveError::Json {
        path: path.clone(),
        line: None,
        source,
    })?;
    {
        let mut file = File::create(&tmp).map_err(io_err(&tmp))?;
        file.write_all(&json).map_err(io_err(&tmp))?;
        file.write_all(b"\n").map_err(io_err(&tmp))?;
        file.sync_all().map_err(io_err(&tmp))?;
    }
    fs::rename(&tmp, &path).map_err(io_err(&path))
}

/// Read and version-check `manifest.json`.
pub fn read_manifest(dir: &Path) -> Result<Manifest, ArchiveError> {
    let path = dir.join(MANIFEST_FILE);
    let bytes = fs::read(&path).map_err(io_err(&path))?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|source| ArchiveError::Json {
            path: path.clone(),
            line: None,
            source,
        })?;
    if manifest.format != FORMAT_NAME {
        return Err(ArchiveError::Format {
            path,
            found: manifest.format,
        });
    }
    if !FORMAT_VERSION.can_read(manifest.format_version) {
        return Err(ArchiveError::Version {
            path,
            found: manifest.format_version,
            supported: FORMAT_VERSION,
        });
    }
    Ok(manifest)
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

/// Streaming timeline writer. Dropping it without `finish` leaves the manifest
/// `open`, which readers report as incomplete.
pub struct ArchiveWriter {
    dir: PathBuf,
    timeline_path: PathBuf,
    manifest: Manifest,
    out: BufWriter<File>,
    flush_each_event: bool,
    written: u64,
    last_seq: Option<u64>,
}

impl ArchiveWriter {
    pub fn create(
        dir: impl AsRef<Path>,
        mut manifest: Manifest,
        options: &WriteOptions,
    ) -> Result<Self, ArchiveError> {
        let dir = dir.as_ref().to_path_buf();
        let timeline_path = timeline_path(&dir, &manifest.timeline_file)?;
        fs::create_dir_all(&dir).map_err(io_err(&dir))?;
        let manifest_path = dir.join(MANIFEST_FILE);
        if !options.overwrite {
            for path in [&manifest_path, &timeline_path] {
                if path.exists() {
                    return Err(ArchiveError::AlreadyExists { path: path.clone() });
                }
            }
        }
        manifest.format = FORMAT_NAME.to_string();
        manifest.format_version = FORMAT_VERSION;
        manifest.completion = Completion::open();
        manifest.boundaries.clear();
        manifest.actors.clear();
        write_manifest(&dir, &manifest)?;
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&timeline_path)
            .map_err(io_err(&timeline_path))?;
        Ok(ArchiveWriter {
            dir,
            timeline_path,
            manifest,
            out: BufWriter::new(file),
            flush_each_event: options.flush_each_event,
            written: 0,
            last_seq: None,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn events_written(&self) -> u64 {
        self.written
    }

    pub fn last_seq(&self) -> Option<u64> {
        self.last_seq
    }

    pub fn write_event(&mut self, event: &Event) -> Result<(), ArchiveError> {
        if let Some(previous) = self.last_seq {
            if event.seq <= previous {
                return Err(ArchiveError::SeqOrder {
                    path: self.timeline_path.clone(),
                    line: self.written + 1,
                    previous,
                    found: event.seq,
                });
            }
        }
        let mut line = serde_json::to_vec(event).map_err(|source| ArchiveError::Json {
            path: self.timeline_path.clone(),
            line: Some(self.written + 1),
            source,
        })?;
        line.push(b'\n');
        self.out
            .write_all(&line)
            .map_err(io_err(&self.timeline_path))?;
        if self.flush_each_event {
            self.out.flush().map_err(io_err(&self.timeline_path))?;
        }
        self.written += 1;
        self.last_seq = Some(event.seq);
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), ArchiveError> {
        self.out.flush().map_err(io_err(&self.timeline_path))
    }

    /// Sync the timeline and publish the final manifest. `manifest.completion`
    /// must be `complete` or `truncated`; its counts are filled in here.
    pub fn finish(mut self, mut manifest: Manifest) -> Result<Manifest, ArchiveError> {
        self.out.flush().map_err(io_err(&self.timeline_path))?;
        self.out
            .get_ref()
            .sync_all()
            .map_err(io_err(&self.timeline_path))?;
        manifest.format = FORMAT_NAME.to_string();
        manifest.format_version = FORMAT_VERSION;
        manifest.timeline_file = self.manifest.timeline_file.clone();
        manifest.completion.events = Some(self.written);
        manifest.completion.last_seq = self.last_seq;
        write_manifest(&self.dir, &manifest)?;
        Ok(manifest)
    }
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

/// Streaming timeline reader. Drive it to exhaustion (`next_event` returning
/// `Ok(None)` or an error) and then ask for `completeness`.
pub struct TimelineReader {
    path: PathBuf,
    reader: BufReader<File>,
    declared: Completion,
    buf: String,
    lines: u64,
    events: u64,
    last_seq: Option<u64>,
    end_seen: bool,
    problems: Vec<TruncationProblem>,
    completeness: Option<Completeness>,
}

/// Open the timeline named by `manifest` inside `dir`.
pub fn open_timeline(dir: &Path, manifest: &Manifest) -> Result<TimelineReader, ArchiveError> {
    let path = timeline_path(dir, &manifest.timeline_file)?;
    let file = File::open(&path).map_err(io_err(&path))?;
    Ok(TimelineReader {
        path,
        reader: BufReader::new(file),
        declared: manifest.completion.clone(),
        buf: String::new(),
        lines: 0,
        events: 0,
        last_seq: None,
        end_seen: false,
        problems: Vec::new(),
        completeness: None,
    })
}

impl TimelineReader {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Events returned so far.
    pub fn events_read(&self) -> u64 {
        self.events
    }

    /// Available once the reader is exhausted.
    pub fn completeness(&self) -> Option<&Completeness> {
        self.completeness.as_ref()
    }

    pub fn next_event(&mut self) -> Result<Option<Event>, ArchiveError> {
        if self.completeness.is_some() {
            return Ok(None);
        }
        self.buf.clear();
        let read = self
            .reader
            .read_line(&mut self.buf)
            .map_err(io_err(&self.path))?;
        if read == 0 {
            self.finish();
            return Ok(None);
        }
        self.lines += 1;
        let line = self.lines;
        let partial = !self.buf.ends_with('\n');
        let text = self.buf.trim_end_matches(['\n', '\r']);
        let event: Event = match serde_json::from_str(text) {
            Ok(event) => event,
            Err(_) if partial => {
                self.problems
                    .push(TruncationProblem::PartialTrailingLine { line });
                self.finish();
                return Ok(None);
            }
            Err(source) => {
                return Err(ArchiveError::Json {
                    path: self.path.clone(),
                    line: Some(line),
                    source,
                })
            }
        };
        if self.end_seen {
            return Err(ArchiveError::EventAfterEnd {
                path: self.path.clone(),
                line,
            });
        }
        if let Some(previous) = self.last_seq {
            if event.seq <= previous {
                return Err(ArchiveError::SeqOrder {
                    path: self.path.clone(),
                    line,
                    previous,
                    found: event.seq,
                });
            }
        }
        if partial {
            self.problems
                .push(TruncationProblem::MissingTrailingNewline { line });
        }
        if let EventBody::End { events } = event.body {
            self.end_seen = true;
            if events != self.events {
                self.problems.push(TruncationProblem::EndCountMismatch {
                    declared: events,
                    observed: self.events,
                });
            }
        }
        self.last_seq = Some(event.seq);
        self.events += 1;
        if partial {
            self.finish();
        }
        Ok(Some(event))
    }

    fn finish(&mut self) {
        let mut problems = Vec::new();
        if self.declared.state == CompletionState::Open {
            problems.push(TruncationProblem::ManifestOpen);
        }
        if !self.end_seen {
            problems.push(TruncationProblem::MissingEnd);
        }
        problems.append(&mut self.problems);
        if let Some(declared) = self.declared.events {
            if declared != self.events {
                problems.push(TruncationProblem::ManifestCountMismatch {
                    declared,
                    observed: self.events,
                });
            }
        }
        if let (Some(declared), Some(observed)) = (self.declared.last_seq, self.last_seq) {
            if declared != observed {
                problems.push(TruncationProblem::ManifestLastSeqMismatch { declared, observed });
            }
        }
        self.completeness = Some(if !problems.is_empty() {
            Completeness::Incomplete { problems }
        } else if self.declared.state == CompletionState::Truncated {
            Completeness::DeclaredTruncated {
                reason: self.declared.reason.clone(),
            }
        } else {
            Completeness::Complete
        });
    }
}

impl Iterator for TimelineReader {
    type Item = Result<Event, ArchiveError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_event().transpose()
    }
}

// ---------------------------------------------------------------------------
// Whole-capture convenience
// ---------------------------------------------------------------------------

impl Capture {
    /// Load a capture directory fully into memory.
    pub fn load(dir: impl AsRef<Path>) -> Result<Capture, ArchiveError> {
        let dir = dir.as_ref();
        let manifest = read_manifest(dir)?;
        let mut reader = open_timeline(dir, &manifest)?;
        let mut events = Vec::new();
        while let Some(event) = reader.next_event()? {
            events.push(event);
        }
        let completeness = reader
            .completeness()
            .cloned()
            .unwrap_or(Completeness::Incomplete {
                problems: vec![TruncationProblem::MissingEnd],
            });
        Ok(Capture {
            manifest,
            events,
            completeness,
        })
    }

    /// Write this capture to `dir`. Any existing `end` event is replaced by a
    /// fresh one so the output is self-consistent. An `incomplete` source is
    /// saved as `truncated` with the original problems in the reason.
    pub fn save(
        &self,
        dir: impl AsRef<Path>,
        options: &WriteOptions,
    ) -> Result<Manifest, ArchiveError> {
        let mut manifest = self.manifest.clone();
        let mut writer = ArchiveWriter::create(dir, manifest.clone(), options)?;
        let mut last_clocks = Clocks::NONE;
        let mut count = 0u64;
        for event in &self.events {
            if matches!(event.body, EventBody::End { .. }) {
                continue;
            }
            writer.write_event(event)?;
            last_clocks = event.clocks;
            count += 1;
        }
        let end_seq = writer.last_seq().map_or(0, |seq| seq + 1);
        writer.write_event(&Event {
            seq: end_seq,
            clocks: last_clocks,
            body: EventBody::End { events: count },
        })?;
        manifest.completion = match &self.completeness {
            Completeness::Complete => Completion::complete(),
            Completeness::DeclaredTruncated { reason } => Completion {
                state: CompletionState::Truncated,
                reason: reason.clone(),
                events: None,
                last_seq: None,
            },
            other @ Completeness::Incomplete { .. } => {
                Completion::truncated(format!("re-saved {other}"))
            }
        };
        writer.finish(manifest)
    }
}
