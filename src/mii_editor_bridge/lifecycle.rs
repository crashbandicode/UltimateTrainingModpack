//! The outer Quickplay context survives the match/results/character-select
//! cycle. Practice's IngameCall owner does not define this lifetime.
use super::{native::{aligned, read}, BASE};
use std::sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Mutex};

#[derive(Clone, Copy)]
struct Owner { scene: usize, context: usize, generation: u64, current: i32 }

static SERIAL: AtomicU64 = AtomicU64::new(0);
static OWNER: Mutex<Option<Owner>> = Mutex::new(None);

/// One counter for both waiting rooms. Local Wireless calls this too, so a
/// generation number cannot refer to the other room's checkpoint.
pub(super) fn next_generation() -> Option<u64> {
    let previous = SERIAL.fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| value.checked_add(1)).ok()?;
    Some(previous + 1)
}
static UNBOUND_REPORTED: AtomicBool = AtomicBool::new(false);

fn note(name: &'static str, values: [u64; 4]) {
    if let Ok(mut state) = super::STATE.try_lock() {
        if let Some(s) = state.as_mut().filter(|s| s.origin == super::Origin::Quickplay) {
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
    if !aligned(scene) || read::<usize>(scene, 0) != base + 0x50b9000 { return None; }
    let context: usize = read(scene, 0xe8);
    if !aligned(context) || read::<usize>(context, 0) != scene + 0x50
        || read::<usize>(context, 8) != context { return None; }
    Some(context)
}

/// Rebind from the current IngameCall callback. Stored pointers are identity
/// tokens only; the returned generation is the only lifetime value in a ticket.
pub(super) unsafe fn generation_for(sequence: usize) -> Option<u64> {
    if !aligned(sequence) { return None; }
    let parent: usize = read(sequence, 0x18);
    let scene = parent.checked_sub(0x50)?;
    let context = context(scene)?;
    if read::<i32>(context, 0x150) != 8 { return None; }
    OWNER.lock().unwrap().filter(|owner|
        owner.scene == scene && owner.context == context)
        .map(|owner| owner.generation)
}

#[skyline::hook(offset = 0x236af60)]
unsafe fn initialized(scene: usize) {
    original!()(scene);
    let previous = OWNER.lock().unwrap().take();
    if let Some(previous) = previous { retire(previous.generation, "resume_owner_replaced"); }
    let Some(context) = context(scene) else { return; };
    let Some(generation) = next_generation() else { return; };
    *OWNER.lock().unwrap() = Some(Owner { scene, context, generation, current: -1 });
    note("quickplay_owner_initialized", [scene as u64, context as u64, generation, 0]);
}

#[skyline::hook(offset = 0x236b400)]
unsafe fn released(scene: usize) {
    let current = context(scene);
    let previous = {
        let mut owner = OWNER.lock().unwrap();
        if owner.as_ref().is_some_and(|owner| owner.scene == scene && current == Some(owner.context)) {
            owner.take()
        } else { None }
    };
    if let Some(previous) = previous {
        note("quickplay_owner_released", [scene as u64, previous.context as u64, previous.generation, 0]);
        retire(previous.generation, "resume_owner_released");
    }
    original!()(scene);
}

#[skyline::hook(offset = 0x236b470)]
unsafe fn frame(scene: usize, arg: usize) {
    original!()(scene, arg);
    let Some(context) = context(scene) else {
        if super::ui_resume::pending() && !UNBOUND_REPORTED.swap(true, Ordering::Relaxed) {
            note("quickplay_owner_unbound", [scene as u64, 0, 0, 0]);
        }
        return;
    };
    UNBOUND_REPORTED.store(false, Ordering::Relaxed);
    let current: i32 = read(context, 0x150);
    let previous: i32 = read(context, 0x158);
    if !(0..13).contains(&current) { return; }
    let observation = {
        let mut owner = OWNER.lock().unwrap();
        owner.as_mut().filter(|owner| owner.scene == scene && owner.context == context).map(|owner| {
            let changed = owner.current != current;
            owner.current = current;
            (owner.generation, changed)
        })
    };
    if let Some((generation, changed)) = observation {
        if changed { note("quickplay_owner_state", [generation, current as u64, previous as u64, 0]); }
        super::ui_resume::observe_session(generation, current, previous, changed);
    }
}

pub(super) fn init() { skyline::install_hooks!(initialized, released, frame); }
