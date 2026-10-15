use super::*;
use training_mod_tui::{roster_grid::Role, AppPage};

fn editor() -> App<'static> {
    let mut app = unsafe { create_app() };
    let defaults = serde_json::to_string(&BASE_MENU).unwrap();
    app.update_all_from_json(&defaults);
    app.serialized_default_settings = defaults;
    let index = app.tabs.iter().position(|tab| tab.id == "player_characters").unwrap();
    app.tabs.state.select(Some(index));
    app.selected_tab().submenus.select(0, 3);
    app.on_a();
    assert!(app.page == AppPage::ROSTER);
    app
}

fn seek(app: &mut App<'_>, kind: u32) {
    // Walk the actual input interface, preserving the current shared-form preview.
    for _ in 0..7 {
        for _ in 0..13 {
            if app.roster_grid.cursor_info().kind() == Some(kind) { return; }
            app.on_right();
        }
        app.on_down();
    }
    panic!("fighter not found: {}", kind);
}

#[test]
fn shared_editor_roundtrips_both_roles_without_reordering_legacy_selections() {
    let mut app = editor();
    let mut player = vec![0; 81];
    let mut cpu = vec![0; 81];
    // Legacy catalog: Falcon then Kirby; Hero then Mario, deliberately non-menu order.
    for (values, kinds) in [(&mut player, [0x0b, 0x06]), (&mut cpu, [0x53, 0x00])] {
        for (rank, kind) in kinds.iter().copied().enumerate() {
            values[1 + app.roster_kinds.iter().position(|k| *k == kind).unwrap()] = rank as u8 + 1;
        }
    }
    app.update_all_from_json(&serde_json::json!({"player_roster":player,"cpu_roster":cpu}).to_string());
    let before = app.current_settings_to_json();
    let saved_cpu = app.roster_values(Role::Cpu);
    seek(&mut app, 0x5a);
    app.on_x();
    assert_eq!(app.roster_grid.cursor_info().kind(), Some(0x5b));
    assert_eq!(app.current_settings_to_json(), before, "preview must not change either cycle");
    app.on_a();
    assert_eq!(app.roster_values(Role::Cpu), saved_cpu);
    let mythra = 1 + app.roster_kinds.iter().position(|k| *k == 0x5b).unwrap();
    assert_eq!(app.roster_values(Role::Player)[mythra], 3);
    app.on_y();
    app.on_a();
    assert_eq!(app.roster_values(Role::Cpu)[mythra], 3);
    app.on_r();
    assert_eq!(app.roster_values(Role::Cpu), saved_cpu);
    assert_eq!(app.roster_values(Role::Player)[mythra], 3);
    let stored = app.current_settings_to_json();
    let mut reopened = editor();
    reopened.update_all_from_json(&stored);
    assert_eq!(reopened.current_settings_to_json(), stored);
    // Exercise the product deserializer, not just TUI JSON.
    let menu: TrainingModpackMenu = serde_json::from_str(&stored).unwrap();
    let mut loaded = editor();
    loaded.update_all_from_json(&serde_json::to_string(&menu).unwrap());
    assert_eq!(loaded.current_settings_to_json(), stored);
}

#[test]
fn role_all_and_defaults_include_the_hidden_cpu_setting() {
    let mut app = editor();
    app.on_zl();
    app.on_a(); // Mario becomes the sole explicit player member.
    let player = app.roster_values(Role::Player);
    app.on_y();
    app.on_zl();
    assert_eq!(app.roster_values(Role::Player), player);
    assert_eq!(app.roster_values(Role::Cpu).iter().sum::<u8>(), 1);
    assert_eq!(app.roster_values(Role::Cpu)[0], 1);
    app.on_right();
    app.on_a(); // DK becomes sole CPU member.
    app.on_r(); // Removing the final member must leave a usable cycle.
    assert_eq!(app.roster_values(Role::Cpu).iter().sum::<u8>(), 1);
    app.on_b();
    assert!(app.page == AppPage::SUBMENU);
    app.on_x(); // Save both as defaults using the normal menu controls.
    let saved = app.current_settings_to_json();
    app.on_a();
    app.on_zl();
    app.load_defaults();
    assert_eq!(app.current_settings_to_json(), saved);
    assert_eq!(app.hidden_submenus.len(), 1);
    assert!(app.tabs.iter().all(|tab| tab.id != "cpu_characters"));
}
