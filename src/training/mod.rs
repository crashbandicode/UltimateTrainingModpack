use std::ptr::addr_of_mut;

use crate::common::button_config;
use crate::common::consts::{BuffOption, FighterId, MENU};
use crate::common::offsets::*;
use crate::common::{
    dev_config, is_operation_cpu, is_training_mode, menu, try_get_module_accessor,
    FIGHTER_MANAGER_ADDR, ITEM_MANAGER_ADDR, STAGE_MANAGER_ADDR,
};
use crate::filesystem;
use crate::hitbox_visualizer;
use crate::input::*;
use crate::logging::*;
use crate::training::character_specific::{items, kirby, pikmin, ptrainer};
use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use skyline::nn::ro::LookupSymbol;
use smash::app::{
    enSEType, lua_bind::*, utility, BattleObjectModuleAccessor, FighterSpecializer_Jack,
};
use smash::lib::lua_const::*;
use smash::params::*;
use smash::phx::{Hash40, Vector3f};
use training_mod_sync::*;

pub mod buff;
pub mod charge;
pub mod clatter;
pub mod combo;
pub mod crouch;
pub mod directional_influence;
pub mod frame_counter;
pub mod ledge;
pub mod lobby;
pub mod sdi;
pub mod shield;
pub mod tech;
pub mod throw;
pub mod ui;

mod air_dodge_direction;
mod attack_angle;
pub mod character_specific;
mod fast_fall;
mod full_hop;
pub mod input_delay;
mod input_log;
pub(crate) mod input_record;
pub mod named_macros;

pub(crate) fn recording_or_playback() -> bool {
    input_record::is_recording()
        || unsafe { input_record::is_standby() }
        || input_record::is_playback()
}
mod mash;
mod reset;
pub mod save_states;
mod shield_tilt;

/// An explicit menu disable cancels the old sequence once. Ordinary trigger
/// inactivity and unrelated menu edits must leave queued follow-ups alone.
pub(crate) fn mash_settings_changed(
    previous: &training_mod_consts::TrainingModpackMenu,
    current: &training_mod_consts::TrainingModpackMenu,
) {
    use training_mod_consts::{Action, MashTrigger};
    if current.mash_state == Action::empty()
        && current.mash_triggers == MashTrigger::empty()
        && (previous.mash_state != current.mash_state
            || previous.mash_triggers != current.mash_triggers)
    {
        mash::full_reset();
    }
}

/// Cancel a departing practice session without touching game objects or game RNG.
pub fn clear_session_state() {
    practice::forget();
    crate::practice_trace::checkpoint(20, [0; 5]);
    crate::kirby_hats::cancel_pending();
    crate::character_items::cancel_pending();
    menu::clear_session_state();
    button_config::clear_session_state();
    input_delay::clear_session_state();
    input_log::clear_session_state();
    combo::clear_session_state();
    shield::clear_session_state();
    buff::clear_session_state();
    mash::full_reset();
    mash::set_aerial(crate::consts::Action::empty());
    ledge::reset_ledge_case();
    ledge::reset_ledge_delay();
    throw::reset_throw_case();
    throw::reset_throw_delay();
    throw::reset_pummel_delay();
    frame_counter::reset_all();
    save_states::cancel_pending();
    save_states::clear_quick_reset();
    reset::clear_session_state();
    ui::notifications::clear_all_notifications();
    named_macros::clear_session();
    crate::replay_preview::clear_session();
    unsafe {
        input_record::stop_playback();
    }
    crate::practice_trace::checkpoint(21, [0; 5]);
}

pub mod practice;
mod practice_policy;

#[cfg(debug_assertions)]
mod debug;

#[skyline::hook(replace = WorkModule::get_param_float)]
pub unsafe fn handle_get_param_float(
    module_accessor: &mut BattleObjectModuleAccessor,
    param_type: u64,
    param_hash: u64,
) -> f32 {
    let ori = original!()(module_accessor, param_type, param_hash);
    if !is_training_mode() {
        return ori;
    }

    shield::get_param_float(module_accessor, param_type, param_hash).unwrap_or(ori)
}

#[skyline::hook(replace = WorkModule::get_param_int)]
pub unsafe fn handle_get_param_int(
    module_accessor: &mut BattleObjectModuleAccessor,
    param_type: u64,
    param_hash: u64,
) -> i32 {
    let ori = original!()(module_accessor, param_type, param_hash);

    if !is_training_mode() {
        return ori;
    }

    save_states::get_param_int(module_accessor, param_type, param_hash).unwrap_or(ori)
}

#[skyline::hook(replace = ControlModule::get_attack_air_kind)]
pub unsafe fn handle_get_attack_air_kind(module_accessor: &mut BattleObjectModuleAccessor) -> i32 {
    let ori = original!()(module_accessor);
    if !is_training_mode() {
        return ori;
    }

    if input_record::is_playback() {
        return ori;
    }
    mash::get_attack_air_kind(module_accessor).unwrap_or(ori)
}

#[skyline::hook(replace = ControlModule::get_command_flag_cat)]
pub unsafe fn handle_get_command_flag_cat(
    module_accessor: &mut BattleObjectModuleAccessor,
    category: i32,
) -> i32 {
    let mut flag = original!()(module_accessor, category);

    lobby::update();

    if !is_training_mode() || crate::replay_preview::occupied() { return flag; }
    lobby::observe_fighter(module_accessor);

    if lobby::is_waiting_scene()
        && (!crate::common::is_fighter(module_accessor)
            || !lobby::permits_entry(WorkModule::get_int(
                module_accessor,
                *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID,
            )))
    {
        return flag;
    }

    flag |= mash::get_command_flag_cat(module_accessor, category);
    // Get throw directions
    flag |= throw::get_command_flag_throw_direction(module_accessor);

    flag
}

