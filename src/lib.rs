#![allow(clippy::missing_safety_doc)]

use std::fs;
use std::path::PathBuf;

use skyline::nn::oe;
use skyline::nro::{self, NroInfo};
use training_mod_consts::{OnOff, LEGACY_TRAINING_MODPACK_ROOT};
use training_mod_sync::*;

use crate::common::button_config::DEFAULT_OPEN_MENU_CONFIG;
use crate::common::*;
use crate::consts::TRAINING_MODPACK_ROOT;
use crate::logging::*;
use crate::training::ui::notifications::notification;

pub mod common;
mod hazard_manager;
mod hitbox_visualizer;
mod training;
#[cfg(feature = "cpu-pilot-probe")]
mod cpu_pilot;
#[cfg(any(feature = "automation-results-observer", feature = "automation-results-native"))]
mod automation_helper;

#[cfg(feature = "deluxe-sdk")]
mod capture_plugin;
mod character_lifecycle;
mod character_memory;
mod character_roster;
mod character_items;
mod turnip_identity;
pub mod source_lifetime;
mod kirby_hat_cycle;
mod kirby_hats;
#[cfg(any(feature = "mii-identity-probe", feature = "mii-pool-observer", feature="mii-saved-alpha"))]
mod mii_identity_binding;
#[cfg(any(feature="mii-fullconfig-probe",feature="mii-saved-alpha"))]
mod mii_native_access;
#[cfg(all(feature="mii-saved-alpha",any(feature="mii-identical-reload-probe",feature="mii-pool-observer")))]
compile_error!("saved Mii alpha owns its menu lifetime; do not combine Mii mutation/pool diagnostic dispatch");
#[cfg(all(feature="mii-saved-alpha",any(
    all(feature="managed-sources",not(feature="mii-managed-source-coexist-alpha")),
    feature="source-owner-probe",feature="source-capacity-probe")))]
compile_error!("saved Mii alpha preserves its captured scene scope; helper-source mutation features are not qualified together");
#[cfg(all(feature="mii-managed-source-coexist-alpha",feature="managed-third-source-alpha"))]
compile_error!("Mii/source coexistence alpha owns Training0/1 with helpers2/3 only; entry7 is not qualified");
#[cfg(all(feature="mii-ordinary-player-alpha",feature="source-ldn-capacity-probe"))]
compile_error!("ordinary Player wrapper does not emulate InlineCtx for source-ldn-capacity-probe");
#[cfg(feature = "mii-identity-probe")]
mod mii_identity_probe;
#[cfg(all(feature = "mii-pool-observer", any(feature = "mii-identical-reload-probe", feature = "item-state-probe", feature = "source-capacity-probe")))]
compile_error!("mii-pool-observer owns L3+X read-only; do not combine mutation/capacity probes");
#[cfg(feature = "mii-saved-file-probe")]
mod mii_saved_file_probe;
#[cfg(feature = "mii-fullconfig-probe")]
mod mii_fullconfig_probe;
#[cfg(all(feature = "mii-fullconfig-probe", feature = "mii-special-change-probe"))]
compile_error!("mii-fullconfig-probe and mii-special-change-probe share the two-shot reload lifecycle; enable only one");
#[cfg(all(feature = "mii-identical-reload-probe", feature = "item-state-probe"))]
compile_error!("mii-identical-reload-probe shares the L3+X chord with item-state-probe; enable only one");
#[cfg(feature = "item-state-probe")]
mod item_state_probe;
#[cfg(feature = "item-state-probe")]
mod turnip_face_caller_probe;
#[cfg(feature = "kirby-resource-probe")]
mod kirby_resource_probe;
#[cfg(feature = "practice-stage-observer")]
mod practice_stage_observer;
#[cfg(feature = "practice-stage-request-alpha")]
pub mod practice_stage_request;
#[cfg(feature = "lobby-stage-probe")]
mod lobby_stage_probe;
#[cfg(feature = "lobby-stage-probe")]
mod lobby_stage_cycle;
#[cfg(feature = "stage-asset-probe")]
mod stage_asset_probe;
mod hang_trace;
mod logging;
mod practice_trace;
#[cfg(feature = "console-practice-trace")]
mod practice_trace_core;
#[cfg(feature = "native-unlink-probe")]
mod native_unlink_core;
#[cfg(feature = "native-unlink-probe")]
mod native_unlink_probe;
#[cfg(feature = "native-unlink-guard")]
mod native_unlink_guard;
#[cfg(feature = "native-unlink-guard")]
mod native_gc_guard;
mod effect_hook_chain;
#[cfg(feature = "console-map-probe")]
mod console_map_probe;
#[cfg(feature = "console-map-probe")]
mod console_load_probe;
mod live_replay;
mod replay_capture;
mod replay_preview;
mod replay_store;
mod native_keyboard;
#[cfg(feature = "mii-preview-observer")]
mod mii_preview_observer;
#[cfg(feature = "mii-quit-observer")]
mod mii_quit_observer;
#[cfg(feature = "mii-editor-alpha")]
mod mii_editor_bridge;
#[cfg(all(feature = "mii-editor-alpha", any(feature = "mii-quit-observer", feature = "mii-preview-observer")))]
compile_error!("native editor bridge owns its hook boundaries; do not combine standalone observers");

