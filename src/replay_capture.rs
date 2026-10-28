//! Observe the latest native Vault replay without changing its input or simulation.
//!
//! Call `before_frame` / `after_frame` around the existing original fighter-frame
//! callback, outside the Training-only gate. Export uses the same portable macro
//! as manual recording. A replay is selected and played through the native Vault;
//! its captured timeline remains available when returning to Training.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::convert::TryFrom;

use smash::app::{self, lua_bind::*, BattleObjectModuleAccessor, Fighter};
use smash::lib::lua_const::*;
use training_mod_macros::{InputFrame, LiveFrame, LiveInput, ReplayFrame, ReplaySession, ReplayTrack,
    StartState, MAX_FRAMES};
use training_mod_tui::{macro_panel::{fighter_gate, FighterChoice, FighterGate, LibraryRow, Pad, View}, clip_browser::{ClipBrowser, Effect as BrowserEffect}, clip_panel::{direction_action, scrub_repeat_step, ClipAction, ClipEditor, ClipEffect, ClipView, CloseInputGuard, Handle}};

use crate::common::game_mode;

const REPLAY_MODE_13_0_5: u32 = 0x05010100;
// Native calibration: all 4,222 post-frame samples exactly recovered the mapped
// buttons/left axes with this scale. The mapped callback precedes that fighter
// simulation tick; replay itself never calls that callback. AI values may be off-grid.
// The120-scale conversion now lives in LiveInput::project, at the consumer seam.
static ENABLED: AtomicBool = AtomicBool::new(false);
static EDITOR: Mutex<Option<Editor>> = Mutex::new(None);
static BROWSER: Mutex<Option<Browser>> = Mutex::new(None);
static REVISION: AtomicU64 = AtomicU64::new(1);
static SELECTION_REVISION: AtomicU64 = AtomicU64::new(1);
static HOLD: Mutex<(i32, u32, bool)> = Mutex::new((0, 0, false));
static RELOAD: Mutex<Option<Reload>> = Mutex::new(None);
static FAILED_SELECTION: AtomicU64 = AtomicU64::new(0);
static CLOSE_INPUT: Mutex<CloseInputGuard> = Mutex::new(CloseInputGuard::new());
#[cfg(feature = "named-input-trace")]
static CLOSE_TRACE_LEFT: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "named-input-trace")]
static CLOSE_TRACE_ID: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "named-input-trace")]
const CLOSE_TRACE_LIMIT: u64 = 600;
use training_mod_consts::mii::{inventory::MiiClass, lifetime::Configuration, replay::{RawMii,Target}};
struct Browser { ui: ClipBrowser, revision: u64 }
/// `switch` is set only by an answered Switch prompt: the recorded fighter is then
/// requested once, and cleared when that request settles, fails or is cancelled. An
/// editor without it never changes the CPU, whatever fighter the track recorded.
struct Editor { clip:ClipEditor, association:[Result<RawMii,String>;8], resolved:[Option<Configuration>;8], resolve_failed:u8, switch:bool }
impl std::ops::Deref for Editor {type Target=ClipEditor;fn deref(&self)->&ClipEditor {&self.clip}}
impl std::ops::DerefMut for Editor {fn deref_mut(&mut self)->&mut ClipEditor {&mut self.clip}}
impl Editor {
    /// Recorded Mii of one track. `previous` is the older capture, which keeps no
    /// exact association.
    fn raw_for(&self,entry:i32,kind:i32,previous:bool)->Result<training_mod_consts::mii::replay::RawMii,String> {
        if previous { return Err("Previous capture has no exact Mii association".into()); }
        let entry=usize::try_from(entry).map_err(|_|"Invalid capture entry")?;
        let raw=self.association.get(entry).ok_or("Invalid capture entry")?.as_ref().map_err(|error| error.clone())?;
        if raw.class().kind()!=kind as u32 { return Err("Capture fighter identity changed".into()); }
        Ok(*raw)
    }
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    fn raw(&self)->Result<training_mod_consts::mii::replay::RawMii,String> {
        let view=self.view();
        self.raw_for(view.entry,view.fighter_kind,view.previous_selected)
    }
    /// The recorded identity a Switch would load. `Ok(None)` is a Mii whose exact
    /// configuration is not resolved yet. Preview and Keep never need either.
    fn target_for(&self,entry:i32,kind:i32,previous:bool)->Result<Option<Target>,String> {
        if MiiClass::from_kind(kind as u32).is_none() {
            if training_mod_consts::TrioFighter::from_kind(kind as u32).is_none() {
                return Err("Preview fighter is not available".into());
            }
            return Ok(Some(Target::fighter(kind as u32)));
        }
        self.raw_for(entry,kind,previous)?;
        if self.resolve_failed & (1 << entry as usize) != 0 { return Err("Recorded Mii metadata is unavailable".into()); }
        if !cfg!(feature="mii-ordinary-cpu-alpha") {return Err("Replay Mii loading is unavailable in this build".into());}
        Ok(self.resolved[entry as usize].map(Target::mii))
    }
    fn target(&self)->Result<Option<Target>,String> {
        let view=self.view();
        self.target_for(view.entry,view.fighter_kind,view.previous_selected)
    }
    /// One selection's fighter gate against the settled CPU. Only the exact
    /// readiness predicate counts as a match, so a same-class Mii whose exact
    /// configuration is unknown or different is a mismatch.
    fn gate_for(&self,entry:i32,kind:i32,previous:bool,current:Option<i32>)->FighterGate {
        let target=self.target_for(entry,kind,previous);
        let switchable=target.is_ok();
        let matches=target.ok().flatten().is_some_and(|target|
            crate::training::named_macros::recorded_target_matches(target,current));
        let prompt=training_mod_sync::read(&crate::common::consts::MENU).fighter_prompt==crate::common::consts::OnOff::ON;
        fighter_gate(prompt,switchable,matches)
    }
}
/// What the open fighter prompt will do with its answer. `action` is the track or
/// capture change held back until then; None means the editor's opening track.
#[derive(Clone,Copy)]
struct Pending { revision:u64, action:Option<ClipAction> }
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
/// Resolve recorded metadata before asking, without changing the CPU or track.
/// This uses the existing native-service loading window and its safety gates.
#[derive(Clone,Copy)]
struct ResolveSelection { action:Option<ClipAction>, entry:usize }
struct Reload { editor:Editor, requested:bool, ticks:u32, revision:u64,
    selection:Option<ResolveSelection>,
    resolving:Option<std::sync::mpsc::Receiver<Result<Configuration,String>>> }
