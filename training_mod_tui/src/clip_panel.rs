//! Clip selection. Native input consumption, not rendering, advances the preview.
use training_mod_macros::{FrameWindow, MacroRecording, ReplayFrame, ReplaySession, ReplayTrack, MAX_NAME_CHARS};
use crate::macro_panel::Pad;

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

/// Frames of the same direction before held Left/Right starts repeating.
const SCRUB_REPEAT_DELAY: u32 = 10;
/// Hold time added when the fast modifier is already down, so the scroll
/// starts fast and still picks up from there.
const SCRUB_FAST_BIAS: u32 = 70;

/// Timeline frames moved by one tick after holding a direction this long.
/// A tap stays at one frame; a long hold reaches three minutes of replay per second.
pub fn scrub_step(held_frames: u32) -> i32 {
    match held_frames {
        0..=24 => 1,
        25..=44 => 3,
        45..=69 => 8,
        70..=99 => 20,
        100..=134 => 45,
        135..=174 => 90,
        _ => 180,
    }
}

/// Repeat step for a held direction. Zero during the tap window.
pub fn scrub_repeat_step(held_frames: u32, fast: bool) -> i32 {
    if held_frames < SCRUB_REPEAT_DELAY {
        return 0;
    }
    let paced = if fast {
        held_frames.saturating_add(SCRUB_FAST_BIAS)
    } else {
        held_frames
    };
    scrub_step(paced)
}

/// Horizontal editing wins over a diagonal D-pad. Fighter changes are D-pad
/// Up/Down only, and only while Left/Right is not held.
pub fn direction_action(
    left_edge: bool,
    right_edge: bool,
    up_edge: bool,
    down_edge: bool,
    horizontal_held: bool,
    fast: bool,
) -> Option<ClipAction> {
    let horizontal = i32::from(right_edge) - i32::from(left_edge);
    if horizontal != 0 {
        let step = if fast { scrub_step(SCRUB_FAST_BIAS) } else { 1 };
        return Some(ClipAction::Nudge(horizontal * step));
    }
    if horizontal_held || (up_edge && down_edge) {
        return None;
    }
    if up_edge {
        return Some(ClipAction::PreviousFighter);
    }
    if down_edge {
        return Some(ClipAction::NextFighter);
    }
    None
}

pub struct ClipHelp {
    pub primary: String,
    pub follow: String,
    pub controls: String,
    pub notice: String,
}

/// Button text for the step the crop editor is actually on.
pub fn clip_help(view: &ClipView, coarse: &str, chord: &str, pace: i32) -> ClipHelp {
    let controls = "D-pad Up/Down: fighter. X preview. A save. B close.".to_owned();
    if view.playing {
        return ClipHelp {
            primary: "Previewing the selection.".to_owned(),
            follow: "Press X to stop. Press L or R to edit the crop.".to_owned(),
            controls,
            notice: String::new(),
        };
    }
    let (primary, follow) = match view.handle {
        Handle::Out => (
            "End selected. Hold Left or Right to move it.".to_owned(),
            format!("Hold longer to speed up, or hold {coarse} to start fast."),
        ),
        Handle::In => (
            "Start selected. Hold Left or Right to move it.".to_owned(),
            format!("Hold longer to speed up, or hold {coarse} to start fast."),
        ),
        Handle::Playhead => (
            "Press L for the start, or R for the end.".to_owned(),
            format!("Then hold Left or Right. Hold {coarse} to start fast."),
        ),
    };
    let follow = if pace > 1 {
        if pace >= 180 {
            format!("Moving {pace} frames per tick.")
        } else {
            format!("Moving {pace} frames per tick and speeding up.")
        }
    } else {
        follow
    };
    let notice = if view.clean_start {
        String::new()
    } else {
        format!("Mid-action start: press {chord}+Y for lead-in, then X to preview.")
    };
    if let Some(error) = &view.error {
        let [primary, follow] = feedback_lines(error);
        return ClipHelp { primary, follow, controls, notice };
    }
    ClipHelp { primary, follow, controls, notice }
}

