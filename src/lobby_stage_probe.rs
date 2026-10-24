//! Development-only native entry probe. No per-frame mutation or saved pointer.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use training_mod_consts::{OnOff, MENU};
use training_mod_sync::read;

use crate::lobby_stage_cycle::StageCycle;

static CYCLE: Mutex<StageCycle> = Mutex::new(StageCycle::new());
static RESOURCE_LINES: AtomicUsize = AtomicUsize::new(0);

#[skyline::hook(offset = 0x25f7860, inline)]
unsafe fn resource_request(ctx: &mut InlineCtx) {
    #[cfg(feature = "practice-stage-observer")]
    crate::practice_stage_observer::observe_resource_request(ctx);
    let Some(mode) = crate::common::game_mode::current() else {
        return;
    };
    if mode.mode != 0x02010000 {
        return;
    }
    let config = ctx.registers[1].x() as *const u8;
    if config.is_null() || config as usize & 7 != 0 {
        return;
    }
    let base = getRegionAddress(Region::Text) as usize;
    let caller = (ctx.registers[30].x() as usize).wrapping_sub(base);
    let kind = std::ptr::read(config as *const u32);
    let hash = std::ptr::read(config.add(8) as *const u64);
    if RESOURCE_LINES.fetch_add(1, Ordering::Relaxed) < 32 {
        crate::logging::diagnostic(format_args!(
            "[LobbyStageProbe] resource caller=0x{:x} index={} kind={} hash=0x{:x} mode={:?}",
            caller,
            ctx.registers[2].w(),
            kind,
            hash,
            mode
        ));
    }
    // Proven local-wireless CSS preload only. The native stack configuration is
    // consumed synchronously by this request; no pointer survives the callback.
    if caller == 0x2335cf0
        && ctx.registers[2].w() == 2
        && kind == 311
        && hash == 0x165a48f367
        && read(&MENU).lobby_practice == OnOff::ON
    {
        let stages = read(&MENU).lobby_stages.stages();
        let mut cycle = CYCLE.lock().unwrap();
        #[cfg(feature = "practice-stage-request-alpha")]
        let selected = {
            let prefs = crate::common::menu::stage_preferences();
            if prefs.mode == training_mod_tui::stage_menu::Mode::Direct {
                cycle.prepare_direct(prefs.direct_stage)
            } else { cycle.prepare(&stages) }
        };
        #[cfg(not(feature = "practice-stage-request-alpha"))]
        let selected = cycle.prepare(&stages);
        let Some(stage) = selected else {
            return;
        };
        let record = (base + 0x454a7e8 + stage as usize * 72) as *const u8;
        let native_hash = std::ptr::read(record.add(24) as *const u64) & 0xffffffffff;
        std::ptr::write(config as *mut u32, stage);
        std::ptr::write(config.add(8) as *mut u64, native_hash);
        crate::logging::diagnostic(format_args!(
            "[LobbyStageProbe] preload selected={stage} hash=0x{native_hash:x}"
        ));
    }
}

pub unsafe fn observe_loaded() {
    let base = getRegionAddress(Region::Text) as usize;
    crate::logging::diagnostic(format_args!(
        "[LobbyStageProbe] loaded={} config={}",
        crate::training::save_states::stage_id(),
        std::ptr::read_volatile((base + 0x52c55d0) as *const i32)
    ));
}

// This is the unique stage-setter call in shared PracticeScene setup. Both
// local wireless and online practice callers enter it once before scene load.
// Native w1 is the stage ID; w0=0 selects the practice stage config, w2 is music.
// Leave the native setter, resource loading and all other arguments intact.
#[skyline::hook(offset = 0x1790e18, inline)]
unsafe fn stage_entry(ctx: &mut InlineCtx) {
    // Only the local-wireless resource preload above can prepare an entry.
    // Keep an already preloaded selection even if preferences change meanwhile.
    if !matches!(crate::common::game_mode::current(), Some(mode)
        if mode.initialized && mode.mode == 0x02010000 && mode.melee == 0x0d)
    {
        return;
    }
    let Some(stage) = CYCLE.lock().unwrap().enter() else {
        return;
    };
    let previous = ctx.registers[1].w();
    ctx.registers[1].set_w(stage);
    crate::logging::diagnostic(format_args!(
        "[LobbyStageProbe] entry native={} selected={} mode={:?}",
        previous,
        stage,
        crate::common::game_mode::current()
    ));
}

pub fn init() {
    let base = unsafe { getRegionAddress(Region::Text) } as usize;
    // Own 13.0.5 instructions, including exact BL target. A different game build
    // leaves the waiting stage untouched rather than accepting an offset alone.
    const EXPECTED: [u32; 5] = [0x320003e3, 0xaa1f03e0, 0xaa1403e2, 0x2a1f03e4, 0x97ffe6e6];
    let verified = unsafe {
        std::slice::from_raw_parts((base + 0x1790e18) as *const u32, EXPECTED.len()) == EXPECTED
            && std::slice::from_raw_parts((base + 0x25f7860) as *const u32, 5)
                == [0xd10103ff, 0xa90157f6, 0xa9024ff4, 0xa9037bfd, 0x9100c3fd]
    };
    crate::logging::diagnostic(format_args!("[LobbyStageProbe] verified={verified}"));
    if verified {
        skyline::install_hooks!(stage_entry, resource_request);
    }
}