pub(crate) fn selection_revision()->u64 {SELECTION_REVISION.load(Ordering::Acquire)}
fn target_ready(target:Target,current:Option<i32>)->bool {
    let Some(kind)=current else {return false;};
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    { crate::character_roster::mii_replay::ready(target,kind as u32) }
    #[cfg(not(feature="mii-ordinary-cpu-alpha"))]
    {target.ready(kind as u32,None)}
}
static CAPTURE: Mutex<Capture> = Mutex::new(Capture {
    recording: false,
    session: ReplaySession { tracks: Vec::new(), truncated: false },
    previous: None,
    pending: [None; 8],
});

struct Capture {
    recording: bool,
    session: ReplaySession,
    previous: Option<ReplaySession>,
    pending: [Option<ReplayFrame>; 8],
}

pub fn init() {
    // This verifies the owned 13.0.5 scene-access boundary before direct reads.
    ENABLED.store(game_mode::init(), Ordering::Release);
}

fn in_replay() -> bool {
    ENABLED.load(Ordering::Acquire)
        && game_mode::current().is_some_and(|mode| {
            mode.initialized && mode.melee == 0 && mode.mode == REPLAY_MODE_13_0_5
        })
}

/// Clone once when opening the replay-window editor, not on each render frame.
pub fn latest() -> Option<ReplaySession> {
    let capture = CAPTURE.lock().unwrap();
    capture.session.tracks.iter().any(|t| !t.frames.is_empty())
        .then(|| capture.session.clone())
}

fn notify(message: String) {
    if let Some(editor) = EDITOR.lock().unwrap().as_mut() {
        editor.report_error(message);
        training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
        return;
    }
    crate::training::ui::notifications::notification("Clip Editor".into(), message, 180);
}

pub fn open_editor() {
    open_browser(None);
}

fn open_browser(expected_revision: Option<u64>) {
    if !crate::training::practice::enabled() || unsafe { !Fighter::is_ready_go() || !crate::training::practice::acquire() }
        || crate::training::recording_or_playback()
        || unsafe { crate::training::save_states::is_loading() }
        || crate::character_roster::busy() || crate::replay_preview::occupied() || loading()
    {
        notify("Open the replay window in idle practice".into());
        return;
    }
    let mut rows = Vec::new();
    {
        let capture = CAPTURE.lock().unwrap();
        if capture.session.tracks.iter().any(|track| !track.frames.is_empty()) {
            rows.push(LibraryRow { id: "latest".into(), label: "Latest Vault capture".into() });
        }
        if capture.previous.is_some() {
            rows.push(LibraryRow { id: "previous".into(), label: "Previous Vault capture".into() });
        }
    }
    let mut skipped = 0;
    match crate::replay_store::archive().list() {
        Ok(listing) => {
            skipped = listing.skipped.len();
            rows.extend(listing.entries.into_iter().map(|entry| LibraryRow {
                id: format!("saved:{}", entry.id),
                label: format!("{} · {}f{}", entry.replay_name, entry.frames,
                    if entry.truncated { " (partial)" } else { "" }),
            }));
        }
        Err(error) => notify(format!("Saved capture library: {error}")),
    }
    if rows.is_empty() {
        notify("Save a replay with match capture on, or play one in Vault first".into());
        return;
    }
    {
        let mut browser = BROWSER.lock().unwrap();
        // A refusal may rescan the library, but must not reopen after Cancel or
        // interruption while that I/O was in flight.
        if expected_revision.is_some_and(|revision|
            !browser.as_ref().is_some_and(|b| b.revision == revision && b.ui.opening())) { return; }
        if crate::training::named_macros::suspended() || !crate::training::practice::enabled() { return; }
        *browser = Some(Browser { ui: ClipBrowser::new(rows, skipped),
            revision: REVISION.fetch_add(1, Ordering::Relaxed) + 1 });
    }
    training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
}

