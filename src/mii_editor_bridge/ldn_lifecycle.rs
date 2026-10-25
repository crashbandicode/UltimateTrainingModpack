//! Outer Local Wireless lifetime. The parent vtable is `base+0x50b8028`.
//! Its init, release, and frame are the same slots Quickplay uses on
//! `base+0x50b9000`, and each function is referenced only from that slot.
//! Generations share Quickplay's counter so one number cannot name both rooms.
use super::{connected_native::local_wireless_parent_waiting, lifecycle::next_generation,
    native::{aligned, read}, BASE};
use std::sync::{atomic::{AtomicBool, Ordering}, Mutex};

#[derive(Clone, Copy)]
struct Owner { scene: usize, context: usize, generation: u64, current: i32 }

static OWNER: Mutex<Option<Owner>> = Mutex::new(None);
static UNBOUND_REPORTED: AtomicBool = AtomicBool::new(false);

fn note(name: &'static str, values: [u64; 4]) {
    if let Ok(mut state) = super::STATE.try_lock() {
        if let Some(s) = state.as_mut().filter(|s| s.origin == super::Origin::LocalWireless) {
            s.event(name, values);
        }
    }
}

fn retire(generation: u64, reason: &'static str) {
    if super::ui_resume::end_session(generation) {
        if let Some(s) = super::STATE.lock().unwrap().as_mut() {
            s.event(reason, [generation, 0, 0, 0]);
        }
    }
}

unsafe fn context(scene: usize) -> Option<usize> {
    let base = BASE.load(Ordering::Acquire);
    if !aligned(scene) || read::<usize>(scene, 0) != base + 0x50b8028 { return None; }
    let context: usize = read(scene, 0xe8);
    if !aligned(context) || read::<usize>(context, 0) != scene + 0x50
        || read::<usize>(context, 8) != context { return None; }
    Some(context)
}

/// Rebind from the child sequence's parent link. State 6 is the captured
/// waiting value of `context+0x120`, and the stack top has to match it.
pub(super) unsafe fn generation_for(sequence: usize) -> Option<u64> {
    if !aligned(sequence) { return None; }
    let parent: usize = read(sequence, 0x18);
    let scene = parent.checked_sub(0x50)?;
    let context = context(scene)?;
    let level: i32 = read(context, 0x11c);
    if !(0..0x13).contains(&level) { return None; }
    let top: i32 = read(context, 0xd0 + level as usize * 4);
    let state: i32 = read(context, 0x120);
    if !local_wireless_parent_waiting(level, state, top) { return None; }
    OWNER.lock().unwrap().filter(|owner|
        owner.scene == scene && owner.context == context)
        .map(|owner| owner.generation)
}

#[skyline::hook(offset = 0x2336c90)]
unsafe fn initialized(scene: usize) {
    original!()(scene);
    let previous = OWNER.lock().unwrap().take();
    if let Some(previous) = previous { retire(previous.generation, "local_wireless_owner_replaced"); }
    let Some(context) = context(scene) else { return; };
    let Some(generation) = next_generation() else { return; };
    *OWNER.lock().unwrap() = Some(Owner { scene, context, generation, current: -1 });
    note("local_wireless_owner_initialized", [scene as u64, context as u64, generation, 0]);
}

#[skyline::hook(offset = 0x2337330)]
unsafe fn released(scene: usize) {
    let current = context(scene);
    let previous = {
        let mut owner = OWNER.lock().unwrap();
        if owner.as_ref().is_some_and(|owner| owner.scene == scene && current == Some(owner.context)) {
            owner.take()
        } else { None }
    };
    if let Some(previous) = previous {
        note("local_wireless_owner_released", [scene as u64, previous.context as u64, previous.generation, 0]);
        retire(previous.generation, "local_wireless_owner_released");
    }
    original!()(scene);
}

#[skyline::hook(offset = 0x23374b0)]
unsafe fn frame(scene: usize, arg: usize) {
    original!()(scene, arg);
    let Some(context) = context(scene) else {
        if super::ui_resume::pending() && !UNBOUND_REPORTED.swap(true, Ordering::Relaxed) {
            note("local_wireless_owner_unbound", [scene as u64, 0, 0, 0]);
        }
        return;
    };
    UNBOUND_REPORTED.store(false, Ordering::Relaxed);
    // After the native frame, +0x120 is the slot it just installed and +0x128
    // is the previous state. The dispatcher rejects a slot >= 10.
    let current: i32 = read(context, 0x120);
    let previous: i32 = read(context, 0x128);
    if !(0..10).contains(&current) { return; }
    let observation = {
        let mut owner = OWNER.lock().unwrap();
        owner.as_mut().filter(|owner| owner.scene == scene && owner.context == context).map(|owner| {
            let changed = owner.current != current;
            owner.current = current;
            (owner.generation, changed)
        })
    };
    if let Some((generation, changed)) = observation {
        if changed { note("local_wireless_owner_state", [generation, current as u64, previous as u64, 0]); }
        super::ui_resume::observe_session(generation, current, previous, changed);
    }
}

pub(super) fn init() { skyline::install_hooks!(initialized, released, frame); }
