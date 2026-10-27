//! Lobby-stage preferences. Catalog indices are stable; ranks store selection order.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use training_mod_tui::{
    StatefulTable, SubMenu, SubMenuType, Toggle, NX_SUBMENU_COLUMNS, NX_SUBMENU_ROWS,
};
pub use training_mod_tui::stage_forms::{Form, Forms, Selection};

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

// Forms persists one entry for each stable catalog index.
const _: () = assert!(STAGES.len() == 117);

// Explicit [normal, omega, battlefield] native IDs from the recorded 13.0.5
// table (0x454a7e8, stride72), joined by base_stage_id and table form, not offsets.
// Special stages below have missing forms; do not infer IDs from adjacency.
const THREE_FORM_STAGES: &[[u32; 3]] = &[
    [5, 6, 7], [8, 9, 10], [11, 12, 13], [14, 15, 16], [17, 18, 19],
    [20, 21, 22], [23, 24, 25], [26, 27, 28], [29, 30, 31], [32, 33, 34],
    [35, 36, 37], [38, 39, 40], [41, 42, 43], [44, 45, 46], [47, 48, 49],
    [50, 51, 52], [53, 54, 55], [56, 57, 58], [59, 60, 61], [62, 63, 64],
    [65, 66, 67], [68, 69, 70], [71, 72, 73], [74, 75, 76], [77, 78, 79],
    [80, 81, 82], [83, 84, 85], [86, 87, 88], [89, 90, 91], [92, 93, 94],
    [95, 96, 97], [98, 99, 100], [101, 102, 103], [104, 105, 106],
    [107, 108, 109], [110, 111, 112], [113, 114, 115], [116, 117, 118],
    [119, 120, 121], [122, 123, 124], [125, 126, 127], [128, 129, 130],
    [131, 132, 133], [134, 135, 136], [137, 138, 139], [140, 141, 142],
    [143, 144, 145], [146, 147, 148], [149, 150, 151], [152, 153, 154],
    [155, 156, 157], [158, 159, 160], [161, 162, 163], [164, 165, 166],
    [167, 168, 169], [170, 171, 172], [173, 174, 175], [176, 177, 178],
    [179, 180, 181], [182, 183, 184], [185, 186, 187], [188, 189, 190],
    [191, 192, 193], [197, 198, 199], [200, 201, 202], [203, 204, 205],
    [206, 207, 208], [209, 210, 211], [212, 213, 214], [215, 216, 217],
    [218, 219, 220], [221, 222, 223], [224, 225, 226], [227, 228, 229],
    [230, 231, 232], [233, 234, 235], [236, 237, 238], [239, 240, 241],
    [242, 243, 244], [245, 246, 247], [248, 249, 250], [251, 252, 253],
    [254, 255, 256], [257, 258, 259], [260, 261, 262], [263, 264, 265],
    [266, 267, 268], [269, 270, 271], [272, 273, 274], [275, 276, 277],
    [278, 279, 280], [281, 282, 283], [284, 285, 286], [287, 288, 289],
    [290, 291, 292], [293, 294, 295], [296, 297, 298], [299, 300, 301],
    [302, 303, 304], [305, 306, 307], [328, 329, 330], [332, 333, 334],
    [335, 336, 337], [338, 339, 340], [341, 342, 343], [344, 345, 346],
    [348, 349, 350], [351, 352, 353], [355, 356, 357], [358, 359, 360],
    [361, 362, 363],
];

pub fn native_forms(stage: u32) -> [Option<u32>; 3] {
    match stage {
        0 => [Some(0), Some(1), None],
        3 => [Some(3), None, Some(4)],
        2 | 347 | 308 | 311 => [Some(stage), None, None],
        _ => THREE_FORM_STAGES.iter().find(|row| row[0] == stage)
            .map_or([None; 3], |row| row.map(Some)),
    }
}

pub fn native_stage(selection: Selection) -> Result<u32, &'static str> {
    if !STAGES.iter().any(|(stage, _)| *stage == selection.stage) {
        return Err("stage outside native selectable catalog");
    }
    native_forms(selection.stage)[selection.form as usize]
        .ok_or("form unavailable for this stage")
}

pub fn form_catalog() -> Vec<[Option<u32>; 3]> {
    STAGES.iter().map(|(stage, _)| native_forms(*stage)).collect()
}

