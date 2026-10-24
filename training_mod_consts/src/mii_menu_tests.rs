use super::*;
use mii::inventory::{Appearance, Canonical, MiiClass, CHARINFO_LEN};
use mii::selection::{Preferences, Role, CLASSES};

fn identity(class: MiiClass, voice: u8) -> Canonical {
    Canonical {
        class,
        appearance: Appearance {
            charinfo_masked: [0; CHARINFO_LEN],
            specials: [0, 1, 2, 0],
            colour: 2,
            voice,
            outfit: 0x1cce3b03f5,
            hat: 0x0fa9efa5ea,
            chara: class.chara_hash40(),
        },
    }
}

fn six_preferences() -> Preferences {
    let mut prefs = Preferences::default();
    for (role, voice) in [(Role::Player, 37), (Role::Cpu, 3)] {
        for class in CLASSES {
            prefs = prefs
                .with(role, class, Some(identity(class, voice)))
                .unwrap();
        }
    }
    prefs
}

fn editor(envelope: &MenuJsonStruct) -> App<'static> {
    let mut app = unsafe { create_app() };
    app.mii_preferences = envelope.mii_preferences.clone();
    app.serialized_default_settings = serde_json::to_string(&envelope.defaults_menu).unwrap();
    app.open_settings_for_player(&serde_json::to_string(&envelope.menu).unwrap(), None);
    app
}

#[test]
fn old_menu_envelope_roundtrips_without_mii_preferences_or_new_controls() {
    let old = serde_json::json!({"menu":BASE_MENU,"defaults_menu":BASE_MENU});
    let loaded: MenuJsonStruct = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(loaded.mii_preferences, Preferences::default());
    let mut app = editor(&loaded);
    app.save_settings();
    let stored: serde_json::Value =
        serde_json::from_str(&app.get_serialized_settings_with_defaults()).unwrap();
    // App emits existing UI-only named_macro_* entries. Compare at the actual product
    // deserialization boundary, where those entries are intentionally ignored.
    let parsed: MenuJsonStruct = serde_json::from_value(stored).unwrap();
    assert_eq!(serde_json::to_value(parsed.menu).unwrap(), old["menu"]);
    assert_eq!(
        serde_json::to_value(parsed.defaults_menu).unwrap(),
        old["defaults_menu"]
    );
    assert_eq!(parsed.mii_preferences, Preferences::default());
    let mii_kinds: Vec<u32> = app
        .roster_kinds
        .iter()
        .copied()
        .filter(|kind| MiiClass::from_kind(*kind).is_some())
        .collect();
    #[cfg(not(feature = "mii-saved-alpha"))]
    assert!(mii_kinds.is_empty());
    #[cfg(feature = "mii-saved-alpha")]
    assert_eq!(mii_kinds, vec![0x49, 0x48, 0x4a]);
    let extras = app
        .tabs
        .iter()
        .find(|tab| tab.id == "character_extras")
        .unwrap();
    assert_eq!(extras.submenus.len(), 3);
}

#[test]
fn unrelated_menu_edits_and_defaults_preserve_all_six_identity_selectors() {
    let prefs = six_preferences();
    let envelope: MenuJsonStruct = serde_json::from_value(serde_json::json!({
        "menu":BASE_MENU, "defaults_menu":BASE_MENU, "mii_preferences":prefs,
    }))
    .unwrap();
    let mut app = editor(&envelope);
    let before = serde_json::to_vec(&app.mii_preferences).unwrap();
    let mut changed = BASE_MENU;
    changed.lobby_practice = OnOff::ON;
    app.update_one_from_json(&serde_json::to_string(&changed).unwrap(), "lobby_practice");
    app.save_settings();
    let saved: MenuJsonStruct =
        serde_json::from_str(&app.get_serialized_settings_with_defaults()).unwrap();
    assert_eq!(saved.menu.lobby_practice, OnOff::ON);
    assert_eq!(saved.mii_preferences, prefs);
    let mut reopened = editor(&saved);
    reopened.load_defaults();
    reopened.save_settings();
    let restored: MenuJsonStruct =
        serde_json::from_str(&reopened.get_serialized_settings_with_defaults()).unwrap();
    assert_eq!(restored.menu.lobby_practice, BASE_MENU.lobby_practice);
    assert_eq!(
        serde_json::to_vec(&restored.mii_preferences).unwrap(),
        before
    );
    // Identity metadata stays outside the numeric toggle map.
    let settings: serde_json::Value =
        serde_json::from_str(&app.current_settings_to_json()).unwrap();
    assert!(settings.get("mii_preferences").is_none());
}

