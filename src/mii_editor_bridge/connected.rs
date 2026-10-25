//! Private waiting-room adapter. Quickplay and Local Wireless share the child
//! controller at `0x2310570` and keep separate parent binds. The retained owner
//! is custom orchestration; native practice, Test, editor teardown and
//! matchmaking still perform their own work.
use super::{connected_native as contract, native, current_thread,
    Origin, Phase, Session, ACTIVE, BASE, EDIT_LEAF_SCENE, EDIT_OWNER,
    EDIT_OWNED, EDIT_SCENE, RELEASED_SCENE, STATE};
use std::sync::atomic::{AtomicUsize, Ordering};

// Identity only. Every native object is rebound by its own current callback.
static RETAINED: AtomicUsize = AtomicUsize::new(0);
pub(super) fn forget(_seed: &super::draft::Draft) { super::ui_resume::discard(); }

unsafe fn note_pending(s: &mut Session) {
    if !matches!(s.origin, Origin::Quickplay | Origin::LocalWireless) || s.interrupted { return; }
    let Some(event) = contract::pending_interruption(BASE.load(Ordering::Acquire)) else { return; };
    if event == 0 { return; }
    // A real opponent's Join returns ownership to native waiting practice,
    // which completes matchmaking. Both it and an already queued match-start
    // remain unconsumed until the editor has released its resources.
    s.interrupted = true;
    // Explicit Save already handed this draft to native persistence. It is no
    // longer an unfinished interaction, even if matchmaking arrives during exit.
    if s.explicit_save_finished { s.snapshot_saved = true; }
    s.observe_inputs("network_interruption");
    s.event("network_interruption", [event, 0, 0, 0]);
}

pub(super) unsafe fn before_parent() {
    let queued = {
        let mut state = STATE.lock().unwrap();
        if let Some(s) = state.as_mut().filter(|s| s.thread == current_thread()
            && matches!(s.phase, Phase::Queued | Phase::Editing)) {
            note_pending(s);
            s.phase == Phase::Queued && s.interrupted && !s.snapshot_saved
        } else { false }
    };
    if queued { super::resume_adapter::capture_queued(); }
}

pub(super) unsafe fn before_edit(scene: usize, frame: usize, context: usize) -> bool {
    let (abort, completed) = {
        let mut state = STATE.lock().unwrap();
        let Some(s) = state.as_mut().filter(|s| s.phase == Phase::Editing
            && s.thread == current_thread()) else { return false; };
        note_pending(s);
        (s.interrupted, s.submenu_completed)
    };
    if completed { return true; }
    if !abort { return false; }
    // State 11 is the shared preview waiter. Pop its one success store before
    // capture so this frame's dispatcher enters the page underneath. Voice
    // entry reveals state 9 and Test entry reveals state 10. Capture and leaf
    // completion follow on that page.
    if let Some(revealed) = contract::release_preview_wait(context) {
        if let Some(s) = STATE.lock().unwrap().as_mut() {
            let underneath = native::read::<i32>(context, 0xf8 + revealed as usize * 4);
            s.event("interrupted_preview_wait_released", [
                native::read::<i32>(context, 0x148) as u64,
                revealed as u64,
                underneath as u64,
                0,
            ]);
        }
        return false;
    }
    if !super::resume_adapter::before_edit(context) { return false; }
    let base = BASE.load(Ordering::Acquire);
    if let Some(submenu) = contract::Submenu::bind(scene, frame, context,
        EDIT_SCENE.load(Ordering::Acquire)) {
        // Pending screen values have been captured separately from DRAFT.
        // Native leaf completion releases the layouts without confirming them.
        {
            let mut state = STATE.lock().unwrap();
            let s = state.as_mut().unwrap();
            s.submenu_completed = true;
            s.event("interrupted_submenu_completed", [submenu.state as u64, 0, 0, 0]);
        }
        super::resume_adapter::before_release(context);
        submenu.complete(base);
        // Native pre-emption returns immediately after completing the leaf.
        // Its original update must not run again while release is pending.
        return true;
    }
    if let Some(event) = contract::interrupt_editor(base, context, || {
        // Do not replace the checkpoint with teardown's confirmed DRAFT.
        if let Some(s) = STATE.lock().unwrap().as_mut() {
            s.event("interrupted_editor_ready_to_cancel", [0; 4]);
        }
    }) {
        let mut state = STATE.lock().unwrap();
        if let Some(s) = state.as_mut() { s.event(event, [native::read::<i32>(context, 0x148) as u64,
            native::read::<u32>(context, 0x14c) as u64, native::read::<i32>(context, 0x144) as u64, 0]); }
    }
    false
}

