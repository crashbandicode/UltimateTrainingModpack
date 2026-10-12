//! Training, playback-export and AI views of a shared capture.
//!
//! Clip state comes from [`crate::clip`]. Actor-selected clips use
//! [`clip_context_for`](crate::clip::clip_context_for) and
//! [`selection_end_for`](crate::clip::selection_end_for), so a tick or record
//! cut follows that actor's own input. Whole-recording export keeps the
//! unscoped cut. Each input resolves inside the actor's lifetime at that event
//! and inside its own origin, layout, controller and entry stream.
//!
//! Stick and button bytes are produced only when the caller passes
//! [`TrainingQuantizeOptions`]. Otherwise native [`Stick`] values are copied.
//! A missing, unsupported or unresolved channel fails the Training conversion.
//! Playback and AI keep that missingness on the value. Replay record ordinals
//! stay on `clocks.record` and are never written into `clocks.tick`. Actor ids
//! are never replaced with controller slots.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::clip::{
    resolve_actor, resolve_object, selection_end, selection_end_for, ActorStatus, MissingReason,
    Resolved, ResolvedActor, ResolvedObject, SelectAt,
};
use crate::model::*;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportCode {
    Missing,
    Unsupported,
    NoSample,
    UnresolvedUnchanged,
    Gap,
    ActorAbsent,
    Truncated,
    Incomplete,
    NonFinite,
    OutOfRange,
    UnmappedButton,
    UndeclaredScale,
    InvalidScale,
    ClockUnavailable,
    MissingClipContext,
    EndBeforeStart,
}

impl fmt::Display for ReportCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            ReportCode::Missing => "missing",
            ReportCode::Unsupported => "unsupported",
            ReportCode::NoSample => "no_sample",
            ReportCode::UnresolvedUnchanged => "unresolved_unchanged",
            ReportCode::Gap => "gap",
            ReportCode::ActorAbsent => "actor_absent",
            ReportCode::Truncated => "truncated",
            ReportCode::Incomplete => "incomplete",
            ReportCode::NonFinite => "non_finite",
            ReportCode::OutOfRange => "out_of_range",
            ReportCode::UnmappedButton => "unmapped_button",
            ReportCode::UndeclaredScale => "undeclared_scale",
            ReportCode::InvalidScale => "invalid_scale",
            ReportCode::ClockUnavailable => "clock_unavailable",
            ReportCode::MissingClipContext => "missing_clip_context",
            ReportCode::EndBeforeStart => "end_before_start",
        };
        f.write_str(name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterReport {
    pub code: ReportCode,
    pub path: String,
    pub detail: String,
}

impl fmt::Display for AdapterReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code, self.path, self.detail)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterError {
    pub reports: Vec<AdapterReport>,
}

impl AdapterError {
    fn one(code: ReportCode, path: impl Into<String>, detail: impl Into<String>) -> Self {
        AdapterError {
            reports: vec![AdapterReport {
                code,
                path: path.into(),
                detail: detail.into(),
            }],
        }
    }
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, report) in self.reports.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{report}")?;
        }
        Ok(())
    }
}

impl std::error::Error for AdapterError {}

fn report(code: ReportCode, path: impl Into<String>, detail: impl Into<String>) -> AdapterReport {
    AdapterReport {
        code,
        path: path.into(),
        detail: detail.into(),
    }
}

// ---------------------------------------------------------------------------
// Quantize options
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleRounding {
    /// Half away from zero, then the integer result.
    HalfAwayFromZero,
    TowardZero,
}

/// `signed_byte = round((native - origin) / span * max_abs)`.
///
/// `span` is the native distance that maps to `max_abs`. For a unit float in
/// `[-1, 1]` that becomes a signed byte, use `origin = 0`, `span = 1`,
/// `max_abs = 127`. For an identity map of stored `i8` values onto
/// `[-127, 127]`, use `origin = 0`, `span = 127`, `max_abs = 127`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinearScale {
    pub origin: f32,
    pub span: f32,
    pub max_abs: u8,
    pub rounding: ScaleRounding,
    pub saturate: bool,
}

