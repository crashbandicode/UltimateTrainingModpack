//! Private native-editor integration. Ordinary Training is qualified; the new
//! Quickplay adapter remains an unpublished runtime qualification candidate.
mod native;
mod native_default;
mod draft;
use draft::Draft;
mod connected_native;
mod connected;
mod inline_keyboard;
mod name_keyboard;
mod lua_vm;
mod specials_resume;
mod settings_resume;
mod return_gate;
mod lifecycle;
mod ui_resume;
mod resume_adapter;
mod screen_reentry;
use training_mod_consts::mii::editor_row as value;
mod fingerprints;

use serde::Serialize;
use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use std::sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, Arc, Mutex};
use std::time::{Duration, Instant};
use training_mod_consts::mii::inventory::{self, Snapshot};
use training_mod_tui::resume::ResumeTicket;

static BASE: AtomicUsize = AtomicUsize::new(0);
static ENABLED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static EDIT_OWNED: AtomicBool = AtomicBool::new(false);
// These are comparison tokens. They are never dereferenced after their callback.
static EDIT_CONTEXT: AtomicUsize = AtomicUsize::new(0);
static EDIT_OWNER: AtomicUsize = AtomicUsize::new(0);
static EDIT_SCENE: AtomicUsize = AtomicUsize::new(0);
static EDIT_LEAF_SCENE: AtomicUsize = AtomicUsize::new(0);
static RELEASED_SCENE: AtomicUsize = AtomicUsize::new(0);
static STATE: Mutex<Option<Session>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum Phase { Requested, Quitting, Queued, Editing, Exiting, Returning, Finished, Refused }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin { Training, Quickplay }

#[derive(Clone, Copy, Serialize)]
struct Event { name: &'static str, phase: Phase, values: [u64; 4] }

#[derive(Clone, Serialize)]
pub(crate) struct Trace {
    events: Vec<Event>,
    input_checks: Vec<(&'static str, [native::InputChange; 3])>,
}

struct Session {
    origin: Origin,
    interrupted: bool,
    submenu_completed: bool,
    snapshot_saved: bool,
    resume_generation: Option<u64>,
    restore_ticket: Option<ResumeTicket>,
    restore_screen: Option<ui_resume::Screen>,
    restore_focus: i32,
    restore_entered: bool,
    restore_frames: u16,
    restore_state: i32,
    phase: Phase,
    owner: usize,
    editor: usize,
    thread: usize,
    mode: u32,
    wait_frames: u16,
    request_deadline: Instant,
    observation_bits: u8,
    inputs: [u64; 3],
    input_baseline: Option<native::InputSnapshot>,
    seed: Draft,
    catalog: Arc<Snapshot>,
    explicit_save_finished: bool,
    saved_native: Option<(u32, inventory::Canonical)>,
    writer_state: Option<[u64; 4]>,
    trace: Trace,
}

impl Session {
    unsafe fn observe_inputs(&mut self, name: &'static str) {
        if self.trace.input_checks.len() < 4 {
            if let Some(baseline) = &self.input_baseline {
                self.trace.input_checks.push((name, baseline.compare(BASE.load(Ordering::Acquire))));
            }
        }
    }

