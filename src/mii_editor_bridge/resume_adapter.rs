//! Bridges value checkpoints to the owned native editor's current callbacks.
use super::{connected_native as contract, cosmetic_resume, current_thread, lifecycle, name_keyboard, native,
    settings_resume, specials_resume, ui_resume, Origin, Phase, BASE, EDIT_CONTEXT, EDIT_OWNED, STATE, ACTIVE};
use std::sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex};

struct Capture {
    context: usize,
    frames: u16,
    result: Option<Result<Option<specials_resume::Snapshot>, super::lua_vm::Error>>,
}
static CAPTURE: Mutex<Option<Capture>> = Mutex::new(None);

struct Reconstruction {
    context: usize,
    restore: specials_resume::Restore,
    result: Option<Result<(), super::lua_vm::Error>>,
}
static RECONSTRUCTION: Mutex<Option<Reconstruction>> = Mutex::new(None);
static LAST_AUTO_OBSERVATION: AtomicU64 = AtomicU64::new(u64::MAX);

fn note(name: &'static str, values: [u64; 4]) {
    if let Some(s) = STATE.lock().unwrap().as_mut() { s.event(name, values); }
}

fn auto_observation(guard: u64, flags: u64) {
    let key = guard | (flags << 8);
    if LAST_AUTO_OBSERVATION.swap(key, Ordering::Relaxed) != key {
        note("automatic_resume_readiness", [guard, flags, 0, 0]);
    }
}

unsafe fn store(screen: ui_resume::Screen, focus: i32) -> bool {
    let (origin, owner, seed, catalog, restoring, queued) = {
        let state = STATE.lock().unwrap();
        let Some(s) = state.as_ref().filter(|s| matches!(s.origin, Origin::Quickplay | Origin::LocalWireless)
            && s.interrupted && !s.snapshot_saved && s.thread == current_thread()) else { return true; };
        (s.origin, s.owner, s.seed, Arc::clone(&s.catalog), s.restore_ticket, s.phase == Phase::Queued)
    };
    let base = BASE.load(Ordering::Acquire);
    let seed = if queued { seed } else { super::draft::Draft::Ready(native::snapshot_draft(base)) };
    let generation = match origin {
        Origin::Quickplay => lifecycle::generation_for(owner),
        Origin::LocalWireless => super::ldn_lifecycle::generation_for(owner),
        Origin::Training => None,
    };
    let result = generation.ok_or("resume_lobby_owner_missing").and_then(|generation| {
        seed.validate().map_err(|_| "resume_confirmed_draft_invalid")?;
        ui_resume::suspend(generation, ui_resume::Checkpoint { seed, catalog, screen, focus }, restoring)
    });
    *RECONSTRUCTION.lock().unwrap() = None;
    settings_resume::clear();
    cosmetic_resume::clear();
    let mut state = STATE.lock().unwrap();
    if let Some(s) = state.as_mut() {
        // Capture failure must not trap an incoming match. It is a qualification
        // failure, emitted explicitly; never claim a partial capture was exact.
        s.snapshot_saved = true;
        match result {
            Ok(_) => {
                s.resume_generation = generation;
                s.restore_ticket = None;
                s.restore_screen = None;
                if restoring.is_some() {
                    s.event("ui_checkpoint_rearmed", [generation.unwrap(), 0, 0, 0]);
                } else {
                    s.event("ui_checkpoint_suspended", [generation.unwrap(), focus as u64,
                        seed.hash(), 0]);
                    if let super::draft::Draft::NativeDefault(class) = seed {
                        s.event("ui_default_intent_suspended", [generation.unwrap(), class.kind() as u64, 0, 0]);
                    }
                    if let ui_resume::Screen::Specials(snapshot) = screen {
                        s.event("ui_specials_checkpoint_values", [
                            u32::from_le_bytes(snapshot.pending) as u64,
                            snapshot.focus as u64,
                            snapshot.subwindow.map_or(u64::MAX, |sub| u64::from(sub.command)),
                            snapshot.subwindow.map_or(u64::MAX, |sub| u64::from(sub.cursor)),
                        ]);
                    }
                    if let ui_resume::Screen::Settings(snapshot) = screen {
                        s.event("ui_settings_checkpoint_values", [snapshot.row as u64,
                            snapshot.area as u64, 0, 0]);
                    }
                    if let ui_resume::Screen::Headgear(snapshot) | ui_resume::Screen::Outfit(snapshot) = screen {
                        s.event("ui_gear_checkpoint_values", [snapshot.cursor as u64,
                            snapshot.live, snapshot.paired, 0]);
                    }
                    if let ui_resume::Screen::Color(snapshot) = screen {
                        s.event("ui_color_checkpoint_values", [snapshot.cursor as u64,
                            snapshot.paired as u64, 0, 0]);
                    }
                    if let ui_resume::Screen::Voice(snapshot) = screen {
                        s.event("ui_voice_checkpoint_values", [snapshot.cursor as u64,
                            snapshot.voice_type as u64, snapshot.voice_pitch as u64, 0]);
                    }
                }
            }
            Err(reason) => {
                if let Some(ticket) = s.restore_ticket.take() { ui_resume::restore_failed(ticket); }
                s.event("ui_checkpoint_failed", [0; 4]);
                crate::logging::diagnostic(format_args!("[MiiEditor] {reason}"));
            }
        }
    }
    true
}