#[test]
fn changing_one_role_class_preserves_other_five_selectors() {
    let before = six_preferences();
    let changed = before
        .clone()
        .with(
            Role::Player,
            MiiClass::Swordfighter,
            Some(identity(MiiClass::Swordfighter, 8)),
        )
        .unwrap();
    let envelope =
        serde_json::json!({"menu":BASE_MENU,"defaults_menu":BASE_MENU,"mii_preferences":changed});
    let loaded: MenuJsonStruct = serde_json::from_value(envelope).unwrap();
    for role in [Role::Player, Role::Cpu] {
        for class in CLASSES {
            if role == Role::Player && class == MiiClass::Swordfighter {
                assert_ne!(
                    loaded.mii_preferences.get(role, class),
                    before.get(role, class)
                );
            } else {
                assert_eq!(
                    loaded.mii_preferences.get(role, class),
                    before.get(role, class)
                );
            }
        }
    }
}

#[test]
fn invalid_mii_selector_preserves_menu_and_other_five_preferences() {
    let prefs = six_preferences();
    let mut menu = BASE_MENU;
    menu.lobby_practice = OnOff::ON;
    for bad in [
        serde_json::json!("invalid identity"),
        serde_json::to_value(identity(MiiClass::Gunner, 9)).unwrap(),
    ] {
        let mut metadata = serde_json::to_value(&prefs).unwrap();
        metadata["player"][1] = bad;
        metadata["future_metadata"] = serde_json::json!({"version": 1});
        let envelope =
            serde_json::json!({"menu":menu,"defaults_menu":BASE_MENU,"mii_preferences":metadata});
        let loaded: MenuJsonStruct = serde_json::from_value(envelope).unwrap();
        assert_eq!(
            serde_json::to_value(loaded.menu).unwrap(),
            serde_json::to_value(menu).unwrap()
        );
        assert_eq!(
            serde_json::to_value(loaded.defaults_menu).unwrap(),
            serde_json::to_value(BASE_MENU).unwrap()
        );
        assert_eq!(
            loaded
                .mii_preferences
                .get(Role::Player, MiiClass::Swordfighter),
            None
        );
        for role in [Role::Player, Role::Cpu] {
            for class in CLASSES {
                if role != Role::Player || class != MiiClass::Swordfighter {
                    assert_eq!(
                        loaded.mii_preferences.get(role, class),
                        prefs.get(role, class)
                    );
                }
            }
        }
        let mut app = editor(&loaded);
        app.save_settings();
        let reopened: MenuJsonStruct =
            serde_json::from_str(&app.get_serialized_settings_with_defaults()).unwrap();
        assert_eq!(reopened.mii_preferences, loaded.mii_preferences);
        assert_eq!(reopened.menu.lobby_practice, OnOff::ON);
    }
}

