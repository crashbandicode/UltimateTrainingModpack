use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use log::{info, warn};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use smash::app::{self, lua_bind::*, ArticleOperationTarget, Item};
use smash::cpp::l2c_value::LuaConst;
use smash::hash40;
use smash::lib::lua_const::*;
use smash::phx::{Hash40, Vector3f};
use training_mod_consts::{CharacterItem, SaveDamage, SaveStateSlot};

use SaveState::*;

use crate::common::button_config;
use crate::common::consts::get_random_float;
use crate::common::consts::get_random_int;
use crate::common::consts::FighterId;
use crate::common::consts::OnOff;
use crate::common::consts::PlaybackSlot;
use crate::common::consts::RecordTrigger;
use crate::common::consts::SaveStateMirroring;
//TODO: Cleanup above
use crate::common::consts::SAVE_STATES_TOML_PATH;
use crate::common::filesystem;
use crate::common::is_dead;
use crate::common::try_get_module_accessor;
use crate::common::MENU;
use crate::is_operation_cpu;
use crate::training::buff;
use crate::training::character_specific::{ptrainer, steve};
use crate::training::charge::{self, ChargeState};
use crate::training::input_record;
use crate::training::items::apply_item;
use crate::training::reset;
use crate::training::ui::notifications;
use crate::{is_ptrainer, ITEM_MANAGER_ADDR};
use training_mod_sync::*;

// Don't remove Mii hats, Pikmin, Luma, or crafting table
const ARTICLE_ALLOWLIST: [(LuaConst, LuaConst); 9] = [
    (
        FIGHTER_KIND_MIIFIGHTER,
        FIGHTER_MIIFIGHTER_GENERATE_ARTICLE_HAT,
    ),
    (
        FIGHTER_KIND_MIISWORDSMAN,
        FIGHTER_MIISWORDSMAN_GENERATE_ARTICLE_HAT,
    ),
    (
        FIGHTER_KIND_MIIGUNNER,
        FIGHTER_MIIGUNNER_GENERATE_ARTICLE_HAT,
    ),
    (FIGHTER_KIND_ROSETTA, FIGHTER_ROSETTA_GENERATE_ARTICLE_TICO),
    (FIGHTER_KIND_PICKEL, FIGHTER_PICKEL_GENERATE_ARTICLE_TABLE),
    (FIGHTER_KIND_ELIGHT, FIGHTER_ELIGHT_GENERATE_ARTICLE_ESWORD),
    (FIGHTER_KIND_EFLAME, FIGHTER_EFLAME_GENERATE_ARTICLE_ESWORD),
    (FIGHTER_KIND_PIKMIN, FIGHTER_PIKMIN_GENERATE_ARTICLE_PIKMIN),
    (
        FIGHTER_KIND_DUCKHUNT,
        FIGHTER_DUCKHUNT_GENERATE_ARTICLE_RETICLE,
    ),
];

extern "C" {
    #[link_name = "\u{1}_ZN3app14sv_information8stage_idEv"]
    pub fn stage_id() -> i32;
}

#[derive(Serialize, Deserialize, PartialEq, Copy, Clone, Debug)]
pub enum SaveState {
    Save,
    NoAction,
    KillPlayer,
    WaitForAlive,
    PosMove,
    ApplyBuff,
    NanaPosMove,
    WaitForPokemonSwitch,
    WaitForCopyAbility,
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug)]
pub struct SavedState {
    pub x: f32,
    pub y: f32,
    pub percent: f32,
    pub lr: f32,
    pub situation_kind: i32,
    pub state: SaveState,
    pub fighter_kind: i32,
    pub charge: ChargeState,
    pub steve_state: Option<steve::SteveState>,
    #[serde(default, deserialize_with = "training_mod_consts::character_items::deserialize_saved_item")]
    pub held_item: Option<training_mod_consts::character_items::HeldItemSnapshot>,
    /// Capture provenance belongs to the saved slot, never to a later live fighter.
    /// Old numbered-slot files remain readable; resave to attach this metadata.
    #[serde(default)]
    pub saved_stage: Option<i32>,
    #[serde(default)]
    pub mii: Option<training_mod_consts::mii::lifetime::Configuration>,
}

#[macro_export]
macro_rules! default_save_state {
    () => {
        SavedState {
            x: 0.0,
            y: 0.0,
            percent: 0.0,
            lr: 1.0,
            situation_kind: 0,
            state: NoAction,
            fighter_kind: -1,
            held_item: None,
            saved_stage: None,
            mii: None,
            charge: ChargeState {
                int_x: None,
                int_y: None,
                int_z: None,
                float_x: None,
                float_y: None,
                float_z: None,
                has_charge: None,
                robin_charges: None,
            },
            steve_state: Some(steve::SteveState {
                mat_g1: 36,
                mat_wood: 18,
                mat_stone: 0,
                mat_iron: 3,
                mat_gold: 0,
                mat_redstone: 2,
                mat_diamond: 0,
                sword_mat: 1 as char,
                sword_durability: 25.0,
                axe_mat: 1 as char,
                axe_durability: 70.0,
                pick_mat: 1 as char,
                pick_durability: 70.0,
                shovel_mat: 1 as char,
                shovel_durability: 70.0,
            }),
        }
    };
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug)]
pub struct SaveStateSlots {
    player: [SavedState; NUM_SAVE_STATE_SLOTS],
    cpu: [SavedState; NUM_SAVE_STATE_SLOTS],
}

const NUM_SAVE_STATE_SLOTS: usize = 5;
// I actually had to do it this way, a simple load-from-file in main() caused crashes.
lazy_static::lazy_static! {
    static ref SAVE_STATE_SLOTS : Mutex<SaveStateSlots> = Mutex::new({
        info!("Initialized lazy_static: SAVE_STATE_SLOTS");
        load_from_file()
    });
}

pub fn load_from_file() -> SaveStateSlots {
    let defaults = SaveStateSlots {
        player: [default_save_state!(); NUM_SAVE_STATE_SLOTS],
        cpu: [default_save_state!(); NUM_SAVE_STATE_SLOTS],
    };

    info!("Checking for previous save state settings in {SAVE_STATES_TOML_PATH}...");
    let mut slots: SaveStateSlots = filesystem::read_toml(SAVE_STATES_TOML_PATH).unwrap_or_else(|e| {
        warn!("Could not load previous save state settings! {e}");
        defaults
    });
    // Persisted positions are data, never permission to resume a native operation
    // from a prior process/entry lifetime.
    for state in slots.player.iter_mut().chain(slots.cpu.iter_mut()) { state.state = NoAction; }
    slots
}

