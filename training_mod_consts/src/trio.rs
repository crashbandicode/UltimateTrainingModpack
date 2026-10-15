//! Practice roster choices, including individual starting forms.
//! Order is serialized as one-hot menu choices: keep it stable.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use training_mod_tui::{
    StatefulTable, SubMenu, SubMenuType, Toggle, NX_SUBMENU_COLUMNS, NX_SUBMENU_ROWS,
};

pub(crate) const LEGACY_CHOICE_COUNT: usize = 80;

pub(crate) const CHOICES: &[(u32, &str)] = &[
    (0x00, "Mario"),
    (0x01, "Donkey Kong"),
    (0x02, "Link"),
    (0x03, "Samus"),
    (0x04, "Dark Samus"),
    (0x05, "Yoshi"),
    (0x06, "Kirby"),
    (0x07, "Fox"),
    (0x08, "Pikachu"),
    (0x09, "Luigi"),
    (0x0a, "Ness"),
    (0x0b, "Captain Falcon"),
    (0x0c, "Jigglypuff"),
    (0x0d, "Peach"),
    (0x0e, "Daisy"),
    (0x0f, "Bowser"),
    (0x10, "Sheik"),
    (0x11, "Zelda"),
    (0x12, "Dr. Mario"),
    (0x13, "Pichu"),
    (0x14, "Falco"),
    (0x15, "Marth"),
    (0x16, "Lucina"),
    (0x17, "Young Link"),
    (0x18, "Ganondorf"),
    (0x19, "Mewtwo"),
    (0x1a, "Roy"),
    (0x1b, "Chrom"),
    (0x1c, "Mr. Game & Watch"),
    (0x1d, "Meta Knight"),
    (0x1e, "Pit"),
    (0x1f, "Dark Pit"),
    (0x20, "Zero Suit Samus"),
    (0x21, "Wario"),
    (0x22, "Snake"),
    (0x23, "Ike"),
    (0x27, "Diddy Kong"),
    (0x28, "Lucas"),
    (0x29, "Sonic"),
    (0x2a, "King Dedede"),
    (0x2b, "Olimar"),
    (0x2c, "Lucario"),
    (0x2d, "R.O.B."),
    (0x2e, "Toon Link"),
    (0x2f, "Wolf"),
    (0x30, "Villager"),
    (0x31, "Mega Man"),
    (0x32, "Wii Fit Trainer"),
    (0x33, "Rosalina & Luma"),
    (0x34, "Little Mac"),
    (0x35, "Greninja"),
    (0x36, "Palutena"),
    (0x37, "PAC-MAN"),
    (0x38, "Robin"),
    (0x39, "Shulk"),
    (0x3a, "Bowser Jr."),
    (0x3b, "Duck Hunt"),
    (0x3c, "Ryu"),
    (0x3d, "Ken"),
    (0x3e, "Cloud"),
    (0x3f, "Corrin"),
    (0x40, "Bayonetta"),
    (0x41, "Inkling"),
    (0x42, "Ridley"),
    (0x43, "Simon"),
    (0x44, "Richter"),
    (0x45, "King K. Rool"),
    (0x46, "Isabelle"),
    (0x47, "Incineroar"),
    (0x51, "Piranha Plant"),
    (0x52, "Joker"),
    (0x53, "Hero"),
    (0x54, "Banjo & Kazooie"),
    (0x55, "Terry"),
    (0x56, "Byleth"),
    (0x57, "Min Min"),
    (0x58, "Steve"),
    (0x59, "Sephiroth"),
    (0x5c, "Kazuya"),
    (0x5d, "Sora"),
    // Append forms: old saved catalog positions must not move.
    (0x5a, "Pyra"),
    (0x5b, "Mythra"),
    (0x24, "Squirtle"),
    (0x25, "Ivysaur"),
    (0x26, "Charizard"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrioFighter(u32);

impl TrioFighter {
    pub const MARIO: Self = Self(0);
    pub const KIRBY: Self = Self(0x06);
    pub const PIKACHU: Self = Self(0x08);
    pub const BOWSER: Self = Self(0x0f);
    pub const LUCINA: Self = Self(0x16);
    pub const GAMEWATCH: Self = Self(0x1c);
    pub const PALUTENA: Self = Self(0x36);
    pub const BAYONETTA: Self = Self(0x40);

    pub const fn kind(self) -> u32 {
        self.0
    }
    pub fn name(self) -> &'static str {
        CHOICES
            .iter()
            .find(|(kind, _)| *kind == self.0)
            .map(|(_, name)| *name)
            .unwrap_or("Unavailable")
    }
    pub fn supported_kind(kind: u32) -> bool {
        CHOICES.iter().any(|(candidate, _)| *candidate == kind)
    }
    pub fn available() -> Vec<Self> {
        CHOICES.iter().map(|(kind, _)| Self(*kind)).collect()
    }
    pub fn from_kind(kind: u32) -> Option<Self> {
        Self::supported_kind(kind).then_some(Self(kind))
    }
    pub fn to_submenu<'a>(title: &'a str, id: &'a str, help_text: &'a str) -> SubMenu<'a> {
        let toggles = CHOICES
            .iter()
            .map(|(_, name)| Toggle {
                title: name,
                value: 0,
                max: 1,
            })
            .collect();
        SubMenu {
            title,
            id,
            help_text,
            submenu_type: SubMenuType::ToggleSingle,
            toggles: StatefulTable::with_items(
                CHOICES
                    .len()
                    .div_ceil(NX_SUBMENU_COLUMNS)
                    .max(NX_SUBMENU_ROWS),
                NX_SUBMENU_COLUMNS,
                toggles,
            ),
            slider: None,
        }
    }
}

