use super::*;
use training_mod_tui::{
    roster_grid::{Role, RosterPurpose},
    AppPage,
};

fn editor() -> App<'static> {
    let mut app = unsafe { create_app() };
    let defaults = serde_json::to_string(&BASE_MENU).unwrap();
    app.update_all_from_json(&defaults);
    app.serialized_default_settings = defaults;
    let index = app
        .tabs
        .iter()
        .position(|tab| tab.id == "player_characters")
        .unwrap();
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
            if app.roster_grid.cursor_info().kind() == Some(kind) {
                return;
            }
            app.on_right();
        }
        app.on_down();
    }
    panic!("fighter not found: {}", kind);
}

#[test]
fn cancelled_scene_editor_restores_against_the_new_original() {
    let mut menu = BASE_MENU;
    let roster = OrderedFighterRoster::from_fighters(&[
        TrioFighter::MARIO,
        TrioFighter::KIRBY,
        TrioFighter::PIKACHU,
    ])
    .unwrap();
    menu.player_roster = roster;
    menu.cpu_roster = roster;
    let authoritative = serde_json::to_string(&menu).unwrap();
    let mut app = editor();
    app.open_settings_for_player(&authoritative, Some(0));
    assert_eq!(app.roster_badge(Role::Player, 6), Some(1));
    // This editor is cancelled by scene exit, so the authoritative menu has
    // not committed its Mario exclusion. The next session starts as Kirby.
    app.open_settings_for_player(&authoritative, Some(6));
    let restored: TrainingModpackMenu =
        serde_json::from_str(&app.current_settings_to_json()).unwrap();
    assert_eq!(
        restored.player_roster.fighters(),
        vec![TrioFighter::MARIO, TrioFighter::PIKACHU]
    );
    assert_eq!(restored.cpu_roster, roster);
    assert_eq!(app.roster_badge(Role::Player, 0), Some(1));
    assert_eq!(app.roster_badge(Role::Player, 6), Some(0));
    assert_eq!(app.roster_badge(Role::Player, 8), Some(2));
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
    app.update_all_from_json(
        &serde_json::json!({"player_roster":player,"cpu_roster":cpu}).to_string(),
    );
    let before = app.current_settings_to_json();
    let saved_cpu = app.roster_values(Role::Cpu);
    seek(&mut app, 0x5a);
    app.on_x();
    assert_eq!(app.roster_grid.cursor_info().kind(), Some(0x5b));
    assert_eq!(
        app.current_settings_to_json(),
        before,
        "preview must not change either cycle"
    );
    app.on_a();
    assert_eq!(app.roster_values(Role::Cpu), saved_cpu);
    let mythra = 1 + app.roster_kinds.iter().position(|k| *k == 0x5b).unwrap();
    assert_eq!(app.roster_values(Role::Player)[mythra], 3);
    app.on_zr();
    app.on_x(); // CPU has its own default Pyra start/preview.
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
    app.on_zr();
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
    assert_eq!(app.hidden_submenus.len(), 4);
    assert!(app.tabs.iter().all(|tab| tab.id != "cpu_characters"));
}

fn committed(app: &App<'_>) -> TrainingModpackMenu {
    serde_json::from_str(&app.current_settings_to_json()).unwrap()
}

fn open_manual(app: &mut App<'_>, role: Role) {
    app.page = AppPage::SUBMENU;
    let tab = app.tabs.iter().position(|tab| tab.id == "player_characters").unwrap();
    app.tabs.state.select(Some(tab));
    app.selected_tab().submenus.select(0, if role == Role::Player { 1 } else { 2 });
    app.on_a();
    assert!(app.page == AppPage::ROSTER);
    assert_eq!(app.roster_purpose, RosterPurpose::ManualSwap);
    assert_eq!(app.roster_grid.role(), role);
}

fn finish_manual_flash(app: &mut App<'_>, role: Role, kind: u32) {
    assert_eq!(app.take_pending_manual_selection(), None);
    for remaining in (1..=training_mod_tui::roster_grid::MANUAL_FLASH_FRAMES).rev() {
        assert_eq!(app.manual_selection_flash(), Some((role, kind, remaining)));
        assert!(app.page == AppPage::ROSTER);
        app.advance_manual_selection();
    }
    assert!(app.page == AppPage::CLOSE);
    assert_eq!(app.manual_selection_flash(), None);
    assert_eq!(app.take_pending_manual_selection(), Some((role, kind)));
    assert_eq!(app.take_pending_manual_selection(), None);
}