fn relinquish(s: &mut Session, event: &'static str) {
    if let Some(ticket) = s.restore_ticket.take() { super::ui_resume::restore_failed(ticket); }
    s.phase = Phase::Refused;
    s.event(event, [0; 4]);
    RETAINED.store(0, Ordering::Release);
    ACTIVE.store(false, Ordering::Release);
}

trait WaitingOwner {
    fn sequence(&self) -> usize;
    unsafe fn age(&self) -> u32;
    unsafe fn result(&self) -> i32;
    unsafe fn drained(&self) -> bool;
    unsafe fn reenter(&self) -> bool;
}

impl WaitingOwner for contract::Owner {
    fn sequence(&self) -> usize { self.sequence }
    unsafe fn age(&self) -> u32 { contract::Owner::age(self) }
    unsafe fn result(&self) -> i32 { contract::Owner::result(self) }
    unsafe fn drained(&self) -> bool { contract::Owner::drained(self) }
    unsafe fn reenter(&self) -> bool { contract::Owner::reenter(self) }
}

impl WaitingOwner for contract::LdnOwner {
    fn sequence(&self) -> usize { self.sequence }
    unsafe fn age(&self) -> u32 { contract::LdnOwner::age(self) }
    unsafe fn result(&self) -> i32 { contract::LdnOwner::result(self) }
    unsafe fn drained(&self) -> bool { contract::LdnOwner::drained(self) }
    unsafe fn reenter(&self) -> bool { contract::LdnOwner::reenter(self) }
}

enum Drive { Original, Value(usize) }

