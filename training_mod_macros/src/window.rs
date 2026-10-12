//! Replay-window selection: a cursor stepped in frames or game seconds, a
//! half-open `[in, out)` selection, and cropping that selection into a macro.
//!
//! Time is game time: frame `f` is `f / fps` seconds into the source, never
//! host wall time.

use std::ops::Range;

use crate::recording::{InputFrame, MacroError, MacroRecording, StartState, DEFAULT_FPS};

/// Cursor increment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Frame,
    /// Whole game seconds. `Seconds(0)` moves nothing.
    Seconds(u32),
}

impl Step {
    pub const SECOND: Step = Step::Seconds(1);
    pub const FIVE_SECONDS: Step = Step::Seconds(5);

    /// Frames this step covers at `fps` (saturating).
    pub fn frames(self, fps: u32) -> u32 {
        match self {
            Step::Frame => 1,
            Step::Seconds(n) => n.saturating_mul(fps),
        }
    }
}

/// Fighter state captured by the runtime at one specific source frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameSnapshot {
    pub frame: u32,
    pub state: StartState,
}

/// Cursor and half-open selection over a source of `total` frames.
///
/// The cursor and both marks are boundary positions in `0..=total`: position
/// `p` lies before frame `p`, so `total` is the end of the source. The
/// selection always holds `start < end <= total` (never empty) and out-of-range
/// requests are clamped instead of rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameWindow {
    total: u32,
    fps: u32,
    cursor: u32,
    start: u32,
    end: u32,
}

impl FrameWindow {
    /// Window over `total_frames` at the default 60 fps, selecting everything.
    pub fn new(total_frames: u32) -> Result<Self, MacroError> {
        Self::with_fps(total_frames, DEFAULT_FPS)
    }

    pub fn with_fps(total_frames: u32, fps: u32) -> Result<Self, MacroError> {
        if total_frames == 0 {
            return Err(MacroError::InvalidWindow("source has no frames"));
        }
        if fps == 0 {
            return Err(MacroError::InvalidWindow("fps must be positive"));
        }
        Ok(Self {
            total: total_frames,
            fps,
            cursor: 0,
            start: 0,
            end: total_frames,
        })
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn fps(&self) -> u32 {
        self.fps
    }

    pub fn cursor(&self) -> u32 {
        self.cursor
    }

    /// First selected frame (inclusive).
    pub fn start(&self) -> u32 {
        self.start
    }

    /// End of the selection (exclusive).
    pub fn end(&self) -> u32 {
        self.end
    }

    pub fn range(&self) -> Range<u32> {
        self.start..self.end
    }

    pub fn selected_frames(&self) -> u32 {
        self.end - self.start
    }

    /// Moves the cursor to `frame`, clamped to `0..=total`.
    pub fn seek(&mut self, frame: u32) -> u32 {
        self.cursor = frame.min(self.total);
        self.cursor
    }

    pub fn forward(&mut self, step: Step) -> u32 {
        self.seek(self.cursor.saturating_add(step.frames(self.fps)))
    }

    pub fn back(&mut self, step: Step) -> u32 {
        self.cursor = self.cursor.saturating_sub(step.frames(self.fps));
        self.cursor
    }

    /// Sets the selection start, clamped below `end`. Returns the applied value.
    pub fn set_in(&mut self, frame: u32) -> u32 {
        self.start = frame.min(self.end - 1);
        self.start
    }

    /// Sets the exclusive selection end, clamped to `start + 1..=total`.
    /// Returns the applied value.
    pub fn set_out(&mut self, frame: u32) -> u32 {
        self.end = frame.clamp(self.start + 1, self.total);
        self.end
    }

    /// Crops the selection of `source` into a new macro with default placement.
    ///
    /// Momentum and action state cannot be inferred from inputs, so the caller
    /// must pass the actual fighter state sampled at the selected start frame,
    /// even at frame 0 (use `StartState::neutral` there). A snapshot for any
    /// other frame, such as one left over from an earlier in-point, is refused.
    pub fn crop(
        &self,
        source: &[InputFrame],
        snapshot: &FrameSnapshot,
        name: &str,
        game_version: &str,
    ) -> Result<MacroRecording, MacroError> {
        if source.len() != self.total as usize {
            return Err(MacroError::InvalidWindow(
                "source length differs from the window's total",
            ));
        }
        if snapshot.frame != self.start {
            return Err(MacroError::SnapshotFrame {
                selected: self.start,
                snapshot: snapshot.frame,
            });
        }
        let frames = source[self.start as usize..self.end as usize].to_vec();
        MacroRecording::new(name, game_version, snapshot.state, frames)
    }
}

/// Frame readout as `M:SS +Ff`, e.g. frame 312 at 60 fps is `0:05 +12f`.
pub fn timestamp(frame: u32, fps: u32) -> String {
    let fps = fps.max(1);
    let seconds = frame / fps;
    format!("{}:{:02} +{}f", seconds / 60, seconds % 60, frame % fps)
}

#[cfg(test)]
mod tests;
