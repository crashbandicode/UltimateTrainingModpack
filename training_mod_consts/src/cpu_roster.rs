use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{ButtonConfig, TrioFighter};

/// Stable one-hot menu order: one through eight active CPU roster slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CpuRosterSize {
    One,
    Two,
    Three,
    #[default]
    Four,
    Five,
    Six,
    Seven,
    Eight,
}

const SIZES: [CpuRosterSize; 8] = [
    CpuRosterSize::One,
    CpuRosterSize::Two,
    CpuRosterSize::Three,
    CpuRosterSize::Four,
    CpuRosterSize::Five,
    CpuRosterSize::Six,
    CpuRosterSize::Seven,
    CpuRosterSize::Eight,
];

impl CpuRosterSize {
    pub const fn count(self) -> usize {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Three => 3,
            Self::Four => 4,
            Self::Five => 5,
            Self::Six => 6,
            Self::Seven => 7,
            Self::Eight => 8,
        }
    }
}

impl Serialize for CpuRosterSize {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SIZES
            .map(|size| u8::from(size == *self))
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CpuRosterSize {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = <[u8; 8]>::deserialize(deserializer)?;
        if values.iter().any(|value| *value > 1)
            || values.iter().filter(|value| **value == 1).count() != 1
        {
            return Err(serde::de::Error::custom(
                "select exactly one CPU roster size",
            ));
        }
        Ok(SIZES[values.iter().position(|value| *value == 1).unwrap()])
    }
}

pub const DEFAULT_CPU_CYCLE: ButtonConfig = ButtonConfig {
    ZL: 1,
    DPAD_LEFT: 1,
    ..ButtonConfig::empty()
};

pub const DEFAULT_PLAYER_CYCLE: ButtonConfig = ButtonConfig {
    ZL: 1,
    DPAD_RIGHT: 1,
    ..ButtonConfig::empty()
};

pub fn player_cycle_default() -> ButtonConfig {
    DEFAULT_PLAYER_CYCLE
}
pub fn player_character_default() -> TrioFighter {
    TrioFighter::GAMEWATCH
}
pub fn player_roster_size_default() -> CpuRosterSize {
    CpuRosterSize::Three
}
pub fn player_roster_4_default() -> TrioFighter {
    TrioFighter::KIRBY
}
pub fn player_roster_5_default() -> TrioFighter {
    TrioFighter::BOWSER
}
pub fn player_roster_6_default() -> TrioFighter {
    TrioFighter::BAYONETTA
}
pub fn player_roster_7_default() -> TrioFighter {
    TrioFighter::MARIO
}
pub fn player_roster_8_default() -> TrioFighter {
    TrioFighter::LUCINA
}

