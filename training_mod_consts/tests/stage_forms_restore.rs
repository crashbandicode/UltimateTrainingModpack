//! Real settings ingress after the saving process has exited. Native game entry
//! is a separate runtime check; this covers persisted ranks/forms and UI intent.

use std::process::Command;
use training_mod_consts::{lobby_stages::{Form, Selection, STAGES}, MenuJsonStruct, BASE_MENU};
use training_mod_tui::{stage_menu::Preferences, App, InputControl};

const CHILD: &str = "stage_forms_restore_child";
const PHASE: &str = "STAGE185_RESTORE_PHASE";
const PATH: &str = "STAGE185_RESTORE_PATH";

fn picker(settings: &str, preferences: Preferences) -> App<'static> {
    let mut app = unsafe { training_mod_consts::create_app() };
    app.serialized_default_settings = serde_json::to_string(&BASE_MENU).unwrap();
    app.open_settings_for_player(settings, Some(9));
    app.open_stage_menu(true, preferences);
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

fn select_stage(app: &mut App<'_>, stage: u32) {
    let index = STAGES.iter().position(|(id, _)| *id == stage).unwrap() + 1;
    let cols = app.selected_submenu().toggles.cols;
    app.selected_submenu().toggles.select(index / cols, index % cols);
}

#[test]
fn stage_forms_continue_after_the_saving_process_exits() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("stage185-{}-{nonce}.json", std::process::id()));
    for phase in ["save", "restore"] {
        // Waiting for save's successful exit happens before restore is launched.
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", CHILD, "--nocapture"])
            .env(PHASE, phase).env(PATH, &path).output().unwrap();
        println!("phase={phase} status={}\n{}\n{}", output.status,
            String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert!(output.status.success(), "{phase} failed; retained fixture at {}", path.display());
        assert!(String::from_utf8_lossy(&output.stdout).contains(&format!("stage185-{phase}-complete")),
            "child did not execute the selected phase");
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "invoked twice by stage_forms_continue_after_the_saving_process_exits"]
fn stage_forms_restore_child() {
    let path = std::env::var_os(PATH).expect("restore child path");
    match std::env::var(PHASE).as_deref() {
        Ok("save") => {
            let mut menu = BASE_MENU;
            menu.lobby_stages = serde_json::from_str("[0,0,1,2]").unwrap();
            let mut app = picker(&serde_json::to_string(&menu).unwrap(), Preferences::default());
            assert!(!app.stage_is_direct());
            select_stage(&mut app, 0);
            app.on_y(); // Actual UI edit before saving, Normal -> Omega.
            select_stage(&mut app, 3);
            app.on_y(); // Final Destination skips absent Omega -> Battlefield.
            app.on_b(); app.on_start();
            assert!(app.take_pending_stage().is_none());
            let text = app.get_serialized_settings_with_defaults();
            let saved: MenuJsonStruct = serde_json::from_str(&text).unwrap();
            assert_eq!(saved.menu.lobby_stages.selections(&saved.stage_preferences.rotation_forms).unwrap(),
                vec![Selection { stage: 0, form: Form::Omega }, Selection { stage: 3, form: Form::Battlefield }]);
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).unwrap();
            std::io::Write::write_all(&mut file, text.as_bytes()).unwrap();
            file.sync_all().unwrap();
            println!("stage185-save-complete: UI saved BF/Omega and FD/Battlefield; ranks BF1 FD2; no live request");
        }
        Ok("restore") => {
            let saved: MenuJsonStruct = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!(saved.menu.lobby_stages.selections(&saved.stage_preferences.rotation_forms).unwrap(),
                vec![Selection { stage: 0, form: Form::Omega }, Selection { stage: 3, form: Form::Battlefield }]);
            let mut app = picker(&serde_json::to_string(&saved.menu).unwrap(), saved.stage_preferences);
            select_stage(&mut app, 0);
            app.on_y(); // Continue from restored Omega, wrapping to Normal.
            select_stage(&mut app, 3);
            app.on_y(); // Continue from restored Battlefield, wrapping to Normal.
            app.on_b(); app.on_start();
            assert!(app.take_pending_stage().is_none());
            let continued: MenuJsonStruct = serde_json::from_str(&app.get_serialized_settings_with_defaults()).unwrap();
            assert_eq!(continued.menu.lobby_stages.selections(&continued.stage_preferences.rotation_forms).unwrap(),
                vec![Selection::normal(0), Selection::normal(3)]);
            assert_eq!(continued.menu.lobby_stages, saved.menu.lobby_stages);
            println!("stage185-restore-complete: resumed both forms -> Normal; ranks BF1 FD2 preserved; no live request");
        }
        phase => panic!("unexpected child phase {:?}", phase),
    }
}