fn open_capture(id: &str, revision: u64) -> Result<(), String> {
    let (session, name, previous, miis) = match id {
        "latest" => (
            latest().ok_or("No latest Vault capture")?,
            "Latest Vault capture".to_owned(),
            CAPTURE.lock().unwrap().previous.clone(),
            None,
        ),
        "previous" => (
            CAPTURE.lock().unwrap().previous.clone().ok_or("No previous Vault capture")?,
            "Previous Vault capture".to_owned(),
            None,
            None,
        ),
        _ => {
            let id = id.strip_prefix("saved:").ok_or("Unknown replay capture")?;
            let saved = crate::replay_store::archive().load(id).map_err(|error| error.to_string())?;
            if saved.game_version != "13.0.5" {
                return Err(format!("Capture is for Smash {}, expected 13.0.5", saved.game_version));
            }
            (saved.session, saved.replay_name, None, saved.miis)
        }
    };
    // Inspect the full owned session once, before ClipEditor selects/crops a track.
    let association=std::array::from_fn(|entry| {
        let miis=miis.as_ref().ok_or_else(||"This capture has no exact Mii configuration; ordinary Mii preferences cannot substitute".to_string())?;
        let kind=session.tracks.iter().find(|track|track.entry==entry as i32)
            .and_then(|track|track.frames.first()).map(|frame|frame.start.fighter_kind as u32)
            .ok_or_else(||"Capture track is absent".to_string())?;
        session.mii_for_track(miis,entry as i32,kind).map_err(str::to_string)
    });
    let result = ClipEditor::new(session, clean_statuses())
        .map(|editor| editor.with_move_rules(crate::replay_move_start::rules()));
    match result {
        Ok(mut editor) => {
            // Do not take preview/named-UI locks under BROWSER: their visibility
            // paths can acquire it. Lifecycle retirement below is the publication
            // fence, with the native mode/status checked again before publication.
            if !capture_ready() { return Err("Practice changed before the capture could open".into()); }
            let mut browser = BROWSER.lock().unwrap();
            if !browser.as_ref().is_some_and(|b| b.revision == revision && b.ui.opening()) {
                return Ok(()); // Cancel/departure owns this result; never publish it late.
            }
            if crate::training::named_macros::suspended() || !crate::training::practice::enabled() {
                return Err("Practice changed before the capture could open".into());
            }
            let cpu = crate::common::try_get_module_accessor(crate::consts::FighterId::CPU)
                .ok_or("Wait for the practice CPU before opening a capture")?;
            if !clean_statuses().contains(&unsafe { StatusModule::status_kind(cpu) }) {
                return Err("Open the clip editor while the CPU is idle or falling".into());
            }
            let current = unsafe { app::utility::get_kind(&mut *cpu) };
            editor.select_kind(current);
            *EDITOR.lock().unwrap() = Some(Editor {clip:editor.with_name(name).with_previous(previous),
                association,resolved:[None;8],resolve_failed:0,switch:false});
            PENDING.lock().unwrap().take();
            browser.take();
            drop(browser);
            SELECTION_REVISION.fetch_add(1,Ordering::AcqRel);
            crate::training::named_macros::cancel_character_selection();
            REVISION.fetch_add(1, Ordering::Relaxed);
            *HOLD.lock().unwrap() = (0, 0, false);
            FAILED_SELECTION.store(0, Ordering::Relaxed);
            training_mod_sync::assign(&crate::common::menu::QUICK_MENU_ACTIVE, false);
            // The opening track is the first explicit selection. The editor stays
            // inert behind the prompt; nothing reloads the CPU unless Switch is chosen.
            gate_selection(None, Some(current));
        }
        Err(error) => return Err(error),
    }
    training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
    Ok(())
}

/// Ask once for one explicit selection when its recorded fighter is not the
/// settled CPU. `action` is the held-back track/capture change, or None for the
/// opening track. Returns true when a prompt now owns the selection. A prompt that
/// cannot open degrades to Keep: the selection proceeds on the current CPU.
fn gate_selection(action: Option<ClipAction>, current: Option<i32>) -> bool {
    let (gate, recorded, unresolved) = {
        let editor = EDITOR.lock().unwrap();
        let Some(editor) = editor.as_ref() else { return false; };
        let (entry, kind, previous) = match action {
            Some(action) => match editor.peek_selection(action) { Some(candidate) => candidate, None => return false },
            None => { let view = editor.view(); (view.entry, view.fighter_kind, view.previous_selected) }
        };
        let unresolved = matches!(editor.target_for(entry, kind, previous), Ok(None))
            .then(|| editor.raw_for(entry, kind, previous).ok().map(|raw| (entry as usize, raw)))
            .flatten();
        (editor.gate_for(entry, kind, previous, current), kind_label(kind), unresolved)
    };
    // Resolve appearance metadata independently of the user's fighter choice.
    // Keep can then export the original Mii, and an already-matching Mii does not
    // prompt just because this selection lacks a loader acknowledgement.
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    if let Some((entry, raw)) = unresolved {
        let revision = SELECTION_REVISION.load(Ordering::Acquire);
        match crate::character_roster::mii_replay::resolve(raw, revision) {
            Ok(receive) => {
                let taken = EDITOR.lock().unwrap().take();
                if let Some(mut editor) = taken {
                    editor.pause();
                    *RELOAD.lock().unwrap() = Some(Reload { editor, requested:false, ticks:0,
                        revision, selection:Some(ResolveSelection { action, entry }), resolving:Some(receive) });
                    crate::replay_preview::request_stop();
                    training_mod_sync::assign(&crate::common::menu::QUICK_MENU_ACTIVE, false);
                    training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
                    return true;
                }
            }
            Err(error) => {
                if let Some(editor) = EDITOR.lock().unwrap().as_mut() { editor.resolve_failed |= 1 << entry; }
                notify(format!("{error}; inputs can still preview on the current CPU"));
                return false;
            }
        }
    }
    #[cfg(not(feature="mii-ordinary-cpu-alpha"))]
    let _ = unresolved;
    if gate != FighterGate::Prompt { return false; }
    let revision = SELECTION_REVISION.load(Ordering::Acquire);
    *PENDING.lock().unwrap() = Some(Pending { revision, action });
    let opened = crate::training::named_macros::open_clip_fighter_prompt(
        revision, recorded, current.map_or_else(|| "CPU".to_owned(), kind_label));
    if opened {
        // The question owns the pad: a running preview stops and restores, and the
        // editor shows it paused whichever way the question is answered.
        crate::replay_preview::request_stop();
        if let Some(editor) = EDITOR.lock().unwrap().as_mut() { editor.pause(); }
    } else {
        PENDING.lock().unwrap().take();
    }
    opened
}

fn kind_label(kind: i32) -> String { crate::training::named_macros::fighter_label(kind) }

/// The prompt's answer, from the named panel. Keep and Switch apply the held-back
/// selection; Cancel drops it (the opening selection's Cancel closes the editor).
/// A selection that moved on since the question was asked is never applied.
pub fn fighter_choice(choice: Option<FighterChoice>) {
    let Some(pending) = PENDING.lock().unwrap().take() else { return; };
    if pending.revision != SELECTION_REVISION.load(Ordering::Acquire) { return; }
    match (choice, pending.action) {
        (None, None) => close_editor(),
        (None, Some(_)) => {}
        (Some(choice), action) => {
            if let Some(action) = action { apply_action(action); }
            if choice == FighterChoice::Switch {
                if let Some(editor) = EDITOR.lock().unwrap().as_mut() { editor.switch = true; }
            }
        }
    }
    training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
}

