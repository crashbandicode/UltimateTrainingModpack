//! Named-macro runtime.
//!
//! The portable recording lives in `training_mod_macros`. This module owns the
//! Training-mode file library, the in-game panel, character requests, and the
//! restore offset. Capture and playback still go through the existing recorder
//! hooks: live playback advances only `INPUT_RECORD_FRAME` inside
//! `set_cpu_controls`. `MacroUi::feed_playback` is the host-tested clock and is
//! not called from these hooks, so a frame is not advanced twice.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

use smash::app::{lua_bind::*, utility, BattleObjectModuleAccessor, Fighter};
use smash::lib::lua_const::*;
use smash::phx::{Hash40, Vector2f, Vector3f};
use training_mod_macros::{InputFrame, Library, MacroRecording, StartState};
use training_mod_sync::*;
use training_mod_tui::macro_panel::{Effect, LibraryRow, MacroUi, Pad, View};

use crate::common::consts::{FighterId, MacroSource, MENU};
use crate::common::input::{Buttons, MappedInputs};
use crate::common::{menu, try_get_module_accessor};
use super::practice;
use crate::training::ui::notifications::notification;

static UI: RwLock<MacroUi> = RwLock::new(MacroUi::new());
static SUSPENDED: AtomicBool = AtomicBool::new(false);

pub fn suspended() -> bool { SUSPENDED.load(Ordering::Acquire) }
pub fn suspend_waiting() {
    if SUSPENDED.swap(true, Ordering::AcqRel) { return; }
    lock_write(&UI).invalidate_practice_context();
    crate::replay_capture::suspend_waiting();
    cancel_character_selection();
    NAME_JOB.lock().unwrap().take();
    SESSION_GENERATION.fetch_add(1, Ordering::Relaxed);
}
pub fn waiting_scene_changed(same_lobby: bool) {
    if !same_lobby && SUSPENDED.swap(false, Ordering::AcqRel) {
        lock_write(&UI).close_transient();
        crate::replay_capture::discard_waiting();
    }
}
pub fn resume_waiting() {
    if super::lobby::active() && practice::enabled()
        && crate::character_roster::waiting_drafts_ready()
        && unsafe { crate::common::is_ready_go() }
        && try_get_module_accessor(FighterId::Player).is_some()
        && try_get_module_accessor(FighterId::CPU).is_some()
    { SUSPENDED.store(false, Ordering::Release); }
}
static GENERATION: AtomicU64 = AtomicU64::new(0);
// A preview's normal stop/restore advances the motion generation while its
// export keyboard is open. Only session cleanup invalidates that naming draft.
static SESSION_GENERATION: AtomicU64 = AtomicU64::new(0);
static NAME_SERIAL: AtomicU64 = AtomicU64::new(1);
// Kept through the requested native install's ordinary session cleanup. True
// scene/destruction retirement remains the existing CPU generation boundary.
static NAMED_SELECTION_REVISION: AtomicU64 = AtomicU64::new(1);
static NAMED_REFUSAL_SHOWN: AtomicU64 = AtomicU64::new(0);
static NAMED_SOURCE_ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) fn selection_revision() -> u64 { NAMED_SELECTION_REVISION.load(Ordering::Acquire) }
pub(crate) fn cancel_character_selection() {
    NAMED_SELECTION_REVISION.fetch_add(1, Ordering::AcqRel);
    NAMED_REFUSAL_SHOWN.store(0, Ordering::Release);
    lock_write(&UI).invalidate_character_request();
}
fn commit_loaded(id:String, recording:MacroRecording, current:Option<i32>) -> Result<Effect,training_mod_macros::MacroError> {
    NAMED_SELECTION_REVISION.fetch_add(1, Ordering::AcqRel);
    NAMED_REFUSAL_SHOWN.store(0, Ordering::Release);
    lock_write(&UI).commit_loaded(id,recording,current)
}
fn exact_target_ready(target:training_mod_consts::mii::replay::Target,current:Option<i32>) -> bool {
    let Some(kind)=current else{return false;};
    if target.configuration().is_none() {return target.ready(kind as u32,None);}
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    {crate::character_roster::mii_replay::named_ready(target,kind as u32)}
    #[cfg(not(feature="mii-ordinary-cpu-alpha"))]
    {false}
}
fn observe_loaded() {
    let target=lock_read(&UI).loaded_target();
    let current=cpu_kind();
    let busy=crate::character_roster::busy();
    let configuration=target.filter(|target|exact_target_ready(*target,current)).and_then(|target|target.configuration());
    lock_write(&UI).observe_exact_cpu(current,configuration,busy);
}
fn submit_loaded_character() {
    if !crate::character_roster::cpu_reload_ready() {return;}
    let target=lock_read(&UI).loaded_target();
    let Some(target)=target else{return;};
    #[cfg(not(feature="mii-ordinary-cpu-alpha"))]
    if target.configuration().is_some() {
        let revision=selection_revision();
        if NAMED_REFUSAL_SHOWN.swap(revision,Ordering::AcqRel)!=revision {
            notify("Exact Mii macro loading is unavailable in this build");
        }
        return;
    }
    if lock_write(&UI).claim_character_request()!=Some(target.kind() as i32) {return;}
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    let accepted=crate::character_roster::mii_replay::request_named(target);
    #[cfg(not(feature="mii-ordinary-cpu-alpha"))]
    let accepted=target.configuration().is_none() && crate::character_roster::request_cpu_kind(target.kind());
    if !accepted {
        // No native request was admitted. A not-yet-qualified owner may become
        // ready after auto-init; don't consume this one submission prematurely.
        lock_write(&UI).invalidate_character_request();
    }
}
static NAME_JOB: std::sync::Mutex<Option<NameJob>> = std::sync::Mutex::new(None);

