//! Producer side. A [`Recorder`] takes real caller-supplied observations,
//! validates each one against the manifest and the lifecycle rules, assigns
//! `seq`, and either streams to a capture directory or keeps the events in
//! memory. It never invents data: what the producer does not pass stays
//! `missing`/`unsupported`/`None`.
//!
//! Typical live use:
//!
//! ```no_run
//! use smash_match_data::*;
//!
//! # fn main() -> Result<(), CaptureError> {
//! let header = CaptureHeader {
//!     capture_id: "6b4d…".into(),
//!     recorder: ToolVersion::new("my-hook", "0.3.0"),
//!     game: GameBuild::ultimate("13.0.5"),
//!     source: Source::live_match(),
//!     channels: ChannelSupport::all_full(),
//!     ..Default::default()
//! };
//! let mut rec = Recorder::create("/tmp/capture-1", header, RecorderOptions::default())?;
//! rec.boundary(Clocks::tick(0), BoundaryKind::RecordingStart)?;
//! let p1 = ActorId(1);
//! rec.spawn(Clocks::tick(0), p1, ActorIdentity { entry: Some(0), ..Default::default() }, SpawnCause::MatchStart)?;
//! // per tick: before-state, input, after-state
//! rec.state(Clocks::tick(1), p1, Phase::Before, FighterSample::unsupported().with_vitals(Vitals { damage: 0.0, stocks: Some(3), shield: None }))?;
//! rec.finish(Clocks::tick(1), Completion::complete())?;
//! # Ok(()) }
//! ```

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::archive::{ArchiveError, ArchiveWriter, WriteOptions};
use crate::model::*;
use crate::validate::{self, ActorTrack, Issue, ObjectTrack, StreamChecker};

#[derive(Debug)]
pub enum CaptureError {
    /// The header or an event broke a rule. The recorder state is unchanged and
    /// recording can continue.
    Invalid(Issue),
    /// Disk failure. The recorder is poisoned afterwards: every further call
    /// fails with [`CaptureError::Poisoned`].
    Archive(ArchiveError),
    /// A previous disk failure already broke this recorder.
    Poisoned,
    /// `finish` was given `Completion::open()`.
    OpenCompletion,
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaptureError::Invalid(issue) => write!(f, "invalid event: {issue}"),
            CaptureError::Archive(err) => write!(f, "archive error: {err}"),
            CaptureError::Poisoned => f.write_str("recorder poisoned by an earlier archive error"),
            CaptureError::OpenCompletion => {
                f.write_str("finish requires a complete or truncated completion")
            }
        }
    }
}

impl std::error::Error for CaptureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CaptureError::Invalid(issue) => Some(issue),
            CaptureError::Archive(err) => Some(err),
            _ => None,
        }
    }
}