#[skyline::from_offset(OFFSET_FIGHTER_VTABLE_ON_FRAME)]
unsafe extern "C" fn fighter_vtable_on_frame_inner(fighter: &mut smash::app::Fighter);

unsafe extern "C" fn fighter_vtable_on_frame(fighter: &mut smash::app::Fighter) {
    lobby::update();
    // One native entry-lease refresh per game frame, on the P1 body's own callback.
    // Entry destruction and scene changes still retire immediately through
    // `practice::entry_departed` / `practice::scene_departed`.
    if fighter.battle_object.entry_id == 0
        && fighter.battle_object.kind != *FIGHTER_KIND_NANA as u32 {
        practice::refresh();
    }
    tech::restore_visibility(&mut *fighter.battle_object.module_accessor);
    let trace_identity = [fighter as *mut _ as u64, fighter.battle_object.module_accessor as u64,
        u64::from(fighter.battle_object.battle_object_id), u64::from(fighter.battle_object.kind), fighter.battle_object.entry_id];
    crate::practice_trace::event(200, trace_identity);
    #[cfg(feature = "source-lifecycle-probe")]
    crate::character_roster::source_lifecycle_probe::body_frame(fighter.battle_object.module_accessor);
    if crate::character_roster::sources::reserved(&mut *fighter.battle_object.module_accessor) {
        crate::practice_trace::event(210, trace_identity);
        fighter_vtable_on_frame_inner(fighter);
        crate::practice_trace::event(211, trace_identity);
        return;
    }
    #[cfg(any(feature = "source-owner-probe", feature = "managed-sources"))]
    if crate::character_roster::sources::legacy_training_owner_reserved(&mut *fighter.battle_object.module_accessor) {
        crate::practice_trace::event(212, trace_identity);
        fighter_vtable_on_frame_inner(fighter);
        crate::practice_trace::event(213, trace_identity);
        return;
    }
    named_macros::observe_playback_before(&mut *fighter.battle_object.module_accessor);
    crate::practice_trace::event(201, trace_identity);
    crate::replay_capture::before_frame(fighter);
    crate::practice_trace::event(202, trace_identity);
    crate::replay_preview::before_frame(&mut *fighter.battle_object.module_accessor);
    crate::practice_trace::event(203, trace_identity);
    named_macros::hold_panel_fighter(&mut *fighter.battle_object.module_accessor);
    crate::practice_trace::event(204, trace_identity);
    fighter_vtable_on_frame_inner(fighter);
    crate::practice_trace::event(205, trace_identity);
    crate::replay_capture::after_frame(fighter);
    #[cfg(feature = "cpu-pilot-probe")]
    crate::cpu_pilot::on_frame(&mut *fighter.battle_object.module_accessor);
    #[cfg(feature = "source-owner-probe")]
    crate::character_roster::source_owner_probe::observe_copy(fighter.battle_object.module_accessor);
    once_per_frame_per_fighter(&mut *fighter.battle_object.module_accessor);
    crate::practice_trace::event(206, trace_identity);
    crate::replay_preview::after_frame(&mut *fighter.battle_object.module_accessor);
    crate::practice_trace::event(207, trace_identity);
}

fn once_per_frame_per_fighter(module_accessor: &mut BattleObjectModuleAccessor) {
    unsafe {
        lobby::update();
        crate::character_roster::sources::on_frame(module_accessor);
        // Restore shield parameters on departure even when practice is inactive.
        shield::param_installer();
        crate::character_items::on_frame(module_accessor);
        if !is_training_mode() {
            return;
        }
        if lobby::is_waiting_scene()
            && !lobby::permits_entry(WorkModule::get_int(
                module_accessor,
                *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID,
            ))
        {
            return;
        }

        if crate::replay_preview::occupied() {
            // Keep native simulation and saved-state cleanup live, but suppress
            // Training mash/DI/ledge/shield automations during the clip preview.
            combo::once_per_frame(module_accessor);
            save_states::save_states(module_accessor);
            if practice::primary_cpu(module_accessor) {
                named_macros::service_restore(module_accessor);
                named_macros::observe_playback_start(module_accessor);
                input_record::handle_recording();
                frame_counter::tick_ingame();
            }
            return;
        }

        #[cfg(feature = "item-state-probe")]
        crate::item_state_probe::on_frame(module_accessor);

        if menu::menu_condition() {
            menu::spawn_menu();
        }

        if is_operation_cpu(module_accessor) {
            lobby::prepare_cpu_options();
            // Handle dodge staling here b/c input recording or mash can cause dodging
            WorkModule::set_flag(
                module_accessor,
                !(read(&MENU).stale_dodges.as_bool()),
                *FIGHTER_INSTANCE_WORK_ID_FLAG_DISABLE_ESCAPE_PENALTY,
            );
            if practice::primary_cpu(module_accessor) {
                named_macros::observe_playback_start(module_accessor);
                input_record::handle_recording();
            }
            frame_counter::tick_ingame();
            if practice::primary_cpu(module_accessor) {
                tech::hide_tech(module_accessor);
            }
        }

        combo::once_per_frame(module_accessor);
        if practice::role(module_accessor).is_some() {
            named_macros::service_character();
            if !practice::enabled() { return; }
            hitbox_visualizer::once_per_frame(module_accessor);
            save_states::save_states(module_accessor);
            if practice::primary_cpu(module_accessor) {
                named_macros::service_restore(module_accessor);
                named_macros::preview(module_accessor);
            }
        } else if crate::common::is_native_training_mode() && practice::enabled() {
            // Extra native Training CPUs retain visualization; they still do
            // not own either of the two save/record slots.
            hitbox_visualizer::once_per_frame(module_accessor);
        }
        tech::once_per_frame(module_accessor);
        clatter::handle_clatter(module_accessor);
    }

    fast_fall::do_fast_fall(module_accessor);
    ledge::do_ledge_option(module_accessor);
    shield::handle_shield(module_accessor);
    directional_influence::set_di(module_accessor);
}

