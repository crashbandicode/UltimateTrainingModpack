use std::collections::HashMap;

use crate::common::menu::QUICK_MENU_ACTIVE;
use crate::common::ButtonConfig;
use crate::input::{ControllerStyle::*, *};
use crate::training::ui::menu::VANILLA_MENU_ACTIVE;

use training_mod_consts::{OnOff, MENU};
use training_mod_sync::*;

use strum_macros::EnumIter;

use training_mod_consts::menu_shortcuts::{self, MenuShortcuts, BUTTONS, DEFAULT_SHORTCUTS};

pub fn menu_shortcut_description() -> String { read(&MENU_SHORTCUTS).description() }

static MENU_SHORTCUTS: RwLock<MenuShortcuts> = RwLock::new(DEFAULT_SHORTCUTS);
static SHORTCUTS_INIT: std::sync::Once = std::sync::Once::new();
const BUTTON_CONFIG_PATH: &str = "sd:/ultimate/TrainingModpack/button_config.ini";

/// Startup-only: input hooks only copy the fixed-size cached configuration.
pub fn init() {
    SHORTCUTS_INIT.call_once(|| {
        use std::io::Read;

        let contents = (|| -> std::io::Result<String> {
            // Pinned Skyline open() loses NotFound; metadata() preserves it.
            // Use the same existence check as the Mii library, then bound the read.
            match std::fs::metadata(BUTTON_CONFIG_PATH) {
                Ok(_) => {
                    let file = std::fs::File::open(BUTTON_CONFIG_PATH)?;
                    let mut contents = String::new();
                    file.take(8193).read_to_string(&mut contents)?;
                    if contents.len() > 8192 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "button_config.ini exceeds 8 KiB",
                        ));
                    }
                    Ok(contents)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::create_dir_all(training_mod_consts::TRAINING_MODPACK_ROOT)?;
                    use std::io::Write;
                    // Never truncate a file created by another initializer.
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create(true)
                        .create_new(true)
                        .truncate(false)
                        .open(BUTTON_CONFIG_PATH)?
                        .write_all(menu_shortcuts::DEFAULT_INI.as_bytes())?;
                    Ok(menu_shortcuts::DEFAULT_INI.to_owned())
                }
                Err(error) => Err(error),
            }
        })();
        match contents {
            Ok(contents) => {
                let shortcuts = MenuShortcuts::parse(&contents);
                assign(&MENU_SHORTCUTS, shortcuts);
                crate::logging::diagnostic(format_args!("[MenuShortcuts] loaded {}", shortcuts.description()));
            }
            Err(error) => {
                log::warn!("Could not load {BUTTON_CONFIG_PATH}; using default menu shortcuts: {error}");
                crate::logging::diagnostic(format_args!("[MenuShortcuts] defaults: {error}"));
            }
        }
        LazyLock::force(&BUTTON_COMBO_REQUESTS);
    });
}

pub fn button_mapping(
    button_config: ButtonConfig,
    style: ControllerStyle,
    b: ButtonBitfield,
) -> bool {
    match button_config {
        ButtonConfig::A => b.a(),
        ButtonConfig::B => b.b(),
        ButtonConfig::X => b.x(),
        ButtonConfig::Y => b.y(),
        ButtonConfig::L => match style {
            GCController => false,
            _ => b.l(),
        },
        ButtonConfig::R => match style {
            GCController => b.zr(),
            _ => b.r(),
        },
        ButtonConfig::ZL => match style {
            GCController => b.l() || b.real_digital_l(),
            _ => b.zl() || b.left_sl() || b.right_sl(),
        },
        ButtonConfig::ZR => match style {
            GCController => b.r() || b.real_digital_r(),
            _ => b.zr() || b.left_sr() || b.right_sr(),
        },
        ButtonConfig::DPAD_UP => b.dpad_up(),
        ButtonConfig::DPAD_DOWN => b.dpad_down(),
        ButtonConfig::DPAD_LEFT => b.dpad_left(),
        ButtonConfig::DPAD_RIGHT => b.dpad_right(),
        ButtonConfig::PLUS => b.plus(),
        ButtonConfig::MINUS => b.minus(),
        ButtonConfig::LSTICK => b.stick_l(),
        ButtonConfig::RSTICK => b.stick_r(),
        _ => panic!("Invalid value in button_mapping: {}", button_config),
    }
}

