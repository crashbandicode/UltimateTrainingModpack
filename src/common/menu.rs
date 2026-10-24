use std::fs;

use skyline::nn::hid::GetNpadStyleSet;

use crate::common::{button_config, ButtonConfig};
use crate::common::{DEFAULTS_MENU, MENU};
use crate::filesystem;
use crate::input::{ButtonBitfield, ControllerStyle, MappedInputs, SomeControllerStruct};
use crate::logging::*;
use crate::training::frame_counter;
use crate::training::ui::menu::VANILLA_MENU_ACTIVE;

use training_mod_consts::{create_app, InputControl, MenuJsonStruct, MENU_OPTIONS_PATH};
use training_mod_sync::*;
use training_mod_tui::macro_panel::Pad;
use training_mod_tui::AppPage;

pub const MENU_CLOSE_WAIT_FRAMES: u32 = 15;
// Persistence only: no catalog scan, native install, or Mii user control is enabled here.
static MII_PREFERENCES: LazyLock<RwLock<training_mod_consts::mii::selection::Preferences>> =
    LazyLock::new(|| RwLock::new(Default::default()));

static STAGE_PREFERENCES: LazyLock<RwLock<training_mod_tui::stage_menu::Preferences>> =
    LazyLock::new(|| RwLock::new(Default::default()));
#[cfg(feature = "practice-stage-request-alpha")]
static STAGE_CONTEXT: RwLock<Option<crate::practice_stage_request::MenuContext>> = RwLock::new(None);

#[cfg(feature = "practice-stage-request-alpha")]
pub fn stage_preferences() -> training_mod_tui::stage_menu::Preferences { read(&STAGE_PREFERENCES) }

pub fn mii_preferences() -> training_mod_consts::mii::selection::Preferences {
    read_clone(&MII_PREFERENCES)
}
pub static QUICK_MENU_ACTIVE: RwLock<bool> = RwLock::new(false);

pub fn captures_input() -> bool {
    let frames = frame_counter::get_frame_count(*MENU_CLOSE_FRAME_COUNTER);
    read(&QUICK_MENU_ACTIVE)
        || crate::training::named_macros::panel_visible()
        || (frame_counter::_is_counting(*MENU_CLOSE_FRAME_COUNTER)
            && frames < MENU_CLOSE_WAIT_FRAMES)
}

pub fn clear_session_state() {
    #[cfg(feature = "practice-stage-request-alpha")]
    assign(&STAGE_CONTEXT, None);
    assign(&QUICK_MENU_ACTIVE, false);
    assign(&VANILLA_MENU_ACTIVE, false);
    assign(&MENU_RECEIVED_INPUT, true);
    lock_write(&DIRECTION_HOLD_FRAMES).fill(0);
    frame_counter::full_reset(*MENU_CLOSE_FRAME_COUNTER);
}

pub unsafe fn menu_condition() -> bool {
    button_config::combo_passes(button_config::ButtonCombo::OpenMenu)
}

