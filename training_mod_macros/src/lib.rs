//! Portable core of the named CPU macros: the recording model, a directory
//! backed library and replay-window selection. It depends on std, serde and
//! the shared match-data module (no Skyline), so it is fully host-testable. The runtime
//! owns recording, playback, character loading and applying a placement.
//!
//! A macro file is compact JSON of [`MacroRecording`] (schema version 1).
//!
//! ```
//! use training_mod_macros::*;
//!
//! # fn main() -> Result<(), MacroError> {
//! let dir = std::env::temp_dir().join(format!("tmm-doc-{}", std::process::id()));
//! let library = Library::new(&dir);
//!
//! // Recording finished: the runtime supplies the frames and the fighter state.
//! let frames = vec![InputFrame { buttons: 1, lstick_x: 127, ..InputFrame::default() }];
//! let mut rec = MacroRecording::new("Fox up-B", "13.0.4", StartState::neutral(8, 0, 1.0), frames)?;
//! let id = library.save_new(&rec)?;
//!
//! // Edit Location later: only the placement changes, then overwrite explicitly.
//! rec.set_placement(Placement { delta_y: 40.0, ..Placement::default() })?;
//! library.overwrite(&id, &rec)?;
//! assert_eq!(library.list()?.entries[0].id, id);
//! # std::fs::remove_dir_all(&dir).ok();
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

mod capture_archive;
mod capture_settings;
mod library;
mod live_capture;
mod live_frame;
mod recording;
mod replay;
mod shared_capture;
mod window;

pub use capture_archive::{CaptureArchive, CaptureEntry, CaptureListing, CaptureSaveOutcome, SavedCapture};
pub use capture_settings::{CaptureSettings, DEFAULT_CAPTURE_INI};
pub use library::{Library, Listing, MacroEntry, SkippedFile, MAX_FILE_BYTES, MAX_ID_CHARS};
pub use live_capture::{LiveCapturePool, NativeRecordKey, MAX_RETAINED_CAPTURES};
pub use live_frame::{LiveFrame, LiveInput, LiveSession, LiveTrack};
pub use recording::{
    InputFrame, MacroError, MacroRecording, MomentumMode, Placement, StartState, DEFAULT_FPS,
    MAX_FRAMES, MAX_GAME_VERSION_BYTES, MAX_NAME_CHARS, SCHEMA_VERSION,
};
pub use window::{timestamp, FrameSnapshot, FrameWindow, Step};
pub use replay::{ReplayFrame, ReplaySession, ReplayTrack};
pub use shared_capture::SharedCaptureArchive;

#[cfg(test)]
mod test_util;