struct NameJob { id: u64, generation: u64, replay: bool }
// Only flags set by this panel are released, and only on the same live object.
static PANEL_STOPS: RwLock<Vec<(u32, usize, usize)>> = RwLock::new(Vec::new());

pub fn forget_stopped_entry(entry: usize) {
    lock_write(&PANEL_STOPS).retain(|&(_, _, owner)| owner != entry);
}

pub fn forget_stopped_scene() { lock_write(&PANEL_STOPS).clear(); }
static RESTORING: AtomicBool = AtomicBool::new(false);
// 0 = no automatic action, 1 = record, 2 = playback after both fighters restore.
static RESTORE_ACTION: AtomicU8 = AtomicU8::new(0);
static PLAYBACK_START_PENDING: AtomicBool = AtomicBool::new(false);
static PLAYBACK_START_WAITS: AtomicU8 = AtomicU8::new(0);
static RESTORE_POSITION_WAITS: AtomicU8 = AtomicU8::new(0);
static PLAYBACK_OBSERVATIONS: AtomicU8 = AtomicU8::new(0);
const PLAYBACK_OBSERVATION_LIMIT: u8 = 8;
#[cfg(feature = "named-input-trace")]
static TRACE_RUN: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "named-input-trace")]
static TRACE_WRITES: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "named-input-trace")]
static TRACE_OBSERVATIONS: AtomicU64 = AtomicU64::new(0);
static RESTORE_POSE: RwLock<Option<RestorePose>> = RwLock::new(None);

#[derive(Clone, Copy)]
struct RestorePose {
    object: u32,
    address: usize,
    x: f32,
    y: f32,
    facing: f32,
    air: Option<bool>,
}

struct Motion {
    capturing: bool,
    playing: bool,
    captured: Vec<InputFrame>,
    captured_len: usize,
    play: Vec<InputFrame>,
    start: StartState,
}

static MOTION: RwLock<Motion> = RwLock::new(Motion {
    capturing: false,
    playing: false,
    captured: Vec::new(),
    captured_len: 0,
    play: Vec::new(),
    start: StartState {
        fighter_kind: -1,
        costume: 0,
        facing: 1.0,
        situation: 0,
        status: 0,
        speed_x: 0.0,
        speed_y: 0.0,
    },
});

struct Legacy {
    facing: Option<f32>,
    status: Option<i32>,
}

static LEGACY: RwLock<Legacy> = RwLock::new(Legacy {
    facing: None,
    status: None,
});

pub struct PlaybackStart {
    pub facing: f32,
    pub status: i32,
    pub len: usize,
}

fn notify(message: &str) {
    notification("Named Macro".to_owned(), message.to_owned(), 180);
}

fn macros_dir() -> String {
    format!("{}/macros", training_mod_consts::TRAINING_MODPACK_ROOT)
}

fn library() -> Library {
    Library::new(macros_dir())
}

fn remember(id: &str) {
    let dir = macros_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = std::fs::write(format!("{dir}/selected.txt"), id);
}

fn game_version() -> String {
    crate::common::SMASH_VERSION.clone()
}

pub fn source_is_named() -> bool {
    read(&MENU).macro_source == MacroSource::NAMED
}

pub fn panel_visible() -> bool {
    !suspended() && (lock_read(&UI).is_open() || crate::replay_capture::panel_visible()
        || crate::replay_capture::browser_visible())
}

pub fn panel_view() -> Option<View> {
    if suspended() { return None; }
    let ui = lock_read(&UI);
    ui.native_name_view().or_else(|| ui.view())
        .or_else(crate::replay_capture::browser_view)
        .or_else(crate::replay_capture::loading_view)
}

pub fn clip_foreground() -> bool {
    !suspended() && !lock_read(&UI).is_open() && crate::replay_capture::panel_visible()
        && !naming_pending() && !crate::native_keyboard::busy()
}

pub fn naming_pending() -> bool { NAME_JOB.lock().unwrap().is_some() }

fn request_native_name() {
    if crate::native_keyboard::busy() || naming_pending() { return; }
    let Some(initial) = lock_read(&UI).name_prompt().map(str::to_owned) else { return };
    let id = NAME_SERIAL.fetch_add(1, Ordering::Relaxed);
    let job = NameJob { id, generation: SESSION_GENERATION.load(Ordering::Relaxed),
        replay: crate::replay_capture::panel_visible() };
    *NAME_JOB.lock().unwrap() = Some(job);
    if let Err(error) = crate::native_keyboard::request(id, "Name macro", &initial) {
        NAME_JOB.lock().unwrap().take();
        notify(&error);
    }
}