fn save_to_file(save_state_slots: SaveStateSlots) {
    filesystem::write_toml(SAVE_STATES_TOML_PATH, &save_state_slots);
}

pub fn cancel_pending() {
    let mut slots = SAVE_STATE_SLOTS.lock();
    for state in &mut slots.player {
        state.state = NoAction;
    }
    for state in &mut slots.cpu {
        state.state = NoAction;
    }
    QUICK_RESET_ACTIVE.store(false, Ordering::Release);
    end_preview_restore();
    // A queued library bind never survives practice retirement; nor does its pin.
    PENDING_BIND.lock().take();
    end_library_restore();
}

static QUICK_RESET_ACTIVE: AtomicBool = AtomicBool::new(false);
static QUICK_RESET_STATES: Mutex<[SavedState; 2]> = Mutex::new([default_save_state!(); 2]);

pub fn clear_quick_reset() {
    QUICK_RESET_ACTIVE.store(false, Ordering::Release);
    *QUICK_RESET_STATES.lock() = [default_save_state!(); 2];
}

/// Waiting has no Training menu/reset owner. Remember its fresh native idle
/// spawn values per entry lifetime and reuse the death/rebirth restore protocol.
/// These temporary anchors never overwrite a user's numbered saved slots.
unsafe fn observe_quick_reset(module: &mut app::BattleObjectModuleAccessor, role: usize) {
    if QUICK_RESET_STATES.lock()[role].fighter_kind >= 0 { return; }
    if !super::lobby::active() || !crate::common::is_ready_go()
        || app::utility::get_kind(module) == *FIGHTER_KIND_NANA
        || StatusModule::status_kind(module) != *FIGHTER_STATUS_KIND_WAIT
        || !super::practice::acquire() { return; }
    let mut states = QUICK_RESET_STATES.lock();
    if states[role].fighter_kind >= 0 { return; }
    let kind = app::utility::get_kind(module);
    states[role] = SavedState {
        x: PostureModule::pos_x(module), y: PostureModule::pos_y(module),
        lr: PostureModule::lr(module), fighter_kind: kind,
        charge: charge::get_charge(module, kind),
        steve_state: steve::save_steve_state(module),
        ..default_save_state!()
    };
}

pub unsafe fn request_quick_reset() -> bool {
    if !super::lobby::active() || !super::practice::enabled()
        || !super::practice::acquire() || is_loading() { return false; }
    let mut states = QUICK_RESET_STATES.lock();
    if states.iter().any(|s| s.fighter_kind < 0) { return false; }
    input_record::stop_playback();
    for state in states.iter_mut() { state.state = KillPlayer; }
    MIRROR_STATE = 1.0;
    QUICK_RESET_ACTIVE.store(true, Ordering::Release);
    true
}

unsafe fn waiting_saved_pair_valid(slot: usize) -> bool {
    if !super::lobby::active() { return true; }
    [*save_state_player(slot), *save_state_cpu(slot)].iter().all(|s|
        s.fighter_kind >= 0 && s.x.is_finite() && s.y.is_finite()
            && s.percent.is_finite() && s.percent >= 0.0 && matches!(s.lr, -1.0 | 1.0)
            && [*SITUATION_KIND_GROUND, *SITUATION_KIND_AIR, *SITUATION_KIND_CLIFF].contains(&s.situation_kind))
}

// A preview owns a temporary pair, separate from all five numbered slots. The
// first X captures it on the game thread; restarts and Stop reuse it. Only value
// data survives native death/rebirth. Retirement never writes a setup file.
static PREVIEW_ACTIVE: AtomicBool = AtomicBool::new(false);
static PREVIEW_SELECTION: AtomicU64 = AtomicU64::new(0);
static PREVIEW_STATES: Mutex<[SavedState; 2]> = Mutex::new([default_save_state!(); 2]);

pub fn end_preview_restore() {
    PREVIEW_ACTIVE.store(false, Ordering::Release);
    PREVIEW_SELECTION.store(0, Ordering::Release);
    *PREVIEW_STATES.lock() = [default_save_state!(); 2];
}

/// Stop retires the native redirect but keeps the original coordinates while
/// the same captured fighter remains open. Editing In/Out must not accumulate
/// airborne drift by taking a new snapshot after each restore.
pub fn finish_preview_restore() {
    PREVIEW_ACTIVE.store(false, Ordering::Release);
    if !crate::replay_capture::panel_visible()
        || PREVIEW_SELECTION.load(Ordering::Acquire) != crate::replay_capture::selection_revision()
    { end_preview_restore(); }
}

// A setup-library load pins the numbered slot it was bound to and keeps
// the user's ordinary mirroring, damage, item, charge and partner handling.
static LIBRARY_SLOT: AtomicUsize = AtomicUsize::new(usize::MAX);
static PENDING_BIND: Mutex<Option<SlotBind>> = Mutex::new(None);

/// Value-only slot write queued by the setup library. It is consumed on the
/// game thread inside [`save_states`] before the selected slot is borrowed and
/// never carries a native pointer or object id.
#[derive(Clone)]
pub struct SlotBind {
    pub slot: usize,
    pub player: SavedState,
    pub cpu: SavedState,
    /// Restore through the ordinary death/rebirth machinery after binding.
    pub restore: bool,
    pub name: String,
}

pub fn end_library_restore() {
    LIBRARY_SLOT.store(usize::MAX, Ordering::Relaxed);
}

pub fn quick_reset_active() -> bool {
    QUICK_RESET_ACTIVE.load(Ordering::Acquire)
}

pub fn bind_pending() -> bool {
    PENDING_BIND.lock().is_some()
}

/// Copies of both halves of a numbered slot, when both have been saved.
pub fn slot_pair(slot: usize) -> Option<(SavedState, SavedState)> {
    if slot >= NUM_SAVE_STATE_SLOTS {
        return None;
    }
    let slots = SAVE_STATE_SLOTS.lock();
    let (player, cpu) = (slots.player[slot], slots.cpu[slot]);
    (player.fighter_kind >= 0 && cpu.fighter_kind >= 0).then_some((player, cpu))
}

