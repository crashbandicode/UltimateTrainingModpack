//! Production JSON ingress for `TrainingModpackMenu`.
//!
//! Builds 20-29 stored each cycle roster as fixed slots (`trio_fighter_1..3`,
//! `player_roster_4..8`, `cpu_roster_1..8`) plus a one-hot size. The ordered
//! rosters are now the only stored form: an old file is imported once here, the
//! slot keys are never written again, and release 29 still reads the result
//! because it already prefers a present `player_roster` / `cpu_roster`.

use std::fmt;

use serde::de::{self, DeserializeOwned, Deserializer, MapAccess, Visitor};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::{OrderedFighterRoster, TrainingModpackMenu, TrioFighter};

/// Optional identity metadata must not invalidate the independent numeric menu.
/// Keep each valid selector, ignore unknown fields, and leave only invalid selectors unset.
pub fn deserialize_mii_preferences<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<crate::mii::selection::Preferences, D::Error> {
    use crate::mii::{inventory::Canonical, selection::{Preferences, Role, CLASSES}};
    let value = Value::deserialize(deserializer)?;
    let mut preferences = Preferences::default();
    for (role, key) in [(Role::Player, "player"), (Role::Cpu, "cpu")] {
        if let Some(selectors) = value.get(key).and_then(Value::as_array) {
            for (class, selector) in CLASSES.iter().zip(selectors) {
                if let Ok(identity) = serde_json::from_value::<Canonical>(selector.clone()) {
                    if let Ok(updated) = preferences.clone().with(role, *class, Some(identity)) {
                        preferences = updated;
                    }
                }
            }
        }
    }
    Ok(preferences)
}

/// Absent legacy metadata defaults to Normal at the envelope/field boundary.
/// Present invalid selections fail the whole parse before any settings publication.
pub fn deserialize_stage_preferences<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<training_mod_tui::stage_menu::Preferences, D::Error> {
    let prefs = training_mod_tui::stage_menu::Preferences::deserialize(deserializer)?;
    crate::lobby_stages::validate_preferences(&prefs).map_err(de::Error::custom)?;
    Ok(prefs)
}

/// File ingress may recover independent settings from unsupported stage metadata.
/// Live menu responses still use the strict `MenuJsonStruct` parser directly.
#[derive(Debug)]
pub struct LoadedMenuFile {
    pub settings: crate::MenuJsonStruct,
    pub rejected_stage_preferences: Option<String>,
}

pub fn parse_menu_file(contents: &str) -> Result<LoadedMenuFile, serde_json::Error> {
    match serde_json::from_str::<crate::MenuJsonStruct>(contents) {
        Ok(settings) => Ok(LoadedMenuFile { settings, rejected_stage_preferences: None }),
        Err(stage_error) => {
            // Re-parse the original text, not a Value round-trip: duplicate fields
            // and invalid numeric menus must remain errors, even during recovery.
            #[derive(Deserialize)]
            struct WithoutStage {
                #[serde(deserialize_with = "deserialize_menu")]
                menu: TrainingModpackMenu,
                #[serde(deserialize_with = "deserialize_menu")]
                defaults_menu: TrainingModpackMenu,
                #[serde(default, deserialize_with = "deserialize_mii_preferences")]
                mii_preferences: crate::mii::selection::Preferences,
                #[serde(default, rename = "stage_preferences")]
                _stage_preferences: Option<de::IgnoredAny>,
            }
            let recovered: WithoutStage = serde_json::from_str(contents)?;
            Ok(LoadedMenuFile {
                settings: crate::MenuJsonStruct {
                    menu: recovered.menu,
                    defaults_menu: recovered.defaults_menu,
                    mii_preferences: recovered.mii_preferences,
                    stage_preferences: Default::default(),
                },
                rejected_stage_preferences: Some(stage_error.to_string()),
            })
        }
    }
}

