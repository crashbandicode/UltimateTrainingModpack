//! Stable item menu identities. Runtime item kinds belong to the native adapter.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use crate::{StatefulTable, SubMenu, SubMenuType, Toggle, NX_SUBMENU_COLUMNS, NX_SUBMENU_ROWS};

pub const ITEM_NAMES: [&str; 38] = [
    "Banjo & Kazooie: Grenade Egg", "Bowser Jr.: Mechakoopa",
    "Daisy: Smile Turnip", "Daisy: Winky Turnip", "Daisy: Dot-Eyes Turnip",
    "Daisy: Stitch-Face Turnip", "Daisy: Mr. Saturn", "Daisy: Bob-omb",
    "Diddy Kong: Banana Peel", "Diddy Kong: Peanut", "King K. Rool: Crown",
    "Link: Remote Bomb", "Link: Arrow", "Mega Man: Metal Blade",
    "Pac-Man: Cherry", "Pac-Man: Strawberry", "Pac-Man: Orange", "Pac-Man: Apple",
    "Pac-Man: Melon", "Pac-Man: Galaxian", "Pac-Man: Bell", "Pac-Man: Key",
    "Peach: Smile Turnip", "Peach: Winky Turnip", "Peach: Dot-Eyes Turnip",
    "Peach: Stitch-Face Turnip", "Peach: Mr. Saturn", "Peach: Bob-omb",
    "Richter: Holy Water", "R.O.B.: Gyro", "Robin: Tome", "Robin: Levin Sword",
    "Sheik: Burst Grenade", "Simon: Holy Water", "Snake: Hand Grenade",
    "Toon Link: Bomb", "Villager: Wood Chip", "Young Link: Bomb",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CharacterItemChoice(pub Option<u8>);

/// Values, never native item pointers. Missing snapshots in older saves retain
/// the legacy on-load item setting; Empty and Unsupported are distinct states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HeldItemSnapshot {
    Empty,
    Unsupported,
    Item {
        choice: u8,
        // Logical native variant: turnips store the actual face index 0..7;
        // other items store ITEM_INSTANCE_WORK_INT_VARIATION. No engine work
        // field is rewritten when a natural turnip is saved.
        variation: i32,
        lifetime: i32,
        lifetime_max: i32,
        fuse: i32,
        fuse_max: i32,
    },
}

pub fn valid_variation(choice: usize, variation: i32) -> bool {
    match choice {
        // The source's Smile choice also identifies the four unlisted common
        // native faces in saved data. Menu selection still grants face zero.
        2 | 22 => (0..=4).contains(&variation),
        3 | 23 => variation == 5,
        4 | 24 => variation == 6,
        5 | 25 => variation == 7,
        29 => (0..8).contains(&variation),
        _ => choice < ITEM_NAMES.len() && variation == 0,
    }
}

impl HeldItemSnapshot {
    pub fn valid(self) -> bool {
        match self {
            Self::Item { choice, variation, lifetime, lifetime_max, fuse, fuse_max } => {
                let counter = |value, maximum| (value == -1 && maximum == -1)
                    || (maximum >= 0 && value >= -1 && value <= maximum);
                valid_variation(usize::from(choice), variation)
                    && counter(lifetime, lifetime_max) && counter(fuse, fuse_max)
            }
            Self::Empty | Self::Unsupported => true,
        }
    }
}

/// Consume a present item field independently of its slot's other data. Older
/// saves omit it (the field's serde default is None). A damaged or unsupported
/// item becomes Unsupported instead of discarding every practice save slot.
pub fn deserialize_saved_item<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<HeldItemSnapshot>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(Some(serde_json::from_value::<HeldItemSnapshot>(value)
        .ok().filter(|item| item.valid()).unwrap_or(HeldItemSnapshot::Unsupported)))
}