#[skyline::from_offset(*OFFSET_FIGHTER_VTABLE_ON_INIT)]
unsafe extern "C" fn fighter_vtable_on_init_inner(fighter: &mut smash::app::Fighter, param_2: u64);

unsafe extern "C" fn fighter_vtable_on_init(fighter: &mut smash::app::Fighter, param_2: u64) {
    fighter_vtable_on_init_inner(fighter, param_2);
    if crate::common::is_native_training_mode() && !crate::character_roster::busy() {
        reset::check_reset(&mut *fighter.battle_object.module_accessor);
    }
}

/**
 * This is called to get the stick position when
 * shielding (shield tilt)
 * 1 is fully right, -1 is fully left
 */
#[skyline::hook(replace = ControlModule::get_stick_x_no_clamp)]
pub unsafe fn get_stick_x_no_clamp(module_accessor: &mut BattleObjectModuleAccessor) -> f32 {
    let ori = original!()(module_accessor);
    if !is_training_mode() {
        return ori;
    }

    shield_tilt::mod_get_stick_x(module_accessor).unwrap_or(ori)
}

/**
 * This is called to get the stick position when
 * shielding (shield tilt)
 * 1 is fully up, -1 is fully down
 */
#[skyline::hook(replace = ControlModule::get_stick_y_no_clamp)]
pub unsafe fn get_stick_y_no_clamp(module_accessor: &mut BattleObjectModuleAccessor) -> f32 {
    let ori = original!()(module_accessor);
    if !is_training_mode() {
        return ori;
    }

    shield_tilt::mod_get_stick_y(module_accessor).unwrap_or(ori)
}

/**
 * Called when:
 * Walking in the facing direction
 * Air Dodging
 */
#[skyline::hook(replace = ControlModule::get_stick_x)]
pub unsafe fn get_stick_x(module_accessor: &mut BattleObjectModuleAccessor) -> f32 {
    let ori = original!()(module_accessor);
    if !is_training_mode() {
        return ori;
    }

    air_dodge_direction::mod_get_stick_x(module_accessor).unwrap_or(ori)
}

/**
 * Called when:
 * angled ftilt/fsmash
 */
#[skyline::hook(replace = ControlModule::get_stick_dir)]
pub unsafe fn get_stick_dir(module_accessor: &mut BattleObjectModuleAccessor) -> f32 {
    let ori = original!()(module_accessor);
    if !is_training_mode() {
        return ori;
    }

    let situation_kind = StatusModule::situation_kind(module_accessor);
    if situation_kind == *SITUATION_KIND_CLIFF {
        return ori;
    }

    if input_record::is_playback() {
        return ori;
    }

    attack_angle::mod_get_stick_dir(module_accessor).unwrap_or(ori)
}

/**
 * Called when:
 * Directional airdodge
 * Crouching
 */
#[skyline::hook(replace = ControlModule::get_stick_y)]
pub unsafe fn get_stick_y(module_accessor: &mut BattleObjectModuleAccessor) -> f32 {
    let ori = original!()(module_accessor);
    if !is_training_mode() {
        return ori;
    }

    air_dodge_direction::mod_get_stick_y(module_accessor)
        .unwrap_or_else(|| crouch::mod_get_stick_y(module_accessor).unwrap_or(ori))
}

#[skyline::hook(replace = ControlModule::check_button_on)]
pub unsafe fn handle_check_button_on(
    module_accessor: &mut BattleObjectModuleAccessor,
    button: i32,
) -> bool {
    let ori = original!()(module_accessor, button);
    if !is_training_mode() {
        return ori;
    }

    shield::check_button_on(module_accessor, button)
        .unwrap_or_else(|| full_hop::check_button_on(module_accessor, button).unwrap_or(ori))
}

#[skyline::hook(replace = ControlModule::check_button_off)]
pub unsafe fn handle_check_button_off(
    module_accessor: &mut BattleObjectModuleAccessor,
    button: i32,
) -> bool {
    let ori = original!()(module_accessor, button);
    if !is_training_mode() {
        return ori;
    }

    shield::check_button_off(module_accessor, button)
        .unwrap_or_else(|| full_hop::check_button_off(module_accessor, button).unwrap_or(ori))
}

#[skyline::hook(replace = MotionModule::change_motion)]
pub unsafe fn handle_change_motion(
    module_accessor: &mut BattleObjectModuleAccessor,
    motion_kind: u64,
    unk1: f32,
    unk2: f32,
    unk3: bool,
    unk4: f32,
    unk5: bool,
    unk6: bool,
) -> u64 {
    let mod_motion_kind = if is_training_mode() {
        tech::change_motion(module_accessor, motion_kind).unwrap_or(motion_kind)
    } else {
        motion_kind
    };

    let ori = original!()(
        module_accessor,
        mod_motion_kind,
        unk1,
        unk2,
        unk3,
        unk4,
        unk5,
        unk6,
    );
    // After we've changed motion, speed up if necessary
    if is_training_mode() {
        ptrainer::change_motion(module_accessor, motion_kind);
    }
    ori
}

#[skyline::hook(replace = WorkModule::is_enable_transition_term)]
pub unsafe fn handle_is_enable_transition_term(
    module_accessor: &mut BattleObjectModuleAccessor,
    transition_term: i32,
) -> bool {
    let ori = original!()(module_accessor, transition_term);

    if !is_training_mode() {
        return ori;
    }

    match ledge::is_enable_transition_term(module_accessor, transition_term) {
        Some(r) => r,
        None => ori,
    }
}