/// Keep failures in the wide editor strip. The ordinary notification pane
/// shrinks long text to fit one narrow line, making preview refusals unreadable.
fn feedback_lines(message: &str) -> [String; 2] {
    const WIDTH: usize = 72;
    let mut lines = [String::new(), String::new()];
    let mut row = 0;
    for word in message.split_whitespace() {
        if !lines[row].is_empty() && lines[row].chars().count() + 1 + word.chars().count() > WIDTH {
            if row == 1 { lines[1].push_str("..."); break; }
            row = 1;
        }
        if !lines[row].is_empty() { lines[row].push(' '); }
        let remaining = WIDTH.saturating_sub(lines[row].chars().count());
        lines[row].extend(word.chars().take(remaining));
        if word.chars().count() > remaining { lines[row].push_str("..."); }
    }
    lines
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

/// The press that opens the editor belongs to the opener. Native mapping can run
/// several times before the next pad refresh, re-presenting that same edge and
/// held level, so a fresh Confirm needs a release this guard actually observed.
/// Only an unheld sample without a press clears it. That sample never confirms:
/// the next press must arrive separately, with a held level and no release edge.
#[derive(Default)]
pub struct OpenInputGuard { waiting_for_release: bool }

impl OpenInputGuard {
    pub const fn new() -> Self { Self { waiting_for_release: false } }

    pub fn arm(&mut self) { self.waiting_for_release = true; }

    pub fn pending(&self) -> bool { self.waiting_for_release }

    /// Poll once per P1 mapping. Returns whether a Confirm edge may pass.
    pub fn allows_confirm(&mut self, held: bool, released: bool, pressed: bool) -> bool {
        if self.waiting_for_release {
            if !held && !pressed { self.waiting_for_release = false; }
            return false;
        }
        held && pressed && !released
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
    pub error: Option<String>,
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
    open_guard: OpenInputGuard,
    error: Option<String>,
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
        let mut open_guard = OpenInputGuard::new();
        open_guard.arm();
        Ok(Self { session, previous: None, previous_selected: false, track: 0, window,
            handle: Handle::Playhead, playing: false, clean_statuses,
            source_name: "Latest Vault capture".into(), previous_name: "Previous Vault capture".into(),
            open_guard, error: None })
    }

    /// Physical A edge gate for the native adapter; see `OpenInputGuard`.
    pub fn allows_confirm(&mut self, held: bool, released: bool, pressed: bool) -> bool {
        self.open_guard.allows_confirm(held, released, pressed)
    }

    /// True while the opening press has not been seen released.
    pub fn confirm_release_pending(&self) -> bool { self.open_guard.pending() }

    /// Normalized UI events carry no physical release sample. They must not clear
    /// the opening fence or bypass it when the editor is not in the foreground.
    pub fn action_for_pad(&self, pad: Pad) -> Option<ClipAction> {
        Some(match pad {
            Pad::Confirm if self.confirm_release_pending() => return None,
            Pad::Confirm => ClipAction::Confirm, Pad::Back => ClipAction::Cancel,
            Pad::Left => ClipAction::Nudge(-1), Pad::Right => ClipAction::Nudge(1),
            Pad::Up => ClipAction::PreviousFighter, Pad::Down => ClipAction::NextFighter,
            Pad::Extra => ClipAction::TogglePlay,
        })
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
    pub fn report_error(&mut self, message: String) { self.error = Some(message); }

    pub fn view(&self) -> ClipView {
        let track = self.track();
        ClipView { total: self.window.total(), start: self.window.start(), end: self.window.end(),
            cursor: self.window.cursor(), handle: self.handle, playing: self.playing,
            entry: track.entry, fighter_kind: self.sample().start.fighter_kind,
            previous_selected: self.previous_selected, previous_available: self.previous.is_some(),
            clean_start: self.clean_statuses.contains(&track.frames[self.window.start() as usize].start.status),
            quantized: track.quantized_analog, truncated: self.session.truncated,
            source_name: self.source_name.clone(), error: self.error.clone() }
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
        self.error = None;
        let effect = self.apply_action(action);
        if let ClipEffect::Error(message) = &effect { self.report_error(message.clone()); }
        effect
    }

    fn apply_action(&mut self, action: ClipAction) -> ClipEffect {
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
                        return ClipEffect::Error("Press L for the start or R for the end, then hold Left or Right".into());
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
    /// One native P1 mapping sample of physical A: (held level, release edge, press edge).
    /// Returns what the editor would do with the Confirm slot of that sample.
    fn poll_a(e: &mut ClipEditor, held: bool, released: bool, pressed: bool) -> Option<ClipEffect> {
        e.allows_confirm(held, released, pressed).then(|| e.on_action(ClipAction::Confirm))
    }
    fn exported(effect: Option<ClipEffect>) -> Option<Vec<u32>> {
        match effect {
            Some(ClipEffect::Export(recording)) => Some(recording.frames.iter().map(|f| f.buttons).collect()),
            _ => None,
        }
    }
    #[test]
    fn opening_press_repeated_polls_and_held_level_never_export_until_release_is_observed() {
        let mut e = editor();
        assert!(e.confirm_release_pending());
        // FIM re-presents the opening edge and held level before the next pad refresh.
        for _ in 0..8 { assert!(poll_a(&mut e, true, false, true).is_none()); }
        for _ in 0..30 { assert!(poll_a(&mut e, true, false, false).is_none()); }
        assert!(e.confirm_release_pending());
        // A same-poll edge without an observed release is still the opener's press.
        assert!(poll_a(&mut e, true, false, true).is_none());
        assert!(exported(poll_a(&mut e, false, false, false)).is_none());
        assert!(!e.confirm_release_pending());
        assert_eq!(exported(poll_a(&mut e, true, false, true)).unwrap(), (0..10).collect::<Vec<_>>());
    }
    #[test]
    fn a_release_edge_cannot_override_the_still_held_opening_press() {
        let mut e = editor();
        assert!(poll_a(&mut e, true, true, false).is_none());
        assert!(e.confirm_release_pending());
        assert!(poll_a(&mut e, true, true, true).is_none());
        assert!(poll_a(&mut e, true, false, true).is_none());
        assert!(poll_a(&mut e, false, true, false).is_none());
        assert!(exported(poll_a(&mut e, true, false, true)).is_some());
    }
    #[test]
    fn release_and_stale_press_samples_cannot_export_without_a_separate_fresh_press() {
        let mut e = editor();
        assert!(poll_a(&mut e, false, true, true).is_none());
        assert!(poll_a(&mut e, false, false, true).is_none());
        assert!(e.confirm_release_pending());
        assert!(poll_a(&mut e, false, true, false).is_none());
        assert!(!e.confirm_release_pending());
        assert!(poll_a(&mut e, false, false, true).is_none());
        assert!(poll_a(&mut e, true, true, true).is_none());
        assert_eq!(exported(poll_a(&mut e, true, false, true)).unwrap(), (0..10).collect::<Vec<_>>());
    }
    #[test]
    fn moving_the_editor_through_reload_retains_opening_press_ownership() {
        let mut visible = Some(editor());
        assert!(poll_a(visible.as_mut().unwrap(), true, false, true).is_none());
        let mut parked = visible.take().unwrap();
        parked.pause();
        assert!(parked.confirm_release_pending());
        visible = Some(parked);
        let restored = visible.as_mut().unwrap();
        assert!(poll_a(restored, true, false, true).is_none());
        assert!(poll_a(restored, false, false, false).is_none());
        assert!(exported(poll_a(restored, true, false, true)).is_some());
    }
    #[test]
    fn normalized_pad_cannot_bypass_or_clear_opening_press_ownership() {
        let mut e = editor();
        for _ in 0..8 { assert!(e.action_for_pad(Pad::Confirm).is_none()); }
        assert!(e.confirm_release_pending());
        let preview = e.action_for_pad(Pad::Extra).unwrap();
        assert!(matches!(e.on_action(preview), ClipEffect::Preview(_)));
        let cancel = e.action_for_pad(Pad::Back).unwrap();
        assert!(matches!(e.on_action(cancel), ClipEffect::Close));
        assert!(e.confirm_release_pending());
        assert!(poll_a(&mut e, false, false, false).is_none());
        let confirm = e.action_for_pad(Pad::Confirm).unwrap();
        assert_eq!(exported(Some(e.on_action(confirm))).unwrap(), (0..10).collect::<Vec<_>>());
    }
    #[test]
    fn delayed_open_and_immediate_open_follow_the_same_observed_release_rule() {
        // Delayed: the opener was already released before the first editor poll.
        let mut delayed = editor();
        assert!(exported(poll_a(&mut delayed, false, false, false)).is_none());
        assert!(exported(poll_a(&mut delayed, true, false, true)).is_some());
        // Immediate: the first poll still carries the opening edge.
        let mut immediate = editor();
        assert!(poll_a(&mut immediate, true, false, true).is_none());
        assert!(poll_a(&mut immediate, false, false, false).is_none());
        assert!(exported(poll_a(&mut immediate, true, false, true)).is_some());
    }
    #[test]
    fn close_then_reopen_creates_a_new_owner_that_waits_for_its_own_release() {
        let mut first = editor();
        assert!(poll_a(&mut first, false, false, false).is_none());
        assert!(!first.confirm_release_pending());
        assert!(matches!(first.on_action(ClipAction::Cancel), ClipEffect::Close));
        drop(first); // The native close drops the editor; reopen builds a fresh one.
        let mut reopened = editor();
        assert!(reopened.confirm_release_pending());
        assert!(poll_a(&mut reopened, true, false, true).is_none());
        assert!(poll_a(&mut reopened, false, false, false).is_none());
        assert!(exported(poll_a(&mut reopened, true, false, true)).is_some());
    }
    #[test]
    fn pending_gate_leaves_cancel_preview_and_other_editing_unchanged() {
        let mut e = editor();
        assert!(!e.allows_confirm(true, false, true));
        // These editor actions remain available; native B/X dispatch is separate.
        assert!(matches!(e.on_action(ClipAction::TogglePlay), ClipEffect::Preview(_)));
        assert!(matches!(e.on_action(ClipAction::TogglePlay), ClipEffect::StopPreview));
        e.on_action(ClipAction::SelectIn); e.on_action(ClipAction::Nudge(2));
        assert_eq!(e.view().start, 2);
        assert!(matches!(e.on_action(ClipAction::Cancel), ClipEffect::Close));
        assert!(e.confirm_release_pending());
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
        let feedback = clip_help(&e.view(), "ZL/ZR", "ZL", 0);
        assert!(feedback.primary.contains("mid-action"));
        assert!(!feedback.follow.is_empty());
        assert!(feedback.primary.chars().count() <= 75 && feedback.follow.chars().count() <= 75);
        assert!(feedback.notice.contains("ZL+Y") && feedback.notice.contains("X to preview"));
        e.on_action(ClipAction::UseLeadIn);
        assert_eq!(e.view().start, 3);
        assert!(e.view().error.is_none());
        let ClipEffect::Preview(preview) = e.on_action(ClipAction::TogglePlay) else { panic!() };
        assert_eq!(preview.frames.iter().map(|f| f.buttons).collect::<Vec<_>>(), (3..10).collect::<Vec<_>>());
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
    fn holding_left_moves_the_end_instead_of_the_fighter() {
        let mut editor = unequal_tracks();
        editor.on_action(ClipAction::SelectOut);
        let before = editor.view().fighter_kind;
        let Some(ClipAction::Nudge(delta)) = direction_action(true, false, true, false, true, false) else {
            panic!("a diagonal left press must edit the crop");
        };
        assert_eq!(delta, -1);
        editor.on_action(ClipAction::Nudge(delta));
        assert_eq!(editor.view().fighter_kind, before);
        assert!(editor.view().end < editor.view().total);
        assert!(direction_action(false, false, true, false, true, false).is_none());
        assert!(matches!(
            direction_action(false, false, true, false, false, false),
            Some(ClipAction::PreviousFighter)
        ));
        assert!(matches!(
            direction_action(false, false, false, true, false, false),
            Some(ClipAction::NextFighter)
        ));
    }

    #[test]
    fn held_scrub_speeds_up_and_the_fast_modifier_starts_ahead() {
        assert_eq!(scrub_repeat_step(0, false), 0);
        assert_eq!(scrub_repeat_step(9, false), 0);
        let early = scrub_repeat_step(12, false);
        let middle = scrub_repeat_step(80, false);
        let late = scrub_repeat_step(200, false);
        assert!(early < middle && middle < late);
        assert_eq!(late, 180);
        assert!(scrub_repeat_step(12, true) > early);
        let Some(ClipAction::Nudge(fast)) = direction_action(false, true, false, false, true, true) else {
            panic!("right press");
        };
        assert!(fast > 1);
    }

    #[test]
    fn instructions_follow_the_selected_end() {
        let mut editor = editor();
        let start = clip_help(&editor.view(), "ZL/ZR", "ZL", 0);
        assert!(start.primary.contains("Press L") && start.primary.contains("R"));
        assert!(start.primary.chars().count() <= 60);
        editor.on_action(ClipAction::SelectOut);
        let end = clip_help(&editor.view(), "ZL/ZR", "ZL", 0);
        assert!(end.primary.contains("End selected"));
        assert!(end.follow.contains("ZL/ZR"));
        let moving = clip_help(&editor.view(), "ZL/ZR", "ZL", 45);
        assert!(moving.follow.contains("45"));
        assert!(moving.follow.chars().count() <= 60);
        let capped = clip_help(&editor.view(), "Z", "Z", 180);
        assert_eq!(capped.follow, "Moving 180 frames per tick.");
        editor.on_action(ClipAction::SelectIn);
        editor.on_action(ClipAction::Nudge(4));
        let dirty = clip_help(&editor.view(), "ZL/ZR", "ZL", 0);
        assert!(dirty.notice.contains("ZL+Y"));
        assert!(dirty.primary.contains("Start selected"));
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