    fn event(&mut self, name: &'static str, values: [u64; 4]) {
        if self.trace.events.len() < 192 {
            self.trace.events.push(Event { name, phase: self.phase, values });
            crate::common::filesystem::write_mii_editor(Box::new(self.trace.clone()));
        }
    }
}

pub(crate) fn write_trace(trace: &Trace) {
    if let Some(event) = trace.events.last() {
        crate::logging::diagnostic(format_args!("[MiiEditor] {} {:?} {:?}",
            event.name, event.phase, event.values));
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(trace) {
        let path = "sd:/ultimate/TrainingModpack/mii-editor-last.json";
        if let Err(error) = std::fs::write(path, bytes) {
            crate::logging::diagnostic(format_args!("[MiiEditor] trace write failed: {error}"));
        }
    }
}

pub(crate) fn request(request: training_mod_tui::mii_menu::EditorRequest) {
    let result = (|| -> Result<(), &'static str> {
        if !ENABLED.load(Ordering::Acquire) { return Err("Editor adapter is unavailable"); }
        let origin = if unsafe { crate::common::is_native_training_mode() } { Origin::Training }
            else if quickplay_available() {
                Origin::Quickplay
            } else { return Err("Enter Training or Quickplay waiting practice to edit a Mii"); };
        if !request.catalog.is_complete() { return Err("Mii catalog is incomplete"); }
        let seed = match request.seed {
            training_mod_tui::mii_menu::EditorSeed::Saved(seed) => Draft::Ready(value::from_seed(&seed)?),
            training_mod_tui::mii_menu::EditorSeed::NativeDefault(class) => {
                if !native_default::available() { return Err("Native default adapter is unavailable"); }
                Draft::NativeDefault(class)
            }
        };
        if ui_resume::pending() { return Err("An unfinished editor is waiting to resume"); }
        begin_session(origin, seed, request.catalog, None)
    })();
    if let Err(error) = result {
        crate::logging::diagnostic(format_args!("[MiiEditor] request refused: {error}"));
        crate::training::ui::notifications::notification("Mii Editor".into(), error.into(), 180);
    }
}

fn begin_session(
    origin: Origin, seed: Draft, catalog: Arc<Snapshot>,
    restore: Option<(ResumeTicket, ui_resume::Screen, i32)>,
) -> Result<(), &'static str> {
    seed.validate()?;
    if !catalog.is_complete() { return Err("Mii catalog is incomplete"); }
    let mut state = STATE.try_lock().map_err(|_| "Editor request is busy")?;
    if ACTIVE.load(Ordering::Acquire) { return Err("An editor request is already active"); }
    let trace = if restore.is_some() {
        state.as_ref().map(|s| s.trace.clone())
    } else { None }.unwrap_or_else(|| Trace { events: Vec::with_capacity(192), input_checks: Vec::new() });
    let mut session = Session { origin, interrupted: false, submenu_completed: false,
        snapshot_saved: false, resume_generation: None,
        restore_ticket: restore.map(|(ticket, _, _)| ticket),
        restore_screen: restore.map(|(_, screen, _)| screen),
        restore_focus: restore.map(|(_, _, focus)| focus).unwrap_or(0),
        restore_entered: false, restore_frames: 0, restore_state: -1,
        phase: Phase::Requested, owner: 0, editor: 0, thread: 0,
        mode: 0, wait_frames: 0, request_deadline: Instant::now() + Duration::from_secs(10), observation_bits: 0,
        inputs: [0; 3], input_baseline: None, seed, catalog,
        explicit_save_finished: false, saved_native: None, writer_state: None, trace };
    session.event(if restore.is_some() { "automatic_resume_requested" } else { "requested" },
        [seed.hash(), 0, 0, 0]);
    if let Draft::NativeDefault(class) = seed {
        session.event("native_default_requested", [class.kind() as u64, 0, 0, 0]);
    }
    *state = Some(session);
    EDIT_CONTEXT.store(0, Ordering::Release);
    EDIT_OWNER.store(0, Ordering::Release);
    EDIT_SCENE.store(0, Ordering::Release);
    EDIT_LEAF_SCENE.store(0, Ordering::Release);
    EDIT_OWNED.store(false, Ordering::Release);
    RELEASED_SCENE.store(0, Ordering::Release);
    ACTIVE.store(true, Ordering::Release);
    Ok(())
}

fn resume_checkpoint(ticket: ResumeTicket, checkpoint: ui_resume::Checkpoint) -> Result<(), &'static str> {
    if !quickplay_available() { return Err("Waiting practice is not ready"); }
    begin_session(Origin::Quickplay, checkpoint.seed, checkpoint.catalog,
        Some((ticket, checkpoint.screen, checkpoint.focus)))
}

