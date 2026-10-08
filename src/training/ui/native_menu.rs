//! Copy a native setting while the game owns its menu; never cache a menu/pane pointer.

use std::sync::atomic::{AtomicBool, Ordering};

use skyline::hooks::{getRegionAddress, InlineCtx, Region};

static COMBO_DISPLAY_ENABLED: AtomicBool = AtomicBool::new(true);

pub fn combo_display_enabled() -> bool {
    COMBO_DISPLAY_ENABLED.load(Ordering::Acquire)
}

pub fn reset() {
    COMBO_DISPLAY_ENABLED.store(true, Ordering::Release);
}

pub fn observe_combo_display(value: u8) {
    if value > 1 {
        return;
    }
    let enabled = value != 0;
    if COMBO_DISPLAY_ENABLED.swap(enabled, Ordering::AcqRel) != enabled {
        crate::logging::diagnostic(format_args!(
            "[NativeTrainingMenu] combo_display={enabled} (copied value; no retained pointer)"
        ));
    }
}

// Both sites follow the native LDRB of PauseMenu+0xb6c. Read the register the
// game just loaded, not a pointer saved by an earlier constructor or frame.
#[skyline::hook(offset = 0x13e549c, inline)]
unsafe fn observe_update(ctx: &mut InlineCtx) {
    if crate::common::is_native_training_mode() {
        observe_combo_display(ctx.registers[20].x() as u8);
    }
}

#[skyline::hook(offset = 0x13e54d8, inline)]
unsafe fn observe_initial_update(ctx: &mut InlineCtx) {
    if crate::common::is_native_training_mode() {
        observe_combo_display(ctx.registers[8].x() as u8);
    }
}

pub fn init() {
    // Fail closed if this owned 13.0.5 code differs. Notifications then default
    // to visible; there is no fallback to a cached native object address.
    let sites = [
        (0x13e5498, [0x396db114, 0x6b14013f, 0x540002c0, 0x34000234]),
        (0x13e54d4, [0x396db108, 0x3908c268, 0x320003e8, 0x3908be68]),
    ];
    let base = unsafe { getRegionAddress(Region::Text) } as usize;
    let verified = sites.iter().all(|(offset, expected)| unsafe {
        std::slice::from_raw_parts((base + offset) as *const u32, expected.len()) == expected
    });
    crate::logging::diagnostic(format_args!(
        "[NativeTrainingMenu] register_observers_verified={verified}"
    ));
    if verified {
        skyline::install_hooks!(observe_update, observe_initial_update);
    }
}
