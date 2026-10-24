//! Focused production-serde/TUI checks for root's later host validation.
//! This private packet does not execute or compile tests.
use super::*;

#[cfg(feature = "p1-cpu-control")]
#[test]
fn p1_cpu_old_menu_defaults_off_and_level_three() {
    let mut old = serde_json::to_value(BASE_MENU).unwrap();
    old.as_object_mut().unwrap().remove("p1_cpu_control");
    old.as_object_mut().unwrap().remove("p1_cpu_level");
    let parsed: MenuJsonStruct = serde_json::from_value(serde_json::json!({
        "menu": old.clone(), "defaults_menu": old,
    }))
    .unwrap();
    for menu in [parsed.menu, parsed.defaults_menu] {
        assert_eq!(menu.p1_cpu_control, OnOff::OFF);
        assert_eq!(menu.p1_cpu_level.as_level(), Some(3));
    }
}

#[cfg(feature = "p1-cpu-control")]
#[test]
fn p1_cpu_menu_commit_roundtrips_control_and_level() {
    let mut selected = BASE_MENU;
    selected.p1_cpu_control = OnOff::ON;
    selected.p1_cpu_level = P1CpuLevel::L7;
    let mut app = unsafe { create_app() };
    let misc = app.tabs.iter().find(|tab| tab.id == "misc").unwrap();
    assert!(misc
        .submenus
        .iter()
        .any(|s| s.id == "p1_cpu_control" && s.title == "P1 CPU Control"));
    let levels = misc
        .submenus
        .iter()
        .find(|s| s.id == "p1_cpu_level")
        .unwrap();
    assert_eq!(
        levels.toggles.iter().map(|t| t.title).collect::<Vec<_>>(),
        vec!["1", "2", "3", "4", "5", "6", "7", "8", "9"]
    );
    app.update_all_from_json(&serde_json::to_string(&selected).unwrap());
    let committed: TrainingModpackMenu =
        serde_json::from_str(&app.current_settings_to_json()).unwrap();
    assert_eq!(committed.p1_cpu_control, OnOff::ON);
    assert_eq!(committed.p1_cpu_level.as_level(), Some(7));
    assert_eq!(committed.player_character, selected.player_character);
    assert_eq!(committed.cpu_character, selected.cpu_character);
    assert_eq!(
        serde_json::to_value(committed.p1_cpu_level).unwrap(),
        serde_json::json!([0, 0, 0, 0, 0, 0, 1, 0, 0])
    );
}

#[cfg(not(feature = "p1-cpu-control"))]
#[test]
fn p1_cpu_feature_off_hides_controls_and_ignores_saved_keys() {
    let mut saved = serde_json::to_value(BASE_MENU).unwrap();
    saved["p1_cpu_control"] = serde_json::json!([1, 0]);
    saved["p1_cpu_level"] = serde_json::json!([0, 0, 0, 0, 0, 0, 1, 0, 0]);
    let parsed: MenuJsonStruct = serde_json::from_value(serde_json::json!({
        "menu": saved.clone(), "defaults_menu": saved,
    }))
    .unwrap();
    let stored = serde_json::to_value(parsed.menu).unwrap();
    assert!(stored.get("p1_cpu_control").is_none());
    assert!(stored.get("p1_cpu_level").is_none());
    let app = unsafe { create_app() };
    assert!(!app
        .tabs
        .iter()
        .flat_map(|tab| tab.submenus.iter())
        .any(|s| s.id == "p1_cpu_control" || s.id == "p1_cpu_level"));
}
