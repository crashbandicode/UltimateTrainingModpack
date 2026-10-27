//! The ordinary Training roster refresh needs a new portrait, not the native
//! Pokemon transformation flash. Event 0x1d writes both: 13.0.5 `1363c6c` sets
//! the animation byte, and `139df68..139df90` consumes it to play `pokemon`.
//! Runtime142's exact-store experiment preserved the portrait update without
//! the white/purple flash in both directions. No texture or animation is owned here.

use crate::native_fields;

unsafe fn animation_flag(hud: usize, owner: u32) -> Option<usize> {
    if owner > 1 || !native_fields::object(hud) {
        return None;
    }
    // The native owner-to-presentation mapping is independent of fighter kind.
    let row = (0..8).find(|&row| {
        native_fields::read::<u32>(hud, row * 0x1d8 + 0xd68) == Some(owner)
    })?;
    let slot = native_fields::read::<u32>(hud, row * 0x1d8 + 0xd60)?;
    if slot >= 8 {
        return None;
    }
    native_fields::child(hud, 0xb0)?.checked_add(slot as usize * 0x2d0 + 0x23b)
}

/// Deliver our synthetic event, preserving a transformation already requested
/// by the game. Clear only a newly set boolean on the same owner's same record.
/// No address survives this synchronous call; no per-frame work or allocation.
///
/// # Safety
/// Called only in ordinary Training on the native entry callback, with the HUD
/// listener held by the dispatch-depth protocol. The HUD and its child table
/// must stay live through `refresh`; that closure invokes only this listener's
/// native event functor, not a scene transition or arbitrary user callback.
/// Null structural checks do not themselves establish that lifetime.
pub(super) unsafe fn without_roster_animation(hud: usize, owner: u32, refresh: impl FnOnce()) {
    let idle = animation_flag(hud, owner)
        .filter(|&flag| native_fields::read::<u8>(flag, 0) == Some(0));
    refresh();
    if let Some(flag) = idle {
        if animation_flag(hud, owner) == Some(flag)
            && native_fields::read::<u8>(flag, 0) == Some(1)
        {
            native_fields::write_unchecked(flag, 0, 0u8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Vec<usize>, Vec<usize>) {
        let mut hud = vec![0usize; 0x1c00 / 8];
        let mut cards = vec![0usize; 8 * 0x2d0 / 8];
        let h = hud.as_mut_ptr() as usize;
        unsafe {
            native_fields::write_unchecked(h, 0xb0, cards.as_mut_ptr() as usize);
            for row in 0..8 {
                native_fields::write_unchecked(h, row * 0x1d8 + 0xd68, u32::MAX);
            }
            native_fields::write_unchecked(h, 0xd68, 1u32);
            native_fields::write_unchecked(h, 0xd60, 2u32);
        }
        (hud, cards)
    }

    #[test]
    fn synthetic_refresh_keeps_new_portrait_and_suppresses_only_new_flash() {
        let (mut hud, mut cards) = fixture();
        let h = hud.as_mut_ptr() as usize;
        let c = cards.as_mut_ptr() as usize;
        unsafe {
            without_roster_animation(h, 1, || {
                native_fields::write_unchecked(c, 2 * 0x2d0 + 0x23b, 1u8);
                native_fields::write_unchecked(c, 0, 0xfeedu64);
            });
            assert_eq!(native_fields::read::<u8>(c, 2 * 0x2d0 + 0x23b), Some(0));
            assert_eq!(native_fields::read::<u64>(c, 0), Some(0xfeed));
        }
    }

    #[test]
    fn already_pending_native_animation_is_preserved() {
        let (mut hud, mut cards) = fixture();
        let h = hud.as_mut_ptr() as usize;
        let c = cards.as_mut_ptr() as usize;
        unsafe {
            native_fields::write_unchecked(c, 2 * 0x2d0 + 0x23b, 1u8);
            without_roster_animation(h, 1, || {});
            assert_eq!(native_fields::read::<u8>(c, 2 * 0x2d0 + 0x23b), Some(1));
        }
    }

    #[test]
    fn mapping_change_does_not_clear_a_different_owners_record() {
        let (mut hud, mut cards) = fixture();
        let h = hud.as_mut_ptr() as usize;
        let c = cards.as_mut_ptr() as usize;
        unsafe {
            without_roster_animation(h, 1, || {
                native_fields::write_unchecked(c, 2 * 0x2d0 + 0x23b, 1u8);
                native_fields::write_unchecked(h, 0xd60, 3u32);
                native_fields::write_unchecked(c, 3 * 0x2d0 + 0x23b, 1u8);
            });
            for slot in [2, 3] {
                assert_eq!(native_fields::read::<u8>(c, slot * 0x2d0 + 0x23b), Some(1));
            }
        }
    }

    #[test]
    fn absent_hud_or_invalid_mapping_does_not_prevent_native_refresh() {
        let (mut hud, _cards) = fixture();
        let h = hud.as_mut_ptr() as usize;
        let mut calls = 0;
        unsafe {
            without_roster_animation(0, 1, || calls += 1);
            without_roster_animation(h, 4, || calls += 1);
            native_fields::write_unchecked(h, 0xd60, u32::MAX);
            without_roster_animation(h, 1, || calls += 1);
        }
        assert_eq!(calls, 3);
    }
}