/// Validate all stored forms, including unranked cells, before publishing settings.
pub fn validate_forms(forms: &Forms) -> Result<(), &'static str> {
    if forms.as_slice().len() != STAGES.len() { return Err("stage form catalog mismatch"); }
    for ((stage, _), form) in STAGES.iter().zip(forms.as_slice()) {
        native_stage(Selection { stage: *stage, form: *form })?;
    }
    Ok(())
}

pub fn validate_preferences(prefs: &training_mod_tui::stage_menu::Preferences) -> Result<(), &'static str> {
    native_stage(Selection { stage: prefs.direct_stage, form: prefs.direct_form })?;
    validate_forms(&prefs.rotation_forms)
}

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

    pub fn selections(self, forms: &Forms) -> Result<Vec<Selection>, &'static str> {
        validate_forms(forms)?;
        self.stages().into_iter().map(|stage| {
            let index = STAGES.iter().position(|(id, _)| *id == stage)
                .ok_or("stage outside native selectable catalog")?;
            Ok(Selection { stage, form: forms.get(index).ok_or("stage form index outside catalog")? })
        }).collect()
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
    fn recorded_native_forms_include_exceptions_and_exclude_internal_neighbors() {
        // Independent recorded fixture facts: table rows0/1,3/4,95/96/97,
        // 328/329/330 and361/362/363, plus the four form0-only special rows.
        for (stage, form, expected) in [
            (0, Form::Omega, 1), (3, Form::Normal, 3),
            (95, Form::Omega, 96), (95, Form::Normal, 95),
            (328, Form::Omega, 329), (361, Form::Omega, 362),
            (3, Form::Battlefield, 4), (95, Form::Battlefield, 97),
            (328, Form::Battlefield, 330), (361, Form::Battlefield, 363),
        ] {
            assert_eq!(native_stage(Selection { stage, form }), Ok(expected));
        }
        for stage in [2, 347, 308, 311] {
            assert_eq!(native_stage(Selection::normal(stage)), Ok(stage));
            for form in [Form::Omega, Form::Battlefield] {
                assert_eq!(native_stage(Selection { stage, form }), Err("form unavailable for this stage"));
            }
        }
        assert_eq!(native_forms(3), [Some(3), None, Some(4)]);
        assert_eq!(native_forms(95), [Some(95), Some(96), Some(97)]);
        assert_eq!(form_catalog()[4], [Some(3), None, Some(4)]);
        assert_eq!(Form::ALL, [Form::Normal, Form::Omega, Form::Battlefield]);
        assert!(native_stage(Selection { stage: 0, form: Form::Battlefield }).is_err());
        assert!(native_stage(Selection { stage: 3, form: Form::Omega }).is_err());
        for stage in [1, 4, 194, 309, 310, 327, 331, 354, 364, u32::MAX] {
            assert!(native_stage(Selection::normal(stage)).is_err());
        }
        assert_eq!(Forms::NORMAL.as_slice().len(), STAGES.len());
        for (stage, _) in STAGES {
            assert_eq!(native_stage(Selection::normal(*stage)), Ok(*stage));
        }
    }

    /// Compare the production mapping to the entire independently recorded
    /// native table. The private artifact is supplied by the bounded verifier;
    /// neither game bytes nor a copy of the production mapping is a fixture.
    #[test]
    #[ignore = "requires STAGE185_NATIVE_CATALOG pointing to the recorded private catalog"]
    fn all_forms_match_recorded_native_catalog() {
        use std::convert::TryFrom;
        let path = std::env::var_os("STAGE185_NATIVE_CATALOG")
            .expect("set STAGE185_NATIVE_CATALOG to stage-id-asset-catalog.json");
        let bytes = std::fs::read(path).expect("read recorded native catalog");
        let catalog: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(catalog["elf_sha256"], "0789f0a4ae9652f335eca5eabc7df3e58b0954012a0bebdea374f317706536c2");
        assert_eq!(catalog["table_va"], "0x454a7e8");
        assert_eq!(catalog["stride"], 72);
        let rows = catalog["records"].as_array().unwrap();
        assert_eq!(rows.len(), 364);
        assert_eq!(catalog["count"], rows.len());
        let mut ids = std::collections::BTreeSet::new();
        let mut recorded = std::collections::BTreeMap::new();
        for row in rows {
            let id = u32::try_from(row["stage_id"].as_u64().unwrap()).unwrap();
            let form = usize::try_from(row["form"].as_u64().unwrap()).unwrap();
            assert!(ids.insert(id), "duplicate recorded native ID {}", id);
            assert!(form < Form::ALL.len());
            if row["base_stage_id"].is_null() {
                assert_eq!(id, 327); // Recorded unresolved TestStage, not selectable.
                assert!(!STAGES.iter().any(|(stage, _)| *stage == id));
                continue;
            }
            let base = u32::try_from(row["base_stage_id"].as_u64().unwrap()).unwrap();
            assert!(recorded.insert((base, form), id).is_none(), "duplicate recorded stage/form");
        }
        assert!(ids.into_iter().eq(0..364));
        for (stage, _) in STAGES {
            let expected: [Option<u32>; 3] = std::array::from_fn(|form| recorded.get(&(*stage, form)).copied());
            assert_eq!(native_forms(*stage), expected, "base stage {stage}");
            for form in Form::ALL {
                let resolved = native_stage(Selection { stage: *stage, form });
                assert_eq!(resolved.ok(), expected[form as usize], "stage {stage}, {form:?}");
            }
        }
    }

    fn with_form(mut forms: Forms, stage: u32, form: Form) -> Forms {
        forms.set(STAGES.iter().position(|(id, _)| *id == stage).unwrap(), form).unwrap();
        forms
    }

    #[test]
    fn legacy_rank_formats_keep_identity_order_when_forms_are_added() {
        let mut old_full = vec![0u8; 117];
        old_full[116] = 1; // Hollow Bastion, before Training was appended.
        old_full[1] = 2; // Waiting Room.
        for (value, expected) in [
            (serde_json::json!([0, 0, 2, 1]), vec![3, 0]),
            (serde_json::json!(old_full), vec![361, 311]),
        ] {
            let order: LobbyStageOrder = serde_json::from_value(value).unwrap();
            let ranks_before = serde_json::to_value(order).unwrap();
            let normal = order.selections(&Forms::NORMAL).unwrap();
            assert_eq!(normal.iter().map(|s| s.stage).collect::<Vec<_>>(), expected);
            let forms = with_form(with_form(Forms::NORMAL, 0, Form::Omega), 361, Form::Battlefield);
            let selected = order.selections(&forms).unwrap();
            assert_eq!(selected.iter().map(|s| s.stage).collect::<Vec<_>>(), expected);
            assert_eq!(serde_json::to_value(order).unwrap(), ranks_before);
            let restored_forms: Forms = serde_json::from_value(serde_json::to_value(forms).unwrap()).unwrap();
            assert_eq!(order.selections(&restored_forms).unwrap(), selected);
        }
        let all: LobbyStageOrder = serde_json::from_str("[1,0,0,0]").unwrap();
        let forms = with_form(Forms::NORMAL, 95, Form::Omega);
        let selected = all.selections(&forms).unwrap();
        assert_eq!(selected.len(), STAGES.len());
        assert!(selected.contains(&Selection { stage: 95, form: Form::Omega }));
        assert_eq!(selected.last(), Some(&Selection::normal(308)));
        // An invalid unranked form must not wait to fail until it is selected.
        let invalid = with_form(Forms::NORMAL, 308, Form::Omega);
        assert!(LobbyStageOrder::NATIVE.selections(&invalid).is_err());
    }

    fn settings_with_preferences(preferences: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"menu": crate::BASE_MENU, "defaults_menu": crate::BASE_MENU,
            "stage_preferences": preferences})
    }

    #[test]
    fn settings_ingress_migrates_absent_forms_and_rejects_present_invalid_forms() {
        // Exercise the real MenuJsonStruct deserializer, not only a helper.
        let legacy: crate::MenuJsonStruct = serde_json::from_value(settings_with_preferences(
            serde_json::json!({"mode":"direct","direct_stage":95}))).unwrap();
        assert_eq!(legacy.stage_preferences.direct_form, Form::Normal);
        assert_eq!(legacy.stage_preferences.rotation_forms, Forms::NORMAL);
        let mut valid = serde_json::json!({"mode":"direct","direct_stage":0,"direct_form":"final_destination",
            "rotation_forms":with_form(Forms::NORMAL, 361, Form::Battlefield)});
        let selected: crate::MenuJsonStruct = serde_json::from_value(settings_with_preferences(valid.clone())).unwrap();
        assert_eq!(selected.stage_preferences.direct_form, Form::Omega);
        let saved = serde_json::to_value(&selected).unwrap();
        assert_eq!(saved["stage_preferences"]["direct_form"], "omega");
        let restored: crate::MenuJsonStruct = serde_json::from_value(saved).unwrap();
        assert_eq!(restored.stage_preferences, selected.stage_preferences);
        for stage in [2, 308, 311, 347, 1, 364] {
            valid["direct_stage"] = serde_json::json!(stage);
            assert!(serde_json::from_value::<crate::MenuJsonStruct>(settings_with_preferences(valid.clone())).is_err());
        }
        valid["direct_stage"] = serde_json::json!(0);
        for value in [serde_json::json!("unknown"), serde_json::json!(3), serde_json::Value::Null] {
            valid["direct_form"] = value;
            assert!(serde_json::from_value::<crate::MenuJsonStruct>(settings_with_preferences(valid.clone())).is_err());
        }
        valid["direct_form"] = serde_json::json!("omega");
        for bad in [serde_json::json!([]), serde_json::Value::Null,
            serde_json::to_value(with_form(Forms::NORMAL, 308, Form::Omega)).unwrap(),
            serde_json::to_value(with_form(Forms::NORMAL, 308, Form::Battlefield)).unwrap()] {
            valid["rotation_forms"] = bad;
            assert!(serde_json::from_value::<crate::MenuJsonStruct>(settings_with_preferences(valid.clone())).is_err());
        }
        let menu = serde_json::to_string(&crate::BASE_MENU).unwrap();
        for prefs in [r#"{"direct_form":"normal","direct_form":"omega"}"#,
            r#"{"direct_forms":"omega"}"#] {
            let text = format!(r#"{{"menu":{menu},"defaults_menu":{menu},"stage_preferences":{prefs}}}"#);
            assert!(serde_json::from_str::<crate::MenuJsonStruct>(&text).is_err());
        }
    }

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
            ..Preferences::default()
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
            assert_eq!(app.take_pending_stage(), Some(Selection::normal(308)));
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

    fn select_stage(app: &mut training_mod_tui::App<'_>, stage: u32) {
        let index = STAGES.iter().position(|(id, _)| *id == stage).unwrap() + 1;
        let cols = app.selected_submenu().toggles.cols;
        app.selected_submenu().toggles.select(index / cols, index % cols);
    }

    #[test]
    fn form_edit_uses_the_production_direct_close_intent_and_keeps_rank_order() {
        for ordinary_training in [false, true] {
            for (presses, form) in [(0, Form::Normal), (1, Form::Omega), (2, Form::Battlefield)] {
                let mut app = stage_app(true, ordinary_training);
                let preferences = app.stage_preferences;
                select_stage(&mut app, 95);
                for _ in 0..presses { app.on_y(); }
                assert!(app.stage_help().contains(form.label()));
                assert!(app.take_pending_stage().is_none());
                app.on_a();
                assert_eq!(app.take_pending_stage(), Some(Selection { stage: 95, form }));
                assert_eq!(app.take_pending_stage(), None);
                let saved: crate::MenuJsonStruct = serde_json::from_str(&app.get_serialized_settings_with_defaults()).unwrap();
                assert_eq!(saved.menu.lobby_stages.stages(), vec![3, 0]);
                assert_eq!(saved.stage_preferences.rotation_forms, preferences.rotation_forms);
                if ordinary_training {
                    assert_eq!(saved.stage_preferences, preferences);
                } else {
                    assert_eq!(saved.stage_preferences.direct_stage, 95);
                    assert_eq!(saved.stage_preferences.direct_form, form);
                }
            }
        }
        for stage in [2, 308, 311, 347] {
            let mut app = stage_app(true, false);
            select_stage(&mut app, stage);
            app.on_y();
            app.on_a();
            assert_eq!(app.take_pending_stage(), Some(Selection::normal(stage)));
        }
    }

    #[test]
    fn rotation_form_edit_survives_settings_restore_without_reordering_or_live_request() {
        let mut app = stage_app(true, false);
        app.on_x();
        select_stage(&mut app, 0);
        app.on_y();
        app.on_b();
        app.on_start();
        assert!(app.take_pending_stage().is_none());
        let saved: crate::MenuJsonStruct = serde_json::from_str(&app.get_serialized_settings_with_defaults()).unwrap();
        assert_eq!(saved.menu.lobby_stages.stages(), vec![3, 0]);
        assert_eq!(saved.menu.lobby_stages.selections(&saved.stage_preferences.rotation_forms).unwrap(),
            vec![Selection::normal(3), Selection { stage: 0, form: Form::Omega }]);
        assert_eq!(saved.stage_preferences.direct_stage, 3);
        assert_eq!(saved.stage_preferences.direct_form, Form::Normal);
    }

    #[test]
    fn direct_drafts_survive_paging_but_cannot_replay_on_cancel_or_inherit_rotation_forms() {
        let mut app = stage_app(true, false);
        app.on_x(); // Edit the future rotation's Smashville form twice.
        select_stage(&mut app, 95);
        app.on_y(); app.on_y();
        assert_eq!(app.stage_selection(), Some(Selection { stage: 95, form: Form::Battlefield }));
        let rotation = app.stage_preferences.rotation_forms;
        app.on_x(); // Direct draft starts independently in Normal.
        assert_eq!(app.stage_selection(), Some(Selection::normal(95)));
        app.on_y();
        app.on_r(); app.on_l();
        assert_eq!(app.stage_selection(), Some(Selection { stage: 95, form: Form::Omega }));
        assert_eq!(app.stage_preferences.rotation_forms, rotation);
        app.on_b(); app.on_start();
        assert!(app.take_pending_stage().is_none());
        let saved: crate::MenuJsonStruct = serde_json::from_str(&app.get_serialized_settings_with_defaults()).unwrap();
        assert_eq!(saved.stage_preferences.direct_stage, 3);
        assert_eq!(saved.stage_preferences.direct_form, Form::Normal);
        assert_eq!(saved.stage_preferences.rotation_forms, rotation);
        app.open_stage_menu(true, saved.stage_preferences);
        app.page = training_mod_tui::AppPage::TOGGLE;
        select_stage(&mut app, 95);
        assert_eq!(app.stage_selection(), Some(Selection::normal(95)));
        assert!(app.take_pending_stage().is_none());
        let lobby_fixed = training_mod_tui::stage_menu::Preferences {
            direct_stage: 95, direct_form: Form::Battlefield, ..saved.stage_preferences
        };
        app.open_stage_menu_in_context(true, lobby_fixed, true);
        // Training's manual draft is not evidence of the lobby's saved form.
        assert_eq!(app.stage_selection(), Some(Selection::normal(95)));
        app.on_a();
        assert_eq!(app.take_pending_stage(), Some(Selection::normal(95)));
        assert_eq!(app.stage_preferences, lobby_fixed);
    }

    #[test]
    fn all_and_invalid_catalog_pairs_cannot_emit_a_direct_intent() {
        let mut app = stage_app(true, false);
        app.selected_submenu().toggles.select(0, 0);
        let before = app.stage_preferences;
        app.on_y(); app.on_a();
        assert!(app.stage_page_active());
        assert!(app.take_pending_stage().is_none());
        assert_eq!(app.stage_preferences, before);

        // Even a programmatic bypass of the settings parser must be refused by UI.
        let mut invalid = before;
        invalid.direct_stage = 308;
        invalid.direct_form = Form::Battlefield;
        app.open_stage_menu(true, invalid);
        select_stage(&mut app, 308);
        app.on_a();
        assert!(app.stage_page_active());
        assert!(app.take_pending_stage().is_none());

        app.open_stage_menu(true, before);
        select_stage(&mut app, 95);
        app.configure_stage_forms(Vec::new());
        app.on_a();
        assert!(app.stage_page_active());
        assert!(app.take_pending_stage().is_none());
    }

    #[test]
    fn non_alpha_rotation_edits_forms_without_enabling_direct_requests() {
        let mut app = stage_app(false, false);
        select_stage(&mut app, 3);
        app.on_y(); // Normal -> Battlefield, existing FD-first rank retained.
        assert_eq!(app.stage_selection(), Some(Selection { stage: 3, form: Form::Battlefield }));
        app.on_b(); app.on_start();
        assert!(app.take_pending_stage().is_none());
        let saved: crate::MenuJsonStruct = serde_json::from_str(&app.get_serialized_settings_with_defaults()).unwrap();
        assert_eq!(saved.menu.lobby_stages.selections(&saved.stage_preferences.rotation_forms).unwrap(),
            vec![Selection { stage: 3, form: Form::Battlefield }, Selection::normal(0)]);
    }
}
