//! Selected-frame and clip-start state resolution.
//!
//! A selection point includes a prefix of the timeline. For every actor that
//! has spawned, the state at that point is resolved per channel from samples in
//! the actor's **current lifetime segment** only (the stretch since its last
//! lifecycle event). `unchanged` walks back to a real observation; a `missing`
//! sample, a `gap` or the segment start stops that walk and the channel resolves
//! to missing with the reason. Nothing is carried implicitly.

use crate::model::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectAt {
    /// Everything with `seq <= n`.
    Seq(u64),
    /// Everything before the first `input` of gameplay tick `t` (so the start
    /// state is the `before` sample of that tick); all of tick `t` if it has no
    /// input event. [`selection_end`] uses the first input from any fighter.
    /// [`selection_end_for`] uses only the named actor's inputs.
    Tick(u64),
    /// Same rule as `Tick`, keyed on the saved-replay record clock.
    Record(u64),
}

/// Exclusive end index of the events included by `at`. `None` when a tick or
/// record value does not occur in the timeline.
///
/// Tick and record cuts use the first input from any fighter. Actor-selected
/// clips use [`selection_end_for`] so one fighter's input does not hide
/// another's before-state. `Seq` is the same in both functions.
pub fn selection_end(events: &[Event], at: SelectAt) -> Option<usize> {
    selection_end_scoped(events, at, None)
}

/// Exclusive end index of `at` for one actor.
///
/// `Seq` ignores `actor`. Tick and record stop before that actor's own first
/// input of the clock value, or after the last event of that value when the
/// actor has no input there. An input with no actor id is not that actor's
/// input; controller numbers are not mapped to actors.
pub fn selection_end_for(events: &[Event], at: SelectAt, actor: ActorId) -> Option<usize> {
    selection_end_scoped(events, at, Some(actor))
}

fn selection_end_scoped(events: &[Event], at: SelectAt, actor: Option<ActorId>) -> Option<usize> {
    match at {
        SelectAt::Seq(seq) => Some(
            events
                .iter()
                .rposition(|e| e.seq <= seq)
                .map_or(0, |i| i + 1),
        ),
        SelectAt::Tick(tick) => clock_selection_end(events, ClockKind::Tick, tick, actor),
        SelectAt::Record(record) => clock_selection_end(events, ClockKind::Record, record, actor),
    }
}

fn clock_selection_end(
    events: &[Event],
    kind: ClockKind,
    value: u64,
    actor: Option<ActorId>,
) -> Option<usize> {
    let first = events
        .iter()
        .position(|e| e.clocks.get(kind) == Some(value))?;
    let after_last = events
        .iter()
        .rposition(|e| e.clocks.get(kind) == Some(value))
        .map_or(first, |i| i + 1);
    let first_input = events[first..after_last]
        .iter()
        .position(|event| input_defines_cut(&event.body, actor))
        .map(|offset| first + offset);
    Some(first_input.unwrap_or(after_last))
}

fn input_defines_cut(body: &EventBody, actor: Option<ActorId>) -> bool {
    match (body, actor) {
        (EventBody::Input(_), None) => true,
        (EventBody::Input(input), Some(actor)) => input.actor == Some(actor),
        _ => false,
    }
}

/// Events strictly inside a clip: from the start selection's end up to the end
/// selection's end. With unscoped `Tick` selections the clip begins at the first
/// input of the start tick (its `before` samples belong to the context) and ends
/// with the `before` samples of the end tick. Reversed intervals return `None`.
pub fn clip_events(events: &[Event], start: SelectAt, end: SelectAt) -> Option<&[Event]> {
    let from = selection_end(events, start)?;
    let to = selection_end(events, end)?;
    events.get(from..to)
}