pub fn browser_visible() -> bool {
    !crate::training::named_macros::suspended() && BROWSER.lock().unwrap().is_some()
}

pub fn browser_view() -> Option<View> {
    BROWSER.lock().unwrap().as_ref().and_then(|b| b.ui.view())
}

pub fn browser_input(pad: Pad) {
    let waiting = crate::training::practice::waiting();
    let (effect, revision) = {
        let mut browser = BROWSER.lock().unwrap();
        let Some(browser) = browser.as_mut() else { return };
        (browser.ui.on_pad(pad, waiting), browser.revision)
    };
    browser_effect(effect, revision);
    training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
}

fn browser_effect(effect: BrowserEffect, revision: u64) {
    match effect {
        BrowserEffect::Open(id) => {
            if let Err(error) = open_capture(&id, revision) {
                open_browser(Some(revision));
                // A busy/departed practice context may refuse the rescan too.
                // Retire only the old owner, never a newly opened browser.
                close_editor_revision(false, Some(revision));
                notify(error);
            }
        }
        BrowserEffect::Close => { close_editor_revision(false, Some(revision)); }
        BrowserEffect::TimedOut => {
            if close_editor_revision(false, Some(revision)) {
                notify("CPU did not settle; capture opening cancelled".into());
            }
        }
        BrowserEffect::Notify(error) => notify(error),
        _ => {}
    }
}

fn capture_ready() -> bool {
    crate::training::practice::enabled() && !crate::training::named_macros::suspended()
        && unsafe { Fighter::is_ready_go() && !crate::training::save_states::is_loading() }
        && !crate::training::recording_or_playback() && !crate::replay_preview::occupied()
}

/// The existing browser owns this short neutral-input interval. Native controller
/// writes still require practice::owns_cpu_controls at the actual CPU hook.
pub fn quiescing_capture() -> bool {
    // Most command-flag calls have no browser request; avoid native mode reads
    // on that hot path. Release the browser lock before querying other owners.
    let pending = BROWSER.lock().unwrap().as_ref().is_some_and(|b| b.ui.pending());
    pending && !crate::training::named_macros::suspended()
        && crate::training::practice::waiting()
}

pub unsafe fn browser_needs_native_update(module: &mut BattleObjectModuleAccessor) -> bool {
    if !quiescing_capture() || crate::training::practice::role(module) != Some(1) { return false; }
    let clean = clean_statuses().contains(&StatusModule::status_kind(module));
    BROWSER.lock().unwrap().as_ref().is_some_and(|b| b.ui.release_cpu(clean))
}

unsafe fn service_browser() {
    if !BROWSER.lock().unwrap().as_ref().is_some_and(|b| b.ui.pending()) { return; }
    let eligible = crate::training::practice::waiting() && capture_ready();
    let clean = eligible && crate::common::try_get_module_accessor(crate::consts::FighterId::CPU)
        .is_some_and(|cpu| clean_statuses().contains(&StatusModule::status_kind(cpu)));
    let (effect, revision) = {
        let mut browser = BROWSER.lock().unwrap();
        let Some(browser) = browser.as_mut() else { return };
        (browser.ui.tick(eligible, clean), browser.revision)
    };
    browser_effect(effect, revision);
}

pub fn panel_visible() -> bool {
    !crate::training::named_macros::suspended() && EDITOR.lock().unwrap().is_some()
}

pub fn loading() -> bool { !crate::training::named_macros::suspended() && RELOAD.lock().unwrap().is_some() }

pub fn suspend_waiting() {
    // An unfinished editor is value-only continuation. A pending browser open is
    // an operation instead: retire it so return cannot silently load the old ID.
    {
        let mut browser = BROWSER.lock().unwrap();
        if browser.as_ref().is_some_and(|b| b.ui.pending()) { browser.take(); }
    }
    let revision = SELECTION_REVISION.fetch_add(1, Ordering::AcqRel) + 1;
    // The open fighter question was about the old scene's CPU.
    PENDING.lock().unwrap().take();
    if let Some(editor) = EDITOR.lock().unwrap().as_mut() { editor.pause(); }
    if let Some(reload) = RELOAD.lock().unwrap().as_mut() {
        reload.editor.pause();
        reload.requested = false;
        reload.ticks = 0;
        reload.revision = revision;
        reload.resolving = None;
    }
    REVISION.fetch_add(1, Ordering::Relaxed);
    *HOLD.lock().unwrap() = (0, 0, false);
}

pub fn discard_waiting() {
    RELOAD.lock().unwrap().take();
    close_editor_inner(false);
}
pub fn loading_view() -> Option<View> {
    RELOAD.lock().unwrap().as_ref().map(|pending| View { title: "Clip Editor".into(), selected: 0,
        labels: vec![if pending.selection.is_some() { "Reading recorded Mii..." } else { "Loading preview fighter..." }.into()],
        footer: if pending.selection.is_some_and(|selection| selection.action.is_some()) {
            "B: cancel selection".into()
        } else { "B: close editor".into() } })
}
pub fn cancel_reload() {
    let pending = RELOAD.lock().unwrap().take();
    // B during a track's metadata read cancels that choice, preserving the crop.
    // Opening-time cancel and an actual fighter load still close the editor.
    if pending.as_ref().is_some_and(|p| p.selection.is_some_and(|s| s.action.is_some())) {
        *EDITOR.lock().unwrap() = pending.map(|p| p.editor);
        SELECTION_REVISION.fetch_add(1, Ordering::AcqRel);
        REVISION.fetch_add(1, Ordering::Relaxed);
        training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
        return;
    }
    let was_loading = pending.is_some();
    if was_loading { println!("[ClipPreview] reload_cancel"); }
    close_editor_inner(was_loading);
}