extern "C" {
    #[link_name = "\u{1}_ZN3app15sv_fighter_util15set_dead_rumbleEP9lua_State"]
    pub fn set_dead_rumble(lua_state: u64) -> u64;
}

#[skyline::hook(replace = set_dead_rumble)]
pub unsafe fn handle_set_dead_rumble(lua_state: u64) -> u64 {
    if crate::common::is_native_training_mode() {
        return 0;
    }

    original!()(lua_state)
}

#[skyline::hook(replace = CameraModule::req_quake)]
pub unsafe fn handle_req_quake(
    module_accessor: &mut BattleObjectModuleAccessor,
    my_int: i32,
) -> u64 {
    if !crate::common::is_native_training_mode() {
        return original!()(module_accessor, my_int);
    }
    if save_states::is_killing() {
        return original!()(module_accessor, *CAMERA_QUAKE_KIND_NONE);
    }
    original!()(module_accessor, my_int)
}

pub static mut COMMON_PARAMS: *mut CommonParams = std::ptr::null_mut();

fn params_main(params_info: &ParamsInfo<'_>) {
    if let Ok(common) = params_info.get::<CommonParams>() {
        unsafe {
            COMMON_PARAMS = common as *mut _;
            println!(
                "[TrainingModpack] common params loaded shield_damage_mul={} mode={:?}",
                (*COMMON_PARAMS).shield_damage_mul,
                crate::common::game_mode::current()
            );
        }
    }
}

// this function is used to add limit to Cloud's limit gauge. Hooking it here so we can call it in buff.rs
#[skyline::hook(offset = *OFFSET_CLOUD_ADD_LIMIT)]
pub unsafe fn handle_add_limit(
    add_limit: f32,
    module_accessor: &mut BattleObjectModuleAccessor,
    is_special_lw: u64,
) {
    original!()(add_limit, module_accessor, is_special_lw)
}

#[skyline::hook(replace = EffectModule::req_screen)] // hooked to prevent the screen from darkening when loading a save state with One-Winged Angel
pub unsafe fn handle_req_screen(
    module_accessor: &mut BattleObjectModuleAccessor,
    my_hash: Hash40,
    bool_1: bool,
    bool_2: bool,
    bool_3: bool,
) -> u64 {
    if !crate::common::is_native_training_mode() {
        return original!()(module_accessor, my_hash, bool_1, bool_2, bool_3);
    }
    let new_hash = my_hash.hash;
    if new_hash == 72422354958 && buff::is_buffing(module_accessor) {
        // Wing bg hash
        let replace_hash = Hash40::new("bg");
        return original!()(module_accessor, replace_hash, bool_1, bool_2, bool_3);
    }
    original!()(module_accessor, my_hash, bool_1, bool_2, bool_3)
}

#[skyline::hook(replace = FighterSpecializer_Jack::check_doyle_summon_dispatch)] // returns status of summon dispatch if triggered, -1 as u64 otherwise
pub unsafe fn handle_check_doyle_summon_dispatch(
    module_accessor: &mut BattleObjectModuleAccessor,
    bool_1: bool,
    bool_2: bool,
) -> u64 {
    let ori = original!()(module_accessor, bool_1, bool_2);
    if !crate::common::is_native_training_mode() {
        return ori;
    }
    if ori == *FIGHTER_JACK_STATUS_KIND_SUMMON as u64 && buff::is_buffing(module_accessor) {
        return 4294967295;
    }
    ori
}

#[skyline::hook(offset = *OFFSET_ADD_DAMAGE)]
pub unsafe fn handle_add_damage(
    damage_module: *mut u64, // DamageModule
    mut damage_to_add: f32,
    param_2: i32,
) -> u64 {
    if !crate::common::is_native_training_mode() {
        return original!()(damage_module, damage_to_add, param_2);
    }
    let module_accessor =
        &mut **(damage_module.byte_add(0x8) as *mut *mut BattleObjectModuleAccessor);
    // Prevent Wii Fit Deep Breathing from Healing on Save State Load
    if utility::get_kind(module_accessor) == *FIGHTER_KIND_WIIFIT
        && buff::is_buffing(module_accessor)
    {
        damage_to_add = 0.0;
    }
    #[cfg(feature = "source-owner-probe")]
    let before = DamageModule::damage(module_accessor, 0);
    let result = original!()(damage_module, damage_to_add, param_2);
    #[cfg(feature = "source-owner-probe")]
    crate::character_roster::source_owner_probe::observe_damage(module_accessor, damage_to_add, before);
    result
}

// Control L+R+A Resets
// This function already checks for training mode, so we don't need to check for training mode here
#[skyline::hook(offset = *OFFSET_TRAINING_RESET_CHECK, inline)]
unsafe fn lra_handle(ctx: &mut InlineCtx) {
    if !crate::common::is_native_training_mode() {
        return;
    }
    if !(read(&MENU).lra_reset.as_bool()) {
        ctx.registers[8].set_x(0);
    }
}

// Set Stale Moves to On
// One instruction after stale moves toggle register is set to 0
#[skyline::hook(offset = *OFFSET_STALE, inline)]
unsafe fn stale_handle(ctx: &mut InlineCtx) {
    lobby::update();
    if !crate::common::is_native_training_mode() || ctx.registers[22].x() == 0 {
        return;
    }
    // Native 13.0.5 writes a byte at +0xb60. The caller owns this menu here;
    // never retain its address for a later HUD draw or scene.
    let pause_menu = ctx.registers[22].x() as *mut u8;
    *pause_menu.add(0xb60) = 1;
    ui::native_menu::observe_combo_display(*pause_menu.add(0xb6c));
}

