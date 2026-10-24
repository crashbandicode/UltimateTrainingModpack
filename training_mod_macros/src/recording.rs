//! Portable macro data model: per-frame mapped controls, the start context a
//! macro assumes, and a relative placement that is edited independently of the
//! frames.

use std::{fmt, io};

use serde::{Deserialize, Serialize};

/// Ordinary recordings retain the original schema and serialized shape.
pub const SCHEMA_VERSION: u32 = 1;
/// Exact Mii macros must refuse on older readers that would discard their identity.
pub const MII_SCHEMA_VERSION: u32 = 2;
/// Gameplay frame rate assumed when the caller does not supply one.
pub const DEFAULT_FPS: u32 = 60;
/// Longest macro accepted: 10 minutes of gameplay at 60 fps.
///
/// A CPU reaction drill lasts seconds and the legacy recorder holds 600 frames,
/// so this is far beyond any practical replay window. It bounds a macro to
/// 288 KiB in memory (8 bytes per frame) and about 3 MiB of compact JSON
/// (87 bytes per worst-case frame), which a console parses comfortably.
pub const MAX_FRAMES: usize = 36_000;
/// Longest display name, counted in Unicode scalar values.
pub const MAX_NAME_CHARS: usize = 48;
/// Longest free-form game version tag, in bytes.
pub const MAX_GAME_VERSION_BYTES: usize = 32;

/// One frame of mapped controller input (not a native struct dump).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputFrame {
    pub buttons: u32,
    pub lstick_x: i8,
    pub lstick_y: i8,
    pub rstick_x: i8,
    pub rstick_y: i8,
}

/// Fighter context a macro assumes at its first frame.
///
/// `situation` and `status` are raw game enum values captured by the runtime;
/// this portable crate does not know them, and 0 means "not captured" exactly as
/// in the save-state default. A macro is not an arbitrary full-game save state.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StartState {
    pub fighter_kind: i32,
    pub costume: u32,
    /// Sign gives the facing direction (1.0 = right, -1.0 = left).
    pub facing: f32,
    pub situation: i32,
    pub status: i32,
    pub speed_x: f32,
    pub speed_y: f32,
}

impl Default for StartState {
    /// Mirrors the save-state default: no fighter, facing right, at rest.
    fn default() -> Self {
        Self {
            fighter_kind: -1,
            costume: 0,
            facing: 1.0,
            situation: 0,
            status: 0,
            speed_x: 0.0,
            speed_y: 0.0,
        }
    }
}

impl StartState {
    /// Neutral context (no speed, uncaptured situation/status) for a known fighter.
    pub fn neutral(fighter_kind: i32, costume: u32, facing: f32) -> Self {
        Self {
            fighter_kind,
            costume,
            facing,
            ..Self::default()
        }
    }

    fn validate(&self) -> Result<(), MacroError> {
        if self.fighter_kind < 0 {
            return Err(MacroError::MissingFighter);
        }
        finite(self.facing, "start.facing")?;
        finite(self.speed_x, "start.speed_x")?;
        finite(self.speed_y, "start.speed_y")
    }
}

/// What the runtime does with the recorded momentum when the macro starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MomentumMode {
    /// Start with the `StartState` speeds (rising/falling momentum kept).
    #[default]
    PreserveRecorded,
    /// Start at rest; ordinary physics applies afterwards (not a permanent hover).
    AtRest,
}

/// Start position relative to the CPU's restored position, in world units.
///
/// The offset is always applied to the freshly restored anchor, never to a
/// previous result, so repeated reset/play trials cannot accumulate it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub delta_x: f32,
    pub delta_y: f32,
    pub momentum: MomentumMode,
}

impl Placement {
    pub fn validate(&self) -> Result<(), MacroError> {
        finite(self.delta_x, "placement.delta_x")?;
        finite(self.delta_y, "placement.delta_y")
    }

    /// Target position for the given restored anchor position.
    pub fn apply(&self, anchor: (f32, f32)) -> (f32, f32) {
        (anchor.0 + self.delta_x, anchor.1 + self.delta_y)
    }
}

/// A named, versioned, portable CPU macro.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MacroRecording {
    pub schema_version: u32,
    pub name: String,
    /// Free-form tag of the game version it was recorded on; may be empty.
    pub game_version: String,
    pub start: StartState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mii: Option<training_mod_mii::lifetime::Configuration>,
    pub placement: Placement,
    pub frames: Vec<InputFrame>,
}

impl MacroRecording {
    /// Builds a validated recording with the default (zero, preserve) placement.
    pub fn new(
        name: impl Into<String>,
        game_version: impl Into<String>,
        start: StartState,
        frames: Vec<InputFrame>,
    ) -> Result<Self, MacroError> {
        let name: String = name.into();
        let recording = Self {
            schema_version: SCHEMA_VERSION,
            name: name.trim().to_owned(),
            game_version: game_version.into(),
            start,
            mii: None,
            placement: Placement::default(),
            frames,
        };
        recording.validate()?;
        Ok(recording)
    }

