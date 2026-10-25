use log::info;
use smash::app;

use crate::common::*;
use crate::training::combo;
use crate::training::frame_counter;
use crate::training::ledge;
use crate::training::mash;
use crate::training::sdi;
use crate::training::shield_tilt;
use crate::training::throw;
use std::sync::atomic::{AtomicBool, Ordering};
use training_mod_sync::read;
use training_mod_consts::OnOff;

static RESET_HELD: AtomicBool = AtomicBool::new(false);

pub fn clear_session_state() { RESET_HELD.store(false, Ordering::Relaxed); }

pub fn waiting_shortcut(controller: &crate::common::input::Controller) -> bool {
    let buttons = controller.current_buttons;
    let chord = (buttons.l() || buttons.real_digital_l())
        && (buttons.r() || buttons.real_digital_r()) && buttons.a();
    let previous = RESET_HELD.swap(chord, Ordering::Relaxed);
    if !super::lobby::active() || !super::practice::enabled()
        || read(&MENU).lra_reset != OnOff::ON || menu::captures_input()
        || super::named_macros::panel_visible() || crate::replay_capture::loading()
        || crate::replay_preview::occupied() || !chord { return false; }
    if !previous && !unsafe { super::save_states::request_quick_reset() } {
        super::ui::notifications::notification("Quick Reset".into(),
            "Wait for both practice fighters to finish loading".into(), 120);
    }
    true
}

pub fn check_reset(module_accessor: &mut app::BattleObjectModuleAccessor) {
    if !is_operation_cpu(module_accessor) {
        return;
    }

    info!("Resetting Training Mode...");

    on_reset();
}

pub fn on_reset() {
    mash::full_reset();
    sdi::roll_direction();
    frame_counter::reset_all();
    combo::reset();
    ledge::reset_ledge_delay();
    throw::reset_throw_delay();
    shield_tilt::roll_direction();
}