/// Poll outside input mapping and without holding UI/game locks across the applet.
pub fn service_keyboard() {
    crate::native_keyboard::service();
    let Some((id, result)) = crate::native_keyboard::poll() else { return };
    // A stale completion may race a new scene's request. Never retire the newer
    // job just because the old applet finally returned.
    let job = {
        let mut pending = NAME_JOB.lock().unwrap();
        if pending.as_ref().is_some_and(|job| job.id == id) { pending.take() } else { None }
    };
    let Some(job) = job else { return };
    crate::logging::diagnostic(format_args!("[NativeKeyboard] consume id={} replay={}", id, job.replay));
    if job.id != id || job.generation != SESSION_GENERATION.load(Ordering::Relaxed)
        || !practice::enabled() || suspended()
    { return; }
    // The closing applet's confirm/cancel must not become a gameplay input.
    super::frame_counter::reset_frame_count(*menu::MENU_CLOSE_FRAME_COUNTER);
    super::frame_counter::start_counting(*menu::MENU_CLOSE_FRAME_COUNTER);
    match result {
        crate::native_keyboard::KeyboardResult::Confirmed(name) => {
            let effect = lock_write(&UI).confirm_native_name(name);
            // Only a valid confirmation closes the clip. A failed write restores
            // the naming draft; the selected clip remains available until success.
            apply_effect(effect);
        }
        crate::native_keyboard::KeyboardResult::Cancelled => {
            if job.replay { lock_write(&UI).on_pad(Pad::Back); }
        }
        crate::native_keyboard::KeyboardResult::Failed(code) => {
            notify(&format!("System keyboard failed ({code:#X}); draft kept"));
        }
    }
    assign(&menu::MENU_RECEIVED_INPUT, true);
    crate::logging::diagnostic(format_args!("[NativeKeyboard] applied id={}", id));
}

pub fn capture_active() -> bool {
    lock_read(&MOTION).capturing
}

pub fn playback_active() -> bool {
    lock_read(&MOTION).playing
}

pub fn using_frames() -> bool {
    let motion = lock_read(&MOTION);
    motion.capturing || motion.playing
}

pub fn init() {
    let path = format!("{}/selected.txt", macros_dir());
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let id = text.trim();
    if id.is_empty() {
        return;
    }
    let Ok(recording) = library().load(id) else {
        return;
    };
    let _ = commit_loaded(id.to_owned(), recording, None);
}

/// Save a validated recording from the later replay-export stream and select it.
/// There is no replay browser here. The caller does not exist in this crate yet.
#[allow(dead_code)]
pub fn accept_recording(recording: MacroRecording) -> Result<String, String> {
    recording.validate().map_err(|err| err.to_string())?;
    let id = library()
        .save_new(&recording)
        .map_err(|err| err.to_string())?;
    remember(&id);
    let effect = commit_loaded(id.clone(), recording, cpu_kind())
        .map_err(|err| err.to_string())?;
    apply_effect(effect);
    Ok(id)
}

pub fn on_pad(pad: Pad) {
    if naming_pending() || crate::native_keyboard::busy() { return; }
    if lock_read(&UI).name_prompt().is_some() {
        match pad {
            Pad::Confirm => request_native_name(),
            Pad::Back => { lock_write(&UI).on_pad(Pad::Back); }
            Pad::Extra => {
                let effect = lock_write(&UI).native_name_placement();
                apply_effect(effect);
            }
            _ => {}
        }
        assign(&menu::MENU_RECEIVED_INPUT, true);
        return;
    }
    if crate::replay_capture::browser_visible() {
        crate::replay_capture::browser_input(pad);
        return;
    }
    if crate::replay_capture::panel_visible() {
        crate::replay_capture::on_pad(pad);
        return;
    }
    let effect = lock_write(&UI).on_pad(pad);
    apply_effect(effect);
    assign(&menu::MENU_RECEIVED_INPUT, true);
}

pub fn open_library() {
    if !practice_ready() {
        return;
    }
    let listing = match library().list() {
        Ok(listing) => listing,
        Err(err) => {
            notify(&err.to_string());
            return;
        }
    };
    let skipped = listing.skipped.len();
    let rows = listing
        .entries
        .into_iter()
        .map(|entry| LibraryRow {
            id: entry.id,
            label: format!("{} · {}", entry.name, fighter_label(entry.fighter_kind)),
        })
        .collect();
    lock_write(&UI).open_library(rows, skipped);
    assign(&menu::MENU_RECEIVED_INPUT, true);
}

pub fn open_editor() {
    if !practice_ready() {
        return;
    }
    if !lock_write(&UI).open_editor(None) {
        notify("Load a macro and restore a Training state first");
        return;
    }
    assign(&menu::MENU_RECEIVED_INPUT, true);
}

fn practice_ready() -> bool {
    if !practice::enabled() || !unsafe { practice::acquire() } {
        notify("Wait for both practice fighters");
        return false;
    }
    if crate::training::recording_or_playback()
        || unsafe { crate::training::save_states::is_loading() }
    {
        notify("Finish the current practice action first");
        return false;
    }
    true
}

pub unsafe fn snapshot_start(module_accessor: &mut BattleObjectModuleAccessor) -> StartState {
    StartState {
        fighter_kind: utility::get_kind(module_accessor),
        costume: WorkModule::get_int(module_accessor, *FIGHTER_INSTANCE_WORK_ID_INT_COLOR) as u32,
        facing: PostureModule::lr(module_accessor),
        situation: StatusModule::situation_kind(module_accessor),
        status: StatusModule::status_kind(module_accessor),
        speed_x: KineticModule::get_sum_speed_x(
            module_accessor,
            *KINETIC_ENERGY_RESERVE_ATTRIBUTE_MAIN,
        ),
        speed_y: KineticModule::get_sum_speed_y(
            module_accessor,
            *KINETIC_ENERGY_RESERVE_ATTRIBUTE_MAIN,
        ),
    }
}

