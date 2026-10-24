//! Private 13.0.5 candidate: reuse native-generated values, not a saved fighter.
//! Call only at the owned, drained parent state3 -> state7 handoff. All native
//! addresses are callback-local; only the complete value::Row may be retained.
use super::{inventory::{self, MiiClass, TaggedId}, native, value};
use crate::mii_native_access::{self as access, PoolLock};
use std::{mem::transmute, sync::atomic::{AtomicBool, Ordering}};

static AVAILABLE: AtomicBool = AtomicBool::new(false);

// Retained mii-empty39 caller contracts plus entry signatures. Import names are
// unresolved; these declarations follow the actual registers/storage at callers.
const CODE: &[(usize, usize, u64)] = &[
    (0x3757894, 0x78, 0x88278f5c3c546c62),
    (0x18142c4, 0xc0, 0xb2c77b8c37bc1358),
    (0x18127dc, 0x68, 0xf371cbafc7244863),
    (0x180fc3c, 0x24, 0x6eb8a53d0f252967),
    (0x19e0714, 0x30, 0x46509e598bd0265d),
    (0x19e03d0, 0xac, 0x26436a097b273828),
    (0x1815010, 0x40, 0x553efc81e71901ed),
    (0x32631a0, 0x10, 0x8ee07ae6828e4523),
    (0x32874f0, 0x10, 0x1d1089bdf173ac75),
    (0x3285980, 0x10, 0x07e4b593beaf36fb),
    (0x32845a0, 0x10, 0x79c94938c05e6096),
    (0x39c19c0, 0x20, 0x0be276117a679259),
    (0x39c2930, 0x20, 0x82449120ff3386fa),
    (0x39c2990, 0x20, 0x5bfb9ecc914ce455),
];

pub(super) unsafe fn init(base: usize) {
    let valid = CODE.iter().all(|&(offset, len, hash)| {
        inventory::fnv1a64(std::slice::from_raw_parts((base + offset) as *const u8, len)) == hash
    });
    AVAILABLE.store(valid, Ordering::Release);
    if !valid { crate::logging::diagnostic(format_args!("[MiiEditor] native_default_code_mismatch")); }
}

pub(super) fn available() -> bool { AVAILABLE.load(Ordering::Acquire) }

#[derive(Clone, Copy, PartialEq, Eq)]
struct Database {
    holder: usize,
    db: usize,
    class_table: usize,
    hat_data: usize,
    outfit_table: usize,
}

impl Database {
    unsafe fn bind(base: usize) -> Result<Self, &'static str> {
        let holder = pointer(base, access::UI_DB)?;
        let db = pointer(holder, 8)?;
        let class_table = pointer(db, 0x168)?;
        let hat = pointer(db, access::HAT_TABLE)?;
        let hat_data = pointer(hat, 8)?;
        // 19e03d0 passes db+240 by address to 3285980. Check the live
        // table pointer at that field; do not pass its pointee instead.
        let outfit_table = pointer(db, 0x240)?;
        Ok(Self { holder, db, class_table, hat_data, outfit_table })
    }
}

unsafe fn pointer(base: usize, offset: usize) -> Result<usize, &'static str> {
    let p = native::read(base, offset);
    if native::aligned(p) { Ok(p) } else { Err("default_native_binding_missing") }
}

/// Ordinal zero is one of the six *fixed producer outputs*, not vector record
/// 26, a saved fighter, or a masked canonical appearance. No producer is called.
unsafe fn charinfo(base: usize) -> Result<(usize, usize, [u8; inventory::CHARINFO_LEN]), &'static str> {
    let holder = pointer(base, access::POOL_HOLDER)?;
    let g = pointer(holder, 0)?;
    let _lock = PoolLock::acquire(base, g);
    if native::read::<usize>(base, access::POOL_HOLDER) != holder
        || native::read::<usize>(holder, 0) != g { return Err("default_pool_changed"); }
    let bytes = native::read::<[u8; inventory::CHARINFO_LEN]>(g, 0x22c0);
    if native::read::<usize>(base, access::POOL_HOLDER) != holder
        || native::read::<usize>(holder, 0) != g { return Err("default_pool_changed"); }
    Ok((holder, g, bytes))
}

