//! Version-checked scene observations for the optional waiting-room adapter.

use std::sync::atomic::{AtomicUsize, Ordering};

use skyline::hooks::{getRegionAddress, Region};
use skyline::nn::ro::LookupSymbol;
// Globals are pinned by the verified main image; popup objects are borrowed on
// the UI callback. Preserve the existing nonzero-only child checks below.
use crate::native_fields::read_unchecked as read;

static BASE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    pub initialized: bool,
    pub melee: u8,
    pub mode: u32,
}

pub fn init() -> bool {
    // Owned 13.0.5 main, build ID 21450c647b8c5940ed5b68218ea128fd3c5d597d.
    const CODE: [u8; 32] = [
        0xfd, 0x7b, 0xbf, 0xa9, 0xfd, 0x03, 0x00, 0x91, 0x08, 0xe5, 0x01, 0xf0, 0x08, 0x61, 0x13,
        0x91, 0x08, 0xfd, 0xdf, 0x08, 0x48, 0x01, 0x00, 0x36, 0xa8, 0xe2, 0x01, 0x90, 0x08, 0xc1,
        0x46, 0x39,
    ];
    unsafe {
        let base = getRegionAddress(Region::Text) as usize;
        let mut predicate = 0usize;
        LookupSymbol(
            &mut predicate,
            b"_ZN3app9smashball16is_training_modeEv\0".as_ptr(),
        );
        if predicate != base + 0x1671650
            || std::slice::from_raw_parts(predicate as *const u8, CODE.len()) != CODE
        {
            return false;
        }
        // AppPopupManager's is_busy Lua binding: singleton, state fields and
        // presentation-idle query. Keep this independent of pane animation guesses.
        const POPUP_BINDING: [u32; 10] = [
            0x9000fe48, 0xf9441108, 0xf9400514, 0xf9408a88, 0xaa0003f3, 0xf9404100, 0x97ae214f,
            0xb940fa88, 0x7100011f, 0x39450a89,
        ];
        if std::slice::from_raw_parts((base + 0x33678dc) as *const u32, 10) != POPUP_BINDING {
            return false;
        }
        BASE.store(base, Ordering::Release);
    }
    true
}

pub fn current() -> Option<Mode> {
    let base = BASE.load(Ordering::Acquire);
    if base == 0 {
        return None;
    }
    unsafe {
        Some(Mode {
            initialized: read::<u8>(base, 0x53144d8) & 1 != 0,
            melee: read(base, 0x52c51b0),
            mode: read(base, 0x53050f0),
        })
    }
}

/// Read the game's popup-manager busy condition on the UI draw thread.
/// Missing manager state is unknown; callers must not capture input then.
pub unsafe fn popup_busy() -> Option<bool> {
    let base = BASE.load(Ordering::Acquire);
    if base == 0 {
        return None;
    }
    unsafe fn pointer(at: usize) -> Option<usize> {
        let value: usize = read(at, 0);
        (value != 0).then_some(value)
    }
    let manager = pointer(base + 0x532f820)?;
    let state = pointer(manager + 8)?;
    if read::<u32>(state, 0xf8) != 0
        || read::<u8>(state, 0x142) != 0
    {
        return Some(true);
    }
    let presentation = pointer(pointer(state + 0x110)? + 0x80)?;
    // The native binding calls this read-only presentation/animation query.
    let idle: unsafe extern "C" fn(usize) -> u32 = std::mem::transmute(base + 0x1eefe30);
    Some(idle(presentation) & 1 == 0)
}