/// [`clip_events`] using [`selection_end_for`] at both ends.
///
/// The selected actor's own input defines each tick or record cut. `Seq` is
/// unchanged. A reversed interval returns `None`.
pub fn clip_events_for(
    events: &[Event],
    start: SelectAt,
    end: SelectAt,
    actor: ActorId,
) -> Option<&[Event]> {
    let from = selection_end_for(events, start, actor)?;
    let to = selection_end_for(events, end, actor)?;
    events.get(from..to)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MissingReason {
    /// The latest sample said `missing`.
    SampleMissing { seq: u64 },
    /// The latest sample said `unchanged` but no valid predecessor exists in
    /// this lifetime segment.
    UnchangedWithoutPredecessor { seq: u64 },
    /// A gap was recorded between the latest sample and the selection point.
    GapAfterLatestSample { gap_seq: u64 },
    /// A gap breaks the chain between an `unchanged` sample and its predecessor.
    GapBeforePredecessor { gap_seq: u64 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Resolved<T> {
    Observed {
        value: T,
        /// Sample that actually held the value.
        value_seq: u64,
        /// Latest sample for the channel (equals `value_seq` unless carried).
        latest_seq: u64,
        /// Phase of the sample holding the value; `None` for objects.
        phase: Option<Phase>,
    },
    Missing(MissingReason),
    Unsupported,
    /// No sample at all in the current lifetime segment.
    NoSample,
}

impl<T> Resolved<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Resolved::Observed { value, .. } => Some(value),
            _ => None,
        }
    }

    /// True when the value was carried from an earlier sample via `unchanged`.
    pub fn is_carried(&self) -> bool {
        matches!(self, Resolved::Observed { value_seq, latest_seq, .. } if value_seq != latest_seq)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActorStatus {
    Alive,
    Dead,
    Despawned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Segment {
    pub start_seq: u64,
    pub boundary: LifecycleKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSample {
    pub kinematics: Resolved<Kinematics>,
    pub situation: Resolved<Situation>,
    pub animation: Resolved<Animation>,
    pub vitals: Resolved<Vitals>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedActor {
    pub actor: ActorId,
    /// Current identity (after form changes).
    pub identity: ActorIdentity,
    pub status: ActorStatus,
    pub segment: Segment,
    pub sample: ResolvedSample,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedObject {
    pub object: ObjectId,
    pub identity: ObjectIdentity,
    pub spawn_seq: u64,
    pub transform: Resolved<Transform>,
    pub animation: Resolved<Animation>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipContext {
    pub at: SelectAt,
    /// `events[..included]` are at or before the selection point.
    pub included: usize,
    /// Seq and clocks of the last included event.
    pub seq: Option<u64>,
    pub clocks: Clocks,
    /// Boundaries passed before the selection point, in order.
    pub boundaries: Vec<BoundaryKind>,
    /// Pace in effect (last `pace` event) with its seq.
    pub pace: Option<(Pace, u64)>,
    /// Every actor that has spawned, in id order.
    pub actors: Vec<ResolvedActor>,
    /// Objects spawned and not yet despawned, in id order.
    pub objects: Vec<ResolvedObject>,
}

impl ClipContext {
    pub fn actor(&self, actor: ActorId) -> Option<&ResolvedActor> {
        self.actors.iter().find(|a| a.actor == actor)
    }

    pub fn object(&self, object: ObjectId) -> Option<&ResolvedObject> {
        self.objects.iter().find(|o| o.object == object)
    }
}

/// Resolve the state of the scene at an unscoped selection point.
pub fn clip_context(events: &[Event], at: SelectAt) -> Option<ClipContext> {
    context_at(events, at, selection_end(events, at)?)
}

/// Resolve the scene at `at` using `actor`'s own tick or record input as the cut.
///
/// `Seq` matches [`clip_context`]. A tick or record cut includes this actor's
/// before-state on that clock even when another fighter already recorded an
/// input earlier in the same clock value.
pub fn clip_context_for(events: &[Event], at: SelectAt, actor: ActorId) -> Option<ClipContext> {
    context_at(events, at, selection_end_for(events, at, actor)?)
}

fn context_at(events: &[Event], at: SelectAt, included: usize) -> Option<ClipContext> {
    let prefix = &events[..included];
    let last = prefix.last();

    let mut boundaries = Vec::new();
    let mut pace = None;
    let mut actor_ids = Vec::new();
    let mut object_ids = Vec::new();
    for event in prefix {
        match &event.body {
            EventBody::Boundary(kind) => boundaries.push(*kind),
            EventBody::Pace(p) => pace = Some((p.clone(), event.seq)),
            EventBody::Lifecycle {
                actor,
                event: LifecycleEvent::Spawn { .. },
            } => actor_ids.push(*actor),
            EventBody::Object {
                object,
                event: ObjectEvent::Spawn { .. },
            } => object_ids.push(*object),
            _ => {}
        }
    }
    actor_ids.sort_unstable();
    actor_ids.dedup();
    object_ids.sort_unstable();
    object_ids.dedup();

    let actors = actor_ids
        .into_iter()
        .filter_map(|actor| resolve_actor(prefix, actor))
        .collect();
    let objects = object_ids
        .into_iter()
        .filter_map(|object| resolve_object(prefix, object))
        .collect();

    Some(ClipContext {
        at,
        included,
        seq: last.map(|e| e.seq),
        clocks: last.map_or(Clocks::NONE, |e| e.clocks),
        boundaries,
        pace,
        actors,
        objects,
    })
}

/// Resolve one actor over `events` (all of them are considered "before" the
/// selection point). `None` if the actor never spawned.
pub fn resolve_actor(events: &[Event], actor: ActorId) -> Option<ResolvedActor> {
    let mut identity = None;
    let mut status = ActorStatus::Alive;
    let mut segment = None;
    let mut segment_index = 0;
    for (index, event) in events.iter().enumerate() {
        if let EventBody::Lifecycle {
            actor: a,
            event: life,
        } = &event.body
        {
            if *a != actor {
                continue;
            }
            match life {
                LifecycleEvent::Spawn { identity: id, .. } => {
                    identity = Some(id.clone());
                    status = ActorStatus::Alive;
                }
                LifecycleEvent::Death { .. } => status = ActorStatus::Dead,
                LifecycleEvent::Respawn => status = ActorStatus::Alive,
                LifecycleEvent::Despawn { .. } => status = ActorStatus::Despawned,
                LifecycleEvent::FormChange { identity: id } => identity = Some(id.clone()),
            }
            segment = Some(Segment {
                start_seq: event.seq,
                boundary: life.kind(),
            });
            segment_index = index;
        }
    }
    let identity = identity?;
    let segment = segment?;

    let mut kinematics = Tracker::new();
    let mut situation = Tracker::new();
    let mut animation = Tracker::new();
    let mut vitals = Tracker::new();
    for event in events[segment_index + 1..].iter().rev() {
        match &event.body {
            EventBody::Gap {
                actor: gap_actor, ..
            } if gap_actor.map_or(true, |a| a == actor) => {
                kinematics.gap(event.seq);
                situation.gap(event.seq);
                animation.gap(event.seq);
                vitals.gap(event.seq);
            }
            EventBody::State {
                actor: a,
                phase,
                sample,
            } if *a == actor => {
                kinematics.feed(event.seq, Some(*phase), &sample.kinematics);
                situation.feed(event.seq, Some(*phase), &sample.situation);
                animation.feed(event.seq, Some(*phase), &sample.animation);
                vitals.feed(event.seq, Some(*phase), &sample.vitals);
            }
            _ => continue,
        }
        if kinematics.is_done() && situation.is_done() && animation.is_done() && vitals.is_done() {
            break;
        }
    }

    Some(ResolvedActor {
        actor,
        identity,
        status,
        segment,
        sample: ResolvedSample {
            kinematics: kinematics.finish(),
            situation: situation.finish(),
            animation: animation.finish(),
            vitals: vitals.finish(),
        },
    })
}

/// Resolve one live object over `events`. `None` if it never spawned or has
/// already despawned.
pub fn resolve_object(events: &[Event], object: ObjectId) -> Option<ResolvedObject> {
    let mut spawn = None;
    for (index, event) in events.iter().enumerate() {
        if let EventBody::Object {
            object: o,
            event: obj,
        } = &event.body
        {
            if *o != object {
                continue;
            }
            match obj {
                ObjectEvent::Spawn { identity, .. } => spawn = Some((index, identity.clone())),
                ObjectEvent::Despawn { .. } => return None,
                ObjectEvent::State { .. } => {}
            }
        }
    }
    let (spawn_index, identity) = spawn?;
    let mut transform = Tracker::new();
    let mut animation = Tracker::new();
    for event in events[spawn_index..].iter().rev() {
        match &event.body {
            EventBody::Gap { actor: None, .. } => {
                transform.gap(event.seq);
                animation.gap(event.seq);
            }
            EventBody::Object {
                object: o,
                event: obj,
            } if *o == object => match obj {
                ObjectEvent::Spawn {
                    transform: t,
                    animation: a,
                    ..
                }
                | ObjectEvent::State {
                    transform: t,
                    animation: a,
                } => {
                    transform.feed(event.seq, None, t);
                    animation.feed(event.seq, None, a);
                }
                ObjectEvent::Despawn { .. } => {}
            },
            _ => continue,
        }
        if transform.is_done() && animation.is_done() {
            break;
        }
    }
    Some(ResolvedObject {
        object,
        identity,
        spawn_seq: events[spawn_index].seq,
        transform: transform.finish(),
        animation: animation.finish(),
    })
}

/// Backward-walk state for one channel.
enum Tracker<T> {
    /// No sample seen yet.
    Open,
    /// Latest sample said `unchanged`; looking for its predecessor.
    WantPredecessor {
        latest_seq: u64,
    },
    Done(Resolved<T>),
}

impl<T: Clone> Tracker<T> {
    fn new() -> Self {
        Tracker::Open
    }

    fn is_done(&self) -> bool {
        matches!(self, Tracker::Done(_))
    }

    fn feed(&mut self, seq: u64, phase: Option<Phase>, channel: &Channel<T>) {
        let next = match (&*self, channel) {
            (Tracker::Done(_), _) => return,
            (Tracker::Open, Channel::Observed(value)) => Tracker::Done(Resolved::Observed {
                value: value.clone(),
                value_seq: seq,
                latest_seq: seq,
                phase,
            }),
            (Tracker::Open, Channel::Unchanged) => Tracker::WantPredecessor { latest_seq: seq },
            (Tracker::Open, Channel::Missing) => {
                Tracker::Done(Resolved::Missing(MissingReason::SampleMissing { seq }))
            }
            (Tracker::WantPredecessor { latest_seq, .. }, Channel::Observed(value)) => {
                Tracker::Done(Resolved::Observed {
                    value: value.clone(),
                    value_seq: seq,
                    latest_seq: *latest_seq,
                    phase,
                })
            }
            (Tracker::WantPredecessor { .. }, Channel::Unchanged) => return,
            (Tracker::WantPredecessor { latest_seq, .. }, Channel::Missing) => Tracker::Done(
                Resolved::Missing(MissingReason::UnchangedWithoutPredecessor { seq: *latest_seq }),
            ),
            (_, Channel::Unsupported) => Tracker::Done(Resolved::Unsupported),
        };
        *self = next;
    }

    fn gap(&mut self, gap_seq: u64) {
        let next = match &*self {
            Tracker::Done(_) => return,
            Tracker::Open => Resolved::Missing(MissingReason::GapAfterLatestSample { gap_seq }),
            Tracker::WantPredecessor { .. } => {
                Resolved::Missing(MissingReason::GapBeforePredecessor { gap_seq })
            }
        };
        *self = Tracker::Done(next);
    }

    fn finish(self) -> Resolved<T> {
        match self {
            Tracker::Open => Resolved::NoSample,
            Tracker::WantPredecessor { latest_seq, .. } => {
                Resolved::Missing(MissingReason::UnchangedWithoutPredecessor { seq: latest_seq })
            }
            Tracker::Done(resolved) => resolved,
        }
    }
}

impl Capture {
    pub fn clip_context(&self, at: SelectAt) -> Option<ClipContext> {
        clip_context(&self.events, at)
    }

    pub fn clip_context_for(&self, at: SelectAt, actor: ActorId) -> Option<ClipContext> {
        clip_context_for(&self.events, at, actor)
    }

    pub fn clip_events(&self, start: SelectAt, end: SelectAt) -> Option<&[Event]> {
        clip_events(&self.events, start, end)
    }

    pub fn clip_events_for(
        &self,
        start: SelectAt,
        end: SelectAt,
        actor: ActorId,
    ) -> Option<&[Event]> {
        clip_events_for(&self.events, start, end, actor)
    }
}