    /// Checks everything `Library` requires before it writes or accepts a file.
    pub fn validate(&self) -> Result<(), MacroError> {
        if !matches!(self.schema_version, SCHEMA_VERSION | MII_SCHEMA_VERSION) {
            return Err(MacroError::UnsupportedVersion {
                found: self.schema_version,
            });
        }
        check_name(&self.name)?;
        if self.game_version.len() > MAX_GAME_VERSION_BYTES {
            return Err(MacroError::InvalidGameVersion("is too long"));
        }
        if self.game_version.chars().any(char::is_control) {
            return Err(MacroError::InvalidGameVersion(
                "contains control characters",
            ));
        }
        self.start.validate()?;
        match (self.schema_version, self.mii) {
            (SCHEMA_VERSION, None) => {},
            (MII_SCHEMA_VERSION, Some(configuration))
                if configuration.canonical().class.kind() == self.start.fighter_kind as u32 => {},
            _ => return Err(MacroError::InvalidMii("schema, fighter class and exact configuration disagree")),
        }
        self.placement.validate()?;
        if self.frames.is_empty() {
            return Err(MacroError::EmptyFrames);
        }
        if self.frames.len() > MAX_FRAMES {
            return Err(MacroError::TooManyFrames {
                frames: self.frames.len(),
                max: MAX_FRAMES,
            });
        }
        Ok(())
    }

    /// Attach owned identity after qualified replay resolution. Ordinary files
    /// retain schema1 and their existing JSON representation.
    pub fn set_mii(&mut self, configuration: training_mod_mii::lifetime::Configuration) -> Result<(), MacroError> {
        if configuration.canonical().class.kind() != self.start.fighter_kind as u32 {
            return Err(MacroError::InvalidMii("configuration belongs to a different fighter class"));
        }
        self.mii = Some(configuration);
        self.schema_version = MII_SCHEMA_VERSION;
        Ok(())
    }

    /// Legacy Mii files remain readable; no preferred Mii can stand in for their
    /// absent identity. The caller still needs native exact-target completion.
    pub fn playback_target(&self) -> Result<training_mod_mii::replay::Target, MacroError> {
        self.validate()?;
        if let Some(configuration) = self.mii {
            Ok(training_mod_mii::replay::Target::mii(configuration))
        } else if training_mod_mii::inventory::MiiClass::from_kind(self.start.fighter_kind as u32).is_some() {
            Err(MacroError::InvalidMii("this older macro has no exact Mii configuration"))
        } else {
            Ok(training_mod_mii::replay::Target::fighter(self.start.fighter_kind as u32))
        }
    }

    /// Replaces the placement metadata only; the frames are never touched.
    pub fn set_placement(&mut self, placement: Placement) -> Result<(), MacroError> {
        placement.validate()?;
        self.placement = placement;
        Ok(())
    }

    /// Renames the macro (trimmed and validated); the frames are never touched.
    pub fn set_name(&mut self, name: &str) -> Result<(), MacroError> {
        let name = name.trim();
        check_name(name)?;
        self.name = name.to_owned();
        Ok(())
    }
}

fn check_name(name: &str) -> Result<(), MacroError> {
    if name.trim().is_empty() {
        return Err(MacroError::InvalidName("is empty"));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(MacroError::InvalidName("is too long"));
    }
    if name.chars().any(char::is_control) {
        return Err(MacroError::InvalidName("contains control characters"));
    }
    Ok(())
}

fn finite(value: f32, field: &'static str) -> Result<(), MacroError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(MacroError::NonFinite(field))
    }
}

/// Every failure this crate reports.
#[derive(Debug)]
pub enum MacroError {
    UnsupportedVersion {
        found: u32,
    },
    InvalidName(&'static str),
    InvalidGameVersion(&'static str),
    InvalidMii(&'static str),
    /// `start.fighter_kind` is negative, i.e. the macro names no fighter.
    MissingFighter,
    NonFinite(&'static str),
    EmptyFrames,
    TooManyFrames {
        frames: usize,
        max: usize,
    },
    FileTooLarge {
        bytes: u64,
        max: u64,
    },
    InvalidId(String),
    NotFound(String),
    /// Every candidate id derived from the name is already taken.
    NoFreeId,
    InvalidWindow(&'static str),
    /// The snapshot was not captured at the selected in-point.
    SnapshotFrame {
        selected: u32,
        snapshot: u32,
    },
    Parse(serde_json::Error),
    Io(io::Error),
}

impl fmt::Display for MacroError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion { found } => write!(
                f,
                "unsupported schema version {found} (supported: {SCHEMA_VERSION} and {MII_SCHEMA_VERSION})"
            ),
            Self::InvalidName(why) => write!(f, "invalid name: {why}"),
            Self::InvalidGameVersion(why) => write!(f, "invalid game version: {why}"),
            Self::InvalidMii(why) => write!(f, "Mii macro unavailable: {why}"),
            Self::MissingFighter => write!(f, "start state names no fighter"),
            Self::NonFinite(field) => write!(f, "{field} is not a finite number"),
            Self::EmptyFrames => write!(f, "macro has no frames"),
            Self::TooManyFrames { frames, max } => {
                write!(f, "macro has {frames} frames (maximum {max})")
            }
            Self::FileTooLarge { bytes, max } => {
                write!(f, "file is {bytes} bytes (maximum {max})")
            }
            Self::InvalidId(id) => write!(f, "invalid macro id {id:?}"),
            Self::NotFound(id) => write!(f, "no macro with id {id:?}"),
            Self::NoFreeId => write!(f, "no free macro id for this name"),
            Self::InvalidWindow(why) => write!(f, "invalid window: {why}"),
            Self::SnapshotFrame { selected, snapshot } => write!(
                f,
                "start snapshot is for frame {snapshot}, but the selection starts at frame {selected}"
            ),
            Self::Parse(err) => write!(f, "malformed macro file: {err}"),
            Self::Io(err) => write!(f, "i/o error: {err}"),
        }
    }
}

impl std::error::Error for MacroError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(err) => Some(err),
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for MacroError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for MacroError {
    fn from(err: serde_json::Error) -> Self {
        Self::Parse(err)
    }
}

#[cfg(test)]
mod tests;