/// Only the value-only editor survives its requested CPU install. Native preview
/// cleanup must finish before handing control to the existing character loader.
pub unsafe fn service_reload() {
    if crate::training::named_macros::suspended() { return; }
    if !crate::training::practice::scene_enabled() {
        let had_reload=RELOAD.lock().unwrap().take().is_some();
        if had_reload || panel_visible() || browser_visible() {close_editor_inner(had_reload);}
        return;
    }
    service_browser();
    if crate::training::lobby::native_ui_busy() { return; }
    let cpu=crate::common::try_get_module_accessor(crate::consts::FighterId::CPU);
    let current=cpu.map(|cpu|app::utility::get_kind(&mut *cpu));
    let mut reload=RELOAD.lock().unwrap();
    if let Some(pending)=reload.as_mut() {
        pending.ticks=pending.ticks.saturating_add(1);
        if let Some(selection) = pending.selection {
            // A waiting-scene interruption retires the receiver. Keep the old
            // crop, never apply that old choice or mark its metadata invalid.
            if pending.resolving.is_none() {
                let pending = reload.take().unwrap();
                drop(reload);
                *EDITOR.lock().unwrap() = Some(pending.editor);
                REVISION.fetch_add(1, Ordering::Relaxed);
                notify("Selection changed; choose again".into());
                return;
            }
            let result = match pending.resolving.as_ref().map(|receive| receive.try_recv()) {
                Some(Ok(result)) => Some(result),
                Some(Err(std::sync::mpsc::TryRecvError::Empty)) if pending.ticks <= 600 => None,
                _ => Some(Err("Recorded Mii metadata could not be read; reopen this capture".into())),
            };
            if let Some(result) = result {
                let mut pending = reload.take().unwrap();
                drop(reload);
                let retired = result.as_ref().is_err_and(|error| error == "Replay selection retired");
                if let Ok(configuration) = result.as_ref() {
                    pending.editor.resolved[selection.entry] = Some(*configuration);
                } else if !retired {
                    pending.editor.resolve_failed |= 1 << selection.entry;
                }
                *EDITOR.lock().unwrap() = Some(pending.editor);
                REVISION.fetch_add(1, Ordering::Relaxed);
                training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
                match result {
                    Ok(_) => if !gate_selection(selection.action, current) {
                        if let Some(action) = selection.action { apply_action(action); }
                    },
                    Err(error) => {
                        if !retired {
                            // Metadata failure disables Switch/export, not the inputs.
                            if let Some(action) = selection.action { apply_action(action); }
                            notify(format!("{error}; inputs can still preview on the current CPU"));
                        } else { notify("Selection changed; choose again".into()); }
                    },
                }
            }
            return;
        }
        let mut failure=None;
        if let Some(receive)=pending.resolving.as_ref() {
            match receive.try_recv() {
                Ok(Ok(configuration))=>{
                    let entry=pending.editor.view().entry as usize;
                    pending.editor.resolved[entry]=Some(configuration);
                    pending.resolving=None;
                }
                Ok(Err(error))=>failure=Some(error),
                Err(std::sync::mpsc::TryRecvError::Disconnected)=>failure=Some("Mii resolver unavailable".into()),
                Err(std::sync::mpsc::TryRecvError::Empty)=>{},
            }
        }
        let target=match pending.editor.target() {Ok(target)=>target,Err(error)=>{failure=Some(error);None}};
        #[cfg(feature="mii-ordinary-cpu-alpha")]
        if let Some(reason)=target.and_then(|target|crate::character_roster::mii_replay::refusal(target,pending.revision)) {
            failure=Some(reason.into());
        }
        let grounded_idle=cpu.is_some_and(|cpu|StatusModule::status_kind(cpu)==*FIGHTER_STATUS_KIND_WAIT
            && StatusModule::situation_kind(cpu)==*SITUATION_KIND_GROUND);
        if failure.is_none() && target.is_some_and(|target|target_ready(target,current))
            && !crate::character_roster::busy() && Fighter::is_ready_go() && grounded_idle {
            // The requested switch has settled; the editor stops enforcing it.
            let mut pending=reload.take().unwrap();
            pending.editor.switch=false;
            *EDITOR.lock().unwrap()=Some(pending.editor);
            REVISION.fetch_add(1,Ordering::Relaxed);
            training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT,true);
        } else if failure.is_some() || pending.ticks>600 {
            let mut pending=reload.take().unwrap();
            pending.editor.switch=false;
            let cancelled=SELECTION_REVISION.fetch_add(1,Ordering::AcqRel)+1;
            FAILED_SELECTION.store(cancelled,Ordering::Release);
            *EDITOR.lock().unwrap()=Some(pending.editor);
            notify(failure.unwrap_or_else(||"Exact preview fighter did not load; selection kept".into()));
        } else if target.is_none() && pending.resolving.is_none() {
            #[cfg(feature="mii-ordinary-cpu-alpha")]
            {
                let raw=pending.editor.raw();
                let revision=pending.revision;
                drop(reload);
                let result=raw.and_then(|raw|crate::character_roster::mii_replay::resolve(raw,revision));
                if let Some(pending)=RELOAD.lock().unwrap().as_mut().filter(|p|p.revision==revision) {
                    match result {Ok(receive)=>pending.resolving=Some(receive),Err(error)=>{
                        let (send,receive)=std::sync::mpsc::channel();let _=send.send(Err(error));
                        pending.resolving=Some(receive);
                    }}
                }
            }
        } else if let Some(target)=target.filter(|_|!pending.requested) {
            let revision=pending.revision;
            drop(reload);
            if crate::character_roster::cpu_reload_ready() {
                #[cfg(feature="mii-ordinary-cpu-alpha")]
                let requested=crate::character_roster::mii_replay::request(target,revision);
                #[cfg(not(feature="mii-ordinary-cpu-alpha"))]
                let requested=crate::character_roster::request_cpu_kind(target.kind());
                if let Some(pending)=RELOAD.lock().unwrap().as_mut().filter(|p|p.revision==revision) {
                    pending.requested=requested;
                }
            }
        }
        return;
    }
    drop(reload);
    if crate::replay_preview::occupied() || !crate::training::named_macros::clip_foreground()
        || !Fighter::is_ready_go() {return;}
    let mut editor=EDITOR.lock().unwrap();
    let Some(active)=editor.as_ref() else {return;};
    // The recorded fighter is requested only for an answered Switch. Keep, a
    // matching fighter, or a Switch that already settled never reload the CPU, so a
    // later manual CPU change is not undone.
    if !active.switch {return;}
    if FAILED_SELECTION.load(Ordering::Acquire)==SELECTION_REVISION.load(Ordering::Acquire) {return;}
    match active.target() {
        Err(error)=>{
            FAILED_SELECTION.store(SELECTION_REVISION.load(Ordering::Acquire),Ordering::Release);
            if let Some(active)=editor.as_mut() {active.switch=false;}
            drop(editor);notify(error);
        },
        Ok(target) if target.is_none_or(|target|!target_ready(target,current))=>{
            let mut pending=editor.take().unwrap();pending.pause();
            *RELOAD.lock().unwrap()=Some(Reload {editor:pending,requested:false,ticks:0,
                revision:SELECTION_REVISION.load(Ordering::Acquire),selection:None,resolving:None});
            training_mod_sync::assign(&crate::common::menu::QUICK_MENU_ACTIVE,false);
            training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT,true);
        }
        _=>{if let Some(active)=editor.as_mut() {active.switch=false;}},
    }
}