// Set Stale Moves to On in the menu text
// One instruction after menu text register is set to off
#[skyline::hook(offset = *OFFSET_STALE_MENU, inline)]
unsafe fn stale_menu_handle(ctx: &mut InlineCtx) {
    if !crate::common::is_native_training_mode() {
        return;
    }
    // Set the text pointer to where "mel_training_on" is located
    let on_text_ptr = (getRegionAddress(Region::Text) as u64) + 0x42b415e;
    ctx.registers[1].set_x(on_text_ptr);
}

#[skyline::hook(replace = SoundModule::play_se)] // hooked to prevent death sfx from playing when loading save states
pub unsafe fn handle_se(
    module_accessor: &mut BattleObjectModuleAccessor,
    my_hash: Hash40,
    bool1: bool,
    bool2: bool,
    bool3: bool,
    bool4: bool,
    se_type: enSEType,
) -> u64 {
    if !save_states::effects_enabled() {
        return original!()(
            module_accessor,
            my_hash,
            bool1,
            bool2,
            bool3,
            bool4,
            se_type,
        );
    }
    // Make effects silent while we're killing fighters. Stops death explosion and fighter misfoot.
    if save_states::is_killing() {
        let silent_hash = Hash40::new("se_silent");
        return original!()(
            module_accessor,
            silent_hash,
            bool1,
            bool2,
            bool3,
            bool4,
            se_type,
        );
    }
    original!()(
        module_accessor,
        my_hash,
        bool1,
        bool2,
        bool3,
        bool4,
        se_type,
    )
}

#[repr(C)]
pub struct FighterSoundModule {
    vtable: u64,
    owner: *mut BattleObjectModuleAccessor,
}

// fighters don't use the symbol and go straight through their vtable to this function
#[skyline::hook(offset = *OFFSET_PLAY_SE)]
pub unsafe fn handle_fighter_play_se(
    sound_module: *mut FighterSoundModule, // pointer to fighter's SoundModule
    mut my_hash: Hash40,
    bool1: bool,
    bool2: bool,
    bool3: bool,
    bool4: bool,
    se_type: enSEType,
) -> u64 {
    if !save_states::effects_enabled() {
        return original!()(sound_module, my_hash, bool1, bool2, bool3, bool4, se_type);
    }
    // Supress Buff Sound Effects while buffing
    if buff::is_buffing_any() {
        my_hash = Hash40::new("se_silent");
    }
    // Supress Kirby Copy Ability SFX when loading Save State
    if my_hash.hash == 0x1453dd86e4 || my_hash.hash == 0x14bdd3e7c8 {
        let module_accessor = (*sound_module).owner;
        if StatusModule::status_kind(module_accessor) != FIGHTER_KIRBY_STATUS_KIND_SPECIAL_N_DRINK {
            my_hash = Hash40::new("se_silent");
        }
    }
    // Supress Sora Magic Switch SFX when loading Save State
    if my_hash.hash == 0x156fdf29ba {
        let module_accessor = (*sound_module).owner;
        // Kirby and Sora's Special N statuses are all here or higher
        if StatusModule::status_kind(module_accessor) < *FIGHTER_TRAIL_STATUS_KIND_SPECIAL_N1 {
            my_hash = Hash40::new("se_silent");
        }
    }
    my_hash = ptrainer::handle_pokemon_sound_effect(my_hash);
    original!()(sound_module, my_hash, bool1, bool2, bool3, bool4, se_type)
}

// hooked to prevent score gfx from playing when loading save states
#[skyline::hook(replace = EffectModule::req_follow)]
pub unsafe fn handle_effect_follow(
    module_accessor: &mut BattleObjectModuleAccessor,
    eff_hash: Hash40,
    joint_hash: Hash40,
    pos: *const Vector3f,
    rot: *const Vector3f,
    mut size: f32,
    arg5: bool,
    arg6: u32,
    arg7: i32,
    arg8: i32,
    arg9: i32,
    arg10: i32,
    arg11: bool,
    arg12: bool,
) -> u64 {
    if !save_states::effects_enabled() {
        return original!()(
            module_accessor,
            eff_hash,
            joint_hash,
            pos,
            rot,
            size,
            arg5,
            arg6,
            arg7,
            arg8,
            arg9,
            arg10,
            arg11,
            arg12,
        );
    }
    // Prevent the score GFX from playing on the CPU when loading save state during hitstop
    if eff_hash == Hash40::new("sys_score_aura") && save_states::is_loading() {
        size = 0.0
    }
    original!()(
        module_accessor,
        eff_hash,
        joint_hash,
        pos,
        rot,
        size,
        arg5,
        arg6,
        arg7,
        arg8,
        arg9,
        arg10,
        arg11,
        arg12,
    )
}

#[skyline::hook(replace = EffectModule::req_on_joint)] // hooked to prevent death gfx from playing when loading save states
pub unsafe fn handle_fighter_joint_effect(
    module_accessor: &mut BattleObjectModuleAccessor,
    eff_hash: Hash40,
    joint_hash: Hash40,
    pos: *const Vector3f,
    rot: *const Vector3f,
    mut size: f32,
    pos2: *const Vector3f, //unk, maybe displacement and not pos/rot
    rot2: *const Vector3f, //unk, ^
    arg5: bool,
    arg6: u32,
    arg7: i32,
    arg9: i32,
) -> u64 {
    if !save_states::effects_enabled() {
        return original!()(
            module_accessor,
            eff_hash,
            joint_hash,
            pos,
            rot,
            size,
            pos2,
            rot2,
            arg5,
            arg6,
            arg7,
            arg9,
        );
    }
    size = ptrainer::handle_pokemon_effect(module_accessor, eff_hash, size);
    original!()(
        module_accessor,
        eff_hash,
        joint_hash,
        pos,
        rot,
        size,
        pos2,
        rot2,
        arg5,
        arg6,
        arg7,
        arg9,
    )
}

