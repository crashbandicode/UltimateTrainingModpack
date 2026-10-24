//! Training ↔ smash-match-data adapter, built against the shared crate's HEAD
//! `faac04e1a49bebd33cb2fd8f0967e120eabca899`. The shared crate supplies the
//! model, archive I/O, validation and clip resolution; it ships no consumer
//! adapters. Everything Training-specific is owned here, in `live_frame.rs`
//! (`LiveInput::project`) and in `replay.rs` (`ReplayTrack::export`):
//! * header and provenance: the `training_native_f32` layout, the opaque
//!   serializer-counter raw unit, partial channel declarations, `Clocks::NONE`;
//! * actor lifecycle derived from native object ids (spawn, form change, despawn);
//! * canonical native f32 inputs and kinematics, bit-exact in the timeline;
//! * the one-pass `Projector` into Training's compact cache, which quantises
//!   analog values only at this consumer boundary (B7–B9 bounds on actors,
//!   manifest/timeline/line sizes and event count);
//! * Switch-compatible staging: the absent-destination manifest rewrite and a
//!   same-parent publish rename, instead of the shared `Recorder::finish`.
//!
//! Host tests include a HEAD `Recorder` parity oracle and selected-boundary
//! checks against known source frames.
//!
//! Canonical native observations, published together with a compact editor cache.
//! All JSONL work belongs on a serialized worker, never on a menu/frame callback.
//! `Before` is the proposed pre-fighter-callback phase, pending runtime qualification;
//! it is not an acceptance claim about input application order.
//! Native XYZ ABI, body transitions, retained-float heap headroom, SD throughput/
//! queue drops and same-parent target directory rename still need parent acceptance.
//! Published files must remain immutable during canonical load: the bounded
//! prescan/reopen protocol does not protect against concurrent external replacement.
//! Host tests below are implementation evidence only after the parent runs them.

use std::{collections::BTreeMap, fs::{self, File, OpenOptions}, io::{self, BufRead, BufReader},
    path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}};
use smash_match_data::{archive::{self, ArchiveWriter}, model::*, validate::{self, StreamChecker}, WriteOptions};
use training_mod_mii::replay::{ReplayMiis, MAX_ENCODED_LEN};
use crate::{capture_archive::{decode_file_id, encode_file_id, validate_game_version,
    validate_replay_id, validate_replay_name, MAX_CAPTURE_TRACKS}, live_frame::{LiveFrame, LiveInput, LiveSession},
    CaptureArchive, CaptureSaveOutcome, ReplayFrame, ReplaySession, ReplayTrack, SavedCapture,
    StartState, MAX_FRAMES};

const MAX_ACTORS: usize = 1024;
const MAX_MANIFEST: u64 = 1024 * 1024;
const MAX_TIMELINE: u64 = 256 * 1024 * 1024;
const MAX_LINE: usize = 16 * 1024;
// State + input + possible form change per sample, raw per group, replacement
// spawn/despawn per actor, optional Mii metadata, two recording boundaries and one End.
const MAX_EVENTS: u64 = (3 * MAX_CAPTURE_TRACKS * MAX_FRAMES + MAX_FRAMES + 2 * MAX_ACTORS + 4) as u64;
const LAYOUT: &str = "training_native_f32";
const COUNTER: &str = "smash13_0_5_serializer_counter_u32le";
const MII_REPLAY: &str = "training_mii_replay_v1";
const PHASE_NOTE: &str = "State(Before) is the proposed pre-fighter-callback phase, NOT an acceptance claim; input application order and Effective inputs are pending runtime qualification.";
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

fn bad(message: impl ToString) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message.to_string()) }
fn check_header(header: &CaptureHeader) -> io::Result<()> {
    if let Some(issue) = validate::validate_header(header).into_iter().find(|i| i.is_error()) {
        return Err(bad(format!("shared header: {issue:?}")));
    }
    Ok(())
}
fn check_event(checker: &mut StreamChecker, event: &Event) -> io::Result<()> {
    if let Some(issue) = checker.check(event).into_iter().find(|i| i.is_error()) {
        return Err(bad(format!("shared event: {issue:?}")));
    }
    Ok(())
}

fn header(id: &str, name: &str, version: &str, entries: &[u8]) -> CaptureHeader {
    let axes = AxisLayout { encoding: AxisEncoding::F32, min: None, max: None,
        neutral: None, float_scale: None, note: None };
    let mut source = Source::live_match();
    source.description = Some(name.into());
    CaptureHeader { capture_id: id.into(), recorder: ToolVersion::new("Training", "shared-capture-1"),
        game: GameBuild::ultimate(version), source,
        rules: Rules { target: RulesetTarget::Competitive, ..Rules::default() },
        entries: entries.iter().copied().map(FighterEntry::new).collect(),
        input_layouts: vec![InputLayout { id: LAYOUT.into(), description: Some("Native ControlModule observations".into()),
            buttons: ButtonLayout { width_bits: 32, ..ButtonLayout::default() },
            left_stick: axes.clone(), right_stick: axes, raw_encoding: None }],
        channels: ChannelSupport { inputs: Supply::partial(PHASE_NOTE),
            kinematics: Supply::partial(PHASE_NOTE), situation: Supply::partial(PHASE_NOTE),
            lifecycle: Supply::partial("First observation, native object replacement and form changes only; no observed death/respawn."),
            ..ChannelSupport::default() },
        notes: vec![PHASE_NOTE.into(), "Serializer counter is opaque raw provenance, not a tick/input_seq/record clock. All samples explicit; actual rules, stage and controllers unknown.".into()],
        ..CaptureHeader::default() }
}

fn identity(entry: u8, frame: &LiveFrame) -> io::Result<ActorIdentity> {
    Ok(ActorIdentity { entry: Some(entry), fighter_kind: Some(frame.start.fighter_kind),
        costume: Some(u8::try_from(frame.start.costume).map_err(bad)?),
        native_object_id: Some(frame.body), ..ActorIdentity::default() })
}

