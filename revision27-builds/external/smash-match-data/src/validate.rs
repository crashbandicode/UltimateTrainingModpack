//! Rule checks shared by the recorder (fail fast) and offline validation
//! (collect everything). One [`StreamChecker`] consumes events in order and keeps
//! the actor/object/boundary state needed to judge the next one.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCode {
    Format,
    EmptyCaptureId,
    EmptyGameTitle,
    InvalidInputLayout,
    DuplicateInputLayout,
    DuplicateEntry,
    DuplicateController,
    UnknownController,
    UndeclaredClock,
    SeqOrder,
    ClockRegression,
    UnknownActor,
    ActorReused,
    LifecycleOrder,
    UnknownObject,
    ObjectReused,
    UnsupportedChannelObserved,
    UndeclaredUnsupported,
    UnknownInputLayout,
    LayoutMismatch,
    PhaseOrder,
    InvalidRawBytes,
    NonFinite,
    BoundaryOrder,
    EventAfterEnd,
    DuplicateEnd,
    EndCountMismatch,
    MissingEnd,
    Completeness,
    ManifestMismatch,
    RulesTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub severity: Severity,
    pub code: IssueCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    pub message: String,
}

impl Issue {
    pub fn error(code: IssueCode, seq: Option<u64>, message: impl Into<String>) -> Self {
        Issue {
            severity: Severity::Error,
            code,
            seq,
            message: message.into(),
        }
    }

    pub fn warning(code: IssueCode, seq: Option<u64>, message: impl Into<String>) -> Self {
        Issue {
            severity: Severity::Warning,
            code,
            seq,
            message: message.into(),
        }
    }

    pub fn info(code: IssueCode, seq: Option<u64>, message: impl Into<String>) -> Self {
        Issue {
            severity: Severity::Info,
            code,
            seq,
            message: message.into(),
        }
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        match self.seq {
            Some(seq) => write!(
                f,
                "{severity} at seq {seq}: {} [{:?}]",
                self.message, self.code
            ),
            None => write!(f, "{severity}: {} [{:?}]", self.message, self.code),
        }
    }
}

impl std::error::Error for Issue {}

pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(Issue::is_error)
}

// ---------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------

