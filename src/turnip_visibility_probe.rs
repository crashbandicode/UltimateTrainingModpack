//! DEVELOPMENT PROPOSAL ONLY: unbuilt and unexecuted; no face classifier.
//! Replacement for the disproven group observer, reusing its 24/20 limits.
//! Logs concrete MotionModule material targets. Reads STATIC frame only when
//! exact vtable, native targets and complete pure-getter code all match.
//! Caller enforces permitted native P1 callback and no pending item experiment.

use smash::app::{self, lua_bind::*, sv_battle_object};
use smash::lib::lua_const::*;
use std::convert::TryFrom;

const MAX_OBSERVATIONS: u8 = 24;
const SETTLE_CALLBACKS: u8 = 20;
const MOTION_VTABLE: usize = 0x5078160;
const MATERIAL_SETTER: usize = 0x49f0f0;
const MATERIAL_GETTER: usize = 0x49f120;

#[derive(Default)]
pub struct Observer {
    fingerprint_ok: Option<bool>,
    held: Option<(u32, u8)>,
    observations: u8,
}

unsafe fn code_matches(text: usize, rodata: usize, offset: usize, words: &[u32]) -> bool {
    let Some(address) = text.checked_add(offset) else {
        return false;
    };
    let Some(end) = address.checked_add(words.len() * 4) else {
        return false;
    };
    address >= text
        && end <= rodata
        && words
            .iter()
            .enumerate()
            .all(|(i, word)| ((address + i * 4) as *const u32).read() == *word)
}

unsafe fn supported_code(text: usize, rodata: usize) -> bool {
    // Exact public wrappers plus entire concrete setter/getter. The getter
    // performs no writes/calls/callbacks and reads module+0x224 only for kind 0.
    code_matches(
        text,
        rodata,
        0x205d0a0,
        &[0xf9404400, 0xf9400008, 0xf941d902, 0xd61f0040],
    ) && code_matches(
        text,
        rodata,
        0x205d0d0,
        &[0xf9404400, 0xf9400008, 0xf941e502, 0xd61f0040],
    ) && code_matches(
        text,
        rodata,
        0x49f0f0,
        &[0x34000041, 0xd65f03c0, 0xbd022400, 0xd65f03c0],
    ) && code_matches(
        text,
        rodata,
        0x49f120,
        &[0x34000061, 0x1e2703e0, 0xd65f03c0, 0xbd422400, 0xd65f03c0],
    )
}

fn aligned(address: usize) -> bool {
    address != 0 && address.is_multiple_of(std::mem::align_of::<usize>())
}

unsafe fn targets(
    item: *mut app::BattleObjectModuleAccessor,
    text: usize,
) -> Option<(usize, usize, usize, Option<f32>)> {
    let module = item.cast::<u8>().add(0x88).cast::<usize>().read();
    if !aligned(module) {
        return None;
    }
    let vtable = (module as *const usize).read();
    if !aligned(vtable) {
        return None;
    }
    let set_slot = vtable.checked_add(0x3b0)?;
    let get_slot = vtable.checked_add(0x3c8)?;
    let setter = (set_slot as *const usize).read();
    let getter = (get_slot as *const usize).read();
    let frame = if vtable == text.checked_add(MOTION_VTABLE)?
        && setter == text.checked_add(MATERIAL_SETTER)?
        && getter == text.checked_add(MATERIAL_GETTER)?
    {
        // Native Lua caller consumes S0; the pin incorrectly declares u64.
        // Named MATERIAL_ANIME_KIND_STATIC is 0, also checked by the native
        // getter. No other channel is queried. Caller verified complete code.
        let get: unsafe extern "C" fn(*const u8, i32) -> f32 = std::mem::transmute(getter);
        Some(get(module as *const u8, *MATERIAL_ANIME_KIND_STATIC))
    } else {
        // Unexpected concrete implementation: log targets only, never call.
        None
    };
    Some((vtable, setter, getter, frame))
}

impl Observer {
    pub const fn new() -> Self {
        Self {
            fingerprint_ok: None,
            held: None,
            observations: 0,
        }
    }

    /// Caller must verify native Training/readiness/P1/closed menu, no character
    /// switch/save/replay, and no pending item action. Only IDs and counters
    /// persist. IDs limit logs; they never authorize a later pointer read.
    pub unsafe fn on_frame(&mut self, body: *mut app::BattleObjectModuleAccessor) {
        if self.observations >= MAX_OBSERVATIONS || self.fingerprint_ok == Some(false) {
            return;
        }
        if body.is_null() || !ItemModule::is_have_item(body, 0) {
            self.held = None;
            return;
        }
        let Ok(id) = u32::try_from(ItemModule::get_have_item_id(body, 0)) else {
            return;
        };
        if !sv_battle_object::is_active(id) {
            return;
        }
        let item = sv_battle_object::module_accessor(id);
        if item.is_null()
            || (*item).battle_object_id != id
            || app::utility::get_category(&mut *item) != *BATTLE_OBJECT_CATEGORY_ITEM
        {
            return;
        }
        let kind = app::utility::get_kind(&mut *item);
        if kind != *ITEM_KIND_PEACHDAIKON && kind != *ITEM_KIND_DAISYDAIKON {
            self.held = None;
            return;
        }
        let callbacks = match self.held {
            Some((previous, callbacks)) if previous == id => callbacks,
            _ => 0,
        };
        if callbacks > SETTLE_CALLBACKS {
            return;
        }
        self.held = Some((id, callbacks + 1));
        if callbacks < SETTLE_CALLBACKS {
            return;
        }
        self.observations += 1;

        let text = skyline::hooks::getRegionAddress(skyline::hooks::Region::Text) as usize;
        let rodata = skyline::hooks::getRegionAddress(skyline::hooks::Region::Rodata) as usize;
        if self.fingerprint_ok.is_none() {
            self.fingerprint_ok = Some(supported_code(text, rodata));
        }
        if self.fingerprint_ok != Some(true) {
            crate::logging::diagnostic(format_args!("[TurnipMaterialDispatch] unsupported_code"));
            return;
        }
        let variation = WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_VARIATION);
        match targets(item, text) {
            Some((vtable, setter, getter, frame)) => {
                let setter_rel = setter.checked_sub(text).filter(|_| setter < rodata);
                let getter_rel = getter.checked_sub(text).filter(|_| getter < rodata);
                crate::logging::diagnostic(format_args!(
                    "[TurnipMaterialDispatch] sample={} id=0x{id:x} kind={kind} variation={variation} main_text=0x{text:x} main_rodata=0x{rodata:x} motion_vtable=0x{vtable:x} set_frame_material=0x{setter:x} set_main_relative={setter_rel:x?} get_frame_material=0x{getter:x} get_main_relative={getter_rel:x?} static_frame={frame:?} static_frame_bits={:x?}",
                    self.observations, frame.map(f32::to_bits)));
            }
            None => crate::logging::diagnostic(format_args!(
                "[TurnipMaterialDispatch] unsupported_layout sample={} id=0x{id:x} kind={kind} variation={variation}",
                self.observations)),
        }
    }
}