fn validate_session(session: &LiveSession) -> io::Result<usize> {
    if session.tracks.is_empty() || session.tracks.len() > MAX_CAPTURE_TRACKS { return Err(bad("expected 1..8 tracks")); }
    let mut seen = [false; 8];
    let mut actors = 0;
    let mut longest = 0;
    for track in &session.tracks {
        let entry = usize::try_from(track.entry).map_err(bad)?;
        if entry >= 8 || seen[entry] { return Err(bad("duplicate or invalid fighter entry")); }
        seen[entry] = true;
        if track.frames.is_empty() || track.frames.len() > MAX_FRAMES { return Err(bad("invalid track length")); }
        longest = longest.max(track.frames.len());
        let mut body = None;
        for frame in &track.frames {
            if !frame.is_valid() { return Err(bad("invalid native frame")); }
            identity(entry as u8, frame)?;
            if body != Some(frame.body) { actors += 1; body = Some(frame.body); }
        }
    }
    if actors > MAX_ACTORS { return Err(bad("too many actor incarnations")); }
    session.first_counter.ok_or_else(|| bad("missing serializer counter"))?
        .checked_add((longest - 1) as u32).ok_or_else(|| bad("serializer counter wraps"))?;
    Ok(longest)
}

#[derive(Clone, Debug)]
pub struct SharedCaptureArchive { root: PathBuf }

impl SharedCaptureArchive {
    pub fn new(root: impl Into<PathBuf>) -> Self { Self { root: root.into() } }

    fn require_absolute(&self) -> io::Result<()> {
        if !self.root.is_absolute() { return Err(bad("shared archive root must be absolute")); }
        Ok(())
    }

    pub fn save_with_outcome(&self, replay_id: &str, replay_name: &str, game_version: &str,
        session: &LiveSession) -> io::Result<CaptureSaveOutcome> {
        self.save_with_miis(replay_id, replay_name, game_version, session, None)
    }

    /// The optional replay Mii payload and editor cache publish in the same bundle rename.
    /// An existing capture keeps its original association, including unavailable metadata.
    pub fn save_with_miis(&self, replay_id: &str, replay_name: &str, game_version: &str,
        session: &LiveSession, miis: Option<&ReplayMiis>) -> io::Result<CaptureSaveOutcome> {
        self.require_absolute()?;
        validate_replay_id(replay_id).map_err(bad)?;
        validate_replay_name(replay_name).map_err(bad)?;
        validate_game_version(game_version).map_err(bad)?;
        let longest = validate_session(session)?;
        let id = encode_file_id(replay_id);
        let final_dir = self.root.join(format!("{id}.smd"));
        if exists(&final_dir)? {
            same_identity(&self.load(&id)?, replay_id, replay_name, game_version, miis)?;
            return Ok(CaptureSaveOutcome { id, created: false });
        }
        // A pre-existing legacy capture remains authoritative for its identity.
        if exists(&self.root.join(format!("{id}.tmrc")))? {
            same_identity(&CaptureArchive::new(&self.root).load(&id)?, replay_id, replay_name, game_version, miis)?;
            return Ok(CaptureSaveOutcome { id, created: false });
        }
        fs::create_dir_all(&self.root)?;
        let staging = self.create_staging(&id)?;
        let entries: Vec<u8> = session.tracks.iter().map(|t| t.entry as u8).collect();
        let h = header(&id, replay_name, game_version, &entries);
        check_header(&h)?;
        let mut checker = StreamChecker::new(&h);
        let mut projector = Projector::new(&h)?;
        let mut writer = ArchiveWriter::create(&staging, Manifest::new(h),
            &WriteOptions { flush_each_event: false, overwrite: false }).map_err(bad)?;
        let mut seq = 0;
        let mut emit = |body| -> io::Result<()> {
            let event = Event { seq, clocks: Clocks::NONE, body };
            projector.consume(&event)?;
            check_event(&mut checker, &event)?;
            writer.write_event(&event).map_err(bad)?;
            seq += 1;
            Ok(())
        };
        emit(EventBody::Boundary(BoundaryKind::RecordingStart))?;
        if let Some(miis) = miis {
            emit(EventBody::Raw(RawUnit {
                raw: RawBytes::new(MII_REPLAY, &miis.to_bytes()), source_offset: None,
            }))?;
        }
        let mut tracks: Vec<_> = session.tracks.iter().collect();
        tracks.sort_by_key(|t| t.entry);
        let mut active: [Option<(ActorId, ActorIdentity)>; 8] = std::array::from_fn(|_| None);
        let mut next_actor = 0;
        for index in 0..longest {
            let counter = session.first_counter.ok_or_else(|| bad("missing counter"))? + index as u32;
            emit(EventBody::Raw(RawUnit { raw: RawBytes::new(COUNTER, &counter.to_le_bytes()), source_offset: Some(0x200078) }))?;
            for track in &tracks {
                let Some(frame) = track.frames.get(index) else { continue };
                let entry = track.entry as u8;
                let ident = identity(entry, frame)?;
                let slot = &mut active[entry as usize];
                if slot.as_ref().is_some_and(|(_, old)| old.native_object_id != ident.native_object_id) {
                    emit(EventBody::Lifecycle { actor: slot.as_ref().ok_or_else(|| bad("missing actor"))?.0,
                        event: LifecycleEvent::Despawn { cause: DespawnCause::Unknown } })?;
                    *slot = None;
                }
                let actor = if let Some((actor, old)) = slot {
                    if *old != ident {
                        emit(EventBody::Lifecycle { actor: *actor, event: LifecycleEvent::FormChange { identity: ident.clone() } })?;
                        *old = ident;
                    }
                    *actor
                } else {
                    let actor = ActorId(next_actor); next_actor += 1;
                    emit(EventBody::Lifecycle { actor, event: LifecycleEvent::Spawn { identity: ident.clone(), cause: SpawnCause::Unknown } })?;
                    *slot = Some((actor, ident)); actor
                };
                emit(EventBody::State { actor, phase: Phase::Before,
                    sample: FighterSample::unsupported().with_kinematics(Kinematics {
                        position: Vec3::new(frame.x, frame.y, frame.position_z), facing: frame.start.facing,
                        velocity: Some(Vec3::new(frame.start.speed_x, frame.start.speed_y, frame.speed_z)),
                    }).with_situation(Situation { situation_kind: frame.start.situation, status_kind: frame.start.status,
                        grounded: None, hitstop_frames: None }) })?;
                let [lx, ly, rx, ry] = frame.input.axes;
                emit(EventBody::Input(InputEvent { actor: Some(actor), entry: Some(entry), controller: None,
                    origin: InputOrigin::Effective, layout: LAYOUT.into(), buttons: Channel::Observed(Buttons::new(frame.input.buttons)),
                    left_stick: Channel::Observed(Stick::F32 { x: lx, y: ly }),
                    right_stick: Channel::Observed(Stick::F32 { x: rx, y: ry }), raw: None }))?;
            }
        }
        emit(EventBody::Boundary(BoundaryKind::RecordingEnd))?;
        // End.events counts the preceding lines, not itself.
        let preceding = writer.events_written();
        let end = Event { seq: preceding, clocks: Clocks::NONE, body: EventBody::End { events: preceding } };
        projector.consume(&end)?;
        check_event(&mut checker, &end)?;
        writer.write_event(&end).map_err(bad)?;
        writer.flush().map_err(bad)?;
        let mut manifest = writer.manifest().clone();
        manifest.completion = if session.truncated { Completion::truncated("Training native capture was truncated") } else { Completion::complete() };
        manifest.completion.events = Some(writer.events_written());
        manifest.completion.last_seq = writer.last_seq();
        manifest.actors = checker.actor_summaries();
        manifest.boundaries = checker.boundaries().to_vec();
        drop(writer);
        // Sync through a write-capable handle without truncation. Buffer flush
        // alone is not a durability barrier on every host filesystem.
        OpenOptions::new().write(true).truncate(false).open(staging.join(TIMELINE_FILE))?.sync_all()?;
        // Only this exclusive staging directory's own open manifest is removed.
        // Switch cannot replace an existing destination with rename.
        fs::remove_file(staging.join(MANIFEST_FILE))?;
        archive::write_manifest(&staging, &manifest).map_err(bad)?;
        let (projected, projected_miis) = projector.finish(session.truncated)?;
        CaptureArchive::new(&staging).save_with_miis(replay_id, replay_name, game_version, &projected, projected_miis.as_ref())?;
        prescan(&staging.join(TIMELINE_FILE))?;
        bounded_file(&staging.join(MANIFEST_FILE), MAX_MANIFEST)?;
        // Caller serializes publication. Never replace a concurrently appearing path.
        if exists(&final_dir)? || exists(&self.root.join(format!("{id}.tmrc")))? {
            return Err(bad("capture destination appeared during publication"));
        }
        fs::rename(&staging, &final_dir)?;
        Ok(CaptureSaveOutcome { id, created: true })
    }

