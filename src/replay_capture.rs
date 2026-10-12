//! Observe the latest native Vault replay without changing its input or simulation.
//!
//! Call `before_frame` / `after_frame` around the existing original fighter-frame
//! callback, outside the Training-only gate. Export uses the same portable macro
//! as manual recording. A replay is selected and played through the native Vault;
//! its captured timeline remains available when returning to Training.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::Mutex;

use smash::app::{self, lua_bind::*, BattleObjectModuleAccessor, Fighter};
use smash::lib::lua_const::*;
use training_mod_macros::{InputFrame, LiveFrame, LiveInput, ReplayFrame, ReplaySession, ReplayTrack,
    StartState, MAX_FRAMES};
use training_mod_tui::{macro_panel::{Effect as MenuEffect, LibraryRow, MacroUi, Pad, View}, clip_panel::{ClipAction, ClipEditor, ClipEffect, ClipView, CloseInputGuard}};

use crate::common::game_mode;

const REPLAY_MODE_13_0_5: u32 = 0x05010100;
// Native calibration: all 4,222 post-frame samples exactly recovered the mapped
// buttons/left axes with this scale. The mapped callback precedes that fighter
// simulation tick; replay itself never calls that callback. AI values may be off-grid.
// The120-scale conversion now lives in LiveInput::project, at the consumer seam.
static ENABLED: AtomicBool = AtomicBool::new(false);
static EDITOR: Mutex<Option<ClipEditor>> = Mutex::new(None);
static BROWSER: Mutex<Option<MacroUi>> = Mutex::new(None);
static REVISION: AtomicU64 = AtomicU64::new(1);
static HOLD: Mutex<(i32, u32, bool)> = Mutex::new((0, 0, false));
static RELOAD: Mutex<Option<Reload>> = Mutex::new(None);
static FAILED_KIND: AtomicI32 = AtomicI32::new(-1);
static CLOSE_INPUT: Mutex<CloseInputGuard> = Mutex::new(CloseInputGuard::new());
#[cfg(feature = "named-input-trace")]
static CLOSE_TRACE_LEFT: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "named-input-trace")]
static CLOSE_TRACE_ID: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "named-input-trace")]
const CLOSE_TRACE_LIMIT: u64 = 600;
struct Reload { editor: ClipEditor, requested: bool, ticks: u32 }
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
    crate::training::ui::notifications::notification("Clip Editor".into(), message, 180);
}