pub fn begin_capture(start: StartState) {
    let mut motion = lock_write(&MOTION);
    motion.capturing = true;
    motion.playing = false;
    motion.captured.clear();
    motion.captured_len = 0;
    motion.play.clear();
    motion.start = start;
}

pub unsafe fn capture_start(module_accessor: &mut BattleObjectModuleAccessor) -> StartState {
    let start = snapshot_start(module_accessor);
    lock_write(&MOTION).start = start;
    start
}

pub fn has_captured_frame(index: usize) -> bool {
    index < lock_read(&MOTION).captured_len
}

pub fn store_frame(index: usize, input: &MappedInputs) {
    if index >= training_mod_macros::MAX_FRAMES {
        return;
    }
    let frame = InputFrame {
        buttons: input.buttons.bits(),
        lstick_x: input.lstick_x,
        lstick_y: input.lstick_y,
        rstick_x: input.rstick_x,
        rstick_y: input.rstick_y,
    };
    let mut motion = lock_write(&MOTION);
    if !motion.capturing {
        return;
    }
    if motion.captured.len() <= index {
        motion.captured.resize(index + 1, InputFrame::default());
    }
    motion.captured[index] = frame;
    motion.captured_len = motion.captured_len.max(index + 1);
}

pub fn frame_at(index: usize) -> MappedInputs {
    let motion = lock_read(&MOTION);
    let frames = if motion.playing {
        motion.play.as_slice()
    } else {
        motion.captured.as_slice()
    };
    let frame = frames.get(index).copied().unwrap_or_default();
    MappedInputs {
        buttons: Buttons::from_bits_truncate(frame.buttons),
        lstick_x: frame.lstick_x,
        lstick_y: frame.lstick_y,
        rstick_x: frame.rstick_x,
        rstick_y: frame.rstick_y,
    }
}

pub fn finish_capture(crop: bool) {
    let (frames, start) = {
        let mut motion = lock_write(&MOTION);
        motion.capturing = false;
        let frames = motion.captured[..motion.captured_len].to_vec();
        (frames, motion.start)
    };
    let version = game_version();
    crate::logging::diagnostic(format_args!(
        "[PracticeRecording] named_recording_complete captured_frames={} crop={}", frames.len(), crop
    ));
    let effect = lock_write(&UI).offer_save(frames, start, crop, &version);
    apply_effect(effect);
    request_native_name();
    if panel_visible() {
        assign(&menu::MENU_RECEIVED_INPUT, true);
    }
}

pub fn begin_playback() -> Option<PlaybackStart> {
    observe_loaded();
    let ui = lock_read(&UI);
    let (facing, status, frames) = ui.copy_playback()?;
    let info = PlaybackStart {
        facing,
        status,
        len: frames.len(),
    };
    drop(ui);
    let mut motion = lock_write(&MOTION);
    motion.playing = true;
    motion.capturing = false;
    motion.play = frames;
    #[cfg(feature = "named-input-trace")]
    {
        TRACE_WRITES.store(0, Ordering::Relaxed);
        TRACE_OBSERVATIONS.store(0, Ordering::Relaxed);
        let run = TRACE_RUN.fetch_add(1, Ordering::Relaxed) + 1;
        println!("[NamedTrace] begin run={} len={}", run, info.len);
    }
    Some(info)
}

/// Transient preview bypasses MacroUi and never replaces the user's loaded entry.
pub fn begin_preview(recording: MacroRecording) -> PlaybackStart {
    let info = PlaybackStart { facing: recording.start.facing, status: recording.start.status,
        len: recording.frames.len() };
    let mut motion = lock_write(&MOTION);
    motion.playing = true;
    motion.capturing = false;
    motion.start = recording.start;
    motion.play = recording.frames;
    PLAYBACK_START_WAITS.store(0, Ordering::Relaxed);
    PLAYBACK_START_PENDING.store(true, Ordering::Relaxed);
    #[cfg(feature = "named-input-trace")]
    {
        TRACE_WRITES.store(0, Ordering::Relaxed);
        TRACE_OBSERVATIONS.store(0, Ordering::Relaxed);
        let run = TRACE_RUN.fetch_add(1, Ordering::Relaxed) + 1;
        println!("[NamedTrace] preview run={} len={}", run, info.len);
    }
    info
}

/// Qualification only: the native getter observation is independent of this write.
/// Never use the persistent diagnostic logger for per-frame samples.
#[cfg(feature = "named-input-trace")]
pub fn trace_write(index: usize, input: &MappedInputs, buffered: bool, mirror: f32) {
    if TRACE_WRITES.load(Ordering::Relaxed) >= training_mod_macros::MAX_FRAMES as u64 { return; }
    let seq = TRACE_WRITES.fetch_add(1, Ordering::Relaxed) + 1;
    println!(
        "[NamedTrace] w run={} seq={} idx={} buttons={:#x} lx={} ly={} rx={} ry={} buffered={} mirror={}",
        TRACE_RUN.load(Ordering::Relaxed), seq, index, input.buttons.bits(),
        input.lstick_x, input.lstick_y, input.rstick_x, input.rstick_y,
        u8::from(buffered), mirror
    );
}

pub fn end_playback() {
    PLAYBACK_OBSERVATIONS.store(0, Ordering::Relaxed);
    lock_write(&MOTION).playing = false;
}