    fn create_staging(&self, id: &str) -> io::Result<PathBuf> {
        for _ in 0..64 {
            let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = self.root.join(format!(".{id}.smd-{nonce}.tmp"));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(path),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(io::ErrorKind::AlreadyExists, "staging retry limit reached"))
    }

    /// Canonical streaming re-projection for host/off-thread use only. Never
    /// mutates the canonical capture or repairs its derived editor cache.
    pub fn load(&self, id: &str) -> io::Result<SavedCapture> {
        self.require_absolute()?;
        let replay_id = decode_file_id(id).map_err(bad)?;
        let dir = self.root.join(format!("{id}.smd"));
        if !fs::symlink_metadata(&dir)?.file_type().is_dir() { return Err(bad("capture is not a directory")); }
        bounded_file(&dir.join(MANIFEST_FILE), MAX_MANIFEST)?;
        let manifest = archive::read_manifest(&dir).map_err(bad)?;
        if manifest.actors.len() > MAX_ACTORS || manifest.timeline_file != TIMELINE_FILE { return Err(bad("invalid shared manifest bounds/path")); }
        let name = manifest.capture.source.description.as_deref().ok_or_else(|| bad("missing replay title"))?;
        let version = manifest.capture.game.version.as_deref().ok_or_else(|| bad("missing game version"))?;
        validate_replay_name(name).map_err(bad)?;
        validate_game_version(version).map_err(bad)?;
        let entries: Vec<u8> = manifest.capture.entries.iter().map(|e| e.entry).collect();
        if manifest.capture != header(id, name, version, &entries) { return Err(bad("unexpected capture identity/provenance/layout")); }
        check_header(&manifest.capture)?;
        prescan(&dir.join(TIMELINE_FILE))?;
        let mut projector = Projector::new(&manifest.capture)?;
        let mut checker = StreamChecker::new(&manifest.capture);
        let mut reader = archive::open_timeline(&dir, &manifest).map_err(bad)?;
        while let Some(event) = reader.next_event().map_err(bad)? {
            // Project first: its actor/channel restrictions bound checker state.
            projector.consume(&event)?;
            check_event(&mut checker, &event)?;
        }
        let truncated = match reader.completeness() {
            Some(Completeness::Complete) => false,
            Some(Completeness::DeclaredTruncated { .. }) => true,
            _ => return Err(bad("incomplete shared timeline")),
        };
        if manifest.actors != checker.actor_summaries() || manifest.boundaries != checker.boundaries()
            || manifest.completion.events != Some(checker.accepted()) || manifest.completion.last_seq != checker.last_seq() {
            return Err(bad("manifest derived metadata disagrees with timeline"));
        }
        let (session, miis) = projector.finish(truncated)?;
        let cache = CaptureArchive::new(&dir).load(id)?;
        same_identity(&cache, &replay_id, name, version, miis.as_ref())?;
        if !same_projection(&session, &cache.session) { return Err(bad("compact cache disagrees with canonical timeline")); }
        Ok(SavedCapture { replay_id, replay_name: name.into(), game_version: version.into(), session, miis })
    }
}

fn exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) { Ok(_) => Ok(true), Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false), Err(e) => Err(e) }
}
fn bounded_file(path: &Path, limit: u64) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.file_type().is_file() || meta.len() > limit { return Err(bad("shared file type/size exceeds bound")); }
    Ok(())
}
fn prescan(path: &Path) -> io::Result<()> {
    bounded_file(path, MAX_TIMELINE)?;
    let mut reader = BufReader::new(File::open(path)?);
    let (mut line, mut lines, mut total) = (0usize, 0u64, 0u64);
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() { break; }
        for &byte in buf {
            line += 1; total += 1;
            if line > MAX_LINE || total > MAX_TIMELINE { return Err(bad("timeline line/size exceeds bound")); }
            if byte == b'\n' { lines += 1; line = 0; }
            if lines > MAX_EVENTS { return Err(bad("timeline event count exceeds bound")); }
        }
        let len = buf.len(); reader.consume(len);
    }
    if line != 0 || lines == 0 { return Err(bad("empty or unterminated timeline")); }
    Ok(())
}
fn same_identity(saved: &SavedCapture, id: &str, name: &str, version: &str, miis: Option<&ReplayMiis>) -> io::Result<()> {
    if saved.replay_id != id || saved.replay_name != name || saved.game_version != version { return Err(bad("capture identity mismatch")); }
    if saved.miis.as_ref() != miis { return Err(bad("capture Mii association mismatch")); }
    Ok(())
}
fn same_projection(a: &ReplaySession, b: &ReplaySession) -> bool {
    a.truncated == b.truncated && a.tracks.len() == b.tracks.len() && a.tracks.iter().zip(&b.tracks).all(|(a,b)| {
        a.entry == b.entry && a.quantized_analog == b.quantized_analog && a.frames.len() == b.frames.len()
            && a.frames.iter().zip(&b.frames).all(|(a,b)| a.input == b.input
                && a.start.fighter_kind == b.start.fighter_kind && a.start.costume == b.start.costume
                && a.start.situation == b.start.situation && a.start.status == b.start.status
                && [a.x,a.y,a.start.facing,a.start.speed_x,a.start.speed_y].map(f32::to_bits)
                    == [b.x,b.y,b.start.facing,b.start.speed_x,b.start.speed_y].map(f32::to_bits))
    })
}

