//! Value-only replay observations. The runtime supplies real simulation samples;
//! this module neither decodes proprietary replay bytes nor invents action state.
use crate::{FrameWindow, InputFrame, MacroRecording, MoveStart, StartState};

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

impl ReplaySession {
    /// Associate only values from the same immutable committed capture. Native
    /// CFMK row numbers and local live entry IDs are different namespaces.
    pub fn mii_for_track(&self, miis: &training_mod_mii::replay::ReplayMiis,
        entry: i32, kind: u32) -> Result<training_mod_mii::replay::RawMii, &'static str>
    {
        let raw = miis.unique_class(kind)?;
        let mut matching = self.tracks.iter().filter(|track|
            track.frames.iter().any(|frame| frame.start.fighter_kind as u32 == kind));
        let track = matching.next().ok_or("Capture has no track for this Mii class")?;
        if matching.next().is_some() { return Err("Capture has ambiguous same-class Mii tracks"); }
        if track.entry != entry || track.frames.iter().any(|frame| frame.start.fighter_kind as u32 != kind) {
            return Err("Capture Mii track identity changes or selection differs");
        }
        Ok(*raw)
    }
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
            return Err("In is mid-action. Snap to a move start before previewing or saving.".into());
        }
        self.export_inputs(window, name, game_version, first.start)
    }

    /// Explicitly restart a snapped input sequence from neutral. No source frame
    /// is rewritten and no animation/work-state restore is claimed. Ordinary
    /// macro JSON already persists this context and the exact selected inputs.
    pub fn export_move(&self, window: &FrameWindow, name: &str, game_version: &str,
        start: &MoveStart) -> Result<MacroRecording, String> {
        if start.frame != window.start() || start.action_frame < start.frame
            || start.action_frame >= window.end()
            || self.frames.get(start.action_frame as usize).is_none_or(|frame|
                frame.start.status != start.action_status || frame.start.fighter_kind != start.context.fighter_kind) {
            return Err("Move alignment changed; snap the start again".into());
        }
        self.export_inputs(window, name, game_version, start.context)
    }

    fn export_inputs(&self, window: &FrameWindow, name: &str, game_version: &str,
        start: StartState) -> Result<MacroRecording, String> {
        let range = window.range();
        let selected = self.frames.get(range.start as usize..range.end as usize)
            .ok_or("Window exceeds captured replay")?;
        if selected.iter().any(|f| f.start.fighter_kind != start.fighter_kind) {
            return Err("Window crosses a fighter transformation; select one fighter form.".into());
        }
        let inputs: Vec<InputFrame> = selected.iter().map(|f| f.input).collect();
        MacroRecording::new(name, game_version, start, inputs)
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

    #[test]
    fn replay_mii_unique_class_binds_different_indices_and_refuses_ambiguous_history() {
        use training_mod_mii::replay::{ReplayMiis, HEADER_LEN, VERSION};
        let mut bytes = vec![0; HEADER_LEN];
        bytes[4..8].copy_from_slice(b"CFMK");
        bytes[8..12].copy_from_slice(&VERSION.to_le_bytes());
        bytes[0x10..0x12].copy_from_slice(&(HEADER_LEN as u16).to_le_bytes());
        for index in 0..8 { bytes[0x468 + index * 0x260 + 0xa] = 2; }
        bytes[0x468 + 8] = 0x48;
        bytes[0x468 + 0xa] = 0;
        let miis = ReplayMiis::parse_inner(&bytes).unwrap();
        let mut brawler = track();
        for frame in &mut brawler.frames { frame.start.fighter_kind = 0x48; }
        let mut mario = track(); mario.entry = 0;
        for frame in &mut mario.frames { frame.start.fighter_kind = 0; }
        let mut session = ReplaySession {tracks: vec![mario, brawler], truncated: false};
        let associated = session.mii_for_track(&miis, 1, 0x48).unwrap();
        assert_eq!(associated.entry(), 0); // Native index never replaces the selected live entry.
        assert!(session.mii_for_track(&miis, 0, 0x48).is_err());
        bytes[0x468 + 0x260 + 8] = 0x48;
        bytes[0x468 + 0x260 + 0xa] = 0;
        assert!(session.mii_for_track(&ReplayMiis::parse_inner(&bytes).unwrap(), 1, 0x48).is_err());
        let mut duplicate = session.tracks[1].clone(); duplicate.entry = 2;
        session.tracks.push(duplicate);
        assert!(session.mii_for_track(&miis, 1, 0x48).is_err());
        session.tracks.pop();
        session.tracks[1].frames[0].start.fighter_kind = 0x49;
        assert!(session.mii_for_track(&miis, 1, 0x48).is_err());
    }
}
