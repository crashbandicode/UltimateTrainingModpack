//! Host-testable named-macro panels and restore/playback sequencing.
//!
//! The Switch runtime draws [`View`] on the existing 8×4 training menu and
//! performs disk, character-load, and controller I/O. This module does not
//! touch game pointers or a replay browser.

use training_mod_macros::{
    InputFrame, MacroError, MacroRecording, MomentumMode, Placement, StartState, MAX_NAME_CHARS,
};

use crate::{NX_SUBMENU_COLUMNS, NX_SUBMENU_ROWS};

mod fighter;
pub use fighter::{gate as fighter_gate, Choice as FighterChoice, Gate as FighterGate, Origin as FighterOrigin};

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

/// Native keyboard header for a numbered or named CPU recording.
pub const MACRO_KEYBOARD_TITLE: &str = "Name macro";
/// Native keyboard header for a setup draft. Callers may pass another literal.
pub const SETUP_KEYBOARD_TITLE: &str = "Name setup";

/// Caller-owned naming session. The TUI stores the token and keyboard title, not the payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameContext {
    pub token: u64,
    pub title: &'static str,
}

/// Whether a recording may be saved as schema 1, with an exact Mii, or not at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MiiIdentity {
    /// Fighter is not a Mii. Schema 1 is the right file.
    Ordinary,
    /// Exact configuration from record start, and its class matches the fighter.
    Exact,
    /// Mii fighter whose exact configuration was missing or belonged to another class.
    Unknown,
}

/// `observed_kind` is `configuration.canonical().class.kind()` when a configuration
/// was actually read. The source (saved, original, or discovered) is not a separate case.
pub fn mii_identity(fighter_kind: i32, observed_kind: Option<u32>) -> MiiIdentity {
    let Some(class) = training_mod_mii::inventory::MiiClass::from_kind(fighter_kind as u32) else {
        return MiiIdentity::Ordinary;
    };
    match observed_kind {
        Some(kind) if kind == class.kind() => MiiIdentity::Exact,
        _ => MiiIdentity::Unknown,
    }
}

pub fn export_mii_refusal(identity: MiiIdentity) -> Option<&'static str> {
    match identity {
        MiiIdentity::Unknown => Some(
            "Exact Mii was not available when recording started. Record this slot again.",
        ),
        MiiIdentity::Ordinary | MiiIdentity::Exact => None,
    }
}

/// Numbered CPU slots hold this many frames. Longer named files stay playable.
pub const NUMBERED_SLOT_FRAMES: usize = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotAdmit {
    Ready,
    Empty,
    OverLimit { frames: usize, limit: usize },
}

/// Full copy, or the original length left untouched. Never a shortened vec.
pub fn copy_numbered_frames(frames: &[InputFrame]) -> Result<Vec<InputFrame>, SlotAdmit> {
    match admit_numbered(frames.len()) {
        SlotAdmit::Ready => Ok(frames.to_vec()),
        other => Err(other),
    }
}

pub fn admit_numbered(frame_count: usize) -> SlotAdmit {
    if frame_count == 0 {
        SlotAdmit::Empty
    } else if frame_count > NUMBERED_SLOT_FRAMES {
        SlotAdmit::OverLimit { frames: frame_count, limit: NUMBERED_SLOT_FRAMES }
    } else {
        SlotAdmit::Ready
    }
}

/// `slot` is the captured facing/status. `None` keeps the shared recorder value
/// and does not park it. A captured slot parks the shared pair so playback can
/// restore it instead of leaving the imported facing behind.
pub fn engage_slot_origin(
    shared: (f32, i32),
    parked: Option<(f32, i32)>,
    slot: Option<(f32, i32)>,
) -> ((f32, i32), Option<(f32, i32)>) {
    match slot {
        Some(origin) => (origin, parked.or(Some(shared))),
        None => (shared, parked),
    }
}

pub fn release_slot_origin(shared: (f32, i32), parked: Option<(f32, i32)>) -> ((f32, i32), Option<(f32, i32)>) {
    match parked {
        Some(saved) => (saved, None),
        None => (shared, None),
    }
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
    /// Y on the named CPU library. A remains [`Effect::Choose`].
    BindToSlot { id: String },
    /// Y on a caller-titled library. The runtime owns the meaning of the row.
    Alternate { id: String },
    /// A on a caller-titled library opened with [`MacroUi::open_library_forward`].
    /// The runtime owns the row; the named-macro loader never sees it.
    ForwardChoose { id: String },
    /// Keyboard confirmed for a payload the runtime still owns.
    NameConfirmed { context: NameContext, name: String },
    NeedCharacter(i32),
    /// Result of a prompt opened for a replay clip or track. `None` is Cancel.
    /// A named-library prompt is applied inside [`MacroUi`] and never reports here.
    ClipFighter(Option<FighterChoice>),
}

/// Everything the runtime knows about the selection being loaded. The panel
/// never reads native state; `matches` comes from the exact readiness predicate.
#[derive(Clone, Debug)]
pub struct LoadContext {
    /// Live CPU kind, if a CPU exists.
    pub cpu_kind: Option<i32>,
    /// The recorded identity is already the settled CPU (same class with a
    /// different exact Mii is not a match).
    pub matches: bool,
    /// Persistent Fighter Prompt setting. False keeps the current CPU silently.
    pub prompt_enabled: bool,
    /// This build can load the recorded identity (an exact Mii needs the Mii
    /// alpha). False offers nothing to switch to, so the current CPU is kept.
    pub can_switch: bool,
    /// Selection revision the runtime will compare before applying the choice.
    pub revision: u64,
    pub recorded_label: String,
    pub current_label: String,
}

impl LoadContext {
    /// The gate for `recording`. A Switch needs an exact recorded target that this
    /// build can load; an older Mii file without its exact Mii has none.
    pub fn gate(&self, recording: &MacroRecording) -> FighterGate {
        let switchable = self.can_switch && recording.playback_target().is_ok();
        fighter::gate(self.prompt_enabled, switchable, self.matches)
    }
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

/// Which CPU a loaded macro's inputs are for. The file itself is never changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plays {
    /// The recorded fighter (and exact Mii) must be the settled CPU.
    Recorded,
    /// An explicit Switch choice waits once for its requested fighter, then
    /// inputs follow the current CPU; later manual swaps are never undone.
    SwitchOnce,
    /// Whatever CPU is settled on stage when playback starts.
    Current,
}

struct Loaded {
    id: Option<String>,
    recording: MacroRecording,
    plays: Plays,
}

/// A selection waiting for the user's fighter choice. Nothing is committed yet,
/// so Cancel leaves the previously loaded macro and the CPU untouched.
struct FighterScreen {
    prompt: fighter::Prompt,
    /// Live CPU kind when the prompt opened, for Keep's readiness.
    cpu_kind: Option<i32>,
    /// Named library only. A clip prompt keeps its candidate in the editor.
    candidate: Option<(String, MacroRecording)>,
    /// Library page to return to on Cancel.
    prior: Option<Box<LibraryPage>>,
}