#[test]
fn manual_portrait_selection_is_one_shot_and_preserves_both_cycles_and_hat_ranks() {
    let mut menu = BASE_MENU;
    menu.trio_practice = OnOff::ON;
    menu.player_character = TrioFighter::MARIO;
    menu.player_kirby_hats = OrderedFighterRoster::from_fighters(&[
        TrioFighter::MARIO, TrioFighter::PIKACHU, TrioFighter::BOWSER,
    ]).unwrap();
    let mut app = editor();
    app.open_settings_for_player(&serde_json::to_string(&menu).unwrap(), Some(6));
    open_manual(&mut app, Role::Player);
    assert_eq!(app.roster_grid.cursor_info().kind(), Some(0));
    let before = committed(&app);
    let preferences = app.mii_preferences.clone();
    app.on_zl(); // No All action in a single-choice picker.
    app.on_r();  // No cycle removal.
    seek(&mut app, 0x4b);
    app.on_y();  // No cycle Solo edit.
    seek(&mut app, 6); // Original P1 is selectable here, still slot 0 in the cycle.
    assert!(!app.roster_excluded(Role::Player, 6));
    app.on_a();
    let chosen = app.current_settings_to_json();
    app.on_right(); app.on_zr(); app.on_a(); app.on_b(); app.on_start();
    assert_eq!(app.current_settings_to_json(), chosen, "acknowledgement owns input");
    finish_manual_flash(&mut app, Role::Player, 6);
    let after = committed(&app);
    assert_eq!(after.player_character, TrioFighter::KIRBY);
    assert_eq!(after.cpu_character, before.cpu_character);
    assert_eq!(after.player_roster, before.player_roster);
    assert_eq!(after.cpu_roster, before.cpu_roster);
    assert_eq!(after.player_kirby_hats, before.player_kirby_hats);
    assert_eq!(after.cpu_kirby_hats, before.cpu_kirby_hats);
    assert_eq!(after.solo_popo_player_cycle, before.solo_popo_player_cycle);
    assert_eq!(app.mii_preferences, preferences);
    // Continue editing through the actual cycle launcher after the manual action.
    app.page = AppPage::SUBMENU;
    app.selected_tab().submenus.select(0, 3);
    app.on_a();
    assert_eq!(app.roster_badge(Role::Player, 6), Some(0));
    seek(&mut app, 1);
    app.on_a();
    assert_eq!(committed(&app).player_kirby_hats, before.player_kirby_hats);
    open_hats(&mut app);
    assert_eq!(app.roster_badge(Role::Player, 0), Some(1));
    assert_eq!(app.roster_badge(Role::Player, 8), Some(2));
    assert_eq!(app.roster_badge(Role::Player, 0x0f), Some(3));
}

#[test]
fn manual_cpu_forms_cancel_and_same_choice_retry_use_the_portrait_input_path() {
    let mut app = editor();
    let mut menu = BASE_MENU;
    menu.cpu_character = TrioFighter::from_kind(0x25).unwrap();
    app.open_settings_for_player(&serde_json::to_string(&menu).unwrap(), Some(0));
    open_manual(&mut app, Role::Cpu);
    assert_eq!(app.roster_grid.cursor_info().kind(), Some(0x25));
    let before = app.current_settings_to_json();
    app.on_x(); // Charizard is only previewed.
    assert_eq!(app.roster_grid.cursor_info().kind(), Some(0x26));
    app.on_b();
    assert_eq!(app.current_settings_to_json(), before);
    assert_eq!(app.take_pending_manual_selection(), None);
    open_manual(&mut app, Role::Cpu);
    assert_eq!(app.roster_grid.cursor_info().kind(), Some(0x25));
    app.on_x();
    app.on_a();
    finish_manual_flash(&mut app, Role::Cpu, 0x26);
    assert_eq!(committed(&app).cpu_character.kind(), 0x26);
    assert_eq!(committed(&app).player_character, menu.player_character);
    let stored = app.current_settings_to_json();
    app.open_settings_for_player(&stored, Some(0));
    open_manual(&mut app, Role::Cpu);
    app.on_a(); // Retry must exist even though the serialized choice is unchanged.
    assert_eq!(app.current_settings_to_json(), stored);
    finish_manual_flash(&mut app, Role::Cpu, 0x26);
}