pub fn load_from_file() {
    // Note that this function requires a larger stack size
    // With the switch default, it'll crash w/o a helpful error message
    info!("Checking for previous menu in {MENU_OPTIONS_PATH}...");
    if fs::metadata(MENU_OPTIONS_PATH).is_ok() {
        match filesystem::read_json::<MenuJsonStruct>(MENU_OPTIONS_PATH) {
            Ok(menu_conf_json) => {
                assign(&MENU, menu_conf_json.menu);
                assign(&DEFAULTS_MENU, menu_conf_json.defaults_menu);
                assign(&MII_PREFERENCES, menu_conf_json.mii_preferences);
                assign(&STAGE_PREFERENCES, menu_conf_json.stage_preferences);
                info!("Previous menu found. Loading...");
            }
            Err(e) => {
                warn!("Previous menu found at {MENU_OPTIONS_PATH} but could not be loaded: {e}");
                info!("Deleting previous menu file so it can be recreated");
                fs::remove_file(MENU_OPTIONS_PATH).expect("Could not remove invalid menu file!");
            }
        }
    } else {
        info!("No previous menu file found.");
    }
    info!("Setting initial menu selections...");
    let mut app = lock_write(&QUICK_MENU_APP);
    app.mii_preferences = mii_preferences();
    app.open_stage_menu(cfg!(feature = "practice-stage-request-alpha"), read(&STAGE_PREFERENCES));
    app.serialized_default_settings =
        serde_json::to_string(&read(&DEFAULTS_MENU)).expect("Could not serialize DEFAULTS_MENU");
    app.update_all_from_json(
        &serde_json::to_string(&read(&MENU)).expect("Could not serialize MENU"),
    );
    // Driver init precedes saved-data loading. Atomics only, even with App held.
    #[cfg(feature = "cpu-pilot-probe")]
    {
        let saved = read(&MENU);
        crate::cpu_pilot::menu_settings_changed(
            saved.p1_cpu_control == training_mod_consts::OnOff::ON,
            saved.p1_cpu_level.as_level(),
        );
    }
}

pub fn set_menu_from_json(message: &str, manual: Option<(training_mod_tui::roster_grid::Role, u32)>) {
    let response = serde_json::from_str::<MenuJsonStruct>(message);
    info!("Received menu message: {message}");
    if let Ok(message_json) = response {
        let previous = read(&MENU);
        // Includes both MENU and DEFAULTS_MENU
        assign(&MENU, message_json.menu);
        assign(&DEFAULTS_MENU, message_json.defaults_menu);
        assign(&MII_PREFERENCES, message_json.mii_preferences.clone());
        assign(&STAGE_PREFERENCES, message_json.stage_preferences);
        filesystem::write_json(MENU_OPTIONS_PATH, &message_json);
        crate::training::lobby::settings_changed();
        crate::training::mash_settings_changed(&previous, &message_json.menu);
        crate::character_roster::settings_changed(&previous, manual);
        #[cfg(feature = "cpu-pilot-probe")]
        crate::cpu_pilot::menu_settings_changed(
            message_json.menu.p1_cpu_control == training_mod_consts::OnOff::ON,
            message_json.menu.p1_cpu_level.as_level(),
        );
    } else {
        skyline::error::show_error(
            0x70,
            "Could not parse the menu response!\nPlease send a screenshot of the details page to the developers.\n\0",
            &format!("{message:#?}\0"),
        );
    };
}

pub fn spawn_menu() {
    crate::practice_trace::checkpoint(10, [0; 5]);
    #[cfg(feature="mii-saved-alpha")]
    let mii_opening = crate::character_roster::mii_saved_alpha::menu_open();
    crate::practice_trace::event(11, [0; 5]);
    let mut app = lock_write(&QUICK_MENU_APP);
    crate::practice_trace::event(12, [0; 5]);
    app.mii_preferences = mii_preferences();
    app.open_stage_menu_in_context(
        cfg!(feature = "practice-stage-request-alpha"),
        read(&STAGE_PREFERENCES),
        unsafe { crate::common::is_native_training_mode() },
    );
    // A scene change can cancel an open editor before its normal commit path.
    app.open_settings_for_player(
        &serde_json::to_string(&read(&MENU)).expect("Serialize MENU"),
        crate::character_roster::original_player_kind(),
    );
    crate::practice_trace::checkpoint(13, [0; 5]);
    app.set_original_cpu_kind(crate::character_roster::original_cpu_kind());
    #[cfg(feature = "practice-stage-request-alpha")]
    assign(&STAGE_CONTEXT, crate::practice_stage_request::menu_context());
    app.serialized_default_settings =
        serde_json::to_string(&read(&DEFAULTS_MENU)).expect("Serialize DEFAULTS_MENU");
    app.page = AppPage::SUBMENU;
    assign(&QUICK_MENU_ACTIVE, true);
    assign(&MENU_RECEIVED_INPUT, true);
    drop(app);
    crate::practice_trace::checkpoint(14, [0; 5]);
    // Capture can publish only after reset; catalog IO never holds the App guard.
    #[cfg(feature="mii-saved-alpha")]
    crate::character_roster::mii_saved_alpha::menu_ready(mii_opening);
    crate::practice_trace::checkpoint(15, [0; 5]);
    #[cfg(feature="mii-editor-alpha")]
    crate::mii_editor_bridge::menu_ready(mii_opening);
    crate::practice_trace::checkpoint(16, [0; 5]);
}