fn nro_main(nro: &NroInfo<'_>) {
    let _probe = hang_trace::NRO_LOAD.enter(nro.name.len() as u64);
    if nro.module.isLoaded {
        return;
    }
    unsafe { turnip_identity::on_load(nro); }
    #[cfg(feature = "item-state-probe")]
    unsafe { turnip_face_caller_probe::on_load(nro); }
    if hang_trace::within_budget(&hang_trace::NRO_LINES, 200) {
        logging::diagnostic(format_args!("[HangTrace] nro_load name={}", nro.name));
    }

    if nro.name == "common" {
        skyline::install_hooks!(
            training::shield::handle_sub_guard_cont,
            training::directional_influence::handle_correct_damage_vector_common,
            training::tech::handle_change_status,
        );
    }
}

#[skyline::main(name = "training_modpack")]
pub fn main() {
    logging::init_diagnostics();
    practice_trace::init();
    #[cfg(feature = "native-unlink-probe")]
    native_unlink_probe::init();
    #[cfg(feature = "native-unlink-guard")]
    native_gc_guard::init();
    #[cfg(feature = "console-map-probe")]
    console_map_probe::init();
    hang_trace::start();
    #[cfg(feature = "automation-results-observer")]
    automation_helper::observer::init();
    #[cfg(feature = "automation-results-native")]
    automation_helper::native::init();
    character_roster::init();
    #[cfg(feature = "mii-preview-observer")]
    mii_preview_observer::init();
    #[cfg(feature = "mii-quit-observer")]
    mii_quit_observer::init();
    #[cfg(feature = "mii-editor-alpha")]
    mii_editor_bridge::init();
    #[cfg(feature = "mii-identity-probe")]
    mii_identity_probe::init();
    #[cfg(feature = "mii-pool-observer")]
    character_roster::mii_pool_observer::init();
    #[cfg(feature = "mii-saved-file-probe")]
    mii_saved_file_probe::init();
    // Before mii_reload_probe::init: its arming consults mii_fullconfig_probe::armed().
    #[cfg(feature = "mii-fullconfig-probe")]
    mii_fullconfig_probe::init();
    #[cfg(feature = "mii-identical-reload-probe")]
    character_roster::mii_reload_probe::init();
    #[cfg(feature = "kirby-resource-probe")]
    kirby_resource_probe::init();
    // Verify original resource instructions before the entry probe patches them.
    #[cfg(feature = "practice-stage-observer")]
    practice_stage_observer::init();
    #[cfg(feature = "lobby-stage-probe")]
    lobby_stage_probe::init();
    #[cfg(feature = "stage-asset-probe")]
    stage_asset_probe::init();
    initialize_training_modpack();
}