#[test]
fn manual_picker_keeps_player_enable_and_unqualified_mii_boundaries() {
    let mut app = editor();
    open_manual(&mut app, Role::Player);
    let before = app.current_settings_to_json();
    seek(&mut app, 6);
    app.on_a();
    assert_eq!(app.manual_selection_flash(), None);
    assert_eq!(app.current_settings_to_json(), before);
    app.on_zr();
    assert_eq!(app.roster_grid.role(), Role::Cpu);
    seek_label(&mut app, "Mii Brawler");
    app.on_a(); // No qualified saved catalog has been mounted.
    assert_eq!(app.manual_selection_flash(), None);
    assert_eq!(app.current_settings_to_json(), before);
    seek_label(&mut app, "Mii Gunner");
    app.on_right(); // Single-choice navigation skips All, wrapping to Steve.
    assert_eq!(app.roster_grid.cursor_info().kind(), Some(0x58));
    app.on_a();
    finish_manual_flash(&mut app, Role::Cpu, 0x58);
}

#[cfg(feature = "mii-managed-helper-alpha")]
#[test]
fn manual_mii_portraits_use_qualified_roles_without_rewriting_saved_identities() {
    use training_mod_mii::{inventory::*, selection::{Active, PlayerLive, PlayerOrigin}};
    let catalog = build_snapshot((0..2).map(|index| RawSlot {
        index, slot_id: 10 + index as u32, name_len: 1,
        name_units: [65 + index as u16, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        colour: 0, voice: 37, specials: [index as u8, 0, 0, 0],
        charinfo: [0; CHARINFO_LEN],
        tag_outfit: TaggedId(0x1300011bfef7a5f7),
        tag_hat: TaggedId(0x5a00000fa9efa5ea),
        tag_chara: TaggedId(0xc1004c13fdfdfb2b),
    }).collect());
    let mut app = editor();
    let mut menu = BASE_MENU;
    menu.trio_practice = OnOff::ON;
    app.open_settings_for_player(&serde_json::to_string(&menu).unwrap(), Some(0));
    app.page = AppPage::SUBMENU;
    for (role, index) in [(training_mod_mii::selection::Role::Player, 0), (training_mod_mii::selection::Role::Cpu, 1)] {
        app.mii_preferences = app.mii_preferences.clone()
            .with(role, MiiClass::Brawler, Some(catalog.configs[index].canonical)).unwrap();
    }
    assert!(app.mount_mii_catalog(catalog, training_mod_tui::mii_menu::Context {
        epoch: 7, original: PlayerOrigin::NotMii,
        active: Active { player: PlayerLive::Origin, cpu: None },
    }));
    app.qualify_ordinary_cpu_cycle(true);
    let preferences = app.mii_preferences.clone();
    for role in [Role::Player, Role::Cpu] {
        let mut picker = app.clone();
        open_manual(&mut picker, role);
        let before = committed(&picker);
        seek(&mut picker, 0x48);
        assert!(!picker.roster_excluded(role, 0x48));
        picker.on_a();
        finish_manual_flash(&mut picker, role, 0x48);
        let after = committed(&picker);
        assert_eq!(if role == Role::Player { after.player_character.kind() } else { after.cpu_character.kind() }, 0x48);
        assert_eq!(after.player_roster, before.player_roster);
        assert_eq!(after.cpu_roster, before.cpu_roster);
        assert_eq!(after.player_kirby_hats, before.player_kirby_hats);
        assert_eq!(after.cpu_kirby_hats, before.cpu_kirby_hats);
        assert_eq!(picker.mii_preferences, preferences);
        assert_eq!(picker.take_pending_mii_selection(7), None);
    }
}

#[test]
fn grouped_start_replacement_keeps_slot_and_badges_for_both_roles() {
    let mut app = editor();
    seek(&mut app, 0x5a);
    app.on_a();
    let rank = app.roster_badge(Role::Player, 0x5a);
    app.on_x();
    app.on_a();
    assert_eq!(app.roster_badge(Role::Player, 0x5a), rank);
    assert_eq!(app.roster_badge(Role::Player, 0x5b), rank);
    assert_eq!(
        committed(&app)
            .player_roster
            .fighters()
            .last()
            .unwrap()
            .kind(),
        0x5b
    );
    app.on_zr();
    app.on_x(); // CPU defaults to Pyra; X previews Mythra.
    app.on_a();
    assert_eq!(app.roster_badge(Role::Cpu, 0x5a), Some(5));
    assert_eq!(app.roster_badge(Role::Player, 0x5a), rank);
    seek(&mut app, 0x24);
    app.on_x();
    app.on_a();
    app.on_x();
    app.on_a();
    assert_eq!(
        committed(&app).cpu_roster.fighters().last().unwrap().kind(),
        0x26
    );
    assert_eq!(app.roster_badge(Role::Cpu, 0x24), Some(6));
}

#[test]
fn original_group_is_slot_zero_for_player_only_and_all_retains_start_forms() {
    let mut app = editor();
    app.set_original_player_kind(Some(0x25));
    seek(&mut app, 0x24);
    let before = app.current_settings_to_json();
    app.on_a();
    app.on_x();
    app.on_a();
    assert_eq!(app.current_settings_to_json(), before);
    assert_eq!(app.roster_badge(Role::Player, 0x26), Some(0));
    app.on_zr();
    // CPU preview defaults to Squirtle on role switch.
    app.on_x();
    app.on_x();
    app.on_a();
    assert_eq!(app.roster_badge(Role::Cpu, 0x24), Some(5));
    app.on_zl();
    let menu = committed(&app);
    let cpu = menu.cpu_roster.fighters();
    assert_eq!(cpu[16], TrioFighter::ICE_CLIMBERS);
    assert_eq!(cpu[37].kind(), 0x26);
    let alpha_miis = 3 * usize::from(cfg!(feature = "mii-saved-alpha"));
    assert_eq!(cpu.len(), 83 + alpha_miis);
    app.on_zr();
    app.on_zl();
    assert_eq!(
        committed(&app)
            .player_roster
            .fighters_excluding(Some(0x26))
            .len(),
        82 + alpha_miis
    );
    assert_eq!(app.roster_badge(Role::Player, 0x24), Some(0));
    assert_eq!(app.roster_badge(Role::Cpu, 0x24), Some(38));
}

#[test]
fn grey_original_ic_solo_preferences_persist_independently_and_old_files_default_off() {
    let mut app = editor();
    app.set_original_player_kind(Some(0x4b));
    seek(&mut app, 0x4b);
    let roster = app.roster_values(Role::Player);
    app.on_a();
    app.on_y();
    assert_eq!(app.roster_values(Role::Player), roster);
    assert_eq!(app.roster_badge(Role::Player, 0x4b), Some(0));
    let menu = committed(&app);
    assert_eq!(menu.solo_popo_original, OnOff::OFF);
    assert_eq!(menu.solo_popo_player_cycle, OnOff::ON);
    assert_eq!(menu.solo_popo_cpu, OnOff::OFF);
    app.on_zr();
    app.on_a();
    app.on_y();
    assert_eq!(committed(&app).solo_popo_cpu, OnOff::ON);
    assert!(app.roster_badge(Role::Cpu, 0x4b).is_some());
    let mut reopened = editor();
    reopened.update_all_from_json(&app.current_settings_to_json());
    reopened.set_original_player_kind(Some(0x4b));
    assert!(reopened.roster_solo_enabled(Role::Player));
    assert!(reopened.roster_solo_enabled(Role::Cpu));
    assert_eq!(committed(&reopened).solo_popo_original, OnOff::OFF);
    reopened.on_b();
    let extras = reopened
        .tabs
        .iter()
        .position(|tab| tab.id == "character_extras")
        .unwrap();
    reopened.tabs.state.select(Some(extras));
    assert_eq!(reopened.selected_tab().submenus.len(), 3);
    reopened.selected_tab().submenus.select(0, 0);
    reopened.on_a();
    reopened.selected_submenu().toggles.select(0, 0); // On, through the real extras picker.
    reopened.on_a();
    reopened.on_b();
    assert_eq!(
        (
            committed(&reopened).solo_popo_original,
            committed(&reopened).solo_popo_player_cycle,
            committed(&reopened).solo_popo_cpu
        ),
        (OnOff::ON, OnOff::ON, OnOff::ON)
    );
    let mut old = serde_json::to_value(BASE_MENU).unwrap();
    for key in [
        "solo_popo_original",
        "solo_popo_player_cycle",
        "solo_popo_cpu",
    ] {
        old.as_object_mut().unwrap().remove(key);
    }
    let parsed: TrainingModpackMenu = serde_json::from_value(old).unwrap();
    assert_eq!(
        (
            parsed.solo_popo_original,
            parsed.solo_popo_player_cycle,
            parsed.solo_popo_cpu
        ),
        (OnOff::OFF, OnOff::OFF, OnOff::OFF)
    );
}

#[test]
fn legacy_group_duplicates_migrate_earliest_start_and_renumber() {
    // Immediate previous format: Charizard, Kirby, Squirtle, Mythra, Pyra, Mario.
    let mut values = vec![0u8; 86];
    for (rank, index) in [84, 6, 82, 81, 80, 0].iter().enumerate() {
        values[index + 1] = rank as u8 + 1;
    }
    let migrated: OrderedFighterRoster = serde_json::from_value(serde_json::json!(values)).unwrap();
    let kinds: Vec<_> = migrated.fighters().iter().map(|f| f.kind()).collect();
    assert_eq!(kinds, [0x26, 6, 0x5b, 0]);
    let stored = serde_json::to_value(migrated).unwrap();
    assert_eq!(stored[85], 1);
    assert_eq!(stored[7], 2);
    assert_eq!(stored[82], 3);
    assert_eq!(stored[1], 4);
    assert_eq!(stored[83], 0);
    assert_eq!(stored[81], 0);
    let mut bad = values;
    bad[1] = 7; // malformed original sequence must fail before duplicate migration.
    assert!(serde_json::from_value::<OrderedFighterRoster>(serde_json::json!(bad)).is_err());
    let mut one_hot = vec![0u8; 85];
    one_hot[81] = 1;
    assert_eq!(
        serde_json::from_value::<TrioFighter>(serde_json::json!(one_hot))
            .unwrap()
            .kind(),
        0x5b
    );
}

#[test]
fn all_form_preferences_validate_before_restore_and_old_catalog_suffix_is_cleared() {
    let mut values = vec![0u8; 87];
    values[0] = 1;
    values[82] = 1; // Mythra.
    values[84] = 1; // Ivysaur.
    let all: OrderedFighterRoster = serde_json::from_value(serde_json::json!(values)).unwrap();
    let kinds: Vec<_> = all.fighters().iter().map(|f| f.kind()).collect();
    assert_eq!(kinds[37], 0x25);
    assert!(kinds.contains(&0x5b) && !kinds.contains(&0x5a));
    for bad in [
        {
            let mut v = values.clone();
            v[85] = 1;
            v
        }, // two PT starts
        {
            let mut v = values.clone();
            v[82] = 2;
            v
        }, // preference isn't boolean
        {
            let mut v = values;
            v[1] = 1;
            v
        }, // All and an ordinary rank
    ] {
        assert!(serde_json::from_value::<OrderedFighterRoster>(serde_json::json!(bad)).is_err());
    }
    let mut app = editor();
    seek(&mut app, 0x4b);
    app.on_a();
    let mut old = vec![0u8; 81];
    old[1] = 1; // only Mario, from before IC/forms were appended
    app.update_all_from_json(&serde_json::json!({"player_roster":old}).to_string());
    assert_eq!(
        committed(&app).player_roster.fighters(),
        [TrioFighter::MARIO]
    );
    app.set_original_player_kind(Some(0));
    assert_eq!(app.roster_badge(Role::Player, 0), Some(0));
    let defaults = serde_json::to_string(&BASE_MENU).unwrap();
    app.update_one_from_json(&defaults, "player_roster");
    assert_eq!(app.roster_badge(Role::Player, 0), Some(0));
    assert!(!committed(&app)
        .player_roster
        .fighters_excluding(Some(0))
        .contains(&TrioFighter::MARIO));
}

#[test]
fn persisted_nondefault_solo_and_start_forms_continue_in_fresh_processes() {
    const PHASE: &str = "TRAINING_ROSTER_RESTORE_PHASE";
    const PATH: &str = "TRAINING_ROSTER_RESTORE_PATH";
    if let Ok(phase) = std::env::var(PHASE) {
        let path = std::env::var(PATH).unwrap();
        let mut app = editor();
        if phase == "save" {
            seek(&mut app, 0x5a);
            app.on_x();
            app.on_a(); // non-default Mythra starts at player slot 4 before saving.
            app.on_zl(); // All retains Mythra.
            seek(&mut app, 0x4b);
            app.on_y(); // non-default cycle Solo enabled before saving process exits.
            std::fs::write(path, app.current_settings_to_json()).unwrap();
        } else {
            let text = std::fs::read_to_string(path).unwrap();
            let restored: TrainingModpackMenu = serde_json::from_str(&text).unwrap();
            app.update_all_from_json(&serde_json::to_string(&restored).unwrap());
            assert!(app.roster_solo_enabled(Role::Player));
            assert!(!app.roster_solo_enabled(Role::Cpu));
            assert!(committed(&app)
                .player_roster
                .fighters()
                .iter()
                .any(|f| f.kind() == 0x5b));
            seek(&mut app, 0x4b);
            app.on_y();
            assert_eq!(committed(&app).solo_popo_player_cycle, OnOff::OFF);
            assert_eq!(committed(&app).solo_popo_original, OnOff::OFF);
        }
        return;
    }
    let path = std::env::temp_dir().join(format!("roster-restore-{}.json", std::process::id()));
    for phase in ["save", "restore"] {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "roster_menu_tests::persisted_nondefault_solo_and_start_forms_continue_in_fresh_processes"])
            .env(PHASE, phase).env(PATH, &path).status().unwrap();
        assert!(status.success(), "{} process failed", phase);
    }
    std::fs::remove_file(path).unwrap();
}