#[derive(Eq, PartialEq, Copy, Clone)]
#[repr(usize)]
enum DirectionButton {
    LLeft,
    RLeft,
    LDown,
    RDown,
    LRight,
    RRight,
    LUp,
    RUp,
}

pub static QUICK_MENU_APP: LazyLock<RwLock<training_mod_tui::App<'static>>> = LazyLock::new(|| {
    RwLock::new({
        info!("Initialized lazy_static: QUICK_MENU_APP");
        unsafe { create_app() }
    })
});
pub static P1_CONTROLLER_STYLE: LazyLock<RwLock<ControllerStyle>> =
    LazyLock::new(|| RwLock::new(ControllerStyle::default()));
static DIRECTION_HOLD_FRAMES: RwLock<[u32; 8]> = RwLock::new([0; 8]);
pub static MENU_RECEIVED_INPUT: RwLock<bool> = RwLock::new(true);

pub static MENU_CLOSE_FRAME_COUNTER: LazyLock<usize> =
    LazyLock::new(|| frame_counter::register_counter(frame_counter::FrameCounterType::Real));

pub unsafe fn handle_final_input_mapping(
    player_idx: i32,
    controller_struct: &mut SomeControllerStruct,
    out: *mut MappedInputs,
) {
    if player_idx == 0 {
        let p1_controller = &mut *controller_struct.controller;
        assign(&P1_CONTROLLER_STYLE, p1_controller.style);
        if crate::replay_capture::loading() {
            *out = MappedInputs::empty();
            if p1_controller.just_down.b() {
                crate::replay_capture::cancel_reload();
            }
            return;
        }
        if crate::training::named_macros::panel_visible() {
            *out = MappedInputs::empty();
            let closing = frame_counter::get_frame_count(*MENU_CLOSE_FRAME_COUNTER);
            if frame_counter::_is_counting(*MENU_CLOSE_FRAME_COUNTER)
                && closing < MENU_CLOSE_WAIT_FRAMES
            {
                return;
            }
            if closing >= MENU_CLOSE_WAIT_FRAMES {
                frame_counter::full_reset(*MENU_CLOSE_FRAME_COUNTER);
            }
            if crate::training::named_macros::clip_foreground() {
                crate::replay_capture::on_input(p1_controller);
                return;
            }
            let style = p1_controller.style;
            let presses = p1_controller.just_down;
            let pad = if button_config::button_mapping(ButtonConfig::A, style, presses) {
                Some(Pad::Confirm)
            } else if button_config::button_mapping(ButtonConfig::B, style, presses) {
                Some(Pad::Back)
            } else if button_config::button_mapping(ButtonConfig::Y, style, presses) {
                Some(Pad::Extra)
            } else if presses.dpad_left() || presses.l_left() || presses.r_left() {
                Some(Pad::Left)
            } else if presses.dpad_right() || presses.l_right() || presses.r_right() {
                Some(Pad::Right)
            } else if presses.dpad_up() || presses.l_up() || presses.r_up() {
                Some(Pad::Up)
            } else if presses.dpad_down() || presses.l_down() || presses.r_down() {
                Some(Pad::Down)
            } else {
                None
            };
            if let Some(pad) = pad {
                crate::training::named_macros::on_pad(pad);
            }
            return;
        }
        let visual_frame_count = frame_counter::get_frame_count(*MENU_CLOSE_FRAME_COUNTER);
        if frame_counter::_is_counting(*MENU_CLOSE_FRAME_COUNTER)
            && visual_frame_count < MENU_CLOSE_WAIT_FRAMES
        {
            // If we just closed the menu, kill all inputs to avoid accidental presses
            // even before the first draw tick advances the active counter from zero.
            *out = MappedInputs::empty();
            p1_controller.current_buttons = ButtonBitfield::default();
            p1_controller.previous_buttons = ButtonBitfield::default();
            p1_controller.just_down = ButtonBitfield::default();
            p1_controller.just_release = ButtonBitfield::default();
        } else if visual_frame_count >= MENU_CLOSE_WAIT_FRAMES {
            frame_counter::stop_counting(*MENU_CLOSE_FRAME_COUNTER);
            frame_counter::reset_frame_count(*MENU_CLOSE_FRAME_COUNTER);
        }

        if read(&QUICK_MENU_ACTIVE) {
            // If we're here, remove all other presses
            *out = MappedInputs::empty();

            let mut received_input = false;

            const DIRECTION_HOLD_REPEAT_FRAMES: u32 = 20;
            use DirectionButton::*;
            let mut direction_hold_frames = read(&DIRECTION_HOLD_FRAMES);

            // Nine fixed native controller ids; no per-poll allocation.
            const POTENTIAL_CONTROLLER_IDS: [u32; 9] = [0, 1, 2, 3, 4, 5, 6, 7, 0x20];
            if POTENTIAL_CONTROLLER_IDS
                .iter()
                .all(|i| GetNpadStyleSet(i as *const _).flags == 0)
            {
                assign(&QUICK_MENU_ACTIVE, false);
                return;
            }

            let style = p1_controller.style;
            let button_presses = p1_controller.just_down;

            let button_current_held = p1_controller.current_buttons;
            [LLeft, RLeft, LDown, RDown, LRight, RRight, LUp, RUp]
                .iter()
                .zip(direction_hold_frames.iter_mut())
                .for_each(|(direction, frames)| {
                    let still_held = match direction {
                        LLeft => button_current_held.l_left(),
                        RLeft => button_current_held.r_left(),
                        LDown => button_current_held.l_down(),
                        RDown => button_current_held.r_down(),
                        LRight => button_current_held.l_right(),
                        RRight => button_current_held.r_right(),
                        LUp => button_current_held.l_up(),
                        RUp => button_current_held.r_up(),
                    };
                    if still_held {
                        *frames += 1;
                    } else {
                        *frames = 0;
                    }
                });

            let mut app = lock_write(&QUICK_MENU_APP);
            button_config::button_mapping(ButtonConfig::A, style, button_presses).then(|| {
                if app.page == AppPage::SUBMENU {
                    match app.selected_submenu().id {
                        "named_macro_library" => {
                            received_input = true;
                            crate::training::named_macros::open_library();
                            return;
                        }
                        "named_macro_placement" => {
                            received_input = true;
                            crate::training::named_macros::open_editor();
                            return;
                        }
                        "named_macro_replay" => {
                            received_input = true;
                            crate::replay_capture::open_editor();
                            return;
                        }
                        _ => {}
                    }
                }
                if app.page == AppPage::TOGGLE
                    && app.selected_submenu().id == "character_item_grant"
                {
                    let title = app.selected_submenu().selected_toggle().title;
                    if let Some(index) = training_mod_consts::character_items::ITEM_NAMES
                        .iter().position(|name| *name == title)
                    {
                        crate::character_items::request(index);
                        crate::training::ui::menu::flash_character_item(index);
                    }
                }
                app.on_a();
                received_input = true;
            });
            if app.page != AppPage::CLOSE {
                button_config::button_mapping(ButtonConfig::B, style, button_presses).then(|| {
                    received_input = true;
                    app.on_b();
                });
                button_config::button_mapping(ButtonConfig::X, style, button_presses).then(|| {
                    app.on_x();
                    received_input = true;
                });
                button_config::button_mapping(ButtonConfig::Y, style, button_presses).then(|| {
                    app.on_y();
                    received_input = true;
                });

                button_config::button_mapping(ButtonConfig::ZL, style, button_presses).then(|| {
                    app.on_zl();
                    received_input = true;
                });
                button_config::button_mapping(ButtonConfig::ZR, style, button_presses).then(|| {
                    app.on_zr();
                    received_input = true;
                });
                button_config::button_mapping(ButtonConfig::R, style, button_presses).then(|| {
                    app.on_r();
                    received_input = true;
                });

                let hold_condition = |direction_button: &DirectionButton| {
                    direction_hold_frames[*direction_button as usize] > DIRECTION_HOLD_REPEAT_FRAMES
                };
                (button_presses.dpad_left()
                    || button_presses.l_left()
                    || button_presses.r_left()
                    || [LLeft, RLeft].iter().any(hold_condition))
                .then(|| {
                    received_input = true;
                    app.on_left();
                });
                (button_presses.dpad_right()
                    || button_presses.l_right()
                    || button_presses.r_right()
                    || [LRight, RRight].iter().any(hold_condition))
                .then(|| {
                    received_input = true;
                    app.on_right();
                });
                (button_presses.dpad_up()
                    || button_presses.l_up()
                    || button_presses.r_up()
                    || [LUp, RUp].iter().any(hold_condition))
                .then(|| {
                    received_input = true;
                    app.on_up();
                });
                (button_presses.dpad_down()
                    || button_presses.l_down()
                    || button_presses.r_down()
                    || [LDown, RDown].iter().any(hold_condition))
                .then(|| {
                    received_input = true;
                    app.on_down();
                });

            }
            let closing = app.page == AppPage::CLOSE;
            let stage_close = app.take_pending_stage();
            let manual_close = app.take_pending_manual_selection();
            if closing {
                frame_counter::start_counting(*MENU_CLOSE_FRAME_COUNTER);
                assign(&QUICK_MENU_ACTIVE, false);
                set_menu_from_json(&app.get_serialized_settings_with_defaults(), manual_close);
            }
            #[cfg(feature="mii-saved-alpha")]
            let mii_close = (app.page==AppPage::CLOSE).then(||app.take_pending_mii_selection(
                crate::character_roster::mii_saved_alpha::menu_epoch()));
            #[cfg(feature = "mii-editor-alpha")]
            let editor_seed = closing.then(|| app.take_pending_mii_editor()).flatten();
            drop(app);
            #[cfg(feature = "mii-editor-alpha")]
            if let Some(seed) = editor_seed { crate::mii_editor_bridge::request(seed); }
            #[cfg(feature="mii-saved-alpha")]
            if let Some(pending)=mii_close {crate::character_roster::mii_saved_alpha::menu_close(pending);}
            #[cfg(feature = "practice-stage-request-alpha")]
            if closing {
                let context = lock_write(&STAGE_CONTEXT).take();
                if let Some(stage) = stage_close {
                    let result = context.ok_or("current stage context unavailable")
                        .and_then(|context| crate::practice_stage_request::request_in(context, stage));
                    if let Err(why) = result {
                        crate::training::ui::notifications::clear_notification("Practice Stage");
                        crate::training::ui::notifications::notification("Practice Stage".into(),
                            format!("Stage unchanged: {why}"), 180);
                    }
                }
            }
            #[cfg(not(feature = "practice-stage-request-alpha"))]
            let _ = stage_close;
            if received_input {
                direction_hold_frames.fill(0);
                assign(&MENU_RECEIVED_INPUT, true);
            }
        }
    }
}