fn quickplay_available() -> bool {
    ENABLED.load(Ordering::Acquire) && crate::training::lobby::active()
        && unsafe { native::read::<u32>(BASE.load(Ordering::Acquire), native::MODE) }
            & 0x7fffff00 == 0x07010100
}

/// File boundary on menu opening; no per-frame catalog work and no fighter role
/// capture. Ordinary Training continues to use its qualified role adapters.
pub(crate) fn menu_ready(opening: u64) {
    if !quickplay_available() || ACTIVE.load(Ordering::Acquire) { return; }
    let catalog = crate::replay_store::mii_catalog();
    let mut app = training_mod_sync::lock_write(&crate::common::menu::QUICK_MENU_APP);
    if opening == crate::character_roster::mii_saved_alpha::menu_epoch()
        && quickplay_available() && app.mount_mii_editor_catalog(catalog, opening) {
        training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
    }
}

fn current_thread() -> usize { unsafe { skyline::nn::os::GetCurrentThread() as usize } }

#[skyline::hook(offset = 0x13e8a60)]
unsafe fn training_ui(object: usize, args: usize) -> usize {
    let result = original!()(object, args);
    if !ACTIVE.load(Ordering::Acquire) || crate::common::menu::captures_input() { return result; }
    let base = BASE.load(Ordering::Acquire);
    let begin = {
        let Ok(mut state) = STATE.try_lock() else { return result; };
        let Some(s) = state.as_mut().filter(|s| s.origin == Origin::Training) else { return result; };
        if s.phase == Phase::Requested {
            s.wait_frames = s.wait_frames.saturating_add(1);
            if s.wait_frames >= 600 {
                s.phase = Phase::Refused;
                s.event("idle_ui_timeout", [0; 4]);
                ACTIVE.store(false, Ordering::Release);
                return result;
            }
        }
        if s.phase == Phase::Returning && s.thread == current_thread()
            && native::read::<u32>(base, native::MODE) == s.mode {
            s.phase = Phase::Finished;
            s.event("practice_ui_returned", [object as u64, 0, 0, 0]);
            ACTIVE.store(false, Ordering::Release);
            false
        } else { s.phase == Phase::Requested && s.owner != 0 && s.thread == current_thread() }
    };
    if begin {
        let accepted = native::request_quit(base, object);
        {
            let mut state = STATE.lock().unwrap();
            if let Some(s) = state.as_mut().filter(|s| s.phase == Phase::Requested) {
                if accepted {
                    s.phase = Phase::Quitting;
                    s.wait_frames = 0;
                    s.event("native_quit_requested", [object as u64, 0, 0, 0]);
                }
            }
        }
    }
    result
}