pub fn cpu_cycle_default() -> ButtonConfig {
    DEFAULT_CPU_CYCLE
}
pub fn cpu_character_default() -> TrioFighter {
    TrioFighter::PIKACHU
}
pub fn cpu_roster_1_default() -> TrioFighter {
    TrioFighter::PIKACHU
}
pub fn cpu_roster_2_default() -> TrioFighter {
    TrioFighter::KIRBY
}
pub fn cpu_roster_3_default() -> TrioFighter {
    TrioFighter::BOWSER
}
pub fn cpu_roster_4_default() -> TrioFighter {
    TrioFighter::BAYONETTA
}
pub fn cpu_roster_5_default() -> TrioFighter {
    TrioFighter::MARIO
}
pub fn cpu_roster_6_default() -> TrioFighter {
    TrioFighter::LUCINA
}
pub fn cpu_roster_7_default() -> TrioFighter {
    TrioFighter::PALUTENA
}
pub fn cpu_roster_8_default() -> TrioFighter {
    TrioFighter::GAMEWATCH
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TrainingModpackMenu, BASE_MENU};
    use training_mod_tui::InputControl;

    #[test]
    fn sizes_round_trip_and_reject_ambiguous_selections() {
        for (index, size) in SIZES.iter().copied().enumerate() {
            assert_eq!(size.count(), index + 1);
            let json = serde_json::to_string(&size).unwrap();
            assert_eq!(serde_json::from_str::<CpuRosterSize>(&json).unwrap(), size);
        }
        for json in [
            "[0,0,0,0,0,0,0,0]",
            "[1,1,0,0,0,0,0,0]",
            "[2,0,0,0,0,0,0,0]",
            "[1]",
        ] {
            assert!(serde_json::from_str::<CpuRosterSize>(json).is_err());
        }
    }

    #[test]
    fn old_configuration_gains_cpu_defaults_and_preserves_player_roster() {
        let mut json = serde_json::to_value(BASE_MENU).unwrap();
        json.as_object_mut()
            .unwrap()
            .retain(|key, _| !key.starts_with("cpu_"));
        let old: TrainingModpackMenu = serde_json::from_value(json).unwrap();
        assert_eq!(old.cpu_character, TrioFighter::PIKACHU);
        assert_eq!(old.cpu_roster_size.count(), 4);
        assert_eq!(
            [
                old.cpu_roster_1,
                old.cpu_roster_2,
                old.cpu_roster_3,
                old.cpu_roster_4
            ],
            [
                TrioFighter::PIKACHU,
                TrioFighter::KIRBY,
                TrioFighter::BOWSER,
                TrioFighter::BAYONETTA
            ]
        );
        assert_eq!(old.cpu_cycle, DEFAULT_CPU_CYCLE);
        assert_eq!(old.trio_fighter_1, BASE_MENU.trio_fighter_1);
        assert_eq!(old.save_state_enable, BASE_MENU.save_state_enable);
    }

    #[test]
    fn cpu_menu_round_trips_and_commits_an_ordered_choice() {
        let mut app = unsafe { crate::create_app() };
        app.update_all_from_json(&serde_json::to_string(&BASE_MENU).unwrap());
        let cpu = app
            .tabs
            .iter()
            .position(|tab| tab.id == "cpu_characters")
            .unwrap();
        app.tabs.state.select(Some(cpu));
        let index = app
            .selected_tab()
            .submenus
            .iter()
            .position(|submenu| submenu.id == "cpu_roster")
            .unwrap();
        let cols = app.selected_tab().submenus.cols;
        app.selected_tab()
            .submenus
            .select(index / cols, index % cols);
        app.on_a();
        // Mario follows All and appends after the four configured characters.
        app.selected_submenu().toggles.select(0, 1);
        app.on_a();
        app.on_b();
        let menu: TrainingModpackMenu =
            serde_json::from_str(&app.current_settings_to_json()).unwrap();
        assert_eq!(
            menu.cpu_roster.unwrap().fighters(),
            vec![
                TrioFighter::PIKACHU,
                TrioFighter::KIRBY,
                TrioFighter::BOWSER,
                TrioFighter::BAYONETTA,
                TrioFighter::MARIO,
            ]
        );
        assert_eq!(menu.cpu_roster_size, CpuRosterSize::Four);
        assert_eq!(menu.cpu_character, TrioFighter::PIKACHU);
        assert_eq!(menu.cpu_cycle, DEFAULT_CPU_CYCLE);
        assert_eq!(menu.trio_fighter_1, TrioFighter::GAMEWATCH);
    }

    #[test]
    fn old_custom_trio_gains_player_defaults_without_changing_slots() {
        let mut initial = BASE_MENU;
        initial.trio_fighter_1 = TrioFighter::MARIO;
        initial.trio_fighter_2 = TrioFighter::BOWSER;
        initial.trio_fighter_3 = TrioFighter::KIRBY;
        let mut json = serde_json::to_value(initial).unwrap();
        for key in [
            "player_character",
            "player_roster_size",
            "player_roster_4",
            "player_roster_5",
            "player_roster_6",
            "player_roster_7",
            "player_roster_8",
            "player_cycle",
        ] {
            json.as_object_mut().unwrap().remove(key);
        }
        let old: TrainingModpackMenu = serde_json::from_value(json).unwrap();
        assert_eq!(old.player_character, TrioFighter::GAMEWATCH);
        assert_eq!(old.player_roster_size.count(), 3);
        assert_eq!(
            [old.trio_fighter_1, old.trio_fighter_2, old.trio_fighter_3],
            [TrioFighter::MARIO, TrioFighter::BOWSER, TrioFighter::KIRBY]
        );
        assert_eq!(old.trio_practice, initial.trio_practice);
        assert_eq!(old.player_roster_4, TrioFighter::KIRBY);
        assert_eq!(old.player_roster_8, TrioFighter::LUCINA);
        assert_eq!(old.player_cycle, DEFAULT_PLAYER_CYCLE);
        assert_eq!(old.cpu_cycle, DEFAULT_CPU_CYCLE);
        assert_ne!(old.player_cycle, old.cpu_cycle);
        assert_eq!(old.player_cycle.ZL, 1);
        assert_eq!(old.player_cycle.DPAD_RIGHT, 1);
        assert_eq!(old.cpu_cycle.DPAD_LEFT, 1);
    }

    #[test]
    fn player_tab_commits_order_and_preserves_hidden_legacy_slots() {
        let mut app = unsafe { crate::create_app() };
        app.update_all_from_json(&serde_json::to_string(&BASE_MENU).unwrap());
        let player = app
            .tabs
            .iter()
            .position(|tab| tab.id == "player_characters")
            .unwrap();
        let misc = app.tabs.iter().find(|tab| tab.id == "misc").unwrap();
        assert!(!misc
            .submenus
            .iter()
            .any(|submenu| submenu.id.starts_with("trio_")));
        app.tabs.state.select(Some(player));
        let index = app
            .selected_tab()
            .submenus
            .iter()
            .position(|submenu| submenu.id == "player_roster")
            .unwrap();
        let cols = app.selected_tab().submenus.cols;
        app.selected_tab()
            .submenus
            .select(index / cols, index % cols);
        app.on_a();
        app.selected_submenu().toggles.select(0, 1);
        app.on_a();
        app.on_b();
        let menu: TrainingModpackMenu =
            serde_json::from_str(&app.current_settings_to_json()).unwrap();
        assert_eq!(
            menu.player_roster.unwrap().fighters(),
            vec![
                TrioFighter::GAMEWATCH,
                TrioFighter::PALUTENA,
                TrioFighter::PIKACHU,
                TrioFighter::MARIO,
            ]
        );
        assert_eq!(menu.player_roster_size, CpuRosterSize::Three);
        assert_eq!(menu.player_cycle, DEFAULT_PLAYER_CYCLE);
        assert_eq!(menu.trio_fighter_1, TrioFighter::GAMEWATCH);
        assert_eq!(menu.trio_fighter_2, TrioFighter::PALUTENA);
        assert_eq!(menu.trio_fighter_3, TrioFighter::PIKACHU);
        assert_eq!(menu.cpu_roster_4, TrioFighter::BAYONETTA);
    }
}
