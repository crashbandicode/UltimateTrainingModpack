//! Revision 29 UI entry points: two inline anchors in Smash's UI manager, no `Layout::draw` hook.
//!
//! The removed hook replaced `nn::ui2d::Layout`'s `+0x98` entry (main+0x4b620, the per-frame
//! calculate). Smash reaches that entry from exactly two places, both on a holder's layout. Each
//! anchor sits on the instruction before that virtual call, so `tick` runs at the same point, on
//! the same thread and with the same cadence as the old hook. In particular it runs after the
//! holder's own animate and before its calculate.
//!
//! | Anchor | Site (13.0.5) | Thread | Holder |
//! | --- | --- | --- | --- |
//! | `ui_pass_calculate` | main+0x375efa8 in the per-frame pass | UI job (worker 1) | x23 |
//! | `ui_register_calculate` | main+0x3784ad4 in the manager insert | main | x21 |
//!
//! Both sites are pinned to five words and panic (version report) on any mismatch: there is no
//! fallback and no skip. See `docs/REVISION29_DRAW_DECOUPLING.md`.

use std::sync::atomic::{AtomicBool, Ordering};

use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use training_mod_macros::ui_anchor::{
    decode_adrp_ldr, verify_site, AnchorSite, SiteVerdict, UI_MANAGER_ADRP_DELTA, UI_PASS_CALCULATE,
    UI_REGISTER_CALCULATE,
};

use super::layout_registry;
use crate::common::offsets::{
    OFFSET_UI_MANAGER_PROLOGUE, OFFSET_UI_PASS_CALCULATE, OFFSET_UI_REGISTER_CALCULATE,
};

static FIRST_PASS_LOGGED: AtomicBool = AtomicBool::new(false);

fn text() -> usize {
    unsafe { getRegionAddress(Region::Text) as usize }
}

unsafe fn word(address: usize) -> u32 {
    (address as *const u32).read_volatile()
}

/// The `install.log` pattern of ssbu-dispatcher-backoff, except that every failure panics.
fn verify(site: &AnchorSite, offset: usize) {
    let base = text() + offset;
    let words: [u32; 5] = std::array::from_fn(|index| unsafe { word(base + index * 4) });
    let verdict = verify_site(words, site.pin);
    let report = format!(
        "{} site=main+{:#x} words={:08x} {:08x} {:08x} {:08x} {:08x} result={}",
        site.name,
        offset,
        words[0],
        words[1],
        words[2],
        words[3],
        words[4],
        verdict.as_str()
    );
    crate::logging::diagnostic(format_args!("[UiAnchor] {report}"));
    if verdict != SiteVerdict::Installed {
        panic!(
            "Revision 29 UI anchor failed: {}. This build needs an unpatched Smash 13.0.5 UI manager.",
            report
        );
    }
}

pub fn install() {
    verify(&UI_PASS_CALCULATE, *OFFSET_UI_PASS_CALCULATE);
    verify(&UI_REGISTER_CALCULATE, *OFFSET_UI_REGISTER_CALCULATE);

    let pc = text() + *OFFSET_UI_MANAGER_PROLOGUE + UI_MANAGER_ADRP_DELTA;
    let (adrp, ldr) = unsafe { (word(pc), word(pc + 4)) };
    let slot = decode_adrp_ldr(pc as u64, adrp, ldr).unwrap_or_else(|| {
        panic!("Revision 29 UI manager global did not decode at main+{:#x}: {adrp:08x} {ldr:08x}", pc - text())
    }) as usize;
    layout_registry::set_manager_slot(slot);
    crate::logging::diagnostic(format_args!(
        "[UiAnchor] manager_slot=main+{:#x} from main+{:#x} words={adrp:08x} {ldr:08x}",
        slot.wrapping_sub(text()),
        pc - text()
    ));

    skyline::install_hooks!(
        training_modpack_ui_pass_calculate,
        training_modpack_ui_register_calculate
    );
}

/// Per-frame pass, before `blr` to the holder layout's `+0x98` (x23 = holder).
#[skyline::hook(offset = *OFFSET_UI_PASS_CALCULATE, inline)]
unsafe fn training_modpack_ui_pass_calculate(ctx: &mut InlineCtx) {
    let Some(layout) = layout_registry::layout_of(ctx.registers[23].x() as *const u8) else {
        return;
    };
    if !FIRST_PASS_LOGGED.swap(true, Ordering::Relaxed) {
        crate::logging::diagnostic(format_args!(
            "[UiAnchor] first pass holders={}",
            layout_registry::holders().len()
        ));
    }
    super::tick(layout);
}

/// Manager insert, before the initial calculate of a newly registered holder (x21 = holder).
#[skyline::hook(offset = *OFFSET_UI_REGISTER_CALCULATE, inline)]
unsafe fn training_modpack_ui_register_calculate(ctx: &mut InlineCtx) {
    let Some(layout) = layout_registry::layout_of(ctx.registers[21].x() as *const u8) else {
        return;
    };
    super::tick(layout);
    // Live evidence that the registry resolves the modpack's own layouts by name.
    if let Some(name @ (b"info_training" | b"info_waiting")) = layout_registry::layout_name(layout) {
        let name = std::str::from_utf8(name).unwrap_or_default();
        crate::logging::diagnostic(format_args!(
            "[UiAnchor] registered layout={} holders={} registry_hit={}",
            name,
            layout_registry::holders().len(),
            layout_registry::find(name) == Some(layout)
        ));
    }
}