pub(super) unsafe fn materialize(base: usize, class: MiiClass) -> Result<value::Row, &'static str> {
    if !available() { return Err("default_native_code_unqualified"); }
    // The caller already owns the drained parent state3/desired4 handoff.
    // Preview readiness is checked when requesting a preview, after editor
    // entry; the native default/table readers do not consume that state.
    if native::read::<u32>(base, native::MODE) != 0x04050000
        || native::read::<u8>(base, native::DRAFT + 0x1c4) != 0 {
        return Err("default_editor_not_ready");
    }
    let db = Database::bind(base)?;
    let (pool_holder, pool, bytes) = charinfo(base)?;
    #[repr(C, align(8))]
    struct CharInfo([u8; inventory::CHARINFO_LEN]);
    let charinfo = CharInfo(bytes);

    // At 19e0408..19e0430 and 18127fc..1812824, x0 names sp+8;
    // sp+60 starts saved registers. Reserve that full 0x58-byte storage bound,
    // aligned to 8; no destructor call occurs in either retained caller.
    #[repr(C, align(8))]
    struct Wrapper([u8; 0x58]);
    let mut wrapper = Wrapper([0; 0x58]);
    let wrap: unsafe extern "C" fn(*mut u8, *const u8) = transmute(base + 0x39c2930);
    let valid: unsafe extern "C" fn(*const u8) -> u32 = transmute(base + 0x39c2940);
    let colour: unsafe extern "C" fn(*const u8) -> u32 = transmute(base + 0x39c2990);
    wrap(wrapper.0.as_mut_ptr(), charinfo.0.as_ptr());
    if valid(wrapper.0.as_ptr()) & 1 == 0 { return Err("default_charinfo_invalid"); }
    wrap(wrapper.0.as_mut_ptr(), charinfo.0.as_ptr());
    let colour = colour(wrapper.0.as_ptr());
    let colour = if colour & 0xfc > 11 { 11 } else { colour as u8 };

    // x0 table, x1 low40 key, x0 tagged result at the retained callers.
    let class_lookup: unsafe extern "C" fn(usize, u64) -> u64 = transmute(base + 0x32631a0);
    let hat_lookup: unsafe extern "C" fn(usize, u64) -> u64 = transmute(base + 0x32874f0);
    let chara = TaggedId(class_lookup(db.class_table, class.chara_hash40()));
    let hat = TaggedId(hat_lookup(db.hat_data, 0x0fa9efa5ea));
    if MiiClass::from_chara_id(chara) != Ok(class) || chara.row_hint() == u16::MAX
        || hat.tag() != inventory::TAG_HAT || hat.hash40() != 0x0fa9efa5ea
        || hat.row_hint() == u16::MAX { return Err("default_native_lookup_invalid"); }

    // 19e03d0 reads only class at +1a0 and CharInfo at +148 from x0;
    // SelectType stores its x0 return directly as the outfit. This scratch
    // buffer is never published as a draft or passed to an editor converter.
    #[repr(C, align(8))]
    struct Scratch(value::Row);
    let mut scratch = Scratch([0; training_mod_consts::mii::saved::HEADER_LEN]);
    scratch.0[0x148..0x1a0].copy_from_slice(&charinfo.0);
    scratch.0[0x1a0..0x1a8].copy_from_slice(&chara.0.to_le_bytes());
    let outfit: unsafe extern "C" fn(*const u8) -> u64 = transmute(base + 0x19e03d0);
    let outfit = TaggedId(outfit(scratch.0.as_ptr()));
    if Database::bind(base)? != db { return Err("default_ui_database_changed"); }
    if native::read::<usize>(base, access::POOL_HOLDER) != pool_holder
        || native::read::<usize>(pool_holder, 0) != pool { return Err("default_pool_changed"); }
    if outfit.tag() != inventory::TAG_OUTFIT || outfit.hash40() == 0
        || outfit.row_hint() == u16::MAX { return Err("default_outfit_invalid"); }

    // Native New resets specials to zero; SelectMii supplies colour and voice37.
    // Apply the ordinary owned-row path's existing "New Mii" name policy.
    // All resource tags and unmasked CharInfo are actual native outputs.
    let mut name_units = [0; 10];
    for (unit, ch) in name_units.iter_mut().zip("New Mii".encode_utf16()) { *unit = ch; }
    let raw = inventory::RawSlot {
        index: 0, slot_id: 0, name_len: 7, name_units,
        colour, voice: 37, specials: [0; 4], charinfo: charinfo.0,
        tag_outfit: outfit, tag_hat: hat, tag_chara: chara,
    };
    let config = inventory::classify(&raw).map_err(|_| "default_owned_values_invalid")?;
    value::from_seed(&config)
}