fn open_hats(app: &mut App<'_>) {
    if app.page == AppPage::ROSTER {
        app.on_b();
    }
    let extras = app
        .tabs
        .iter()
        .position(|tab| tab.id == "character_extras")
        .unwrap();
    app.tabs.state.select(Some(extras));
    app.selected_tab().submenus.select(0, 1);
    let player = committed(app).player_roster;
    let cpu = committed(app).cpu_roster;
    app.on_a();
    assert!(app.page == AppPage::ROSTER);
    assert_eq!(app.roster_purpose, RosterPurpose::KirbyHats);
    assert_eq!(committed(app).player_roster, player);
    assert_eq!(committed(app).cpu_roster, cpu);
}

fn seek_label(app: &mut App<'_>, label: &str) {
    for _ in 0..7 {
        for _ in 0..13 {
            if app.roster_grid.cursor_info().label == label {
                return;
            }
            app.on_right();
        }
        app.on_down();
    }
    panic!("cell not found: {}", label);
}

#[test]
fn kirby_hats_stay_independent_of_each_role_and_of_fighter_cycles() {
    let mut app = editor();
    let fighters = committed(&app);
    open_hats(&mut app);
    assert_eq!(
        committed(&app).ordered_kirby_hat_kinds(Role::Player),
        vec![0]
    );
    assert_eq!(committed(&app).ordered_kirby_hat_kinds(Role::Cpu), vec![0]);
    seek(&mut app, 0x01);
    app.on_a();
    assert_eq!(
        committed(&app).ordered_kirby_hat_kinds(Role::Player),
        vec![0, 0x01]
    );
    assert_eq!(committed(&app).ordered_kirby_hat_kinds(Role::Cpu), vec![0]);
    app.on_zr();
    seek(&mut app, 0x0f);
    app.on_a();
    assert_eq!(
        committed(&app).ordered_kirby_hat_kinds(Role::Cpu),
        vec![0, 0x0f]
    );
    assert_eq!(
        committed(&app).ordered_kirby_hat_kinds(Role::Player),
        vec![0, 0x01]
    );
    let hats = committed(&app);
    assert_eq!(hats.player_roster, fighters.player_roster);
    assert_eq!(hats.cpu_roster, fighters.cpu_roster);
    app.on_b();
    let cycles = app
        .tabs
        .iter()
        .position(|tab| tab.id == "player_characters")
        .unwrap();
    app.tabs.state.select(Some(cycles));
    app.selected_tab().submenus.select(0, 3);
    app.on_a();
    assert_eq!(app.roster_purpose, RosterPurpose::FighterCycles);
    if app.roster_grid.role() == Role::Cpu {
        app.on_zr();
    }
    seek(&mut app, 0x05);
    app.on_a();
    let after = committed(&app);
    assert_eq!(after.player_kirby_hats, hats.player_kirby_hats);
    assert_eq!(after.cpu_kirby_hats, hats.cpu_kirby_hats);
    assert_ne!(after.player_roster, fighters.player_roster);
}

