//! Native input preview, using the existing Training save-state/playback paths.
//! UI requests carry values only. Game-object work runs on the CPU callback.
use std::sync::{atomic::{AtomicU8, Ordering}, Mutex};
use smash::{app::{self, lua_bind::WorkModule, BattleObjectModuleAccessor, Fighter}, lib::lua_const::*};
use training_mod_macros::{MacroRecording, StartState};

use crate::training::{input_record, save_states};

enum Request { Play { recording: MacroRecording, revision: u64 }, Stop }
#[derive(Clone, Copy, PartialEq)]
enum Phase { Starting, Playing, Complete, Stopping, Failed }
struct Session {
    recording: MacroRecording,
    revision: u64,
    phase: Phase,
    waits: u32,
    cpu: Option<(u32, usize)>,
}
static REQUEST: Mutex<Option<Request>> = Mutex::new(None);
static SESSION: Mutex<Option<Session>> = Mutex::new(None);
static STATUS: AtomicU8 = AtomicU8::new(0);

pub fn request_play(recording: MacroRecording, revision: u64) {
    *REQUEST.lock().unwrap() = Some(Request::Play { recording, revision });
    STATUS.store(1, Ordering::Relaxed);
}

pub fn request_stop() {
    if occupied() || REQUEST.lock().unwrap().is_some() {
        *REQUEST.lock().unwrap() = Some(Request::Stop);
    }
}

pub fn occupied() -> bool {
    SESSION.lock().unwrap().is_some() || REQUEST.lock().unwrap().is_some()
}
/// Failed native cleanup keeps its ownership, but closing the panel must leave
/// a controller route to the native pause/exit menu. No autoplay is re-enabled.
pub fn recovery_available() -> bool {
    SESSION.lock().unwrap().as_ref().is_some_and(|s| s.phase == Phase::Failed)
}
pub fn start_context() -> Option<StartState> {
    SESSION.lock().unwrap().as_ref().filter(|s| matches!(s.phase, Phase::Starting | Phase::Playing))
        .map(|s| s.recording.start)
}
pub fn can_emit(module: &mut BattleObjectModuleAccessor) -> bool {
    REQUEST.lock().unwrap().is_none()
        && SESSION.lock().unwrap().as_ref().is_some_and(|s| s.phase == Phase::Playing
            && s.cpu == Some((module.battle_object_id, module as *mut _ as usize)))
        && crate::training::named_macros::clip_foreground()
}
pub fn status() -> &'static str {
    match STATUS.load(Ordering::Relaxed) {
        1 => "Restoring saved Training setup...",
        2 => "Native input preview: attacks and effects run normally",
        3 => "Inputs complete; native effects finish normally. Y: restart",
        4 => "Restoring saved Training setup after preview...",
        5 => "Preview stopped; check the notification",
        _ => "X: preview from saved Training setup | Y: restart",
    }
}

/// Both fighters and partners remain native: hits, effects and cleanup need
/// normal simulation. P1's controls are consumed separately by the visible UI.
pub fn needs_native_update(_module: &BattleObjectModuleAccessor) -> bool { occupied() }

pub fn clear_session() {
    REQUEST.lock().unwrap().take();
    SESSION.lock().unwrap().take();
    save_states::end_preview_restore();
    STATUS.store(0, Ordering::Relaxed);
}

unsafe fn fail(message: &str) {
    input_record::stop_playback();
    REQUEST.lock().unwrap().take();
    let was_running = SESSION.lock().unwrap().as_ref().is_some_and(|s|
        matches!(s.phase, Phase::Playing | Phase::Complete));
    if was_running {
        request_stop();
    } else if save_states::is_loading() {
        // A deadline is not a successful restore. Let native sleep/buff/partner
        // cleanup continue, without injection or autoplay, until reset/departure.
        if let Some(s) = SESSION.lock().unwrap().as_mut() { s.phase = Phase::Failed; }
    } else { clear_session(); }
    STATUS.store(5, Ordering::Relaxed);
    crate::replay_capture::preview_failed(message);
}

pub fn progress(consumed: usize) {
    let revision = SESSION.lock().unwrap().as_ref()
        .filter(|s| s.phase == Phase::Playing).map(|s| s.revision);
    if let Some(revision) = revision {
        crate::replay_capture::preview_progress(revision, consumed as u32);
    }
}

pub fn complete() {
    if let Some(s) = SESSION.lock().unwrap().as_mut() {
        s.phase = Phase::Complete;
        STATUS.store(3, Ordering::Relaxed);
        println!("[ClipPreview] inputs_complete revision={} frames={}", s.revision, s.recording.frames.len());
    }
}

