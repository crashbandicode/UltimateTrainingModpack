//! Name -> `Layout*` over Smash's own UI registry (revision 29).
//!
//! Every Smash UI layout lives in a holder (`+0x458` is its `nn::ui2d::Layout*`). The UI manager
//! singleton keeps the live holders in a priority-sorted `std::vector<Holder*>` at `+0x0`/`+0x8`,
//! and its per-frame pass walks that vector to animate and calculate each layout. This module
//! reads the same vector; it never writes game memory. Call it only from the UI anchors, which
//! run where the game itself owns the vector (the pass, or the insert that just updated it).

use std::ffi::CStr;
use std::sync::atomic::{AtomicUsize, Ordering};

use skyline::nn::ui2d::Layout;
use training_mod_macros::ui_anchor::{find_by_name, HOLDER_LAYOUT, MANAGER_HOLDERS};

/// Absolute address of the manager-global slot (main+0x593ba90 on 13.0.5), decoded at install.
static MANAGER_SLOT: AtomicUsize = AtomicUsize::new(0);

pub(super) fn set_manager_slot(slot: usize) {
    MANAGER_SLOT.store(slot, Ordering::Release);
}

/// The game loads the manager as `**slot`, exactly like its own `adrp`/`ldr`/`ldr` sequence.
unsafe fn manager() -> Option<*const u8> {
    let slot = MANAGER_SLOT.load(Ordering::Acquire) as *const *const *const u8;
    if slot.is_null() {
        return None;
    }
    let instance = *slot;
    if instance.is_null() {
        return None;
    }
    let manager = *instance;
    (!manager.is_null()).then_some(manager)
}

/// Live holders in the manager's priority order (empty before the manager exists).
pub unsafe fn holders() -> &'static [*const u8] {
    let Some(manager) = manager() else { return &[] };
    let vector = manager.add(MANAGER_HOLDERS) as *const *const *const u8;
    let (begin, end) = (*vector, *vector.add(1));
    if begin.is_null() || end < begin {
        return &[];
    }
    std::slice::from_raw_parts(begin, end.offset_from(begin) as usize)
}

/// A holder's layout; `None` for a null holder or layout.
pub unsafe fn layout_of(holder: *const u8) -> Option<*mut Layout> {
    if holder.is_null() {
        return None;
    }
    let layout = *(holder.add(HOLDER_LAYOUT) as *const *mut Layout);
    (!layout.is_null()).then_some(layout)
}

/// `Layout::layout_name`, the field the removed draw hook compared.
pub unsafe fn layout_name<'a>(layout: *mut Layout) -> Option<&'a [u8]> {
    let name = (*layout).layout_name;
    (!name.is_null()).then(|| CStr::from_ptr(name).to_bytes())
}

/// The first live layout named `name`, in the manager's priority order.
pub unsafe fn find(name: &str) -> Option<*mut Layout> {
    find_by_name(
        holders()
            .iter()
            .filter_map(|holder| layout_of(*holder))
            .map(|layout| (layout_name(layout), layout)),
        name,
    )
}