pub(super) unsafe fn capture_queued() {
    store(ui_resume::Screen::Settings(settings_resume::Snapshot::INITIAL), 0);
}

/// False lets one native update reach the Special Moves coroutine boundary.
/// Once captured, the following callback can complete the native leaf.
pub(super) unsafe fn before_edit(context: usize) -> bool {
    let needed = STATE.lock().unwrap().as_ref().is_some_and(|s| s.interrupted
        && !s.snapshot_saved && s.thread == current_thread());
    if !needed { return true; }
    // Native output mirror only; this does not set or restore Settings focus.
    let focus = native::read::<f32>(context, 0x1b0);
    let focus = if (0.0..=6.0).contains(&focus) && focus.fract() == 0.0 {
        focus as i32
    } else { 0 };
    let current: i32 = native::read(context, 0x148);
    let previous = cosmetic_resume::previous_state(context);
    let screen = match cosmetic_resume::leaf(current, previous) {
        cosmetic_resume::Leaf::Settings => match settings_resume::capture(context) {
            None => return false,
            Some(Ok(Some(snapshot))) => ui_resume::Screen::Settings(snapshot),
            // Let a completed selection/Cancel/Save propagate before capturing.
            Some(Ok(None)) => return false,
            Some(Err(error)) => {
                note("ui_settings_capture_failed", [error.code() as u64, 0, 0, 0]);
                ui_resume::Screen::Settings(settings_resume::Snapshot::INITIAL)
            }
        },
        cosmetic_resume::Leaf::Name => match name_keyboard::suspend(context) {
            Ok(text) => ui_resume::Screen::Name(text),
            Err("keyboard_not_owned") if native::read::<u32>(context, 0x14c) == 0 => return false,
            Err(reason) => {
                note("ui_keyboard_capture_failed", [current as u64, 0, 0, 0]);
                crate::logging::diagnostic(format_args!("[MiiEditor] {reason}"));
                // Preserve the confirmed draft, but do not assert exact resume.
                ui_resume::Screen::Settings(settings_resume::Snapshot::INITIAL)
            }
        },
        cosmetic_resume::Leaf::Headgear | cosmetic_resume::Leaf::Outfit => {
            match cosmetic_resume::capture_gear(context, current) {
                cosmetic_resume::Shot::Wait => return false,
                cosmetic_resume::Shot::Found(snapshot) => match cosmetic_resume::leaf(current, previous) {
                    cosmetic_resume::Leaf::Headgear => ui_resume::Screen::Headgear(snapshot),
                    _ => ui_resume::Screen::Outfit(snapshot),
                },
                cosmetic_resume::Shot::Abandoned { detail, .. } => {
                    note("ui_gear_capture_abandoned", [current as u64, detail, 0, 0]);
                    ui_resume::Screen::Settings(settings_resume::Snapshot::INITIAL)
                }
            }
        }
        cosmetic_resume::Leaf::Color => match cosmetic_resume::capture_color(context, current) {
            cosmetic_resume::Shot::Wait => return false,
            cosmetic_resume::Shot::Found(snapshot) => ui_resume::Screen::Color(snapshot),
            cosmetic_resume::Shot::Abandoned { detail, .. } => {
                note("ui_color_capture_abandoned", [current as u64, detail, 0, 0]);
                ui_resume::Screen::Settings(settings_resume::Snapshot::INITIAL)
            }
        },
        cosmetic_resume::Leaf::Voice => match cosmetic_resume::capture_voice(context, current) {
            cosmetic_resume::Shot::Wait => return false,
            cosmetic_resume::Shot::Found(snapshot) => ui_resume::Screen::Voice(snapshot),
            cosmetic_resume::Shot::Abandoned { detail, .. } => {
                note("ui_voice_capture_abandoned", [current as u64, detail, 0, 0]);
                ui_resume::Screen::Settings(settings_resume::Snapshot::INITIAL)
            }
        },
        cosmetic_resume::Leaf::Specials => {
            let result = {
                let mut capture = CAPTURE.lock().unwrap();
                if capture.as_ref().is_none_or(|capture| capture.context != context) {
                    *capture = Some(Capture { context, frames: 0, result: None });
                    return false;
                }
                let capture = capture.as_mut().unwrap();
                capture.frames = capture.frames.saturating_add(1);
                if capture.frames > 120 && capture.result.is_none() {
                    capture.result = Some(Err(super::lua_vm::Error::Binding));
                }
                capture.result.take()
            };
            match result {
                None => return false,
                Some(Ok(Some(snapshot))) => {
                    *CAPTURE.lock().unwrap() = None;
                    ui_resume::Screen::Specials(snapshot)
                }
                Some(Ok(None)) => {
                    // The user already completed this script; let native code
                    // propagate its confirmed choices before capturing DRAFT.
                    *CAPTURE.lock().unwrap() = None;
                    return false;
                }
                Some(Err(error)) => {
                    *CAPTURE.lock().unwrap() = None;
                    note("ui_specials_capture_failed", [error.code() as u64, 0, 0, 0]);
                    ui_resume::Screen::Settings(settings_resume::Snapshot::INITIAL)
                }
            }
        }
        cosmetic_resume::Leaf::Test => ui_resume::Screen::Test,
        cosmetic_resume::Leaf::Other(_) => {
            note("ui_screen_capture_unqualified", [current as u64, 0, 0, 0]);
            ui_resume::Screen::Settings(settings_resume::Snapshot::INITIAL)
        }
    };
    store(screen, focus)
}