struct ProjectedActor { identity: ActorIdentity, alive: bool }
struct Projector {
    miis: Option<ReplayMiis>,
    tracks: Vec<ReplayTrack>, slots: [Option<usize>; 8],
    actors: BTreeMap<ActorId, ProjectedActor>, active: [Option<ActorId>; 8],
    pending: Option<(ActorId, Kinematics, Situation)>,
    counter: Option<u32>, group: usize, last_entry: Option<u8>, group_samples: usize,
    started: bool, stopped: bool, ended: bool, seq: u64,
}

impl Projector {
    fn new(h: &CaptureHeader) -> io::Result<Self> {
        if h.entries.is_empty() || h.entries.len() > MAX_CAPTURE_TRACKS { return Err(bad("invalid projected roster")); }
        let mut slots = [None; 8];
        let mut tracks = Vec::new();
        for entry in &h.entries {
            if entry.entry >= 8 || slots[entry.entry as usize].is_some() { return Err(bad("duplicate/invalid projected entry")); }
            slots[entry.entry as usize] = Some(tracks.len());
            tracks.push(ReplayTrack { entry: entry.entry as i32, frames: Vec::new(), quantized_analog: false });
        }
        Ok(Self { miis: None, tracks, slots, actors: BTreeMap::new(), active: [None; 8], pending: None,
            counter: None, group: 0, last_entry: None, group_samples: 0, started: false, stopped: false, ended: false, seq: 0 })
    }

    fn consume(&mut self, e: &Event) -> io::Result<()> {
        if e.seq != self.seq || e.clocks != Clocks::NONE || self.ended { return Err(bad("unexpected event ordering/clocks")); }
        self.seq += 1;
        if self.pending.is_some() && !matches!(e.body, EventBody::Input(_)) { return Err(bad("state must be immediately followed by input")); }
        match &e.body {
            EventBody::Boundary(BoundaryKind::RecordingStart) if !self.started && e.seq == 0 => self.started = true,
            EventBody::Boundary(BoundaryKind::RecordingEnd) if self.started && !self.stopped && self.group_samples > 0 => self.stopped = true,
            EventBody::End { events } if self.stopped && *events == e.seq => self.ended = true,
            EventBody::Raw(raw) if raw.raw.encoding == MII_REPLAY => {
                if !self.started || self.stopped || e.seq != 1 || self.miis.is_some()
                    || self.counter.is_some() || raw.source_offset.is_some()
                    || raw.raw.hex.len() > 2 * MAX_ENCODED_LEN {
                    return Err(bad("invalid replay Mii placement or bounds"));
                }
                self.miis = Some(ReplayMiis::from_bytes(&raw.raw.bytes().map_err(bad)?).map_err(bad)?);
            }
            EventBody::Raw(raw) if self.started && !self.stopped => {
                if raw.raw.encoding != COUNTER || raw.source_offset != Some(0x200078) || raw.raw.hex.len() != 8 { return Err(bad("invalid serializer provenance")); }
                let bytes: [u8;4] = raw.raw.bytes().map_err(bad)?.try_into().map_err(|_| bad("counter width"))?;
                let value = u32::from_le_bytes(bytes);
                if let Some(previous) = self.counter {
                    if self.group_samples == 0 || previous.checked_add(1) != Some(value) { return Err(bad("counter gap/wrap or empty group")); }
                    self.group += 1;
                }
                if self.group >= MAX_FRAMES { return Err(bad("too many sample groups")); }
                self.counter = Some(value); self.last_entry = None; self.group_samples = 0;
            }
            EventBody::Lifecycle { actor, event } if self.counter.is_some() && !self.stopped => {
                match event {
                    LifecycleEvent::Spawn { identity, cause: SpawnCause::Unknown } => {
                        let entry = self.identity_entry(identity)?;
                        if self.actors.len() >= MAX_ACTORS || self.actors.contains_key(actor) || self.active[entry].is_some() { return Err(bad("actor limit/reuse or overlapping entry")); }
                        self.actors.insert(*actor, ProjectedActor { identity: identity.clone(), alive: true });
                        self.active[entry] = Some(*actor);
                    }
                    LifecycleEvent::FormChange { identity } => {
                        let entry = self.identity_entry(identity)?;
                        let old = self.actors.get_mut(actor).ok_or_else(|| bad("unknown actor"))?;
                        if !old.alive || self.active[entry] != Some(*actor) || old.identity.entry != identity.entry
                            || old.identity.native_object_id != identity.native_object_id { return Err(bad("invalid form identity")); }
                        old.identity = identity.clone();
                    }
                    LifecycleEvent::Despawn { cause: DespawnCause::Unknown } => {
                        let old = self.actors.get_mut(actor).ok_or_else(|| bad("unknown actor"))?;
                        let entry = old.identity.entry.ok_or_else(|| bad("missing entry"))? as usize;
                        if !old.alive || self.active[entry] != Some(*actor) { return Err(bad("invalid despawn")); }
                        old.alive = false; self.active[entry] = None;
                    }
                    _ => return Err(bad("unsupported lifecycle")),
                }
            }
            EventBody::State { actor, phase: Phase::Before, sample } if self.counter.is_some() && !self.stopped => {
                let observed = self.actors.get(actor).ok_or_else(|| bad("unknown state actor"))?;
                if !observed.alive { return Err(bad("state for inactive actor")); }
                let (Channel::Observed(k), Channel::Observed(s)) = (&sample.kinematics, &sample.situation) else { return Err(bad("missing explicit state")); };
                if !matches!(sample.animation, Channel::Unsupported) || !matches!(sample.vitals, Channel::Unsupported)
                    || k.velocity.is_none() || s.grounded.is_some() || s.hitstop_frames.is_some() { return Err(bad("unexpected state channels")); }
                self.pending = Some((*actor, *k, *s));
            }
            EventBody::Input(input) if !self.stopped => {
                let (actor, k, s) = self.pending.take().ok_or_else(|| bad("input without explicit before state"))?;
                let ident = &self.actors.get(&actor).ok_or_else(|| bad("missing input identity"))?.identity;
                let entry = ident.entry.ok_or_else(|| bad("missing entry"))?;
                if input.actor != Some(actor) || input.entry != Some(entry) || input.controller.is_some()
                    || input.origin != InputOrigin::Effective || input.layout != LAYOUT || input.raw.is_some()
                    || self.last_entry.is_some_and(|last| entry <= last) { return Err(bad("mixed actor/entry/layout or sample order")); }
                let (Channel::Observed(buttons), Channel::Observed(Stick::F32 { x: lx, y: ly }),
                    Channel::Observed(Stick::F32 { x: rx, y: ry })) = (&input.buttons, &input.left_stick, &input.right_stick)
                    else { return Err(bad("missing native axes/buttons")); };
                let native = LiveInput { buttons: buttons.bits, axes: [*lx,*ly,*rx,*ry] };
                let velocity = k.velocity.ok_or_else(|| bad("missing velocity"))?;
                if !native.is_finite() || !k.position.is_finite() || !velocity.is_finite() || !k.facing.is_finite() { return Err(bad("non-finite sample")); }
                let track = &mut self.tracks[self.slots[entry as usize].ok_or_else(|| bad("undeclared entry"))?];
                if track.frames.len() != self.group || track.frames.len() >= MAX_FRAMES { return Err(bad("noncontiguous or oversized track")); }
                let (input, quantized) = native.project();
                track.quantized_analog |= quantized;
                if track.frames.len() == track.frames.capacity() {
                    let target = track.frames.capacity().saturating_mul(2).clamp(256, MAX_FRAMES);
                    track.frames.try_reserve_exact(target - track.frames.len()).map_err(bad)?;
                }
                track.frames.push(ReplayFrame { input, x: k.position.x, y: k.position.y,
                    start: StartState { fighter_kind: ident.fighter_kind.ok_or_else(|| bad("missing kind"))?,
                        costume: ident.costume.ok_or_else(|| bad("missing costume"))? as u32,
                        facing: k.facing, situation: s.situation_kind, status: s.status_kind,
                        speed_x: velocity.x, speed_y: velocity.y } });
                self.last_entry = Some(entry); self.group_samples += 1;
            }
            _ => return Err(bad("unexpected shared event")),
        }
        Ok(())
    }