#[skyline::hook(replace = EffectModule::req)] // hooked to prevent death gfx from playing when loading save states
pub unsafe fn handle_effect(
    module_accessor: &mut BattleObjectModuleAccessor,
    eff_hash: Hash40,
    pos: *const Vector3f,
    rot: *const Vector3f,
    mut size: f32,
    arg6: u32,
    arg7: i32,
    arg8: bool,
    arg9: i32,
) -> u64 {
    if !save_states::effects_enabled() {
        return original!()(
            module_accessor,
            eff_hash,
            pos,
            rot,
            size,
            arg6,
            arg7,
            arg8,
            arg9,
        );
    }
    // This used to be a second hook on req. Keep its adjustment before the
    // save-state suppression, then call the pre-existing effect handler once.
    size = ptrainer::handle_pokemon_effect(module_accessor, eff_hash, size);
    if save_states::is_loading() && !buff::is_buffing(module_accessor) {
        // Making the size 0 prevents these effects from being displayed. Fixes throw explosions, ICs squall, etc.
        return original!()(
            module_accessor,
            eff_hash,
            pos,
            rot,
            0.0,
            arg6,
            arg7,
            arg8,
            arg9,
        );
    }
    original!()(
        module_accessor,
        eff_hash,
        pos,
        rot,
        size,
        arg6,
        arg7,
        arg8,
        arg9,
    )
}

// can_futtobi_back, checks if stage allows for star KOs
#[skyline::hook(offset = *OFFSET_CAN_FUTTOBI_BACK)]
pub unsafe fn handle_star_ko(my_long_ptr: &mut u64) -> bool {
    let ori = original!()(my_long_ptr);
    if !save_states::effects_enabled() {
        ori
    } else {
        false
    }
}

// A function reused by many functions to update UI. Called to update at least Little Mac's meter.
#[skyline::hook(offset = *OFFSET_REUSED_UI)]
pub unsafe fn handle_reused_ui(
    fighter_data: *mut u32, // a pointer to length 4 data in the Fighter's FighterEntry in the FighterManager
    mut param_2: u32,       // In Little Mac's case, the meter value as an integer
) {
    if !is_training_mode() {
        return original!()(fighter_data, param_2);
    }

    if save_states::is_loading() {
        let player_module_accessor = &mut *try_get_module_accessor(FighterId::Player)
            .expect("Could not get player module accessor in reused_ui");
        let cpu_module_accessor = &mut *try_get_module_accessor(FighterId::CPU)
            .expect("Could not get CPU module accessor in reused_ui");
        let player_fighter_kind = utility::get_kind(player_module_accessor);
        let cpu_fighter_kind = utility::get_kind(cpu_module_accessor);
        // If Little Mac is in the game and we're buffing him, set the meter to 100
        if (player_fighter_kind == *FIGHTER_KIND_LITTLEMAC
            || cpu_fighter_kind == *FIGHTER_KIND_LITTLEMAC)
            && read(&MENU).buff_state.contains(&BuffOption::KO)
        {
            param_2 = 100;
        }
    }

    original!()(fighter_data, param_2)
}

#[skyline::hook(replace = ArticleModule::get_int)]
pub unsafe fn handle_article_get_int(
    module_accessor: *mut BattleObjectModuleAccessor,
    generate_article: i32,
    address: i32,
) -> i32 {
    original!()(module_accessor, generate_article, address)
}

// Instruction run on the completion of the CPU Control function
// One instruction after the CPU Control function completes
// #[skyline::hook(offset = *OFFSET_OPCF, inline)]
// unsafe fn handle_once_per_cpu_frame(_ctx: &mut InlineCtx) {
//     input_record::handle_recording();
//     frame_counter::tick_ingame();
//     tech::hide_tech();
// }

