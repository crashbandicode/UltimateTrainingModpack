//! Lobby-stage preferences. Catalog indices are stable; ranks store selection order.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use training_mod_tui::{
    StatefulTable, SubMenu, SubMenuType, Toggle, NX_SUBMENU_COLUMNS, NX_SUBMENU_ROWS,
};

// Owned ui_stage_db.prc: can_select=true, sorted by native disp_order.
// Labels join name_id to msg_name+us_en.msbt's nam_stg1_* keys; stage IDs
// join stage_place_id to the native form-0 table. Waiting Room is our default;
// Training is appended separately because it is not a normal selectable stage.
pub const STAGES: &[(u32, &str)] = &[
    (311, "Waiting Room"),
    (0, "Battlefield"),
    (347, "Small Battlefield"),
    (2, "Big Battlefield"),
    (3, "Final Destination"),
    (5, "Peach's Castle"),
    (8, "Kongo Jungle"),
    (11, "Hyrule Castle"),
    (14, "Super Happy Tree"),
    (17, "Dream Land"),
    (20, "Saffron City"),
    (23, "Mushroom Kingdom"),
    (26, "Princess Peach's Castle"),
    (29, "Rainbow Cruise"),
    (32, "Kongo Falls"),
    (35, "Jungle Japes"),
    (38, "Great Bay"),
    (41, "Temple"),
    (62, "Brinstar"),
    (47, "Yoshi's Island (Melee)"),
    (44, "Yoshi's Story"),
    (50, "Fountain of Dreams"),
    (53, "Green Greens"),
    (56, "Corneria"),
    (59, "Venom"),
    (68, "Pokémon Stadium"),
    (65, "Onett"),
    (80, "Mushroom Kingdom II"),
    (71, "Brinstar Depths"),
    (77, "Big Blue"),
    (74, "Fourside"),
    (83, "Delfino Plaza"),
    (134, "Mushroomy Kingdom"),
    (125, "Figure-8 Circuit"),
    (104, "WarioWare, Inc."),
    (92, "Bridge of Eldin"),
    (122, "Norfair"),
    (128, "Frigate Orpheon"),
    (86, "Yoshi's Island"),
    (110, "Halberd"),
    (89, "Lylat Cruise"),
    (107, "Pokémon Stadium 2"),
    (137, "Port Town Aero Dive"),
    (101, "Castle Siege"),
    (131, "Distant Planet"),
    (95, "Smashville"),
    (116, "New Pork City"),
    (119, "Summit"),
    (98, "Skyworld"),
    (113, "Shadow Moses Island"),
    (140, "Luigi's Mansion"),
    (143, "Pirate Ship"),
    (146, "Spear Pillar"),
    (149, "75 m"),
    (152, "Mario Bros."),
    (155, "Hanenbow"),
    (158, "Green Hill Zone"),
    (161, "3D Land"),
    (164, "Golden Plains"),
    (167, "Paper Mario"),
    (170, "Gerudo Valley"),
    (173, "Spirit Train"),
    (218, "Dream Land GB"),
    (176, "Unova Pokémon League"),
    (179, "Prism Tower"),
    (224, "Mute City SNES"),
    (215, "Magicant"),
    (182, "Arena Ferox"),
    (185, "Reset Bomb Forest"),
    (188, "Tortimer Island"),
    (221, "Balloon Fight"),
    (200, "Living Room"),
    (203, "Find Mii"),
    (206, "Tomodachi Life"),
    (209, "PictoChat 2"),
    (227, "Mushroom Kingdom U"),
    (230, "Mario Galaxy"),
    (233, "Mario Circuit"),
    (236, "Skyloft"),
    (239, "The Great Cave Offensive"),
    (242, "Kalos Pokémon League"),
    (245, "Coliseum"),
    (278, "Flat Zone X"),
    (248, "Palutena's Temple"),
    (251, "Gamer"),
    (254, "Garden of Hope"),
    (257, "Town and City"),
    (260, "Wii Fit Studio"),
    (191, "Boxing Ring"),
    (197, "Gaur Plain"),
    (281, "Duck Hunt"),
    (263, "Wrecking Crew"),
    (266, "Pilotwings"),
    (269, "Wuhu Island"),
    (272, "Windy Hill Zone"),
    (212, "Wily Castle"),
    (275, "PAC-LAND"),
    (287, "Super Mario Maker"),
    (284, "Suzaku Castle"),
    (290, "Midgar"),
    (293, "Umbra Clock Tower"),
    (305, "New Donk City Hall"),
    (302, "Great Plateau Tower"),
    (296, "Moray Towers"),
    (299, "Dracula's Castle"),
    (328, "Mementos"),
    (332, "Yggdrasil's Altar"),
    (335, "Spiral Mountain"),
    (338, "King of Fighters Stadium"),
    (341, "Garreg Mach Monastery"),
    (344, "Spring Stadium"),
    (348, "Minecraft World"),
    (351, "Northern Cave"),
    (355, "Cloud Sea of Alrest"),
    (358, "Mishima Dojo"),
    (361, "Hollow Bastion"),
    // Append special practice stages so existing persisted catalog indices stay stable.
    (308, "Training"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LobbyStageOrder {
    all: bool,
    ranks: [u8; STAGES.len()],
}

impl Default for LobbyStageOrder {
    fn default() -> Self {
        Self::NATIVE
    }
}

impl LobbyStageOrder {
    pub const NATIVE: Self = Self {
        all: false,
        ranks: {
            let mut ranks = [0; STAGES.len()];
            ranks[0] = 1;
            ranks
        },
    };

    pub fn stages(self) -> Vec<u32> {
        if self.all {
            return STAGES.iter().map(|(id, _)| *id).collect();
        }
        let mut selected: Vec<_> = self
            .ranks
            .iter()
            .zip(STAGES)
            .filter(|(rank, _)| **rank != 0)
            .collect();
        selected.sort_unstable_by_key(|(rank, _)| **rank);
        selected.into_iter().map(|(_, (id, _))| *id).collect()
    }

    pub fn to_submenu() -> SubMenu<'static> {
        let toggles = std::iter::once("All (listed order)")
            .chain(STAGES.iter().map(|(_, name)| *name))
            .enumerate()
            .map(|(index, title)| Toggle {
                title,
                value: u8::from(index == 1),
                max: 1,
            })
            .collect();
        SubMenu {
            title: "Lobby Stages",
            id: "lobby_stages",
            help_text: "Select stages in order. Changes apply on the next lobby entry.\nThe match stage is unchanged.",
            submenu_type: SubMenuType::Ordered,
            toggles: StatefulTable::with_items(
                (STAGES.len() + 1).div_ceil(NX_SUBMENU_COLUMNS).max(NX_SUBMENU_ROWS),
                NX_SUBMENU_COLUMNS, toggles,
            ),
            slider: None,
        }
    }
}

impl Serialize for LobbyStageOrder {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        std::iter::once(u8::from(self.all))
            .chain(self.ranks)
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LobbyStageOrder {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = Vec::<u8>::deserialize(deserializer)?;
        // The first full catalog had 116 stages, before Training was appended.
        if !matches!(values.len(), 4 | 117) && values.len() != STAGES.len() + 1 {
            return Err(serde::de::Error::custom("invalid lobby stage catalog"));
        }
        if values[0] > 1 {
            return Err(serde::de::Error::custom("invalid lobby stage catalog"));
        }
        let mut ranks = [0; STAGES.len()];
        if values.len() == 4 {
            // Initial development preferences used Waiting Room/Battlefield/FD.
            for (old, stage) in [311, 0, 3].iter().enumerate() {
                let index = STAGES.iter().position(|(id, _)| id == stage).unwrap();
                ranks[index] = values[old + 1];
            }
        } else {
            ranks[..values.len() - 1].copy_from_slice(&values[1..]);
        }
        let mut selected: Vec<_> = ranks.iter().copied().filter(|rank| *rank != 0).collect();
        selected.sort_unstable();
        let all = values[0] == 1;
        if (all && !selected.is_empty())
            || (!all
                && (selected.is_empty() || !selected.iter().copied().eq(1..=selected.len() as u8)))
        {
            return Err(serde::de::Error::custom(
                "choose All or a contiguous lobby stage order",
            ));
        }
        Ok(Self { all, ranks })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use training_mod_tui::InputControl;

    #[test]
    fn editor_order_roundtrips_and_native_is_the_default() {
        let mut menu = LobbyStageOrder::to_submenu();
        let old: LobbyStageOrder = serde_json::from_str("[0,0,2,1]").unwrap();
        menu.update_from_vec(serde_json::from_value(serde_json::to_value(old).unwrap()).unwrap());
        let order: LobbyStageOrder =
            serde_json::from_str(&serde_json::to_string(&menu).unwrap()).unwrap();
        assert_eq!(order.stages(), vec![3, 0]);
        assert_eq!(LobbyStageOrder::default().stages(), vec![311]);
        menu.toggles.select(0, 0);
        menu.on_a();
        let all: LobbyStageOrder =
            serde_json::from_str(&serde_json::to_string(&menu).unwrap()).unwrap();
        assert_eq!(all.stages(), STAGES.iter().map(|(id, _)| *id).collect::<Vec<_>>());
        assert_eq!(&all.stages()[..5], &[311, 0, 347, 2, 3]);
        assert_eq!(all.stages().len(), 117);
        assert_eq!(all.stages().last(), Some(&308));
    }

    #[test]
    fn invalid_orders_do_not_replace_saved_preferences() {
        for values in [
            vec![],
            vec![0, 0, 0, 0],
            vec![0, 1, 1, 0],
            vec![0, 1, 3, 0],
            vec![1, 1, 0, 0],
        ] {
            assert!(serde_json::from_value::<LobbyStageOrder>(serde_json::json!(values)).is_err());
        }
    }

    #[test]
    fn training_stage_is_selectable_without_shifting_saved_choices() {
        // Old full-catalog preferences: Hollow Bastion then Waiting Room.
        let mut old = vec![0u8; 117];
        old[116] = 1;
        old[1] = 2;
        let restored: LobbyStageOrder = serde_json::from_value(serde_json::json!(old)).unwrap();
        assert_eq!(restored.stages(), vec![361, 311]);
        let mut current: Vec<u8> = serde_json::from_value(serde_json::to_value(restored).unwrap()).unwrap();
        assert_eq!(current.len(), 118);
        assert_eq!(current[117], 0);
        current[117] = 3;
        let selected: LobbyStageOrder = serde_json::from_value(serde_json::json!(current)).unwrap();
        assert_eq!(selected.stages(), vec![361, 311, 308]);
        let menu = LobbyStageOrder::to_submenu();
        assert_eq!(menu.toggles.get(117 / NX_SUBMENU_COLUMNS, 117 % NX_SUBMENU_COLUMNS).unwrap().title, "Training");
    }

    fn stage_app(enabled: bool, ordinary_training: bool) -> training_mod_tui::App<'static> {
        use training_mod_tui::stage_menu::{Mode, Preferences};
        let mut menu = crate::BASE_MENU;
        menu.lobby_stages = serde_json::from_str("[0,0,2,1]").unwrap(); // FD, BF.
        let settings = serde_json::to_string(&menu).unwrap();
        let mut app = unsafe { crate::create_app() };
        app.serialized_default_settings = serde_json::to_string(&crate::BASE_MENU).unwrap();
        app.open_settings_for_player(&settings, Some(9));
        app.open_stage_menu_in_context(enabled, Preferences {
            mode: Mode::Direct,
            direct_stage: 3,
        }, ordinary_training);
        let tab = app.tabs.iter().position(|tab| tab.submenus.iter()
            .any(|submenu| submenu.id == "lobby_stages")).unwrap();
        app.tabs.state.select(Some(tab));
        let index = app.selected_tab().submenus.iter()
            .position(|submenu| submenu.id == "lobby_stages").unwrap();
        let cols = app.selected_tab().submenus.cols;
        app.selected_tab().submenus.select(index / cols, index % cols);
        app.on_a();
        assert!(app.stage_page_active());
        app
    }

    fn stage_cursor(app: &mut training_mod_tui::App<'_>) -> usize {
        let toggles = &app.selected_submenu().toggles;
        toggles.state.selected_row().unwrap() * toggles.cols
            + toggles.state.selected_col().unwrap()
    }

    #[test]
    fn stage_shoulders_page_without_editing_rotation_or_direct_preferences() {
        use training_mod_tui::AppPage;
        // Rotation-only, lobby Direct/Rotation, and Training Direct/Rotation.
        for (enabled, ordinary_training) in [(false, false), (true, false), (true, true)] {
            let mut app = stage_app(enabled, ordinary_training);
            for _ in 0..if enabled { 2 } else { 1 } {
                let settings = app.current_settings_to_json();
                let preferences = app.stage_preferences;
                let direct = app.stage_is_direct();
                let tab = app.tabs.state.selected();
                let cols = app.selected_submenu().toggles.cols;
                app.selected_submenu().toggles.select(15 / cols, 15 % cols);
                app.on_r();
                assert_eq!(stage_cursor(&mut app), 31);
                app.on_l();
                assert_eq!(stage_cursor(&mut app), 15);
                app.on_l(); // Wrap backward, clamp to Training on the short page.
                assert_eq!(stage_cursor(&mut app), 117);
                assert_eq!(app.selected_submenu().selected_toggle().title, "Training");
                app.on_r(); // Wrap forward, keep final-page cell 5.
                assert_eq!(stage_cursor(&mut app), 5);
                app.on_l();
                assert_eq!(stage_cursor(&mut app), 117);
                assert!(app.page == AppPage::TOGGLE); // Never reset confirmation.
                assert_eq!(app.tabs.state.selected(), tab);
                assert_eq!(app.stage_preferences, preferences);
                assert_eq!(app.stage_is_direct(), direct);
                assert_eq!(app.current_settings_to_json(), settings);
                assert!(app.stage_help().contains("L/R Page"));
                if enabled { app.on_x(); }
            }
            app.on_start();
            assert!(app.take_pending_stage().is_none());
            let saved: crate::TrainingModpackMenu =
                serde_json::from_str(&app.current_settings_to_json()).unwrap();
            assert_eq!(saved.lobby_stages.stages(), vec![3, 0]);
        }
    }

    #[test]
    fn paged_training_direct_request_is_one_shot_and_preserves_rotation() {
        use training_mod_tui::AppPage;
        for ordinary_training in [false, true] {
            let mut app = stage_app(true, ordinary_training);
            let preferences = app.stage_preferences;
            let cols = app.selected_submenu().toggles.cols;
            app.selected_submenu().toggles.select(15 / cols, 15 % cols);
            app.on_l();
            assert_eq!(stage_cursor(&mut app), 117);
            app.on_a();
            assert!(app.page == AppPage::CLOSE);
            assert_eq!(app.take_pending_stage(), Some(308));
            assert_eq!(app.take_pending_stage(), None);
            let saved: crate::MenuJsonStruct =
                serde_json::from_str(&app.get_serialized_settings_with_defaults()).unwrap();
            assert_eq!(saved.menu.lobby_stages.stages(), vec![3, 0]);
            assert_eq!(saved.defaults_menu.lobby_stages, crate::BASE_MENU.lobby_stages);
            assert_eq!(saved.stage_preferences.mode, preferences.mode);
            assert_eq!(saved.stage_preferences.direct_stage,
                if ordinary_training { preferences.direct_stage } else { 308 });
        }
    }

    #[test]
    fn paged_training_rotation_appends_after_existing_ranks_without_a_request() {
        let mut app = stage_app(false, false);
        let cols = app.selected_submenu().toggles.cols;
        app.selected_submenu().toggles.select(15 / cols, 15 % cols);
        app.on_l();
        assert_eq!(stage_cursor(&mut app), 117);
        app.on_a();
        app.on_b();
        app.on_start();
        assert!(app.take_pending_stage().is_none());
        let saved: crate::TrainingModpackMenu =
            serde_json::from_str(&app.current_settings_to_json()).unwrap();
        assert_eq!(saved.lobby_stages.stages(), vec![3, 0, 308]);
    }
}