pub fn name_to_font_glyph(button: ButtonConfig, style: ControllerStyle) -> Option<u16> {
    let is_gcc = style == ControllerStyle::GCController;
    Some(match button {
        ButtonConfig::A => 0xE0E0,
        // TODO: Find one that works in training...
        ButtonConfig::B => 0xE0E0,
        ButtonConfig::X => {
            if is_gcc {
                0xE206
            } else {
                0xE0E2
            }
        }
        ButtonConfig::Y => {
            if is_gcc {
                0xE207
            } else {
                0xE0E3
            }
        }
        ButtonConfig::L => {
            if is_gcc {
                return None;
            } else {
                0xE0E4
            }
        }
        ButtonConfig::R => {
            if is_gcc {
                0xE205
            } else {
                0xE0E5
            }
        }
        ButtonConfig::ZL => {
            if is_gcc {
                0xE204
            } else {
                0xE0E6
            }
        }
        ButtonConfig::ZR => {
            if is_gcc {
                0xE208
            } else {
                0xE0E7
            }
        }
        ButtonConfig::DPAD_UP => {
            if is_gcc {
                0xE209
            } else {
                0xE079
            }
        }
        ButtonConfig::DPAD_DOWN => {
            if is_gcc {
                0xE20A
            } else {
                0xE07A
            }
        }
        ButtonConfig::DPAD_LEFT => {
            if is_gcc {
                0xE20B
            } else {
                0xE07B
            }
        }
        ButtonConfig::DPAD_RIGHT => {
            if is_gcc {
                0xE20C
            } else {
                0xE07C
            }
        }
        ButtonConfig::PLUS => {
            if is_gcc {
                0xE20D
            } else {
                0xE0EF
            }
        }
        ButtonConfig::MINUS => {
            if is_gcc {
                return None;
            } else {
                0xE0F0
            }
        }
        ButtonConfig::LSTICK => {
            if is_gcc {
                return None;
            } else {
                0xE104
            }
        }
        ButtonConfig::RSTICK => {
            if is_gcc {
                return None;
            } else {
                0xE105
            }
        }
        _ => return None,
    })
}

#[derive(Debug, EnumIter, PartialEq, Eq, Hash, Copy, Clone)]
pub enum ButtonCombo {
    OpenMenu,
    SaveState,
    LoadState,
    InputRecord,
    InputPlayback,
    CpuCycle,
    PlayerCycle,
}

pub const DEFAULT_OPEN_MENU_CONFIG: ButtonConfig = menu_shortcuts::DEFAULT_OPEN_MENU_CONFIG;

unsafe fn get_combo_keys(combo: ButtonCombo) -> ButtonConfig {
    match combo {
        // Permanent fallback; configurable alternatives are dispatched separately.
        ButtonCombo::OpenMenu => DEFAULT_OPEN_MENU_CONFIG,
        ButtonCombo::SaveState => read(&MENU).save_state_save,
        ButtonCombo::LoadState => read(&MENU).save_state_load,
        ButtonCombo::InputRecord => read(&MENU).input_record,
        ButtonCombo::InputPlayback => read(&MENU).input_playback,
        ButtonCombo::CpuCycle => read(&MENU).cpu_cycle,
        ButtonCombo::PlayerCycle => read(&MENU).player_cycle,
    }
}

// Note: in addition to RwLock we also need a LazyLock initializer because HashMap::from() is not const
static BUTTON_COMBO_REQUESTS: LazyLock<RwLock<HashMap<ButtonCombo, bool>>> = LazyLock::new(|| {
    RwLock::new(HashMap::from([
        (ButtonCombo::OpenMenu, false),
        (ButtonCombo::SaveState, false),
        (ButtonCombo::LoadState, false),
        (ButtonCombo::InputRecord, false),
        (ButtonCombo::InputPlayback, false),
        (ButtonCombo::CpuCycle, false),
        (ButtonCombo::PlayerCycle, false),
    ]))
});