/// One role's retired fixed-slot keys and the value each slot had when absent.
struct LegacySlots {
    roster: &'static str,
    size: &'static str,
    default_size: usize,
    slots: [(&'static str, TrioFighter); 8],
}

static PLAYER: LegacySlots = LegacySlots {
    roster: "player_roster",
    size: "player_roster_size",
    default_size: 3,
    slots: [
        ("trio_fighter_1", TrioFighter::GAMEWATCH),
        ("trio_fighter_2", TrioFighter::PALUTENA),
        ("trio_fighter_3", TrioFighter::PIKACHU),
        ("player_roster_4", TrioFighter::KIRBY),
        ("player_roster_5", TrioFighter::BOWSER),
        ("player_roster_6", TrioFighter::BAYONETTA),
        ("player_roster_7", TrioFighter::MARIO),
        ("player_roster_8", TrioFighter::LUCINA),
    ],
};

static CPU: LegacySlots = LegacySlots {
    roster: "cpu_roster",
    size: "cpu_roster_size",
    default_size: 4,
    slots: [
        ("cpu_roster_1", TrioFighter::PIKACHU),
        ("cpu_roster_2", TrioFighter::KIRBY),
        ("cpu_roster_3", TrioFighter::BOWSER),
        ("cpu_roster_4", TrioFighter::BAYONETTA),
        ("cpu_roster_5", TrioFighter::MARIO),
        ("cpu_roster_6", TrioFighter::LUCINA),
        ("cpu_roster_7", TrioFighter::PALUTENA),
        ("cpu_roster_8", TrioFighter::GAMEWATCH),
    ],
};

/// `deserialize_with` target for both `MenuJsonStruct` fields. Everything except
/// the roster import is still the derived parser: missing upstream fields fail,
/// unknown keys are ignored, and the fork's live keys keep their field defaults.
pub fn deserialize_menu<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<TrainingModpackMenu, D::Error> {
    let mut fields = deserializer.deserialize_map(UniqueKeys)?;
    for legacy in [&PLAYER, &CPU] {
        let roster = import_roster(&mut fields, legacy).map_err(de::Error::custom)?;
        let value = serde_json::to_value(roster).map_err(de::Error::custom)?;
        fields.insert(legacy.roster.to_owned(), value);
    }
    TrainingModpackMenu::deserialize(Value::Object(fields)).map_err(de::Error::custom)
}

/// Every present slot and size value is validated exactly as the old struct
/// fields were, even when an ordered roster is present and wins. A present,
/// non-null ordered roster is kept as written; an absent or null one is built
/// from the first `size` slots, with absent slots taking the old defaults.
fn import_roster(
    fields: &mut Map<String, Value>,
    legacy: &LegacySlots,
) -> Result<OrderedFighterRoster, String> {
    let mut fighters = [TrioFighter::MARIO; 8];
    for (fighter, (key, default)) in fighters.iter_mut().zip(legacy.slots.iter()) {
        *fighter = match fields.remove(*key) {
            Some(value) => parse(key, value)?,
            None => *default,
        };
    }
    let size = match fields.remove(legacy.size) {
        Some(value) => parse::<LegacySize>(legacy.size, value)?.0,
        None => legacy.default_size,
    };
    match fields.remove(legacy.roster) {
        Some(value) if !value.is_null() => parse(legacy.roster, value),
        _ => OrderedFighterRoster::from_fighters(&fighters[..size])
            .ok_or_else(|| format!("{}: no supported fighters", legacy.roster)),
    }
}

fn parse<T: DeserializeOwned>(key: &str, value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| format!("{key}: {error}"))
}

/// Retired one-hot roster size: exactly one of eight.
struct LegacySize(usize);

impl<'de> Deserialize<'de> for LegacySize {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = <[u8; 8]>::deserialize(deserializer)?;
        if values.iter().any(|value| *value > 1)
            || values.iter().filter(|value| **value == 1).count() != 1
        {
            return Err(de::Error::custom("select exactly one CPU roster size"));
        }
        Ok(Self(
            values.iter().position(|value| *value == 1).unwrap() + 1,
        ))
    }
}

/// Collects the menu object while rejecting any repeated key. The derived parser
/// rejected repeated known fields; this is stricter only for repeated unknown
/// metadata, which a valid file never contains.
struct UniqueKeys;