#[skyline::hook(offset = 0x19dcc80)]
unsafe fn specials_frame(component: usize) {
    let context = EDIT_CONTEXT.load(Ordering::Acquire);
    let mut bound = false;
    // The global window ticker runs immediately before the scene pass, on the
    // same native main-loop thread; it is not nested in the editor frame.
    // Rebind this live component against the still-owned editor identity.
    let owned_thread = EDIT_OWNED.load(Ordering::Acquire) && context != 0
        && STATE.lock().unwrap().as_ref().is_some_and(|s|
            s.phase == Phase::Editing && s.thread == current_thread());
    if owned_thread {
        let holder: usize = native::read(context, 0x198);
        if native::aligned(holder) && native::read::<usize>(holder, 0x80) == component {
            bound = true;
        }
    }
    let capture_needed = bound && CAPTURE.lock().unwrap().as_ref().is_some_and(|capture|
        capture.context == context && capture.result.is_none());
    // Age 0 creates the Lua coroutine; age 1 resumes it for the first time.
    // This hook precedes the native update, so only age >= 2 has initialized
    // the pending choices. Let both startup updates run first.
    let script_started = bound && native::read::<u32>(component, 0xe4) > 1;
    if capture_needed && script_started {
        let result = specials_resume::capture(BASE.load(Ordering::Acquire), component);
        let completed = matches!(result, Ok(None));
        if let Some(capture) = CAPTURE.lock().unwrap().as_mut().filter(|capture| capture.context == context) {
            capture.result = Some(result);
        }
        // A completed script can finish its native propagation. Otherwise hold
        // this one update so no pending selection is committed during teardown.
        if !completed { return; }
    }
    if script_started {
        let mut reconstruction = {
            let mut stored = RECONSTRUCTION.lock().unwrap();
            if stored.as_ref().is_some_and(|r| r.context == context && r.result.is_none()) {
                stored.take()
            } else { None }
        };
        // Protected Lua and native UI calls run without a Rust mutex held.
        if let Some(r) = reconstruction.as_mut() {
            match r.restore.step(BASE.load(Ordering::Acquire), component) {
                Ok(true) => r.result = Some(Ok(())),
                Ok(false) => {}
                Err(error) => r.result = Some(Err(error)),
            }
        }
        if reconstruction.is_some() { *RECONSTRUCTION.lock().unwrap() = reconstruction; }
    }
    original!()(component);
}