unsafe fn primary_cpu(module: &mut BattleObjectModuleAccessor) -> bool {
    WorkModule::get_int(module, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID)
        == crate::consts::FighterId::CPU as i32
        && crate::common::is_operation_cpu(module)
        && app::utility::get_kind(module) != *FIGHTER_KIND_NANA
}

pub unsafe fn before_frame(module: &mut BattleObjectModuleAccessor) {
    if !crate::common::is_native_training_mode() {
        if occupied() { input_record::stop_playback(); save_states::cancel_pending(); }
        clear_session();
        return;
    }
    if !primary_cpu(module) {
        return;
    }
    if crate::character_roster::busy() || !Fighter::is_ready_go() {
        if occupied() { input_record::stop_playback(); save_states::cancel_pending(); }
        clear_session();
        return;
    }
    let replaced = SESSION.lock().unwrap().as_ref().is_some_and(|s|
        matches!(s.phase, Phase::Playing | Phase::Complete)
            && (s.cpu != Some((module.battle_object_id, module as *mut _ as usize))
                || s.recording.start.fighter_kind != app::utility::get_kind(module)));
    if replaced {
        input_record::stop_playback();
        clear_session();
        STATUS.store(5, Ordering::Relaxed);
        crate::replay_capture::preview_failed("Preview fighter changed; reset Training before retrying");
        return;
    }
    if SESSION.lock().unwrap().as_ref().is_some_and(|s| s.phase == Phase::Playing)
        && !crate::training::named_macros::clip_foreground() { request_stop(); }
    // A queued restore owns the native loader until completion. Keep only the
    // newest UI request, then handle it once the current restore is finished.
    if save_states::is_loading() { return; }
    let request = REQUEST.lock().unwrap().take();
    match request {
        Some(Request::Play { recording, revision }) => {
            if recording.validate().is_err() || recording.start.fighter_kind != app::utility::get_kind(module) {
                fail("Wait for the selected CPU fighter before previewing");
                return;
            }
            input_record::stop_playback();
            *SESSION.lock().unwrap() = Some(Session {
                recording, revision, phase: Phase::Starting, waits: 0, cpu: None,
            });
            if let Err(error) = save_states::request_preview_restore() { fail(error); }
            else { println!("[ClipPreview] restore_start revision={}", revision); }
        }
        Some(Request::Stop) if occupied() => {
            input_record::stop_playback();
            if let Some(s) = SESSION.lock().unwrap().as_mut() {
                s.phase = Phase::Stopping;
                s.waits = 0;
            }
            STATUS.store(4, Ordering::Relaxed);
            if let Err(error) = save_states::request_preview_restore() { fail(error); }
            else { println!("[ClipPreview] restore_stop"); }
        }
        Some(Request::Stop) => STATUS.store(0, Ordering::Relaxed),
        None => {}
    }
}

pub unsafe fn after_frame(module: &mut BattleObjectModuleAccessor) {
    if !crate::common::is_native_training_mode() || !primary_cpu(module) { return; }
    let loading = save_states::is_loading();
    let action = {
        let mut session = SESSION.lock().unwrap();
        let Some(s) = session.as_mut() else { return };
        if s.phase == Phase::Failed {
            if loading { return; }
            drop(session);
            clear_session();
            STATUS.store(5, Ordering::Relaxed);
            return;
        }
        if s.phase == Phase::Playing && !input_record::is_playback() {
            drop(session);
            crate::replay_capture::preview_failed("Preview start did not settle; restoring saved setup");
            request_stop();
            return;
        }
        if !matches!(s.phase, Phase::Starting | Phase::Stopping) { return; }
        s.waits += 1;
        if s.waits > 600 { Some(Err("Preview restore stalled: B closes, then + exits Training")) }
        else if loading { None }
        else if s.phase == Phase::Stopping { Some(Ok(None)) }
        else {
            s.phase = Phase::Playing;
            s.cpu = Some((module.battle_object_id, module as *mut _ as usize));
            Some(Ok(Some(s.recording.clone())))
        }
    };
    match action {
        Some(Ok(Some(recording))) => {
            if input_record::playback_preview(recording) {
                STATUS.store(2, Ordering::Relaxed);
                println!("[ClipPreview] native_play");
            } else { fail("Preview could not start; restore the Training setup and retry"); }
        }
        Some(Ok(None)) => {
            clear_session();
            println!("[ClipPreview] restored_idle");
        }
        Some(Err(error)) => {
            fail(error);
        }
        None => {}
    }
}