impl Serialize for TrioFighter {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        CHOICES
            .iter()
            .map(|(kind, _)| u8::from(*kind == self.0))
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for TrioFighter {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = Vec::<u8>::deserialize(deserializer)?;
        if (values.len() != CHOICES.len() && values.len() != LEGACY_CHOICE_COUNT)
            || values.iter().any(|v| *v > 1)
            || values.iter().filter(|v| **v == 1).count() != 1
        {
            return Err(serde::de::Error::custom(
                "select exactly one supported trio fighter",
            ));
        }
        Ok(Self(
            CHOICES[values.iter().position(|v| *v == 1).unwrap()].0,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use training_mod_tui::InputControl;
    #[test]
    fn every_choice_round_trips_and_unsupported_groups_are_excluded() {
        for (kind, _) in CHOICES {
            let choice = TrioFighter(*kind);
            assert_eq!(
                serde_json::from_str::<TrioFighter>(&serde_json::to_string(&choice).unwrap())
                    .unwrap(),
                choice
            );
        }
        for kind in [
            0x48,
            0x49,
            0x4a,
            0x4b,
            0x4c,
            0x4d,
            0x4e,
            0x4f,
            0x50,
            0x75,
            u32::MAX,
        ] {
            assert!(!TrioFighter::supported_kind(kind));
        }
    }
    #[test]
    fn legacy_one_hot_positions_remain_stable() {
        // Saved before forms existed: Sora was the final choice, at index 79.
        let mut legacy = vec![0u8; 80];
        legacy[79] = 1;
        let fighter: TrioFighter = serde_json::from_value(serde_json::json!(legacy)).unwrap();
        assert_eq!(fighter.kind(), 0x5d);
        let current = serde_json::to_value(fighter).unwrap();
        assert_eq!(current.as_array().unwrap().len(), 85);
        assert_eq!(current[79], 1);
        assert_eq!(current[80], 0);
        for length in [0, 79, 81, 84, 86] {
            let mut invalid = vec![0u8; length];
            if length > 0 {
                invalid[0] = 1;
            }
            assert!(serde_json::from_value::<TrioFighter>(serde_json::json!(invalid)).is_err());
        }
        legacy[0] = 1;
        assert!(serde_json::from_value::<TrioFighter>(serde_json::json!(legacy)).is_err());
    }
    #[test]
    fn malformed_choices_are_not_randomized() {
        for values in [vec![0; CHOICES.len()], vec![1; CHOICES.len()], vec![1]] {
            assert!(
                serde_json::from_str::<TrioFighter>(&serde_json::to_string(&values).unwrap())
                    .is_err()
            );
        }
    }
    #[test]
    fn picker_navigates_all_pages_and_retains_last_choice() {
        let mut picker = TrioFighter::to_submenu("Slot", "slot", "Choose");
        picker.toggles.select(7, 0);
        picker.on_down();
        assert_eq!(picker.toggles.state.selected_row(), Some(8));
        picker.toggles.select(19, 3);
        picker.on_a();
        assert_eq!(
            serde_json::from_str::<TrioFighter>(&serde_json::to_string(&picker).unwrap())
                .unwrap()
                .kind(),
            0x5d
        );
        picker.on_down();
        assert_eq!(picker.toggles.state.selected_row(), Some(20));
        picker.toggles.select(21, 0);
        picker.on_a();
        assert_eq!(
            serde_json::from_str::<TrioFighter>(&serde_json::to_string(&picker).unwrap())
                .unwrap().kind(),
            0x26
        );
        picker.on_down();
        assert_eq!(picker.toggles.state.selected_row(), Some(0));
    }
    #[test]
    fn full_menu_reopens_selected_page_and_commits_a_new_fighter() {
        let mut app = unsafe { crate::create_app() };
        let mut initial = crate::BASE_MENU;
        initial.player_character = TrioFighter::PALUTENA;
        app.update_all_from_json(&serde_json::to_string(&initial).unwrap());
        let player = app
            .tabs
            .iter()
            .position(|tab| tab.id == "player_characters")
            .unwrap();
        app.tabs.state.select(Some(player));
        let index = app
            .selected_tab()
            .submenus
            .iter()
            .position(|submenu| submenu.id == "player_character")
            .unwrap();
        let cols = app.selected_tab().submenus.cols;
        app.selected_tab()
            .submenus
            .select(index / cols, index % cols);
        app.on_a();
        assert_eq!(app.selected_submenu().selected_toggle().title, "Palutena");
        assert!(app.selected_submenu().toggles.state.selected_row().unwrap() >= NX_SUBMENU_ROWS);
        app.selected_submenu().toggles.select(19, 3);
        app.on_a();
        app.on_b();
        let menu: crate::TrainingModpackMenu =
            serde_json::from_str(&app.current_settings_to_json()).unwrap();
        assert_eq!(menu.player_character.name(), "Sora");
        assert_eq!(menu.save_state_enable, crate::BASE_MENU.save_state_enable);
    }
}
