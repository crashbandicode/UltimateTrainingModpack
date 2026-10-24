//! Shared read-only saved-row binding; no hook registration or native mutation.
use skyline::hooks::{getRegionAddress, Region};

// The live FighterEntry callback and binding checks below supply lifetime;
// structural pointer checks alone are not permission to use an old entry.
use crate::native_fields::{object as aligned, read_unchecked as rd};

/// A live FighterEntry bound to its saved player entry through the game's own lookup
/// (`FighterManager::get_fighter_entry`, as `common::try_get_module_accessor` does), then verified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundEntry {
    pub id: u32,
    pub e: usize,
    pub kind: u32,
}

/// Returns the saved row `E = main + 0x52c5730 + 0x260*id` only when the row agrees with the live
/// entry on kind (`s8 E+8 == FE+0x18`), id (`u32 E+0x14`), record index (`u16 E+0x1a8 == FE+0x46`)
/// and all four specials (`u8 E+0x1b2.. == u32 D+0x390..`). `E` is static; `FE`/`D` are re-read
/// from the live callback argument on every call and never retained.
pub unsafe fn bind_saved_entry(entry: *const u8) -> Option<BoundEntry> {
    let fe = entry as usize;
    if !aligned(fe) {
        return None;
    }
    let address = training_mod_sync::read(&crate::common::FIGHTER_MANAGER_ADDR)
        as *mut *mut smash::app::FighterManager;
    if address.is_null() || (*address).is_null() {
        return None;
    }
    let mgr = *address;
    let id = (0u32..8).find(|id| {
        smash::app::lua_bind::FighterManager::get_fighter_entry(mgr, smash::app::FighterEntryID(*id as i32)) as usize == fe
    })?;
    let base = getRegionAddress(Region::Text) as usize;
    let e = base + 0x52c5730 + 0x260 * id as usize;
    let kind: u32 = rd(fe, 0x18);
    let d: usize = rd(fe, 0xf8);
    if !aligned(d) {
        return None;
    }
    let specials_d: [u32; 4] = [rd(d, 0x390), rd(d, 0x394), rd(d, 0x398), rd(d, 0x39c)];
    let specials_e: [u8; 4] = [rd(e, 0x1b2), rd(e, 0x1b3), rd(e, 0x1b4), rd(e, 0x1b5)];
    let consistent = i64::from(rd::<i8>(e, 8)) == i64::from(kind)
        && rd::<u32>(e, 0x14) == id
        && rd::<u16>(e, 0x1a8) == rd::<u16>(fe, 0x46)
        && specials_e.iter().zip(specials_d.iter()).all(|(a, b)| u32::from(*a) == *b);
    consistent.then_some(BoundEntry { id, e, kind })
}