#[skyline::hook(offset = *OFFSET_FIM)]
unsafe fn handle_final_input_mapping(
    mappings: *mut ControllerMapping,
    player_idx: i32, // Is this the player index, or plugged in controller index? Need to check, assuming player for now - is this 0 indexed or 1?
    out: *mut MappedInputs,
    controller_struct: &mut SomeControllerStruct,
    arg: bool,
) {
    // Order of hooks here REALLY matters. Tread lightly

    // Go through the original mapping function first
    #[cfg(feature = "named-input-trace")]
    let close_raw_before = controller_struct.controller.current_buttons.b();
    original!()(mappings, player_idx, out, controller_struct, arg);
    #[cfg(feature = "cpu-pilot-probe")]
    crate::cpu_pilot::input(player_idx, out);
    lobby::update();
    // is_training_mode deliberately excludes a busy character loader. The parked
    // clip editor still owns P1 input then, including B cancellation and raw Plus
    // edges used by the native pause menu. This path must not access fighter objects.
    // After cancel, keep the existing close grace active while that loader finishes;
    // include count zero because input can run again before the next UI draw tick.
    let close_counter = *menu::MENU_CLOSE_FRAME_COUNTER;
    let closing_during_reload = crate::character_roster::busy()
        && frame_counter::_is_counting(close_counter)
        && frame_counter::get_frame_count(close_counter) < menu::MENU_CLOSE_WAIT_FRAMES;
    // Cover ordinary closes before the first draw tick and a closing B held beyond
    // the draw-clock grace. Preserve this physical B level in the owned path below:
    // FIM can run again before the next pad refresh, so clearing our release signal
    // would make that repeated call mistake our own neutralization for a release.
    let editor_close_owned = player_idx == 0
        && crate::replay_capture::owns_close_input(
            practice::scene_enabled(),
            controller_struct.controller.current_buttons.b(),
        );
    #[cfg(feature = "named-input-trace")]
    if player_idx == 0 {
        crate::replay_capture::trace_close_input(
            controller_struct.controller as *const _ as usize,
            frame_counter::get_frame_count(close_counter), close_raw_before,
            controller_struct.controller.current_buttons.b(), (*out).buttons.bits(), editor_close_owned);
    }
    let editor_was_loading = crate::replay_capture::loading();
    let editor_was_open = crate::replay_capture::panel_visible();
    if player_idx == 0
        && practice::scene_enabled() && !lobby::native_ui_busy()
        && (editor_was_loading || closing_during_reload || editor_close_owned
            || (crate::replay_preview::occupied() && !named_macros::panel_visible()
                && !crate::replay_preview::recovery_available()))
    {
        let current_b = controller_struct.controller.current_buttons.b();
        let previous_b = controller_struct.controller.previous_buttons.b();
        menu::handle_final_input_mapping(player_idx, controller_struct, out);
        // Loading cancellation arms the close guard inside menu handling, after
        // this poll's ownership check. Preserve its closing sample on this call too.
        let preserve_close_b = editor_close_owned
            || (editor_was_loading && !crate::replay_capture::loading())
            || (editor_was_open && !crate::replay_capture::panel_visible());
        input_delay::clear_session_state();
        *out = MappedInputs::empty();
        let controller = &mut *controller_struct.controller;
        controller.current_buttons = Default::default();
        controller.previous_buttons = Default::default();
        controller.just_down = Default::default();
        controller.just_release = Default::default();
        if preserve_close_b {
            controller.current_buttons.set_b(current_b);
            controller.previous_buttons.set_b(previous_b);
        }
        return;
    }
    if player_idx == 0 && practice::scene_enabled() && !lobby::native_ui_busy()
        && crate::replay_preview::recovery_available() && !named_macros::panel_visible() {
        // After the closing B has been consumed, leave native Plus/menu input
        // intact. Pending cleanup still owns the CPU and suppresses autoplay.
        input_delay::clear_session_state();
        return;
    }
    if !is_training_mode() {
        if player_idx == 0 {
            crate::character_roster::suppress_owned_buttons(&mut *out);
        }
        return;
    }

    if lobby::active() && lobby::native_ui_busy() {
        // Leave the original mapping and controller edges intact for native dialogs.
        return;
    }

    if player_idx == 0 && reset::waiting_shortcut(&controller_struct.controller) {
        *out = MappedInputs::empty();
        input_delay::clear_session_state();
        return;
    }

    // Check if we should apply hot reload configs
    // Non-mutable pull
    dev_config::handle_final_input_mapping(player_idx, controller_struct);

    // Grab menu inputs from player
    // MUTATES controller state to kill inputs when in or closing menu
    // Loading captures gameplay input, but must not enter menu::captures_input:
    // the CPU loader uses that predicate to decide whether it may start.
    let menu_was_capturing = player_idx == 0
        && (menu::captures_input() || crate::replay_capture::loading());
    menu::handle_final_input_mapping(player_idx, controller_struct, out);

    // Grab button input requests from player
    // MUTATES controller state to kill start presses for menu
    button_config::handle_final_input_mapping(player_idx, controller_struct);
    let named_stopped = practice::enabled()
        && input_record::handle_named_stop_request(player_idx);
    if player_idx == 0 {
        crate::character_roster::handle_input(&mut *out);
        #[cfg(feature = "source-capacity-probe")]
        crate::character_roster::source_capacity_probe::handle_input(
            &mut *out,
            controller_struct.controller.current_buttons.stick_l(),
            controller_struct.controller.current_buttons.stick_r(),
            controller_struct.controller.current_buttons.x(),
        );
        #[cfg(feature = "item-state-probe")]
        crate::item_state_probe::handle_input(
            &mut *out,
            controller_struct.controller.current_buttons.stick_l(),
            controller_struct.controller.current_buttons.x(),
        );
        #[cfg(feature = "mii-pool-observer")]
        crate::character_roster::mii_pool_observer::handle_input(
            &mut *out,
            controller_struct.controller.current_buttons.stick_l(),
            controller_struct.controller.current_buttons.x(),
        );
        #[cfg(feature = "mii-identical-reload-probe")]
        crate::character_roster::mii_reload_probe::handle_input(
            &mut *out,
            controller_struct.controller.current_buttons.stick_l(),
            controller_struct.controller.current_buttons.x(),
        );
        crate::kirby_hats::handle_input(
            &mut *out,
            controller_struct.controller.current_buttons.stick_l(),
            controller_struct.controller.current_buttons.stick_r(),
        );
    }

    // Potentially apply input delay
    // MUTATES controller state to delay inputs
    if player_idx == 0 && (named_stopped || menu_was_capturing || menu::captures_input()) {
        input_delay::clear_session_state();
        *out = MappedInputs::empty();
    } else {
        input_delay::handle_final_input_mapping(player_idx, out);
    }

    // Read potentially delayed state for loggers
    input_log::handle_final_input_mapping(player_idx, controller_struct, out);

    // Potentially apply input recording, thus with delay
    // MUTATES controller state to apply recording or playback
    if practice::enabled() {
        input_record::handle_final_input_mapping(player_idx, out);
    }
}