fn settle(restored: bool, detail: u64) {
    let ticket = {
        let mut state = STATE.lock().unwrap();
        let Some(s) = state.as_mut().filter(|s| s.thread == current_thread()) else { return; };
        let Some(ticket) = s.restore_ticket else { return; };
        if s.restore_screen.take().is_none() { return; }
        // A failure leaves an unacknowledged checkpoint. Keep its ownership
        // ticket so a genuine later interruption can re-arm the same snapshot.
        if restored { s.restore_ticket = None; }
        s.event(if restored { "ui_restore_observed" } else { "ui_restore_failed" },
            [detail, s.restore_focus as u64, u64::from(s.restore_entered), 0]);
        ticket
    };
    *RECONSTRUCTION.lock().unwrap() = None;
    settings_resume::clear();
    cosmetic_resume::clear();
    if restored { ui_resume::restored(ticket); } else { ui_resume::restore_failed(ticket); }
}

/// Write an unfinished cosmetic page's cursor before this frame's native handler.
pub(super) unsafe fn prepare_restore(context: usize) {
    let screen = {
        let state = STATE.lock().unwrap();
        let Some(s) = state.as_ref().filter(|s| s.phase == Phase::Editing && !s.interrupted
            && s.restore_entered && s.restore_ticket.is_some() && s.thread == current_thread())
            else { return; };
        s.restore_screen.clone()
    };
    if let Some(screen) = screen { cosmetic_resume::prepare(context, &screen); }
}

/// Called after the owned native edit frame. Re-entry uses the one frame in
/// which state 0 has queued Settings, before Settings has registered any input.
pub(super) unsafe fn after_edit(context: usize) {
    let request = {
        let mut state = STATE.lock().unwrap();
        let Some(s) = state.as_mut().filter(|s| s.phase == Phase::Editing
            && s.thread == current_thread() && !s.interrupted && s.restore_ticket.is_some())
            else { return; };
        let Some(screen) = s.restore_screen else { return; };
        s.restore_frames = s.restore_frames.saturating_add(1);
        let current: i32 = native::read(context, 0x148);
        if s.restore_state != current {
            s.restore_state = current;
            s.event("ui_restore_native_state", [current as u64,
                native::read::<u32>(context, 0x14c) as u64, s.restore_frames as u64, 0]);
        }
        (screen, s.restore_entered, current, s.restore_frames)
    };
    let (screen, entered, current, frames) = request;
    if frames > 600 { settle(false, 1); return; }
    if !entered {
        match super::screen_reentry::enter(context, screen) {
            super::screen_reentry::Entry::Wait => return,
            super::screen_reentry::Entry::Refused => { settle(false, 2); return; }
            super::screen_reentry::Entry::Entered => {
                if let ui_resume::Screen::Settings(wanted) = screen {
                    settings_resume::begin_restore(context, wanted);
                }
                if let ui_resume::Screen::Specials(wanted) = screen {
                    *RECONSTRUCTION.lock().unwrap() = Some(Reconstruction {
                        context, restore: specials_resume::Restore::new(wanted), result: None,
                    });
                }
                if let Some(s) = STATE.lock().unwrap().as_mut() {
                    s.restore_entered = true;
                    s.event("ui_restore_target_queued", [current as u64, 0, 0, 0]);
                }
                return;
            }
        }
    }
    let restored = match screen {
        ui_resume::Screen::Settings(wanted) => match settings_resume::restore_result(context) {
            Some(Ok(Some(seen))) if seen == wanted && current == 3 => {
                note("ui_settings_restore_values", [seen.row as u64, seen.area as u64, 0, 0]);
                true
            }
            Some(Err(error)) => { settle(false, 0x2000 + error.code() as u64); return; }
            Some(_) => { settle(false, 0x2000 + super::lua_vm::Error::Value.code() as u64); return; }
            None => false,
        },
        ui_resume::Screen::Headgear(wanted) => match cosmetic_resume::poll_gear(context, 5, wanted) {
            cosmetic_resume::Poll::Ready => true,
            cosmetic_resume::Poll::Failed(detail) => { settle(false, 0x3000 + detail); return; }
            cosmetic_resume::Poll::Pending => false,
        },
        ui_resume::Screen::Outfit(wanted) => match cosmetic_resume::poll_gear(context, 6, wanted) {
            cosmetic_resume::Poll::Ready => true,
            cosmetic_resume::Poll::Failed(detail) => { settle(false, 0x3000 + detail); return; }
            cosmetic_resume::Poll::Pending => false,
        },
        ui_resume::Screen::Color(wanted) => match cosmetic_resume::poll_color(context, wanted) {
            cosmetic_resume::Poll::Ready => true,
            cosmetic_resume::Poll::Failed(detail) => { settle(false, 0x3000 + detail); return; }
            cosmetic_resume::Poll::Pending => false,
        },
        ui_resume::Screen::Name(_) => current == 4 && name_keyboard::restored(context),
        ui_resume::Screen::Specials(_) => {
            let result = RECONSTRUCTION.lock().unwrap().as_mut()
                .filter(|r| r.context == context).and_then(|r| r.result.take());
            match result {
                Some(Ok(())) => current == 8,
                Some(Err(error)) => { settle(false, 0x1000 + error.code() as u64); return; }
                None => false,
            }
        }
        ui_resume::Screen::Voice(wanted) => match cosmetic_resume::poll_voice(context, wanted) {
            cosmetic_resume::Poll::Ready => true,
            cosmetic_resume::Poll::Failed(detail) => { settle(false, 0x3000 + detail); return; }
            cosmetic_resume::Poll::Pending => false,
        },
        ui_resume::Screen::Test => current == 10
            && native::read::<u32>(context, 0x14c) > 0
            && contract::test_running(BASE.load(Ordering::Acquire)),
    };
    if restored { settle(true, current as u64); }
}

