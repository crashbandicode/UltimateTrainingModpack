//! Clip selection. Native input consumption, not rendering, advances the preview.
use training_mod_macros::{FrameWindow, MacroRecording, ReplayFrame, ReplaySession, ReplayTrack, MAX_NAME_CHARS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle { Playhead, In, Out }
#[derive(Clone, Copy, Debug)]
pub enum ClipAction {
    SelectIn, SelectOut, Nudge(i32), TogglePlay, Restart,
    PreviousFighter, NextFighter, ToggleCapture, UseLeadIn, Confirm, Cancel,
}
pub enum ClipEffect {
    None, StopPreview, Preview(Box<MacroRecording>), Close,
    Export(Box<MacroRecording>), Error(String),
}

/// Own the closing press independently of the draw clock. The native adapter polls
/// this once per P1 input mapping, preserving the physical closing-button level
/// between repeated mappings until the next native pad refresh. Neutralizing the
/// mapped output must not clear this release signal as a side effect.
#[derive(Default)]
pub struct CloseInputGuard { polls: u32, waiting_for_release: bool }

impl CloseInputGuard {
    pub const fn new() -> Self { Self { polls: 0, waiting_for_release: false } }

    pub fn arm(&mut self, grace_polls: u32) {
        self.polls = grace_polls;
        self.waiting_for_release = true;
    }

    pub fn poll(&mut self, in_training: bool, close_held: bool) -> bool {
        if !in_training { *self = Self::new(); return false; }
        self.waiting_for_release &= close_held;
        let owned = self.polls > 0 || self.waiting_for_release;
        self.polls = self.polls.saturating_sub(1);
        owned
    }
}

#[derive(Clone, Debug)]
pub struct ClipView {
    pub total: u32,
    pub start: u32,
    pub end: u32,
    pub cursor: u32,
    pub handle: Handle,
    pub playing: bool,
    pub entry: i32,
    pub fighter_kind: i32,
    pub previous_selected: bool,
    pub previous_available: bool,
    pub clean_start: bool,
    pub quantized: bool,
    pub truncated: bool,
    pub source_name: String,
}

pub struct ClipEditor {
    session: ReplaySession,
    previous: Option<ReplaySession>,
    previous_selected: bool,
    track: usize,
    window: FrameWindow,
    handle: Handle,
    playing: bool,
    clean_statuses: Vec<i32>,
    source_name: String,
    previous_name: String,
}

fn prepare(mut session: ReplaySession) -> Option<ReplaySession> {
    session.tracks.retain(|track| !track.frames.is_empty());
    session.tracks.sort_by_key(|track| track.entry);
    (!session.tracks.is_empty()).then_some(session)
}

impl ClipEditor {
    pub fn new(session: ReplaySession, clean_statuses: Vec<i32>) -> Result<Self, String> {
        let session = prepare(session).ok_or("Play a saved replay in Vault first")?;
        let window = session.tracks[0].window()?;
        Ok(Self { session, previous: None, previous_selected: false, track: 0, window,
            handle: Handle::Playhead, playing: false, clean_statuses,
            source_name: "Latest Vault capture".into(), previous_name: "Previous Vault capture".into() })
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.source_name = name;
        self
    }

    pub fn with_previous(mut self, previous: Option<ReplaySession>) -> Self {
        self.previous = previous.and_then(prepare);
        self
    }

    pub fn select_kind(&mut self, kind: i32) {
        if let Some(index) = self.session.tracks.iter().position(|t| t.frames[0].start.fighter_kind == kind) {
            self.set_track(index);
        }
    }

    pub fn track(&self) -> &ReplayTrack { &self.session.tracks[self.track] }
    pub fn sample(&self) -> ReplayFrame { self.track().frames[self.window.cursor() as usize] }
    pub fn pause(&mut self) { self.playing = false; }

    pub fn view(&self) -> ClipView {
        let track = self.track();
        ClipView { total: self.window.total(), start: self.window.start(), end: self.window.end(),
            cursor: self.window.cursor(), handle: self.handle, playing: self.playing,
            entry: track.entry, fighter_kind: self.sample().start.fighter_kind,
            previous_selected: self.previous_selected, previous_available: self.previous.is_some(),
            clean_start: self.clean_statuses.contains(&track.frames[self.window.start() as usize].start.status),
            quantized: track.quantized_analog, truncated: self.session.truncated,
            source_name: self.source_name.clone() }
    }

    /// Report actual input consumption, relative to the selected range. Drawing
    /// the timeline never advances inputs or fabricates a simulated pose.
    pub fn preview_progress(&mut self, consumed: u32) {
        if !self.playing { return; }
        let start = self.window.start();
        let end = self.window.end();
        self.window.seek(start.saturating_add(consumed.saturating_sub(1)).min(end - 1));
        if consumed >= end - start { self.playing = false; }
    }

    fn start_preview(&mut self) -> ClipEffect {
        self.pause();
        self.window.seek(self.window.start());
        self.handle = Handle::Playhead;
        match self.track().export(&self.window, "Preview", "13.0.5", &self.clean_statuses) {
            Ok(recording) => {
                self.playing = true;
                ClipEffect::Preview(Box::new(recording))
            }
            Err(error) => ClipEffect::Error(error),
        }
    }

    fn set_track(&mut self, index: usize) {
        let mut window = self.session.tracks[index].window().expect("nonempty track");
        // A full-capture selection means the whole newly selected fighter too.
        // Preserve explicit crops, not the previous fighter's shorter default Out.
        if self.window.start() != 0 || self.window.end() != self.window.total() {
            window.set_out(self.window.end());
            window.set_in(self.window.start());
        }
        window.seek(self.window.cursor().min(window.total() - 1));
        self.track = index;
        self.window = window;
        self.playing = false;
    }

    pub fn on_action(&mut self, action: ClipAction) -> ClipEffect {
        match action {
            ClipAction::SelectIn | ClipAction::SelectOut => {
                self.pause();
                let handle = if matches!(action, ClipAction::SelectIn) { Handle::In } else { Handle::Out };
                self.handle = if self.handle == handle { Handle::Playhead } else { handle };
                match self.handle {
                    Handle::In => { self.window.seek(self.window.start()); }
                    Handle::Out => { self.window.seek(self.window.end() - 1); }
                    Handle::Playhead => {}
                }
            }
            ClipAction::Nudge(delta) => {
                self.pause();
                let add = |value: u32| (i64::from(value) + i64::from(delta)).clamp(0, i64::from(u32::MAX)) as u32;
                match self.handle {
                    Handle::In => {
                        self.window.set_in(add(self.window.start()));
                        self.window.seek(self.window.start());
                    }
                    Handle::Out => {
                        self.window.set_out(add(self.window.end()));
                        self.window.seek(self.window.end() - 1);
                    }
                    Handle::Playhead => {
                        return ClipEffect::Error("Use L/R to edit In/Out; X plays the selected range".into());
                    }
                }
            }
            ClipAction::TogglePlay => {
                if !self.playing { return self.start_preview(); }
                self.pause();
                self.window.seek(self.window.start());
            }
            ClipAction::Restart => { return self.start_preview(); }
            ClipAction::PreviousFighter | ClipAction::NextFighter => {
                let count = self.session.tracks.len();
                let offset = if matches!(action, ClipAction::PreviousFighter) { count - 1 } else { 1 };
                self.set_track((self.track + offset) % count);
            }
            ClipAction::ToggleCapture => {
                if let Some(previous) = &mut self.previous {
                    std::mem::swap(&mut self.session, previous);
                    std::mem::swap(&mut self.source_name, &mut self.previous_name);
                    self.previous_selected = !self.previous_selected;
                    self.track = 0;
                    self.window = self.session.tracks[0].window().expect("nonempty capture");
                    self.handle = Handle::Playhead;
                    self.pause();
                }
            }
            ClipAction::UseLeadIn => {
                self.pause();
                if let Some(start) = self.track().previous_clean_start(self.window.start(), &self.clean_statuses) {
                    self.window.set_in(start);
                    self.window.seek(start);
                    self.handle = Handle::In;
                } else { return ClipEffect::Error("No earlier idle/fall start in this capture".into()); }
            }
            ClipAction::Confirm => {
                self.pause();
                let suffix = format!(" P{}", self.track().entry + 1);
                let prefix: String = self.source_name.chars()
                    .take(MAX_NAME_CHARS.saturating_sub(suffix.chars().count())).collect();
                return match self.track().export(&self.window, &format!("{prefix}{suffix}"),
                    "13.0.5", &self.clean_statuses) {
                    Ok(recording) => ClipEffect::Export(Box::new(recording)),
                    Err(error) => ClipEffect::Error(error),
                };
            }
            ClipAction::Cancel => { self.pause(); return ClipEffect::Close; }
        }
        ClipEffect::StopPreview
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use training_mod_macros::{InputFrame, StartState};
    #[test]
    fn close_owns_first_poll_without_a_draw_and_retains_short_press_grace() {
        let mut guard = CloseInputGuard::new();
        assert!(!guard.poll(true, true));
        guard.arm(15);
        assert!(guard.poll(true, true));
        for _ in 1..15 { assert!(guard.poll(true, false)); }
        assert!(!guard.poll(true, false));
        assert!(!guard.poll(true, true)); // A new press belongs to gameplay.
    }
    #[test]
    fn closing_press_held_past_grace_never_becomes_a_gameplay_press() {
        let mut guard = CloseInputGuard::new();
        guard.arm(15);
        for _ in 0..90 { assert!(guard.poll(true, true)); }
        assert!(!guard.poll(true, false));
        assert!(!guard.poll(true, true));
    }
    #[test]
    fn scene_departure_discards_close_ownership() {
        let mut guard = CloseInputGuard::new();
        guard.arm(15);
        assert!(!guard.poll(false, true));
        assert!(!guard.poll(true, true));
        guard.arm(15);
        assert!(guard.poll(true, true));
    }
    fn editor() -> ClipEditor {
        let frames = (0..10).map(|i| ReplayFrame { input: InputFrame { buttons: i, ..InputFrame::default() },
            start: StartState { status: if i == 4 { 99 } else { 14 }, ..StartState::neutral(18, 0, 1.0) },
            x: i as f32, y: 0.0 }).collect();
        ClipEditor::new(ReplaySession { tracks: vec![ReplayTrack { entry: 1, frames,
            quantized_analog: false }], truncated: false }, vec![14]).unwrap()
    }
    #[test]
    fn independent_handles_preview_last_included_frame_and_never_cross() {
        let mut e = editor();
        e.on_action(ClipAction::SelectIn); e.on_action(ClipAction::Nudge(3));
        assert_eq!((e.view().start, e.view().end, e.view().cursor), (3, 10, 3));
        e.on_action(ClipAction::SelectOut); e.on_action(ClipAction::Nudge(-2));
        assert_eq!((e.view().start, e.view().end, e.view().cursor), (3, 8, 7));
        e.on_action(ClipAction::Nudge(i32::MIN));
        assert_eq!((e.view().start, e.view().end, e.view().cursor), (3, 4, 3));
        e.on_action(ClipAction::SelectIn); e.on_action(ClipAction::Nudge(i32::MAX));
        assert_eq!(e.view().start, 3);
    }
    #[test]
    fn native_preview_exports_exact_selection_and_only_consumption_advances_it() {
        let mut e = editor();
        e.on_action(ClipAction::SelectOut); e.on_action(ClipAction::Nudge(-7));
        let ClipEffect::Preview(recording) = e.on_action(ClipAction::TogglePlay) else { panic!() };
        assert_eq!(recording.frames.iter().map(|f| f.buttons).collect::<Vec<_>>(), [0, 1, 2]);
        for _ in 0..20 { assert_eq!(e.view().cursor, 0); }
        e.preview_progress(1);
        assert_eq!(e.view().cursor, 0);
        e.preview_progress(2);
        assert_eq!(e.view().cursor, 1);
        e.preview_progress(3);
        assert_eq!((e.view().cursor, e.view().end), (2, 3));
        assert!(!e.view().playing);
        assert!(matches!(e.on_action(ClipAction::Restart), ClipEffect::Preview(_)));
        assert_eq!(e.view().cursor, 0);
        assert!(matches!(e.on_action(ClipAction::TogglePlay), ClipEffect::StopPreview));
        e.preview_progress(2);
        assert_eq!(e.view().cursor, 0); // Late progress cannot move a stopped editor.
    }
    #[test]
    fn playhead_cannot_jump_to_an_unsimulated_pose() {
        let mut e = editor();
        assert!(matches!(e.on_action(ClipAction::Nudge(7)), ClipEffect::Error(_)));
        assert!(matches!(e.on_action(ClipAction::Nudge(-7)), ClipEffect::Error(_)));
        assert_eq!((e.view().cursor, e.view().start, e.view().end), (0, 0, 10));
    }
    #[test]
    fn invalid_start_requires_explicit_lead_in_and_keeps_selection() {
        let mut e = editor();
        e.on_action(ClipAction::SelectIn); e.on_action(ClipAction::Nudge(4));
        assert!(matches!(e.on_action(ClipAction::Confirm), ClipEffect::Error(_)));
        assert!(matches!(e.on_action(ClipAction::TogglePlay), ClipEffect::Error(_)));
        assert!(!e.view().playing);
        assert_eq!(e.view().start, 4);
        e.on_action(ClipAction::UseLeadIn);
        assert_eq!(e.view().start, 3);
        let ClipEffect::Export(recording) = e.on_action(ClipAction::Confirm) else { panic!() };
        assert_eq!(recording.frames.iter().map(|f| f.buttons).collect::<Vec<_>>(), (3..10).collect::<Vec<_>>());
    }
    #[test]
    fn capture_switch_and_cancel_pause_without_losing_current_window() {
        let previous = editor().session;
        let mut e = editor().with_previous(Some(previous));
        e.on_action(ClipAction::ToggleCapture);
        assert!(e.view().previous_selected);
        assert_eq!(e.view().source_name, "Previous Vault capture");
        e.on_action(ClipAction::SelectIn); e.on_action(ClipAction::Nudge(2));
        assert!(matches!(e.on_action(ClipAction::Cancel), ClipEffect::Close));
        assert_eq!(e.view().start, 2);
        assert!(!e.view().playing);
    }
    #[test]
    fn saved_capture_name_survives_crop_without_exceeding_macro_name_limit() {
        let mut e = editor().with_name("試合".repeat(40));
        e.on_action(ClipAction::SelectIn);
        e.on_action(ClipAction::Nudge(3));
        let ClipEffect::Export(recording) = e.on_action(ClipAction::Confirm) else { panic!() };
        assert_eq!(recording.name.chars().count(), MAX_NAME_CHARS);
        assert!(recording.name.ends_with(" P2"));
        assert!(recording.name.starts_with("試合"));
        assert_eq!(recording.frames.iter().map(|f| f.buttons).collect::<Vec<_>>(), (3..10).collect::<Vec<_>>());
    }

    fn unequal_tracks() -> ClipEditor {
        let mut session = editor().session;
        let mut short = session.tracks[0].clone();
        short.entry = 0;
        short.frames.truncate(3);
        for sample in &mut short.frames { sample.start.fighter_kind = 8; }
        session.tracks.push(short);
        ClipEditor::new(session, vec![14]).unwrap()
    }

    #[test]
    fn automatic_cpu_selection_and_full_range_fighter_switch_keep_all_inputs() {
        let mut e = unequal_tracks();
        assert_eq!(e.view().end, 3);
        e.select_kind(18);
        let ClipEffect::Export(recording) = e.on_action(ClipAction::Confirm) else { panic!() };
        assert_eq!(recording.frames.iter().map(|frame| frame.buttons).collect::<Vec<_>>(),
                   (0..10).collect::<Vec<_>>());
        e.on_action(ClipAction::NextFighter);
        assert_eq!((e.view().start, e.view().end), (0, 3));
        e.on_action(ClipAction::NextFighter);
        assert_eq!((e.view().start, e.view().end), (0, 10));
    }

    #[test]
    fn explicit_crop_survives_selection_of_a_longer_fighter_track() {
        let mut e = unequal_tracks();
        e.on_action(ClipAction::SelectIn);
        e.on_action(ClipAction::Nudge(1));
        e.on_action(ClipAction::NextFighter);
        let ClipEffect::Export(recording) = e.on_action(ClipAction::Confirm) else { panic!() };
        assert_eq!(recording.frames.iter().map(|frame| frame.buttons).collect::<Vec<_>>(), [1, 2]);
    }
}
