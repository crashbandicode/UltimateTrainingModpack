//! Host-testable named-macro panels and restore/playback sequencing.
//!
//! The Switch runtime draws [`View`] on the existing 8×4 training menu and
//! performs disk, character-load, and controller I/O. This module does not
//! touch game pointers or a replay browser.

use training_mod_macros::{
    InputFrame, MacroError, MacroRecording, MomentumMode, Placement, StartState, MAX_NAME_CHARS,
};

use crate::{NX_SUBMENU_COLUMNS, NX_SUBMENU_ROWS};

const PAGE: usize = NX_SUBMENU_ROWS * NX_SUBMENU_COLUMNS;
/// Matches the legacy recorder's flick-jump bit. Other buttons count as input.
const FLICK_JUMP: u32 = 0x8000;
const STICK_NEUTRAL: i8 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pad {
    Left,
    Right,
    Up,
    Down,
    Confirm,
    Back,
    /// Y: backspace while naming.
    Extra,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    None,
    Notify(String),
    Save(Box<MacroRecording>),
    Overwrite {
        id: String,
        recording: Box<MacroRecording>,
    },
    Choose(String),
    NeedCharacter(i32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryRow {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub title: String,
    pub footer: String,
    pub labels: Vec<String>,
    pub selected: usize,
}

#[derive(Clone, Copy, Debug)]
enum Key {
    Char(char),
    Backspace,
    Clear,
    Next,
    Prev,
    Save,
    Cancel,
    Place,
    Momentum,
    Nudge(i16, i16),
    Keep,
    Write,
    SaveAs,
    Back,
    Empty,
}

struct Loaded {
    id: Option<String>,
    recording: MacroRecording,
}

struct NameDraft {
    name: String,
    page: usize,
    cursor: usize,
    frames: Vec<InputFrame>,
    start: StartState,
    placement: Placement,
    version: String,
}

struct PlaceDraft {
    working: Placement,
    cursor: usize,
    /// Return to the save dialog instead of closing.
    from_save: bool,
}

struct LibraryPage {
    rows: Vec<LibraryRow>,
    skipped: usize,
    cursor: usize,
}

enum Screen {
    Closed,
    Save(Box<NameDraft>),
    Place(Box<PlaceDraft>),
    Library(Box<LibraryPage>),
}

/// Menu, naming, placement, and the restore/character sequence for one macro.
pub struct MacroUi {
    screen: Screen,
    loaded: Option<Loaded>,
    /// Name dialog parked while its placement page is open.
    held_save: Option<Box<NameDraft>>,
    /// CPU position from the latest save-state restore, before the offset.
    anchor: Option<(f32, f32)>,
    anchor_generation: Option<u64>,
    edit_anchor: Option<(f32, f32)>,
    pending_kind: Option<i32>,
    claimed: bool,
    character_ready: bool,
    playback_on: bool,
    playback_frame: usize,
    playback_len: usize,
    emitted: Option<InputFrame>,
    emitted_tick: Option<u64>,
}

impl MacroUi {
    pub const fn new() -> Self {
        Self {
            screen: Screen::Closed,
            loaded: None,
            held_save: None,
            anchor: None,
            anchor_generation: None,
            edit_anchor: None,
            pending_kind: None,
            claimed: false,
            character_ready: false,
            playback_on: false,
            playback_frame: 0,
            playback_len: 0,
            emitted: None,
            emitted_tick: None,
        }
    }

    pub fn is_open(&self) -> bool {
        !matches!(self.screen, Screen::Closed)
    }

    pub fn playback_allowed(&self) -> bool {
        self.character_ready && !self.is_open() && self.loaded.is_some()
    }

    pub fn loaded_id(&self) -> Option<&str> {
        self.loaded.as_ref().and_then(|loaded| loaded.id.as_deref())
    }

    pub fn loaded_frames(&self) -> Option<&[InputFrame]> {
        self.loaded
            .as_ref()
            .map(|loaded| loaded.recording.frames.as_slice())
    }

    pub fn loaded_facing(&self) -> Option<f32> {
        self.loaded
            .as_ref()
            .map(|loaded| loaded.recording.start.facing)
    }

    pub fn loaded_start(&self) -> Option<StartState> {
        self.loaded.as_ref().map(|loaded| loaded.recording.start)
    }

    pub fn loaded_placement(&self) -> Option<Placement> {
        self.loaded
            .as_ref()
            .map(|loaded| loaded.recording.placement)
    }

    /// Native frontends supply their system keyboard instead of the terminal grid.
    pub fn name_prompt(&self) -> Option<&str> {
        match &self.screen { Screen::Save(draft) => Some(&draft.name), _ => None }
    }

    pub fn native_name_view(&self) -> Option<View> {
        let Screen::Save(draft) = &self.screen else { return None };
        Some(View { title: "Save Recording".into(), selected: 0,
            labels: vec!["Name and save".into()],
            footer: format!("{} · {}f · {:.2}s\nA: system keyboard   Y: placement   B: return",
                shown_name(&draft.name), draft.frames.len(), draft.frames.len() as f32 / 60.0) })
    }

    /// Validation failure leaves the full draft available for correction/retry.
    pub fn confirm_native_name(&mut self, name: String) -> Effect {
        let Screen::Save(draft) = &mut self.screen else { return Effect::None };
        draft.name = name;
        self.finish_save()
    }

    pub fn native_name_placement(&mut self) -> Effect { self.open_place_from_save() }

    pub fn view(&self) -> Option<View> {
        let mut labels = vec![String::new(); PAGE];
        let (title, footer, selected) = match &self.screen {
            Screen::Closed => return None,
            Screen::Save(draft) => {
                fill_keys(&mut labels, name_keys(draft.page));
                (
                    "Save Recording".to_owned(),
                    format!(
                        "Name: {}    page {}/2\nFighter {} · {}f · {:.2}s. A confirms, B cancels, Y deletes.",
                        shown_name(&draft.name),
                        draft.page + 1,
                        draft.start.fighter_kind,
                        draft.frames.len(),
                        draft.frames.len() as f32 / 60.0
                    ),
                    draft.cursor,
                )
            }
            Screen::Place(draft) => {
                fill_keys(&mut labels, place_keys());
                (
                    "Placement".to_owned(),
                    format!(
                        "Offset X {:.2}  Y {:.2}  {}\nAnchor is the restored CPU position. Inputs stay recorded.",
                        draft.working.delta_x,
                        draft.working.delta_y,
                        momentum_label(draft.working.momentum)
                    ),
                    draft.cursor,
                )
            }
            Screen::Library(page) => {
                let start = (page.cursor / PAGE) * PAGE;
                for (slot, label) in labels.iter_mut().enumerate() {
                    if let Some(row) = page.rows.get(start + slot) {
                        *label = clip_label(&row.label);
                    }
                }
                let pages = page.rows.len().div_ceil(PAGE).max(1);
                let selected_label = page
                    .rows
                    .get(page.cursor)
                    .map(|row| row.label.as_str())
                    .unwrap_or("No recordings");
                (
                    "Named Library".to_owned(),
                    format!(
                        "{selected_label}\npage {}/{pages}  skipped {}    A loads, B closes.",
                        page.cursor / PAGE + 1,
                        page.skipped
                    ),
                    page.cursor % PAGE,
                )
            }
        };
        Some(View {
            title,
            footer,
            labels,
            selected,
        })
    }

    /// Opens the save dialog. An empty or fighter-less take is refused.
    pub fn offer_save(
        &mut self,
        mut frames: Vec<InputFrame>,
        start: StartState,
        crop: bool,
        version: &str,
    ) -> Effect {
        if crop {
            crop_neutral_tail(&mut frames);
        }
        if frames.is_empty() || start.fighter_kind < 0 {
            return Effect::Notify("Recording was empty".to_owned());
        }
        self.playback_on = false;
        self.held_save = None;
        self.screen = Screen::Save(Box::new(NameDraft {
            name: "Recording".to_owned(),
            page: 0,
            cursor: 31,
            frames,
            start,
            placement: Placement::default(),
            version: clip_version(version),
        }));
        Effect::None
    }

    pub fn open_library(&mut self, rows: Vec<LibraryRow>, skipped: usize) {
        self.playback_on = false;
        self.screen = Screen::Library(Box::new(LibraryPage {
            rows,
            skipped,
            cursor: 0,
        }));
    }

    /// The supplied reference must come from the Training restore, never live movement.
    pub fn open_editor(&mut self, restored_reference: Option<(f32, f32)>) -> bool {
        if self.loaded.is_none() {
            return false;
        }
        let Some(anchor) = self.anchor.or(restored_reference) else {
            return false;
        };
        let working = self.loaded.as_ref().unwrap().recording.placement;
        self.edit_anchor = Some(anchor);
        self.playback_on = false;
        self.screen = Screen::Place(Box::new(PlaceDraft {
            working,
            cursor: 1,
            from_save: false,
        }));
        true
    }

    pub fn on_pad(&mut self, pad: Pad) -> Effect {
        match pad {
            Pad::Left => self.shift(0, -1),
            Pad::Right => self.shift(0, 1),
            Pad::Up => self.shift(-1, 0),
            Pad::Down => self.shift(1, 0),
            Pad::Confirm => self.confirm(),
            Pad::Back => self.back(),
            Pad::Extra => self.extra(),
        }
    }

    /// Install a macro that was just written or loaded. Closes any panel.
    pub fn commit_loaded(
        &mut self,
        id: String,
        recording: MacroRecording,
        cpu_kind: Option<i32>,
    ) -> Result<Effect, MacroError> {
        recording.validate()?;
        self.install(Some(id), recording, cpu_kind)
    }

    /// Validated recording from the replay-export stream. No selector is created.
    pub fn import_recording(
        &mut self,
        recording: MacroRecording,
        cpu_kind: Option<i32>,
    ) -> Result<Effect, MacroError> {
        recording.validate()?;
        self.install(None, recording, cpu_kind)
    }

    /// One character-load request. Further calls do nothing until a new commit.
    pub fn claim_character_request(&mut self) -> Option<i32> {
        if self.claimed || self.character_ready {
            return None;
        }
        let kind = self.pending_kind?;
        self.claimed = true;
        Some(kind)
    }

    pub fn observe_cpu(&mut self, kind: Option<i32>, busy: bool) {
        if busy {
            self.character_ready = false;
            return;
        }
        self.character_ready = self.loaded_start().is_some_and(|start| Some(start.fighter_kind) == kind);
        if self.character_ready {
            self.pending_kind = None;
            self.claimed = false;
        }
    }

    /// Absolute CPU position for this restore. The same generation keeps the
    /// first anchor, so a later call with the already-offset position does not
    /// add the delta again.
    pub fn restored_target(
        &mut self,
        generation: u64,
        saved_anchor: (f32, f32),
    ) -> Option<(f32, f32)> {
        if self.anchor_generation != Some(generation) {
            self.anchor = Some(saved_anchor);
            self.anchor_generation = Some(generation);
        }
        let placement = self.loaded.as_ref()?.recording.placement;
        let anchor = self.anchor?;
        Some(placement.apply(anchor))
    }

    /// Airborne recordings retain their situation at a grounded/zero-offset anchor.
    pub fn restore_in_air(&self) -> bool {
        self.loaded.as_ref().is_some_and(|loaded| {
            loaded.recording.start.situation == 2
                || loaded.recording.start.speed_y != 0.0
                || loaded.recording.placement.delta_y != 0.0
        })
    }

    /// Speeds to apply at playback frame zero, once Wait/Fall is ready.
    /// AtRest is explicit: drift while the other fighter restores is not retained.
    pub fn ready_velocity(&self, settled: bool) -> Option<(f32, f32)> {
        let loaded = self.loaded.as_ref()?;
        if !settled {
            return None;
        }
        if loaded.recording.placement.momentum == MomentumMode::AtRest {
            return Some((0.0, 0.0));
        }
        Some((
            loaded.recording.start.speed_x,
            loaded.recording.start.speed_y,
        ))
    }

    pub fn marker(&self) -> Option<(f32, f32)> {
        let Screen::Place(draft) = &self.screen else {
            return None;
        };
        let anchor = self.anchor.or(self.edit_anchor)?;
        Some(draft.working.apply(anchor))
    }

    pub fn arm_playback(&mut self) -> bool {
        if !self.playback_allowed() {
            return false;
        }
        let len = self.loaded.as_ref().unwrap().recording.frames.len();
        self.playback_on = len > 0;
        self.playback_frame = 0;
        self.playback_len = len;
        self.emitted = None;
        self.emitted_tick = None;
        self.playback_on
    }

    /// Returns the input for `tick`. A repeated tick does not advance.
    pub fn feed_playback(&mut self, tick: u64) -> Option<InputFrame> {
        if self.emitted_tick == Some(tick) {
            return self.emitted;
        }
        if !self.playback_on {
            return None;
        }
        let frame = self
            .loaded
            .as_ref()?
            .recording
            .frames
            .get(self.playback_frame)
            .copied();
        self.emitted = frame;
        self.emitted_tick = Some(tick);
        if self.playback_frame + 1 >= self.playback_len {
            self.playback_on = false;
        } else {
            self.playback_frame += 1;
        }
        frame
    }

    pub fn playback_frame(&self) -> usize {
        self.playback_frame
    }

    /// Facing, status, and frames for one playback. Empty when the fighter is not ready.
    pub fn copy_playback(&self) -> Option<(f32, i32, Vec<InputFrame>)> {
        if !self.playback_allowed() {
            return None;
        }
        let recording = &self.loaded.as_ref()?.recording;
        if recording.frames.is_empty() {
            return None;
        }
        Some((
            recording.start.facing,
            recording.start.status,
            recording.frames.clone(),
        ))
    }

    /// Put an unsaved recording back on screen after the disk write failed.
    pub fn restore_draft(&mut self, recording: MacroRecording) {
        self.screen = Screen::Save(Box::new(NameDraft {
            name: recording.name.clone(),
            page: 0,
            cursor: 31,
            frames: recording.frames.clone(),
            start: recording.start,
            placement: recording.placement,
            version: recording.game_version.clone(),
        }));
        self.playback_on = false;
    }

    pub fn close_transient(&mut self) {
        self.screen = Screen::Closed;
        self.held_save = None;
        self.playback_on = false;
        self.emitted = None;
        self.emitted_tick = None;
        self.anchor = None;
        self.anchor_generation = None;
        self.edit_anchor = None;
    }

    fn install(
        &mut self,
        id: Option<String>,
        recording: MacroRecording,
        cpu_kind: Option<i32>,
    ) -> Result<Effect, MacroError> {
        let kind = recording.start.fighter_kind;
        self.loaded = Some(Loaded { id, recording });
        self.screen = Screen::Closed;
        self.held_save = None;
        self.playback_on = false;
        self.playback_frame = 0;
        self.playback_len = 0;
        if cpu_kind == Some(kind) {
            self.character_ready = true;
            self.pending_kind = None;
            self.claimed = false;
            Ok(Effect::None)
        } else {
            self.character_ready = false;
            self.pending_kind = Some(kind);
            self.claimed = false;
            Ok(Effect::NeedCharacter(kind))
        }
    }

    fn shift(&mut self, d_row: i32, d_col: i32) -> Effect {
        match &mut self.screen {
            Screen::Save(draft) => draft.cursor = nudge(draft.cursor, d_row, d_col),
            Screen::Place(draft) => draft.cursor = nudge(draft.cursor, d_row, d_col),
            Screen::Library(page) => {
                page.cursor = move_library(page.cursor, page.rows.len(), d_row, d_col);
            }
            Screen::Closed => {}
        }
        Effect::None
    }

    fn confirm(&mut self) -> Effect {
        match &self.screen {
            Screen::Save(draft) => {
                let key = name_keys(draft.page)[draft.cursor];
                self.activate_name(key)
            }
            Screen::Place(draft) => {
                let key = place_keys()[draft.cursor];
                self.activate_place(key)
            }
            Screen::Library(page) => {
                let id = page.rows.get(page.cursor).map(|row| row.id.clone());
                match id {
                    Some(id) => {
                        self.screen = Screen::Closed;
                        Effect::Choose(id)
                    }
                    None => Effect::Notify("No recordings saved yet".to_owned()),
                }
            }
            Screen::Closed => Effect::None,
        }
    }

    fn activate_name(&mut self, key: Key) -> Effect {
        match key {
            Key::Char(ch) => {
                self.push_char(ch);
                Effect::None
            }
            Key::Backspace => {
                self.pop_char();
                Effect::None
            }
            Key::Clear => {
                if let Screen::Save(draft) = &mut self.screen {
                    draft.name.clear();
                }
                Effect::None
            }
            Key::Next => {
                if let Screen::Save(draft) = &mut self.screen {
                    draft.page = 1;
                    draft.cursor = 0;
                }
                Effect::None
            }
            Key::Prev => {
                if let Screen::Save(draft) = &mut self.screen {
                    draft.page = 0;
                    draft.cursor = 0;
                }
                Effect::None
            }
            Key::Save => self.finish_save(),
            Key::Cancel => self.discard(),
            Key::Place => self.open_place_from_save(),
            Key::Momentum => {
                self.toggle_draft_momentum();
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn activate_place(&mut self, key: Key) -> Effect {
        match key {
            Key::Nudge(x, y) => {
                if let Screen::Place(draft) = &mut self.screen {
                    draft.working.delta_x += f32::from(x) / 4.0;
                    draft.working.delta_y += f32::from(y) / 4.0;
                }
                Effect::None
            }
            Key::Momentum => {
                if let Screen::Place(draft) = &mut self.screen {
                    draft.working.momentum = flip(draft.working.momentum);
                }
                Effect::None
            }
            Key::Keep => self.keep_temporary(),
            Key::Write => self.write_over(),
            Key::SaveAs => self.save_as(),
            Key::Back | Key::Cancel => self.back(),
            _ => Effect::None,
        }
    }

    fn push_char(&mut self, ch: char) {
        let Screen::Save(draft) = &mut self.screen else {
            return;
        };
        if draft.name.chars().count() >= MAX_NAME_CHARS || ch.is_control() {
            return;
        }
        draft.name.push(ch);
    }

    fn pop_char(&mut self) {
        let Screen::Save(draft) = &mut self.screen else {
            return;
        };
        draft.name.pop();
    }

    fn extra(&mut self) -> Effect {
        if matches!(self.screen, Screen::Save(_)) {
            self.pop_char();
        }
        Effect::None
    }

    fn toggle_draft_momentum(&mut self) {
        if let Screen::Save(draft) = &mut self.screen {
            draft.placement.momentum = flip(draft.placement.momentum);
        }
    }

    fn open_place_from_save(&mut self) -> Effect {
        if self.anchor.is_none() {
            return Effect::Notify("Restore a Training state before editing placement".into());
        }
        let screen = std::mem::replace(&mut self.screen, Screen::Closed);
        let Screen::Save(draft) = screen else {
            self.screen = screen;
            return Effect::None;
        };
        let working = draft.placement;
        self.held_save = Some(draft);
        self.screen = Screen::Place(Box::new(PlaceDraft {
            working,
            cursor: 1,
            from_save: true,
        }));
        Effect::None
    }

    fn finish_save(&mut self) -> Effect {
        let (name, version, start, frames, placement) = match &self.screen {
            Screen::Save(draft) => (
                draft.name.clone(),
                draft.version.clone(),
                draft.start,
                draft.frames.clone(),
                draft.placement,
            ),
            _ => return Effect::None,
        };
        let built = MacroRecording::new(name.trim(), version, start, frames);
        let mut recording = match built {
            Ok(recording) => recording,
            Err(err) => return Effect::Notify(err.to_string()),
        };
        if let Err(err) = recording.set_placement(placement) {
            return Effect::Notify(err.to_string());
        }
        self.screen = Screen::Closed;
        Effect::Save(Box::new(recording))
    }

    fn keep_temporary(&mut self) -> Effect {
        let (placement, from_save) = match &self.screen {
            Screen::Place(draft) => (draft.working, draft.from_save),
            _ => return Effect::None,
        };
        if from_save {
            return self.restore_save_from_place(placement);
        }
        if let Some(loaded) = self.loaded.as_mut() {
            if loaded.recording.set_placement(placement).is_err() {
                return Effect::Notify("Placement is not usable".to_owned());
            }
        }
        self.screen = Screen::Closed;
        Effect::Notify("Placement kept for this session".to_owned())
    }

    fn write_over(&mut self) -> Effect {
        let placement = match &self.screen {
            Screen::Place(draft) => draft.working,
            _ => return Effect::None,
        };
        let Some(id) = self.loaded.as_ref().and_then(|loaded| loaded.id.clone()) else {
            return Effect::Notify("Save the recording before overwriting a file".to_owned());
        };
        let Some(loaded) = self.loaded.as_mut() else {
            return Effect::None;
        };
        if loaded.recording.set_placement(placement).is_err() {
            return Effect::Notify("Placement is not usable".to_owned());
        }
        let recording = Box::new(loaded.recording.clone());
        self.screen = Screen::Closed;
        Effect::Overwrite { id, recording }
    }

    fn save_as(&mut self) -> Effect {
        let placement = match &self.screen {
            Screen::Place(draft) => draft.working,
            _ => return Effect::None,
        };
        let Some(source) = self.loaded.as_ref() else {
            return Effect::Notify("Nothing to save".to_owned());
        };
        let draft = NameDraft {
            name: source.recording.name.clone(),
            page: 0,
            cursor: 31,
            frames: source.recording.frames.clone(),
            start: source.recording.start,
            placement,
            version: source.recording.game_version.clone(),
        };
        self.held_save = None;
        self.screen = Screen::Save(Box::new(draft));
        Effect::None
    }

    fn back(&mut self) -> Effect {
        let from_save = matches!(&self.screen, Screen::Place(draft) if draft.from_save);
        if from_save {
            let placement = match &self.screen {
                Screen::Place(draft) => draft.working,
                _ => Placement::default(),
            };
            return self.restore_save_from_place(placement);
        }
        match &self.screen {
            Screen::Place(_) | Screen::Save(_) | Screen::Library(_) => self.discard(),
            Screen::Closed => Effect::None,
        }
    }

    fn restore_save_from_place(&mut self, placement: Placement) -> Effect {
        let Some(mut held) = self.held_save.take() else {
            self.screen = Screen::Closed;
            return Effect::None;
        };
        held.placement = placement;
        self.screen = Screen::Save(held);
        Effect::None
    }

    fn discard(&mut self) -> Effect {
        self.held_save = None;
        self.screen = Screen::Closed;
        Effect::None
    }
}

impl Default for MacroUi {
    fn default() -> Self {
        Self::new()
    }
}

fn nudge(cursor: usize, d_row: i32, d_col: i32) -> usize {
    let cols = NX_SUBMENU_COLUMNS as i32;
    let row = ((cursor / NX_SUBMENU_COLUMNS) as i32 + d_row).clamp(0, NX_SUBMENU_ROWS as i32 - 1);
    let col = ((cursor % NX_SUBMENU_COLUMNS) as i32 + d_col).clamp(0, cols - 1);
    (row as usize) * NX_SUBMENU_COLUMNS + col as usize
}

fn move_library(cursor: usize, count: usize, d_row: i32, d_col: i32) -> usize {
    if count == 0 {
        return 0;
    }
    let cols = NX_SUBMENU_COLUMNS as i32;
    let mut page = (cursor / PAGE) as i32;
    let local = cursor % PAGE;
    let mut row = (local / NX_SUBMENU_COLUMNS) as i32 + d_row;
    let col = ((local % NX_SUBMENU_COLUMNS) as i32 + d_col).clamp(0, cols - 1);
    if row < 0 {
        if page > 0 {
            page -= 1;
            row = NX_SUBMENU_ROWS as i32 - 1;
        } else {
            row = 0;
        }
    } else if row >= NX_SUBMENU_ROWS as i32 {
        let pages = count.div_ceil(PAGE) as i32;
        if page + 1 < pages {
            page += 1;
            row = 0;
        } else {
            row = NX_SUBMENU_ROWS as i32 - 1;
        }
    }
    let index = page as usize * PAGE + row as usize * NX_SUBMENU_COLUMNS + col as usize;
    index.min(count - 1)
}

fn name_keys(page: usize) -> [Key; PAGE] {
    let mut keys = [Key::Empty; PAGE];
    if page == 0 {
        for (index, ch) in "ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().enumerate() {
            keys[index] = Key::Char(ch);
        }
        keys[26] = Key::Char('0');
        keys[27] = Key::Char('1');
        keys[28] = Key::Backspace;
        keys[29] = Key::Char(' ');
        keys[30] = Key::Next;
        keys[31] = Key::Save;
    } else {
        for (index, ch) in "23456789_-.'".chars().enumerate() {
            keys[index] = Key::Char(ch);
        }
        keys[12] = Key::Backspace;
        keys[13] = Key::Clear;
        keys[14] = Key::Prev;
        keys[16] = Key::Save;
        keys[17] = Key::Cancel;
        keys[18] = Key::Place;
        keys[19] = Key::Momentum;
    }
    keys
}

fn place_keys() -> [Key; PAGE] {
    let mut keys = [Key::Empty; PAGE];
    // Whole units are stored as quarters so the cell value stays an integer.
    keys[0] = Key::Nudge(-4, 0);
    keys[1] = Key::Nudge(4, 0);
    keys[2] = Key::Nudge(0, -4);
    keys[3] = Key::Nudge(0, 4);
    keys[4] = Key::Nudge(-1, 0);
    keys[5] = Key::Nudge(1, 0);
    keys[6] = Key::Nudge(0, -1);
    keys[7] = Key::Nudge(0, 1);
    keys[8] = Key::Momentum;
    keys[9] = Key::Keep;
    keys[10] = Key::Write;
    keys[11] = Key::SaveAs;
    keys[12] = Key::Back;
    keys
}

fn fill_keys(labels: &mut [String], keys: [Key; PAGE]) {
    for (label, key) in labels.iter_mut().zip(keys) {
        *label = key_label(key).to_owned();
    }
}

fn key_label(key: Key) -> &'static str {
    match key {
        Key::Char(' ') => "Space",
        Key::Char(ch) => match ch {
            'A' => "A",
            'B' => "B",
            'C' => "C",
            'D' => "D",
            'E' => "E",
            'F' => "F",
            'G' => "G",
            'H' => "H",
            'I' => "I",
            'J' => "J",
            'K' => "K",
            'L' => "L",
            'M' => "M",
            'N' => "N",
            'O' => "O",
            'P' => "P",
            'Q' => "Q",
            'R' => "R",
            'S' => "S",
            'T' => "T",
            'U' => "U",
            'V' => "V",
            'W' => "W",
            'X' => "X",
            'Y' => "Y",
            'Z' => "Z",
            '0' => "0",
            '1' => "1",
            '2' => "2",
            '3' => "3",
            '4' => "4",
            '5' => "5",
            '6' => "6",
            '7' => "7",
            '8' => "8",
            '9' => "9",
            '_' => "_",
            '-' => "-",
            '.' => ".",
            '\'' => "'",
            _ => "?",
        },
        Key::Backspace => "Del",
        Key::Clear => "Clear",
        Key::Next => "Next",
        Key::Prev => "Prev",
        Key::Save => "Save",
        Key::Cancel => "Cancel",
        Key::Place => "Place",
        Key::Momentum => "Momentum",
        Key::Nudge(-4, 0) => "X-",
        Key::Nudge(4, 0) => "X+",
        Key::Nudge(0, -4) => "Y-",
        Key::Nudge(0, 4) => "Y+",
        Key::Nudge(-1, 0) => "x-",
        Key::Nudge(1, 0) => "x+",
        Key::Nudge(0, -1) => "y-",
        Key::Nudge(0, 1) => "y+",
        Key::Keep => "Keep",
        Key::Write => "Write",
        Key::SaveAs => "New",
        Key::Back => "Back",
        Key::Empty | Key::Nudge(_, _) => "",
    }
}

fn momentum_label(mode: MomentumMode) -> &'static str {
    match mode {
        MomentumMode::PreserveRecorded => "Keep recorded speed",
        MomentumMode::AtRest => "Start at rest",
    }
}

fn flip(mode: MomentumMode) -> MomentumMode {
    match mode {
        MomentumMode::PreserveRecorded => MomentumMode::AtRest,
        MomentumMode::AtRest => MomentumMode::PreserveRecorded,
    }
}

fn shown_name(name: &str) -> String {
    if name.is_empty() {
        "(empty)".to_owned()
    } else {
        name.to_owned()
    }
}

fn clip_label(label: &str) -> String {
    label.chars().take(16).collect()
}

fn clip_version(version: &str) -> String {
    let mut out = String::new();
    for ch in version.chars().filter(|ch| !ch.is_control()) {
        if out.len() + ch.len_utf8() > training_mod_macros::MAX_GAME_VERSION_BYTES {
            break;
        }
        out.push(ch);
    }
    out
}

pub fn crop_neutral_tail(frames: &mut Vec<InputFrame>) {
    while frames.last().is_some_and(neutral_frame) {
        frames.pop();
    }
}

fn neutral_frame(frame: &InputFrame) -> bool {
    let stick = |value: i8| value.unsigned_abs() < STICK_NEUTRAL as u8;
    (frame.buttons == 0 || frame.buttons == FLICK_JUMP)
        && stick(frame.lstick_x)
        && stick(frame.lstick_y)
        && stick(frame.rstick_x)
        && stick(frame.rstick_y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start() -> StartState {
        StartState {
            fighter_kind: 0x08,
            costume: 2,
            facing: -1.0,
            situation: 2,
            status: 0x22,
            speed_x: 0.5,
            speed_y: 1.25,
        }
    }

    fn attack() -> InputFrame {
        InputFrame {
            buttons: 0x1,
            lstick_x: 0,
            lstick_y: 40,
            rstick_x: 0,
            rstick_y: 0,
        }
    }

    fn frames() -> Vec<InputFrame> {
        vec![attack()]
    }

    fn goto(ui: &mut MacroUi, target: usize) {
        for _ in 0..64 {
            let current = ui.view().expect("panel open").selected;
            if current == target {
                return;
            }
            let row = current / NX_SUBMENU_COLUMNS;
            let col = current % NX_SUBMENU_COLUMNS;
            let target_row = target / NX_SUBMENU_COLUMNS;
            let target_col = target % NX_SUBMENU_COLUMNS;
            let pad = if row < target_row {
                Pad::Down
            } else if row > target_row {
                Pad::Up
            } else if col < target_col {
                Pad::Right
            } else {
                Pad::Left
            };
            ui.on_pad(pad);
        }
        panic!("cursor stayed at {}", ui.view().unwrap().selected);
    }

    #[test]
    fn native_keyboard_validation_preserves_draft_and_inputs() {
        let mut ui = MacroUi::new();
        ui.offer_save(frames(), start(), false, "13.0.5");
        assert_eq!(ui.name_prompt(), Some("Recording"));
        assert_eq!(ui.native_name_view().unwrap().labels, ["Name and save"]);
        assert!(matches!(ui.confirm_native_name("   ".into()), Effect::Notify(_)));
        assert!(ui.is_open());
        let Effect::Save(recording) = ui.confirm_native_name("  Aerial turn 2  ".into()) else { panic!() };
        assert_eq!(recording.name, "Aerial turn 2");
        assert_eq!(recording.frames, frames());
        assert_eq!(recording.start, start());
        assert_eq!(recording.placement.momentum, MomentumMode::PreserveRecorded);
        assert!(ui.name_prompt().is_none());
    }

    #[test]
    fn default_name_saves_and_cancel_discards() {
        let mut ui = MacroUi::new();
        assert!(matches!(
            ui.offer_save(frames(), start(), false, "13.0.5"),
            Effect::None
        ));
        let saved = ui.on_pad(Pad::Confirm);
        let Effect::Save(recording) = saved else {
            panic!("expected save");
        };
        assert_eq!(recording.name, "Recording");
        assert_eq!(recording.frames, frames());
        assert_eq!(recording.start.facing, -1.0);
        assert_eq!(recording.placement.momentum, MomentumMode::PreserveRecorded);
        assert!(!ui.is_open());

        ui.offer_save(frames(), start(), false, "13.0.5");
        assert!(matches!(ui.on_pad(Pad::Back), Effect::None));
        assert!(!ui.is_open());
        assert!(ui.loaded_id().is_none());
    }

    #[test]
    fn empty_name_stays_on_the_dialog_and_typed_name_pages() {
        let mut ui = MacroUi::new();
        ui.offer_save(frames(), start(), false, "13.0.5");
        for _ in 0..9 {
            ui.on_pad(Pad::Extra);
        }
        assert!(matches!(ui.on_pad(Pad::Confirm), Effect::Notify(_)));
        assert!(ui.is_open());

        goto(&mut ui, 0);
        ui.on_pad(Pad::Confirm);
        goto(&mut ui, 30);
        ui.on_pad(Pad::Confirm);
        assert_eq!(ui.view().unwrap().selected, 0);
        ui.on_pad(Pad::Confirm);
        goto(&mut ui, 16);
        let Effect::Save(recording) = ui.on_pad(Pad::Confirm) else {
            panic!("expected save");
        };
        assert_eq!(recording.name, "A2");
        assert_eq!(recording.frames.len(), 1);
    }

    #[test]
    fn crop_drops_only_the_neutral_tail() {
        let mut ui = MacroUi::new();
        let mut recorded = frames();
        recorded.push(InputFrame::default());
        recorded.push(InputFrame {
            buttons: FLICK_JUMP,
            ..InputFrame::default()
        });
        ui.offer_save(recorded, start(), true, "13.0.5");
        let Effect::Save(recording) = ui.on_pad(Pad::Confirm) else {
            panic!("expected save");
        };
        assert_eq!(recording.frames, frames());
    }

    #[test]
    fn library_pages_and_cancel_keeps_the_previous_choice() {
        let mut ui = MacroUi::new();
        ui.commit_loaded("kept".to_owned(), sample(Placement::default()), Some(0x08))
            .unwrap();
        let rows: Vec<LibraryRow> = (0..40)
            .map(|index| LibraryRow {
                id: format!("id{index}"),
                label: format!("macro {index}"),
            })
            .collect();
        ui.open_library(rows, 2);
        for _ in 0..8 {
            ui.on_pad(Pad::Down);
        }
        ui.on_pad(Pad::Right);
        let view = ui.view().unwrap();
        assert_eq!(view.labels[1], "macro 33");
        assert!(view.footer.contains("page 2/2"));
        let Effect::Choose(id) = ui.on_pad(Pad::Confirm) else {
            panic!("expected choose");
        };
        assert_eq!(id, "id33");
        assert_eq!(ui.loaded_id(), Some("kept"));

        ui.open_library(
            vec![LibraryRow {
                id: "other".to_owned(),
                label: "other".to_owned(),
            }],
            0,
        );
        assert!(matches!(ui.on_pad(Pad::Back), Effect::None));
        assert!(!ui.is_open());
        assert_eq!(ui.loaded_id(), Some("kept"));
    }

    #[test]
    fn placement_edit_keeps_inputs_and_can_stay_temporary() {
        let mut ui = MacroUi::new();
        let original = frames();
        ui.commit_loaded("drill".to_owned(), sample(Placement::default()), Some(0x08))
            .unwrap();
        assert!(ui.open_editor(Some((4.0, 8.0))));
        ui.on_pad(Pad::Confirm);
        assert!(matches!(ui.on_pad(Pad::Back), Effect::None));
        assert_eq!(ui.loaded_placement().unwrap().delta_x, 0.0);
        assert_eq!(ui.loaded_frames(), Some(original.as_slice()));

        assert!(ui.open_editor(Some((4.0, 8.0))));
        ui.on_pad(Pad::Confirm);
        goto(&mut ui, 9);
        assert!(matches!(ui.on_pad(Pad::Confirm), Effect::Notify(_)));
        let placement = ui.loaded_placement().unwrap();
        assert_eq!(placement.delta_x, 1.0);
        assert_eq!(placement.momentum, MomentumMode::PreserveRecorded);
        assert_eq!(ui.loaded_facing(), Some(-1.0));
        assert_eq!(ui.loaded_frames(), Some(original.as_slice()));
        assert!(!ui.is_open());
    }

    #[test]
    fn manual_fighter_change_invalidates_loaded_playback() {
        let mut ui = MacroUi::new();
        ui.commit_loaded("drill".into(), sample(Placement::default()), Some(0x08)).unwrap();
        assert!(ui.playback_allowed());
        ui.observe_cpu(Some(0x12), false);
        assert!(!ui.playback_allowed());
        assert!(!ui.arm_playback());
        ui.observe_cpu(Some(0x08), true);
        assert!(!ui.playback_allowed());
        ui.observe_cpu(Some(0x08), false);
        assert!(ui.playback_allowed());
    }

    #[test]
    fn placement_needs_restored_reference_and_save_keeps_it() {
        let mut ui = MacroUi::new();
        ui.commit_loaded("drill".into(), sample(Placement::default()), Some(0x08)).unwrap();
        assert!(!ui.open_editor(None));
        ui.restored_target(1, (5.0, 8.0));
        ui.commit_loaded("copy".into(), sample(Placement::default()), Some(0x08)).unwrap();
        assert!(ui.open_editor(None));
        assert_eq!(ui.marker(), Some((5.0, 8.0)));
        ui.close_transient();
        assert!(!ui.open_editor(None));
    }

    #[test]
    fn save_dialog_placement_round_trip_and_momentum() {
        let mut ui = MacroUi::new();
        // A restored reference exists even before the first macro is saved.
        assert!(ui.restored_target(1, (4.0, 8.0)).is_none());
        ui.offer_save(frames(), start(), false, "13.0.5");
        ui.on_pad(Pad::Left);
        ui.on_pad(Pad::Confirm);
        goto(&mut ui, 18);
        ui.on_pad(Pad::Confirm);
        ui.on_pad(Pad::Confirm);
        goto(&mut ui, 8);
        ui.on_pad(Pad::Confirm);
        ui.on_pad(Pad::Back);
        goto(&mut ui, 16);
        let Effect::Save(recording) = ui.on_pad(Pad::Confirm) else {
            panic!("expected save");
        };
        assert_eq!(recording.placement.delta_x, 1.0);
        assert_eq!(recording.placement.momentum, MomentumMode::AtRest);
        assert_eq!(recording.frames, frames());
        assert_eq!(recording.start.status, 0x22);
    }

    #[test]
    fn restore_uses_the_first_anchor_for_the_whole_generation() {
        let mut ui = MacroUi::new();
        let placement = Placement {
            delta_x: 3.0,
            delta_y: 4.0,
            ..Placement::default()
        };
        ui.commit_loaded("drill".to_owned(), sample(placement), Some(0x08))
            .unwrap();
        let first = ui.restored_target(1, (10.0, 20.0)).unwrap();
        let repeated = ui.restored_target(1, first).unwrap();
        let again = ui.restored_target(2, (10.0, 20.0)).unwrap();
        assert_eq!(first, (13.0, 24.0));
        assert_eq!(repeated, first);
        assert_eq!(again, first);
        assert!(ui.ready_velocity(true).is_some());
        let at_rest = Placement {
            momentum: MomentumMode::AtRest,
            ..placement
        };
        ui.commit_loaded("drill".to_owned(), sample(at_rest), Some(0x08))
            .unwrap();
        assert_eq!(ui.ready_velocity(true), Some((0.0, 0.0)));
        assert!(ui.ready_velocity(false).is_none());
    }

    #[test]
    fn aerial_start_survives_zero_offset_and_rest_mode() {
        let mut ui = MacroUi::new();
        let mut recording = sample(Placement::default());
        recording.start.speed_y = -1.5;
        ui.commit_loaded("fall".into(), recording.clone(), Some(0x08)).unwrap();
        assert!(ui.restore_in_air());
        assert_eq!(ui.ready_velocity(true), Some((0.5, -1.5)));
        recording.placement.momentum = MomentumMode::AtRest;
        ui.commit_loaded("rest".into(), recording.clone(), Some(0x08)).unwrap();
        assert!(ui.restore_in_air());
        assert_eq!(ui.ready_velocity(true), Some((0.0, 0.0)));
        recording.start.situation = 0;
        recording.start.speed_y = 0.0;
        ui.commit_loaded("ground".into(), recording, Some(0x08)).unwrap();
        assert!(!ui.restore_in_air());
    }

    #[test]
    fn character_request_is_once_and_playback_does_not_double_advance() {
        let mut ui = MacroUi::new();
        let mut recording = sample(Placement::default());
        recording.frames.push(InputFrame {
            buttons: 0x4,
            ..InputFrame::default()
        });
        let effect = ui.import_recording(recording, Some(0)).unwrap();
        assert!(matches!(effect, Effect::NeedCharacter(0x08)));
        assert_eq!(ui.claim_character_request(), Some(0x08));
        assert_eq!(ui.claim_character_request(), None);
        ui.observe_cpu(Some(0x08), true);
        assert!(!ui.playback_allowed());
        ui.observe_cpu(Some(0x08), false);
        assert!(ui.playback_allowed());
        assert!(ui.arm_playback());
        let first = ui.feed_playback(1);
        let duplicate = ui.feed_playback(1);
        assert_eq!(first, duplicate);
        assert_eq!(ui.playback_frame(), 1);
        ui.restored_target(3, (1.0, 2.0));
        assert_eq!(ui.playback_frame(), 1);
        assert!(ui.feed_playback(1).is_some());
    }

    #[test]
    fn empty_import_is_rejected_without_opening_a_selector() {
        let mut ui = MacroUi::new();
        let empty = MacroRecording {
            schema_version: training_mod_macros::SCHEMA_VERSION,
            name: "Nope".to_owned(),
            game_version: String::new(),
            start: start(),
            placement: Placement::default(),
            frames: Vec::new(),
        };
        assert!(ui.import_recording(empty, None).is_err());
        assert!(!ui.is_open());
        assert!(ui.loaded_id().is_none());
        ui.import_recording(sample(Placement::default()), Some(0x08))
            .unwrap();
        assert!(ui.playback_allowed());
        assert!(!ui.is_open());
    }

    fn sample(placement: Placement) -> MacroRecording {
        let mut recording = MacroRecording::new("Drill", "13.0.5", start(), frames()).unwrap();
        recording.set_placement(placement).unwrap();
        recording
    }
}