pub fn cancel_motion() {
    PLAYBACK_START_PENDING.store(false, Ordering::Relaxed);
    PLAYBACK_START_WAITS.store(0, Ordering::Relaxed);
    RESTORE_POSITION_WAITS.store(0, Ordering::Relaxed);
    PLAYBACK_OBSERVATIONS.store(0, Ordering::Relaxed);
    let mut motion = lock_write(&MOTION);
    motion.capturing = false;
    motion.playing = false;
}

pub fn remember_legacy(facing: f32, status: i32) {
    let mut legacy = lock_write(&LEGACY);
    if legacy.facing.is_none() {
        legacy.facing = Some(facing);
        legacy.status = Some(status);
    }
}

pub fn take_legacy() -> Option<(f32, i32)> {
    let mut legacy = lock_write(&LEGACY);
    match (legacy.facing.take(), legacy.status.take()) {
        (Some(facing), Some(status)) => Some((facing, status)),
        _ => {
            legacy.facing = None;
            legacy.status = None;
            None
        }
    }
}

/// Close the panel and stop playback. The loaded recording stays, because a
/// character install calls session cleanup as part of the swap this macro asked for.
pub fn clear_session() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
    SESSION_GENERATION.fetch_add(1, Ordering::Relaxed);
    NAME_JOB.lock().unwrap().take();
    if !suspended() {
        lock_write(&UI).close_transient();
        crate::replay_capture::close_editor();
    }
    cancel_motion();
    RESTORING.store(false, Ordering::Relaxed);
    RESTORE_ACTION.store(0, Ordering::Relaxed);
    *lock_write(&RESTORE_POSE) = None;
}

pub fn begin_restore() {
    *lock_write(&RESTORE_POSE) = None;
    PLAYBACK_START_PENDING.store(false, Ordering::Relaxed);
    RESTORE_POSITION_WAITS.store(0, Ordering::Relaxed);
    GENERATION.fetch_add(1, Ordering::Relaxed);
    RESTORING.store(true, Ordering::Relaxed);
    if crate::replay_preview::occupied() {
        RESTORE_ACTION.store(0, Ordering::Relaxed);
    } else if source_is_named() && RESTORE_ACTION.load(Ordering::Relaxed) == 0 {
        let settings = read(&MENU);
        let action = if settings.record_trigger.contains(&training_mod_consts::RecordTrigger::SAVESTATE) {
            1
        } else if settings.save_state_playback.get_random() != training_mod_consts::PlaybackSlot::empty() {
            2
        } else { 0 };
        RESTORE_ACTION.store(action, Ordering::Relaxed);
    }
}

pub unsafe fn request_playback() {
    observe_loaded();
    if !lock_read(&UI).playback_allowed() {
        notify("Load a named macro and wait for its fighter");
        return;
    }
    super::input_record::stop_playback();
    match super::save_states::request_macro_restore() {
        Ok(()) => RESTORE_ACTION.store(2, Ordering::Relaxed),
        Err(message) => notify(message),
    }
}

/// Bound the earlier CPU position/status retry too: preparation cannot run while
/// this phase is pending. On failure, suppress playback and let the save-state
/// caller finish its normal charge/buff/partner cleanup without further re-pins.
pub unsafe fn position_wait_timed_out(status: i32, target: i32) -> bool {
    if !source_is_named() || !RESTORING.load(Ordering::Relaxed)
        || RESTORE_ACTION.load(Ordering::Relaxed) != 2
    {
        return false;
    }
    let waits = RESTORE_POSITION_WAITS.fetch_add(1, Ordering::Relaxed);
    if waits == 0 {
        crate::logging::diagnostic(format_args!(
            "[PracticeRecording] named_restore_wait phase=position status={} target={}", status, target
        ));
    }
    if waits < 60 { return false; }
    RESTORING.store(false, Ordering::Relaxed);
    RESTORE_ACTION.store(0, Ordering::Relaxed);
    *lock_write(&RESTORE_POSE) = None;
    super::input_record::stop_playback();
    crate::logging::diagnostic(format_args!(
        "[PracticeRecording] named_restore_failed phase=position status={} target={}", status, target
    ));
    notify("Start position could not settle; adjust placement and retry");
    true
}

/// Run after the CPU's restore callback. No inputs start while either fighter
/// still has charge/buff/character restoration work pending.
pub unsafe fn service_restore(_module_accessor: &mut BattleObjectModuleAccessor) {
    if !RESTORING.load(Ordering::Relaxed) || super::save_states::is_loading() {
        return;
    }
    RESTORING.store(false, Ordering::Relaxed);
    if crate::replay_preview::occupied() {
        RESTORE_ACTION.store(0, Ordering::Relaxed);
        // Native preview starts after this callback and still needs this target
        // at input zero, after BOTH fighters have completed restoration.
        return;
    }
    match RESTORE_ACTION.swap(0, Ordering::Relaxed) {
        1 => super::input_record::start_named_lockout(),
        2 => {
            if super::input_record::playback_named() {
                // The CPU may have finished restoring before the player. Reapply
                // its saved target and speed at the actual input-0 boundary.
                PLAYBACK_START_WAITS.store(0, Ordering::Relaxed);
                PLAYBACK_START_PENDING.store(true, Ordering::Relaxed);
                return;
            }
        }
        _ => {}
    }
    *lock_write(&RESTORE_POSE) = None;
}

