//! Native waiting-stage entry selection. No per-frame mutation or saved pointer.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use training_mod_consts::{OnOff, MENU};
use training_mod_sync::read;

use crate::lobby_stage_cycle::{self, StageCycle, WaitingStageFamily};

// Each native waiting family owns its preload until its own scene consumes it.
// An abandoned Quickplay search cannot lend its prepared descriptor to LDN.
static CYCLES: Mutex<[StageCycle; 2]> = Mutex::new([StageCycle::new(), StageCycle::new()]);
static QUICKPLAY_VERIFIED: AtomicBool = AtomicBool::new(false);
static RESOURCE_LINES: AtomicUsize = AtomicUsize::new(0);

fn cycle_index(mode: u32) -> Option<usize> {
    match lobby_stage_cycle::family(mode)? {
        WaitingStageFamily::LocalWireless => Some(0),
        WaitingStageFamily::Quickplay if QUICKPLAY_VERIFIED.load(Ordering::Acquire) => Some(1),
        WaitingStageFamily::Quickplay => None,
    }
}

pub(super) fn qualified_waiting(mode: u32, melee: u8) -> bool {
    lobby_stage_cycle::waiting(mode, melee) && cycle_index(mode).is_some()
}

fn prepare_entry(index: usize) -> Option<u32> {
    let menu = read(&MENU);
    let stages = menu.lobby_stages.stages();
    #[cfg(feature = "practice-stage-request-alpha")]
    let prefs = crate::common::menu::stage_preferences();
    let mut cycles = CYCLES.lock().unwrap();
    let cycle = &mut cycles[index];
    if menu.lobby_practice != OnOff::ON {
        cycle.cancel_preload();
        return None;
    }
    #[cfg(feature = "practice-stage-request-alpha")]
    let selected = if prefs.mode == training_mod_tui::stage_menu::Mode::Direct {
        cycle.prepare_direct(prefs.direct_stage)
    } else if stages.is_empty() { None } else { cycle.prepare(&stages) };
    #[cfg(not(feature = "practice-stage-request-alpha"))]
    let selected = if stages.is_empty() { None } else { cycle.prepare(&stages) };
    if selected.is_none() { cycle.cancel_preload(); }
    selected
}