// NameDraft is already boxed and only one draft is active. Keep the exact Mii
// value inline rather than adding a second allocation to every macro name edit.
#[allow(clippy::large_enum_variant)]
enum NameBody {
    Macro {
        frames: Vec<InputFrame>,
        start: StartState,
        placement: Placement,
        version: String,
        mii: Option<training_mod_mii::lifetime::Configuration>,
    },
    /// Name and caret only. The setup or other draft stays with the caller.
    External { token: u64, summary: String, title: &'static str },
}

struct NameDraft {
    name: String,
    /// Native caret in UTF-16 units; independent of the terminal grid selection.
    native_cursor: usize,
    page: usize,
    cursor: usize,
    body: NameBody,
    /// Header passed to the system keyboard, including after a draft is resumed.
    keyboard_title: &'static str,
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
    title: String,
    /// Y label. Absent means Y does nothing.
    alternate: Option<String>,
    /// True copies into a numbered CPU slot. False reports [`Effect::Alternate`].
    binds_slot: bool,
}

enum Screen {
    Closed,
    Save(Box<NameDraft>),
    Place(Box<PlaceDraft>),
    Library(Box<LibraryPage>),
    Fighter(Box<FighterScreen>),
}

/// Menu, naming, placement, and the restore/character sequence for one macro.
pub struct MacroUi {
    screen: Screen,
    loaded: Option<Loaded>,
    /// Library page the last A choice came from, for a Cancelled fighter prompt.
    picked: Option<Box<LibraryPage>>,
    /// Id and name of a library load a fighter prompt just committed, until the
    /// runtime has remembered it.
    prompt_commit: Option<(String, String)>,
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
            picked: None,
            prompt_commit: None,
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

    pub fn loaded_target(&self) -> Option<training_mod_mii::replay::Target> {
        self.loaded.as_ref()?.recording.playback_target().ok()
    }

    /// Invalidate only unsubmitted intent/readiness; the native adapter owns an
    /// installation already in flight. Used when switching away to a clip.
    pub fn invalidate_character_request(&mut self) {
        self.character_ready = false;
        self.claimed = false;
        // A macro playing on the current CPU never requests a character.
        self.pending_kind = if self.plays_current() {
            None
        } else {
            self.loaded_target().map(|target| target.kind() as i32)
        };
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

    pub fn native_name_cursor(&self) -> Option<usize> {
        match &self.screen { Screen::Save(draft) => Some(draft.native_cursor), _ => None }
    }

    /// Header for the open name draft. Unchanged by caret checkpoints and practice invalidation.
    pub fn native_name_title(&self) -> Option<&'static str> {
        match &self.screen {
            Screen::Save(draft) => Some(draft.keyboard_title),
            _ => None,
        }
    }

    /// Token of the caller-owned name draft on screen, open or parked.
    pub fn external_name_token(&self) -> Option<u64> {
        match &self.screen {
            Screen::Save(draft) => match draft.body {
                NameBody::External { token, .. } => Some(token),
                NameBody::Macro { .. } => None,
            },
            _ => None,
        }
    }

    /// Checkpoint an unfinished native edit, without confirmation or any I/O
    /// effect. Empty/whitespace drafts are valid; final save validation is separate.
    /// Rejected text/caret leaves the entire existing draft untouched.
    pub fn checkpoint_native_name(&mut self, name: &str, cursor_utf16: usize) -> bool {
        let Screen::Save(draft) = &mut self.screen else { return false };
        if name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
            return false;
        }
        let units: Vec<u16> = name.encode_utf16().collect();
        if cursor_utf16 > units.len() || String::from_utf16(&units[..cursor_utf16]).is_err() {
            return false;
        }
        draft.name = name.to_owned();
        draft.native_cursor = cursor_utf16;
        true
    }

    pub fn native_name_view(&self) -> Option<View> {
        let Screen::Save(draft) = &self.screen else { return None };
        let footer = match &draft.body {
            NameBody::Macro { frames, .. } => format!(
                "{} · {}f · {:.2}s\nA: system keyboard   Y: placement   B: return",
                shown_name(&draft.name), frames.len(), frames.len() as f32 / 60.0),
            NameBody::External { summary, .. } => format!(
                "{} · {summary}\nA: system keyboard   B: return",
                shown_name(&draft.name)),
        };
        Some(View { title: draft.keyboard_title.into(), selected: 0,
            labels: vec!["Name and save".into()], footer })
    }

    /// Validation failure leaves the full draft available for correction/retry.
    pub fn confirm_native_name(&mut self, name: String) -> Effect {
        let Screen::Save(draft) = &mut self.screen else { return Effect::None };
        draft.native_cursor = name.encode_utf16().count();
        draft.name = name;
        self.finish_save()
    }

    pub fn native_name_placement(&mut self) -> Effect { self.open_place_from_save() }