#[skyline::hook(offset = 0x23ebbf0)]
unsafe fn training_state(context: usize, args: usize) -> usize {
    if !ACTIVE.load(Ordering::Acquire) { return original!()(context, args); }
    let base = BASE.load(Ordering::Acquire);
    let Some(current) = native::Training::bind(base, context) else { return original!()(context, args); };
    let phase = {
        let mut state = STATE.lock().unwrap();
        let Some(s) = state.as_mut().filter(|s| s.origin == Origin::Training) else {
            drop(state);
            return original!()(context, args);
        };
        if s.phase == Phase::Requested && s.owner == 0 && !current.drained() {
            if !native::catalog_clean(base) {
                s.phase = Phase::Refused;
                s.event("native_catalog_dirty", [0; 4]);
                ACTIVE.store(false, Ordering::Release);
                drop(state);
                return original!()(context, args);
            }
            s.owner = current.sequence;
            s.thread = current_thread();
            s.mode = native::read(base, native::MODE);
            s.event("training_owner_bound", [current.sequence as u64, context as u64, s.thread as u64, 0]);
            let preview = native::preview_state(base);
            s.event("preview_before_quit", preview.map(|(a, k)| [1, a as u64, k as u64, 0]).unwrap_or([0; 4]));
        }
        if s.owner != current.sequence || s.thread != current_thread() {
            drop(state);
            return original!()(context, args);
        }
        if s.phase == Phase::Quitting && !current.drained() {
            s.wait_frames = s.wait_frames.saturating_add(1);
            if s.wait_frames >= 600 {
                s.phase = Phase::Refused;
                s.event("native_quit_timeout", [0; 4]);
                ACTIVE.store(false, Ordering::Release);
            }
        }
        s.phase
    };
    match phase {
        Phase::Quitting if current.drained() => {
            // Do not run the native Quit5 -> character-select rewrite. This owner
            // instead queues a full editor after IngameCall has actually released.
            native::register_editor(base, &current, true);
            let inputs = native::practice_inputs(base);
            native::write(base, 0x5307d40, 0u8);
            // Protection begins before any native editor child can be created.
            EDIT_OWNER.store(current.sequence, Ordering::Release);
            EDIT_OWNED.store(true, Ordering::Release);
            let queued = native::enqueue(base, &current, b"CharaMakeSequenceScene");
            let mut state = STATE.lock().unwrap();
            let s = state.as_mut().unwrap();
            s.inputs = inputs;
            s.phase = if queued { Phase::Queued } else { Phase::Refused };
            s.event("editor_queued", [u64::from(queued), inputs[0], inputs[1], inputs[2]]);
            if !queued {
                native::register_editor(base, &current, false);
                EDIT_OWNED.store(false, Ordering::Release);
                EDIT_OWNER.store(0, Ordering::Release);
                ACTIVE.store(false, Ordering::Release);
                drop(state);
                return original!()(context, args);
            }
            0
        }
        Phase::Queued | Phase::Editing => 0,
        Phase::Exiting => {
            if !current.drained() { return 0; }
            if !save_ready_for_return(base) { return 0; }
            let released = RELEASED_SCENE.load(Ordering::Acquire);
            let (mode, expected) = {
                let state = STATE.lock().unwrap(); let s = state.as_ref().unwrap();
                (s.mode, s.inputs)
            };
            // At this phase the native sequence drain follows child release,
            // completion and deletion. The release token is diagnostic only.
            native::register_editor(base, &current, false);
            let observed = native::practice_inputs(base);
            let preview = native::preview_state(base);
            native::write(base, native::MODE, mode);
            let unchanged = expected == observed;
            // First qualification never restores native pointer-bearing objects.
            // A changed input contract falls back to native Quit/CSS for inspection.
            let queued = unchanged && preview.is_some_and(|(active, _)| active == 0) && current.reenter();
            let mut state = STATE.lock().unwrap(); let s = state.as_mut().unwrap();
            s.phase = if queued { Phase::Returning } else { Phase::Refused };
            if !s.interrupted { connected::forget(&s.seed); }
            s.event("preview_after_release", preview.map(|(a, k)| [1, a as u64, k as u64, released as u64]).unwrap_or([0; 4]));
            s.event("practice_reentry_requested", [u64::from(queued), observed[0], observed[1], observed[2]]);
            EDIT_OWNED.store(false, Ordering::Release);
            EDIT_OWNER.store(0, Ordering::Release);
            if !queued {
                ACTIVE.store(false, Ordering::Release);
                drop(state);
                return original!()(context, args);
            }
            0
        }
        _ => original!()(context, args),
    }
}

