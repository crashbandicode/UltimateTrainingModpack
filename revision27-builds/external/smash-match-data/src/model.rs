//! Data model of the shared match recording format.
//!
//! Everything in this module is plain data. Behaviour lives in [`crate::capture`]
//! (producer side), [`crate::archive`] (manifest + JSONL files), [`crate::clip`]
//! (selected-frame state resolution) and [`crate::validate`]. The JSON contract is
//! documented in `docs/FORMAT.md`; the two must stay in step.
//!
//! Two serialization rules keep native `f32` values bit-exact across a round trip
//! (`serde_json` with the `float_roundtrip` feature):
//!
//! * no `#[serde(flatten)]` and no internally or adjacently tagged enums anywhere a
//!   float can appear, because serde buffers those through `f64`;
//! * `null` is never a channel value; absence is always one of the explicit
//!   [`Channel`] markers, so unknown can never silently become zero.

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use serde::de::value::MapAccessDeserializer;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

/// Value of the manifest `format` field.
pub const FORMAT_NAME: &str = "smash-match-data";
/// Format version written by this crate.
pub const FORMAT_VERSION: FormatVersion = FormatVersion { major: 1, minor: 0 };
/// Manifest file name inside a capture directory.
pub const MANIFEST_FILE: &str = "manifest.json";
/// Default timeline file name inside a capture directory.
pub const TIMELINE_FILE: &str = "timeline.jsonl";

// ---------------------------------------------------------------------------
// Versioning
// ---------------------------------------------------------------------------

/// `major` changes break readers; `minor` changes only add optional data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FormatVersion {
    pub major: u32,
    pub minor: u32,
}

impl FormatVersion {
    /// A reader built for `self` can read `other` when the majors match and
    /// `other` adds nothing newer than `self` knows.
    pub fn can_read(self, other: FormatVersion) -> bool {
        self.major == other.major && other.minor <= self.minor
    }
}

impl fmt::Display for FormatVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

// ---------------------------------------------------------------------------
// Missingness
// ---------------------------------------------------------------------------

/// Presence wrapper for one group of sampled data.
///
/// JSON: an observed value is the inner object itself; the other three states
/// are the strings `"unchanged"`, `"missing"` and `"unsupported"`. `null` is
/// rejected on read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel<T> {
    /// A real observation. Neutral or zero values are observations, not absence.
    Observed(T),
    /// Equal to the nearest valid predecessor in the same actor lifetime segment.
    /// It resolves to nothing if a missing sample, a gap or a lifecycle boundary
    /// sits between here and that predecessor (see `clip`).
    Unchanged,
    /// The producer supports this channel but did not obtain a sample here.
    Missing,
    /// The producer never supplies this channel.
    Unsupported,
}

impl<T> Channel<T> {
    pub const UNCHANGED: &'static str = "unchanged";
    pub const MISSING: &'static str = "missing";
    pub const UNSUPPORTED: &'static str = "unsupported";

    pub fn observed(&self) -> Option<&T> {
        match self {
            Channel::Observed(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_observed(&self) -> bool {
        matches!(self, Channel::Observed(_))
    }

    pub fn as_ref(&self) -> Channel<&T> {
        match self {
            Channel::Observed(value) => Channel::Observed(value),
            Channel::Unchanged => Channel::Unchanged,
            Channel::Missing => Channel::Missing,
            Channel::Unsupported => Channel::Unsupported,
        }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Channel<U> {
        match self {
            Channel::Observed(value) => Channel::Observed(f(value)),
            Channel::Unchanged => Channel::Unchanged,
            Channel::Missing => Channel::Missing,
            Channel::Unsupported => Channel::Unsupported,
        }
    }

    /// Marker name for the non-observed states; `None` for an observation.
    pub fn marker(&self) -> Option<&'static str> {
        match self {
            Channel::Observed(_) => None,
            Channel::Unchanged => Some(Self::UNCHANGED),
            Channel::Missing => Some(Self::MISSING),
            Channel::Unsupported => Some(Self::UNSUPPORTED),
        }
    }
}

impl<T: Serialize> Serialize for Channel<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Channel::Observed(value) => value.serialize(serializer),
            other => serializer.serialize_str(other.marker().unwrap_or_default()),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Channel<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ChannelVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for ChannelVisitor<T> {
            type Value = Channel<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "an observed object or one of \"unchanged\", \"missing\", \"unsupported\"",
                )
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                match value {
                    "unchanged" => Ok(Channel::Unchanged),
                    "missing" => Ok(Channel::Missing),
                    "unsupported" => Ok(Channel::Unsupported),
                    other => Err(E::unknown_variant(
                        other,
                        &["unchanged", "missing", "unsupported"],
                    )),
                }
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                // Values are pulled straight from the underlying deserializer, so
                // f32 fields keep single-precision parsing.
                T::deserialize(MapAccessDeserializer::new(map)).map(Channel::Observed)
            }

            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Err(E::custom(
                    "null is not a channel value; use \"missing\", \"unchanged\" or \"unsupported\"",
                ))
            }

            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                self.visit_unit()
            }
        }

        deserializer.deserialize_any(ChannelVisitor(PhantomData))
    }
}