pub fn clear_session_state() {
    for request in lock_write(&BUTTON_COMBO_REQUESTS).values_mut() {
        *request = false;
    }
}

/// Observe ownership without consuming a request needed by its normal handler.
pub fn has_pending_request() -> bool {
    let native_training = unsafe { crate::common::is_native_training_mode() };
    let practice = crate::training::practice::enabled();
    lock_read(&BUTTON_COMBO_REQUESTS)
        .iter()
        .any(|(combo, requested)| {
            *requested
                && match combo {
                    ButtonCombo::OpenMenu => true,
                    ButtonCombo::SaveState | ButtonCombo::LoadState => {
                        practice && read(&MENU).save_state_enable == OnOff::ON
                    }
                    ButtonCombo::InputRecord => {
                        practice
                            && read(&MENU)
                                .record_trigger
                                .contains(&training_mod_consts::RecordTrigger::COMMAND)
                    }
                    ButtonCombo::InputPlayback => practice,
                    ButtonCombo::CpuCycle | ButtonCombo::PlayerCycle => native_training,
                }
        })
}

fn _combo_passes(p1_controller: Controller, combo: ButtonCombo) -> bool {
    unsafe {
        // Prevent button combos from passing if either the vanilla or mod menu is open
        if read(&VANILLA_MENU_ACTIVE)
            || read(&QUICK_MENU_ACTIVE)
            || crate::training::named_macros::panel_visible()
            || crate::replay_capture::loading()
        {
            return false;
        }

        if combo == ButtonCombo::OpenMenu {
            return read(&MENU_SHORTCUTS).passes(
                |button| button_mapping(button, p1_controller.style, p1_controller.current_buttons),
                |button| button_mapping(button, p1_controller.style, p1_controller.just_down),
            );
        }

        legacy_combo_passes(p1_controller, combo, get_combo_keys(combo))
    }
}

// Preserve the existing save/load/record/playback chord semantics while avoiding
// the old per-combo Vec allocation. Menu shortcuts have their own edge semantics.
fn legacy_combo_passes(
    p1_controller: Controller,
    combo: ButtonCombo,
    combo_keys: ButtonConfig,
) -> bool {
    // Even a one-button character mapping is an edge, never repeat while held.
    if matches!(combo, ButtonCombo::CpuCycle | ButtonCombo::PlayerCycle)
        && !BUTTONS
            .iter()
            .filter(|button| combo_keys.contains(button))
            .any(|button| button_mapping(*button, p1_controller.style, p1_controller.just_down))
    {
        return false;
    }
    let mut this_combo_passes = false;

    for hold_button in BUTTONS.iter().filter(|button| combo_keys.contains(button)) {
        if button_mapping(
            *hold_button,
            p1_controller.style,
            p1_controller.current_buttons,
        ) && BUTTONS
            .iter()
            .filter(|button| combo_keys.contains(button))
            .filter(|press_button| press_button != &hold_button)
            .all(|press_button| {
                button_mapping(*press_button, p1_controller.style, p1_controller.just_down)
            })
        {
            this_combo_passes = true;
        }
    }

    this_combo_passes
}

pub fn combo_passes(combo: ButtonCombo) -> bool {
    let mut button_combo_requests_lock = lock_write(&BUTTON_COMBO_REQUESTS);
    let passes = (*button_combo_requests_lock).get_mut(&combo);
    let mut did_pass = false;
    if let Some(passes) = passes {
        if *passes {
            did_pass = true;
        }
        *passes = false;
    }
    did_pass
}

/// Consume only the CPU cycle chord; other practice requests retain ownership.
pub fn consume_cpu_cycle_request() -> bool {
    combo_passes(ButtonCombo::CpuCycle)
}

/// Consume only the player cycle chord; other practice requests retain ownership.
pub fn consume_player_cycle_request() -> bool {
    combo_passes(ButtonCombo::PlayerCycle)
}