#[test]
fn optional_mii_metadata_does_not_invalidate_numeric_settings() {
    for metadata in [
        serde_json::Value::Null,
        serde_json::json!("bad"),
        serde_json::json!({"future":true}),
    ] {
        let loaded: MenuJsonStruct = serde_json::from_value(serde_json::json!({
            "menu":BASE_MENU,"defaults_menu":BASE_MENU,"mii_preferences":metadata,
        }))
        .unwrap();
        assert_eq!(loaded.mii_preferences, Preferences::default());
        assert_eq!(
            serde_json::to_value(loaded.menu).unwrap(),
            serde_json::to_value(BASE_MENU).unwrap()
        );
    }
    let prefs = six_preferences();
    let mut metadata = serde_json::to_value(&prefs).unwrap();
    metadata["future"] = serde_json::json!(true);
    let loaded: MenuJsonStruct = serde_json::from_value(serde_json::json!({
        "menu":BASE_MENU,"defaults_menu":BASE_MENU,"mii_preferences":metadata,
    }))
    .unwrap();
    assert_eq!(loaded.mii_preferences, prefs);
}

#[cfg(not(feature = "mii-saved-alpha"))]
#[test]
fn alpha_downgrade_preserves_nondefault_menu_defaults_and_six_preferences() {
    for tail in [1usize, 3] {
        let prefs = six_preferences();
        let mut menu = BASE_MENU;
        menu.lobby_practice = OnOff::ON;
        menu.save_state_enable = OnOff::OFF;
        menu.player_character = TrioFighter::PALUTENA;
        menu.cpu_character = TrioFighter::BOWSER;
        menu.player_roster =
            OrderedFighterRoster::from_fighters(&[TrioFighter::BAYONETTA, TrioFighter::KIRBY]).unwrap();
        let mut defaults = menu;
        defaults.player_character = TrioFighter::LUCINA;
        defaults.save_state_enable = OnOff::ON;
        let mut wire =
            serde_json::json!({"menu":menu,"defaults_menu":defaults,"mii_preferences":prefs});
        // Exact alpha lengths for every affected field, including custom defaults.
        for section in ["menu", "defaults_menu"] {
            for field in [
                "player_character",
                "cpu_character",
                "player_roster",
                "cpu_roster",
                "player_kirby_hats",
                "cpu_kirby_hats",
            ] {
                wire[section][field]
                    .as_array_mut()
                    .unwrap()
                    .extend(std::iter::repeat_n(serde_json::json!(0), tail));
            }
        }
        let mut mii_only = vec![0u8; 86 + tail];
        mii_only[85 + tail] = 1;
        wire["menu"]["player_character"] = serde_json::json!(mii_only);
        let mut order = vec![0u8; 87 + tail];
        order[trio::CHOICES
            .iter()
            .position(|(kind, _)| *kind == TrioFighter::BAYONETTA.kind())
            .unwrap()
            + 1] = 1;
        order[86 + tail] = 2; // Last appended Mii between the two supported choices.
        order[7] = 3;
        wire["menu"]["player_roster"] = serde_json::json!(order);
        let mut only = vec![0u8; 87 + tail];
        only[86 + tail] = 1;
        wire["defaults_menu"]["cpu_roster"] = serde_json::json!(only);
        let loaded: MenuJsonStruct = serde_json::from_value(wire.clone()).unwrap();
        menu.player_character = TrioFighter::MARIO;
        defaults.cpu_roster = OrderedFighterRoster::from_fighters(&[TrioFighter::MARIO]).unwrap();
        assert_eq!(
            serde_json::to_value(loaded.menu).unwrap(),
            serde_json::to_value(menu).unwrap()
        );
        assert_eq!(
            serde_json::to_value(loaded.defaults_menu).unwrap(),
            serde_json::to_value(defaults).unwrap()
        );
        assert_eq!(loaded.mii_preferences, prefs);
        // Validation covers the appended one-hot byte before filtering.
        wire["menu"]["player_character"][85 + tail] = serde_json::json!(2);
        assert!(serde_json::from_value::<MenuJsonStruct>(wire.clone()).is_err());
        wire["menu"]["player_character"][85 + tail] = serde_json::json!(1);
        wire["menu"]["player_character"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!(0));
        assert!(serde_json::from_value::<MenuJsonStruct>(wire).is_err());
    }
}