    fn identity_entry(&self, i: &ActorIdentity) -> io::Result<usize> {
        let entry = i.entry.ok_or_else(|| bad("missing actor entry"))? as usize;
        if entry >= 8 || self.slots[entry].is_none() || i.fighter_kind.is_none_or(|k| k < 0)
            || i.costume.is_none() || i.native_object_id.is_none() || i.controller.is_some()
            || i.control != ControlKind::Unknown || i.fighter_name.is_some() || i.form.is_some() || i.team.is_some() {
            return Err(bad("invalid actor identity"));
        }
        Ok(entry)
    }

    fn finish(self, truncated: bool) -> io::Result<(ReplaySession, Option<ReplayMiis>)> {
        if !self.ended || self.pending.is_some() || self.tracks.iter().any(|t| t.frames.is_empty()) { return Err(bad("empty or unfinished projection")); }
        Ok((ReplaySession { tracks: self.tracks, truncated }, self.miis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_frame::LiveTrack;
    use smash_match_data::{clip::{clip_context_for, ActorStatus, Resolved, SelectAt}, Recorder, RecorderOptions};
    use std::io::{Seek, SeekFrom, Write};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            for _ in 0..64 {
                let p = std::env::temp_dir().join(format!("training-shared-test-{}", NEXT_TEMP.fetch_add(1, Ordering::Relaxed)));
                match fs::create_dir(&p) {
                    Ok(()) => return Self(p),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(e) => panic!("fixture: {e}"),
                }
            }
            panic!("fixture collision limit");
        }
        fn archive(&self) -> SharedCaptureArchive { SharedCaptureArchive::new(&self.0) }
        fn publish(&self, s: &LiveSession) -> PathBuf {
            let outcome = self.archive().save_with_outcome("native-id", "Native title", "13.0.5", s).unwrap();
            assert!(outcome.created);
            self.0.join(format!("{}.smd", outcome.id))
        }
        fn load(&self) -> io::Result<SavedCapture> { self.archive().load(&encode_file_id("native-id")) }
    }
    impl Drop for Fixture {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    fn frame(axes: [f32;4]) -> LiveFrame {
        LiveFrame { input: LiveInput { buttons: 0x80000201, axes },
            start: StartState { speed_x: -0.0, speed_y: f32::from_bits(1), status: 14,
                ..StartState::neutral(18, 2, -1.0) },
            x: -0.0, y: f32::from_bits(2), position_z: f32::from_bits(0x80000001),
            speed_z: f32::from_bits(3), body: 91 }
    }
    fn session() -> LiveSession {
        LiveSession { tracks: vec![LiveTrack { entry: 1, frames: vec![
            frame([-0.0, f32::from_bits(1), 0.49999997 / 120.0, 2.0]),
            frame([-2.0, -0.5 / 120.0, 0.5 / 120.0, f32::from_bits(0x80000001)]),
        ] }], truncated: false, first_counter: Some(123) }
    }

    #[test]
    fn replay_miis_publish_with_the_canonical_timeline_and_cache() {
        let mut raw = vec![0; 106];
        raw[..4].copy_from_slice(&[1, 1, 1, 72]);
        raw[20] = 0x5a;
        raw[102] = 1;
        for miis in [ReplayMiis::from_bytes(&[1, 0]).unwrap(), ReplayMiis::from_bytes(&raw).unwrap()] {
            let f = Fixture::new();
            let mut s = session();
            for frame in &mut s.tracks[0].frames { frame.start.fighter_kind = 72; }
            let saved = f.archive().save_with_miis("native-id", "Native title", "13.0.5", &s, Some(&miis)).unwrap();
            assert!(saved.created);
            let dir = f.0.join(format!("{}.smd", saved.id));
            let canonical = f.load().unwrap();
            let cache = CaptureArchive::new(&dir).load(&saved.id).unwrap();
            assert_eq!(canonical.miis.as_ref(), Some(&miis));
            assert_eq!(cache.miis, canonical.miis);
            assert!(same_projection(&canonical.session, &cache.session));
            let capture = Capture::load(&dir).unwrap();
            let EventBody::Raw(event) = &capture.events[1].body else { panic!("missing Mii metadata") };
            assert_eq!(event.raw.encoding, MII_REPLAY);
            assert_eq!(event.raw.bytes().unwrap(), miis.to_bytes());
            assert_eq!(event.source_offset, None);
            let files = [MANIFEST_FILE.to_owned(), TIMELINE_FILE.to_owned(), format!("{}.tmrc", saved.id)];
            let before: Vec<_> = files.iter().map(|p| fs::read(dir.join(p)).unwrap()).collect();
            assert!(!f.archive().save_with_miis("native-id", "Native title", "13.0.5", &s, Some(&miis)).unwrap().created);
            assert!(f.archive().save_with_outcome("native-id", "Native title", "13.0.5", &s).is_err());
            for (file, bytes) in files.iter().zip(before) { assert_eq!(fs::read(dir.join(file)).unwrap(), bytes); }
        }
    }

    #[test]
    fn canonical_load_rejects_cache_that_loses_its_mii_association() {
        let f = Fixture::new();
        let s = session();
        let miis = ReplayMiis::from_bytes(&[1, 0]).unwrap();
        let saved = f.archive().save_with_miis("native-id", "Native title", "13.0.5", &s, Some(&miis)).unwrap();
        let canonical = f.load().unwrap();
        let dir = f.0.join(format!("{}.smd", saved.id));
        fs::remove_file(dir.join(format!("{}.tmrc", saved.id))).unwrap();
        CaptureArchive::new(&dir).save("native-id", "Native title", "13.0.5", &canonical.session).unwrap();
        assert!(f.load().unwrap_err().to_string().contains("Mii association mismatch"));
    }

    #[test]
    fn canonical_floats_are_bit_exact_and_projection_matches_training() {
        let f = Fixture::new(); let s = session(); let dir = f.publish(&s);
        let capture = Capture::load(&dir).unwrap();
        assert!(validate::validate_capture(&capture).iter().all(|i| !i.is_error()));
        assert_eq!(capture.completeness, Completeness::Complete);
        assert!(capture.events.iter().all(|e| e.clocks == Clocks::NONE));
        let mut input_index = 0; let mut state_index = 0;
        for e in &capture.events {
            match &e.body {
                EventBody::Input(i) => {
                    let (Channel::Observed(Stick::F32 { x: lx, y: ly }), Channel::Observed(Stick::F32 { x: rx, y: ry })) = (&i.left_stick, &i.right_stick) else { panic!("native axes missing") };
                    assert_eq!([*lx,*ly,*rx,*ry].map(f32::to_bits), s.tracks[0].frames[input_index].input.axes.map(f32::to_bits));
                    input_index += 1;
                }
                EventBody::State { sample, .. } => {
                    let Channel::Observed(k) = &sample.kinematics else { panic!("kinematics missing") };
                    let original = s.tracks[0].frames[state_index];
                    assert_eq!([k.position.x,k.position.y,k.position.z,k.facing].map(f32::to_bits),
                        [original.x,original.y,original.position_z,original.start.facing].map(f32::to_bits));
                    let velocity = k.velocity.unwrap();
                    assert_eq!([velocity.x,velocity.y,velocity.z].map(f32::to_bits),
                        [original.start.speed_x,original.start.speed_y,original.speed_z].map(f32::to_bits));
                    state_index += 1;
                }
                _ => {}
            }
        }
        assert_eq!((input_index,state_index),(2,2));
        let canonical = f.load().unwrap();
        let cache = CaptureArchive::new(dir).load(&encode_file_id("native-id")).unwrap();
        assert!(same_projection(&canonical.session, &cache.session));
        for (original, projected) in s.tracks[0].frames.iter().zip(&canonical.session.tracks[0].frames) {
            assert_eq!(original.input.project().0, projected.input);
        }
        assert!(canonical.session.tracks[0].quantized_analog);
        assert_eq!(canonical.session.tracks[0].frames[0].input.rstick_x, 0);
        assert_eq!(canonical.session.tracks[0].frames[0].input.rstick_y, 127);
        assert_eq!(canonical.session.tracks[0].frames[1].input.lstick_x, -128);
    }

    #[test]
    fn duplicate_preserves_every_published_byte_and_rejects_mismatched_identity() {
        let f = Fixture::new(); let s = session(); let dir = f.publish(&s);
        let files = [MANIFEST_FILE.to_owned(), TIMELINE_FILE.to_owned(), format!("{}.tmrc", encode_file_id("native-id"))];
        let bytes: Vec<_> = files.iter().map(|p| fs::read(dir.join(p)).unwrap()).collect();
        let mut changed = s; changed.tracks[0].frames[0].input.buttons = 0;
        assert!(!f.archive().save_with_outcome("native-id", "Native title", "13.0.5", &changed).unwrap().created);
        assert!(f.archive().save_with_outcome("native-id", "Other title", "13.0.5", &changed).is_err());
        for (p, before) in files.iter().zip(bytes) { assert_eq!(fs::read(dir.join(p)).unwrap(), before); }
    }

    #[test]
    fn legacy_capture_is_preserved_without_creating_a_shared_replacement() {
        let f = Fixture::new(); let s = session();
        let mut projected = ReplaySession::default();
        projected.tracks.push(ReplayTrack { entry: 1, frames: s.tracks[0].frames.iter().map(|v| ReplayFrame {
            input: v.input.project().0, start: v.start, x: v.x, y: v.y }).collect(), quantized_analog: true });
        let legacy = CaptureArchive::new(&f.0).save_with_outcome("native-id", "Native title", "13.0.5", &projected).unwrap();
        let path = f.0.join(format!("{}.tmrc",legacy.id)); let before = fs::read(&path).unwrap();
        assert!(!f.archive().save_with_outcome("native-id", "Native title", "13.0.5", &s).unwrap().created);
        assert_eq!(fs::read(path).unwrap(),before);
        assert!(!f.0.join(format!("{}.smd",legacy.id)).exists());
    }

    #[test]
    fn partial_capture_and_identity_transitions_survive_one_pass() {
        let f = Fixture::new(); let mut s = session();
        s.truncated = true;
        s.tracks[0].frames[1].start.fighter_kind = 19;
        let mut replacement = s.tracks[0].frames[1]; replacement.body = 92;
        replacement.start.fighter_kind = 20; replacement.start.costume = 3;
        s.tracks[0].frames.push(replacement);
        // Original roster order is retained while samples interleave by entry.
        s.tracks.insert(0, LiveTrack { entry: 7, frames: vec![frame([0.0;4])] });
        let dir = f.publish(&s); let capture = Capture::load(&dir).unwrap();
        assert!(matches!(capture.completeness, Completeness::DeclaredTruncated { .. }));
        assert!(validate::validate_capture(&capture).iter().all(|i| !i.is_error()));
        assert_eq!(capture.manifest.actors.len(),3);
        assert_eq!(capture.manifest.actors.iter().map(|a| a.form_changes).sum::<u32>(),1);
        assert!(capture.events.iter().all(|e| !matches!(e.body, EventBody::Boundary(BoundaryKind::MatchEnd))));
        let loaded = f.load().unwrap(); assert!(loaded.session.truncated);
        assert_eq!(loaded.session.tracks.iter().map(|t| t.entry).collect::<Vec<_>>(),[7,1]);
        let t = &loaded.session.tracks[1];
        assert_eq!(t.frames.iter().map(|f| (f.start.fighter_kind,f.start.costume)).collect::<Vec<_>>(),[(18,2),(19,2),(20,3)]);
        assert!(t.export(&t.window().unwrap(),"mixed","13.0.5",&[14]).is_err());
    }

    #[test]
    fn empty_nonfinite_duplicate_entry_missing_counter_and_wrap_fail_before_publication() {
        let f = Fixture::new(); let s = session();
        let mut cases = vec![LiveSession::default()];
        let mut no_counter = s.clone(); no_counter.first_counter = None; cases.push(no_counter);
        let mut wrap = s.clone(); wrap.first_counter = Some(u32::MAX); cases.push(wrap);
        let mut repeated = s.clone(); repeated.tracks.push(s.tracks[0].clone()); cases.push(repeated);
        let mut nan = s.clone(); nan.tracks[0].frames[0].speed_z = f32::NAN; cases.push(nan);
        let mut costume = s; costume.tracks[0].frames[0].start.costume = 256; cases.push(costume);
        for invalid in cases { assert!(f.archive().save_with_outcome("native-id", "Native title", "13.0.5", &invalid).is_err()); }
        assert_eq!(fs::read_dir(&f.0).unwrap().count(),0);
    }

    #[test]
    fn corrupt_cache_and_unterminated_timeline_are_rejected_without_repair() {
        let f = Fixture::new(); let dir = f.publish(&session());
        let cache = dir.join(format!("{}.tmrc",encode_file_id("native-id")));
        let before = fs::read(&cache).unwrap(); fs::write(&cache,b"bad").unwrap();
        assert!(f.load().is_err()); assert_eq!(fs::read(&cache).unwrap(),b"bad");
        fs::write(cache,before).unwrap();
        let manifest_path = dir.join(MANIFEST_FILE);
        let manifest_bytes = fs::read(&manifest_path).unwrap();
        let mut manifest = archive::read_manifest(&dir).unwrap();
        manifest.actors[0].form_changes += 1;
        fs::write(&manifest_path,serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(f.load().is_err());
        fs::write(&manifest_path,&manifest_bytes).unwrap();
        let timeline = dir.join(TIMELINE_FILE);
        let mut bytes = fs::read(&timeline).unwrap();
        let original = bytes.clone();
        // Removing a complete End line must also fail (even though every
        // remaining line passes the bounded prescan).
        let last_line = bytes[..bytes.len()-1].iter().rposition(|b| *b == b'\n').unwrap()+1;
        bytes.truncate(last_line); fs::write(&timeline,bytes).unwrap();
        assert!(f.load().is_err());
        let mut bytes = original; bytes.pop(); fs::write(timeline,bytes).unwrap();
        assert!(f.load().is_err());
    }

    #[test]
    fn reader_rejects_oversized_files_and_lines_before_shared_reader() {
        let f = Fixture::new(); let dir = f.publish(&session());
        let manifest = dir.join(MANIFEST_FILE); let before = fs::read(&manifest).unwrap();
        let file = fs::OpenOptions::new().write(true).open(&manifest).unwrap(); file.set_len(MAX_MANIFEST+1).unwrap(); drop(file);
        assert!(f.load().is_err()); fs::write(&manifest,before).unwrap();
        let timeline = dir.join(TIMELINE_FILE);
        let mut file = fs::OpenOptions::new().write(true).open(&timeline).unwrap();
        file.set_len(MAX_TIMELINE+1).unwrap(); assert!(f.load().is_err());
        file.set_len(0).unwrap(); file.seek(SeekFrom::Start(0)).unwrap(); file.write_all(&vec![b' ';MAX_LINE+1]).unwrap();
        file.write_all(b"\n").unwrap(); drop(file); assert!(f.load().is_err());
    }

    #[test]
    fn projector_rejects_missing_state_axes_and_mixed_entry() {
        let f = Fixture::new(); let dir = f.publish(&session());
        let capture = Capture::load(dir).unwrap();
        let index = capture.events.iter().position(|e| matches!(e.body,EventBody::Input(_))).unwrap();
        for mode in 0..3 {
            let mut projector = Projector::new(&capture.manifest.capture).unwrap();
            for e in &capture.events[..index-1] { projector.consume(e).unwrap(); }
            let mut state = capture.events[index-1].clone();
            if mode == 0 {
                if let EventBody::State { sample, .. } = &mut state.body { sample.kinematics = Channel::Missing; }
                assert!(projector.consume(&state).is_err()); continue;
            }
            projector.consume(&state).unwrap();
            let mut input = capture.events[index].clone();
            if let EventBody::Input(i) = &mut input.body {
                if mode == 1 { i.left_stick = Channel::Missing; } else { i.entry = Some(2); }
            }
            assert!(projector.consume(&input).is_err());
        }
    }

    /// T3: the same event list through the shared HEAD `Recorder` yields the same
    /// manifest (completion, actors, boundaries) and the same timeline bytes as
    /// this adapter's streaming writer and absent-destination manifest rewrite.
    #[test]
    fn head_recorder_oracle_reproduces_the_adapter_manifest_and_timeline() {
        for truncated in [false, true] {
            let f = Fixture::new();
            let mut s = session();
            s.truncated = truncated;
            if truncated {
                let mut replacement = s.tracks[0].frames[1]; replacement.body = 92;
                s.tracks[0].frames.push(replacement);
                s.tracks.insert(0, LiveTrack { entry: 7, frames: vec![frame([0.0; 4])] });
            }
            let dir = f.publish(&s);
            let capture = Capture::load(&dir).unwrap();
            let oracle = Fixture::new();
            let oracle_dir = oracle.0.join("oracle.smd");
            let mut recorder = Recorder::create(&oracle_dir, capture.manifest.capture.clone(),
                RecorderOptions { write: WriteOptions { flush_each_event: false, overwrite: false }, keep_events: false }).unwrap();
            let (end, body) = capture.events.split_last().unwrap();
            assert!(matches!(end.body, EventBody::End { .. }));
            for event in body {
                assert_eq!(recorder.push(event.clocks, event.body.clone()).unwrap(), event.seq);
            }
            let completion = if truncated { Completion::truncated("Training native capture was truncated") } else { Completion::complete() };
            let finished = recorder.finish(Clocks::NONE, completion).unwrap();
            assert_eq!(finished.manifest, capture.manifest, "truncated={truncated}");
            assert_eq!(finished.completeness, capture.completeness);
            assert!(finished.warnings.iter().all(|issue| !issue.is_error()));
            assert_eq!(fs::read(oracle_dir.join(TIMELINE_FILE)).unwrap(), fs::read(dir.join(TIMELINE_FILE)).unwrap());
            assert_eq!(archive::read_manifest(&oracle_dir).unwrap(), archive::read_manifest(&dir).unwrap());
        }
    }

    /// T5: at every selectable boundary the shared clip resolution, keyed by the
    /// actor's own before-state event seq, returns the known source frame, and
    /// Training's export from that boundary carries the same start context.
    #[test]
    fn selected_boundary_state_matches_source_frames_and_export_start() {
        for truncated in [false, true] {
            let f = Fixture::new();
            let mut s = session();
            s.truncated = truncated;
            // A third sample with different status, situation and momentum.
            let mut third = frame([0.25, -0.75, 0.0, 1.0]);
            third.start.status = 0; third.start.situation = 0; third.start.speed_y = -1.5; third.y = 42.0;
            s.tracks[0].frames.push(third);
            let dir = f.publish(&s);
            let capture = Capture::load(&dir).unwrap();
            assert_eq!(matches!(capture.completeness, Completeness::DeclaredTruncated { .. }), truncated);
            let loaded = f.load().unwrap();
            let track = &loaded.session.tracks[0];
            let source = &s.tracks[0].frames;
            let actor = capture.events.iter().find_map(|e| match &e.body {
                EventBody::Lifecycle { actor, event: LifecycleEvent::Spawn { identity, .. } } if identity.entry == Some(1) => Some(*actor),
                _ => None,
            }).unwrap();
            let state_seqs: Vec<u64> = capture.events.iter()
                .filter(|e| matches!(&e.body, EventBody::State { actor: a, phase: Phase::Before, .. } if *a == actor))
                .map(|e| e.seq).collect();
            assert_eq!(state_seqs.len(), source.len());
            for (k, seq) in state_seqs.iter().enumerate() {
                let expected = source[k];
                let context = clip_context_for(&capture.events, SelectAt::Seq(*seq), actor).unwrap();
                assert_eq!(context.boundaries, [BoundaryKind::RecordingStart]);
                assert_eq!(context.seq, Some(*seq));
                // The prefix holds this actor's inputs for frames 0..k only: the selected
                // before-state is included, the input it precedes is not.
                let inputs = capture.events[..context.included].iter()
                    .filter(|e| matches!(&e.body, EventBody::Input(i) if i.actor == Some(actor))).count();
                assert_eq!(inputs, k);
                let resolved = context.actor(actor).unwrap();
                assert_eq!(resolved.status, ActorStatus::Alive);
                assert_eq!(resolved.identity.fighter_kind, Some(expected.start.fighter_kind));
                assert_eq!(resolved.identity.costume, Some(expected.start.costume as u8));
                let Resolved::Observed { value: kinematics, value_seq, latest_seq, phase } = &resolved.sample.kinematics
                    else { panic!("kinematics unresolved at frame {k}") };
                assert_eq!((*value_seq, *latest_seq, *phase), (*seq, *seq, Some(Phase::Before)));
                assert_eq!([kinematics.position.x, kinematics.position.y, kinematics.position.z, kinematics.facing].map(f32::to_bits),
                    [expected.x, expected.y, expected.position_z, expected.start.facing].map(f32::to_bits));
                let velocity = kinematics.velocity.unwrap();
                assert_eq!([velocity.x, velocity.y, velocity.z].map(f32::to_bits),
                    [expected.start.speed_x, expected.start.speed_y, expected.speed_z].map(f32::to_bits));
                let Resolved::Observed { value: situation, .. } = &resolved.sample.situation
                    else { panic!("situation unresolved at frame {k}") };
                assert_eq!((situation.situation_kind, situation.status_kind), (expected.start.situation, expected.start.status));
                // Training's export from the same boundary carries the same start context
                // and the exact selected inputs.
                let mut window = track.window().unwrap();
                window.set_in(k as u32);
                let exported = track.export(&window, "boundary", "13.0.5", &[expected.start.status]).unwrap();
                assert_eq!(exported.frames.len(), source.len() - k);
                assert_eq!(exported.frames[0], expected.input.project().0);
                let start = exported.start;
                assert_eq!((start.fighter_kind, start.costume, start.situation, start.status),
                    (expected.start.fighter_kind, expected.start.costume, expected.start.situation, expected.start.status));
                assert_eq!([start.facing, start.speed_x, start.speed_y].map(f32::to_bits),
                    [kinematics.facing, velocity.x, velocity.y].map(f32::to_bits));
                assert!(track.export(&window, "boundary", "13.0.5", &[expected.start.status + 1]).is_err());
            }
        }
    }
}