fn suppress_pause(controller: &mut Controller, owns_input: bool) {
    if owns_input {
        controller.previous_buttons.set_plus(false);
        controller.current_buttons.set_plus(false);
        controller.just_down.set_plus(false);
        controller.just_release.set_plus(false);
    }
}

pub fn handle_final_input_mapping(player_idx: i32, controller_struct: &mut SomeControllerStruct) {
    if player_idx != 0 {
        return;
    }
    let p1_controller = &mut *controller_struct.controller;
    // Plus passes straight through to the game's normal pause handler, except
    // while a modal feature owns input. There is no implicit long-Plus opener.
    suppress_pause(
        p1_controller,
        crate::training::named_macros::panel_visible() || crate::replay_capture::loading(),
    );

    let mut button_combo_requests_lock = lock_write(&BUTTON_COMBO_REQUESTS);
    (*button_combo_requests_lock)
        .iter_mut()
        .for_each(|(combo, is_request)| {
            if !*is_request {
                *is_request = _combo_passes(*p1_controller, *combo);
            }
        });
    // An explicitly configured Plus chord owns its pause press as well. Evaluate
    // all practice shortcuts first, against the same unmodified controller data.
    suppress_pause(
        p1_controller,
        button_combo_requests_lock.get(&ButtonCombo::OpenMenu).copied().unwrap_or(false),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_plus_press_hold_release_are_passed_through() {
        let plus = ButtonBitfield::new().with_plus(true);
        for (previous, current, pressed, released) in [
            (ButtonBitfield::new(), plus, plus, ButtonBitfield::new()),
            (plus, plus, ButtonBitfield::new(), ButtonBitfield::new()),
            (plus, ButtonBitfield::new(), ButtonBitfield::new(), plus),
        ] {
            let mut controller = Controller {
                previous_buttons: previous,
                current_buttons: current,
                just_down: pressed,
                just_release: released,
                ..Default::default()
            };
            suppress_pause(&mut controller, false);
            assert_eq!(controller.previous_buttons, previous);
            assert_eq!(controller.current_buttons, current);
            assert_eq!(controller.just_down, pressed);
            assert_eq!(controller.just_release, released);
        }
    }

    #[test]
    fn menu_or_modal_ownership_suppresses_plus_without_erasing_other_buttons() {
        let buttons = ButtonBitfield::new().with_plus(true).with_x(true);
        let mut controller = Controller {
            previous_buttons: buttons,
            current_buttons: buttons,
            just_down: buttons,
            just_release: buttons,
            ..Default::default()
        };
        suppress_pause(&mut controller, true);
        for buttons in [controller.previous_buttons, controller.current_buttons,
            controller.just_down, controller.just_release] {
            assert!(!buttons.plus());
            assert!(buttons.x());
        }
    }

    #[test]
    fn practice_shortcuts_keep_hold_modifier_and_press_semantics() {
        let keys = ButtonConfig::ZL.union(ButtonConfig::X);
        let mut controller = Controller {
            current_buttons: ButtonBitfield::new().with_zl(true).with_x(true),
            just_down: ButtonBitfield::new().with_x(true),
            style: ProController,
            ..Default::default()
        };
        for combo in [ButtonCombo::SaveState, ButtonCombo::LoadState,
            ButtonCombo::InputRecord, ButtonCombo::InputPlayback] {
            assert!(legacy_combo_passes(controller, combo, keys));
            controller.just_down.set_x(false);
            assert!(!legacy_combo_passes(controller, combo, keys));
            controller.just_down.set_x(true);
            assert!(!legacy_combo_passes(controller, combo, ButtonConfig::empty()));
        }
        // Existing one-button playback remains level-triggered; character cycle
        // shortcuts retain their stricter press-edge rule.
        controller.just_down = ButtonBitfield::new();
        assert!(legacy_combo_passes(controller, ButtonCombo::InputPlayback, ButtonConfig::X));
        assert!(!legacy_combo_passes(controller, ButtonCombo::CpuCycle, ButtonConfig::X));
        assert!(!legacy_combo_passes(controller, ButtonCombo::PlayerCycle, ButtonConfig::X));
    }
}