pub fn validate_header(header: &CaptureHeader) -> Vec<Issue> {
    let mut issues = Vec::new();
    if header.capture_id.trim().is_empty() {
        issues.push(Issue::error(
            IssueCode::EmptyCaptureId,
            None,
            "capture_id must not be empty",
        ));
    }
    if header.game.title.trim().is_empty() {
        issues.push(Issue::error(
            IssueCode::EmptyGameTitle,
            None,
            "game.title must not be empty",
        ));
    }

    let mut layouts = BTreeSet::new();
    for layout in &header.input_layouts {
        if layout.id.trim().is_empty() {
            issues.push(Issue::error(
                IssueCode::InvalidInputLayout,
                None,
                "input layout id must not be empty",
            ));
        } else if !layouts.insert(layout.id.as_str()) {
            issues.push(Issue::error(
                IssueCode::DuplicateInputLayout,
                None,
                format!("input layout id {:?} declared twice", layout.id),
            ));
        }
        for (name, axis) in [
            ("left_stick", &layout.left_stick),
            ("right_stick", &layout.right_stick),
        ] {
            for (field, value) in [
                ("min", axis.min),
                ("max", axis.max),
                ("neutral", axis.neutral),
                ("float_scale", axis.float_scale),
            ] {
                if value.is_some_and(|v| !v.is_finite()) {
                    issues.push(Issue::error(
                        IssueCode::NonFinite,
                        None,
                        format!("input layout {:?} {name}.{field} is not finite", layout.id),
                    ));
                }
            }
        }
    }

    let mut entries = BTreeSet::new();
    let controllers: BTreeSet<u8> = header.controllers.iter().map(|c| c.slot).collect();
    if controllers.len() != header.controllers.len() {
        issues.push(Issue::error(
            IssueCode::DuplicateController,
            None,
            "controller slot declared twice",
        ));
    }
    for entry in &header.entries {
        if !entries.insert(entry.entry) {
            issues.push(Issue::error(
                IssueCode::DuplicateEntry,
                None,
                format!("fighter entry {} declared twice", entry.entry),
            ));
        }
        if let Some(slot) = entry.controller {
            if !header.controllers.is_empty() && !controllers.contains(&slot) {
                issues.push(Issue::warning(
                    IssueCode::UnknownController,
                    None,
                    format!(
                        "entry {} references undeclared controller slot {slot}",
                        entry.entry
                    ),
                ));
            }
        }
    }

    for (field, value) in [
        ("damage_ratio", header.rules.actual.damage_ratio),
        ("launch_rate", header.rules.actual.launch_rate),
    ] {
        if value.is_some_and(|v| !v.is_finite()) {
            issues.push(Issue::error(
                IssueCode::NonFinite,
                None,
                format!("rules.actual.{field} is not finite"),
            ));
        }
    }
    if let Some(raw) = &header.rules.actual.raw_setup {
        if let Err(err) = raw.bytes() {
            issues.push(Issue::error(
                IssueCode::InvalidRawBytes,
                None,
                format!("rules.actual.raw_setup: {err}"),
            ));
        }
    }

    for relation in &header.clocks.relations {
        for kind in [relation.a, relation.b] {
            if !header.clocks.supplied.contains(&kind) {
                issues.push(Issue::warning(
                    IssueCode::UndeclaredClock,
                    None,
                    format!("clock relation references unsupplied clock {kind}"),
                ));
            }
        }
    }
    issues
}

// ---------------------------------------------------------------------------
// Competitive target
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conformance {
    Conforms,
    Deviates {
        reasons: Vec<String>,
    },
    /// No deviation seen, but the decisive settings were not recorded.
    Undetermined {
        unknown: Vec<String>,
    },
}

/// Compare actual rules against the competitive target: match items off,
/// stage hazards off. Fields the producer could not read stay unknown; they
/// are never assumed to conform.
pub fn competitive_conformance(actual: &ActualRules) -> Conformance {
    let mut reasons = Vec::new();
    let mut unknown = Vec::new();
    match actual.items {
        Some(ItemSetting::Off) => {}
        Some(ItemSetting::On) => reasons.push("match items are on".to_string()),
        None => unknown.push("items".to_string()),
    }
    match actual.stage_hazards {
        Some(false) => {}
        Some(true) => reasons.push("stage hazards are on".to_string()),
        None => unknown.push("stage_hazards".to_string()),
    }
    if !reasons.is_empty() {
        Conformance::Deviates { reasons }
    } else if !unknown.is_empty() {
        Conformance::Undetermined { unknown }
    } else {
        Conformance::Conforms
    }
}