    pub fn view(&self) -> Option<View> {
        let mut labels = vec![String::new(); PAGE];
        let (title, footer, selected) = match &self.screen {
            Screen::Closed => return None,
            Screen::Fighter(screen) => return Some(screen.prompt.view()),
            Screen::Save(draft) => {
                fill_keys(&mut labels, name_keys(draft.page));
                let detail = match &draft.body {
                    NameBody::Macro { frames, start, .. } => format!(
                        "Fighter {} · {}f · {:.2}s. A confirms, B cancels, Y deletes.",
                        start.fighter_kind, frames.len(), frames.len() as f32 / 60.0),
                    NameBody::External { summary, .. } => format!(
                        "{summary}. A confirms, B cancels, Y deletes."),
                };
                (
                    draft.keyboard_title.to_owned(),
                    format!(
                        "Name: {}    page {}/2\n{detail}",
                        shown_name(&draft.name),
                        draft.page + 1,
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
                // A reads the file and arms it for playback; it does not start it.
                // Y copies the inputs into the numbered Record Slot named by `label`.
                let action = match (&page.alternate, page.binds_slot) {
                    (Some(label), true) => format!("A Load from file, Y Copy to {label}, B closes."),
                    (Some(label), false) => format!("A loads, Y {label}, B closes."),
                    (None, _) => "A loads, B closes.".to_owned(),
                };
                (
                    page.title.clone(),
                    format!(
                        "{selected_label}\npage {}/{pages}  skipped {}    {action}",
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
        self.offer_save_with_mii(frames, start, false, version, None)
    }

    /// Same dialog as [`Self::offer_save`], retaining an exact Mii captured with the take.
    pub fn offer_save_with_mii(
        &mut self,
        mut frames: Vec<InputFrame>,
        start: StartState,
        crop: bool,
        version: &str,
        mii: Option<training_mod_mii::lifetime::Configuration>,
    ) -> Effect {
        if crop {
            crop_neutral_tail(&mut frames);
        }
        if frames.is_empty() || start.fighter_kind < 0 {
            return Effect::Notify("Recording was empty".to_owned());
        }
        self.open_name(NameDraft {
            name: "Recording".to_owned(),
            native_cursor: "Recording".encode_utf16().count(),
            page: 0,
            cursor: 31,
            body: NameBody::Macro {
                frames,
                start,
                placement: Placement::default(),
                version: clip_version(version),
                mii,
            },
            keyboard_title: MACRO_KEYBOARD_TITLE,
        });
        Effect::None
    }

    /// Opens the keyboard for a caller-owned draft. No macro frames are invented.
    pub fn offer_name(&mut self, context: NameContext, default_name: &str, summary: &str) -> Effect {
        if default_name.chars().count() > MAX_NAME_CHARS || default_name.chars().any(char::is_control) {
            return Effect::Notify(MacroError::InvalidName("is too long").to_string());
        }
        if context.title.is_empty() || context.title.chars().any(char::is_control) {
            return Effect::Notify("Keyboard title is empty".to_owned());
        }
        self.open_name(NameDraft {
            name: default_name.to_owned(),
            native_cursor: default_name.encode_utf16().count(),
            page: 0,
            cursor: 31,
            body: NameBody::External {
                token: context.token,
                summary: summary.to_owned(),
                title: context.title,
            },
            keyboard_title: context.title,
        });
        Effect::None
    }

    fn open_name(&mut self, draft: NameDraft) {
        self.playback_on = false;
        self.held_save = None;
        self.screen = Screen::Save(Box::new(draft));
    }

    pub fn open_library(&mut self, rows: Vec<LibraryRow>, skipped: usize) {
        self.open_library_titled(rows, skipped, "Named Library", None);
    }

    /// `alternate` is the Record Slot label, for example "Record Slot 2".
    /// `None` leaves Y inert. Y emits [`Effect::BindToSlot`].
    pub fn open_library_titled(
        &mut self,
        rows: Vec<LibraryRow>,
        skipped: usize,
        title: impl Into<String>,
        alternate: Option<String>,
    ) {
        self.open_listed(rows, skipped, title.into(), alternate, true);
    }

    /// Same list as [`Self::open_library_titled`]. Y emits [`Effect::Alternate`]
    /// and the footer uses `alternate` as the action words after "Y".
    pub fn open_library_forward(
        &mut self,
        rows: Vec<LibraryRow>,
        skipped: usize,
        title: impl Into<String>,
        alternate: Option<String>,
    ) {
        self.open_listed(rows, skipped, title.into(), alternate, false);
    }

    fn open_listed(
        &mut self,
        rows: Vec<LibraryRow>,
        skipped: usize,
        title: String,
        alternate: Option<String>,
        binds_slot: bool,
    ) {
        self.playback_on = false;
        self.picked = None;
        self.screen = Screen::Library(Box::new(LibraryPage {
            rows,
            skipped,
            cursor: 0,
            title,
            alternate,
            binds_slot,
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
        if matches!(self.screen, Screen::Fighter(_)) {
            return self.fighter_pad(pad);
        }
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

    /// One character-load request per explicit load or retry. Readiness loss alone
    /// never re-arms it, so changing the CPU does not undo the user's choice.
    pub fn claim_character_request(&mut self) -> Option<i32> {
        if self.claimed || self.character_ready {
            return None;
        }
        let kind = self.pending_kind?;
        self.claimed = true;
        Some(kind)
    }

    pub fn observe_cpu(&mut self, kind: Option<i32>, busy: bool) {
        self.observe_exact_cpu(kind, None, busy);
    }

    /// Configuration is supplied only after the runtime's exact selection ack.
    pub fn observe_exact_cpu(&mut self, kind: Option<i32>, configuration: Option<training_mod_mii::lifetime::Configuration>, busy: bool) {
        if self.plays_current() {
            // Only the recorded-identity match is bypassed: the CPU must still
            // exist and be settled (not mid-install) before inputs play.
            self.character_ready = !busy && kind.is_some();
            self.pending_kind = None;
            self.claimed = false;
            return;
        }
        let target = self.loaded_target();
        self.character_ready = !busy && target.is_some_and(|target|
            kind.is_some_and(|kind| target.ready(kind as u32, configuration)));
        if self.character_ready {
            self.pending_kind = None;
            self.claimed = false;
            if let Some(loaded) = self.loaded.as_mut() {
                if loaded.plays == Plays::SwitchOnce { loaded.plays = Plays::Current; }
            }
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
            native_cursor: recording.name.encode_utf16().count(),
            page: 0,
            cursor: 31,
            body: NameBody::Macro {
                frames: recording.frames.clone(),
                start: recording.start,
                placement: recording.placement,
                version: recording.game_version.clone(),
                mii: recording.mii,
            },
            keyboard_title: MACRO_KEYBOARD_TITLE,
        }));
        self.playback_on = false;
    }

    pub fn close_transient(&mut self) {
        self.screen = Screen::Closed;
        self.picked = None;
        self.prompt_commit = None;
        self.held_save = None;
        self.clear_anchor();
    }

    /// Keep exact unfinished panels and draft values across a waiting match, but
    /// invalidate the old stage anchor and all playback/character acknowledgments.
    /// A fighter prompt is about the old stage's CPU: it is retired, not parked.
    pub fn invalidate_practice_context(&mut self) {
        self.clear_anchor();
        self.drop_fighter_prompt();
        self.picked = None;
        self.character_ready = false;
        self.claimed = false;
    }

    fn clear_anchor(&mut self) {
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
        self.loaded = Some(Loaded { id, recording, plays: Plays::Recorded });
        self.screen = Screen::Closed;
        self.picked = None;
        self.held_save = None;
        self.playback_on = false;
        self.playback_frame = 0;
        self.playback_len = 0;
        let target = self.loaded_target();
        if target.is_none() {
            self.character_ready = false;
            self.pending_kind = None;
            self.claimed = false;
            return Ok(Effect::Notify("This macro has no exact Mii configuration".into()));
        }
        if cpu_kind == Some(kind) && target.is_some_and(|target| target.configuration().is_none()) {
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

    /// Load a recording the user explicitly selected. The fighter gate decides
    /// whether it commits at once (same fighter, or Keep), or waits for a choice.
    /// Nothing is committed, and the previous macro stays loaded, until then.
    pub fn offer_load(
        &mut self,
        id: String,
        recording: MacroRecording,
        context: LoadContext,
    ) -> Result<Effect, MacroError> {
        recording.validate()?;
        match context.gate(&recording) {
            FighterGate::Proceed => Ok(self.install_current(Some(id), recording, context.cpu_kind)),
            FighterGate::Keep => {
                let legacy = recording.playback_target().is_err() && !context.matches;
                let notice = legacy.then(|| {
                    Effect::Notify("Older Mii macro: playing on the current fighter".into())
                });
                let effect = self.install_current(Some(id), recording, context.cpu_kind);
                Ok(notice.unwrap_or(effect))
            }
            FighterGate::Prompt => {
                let prior = self.picked.take();
                self.playback_on = false;
                self.held_save = None;
                self.screen = Screen::Fighter(Box::new(FighterScreen {
                    prompt: fighter::Prompt::new(
                        FighterOrigin::Named,
                        context.revision,
                        context.recorded_label,
                        context.current_label,
                    ),
                    cpu_kind: context.cpu_kind,
                    candidate: Some((id, recording)),
                    prior,
                }));
                Ok(Effect::None)
            }
        }
    }

    /// Prompt for a replay clip or track. The clip editor keeps the candidate and
    /// receives [`Effect::ClipFighter`]. Refused while another panel is open.
    pub fn open_fighter_prompt(
        &mut self,
        revision: u64,
        recorded_label: String,
        current_label: String,
    ) -> bool {
        if !matches!(self.screen, Screen::Closed) {
            return false;
        }
        self.playback_on = false;
        self.screen = Screen::Fighter(Box::new(FighterScreen {
            prompt: fighter::Prompt::new(FighterOrigin::Clip, revision, recorded_label, current_label),
            cpu_kind: None,
            candidate: None,
            prior: None,
        }));
        true
    }

    /// The library load a fighter prompt just committed, once. The runtime then
    /// remembers it as the selected macro; a Cancelled or stale prompt yields none.
    pub fn take_prompt_commit(&mut self) -> Option<(String, String)> {
        self.prompt_commit.take()
    }

    /// Origin and selection revision of the prompt on screen, if any.
    pub fn fighter_prompt(&self) -> Option<(FighterOrigin, u64)> {
        match &self.screen {
            Screen::Fighter(screen) => Some((screen.prompt.origin(), screen.prompt.revision())),
            _ => None,
        }
    }

    /// Retire a prompt whose selection or scene ended. Never applies a choice and
    /// never changes the loaded macro.
    pub fn drop_fighter_prompt(&mut self) -> bool {
        if !matches!(self.screen, Screen::Fighter(_)) {
            return false;
        }
        self.screen = Screen::Closed;
        true
    }

    /// Plays on the settled CPU, whatever it is. No character request is created,
    /// and the file's recorded fighter and Mii are untouched.
    fn install_current(
        &mut self,
        id: Option<String>,
        recording: MacroRecording,
        cpu_kind: Option<i32>,
    ) -> Effect {
        self.loaded = Some(Loaded { id, recording, plays: Plays::Current });
        self.screen = Screen::Closed;
        self.picked = None;
        self.held_save = None;
        self.playback_on = false;
        self.playback_frame = 0;
        self.playback_len = 0;
        self.character_ready = cpu_kind.is_some();
        self.pending_kind = None;
        self.claimed = false;
        Effect::None
    }

    /// True when the loaded macro plays on whatever CPU is settled, so the
    /// runtime has no recorded identity to wait for.
    pub fn plays_current(&self) -> bool {
        self.loaded.as_ref().is_some_and(|loaded| loaded.plays == Plays::Current)
    }

    fn fighter_pad(&mut self, pad: Pad) -> Effect {
        let Screen::Fighter(screen) = &mut self.screen else { return Effect::None };
        match screen.prompt.on_pad(pad) {
            fighter::Outcome::None => Effect::None,
            fighter::Outcome::Cancelled => self.cancel_fighter(),
            fighter::Outcome::Chose(choice) => self.choose_fighter(choice),
        }
    }

    fn cancel_fighter(&mut self) -> Effect {
        let Screen::Fighter(screen) = std::mem::replace(&mut self.screen, Screen::Closed) else {
            return Effect::None;
        };
        match screen.prompt.origin() {
            FighterOrigin::Clip => Effect::ClipFighter(None),
            FighterOrigin::Named => {
                // Back to the list the user picked from; the loaded macro never changed.
                if let Some(page) = screen.prior {
                    self.screen = Screen::Library(page);
                }
                Effect::None
            }
        }
    }

    fn choose_fighter(&mut self, choice: FighterChoice) -> Effect {
        let Screen::Fighter(screen) = std::mem::replace(&mut self.screen, Screen::Closed) else {
            return Effect::None;
        };
        let FighterScreen { prompt, cpu_kind, candidate, .. } = *screen;
        let Some((id, recording)) = candidate.filter(|_| prompt.origin() == FighterOrigin::Named) else {
            return Effect::ClipFighter(Some(choice));
        };
        self.prompt_commit = Some((id.clone(), recording.name.clone()));
        match choice {
            FighterChoice::Keep => self.install_current(Some(id), recording, cpu_kind),
            // The existing exact-fighter route. The CPU differs by construction, so
            // pass no live kind and let install request the recorded target.
            FighterChoice::Switch => {
                let effect = self.install(Some(id), recording, None)
                    .unwrap_or_else(|error| Effect::Notify(error.to_string()));
                if let Some(loaded) = self.loaded.as_mut() { loaded.plays = Plays::SwitchOnce; }
                effect
            },
        }
    }

    fn shift(&mut self, d_row: i32, d_col: i32) -> Effect {
        match &mut self.screen {
            Screen::Save(draft) => draft.cursor = nudge(draft.cursor, d_row, d_col),
            Screen::Place(draft) => draft.cursor = nudge(draft.cursor, d_row, d_col),
            Screen::Library(page) => {
                page.cursor = move_library(page.cursor, page.rows.len(), d_row, d_col);
            }
            Screen::Closed | Screen::Fighter(_) => {}
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
                let forward = !page.binds_slot;
                match id {
                    Some(id) => {
                        let screen = std::mem::replace(&mut self.screen, Screen::Closed);
                        if forward {
                            Effect::ForwardChoose { id }
                        } else {
                            // Keep the list for a Cancelled fighter prompt.
                            if let Screen::Library(page) = screen {
                                self.picked = Some(page);
                            }
                            Effect::Choose(id)
                        }
                    }
                    None => Effect::Notify("No recordings saved yet".to_owned()),
                }
            }
            Screen::Closed | Screen::Fighter(_) => Effect::None,
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
                    draft.native_cursor = 0;
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
        draft.native_cursor = draft.name.encode_utf16().count();
    }

    fn pop_char(&mut self) {
        let Screen::Save(draft) = &mut self.screen else {
            return;
        };
        draft.name.pop();
        draft.native_cursor = draft.name.encode_utf16().count();
    }

    fn extra(&mut self) -> Effect {
        if matches!(self.screen, Screen::Save(_)) {
            self.pop_char();
            return Effect::None;
        }
        let Screen::Library(page) = &self.screen else { return Effect::None };
        if page.alternate.is_none() {
            return Effect::None;
        }
        let binds_slot = page.binds_slot;
        let id = page.rows.get(page.cursor).map(|row| row.id.clone());
        match id {
            Some(id) => {
                self.screen = Screen::Closed;
                if binds_slot {
                    Effect::BindToSlot { id }
                } else {
                    Effect::Alternate { id }
                }
            }
            None => Effect::Notify("No recordings saved yet".to_owned()),
        }
    }

    fn toggle_draft_momentum(&mut self) {
        if let Screen::Save(draft) = &mut self.screen {
            if let NameBody::Macro { placement, .. } = &mut draft.body {
                placement.momentum = flip(placement.momentum);
            }
        }
    }

    fn open_place_from_save(&mut self) -> Effect {
        if matches!(&self.screen, Screen::Save(draft) if matches!(draft.body, NameBody::External { .. })) {
            return Effect::None;
        }
        if self.anchor.is_none() {
            return Effect::Notify("Restore a Training state before editing placement".into());
        }
        let screen = std::mem::replace(&mut self.screen, Screen::Closed);
        let Screen::Save(draft) = screen else {
            self.screen = screen;
            return Effect::None;
        };
        let NameBody::Macro { placement, .. } = &draft.body else {
            self.screen = Screen::Save(draft);
            return Effect::None;
        };
        let working = *placement;
        self.held_save = Some(draft);
        self.screen = Screen::Place(Box::new(PlaceDraft {
            working,
            cursor: 1,
            from_save: true,
        }));
        Effect::None
    }

    fn finish_save(&mut self) -> Effect {
        let external = match &self.screen {
            Screen::Save(draft) => match draft.body {
                NameBody::External { token, title, .. } => Some((token, title, draft.name.clone())),
                NameBody::Macro { .. } => None,
            },
            _ => return Effect::None,
        };
        if let Some((token, title, raw)) = external {
            let name = raw.trim();
            if let Err(err) = external_name(name) {
                return Effect::Notify(err.to_string());
            }
            let name = name.to_owned();
            self.screen = Screen::Closed;
            return Effect::NameConfirmed { context: NameContext { token, title }, name };
        }
        let (name, version, start, frames, placement, mii) = match &self.screen {
            Screen::Save(draft) => match &draft.body {
                NameBody::Macro { frames, start, placement, version, mii } => (
                    draft.name.clone(),
                    version.clone(),
                    *start,
                    frames.clone(),
                    *placement,
                    *mii,
                ),
                NameBody::External { .. } => return Effect::None,
            },
            _ => return Effect::None,
        };
        let built = MacroRecording::new(name.trim(), version, start, frames);
        let mut recording = match built {
            Ok(recording) => recording,
            Err(err) => return Effect::Notify(err.to_string()),
        };
        if let Some(configuration) = mii {
            if let Err(err) = recording.set_mii(configuration) { return Effect::Notify(err.to_string()); }
        }
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
            native_cursor: source.recording.name.encode_utf16().count(),
            page: 0,
            cursor: 31,
            body: NameBody::Macro {
                frames: source.recording.frames.clone(),
                start: source.recording.start,
                placement,
                version: source.recording.game_version.clone(),
                mii: source.recording.mii,
            },
            keyboard_title: MACRO_KEYBOARD_TITLE,
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
            Screen::Closed | Screen::Fighter(_) => Effect::None,
        }
    }

    fn restore_save_from_place(&mut self, placement: Placement) -> Effect {
        let Some(mut held) = self.held_save.take() else {
            self.screen = Screen::Closed;
            return Effect::None;
        };
        if let NameBody::Macro { placement: saved, .. } = &mut held.body {
            *saved = placement;
        }
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

fn external_name(name: &str) -> Result<(), MacroError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(MacroError::InvalidName("is empty"));
    }
    if name.chars().any(char::is_control) {
        return Err(MacroError::InvalidName("contains control characters"));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(MacroError::InvalidName("is too long"));
    }
    Ok(())
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
    fn waiting_interruption_preserves_unfinished_name_and_explicit_cancel() {
        let mut ui = MacroUi::new();
        let original = sample(Placement { delta_x: 7.0, delta_y: -2.0, ..Placement::default() });
        ui.restore_draft(original.clone());
        ui.on_pad(Pad::Left);
        let before = ui.view();
        ui.invalidate_practice_context();
        assert_eq!(ui.view(), before);
        assert_eq!(ui.name_prompt(), Some("Drill"));
        assert!(!ui.playback_allowed());
        let Effect::Save(saved) = ui.confirm_native_name("Returned drill".into()) else { panic!("explicit save expected"); };
        assert_eq!(saved.frames, original.frames);
        assert_eq!(saved.placement, original.placement);
        ui.restore_draft(*saved);
        ui.on_pad(Pad::Back);
        ui.invalidate_practice_context();
        assert!(!ui.is_open(), "a cancelled draft must not reappear on return");
    }

    #[test]
    fn native_checkpoint_survives_repeated_interruption_without_commit() {
        let mut ui = MacroUi::new();
        let mut original = sample(Placement { delta_x: 7.0, delta_y: -2.0, ..Placement::default() });
        original.start = StartState::neutral(0x48, 0, 1.0);
        original.set_mii(exact_mii()).unwrap();
        ui.restore_draft(original.clone());
        ui.on_pad(Pad::Left);
        let grid_selection = ui.view().unwrap().selected;
        for (text, caret) in [("😀 middle", 2), ("still unfinished", 6)] {
            assert!(ui.checkpoint_native_name(text, caret));
            ui.invalidate_practice_context();
            ui.invalidate_practice_context();
            assert_eq!(ui.name_prompt(), Some(text));
            assert_eq!(ui.native_name_cursor(), Some(caret));
            assert_eq!(ui.view().unwrap().selected, grid_selection);
            assert!(ui.loaded_id().is_none());
            assert!(!ui.playback_allowed());
        }
        let Effect::Save(saved) = ui.confirm_native_name(ui.name_prompt().unwrap().to_owned()) else {
            panic!("only explicit confirm may save");
        };
        assert_eq!(saved.name, "still unfinished");
        assert_eq!(saved.frames, original.frames);
        assert_eq!(saved.start, original.start);
        assert_eq!(saved.placement, original.placement);
        assert_eq!(saved.mii, original.mii);
    }

    #[test]
    fn native_checkpoint_rejects_invalid_values_atomically_and_keeps_empty_drafts() {
        let mut ui = MacroUi::new();
        ui.restore_draft(sample(Placement::default()));
        let full = "😀".repeat(48); // 48 scalars, 96 UTF-16 units.
        assert!(ui.checkpoint_native_name(&full, 2));
        for (bad, caret) in [("a".repeat(49), 0), ("x\0y".into(), 0),
            ("x\ny".into(), 0), ("😀".into(), 1), ("a".into(), 2)] {
            assert!(!ui.checkpoint_native_name(&bad, caret));
            assert_eq!(ui.name_prompt(), Some(full.as_str()));
            assert_eq!(ui.native_name_cursor(), Some(2));
        }
        assert!(ui.checkpoint_native_name("", 0));
        ui.invalidate_practice_context();
        assert_eq!(ui.name_prompt(), Some(""));
        assert!(matches!(ui.confirm_native_name("".into()), Effect::Notify(_)));
        assert_eq!(ui.name_prompt(), Some(""), "invalid confirmation cannot discard a draft");
        assert!(matches!(ui.on_pad(Pad::Back), Effect::None));
        ui.invalidate_practice_context();
        assert_eq!(ui.name_prompt(), None);
        assert_eq!(ui.native_name_cursor(), None);
        assert!(!ui.checkpoint_native_name("late callback", 0));
        assert!(matches!(ui.confirm_native_name("late enter".into()), Effect::None));
    }

    #[test]
    fn native_caret_survives_placement_but_not_new_draft_or_discard() {
        let mut ui = MacroUi::new();
        ui.restore_draft(sample(Placement::default()));
        ui.restored_target(1, (4.0, 8.0));
        assert!(ui.checkpoint_native_name("custom name", 3));
        ui.native_name_placement();
        assert!(!ui.checkpoint_native_name("wrong screen", 0));
        ui.on_pad(Pad::Back);
        assert_eq!(ui.name_prompt(), Some("custom name"));
        assert_eq!(ui.native_name_cursor(), Some(3));
        ui.close_transient(); // A new lobby discards all unfinished naming state.
        assert_eq!(ui.native_name_cursor(), None);
        ui.offer_save(frames(), start(), false, "13.0.5");
        assert_eq!(ui.name_prompt(), Some("Recording"));
        assert_eq!(ui.native_name_cursor(), Some(9));
    }

    #[test]
    fn waiting_interruption_keeps_placement_edits_but_drops_old_stage_anchor() {
        let mut ui = MacroUi::new();
        ui.commit_loaded("drill".into(), sample(Placement::default()), Some(0x08)).unwrap();
        assert!(ui.open_editor(Some((4.0, 8.0))));
        ui.on_pad(Pad::Confirm); // X +1, still an uncommitted draft.
        goto(&mut ui, 9);
        let before = ui.view();
        ui.invalidate_practice_context();
        assert_eq!(ui.view(), before);
        assert_eq!(ui.marker(), None);
        assert_eq!(ui.loaded_placement().unwrap().delta_x, 0.0);
        assert!(matches!(ui.on_pad(Pad::Confirm), Effect::Notify(_)));
        assert_eq!(ui.loaded_placement().unwrap().delta_x, 1.0);
        assert!(!ui.playback_allowed());
        ui.observe_cpu(Some(0x08), false);
        assert!(ui.playback_allowed());
        ui.restored_target(2, (12.0, 6.0));
        assert_eq!(ui.restored_target(2, (99.0, 99.0)), Some((13.0, 6.0)));
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
            mii: None,
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

    fn exact_mii() -> training_mod_mii::lifetime::Configuration {
        use training_mod_mii::inventory::{classify, MiiClass, RawSlot, TaggedId, CHARINFO_LEN, TAG_CHARA, TAG_HAT, TAG_OUTFIT};
        let saved = classify(&RawSlot {
            index: 0, slot_id: 13231, name_len: 1,
            name_units: [65, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            charinfo: [7; CHARINFO_LEN], colour: 4, voice: 37, specials: [1, 0, 0, 0],
            tag_outfit: TaggedId((u64::from(TAG_OUTFIT) << 56) | (1 << 40) | 0x1b_fef7_a5f7),
            tag_hat: TaggedId((u64::from(TAG_HAT) << 56) | 0x0f_a9ef_a5ea),
            tag_chara: TaggedId((u64::from(TAG_CHARA) << 56) | MiiClass::Brawler.chara_hash40()),
        }).unwrap();
        training_mod_mii::lifetime::Configuration::from_saved(&saved).unwrap()
    }

    #[test]
    fn exact_mii_naming_save_as_and_same_class_ack_keep_identity() {
        let configuration = exact_mii();
        let mut original = MacroRecording::new("Captured", "13.0.5", StartState::neutral(0x48, 0, 1.0), frames()).unwrap();
        original.set_mii(configuration).unwrap();
        let mut ui = MacroUi::new();
        ui.restore_draft(original.clone());
        let Effect::Save(named) = ui.confirm_native_name("Named Brawler".into()) else { panic!() };
        assert_eq!(named.mii, Some(configuration));
        ui.restore_draft((*named).clone()); // Failed filesystem save returns the same owned draft.
        let Effect::Save(retried) = ui.confirm_native_name("Retried Brawler".into()) else { panic!() };
        assert_eq!(retried.mii, Some(configuration));
        assert!(matches!(ui.commit_loaded("brawler".into(), *retried, Some(0x48)).unwrap(), Effect::NeedCharacter(0x48)));
        assert!(!ui.playback_allowed());
        assert_eq!(ui.claim_character_request(), Some(0x48));
        ui.observe_cpu(Some(0x48), false);
        assert!(!ui.playback_allowed());
        let mut wrong = serde_json::to_value(configuration).unwrap();
        wrong["charinfo"][0] = serde_json::json!(9); // Same canonical identity, different exact bytes.
        let wrong = serde_json::from_value(wrong).unwrap();
        ui.observe_exact_cpu(Some(0x48), Some(wrong), false);
        assert!(!ui.playback_allowed());
        ui.observe_exact_cpu(Some(0x48), Some(configuration), true);
        assert!(!ui.playback_allowed());
        ui.observe_exact_cpu(Some(0x48), Some(configuration), false);
        assert!(ui.playback_allowed());
        assert!(ui.open_editor(Some((0.0, 0.0))));
        ui.save_as();
        let Effect::Save(copy) = ui.confirm_native_name("Copy Brawler".into()) else { panic!() };
        assert_eq!(copy.mii, Some(configuration));
        assert_eq!(copy.schema_version, 2);
        assert_eq!(copy.frames, original.frames);
        ui.invalidate_character_request();
        assert!(!ui.playback_allowed());
        assert_eq!(ui.claim_character_request(), Some(0x48));
    }
    #[test]
    fn loaded_macro_does_not_undo_manual_cpu_choice() {
        let mut ui = MacroUi::new();
        ui.commit_loaded("ordinary".into(), sample(Placement::default()), Some(0x08)).unwrap();
        assert!(ui.playback_allowed());
        ui.observe_cpu(Some(0x08), true);
        assert_eq!(ui.claim_character_request(), None);
        ui.observe_cpu(Some(0x0f), false);
        assert!(!ui.playback_allowed());
        assert_eq!(ui.claim_character_request(), None);
        ui.observe_cpu(Some(0x08), false);
        assert!(ui.playback_allowed());
        assert_eq!(ui.claim_character_request(), None);
        ui.invalidate_character_request();
        assert_eq!(ui.claim_character_request(), Some(0x08));

        let configuration = exact_mii();
        let mut recording = MacroRecording::new("Exact", "13.0.5", StartState::neutral(0x48, 0, 1.0), frames()).unwrap();
        recording.set_mii(configuration).unwrap();
        ui.commit_loaded("exact".into(), recording, Some(0x48)).unwrap();
        assert_eq!(ui.claim_character_request(), Some(0x48));
        ui.observe_exact_cpu(Some(0x48), Some(configuration), false);
        assert!(ui.playback_allowed());
        ui.observe_cpu(Some(0x0f), false);
        assert_eq!(ui.claim_character_request(), None);
        let mut wrong = serde_json::to_value(configuration).unwrap();
        wrong["charinfo"][0] = serde_json::json!(9);
        let wrong = serde_json::from_value(wrong).unwrap();
        ui.observe_cpu(Some(0x48), false);
        ui.observe_exact_cpu(Some(0x48), Some(wrong), false);
        assert_eq!(ui.claim_character_request(), None);
        assert!(!ui.playback_allowed());
        // Runtime can renew an exact current receipt without a UI install request.
        assert_eq!(ui.claim_character_request(), None);
        ui.observe_exact_cpu(Some(0x48), Some(configuration), false);
        assert!(ui.playback_allowed());
        assert_eq!(ui.claim_character_request(), None);
    }

    #[test]
    fn numbered_slot_copy_rejects_over_limit_without_truncation() {
        let mut over = vec![InputFrame::default(); NUMBERED_SLOT_FRAMES + 1];
        over[0].buttons = 1;
        let rejected = copy_numbered_frames(&over);
        assert_eq!(rejected, Err(SlotAdmit::OverLimit { frames: 601, limit: NUMBERED_SLOT_FRAMES }));
        assert_eq!(over.len(), 601);
        assert_eq!(over[0].buttons, 1);
        assert_eq!(copy_numbered_frames(&[]), Err(SlotAdmit::Empty));
        let full = vec![InputFrame::default(); NUMBERED_SLOT_FRAMES];
        let copied = copy_numbered_frames(&full).unwrap();
        assert_eq!(copied.len(), NUMBERED_SLOT_FRAMES);
        assert_eq!(copied, full);
    }

    #[test]
    fn slot_facing_parks_only_a_captured_slot() {
        let shared = (1.0, 7);
        let (active, parked) = engage_slot_origin(shared, None, Some((-1.0, 4)));
        assert_eq!(active, (-1.0, 4));
        assert_eq!(parked, Some(shared));
        let (restored, parked) = release_slot_origin(active, parked);
        assert_eq!(restored, shared);
        assert_eq!(parked, None);
        let (active, parked) = engage_slot_origin((-1.0, 4), Some(shared), Some((1.0, 9)));
        assert_eq!(active, (1.0, 9));
        assert_eq!(parked, Some(shared));
        let (active, parked) = engage_slot_origin(shared, None, None);
        assert_eq!(active, shared);
        assert_eq!(parked, None);
    }

    #[test]
    fn library_y_binds_only_when_the_slot_label_is_set() {
        let rows = vec![LibraryRow { id: "fox".into(), label: "Fox drill".into() }];
        let mut plain = MacroUi::new();
        plain.open_library(rows.clone(), 0);
        assert_eq!(plain.view().unwrap().title, "Named Library");
        assert!(plain.view().unwrap().footer.contains("A loads, B closes."));
        assert!(matches!(plain.on_pad(Pad::Extra), Effect::None));
        assert!(plain.is_open());

        let mut labeled = MacroUi::new();
        labeled.open_library_titled(rows, 0, "Named Library", Some("Record Slot 3".into()));
        let footer = labeled.view().unwrap().footer;
        // A reads the file for playback (it does not start it); Y copies to the slot.
        assert!(footer.contains("A Load from file, Y Copy to Record Slot 3"), "{footer}");
        assert!(!footer.contains("binds"), "{footer}");
        let Effect::BindToSlot { id } = labeled.on_pad(Pad::Extra) else { panic!("expected bind") };
        assert_eq!(id, "fox");
        assert!(!labeled.is_open());
    }

    fn context(cpu_kind: i32, matches: bool, prompt_enabled: bool) -> LoadContext {
        LoadContext {
            cpu_kind: Some(cpu_kind),
            matches,
            prompt_enabled,
            can_switch: true,
            revision: 5,
            recorded_label: "Kirby".into(),
            current_label: "Hero".into(),
        }
    }

    /// The library A press the runtime sees before it reads the file.
    fn pick(ui: &mut MacroUi) {
        let rows = ["id0", "id1"].map(|id| LibraryRow { id: id.into(), label: id.into() });
        ui.open_library_titled(rows.to_vec(), 0, "Named Library", Some("Record Slot 1".into()));
        ui.on_pad(Pad::Right);
        let Effect::Choose(id) = ui.on_pad(Pad::Confirm) else { panic!("expected choose") };
        assert_eq!(id, "id1");
    }

    #[test]
    fn mismatch_prompts_and_keep_plays_the_same_inputs_on_the_current_cpu() {
        let mut ui = MacroUi::new();
        ui.commit_loaded("kept".into(), sample(Placement::default()), Some(0x08)).unwrap();
        let mut candidate = sample(Placement::default());
        candidate.frames.push(InputFrame { buttons: 0x4, ..InputFrame::default() });
        let original = candidate.clone();
        pick(&mut ui);
        // Recorded fighter 0x08 against a CPU that is 0x12.
        let effect = ui.offer_load("id1".into(), candidate, context(0x12, false, true)).unwrap();
        assert!(matches!(effect, Effect::None));
        assert_eq!(ui.fighter_prompt(), Some((FighterOrigin::Named, 5)));
        // Nothing is committed, and nothing plays, while the question is open.
        assert_eq!(ui.loaded_id(), Some("kept"));
        assert!(!ui.playback_allowed());
        assert_eq!(ui.claim_character_request(), None);
        let view = ui.view().unwrap();
        assert_eq!((view.labels[0].as_str(), view.labels[1].as_str()),
            ("Keep current fighter", "Switch to recorded fighter"));
        assert_eq!(view.selected, 0, "Keep is the safe default");

        assert_eq!(ui.take_prompt_commit(), None, "nothing is remembered before the answer");
        assert!(matches!(ui.on_pad(Pad::Confirm), Effect::None));
        assert_eq!(ui.fighter_prompt(), None);
        assert_eq!(ui.take_prompt_commit(), Some(("id1".to_owned(), "Drill".to_owned())));
        assert_eq!(ui.take_prompt_commit(), None);
        assert_eq!(ui.loaded_id(), Some("id1"));
        assert!(ui.plays_current());
        assert_eq!(ui.claim_character_request(), None, "Keep must not schedule a CPU reload");
        // The inputs and recorded identity are the file's, untouched.
        assert_eq!(ui.loaded_frames(), Some(original.frames.as_slice()));
        assert_eq!(ui.loaded_start(), Some(original.start));
        assert_eq!(ui.loaded_target().unwrap().kind(), 0x08);
        // They play on whichever CPU is settled, but never mid-install.
        ui.observe_cpu(Some(0x12), true);
        assert!(!ui.playback_allowed());
        ui.observe_cpu(Some(0x12), false);
        assert!(ui.playback_allowed());
        ui.observe_cpu(Some(0x0f), false);
        assert!(ui.playback_allowed());
        assert!(ui.arm_playback());
        assert_eq!(ui.feed_playback(1), Some(original.frames[0]));
        assert_eq!(ui.feed_playback(2), Some(original.frames[1]));
        assert_eq!(ui.claim_character_request(), None);
    }

    #[test]
    fn switch_uses_the_existing_exact_fighter_request_and_keeps_the_inputs() {
        let mut ui = MacroUi::new();
        let candidate = sample(Placement::default());
        pick(&mut ui);
        ui.offer_load("id1".into(), candidate.clone(), context(0x12, false, true)).unwrap();
        ui.on_pad(Pad::Down);
        assert_eq!(ui.view().unwrap().selected, 1);
        assert!(matches!(ui.on_pad(Pad::Confirm), Effect::NeedCharacter(0x08)));
        assert_eq!(ui.loaded_id(), Some("id1"));
        assert!(!ui.plays_current());
        assert!(!ui.playback_allowed());
        assert_eq!(ui.claim_character_request(), Some(0x08));
        ui.observe_cpu(Some(0x12), false);
        assert!(!ui.playback_allowed(), "Switch still waits for the recorded fighter");
        ui.observe_cpu(Some(0x08), false);
        assert!(ui.playback_allowed());
        assert!(ui.plays_current(), "an explicit Switch is completed once");
        ui.observe_cpu(Some(0x12), false);
        assert!(ui.playback_allowed(), "a later manual swap stays the user's choice");
        assert_eq!(ui.claim_character_request(), None);
        assert_eq!(ui.loaded_frames(), Some(candidate.frames.as_slice()));
    }

    #[test]
    fn cancel_returns_to_the_list_and_changes_nothing() {
        let mut ui = MacroUi::new();
        ui.commit_loaded("kept".into(), sample(Placement::default()), Some(0x08)).unwrap();
        pick(&mut ui);
        ui.offer_load("id1".into(), sample(Placement::default()), context(0x12, false, true)).unwrap();
        assert!(matches!(ui.on_pad(Pad::Back), Effect::None));
        assert_eq!(ui.fighter_prompt(), None);
        assert_eq!(ui.take_prompt_commit(), None, "a cancelled load is never remembered");
        let view = ui.view().unwrap();
        assert_eq!((view.title.as_str(), view.selected), ("Named Library", 1));
        assert_eq!(ui.loaded_id(), Some("kept"));
        assert!(!ui.plays_current());
        assert_eq!(ui.claim_character_request(), None);
        assert!(matches!(ui.on_pad(Pad::Back), Effect::None));
        assert!(ui.playback_allowed(), "the previous macro is still ready on its fighter");
    }

    #[test]
    fn same_fighter_never_prompts_and_a_disabled_prompt_keeps_the_cpu_silently() {
        for enabled in [true, false] {
            let mut ui = MacroUi::new();
            pick(&mut ui);
            let effect = ui.offer_load("id1".into(), sample(Placement::default()), context(0x08, true, enabled)).unwrap();
            assert!(matches!(effect, Effect::None));
            assert_eq!(ui.fighter_prompt(), None);
            assert!(ui.plays_current());
            assert!(ui.playback_allowed());
        }
        let mut ui = MacroUi::new();
        pick(&mut ui);
        let effect = ui.offer_load("id1".into(), sample(Placement::default()), context(0x12, false, false)).unwrap();
        assert!(matches!(effect, Effect::None), "no NeedCharacter and no prompt");
        assert_eq!(ui.fighter_prompt(), None);
        assert!(!ui.is_open());
        assert!(ui.plays_current());
        assert_eq!(ui.claim_character_request(), None);
        assert!(ui.playback_allowed());
    }

    #[test]
    fn already_matching_exact_mii_loads_without_a_prompt_or_reload() {
        let configuration = exact_mii();
        let mut recording = MacroRecording::new("Exact", "13.0.5", StartState::neutral(0x48, 0, 1.0), frames()).unwrap();
        recording.set_mii(configuration).unwrap();
        let mut ui = MacroUi::new();
        let effect = ui.offer_load("same".into(), recording.clone(), context(0x48, true, true)).unwrap();
        assert!(matches!(effect, Effect::None));
        assert_eq!(ui.fighter_prompt(), None);
        assert_eq!(ui.claim_character_request(), None);
        assert!(ui.playback_allowed());
        assert_eq!(ui.loaded_target().unwrap().configuration(), Some(configuration));
        assert_eq!(ui.loaded_frames(), Some(recording.frames.as_slice()));
    }

    #[test]
    fn same_class_with_a_different_exact_mii_is_a_mismatch_and_keep_ignores_the_exact_mii() {
        let configuration = exact_mii();
        let mut recording = MacroRecording::new("Exact", "13.0.5", StartState::neutral(0x48, 0, 1.0), frames()).unwrap();
        recording.set_mii(configuration).unwrap();
        let mut ui = MacroUi::new();
        pick(&mut ui);
        // The CPU is the same Brawler class; the runtime's exact predicate says it is another Mii.
        ui.offer_load("id1".into(), recording, context(0x48, false, true)).unwrap();
        assert!(ui.fighter_prompt().is_some());
        ui.on_pad(Pad::Confirm);
        assert!(ui.plays_current());
        let mut other = serde_json::to_value(configuration).unwrap();
        other["charinfo"][0] = serde_json::json!(9);
        let other = serde_json::from_value(other).unwrap();
        ui.observe_exact_cpu(Some(0x48), Some(other), false);
        assert!(ui.playback_allowed());
        // The file keeps its exact Mii. Keep neither rewrites nor reapplies it.
        assert_eq!(ui.loaded_target().unwrap().configuration(), Some(configuration));
        assert_eq!(ui.claim_character_request(), None);
    }

    #[test]
    fn an_older_mii_file_without_its_exact_mii_plays_on_the_current_cpu_without_a_prompt() {
        let legacy = MacroRecording::new("Old", "13.0.5", StartState::neutral(0x48, 0, 1.0), frames()).unwrap();
        assert!(legacy.playback_target().is_err());
        let mut ui = MacroUi::new();
        let effect = ui.offer_load("old".into(), legacy, context(0x08, false, true)).unwrap();
        assert!(matches!(effect, Effect::Notify(_)));
        assert_eq!(ui.fighter_prompt(), None);
        assert!(ui.plays_current());
        assert_eq!(ui.loaded_id(), Some("old"));
        assert!(ui.playback_allowed());
        assert_eq!(ui.claim_character_request(), None);
    }

    #[test]
    fn a_build_that_cannot_load_the_recorded_identity_keeps_the_cpu_without_asking() {
        let mut ui = MacroUi::new();
        pick(&mut ui);
        let context = LoadContext { can_switch: false, ..context(0x12, false, true) };
        let effect = ui.offer_load("id1".into(), sample(Placement::default()), context).unwrap();
        assert!(matches!(effect, Effect::None), "nothing to switch to, and not an older-file notice");
        assert_eq!(ui.fighter_prompt(), None);
        assert!(ui.plays_current());
        assert_eq!(ui.claim_character_request(), None);
        assert!(ui.playback_allowed());
    }

    #[test]
    fn a_stale_or_departed_prompt_never_commits_and_leaves_the_previous_macro() {
        let mut ui = MacroUi::new();
        ui.commit_loaded("kept".into(), sample(Placement::default()), Some(0x08)).unwrap();
        pick(&mut ui);
        ui.offer_load("id1".into(), sample(Placement::default()), context(0x12, false, true)).unwrap();
        ui.invalidate_practice_context(); // The waiting scene changed.
        assert_eq!(ui.fighter_prompt(), None);
        assert!(!ui.is_open());
        assert!(matches!(ui.on_pad(Pad::Confirm), Effect::None));
        assert_eq!(ui.loaded_id(), Some("kept"));

        // The runtime retires a prompt whose selection revision moved on.
        pick(&mut ui);
        ui.offer_load("id1".into(), sample(Placement::default()), context(0x12, false, true)).unwrap();
        assert!(ui.drop_fighter_prompt());
        assert!(!ui.drop_fighter_prompt());
        assert_eq!(ui.loaded_id(), Some("kept"));
        assert!(!ui.is_open());
    }

    #[test]
    fn a_clip_prompt_reports_its_choice_and_leaves_the_candidate_to_the_editor() {
        let mut ui = MacroUi::new();
        assert!(ui.open_fighter_prompt(3, "Kirby".into(), "Hero".into()));
        assert_eq!(ui.fighter_prompt(), Some((FighterOrigin::Clip, 3)));
        assert!(!ui.open_fighter_prompt(4, "Fox".into(), "Hero".into()), "one prompt at a time");
        assert!(matches!(ui.on_pad(Pad::Back), Effect::ClipFighter(None)));
        assert!(!ui.is_open());
        ui.open_fighter_prompt(3, "Kirby".into(), "Hero".into());
        ui.on_pad(Pad::Down);
        assert!(matches!(ui.on_pad(Pad::Confirm), Effect::ClipFighter(Some(FighterChoice::Switch))));
        ui.open_fighter_prompt(3, "Kirby".into(), "Hero".into());
        assert!(matches!(ui.on_pad(Pad::Confirm), Effect::ClipFighter(Some(FighterChoice::Keep))));
        assert_eq!(ui.loaded_id(), None);
    }

    #[test]
    fn external_name_confirms_without_a_macro_payload() {
        let mut ui = MacroUi::new();
        assert!(matches!(
            ui.offer_name(NameContext { token: 9, title: SETUP_KEYBOARD_TITLE }, "Setup", "Fox vs Marth"),
            Effect::None
        ));
        assert_eq!(ui.name_prompt(), Some("Setup"));
        assert_eq!(ui.native_name_title(), Some(SETUP_KEYBOARD_TITLE));
        assert!(ui.checkpoint_native_name("Slot draft", 4));
        ui.invalidate_practice_context();
        assert_eq!(ui.name_prompt(), Some("Slot draft"));
        assert_eq!(ui.native_name_title(), Some(SETUP_KEYBOARD_TITLE));
        assert_eq!(ui.native_name_cursor(), Some(4));
        assert!(matches!(ui.native_name_placement(), Effect::None));
        assert_eq!(ui.name_prompt(), Some("Slot draft"));
        assert!(matches!(ui.confirm_native_name("   ".into()), Effect::Notify(_)));
        assert_eq!(ui.name_prompt(), Some("   "));
        let Effect::NameConfirmed { context, name } = ui.confirm_native_name("  Morning setup  ".into()) else {
            panic!("expected name confirmation");
        };
        assert_eq!(context.token, 9);
        assert_eq!(context.title, SETUP_KEYBOARD_TITLE);
        assert_eq!(name, "Morning setup");
        assert!(ui.name_prompt().is_none());
        assert!(ui.native_name_title().is_none());
    }

    #[test]
    fn keyboard_title_stays_with_the_draft_kind() {
        let mut ui = MacroUi::new();
        let effect = ui.offer_save(frames(), start(), false, "13.0.5");
        assert!(matches!(effect, Effect::None));
        assert_eq!(ui.native_name_title(), Some(MACRO_KEYBOARD_TITLE));
        assert!(ui.checkpoint_native_name("Kept macro", 4));
        ui.invalidate_practice_context();
        assert_eq!(ui.native_name_title(), Some(MACRO_KEYBOARD_TITLE));
        assert_eq!(ui.name_prompt(), Some("Kept macro"));

        let effect = ui.offer_name(
            NameContext { token: 3, title: SETUP_KEYBOARD_TITLE },
            "Setup",
            "Slot 2",
        );
        assert!(matches!(effect, Effect::None));
        assert_eq!(ui.native_name_title(), Some(SETUP_KEYBOARD_TITLE));
        assert_ne!(ui.native_name_title(), Some(MACRO_KEYBOARD_TITLE));
    }

    #[test]
    fn mii_identity_keeps_exact_class_and_refuses_unknown() {
        use training_mod_mii::inventory::MiiClass;
        let classes = [MiiClass::Brawler, MiiClass::Swordfighter, MiiClass::Gunner];
        for class in classes {
            assert_eq!(mii_identity(class.kind() as i32, Some(class.kind())), MiiIdentity::Exact);
            assert_eq!(mii_identity(class.kind() as i32, None), MiiIdentity::Unknown);
        }
        assert_eq!(
            mii_identity(MiiClass::Brawler.kind() as i32, Some(MiiClass::Gunner.kind())),
            MiiIdentity::Unknown
        );
        assert_eq!(mii_identity(0x07, None), MiiIdentity::Ordinary);
        assert_eq!(mii_identity(0x07, Some(MiiClass::Brawler.kind())), MiiIdentity::Ordinary);
        assert_eq!(mii_identity(-1, Some(MiiClass::Brawler.kind())), MiiIdentity::Ordinary);
        assert_eq!(export_mii_refusal(MiiIdentity::Unknown), Some(
            "Exact Mii was not available when recording started. Record this slot again."
        ));
        assert_eq!(export_mii_refusal(MiiIdentity::Ordinary), None);
        assert_eq!(export_mii_refusal(MiiIdentity::Exact), None);
    }

}