/// Native calls run outside STATE's value-only lock, after the original parent
/// has finished initialization and before either checkpoint publication or entry
/// can use a complete row. Before this point the owned intent is checkpointable.
unsafe fn materialize_default(base: usize, context: usize, scene: usize) {
    let (class, restoring) = {
        let state = STATE.lock().unwrap();
        let Some(s) = state.as_ref().filter(|s| s.phase == Phase::Queued
            && !s.interrupted && s.thread == current_thread() && s.editor == scene) else { return; };
        let Draft::NativeDefault(class) = s.seed else { return; };
        let Some(editor) = native::Editor::bind(base, context, s.owner) else { return; };
        if editor.state != 3 || editor.desired != 4 || !native::drained(editor.sequence) { return; }
        (class, s.restore_ticket)
    };
    let result = native_default::materialize(base, class);
    let mut state = STATE.lock().unwrap();
    let Some(s) = state.as_mut().filter(|s| s.phase == Phase::Queued && !s.interrupted
        && s.thread == current_thread() && s.editor == scene
        && s.seed == Draft::NativeDefault(class) && s.restore_ticket == restoring) else { return; };
    let Some(editor) = native::Editor::bind(base, context, s.owner) else { return; };
    if editor.state != 3 || editor.desired != 4 || !native::drained(editor.sequence) { return; }
    let result = result.and_then(|row| {
        let mut seed = s.seed;
        seed.materialize(class, row)?;
        if let Some(ticket) = restoring { ui_resume::materialized(ticket, class, row)?; }
        s.seed = seed;
        Ok(row)
    });
    match result {
        Ok(row) => s.event("native_default_materialized", [class.kind() as u64,
            inventory::fnv1a64(&row), inventory::fnv1a64(&row[0x148..0x1a0]), 0]),
        Err(reason) => {
            let preview = native::preview_state(base).map_or(u64::MAX,
                |(state, kind)| (u64::from(state) << 32) | u64::from(kind));
            s.event("native_default_refused", [inventory::fnv1a64(reason.as_bytes()),
                native::read::<u32>(base, native::MODE) as u64,
                native::read::<u8>(base, native::DRAFT + 0x1c4) as u64, preview]);
            s.event(reason, [class.kind() as u64, 0, 0, 0]);
            crate::logging::diagnostic(format_args!("[MiiEditor] {reason}"));
            // The existing failed-entry branch cancels this drained parent
            // and discards its creation intent; no incomplete draft is copied.
        }
    }
}

#[skyline::hook(offset = 0x22fa6d0)]
unsafe fn editor_parent(scene: usize, args: usize) -> usize {
    if !EDIT_OWNED.load(Ordering::Acquire)
        || native::read::<usize>(scene, 0x68) != EDIT_OWNER.load(Ordering::Acquire) {
        return original!()(scene, args);
    }
    let base = BASE.load(Ordering::Acquire);
    let context: usize = native::read(scene, 0xe8);
    {
        // No native call runs while this short value-only lock is held. A missed
        // callback must not allow the normal saved-Mii SelectData page to open.
        let mut state = STATE.lock().unwrap();
        let Some(s) = state.as_mut() else { return 0; };
        if current_thread() != s.thread { return 0; }
        let Some(editor) = native::Editor::bind(base, context, s.owner) else { return 0; };
        if s.editor == 0 {
            s.editor = scene;
            EDIT_SCENE.store(scene, Ordering::Release);
        }
        if s.editor != scene { return 0; }
        if s.phase == Phase::Queued && editor.state == 4 {
            editor.cancel_parent();
            s.phase = Phase::Exiting;
            s.event("unexpected_select_cancelled", [0; 4]);
        }
    }
    connected::before_parent();
    let result = original!()(scene, args);
    materialize_default(base, context, scene);
    let mut state = STATE.lock().unwrap();
    let Some(s) = state.as_mut() else { return result; };
    let Some(editor) = native::Editor::bind(base, context, s.owner) else { return result; };
    if s.phase == Phase::Queued && s.interrupted && editor.state == 3
        && editor.desired == 4 && native::drained(editor.sequence) {
        // A network event won before the edit leaf existed. Native Back can
        // release CharaMake directly; do not load a Test just to cancel it.
        editor.cancel_parent();
        s.phase = Phase::Exiting;
        s.event("interrupted_before_edit_entry", [0; 4]);
    } else if s.phase == Phase::Queued && s.seed.row().is_some_and(|row| editor.enter_edit(base, row)) {
        s.phase = Phase::Editing;
        s.observe_inputs("draft_entered");
        let row = native::snapshot_draft(base);
        let same = value::parse(&row).ok().map(|(_, c)| c.canonical())
            == s.seed.row().and_then(|row| value::parse(row).ok()).map(|(_, c)| c.canonical());
        s.event("draft_entered", [u64::from(same), native::read::<u32>(base, native::DRAFT) as u64,
            context as u64, scene as u64]);
    } else if s.phase == Phase::Queued && editor.state == 3 && editor.desired == 4 {
        editor.cancel_parent();
        s.phase = Phase::Exiting;
        s.event("seed_entry_cancelled", [0; 4]);
    } else if s.phase == Phase::Editing && s.editor == scene && editor.leave_edit() {
        s.phase = Phase::Exiting;
        s.event("native_exit_requested", [2, 0, 0, 0]);
    }
    let leaving = s.phase == Phase::Exiting;
    drop(state);
    if leaving {
        resume_adapter::before_release(EDIT_CONTEXT.load(Ordering::Acquire));
        EDIT_CONTEXT.store(0, Ordering::Release);
    }
    result
}

