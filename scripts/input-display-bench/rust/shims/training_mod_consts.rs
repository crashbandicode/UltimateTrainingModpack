//! Host shim for the menu fields the input-log renderer reads.
//! `InputDisplay` / `OnOff` are fixture enums, not the product byteflags.

use training_mod_sync::RwLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputDisplay {
    NONE,
    SMASH,
    RAW,
    STATUS,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OnOff {
    OFF,
    ON,
}

impl OnOff {
    pub fn as_bool(self) -> bool {
        matches!(self, OnOff::ON)
    }
}

#[derive(Clone, Copy)]
pub struct TrainingModpackMenu {
    pub input_display: InputDisplay,
    pub input_display_status: OnOff,
}

pub static MENU: RwLock<TrainingModpackMenu> = RwLock::new(TrainingModpackMenu {
    input_display: InputDisplay::SMASH,
    input_display_status: OnOff::OFF,
});