#[skyline::hook(offset = 0x25f7860, inline)]
unsafe fn resource_request(ctx: &mut InlineCtx) {
    #[cfg(feature = "practice-stage-observer")]
    crate::practice_stage_observer::observe_resource_request(ctx);
    let Some(mode) = crate::common::game_mode::current() else {
        return;
    };
    let Some(index) = cycle_index(mode.mode) else { return; };
    let config = ctx.registers[1].x() as *const u8;
    if config.is_null() || config as usize & 7 != 0 {
        return;
    }
    let base = getRegionAddress(Region::Text) as usize;
    let caller = (ctx.registers[30].x() as usize).wrapping_sub(base);
    // The native caller owns this synchronous stack descriptor throughout the
    // callback. Alignment was checked above; the fingerprint proves its layout.
    let kind = crate::native_fields::read_unchecked::<u32>(config as usize, 0);
    let hash = crate::native_fields::read_unchecked::<u64>(config as usize, 8);
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
    // Proven waiting-entry preload only. The native stack configuration is
    // consumed synchronously by this request; no pointer survives the callback.
    if lobby_stage_cycle::entry_preload(mode.mode, caller)
        && ctx.registers[2].w() == 2
        && kind == 311
        && hash == 0x165a48f367
    {
        let Some(stage) = prepare_entry(index) else { return; };
        let record = (base + 0x454a7e8 + stage as usize * 72) as *const u8;
        let native_hash = crate::native_fields::read_unchecked::<u64>(record as usize, 24) & 0xffffffffff;
        crate::native_fields::write_unchecked(config as usize, 0, stage);
        crate::native_fields::write_unchecked(config as usize, 8, native_hash);
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
    // Keep an already preloaded selection even if preferences change meanwhile.
    let Some(mode) = crate::common::game_mode::current() else { return; };
    if !mode.initialized || !qualified_waiting(mode.mode, mode.melee) {
        return;
    }
    let Some(index) = cycle_index(mode.mode) else { return; };
    let previous = ctx.registers[1].w();
    let prepared = CYCLES.lock().unwrap()[index].enter();
    let stage = if let Some(stage) = prepared {
        stage
    } else {
        // Retained Quickplay returns omit the driver's stack preload. Run109
        // captured this constructor with w1=311 and all four resource slots
        // empty, before the generic 0x14f57c4 loader. Change only the setter's
        // argument; that loader must build its resource from native config.
        // Never rewrite its live descriptor or replace an occupied slot.
        if !lobby_stage_cycle::unprepared_quickplay_entry(mode.mode, mode.melee, previous, true) {
            return;
        }
        let base = getRegionAddress(Region::Text) as usize;
        let empty = crate::native_fields::child(base, 0x532a9d0)
            .and_then(|owner| crate::native_fields::read::<usize>(owner, 2 * 8)) == Some(0);
        if !empty { return; }
        if prepare_entry(index).is_none() { return; }
        let Some(stage) = CYCLES.lock().unwrap()[index].enter() else { return; };
        stage
    };
    ctx.registers[1].set_w(stage);
    crate::logging::diagnostic(format_args!(
        "[LobbyStageProbe] entry native={} selected={} mode={:?}",
        previous,
        stage,
        mode
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
    // Quickplay's CSS/return drivers construct stage311/hash165a48f367 in slot2,
    // then enter the same PracticeScene setup as LDN. Verify the callers as well
    // as the shared callee so normal match preloads cannot acquire this gate.
    let quickplay_checks: &[(usize, &[u32])] = &[
        // Generic loader used after a retained waiting owner reconstructs its
        // scene. It selects resource slot2 for melee0d, then reads the stage
        // descriptor written by the native setter; it does not construct one.
        (0x14f5754, &[
            0x39418308, 0x7100351f, 0x1a9f17e8, 0xb001f1b4,
            0x531f7913, 0xf944ea88, 0xf8735908, 0xb40000a8,
            0xb9400108, 0x321f0108, 0x71000d1f, 0x54000220,
        ]),
        (0x14f5794, &[
            0x9001ee88, 0xf942e108, 0xf100091f, 0x5400a5a2,
            0x52801609, 0x9001ee8a, 0x9106014a, 0x9b092908,
            0xf944ea80, 0x91114101, 0x2a1303e2, 0x94440828,
        ]),
        (0x236841c, &[
            0xb9415269, 0x7100113f, 0x54000461, 0x528026e9, 0xb90063e9,
            0xd0017e09, 0xf944e920, 0xd29e6cea, 0xf2ab490a, 0x3901c3e8,
            0xd2dfe008, 0xf2c002ca, 0xf2fb3fe8, 0x910183e1, 0x321f03e2,
        ]),
        (0x23684a8, &[0xf803429f, 0x940a3ced, 0xa9547bfd]),
        (0x2369174, &[
            0xb94152a9, 0x7100113f, 0x54000461, 0x528026e9, 0xb9005be9,
            0xb0017e09, 0xf944e920, 0xd29e6cea, 0xf2ab490a, 0x3901a3e8,
            0xd2dfe008, 0xf2c002ca, 0xf2fb3fe8, 0x910163e1, 0x321f03e2,
        ]),
        (0x2369200, &[0xf808c3ff, 0x940a3997, 0x910803ff]),
        (0x236c9c0, &[
            0xd0017de9, 0xf944e920, 0x528026ea, 0xb9001bea, 0xd29e6cea, 0xf2ab490a,
            0x3900a3e8, 0xd2dfe008, 0xf2c002ca, 0xf2fb3fe8, 0x910063e1, 0x321f03e2,
        ]),
        (0x236ca40, &[0xf804c3ff, 0x940a2b87, 0xf940d7e0]),
        (0x237cc4c, &[
            0xb9421aa9, 0x7100193f, 0x54000461, 0x528026e9, 0xb90033e9,
            0xd0017d69, 0xf944e920, 0xd29e6cea, 0xf2ab490a, 0x390103e8,
            0xd2dfe008, 0xf2c002ca, 0xf2fb3fe8, 0x9100c3e1, 0x321f03e2,
        ]),
        (0x237ccd8, &[0xf80643ff, 0x9409eae1, 0x910943ff]),
        (0x237ebd4, &[0x97d032bf, 0x97d04512]),
    ];
    let quickplay_verified = verified && quickplay_checks.iter().all(|(offset, words)| unsafe {
        std::slice::from_raw_parts((base + offset) as *const u32, words.len()) == *words
    });
    QUICKPLAY_VERIFIED.store(quickplay_verified, Ordering::Release);
    crate::logging::diagnostic(format_args!("[LobbyStageProbe] verified={verified} quickplay={quickplay_verified}"));
    if verified {
        skyline::install_hooks!(stage_entry, resource_request);
    }
}