// ---------------------------------------------------------------------------
// Stream checker
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Liveness {
    Alive,
    Dead,
    Despawned,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorTrack {
    /// Identity at spawn.
    pub spawn_identity: ActorIdentity,
    /// Current identity (after form changes).
    pub identity: ActorIdentity,
    pub spawn_seq: u64,
    pub liveness: Liveness,
    pub despawn_seq: Option<u64>,
    pub deaths: u32,
    pub respawns: u32,
    pub form_changes: u32,
}

impl ActorTrack {
    pub fn summary(&self, actor: ActorId) -> ActorSummary {
        ActorSummary {
            actor,
            identity: self.spawn_identity.clone(),
            spawn_seq: self.spawn_seq,
            despawn_seq: self.despawn_seq,
            deaths: self.deaths,
            respawns: self.respawns,
            form_changes: self.form_changes,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectTrack {
    pub identity: ObjectIdentity,
    pub spawn_seq: u64,
    pub despawn_seq: Option<u64>,
}

/// Sequential rule checker. `check` reports every issue for one event and only
/// updates its state when the event has no error.
#[derive(Clone, Debug)]
pub struct StreamChecker {
    channels: ChannelSupport,
    layouts: BTreeMap<String, InputLayout>,
    /// Last state sample phase per actor with its tick, for phase ordering.
    last_phase: BTreeMap<ActorId, (u64, Phase)>,
    last_seq: Option<u64>,
    clock_max: [Option<u64>; 5],
    actors: BTreeMap<ActorId, ActorTrack>,
    objects: BTreeMap<ObjectId, ObjectTrack>,
    boundaries: Vec<BoundaryMark>,
    end_seq: Option<u64>,
    accepted: u64,
}

impl StreamChecker {
    pub fn new(header: &CaptureHeader) -> Self {
        StreamChecker {
            channels: header.channels.clone(),
            layouts: header
                .input_layouts
                .iter()
                .map(|l| (l.id.clone(), l.clone()))
                .collect(),
            last_phase: BTreeMap::new(),
            last_seq: None,
            clock_max: [None; 5],
            actors: BTreeMap::new(),
            objects: BTreeMap::new(),
            boundaries: Vec::new(),
            end_seq: None,
            accepted: 0,
        }
    }

    pub fn actors(&self) -> &BTreeMap<ActorId, ActorTrack> {
        &self.actors
    }

    pub fn objects(&self) -> &BTreeMap<ObjectId, ObjectTrack> {
        &self.objects
    }

    pub fn boundaries(&self) -> &[BoundaryMark] {
        &self.boundaries
    }

    pub fn actor_summaries(&self) -> Vec<ActorSummary> {
        self.actors
            .iter()
            .map(|(actor, track)| track.summary(*actor))
            .collect()
    }

    /// Events accepted so far, including the `end` event once seen.
    pub fn accepted(&self) -> u64 {
        self.accepted
    }

    pub fn last_seq(&self) -> Option<u64> {
        self.last_seq
    }

    pub fn ended(&self) -> bool {
        self.end_seq.is_some()
    }

    /// Check one event. Errors leave the checker state untouched.
    pub fn check(&mut self, event: &Event) -> Vec<Issue> {
        let mut issues = Vec::new();
        let seq = Some(event.seq);
        let err = |issues: &mut Vec<Issue>, code, message: String| {
            issues.push(Issue::error(code, seq, message));
        };

        if let Some(last) = self.last_seq {
            if event.seq <= last {
                err(
                    &mut issues,
                    IssueCode::SeqOrder,
                    format!("seq {} does not increase after {last}", event.seq),
                );
            }
        }
        if let Some(end) = self.end_seq {
            err(
                &mut issues,
                IssueCode::EventAfterEnd,
                format!("event after the end event at seq {end}"),
            );
        }
        for (index, kind) in ClockKind::ALL.iter().enumerate() {
            if let (Some(value), Some(max)) = (event.clocks.get(*kind), self.clock_max[index]) {
                if value < max {
                    err(
                        &mut issues,
                        IssueCode::ClockRegression,
                        format!("clock {kind} went back from {max} to {value}"),
                    );
                }
            }
        }
        if let Some(field) = first_non_finite(&event.body) {
            err(
                &mut issues,
                IssueCode::NonFinite,
                format!("{field} is not finite"),
            );
        }

        match &event.body {
            EventBody::Boundary(kind) => {
                for mark in &self.boundaries {
                    if mark.boundary == *kind {
                        err(
                            &mut issues,
                            IssueCode::BoundaryOrder,
                            format!("boundary {kind:?} already recorded at seq {}", mark.seq),
                        );
                    } else if mark.boundary.order() > kind.order() {
                        err(
                            &mut issues,
                            IssueCode::BoundaryOrder,
                            format!("boundary {kind:?} after {:?}", mark.boundary),
                        );
                    }
                }
            }
            EventBody::Pace(_) | EventBody::Note { .. } => {}
            EventBody::Raw(unit) => {
                if let Err(error) = unit.raw.bytes() {
                    err(
                        &mut issues,
                        IssueCode::InvalidRawBytes,
                        format!("raw source unit: {error}"),
                    );
                }
            }
            EventBody::Lifecycle { actor, event: life } => {
                if self.channels.lifecycle.is_unsupported() {
                    err(
                        &mut issues,
                        IssueCode::UnsupportedChannelObserved,
                        "lifecycle event on a capture that declares lifecycle unsupported"
                            .to_string(),
                    );
                }
                match life {
                    LifecycleEvent::Spawn { .. } => {
                        if self.actors.contains_key(actor) {
                            err(
                                &mut issues,
                                IssueCode::ActorReused,
                                format!("{actor} spawned twice; actor ids are never reused"),
                            );
                        }
                    }
                    other => match self.actors.get(actor) {
                        None => err(
                            &mut issues,
                            IssueCode::UnknownActor,
                            format!("{actor} has not spawned"),
                        ),
                        Some(track) => {
                            let allowed = match other.kind() {
                                LifecycleKind::Death => track.liveness == Liveness::Alive,
                                LifecycleKind::Respawn => track.liveness == Liveness::Dead,
                                LifecycleKind::Despawn | LifecycleKind::FormChange => {
                                    track.liveness != Liveness::Despawned
                                }
                                LifecycleKind::Spawn => unreachable!(),
                            };
                            if !allowed {
                                err(
                                    &mut issues,
                                    IssueCode::LifecycleOrder,
                                    format!(
                                        "{actor} cannot {:?} while {:?}",
                                        other.kind(),
                                        track.liveness
                                    ),
                                );
                            }
                        }
                    },
                }
            }
            EventBody::State {
                actor,
                phase,
                sample,
            } => {
                self.check_live_actor(*actor, &mut issues, seq);
                if let (Some(tick), Some((last_tick, Phase::After))) =
                    (event.clocks.tick, self.last_phase.get(actor).copied())
                {
                    if *phase == Phase::Before && tick == last_tick {
                        err(
                            &mut issues,
                            IssueCode::PhaseOrder,
                            format!("{actor} before-sample after its after-sample at tick {tick}"),
                        );
                    }
                }
                self.check_channel(
                    "kinematics",
                    &self.channels.kinematics,
                    &sample.kinematics,
                    &mut issues,
                    seq,
                );
                self.check_channel(
                    "situation",
                    &self.channels.situation,
                    &sample.situation,
                    &mut issues,
                    seq,
                );
                self.check_channel(
                    "animation",
                    &self.channels.animation,
                    &sample.animation,
                    &mut issues,
                    seq,
                );
                self.check_channel(
                    "vitals",
                    &self.channels.vitals,
                    &sample.vitals,
                    &mut issues,
                    seq,
                );
            }
            EventBody::Input(input) => {
                match self.layouts.get(&input.layout) {
                    None => err(
                        &mut issues,
                        IssueCode::UnknownInputLayout,
                        format!(
                            "input layout {:?} is not declared in the manifest",
                            input.layout
                        ),
                    ),
                    Some(layout) => {
                        for (name, stick, axis) in [
                            ("left_stick", &input.left_stick, &layout.left_stick),
                            ("right_stick", &input.right_stick, &layout.right_stick),
                        ] {
                            let encoding = match stick.observed() {
                                Some(Stick::F32 { .. }) => AxisEncoding::F32,
                                Some(Stick::I8 { .. }) => AxisEncoding::I8,
                                None => continue,
                            };
                            if encoding != axis.encoding {
                                err(
                                    &mut issues,
                                    IssueCode::LayoutMismatch,
                                    format!(
                                        "{name} is {encoding:?} but layout {:?} declares {:?}",
                                        input.layout, axis.encoding
                                    ),
                                );
                            }
                        }
                        if let (Some(raw), Some(expected)) = (&input.raw, &layout.raw_encoding) {
                            if raw.encoding != *expected {
                                err(
                                    &mut issues,
                                    IssueCode::LayoutMismatch,
                                    format!(
                                        "raw encoding {:?} but layout {:?} declares {expected:?}",
                                        raw.encoding, input.layout
                                    ),
                                );
                            }
                        }
                    }
                }
                if let Some(raw) = &input.raw {
                    if let Err(e) = raw.bytes() {
                        err(
                            &mut issues,
                            IssueCode::InvalidRawBytes,
                            format!("input raw bytes: {e}"),
                        );
                    }
                }
                if let Some(actor) = input.actor {
                    self.check_live_actor(actor, &mut issues, seq);
                }
                self.check_channel(
                    "buttons",
                    &self.channels.inputs,
                    &input.buttons,
                    &mut issues,
                    seq,
                );
                self.check_channel(
                    "left_stick",
                    &self.channels.inputs,
                    &input.left_stick,
                    &mut issues,
                    seq,
                );
                self.check_channel(
                    "right_stick",
                    &self.channels.inputs,
                    &input.right_stick,
                    &mut issues,
                    seq,
                );
            }
            EventBody::Object { object, event: obj } => {
                if self.channels.objects.is_unsupported() {
                    err(
                        &mut issues,
                        IssueCode::UnsupportedChannelObserved,
                        "object event on a capture that declares objects unsupported".to_string(),
                    );
                }
                match obj {
                    ObjectEvent::Spawn {
                        identity,
                        transform,
                        animation,
                    } => {
                        if self.objects.contains_key(object) {
                            err(
                                &mut issues,
                                IssueCode::ObjectReused,
                                format!("{object} spawned twice; object ids are never reused"),
                            );
                        }
                        if let Some(owner) = identity.owner {
                            self.check_live_actor(owner, &mut issues, seq);
                        }
                        self.check_channel(
                            "transform",
                            &self.channels.objects,
                            transform,
                            &mut issues,
                            seq,
                        );
                        self.check_channel(
                            "animation",
                            &self.channels.objects,
                            animation,
                            &mut issues,
                            seq,
                        );
                    }
                    ObjectEvent::State {
                        transform,
                        animation,
                    } => {
                        self.check_live_object(*object, &mut issues, seq);
                        self.check_channel(
                            "transform",
                            &self.channels.objects,
                            transform,
                            &mut issues,
                            seq,
                        );
                        self.check_channel(
                            "animation",
                            &self.channels.objects,
                            animation,
                            &mut issues,
                            seq,
                        );
                    }
                    ObjectEvent::Despawn { .. } => {
                        self.check_live_object(*object, &mut issues, seq)
                    }
                }
            }
            EventBody::Random(random) => {
                if self.channels.random.is_unsupported() {
                    err(
                        &mut issues,
                        IssueCode::UnsupportedChannelObserved,
                        "random event on a capture that declares random unsupported".to_string(),
                    );
                }
                if let Some(actor) = random.actor {
                    if !self.actors.contains_key(&actor) {
                        err(
                            &mut issues,
                            IssueCode::UnknownActor,
                            format!("{actor} has not spawned"),
                        );
                    }
                }
            }
            EventBody::Gap {
                actor: Some(actor), ..
            } => {
                if !self.actors.contains_key(actor) {
                    err(
                        &mut issues,
                        IssueCode::UnknownActor,
                        format!("{actor} has not spawned"),
                    );
                }
            }
            EventBody::Gap { actor: None, .. } => {}
            EventBody::End { events } => {
                if self.end_seq.is_some() {
                    err(
                        &mut issues,
                        IssueCode::DuplicateEnd,
                        "second end event".to_string(),
                    );
                } else if *events != self.accepted {
                    err(
                        &mut issues,
                        IssueCode::EndCountMismatch,
                        format!(
                            "end declares {events} events but {} precede it",
                            self.accepted
                        ),
                    );
                }
            }
        }

        if !has_errors(&issues) {
            self.apply(event);
        }
        issues
    }

    fn check_live_actor(&self, actor: ActorId, issues: &mut Vec<Issue>, seq: Option<u64>) {
        match self.actors.get(&actor) {
            None => issues.push(Issue::error(
                IssueCode::UnknownActor,
                seq,
                format!("{actor} has not spawned"),
            )),
            Some(track) if track.liveness == Liveness::Despawned => issues.push(Issue::error(
                IssueCode::LifecycleOrder,
                seq,
                format!(
                    "{actor} despawned at seq {}",
                    track.despawn_seq.unwrap_or_default()
                ),
            )),
            Some(_) => {}
        }
    }

    fn check_live_object(&self, object: ObjectId, issues: &mut Vec<Issue>, seq: Option<u64>) {
        match self.objects.get(&object) {
            None => issues.push(Issue::error(
                IssueCode::UnknownObject,
                seq,
                format!("{object} has not spawned"),
            )),
            Some(track) if track.despawn_seq.is_some() => issues.push(Issue::error(
                IssueCode::LifecycleOrder,
                seq,
                format!(
                    "{object} despawned at seq {}",
                    track.despawn_seq.unwrap_or_default()
                ),
            )),
            Some(_) => {}
        }
    }

    fn check_channel<T>(
        &self,
        name: &str,
        declared: &Supply,
        channel: &Channel<T>,
        issues: &mut Vec<Issue>,
        seq: Option<u64>,
    ) {
        match (declared.level, channel) {
            (SupplyLevel::Unsupported, Channel::Observed(_)) => issues.push(Issue::error(
                IssueCode::UnsupportedChannelObserved,
                seq,
                format!("{name} observed but declared unsupported in the manifest"),
            )),
            (SupplyLevel::Unsupported, Channel::Unchanged | Channel::Missing) => {
                issues.push(Issue::warning(
                    IssueCode::UndeclaredUnsupported,
                    seq,
                    format!(
                        "{name} marked {} but declared unsupported",
                        channel.marker().unwrap_or_default()
                    ),
                ))
            }
            (SupplyLevel::Full, Channel::Unsupported) => issues.push(Issue::warning(
                IssueCode::UndeclaredUnsupported,
                seq,
                format!("{name} marked unsupported but declared full"),
            )),
            _ => {}
        }
    }

    fn apply(&mut self, event: &Event) {
        self.last_seq = Some(event.seq);
        self.accepted += 1;
        for (index, kind) in ClockKind::ALL.iter().enumerate() {
            if let Some(value) = event.clocks.get(*kind) {
                self.clock_max[index] = Some(value);
            }
        }
        match &event.body {
            EventBody::Boundary(kind) => self.boundaries.push(BoundaryMark {
                boundary: *kind,
                seq: event.seq,
                clocks: event.clocks,
            }),
            EventBody::Lifecycle { actor, event: life } => match life {
                LifecycleEvent::Spawn { identity, .. } => {
                    self.actors.insert(
                        *actor,
                        ActorTrack {
                            spawn_identity: identity.clone(),
                            identity: identity.clone(),
                            spawn_seq: event.seq,
                            liveness: Liveness::Alive,
                            despawn_seq: None,
                            deaths: 0,
                            respawns: 0,
                            form_changes: 0,
                        },
                    );
                }
                other => {
                    if let Some(track) = self.actors.get_mut(actor) {
                        match other {
                            LifecycleEvent::Death { .. } => {
                                track.liveness = Liveness::Dead;
                                track.deaths += 1;
                            }
                            LifecycleEvent::Respawn => {
                                track.liveness = Liveness::Alive;
                                track.respawns += 1;
                            }
                            LifecycleEvent::Despawn { .. } => {
                                track.liveness = Liveness::Despawned;
                                track.despawn_seq = Some(event.seq);
                            }
                            LifecycleEvent::FormChange { identity } => {
                                track.identity = identity.clone();
                                track.form_changes += 1;
                            }
                            LifecycleEvent::Spawn { .. } => {}
                        }
                    }
                }
            },
            EventBody::Object { object, event: obj } => match obj {
                ObjectEvent::Spawn { identity, .. } => {
                    self.objects.insert(
                        *object,
                        ObjectTrack {
                            identity: identity.clone(),
                            spawn_seq: event.seq,
                            despawn_seq: None,
                        },
                    );
                }
                ObjectEvent::Despawn { .. } => {
                    if let Some(track) = self.objects.get_mut(object) {
                        track.despawn_seq = Some(event.seq);
                    }
                }
                ObjectEvent::State { .. } => {}
            },
            EventBody::State { actor, phase, .. } => {
                if let Some(tick) = event.clocks.tick {
                    self.last_phase.insert(*actor, (tick, *phase));
                }
            }
            EventBody::End { .. } => self.end_seq = Some(event.seq),
            _ => {}
        }
    }
}

/// Path of the first non-finite float in an event body, if any.
pub fn first_non_finite(body: &EventBody) -> Option<String> {
    fn animation(prefix: &str, animation: &Animation) -> Option<String> {
        if !animation.frame.is_finite() {
            return Some(format!("{prefix}.frame"));
        }
        if !animation.rate.is_finite() {
            return Some(format!("{prefix}.rate"));
        }
        if animation.root_rotation.is_some_and(|r| !r.is_finite()) {
            return Some(format!("{prefix}.root_rotation"));
        }
        if animation.root_scale.is_some_and(|s| !s.is_finite()) {
            return Some(format!("{prefix}.root_scale"));
        }
        if let Some(secondary) = &animation.secondary {
            if !secondary.frame.is_finite() || !secondary.weight.is_finite() {
                return Some(format!("{prefix}.secondary"));
            }
        }
        None
    }
    fn transform(prefix: &str, transform: &Transform) -> Option<String> {
        if !transform.position.is_finite() {
            return Some(format!("{prefix}.position"));
        }
        if transform.rotation.is_some_and(|r| !r.is_finite()) {
            return Some(format!("{prefix}.rotation"));
        }
        if transform.scale.is_some_and(|s| !s.is_finite()) {
            return Some(format!("{prefix}.scale"));
        }
        if transform.velocity.is_some_and(|v| !v.is_finite()) {
            return Some(format!("{prefix}.velocity"));
        }
        None
    }
    match body {
        EventBody::Pace(pace) => pace
            .speed
            .is_some_and(|s| !s.is_finite())
            .then(|| "pace.speed".to_string()),
        EventBody::State { sample, .. } => {
            if let Channel::Observed(k) = &sample.kinematics {
                if !k.position.is_finite() {
                    return Some("kinematics.position".to_string());
                }
                if !k.facing.is_finite() {
                    return Some("kinematics.facing".to_string());
                }
                if k.velocity.is_some_and(|v| !v.is_finite()) {
                    return Some("kinematics.velocity".to_string());
                }
            }
            if let Channel::Observed(s) = &sample.situation {
                if s.hitstop_frames.is_some_and(|h| !h.is_finite()) {
                    return Some("situation.hitstop_frames".to_string());
                }
            }
            if let Channel::Observed(a) = &sample.animation {
                if let Some(path) = animation("animation", a) {
                    return Some(path);
                }
            }
            if let Channel::Observed(v) = &sample.vitals {
                if !v.damage.is_finite() {
                    return Some("vitals.damage".to_string());
                }
                if v.shield.is_some_and(|s| !s.is_finite()) {
                    return Some("vitals.shield".to_string());
                }
            }
            None
        }
        EventBody::Input(input) => {
            if input.left_stick.observed().is_some_and(|s| !s.is_finite()) {
                return Some("input.left_stick".to_string());
            }
            if input.right_stick.observed().is_some_and(|s| !s.is_finite()) {
                return Some("input.right_stick".to_string());
            }
            None
        }
        EventBody::Object { event, .. } => {
            let (t, a) = match event {
                ObjectEvent::Spawn {
                    transform,
                    animation,
                    ..
                }
                | ObjectEvent::State {
                    transform,
                    animation,
                } => (transform, animation),
                ObjectEvent::Despawn { .. } => return None,
            };
            if let Some(t) = t.observed() {
                if let Some(path) = transform("object.transform", t) {
                    return Some(path);
                }
            }
            if let Some(a) = a.observed() {
                if let Some(path) = animation("object.animation", a) {
                    return Some(path);
                }
            }
            None
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Whole capture
// ---------------------------------------------------------------------------

pub fn validate_capture(capture: &Capture) -> Vec<Issue> {
    let manifest = &capture.manifest;
    let mut issues = Vec::new();
    if manifest.format != FORMAT_NAME {
        issues.push(Issue::error(
            IssueCode::Format,
            None,
            format!("format is {:?}, expected {FORMAT_NAME:?}", manifest.format),
        ));
    }
    if !FORMAT_VERSION.can_read(manifest.format_version) {
        issues.push(Issue::error(
            IssueCode::Format,
            None,
            format!(
                "format version {} is not readable by {FORMAT_VERSION}",
                manifest.format_version
            ),
        ));
    }
    issues.extend(validate_header(&manifest.capture));

    let mut checker = StreamChecker::new(&manifest.capture);
    for event in &capture.events {
        issues.extend(checker.check(event));
    }

    match &capture.completeness {
        Completeness::Complete => {
            if !checker.ended() {
                issues.push(Issue::error(
                    IssueCode::MissingEnd,
                    None,
                    "capture claims complete but the timeline has no end event",
                ));
            }
        }
        other @ Completeness::DeclaredTruncated { .. } => {
            issues.push(Issue::info(
                IssueCode::Completeness,
                None,
                other.to_string(),
            ));
        }
        other @ Completeness::Incomplete { .. } => {
            issues.push(Issue::warning(
                IssueCode::Completeness,
                None,
                other.to_string(),
            ));
        }
    }

    if manifest.completion.state != CompletionState::Open {
        if manifest.actors != checker.actor_summaries() {
            issues.push(Issue::warning(
                IssueCode::ManifestMismatch,
                None,
                "manifest actor summary differs from the timeline",
            ));
        }
        if manifest.boundaries != checker.boundaries() {
            issues.push(Issue::warning(
                IssueCode::ManifestMismatch,
                None,
                "manifest boundaries differ from the timeline",
            ));
        }
        if let Some(declared) = manifest.completion.events {
            if declared != capture.events.len() as u64 {
                issues.push(Issue::warning(
                    IssueCode::ManifestMismatch,
                    None,
                    format!(
                        "manifest declares {declared} events but the capture holds {}",
                        capture.events.len()
                    ),
                ));
            }
        }
    }

    if manifest.capture.rules.target == RulesetTarget::Competitive {
        match competitive_conformance(&manifest.capture.rules.actual) {
            Conformance::Conforms => {}
            Conformance::Deviates { reasons } => issues.push(Issue::warning(
                IssueCode::RulesTarget,
                None,
                format!(
                    "actual rules deviate from the competitive target: {}",
                    reasons.join(", ")
                ),
            )),
            Conformance::Undetermined { unknown } => issues.push(Issue::info(
                IssueCode::RulesTarget,
                None,
                format!(
                    "competitive conformance undetermined; unknown: {}",
                    unknown.join(", ")
                ),
            )),
        }
    }
    issues
}

impl Capture {
    pub fn validate(&self) -> Vec<Issue> {
        validate_capture(self)
    }
}