// ---------------------------------------------------------------------------
// Clocks
// ---------------------------------------------------------------------------

/// Independent clock readings attached to every event. Each is optional; a
/// producer sets only the clocks it actually observes. No clock is derived from
/// another unless the manifest declares that relationship.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Clocks {
    /// Gameplay tick: the engine's fixed-step scene counter during play.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick: Option<u64>,
    /// Input-consumption sequence: increments each time the game consumes input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_seq: Option<u64>,
    /// Saved-replay record ordinal (`smash_replay_core::records::Record::ordinal`).
    /// A record is not a gameplay tick or a video frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<u64>,
    /// Rendered/video frame counter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_frame: Option<u64>,
    /// Host monotonic time in nanoseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_ns: Option<u64>,
}

impl Clocks {
    pub const NONE: Clocks = Clocks {
        tick: None,
        input_seq: None,
        record: None,
        video_frame: None,
        host_ns: None,
    };

    pub fn tick(tick: u64) -> Self {
        Clocks::NONE.with_tick(tick)
    }

    pub fn record(record: u64) -> Self {
        Clocks::NONE.with_record(record)
    }

    pub fn with_tick(mut self, tick: u64) -> Self {
        self.tick = Some(tick);
        self
    }

    pub fn with_input_seq(mut self, input_seq: u64) -> Self {
        self.input_seq = Some(input_seq);
        self
    }

    pub fn with_record(mut self, record: u64) -> Self {
        self.record = Some(record);
        self
    }

    pub fn with_video_frame(mut self, video_frame: u64) -> Self {
        self.video_frame = Some(video_frame);
        self
    }

    pub fn with_host_ns(mut self, host_ns: u64) -> Self {
        self.host_ns = Some(host_ns);
        self
    }

    pub fn get(&self, kind: ClockKind) -> Option<u64> {
        match kind {
            ClockKind::Tick => self.tick,
            ClockKind::InputSeq => self.input_seq,
            ClockKind::Record => self.record,
            ClockKind::VideoFrame => self.video_frame,
            ClockKind::HostNs => self.host_ns,
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Clocks::NONE
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockKind {
    Tick,
    InputSeq,
    Record,
    VideoFrame,
    HostNs,
}

impl ClockKind {
    pub const ALL: [ClockKind; 5] = [
        ClockKind::Tick,
        ClockKind::InputSeq,
        ClockKind::Record,
        ClockKind::VideoFrame,
        ClockKind::HostNs,
    ];
}

impl fmt::Display for ClockKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ClockKind::Tick => "tick",
            ClockKind::InputSeq => "input_seq",
            ClockKind::Record => "record",
            ClockKind::VideoFrame => "video_frame",
            ClockKind::HostNs => "host_ns",
        })
    }
}

/// Which clocks a producer supplies and which relationships between them are
/// actually established. Absent relation means "not established".
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockSupport {
    #[serde(default)]
    pub supplied: Vec<ClockKind>,
    #[serde(default)]
    pub relations: Vec<ClockRelation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockRelation {
    pub a: ClockKind,
    pub b: ClockKind,
    pub relation: ClockRelationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockRelationKind {
    /// `a == b` at every event.
    Identity,
    /// `b == a + b_minus_a` at every event.
    Offset { b_minus_a: i64 },
    /// Both are recorded but no relationship has been established.
    Unestablished,
}

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// Stable actor identity assigned by the producer, unique within a capture and
/// never reused. Distinct from controller slot, fighter entry and the game's
/// reusable native object id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ActorId(pub u32);

impl fmt::Display for ActorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "actor#{}", self.0)
    }
}

/// Stable scene-object identity (article, projectile, item), same rules as
/// [`ActorId`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ObjectId(pub u32);

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "object#{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlKind {
    Human,
    Cpu {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        level: Option<u8>,
    },
    #[default]
    Unknown,
}