// Current editor constructor's audited post-binding window. The initialized
// context is an identity token only, never dereferenced by another callback.
#[skyline::hook(offset = 0x180e13c, inline)]
unsafe fn edit_initialized(ctx: &mut InlineCtx) {
    if !ACTIVE.load(Ordering::Acquire) || !EDIT_OWNED.load(Ordering::Acquire) { return; }
    let context = ctx.registers[19].x() as usize;
    let scene = ctx.registers[20].x() as usize;
    if context == 0 || scene == 0 || native::read::<usize>(scene, 0x58) != context { return; }
    let mut state = STATE.lock().unwrap();
    // Native scene initialization runs on the queued-init worker. Bind the
    // current scene/context here; Decide checks the scene thread separately.
    if let Some(s) = state.as_mut().filter(|s| s.phase == Phase::Editing) {
        EDIT_CONTEXT.store(context, Ordering::Release);
        EDIT_LEAF_SCENE.store(scene, Ordering::Release);
        s.event("edit_context_bound", [context as u64, current_thread() as u64,
            native::read::<u8>(context, 0x160) as u64, 0]);
    }
}

// Complete the two native selection-screen operations omitted by direct entry.
// Temporary private-alpha milestones distinguish preview wait from active Test;
// native state dispatch and readiness checks always run unchanged.
#[skyline::hook(offset = 0x180ea90)]
unsafe fn edit_frame(scene: usize, args: usize) -> usize {
    if EDIT_OWNED.load(Ordering::Acquire) && scene == EDIT_LEAF_SCENE.load(Ordering::Acquire) {
        let context: usize = native::read(scene, 0x58);
        if context != 0 && context == EDIT_CONTEXT.load(Ordering::Acquire)
            && connected::before_edit(scene, args, context) { return 0; }
    }
    let result = original!()(scene, args);
    if !EDIT_OWNED.load(Ordering::Acquire) { return result; }
    let context: usize = native::read(scene, 0x58);
    if context == 0 || context != EDIT_CONTEXT.load(Ordering::Acquire) { return result; }
    let Ok(mut state) = STATE.try_lock() else { return result; };
    let Some(s) = state.as_mut().filter(|s| s.phase == Phase::Editing && s.thread == current_thread()) else { return result; };
    let current: i32 = native::read(context, 0x148);
    let observation = if current == 3 && s.observation_bits & 1 == 0 { Some((1, "editor_main_frame")) }
        else if current == 11 && s.observation_bits & 2 == 0 { Some((2, "editor_preview_wait")) }
        else if current == 10 && s.observation_bits & 4 == 0 { Some((4, "editor_test_frame")) }
        else { None };
    if let Some((bit, name)) = observation {
        s.observation_bits |= bit;
        let preview = native::preview_state(BASE.load(Ordering::Acquire))
            .map(|(active, kind)| (u64::from(active) << 8) | u64::from(kind)).unwrap_or(u64::MAX);
        s.event(name, [current as u64, native::read::<u32>(context, 0x14c) as u64,
            native::read::<i32>(context, 0x164) as u64, preview]);
    }
    let reveal = current != 0 && s.observation_bits & 16 == 0;
    if reveal { s.observation_bits |= 16; }
    // Request native presentation outside the session lock, once on ready entry.
    drop(state);
    if reveal {
        let flags = native::reveal_editor(BASE.load(Ordering::Acquire));
        let preview = native::request_editor_preview(BASE.load(Ordering::Acquire));
        let mut state = STATE.lock().unwrap();
        if let Some(s) = state.as_mut().filter(|s| s.phase == Phase::Editing) {
            s.observe_inputs("editor_preview_requested");
            s.event("editor_presentation_requested", flags.unwrap_or([u64::MAX; 4]));
            s.event("editor_preview_requested", preview.unwrap_or([u64::MAX; 4]));
        }
    }
    resume_adapter::after_edit(context);
    result
}