/// Remember values, not a live-position anchor. The accessor identity prevents a
/// pending restore from being applied to a replacement fighter.
pub fn remember_restore_pose(
    module_accessor: &mut BattleObjectModuleAccessor,
    x: f32,
    y: f32,
    facing: f32,
    air: Option<bool>,
) {
    if crate::replay_preview::start_context().is_some()
        || (source_is_named() && loaded_fighter_matches() && !crate::replay_preview::occupied()) {
        *lock_write(&RESTORE_POSE) = Some(RestorePose {
            object: module_accessor.battle_object_id,
            address: module_accessor as *mut BattleObjectModuleAccessor as usize,
            x,
            y,
            facing,
            air,
        });
    }
}

/// Called immediately before named input 0. A status transition may need another
/// callback; until then no input is emitted and the recorder clock stays at zero.
pub unsafe fn prepare_playback_start(module_accessor: &mut BattleObjectModuleAccessor) -> bool {
    if !PLAYBACK_START_PENDING.load(Ordering::Relaxed) {
        return true;
    }
    if !crate::replay_preview::occupied() && source_is_named() && !loaded_fighter_matches() {
        super::input_record::stop_playback();
        notify("Wait for the exact macro fighter before playback");
        return false;
    }
    let pose = *lock_read(&RESTORE_POSE);
    let Some(pose) = pose.filter(|pose| {
        pose.object == module_accessor.battle_object_id
            && pose.address == module_accessor as *mut BattleObjectModuleAccessor as usize
    }) else {
        PLAYBACK_START_PENDING.store(false, Ordering::Relaxed);
        super::input_record::stop_playback();
        notify("Restore the Training setup again before playback");
        return false;
    };
    let Some(air) = pose.air else {
        super::input_record::stop_playback();
        notify("Use an idle/fall saved setup for named playback");
        return false;
    };
    KineticModule::clear_speed_all(module_accessor);
    PostureModule::set_pos(module_accessor, &Vector3f { x: pose.x, y: pose.y, z: 0.0 });
    PostureModule::set_lr(module_accessor, pose.facing);
    let target_status = if air { *FIGHTER_STATUS_KIND_FALL } else { *FIGHTER_STATUS_KIND_WAIT };
    if StatusModule::status_kind(module_accessor) != target_status {
        let waits = PLAYBACK_START_WAITS.fetch_add(1, Ordering::Relaxed);
        if waits == 0 {
            crate::logging::diagnostic(format_args!(
                "[PracticeRecording] named_playback_wait status={} target={}",
                StatusModule::status_kind(module_accessor), target_status
            ));
        }
        if waits >= 60 {
            super::input_record::stop_playback();
            notify("Start position could not settle; adjust placement and retry");
            return false;
        }
        StatusModule::change_status_request(module_accessor, target_status, false);
        return false;
    }
    assign(&super::input_record::CURRENT_LR, pose.facing);
    apply_ready_speed(module_accessor, air);
    PLAYBACK_START_PENDING.store(false, Ordering::Relaxed);
    PLAYBACK_OBSERVATIONS.store(PLAYBACK_OBSERVATION_LIMIT, Ordering::Relaxed);
    *lock_write(&RESTORE_POSE) = None;
    let start = snapshot_start(module_accessor);
    crate::logging::diagnostic(format_args!(
        "[PracticeRecording] named_playback_start x={} y={} facing={} situation={} status={} speed_x={} speed_y={}",
        PostureModule::pos_x(module_accessor), PostureModule::pos_y(module_accessor),
        start.facing, start.situation, start.status, start.speed_x, start.speed_y
    ));
    true
}

unsafe fn observation_fighter(module_accessor: &mut BattleObjectModuleAccessor) -> bool {
    practice::primary_cpu(module_accessor)
}

/// Pair the pre-original boundary with the post-original observations below.
/// This measures input/state ordering rather than inferring it from clock names.
pub unsafe fn observe_playback_before(module_accessor: &mut BattleObjectModuleAccessor) {
    if PLAYBACK_OBSERVATIONS.load(Ordering::Relaxed) == 0
        && !(PLAYBACK_START_PENDING.load(Ordering::Relaxed)
            && PLAYBACK_START_WAITS.load(Ordering::Relaxed) == 0)
    { return; }
    if !observation_fighter(module_accessor) { return; }
    let start = snapshot_start(module_accessor);
    crate::logging::diagnostic(format_args!(
        "[PracticeRecording] named_playback_before next_input={} x={} y={} status={} speed_x={} speed_y={}",
        read(&super::input_record::INPUT_RECORD_FRAME), PostureModule::pos_x(module_accessor),
        PostureModule::pos_y(module_accessor), start.status, start.speed_x, start.speed_y
    ));
}