impl<'de> Visitor<'de> for UniqueKeys {
    type Value = Map<String, Value>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a menu object without repeated keys")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut fields = Map::new();
        while let Some(key) = access.next_key::<String>()? {
            if fields.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate field `{key}`")));
            }
            let value = access.next_value::<Value>()?;
            fields.insert(key, value);
        }
        Ok(fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trio::CHOICES;
    use crate::TrioFighter as F;
    use crate::{
        MenuJsonStruct, BASE_MENU, DEFAULT_CPU_CYCLE, DEFAULT_CPU_ROSTER, DEFAULT_PLAYER_CYCLE,
    };
    use serde_json::json;
    use training_mod_tui::InputControl;

    /// The current stored form.
    fn modern() -> Value {
        serde_json::to_value(BASE_MENU).unwrap()
    }

    /// The current form with every fork key removed, as an upstream file has it.
    fn stripped() -> Value {
        let mut file = modern();
        file.as_object_mut().unwrap().retain(|key, _| {
            !["player_", "cpu_", "trio_", "lobby_"]
                .iter()
                .any(|prefix| key.starts_with(prefix))
        });
        file
    }

    fn with(mut file: Value, entries: Vec<(&str, Value)>) -> Value {
        for (key, value) in entries {
            file[key] = value;
        }
        file
    }

    fn one_hot(size: usize) -> Value {
        json!((1..=8).map(|n| u8::from(n == size)).collect::<Vec<_>>())
    }

    fn fighter(fighter: F) -> Value {
        serde_json::to_value(fighter).unwrap()
    }

    fn bits(ones: &[usize]) -> Value {
        json!((0..CHOICES.len())
            .map(|index| u8::from(ones.contains(&index)))
            .collect::<Vec<_>>())
    }

    /// Fixed-slot keys as builds 20-29 wrote them for one role.
    fn slots(legacy: &LegacySlots, chosen: &[F]) -> Vec<(&'static str, Value)> {
        let mut entries = vec![(legacy.size, one_hot(chosen.len()))];
        entries.extend(
            legacy
                .slots
                .iter()
                .zip(chosen)
                .map(|((key, _), chosen)| (*key, fighter(*chosen))),
        );
        entries
    }

    /// Production path: one JSON text through the derived `MenuJsonStruct` parser.
    fn ingress(menu: &Value, defaults: &Value) -> serde_json::Result<MenuJsonStruct> {
        serde_json::from_str(&json!({ "menu": menu, "defaults_menu": defaults }).to_string())
    }

    fn menu_of(file: &Value) -> TrainingModpackMenu {
        ingress(file, &modern()).unwrap().menu
    }

    fn rosters(menu: &TrainingModpackMenu) -> (Vec<F>, Vec<F>) {
        (menu.player_roster.fighters(), menu.cpu_roster.fighters())
    }

    #[test]
    fn fixed_slot_files_import_once_for_menu_and_defaults() {
        let mut player = slots(&PLAYER, &[F::BOWSER, F::KIRBY, F::BAYONETTA]);
        player.extend(slots(&CPU, &[F::MARIO, F::PIKACHU, F::MARIO, F::KIRBY]));
        let mut defaults = slots(&PLAYER, &[F::LUCINA]);
        defaults.extend(slots(&CPU, &[F::PALUTENA, F::GAMEWATCH]));
        let parsed = ingress(&with(stripped(), player), &with(stripped(), defaults)).unwrap();
        // Legacy order is kept and the repeated Mario is dropped, as before.
        assert_eq!(
            rosters(&parsed.menu),
            (
                vec![F::BOWSER, F::KIRBY, F::BAYONETTA],
                vec![F::MARIO, F::PIKACHU, F::KIRBY]
            )
        );
        assert_eq!(
            rosters(&parsed.defaults_menu),
            (vec![F::LUCINA], vec![F::PALUTENA, F::GAMEWATCH])
        );
        // Live keys the old file lacked keep their defaults; unrelated settings survive.
        let menu = parsed.menu;
        assert_eq!(menu.player_character, F::GAMEWATCH);
        assert_eq!(menu.cpu_character, F::PIKACHU);
        assert_eq!(menu.trio_practice, BASE_MENU.trio_practice);
        assert_eq!(menu.save_state_enable, BASE_MENU.save_state_enable);
        assert_eq!(menu.player_cycle, DEFAULT_PLAYER_CYCLE);
        assert_eq!(menu.cpu_cycle, DEFAULT_CPU_CYCLE);
        assert_ne!(menu.player_cycle, menu.cpu_cycle);
        assert_eq!((menu.player_cycle.ZL, menu.player_cycle.DPAD_RIGHT), (1, 1));
        assert_eq!((menu.cpu_cycle.ZL, menu.cpu_cycle.DPAD_LEFT), (1, 1));
    }

    #[test]
    fn absent_slots_and_sizes_take_the_old_defaults() {
        let trio = [
            ("trio_fighter_1", fighter(F::MARIO)),
            ("trio_fighter_2", fighter(F::LUCINA)),
            ("trio_fighter_3", fighter(F::PALUTENA)),
        ];
        let cases = [
            // An upstream file: shipped defaults.
            (vec![], vec![F::GAMEWATCH, F::PALUTENA, F::PIKACHU]),
            // A build-2x trio without a size or slots 4-8: exactly those three.
            (trio.to_vec(), vec![F::MARIO, F::LUCINA, F::PALUTENA]),
            // A size beyond the present slots fills from the old slot defaults.
            (
                [trio.to_vec(), vec![("player_roster_size", one_hot(5))]].concat(),
                vec![F::MARIO, F::LUCINA, F::PALUTENA, F::KIRBY, F::BOWSER],
            ),
            // A smaller size ignores the later present slots.
            (
                [trio.to_vec(), vec![("player_roster_size", one_hot(1))]].concat(),
                vec![F::MARIO],
            ),
        ];
        for (entries, expected) in cases {
            let menu = menu_of(&with(stripped(), entries));
            assert_eq!(menu.player_roster.fighters(), expected);
            assert_eq!(menu.cpu_roster, DEFAULT_CPU_ROSTER);
        }
    }

    #[test]
    fn ordered_roster_wins_when_present_and_imports_slots_when_absent_or_null() {
        let legacy = with(stripped(), slots(&PLAYER, &[F::LUCINA]));
        let ordered = OrderedFighterRoster::from_fighters(&[F::MARIO, F::KIRBY]).unwrap();
        let cases = [
            (None, vec![F::LUCINA]),
            (Some(Value::Null), vec![F::LUCINA]),
            (
                Some(serde_json::to_value(ordered).unwrap()),
                vec![F::MARIO, F::KIRBY],
            ),
        ];
        for (present, expected) in cases {
            let mut file = legacy.clone();
            if let Some(value) = present {
                file["player_roster"] = value;
            }
            assert_eq!(menu_of(&file).player_roster.fighters(), expected);
        }
        let mut file = legacy;
        file["player_roster"] = serde_json::to_value(OrderedFighterRoster::all()).unwrap();
        assert_eq!(menu_of(&file).player_roster, OrderedFighterRoster::all());
    }

    #[test]
    fn present_slot_size_and_roster_values_are_validated_even_when_unused() {
        // The ordered roster wins, so slots and size are unused but still checked.
        let base = with(
            with(stripped(), slots(&CPU, &[F::MARIO, F::KIRBY])),
            vec![(
                "cpu_roster",
                serde_json::to_value(DEFAULT_CPU_ROSTER).unwrap(),
            )],
        );
        let mut gapped = vec![0u8; CHOICES.len() + 1];
        gapped[1] = 1;
        gapped[2] = 3;
        let rejected = [
            ("cpu_roster_7", bits(&[0, 1])),
            ("cpu_roster_7", bits(&[])),
            ("cpu_roster_7", Value::Null),
            ("cpu_roster_7", json!([1])),
            ("cpu_roster_size", one_hot(0)),
            ("cpu_roster_size", json!([1, 1, 0, 0, 0, 0, 0, 0])),
            ("cpu_roster_size", json!([1])),
            ("cpu_roster_size", Value::Null),
            ("cpu_roster", json!(gapped)),
            ("cpu_roster", json!([0, 1, 2])),
        ];
        for (key, value) in rejected {
            let file = with(base.clone(), vec![(key, value.clone())]);
            let error = ingress(&file, &modern()).unwrap_err().to_string();
            assert!(error.contains(key), "{} {}: {}", key, value, error);
            // defaults_menu is parsed by the same rule.
            assert!(
                ingress(&modern(), &file).is_err(),
                "defaults {} {}",
                key,
                value
            );
        }
        // A valid inactive slot is accepted and still has no effect.
        let file = with(base, vec![("cpu_roster_7", fighter(F::LUCINA))]);
        assert_eq!(menu_of(&file).cpu_roster, DEFAULT_CPU_ROSTER);
    }

    #[test]
    fn missing_upstream_keys_and_repeated_keys_still_fail() {
        let mut file = modern();
        file.as_object_mut().unwrap().remove("lra_reset");
        let error = ingress(&file, &modern()).unwrap_err().to_string();
        assert!(error.contains("missing field `lra_reset`"), "{}", error);
        // Fork keys stay optional and unknown metadata is still ignored.
        let mut file = modern();
        file.as_object_mut().unwrap().remove("cpu_cycle");
        file["unrelated_note"] = json!("kept by nobody");
        assert_eq!(menu_of(&file).cpu_cycle, DEFAULT_CPU_CYCLE);
        // Repeated keys: live, retired slot, and envelope.
        let menu = modern().to_string();
        let mario = fighter(F::MARIO);
        let repeated_live = menu.replacen('{', &format!("{{\"cpu_character\":{mario},"), 1);
        let repeated_slot = menu.replacen(
            '{',
            &format!("{{\"cpu_roster_3\":{mario},\"cpu_roster_3\":{mario},"),
            1,
        );
        for text in [
            format!("{{\"menu\":{repeated_live},\"defaults_menu\":{menu}}}"),
            format!("{{\"menu\":{menu},\"defaults_menu\":{repeated_slot}}}"),
            format!("{{\"menu\":{menu},\"menu\":{menu},\"defaults_menu\":{menu}}}"),
        ] {
            let error = serde_json::from_str::<MenuJsonStruct>(&text)
                .unwrap_err()
                .to_string();
            assert!(error.contains("duplicate field"), "{}", error);
        }
    }

    #[test]
    fn missing_macro_source_stays_on_numbered_slots() {
        let mut file = modern();
        file.as_object_mut().unwrap().remove("macro_source");
        let text = format!("{{\"menu\":{file},\"defaults_menu\":{file}}}");
        let parsed = serde_json::from_str::<MenuJsonStruct>(&text).unwrap();
        assert_eq!(parsed.menu.macro_source, crate::MacroSource::NUMBERED);
        assert_eq!(
            parsed.defaults_menu.macro_source,
            crate::MacroSource::NUMBERED
        );
    }

    #[test]
    fn every_live_stored_key_has_a_submenu_and_retired_keys_are_not_written() {
        let mut app = unsafe { crate::create_app() };
        let ids: Vec<&str> = app
            .tabs
            .iter()
            .flat_map(|tab| tab.submenus.iter().map(|submenu| submenu.id))
            .chain(app.hidden_submenus.iter().map(|submenu| submenu.id))
            .collect();
        let mut stored = modern();
        // Old files still accept this field, but shortcuts now live in the INI.
        stored["menu_open_start_press"] = serde_json::to_value(crate::OnOff::ON).unwrap();
        let keys = stored.as_object().unwrap();
        for key in keys.keys().filter(|key| key.as_str() != "menu_open_start_press") {
            assert!(ids.contains(&key.as_str()), "{} has no submenu", key);
        }
        assert!(!ids.contains(&"menu_open_start_press"));
        for (key, _) in PLAYER.slots.iter().chain(CPU.slots.iter()) {
            assert!(!keys.contains_key(*key), "{} is still written", key);
        }
        assert!(!keys.contains_key(PLAYER.size) && !keys.contains_key(CPU.size));
        assert!(keys.contains_key("trio_practice"));
        // Player Swapping lives in the player tab, not under Misc.
        let misc = app.tabs.iter().find(|tab| tab.id == "misc").unwrap();
        assert!(!misc
            .submenus
            .iter()
            .any(|submenu| submenu.id.starts_with("trio_")));
        // Both visible and shared-editor settings must survive a full commit.
        app.update_all_from_json(&stored.to_string());
        let emitted: Value = serde_json::from_str(&app.current_settings_to_json()).unwrap();
        assert!(!emitted.as_object().unwrap().contains_key("menu_open_start_press"));
        let committed: TrainingModpackMenu =
            serde_json::from_str(&app.current_settings_to_json()).unwrap();
        assert_eq!(rosters(&committed), rosters(&BASE_MENU));
        assert_eq!(committed.menu_open_start_press, crate::OnOff::OFF);
    }

    #[test]
    fn editor_commits_reparse_through_ingress_with_the_edited_order() {
        let cases = [
            (
                "player_characters",
                "cpu_roster",
                vec![F::PIKACHU, F::KIRBY, F::BOWSER, F::BAYONETTA, F::MARIO],
            ),
            (
                "player_characters",
                "player_roster",
                vec![F::GAMEWATCH, F::PALUTENA, F::PIKACHU, F::MARIO],
            ),
        ];
        for (tab_id, submenu_id, expected) in cases {
            let mut app = unsafe { crate::create_app() };
            app.serialized_default_settings = modern().to_string();
            app.update_all_from_json(&modern().to_string());
            let tab = app.tabs.iter().position(|tab| tab.id == tab_id).unwrap();
            app.tabs.state.select(Some(tab));
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
            // The shared grid starts on Mario; append after the configured order.
            if submenu_id == "cpu_roster" {
                app.on_zr();
            }
            app.on_a();
            app.on_b();
            let text = app.get_serialized_settings_with_defaults();
            assert!(
                !text.contains("trio_fighter_") && !text.contains("_roster_"),
                "{}",
                text
            );
            let parsed: MenuJsonStruct = serde_json::from_str(&text).unwrap();
            let edited = if submenu_id == "cpu_roster" {
                parsed.menu.cpu_roster
            } else {
                parsed.menu.player_roster
            };
            assert_eq!(edited.fighters(), expected);
            assert_eq!(parsed.menu.cpu_character, F::PIKACHU);
            assert_eq!(parsed.menu.player_character, F::GAMEWATCH);
            assert_eq!(rosters(&parsed.defaults_menu), rosters(&BASE_MENU));
            // Saving and loading the imported file again keeps the edited order.
            let again: MenuJsonStruct =
                serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
            assert_eq!(rosters(&again.menu), rosters(&parsed.menu));
        }
    }

    #[test]
    fn stage_direct_close_preserves_rotation_and_emits_only_one_explicit_action() {
        use training_mod_tui::{stage_forms::{Form, Selection}, stage_menu::{Mode, Preferences}, AppPage};
        let mut app = unsafe { crate::create_app() };
        let mut menu = BASE_MENU;
        // Existing independent serde contract: All=0, Waiting rank2, BF rank1, FD rank3.
        let mut ranks = vec![0u8; crate::lobby_stages::STAGES.len() + 1];
        for (id, rank) in [(311, 2), (0, 1), (3, 3)] {
            ranks[crate::lobby_stages::STAGES.iter().position(|(kind, _)| *kind == id).unwrap() + 1] = rank;
        }
        menu.lobby_stages = serde_json::from_value(json!(ranks)).unwrap();
        let settings = serde_json::to_string(&menu).unwrap();
        app.serialized_default_settings = modern().to_string();
        app.open_settings_for_player(&settings, Some(9));
        app.open_stage_menu(true, Preferences::default());
        let tab = app.tabs.iter().position(|tab| tab.submenus.iter().any(|s| s.id == "lobby_stages")).unwrap();
        app.tabs.state.select(Some(tab));
        let index = app.selected_tab().submenus.iter().position(|s| s.id == "lobby_stages").unwrap();
        let cols = app.selected_tab().submenus.cols;
        app.selected_tab().submenus.select(index / cols, index % cols);
        app.on_a();
        assert!(app.stage_page_active());
        assert!(app.take_pending_stage().is_none());
        app.on_x();
        assert!(app.stage_is_direct());
        assert_eq!(app.stage_direct_marker(), Some(Preferences::default().direct_stage));
        app.selected_submenu().toggles.select(0, 0);
        app.on_a(); // All is not a Direct request.
        assert!(app.page == AppPage::TOGGLE);
        assert!(app.take_pending_stage().is_none());
        let fd = crate::lobby_stages::STAGES.iter().position(|(id, _)| *id == 3).unwrap() + 1;
        let cols = app.selected_submenu().toggles.cols;
        app.selected_submenu().toggles.select(fd / cols, fd % cols);
        app.on_y(); // Final Destination Normal -> Battlefield; ranks stay intact.
        app.on_a(); // Actual production action: save choice, close, emit once.
        assert!(app.page == AppPage::CLOSE);
        assert_eq!(app.take_pending_stage(), Some(Selection { stage: 3, form: Form::Battlefield }));
        assert_eq!(app.take_pending_stage(), None);
        assert!(app.take_pending_mii_selection(0).is_none());
        let saved: MenuJsonStruct = serde_json::from_str(&app.get_serialized_settings_with_defaults()).unwrap();
        assert_eq!(saved.menu.lobby_stages.stages(), vec![0, 311, 3]);
        assert_eq!(saved.defaults_menu.lobby_stages, BASE_MENU.lobby_stages);
        assert_eq!(saved.stage_preferences, Preferences { mode: Mode::Direct, direct_stage: 3,
            direct_form: Form::Battlefield, ..Preferences::default() });
        // A later menu opening restores preferences but does not replay a live action.
        app.open_settings_for_player(&serde_json::to_string(&saved.menu).unwrap(), Some(9));
        app.open_stage_menu(true, saved.stage_preferences);
        app.page = AppPage::TOGGLE;
        assert_eq!(app.stage_selection(), Some(Selection { stage: 3, form: Form::Battlefield }));
        assert!(app.take_pending_stage().is_none());
        app.on_x();
        assert!(!app.stage_is_direct());
        app.on_b(); app.on_b();
        assert!(app.take_pending_stage().is_none());
        let rotated: MenuJsonStruct = serde_json::from_str(&app.get_serialized_settings_with_defaults()).unwrap();
        assert_eq!(rotated.stage_preferences.mode, Mode::Rotation);
        assert_eq!(rotated.menu.lobby_stages.stages(), vec![0, 311, 3]);
    }

    #[test]
    fn stage_training_view_keeps_manual_actions_separate_from_lobby_preferences() {
        use training_mod_tui::{stage_forms::Selection, stage_menu::{Mode, Preferences}, App, AppPage};

        fn select_picker(app: &mut App<'_>) {
            let tab = app.tabs.iter().position(|tab| tab.submenus.iter()
                .any(|s| s.id == "lobby_stages")).unwrap();
            app.tabs.state.select(Some(tab));
            let index = app.selected_tab().submenus.iter()
                .position(|s| s.id == "lobby_stages").unwrap();
            let cols = app.selected_tab().submenus.cols;
            app.selected_tab().submenus.select(index / cols, index % cols);
            app.on_a();
        }

        let mut menu = BASE_MENU;
        let mut ranks = vec![0u8; crate::lobby_stages::STAGES.len() + 1];
        for (id, rank) in [(311, 2), (0, 1), (3, 3)] {
            let index = crate::lobby_stages::STAGES.iter()
                .position(|(kind, _)| *kind == id).unwrap() + 1;
            ranks[index] = rank;
        }
        menu.lobby_stages = serde_json::from_value(json!(ranks)).unwrap();
        let settings = serde_json::to_string(&menu).unwrap();
        let fd = crate::lobby_stages::STAGES.iter().position(|(id, _)| *id == 3).unwrap() + 1;

        for mode in [Mode::Rotation, Mode::Direct] {
            let preferences = Preferences { mode, direct_stage: 311, ..Preferences::default() };
            let mut app = unsafe { crate::create_app() };
            app.serialized_default_settings = modern().to_string();
            app.open_settings_for_player(&settings, Some(9));
            app.open_stage_menu_in_context(true, preferences, true);
            select_picker(&mut app);
            assert!(app.stage_is_direct());
            assert_eq!(app.stage_title(), "Change Stage Now");
            assert_eq!(app.stage_direct_marker(), None);
            assert!(app.stage_help().contains("Training only"));
            app.on_b(); app.on_b();
            let cancelled: MenuJsonStruct = serde_json::from_str(
                &app.get_serialized_settings_with_defaults()).unwrap();
            assert_eq!(cancelled.stage_preferences, preferences);
            assert_eq!(cancelled.menu.lobby_stages, menu.lobby_stages);
            assert!(app.take_pending_stage().is_none());

            app.open_stage_menu_in_context(true, preferences, true);
            app.page = AppPage::SUBMENU;
            select_picker(&mut app);
            let cols = app.selected_submenu().toggles.cols;
            app.selected_submenu().toggles.select(fd / cols, fd % cols);
            app.on_a();
            assert!(app.page == AppPage::CLOSE);
            assert_eq!(app.take_pending_stage(), Some(Selection::normal(3)));
            assert_eq!(app.take_pending_stage(), None);
            let applied: MenuJsonStruct = serde_json::from_str(
                &app.get_serialized_settings_with_defaults()).unwrap();
            assert_eq!(applied.stage_preferences, preferences);
            assert_eq!(applied.menu.lobby_stages, menu.lobby_stages);

            app.open_stage_menu_in_context(true, preferences, true);
            app.page = AppPage::SUBMENU;
            select_picker(&mut app);
            app.on_x();
            assert!(!app.stage_is_direct());
            assert_eq!(app.stage_title(), "Lobby Stage Rotation");
            assert!(app.stage_help().contains("Next lobby entry only"));
            assert_eq!(app.stage_preferences.mode, Mode::Rotation);
            app.on_x(); // Returning to the manual view must not pin the lobby stage.
            assert!(app.stage_is_direct());
            assert_eq!(app.stage_preferences.mode, Mode::Rotation);
            app.on_x();
            let cols = app.selected_submenu().toggles.cols;
            app.selected_submenu().toggles.select(fd / cols, fd % cols);
            app.on_a(); // Remove FD from the future lobby rotation, not a live request.
            assert!(app.take_pending_stage().is_none());
            app.on_b();
            select_picker(&mut app); // Reopening the Training picker is manual again.
            assert!(app.stage_is_direct());
            app.on_b(); app.on_b();
            let rotated: MenuJsonStruct = serde_json::from_str(
                &app.get_serialized_settings_with_defaults()).unwrap();
            assert_eq!(rotated.stage_preferences, Preferences { mode: Mode::Rotation, direct_stage: 311,
                ..Preferences::default() });
            assert_eq!(rotated.menu.lobby_stages.stages(), vec![0, 311]);
            assert!(app.take_pending_stage().is_none());

            app.open_stage_menu(true, rotated.stage_preferences);
            assert!(!app.stage_is_direct()); // Lobby restores its persisted mode.
            app.open_stage_menu(true, Preferences { mode: Mode::Direct, direct_stage: 311,
                ..Preferences::default() });
            assert_eq!(app.stage_direct_marker(), Some(311));
        }
    }

    #[test]
    fn stage_preferences_ingress_and_feature_off_do_not_enable_direct_actions() {
        use training_mod_tui::{stage_menu::{Mode, Preferences}, AppPage};
        let mut value = json!({"menu": modern(), "defaults_menu": modern()});
        let old: MenuJsonStruct = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(old.stage_preferences, Preferences::default());
        value["stage_preferences"] = json!({"mode": "direct", "direct_stage": 0xffffffffu32});
        assert!(serde_json::from_value::<MenuJsonStruct>(value).is_err());
        let mut app = unsafe { crate::create_app() };
        app.open_settings_for_player(&modern().to_string(), Some(9));
        app.open_stage_menu_in_context(false, Preferences { mode: Mode::Direct, direct_stage: 3,
            ..Preferences::default() }, true);
        let tab = app.tabs.iter().position(|tab| tab.submenus.iter().any(|s| s.id == "lobby_stages")).unwrap();
        app.tabs.state.select(Some(tab));
        let index = app.selected_tab().submenus.iter().position(|s| s.id == "lobby_stages").unwrap();
        let cols = app.selected_tab().submenus.cols;
        app.selected_tab().submenus.select(index / cols, index % cols);
        app.on_a();
        app.selected_submenu().toggles.select(0, 2); // Ordinary BF rotation toggle.
        app.on_a();
        assert!(app.page == AppPage::TOGGLE);
        assert!(!app.stage_is_direct());
        assert!(app.take_pending_stage().is_none());
        let numeric: TrainingModpackMenu = serde_json::from_str(&app.current_settings_to_json()).unwrap();
        assert_eq!(numeric.lobby_stages.stages(), vec![311, 0]);
    }

    #[cfg(feature = "training-stage-rotation")]
    #[test]
    fn training_stage_rotation_defaults_off_for_legacy_files_and_round_trips_on() {
        use crate::OnOff;
        // A file written before the activation existed: no key, no activation.
        let mut legacy = modern();
        assert!(legacy.as_object_mut().unwrap().remove("training_stage_rotation").is_some());
        let parsed = ingress(&legacy, &legacy).unwrap();
        assert_eq!(parsed.menu.training_stage_rotation, OnOff::OFF);
        assert_eq!(parsed.defaults_menu.training_stage_rotation, OnOff::OFF);
        // Old waiting ranks alone never activate Training rotation.
        let mut ranked = legacy.clone();
        ranked["lobby_stages"] = json!([0, 0, 1, 2]);
        let ranked_menu = menu_of(&ranked);
        assert_eq!(ranked_menu.training_stage_rotation, OnOff::OFF);
        assert_eq!(ranked_menu.lobby_stages.stages(), vec![0, 3]);
        // The production file loader keeps an explicit On through save and reload,
        // with ranks, forms and the independent defaults menu untouched.
        let mut on = modern();
        on["training_stage_rotation"] = serde_json::to_value(OnOff::ON).unwrap();
        on["lobby_stages"] = json!([0, 0, 1, 2]);
        let text = json!({ "menu": on, "defaults_menu": modern() }).to_string();
        let loaded = parse_menu_file(&text).unwrap();
        assert!(loaded.rejected_stage_preferences.is_none());
        assert_eq!(loaded.settings.menu.training_stage_rotation, OnOff::ON);
        assert_eq!(loaded.settings.defaults_menu.training_stage_rotation, OnOff::OFF);
        assert_eq!(loaded.settings.menu.lobby_stages.stages(), vec![0, 3]);
        let saved = serde_json::to_string(&loaded.settings).unwrap();
        let again = parse_menu_file(&saved).unwrap();
        assert_eq!(again.settings.menu.training_stage_rotation, OnOff::ON);
        assert_eq!(again.settings.menu.lobby_stages, loaded.settings.menu.lobby_stages);
        assert_eq!(again.settings.stage_preferences, loaded.settings.stage_preferences);
        // The menu exposes the activation as its own submenu next to the stage order.
        let app = unsafe { crate::create_app() };
        let misc = app.tabs.iter().find(|tab| tab.id == "misc").unwrap();
        let ids: Vec<&str> = misc.submenus.iter().map(|submenu| submenu.id).collect();
        let order = ids.iter().position(|id| *id == "lobby_stages").unwrap();
        assert_eq!(ids.get(order + 1), Some(&"training_stage_rotation"));
    }

}