unsafe fn drive_waiting(
    owner: &impl WaitingOwner, origin: Origin, bound: &'static str,
    context: usize, base: usize,
) -> Drive {
    let phase = {
        let mut state = STATE.lock().unwrap();
        let Some(s) = state.as_mut().filter(|s| s.origin == origin) else {
            return Drive::Original;
        };
        if s.phase == Phase::Requested && s.owner == 0 && owner.age() > 0 && !owner.drained() {
            if !native::catalog_clean(base) {
                relinquish(s, "connected_catalog_dirty");
                return Drive::Original;
            }
            s.owner = owner.sequence();
            s.mode = native::read(base, native::MODE);
            s.thread = current_thread();
            s.event(bound, [owner.sequence() as u64, context as u64, s.mode as u64, 0]);
        }
        if s.owner != owner.sequence() || s.thread != current_thread() {
            return Drive::Original;
        }
        if s.phase == Phase::Quitting && !owner.drained() {
            s.wait_frames = s.wait_frames.saturating_add(1);
            if s.wait_frames >= 600 { relinquish(s, "connected_quit_timeout"); }
        }
        s.phase
    };
    match phase {
        Phase::Quitting if owner.drained() => {
            // Practice may have received the match before its quit completed.
            // Its actual result takes precedence; never manufacture result3.
            if owner.result() != 0 {
                let mut state = STATE.lock().unwrap();
                relinquish(state.as_mut().unwrap(), "practice_completed_before_editor");
                return Drive::Original;
            }
            if !contract::practice_resources_released(base) {
                let mut state = STATE.lock().unwrap();
                relinquish(state.as_mut().unwrap(), "practice_resources_not_released");
                return Drive::Original;
            }
            let inputs = native::practice_inputs(base);
            let input_baseline = native::input_snapshot(base);
            native::register_editor_sequence(base, owner.sequence(), true);
            native::write(base, 0x5307d40, 0u8);
            EDIT_OWNER.store(owner.sequence(), Ordering::Release);
            EDIT_OWNED.store(true, Ordering::Release);
            let queued = native::enqueue_sequence(base, owner.sequence(), b"CharaMakeSequenceScene");
            let mut state = STATE.lock().unwrap(); let s = state.as_mut().unwrap();
            s.inputs = inputs;
            s.input_baseline = Some(input_baseline);
            s.phase = if queued { Phase::Queued } else { Phase::Returning };
            s.event("connected_editor_queued", [u64::from(queued), inputs[0], inputs[1], inputs[2]]);
            if !queued {
                if let Some(ticket) = s.restore_ticket.take() {
                    super::ui_resume::restore_failed(ticket);
                }
            }
            drop(state);
            if !queued {
                native::register_editor_sequence(base, owner.sequence(), false);
                EDIT_OWNED.store(false, Ordering::Release);
                EDIT_OWNER.store(0, Ordering::Release);
                owner.reenter();
            }
            Drive::Value(0)
        }
        Phase::Queued | Phase::Editing => Drive::Value(0),
        Phase::Exiting if owner.drained() => {
            if !super::save_ready_for_return(base) { return Drive::Value(0); }
            let (mode, inputs, interrupted, seed) = {
                let state = STATE.lock().unwrap(); let s = state.as_ref().unwrap();
                (s.mode, s.inputs, s.interrupted, s.seed)
            };
            native::register_editor_sequence(base, owner.sequence(), false);
            let observed = native::practice_inputs(base);
            let idle = native::preview_state(base).is_some_and(|(state, _)| state == 0);
            native::write(base, native::MODE, mode);
            let released = RELEASED_SCENE.load(Ordering::Acquire) != 0;
            // These tables belong to the live session. A peer joining updates
            // its entry while the editor is open; treating that update as
            // corruption refused an otherwise valid native return. Never
            // restore or freeze them. The freshly bound waiting owner rebuilds
            // practice from the current native session after editor release.
            let resources_released = contract::practice_resources_released(base);
            let catalog_clean = native::catalog_clean(base);
            let reentered = released && idle && resources_released && catalog_clean && owner.reenter();
            let mut state = STATE.lock().unwrap(); let s = state.as_mut().unwrap();
            s.phase = if reentered { Phase::Returning } else { Phase::Refused };
            s.observe_inputs("after_editor_release");
            s.event("connected_reentry_barrier", [u64::from(resources_released),
                u64::from(catalog_clean), owner.result() as u64, 0]);
            s.event("connected_reentry_requested", [u64::from(reentered), u64::from(idle),
                u64::from(inputs == observed), u64::from(released)]);
            EDIT_OWNED.store(false, Ordering::Release);
            EDIT_OWNER.store(0, Ordering::Release);
            if !interrupted { forget(&seed); }
            if !reentered {
                relinquish(s, "connected_reentry_refused");
                return Drive::Original;
            }
            Drive::Value(0)
        }
        Phase::Exiting => Drive::Value(0),
        _ => Drive::Original,
    }
}

#[skyline::hook(offset = 0x2310570)]
unsafe fn waiting_owner(context: usize, args: usize) -> usize {
    if !ACTIVE.load(Ordering::Acquire) { return original!()(context, args); }
    let base = BASE.load(Ordering::Acquire);
    let origin = STATE.lock().unwrap().as_ref().map(|session| session.origin);
    let driven = match origin {
        Some(Origin::Quickplay) => {
            let Some(owner) = contract::Owner::bind(base, context) else {
                return original!()(context, args);
            };
            drive_waiting(&owner, Origin::Quickplay, "quickplay_owner_bound", context, base)
        }
        Some(Origin::LocalWireless) => {
            let Some(owner) = contract::LdnOwner::bind(base, context) else {
                return original!()(context, args);
            };
            drive_waiting(&owner, Origin::LocalWireless, "local_wireless_owner_bound", context, base)
        }
        _ => return original!()(context, args),
    };
    match driven {
        Drive::Original => original!()(context, args),
        Drive::Value(value) => value,
    }
}