fn initialize_training_modpack() {
    std::panic::set_hook(Box::new(|info| {
        practice_trace::checkpoint(999, [0; 5]);
        let location = info.location().unwrap();

        let msg = match info.payload().downcast_ref::<&'static str>() {
            Some(s) => *s,
            None => match info.payload().downcast_ref::<String>() {
                Some(s) => &s[..],
                None => "Box<Any>",
            },
        };

        let game_version = SMASH_VERSION.to_string();
        let atmosphere_version = oe::get_display_version().to_owned();
        let version_report =
            format!("Smash version: {game_version}.\nAtmosphere version: {atmosphere_version}");

        let complete_error_message = format!("{msg}\n{location}\n{version_report}");
        logging::diagnostic(format_args!(
            "[PracticeDiagnostics] rust_panic {}",
            complete_error_message
        ));

        skyline::error::show_error(
            69,
            "SSBU Training Modpack has panicked! Please open the details and send a screenshot to the developer, then close the game.\n",
            &complete_error_message,
        );
    }));
    init_logger().expect("Could not initialize logger");

    info!("Initialized.");

    notification("Training Modpack".to_string(), "Welcome!".to_string(), 60);

    hitbox_visualizer::hitbox_visualization();
    hazard_manager::hazard_manager();
    training::training_mods();
    #[cfg(feature = "cpu-pilot-probe")]
    cpu_pilot::init();
    nro::add_hook(nro_main).expect("Did not find nro_hook plugin!");
    nro::add_unload_hook(turnip_identity::on_unload)
        .expect("Did not find nro_hook plugin!");
    #[cfg(feature = "item-state-probe")]
    nro::add_unload_hook(turnip_face_caller_probe::on_unload)
        .expect("Did not find nro_hook plugin!");

    fs::create_dir_all(TRAINING_MODPACK_ROOT)
        .expect("Could not create Training Modpack root folder!");

    // Migrate legacy if exists
    if fs::metadata(LEGACY_TRAINING_MODPACK_ROOT).is_ok() {
        for entry in
            fs::read_dir(LEGACY_TRAINING_MODPACK_ROOT).expect("Couldn't read legacy folder")
        {
            let entry = entry.expect("Couldn't read legacy file during migration");
            let src_path = &entry.path();
            let dest_path = &PathBuf::from(TRAINING_MODPACK_ROOT).join(entry.file_name());
            fs::rename(src_path, dest_path).unwrap_or_else(|e| {
                error!("Could not move file from {src_path:#?} to {dest_path:#?} with error {e}")
            });
        }
        fs::remove_dir_all(LEGACY_TRAINING_MODPACK_ROOT).unwrap_or_else(|e| {
            error!("Could not delete legacy Training Modpack folder with error {e}")
        });
    }

    // INI seed first, then the optional Deluxe row (it reads the seed and the
    // persistence verdict), then the native adapter, which installs its hooks
    // whenever the INI says on or a host registered the row.
    replay_store::init();
    #[cfg(feature = "deluxe-sdk")]
    capture_plugin::register_toggle();
    live_replay::init();

    info!("Performing saved data check...");
    let data_loader = std::thread::Builder::new()
        .stack_size(0x20000)
        .spawn(move || {
            menu::load_from_file();
        })
        .expect("Couldn't create data loader thread");
    let _result = data_loader.join();

    // Revision 33: no upstream version check and no analytics thread. The check contacted the
    // upstream GitHub releases on every console boot and could offer to install upstream over this
    // fork; the analytics loop posted device identifiers to the upstream author's database from a
    // permanent thread with the 2 MiB default stack. Both are gone for good.

    notification("Training Modpack".to_string(), "Welcome!".to_string(), 60);
    notification(
        "Open Menu".to_string(),
        if read(&MENU).menu_open_start_press == OnOff::ON {
            "Hold Start".to_string()
        } else {
            DEFAULT_OPEN_MENU_CONFIG.to_string()
        },
        120,
    );
    notification(
        "Save State".to_string(),
        read(&MENU).save_state_save.to_string(),
        120,
    );
    notification(
        "Load State".to_string(),
        read(&MENU).save_state_load.to_string(),
        120,
    );
    notification(
        "Input Record".to_string(),
        read(&MENU).input_record.to_string(),
        120,
    );
    notification(
        "Input Playback".to_string(),
        read(&MENU).input_playback.to_string(),
        120,
    );
}