pub fn clip_view() -> Option<ClipView> {
    EDITOR.lock().unwrap().as_ref().map(|editor|editor.view())
}

pub fn preview_progress(revision: u64, consumed: u32) {
    let mut editor = EDITOR.lock().unwrap();
    if revision == REVISION.load(Ordering::Relaxed) {
        if let Some(editor) = editor.as_mut() { editor.preview_progress(consumed); }
    }
}

pub fn preview_failed(message: &str) {
    if let Some(editor) = EDITOR.lock().unwrap().as_mut() { editor.pause(); }
    notify(message.into());
}

pub fn close_editor() {
    // The native character install calls session cleanup. Its parked editor
    // belongs to RELOAD and must survive that cleanup until explicit B cancel.
    close_editor_inner(false);
}

fn close_editor_inner(was_loading: bool) {
    close_editor_revision(was_loading, None);
}

fn close_editor_revision(was_loading: bool, expected_revision: Option<u64>) -> bool {
    let (was_browsing, was_open) = {
        let mut browser = BROWSER.lock().unwrap();
        if expected_revision.is_some_and(|revision|
            browser.as_ref().is_none_or(|b| b.revision != revision)) { return false; }
        // Publication and close share BROWSER -> EDITOR order. The loader drops
        // BROWSER before touching named UI, whose visibility path reads BROWSER.
        (browser.take().is_some(), EDITOR.lock().unwrap().take().is_some())
    };
    crate::replay_preview::request_stop();
    PENDING.lock().unwrap().take();
    REVISION.fetch_add(1, Ordering::Relaxed);
    *HOLD.lock().unwrap() = (0, 0, false);
    if was_open || was_browsing || was_loading {
        SELECTION_REVISION.fetch_add(1,Ordering::AcqRel);
        crate::training::frame_counter::reset_frame_count(*crate::common::menu::MENU_CLOSE_FRAME_COUNTER);
        crate::training::frame_counter::start_counting(*crate::common::menu::MENU_CLOSE_FRAME_COUNTER);
        CLOSE_INPUT.lock().unwrap().arm(crate::common::menu::MENU_CLOSE_WAIT_FRAMES);
        #[cfg(feature = "named-input-trace")]
        {
            CLOSE_TRACE_LEFT.store(CLOSE_TRACE_LIMIT, Ordering::Relaxed);
            let id = CLOSE_TRACE_ID.fetch_add(1, Ordering::Relaxed) + 1;
            println!("[ClipClose] arm id={}", id);
        }
        training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
    }
    true
}

/// Poll once per P1 mapping, including outside Training to discard stale ownership.
pub fn owns_close_input(in_training: bool, close_held: bool) -> bool {
    CLOSE_INPUT.lock().unwrap().poll(in_training, close_held)
}

/// Bounded console evidence of the actual mapper boundary; no persistent writes.
#[cfg(feature = "named-input-trace")]
pub fn trace_close_input(controller: usize, draw: u32, raw_before: bool, raw_after: bool, mapped: u32, owned: bool) {
    if let Ok(left) = CLOSE_TRACE_LEFT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1)) {
        println!("[ClipClose] poll id={} n={} controller={:#x} draw={} raw_before={} raw_after={} mapped={:#x} owned={}",
            CLOSE_TRACE_ID.load(Ordering::Relaxed), CLOSE_TRACE_LIMIT + 1 - left,
            controller, draw, raw_before, raw_after, mapped, owned);
    }
}

pub fn on_pad(pad: Pad) {
    let action = EDITOR.lock().unwrap().as_ref().and_then(|editor| editor.action_for_pad(pad));
    if let Some(action) = action { on_action(action); }
}

pub fn on_action(action: ClipAction) {
    // A track or capture change is an explicit selection: a recorded fighter that is
    // not the settled CPU asks once, and nothing changes until it is answered.
    if matches!(action, ClipAction::PreviousFighter | ClipAction::NextFighter | ClipAction::ToggleCapture) {
        let current = crate::common::try_get_module_accessor(crate::consts::FighterId::CPU)
            .map(|cpu| unsafe { app::utility::get_kind(&mut *cpu) });
        if gate_selection(Some(action), current) {
            training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
            return;
        }
    }
    apply_action(action);
}