impl From<ArchiveError> for CaptureError {
    fn from(err: ArchiveError) -> Self {
        CaptureError::Archive(err)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecorderOptions {
    pub write: WriteOptions,
    /// Also keep every event in memory so `finish` can return a `Capture`.
    pub keep_events: bool,
}

/// Result of [`Recorder::finish`].
#[derive(Clone, Debug, PartialEq)]
pub struct Finished {
    pub manifest: Manifest,
    pub completeness: Completeness,
    /// Present for in-memory recorders and when `keep_events` was set.
    pub events: Option<Vec<Event>>,
    /// Non-error issues raised while recording.
    pub warnings: Vec<Issue>,
}

impl Finished {
    pub fn into_capture(self) -> Option<Capture> {
        let events = self.events?;
        Some(Capture {
            manifest: self.manifest,
            events,
            completeness: self.completeness,
        })
    }
}

pub struct Recorder {
    manifest: Manifest,
    checker: StreamChecker,
    warnings: Vec<Issue>,
    events: Option<Vec<Event>>,
    writer: Option<ArchiveWriter>,
    next_seq: u64,
    poisoned: bool,
}

impl Recorder {
    /// Record into memory only; `finish` returns the events.
    pub fn in_memory(header: CaptureHeader) -> Result<Self, CaptureError> {
        Self::new(header, None, true)
    }

    /// Record into a capture directory. The manifest is written immediately as
    /// `open`; events stream to the timeline as they arrive.
    pub fn create(
        dir: impl AsRef<Path>,
        header: CaptureHeader,
        options: RecorderOptions,
    ) -> Result<Self, CaptureError> {
        Self::check_header(&header)?;
        let writer = ArchiveWriter::create(dir, Manifest::new(header.clone()), &options.write)?;
        Self::new(header, Some(writer), options.keep_events)
    }

    fn check_header(header: &CaptureHeader) -> Result<Vec<Issue>, CaptureError> {
        let issues = validate::validate_header(header);
        if let Some(error) = issues.iter().find(|i| i.is_error()) {
            return Err(CaptureError::Invalid(error.clone()));
        }
        Ok(issues)
    }

    fn new(
        header: CaptureHeader,
        writer: Option<ArchiveWriter>,
        keep_events: bool,
    ) -> Result<Self, CaptureError> {
        let warnings = Self::check_header(&header)?;
        Ok(Recorder {
            checker: StreamChecker::new(&header),
            manifest: Manifest::new(header),
            warnings,
            events: keep_events.then(Vec::new),
            writer,
            next_seq: 0,
            poisoned: false,
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn header(&self) -> &CaptureHeader {
        &self.manifest.capture
    }

    /// Seq the next accepted event will get.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    pub fn warnings(&self) -> &[Issue] {
        &self.warnings
    }

    pub fn actors(&self) -> &BTreeMap<ActorId, ActorTrack> {
        self.checker.actors()
    }

    pub fn objects(&self) -> &BTreeMap<ObjectId, ObjectTrack> {
        self.checker.objects()
    }

    pub fn boundaries(&self) -> &[BoundaryMark] {
        self.checker.boundaries()
    }

    /// Validate, assign `seq`, and record one event. Returns its seq.
    pub fn push(&mut self, clocks: Clocks, body: EventBody) -> Result<u64, CaptureError> {
        if self.poisoned {
            return Err(CaptureError::Poisoned);
        }
        let event = Event {
            seq: self.next_seq,
            clocks,
            body,
        };
        let issues = self.checker.check(&event);
        if let Some(error) = issues.iter().find(|i| i.is_error()) {
            return Err(CaptureError::Invalid(error.clone()));
        }
        self.warnings.extend(issues);
        if let Some(writer) = &mut self.writer {
            if let Err(err) = writer.write_event(&event) {
                self.poisoned = true;
                return Err(err.into());
            }
        }
        if let Some(events) = &mut self.events {
            events.push(event);
        }
        self.next_seq += 1;
        Ok(self.next_seq - 1)
    }

    pub fn boundary(
        &mut self,
        clocks: Clocks,
        boundary: BoundaryKind,
    ) -> Result<u64, CaptureError> {
        self.push(clocks, EventBody::Boundary(boundary))
    }

    pub fn pace(&mut self, clocks: Clocks, pace: Pace) -> Result<u64, CaptureError> {
        self.push(clocks, EventBody::Pace(pace))
    }

    pub fn lifecycle(
        &mut self,
        clocks: Clocks,
        actor: ActorId,
        event: LifecycleEvent,
    ) -> Result<u64, CaptureError> {
        self.push(clocks, EventBody::Lifecycle { actor, event })
    }

    pub fn spawn(
        &mut self,
        clocks: Clocks,
        actor: ActorId,
        identity: ActorIdentity,
        cause: SpawnCause,
    ) -> Result<u64, CaptureError> {
        self.lifecycle(clocks, actor, LifecycleEvent::Spawn { identity, cause })
    }

    pub fn death(
        &mut self,
        clocks: Clocks,
        actor: ActorId,
        cause: Option<String>,
    ) -> Result<u64, CaptureError> {
        self.lifecycle(clocks, actor, LifecycleEvent::Death { cause })
    }

    pub fn respawn(&mut self, clocks: Clocks, actor: ActorId) -> Result<u64, CaptureError> {
        self.lifecycle(clocks, actor, LifecycleEvent::Respawn)
    }

    pub fn despawn(
        &mut self,
        clocks: Clocks,
        actor: ActorId,
        cause: DespawnCause,
    ) -> Result<u64, CaptureError> {
        self.lifecycle(clocks, actor, LifecycleEvent::Despawn { cause })
    }

    pub fn form_change(
        &mut self,
        clocks: Clocks,
        actor: ActorId,
        identity: ActorIdentity,
    ) -> Result<u64, CaptureError> {
        self.lifecycle(clocks, actor, LifecycleEvent::FormChange { identity })
    }

    pub fn state(
        &mut self,
        clocks: Clocks,
        actor: ActorId,
        phase: Phase,
        sample: FighterSample,
    ) -> Result<u64, CaptureError> {
        self.push(
            clocks,
            EventBody::State {
                actor,
                phase,
                sample,
            },
        )
    }

    pub fn input(&mut self, clocks: Clocks, input: InputEvent) -> Result<u64, CaptureError> {
        self.push(clocks, EventBody::Input(input))
    }

    pub fn object(
        &mut self,
        clocks: Clocks,
        object: ObjectId,
        event: ObjectEvent,
    ) -> Result<u64, CaptureError> {
        self.push(clocks, EventBody::Object { object, event })
    }

    pub fn object_spawn(
        &mut self,
        clocks: Clocks,
        object: ObjectId,
        identity: ObjectIdentity,
        transform: Channel<Transform>,
        animation: Channel<Animation>,
    ) -> Result<u64, CaptureError> {
        self.object(
            clocks,
            object,
            ObjectEvent::Spawn {
                identity,
                transform,
                animation,
            },
        )
    }

    pub fn object_state(
        &mut self,
        clocks: Clocks,
        object: ObjectId,
        transform: Channel<Transform>,
        animation: Channel<Animation>,
    ) -> Result<u64, CaptureError> {
        self.object(
            clocks,
            object,
            ObjectEvent::State {
                transform,
                animation,
            },
        )
    }

    pub fn object_despawn(
        &mut self,
        clocks: Clocks,
        object: ObjectId,
        cause: Option<String>,
    ) -> Result<u64, CaptureError> {
        self.object(clocks, object, ObjectEvent::Despawn { cause })
    }

    pub fn random(&mut self, clocks: Clocks, random: RandomEvent) -> Result<u64, CaptureError> {
        self.push(clocks, EventBody::Random(random))
    }

    pub fn gap(
        &mut self,
        clocks: Clocks,
        actor: Option<ActorId>,
        reason: Option<String>,
    ) -> Result<u64, CaptureError> {
        self.push(clocks, EventBody::Gap { actor, reason })
    }

    pub fn note(
        &mut self,
        clocks: Clocks,
        severity: Severity,
        text: impl Into<String>,
    ) -> Result<u64, CaptureError> {
        self.push(
            clocks,
            EventBody::Note {
                severity,
                text: text.into(),
            },
        )
    }

    /// Flush buffered timeline bytes to the OS (no-op in memory).
    pub fn flush(&mut self) -> Result<(), CaptureError> {
        if self.poisoned {
            return Err(CaptureError::Poisoned);
        }
        if let Some(writer) = &mut self.writer {
            if let Err(err) = writer.flush() {
                self.poisoned = true;
                return Err(err.into());
            }
        }
        Ok(())
    }

    /// Append the `end` event, derive the manifest summaries and publish the
    /// final manifest. `completion` is `complete` or `truncated(reason)`.
    pub fn finish(
        mut self,
        clocks: Clocks,
        completion: Completion,
    ) -> Result<Finished, CaptureError> {
        if completion.state == CompletionState::Open {
            return Err(CaptureError::OpenCompletion);
        }
        let end_seq = self.push(
            clocks,
            EventBody::End {
                events: self.next_seq,
            },
        )?;
        let completeness = match completion.state {
            CompletionState::Truncated => Completeness::DeclaredTruncated {
                reason: completion.reason.clone(),
            },
            _ => Completeness::Complete,
        };
        let mut manifest = self.manifest.clone();
        manifest.completion = completion;
        manifest.boundaries = self.checker.boundaries().to_vec();
        manifest.actors = self.checker.actor_summaries();
        let manifest = match self.writer.take() {
            Some(writer) => writer.finish(manifest)?,
            None => {
                manifest.completion.events = Some(end_seq + 1);
                manifest.completion.last_seq = Some(end_seq);
                manifest
            }
        };
        Ok(Finished {
            manifest,
            completeness,
            events: self.events.take(),
            warnings: std::mem::take(&mut self.warnings),
        })
    }

    /// Stop early on purpose: `finish` with a truncated completion.
    pub fn abort(
        self,
        clocks: Clocks,
        reason: impl Into<String>,
    ) -> Result<Finished, CaptureError> {
        self.finish(clocks, Completion::truncated(reason))
    }
}