impl CharacterItemChoice {
    pub fn to_submenu() -> SubMenu<'static> {
        SubMenu {
            title: "Character Item", id: "character_item_grant",
            help_text: "A: Request item for Player 1; close the menu to apply.\nPress A again to request a fresh item. Flash confirms the button press.",
            submenu_type: SubMenuType::ToggleSingle,
            toggles: StatefulTable::with_items(ITEM_NAMES.len().div_ceil(NX_SUBMENU_COLUMNS).max(NX_SUBMENU_ROWS),
                NX_SUBMENU_COLUMNS, ITEM_NAMES.iter().map(|title| Toggle { title, value: 0, max: 1 }).collect()),
            slider: None,
        }
    }
}

impl Serialize for CharacterItemChoice {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (0..ITEM_NAMES.len()).map(|i| u8::from(self.0 == Some(i as u8))).collect::<Vec<_>>().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CharacterItemChoice {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = Vec::<u8>::deserialize(deserializer)?;
        if values.len() != ITEM_NAMES.len() || values.iter().any(|v| *v > 1)
            || values.iter().filter(|v| **v == 1).count() > 1 {
            return Err(serde::de::Error::custom("invalid character item choice"));
        }
        Ok(Self(values.iter().position(|v| *v == 1).map(|i| i as u8)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn action_picker_discards_old_checks_and_preserves_ordinary_settings() {
        use training_mod_tui::{App, InputControl};
        fn select(app: &mut App<'_>, id: &str) {
            let (tab, row, col) = app.tabs.iter().enumerate().find_map(|(tab, value)| {
                value.submenus.iter().position(|submenu| submenu.id == id)
                    .map(|index| (tab, index / value.submenus.cols, index % value.submenus.cols))
            }).unwrap();
            app.tabs.state.select(Some(tab));
            app.selected_tab().submenus.select(row, col);
        }
        let mut saved = crate::BASE_MENU;
        saved.character_item_grant = CharacterItemChoice(Some(37));
        let mut app = unsafe { crate::create_app() };
        app.open_settings_for_player(&serde_json::to_string(&saved).unwrap(), None);
        select(&mut app, "character_item_grant");
        app.on_a();
        app.selected_submenu().toggles.select(9, 1);
        for _ in 0..2 {
            app.on_a();
            let stored: crate::TrainingModpackMenu =
                serde_json::from_str(&app.current_settings_to_json()).unwrap();
            assert_eq!(stored.character_item_grant, CharacterItemChoice(None));
            assert_eq!(stored.lobby_practice, saved.lobby_practice);
            assert_eq!(app.selected_submenu().toggles.state.selected_row(), Some(9));
        }
        app.on_b();
        app.on_a();
        assert!(app.selected_submenu().toggles.iter().all(|toggle| toggle.value == 0));
        app.on_b();
        select(&mut app, "lobby_practice");
        app.on_a();
        app.selected_submenu().toggles.select(0, 0);
        app.on_a();
        let stored: crate::TrainingModpackMenu =
            serde_json::from_str(&app.current_settings_to_json()).unwrap();
        assert_eq!(stored.lobby_practice, crate::OnOff::ON);
        assert_eq!(stored.character_item_grant, CharacterItemChoice(None));
    }

    #[test]
    fn all_choices_round_trip_including_second_page() {
        for i in 0..ITEM_NAMES.len() {
            let choice = CharacterItemChoice(Some(i as u8));
            assert_eq!(serde_json::from_str::<CharacterItemChoice>(&serde_json::to_string(&choice).unwrap()).unwrap(), choice);
        }
        let none = CharacterItemChoice(None);
        assert_eq!(serde_json::from_str::<CharacterItemChoice>(&serde_json::to_string(&none).unwrap()).unwrap(), none);
    }
    #[test]
    fn malformed_or_multiple_choices_are_rejected() {
        assert!(serde_json::from_str::<CharacterItemChoice>("[]").is_err());
        let mut values = vec![0; ITEM_NAMES.len()];
        values[0] = 1; values[37] = 1;
        assert!(serde_json::from_str::<CharacterItemChoice>(&serde_json::to_string(&values).unwrap()).is_err());
    }

    #[test]
    fn saved_variant_is_validated_independently_of_the_current_menu() {
        let stitch = HeldItemSnapshot::Item { choice: 25, variation: 7,
            lifetime: 0, lifetime_max: 0, fuse: -1, fuse_max: 0 };
        assert!(stitch.valid());
        assert_eq!(serde_json::from_str::<HeldItemSnapshot>(&serde_json::to_string(&stitch).unwrap()).unwrap(), stitch);
        assert!(!valid_variation(25, 0));
        assert!(!valid_variation(255, 0));
        assert!(valid_variation(29, 7));
        assert!(!valid_variation(29, 8));
        let invalid = HeldItemSnapshot::Item { choice: 11, variation: 0,
            lifetime: -1, lifetime_max: -1, fuse: 1801, fuse_max: 1800 };
        assert!(!invalid.valid());
        assert_ne!(HeldItemSnapshot::Empty, HeldItemSnapshot::Unsupported);
    }

    #[test]
    fn all_native_turnip_faces_keep_exact_saved_identity() {
        for base in [2u8, 22u8] {
            for face in 0..=7 {
                let choice = base + match face { 5 => 1, 6 => 2, 7 => 3, _ => 0 };
                let item = HeldItemSnapshot::Item { choice, variation: face,
                    lifetime: 0, lifetime_max: 0, fuse: -1, fuse_max: 0 };
                assert!(item.valid());
                let encoded = toml::to_string(&item).unwrap();
                assert_eq!(toml::from_str::<HeldItemSnapshot>(&encoded).unwrap(), item);
            }
            assert!(!valid_variation(usize::from(base), -1));
            assert!(!valid_variation(usize::from(base), 5));
            assert!(!valid_variation(usize::from(base + 1), 1));
            assert!(!valid_variation(usize::from(base + 3), 8));
        }
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Slot {
        marker: u32,
        #[serde(default, deserialize_with = "deserialize_saved_item")]
        held_item: Option<HeldItemSnapshot>,
    }
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Slots { player: Vec<Slot> }

    #[test]
    fn toml_keeps_legacy_empty_unsupported_and_exact_item_distinct() {
        let saved = Slots { player: vec![
            Slot { marker: 11, held_item: None },
            Slot { marker: 22, held_item: Some(HeldItemSnapshot::Empty) },
            Slot { marker: 33, held_item: Some(HeldItemSnapshot::Unsupported) },
            Slot { marker: 44, held_item: Some(HeldItemSnapshot::Item {
                choice: 25, variation: 7, lifetime: 500, lifetime_max: 1400,
                fuse: -1, fuse_max: 0 }) },
        ] };
        let file = toml::to_string_pretty(&saved).unwrap();
        assert_eq!(toml::from_str::<Slots>(&file).unwrap(), saved);
    }

    #[test]
    fn damaged_item_fields_do_not_discard_adjacent_slots() {
        // These fixtures exercise the actual field deserializer used by SavedState.
        for damaged in [
            "held_item = 'damaged'",
            "[player.held_item]\nstate = 'unknown'",
            "[player.held_item]\nstate = 'item'\nchoice = 25\nvariation = 0\nlifetime = 0\nlifetime_max = 0\nfuse = -1\nfuse_max = 0",
            "[player.held_item]\nstate = 'item'\nchoice = 11\nvariation = 0\nlifetime = -1\nlifetime_max = -1\nfuse = 1801\nfuse_max = 1800",
            "[player.held_item]\nstate = 'item'\nchoice = 999",
        ] {
            let file = format!("[[player]]\nmarker = 11\n\n[[player]]\nmarker = 22\n{damaged}\n\n[[player]]\nmarker = 33\n[player.held_item]\nstate = 'empty'\n");
            let loaded: Slots = toml::from_str(&file).unwrap();
            assert_eq!(loaded.player, vec![
                Slot { marker: 11, held_item: None },
                Slot { marker: 22, held_item: Some(HeldItemSnapshot::Unsupported) },
                Slot { marker: 33, held_item: Some(HeldItemSnapshot::Empty) },
            ]);
        }
    }
}