/// Queue one bind. The caller has already admitted the values; the slot is
/// written only by the game thread, once practice is idle.
pub fn queue_bind(bind: SlotBind) -> Result<(), &'static str> {
    if bind.slot >= NUM_SAVE_STATE_SLOTS {
        return Err("Choose a Save State Slot first");
    }
    let mut pending = PENDING_BIND.lock();
    if pending.is_some() {
        return Err("A setup is already being applied");
    }
    *pending = Some(bind);
    Ok(())
}

/// Game thread only, from `save_states()` for P1 before any slot borrow. Waits
/// for the menu and panels to close, then either writes the slot or drops the
/// bind with a notice when practice became busy meanwhile.
unsafe fn consume_pending_bind() {
    if PENDING_BIND.lock().is_none() {
        return;
    }
    if read(&crate::common::menu::QUICK_MENU_ACTIVE) || crate::training::named_macros::panel_visible() {
        return;
    }
    let Some(bind) = PENDING_BIND.lock().take() else { return; };
    if QUICK_RESET_ACTIVE.load(Ordering::Acquire)
        || is_loading()
        || crate::training::recording_or_playback()
        || crate::replay_preview::occupied()
    {
        notifications::notification(
            "Setup Library".to_owned(),
            "Practice is busy; setup not applied".to_owned(),
            180,
        );
        return;
    }
    if bind.restore {
        if let Err(message) = super::setup_library::restore_bind_matches(&bind) {
            notifications::notification("Setup Library".to_owned(), message, 180);
            return;
        }
    }
    let slots_copy = {
        let mut slots = SAVE_STATE_SLOTS.lock();
        slots.player[bind.slot] = SavedState { state: NoAction, ..bind.player };
        slots.cpu[bind.slot] = SavedState { state: NoAction, ..bind.cpu };
        *slots
    };
    save_to_file(slots_copy);
    crate::logging::diagnostic(format_args!(
        "[PracticeSave] library_bound slot={} restore={} player_kind=0x{:x} cpu_kind=0x{:x}",
        bind.slot, bind.restore, bind.player.fighter_kind, bind.cpu.fighter_kind
    ));
    let slot_label = format!("Slot {}", bind.slot + 1);
    if !bind.restore {
        notifications::notification(
            "Setup Library".to_owned(),
            format!("Bound {} to {slot_label}", bind.name),
            180,
        );
        return;
    }
    if read(&MENU).save_state_enable == OnOff::OFF {
        notifications::notification(
            "Setup Library".to_owned(),
            format!("Bound {} to {slot_label}; enable save states to load", bind.name),
            180,
        );
        return;
    }
    input_record::stop_playback();
    LIBRARY_SLOT.store(bind.slot, Ordering::Relaxed);
    MIRROR_STATE = should_mirror();
    save_state_player(bind.slot).state = KillPlayer;
    save_state_cpu(bind.slot).state = KillPlayer;
    notifications::notification(
        "Setup Library".to_owned(),
        format!("Loading {} into {slot_label}", bind.name),
        180,
    );
}

/// Capture the same value data as an explicit save, without an effect, slot
/// mutation or disk write. Call only on the game thread with a current body.
unsafe fn snapshot_live_state(module: &mut app::BattleObjectModuleAccessor, role: usize) -> SavedState {
    let kind = app::utility::get_kind(module);
    SavedState {
        x: PostureModule::pos_x(module),
        y: PostureModule::pos_y(module),
        lr: PostureModule::lr(module),
        percent: DamageModule::damage(module, 0),
        situation_kind: StatusModule::situation_kind(module),
        fighter_kind: kind,
        charge: charge::get_charge(module, kind),
        held_item: if role == 1 { None } else { Some(crate::character_items::snapshot(module)) },
        steve_state: steve::save_steve_state(module),
        saved_stage: Some(stage_id()),
        mii: crate::character_roster::current_mii_configuration(role),
        state: NoAction,
    }
}

pub unsafe fn request_preview_restore(capture_allowed: bool) -> Result<(), &'static str> {
    if !super::practice::enabled() || !super::practice::acquire() {
        return Err("Wait for both practice fighters before previewing");
    }
    if QUICK_RESET_ACTIVE.load(Ordering::Acquire)
        || LIBRARY_SLOT.load(Ordering::Relaxed) != usize::MAX
    {
        return Err("Wait for the current practice restore before previewing");
    }
    // A queued library bind waits for the editor/preview to close. It may remain
    // queued: this temporary pair never reads or writes its destination slot.
    let selection = crate::replay_capture::selection_revision();
    let previous = PREVIEW_SELECTION.load(Ordering::Acquire);
    // A selection change must still restore the old active pair before reload.
    let captured = previous != 0 && (PREVIEW_ACTIVE.load(Ordering::Acquire) || previous == selection);
    if !captured && !capture_allowed {
        return Err("Preview setup was lost; press X to start again");
    }
    let mut pair = if captured { *PREVIEW_STATES.lock() } else { [default_save_state!(); 2] };
    for (role, id) in [FighterId::Player, FighterId::CPU].iter().enumerate() {
        let Some(live) = try_get_module_accessor(*id) else {
            return Err("Wait for both practice fighters before previewing");
        };
        let live = &mut *live;
        let kind = app::utility::get_kind(live);
        if !captured {
            let status = StatusModule::status_kind(live);
            if is_dead(live) || [*FIGHTER_STATUS_KIND_REBIRTH, *FIGHTER_STATUS_KIND_ENTRY].contains(&status)
                || (role == 1 && !crate::replay_capture::clean_statuses().contains(&status))
            {
                return Err("Close the editor, let both fighters settle, then reopen");
            }
            pair[role] = snapshot_live_state(live, role);
        }
        let saved = &pair[role];
        // Exact replay-Mii admission belongs to replay_capture's target-ready
        // check. Catalog capture can temporarily hide current_mii_configuration;
        // treating that as a body change would reject a valid preview. The
        // practice entry lease retires this checkpoint on actual replacement.
        if saved.fighter_kind != kind || saved.saved_stage != Some(stage_id())
        {
            return Err("Preview setup changed; close and reopen the editor");
        }
        if !saved.x.is_finite() || !saved.y.is_finite() || !saved.percent.is_finite()
            || saved.percent < 0.0 || !matches!(saved.lr, -1.0 | 1.0)
        {
            return Err("Close the editor, let both fighters settle, then reopen");
        }
        if saved.situation_kind != SITUATION_KIND_GROUND
            && saved.situation_kind != SITUATION_KIND_AIR
        {
            return Err("Close the editor, leave the ledge, then reopen");
        }
    }
    // Publish only after both snapshots validate; a refused preview changes no
    // slot, active restore, or checkpoint. Reuse the existing native cleanup.
    for state in &mut pair { state.state = KillPlayer; }
    *PREVIEW_STATES.lock() = pair;
    if !captured { PREVIEW_SELECTION.store(selection, Ordering::Release); }
    PREVIEW_ACTIVE.store(true, Ordering::Release);
    if !captured { println!("[ClipPreview] temporary_setup player={} cpu={}", pair[0].fighter_kind, pair[1].fighter_kind); }
    Ok(())
}