/// Who and what an actor is at spawn (or after a form change). Every field is
/// optional because a saved-replay header does not expose most of them; `None`
/// means "not supplied", never a default value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorIdentity {
    /// Native fighter kind integer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fighter_kind: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fighter_name: Option<String>,
    /// Producer-defined sub-form when the kind alone does not identify the form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub costume: Option<u8>,
    /// Fighter entry index (0..8) this actor belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<u8>,
    /// Controller slot feeding this actor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller: Option<u8>,
    #[serde(default)]
    pub control: ControlKind,
    /// The game's reusable object id for this actor's current native object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_object_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<u8>,
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/// Actor lifecycle boundaries. Every one of them starts a new lifetime segment:
/// `unchanged` values never resolve across a boundary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleEvent {
    Spawn {
        identity: ActorIdentity,
        cause: SpawnCause,
    },
    /// The actor died (blast zone, stamina, etc.). It stays `Dead` until `Respawn`.
    Death {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cause: Option<String>,
    },
    /// The actor is back in play (producer-defined: platform appears or control
    /// resumes; the producer should say which in the manifest notes).
    Respawn,
    /// The native object is gone for good. The actor id is never reused.
    Despawn { cause: DespawnCause },
    /// Same actor, new form/identity (same native object).
    FormChange { identity: ActorIdentity },
}

impl LifecycleEvent {
    pub fn kind(&self) -> LifecycleKind {
        match self {
            LifecycleEvent::Spawn { .. } => LifecycleKind::Spawn,
            LifecycleEvent::Death { .. } => LifecycleKind::Death,
            LifecycleEvent::Respawn => LifecycleKind::Respawn,
            LifecycleEvent::Despawn { .. } => LifecycleKind::Despawn,
            LifecycleEvent::FormChange { .. } => LifecycleKind::FormChange,
        }
    }

    /// Identity carried by spawn/form-change events.
    pub fn identity(&self) -> Option<&ActorIdentity> {
        match self {
            LifecycleEvent::Spawn { identity, .. } | LifecycleEvent::FormChange { identity } => {
                Some(identity)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleKind {
    Spawn,
    Death,
    Respawn,
    Despawn,
    FormChange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnCause {
    MatchStart,
    /// Created by a transformation from another actor of the same entry.
    Transform {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<ActorId>,
    },
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DespawnCause {
    MatchEnd,
    Transform {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<ActorId>,
    },
    /// Out of stocks / eliminated from the match.
    Eliminated,
    Unknown,
}

/// Sampling phase of a state event relative to the input applied at that tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// State before this tick's input is applied.
    Before,
    /// Resulting state after this tick's input is applied.
    After,
}

// ---------------------------------------------------------------------------
// Fighter state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Vec3 { x, y, z }
    }

    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    /// Euler angles in the engine's own order and units (documented by producer).
    Euler(Vec3),
    Quaternion {
        x: f32,
        y: f32,
        z: f32,
        w: f32,
    },
}

impl Rotation {
    pub fn is_finite(&self) -> bool {
        match self {
            Rotation::Euler(v) => v.is_finite(),
            Rotation::Quaternion { x, y, z, w } => {
                x.is_finite() && y.is_finite() && z.is_finite() && w.is_finite()
            }
        }
    }
}

/// Position, facing and momentum.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Kinematics {
    pub position: Vec3,
    /// Native facing value (`lr`), kept as the engine's float.
    pub facing: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity: Option<Vec3>,
}

/// Ground/air situation and status.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Situation {
    /// Native situation kind integer (ground/air/cliff/...).
    pub situation_kind: i32,
    /// Native status (action) kind integer.
    pub status_kind: i32,
    /// Producer's grounded interpretation when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grounded: Option<bool>,
    /// Remaining hitstop/hitlag frames, when the producer tracks them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hitstop_frames: Option<f32>,
}

/// Motion identifier (hash40) with an optional readable label.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MotionRef {
    pub hash40: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl MotionRef {
    pub fn new(hash40: u64) -> Self {
        MotionRef {
            hash40,
            label: None,
        }
    }

    pub fn labelled(hash40: u64, label: impl Into<String>) -> Self {
        MotionRef {
            hash40,
            label: Some(label.into()),
        }
    }
}

/// Playback data for the engine to reproduce animation with its own assets.
/// `frame` is a playback position (possibly fractional), never an image.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    pub motion: MotionRef,
    pub frame: f32,
    pub rate: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_rotation: Option<Rotation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_scale: Option<Vec3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<SecondaryAnimation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SecondaryAnimation {
    pub motion: MotionRef,
    pub frame: f32,
    pub weight: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vitals {
    pub damage: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stocks: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shield: Option<f32>,
}

/// One state sample of one actor. Each group carries its own presence marker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FighterSample {
    pub kinematics: Channel<Kinematics>,
    pub situation: Channel<Situation>,
    pub animation: Channel<Animation>,
    pub vitals: Channel<Vitals>,
}

impl FighterSample {
    /// Every group unsupported; set the groups the producer does supply.
    pub fn unsupported() -> Self {
        FighterSample {
            kinematics: Channel::Unsupported,
            situation: Channel::Unsupported,
            animation: Channel::Unsupported,
            vitals: Channel::Unsupported,
        }
    }