/// Eight primary-CPU post-fighter observations per restored start, bounded by the
/// diagnostics logger. These distinguish applied energy from actual later motion.
pub unsafe fn observe_playback_start(module_accessor: &mut BattleObjectModuleAccessor) {
    #[cfg(feature = "named-input-trace")]
    if playback_active() && observation_fighter(module_accessor)
        && TRACE_WRITES.load(Ordering::Relaxed) > 0
        && TRACE_OBSERVATIONS.load(Ordering::Relaxed) < training_mod_macros::MAX_FRAMES as u64
    {
        let sample = TRACE_OBSERVATIONS.fetch_add(1, Ordering::Relaxed) + 1;
        println!(
            "[NamedTrace] o run={} seq={} sample={} next={} button={:#x} sx={} sy={} ncx={} ncy={} subx={} suby={}",
            TRACE_RUN.load(Ordering::Relaxed), TRACE_WRITES.load(Ordering::Relaxed), sample,
            read(&super::input_record::INPUT_RECORD_FRAME), ControlModule::get_button(module_accessor),
            ControlModule::get_stick_x(module_accessor), ControlModule::get_stick_y(module_accessor),
            ControlModule::get_stick_x_no_clamp(module_accessor), ControlModule::get_stick_y_no_clamp(module_accessor),
            ControlModule::get_sub_stick_x(module_accessor), ControlModule::get_sub_stick_y(module_accessor)
        );
    }
    let remaining = PLAYBACK_OBSERVATIONS.load(Ordering::Relaxed);
    if remaining == 0 || !observation_fighter(module_accessor) { return; }
    PLAYBACK_OBSERVATIONS.store(remaining - 1, Ordering::Relaxed);
    let start = snapshot_start(module_accessor);
    crate::logging::diagnostic(format_args!(
        "[PracticeRecording] named_playback_observed sample={} next_input={} x={} y={} situation={} status={} speed_x={} speed_y={} buttons={} raw_x={} raw_y={}",
        PLAYBACK_OBSERVATION_LIMIT + 1 - remaining, read(&super::input_record::INPUT_RECORD_FRAME),
        PostureModule::pos_x(module_accessor), PostureModule::pos_y(module_accessor),
        start.situation, start.status, start.speed_x, start.speed_y,
        ControlModule::get_button(module_accessor), ControlModule::get_stick_x_no_clamp(module_accessor),
        ControlModule::get_stick_y_no_clamp(module_accessor)
    ));
}

pub fn restored_target(x: f32, y: f32) -> Option<(f32, f32)> {
    if crate::replay_preview::occupied() { return None; }
    if panel_visible() {
        return None;
    }
    let generation = GENERATION.load(Ordering::Relaxed);
    if generation == 0 {
        return None;
    }
    let target = lock_write(&UI).restored_target(generation, (x, y));
    if source_is_named() && loaded_fighter_matches() { target } else { None }
}

fn loaded_fighter_matches() -> bool {
    let target=lock_read(&UI).loaded_target();
    target.is_some_and(|target|exact_target_ready(target,cpu_kind()))
}

/// Recorded aerial motion must survive even at zero vertical offset.
pub fn restore_in_air() -> bool {
    if crate::replay_preview::occupied() {
        return crate::replay_preview::start_context().is_some_and(|start| start.situation == *SITUATION_KIND_AIR);
    }
    source_is_named() && loaded_fighter_matches()
        && lock_read(&UI).restore_in_air()
}

pub fn offer_replay_recording(recording: MacroRecording) {
    if let Err(error) = recording.validate() {
        notify(&error.to_string());
        return;
    }
    lock_write(&UI).restore_draft(recording);
    assign(&menu::MENU_RECEIVED_INPUT, true);
    request_native_name();
}

unsafe fn apply_ready_speed(
    module_accessor: &mut BattleObjectModuleAccessor,
    air: bool,
) {
    let (x, y, facing) = if let Some(start) = crate::replay_preview::start_context() {
        (start.speed_x, start.speed_y, start.facing)
    } else {
        if crate::replay_preview::occupied() || !source_is_named() || panel_visible() || !loaded_fighter_matches() {
            return;
        }
        let ui = lock_read(&UI);
        let Some((x, y)) = ui.ready_velocity(true) else { return; };
        (x, y, ui.loaded_facing().unwrap_or(1.0))
    };
    // Match the recorder's facing-relative input transform; placement remains a
    // world-unit delta from the already mirrored save-state anchor.
    let x = x * facing * PostureModule::lr(module_accessor);
    if air {
        set_normal_speed(module_accessor, *FIGHTER_KINETIC_ENERGY_ID_CONTROL, x, 0.0);
        set_gravity_speed(module_accessor, y);
    } else {
        set_normal_speed(module_accessor, *FIGHTER_KINETIC_ENERGY_ID_MOTION, x, 0.0);
    }
}

unsafe fn set_normal_speed(
    module_accessor: &mut BattleObjectModuleAccessor,
    energy_id: i32,
    x: f32,
    y: f32,
) {
    let energy = KineticModule::get_energy(module_accessor, energy_id)
        as *mut smash::app::KineticEnergyNormal;
    if energy.is_null() {
        return;
    }
    let speed = Vector2f { x, y };
    KineticEnergyNormal::set_speed(energy, &speed);
}

unsafe fn set_gravity_speed(module_accessor: &mut BattleObjectModuleAccessor, y: f32) {
    let energy = KineticModule::get_energy(module_accessor, *FIGHTER_KINETIC_ENERGY_ID_GRAVITY)
        as *mut smash::app::FighterKineticEnergyGravity;
    if energy.is_null() {
        return;
    }
    FighterKineticEnergyGravity::set_speed(energy, y);
}