pub unsafe fn request_macro_restore() -> Result<(), &'static str> {
    if !super::practice::enabled() || !super::practice::acquire() || read(&MENU).save_state_enable == OnOff::OFF {
        return Err("Enable save states and save a practice setup first");
    }
    let slot = get_slot();
    if !waiting_saved_pair_valid(slot) { return Err("Save a valid practice setup first"); }
    if save_state_player(slot).fighter_kind < 0 || save_state_cpu(slot).fighter_kind < 0 {
        return Err("Save a Training setup first");
    }
    let situation = save_state_cpu(slot).situation_kind;
    if situation != SITUATION_KIND_GROUND
        && situation != SITUATION_KIND_AIR
        && !crate::training::named_macros::restore_in_air()
    {
        return Err("Use an idle/fall saved setup for named playback");
    }
    save_state_player(slot).state = KillPlayer;
    save_state_cpu(slot).state = KillPlayer;
    MIRROR_STATE = should_mirror();
    Ok(())
}

unsafe fn save_state_player(slot: usize) -> &'static mut SavedState {
    // Game-thread-only borrows, matching numbered-slot and quick-reset ownership.
    if PREVIEW_ACTIVE.load(Ordering::Acquire) { return &mut (*PREVIEW_STATES.data_ptr())[0]; }
    if QUICK_RESET_ACTIVE.load(Ordering::Acquire) { return &mut (*QUICK_RESET_STATES.data_ptr())[0]; }
    &mut (*SAVE_STATE_SLOTS.data_ptr()).player[slot]
}

unsafe fn save_state_cpu(slot: usize) -> &'static mut SavedState {
    if PREVIEW_ACTIVE.load(Ordering::Acquire) { return &mut (*PREVIEW_STATES.data_ptr())[1]; }
    if QUICK_RESET_ACTIVE.load(Ordering::Acquire) { return &mut (*QUICK_RESET_STATES.data_ptr())[1]; }
    &mut (*SAVE_STATE_SLOTS.data_ptr()).cpu[slot]
}

pub unsafe fn get_state_pokemon(
    ptrainer_module_accessor: *mut app::BattleObjectModuleAccessor,
) -> u32 {
    let selected_slot = get_slot();
    let pokemon_module_accessor = ptrainer::get_pokemon_module_accessor(ptrainer_module_accessor);
    let Some(pokemon) = pokemon_module_accessor.as_ref() else { return u32::MAX; };
    let fighter_kind = match super::practice::role(pokemon) {
        Some(0) => save_state_player(selected_slot).fighter_kind,
        Some(1) => save_state_cpu(selected_slot).fighter_kind,
        _ => return u32::MAX,
    };
    (fighter_kind - *FIGHTER_KIND_PZENIGAME) as u32
}

pub unsafe fn get_charge_state(
    module_accessor: *mut app::BattleObjectModuleAccessor,
) -> ChargeState {
    let selected_slot = get_slot();
    match module_accessor.as_ref().and_then(|module| super::practice::role(module)) {
        Some(0) => save_state_player(selected_slot).charge,
        Some(1) => save_state_cpu(selected_slot).charge,
        _ => default_save_state!().charge,
    }
}

pub unsafe fn end_copy_ability(module_accessor: *mut app::BattleObjectModuleAccessor) {
    let selected_slot = get_slot();
    let save_state = match module_accessor.as_ref().and_then(|module| super::practice::role(module)) {
        Some(0) => save_state_player(selected_slot),
        Some(1) => save_state_cpu(selected_slot),
        _ => return,
    };
    if save_state.state == WaitForCopyAbility {
        save_state.state = NoAction;
    }
}

// MIRROR_STATE == 1 -> Do not mirror
// MIRROR_STATE == -1 -> Do Mirror
static mut MIRROR_STATE: f32 = 1.0;

static mut RANDOM_SLOT: usize = 0;

unsafe fn get_slot() -> usize {
    // The accessors route this token to the temporary pair, never numbered Slot 1.
    if PREVIEW_ACTIVE.load(Ordering::Acquire) { return 0; }
    let library = LIBRARY_SLOT.load(Ordering::Relaxed);
    if library != usize::MAX {
        return library;
    }
    let random_slot = read(&MENU).randomize_slots.get_random();
    if random_slot != SaveStateSlot::empty() {
        RANDOM_SLOT
    } else {
        read(&MENU).save_state_slot.into_idx().unwrap_or(0)
    }
}

pub unsafe fn is_killing() -> bool {
    if !super::practice::scene_enabled() {
        return false;
    }
    let selected_slot = get_slot();
    (save_state_player(selected_slot).state == KillPlayer
        || save_state_player(selected_slot).state == WaitForAlive)
        || (save_state_cpu(selected_slot).state == KillPlayer
            || save_state_cpu(selected_slot).state == WaitForAlive)
}

pub unsafe fn is_loading() -> bool {
    if !super::practice::scene_enabled() {
        return false;
    }
    let selected_slot = get_slot();
    save_state_player(selected_slot).state != NoAction
        || save_state_cpu(selected_slot).state != NoAction
}

/// Keep native Training's historical suppressions; waiting borrows them only
/// during its owned restore, never during ordinary waiting combat or a match.
pub unsafe fn effects_enabled() -> bool {
    crate::common::is_native_training_mode() || (super::practice::enabled() && is_loading())
}