#[test]
fn hat_forms_and_all_use_css_order_without_kirby() {
    let mut app = editor();
    let fighters = committed(&app).player_roster;
    open_hats(&mut app);
    seek(&mut app, 0x5a);
    let before = app.current_settings_to_json();
    app.on_x();
    assert_eq!(app.roster_grid.cursor_info().kind(), Some(0x5b));
    assert_eq!(app.current_settings_to_json(), before);
    app.on_a();
    assert_eq!(committed(&app).player_roster, fighters);
    assert!(committed(&app)
        .ordered_kirby_hat_kinds(Role::Player)
        .ends_with(&[0x5b]));
    app.on_zl();
    let kinds = committed(&app).ordered_kirby_hat_kinds(Role::Player);
    let catalog = OrderedFighterRoster::all().hat_kinds();
    assert_eq!(kinds.len(), catalog.len());
    assert_eq!(kinds.first(), Some(&0));
    assert!(!kinds.contains(&0x06));
    assert!(kinds.contains(&0x5b) && !kinds.contains(&0x5a));
    assert_eq!(committed(&app).ordered_kirby_hat_kinds(Role::Cpu), vec![0]);
}

#[test]
fn hats_keep_original_fighters_and_highlight_the_original_cpu() {
    let mut app = editor();
    open_hats(&mut app);
    seek(&mut app, 0x08);
    app.on_a();
    app.set_original_player_kind(Some(0x08));
    let after_exclusion = app.current_settings_to_json();
    app.set_original_cpu_kind(Some(0x25));
    assert_eq!(app.current_settings_to_json(), after_exclusion);
    let menu = committed(&app);
    assert!(menu
        .ordered_kirby_hat_kinds(Role::Player)
        .contains(&0x08));
    assert_eq!(app.roster_badge(Role::Player, 0x08), Some(2));
    assert!(!app.roster_excluded(Role::Player, 0x08));
    assert!(menu
        .player_roster
        .fighters()
        .iter()
        .all(|fighter| fighter.kind() != 0x08));
    assert!(app.highlights_original_cpu(0x24));
    assert!(app.highlights_original_cpu(0x26));
    assert!(!app.highlights_original_cpu(0));
    assert!(app.hat_cell_disabled(0x06));
    let hats = menu.player_kirby_hats;
    app.set_original_cpu_kind(Some(0x06));
    assert!(app.highlights_original_cpu(0x06));
    assert_eq!(committed(&app).player_kirby_hats, hats);
    assert_eq!(app.current_settings_to_json(), after_exclusion);
    app.on_b();
    let cycles = app
        .tabs
        .iter()
        .position(|tab| tab.id == "player_characters")
        .unwrap();
    app.tabs.state.select(Some(cycles));
    app.selected_tab().submenus.select(0, 3);
    app.on_a();
    assert!(!app.highlights_original_cpu(0x06));
    assert!(app.roster_excluded(Role::Player, 0x08));
}