#[skyline::hook(offset = 0x180d1c0)]
unsafe fn edit_decide(context: usize) -> usize {
    if !EDIT_OWNED.load(Ordering::Acquire) || context == 0
        || native::read::<i32>(context, 0x168) != 1 { return original!()(context); }
    let base = BASE.load(Ordering::Acquire);
    let row = native::snapshot_draft(base);
    let parsed = value::parse(&row);
    let mode = native::save_mode(base).map(u64::from).unwrap_or(u64::MAX);
    let fresh = [0, 4, 8].iter().all(|&offset|
        native::read::<u32>(base, native::DRAFT + offset) == 0);
    let mut create = None;
    let mut reused = false;
    {
        let mut state = STATE.lock().unwrap();
        // Every owned explicit Save starts closed; only a qualified fresh
        // draft reaches the game's native allocator. Existing IDs cannot enter.
        native::write(context, 0x168, 2i32);
        if let Some(s) = state.as_mut().filter(|s| s.phase == Phase::Editing
            && s.thread == current_thread() && EDIT_CONTEXT.load(Ordering::Acquire) == context
            && !s.interrupted) {
            if let Ok((_, configuration)) = parsed {
                let canonical = configuration.canonical();
                if let Some(existing) = s.catalog.configs.iter().find(|c| c.canonical == canonical) {
                    let id = existing.slot_id;
                    s.explicit_save_finished = true;
                    s.event("native_save_reused", [u64::from(id), 0, 0, 0]);
                    reused = true;
                } else if fresh && s.catalog.is_complete()
                    && s.catalog.slots_seen < training_mod_consts::mii::saved::MAX_FILES {
                    // This is the existing catalog reader's supported ceiling,
                    // not an assertion about Smash's native maximum.
                    native::write(context, 0x168, 1i32);
                    create = Some(canonical);
                    s.event("native_save_requested", [inventory::fnv1a64(&row), mode, 0, 0]);
                } else { s.event("native_save_guard_refused", [u64::from(fresh), s.catalog.slots_seen as u64, 0, 0]); }
            } else { s.event("native_save_invalid_draft", [0; 4]); }
        }
    }
    if reused { ui_resume::discard(); }
    let result = original!()(context);
    if let Some(canonical) = create {
        let id = native::read::<u32>(base, native::DRAFT);
        let flags = [u64::from(id),
            u64::from(native::read::<u8>(base, native::DRAFT + 0x1bc)),
            u64::from(native::read::<u8>(base, native::DRAFT + 0x1bd)), 0];
        if let Some(s) = STATE.lock().unwrap().as_mut() {
            s.explicit_save_finished = true;
            s.saved_native = (id != 0).then_some((id, canonical));
            s.event(if id == 0 { "native_save_create_refused" } else { "native_save_allocated" }, flags);
        }
        ui_resume::discard();
    }
    result
}