pub unsafe fn should_mirror() -> f32 {
    match read(&MENU).save_state_mirroring {
        SaveStateMirroring::NONE => 1.0,
        SaveStateMirroring::ALTERNATE => -MIRROR_STATE,
        SaveStateMirroring::RANDOM => ([-1.0, 1.0])[get_random_int(2) as usize],
        _ => panic!(
            "Invalid value in should_mirror: {}",
            read(&MENU).save_state_mirroring
        ),
    }
}

pub unsafe fn get_param_int(
    module_accessor: &mut app::BattleObjectModuleAccessor,
    param_type: u64,
    param_hash: u64,
) -> Option<i32> {
    if !super::practice::enabled()
        || (super::lobby::active() && (!is_loading() || super::practice::role(module_accessor).is_none())) {
        return None;
    }
    if param_type == hash40("common") {
        if param_hash == hash40("dead_rebirth_wait_frame") {
            let jostle_frame = WorkModule::get_int(
                module_accessor,
                *FIGHTER_INSTANCE_WORK_ID_INT_HIT_STOP_IGNORE_JOSTLE_FRAME,
            );
            if jostle_frame > 1 {
                // Allow jostle to stop being ignored before we respawn
                WorkModule::set_int(
                    module_accessor,
                    1,
                    *FIGHTER_INSTANCE_WORK_ID_INT_HIT_STOP_IGNORE_JOSTLE_FRAME,
                );
            }
            // Remove Shulk Monado Art Damage Effects
            WorkModule::set_int(
                module_accessor,
                0,
                *FIGHTER_INSTANCE_WORK_ID_INT_SHULK_MONAD_ARTS_DAMAGE_FLASH_FRAME,
            );
            EffectModule::remove_common(module_accessor, Hash40::new("monad_arts_damage_buster"));
            EffectModule::remove_common(module_accessor, Hash40::new("monad_arts_damage_smash"));
            // TODO: We should try to remove all effects here
            return Some(1);
        }
        if param_hash == hash40("rebirth_move_frame") {
            return Some(0);
        }
        if param_hash == hash40("rebirth_move_frame_trainer") && is_loading() {
            return Some(0);
        }
        if param_hash == hash40("rebirth_wait_frame") {
            return Some(0);
        }
        if param_hash == hash40("rebirth_invincible_frame") {
            return Some(0);
        }
        if param_hash == hash40("rebirth_invincible_add_frame") {
            return Some(0);
        }
    }
    if param_type == hash40("param_mball")
        && param_hash == hash40("change_fly_frame")
        && is_loading()
    {
        return Some(0);
    }

    None
}

fn get_stage_offset(stage_id: i32) -> f32 {
    let offsets: HashMap<i32, f32> = HashMap::from([
        (*StageID::Animal_Village, 1.195),
        (*StageID::Animal_City, 1.448),
        (*StageID::Yoshi_Island, -1.053),
    ]);

    *offsets.get(&stage_id).unwrap_or(&0.0)
}

fn set_damage(module_accessor: &mut app::BattleObjectModuleAccessor, damage: f32) {
    unsafe {
        DamageModule::heal(
            module_accessor,
            -DamageModule::damage(module_accessor, 0),
            0,
        );
        DamageModule::add_damage(module_accessor, damage, 0);
    }
}

unsafe fn on_ptrainer_death(module_accessor: &mut app::BattleObjectModuleAccessor) {
    if !is_ptrainer(module_accessor) {
        return;
    }
    let ptrainer_module_accessor = ptrainer::get_ptrainer_module_accessor(module_accessor);
    WorkModule::off_flag(
        ptrainer_module_accessor,
        *WEAPON_PTRAINER_PTRAINER_INSTANCE_WORK_ID_FLAG_ENABLE_CHANGE_POKEMON,
    );
    MotionModule::set_rate(ptrainer_module_accessor, 1000.0);
    if let Some(ptrainer_masterball_module_accessor) =
        ptrainer::get_ptrainer_mball_module_accessor(ptrainer_module_accessor)
    {
        MotionModule::set_rate(ptrainer_masterball_module_accessor, 1000.0);
        ArticleModule::set_visibility_whole(
            ptrainer::get_ptrainer_module_accessor(module_accessor),
            *WEAPON_PTRAINER_PTRAINER_GENERATE_ARTICLE_MBALL,
            false,
            ArticleOperationTarget(*ARTICLE_OPE_TARGET_ALL),
        );
    }
}

pub unsafe fn on_death(fighter_kind: i32, module_accessor: &mut app::BattleObjectModuleAccessor) {
    SoundModule::stop_all_sound(module_accessor);
    // All articles have ID <= 0x25
    (0..=0x25)
        .filter(|article_idx| {
            !ARTICLE_ALLOWLIST.iter().any(|article_allowed| {
                article_allowed.0 == fighter_kind && article_allowed.1 == *article_idx
            })
        })
        .for_each(|article_idx| {
            if ArticleModule::is_exist(module_accessor, article_idx) {
                let article: *mut app::Article =
                    ArticleModule::get_article(module_accessor, article_idx);
                let article_object_id = Article::get_battle_object_id(article);
                ArticleModule::remove_exist_object_id(module_accessor, article_object_id as u32);
            }
        });
    let item_mgr = *(read(&ITEM_MANAGER_ADDR) as *mut *mut app::ItemManager);
    (0..ItemManager::get_num_of_active_item_all(item_mgr)).for_each(|item_idx| {
        let item = ItemManager::get_active_item(item_mgr, item_idx);
        if item != 0 {
            let item = item as *mut Item;
            let item_battle_object_id = app::lua_bind::Item::get_battle_object_id(item) as u32;
            ItemManager::remove_item_from_id(item_mgr, item_battle_object_id);
        }
    });
    MotionAnimcmdModule::set_sleep(module_accessor, true);
    SoundModule::pause_se_all(module_accessor, true);
    ControlModule::stop_rumble(module_accessor, true);
    SoundModule::stop_all_sound(module_accessor);
    // Return camera to normal when loading save state
    SlowModule::clear_whole(module_accessor);
    CameraModule::zoom_out(module_accessor, 0);
    // Remove blue effect (but does not remove darkened screen)
    EffectModule::kill_kind(
        module_accessor,
        Hash40::new("sys_bg_criticalhit"),
        false,
        false,
    );
    // Removes the darkened screen from special zooms
    // If there's a crit that doesn't get removed, it's likely bg_criticalhit2.
    EffectModule::remove_screen(module_accessor, Hash40::new("bg_criticalhit"), 0);
    // Remove all quakes to prevent screen shake lingering through load.
    for quake_kind in *CAMERA_QUAKE_KIND_NONE..=*CAMERA_QUAKE_KIND_MAX {
        CameraModule::stop_quake(module_accessor, quake_kind);
    }
}

