//! Value-only replay observations. The runtime supplies real simulation samples;
//! this module neither decodes proprietary replay bytes nor invents action state.
use crate::{FrameSnapshot, FrameWindow, InputFrame, MacroRecording, StartState};

/// Input and start-context observation. Native simulation provides the preview;
/// no animation/pose samples are retained in the per-frame capture buffer.
#[derive(Clone, Copy, Debug)]
pub struct ReplayFrame {
    pub input: InputFrame,
    pub start: StartState,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug)]
pub struct ReplayTrack {
    pub entry: i32,
    pub frames: Vec<ReplayFrame>,
    /// Some observed analog values needed rounding to controller resolution.
    pub quantized_analog: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ReplaySession {
    pub tracks: Vec<ReplayTrack>,
    pub truncated: bool,
}

impl ReplayTrack {
    pub fn window(&self) -> Result<FrameWindow, String> {
        FrameWindow::new(self.frames.len() as u32).map_err(|e| e.to_string())
    }

    /// The game adapter supplies its verified idle/fall status IDs. There is no
    /// portable assumption that a numeric status represents an actionable state.
    pub fn previous_clean_start(&self, frame: u32, clean_statuses: &[i32]) -> Option<u32> {
        let limit = (frame as usize).min(self.frames.len().saturating_sub(1));
        (0..=limit).rev()
            .find(|&i| self.frames.get(i).is_some_and(|f| clean_statuses.contains(&f.start.status)))
            .map(|i| i as u32)
    }

    pub fn export(&self, window: &FrameWindow, name: &str, game_version: &str,
                  clean_statuses: &[i32]) -> Result<MacroRecording, String> {
        let first = self.frames.get(window.start() as usize).ok_or("No frame at In")?;
        if !clean_statuses.contains(&first.start.status) {
            return Err("In is mid-action. Choose an idle/falling frame or move In earlier to include the initiating inputs.".into());
        }
        let range = window.range();
        let selected = self.frames.get(range.start as usize..range.end as usize)
            .ok_or("Window exceeds captured replay")?;
        if selected.iter().any(|f| f.start.fighter_kind != first.start.fighter_kind) {
            return Err("Window crosses a fighter transformation; select one fighter form.".into());
        }
        let inputs: Vec<InputFrame> = self.frames.iter().map(|f| f.input).collect();
        window.crop(&inputs, &FrameSnapshot { frame: window.start(), state: first.start }, name, game_version)
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> ReplayTrack {
        ReplayTrack { entry: 1, quantized_analog: false,
            frames: (0..12).map(|i| ReplayFrame {
                input: InputFrame { buttons: if i == 3 { 513 } else { 0 }, ..InputFrame::default() },
                start: StartState { status: if (4..9).contains(&i) { 44 } else { 14 },
                    speed_y: -(i as f32), ..StartState::neutral(18, 0, 1.0) }, x: 0.0, y: i as f32,
            }).collect() }
    }

    #[test]
    fn mid_attack_needs_explicit_lead_in_and_preserves_selected_momentum() {
        let track = track();
        let mut window = track.window().unwrap();
        window.set_in(6);
        window.set_out(10);
        assert!(track.export(&window, "attack", "13.0.5", &[0, 14]).is_err());
        assert_eq!(track.previous_clean_start(6, &[0, 14]), Some(3));
        window.set_in(3);
        let exported = track.export(&window, "attack", "13.0.5", &[0, 14]).unwrap();
        assert_eq!(exported.frames.len(), 7);
        assert_eq!(exported.frames[0].buttons, 513);
        assert_eq!(exported.start.speed_y, -3.0);
    }

    #[test]
    fn rejects_mixed_fighter_window_and_empty_track() {
        let mut track = track();
        track.frames[6].start.fighter_kind = 8;
        assert!(track.export(&track.window().unwrap(), "mixed", "13.0.5", &[14]).is_err());
        track.frames.clear();
        assert!(track.window().is_err());
        assert_eq!(track.previous_clean_start(100, &[14]), None);
    }
}