pub fn open_editor() {
    if unsafe { !crate::common::is_native_training_mode() || !Fighter::is_ready_go() }
        || crate::training::recording_or_playback()
        || unsafe { crate::training::save_states::is_loading() }
        || crate::character_roster::busy() || crate::replay_preview::occupied() || loading()
    {
        notify("Open the replay window in idle Training".into());
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
    let mut browser = MacroUi::new();
    browser.open_library(rows, skipped);
    *BROWSER.lock().unwrap() = Some(browser);
    training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
}

fn open_capture(id: &str) -> Result<(), String> {
    let (session, name, previous) = match id {
        "latest" => (
            latest().ok_or("No latest Vault capture")?,
            "Latest Vault capture".to_owned(),
            CAPTURE.lock().unwrap().previous.clone(),
        ),
        "previous" => (
            CAPTURE.lock().unwrap().previous.clone().ok_or("No previous Vault capture")?,
            "Previous Vault capture".to_owned(),
            None,
        ),
        _ => {
            let id = id.strip_prefix("saved:").ok_or("Unknown replay capture")?;
            let saved = crate::replay_store::archive().load(id).map_err(|error| error.to_string())?;
            if saved.game_version != "13.0.5" {
                return Err(format!("Capture is for Smash {}, expected 13.0.5", saved.game_version));
            }
            (saved.session, saved.replay_name, None)
        }
    };
    let result = ClipEditor::new(session, clean_statuses());
    match result {
        Ok(mut editor) => {
            if let Some(cpu) = crate::common::try_get_module_accessor(crate::consts::FighterId::CPU) {
                if !clean_statuses().contains(&unsafe { StatusModule::status_kind(cpu) }) {
                    return Err("Open the clip editor while the CPU is idle or falling".into());
                }
                editor.select_kind(unsafe { app::utility::get_kind(&mut *cpu) });
            }
            *EDITOR.lock().unwrap() = Some(editor.with_name(name).with_previous(previous));
            *BROWSER.lock().unwrap() = None;
            REVISION.fetch_add(1, Ordering::Relaxed);
            *HOLD.lock().unwrap() = (0, 0, false);
            FAILED_KIND.store(-1, Ordering::Relaxed);
            training_mod_sync::assign(&crate::common::menu::QUICK_MENU_ACTIVE, false);
        }
        Err(error) => return Err(error),
    }
    training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
    Ok(())
}

pub fn browser_visible() -> bool {
    BROWSER.lock().unwrap().is_some()
}

pub fn browser_view() -> Option<View> {
    BROWSER.lock().unwrap().as_ref().and_then(MacroUi::view).map(|mut view| {
        view.title = "Replay Captures".into();
        view
    })
}

pub fn browser_input(pad: Pad) {
    if matches!(pad, Pad::Back) {
        close_editor();
        return;
    }
    let effect = {
        let mut browser = BROWSER.lock().unwrap();
        let Some(ui) = browser.as_mut() else { return };
        let effect = ui.on_pad(pad);
        if !ui.is_open() { *browser = None; }
        effect
    };
    match effect {
        MenuEffect::Choose(id) => {
            if let Err(error) = open_capture(&id) {
                open_editor();
                notify(error);
            }
        }
        MenuEffect::Notify(error) => notify(error),
        _ => {}
    }
    training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
}

pub fn panel_visible() -> bool {
    EDITOR.lock().unwrap().is_some()
}

pub fn loading() -> bool { RELOAD.lock().unwrap().is_some() }
pub fn loading_view() -> Option<View> {
    loading().then(|| View { title: "Clip Editor".into(), selected: 0,
        labels: vec!["Loading preview fighter...".into()], footer: "B: close editor".into() })
}
pub fn cancel_reload() {
    let was_loading = RELOAD.lock().unwrap().take().is_some();
    if was_loading { println!("[ClipPreview] reload_cancel"); }
    close_editor_inner(was_loading);
}

/// Only the value-only editor survives its requested CPU install. Native preview
/// cleanup must finish before handing control to the existing character loader.
pub unsafe fn service_reload() {
    if !crate::common::is_native_training_mode() {
        *RELOAD.lock().unwrap() = None;
        if panel_visible() || browser_visible() { close_editor(); }
        return;
    }
    let cpu = crate::common::try_get_module_accessor(crate::consts::FighterId::CPU);
    let current = cpu.map(|cpu| app::utility::get_kind(&mut *cpu));
    let mut reload = RELOAD.lock().unwrap();
    if let Some(pending) = reload.as_mut() {
        let kind = pending.editor.view().fighter_kind;
        pending.ticks = pending.ticks.saturating_add(1);
        // A completed native install can still be falling from its spawn height.
        // Wait for its grounded anchor before giving the editor back the new body.
        // Ordinary editor opens may still deliberately acquire an airborne CPU.
        let grounded_idle = cpu.is_some_and(|cpu|
            StatusModule::status_kind(cpu) == *FIGHTER_STATUS_KIND_WAIT
                && StatusModule::situation_kind(cpu) == *SITUATION_KIND_GROUND);
        if pending.requested && !crate::character_roster::busy() && current == Some(kind)
            && Fighter::is_ready_go() && grounded_idle {
            let pending = reload.take().unwrap();
            *EDITOR.lock().unwrap() = Some(pending.editor);
            REVISION.fetch_add(1, Ordering::Relaxed);
            training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
        } else if pending.ticks > 600 {
            let pending = reload.take().unwrap();
            FAILED_KIND.store(kind, Ordering::Relaxed);
            *EDITOR.lock().unwrap() = Some(pending.editor);
            notify("Preview fighter did not load; selection kept".into());
        } else if !pending.requested {
            // Drop our state lock across the loader's menu/scope guards.
            drop(reload);
            if crate::character_roster::cpu_reload_ready() && crate::character_roster::request_cpu_kind(kind as u32) {
                if let Some(pending) = RELOAD.lock().unwrap().as_mut() { pending.requested = true; }
            }
        }
        return;
    }
    drop(reload);
    if crate::replay_preview::occupied() || !crate::training::named_macros::clip_foreground()
        || !Fighter::is_ready_go() { return; }
    let mut editor = EDITOR.lock().unwrap();
    if editor.as_ref().is_some_and(|editor| current != Some(editor.view().fighter_kind)
        && FAILED_KIND.load(Ordering::Relaxed) != editor.view().fighter_kind) {
        let mut pending = editor.take().unwrap();
        pending.pause();
        *RELOAD.lock().unwrap() = Some(Reload { editor: pending, requested: false, ticks: 0 });
        training_mod_sync::assign(&crate::common::menu::QUICK_MENU_ACTIVE, false);
        training_mod_sync::assign(&crate::common::menu::MENU_RECEIVED_INPUT, true);
    }
}

pub fn clip_view() -> Option<ClipView> {
    EDITOR.lock().unwrap().as_ref().map(ClipEditor::view)
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
    let was_open = EDITOR.lock().unwrap().take().is_some();
    let was_browsing = BROWSER.lock().unwrap().take().is_some();
    crate::replay_preview::request_stop();
    REVISION.fetch_add(1, Ordering::Relaxed);
    *HOLD.lock().unwrap() = (0, 0, false);
    if was_open || was_browsing || was_loading {
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
    on_action(match pad {
        Pad::Confirm => ClipAction::Confirm, Pad::Back => ClipAction::Cancel,
        Pad::Left => ClipAction::Nudge(-1), Pad::Right => ClipAction::Nudge(1),
        Pad::Up => ClipAction::PreviousFighter, Pad::Down => ClipAction::NextFighter,
        Pad::Extra => ClipAction::TogglePlay,
    });
}

pub fn on_action(action: ClipAction) {
    if matches!(action, ClipAction::PreviousFighter | ClipAction::NextFighter | ClipAction::ToggleCapture) {
        FAILED_KIND.store(-1, Ordering::Relaxed);
    }
    // Drop the editor lock before opening the naming panel or notifying the game.
    let (effect, revision) = {
        let mut editor = EDITOR.lock().unwrap();
        let revision = REVISION.fetch_add(1, Ordering::Relaxed) + 1;
        (editor.as_mut().map(|editor| editor.on_action(action)), revision)
    };
    match effect {
        Some(ClipEffect::Preview(recording)) => crate::replay_preview::request_play(*recording, revision),
        Some(ClipEffect::StopPreview) => crate::replay_preview::request_stop(),
        Some(ClipEffect::Close) => close_editor(),
        Some(ClipEffect::Export(recording)) => {
            crate::replay_preview::request_stop();
            *HOLD.lock().unwrap() = (0, 0, false);
            crate::training::named_macros::offer_replay_recording(*recording);
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
    let action = if p.b() { Some(ClipAction::Cancel) }
        else if p.a() { Some(ClipAction::Confirm) }
        else if p.x() { Some(if modifier { ClipAction::ToggleCapture } else { ClipAction::TogglePlay }) }
        else if p.y() { Some(if modifier { ClipAction::UseLeadIn } else { ClipAction::Restart }) }
        else if p.l() || (gcc && p.real_digital_l()) { Some(ClipAction::SelectIn) }
        else if p.r() || (gcc && p.real_digital_r()) { Some(ClipAction::SelectOut) }
        else if p.dpad_up() || p.l_up() || p.r_up() { Some(ClipAction::PreviousFighter) }
        else if p.dpad_down() || p.l_down() || p.r_down() { Some(ClipAction::NextFighter) }
        else if p.dpad_left() || p.l_left() || p.r_left() { Some(ClipAction::Nudge(if modifier { -60 } else { -1 })) }
        else if p.dpad_right() || p.l_right() || p.r_right() { Some(ClipAction::Nudge(if modifier { 60 } else { 1 })) }
        else { None };
    if let Some(action) = action { on_action(action); }
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
            let interval = if hold.1 > 120 { 1 } else if hold.1 > 60 { 2 } else { 4 };
            (hold.1 >= 20 && hold.1.is_multiple_of(interval)).then_some(hold.0 * if hold.2 { 60 } else { 1 })
        }
    };
    if let Some(delta) = delta { on_action(ClipAction::Nudge(delta)); }
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