    /// Every group missing: the producer supports them but got no sample here.
    pub fn missing() -> Self {
        FighterSample {
            kinematics: Channel::Missing,
            situation: Channel::Missing,
            animation: Channel::Missing,
            vitals: Channel::Missing,
        }
    }

    /// Every group unchanged since the previous valid sample in this lifetime.
    pub fn unchanged() -> Self {
        FighterSample {
            kinematics: Channel::Unchanged,
            situation: Channel::Unchanged,
            animation: Channel::Unchanged,
            vitals: Channel::Unchanged,
        }
    }

    pub fn with_kinematics(mut self, kinematics: Kinematics) -> Self {
        self.kinematics = Channel::Observed(kinematics);
        self
    }

    pub fn with_situation(mut self, situation: Situation) -> Self {
        self.situation = Channel::Observed(situation);
        self
    }

    pub fn with_animation(mut self, animation: Animation) -> Self {
        self.animation = Channel::Observed(animation);
        self
    }

    pub fn with_vitals(mut self, vitals: Vitals) -> Self {
        self.vitals = Channel::Observed(vitals);
        self
    }
}

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputOrigin {
    /// Read from the physical controller/HID layer.
    Physical,
    /// Decoded from a saved replay's record stream (mapped replay input).
    ReplayDecoded,
    /// What the game actually applied to the actor this tick.
    Effective,
    /// Generated by the CPU player logic.
    CpuGenerated,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Buttons {
    pub bits: u32,
}

impl Buttons {
    pub const fn new(bits: u32) -> Self {
        Buttons { bits }
    }

    pub fn is_set(&self, bit: u8) -> bool {
        bit < 32 && self.bits & (1u32 << bit) != 0
    }
}

/// Stick reading in the producer's native representation. Conversions between
/// the two belong to consumer adapters, never to the recording.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stick {
    /// Native floating-point axes.
    F32 { x: f32, y: f32 },
    /// Native signed-byte axes (saved replay packets).
    I8 { x: i8, y: i8 },
}

impl Stick {
    pub fn is_finite(&self) -> bool {
        match self {
            Stick::F32 { x, y } => x.is_finite() && y.is_finite(),
            Stick::I8 { .. } => true,
        }
    }
}

/// Raw bytes exactly as the producer saw them, before any decoding.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RawBytes {
    /// Producer-defined encoding name (for example `fram_packet_13_0_5`).
    pub encoding: String,
    /// Lower-case hex, two digits per byte.
    pub hex: String,
}

impl RawBytes {
    pub fn new(encoding: impl Into<String>, bytes: &[u8]) -> Self {
        RawBytes {
            encoding: encoding.into(),
            hex: hex_encode(bytes),
        }
    }

    pub fn bytes(&self) -> Result<Vec<u8>, HexError> {
        hex_decode(&self.hex)
    }
}

/// One original source unit, preserved without assigning unsupported meanings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawUnit {
    pub raw: RawBytes,
    /// Byte offset within the source buffer identified by the raw encoding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_offset: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InputEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<u8>,
    /// Actor consuming the input, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<ActorId>,
    pub origin: InputOrigin,
    /// Id of an [`InputLayout`] declared in the manifest.
    pub layout: String,
    pub buttons: Channel<Buttons>,
    pub left_stick: Channel<Stick>,
    pub right_stick: Channel<Stick>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<RawBytes>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisEncoding {
    F32,
    I8,
}

/// Documents one stick's representation. Unknown scaling stays `None`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AxisLayout {
    pub encoding: AxisEncoding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neutral: Option<f32>,
    /// Multiplier that maps the native value to a unit float, if established.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub float_scale: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonLayout {
    pub width_bits: u8,
    /// Known bit positions to names. Unknown bits are simply absent.
    #[serde(default)]
    pub names: BTreeMap<u8, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InputLayout {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub buttons: ButtonLayout,
    pub left_stick: AxisLayout,
    pub right_stick: AxisLayout,
    /// Encoding name used by `RawBytes` on inputs of this layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_encoding: Option<String>,
}