pub fn service_character() {
    let named=source_is_named();
    if !named && NAMED_SOURCE_ACTIVE.swap(false,Ordering::AcqRel) {
        cancel_character_selection();
    } else if named { NAMED_SOURCE_ACTIVE.store(true,Ordering::Release); }
    // Persisted selection exists before the stage is playable. Native Training
    // mode and an observed entry alone also hold during the initial load screen.
    if !practice::enabled() || suspended() || unsafe { !Fighter::is_ready_go() }
        || panel_visible() || crate::replay_capture::loading() || !source_is_named()
    {
        return;
    }
    if crate::training::recording_or_playback()
        || unsafe { crate::training::save_states::is_loading() }
    {
        return;
    }
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    {
        let target=lock_read(&UI).loaded_target();
        if let Some(reason)=target.and_then(crate::character_roster::mii_replay::named_refusal) {
            let revision=selection_revision();
            if NAMED_REFUSAL_SHOWN.swap(revision,Ordering::AcqRel)!=revision {notify(reason);}
            return;
        }
    }
    observe_loaded();
    if crate::character_roster::busy() {return;}
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    {
        let target=lock_read(&UI).loaded_target();
        if let Some(target)=target {
            crate::character_roster::mii_replay::reacknowledge_named(target);
        }
    }
    submit_loaded_character();
}

pub unsafe fn preview(module_accessor: &mut BattleObjectModuleAccessor) {
    let Some((x, y)) = lock_read(&UI).marker() else {
        return;
    };
    draw_cross(module_accessor, x, y);
}

pub unsafe fn hold_panel_fighter(module_accessor: &mut BattleObjectModuleAccessor) {
    let id = module_accessor.battle_object_id;
    let pointer = module_accessor as *mut BattleObjectModuleAccessor as usize;
    let mut owned = lock_write(&PANEL_STOPS);
    let found = owned.iter().position(|&(object, address, _)| object == id && address == pointer);
    // Keep P1 held while the editor is parked for a CPU reload. The new CPU must
    // remain free to finish its native install and land before preview acquisition.
    let hold_panel = panel_visible() || (crate::replay_capture::loading()
        && WorkModule::get_int(module_accessor, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID)
            == FighterId::Player as i32);
    let entry = if hold_panel && !crate::replay_preview::needs_native_update(module_accessor) {
        practice::panel_entry(module_accessor)
    } else { None };
    if let Some(entry) = entry {
        if found.is_none() && !StopModule::is_special_stop(module_accessor) {
            StopModule::set_special_stop(module_accessor, true);
            owned.retain(|&(object, _, _)| object != id);
            owned.push((id, pointer, entry));
        }
    } else if let Some(index) = found {
        StopModule::set_special_stop(module_accessor, false);
        owned.remove(index);
    }
}

unsafe fn draw_cross(module_accessor: &mut BattleObjectModuleAccessor, x: f32, y: f32) {
    // Same spark the fast-fall cue uses. Killing the kind also clears other
    // flashes of this effect while the placement editor is open.
    EffectModule::kill_kind(
        module_accessor,
        Hash40::new("sys_smash_flash_s"),
        false,
        true,
    );
    let rotation = Vector3f {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    for (dx, dy) in [(0.0, 0.0), (2.5, 0.0), (-2.5, 0.0), (0.0, 2.5), (0.0, -2.5)] {
        let pos = Vector3f {
            x: x + dx,
            y: y + dy,
            z: 0.0,
        };
        EffectModule::req(
            module_accessor,
            Hash40::new("sys_smash_flash_s"),
            &pos,
            &rotation,
            0.45,
            0,
            0,
            true,
            *EFFECT_SUB_ATTRIBUTE_CONCLUDE_STATUS,
        );
    }
}

fn apply_effect(effect: Effect) {
    match effect {
        Effect::None => {}
        Effect::Notify(text) => notify(&text),
        Effect::Save(recording) => save_new(*recording),
        Effect::Overwrite { id, recording } => overwrite(id, *recording),
        Effect::Choose(id) => load_id(&id),
        Effect::NeedCharacter(kind) => request_kind(kind),
    }
}

fn save_new(recording: MacroRecording) {
    match library().save_new(&recording) {
        Ok(id) => {
            crate::replay_capture::close_editor();
            remember(&id);
            let name = recording.name.clone();
            let result = commit_loaded(id, recording, cpu_kind());
            match result {
                Ok(effect) => {
                    notify(&format!("Saved {name}"));
                    apply_effect(effect);
                }
                Err(err) => notify(&err.to_string()),
            }
        }
        Err(err) => {
            let text = err.to_string();
            lock_write(&UI).restore_draft(recording);
            assign(&menu::MENU_RECEIVED_INPUT, true);
            notify(&text);
        }
    }
}

fn overwrite(id: String, recording: MacroRecording) {
    match library().overwrite(&id, &recording) {
        Ok(()) => notify("Placement written"),
        Err(err) => notify(&err.to_string()),
    }
}

fn load_id(id: &str) {
    match library().load(id) {
        Ok(recording) => {
            let name = recording.name.clone();
            remember(id);
            let result = commit_loaded(id.to_owned(), recording, cpu_kind());
            match result {
                Ok(effect) => {
                    notify(&format!("Loaded {name}"));
                    apply_effect(effect);
                }
                Err(err) => notify(&err.to_string()),
            }
        }
        Err(err) => notify(&err.to_string()),
    }
}

fn request_kind(kind: i32) {
    if kind >= 0 {submit_loaded_character();}
}

fn cpu_kind() -> Option<i32> {
    unsafe {
        let cpu = try_get_module_accessor(FighterId::CPU)?;
        Some(utility::get_kind(&mut *cpu))
    }
}

fn fighter_label(kind: i32) -> String {
    if kind < 0 {
        return "CPU".to_owned();
    }
    training_mod_consts::TrioFighter::from_kind(kind as u32)
        .map(|fighter| fighter.name().to_owned())
        .unwrap_or_else(|| format!("0x{kind:X}"))
}
