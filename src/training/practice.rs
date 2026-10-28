//! Current native entry ownership for save/record/preview. No retained pointers
//! are dereferenced: each callback resolves the manager and current body again.
use std::sync::Mutex;
use smash::{app::{self, lua_bind::*, BattleObjectModuleAccessor}, lib::lua_const::*};
use crate::{common, native_fields};
use super::practice_policy::{self, Context, Entries, EntryLease};

static OWNED: Mutex<EntryLease> = Mutex::new(EntryLease::new());

pub fn context() -> Option<Context> {
    let mode = common::game_mode::current();
    practice_policy::context(unsafe { common::is_native_training_mode() },
        mode.is_some_and(|m| m.initialized), mode.map_or(0, |m| m.melee),
        mode.map_or(0, |m| m.mode), super::lobby::active())
}

pub fn scene_enabled() -> bool { context().is_some() }
pub fn waiting() -> bool { context() == Some(Context::Waiting) }
pub fn enabled() -> bool {
    scene_enabled() && !crate::character_roster::busy() && !super::lobby::native_ui_busy()
}

pub unsafe fn entry(role: usize) -> Option<usize> {
    if !enabled() { return None; }
    let context = context()?;
    let index = if role == 0 { 0 } else { context.cpu_entry() };
    native_entry(index)
}

/// Native Training panels also hold extra CPUs. They have entry receipts for
/// releasing that stop, but never acquire either save/record slot.
pub unsafe fn panel_entry(module: &BattleObjectModuleAccessor) -> Option<usize> {
    if !enabled() || !common::is_fighter(module) { return None; }
    let context = context()?;
    let index = WorkModule::get_int(module as *const _ as *mut BattleObjectModuleAccessor,
        *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID);
    if !(0..8).contains(&index)
        || (context == Context::Waiting && context.role(index).is_none()) { return None; }
    native_entry(index)
}

unsafe fn native_entry(index: i32) -> Option<usize> {
    let manager = native_fields::child(training_mod_sync::read(&common::FIGHTER_MANAGER_ADDR), 0)?;
    let entry = FighterManager::get_fighter_entry(manager as *mut app::FighterManager,
        app::FighterEntryID(index)) as usize;
    (native_fields::object(entry)
        && native_fields::read::<u32>(entry, 0xe0) == Some(index as u32)).then_some(entry)
}

unsafe fn entries() -> Option<Entries> {
    Some(Entries { context: context()?, player: entry(0)?, cpu: entry(1)? })
}

pub unsafe fn acquire() -> bool {
    let Some(current) = entries() else { return false; };
    if common::try_get_module_accessor(crate::consts::FighterId::Player).is_none()
        || common::try_get_module_accessor(crate::consts::FighterId::CPU).is_none() { return false; }
    OWNED.lock().unwrap().acquire(current)
}

pub fn forget() { OWNED.lock().unwrap().retire(); }

/// Invoked before consumer callbacks and before native entry destruction.
pub fn retire() {
    let owned = OWNED.lock().unwrap().retire();
    if owned || crate::replay_preview::occupied() || super::recording_or_playback() {
        super::clear_session_state();
    }
}

/// Once per game frame, from the P1 body's frame callback: retire a lease whose
/// entries were replaced and resume suspended waiting drafts. The two native entry
/// resolutions below are the only per-frame cost; immediate retirement on entry
/// destruction or scene change does not wait for this.
pub unsafe fn refresh() {
    if !enabled() { return; }
    if !OWNED.lock().unwrap().active() && !super::named_macros::suspended() { return; }
    let current = entries();
    let replaced = OWNED.lock().unwrap().replaced(current);
    if replaced { retire(); }
    if current.is_some() { super::named_macros::resume_waiting(); }
}

pub unsafe fn visualizes(module: *mut BattleObjectModuleAccessor) -> bool {
    if !enabled() { return false; }
    let Some(module) = module.as_ref() else { return false; };
    if context() == Some(Context::Training) || role(module).is_some() { return true; }
    if common::get_category(module) != *BATTLE_OBJECT_CATEGORY_WEAPON { return false; }
    let owner = WorkModule::get_int(module as *const _ as *mut BattleObjectModuleAccessor,
        *WEAPON_INSTANCE_WORK_ID_INT_LINK_OWNER) as u32;
    common::try_get_battle_object(owner).is_some_and(|body|
        body.module_accessor.as_ref().is_some_and(|module|
            module.battle_object_id == owner && role(module).is_some()))
}

pub fn entry_departed(entry: usize) {
    super::named_macros::forget_stopped_entry(entry);
    super::tech::forget_hidden_entry(entry);
    let owned = OWNED.lock().unwrap().owns(entry);
    if owned {
        if super::lobby::is_waiting_scene() { super::named_macros::suspend_waiting(); }
        retire();
    }
}

pub fn scene_departed() {
    super::named_macros::forget_stopped_scene();
    super::tech::forget_hidden_scene();
    retire();
}

pub unsafe fn role(module: &BattleObjectModuleAccessor) -> Option<usize> {
    if !enabled() || !common::is_fighter(module) { return None; }
    context()?.role(WorkModule::get_int(module as *const _ as *mut BattleObjectModuleAccessor,
        *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID))
}

pub unsafe fn primary_cpu(module: &mut BattleObjectModuleAccessor) -> bool {
    role(module) == Some(1)
        && app::utility::get_kind(module) != *FIGHTER_KIND_NANA
        && common::try_get_module_accessor(crate::consts::FighterId::CPU)
            .is_some_and(|current| std::ptr::eq(current, module))
}

/// The controller mapping that operates the Training CPU under native CPU
/// Behavior=Control: the CPU entry's own mapping index, only while a valid native
/// FighterInformation positively reports a controller operator. P1, waiting scenes,
/// an AI-owned CPU and every unqualified read refuse.
pub fn cpu_operator(player_idx: i32) -> bool {
    player_idx == crate::consts::FighterId::CPU as i32
        && context() == Some(Context::Training)
        && common::try_get_module_accessor(crate::consts::FighterId::CPU)
            .is_some_and(|cpu| common::operation_cpu(unsafe { &mut *cpu }) == Some(false))
}

/// FE+0x4178 is the native controller owner used by the existing CPU pilot's
/// verified 13.0.5 entry adapter. Do not confuse its controller slot with entry 4.
pub unsafe fn owns_cpu_controls(controls: usize) -> bool {
    entry(1).and_then(|entry| native_fields::child(entry, 0x4178)) == Some(controls)
}