/// Value reconstruction and SDK work belong to this editor only. Native
/// pointers here are comparison tokens, never read after a release callback.
pub(super) unsafe fn before_release(context: usize) {
    name_keyboard::before_release(context);
    *CAPTURE.lock().unwrap() = None;
    *RECONSTRUCTION.lock().unwrap() = None;
    settings_resume::release();
    cosmetic_resume::clear();
    settle(false, 3);
}

pub(super) unsafe fn try_auto_return(scene: usize, frame: usize) {
    if ACTIVE.load(Ordering::Acquire) || !ui_resume::pending() { return; }
    if crate::common::menu::captures_input() { auto_observation(1, 0); return; }
    let base = BASE.load(Ordering::Acquire);
    if !native::aligned(frame) || !native::aligned(scene)
        || native::read::<usize>(scene, 0) != base + 0x50a2e50 { auto_observation(2, 0); return; }
    let sequence: usize = native::read(frame, 8);
    let Some(owner_scene) = sequence.checked_sub(0x50) else { return; };
    if !native::aligned(owner_scene) || native::read::<usize>(owner_scene, 0) != base + 0x50b7998 {
        auto_observation(3, 0); return;
    }
    let context: usize = native::read(owner_scene, 0xe8);
    if let Some(owner) = contract::Owner::bind(base, context) {
        let Some(practice) = contract::Practice::bind(base, scene, frame, owner.sequence) else {
            auto_observation(5, 0); return;
        };
        let Some(generation) = lifecycle::generation_for(owner.sequence) else {
            auto_observation(6, 0); return;
        };
        resume_room(Origin::Quickplay, generation, practice, super::quickplay_available(), base);
        return;
    }
    let Some(owner) = contract::LdnOwner::bind(base, context) else { auto_observation(4, 0); return; };
    let Some(practice) = contract::Practice::bind(base, scene, frame, owner.sequence) else {
        auto_observation(5, 0); return;
    };
    let Some(generation) = super::ldn_lifecycle::generation_for(owner.sequence) else {
        auto_observation(6, 0); return;
    };
    let lobby_ready = super::local_wireless_available()
        && !crate::common::is_native_training_mode();
    resume_room(Origin::LocalWireless, generation, practice, lobby_ready, base);
}

unsafe fn resume_room(
    origin: Origin, generation: u64, practice: contract::Practice, lobby_ready: bool, base: usize,
) {
    let match_received = practice.match_received();
    let no_event = contract::pending_interruption(base) == Some(0);
    let native_ready = practice.ready(base);
    // The native match flag is never cleared on a receiving PracticeScene.
    // A subsequent ready instance under this generation is the return witness.
    ui_resume::observe_practice(generation, match_received, native_ready);
    auto_observation(7, u64::from(native_ready) | (u64::from(match_received) << 1)
        | (u64::from(no_event) << 2) | (u64::from(lobby_ready) << 3)
        | (ui_resume::readiness_bits(generation) << 4));
    let ready = !match_received && no_event && native_ready && lobby_ready;
    if let Some((ticket, checkpoint)) = ui_resume::begin(generation, ready, false) {
        if let Err(reason) = super::resume_checkpoint(origin, ticket, checkpoint) {
            ui_resume::request_refused(ticket);
            note("automatic_resume_request_refused", [generation, 0, 0, 0]);
            crate::logging::diagnostic(format_args!("[MiiEditor] automatic resume request failed: {reason}"));
        }
    }
}

pub(super) fn init() {
    skyline::install_hooks!(specials_frame);
    settings_resume::init();
}