impl LinearScale {
    pub fn checked(self) -> Result<Self, AdapterReport> {
        if !self.origin.is_finite() || !self.span.is_finite() || self.span == 0.0 {
            return Err(report(
                ReportCode::InvalidScale,
                "scale",
                "origin and span must be finite and span must be non-zero",
            ));
        }
        if self.max_abs == 0 || self.max_abs > 127 {
            return Err(report(
                ReportCode::OutOfRange,
                "scale",
                "max_abs must be in 1..=127 so the result fits a signed byte",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BitRoute {
    pub source: u8,
    pub output: u8,
}

/// Declared button mapping. Bits with no route must be clear or the conversion fails.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonMap {
    /// Copy source bits `0..width_bits`. A set bit at or above `width_bits` fails.
    Identity {
        width_bits: u8,
    },
    Bits {
        routes: Vec<BitRoute>,
    },
}

impl ButtonMap {
    fn checked(&self) -> Result<(), AdapterReport> {
        match self {
            ButtonMap::Identity { width_bits } => {
                if *width_bits == 0 || *width_bits > 32 {
                    return Err(report(
                        ReportCode::InvalidScale,
                        "buttons",
                        "identity width_bits must be in 1..=32",
                    ));
                }
                Ok(())
            }
            ButtonMap::Bits { routes } => {
                let mut seen_src = [false; 32];
                let mut seen_dst = [false; 32];
                for route in routes {
                    if route.source >= 32 || route.output >= 32 {
                        return Err(report(
                            ReportCode::InvalidScale,
                            "buttons",
                            "bit routes must use bits 0..=31",
                        ));
                    }
                    if seen_src[usize::from(route.source)] || seen_dst[usize::from(route.output)] {
                        return Err(report(
                            ReportCode::InvalidScale,
                            "buttons",
                            "bit routes must not reuse a source or output bit",
                        ));
                    }
                    seen_src[usize::from(route.source)] = true;
                    seen_dst[usize::from(route.output)] = true;
                }
                Ok(())
            }
        }
    }

    fn output_bit(&self, source: u8) -> Option<u8> {
        match self {
            ButtonMap::Identity { width_bits } => (source < *width_bits).then_some(source),
            ButtonMap::Bits { routes } => routes
                .iter()
                .find(|route| route.source == source)
                .map(|route| route.output),
        }
    }
}

/// Explicit Training conversion. Absent scales refuse that stick encoding.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingQuantizeOptions {
    /// Applied to [`Stick::F32`]. `None` means an f32 stick fails.
    pub f32_scale: Option<LinearScale>,
    /// Applied to [`Stick::I8`]. `None` means an i8 stick fails.
    pub i8_scale: Option<LinearScale>,
    pub buttons: ButtonMap,
}

impl TrainingQuantizeOptions {
    pub fn checked(&self) -> Result<(), AdapterError> {
        let mut reports = Vec::new();
        if let Some(scale) = self.f32_scale {
            if let Err(err) = scale.checked() {
                reports.push(err);
            }
        }
        if let Some(scale) = self.i8_scale {
            if let Err(err) = scale.checked() {
                reports.push(err);
            }
        }
        if let Err(err) = self.buttons.checked() {
            reports.push(err);
        }
        if reports.is_empty() {
            Ok(())
        } else {
            Err(AdapterError { reports })
        }
    }
}

fn apply_scale(native: f32, scale: &LinearScale, path: &str) -> Result<i8, AdapterReport> {
    if !native.is_finite() {
        return Err(report(ReportCode::NonFinite, path, "axis is not finite"));
    }
    let max = f32::from(scale.max_abs);
    let scaled = (native - scale.origin) / scale.span * max;
    if !scaled.is_finite() {
        return Err(report(
            ReportCode::NonFinite,
            path,
            "scale produced a non-finite value",
        ));
    }
    let adjusted = match scale.rounding {
        ScaleRounding::HalfAwayFromZero => {
            if scaled >= 0.0 {
                scaled + 0.5
            } else {
                scaled - 0.5
            }
        }
        ScaleRounding::TowardZero => scaled,
    };
    let mut out = adjusted.trunc();
    if out > max || out < -max {
        if scale.saturate {
            out = out.clamp(-max, max);
        } else {
            return Err(report(
                ReportCode::OutOfRange,
                path,
                format!("scaled value {out} is outside ±{max}"),
            ));
        }
    }
    Ok(out as i8)
}

fn quantize_axis(native: f32, scale: Option<LinearScale>, path: &str) -> Result<i8, AdapterReport> {
    let Some(scale) = scale else {
        return Err(report(
            ReportCode::UndeclaredScale,
            path,
            "this stick encoding has no declared scale",
        ));
    };
    apply_scale(native, &scale, path)
}

fn quantize_stick(
    stick: Stick,
    options: &TrainingQuantizeOptions,
    path: &str,
) -> Result<TrainingStick, AdapterReport> {
    let (x, y) = match stick {
        Stick::F32 { x, y } => (
            quantize_axis(x, options.f32_scale, &format!("{path}.x"))?,
            quantize_axis(y, options.f32_scale, &format!("{path}.y"))?,
        ),
        Stick::I8 { x, y } => (
            quantize_axis(f32::from(x), options.i8_scale, &format!("{path}.x"))?,
            quantize_axis(f32::from(y), options.i8_scale, &format!("{path}.y"))?,
        ),
    };
    Ok(TrainingStick::SignedByte { x, y })
}

fn quantize_buttons(buttons: Buttons, map: &ButtonMap, path: &str) -> Result<u32, AdapterReport> {
    let mut out = 0u32;
    for bit in 0..32 {
        if !buttons.is_set(bit) {
            continue;
        }
        match map.output_bit(bit) {
            Some(dest) => out |= 1u32 << dest,
            None => {
                return Err(report(
                    ReportCode::UnmappedButton,
                    path,
                    format!("source bit {bit} is set and has no declared output bit"),
                ));
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Clip request
// ---------------------------------------------------------------------------

/// Serializable selection point. Converted to [`SelectAt`] without treating a
/// record ordinal as a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipPoint {
    Seq(u64),
    Tick(u64),
    Record(u64),
}

impl From<ClipPoint> for SelectAt {
    fn from(point: ClipPoint) -> Self {
        match point {
            ClipPoint::Seq(seq) => SelectAt::Seq(seq),
            ClipPoint::Tick(tick) => SelectAt::Tick(tick),
            ClipPoint::Record(record) => SelectAt::Record(record),
        }
    }
}

impl fmt::Display for ClipPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClipPoint::Seq(n) => write!(f, "seq {n}"),
            ClipPoint::Tick(n) => write!(f, "tick {n}"),
            ClipPoint::Record(n) => write!(f, "record {n}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipSpec {
    pub actor: ActorId,
    pub start: SelectAt,
    pub end: SelectAt,
    /// A producer-declared truncation may be read. Incomplete files still fail.
    pub allow_declared_truncation: bool,
}

impl ClipSpec {
    pub fn new(actor: ActorId, start: SelectAt, end: SelectAt) -> Self {
        ClipSpec {
            actor,
            start,
            end,
            allow_declared_truncation: false,
        }
    }

    pub fn allow_declared_truncation(mut self) -> Self {
        self.allow_declared_truncation = true;
        self
    }
}

/// Saved selection. `start` is the clip-start context; `end` is the clip body end
/// under [`crate::clip::clip_events`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipFile {
    pub actor: ActorId,
    pub start: ClipPoint,
    pub end: ClipPoint,
    #[serde(default)]
    pub allow_declared_truncation: bool,
}

impl From<&ClipFile> for ClipSpec {
    fn from(file: &ClipFile) -> Self {
        ClipSpec {
            actor: file.actor,
            start: file.start.into(),
            end: file.end.into(),
            allow_declared_truncation: file.allow_declared_truncation,
        }
    }
}

fn select_label(at: SelectAt) -> String {
    match at {
        SelectAt::Seq(n) => format!("seq {n}"),
        SelectAt::Tick(n) => format!("tick {n}"),
        SelectAt::Record(n) => format!("record {n}"),
    }
}

fn clock_missing_detail(capture: &Capture, at: SelectAt) -> String {
    let mut detail = format!("{} does not occur in this capture", select_label(at));
    if matches!(at, SelectAt::Tick(_))
        && !capture.header().clocks.supplied.contains(&ClockKind::Tick)
    {
        detail.push_str(
            "; this capture does not supply gameplay ticks, and a replay record ordinal is not a tick",
        );
    }
    detail
}

// ---------------------------------------------------------------------------
// Shared output values
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "snake_case",
    bound(deserialize = "T: serde::Deserialize<'de>")
)]
pub enum ExportValue<T> {
    Value {
        value: T,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        carried_from: Option<u64>,
    },
    Missing {
        reason: String,
    },
    Unsupported,
    NoSample,
}

impl<T> ExportValue<T> {
    fn from_resolved(resolved: &Resolved<T>) -> Self
    where
        T: Clone,
    {
        match resolved {
            Resolved::Observed {
                value,
                value_seq,
                latest_seq,
                ..
            } => ExportValue::Value {
                value: value.clone(),
                carried_from: (value_seq != latest_seq).then_some(*value_seq),
            },
            Resolved::Missing(reason) => ExportValue::Missing {
                reason: missing_text(reason),
            },
            Resolved::Unsupported => ExportValue::Unsupported,
            Resolved::NoSample => ExportValue::NoSample,
        }
    }
}

fn missing_text(reason: &MissingReason) -> String {
    match reason {
        MissingReason::SampleMissing { seq } => format!("sample at seq {seq} is missing"),
        MissingReason::UnchangedWithoutPredecessor { seq } => {
            format!("unchanged at seq {seq} has no predecessor in this lifetime")
        }
        MissingReason::GapAfterLatestSample { gap_seq } => {
            format!("gap at seq {gap_seq} follows the latest sample")
        }
        MissingReason::GapBeforePredecessor { gap_seq } => {
            format!("gap at seq {gap_seq} breaks the unchanged chain")
        }
    }
}

fn missing_code(reason: &MissingReason) -> ReportCode {
    match reason {
        MissingReason::SampleMissing { .. } => ReportCode::Missing,
        MissingReason::UnchangedWithoutPredecessor { .. } => ReportCode::UnresolvedUnchanged,
        MissingReason::GapAfterLatestSample { .. } | MissingReason::GapBeforePredecessor { .. } => {
            ReportCode::Gap
        }
    }
}

struct Held<T> {
    value: T,
    carried_from: Option<u64>,
    phase: Option<Phase>,
    latest_seq: u64,
}

fn require_value<T: Clone>(resolved: &Resolved<T>, path: &str) -> Result<Held<T>, AdapterReport> {
    match resolved {
        Resolved::Observed {
            value,
            value_seq,
            latest_seq,
            phase,
        } => Ok(Held {
            value: value.clone(),
            carried_from: (value_seq != latest_seq).then_some(*value_seq),
            phase: *phase,
            latest_seq: *latest_seq,
        }),
        Resolved::Missing(reason) => Err(report(missing_code(reason), path, missing_text(reason))),
        Resolved::Unsupported => Err(report(
            ReportCode::Unsupported,
            path,
            "channel is unsupported",
        )),
        Resolved::NoSample => Err(report(
            ReportCode::NoSample,
            path,
            "no sample in this lifetime segment",
        )),
    }
}

fn check_vec3(value: Vec3, path: &str) -> Result<(), AdapterReport> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(report(ReportCode::NonFinite, path, "vector is not finite"))
    }
}

fn check_kinematics(value: &Kinematics, path: &str) -> Result<(), AdapterReport> {
    check_vec3(value.position, &format!("{path}.position"))?;
    if !value.facing.is_finite() {
        return Err(report(
            ReportCode::NonFinite,
            format!("{path}.facing"),
            "facing is not finite",
        ));
    }
    if let Some(velocity) = value.velocity {
        check_vec3(velocity, &format!("{path}.velocity"))?;
    }
    Ok(())
}

fn check_situation(value: &Situation, path: &str) -> Result<(), AdapterReport> {
    if value
        .hitstop_frames
        .is_some_and(|frames| !frames.is_finite())
    {
        return Err(report(
            ReportCode::NonFinite,
            format!("{path}.hitstop_frames"),
            "hitstop is not finite",
        ));
    }
    Ok(())
}

fn check_animation(value: &Animation, path: &str) -> Result<(), AdapterReport> {
    if !value.frame.is_finite() || !value.rate.is_finite() {
        return Err(report(
            ReportCode::NonFinite,
            path,
            "animation frame or rate is not finite",
        ));
    }
    if let Some(rotation) = value.root_rotation {
        if !rotation.is_finite() {
            return Err(report(
                ReportCode::NonFinite,
                format!("{path}.root_rotation"),
                "root rotation is not finite",
            ));
        }
    }
    if let Some(scale) = value.root_scale {
        check_vec3(scale, &format!("{path}.root_scale"))?;
    }
    if let Some(secondary) = &value.secondary {
        if !secondary.frame.is_finite() || !secondary.weight.is_finite() {
            return Err(report(
                ReportCode::NonFinite,
                format!("{path}.secondary"),
                "secondary animation is not finite",
            ));
        }
    }
    Ok(())
}

fn check_vitals(value: &Vitals, path: &str) -> Result<(), AdapterReport> {
    if !value.damage.is_finite() || value.shield.is_some_and(|shield| !shield.is_finite()) {
        return Err(report(
            ReportCode::NonFinite,
            path,
            "damage or shield is not finite",
        ));
    }
    Ok(())
}

fn check_transform(value: &Transform, path: &str) -> Result<(), AdapterReport> {
    check_vec3(value.position, &format!("{path}.position"))?;
    if let Some(rotation) = value.rotation {
        if !rotation.is_finite() {
            return Err(report(
                ReportCode::NonFinite,
                format!("{path}.rotation"),
                "rotation is not finite",
            ));
        }
    }
    if let Some(scale) = value.scale {
        check_vec3(scale, &format!("{path}.scale"))?;
    }
    if let Some(velocity) = value.velocity {
        check_vec3(velocity, &format!("{path}.velocity"))?;
    }
    Ok(())
}

fn check_stick(stick: Stick, path: &str) -> Result<(), AdapterReport> {
    if stick.is_finite() {
        Ok(())
    } else {
        Err(report(ReportCode::NonFinite, path, "stick is not finite"))
    }
}

fn export_checked<T: Clone>(
    resolved: &Resolved<T>,
    path: &str,
    check: impl FnOnce(&T, &str) -> Result<(), AdapterReport>,
) -> Result<ExportValue<T>, AdapterReport> {
    if let Resolved::Observed { value, .. } = resolved {
        check(value, path)?;
    }
    Ok(ExportValue::from_resolved(resolved))
}

// ---------------------------------------------------------------------------
// Prepared clip
// ---------------------------------------------------------------------------

struct Prepared {
    /// Index of the first body event.
    from: usize,
    /// Exclusive end index of the body.
    to: usize,
    anchor_seq: u64,
    anchor_clocks: Clocks,
    identity: ActorIdentity,
    kinematics: Held<Kinematics>,
    situation: Held<Situation>,
    animation: ExportValue<Animation>,
    vitals: ExportValue<Vitals>,
    truncated: bool,
    truncation_reason: Option<String>,
}

struct Span {
    from: usize,
    to: usize,
    truncated: bool,
    truncation_reason: Option<String>,
}

fn capture_span(capture: &Capture, spec: &ClipSpec) -> Result<Span, AdapterError> {
    let (truncated, truncation_reason) = match &capture.completeness {
        Completeness::Complete => (false, None),
        Completeness::DeclaredTruncated { reason } if spec.allow_declared_truncation => {
            (true, reason.clone())
        }
        Completeness::DeclaredTruncated { reason } => {
            return Err(AdapterError::one(
                ReportCode::Truncated,
                "capture",
                match reason {
                    Some(reason) => format!("capture is truncated: {reason}"),
                    None => "capture is truncated".to_string(),
                },
            ));
        }
        Completeness::Incomplete { problems } => {
            return Err(AdapterError::one(
                ReportCode::Incomplete,
                "capture",
                format!("capture is incomplete: {problems:?}"),
            ));
        }
    };

    let from = selection_end_for(&capture.events, spec.start, spec.actor).ok_or_else(|| {
        AdapterError::one(
            ReportCode::ClockUnavailable,
            "start",
            clock_missing_detail(capture, spec.start),
        )
    })?;
    let to = selection_end_for(&capture.events, spec.end, spec.actor).ok_or_else(|| {
        AdapterError::one(
            ReportCode::ClockUnavailable,
            "end",
            clock_missing_detail(capture, spec.end),
        )
    })?;
    if to < from {
        return Err(AdapterError::one(
            ReportCode::EndBeforeStart,
            "clip",
            format!(
                "end {} is before start {}",
                select_label(spec.end),
                select_label(spec.start)
            ),
        ));
    }
    Ok(Span {
        from,
        to,
        truncated,
        truncation_reason,
    })
}

fn prepare(capture: &Capture, spec: &ClipSpec) -> Result<Prepared, AdapterError> {
    let span = capture_span(capture, spec)?;
    let from = span.from;
    let to = span.to;
    let truncated = span.truncated;
    let truncation_reason = span.truncation_reason;

    let ctx = capture
        .clip_context_for(spec.start, spec.actor)
        .ok_or_else(|| {
            AdapterError::one(
                ReportCode::ClockUnavailable,
                "start",
                clock_missing_detail(capture, spec.start),
            )
        })?;
    let Some(actor) = ctx.actor(spec.actor) else {
        return Err(AdapterError::one(
            ReportCode::ActorAbsent,
            spec.actor.to_string(),
            "actor has not spawned at the clip start",
        ));
    };
    if actor.status != ActorStatus::Alive {
        return Err(AdapterError::one(
            ReportCode::ActorAbsent,
            spec.actor.to_string(),
            format!("actor is {:?} at the clip start", actor.status),
        ));
    }

    let path = spec.actor.to_string();
    let mut reports = Vec::new();
    let kinematics = require_value(&actor.sample.kinematics, &format!("{path}.kinematics"));
    let situation = require_value(&actor.sample.situation, &format!("{path}.situation"));
    if let Ok(held) = &kinematics {
        if let Err(err) = check_kinematics(&held.value, &format!("{path}.kinematics")) {
            reports.push(err);
        }
    }
    if let Ok(held) = &situation {
        if let Err(err) = check_situation(&held.value, &format!("{path}.situation")) {
            reports.push(err);
        }
    }
    let animation = export_checked(
        &actor.sample.animation,
        &format!("{path}.animation"),
        check_animation,
    );
    let vitals = export_checked(
        &actor.sample.vitals,
        &format!("{path}.vitals"),
        check_vitals,
    );
    if let Err(err) = &kinematics {
        reports.push(err.clone());
    }
    if let Err(err) = &situation {
        reports.push(err.clone());
    }
    if let Err(err) = &animation {
        reports.push(err.clone());
    }
    if let Err(err) = &vitals {
        reports.push(err.clone());
    }
    if !reports.is_empty() {
        return Err(AdapterError { reports });
    }
    let kinematics = kinematics.expect("checked");
    let situation = situation.expect("checked");
    let anchor_seq = kinematics.latest_seq;
    let anchor_clocks = capture
        .events
        .iter()
        .find(|event| event.seq == anchor_seq)
        .map(|event| event.clocks)
        .unwrap_or(ctx.clocks);

    Ok(Prepared {
        from,
        to,
        anchor_seq,
        anchor_clocks,
        identity: actor.identity.clone(),
        kinematics,
        situation,
        animation: animation.expect("checked"),
        vitals: vitals.expect("checked"),
        truncated,
        truncation_reason,
    })
}

fn body_events(events: &[Event], from: usize, to: usize) -> impl Iterator<Item = (usize, &Event)> {
    events[from..to]
        .iter()
        .enumerate()
        .map(move |(offset, event)| (from + offset, event))
}

// ---------------------------------------------------------------------------
// Input resolution (same lifetime rules as clip:: for fighter channels)
// ---------------------------------------------------------------------------

enum Mark<T> {
    Sample { seq: u64, channel: Channel<T> },
    Gap { seq: u64 },
}

fn resolve_marks<T: Clone>(marks: &[Mark<T>]) -> Resolved<T> {
    let mut state = Walk::Open;
    for mark in marks.iter().rev() {
        if matches!(state, Walk::Done(_)) {
            break;
        }
        state = match state {
            Walk::Done(done) => Walk::Done(done),
            Walk::Open => match mark {
                Mark::Sample { seq, channel } => match channel {
                    Channel::Observed(value) => Walk::Done(Resolved::Observed {
                        value: value.clone(),
                        value_seq: *seq,
                        latest_seq: *seq,
                        phase: None,
                    }),
                    Channel::Unchanged => Walk::Want(*seq),
                    Channel::Missing => {
                        Walk::Done(Resolved::Missing(MissingReason::SampleMissing {
                            seq: *seq,
                        }))
                    }
                    Channel::Unsupported => Walk::Done(Resolved::Unsupported),
                },
                Mark::Gap { seq } => {
                    Walk::Done(Resolved::Missing(MissingReason::GapAfterLatestSample {
                        gap_seq: *seq,
                    }))
                }
            },
            Walk::Want(latest) => match mark {
                Mark::Sample { seq, channel } => match channel {
                    Channel::Observed(value) => Walk::Done(Resolved::Observed {
                        value: value.clone(),
                        value_seq: *seq,
                        latest_seq: latest,
                        phase: None,
                    }),
                    Channel::Unchanged => Walk::Want(latest),
                    Channel::Missing => Walk::Done(Resolved::Missing(
                        MissingReason::UnchangedWithoutPredecessor { seq: latest },
                    )),
                    Channel::Unsupported => Walk::Done(Resolved::Unsupported),
                },
                Mark::Gap { seq } => {
                    Walk::Done(Resolved::Missing(MissingReason::GapBeforePredecessor {
                        gap_seq: *seq,
                    }))
                }
            },
        };
    }
    match state {
        Walk::Open => Resolved::NoSample,
        Walk::Want(latest) => {
            Resolved::Missing(MissingReason::UnchangedWithoutPredecessor { seq: latest })
        }
        Walk::Done(resolved) => resolved,
    }
}

enum Walk<T> {
    Open,
    Want(u64),
    Done(Resolved<T>),
}

fn same_input_stream(input: &InputEvent, sample: &InputEvent) -> bool {
    input.origin == sample.origin
        && input.layout == sample.layout
        && input.controller == sample.controller
        && input.entry == sample.entry
}

/// Seq of the actor's latest lifecycle at or before `at_seq`.
fn lifetime_boundary(events: &[Event], actor: ActorId, at_seq: u64) -> Option<u64> {
    let mut boundary = None;
    for event in events {
        if event.seq > at_seq {
            break;
        }
        if let EventBody::Lifecycle { actor: id, .. } = &event.body {
            if *id == actor {
                boundary = Some(event.seq);
            }
        }
    }
    boundary
}

fn input_marks<T: Clone>(
    events: &[Event],
    actor: ActorId,
    boundary: u64,
    end_seq: u64,
    stream: &InputEvent,
    pick: impl Fn(&InputEvent) -> &Channel<T>,
) -> Vec<Mark<T>> {
    let mut marks = Vec::new();
    for event in events {
        if event.seq <= boundary || event.seq > end_seq {
            continue;
        }
        match &event.body {
            EventBody::Gap {
                actor: gap_actor, ..
            } if gap_actor.map_or(true, |id| id == actor) => {
                marks.push(Mark::Gap { seq: event.seq });
            }
            EventBody::Input(input)
                if input.actor == Some(actor) && same_input_stream(input, stream) =>
            {
                marks.push(Mark::Sample {
                    seq: event.seq,
                    channel: pick(input).clone(),
                });
            }
            _ => {}
        }
    }
    marks
}

fn resolve_input<T: Clone>(
    events: &[Event],
    actor: ActorId,
    end_seq: u64,
    pick: impl Fn(&InputEvent) -> &Channel<T>,
) -> Resolved<T> {
    let Some(boundary) = lifetime_boundary(events, actor, end_seq) else {
        return Resolved::NoSample;
    };
    let Some(stream) = events.iter().find_map(|event| {
        if event.seq != end_seq {
            return None;
        }
        match &event.body {
            EventBody::Input(input) if input.actor == Some(actor) => Some(input),
            _ => None,
        }
    }) else {
        return Resolved::NoSample;
    };
    resolve_marks(&input_marks(events, actor, boundary, end_seq, stream, pick))
}

// ---------------------------------------------------------------------------
// Training
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingStick {
    Native(Stick),
    SignedByte { x: i8, y: i8 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingControl<T> {
    pub value: T,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carried_from: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingStart {
    pub seq: u64,
    pub clocks: Clocks,
    pub phase: Option<Phase>,
    pub kinematics: Kinematics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinematics_from: Option<u64>,
    pub situation: Situation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub situation_from: Option<u64>,
    pub animation: ExportValue<Animation>,
    pub vitals: ExportValue<Vitals>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingStep {
    pub seq: u64,
    /// Copied from the input event. `tick` is set only when that event has a tick.
    pub clocks: Clocks,
    pub origin: InputOrigin,
    pub layout: String,
    pub actor: ActorId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<u8>,
    pub buttons: TrainingControl<Buttons>,
    pub left_stick: TrainingControl<TrainingStick>,
    pub right_stick: TrainingControl<TrainingStick>,
}

/// Format identity and the full capture header, once per consumer document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConsumerMetadata {
    pub format: String,
    pub format_version: FormatVersion,
    pub capture: CaptureHeader,
    pub completion: Completion,
    pub completeness: Completeness,
}

fn metadata_of(capture: &Capture) -> ConsumerMetadata {
    ConsumerMetadata {
        format: capture.manifest.format.clone(),
        format_version: capture.manifest.format_version,
        capture: capture.header().clone(),
        completion: capture.manifest.completion.clone(),
        completeness: capture.completeness.clone(),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingMacro {
    pub metadata: ConsumerMetadata,
    pub capture_id: String,
    pub source_kind: SourceKind,
    pub actor: ActorId,
    pub identity: ActorIdentity,
    pub start: TrainingStart,
    pub steps: Vec<TrainingStep>,
    /// Inputs in the clip body with no actor id. They are not assigned to this actor.
    pub skipped_unbound_inputs: u64,
    /// Inputs in the clip body for a different actor.
    pub skipped_other_actor_inputs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantize: Option<TrainingQuantizeOptions>,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation_reason: Option<String>,
}

fn stick_control(
    resolved: &Resolved<Stick>,
    quantize: Option<&TrainingQuantizeOptions>,
    path: &str,
) -> Result<TrainingControl<TrainingStick>, AdapterReport> {
    let held = require_value(resolved, path)?;
    check_stick(held.value, path)?;
    let value = match quantize {
        Some(options) => quantize_stick(held.value, options, path)?,
        None => TrainingStick::Native(held.value),
    };
    Ok(TrainingControl {
        value,
        carried_from: held.carried_from,
    })
}

/// Training macro for `spec.actor`. Inputs without that actor id are counted and skipped.
pub fn training_macro(
    capture: &Capture,
    spec: &ClipSpec,
    quantize: Option<&TrainingQuantizeOptions>,
) -> Result<TrainingMacro, AdapterError> {
    if let Some(options) = quantize {
        options.checked()?;
    }
    let prepared = prepare(capture, spec)?;
    let mut reports = Vec::new();
    let mut steps = Vec::new();
    let mut skipped_unbound_inputs = 0u64;
    let mut skipped_other_actor_inputs = 0u64;
    for (_, event) in body_events(&capture.events, prepared.from, prepared.to) {
        let EventBody::Input(input) = &event.body else {
            continue;
        };
        match input.actor {
            Some(actor) if actor == spec.actor => {}
            Some(_) => {
                skipped_other_actor_inputs += 1;
                continue;
            }
            None => {
                skipped_unbound_inputs += 1;
                continue;
            }
        }
        let path = format!("{} seq {}", spec.actor, event.seq);
        let buttons_resolved = resolve_input(&capture.events, spec.actor, event.seq, |input| {
            &input.buttons
        });
        let left_resolved = resolve_input(&capture.events, spec.actor, event.seq, |input| {
            &input.left_stick
        });
        let right_resolved = resolve_input(&capture.events, spec.actor, event.seq, |input| {
            &input.right_stick
        });
        let buttons =
            require_value(&buttons_resolved, &format!("{path}.buttons")).and_then(|held| {
                let bits = match quantize {
                    Some(options) => {
                        quantize_buttons(held.value, &options.buttons, &format!("{path}.buttons"))?
                    }
                    None => held.value.bits,
                };
                Ok(TrainingControl {
                    value: Buttons::new(bits),
                    carried_from: held.carried_from,
                })
            });
        let left = stick_control(&left_resolved, quantize, &format!("{path}.left_stick"));
        let right = stick_control(&right_resolved, quantize, &format!("{path}.right_stick"));
        match (buttons, left, right) {
            (Ok(buttons), Ok(left_stick), Ok(right_stick)) => steps.push(TrainingStep {
                seq: event.seq,
                clocks: event.clocks,
                origin: input.origin,
                layout: input.layout.clone(),
                actor: spec.actor,
                controller: input.controller,
                entry: input.entry,
                buttons,
                left_stick,
                right_stick,
            }),
            (buttons, left, right) => {
                for result in [buttons.map(|_| ()), left.map(|_| ()), right.map(|_| ())] {
                    if let Err(err) = result {
                        reports.push(err);
                    }
                }
            }
        }
    }
    if !reports.is_empty() {
        return Err(AdapterError { reports });
    }
    Ok(TrainingMacro {
        metadata: metadata_of(capture),
        capture_id: capture.header().capture_id.clone(),
        source_kind: capture.header().source.kind,
        actor: spec.actor,
        identity: prepared.identity,
        start: TrainingStart {
            seq: prepared.anchor_seq,
            clocks: prepared.anchor_clocks,
            phase: prepared.kinematics.phase,
            kinematics: prepared.kinematics.value,
            kinematics_from: prepared.kinematics.carried_from,
            situation: prepared.situation.value,
            situation_from: prepared.situation.carried_from,
            animation: prepared.animation,
            vitals: prepared.vitals,
        },
        steps,
        skipped_unbound_inputs,
        skipped_other_actor_inputs,
        quantize: quantize.cloned(),
        truncated: prepared.truncated,
        truncation_reason: prepared.truncation_reason,
    })
}

// ---------------------------------------------------------------------------
// Playback export
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaybackFrame {
    pub seq: u64,
    pub clocks: Clocks,
    pub phase: Option<Phase>,
    pub status: String,
    pub identity: ActorIdentity,
    pub kinematics: ExportValue<Kinematics>,
    pub situation: ExportValue<Situation>,
    pub animation: ExportValue<Animation>,
    pub vitals: ExportValue<Vitals>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaybackObjectSample {
    pub seq: u64,
    pub clocks: Clocks,
    pub alive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub despawn_cause: Option<String>,
    pub transform: ExportValue<Transform>,
    pub animation: ExportValue<Animation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaybackObject {
    pub object: ObjectId,
    pub identity: ObjectIdentity,
    pub samples: Vec<PlaybackObjectSample>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaceMark {
    pub seq: u64,
    pub clocks: Clocks,
    pub pace: Pace,
}

/// Engine playback/state data: animation playback positions, transforms and objects.
/// There is no image, pixel buffer or renderer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaybackClip {
    pub metadata: ConsumerMetadata,
    pub capture_id: String,
    pub source_kind: SourceKind,
    pub actor: ActorId,
    pub identity: ActorIdentity,
    pub start_seq: u64,
    pub start_clocks: Clocks,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pace_at_start: Option<PaceMark>,
    pub later_pace: Vec<PaceMark>,
    pub frames: Vec<PlaybackFrame>,
    pub objects: Vec<PlaybackObject>,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation_reason: Option<String>,
}

fn status_name(status: ActorStatus) -> String {
    match status {
        ActorStatus::Alive => "alive".to_string(),
        ActorStatus::Dead => "dead".to_string(),
        ActorStatus::Despawned => "despawned".to_string(),
    }
}

fn frame_from_resolved(
    seq: u64,
    clocks: Clocks,
    phase: Option<Phase>,
    actor: &ResolvedActor,
) -> Result<PlaybackFrame, AdapterReport> {
    let path = actor.actor.to_string();
    if let Resolved::Observed { value, .. } = &actor.sample.kinematics {
        check_kinematics(value, &format!("{path}.kinematics"))?;
    }
    if let Resolved::Observed { value, .. } = &actor.sample.situation {
        check_situation(value, &format!("{path}.situation"))?;
    }
    let animation = export_checked(
        &actor.sample.animation,
        &format!("{path}.animation"),
        check_animation,
    )?;
    let vitals = export_checked(
        &actor.sample.vitals,
        &format!("{path}.vitals"),
        check_vitals,
    )?;
    Ok(PlaybackFrame {
        seq,
        clocks,
        phase,
        status: status_name(actor.status),
        identity: actor.identity.clone(),
        kinematics: ExportValue::from_resolved(&actor.sample.kinematics),
        situation: ExportValue::from_resolved(&actor.sample.situation),
        animation,
        vitals,
    })
}

fn selected_object_sample(
    object: &ResolvedObject,
    seq: u64,
    clocks: Clocks,
) -> Result<PlaybackObjectSample, AdapterReport> {
    if let Resolved::Observed { value, .. } = &object.transform {
        check_transform(value, &format!("{}.transform", object.object))?;
    }
    let animation = export_checked(
        &object.animation,
        &format!("{}.animation", object.object),
        check_animation,
    )?;
    Ok(PlaybackObjectSample {
        seq,
        clocks,
        alive: true,
        despawn_cause: None,
        transform: ExportValue::from_resolved(&object.transform),
        animation,
    })
}

fn object_sample_at(
    events: &[Event],
    index: usize,
    object: ObjectId,
    alive: bool,
    despawn_cause: Option<String>,
) -> Result<Option<(ObjectIdentity, PlaybackObjectSample)>, AdapterReport> {
    let prefix_end = if alive { index + 1 } else { index };
    let Some(resolved) = resolve_object(&events[..prefix_end], object) else {
        return Ok(None);
    };
    if let Resolved::Observed { value, .. } = &resolved.transform {
        check_transform(value, &format!("{object}.transform"))?;
    }
    let animation = export_checked(
        &resolved.animation,
        &format!("{object}.animation"),
        check_animation,
    )?;
    let event = &events[index];
    let (transform, animation) = if alive {
        (ExportValue::from_resolved(&resolved.transform), animation)
    } else {
        (ExportValue::NoSample, ExportValue::NoSample)
    };
    Ok(Some((
        resolved.identity,
        PlaybackObjectSample {
            seq: event.seq,
            clocks: event.clocks,
            alive,
            despawn_cause,
            transform,
            animation,
        },
    )))
}

fn push_object(
    objects: &mut Vec<PlaybackObject>,
    id: ObjectId,
    identity: ObjectIdentity,
    sample: PlaybackObjectSample,
) {
    if let Some(existing) = objects.iter_mut().find(|object| object.object == id) {
        existing.identity = identity;
        existing.samples.push(sample);
    } else {
        objects.push(PlaybackObject {
            object: id,
            identity,
            samples: vec![sample],
        });
    }
}

fn actor_context_phase(events: &[Event], actor: ActorId) -> Option<Phase> {
    events.iter().rev().find_map(|event| match &event.body {
        EventBody::State {
            actor: id, phase, ..
        } if *id == actor => Some(*phase),
        _ => None,
    })
}

/// Playback data for the selected actor and the scene objects visible in the clip.
pub fn playback_clip(capture: &Capture, spec: &ClipSpec) -> Result<PlaybackClip, AdapterError> {
    let span = capture_span(capture, spec)?;
    let ctx = capture
        .clip_context_for(spec.start, spec.actor)
        .ok_or_else(|| {
            AdapterError::one(
                ReportCode::ClockUnavailable,
                "start",
                clock_missing_detail(capture, spec.start),
            )
        })?;
    let Some(actor) = ctx.actor(spec.actor) else {
        return Err(AdapterError::one(
            ReportCode::ActorAbsent,
            spec.actor.to_string(),
            "actor has not spawned at the clip start",
        ));
    };
    let start_seq = ctx.seq.unwrap_or(0);
    let start_clocks = ctx.clocks;
    let start_phase = actor_context_phase(&capture.events[..span.from], spec.actor);
    let mut reports = Vec::new();
    let mut frames = Vec::new();
    match frame_from_resolved(start_seq, start_clocks, start_phase, actor) {
        Ok(frame) => frames.push(frame),
        Err(err) => reports.push(err),
    }

    let mut objects = Vec::new();
    for object in &ctx.objects {
        match selected_object_sample(object, start_seq, start_clocks) {
            Ok(sample) => push_object(&mut objects, object.object, object.identity.clone(), sample),
            Err(err) => reports.push(err),
        }
    }

    let mut later_pace = Vec::new();
    for (index, event) in body_events(&capture.events, span.from, span.to) {
        match &event.body {
            EventBody::Pace(pace) => later_pace.push(PaceMark {
                seq: event.seq,
                clocks: event.clocks,
                pace: pace.clone(),
            }),
            EventBody::State {
                actor: id, phase, ..
            } if *id == spec.actor => {
                if let Some(resolved) = resolve_actor(&capture.events[..=index], spec.actor) {
                    match frame_from_resolved(event.seq, event.clocks, Some(*phase), &resolved) {
                        Ok(frame) => frames.push(frame),
                        Err(err) => reports.push(err),
                    }
                }
            }
            EventBody::Lifecycle { actor: id, .. } if *id == spec.actor => {
                if let Some(resolved) = resolve_actor(&capture.events[..=index], spec.actor) {
                    match frame_from_resolved(event.seq, event.clocks, None, &resolved) {
                        Ok(frame) => frames.push(frame),
                        Err(err) => reports.push(err),
                    }
                }
            }
            EventBody::Gap { actor: id, .. } if id.map_or(true, |gap| gap == spec.actor) => {
                if let Some(resolved) = resolve_actor(&capture.events[..=index], spec.actor) {
                    match frame_from_resolved(event.seq, event.clocks, None, &resolved) {
                        Ok(frame) => frames.push(frame),
                        Err(err) => reports.push(err),
                    }
                }
                if id.is_none() {
                    let ids: Vec<ObjectId> = objects.iter().map(|object| object.object).collect();
                    for id in ids {
                        let Some(resolved) = resolve_object(&capture.events[..=index], id) else {
                            continue;
                        };
                        match selected_object_sample(&resolved, event.seq, event.clocks) {
                            Ok(sample) => {
                                push_object(&mut objects, id, resolved.identity.clone(), sample)
                            }
                            Err(err) => reports.push(err),
                        }
                    }
                }
            }
            EventBody::Object { object, event: obj } => match obj {
                ObjectEvent::Despawn { cause } => {
                    match object_sample_at(&capture.events, index, *object, false, cause.clone()) {
                        Ok(Some((identity, sample))) => {
                            push_object(&mut objects, *object, identity, sample)
                        }
                        Ok(None) => {}
                        Err(err) => reports.push(err),
                    }
                }
                ObjectEvent::Spawn { .. } | ObjectEvent::State { .. } => {
                    match object_sample_at(&capture.events, index, *object, true, None) {
                        Ok(Some((identity, sample))) => {
                            push_object(&mut objects, *object, identity, sample)
                        }
                        Ok(None) => {}
                        Err(err) => reports.push(err),
                    }
                }
            },
            _ => {}
        }
    }
    if !reports.is_empty() {
        return Err(AdapterError { reports });
    }
    Ok(PlaybackClip {
        metadata: metadata_of(capture),
        capture_id: capture.header().capture_id.clone(),
        source_kind: capture.header().source.kind,
        actor: spec.actor,
        identity: actor.identity.clone(),
        start_seq,
        start_clocks,
        pace_at_start: ctx.pace.clone().map(|(pace, seq)| PaceMark {
            seq,
            clocks: capture
                .events
                .iter()
                .find(|event| event.seq == seq)
                .map(|event| event.clocks)
                .unwrap_or(Clocks::NONE),
            pace,
        }),
        later_pace,
        frames,
        objects,
        truncated: span.truncated,
        truncation_reason: span.truncation_reason,
    })
}

// ---------------------------------------------------------------------------
// AI rows
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: serde::Deserialize<'de>"))]
pub struct AiField<T> {
    /// Channel on the sample event. Absent when the lifetime has no sample.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Channel<T>>,
    pub resolved: ExportValue<T>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiProvenance {
    pub capture_id: String,
    pub source_kind: SourceKind,
    pub completeness: Completeness,
    pub game_title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_version: Option<String>,
    pub recorder: ToolVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoder: Option<ToolVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_sha256: Option<String>,
    pub channels: ChannelSupport,
    pub clocks_supplied: Vec<ClockKind>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiRowBody {
    Start {
        actor: ActorId,
        identity: ActorIdentity,
        phase: Option<Phase>,
        kinematics: AiField<Kinematics>,
        situation: AiField<Situation>,
        animation: AiField<Animation>,
        vitals: AiField<Vitals>,
    },
    State {
        actor: ActorId,
        phase: Phase,
        kinematics: AiField<Kinematics>,
        situation: AiField<Situation>,
        animation: AiField<Animation>,
        vitals: AiField<Vitals>,
    },
    Input {
        /// `None` is an input the capture did not attribute to an actor.
        actor: Option<ActorId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        controller: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entry: Option<u8>,
        origin: InputOrigin,
        layout: String,
        buttons: AiField<Buttons>,
        left_stick: AiField<Stick>,
        right_stick: AiField<Stick>,
    },
    Object {
        object: ObjectId,
        identity: ObjectIdentity,
        alive: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        despawn_cause: Option<String>,
        transform: AiField<Transform>,
        animation: AiField<Animation>,
    },
    Random {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        actor: Option<ActorId>,
        basis: OutcomeBasis,
        outcome: RandomOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiRow {
    pub seq: u64,
    pub clocks: Clocks,
    pub body: AiRowBody,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiDataset {
    pub metadata: ConsumerMetadata,
    pub provenance: AiProvenance,
    pub actor: ActorId,
    pub rows: Vec<AiRow>,
    pub skipped_unbound_inputs: u64,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation_reason: Option<String>,
}

fn raw_state_channel<T: Clone>(
    events: &[Event],
    actor: ActorId,
    seq: u64,
    pick: impl Fn(&FighterSample) -> &Channel<T>,
) -> Option<Channel<T>> {
    events.iter().find_map(|event| match &event.body {
        EventBody::State {
            actor: id, sample, ..
        } if *id == actor && event.seq == seq => Some(pick(sample).clone()),
        _ => None,
    })
}

fn ai_fighter_field<T: Clone>(
    events: &[Event],
    actor: ActorId,
    resolved: &Resolved<T>,
    pick: impl Fn(&FighterSample) -> &Channel<T>,
) -> AiField<T> {
    let raw = match resolved {
        Resolved::Observed { latest_seq, .. } => {
            raw_state_channel(events, actor, *latest_seq, pick)
        }
        Resolved::Missing(MissingReason::SampleMissing { seq })
        | Resolved::Missing(MissingReason::UnchangedWithoutPredecessor { seq }) => {
            raw_state_channel(events, actor, *seq, pick)
        }
        Resolved::Unsupported => Some(Channel::Unsupported),
        Resolved::Missing(_) | Resolved::NoSample => None,
    };
    AiField {
        raw,
        resolved: ExportValue::from_resolved(resolved),
    }
}

fn ai_pair<T: Clone>(raw: &Channel<T>, resolved: &Resolved<T>) -> AiField<T> {
    AiField {
        raw: Some(raw.clone()),
        resolved: ExportValue::from_resolved(resolved),
    }
}

fn provenance_of(capture: &Capture) -> AiProvenance {
    let header = capture.header();
    AiProvenance {
        capture_id: header.capture_id.clone(),
        source_kind: header.source.kind,
        completeness: capture.completeness.clone(),
        game_title: header.game.title.clone(),
        game_version: header.game.version.clone(),
        recorder: header.recorder.clone(),
        decoder: header.source.decoder.clone(),
        replay_sha256: header
            .source
            .replay
            .as_ref()
            .map(|replay| replay.sha256.clone()),
        channels: header.channels.clone(),
        clocks_supplied: header.clocks.supplied.clone(),
    }
}

/// One row per selected sample. Raw channels stay missing or unchanged when that
/// is what the producer stored; resolved values say when a predecessor was used.
pub fn ai_dataset(capture: &Capture, spec: &ClipSpec) -> Result<AiDataset, AdapterError> {
    let prepared = prepare(capture, spec)?;
    let ctx = capture
        .clip_context_for(spec.start, spec.actor)
        .expect("prepare already resolved the start");
    let actor = ctx.actor(spec.actor).expect("prepare found the actor");
    let prefix = &capture.events[..prepared.from];
    let mut rows = vec![AiRow {
        seq: prepared.anchor_seq,
        clocks: prepared.anchor_clocks,
        body: AiRowBody::Start {
            actor: spec.actor,
            identity: prepared.identity.clone(),
            phase: prepared.kinematics.phase,
            kinematics: ai_fighter_field(prefix, spec.actor, &actor.sample.kinematics, |s| {
                &s.kinematics
            }),
            situation: ai_fighter_field(prefix, spec.actor, &actor.sample.situation, |s| {
                &s.situation
            }),
            animation: ai_fighter_field(prefix, spec.actor, &actor.sample.animation, |s| {
                &s.animation
            }),
            vitals: ai_fighter_field(prefix, spec.actor, &actor.sample.vitals, |s| &s.vitals),
        },
    }];

    for object in &ctx.objects {
        rows.push(AiRow {
            seq: ctx.seq.unwrap_or(object.spawn_seq),
            clocks: ctx.clocks,
            body: AiRowBody::Object {
                object: object.object,
                identity: object.identity.clone(),
                alive: true,
                despawn_cause: None,
                transform: AiField {
                    raw: object_channel(prefix, object.object, &object.transform, |event| {
                        object_transform(event)
                    }),
                    resolved: ExportValue::from_resolved(&object.transform),
                },
                animation: AiField {
                    raw: object_channel(prefix, object.object, &object.animation, |event| {
                        object_animation(event)
                    }),
                    resolved: ExportValue::from_resolved(&object.animation),
                },
            },
        });
    }

    let mut skipped_unbound_inputs = 0u64;
    for (index, event) in body_events(&capture.events, prepared.from, prepared.to) {
        match &event.body {
            EventBody::State {
                actor,
                phase,
                sample,
            } if *actor == spec.actor => {
                let Some(resolved) = resolve_actor(&capture.events[..=index], spec.actor) else {
                    continue;
                };
                rows.push(AiRow {
                    seq: event.seq,
                    clocks: event.clocks,
                    body: AiRowBody::State {
                        actor: spec.actor,
                        phase: *phase,
                        kinematics: ai_pair(&sample.kinematics, &resolved.sample.kinematics),
                        situation: ai_pair(&sample.situation, &resolved.sample.situation),
                        animation: ai_pair(&sample.animation, &resolved.sample.animation),
                        vitals: ai_pair(&sample.vitals, &resolved.sample.vitals),
                    },
                });
            }
            EventBody::Input(input) => match input.actor {
                Some(actor) if actor == spec.actor => {
                    let buttons =
                        resolve_input(&capture.events, spec.actor, event.seq, |item| &item.buttons);
                    let left = resolve_input(&capture.events, spec.actor, event.seq, |item| {
                        &item.left_stick
                    });
                    let right = resolve_input(&capture.events, spec.actor, event.seq, |item| {
                        &item.right_stick
                    });
                    rows.push(AiRow {
                        seq: event.seq,
                        clocks: event.clocks,
                        body: AiRowBody::Input {
                            actor: Some(spec.actor),
                            controller: input.controller,
                            entry: input.entry,
                            origin: input.origin,
                            layout: input.layout.clone(),
                            buttons: ai_pair(&input.buttons, &buttons),
                            left_stick: ai_pair(&input.left_stick, &left),
                            right_stick: ai_pair(&input.right_stick, &right),
                        },
                    });
                }
                None => {
                    skipped_unbound_inputs += 1;
                    rows.push(AiRow {
                        seq: event.seq,
                        clocks: event.clocks,
                        body: AiRowBody::Input {
                            actor: None,
                            controller: input.controller,
                            entry: input.entry,
                            origin: input.origin,
                            layout: input.layout.clone(),
                            buttons: unbound_field(&input.buttons),
                            left_stick: unbound_field(&input.left_stick),
                            right_stick: unbound_field(&input.right_stick),
                        },
                    });
                }
                Some(_) => {}
            },
            EventBody::Object { object, event: obj } => {
                let (alive, cause, raw_transform, raw_animation) = match obj {
                    ObjectEvent::Despawn { cause } => (false, cause.clone(), None, None),
                    ObjectEvent::Spawn {
                        transform,
                        animation,
                        ..
                    }
                    | ObjectEvent::State {
                        transform,
                        animation,
                        ..
                    } => (true, None, Some(*transform), Some(animation.clone())),
                };
                let resolved = if alive {
                    resolve_object(&capture.events[..=index], *object)
                } else {
                    resolve_object(&capture.events[..index], *object)
                };
                let identity = match (&resolved, obj) {
                    (Some(resolved), _) => resolved.identity.clone(),
                    (None, ObjectEvent::Spawn { identity, .. }) => identity.clone(),
                    (None, _) => ObjectIdentity::default(),
                };
                rows.push(AiRow {
                    seq: event.seq,
                    clocks: event.clocks,
                    body: AiRowBody::Object {
                        object: *object,
                        identity,
                        alive,
                        despawn_cause: cause,
                        transform: AiField {
                            raw: raw_transform,
                            resolved: resolved
                                .as_ref()
                                .map(|item| ExportValue::from_resolved(&item.transform))
                                .unwrap_or(ExportValue::NoSample),
                        },
                        animation: AiField {
                            raw: raw_animation,
                            resolved: resolved
                                .as_ref()
                                .map(|item| ExportValue::from_resolved(&item.animation))
                                .unwrap_or(ExportValue::NoSample),
                        },
                    },
                });
            }
            EventBody::Random(random)
                if random.actor.is_none() || random.actor == Some(spec.actor) =>
            {
                rows.push(AiRow {
                    seq: event.seq,
                    clocks: event.clocks,
                    body: AiRowBody::Random {
                        actor: random.actor,
                        basis: random.basis,
                        outcome: random.outcome.clone(),
                        note: random.note.clone(),
                    },
                });
            }
            _ => {}
        }
    }

    Ok(AiDataset {
        metadata: metadata_of(capture),
        provenance: provenance_of(capture),
        actor: spec.actor,
        rows,
        skipped_unbound_inputs,
        truncated: prepared.truncated,
        truncation_reason: prepared.truncation_reason,
    })
}

fn object_transform(event: &ObjectEvent) -> Option<&Channel<Transform>> {
    match event {
        ObjectEvent::Spawn { transform, .. } | ObjectEvent::State { transform, .. } => {
            Some(transform)
        }
        ObjectEvent::Despawn { .. } => None,
    }
}

fn object_animation(event: &ObjectEvent) -> Option<&Channel<Animation>> {
    match event {
        ObjectEvent::Spawn { animation, .. } | ObjectEvent::State { animation, .. } => {
            Some(animation)
        }
        ObjectEvent::Despawn { .. } => None,
    }
}

fn object_channel<T: Clone>(
    events: &[Event],
    object: ObjectId,
    resolved: &Resolved<T>,
    pick: impl Fn(&ObjectEvent) -> Option<&Channel<T>>,
) -> Option<Channel<T>> {
    let seq = match resolved {
        Resolved::Observed { latest_seq, .. }
        | Resolved::Missing(MissingReason::SampleMissing { seq: latest_seq })
        | Resolved::Missing(MissingReason::UnchangedWithoutPredecessor { seq: latest_seq }) => {
            *latest_seq
        }
        Resolved::Unsupported => return Some(Channel::Unsupported),
        Resolved::Missing(_) | Resolved::NoSample => return None,
    };
    events.iter().find_map(|event| {
        if event.seq != seq {
            return None;
        }
        match &event.body {
            EventBody::Object {
                object: id,
                event: obj,
            } if *id == object => pick(obj).cloned(),
            _ => None,
        }
    })
}

fn unbound_field<T: Clone>(channel: &Channel<T>) -> AiField<T> {
    let resolved = match channel {
        Channel::Observed(value) => ExportValue::Value {
            value: value.clone(),
            carried_from: None,
        },
        Channel::Missing => ExportValue::Missing {
            reason: "missing".into(),
        },
        Channel::Unsupported => ExportValue::Unsupported,
        Channel::Unchanged => ExportValue::Missing {
            reason: "unchanged input has no actor lifetime".into(),
        },
    };
    AiField {
        raw: Some(channel.clone()),
        resolved,
    }
}

/// A recording slice that does not require an actor or fighter context.
/// Actor filtering keeps unattributed inputs and the complete scene-object
/// stream, so object updates retain the identity recorded at their spawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordingQuery {
    pub actor: Option<ActorId>,
    pub start: Option<SelectAt>,
    pub end: Option<SelectAt>,
}

impl RecordingQuery {
    pub fn whole() -> Self {
        RecordingQuery {
            actor: None,
            start: None,
            end: None,
        }
    }
}

/// Typed timeline rows for a capture that may have no roster. Unattributed
/// inputs stay `actor: None`. Raw source units are kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiRecording {
    pub metadata: ConsumerMetadata,
    pub actor: Option<ActorId>,
    pub events: Vec<Event>,
}

pub fn ai_recording(
    capture: &Capture,
    query: &RecordingQuery,
) -> Result<AiRecording, AdapterError> {
    let (from, to) = match (query.start, query.end) {
        (None, None) => (0, capture.events.len()),
        (Some(start), Some(end)) => {
            let from = selection_end(&capture.events, start).ok_or_else(|| {
                AdapterError::one(
                    ReportCode::ClockUnavailable,
                    "start",
                    clock_missing_detail(capture, start),
                )
            })?;
            let to = selection_end(&capture.events, end).ok_or_else(|| {
                AdapterError::one(
                    ReportCode::ClockUnavailable,
                    "end",
                    clock_missing_detail(capture, end),
                )
            })?;
            if to < from {
                return Err(AdapterError::one(
                    ReportCode::EndBeforeStart,
                    "clip",
                    format!(
                        "end {} is before start {}",
                        select_label(end),
                        select_label(start)
                    ),
                ));
            }
            (from, to)
        }
        _ => {
            return Err(AdapterError::one(
                ReportCode::ClockUnavailable,
                "range",
                "a recording range needs both a start and an end",
            ))
        }
    };
    let events = capture.events[from..to]
        .iter()
        .filter(|event| keep_recording_event(&event.body, query.actor))
        .cloned()
        .collect();
    Ok(AiRecording {
        metadata: metadata_of(capture),
        actor: query.actor,
        events,
    })
}

fn keep_recording_event(body: &EventBody, actor: Option<ActorId>) -> bool {
    let Some(actor) = actor else {
        return true;
    };
    match body {
        EventBody::Input(input) => input.actor.map_or(true, |id| id == actor),
        EventBody::State { actor: id, .. } | EventBody::Lifecycle { actor: id, .. } => *id == actor,
        EventBody::Random(random) => random.actor.map_or(true, |id| id == actor),
        EventBody::Gap { actor: id, .. } => id.map_or(true, |id| id == actor),
        EventBody::Object { .. }
        | EventBody::Raw(_)
        | EventBody::Boundary(_)
        | EventBody::Pace(_)
        | EventBody::Note { .. }
        | EventBody::End { .. } => true,
    }
}