pub unsafe fn save_states(module_accessor: &mut app::BattleObjectModuleAccessor) {
    let Some(role) = super::practice::role(module_accessor) else { return; };
    observe_quick_reset(module_accessor, role);
    // A queued setup-library bind is a slot write, not a load: consume it once
    // per frame, before any slot borrow, even while save states are disabled.
    if role == 0 && super::practice::acquire() { consume_pending_bind(); }
    let quick_reset = QUICK_RESET_ACTIVE.load(Ordering::Acquire);
    // Internal preview cleanup remains live even if the user's save-state tool
    // is disabled. Its setting and numbered slots remain untouched.
    if !quick_reset && !PREVIEW_ACTIVE.load(Ordering::Acquire)
        && read(&MENU).save_state_enable == OnOff::OFF { return; }
    // Ordinary Training may contain extra CPUs. They are neither P1 nor the
    // selected practice CPU and must not overwrite P1's saved state.
    if !super::practice::acquire() { return; }
    let selected_slot = get_slot();
    if LIBRARY_SLOT.load(Ordering::Relaxed) == selected_slot
        && save_state_player(selected_slot).state == NoAction
        && save_state_cpu(selected_slot).state == NoAction
    {
        // The library-triggered restore finished; ordinary slot selection resumes.
        end_library_restore();
    }
    let status = StatusModule::status_kind(module_accessor);
    let is_cpu = role == 1;
    let save_state = if is_cpu {
        save_state_cpu(selected_slot)
    } else {
        save_state_player(selected_slot)
    };

    let fighter_kind = app::utility::get_kind(module_accessor);
    let fighter_is_ptrainer = is_ptrainer(module_accessor);
    let fighter_is_popo = fighter_kind == *FIGHTER_KIND_POPO; // For making sure Popo doesn't steal Nana's PosMove
    let fighter_is_nana = fighter_kind == *FIGHTER_KIND_NANA; // Don't want Nana to reopen save states etc.
    let fighter_is_buffable = [
        *FIGHTER_KIND_BRAVE,
        *FIGHTER_KIND_CLOUD,
        *FIGHTER_KIND_JACK,
        *FIGHTER_KIND_LITTLEMAC,
        *FIGHTER_KIND_EDGE,
        *FIGHTER_KIND_WIIFIT,
        *FIGHTER_KIND_SHULK,
        *FIGHTER_KIND_TANTAN,
        *FIGHTER_KIND_WARIO,
    ]
    .contains(&fighter_kind);

    // Reset state
    let autoload_reset = !quick_reset && read(&MENU).save_state_autoload == OnOff::ON
        && save_state.state == NoAction
        && is_dead(module_accessor);
    let mut triggered_reset: bool = false;
    if !quick_reset && !is_operation_cpu(module_accessor) && !fighter_is_nana {
        triggered_reset = button_config::combo_passes(button_config::ButtonCombo::LoadState);
    }
    if (autoload_reset || triggered_reset)
        && !fighter_is_nana
        && !crate::training::named_macros::panel_visible()
        && !crate::replay_preview::occupied()
    {
        if save_state.state == NoAction {
            let random_slot = read(&MENU).randomize_slots.get_random();
            let slot = if random_slot != SaveStateSlot::empty() {
                RANDOM_SLOT = random_slot.into_idx().unwrap_or(0);
                RANDOM_SLOT
            } else {
                selected_slot
            };

            if !waiting_saved_pair_valid(slot) { return; }
            save_state_player(slot).state = KillPlayer;
            save_state_cpu(slot).state = KillPlayer;
            crate::logging::diagnostic(format_args!(
                "[PracticeSave] load_requested slot={} caller_kind=0x{:x} saved_player_kind=0x{:x}",
                slot,
                fighter_kind,
                save_state_player(slot).fighter_kind
            ));
        }
        MIRROR_STATE = should_mirror();
        // end input recording playback
        input_record::stop_playback();
        return;
    }

    // Kill the fighter and move them to camera bounds
    // Note: Nana shouldn't control her state here. Popo will give a signal to have
    // Nana move into NanaPosMove once he moves.
    if save_state.state == KillPlayer && !fighter_is_nana {
        on_ptrainer_death(module_accessor);
        if !is_dead(module_accessor) {
            StatusModule::change_status_force(module_accessor, *FIGHTER_STATUS_KIND_DEAD, true);
        }

        save_state.state = WaitForAlive;
        if is_cpu && !quick_reset {
            crate::training::named_macros::begin_restore();
        }

        return;
    }

    if save_state.state == WaitForAlive {
        on_ptrainer_death(module_accessor);
        if !is_dead(module_accessor) && !fighter_is_nana {
            on_death(fighter_kind, module_accessor);
            save_state.state = PosMove;
        }
    }

    // move to correct pos
    if save_state.state == PosMove || save_state.state == NanaPosMove {
        let status_kind = StatusModule::status_kind(module_accessor);
        if save_state.state == NanaPosMove
            && (!fighter_is_nana || (status_kind == FIGHTER_STATUS_KIND_STANDBY))
        {
            return;
        }
        SoundModule::stop_all_sound(module_accessor);
        MotionAnimcmdModule::set_sleep(module_accessor, false);
        SoundModule::pause_se_all(module_accessor, false);
        ControlModule::stop_rumble(module_accessor, false);
        KineticModule::clear_speed_all(module_accessor);

        let mirror = if crate::replay_preview::occupied() {
            1.0
        } else {
            MIRROR_STATE
        };
        let mut pos = if mirror == -1.0 {
            Vector3f {
                x: mirror * (save_state.x - get_stage_offset(stage_id())),
                y: save_state.y,
                z: 0.0,
            }
        } else {
            Vector3f {
                x: save_state.x,
                y: save_state.y,
                z: 0.0,
            }
        };

        // Adjust fighter y position if they won't grab ledge when they should
        adjust_ledge_pos(&mut pos, save_state.fighter_kind, save_state.situation_kind);
        if is_cpu && !quick_reset {
            if let Some((x, y)) = crate::training::named_macros::restored_target(pos.x, pos.y) {
                pos.x = x;
                pos.y = y;
            }
        }
        let lr = mirror * save_state.lr;
        if save_state.situation_kind == SITUATION_KIND_GROUND {
            // Teleport from respawn with native collision synchronization. Resetting
            // posture history alone still lets the ground module catch an intervening
            // soft platform. The first flag also resets native ground contacts and
            // collision-position history at the destination; the second stays false
            // to avoid translating linked objects. Keep the WAIT handshake below.
            PostureModule::init_pos(module_accessor, &pos, true, false);
        } else {
            PostureModule::set_pos(module_accessor, &pos);
        }
        PostureModule::set_lr(module_accessor, lr);
        reset::on_reset();

        let placed_in_air = !quick_reset && is_cpu && crate::training::named_macros::restore_in_air();
        if !quick_reset && is_cpu && !fighter_is_nana {
            crate::training::named_macros::remember_restore_pose(
                module_accessor,
                pos.x,
                pos.y,
                lr,
                if placed_in_air || save_state.situation_kind == SITUATION_KIND_AIR {
                    Some(true)
                } else if save_state.situation_kind == SITUATION_KIND_GROUND {
                    Some(false)
                } else {
                    None
                },
            );
        }
        if save_state.situation_kind == SITUATION_KIND_GROUND && !placed_in_air {
            if status != FIGHTER_STATUS_KIND_WAIT {
                StatusModule::change_status_request(
                    module_accessor,
                    *FIGHTER_STATUS_KIND_WAIT,
                    false,
                );
            } else {
                save_state.state = NoAction;
            }
        } else if save_state.situation_kind == SITUATION_KIND_AIR || placed_in_air {
            if status != FIGHTER_STATUS_KIND_FALL {
                if is_cpu
                    && !fighter_is_nana
                    && crate::training::named_macros::position_wait_timed_out(
                        status,
                        *FIGHTER_STATUS_KIND_FALL,
                    )
                {
                    // The macro action is cancelled, not successfully ready. Finish
                    // ordinary restore cleanup below (including trainer/partner work)
                    // rather than abandoning a sleeping/buffing fighter mid-phase.
                    save_state.state = NoAction;
                } else {
                    StatusModule::change_status_request(
                        module_accessor,
                        *FIGHTER_STATUS_KIND_FALL,
                        false,
                    );
                }
            } else {
                save_state.state = NoAction;
            }
        } else if save_state.situation_kind == SITUATION_KIND_CLIFF {
            if status != FIGHTER_STATUS_KIND_FALL
                && status != FIGHTER_STATUS_KIND_CLIFF_CATCH_MOVE
                && status != FIGHTER_STATUS_KIND_CLIFF_CATCH
            {
                StatusModule::change_status_request(
                    module_accessor,
                    *FIGHTER_STATUS_KIND_FALL,
                    false,
                );
            } else {
                save_state.state = NoAction;
            }
        } else {
            save_state.state = NoAction;
        }

        // If we're done moving, reset percent, handle charges, and apply buffs
        if save_state.state == NoAction {
            // Set damage of the save state
            crate::logging::diagnostic(format_args!("[PracticeSave] position_restored cpu={} active_kind=0x{:x} saved_kind=0x{:x} x={} y={}", is_cpu, fighter_kind, save_state.fighter_kind, pos.x, pos.y));
            if quick_reset || crate::replay_preview::occupied() {
                set_damage(module_accessor, save_state.percent);
            } else if !is_cpu {
                match read(&MENU).save_damage_player {
                    SaveDamage::SAVED => {
                        set_damage(module_accessor, save_state.percent);
                    }
                    SaveDamage::RANDOM => {
                        // Gen random value
                        let pct: f32 = get_random_float(
                            read(&MENU).save_damage_limits_player.LOWER as f32,
                            read(&MENU).save_damage_limits_player.UPPER as f32,
                        );
                        set_damage(module_accessor, pct);
                    }
                    SaveDamage::DEFAULT => {}
                    _ => panic!(
                        "Invalid value in save_states()::save_damage_player: {}",
                        read(&MENU).save_damage_player
                    ),
                }
            } else {
                match read(&MENU).save_damage_cpu {
                    SaveDamage::SAVED => {
                        set_damage(module_accessor, save_state.percent);
                    }
                    SaveDamage::RANDOM => {
                        // Gen random value
                        let pct: f32 = get_random_float(
                            read(&MENU).save_damage_limits_cpu.UPPER as f32,
                            read(&MENU).save_damage_limits_cpu.LOWER as f32,
                        );
                        set_damage(module_accessor, pct);
                    }
                    SaveDamage::DEFAULT => {}
                    _ => panic!(
                        "Invalid value in save_states()::save_damage_cpu: {}",
                        read(&MENU).save_damage_cpu
                    ),
                }
            }

            // Set to held item
            if !is_cpu && !fighter_is_nana {
                if let Some(item) = save_state.held_item {
                    crate::character_items::restore(module_accessor, item);
                } else if read(&MENU).character_item != CharacterItem::NONE {
                    apply_item(read(&MENU).character_item);
                }
            }

            // Set the charge of special moves if the fighter matches the kind in the save state
            if save_state.fighter_kind == fighter_kind {
                charge::handle_charge(module_accessor, fighter_kind, save_state.charge);
                // If we ever want to buff kirby, we have to move this to after apply buff
                if fighter_kind == FIGHTER_KIND_KIRBY {
                    save_state.state = WaitForCopyAbility;
                }
            }
            // Buff the fighter if they're one of the fighters who can be buffed
            if fighter_is_buffable {
                save_state.state = ApplyBuff;
            }
            // Perform fighter specific loading actions
            if let Some(load_steve) = save_state.steve_state {
                steve::load_steve_state(module_accessor, load_steve);
            }
            // Play Training Reset SFX, since silence is eerie
            // Only play for the CPU so we don't have 2 overlapping
            if is_cpu {
                SoundModule::play_se_no3d(
                    module_accessor,
                    Hash40::new("se_system_position_reset"),
                    true,
                    true,
                );
            }
            if fighter_is_ptrainer {
                WorkModule::on_flag(
                    ptrainer::get_ptrainer_module_accessor(module_accessor),
                    *WEAPON_PTRAINER_PTRAINER_INSTANCE_WORK_ID_FLAG_ENABLE_CHANGE_POKEMON,
                );
                save_state.state = WaitForPokemonSwitch;
            }
        }

        // if the fighter is Popo, change the state to one where only Nana can move
        // This is needed because for some reason, outside of frame by frame mode,
        // Popo will keep trying to move instead of letting Nana move if you just
        // change the state back to PosMove
        let prev_status_kind = StatusModule::prev_status_kind(module_accessor, 0);
        if prev_status_kind == FIGHTER_STATUS_KIND_REBIRTH
            && fighter_is_popo
            && !crate::character_roster::solo_popo_active(is_cpu)
        {
            save_state.state = NanaPosMove;
        }

        // if we're recording on state load, record
        // Named work is queued once and starts after BOTH fighters finish all
        // restoration phases, rather than once per PosMove callback.
        if quick_reset {
            if save_state_player(selected_slot).state == NoAction && save_state_cpu(selected_slot).state == NoAction {
                QUICK_RESET_ACTIVE.store(false, Ordering::Release);
            }
            return;
        }
        if crate::training::named_macros::source_is_named() || crate::replay_preview::occupied() {
            return;
        }
        if read(&MENU)
            .record_trigger
            .contains(&RecordTrigger::SAVESTATE)
        {
            input_record::lockout_record();
            return;
        }
        // otherwise, begin input recording playback if selected
        // for ledge, don't do this - if you want playback on a ledge, you have to set it as a ledge option,
        // otherwise there too many edge cases here
        else if read(&MENU).save_state_playback.get_random() != PlaybackSlot::empty()
            && save_state.situation_kind != SITUATION_KIND_CLIFF
        {
            input_record::playback(read(&MENU).save_state_playback.get_random().into_idx());
        }

        return;
    }

    if save_state.state == ApplyBuff {
        // needs its own save_state.state since this may take multiple frames, want it to loop
        if buff::handle_buffs(module_accessor, fighter_kind, status) {
            // returns true when done buffing fighter
            buff::restart_buff(module_accessor);
            // set is_buffing back to false when done
            save_state.state = NoAction;
        }
    }

    // If we switched last frame, artifacts and sound should be cleaned up, so transition into NoAction
    if save_state.state == WaitForPokemonSwitch
        && ptrainer::is_switched(ptrainer::get_ptrainer_module_accessor(module_accessor))
    {
        save_state.state = NoAction;
    }

    // Wait for Copy Ability to be applied if needed, exit otherwise
    if save_state.state == WaitForCopyAbility {
        let is_copy_start = WorkModule::is_flag(module_accessor, 0x20000104); // *FIGHTER_KIRBY_INSTANCE_WORK_ID_FLAG_COPY_ON_START
        if !is_copy_start {
            save_state.state = NoAction;
        }
    }

    // Save state
    if quick_reset {
        if save_state_player(selected_slot).state == NoAction && save_state_cpu(selected_slot).state == NoAction {
            QUICK_RESET_ACTIVE.store(false, Ordering::Release);
        }
        return;
    }
    if button_config::combo_passes(button_config::ButtonCombo::SaveState)
        && !crate::replay_preview::occupied()
    {
        // Don't begin saving state if Nana's delayed input is captured
        MIRROR_STATE = 1.0;
        save_state_player(read(&MENU).save_state_slot.into_idx().unwrap_or(0)).state = Save;
        save_state_cpu(read(&MENU).save_state_slot.into_idx().unwrap_or(0)).state = Save;
        notifications::clear_notification("Save State");
        notifications::notification(
            "Save State".to_string(),
            format!("Saved Slot {}", read(&MENU).save_state_slot),
            120,
        );
    }

    if save_state.state == Save && !fighter_is_nana {
        // Don't save states with Nana. Should already be fine, just a safety.
        *save_state = snapshot_live_state(module_accessor, role);
        crate::logging::diagnostic(format_args!(
            "[PracticeSave] snapshot_saved slot={} cpu={} kind=0x{:x} x={} y={} damage={}",
            selected_slot, is_cpu, fighter_kind, save_state.x, save_state.y, save_state.percent
        ));

        let zeros = Vector3f {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };

        EffectModule::req_on_joint(
            module_accessor,
            Hash40::new("sys_deku_flash"),
            Hash40::new("top"),
            &zeros,
            &zeros,
            0.25,
            &zeros,
            &zeros,
            true,
            *EFFECT_SUB_ATTRIBUTE_NO_JOINT_SCALE as u32
                | *EFFECT_SUB_ATTRIBUTE_FOLLOW as u32
                | *EFFECT_SUB_ATTRIBUTE_CONCLUDE_STATUS as u32,
            0,
            0,
        );

        // If both chars finished saving by now
        if save_state_player(selected_slot).state != Save
            && save_state_cpu(selected_slot).state != Save
        {
            // Copy here on the game thread, the only mutator of SAVE_STATE_SLOTS, to avoid races.
            let save_state_slots_copy = *SAVE_STATE_SLOTS.data_ptr();
            save_to_file(save_state_slots_copy);
        }
    }
}

