//! Real settings ingress for the Training rotation activation after the saving
//! process has exited: the toggle and one rotation form change through the real
//! menu, the file is read by the production file loader in a second process,
//! and editing continues from the restored state. Native entry is a separate
//! runtime check.
#![cfg(feature = "training-stage-rotation")]

use std::process::Command;
use training_mod_consts::{
    lobby_stages::{Form, Selection, STAGES}, menu_json::parse_menu_file, MenuJsonStruct, OnOff, BASE_MENU,
};
use training_mod_tui::{stage_menu::Preferences, App, AppPage, InputControl};

const CHILD: &str = "training_rotation_restore_child";
const PHASE: &str = "ROTATION185_RESTORE_PHASE";
const PATH: &str = "ROTATION185_RESTORE_PATH";

fn menu_app(settings: &str, preferences: Preferences) -> App<'static> {
    let mut app = unsafe { training_mod_consts::create_app() };
    app.serialized_default_settings = serde_json::to_string(&BASE_MENU).unwrap();
    app.open_settings_for_player(settings, Some(9));
    app.open_stage_menu(true, preferences);
    app
}

/// Open one submenu through the real tab and submenu navigation.
fn open_submenu(app: &mut App<'_>, id: &str) {
    app.page = AppPage::SUBMENU;
    let tab = app.tabs.iter().position(|tab| tab.submenus.iter().any(|submenu| submenu.id == id)).unwrap();
    app.tabs.state.select(Some(tab));
    let index = app.selected_tab().submenus.iter().position(|submenu| submenu.id == id).unwrap();
    let cols = app.selected_tab().submenus.cols;
    app.selected_tab().submenus.select(index / cols, index % cols);
    app.on_a();
    assert!(app.page == AppPage::TOGGLE);
}

fn set_training_rotation(app: &mut App<'_>, on: bool) {
    open_submenu(app, "training_stage_rotation");
    assert_eq!(app.selected_submenu().toggles.get(0, 0).unwrap().title, "On");
    assert_eq!(app.selected_submenu().toggles.get(0, 1).unwrap().title, "Off");
    app.selected_submenu().toggles.select(0, usize::from(!on));
    app.on_a();
    app.on_b();
}

/// Y Form on a highlighted rotation stage; never a live request.
fn cycle_rotation_form(app: &mut App<'_>, stage: u32) {
    open_submenu(app, "lobby_stages");
    assert!(app.stage_page_active());
    assert!(!app.stage_is_direct());
    let index = STAGES.iter().position(|(id, _)| *id == stage).unwrap() + 1;
    let cols = app.selected_submenu().toggles.cols;
    app.selected_submenu().toggles.select(index / cols, index % cols);
    app.on_y();
    app.on_b();
}

fn selections(saved: &MenuJsonStruct) -> Vec<Selection> {
    saved.menu.lobby_stages.selections(&saved.stage_preferences.rotation_forms).unwrap()
}

#[test]
fn training_rotation_activation_and_form_continue_after_the_saving_process_exits() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("rotation185-{}-{nonce}.json", std::process::id()));
    for phase in ["save", "restore"] {
        // The save child has exited successfully before the restore child starts.
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", CHILD, "--nocapture"])
            .env(PHASE, phase).env(PATH, &path).output().unwrap();
        println!("phase={phase} status={}\n{}\n{}", output.status,
            String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert!(output.status.success(), "{phase} failed; retained fixture at {}", path.display());
        assert!(String::from_utf8_lossy(&output.stdout).contains(&format!("rotation185-{phase}-complete")),
            "child did not execute the selected phase");
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "invoked twice by training_rotation_activation_and_form_continue_after_the_saving_process_exits"]
fn training_rotation_restore_child() {
    let path = std::env::var_os(PATH).expect("restore child path");
    match std::env::var(PHASE).as_deref() {
        Ok("save") => {
            let mut menu = BASE_MENU;
            menu.lobby_stages = serde_json::from_str("[0,0,1,2]").unwrap(); // BF rank 1, FD rank 2.
            assert_eq!(menu.training_stage_rotation, OnOff::OFF);
            let mut app = menu_app(&serde_json::to_string(&menu).unwrap(), Preferences::default());
            set_training_rotation(&mut app, true);
            cycle_rotation_form(&mut app, 0); // Battlefield Normal -> Omega through Y Form.
            app.on_start();
            assert!(app.take_pending_stage().is_none());
            let text = app.get_serialized_settings_with_defaults();
            let saved: MenuJsonStruct = serde_json::from_str(&text).unwrap();
            assert_eq!(saved.menu.training_stage_rotation, OnOff::ON);
            assert_eq!(saved.defaults_menu.training_stage_rotation, OnOff::OFF);
            assert_eq!(selections(&saved),
                vec![Selection { stage: 0, form: Form::Omega }, Selection::normal(3)]);
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).unwrap();
            std::io::Write::write_all(&mut file, text.as_bytes()).unwrap();
            file.sync_all().unwrap();
            println!("rotation185-save-complete: menu turned Training Stage Rotation On and set BF/Omega; ranks BF1 FD2; no live request");
        }
        Ok("restore") => {
            // The production file loader, not only the live-response parser.
            let loaded = parse_menu_file(&std::fs::read_to_string(path).unwrap()).unwrap();
            assert!(loaded.rejected_stage_preferences.is_none());
            let saved = loaded.settings;
            assert_eq!(saved.menu.training_stage_rotation, OnOff::ON);
            assert_eq!(saved.defaults_menu.training_stage_rotation, OnOff::OFF);
            assert_eq!(selections(&saved),
                vec![Selection { stage: 0, form: Form::Omega }, Selection::normal(3)]);
            let mut app = menu_app(&serde_json::to_string(&saved.menu).unwrap(), saved.stage_preferences);
            let restored: MenuJsonStruct =
                serde_json::from_str(&app.get_serialized_settings_with_defaults()).unwrap();
            assert_eq!(restored.menu.training_stage_rotation, OnOff::ON);
            set_training_rotation(&mut app, false);
            cycle_rotation_form(&mut app, 0); // Continue from restored Omega, wrapping to Normal.
            app.on_start();
            assert!(app.take_pending_stage().is_none());
            let continued: MenuJsonStruct =
                serde_json::from_str(&app.get_serialized_settings_with_defaults()).unwrap();
            assert_eq!(continued.menu.training_stage_rotation, OnOff::OFF);
            assert_eq!(selections(&continued), vec![Selection::normal(0), Selection::normal(3)]);
            assert_eq!(continued.menu.lobby_stages, saved.menu.lobby_stages);
            assert_eq!(continued.defaults_menu.training_stage_rotation, OnOff::OFF);
            println!("rotation185-restore-complete: restored On + BF/Omega through parse_menu_file, continued to Off + Normal; ranks BF1 FD2 preserved; no live request");
        }
        phase => panic!("unexpected child phase {:?}", phase),
    }
}
