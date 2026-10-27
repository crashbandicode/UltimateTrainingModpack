mod containers;
pub use containers::*;
pub mod macro_panel;
pub mod replay_panel;
pub mod clip_panel;
pub mod clip_browser;
pub mod roster_grid;
pub mod mii_menu;
pub mod stage_menu;
pub mod stage_forms;
mod structures;
pub use structures::*;

pub const NX_SUBMENU_ROWS: usize = 8;
pub const NX_SUBMENU_COLUMNS: usize = 4;

/// Index of the one selected toggle in an open single-choice submenu.
/// Declaration order matches the consts byteflags. An absent id, a non-single
/// submenu, an empty row, or more than one selected toggle is `None`.
/// This does not serialize or commit the menu.
pub fn single_toggle_index(app: &App, id: &str) -> Option<usize> {
    let submenu = app
        .tabs
        .iter()
        .flat_map(|tab| tab.submenus.iter())
        .chain(app.hidden_submenus.iter())
        .find(|submenu| submenu.id == id)?;
    if !matches!(submenu.submenu_type, SubMenuType::ToggleSingle) {
        return None;
    }
    let mut selected = None;
    for (index, toggle) in submenu.toggles.iter().enumerate() {
        if toggle.value == 0 {
            continue;
        }
        if selected.is_some() {
            return None;
        }
        selected = Some(index);
    }
    selected
}

pub mod resume;

#[cfg(test)]
mod single_toggle_tests {
    use super::*;

    fn row(id: &'static str, values: &[u8]) -> SubMenu<'static> {
        let toggles = values
            .iter()
            .map(|value| Toggle {
                title: "Slot",
                value: *value,
                max: 1,
            })
            .collect();
        SubMenu {
            title: id,
            id,
            help_text: "",
            submenu_type: SubMenuType::ToggleSingle,
            toggles: StatefulTable::with_items(NX_SUBMENU_ROWS, NX_SUBMENU_COLUMNS, toggles),
            slider: None,
        }
    }

    fn app_with(submenu: SubMenu<'static>) -> App<'static> {
        let mut app = App::new();
        app.tabs = StatefulList::with_items(vec![Tab {
            title: "Save States",
            id: "save_states",
            submenus: StatefulTable::with_items(1, 1, vec![submenu]),
        }]);
        app
    }

    #[test]
    fn open_slot_two_is_index_one_and_an_empty_row_is_not_slot_one() {
        let selected = app_with(row("save_state_slot", &[0, 1, 0, 0, 0]));
        assert_eq!(single_toggle_index(&selected, "save_state_slot"), Some(1));
        let empty = app_with(row("save_state_slot", &[0, 0, 0, 0, 0]));
        assert_eq!(single_toggle_index(&empty, "save_state_slot"), None);
        assert_eq!(single_toggle_index(&selected, "recording_slot"), None);
    }

    #[test]
    fn two_selected_toggles_and_a_missing_id_are_not_a_slot() {
        assert_eq!(single_toggle_index(&app_with(row("recording_slot", &[1, 1])), "recording_slot"), None);
    }

    #[test]
    fn save_enable_on_is_the_first_toggle() {
        let mut app = App::new();
        app.hidden_submenus.push(row("save_state_enable", &[1, 0]));
        assert_eq!(single_toggle_index(&app, "save_state_enable"), Some(0));
        app.hidden_submenus[0] = row("save_state_enable", &[0, 1]);
        assert_eq!(single_toggle_index(&app, "save_state_enable"), Some(1));
    }
}