#[test]
fn kirby_and_mii_cannot_be_chosen_and_hats_have_no_solo_action() {
    let mut app = editor();
    open_hats(&mut app);
    let values = app.roster_values(Role::Player);
    seek(&mut app, 0x06);
    app.on_a();
    app.on_r();
    assert_eq!(app.roster_values(Role::Player), values);
    seek_label(&mut app, "Mii Brawler");
    assert_eq!(
        app.roster_grid.cursor_info().content,
        training_mod_tui::roster_grid::CellContent::Unsupported
    );
    app.on_a();
    assert_eq!(app.roster_values(Role::Player), values);
    seek(&mut app, 0x4b);
    app.on_y();
    assert_eq!(app.roster_values(Role::Player), values);
    let menu = committed(&app);
    assert_eq!(menu.solo_popo_player_cycle, OnOff::OFF);
    assert_eq!(menu.solo_popo_cpu, OnOff::OFF);
    assert_eq!(menu.solo_popo_original, OnOff::OFF);
}

#[test]
fn hat_defaults_restore_one_role_and_old_files_stay_on_mario() {
    let mut app = editor();
    open_hats(&mut app);
    seek(&mut app, 0x01);
    app.on_a();
    app.on_zr();
    seek(&mut app, 0x0f);
    app.on_a();
    let cpu = committed(&app).cpu_kirby_hats;
    let defaults = app.serialized_default_settings.clone();
    app.update_one_from_json(&defaults, "player_kirby_hats");
    let menu = committed(&app);
    assert_eq!(menu.ordered_kirby_hat_kinds(Role::Player), vec![0]);
    assert_eq!(menu.cpu_kirby_hats, cpu);
    app.load_defaults();
    let reset = committed(&app);
    assert_eq!(reset.ordered_kirby_hat_kinds(Role::Player), vec![0]);
    assert_eq!(reset.ordered_kirby_hat_kinds(Role::Cpu), vec![0]);
    assert_eq!(reset.player_roster, BASE_MENU.player_roster);
    let mut old = serde_json::to_value(BASE_MENU).unwrap();
    old.as_object_mut().unwrap().remove("player_kirby_hats");
    old.as_object_mut().unwrap().remove("cpu_kirby_hats");
    let text = format!("{{\"menu\":{old},\"defaults_menu\":{old}}}");
    let parsed: MenuJsonStruct = serde_json::from_str(&text).unwrap();
    assert_eq!(
        parsed.menu.ordered_kirby_hat_kinds(Role::Player),
        vec![0]
    );
    assert_eq!(parsed.menu.ordered_kirby_hat_kinds(Role::Cpu), vec![0]);
    assert_eq!(parsed.menu.player_roster, BASE_MENU.player_roster);
    assert_eq!(parsed.menu.cpu_roster, BASE_MENU.cpu_roster);
    assert_eq!(parsed.defaults_menu.player_kirby_hats, DEFAULT_KIRBY_HATS);
}