#[skyline::hook(offset = 0x22fa450)]
unsafe fn editor_release(scene: usize, args: usize) -> usize {
    let ours = EDIT_OWNED.load(Ordering::Acquire) && scene != 0
        && EDIT_SCENE.load(Ordering::Acquire) == scene;
    let base = BASE.load(Ordering::Acquire);
    let flags = if ours { [native::read::<u8>(base, native::DRAFT + 0x1bc) as u64,
        u64::from(native::catalog_clean(base)), native::read::<u8>(base, native::DRAFT + 0x1bd) as u64, 0] }
        else { [0; 4] };
    if ours {
        resume_adapter::before_release(EDIT_CONTEXT.load(Ordering::Acquire));
        // The original can destroy native children. Stop all component
        // association before that call; no old context may be read afterward.
        EDIT_CONTEXT.store(0, Ordering::Release);
    }
    let result = original!()(scene, args);
    if ours {
        // Release has returned; scene/context addresses are not read here.
        RELEASED_SCENE.store(scene, Ordering::Release);
        EDIT_CONTEXT.store(0, Ordering::Release);
        EDIT_LEAF_SCENE.store(0, Ordering::Release);
        EDIT_SCENE.store(0, Ordering::Release);
        EDIT_OWNED.store(false, Ordering::Release);
        EDIT_OWNER.store(0, Ordering::Release);
        if let Ok(mut state) = STATE.try_lock() {
            if let Some(s) = state.as_mut() {
                if let Some(generation) = s.resume_generation { ui_resume::previous_ui_released(generation); }
                s.event("editor_environment_released", flags);
            }
        }
    }
    result
}

/// Native writer work continues in the global update while the drained
/// editor owner waits. No filesystem work or second writer runs here.
unsafe fn save_ready_for_return(base: usize) -> bool {
    if STATE.lock().unwrap().as_ref().is_none_or(|s| s.saved_native.is_none()) { return true; }
    let Some(progress) = native::save_progress(base) else {
        if let Some(s) = STATE.lock().unwrap().as_mut() {
            if s.writer_state != Some([u64::MAX; 4]) {
                s.writer_state = Some([u64::MAX; 4]);
                s.event("native_save_writer_unbound", [0; 4]);
            }
        }
        return false;
    };
    if let Some(s) = STATE.lock().unwrap().as_mut() {
        let values = progress.values();
        if s.writer_state != Some(values) {
            s.writer_state = Some(values);
            s.event("native_save_writer", values);
        }
    }
    progress.idle()
}

/// Existing menu/catalog workers already read native saved files. Verify the
/// new native ID and canonical values there, after the writer-idle return gate.
/// This callback uses owned values only and performs no native memory access.
pub(crate) fn catalog_read(catalog: &Snapshot) {
    let mut state = STATE.lock().unwrap();
    let Some(s) = state.as_mut().filter(|s| s.phase == Phase::Finished) else { return; };
    let Some((id, canonical)) = s.saved_native else { return; };
    if !catalog.is_complete() { return; }
    let found = catalog.configs.iter().any(|c| c.slot_id == id && c.canonical == canonical);
    s.event("native_save_file_observed", [u64::from(id), u64::from(found), catalog.slots_seen as u64, 0]);
    s.saved_native = None;
}

pub(crate) fn init() {
    let base = unsafe { getRegionAddress(Region::Text) as usize };
    if !unsafe { fingerprints::matches(base) } {
        crate::logging::diagnostic(format_args!("[MiiEditor] unsupported native code; disabled"));
        return;
    }
    crate::common::filesystem::init();
    BASE.store(base, Ordering::Release);
    unsafe { native_default::init(base); }
    skyline::install_hooks!(training_ui, training_state, editor_parent, edit_initialized,
        edit_frame, edit_decide, editor_release);
    connected::init();
    lifecycle::init();
    name_keyboard::init();
    resume_adapter::init();
    ENABLED.store(true, Ordering::Release);
}