// ---------------------------------------------------------------------------
// Scene objects
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectClass {
    /// Character-native article (Link's bombs, Peach's turnips once thrown, ...).
    Article,
    Projectile,
    Item,
    #[default]
    Other,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectIdentity {
    #[serde(default)]
    pub class: ObjectClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_kind: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Owning actor, when the object belongs to one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<ActorId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_object_id: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub position: Vec3,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Rotation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Vec3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity: Option<Vec3>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectEvent {
    Spawn {
        identity: ObjectIdentity,
        transform: Channel<Transform>,
        animation: Channel<Animation>,
    },
    State {
        transform: Channel<Transform>,
        animation: Channel<Animation>,
    },
    Despawn {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cause: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Random outcomes
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeBasis {
    /// Seen happening in the game.
    Observed,
    /// What a consumer asked for (e.g. a macro wants Judge 9).
    Requested,
    /// Derived from other evidence, not directly observed.
    Inferred,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuOption {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RandomOutcome {
    Judge {
        number: u8,
    },
    ItemPull {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item_kind: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Scene object created by the pull, when recorded.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        object: Option<ObjectId>,
    },
    HeroMenu {
        options: Vec<MenuOption>,
    },
    /// A verified RNG stream observation (see the replay-core RNG model).
    RngObservation {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stream: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seed: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        counter: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        words: Option<[u32; 4]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<u32>,
    },
    Other {
        domain: String,
        value: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RandomEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<ActorId>,
    pub basis: OutcomeBasis,
    pub outcome: RandomOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

// ---------------------------------------------------------------------------
// Timeline events
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryKind {
    RecordingStart,
    FirstPlayable,
    MatchEnd,
    RecordingEnd,
}

impl BoundaryKind {
    pub const ALL: [BoundaryKind; 4] = [
        BoundaryKind::RecordingStart,
        BoundaryKind::FirstPlayable,
        BoundaryKind::MatchEnd,
        BoundaryKind::RecordingEnd,
    ];

    /// Required relative order.
    pub fn order(self) -> u8 {
        match self {
            BoundaryKind::RecordingStart => 0,
            BoundaryKind::FirstPlayable => 1,
            BoundaryKind::MatchEnd => 2,
            BoundaryKind::RecordingEnd => 3,
        }
    }
}

/// Global pacing change in effect from this event onward.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pace {
    pub paused: bool,
    /// Game speed multiplier (1.0 normal, 0.5 half speed, ...), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
    /// Native speed index when the producer reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Pace {
    pub fn normal() -> Self {
        Pace {
            paused: false,
            speed: Some(1.0),
            speed_index: None,
            reason: None,
        }
    }

    pub fn paused() -> Self {
        Pace {
            paused: true,
            speed: None,
            speed_index: None,
            reason: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// One timeline line. `seq` is assigned by the writer, starts at 0 and is
/// strictly increasing (consecutive when written by this crate).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub clocks: Clocks,
    pub body: EventBody,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventBody {
    Boundary(BoundaryKind),
    Pace(Pace),
    /// Original bytes such as a complete decoded replay record.
    Raw(RawUnit),
    Lifecycle {
        actor: ActorId,
        event: LifecycleEvent,
    },
    State {
        actor: ActorId,
        phase: Phase,
        sample: FighterSample,
    },
    Input(InputEvent),
    Object {
        object: ObjectId,
        event: ObjectEvent,
    },
    Random(RandomEvent),
    /// Samples that should have existed between the previous event (for this
    /// actor, or globally when `actor` is `None`) and this clock position were
    /// not captured. Breaks `unchanged` resolution.
    Gap {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        actor: Option<ActorId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Note {
        severity: Severity,
        text: String,
    },
    /// Terminator written by `finish`; `events` counts the lines before it.
    End {
        events: u64,
    },
}

impl EventBody {
    pub fn name(&self) -> &'static str {
        match self {
            EventBody::Boundary(_) => "boundary",
            EventBody::Pace(_) => "pace",
            EventBody::Raw(_) => "raw",
            EventBody::Lifecycle { .. } => "lifecycle",
            EventBody::State { .. } => "state",
            EventBody::Input(_) => "input",
            EventBody::Object { .. } => "object",
            EventBody::Random(_) => "random",
            EventBody::Gap { .. } => "gap",
            EventBody::Note { .. } => "note",
            EventBody::End { .. } => "end",
        }
    }

    /// The actor this event is about, if any.
    pub fn actor(&self) -> Option<ActorId> {
        match self {
            EventBody::Lifecycle { actor, .. } | EventBody::State { actor, .. } => Some(*actor),
            EventBody::Input(input) => input.actor,
            EventBody::Random(random) => random.actor,
            EventBody::Gap { actor, .. } => *actor,
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Manifest: provenance, rules, roster
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolVersion {
    pub name: String,
    pub version: String,
}

impl ToolVersion {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        ToolVersion {
            name: name.into(),
            version: version.into(),
        }
    }

    /// This crate's own identity, for producers built on it.
    pub fn this_crate() -> Self {
        ToolVersion::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameBuild {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    /// Loaded modifications that can change behaviour, if any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mods: Vec<String>,
}

impl GameBuild {
    pub const ULTIMATE_TITLE: &'static str = "Super Smash Bros. Ultimate";

    pub fn ultimate(version: impl Into<String>) -> Self {
        GameBuild {
            title: Self::ULTIMATE_TITLE.to_string(),
            version: Some(version.into()),
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// Captured while the original match was played.
    LiveMatch,
    /// Captured while the game replayed a saved replay.
    ReplayRegeneration,
    /// Decoded offline from a saved replay file; no live state was observed.
    FileDecode,
}

/// Exact saved-replay association by content hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayAssociation {
    /// Lower-case hex SHA-256 of the whole replay file.
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_hint: Option<String>,
    /// Raw version word of the inner setup header, when parsed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_version_word: Option<u32>,
    /// Stored seed word of the inner setup header, when parsed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stored_seed_word: Option<u32>,
}

impl ReplayAssociation {
    pub fn from_bytes(bytes: &[u8], path_hint: Option<String>) -> Self {
        ReplayAssociation {
            sha256: sha256_hex(bytes),
            size_bytes: Some(bytes.len() as u64),
            path_hint,
            header_version_word: None,
            stored_seed_word: None,
        }
    }
}

/// Content hash of an explicitly named source buffer, not implicitly a replay file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentDigest {
    pub scope: String,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay: Option<ReplayAssociation>,
    /// Decoder used for `FileDecode` sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoder: Option<ToolVersion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub digests: Vec<ContentDigest>,
}

impl Source {
    pub fn live_match() -> Self {
        Source {
            kind: SourceKind::LiveMatch,
            description: None,
            replay: None,
            decoder: None,
            digests: Vec::new(),
        }
    }

    pub fn replay_regeneration(replay: ReplayAssociation) -> Self {
        Source {
            kind: SourceKind::ReplayRegeneration,
            description: None,
            replay: Some(replay),
            decoder: None,
            digests: Vec::new(),
        }
    }

    pub fn file_decode(replay: ReplayAssociation, decoder: ToolVersion) -> Self {
        Source {
            kind: SourceKind::FileDecode,
            description: None,
            replay: Some(replay),
            decoder: Some(decoder),
            digests: Vec::new(),
        }
    }
}

/// The ruleset the recording is meant for. Separate from what was actually set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RulesetTarget {
    /// Competitive: items off except character-native ones, hazards off.
    #[default]
    Competitive,
    Casual,
    Unspecified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    Stock,
    Time,
    Stamina,
    Other(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemSetting {
    /// Match item spawning disabled. Character-native items still occur.
    Off,
    On,
}

/// Rules as actually configured. `None` means the producer could not read it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ActualRules {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<MatchMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stocks: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_limit_seconds: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<ItemSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_hazards: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_smash_meter: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spirits: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub damage_ratio: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_rate: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_battle: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_morph: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underdog_boost: Option<bool>,
    /// Raw setup bytes for sources that only have those (e.g. a replay header block).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_setup: Option<RawBytes>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rules {
    #[serde(default)]
    pub target: RulesetTarget,
    #[serde(default)]
    pub actual: ActualRules,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageForm {
    Normal,
    Battlefield,
    Omega,
    Other(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_id: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<StageForm>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerInfo {
    pub slot: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// One fighter entry of the roster as set up for the match.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FighterEntry {
    pub entry: u8,
    #[serde(default)]
    pub control: ControlKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fighter_kind: Option<i32>,
    /// All fighter kinds the entry can create (transforming fighters), in
    /// creation order, when the source exposes the group rather than one kind.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fighter_kind_group: Vec<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fighter_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub costume: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<u8>,
    /// Whether the entry actually takes part. `None` when the source cannot say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub present: Option<bool>,
}

impl FighterEntry {
    pub fn new(entry: u8) -> Self {
        FighterEntry {
            entry,
            control: ControlKind::Unknown,
            controller: None,
            fighter_kind: None,
            fighter_kind_group: Vec::new(),
            fighter_name: None,
            costume: None,
            team: None,
            present: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupplyLevel {
    Full,
    Partial,
    #[default]
    Unsupported,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Supply {
    #[serde(default)]
    pub level: SupplyLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Supply {
    pub fn full() -> Self {
        Supply {
            level: SupplyLevel::Full,
            note: None,
        }
    }

    pub fn partial(note: impl Into<String>) -> Self {
        Supply {
            level: SupplyLevel::Partial,
            note: Some(note.into()),
        }
    }

    pub fn unsupported() -> Self {
        Supply::default()
    }

    pub fn is_unsupported(&self) -> bool {
        self.level == SupplyLevel::Unsupported
    }
}

/// What the producer supplies, channel by channel. Defaults to unsupported so a
/// producer has to opt in explicitly.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelSupport {
    #[serde(default)]
    pub kinematics: Supply,
    #[serde(default)]
    pub situation: Supply,
    #[serde(default)]
    pub animation: Supply,
    #[serde(default)]
    pub vitals: Supply,
    #[serde(default)]
    pub inputs: Supply,
    #[serde(default)]
    pub lifecycle: Supply,
    #[serde(default)]
    pub objects: Supply,
    #[serde(default)]
    pub random: Supply,
}

impl ChannelSupport {
    pub fn all_full() -> Self {
        ChannelSupport {
            kinematics: Supply::full(),
            situation: Supply::full(),
            animation: Supply::full(),
            vitals: Supply::full(),
            inputs: Supply::full(),
            lifecycle: Supply::full(),
            objects: Supply::full(),
            random: Supply::full(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionState {
    /// Writer still active or never finished.
    Open,
    /// Producer finished the full recording.
    Complete,
    /// Producer knowingly stopped early (reason says why) and closed cleanly.
    Truncated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Completion {
    pub state: CompletionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Total timeline lines including the `end` event, once finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seq: Option<u64>,
}

impl Completion {
    pub fn open() -> Self {
        Completion {
            state: CompletionState::Open,
            reason: None,
            events: None,
            last_seq: None,
        }
    }

    pub fn complete() -> Self {
        Completion {
            state: CompletionState::Complete,
            reason: None,
            events: None,
            last_seq: None,
        }
    }

    pub fn truncated(reason: impl Into<String>) -> Self {
        Completion {
            state: CompletionState::Truncated,
            reason: Some(reason.into()),
            events: None,
            last_seq: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundaryMark {
    pub boundary: BoundaryKind,
    pub seq: u64,
    pub clocks: Clocks,
}

/// Per-actor summary derived from the timeline at finish time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorSummary {
    pub actor: ActorId,
    /// Identity at spawn.
    pub identity: ActorIdentity,
    pub spawn_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub despawn_seq: Option<u64>,
    pub deaths: u32,
    pub respawns: u32,
    pub form_changes: u32,
}

/// Everything the producer declares up front.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CaptureHeader {
    /// Producer-chosen unique id (UUID recommended).
    pub capture_id: String,
    /// RFC 3339 UTC timestamp, if the producer has a clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_utc: Option<String>,
    pub recorder: ToolVersion,
    pub game: GameBuild,
    pub source: Source,
    #[serde(default)]
    pub rules: Rules,
    #[serde(default)]
    pub stage: Stage,
    #[serde(default)]
    pub controllers: Vec<ControllerInfo>,
    #[serde(default)]
    pub entries: Vec<FighterEntry>,
    #[serde(default)]
    pub input_layouts: Vec<InputLayout>,
    #[serde(default)]
    pub channels: ChannelSupport,
    #[serde(default)]
    pub clocks: ClockSupport,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl Default for Source {
    fn default() -> Self {
        Source::live_match()
    }
}

/// `manifest.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub format_version: FormatVersion,
    pub capture: CaptureHeader,
    pub timeline_file: String,
    pub completion: Completion,
    #[serde(default)]
    pub boundaries: Vec<BoundaryMark>,
    #[serde(default)]
    pub actors: Vec<ActorSummary>,
}

impl Manifest {
    pub fn new(capture: CaptureHeader) -> Self {
        Manifest {
            format: FORMAT_NAME.to_string(),
            format_version: FORMAT_VERSION,
            capture,
            timeline_file: TIMELINE_FILE.to_string(),
            completion: Completion::open(),
            boundaries: Vec::new(),
            actors: Vec::new(),
        }
    }

    pub fn input_layout(&self, id: &str) -> Option<&InputLayout> {
        self.capture
            .input_layouts
            .iter()
            .find(|layout| layout.id == id)
    }
}

// ---------------------------------------------------------------------------
// Read-side completeness
// ---------------------------------------------------------------------------

/// What a reader observed about the timeline, independent of what the
/// manifest claims. Never derived from the manifest alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    /// Finished by the producer and internally consistent.
    Complete,
    /// Producer stopped early on purpose and said so; the file is consistent.
    DeclaredTruncated {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The file is cut off or inconsistent; the listed problems say how.
    Incomplete { problems: Vec<TruncationProblem> },
}

impl Completeness {
    pub fn is_complete(&self) -> bool {
        matches!(self, Completeness::Complete)
    }
}

impl fmt::Display for Completeness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Completeness::Complete => f.write_str("complete"),
            Completeness::DeclaredTruncated {
                reason: Some(reason),
            } => {
                write!(f, "truncated by producer: {reason}")
            }
            Completeness::DeclaredTruncated { reason: None } => {
                f.write_str("truncated by producer")
            }
            Completeness::Incomplete { problems } => {
                f.write_str("incomplete:")?;
                for problem in problems {
                    write!(f, " {problem};")?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TruncationProblem {
    /// The manifest still says `open`: the writer never finished.
    ManifestOpen,
    /// No `end` event terminates the timeline.
    MissingEnd,
    /// The last line is not valid JSON and has no trailing newline.
    PartialTrailingLine { line: u64 },
    /// The last line parsed but was not newline-terminated.
    MissingTrailingNewline { line: u64 },
    /// `end.events` disagrees with the number of lines before it.
    EndCountMismatch { declared: u64, observed: u64 },
    /// `completion.events` in the manifest disagrees with the file.
    ManifestCountMismatch { declared: u64, observed: u64 },
    /// `completion.last_seq` in the manifest disagrees with the file.
    ManifestLastSeqMismatch { declared: u64, observed: u64 },
}

impl fmt::Display for TruncationProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TruncationProblem::ManifestOpen => f.write_str("manifest completion is still open"),
            TruncationProblem::MissingEnd => f.write_str("timeline has no end event"),
            TruncationProblem::PartialTrailingLine { line } => {
                write!(f, "line {line} is a partial, unparseable trailing line")
            }
            TruncationProblem::MissingTrailingNewline { line } => {
                write!(f, "line {line} has no trailing newline")
            }
            TruncationProblem::EndCountMismatch { declared, observed } => write!(
                f,
                "end event declares {declared} events but {observed} precede it"
            ),
            TruncationProblem::ManifestCountMismatch { declared, observed } => write!(
                f,
                "manifest declares {declared} timeline lines but {observed} were read"
            ),
            TruncationProblem::ManifestLastSeqMismatch { declared, observed } => {
                write!(
                    f,
                    "manifest declares last seq {declared} but file ends at {observed}"
                )
            }
        }
    }
}

/// A fully loaded capture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Capture {
    pub manifest: Manifest,
    pub events: Vec<Event>,
    pub completeness: Completeness,
}

impl Capture {
    pub fn header(&self) -> &CaptureHeader {
        &self.manifest.capture
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HexError {
    OddLength { len: usize },
    InvalidDigit { position: usize },
}

impl fmt::Display for HexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HexError::OddLength { len } => write!(f, "hex string has odd length {len}"),
            HexError::InvalidDigit { position } => {
                write!(f, "invalid hex digit at position {position}")
            }
        }
    }
}

impl std::error::Error for HexError {}

pub fn hex_encode(bytes: &[u8]) -> String {
    use fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a String cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub fn hex_decode(hex: &str) -> Result<Vec<u8>, HexError> {
    let bytes = hex.as_bytes();
    if bytes.len() % 2 != 0 {
        return Err(HexError::OddLength { len: bytes.len() });
    }
    let digit = |position: usize| -> Result<u8, HexError> {
        (bytes[position] as char)
            .to_digit(16)
            .map(|d| d as u8)
            .ok_or(HexError::InvalidDigit { position })
    };
    (0..bytes.len())
        .step_by(2)
        .map(|i| Ok(digit(i)? << 4 | digit(i + 1)?))
        .collect()
}

/// Lower-case hex SHA-256 of `bytes`, the content hash used for replay association.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex_encode(&Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip_and_errors() {
        let bytes = [0x00, 0x7f, 0x80, 0xff];
        let hex = hex_encode(&bytes);
        assert_eq!(hex, "007f80ff");
        assert_eq!(hex_decode(&hex).unwrap(), bytes);
        assert_eq!(hex_decode("abc"), Err(HexError::OddLength { len: 3 }));
        assert_eq!(
            hex_decode("zz"),
            Err(HexError::InvalidDigit { position: 0 })
        );
    }

    #[test]
    fn sha256_known_answer() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn channel_markers() {
        let observed: Channel<Buttons> = Channel::Observed(Buttons::new(3));
        assert_eq!(serde_json::to_string(&observed).unwrap(), r#"{"bits":3}"#);
        assert_eq!(
            serde_json::to_string(&Channel::<Buttons>::Unchanged).unwrap(),
            r#""unchanged""#
        );
        let back: Channel<Buttons> = serde_json::from_str(r#""missing""#).unwrap();
        assert_eq!(back, Channel::Missing);
        assert!(serde_json::from_str::<Channel<Buttons>>("null").is_err());
        assert!(serde_json::from_str::<Channel<Buttons>>(r#""absent""#).is_err());
    }
}