pub fn training_mods() {
    info!("Applying training mods.");
    lobby::init();
    crate::character_lifecycle::init();

    unsafe {
        let mut fighter_manager_addr_lock = lock_write(&FIGHTER_MANAGER_ADDR);
        LookupSymbol(
            addr_of_mut!(*fighter_manager_addr_lock),
            "_ZN3lib9SingletonIN3app14FighterManagerEE9instance_E\u{0}"
                .as_bytes()
                .as_ptr(),
        );
        drop(fighter_manager_addr_lock);

        // TODO!("This seems unused? Can we remove it?")
        let mut stage_manager_addr_lock = lock_write(&STAGE_MANAGER_ADDR);
        LookupSymbol(
            addr_of_mut!(*stage_manager_addr_lock),
            "_ZN3lib9SingletonIN3app12StageManagerEE9instance_E\u{0}"
                .as_bytes()
                .as_ptr(),
        );
        drop(stage_manager_addr_lock);

        let mut item_manager_addr_lock = lock_write(&ITEM_MANAGER_ADDR);
        LookupSymbol(
            addr_of_mut!(*item_manager_addr_lock),
            "_ZN3lib9SingletonIN3app11ItemManagerEE9instance_E\0"
                .as_bytes()
                .as_ptr(),
        );
        drop(item_manager_addr_lock);

        add_hook(params_main).expect("Could not find the params_hook plugin");
    }

    // Enable Custom Stages for Training Mode
    // Specifically, we prevent a field in StageSelectInfo of the Scene that controls if the Custom Stage tab is loaded
    //  from being set to false when we load the SSS in Training Mode
    skyline::patching::Patch::in_text(*OFFSET_SSS_TRAINING)
        .nop()
        .expect("Failed to nop OFFSET_SSS_TRAINING");

    println!(
        "Searching for STALE_MENU offset first! : {}",
        *OFFSET_STALE_MENU
    );
    println!("Searching for STALE offset second! : {}", *OFFSET_STALE);

    println!("Patching global fighter vtable functions with our own!");
    let _ = skyline::patching::Patch::in_text(OFFSET_FIGHTER_VTABLE_ON_INIT_ADDR)
        .data(fighter_vtable_on_init as *const () as u64);
    let _ = skyline::patching::Patch::in_text(OFFSET_FIGHTER_VTABLE_ON_FRAME_ADDR)
        .data(fighter_vtable_on_frame as *const () as u64);

    skyline::install_hooks!(
        // Mash airdodge/jump
        handle_get_command_flag_cat,
        // Hold/Infinite shield
        handle_check_button_on,
        handle_check_button_off,
        handle_get_param_float,
        // Save states
        handle_get_param_int,
        handle_set_dead_rumble,
        handle_req_quake,
        // Mash attack
        handle_get_attack_air_kind,
        // Attack angle
        get_stick_dir,
        // Tech options
        handle_change_motion,
        // Directional AirDodge,
        get_stick_x,
        get_stick_y,
        // Shield Tilt
        get_stick_x_no_clamp,
        get_stick_y_no_clamp,
        // Combo
        handle_is_enable_transition_term,
        // SDI
        crate::training::sdi::check_hit_stop_delay_command,
        // Buffs
        handle_add_limit,
        handle_check_doyle_summon_dispatch,
        handle_reused_ui,
    );
    unsafe {
        crate::effect_hook_chain::install(
            "req_screen", EffectModule::req_screen as *const () as usize,
            handle_req_screen as *const () as usize,
            &handle_req_screen_skyline_internal_original_fn,
            [0xf940a000, 0xf9400008, 0xf9413105, 0x12000042],
            handle_req_screen_skyline_internal_install_hook,
        );
    }
    skyline::install_hooks!(
        handle_add_damage,
        // Buff SFX
        handle_fighter_play_se,
        // Stale Moves
        stale_menu_handle, // This has to be initialized before stale_handle otherwise the offset search fails
        stale_handle,
        // Death SFX
        handle_se,
        // Death GFX
    );
    unsafe {
        crate::effect_hook_chain::install(
            "req", EffectModule::req as *const () as usize,
            handle_effect as *const () as usize,
            &handle_effect_skyline_internal_original_fn,
            [0xf940a000, 0xf9400008, 0xf9403508, 0x120000c6],
            handle_effect_skyline_internal_install_hook,
        );
    }
    skyline::install_hooks!(
    );
    unsafe {
        crate::effect_hook_chain::install(
            "req_follow", EffectModule::req_follow as *const () as usize,
            handle_effect_follow as *const () as usize,
            &handle_effect_follow_skyline_internal_original_fn,
            [0xf940a000, 0xf940000d, 0x394083e9, 0x394063ea],
            handle_effect_follow_skyline_internal_install_hook,
        );
    }
    skyline::install_hooks!(
        // Star KO turn off
        handle_star_ko,
        // Clatter
        clatter::hook_start_clatter,
        // Notifications
        // handle_once_per_cpu_frame,
        // Input
        handle_final_input_mapping,
        // Charge
        handle_article_get_int,
    );
    unsafe {
        crate::effect_hook_chain::install(
            "req_on_joint", EffectModule::req_on_joint as *const () as usize,
            handle_fighter_joint_effect as *const () as usize,
            &handle_fighter_joint_effect_skyline_internal_original_fn,
            [0xf940a000, 0xf940000b, 0xb94003e8, 0xb94013e9],
            handle_fighter_joint_effect_skyline_internal_install_hook,
        );
    }
    skyline::install_hooks!(
        // L+R+A Reset
        lra_handle
    );

    items::init();
    input_record::init();
    named_macros::init();
    crate::replay_capture::init();
    ui::init();
    pikmin::init();
    ptrainer::init();
    kirby::init();
    tech::init();
    filesystem::init();

    #[cfg(debug_assertions)]
    debug::init();
}