fn adjust_ledge_pos(pos: &mut Vector3f, fighter_kind: i32, situation_kind: i32) {
    // Adjusts save states for fighters who have grabbed the ledge
    if situation_kind != SITUATION_KIND_CLIFF {
        return;
    }

    let fighter_needs_ledge_adjust_far = [
        *FIGHTER_KIND_DONKEY,
        *FIGHTER_KIND_DEDEDE,
        *FIGHTER_KIND_KROOL,
    ]
    .contains(&fighter_kind);

    let fighter_needs_ledge_adjust_medium = [
        *FIGHTER_KIND_SAMUS,
        *FIGHTER_KIND_SAMUSD,
        *FIGHTER_KIND_MEWTWO,
        *FIGHTER_KIND_DOLLY,
    ]
    .contains(&fighter_kind);

    let fighter_needs_ledge_adjust_slight = [
        *FIGHTER_KIND_KOOPA,
        *FIGHTER_KIND_ELIGHT,
        *FIGHTER_KIND_SNAKE,
    ]
    .contains(&fighter_kind);

    if fighter_needs_ledge_adjust_far {
        pos.y += 6.0;
    } else if fighter_needs_ledge_adjust_medium {
        pos.y += 4.0;
    }
    if fighter_needs_ledge_adjust_slight {
        pos.y += 2.0;
    }
}