#[skyline::hook(offset = 0x1851480)]
unsafe fn practice_frame(scene: usize, frame: usize) -> usize {
    let result = original!()(scene, frame);
    if !ACTIVE.load(Ordering::Acquire) {
        super::resume_adapter::try_auto_return(scene, frame);
        return result;
    }
    let base = BASE.load(Ordering::Acquire);
    let begin = {
        let mut state = STATE.lock().unwrap();
        let Some(s) = state.as_mut().filter(|s| matches!(s.origin, Origin::Quickplay | Origin::LocalWireless)
            && s.thread == current_thread()) else { return result; };
        let Some(practice) = contract::Practice::bind(base, scene, frame, s.owner) else { return result; };
        if s.phase == Phase::Returning && (practice.ready(base) || practice.match_received()) {
            s.phase = Phase::Finished;
            s.event("connected_practice_returned", [scene as u64, u64::from(practice.match_received()), 0, 0]);
            RETAINED.store(0, Ordering::Release);
            ACTIVE.store(false, Ordering::Release);
            false
        } else if s.phase == Phase::Requested && !crate::common::menu::captures_input() {
            s.wait_frames = s.wait_frames.saturating_add(1);
            if s.wait_frames >= 600 { relinquish(s, "connected_ready_timeout"); false }
            else if contract::pending_match(base).is_none_or(|event| event & 0xff == 6) || practice.match_received() {
                relinquish(s, "network_precedes_editor_request"); false
            } else { practice.ready(base) }
        } else { false }
    };
    if begin {
        let owner = { STATE.lock().unwrap().as_ref().unwrap().owner };
        if let Some(practice) = contract::Practice::bind(base, scene, frame, owner) {
            if practice.request_quit(base) {
                RETAINED.store(owner, Ordering::Release);
                let mut state = STATE.lock().unwrap(); let s = state.as_mut().unwrap();
                s.phase = Phase::Quitting; s.wait_frames = 0;
                s.event("connected_practice_quit_requested", [scene as u64, owner as u64, 0, 0]);
            }
        }
    }
    super::resume_adapter::try_auto_return(scene, frame);
    result
}

#[skyline::hook(offset = 0x17f3810)]
unsafe fn scene_network_ready(scene: usize) -> bool {
    // Practice can disappear before its owner binds. Retiring a Requested
    // session is safe: no native state has been changed yet. This base-scene
    // callback survives the practice scene and may run several times per frame.
    if ACTIVE.load(Ordering::Acquire) {
        if let Ok(mut state) = STATE.try_lock() {
            if let Some(s) = state.as_mut().filter(|s| matches!(s.origin, Origin::Quickplay | Origin::LocalWireless)
                && s.phase == Phase::Requested) {
                let available = match s.origin {
                    Origin::Quickplay => super::quickplay_available(),
                    Origin::LocalWireless => super::local_wireless_available(),
                    Origin::Training => false,
                };
                if !available || std::time::Instant::now() >= s.request_deadline {
                    relinquish(s, "connected_request_expired");
                }
            }
        }
    }
    if RETAINED.load(Ordering::Acquire) != 0 && scene != 0
        && scene == EDIT_LEAF_SCENE.load(Ordering::Acquire)
        && !contract::can_deliver_notification(BASE.load(Ordering::Acquire)) { return false; }
    original!()(scene)
}

#[skyline::hook(offset = 0x22e0470)]
unsafe fn sequence_network_fallback(scene: usize, args: usize) -> usize {
    let owner = RETAINED.load(Ordering::Acquire);
    if owner != 0 && scene != 0 {
        let editor = EDIT_SCENE.load(Ordering::Acquire);
        // Only sequence callbacks reach this hook. Childless retained owner,
        // CharaMake and its direct Unlock child must leave the event queued.
        if scene.checked_add(0x50) == Some(owner) || scene == editor
            || (editor != 0 && native::read::<usize>(scene, 0x68) == editor + 0x50) { return 0; }
    }
    original!()(scene, args)
}

pub(super) fn init() {
    skyline::install_hooks!(waiting_owner, practice_frame, scene_network_ready, sequence_network_fallback);
}