#[test]
fn kirby_hat_roles_restore_in_a_fresh_process() {
    const PHASE: &str = "TRAINING_KIRBY_HAT_RESTORE_PHASE";
    const PATH: &str = "TRAINING_KIRBY_HAT_RESTORE_PATH";
    if let Ok(phase) = std::env::var(PHASE) {
        let path = std::env::var(PATH).unwrap();
        let mut app = editor();
        if phase == "save" {
            open_hats(&mut app);
            seek(&mut app, 0x5a);
            app.on_x();
            app.on_a();
            app.on_zr();
            seek(&mut app, 0x0f);
            app.on_a();
            std::fs::write(path, app.current_settings_to_json()).unwrap();
        } else {
            let text = std::fs::read_to_string(path).unwrap();
            let restored: TrainingModpackMenu = serde_json::from_str(&text).unwrap();
            app.update_all_from_json(&serde_json::to_string(&restored).unwrap());
            assert_eq!(
                committed(&app).ordered_kirby_hat_kinds(Role::Player),
                vec![0, 0x5b]
            );
            assert_eq!(
                committed(&app).ordered_kirby_hat_kinds(Role::Cpu),
                vec![0, 0x0f]
            );
            assert_eq!(committed(&app).player_roster, BASE_MENU.player_roster);
            assert_eq!(committed(&app).cpu_roster, BASE_MENU.cpu_roster);
            assert!(!committed(&app)
                .ordered_kirby_hat_kinds(Role::Player)
                .contains(&0x06));
        }
        return;
    }
    let path = std::env::temp_dir().join(format!("kirby-hats-{}.json", std::process::id()));
    for phase in ["save", "restore"] {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "roster_menu_tests::kirby_hat_roles_restore_in_a_fresh_process",
            ])
            .env(PHASE, phase)
            .env(PATH, &path)
            .status()
            .unwrap();
        assert!(status.success(), "{} process failed", phase);
    }
    std::fs::remove_file(path).unwrap();
}