fn apply_action(action: ClipAction) {
    if matches!(action, ClipAction::TogglePlay | ClipAction::Restart)
        && EDITOR.lock().unwrap().as_ref().is_some_and(|editor| editor.switch) {
        notify("Wait for the requested fighter to load".into());
        return;
    }
    if matches!(action, ClipAction::PreviousFighter | ClipAction::NextFighter | ClipAction::ToggleCapture) {
        // A previous track's Switch is not authority to swap for a later Keep.
        if let Some(editor) = EDITOR.lock().unwrap().as_mut() { editor.switch = false; }
        FAILED_SELECTION.store(0, Ordering::Relaxed);
        SELECTION_REVISION.fetch_add(1,Ordering::AcqRel);
    }
    // Drop the editor lock before opening the naming panel or notifying the game.
    let (effect, revision, configuration) = {
        let mut editor = EDITOR.lock().unwrap();
        let revision = REVISION.fetch_add(1, Ordering::Relaxed) + 1;
        let configuration=editor.as_ref().and_then(|editor|editor.target().ok().flatten())
            .and_then(Target::configuration);
        (editor.as_mut().map(|editor| editor.on_action(action)), revision, configuration)
    };
    // Only a saved file carries identity, and it is always the recorded one: the
    // original kind, plus the exact Mii for a Mii track. A preview is inputs only
    // and needs neither, so it never depends on which fighter the CPU is.
    let attach=|mut recording:Box<training_mod_macros::MacroRecording>| -> Result<Box<training_mod_macros::MacroRecording>,String> {
        if let Some(configuration)=configuration {recording.set_mii(configuration).map_err(|e|e.to_string())?;}
        else if MiiClass::from_kind(recording.start.fighter_kind as u32).is_some() {
            return Err("Recorded Mii metadata is unavailable; reopen this capture before saving".into());
        }
        recording.playback_target().map_err(|e|e.to_string())?;
        Ok(recording)
    };
    match effect {
        Some(ClipEffect::Preview(recording)) => crate::replay_preview::request_play(*recording, revision),
        Some(ClipEffect::StopPreview) => crate::replay_preview::request_stop(),
        Some(ClipEffect::Close) => close_editor(),
        Some(ClipEffect::Export(recording)) => {
            crate::replay_preview::request_stop();
            *HOLD.lock().unwrap() = (0, 0, false);
            match attach(recording) {
                Ok(recording)=>crate::training::named_macros::offer_replay_recording(*recording),Err(error)=>notify(error),
            }
        }
        Some(ClipEffect::Error(error)) => {
            crate::replay_preview::request_stop();
            notify(error);
        }
        _ => {}
    }
    training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
}

/// Native physical controls, independent of gameplay remapping. Clock held-repeat
/// from drawing, independently of native playback's input-consumption clock.
///
/// A Pro Controller D-pad rocks into the neighboring direction when held to one
/// side. Left/Right must win that diagonal, or holding Left to move the end
/// reloads the other fighter and the crop never moves.
pub fn on_input(controller: &crate::input::Controller) {
    use crate::input::ControllerStyle;
    let p = controller.just_down;
    let b = controller.current_buttons;
    let gcc = controller.style == ControllerStyle::GCController;
    let modifier = if gcc { b.zr() } else { b.zl() || b.zr() };
    let left = b.dpad_left() || b.l_left() || b.r_left();
    let right = b.dpad_right() || b.l_right() || b.r_right();
    let direction = i32::from(right) - i32::from(left);
    {
        let mut hold = HOLD.lock().unwrap();
        if hold.0 != direction { hold.1 = 0; }
        hold.0 = direction;
        hold.2 = modifier;
    }
    // The opening A belongs to the browser: a fresh Confirm needs a release this
    // owned gate observed. B (above all) and the other controls are unaffected.
    let confirm = EDITOR.lock().unwrap().as_mut()
        .is_some_and(|editor| editor.allows_confirm(b.a(), controller.just_release.a(), p.a()));
    let action = if p.b() { Some(ClipAction::Cancel) }
        else if confirm { Some(ClipAction::Confirm) }
        else if p.x() { Some(if modifier { ClipAction::ToggleCapture } else { ClipAction::TogglePlay }) }
        else if p.y() { Some(if modifier { ClipAction::UseLeadIn } else { ClipAction::Restart }) }
        else if p.l() || (gcc && p.real_digital_l()) { Some(ClipAction::SelectIn) }
        else if p.r() || (gcc && p.real_digital_r()) { Some(ClipAction::SelectOut) }
        else {
            direction_action(
                p.dpad_left() || p.l_left() || p.r_left(),
                p.dpad_right() || p.l_right() || p.r_right(),
                p.dpad_up(),
                p.dpad_down(),
                left || right,
                modifier,
            )
        };
    if let Some(action) = action { on_action(action); }
}

/// Frames per tick of the current held scrub, or 0 when the end is not being edited.
pub fn scrub_pace() -> i32 {
    let hold = *HOLD.lock().unwrap();
    if hold.0 == 0 { return 0; }
    let editing = clip_view().is_some_and(|view| view.handle != Handle::Playhead);
    if !editing { return 0; }
    scrub_repeat_step(hold.1, hold.2)
}

/// Called AFTER info_training draws, solely for held range-edit repeat.
pub fn after_draw() {
    if !crate::training::named_macros::clip_foreground()
        || training_mod_sync::read(&crate::training::ui::menu::VANILLA_MENU_ACTIVE) {
        *HOLD.lock().unwrap() = (0, 0, false);
        return;
    }
    let delta = {
        let mut hold = HOLD.lock().unwrap();
        if hold.0 == 0 { None } else {
            hold.1 = hold.1.saturating_add(1);
            let step = scrub_repeat_step(hold.1, hold.2);
            (step != 0).then_some(hold.0 * step)
        }
    };
    if let Some(delta) = delta {
        if clip_view().is_some_and(|view| view.handle != Handle::Playhead) {
            on_action(ClipAction::Nudge(delta));
        }
    }
}

pub(crate) unsafe fn state(module: &mut BattleObjectModuleAccessor) -> ReplayFrame {
    ReplayFrame { input: InputFrame::default(), start: StartState {
        fighter_kind: app::utility::get_kind(module),
        costume: WorkModule::get_int(module, *FIGHTER_INSTANCE_WORK_ID_INT_COLOR) as u32,
        facing: PostureModule::lr(module),
        situation: StatusModule::situation_kind(module),
        status: StatusModule::status_kind(module),
        speed_x: KineticModule::get_sum_speed_x(module, *KINETIC_ENERGY_RESERVE_ATTRIBUTE_MAIN),
        speed_y: KineticModule::get_sum_speed_y(module, *KINETIC_ENERGY_RESERVE_ATTRIBUTE_MAIN),
    }, x: PostureModule::pos_x(module), y: PostureModule::pos_y(module) }
}

