//! Ordinary Training entry rotation: native adapter. Explicit opt-in only.
//!
//! The entry decision rides inside the Mii bridge's existing Training state
//! callback, the apply rides inside the lobby probe's existing resource
//! callback, and one fingerprinted inline hook at the Training sequence
//! installer resets owner-bound values when a sequence is constructed. No
//! native function is called; the only writes are the two descriptor fields
//! the native setter itself stores.

#[path = "training_stage_rotation_policy.rs"]
mod policy;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use training_mod_consts::{OnOff, MENU};
use training_mod_sync::read;

static VERIFIED: AtomicBool = AtomicBool::new(false);
static LINES: AtomicUsize = AtomicUsize::new(0);
static ROTATION: Mutex<policy::TrainingRotation> = Mutex::new(policy::TrainingRotation::new());

fn enabled() -> bool {
    read(&MENU).training_stage_rotation == OnOff::ON
}

fn log(args: std::fmt::Arguments<'_>) {
    if LINES.fetch_add(1, Ordering::Relaxed) < 64 {
        crate::logging::diagnostic(args);
    }
}

/// Age-zero state-5 call of a structurally verified Training sequence. Only
/// the mode word gates preparation: at this transition the melee byte still
/// holds the previous scene's value (retained diagnostic lines 38/39, 311/312).
pub(crate) fn training_entry(context: usize) {
    if !VERIFIED.load(Ordering::Acquire) {
        return;
    }
    let Some(mode) = crate::common::game_mode::current() else { return; };
    if !mode.initialized || mode.mode != policy::TRAINING_MODE {
        return;
    }
    let (enabled, order) = {
        let menu = read(&MENU);
        (menu.training_stage_rotation == OnOff::ON, menu.lobby_stages)
    };
    let prefs = crate::common::menu::stage_preferences();
    let outcome = ROTATION.lock().unwrap().entry(context, enabled, order, &prefs);
    if outcome != policy::Entry::Disabled {
        log(format_args!(
            "[TrainingRotation] entry owner=0x{context:x} melee=0x{:02x} native_training={} outcome={outcome:?}",
            mode.melee, unsafe { crate::common::is_native_training_mode() }));
    }
}

/// The product reenter succeeded for `context`; its next age-zero call is a return.
pub(crate) fn reenter_requested(context: usize) {
    ROTATION.lock().unwrap().reenter_requested(context);
    log(format_args!("[TrainingRotation] reenter owner=0x{context:x}"));
}

/// Same native resource callback as the lobby probe, before its waiting gate.
/// The native caller owns the descriptor for this synchronous call; no pointer
/// survives the callback.
pub(crate) unsafe fn resource_request(ctx: &InlineCtx, mode: crate::common::game_mode::Mode) {
    if !VERIFIED.load(Ordering::Acquire) {
        return;
    }
    let base = getRegionAddress(Region::Text) as usize;
    let caller = (ctx.registers[30].x() as usize).wrapping_sub(base);
    let descriptor = ctx.registers[1].x() as usize;
    if !policy::qualified_loader(mode.initialized, mode.mode, mode.melee, caller,
        ctx.registers[2].w(), descriptor, base) {
        return;
    }
    let previous = crate::native_fields::read_unchecked::<u32>(descriptor, 0);
    // Snapshot the setting before taking ROTATION: never nest the menu lock
    // inside the policy lock or retain it across a native callback decision.
    let enabled = enabled();
    let result = ROTATION.lock().unwrap().apply(enabled, |native| {
        // Same catalog row the waiting preload reads: 40-bit package hash at +0x18.
        let record = base + 0x454a7e8 + native as usize * 72;
        Some(crate::native_fields::read_unchecked::<u64>(record, 24) & 0xffffffffff)
    });
    match result {
        Ok(Some(applied)) => {
            // Index-0 practice descriptor: the fingerprinted setter stores the
            // stage id at +0 and the package hash at +8. No other byte changes.
            crate::native_fields::write_unchecked(descriptor, 0, applied.native);
            crate::native_fields::write_unchecked(descriptor, 8, applied.hash);
            log(format_args!(
                "[TrainingRotation] apply previous={previous} selection={:?} native={} hash=0x{:x}",
                applied.selection, applied.native, applied.hash));
        }
        Ok(None) => {}
        Err(why) => log(format_args!("[TrainingRotation] apply refused: {why}")),
    }
}

/// A direct practice change completed natively in ordinary Training.
pub(crate) fn direct_completed(mode: u32, melee: u8, native: u32, hash: u64) {
    if mode != policy::TRAINING_MODE || melee != policy::TRAINING_MELEE {
        return;
    }
    ROTATION.lock().unwrap().direct_completed(native, hash);
}

/// A published direct change departed before completion in ordinary Training.
pub(crate) fn direct_uncertain(mode: u32, melee: u8) {
    if mode != policy::TRAINING_MODE || melee != policy::TRAINING_MELEE {
        return;
    }
    ROTATION.lock().unwrap().direct_uncertain();
}

// Training sequence installer: the store that clears the reenter byte of a
// freshly constructed state context (x20). Only owner-bound rotation values
// are reset here; the native instruction and every register stay untouched.
#[skyline::hook(offset = 0x23ecb18, inline)]
unsafe fn sequence_initialized(ctx: &mut InlineCtx) {
    let context = ctx.registers[20].x() as usize;
    ROTATION.lock().unwrap().sequence_initialized();
    log(format_args!("[TrainingRotation] sequence_initialized context=0x{context:x}"));
}

pub(crate) fn init(base: usize, loader_verified: bool) {
    // Own 13.0.5 words for the setter's descriptor stores (id +0, 40-bit hash
    // +8, flag byte +0x10) and the installer window around the reenter-byte
    // clear. Offsets alone never authorize these writes on another build.
    const CHECKS: &[(usize, &[u32])] = &[
        (0x178ab08, &[0x92409d6b, 0x5280160c, 0x9b0c2669, 0xb9000118, 0xf9022d2b, 0x3911812a]),
        (0x23ecb10, &[
            0xa9017e9f, 0xb900fa88, 0x3903f29f, 0xf9001a9f,
            0xa9027e9f, 0xa90ea29f, 0xa90dfe9f, 0xa90cfe9f,
        ]),
    ];
    let verified = loader_verified && CHECKS.iter().all(|(offset, words)| unsafe {
        std::slice::from_raw_parts((base + offset) as *const u32, words.len()) == *words
    });
    VERIFIED.store(verified, Ordering::Release);
    crate::logging::diagnostic(format_args!("[TrainingRotation] verified={verified} default_off=true"));
    if verified {
        skyline::install_hooks!(sequence_initialized);
    }
}