unsafe fn live_sum_speed3f(module: &mut BattleObjectModuleAccessor, reserve: i32) -> [f32; 3] {
    use core::arch::aarch64::{float32x4_t, vgetq_lane_f32};
    // 13.0.5: the exported thunk at0x2043a30 tail-calls KineticModule vtable+0xe0.
    // Native callers0x583c54/0x5c2e24 consume packed v0 lanes (including z), not
    // the s0/s1/s2 homogeneous aggregate declared by skyline-smash's Vector3f.
    // Borrow only its symbol address, then capture the actual packed return.
    // Declare the complete C clobber set and permit the callee's stack/memory use.
    // This avoids an unsupported Rust SIMD-FFI declaration or a dependency edit.
    let packed: float32x4_t;
    core::arch::asm!(
        "blr {getter}",
        getter = in(reg) KineticModule::get_sum_speed3f as *const (),
        inlateout("x0") module as *mut BattleObjectModuleAccessor => _,
        inlateout("w1") reserve => _,
        lateout("v0") packed,
        clobber_abi("C"),
    );
    [vgetq_lane_f32::<0>(packed), vgetq_lane_f32::<1>(packed), vgetq_lane_f32::<2>(packed)]
}

pub(crate) unsafe fn live_state(module: &mut BattleObjectModuleAccessor) -> Option<LiveFrame> {
    let before = state(module);
    let velocity = live_sum_speed3f(module, *KINETIC_ENERGY_RESERVE_ATTRIBUTE_MAIN);
    // Keep the independent scalar agreement check after correcting the return ABI.
    if velocity[0].to_bits() != before.start.speed_x.to_bits()
        || velocity[1].to_bits() != before.start.speed_y.to_bits() {
        #[cfg(feature = "named-input-trace")]
        println!("[LiveCapture] velocity mismatch body={} scalar_bits={:x?} vector_bits={:x?}",
            module.battle_object_id,
            [before.start.speed_x.to_bits(), before.start.speed_y.to_bits()],
            velocity.map(f32::to_bits));
        return None;
    }
    let sample = LiveFrame { input: LiveInput::default(), start: before.start,
        x: before.x, y: before.y, position_z: PostureModule::pos_z(module),
        speed_z: velocity[2], body: module.battle_object_id };
    #[cfg(feature = "named-input-trace")]
    if !sample.is_valid() {
        println!("[LiveCapture] invalid state body={} kind={} facing={} position={:?} velocity={:?}",
            sample.body, sample.start.fighter_kind, sample.start.facing,
            [sample.x, sample.y, sample.position_z],
            [sample.start.speed_x, sample.start.speed_y, sample.speed_z]);
    }
    sample.is_valid().then_some(sample)
}

pub unsafe fn before_frame(fighter: &mut Fighter) {
    crate::live_replay::before_frame(fighter);
    let replay = in_replay();
    if !replay || !Fighter::is_ready_go() {
        CAPTURE.lock().unwrap().recording = false;
        return;
    }
    let Some(module) = fighter.battle_object.module_accessor.as_mut() else { return };
    // Nana shares Popo's entry/controller; she must not double that fighter's clock.
    if app::utility::get_kind(module) == *FIGHTER_KIND_NANA { return; }
    let entry = WorkModule::get_int(module, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID);
    if !(0..8).contains(&entry) { return; }
    let mut capture = CAPTURE.lock().unwrap();
    if !capture.recording {
        capture.recording = true;
        let previous = std::mem::replace(&mut capture.session,
            ReplaySession { tracks: Vec::new(), truncated: false });
        if previous.tracks.iter().any(|track| !track.frames.is_empty()) {
            capture.previous = Some(previous);
        }
        capture.pending = [None; 8];
    }
    let tracks = &mut capture.session.tracks;
    if !tracks.iter().any(|t| t.entry == entry) {
        tracks.push(ReplayTrack { entry, frames: Vec::new(), quantized_analog: false });
    }
    capture.pending[entry as usize] = Some(state(module));
}

pub unsafe fn after_frame(fighter: &mut Fighter) {
    crate::live_replay::after_frame(fighter);
    if !in_replay() || !Fighter::is_ready_go() { return; }
    let Some(module) = fighter.battle_object.module_accessor.as_mut() else { return };
    if app::utility::get_kind(module) == *FIGHTER_KIND_NANA { return; }
    let entry = WorkModule::get_int(module, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID);
    if !(0..8).contains(&entry) { return; }
    let mut capture = CAPTURE.lock().unwrap();
    if capture.session.truncated { return; }
    let Some(mut sample) = capture.pending[entry as usize].take() else { return };
    let Some(track) = capture.session.tracks.iter_mut().find(|t| t.entry == entry) else { return };
    if track.frames.len() >= MAX_FRAMES {
        capture.session.truncated = true;
        return;
    }
    let Some(observed) = observed_input(module) else {
        capture.session.truncated = true;
        return;
    };
    let (input, quantized) = observed.project();
    track.quantized_analog |= quantized;
    sample.input = input;
    track.frames.push(sample);
}

pub(crate) unsafe fn observed_input(module: &mut BattleObjectModuleAccessor) -> Option<LiveInput> {
    let axes = [ControlModule::get_stick_x_no_clamp(module), ControlModule::get_stick_y_no_clamp(module),
        ControlModule::get_sub_stick_x(module), ControlModule::get_sub_stick_y(module)];
    if !axes.iter().all(|x| x.is_finite()) {
        // End at the last valid frame rather than silently compressing the timeline.
        return None;
    }
    Some(LiveInput { buttons: ControlModule::get_button(module) as u32, axes })
}

pub fn clean_statuses() -> Vec<i32> {
    vec![*FIGHTER_STATUS_KIND_WAIT, *FIGHTER_STATUS_KIND_FALL, *FIGHTER_STATUS_KIND_FALL_AERIAL]
}
