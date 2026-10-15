use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;
use std::collections::HashMap;

use crate::{InputControl, StatefulList, SubMenu, SubMenuType, Tab};
use crate::roster_grid::{Direction, Outcome, Role, RosterGrid};

#[derive(PartialEq, Serialize, Clone, Copy)]
pub enum AppPage {
    SUBMENU,
    TOGGLE,
    SLIDER,
    CONFIRMATION,
    CLOSE,
    ROSTER,
}

#[derive(PartialEq, Clone, Copy)]
pub enum ConfirmationState {
    HoverNo,
    HoverYes,
}

impl ConfirmationState {
    pub fn switch(&self) -> ConfirmationState {
        match self {
            ConfirmationState::HoverNo => ConfirmationState::HoverYes,
            ConfirmationState::HoverYes => ConfirmationState::HoverNo,
        }
    }
}

// Menu structure is:
// App <StatefulTable<Tab>>
// │
// └─ Tab <StatefulTable<Submenu>>
//    │
//    └─ Submenu <Struct>
//       │
//       ├─ StatefulTable<Toggle>
//       │
//       │  OR
//       │
//       └─ Option<Slider>
#[derive(Clone)]
pub struct App<'a> {
    pub tabs: StatefulList<Tab<'a>>,
    pub page: AppPage,
    pub serialized_settings: String,
    pub serialized_default_settings: String,
    pub confirmation_state: ConfirmationState,
    pub confirmation_return_page: AppPage,
    /// Settings with no separate launch control, still serialized and reset normally.
    pub hidden_submenus: Vec<SubMenu<'a>>,
    pub roster_grid: RosterGrid,
    pub roster_kinds: Vec<u32>,
}

impl<'a> App<'a> {
    pub fn new() -> App<'a> {
        App {
            tabs: StatefulList::new(),
            page: AppPage::SUBMENU,
            serialized_settings: String::new(),
            serialized_default_settings: String::new(),
            confirmation_state: ConfirmationState::HoverNo,
            confirmation_return_page: AppPage::SUBMENU,
            hidden_submenus: Vec::new(),
            roster_grid: RosterGrid::new(Role::Player),
            roster_kinds: Vec::new(),
        }
    }

    pub fn current_settings_to_json(&self) -> String {
        serde_json::to_string(&self).expect("Could not serialize the menu to JSON!")
    }

    pub fn get_serialized_settings_with_defaults(&self) -> String {
        format!(
            "{{\"menu\":{}, \"defaults_menu\":{}}}",
            self.serialized_settings, self.serialized_default_settings
        )
    }

    pub fn save_settings(&mut self) {
        self.serialized_settings = self.current_settings_to_json();
    }

    pub fn save_default_settings(&mut self) {
        self.serialized_default_settings = self.current_settings_to_json();
    }

    pub fn load_defaults(&mut self) {
        // TODO!() is there a way to do this without cloning?
        let json = self.serialized_default_settings.clone();
        self.update_all_from_json(&json);
    }

    pub fn load_defaults_for_current_submenu(&mut self) {
        let submenu_id = self.selected_submenu().id;
        let json = self.serialized_default_settings.clone();
        self.update_one_from_json(&json, submenu_id);
    }

    pub fn update_all_from_json(&mut self, json: &str) {
        let all_settings: HashMap<String, Vec<u8>> =
            serde_json::from_str(json).expect("Could not parse the json!");
        for tab in self.tabs.iter_mut() {
            for submenu_opt in tab.submenus.iter_mut() {
                if let Some(submenu) = submenu_opt {
                    if let Some(val) = all_settings.get(submenu.id) {
                        submenu.update_from_vec(val.clone());
                    }
                }
            }
        }
        for submenu in &mut self.hidden_submenus {
            if let Some(val) = all_settings.get(submenu.id) {
                submenu.update_from_vec(val.clone());
            }
        }
        self.save_settings();
    }

    #[allow(unused_labels)]
    pub fn update_one_from_json(&mut self, json: &str, submenu_id: &str) {
        let all_settings: HashMap<String, Vec<u8>> =
            serde_json::from_str(json).expect("Could not parse the json!");
        if let Some(val) = all_settings.get(submenu_id) {
            for submenu in &mut self.hidden_submenus {
                if submenu.id == submenu_id {
                    submenu.update_from_vec(val.clone());
                }
            }
            // No need to iterate through all the submenus if the id doesn't exist in the hashmap
            'tabs_scope: for tab in self.tabs.iter_mut() {
                'submenus_scope: for submenu_opt in tab.submenus.iter_mut() {
                    if let Some(submenu) = submenu_opt {
                        if submenu.id == submenu_id {
                            submenu.update_from_vec(val.clone());
                            break 'tabs_scope;
                        }
                    }
                }
            }
        }
        self.save_settings();
    }

    pub fn confirm(&mut self) -> bool {
        self.confirmation_state == ConfirmationState::HoverYes
    }

    fn roster_id(role: Role) -> &'static str {
        match role {
            Role::Player => "player_roster",
            Role::Cpu => "cpu_roster",
        }
    }

    pub fn roster_values(&self, role: Role) -> Vec<u8> {
        self.tabs.iter()
            .flat_map(|tab| tab.submenus.iter())
            .chain(self.hidden_submenus.iter())
            .find(|submenu| submenu.id == Self::roster_id(role))
            .map(|submenu| submenu.toggles.iter().map(|toggle| toggle.value).collect())
            .unwrap_or_default()
    }

    fn set_roster_values(&mut self, role: Role, values: Vec<u8>) {
        let id = Self::roster_id(role);
        for tab in self.tabs.iter_mut() {
            for submenu in tab.submenus.iter_mut().flatten() {
                if submenu.id == id {
                    submenu.update_from_vec(values);
                    return;
                }
            }
        }
        if let Some(submenu) = self.hidden_submenus.iter_mut().find(|submenu| submenu.id == id) {
            submenu.update_from_vec(values);
        }
    }

    fn edit_roster(&mut self, remove: bool) {
        let role = self.roster_grid.role();
        let mut values = self.roster_values(role);
        let changed = if remove {
            self.roster_grid.remove(&self.roster_kinds, &mut values)
        } else {
            self.roster_grid.confirm(&self.roster_kinds, &mut values) == Outcome::RosterChanged
        };
        if changed {
            self.set_roster_values(role, values);
            self.save_settings();
        }
    }

    pub fn return_from_confirmation(&mut self) {
        self.confirmation_state = ConfirmationState::HoverNo;
        self.page = self.confirmation_return_page;
    }

    pub fn selected_tab(&mut self) -> &mut Tab<'a> {
        self.tabs.get_selected().expect("No tab selected!")
    }

    pub fn selected_submenu(&mut self) -> &mut SubMenu<'a> {
        self.selected_tab()
            .submenus
            .get_selected()
            .expect("No submenu selected!")
    }

    pub fn should_show_clear_keyhelp(&mut self) -> bool {
        // Only show the "Clear Toggle" keyhelp if all of the following are true
        // 1. app.page is TOGGLE,
        // 2. selected_submenu.submenu_type is ToggleMultiple
        // 3. the toggle can be set to values greater than 1 (i.e. its not a boolean toggle)
        if self.page != AppPage::TOGGLE {
            return false;
        }
        let submenu = self.selected_submenu();
        match submenu.submenu_type {
            SubMenuType::ToggleMultiple => submenu.selected_toggle().max > 1,
            _ => false,
        }
    }
}

impl<'a> Serialize for App<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serializes as a mapping between submenu titles and values
        // Need to iterate through tabs to avoid making a list of mappings
        let len: usize = self.tabs.iter().map(|tab| tab.len()).sum::<usize>() + self.hidden_submenus.len();
        let mut map = serializer.serialize_map(Some(len))?;
        for tab in self.tabs.iter() {
            for submenu in tab.submenus.iter() {
                map.serialize_entry(submenu.id, submenu)?;
            }
        }
        for submenu in &self.hidden_submenus {
            map.serialize_entry(submenu.id, submenu)?;
        }
        map.end()
    }
}

impl<'a> InputControl for App<'a> {
    fn on_a(&mut self) {
        match self.page {
            AppPage::SUBMENU => {
                if self.selected_submenu().id == "player_roster" && !self.roster_kinds.is_empty() {
                    self.page = AppPage::ROSTER;
                    return;
                }
                // Large single-choice pickers reopen on the configured option,
                // including choices beyond the first screen of the native UI.
                let submenu = self.selected_submenu();
                if matches!(submenu.submenu_type, SubMenuType::ToggleSingle)
                    && submenu.toggles.rows > crate::NX_SUBMENU_ROWS
                {
                    let selected = submenu.toggles.iter().position(|toggle| toggle.value != 0);
                    if let Some(index) = selected {
                        submenu
                            .toggles
                            .select(index / submenu.toggles.cols, index % submenu.toggles.cols);
                    }
                }
                self.page = match self.selected_submenu().submenu_type {
                    SubMenuType::ToggleSingle => AppPage::TOGGLE,
                    SubMenuType::ToggleMultiple | SubMenuType::Ordered => AppPage::TOGGLE,
                    SubMenuType::Slider => AppPage::SLIDER,
                };
                self.selected_tab().on_a()
            }
            AppPage::TOGGLE => self.selected_submenu().on_a(),
            AppPage::SLIDER => self.selected_submenu().on_a(),
            AppPage::CONFIRMATION => {
                // For resetting defaults
                // TODO: Is this the right place for this logic?
                if self.confirm() {
                    match self.confirmation_return_page {
                        AppPage::SUBMENU => {
                            // Reset ALL settings to default
                            self.load_defaults();
                        }
                        AppPage::TOGGLE | AppPage::SLIDER => {
                            // Reset current submenu to default
                            self.load_defaults_for_current_submenu();
                        }
                        _ => {}
                    }
                }
                self.return_from_confirmation();
            }
            AppPage::CLOSE => {}
            AppPage::ROSTER => self.edit_roster(false),
        }
        self.save_settings(); // A button can make changes, update the serialized settings
    }
    fn on_b(&mut self) {
        match self.page {
            AppPage::SUBMENU => {
                // Exit the app
                self.page = AppPage::CLOSE;
            }
            AppPage::TOGGLE | AppPage::ROSTER => {
                // Return to the list of submenus
                self.page = AppPage::SUBMENU;
            }
            AppPage::SLIDER => {
                // Return to the list of submenus if we don't have a slider handle selected
                let slider = self
                    .selected_submenu()
                    .slider
                    .as_mut()
                    .expect("No slider selected!");
                if !slider.is_handle_selected() {
                    self.page = AppPage::SUBMENU;
                } else {
                    self.selected_submenu().on_b();
                }
            }
            AppPage::CONFIRMATION => {
                // Return to the list of submenus
                self.return_from_confirmation();
            }
            AppPage::CLOSE => {}
        }
        self.save_settings(); // B button can make changes, update the serialized settings
    }
    fn on_x(&mut self) {
        if self.page == AppPage::ROSTER {
            self.roster_grid.cycle_form();
        } else {
            self.save_default_settings();
        }
    }
    fn on_y(&mut self) {
        // Clear current toggle, for toggles w/ weighted selections
        match self.page {
            AppPage::TOGGLE => self.selected_submenu().on_y(),
            AppPage::ROSTER => self.roster_grid.toggle_role(),
            _ => {}
        }
    }
    fn on_up(&mut self) {
        match self.page {
            AppPage::ROSTER => self.roster_grid.step(Direction::Up),
            AppPage::SUBMENU => self.tabs.get_selected().expect("No tab selected!").on_up(),
            AppPage::TOGGLE => self
                .tabs
                .get_selected()
                .expect("No tab selected!")
                .submenus
                .get_selected()
                .expect("No submenu selected!")
                .on_up(),
            AppPage::SLIDER => self
                .tabs
                .get_selected()
                .expect("No tab selected!")
                .submenus
                .get_selected()
                .expect("No submenu selected!")
                .on_up(),
            AppPage::CONFIRMATION => {}
            AppPage::CLOSE => {}
        }
    }
    fn on_down(&mut self) {
        match self.page {
            AppPage::ROSTER => self.roster_grid.step(Direction::Down),
            AppPage::SUBMENU => self
                .tabs
                .get_selected()
                .expect("No tab selected!")
                .on_down(),
            AppPage::TOGGLE => self
                .tabs
                .get_selected()
                .expect("No tab selected!")
                .submenus
                .get_selected()
                .expect("No submenu selected!")
                .on_down(),
            AppPage::SLIDER => self
                .tabs
                .get_selected()
                .expect("No tab selected!")
                .submenus
                .get_selected()
                .expect("No submenu selected!")
                .on_down(),
            AppPage::CONFIRMATION => {}
            AppPage::CLOSE => {}
        }
    }
    fn on_left(&mut self) {
        match self.page {
            AppPage::ROSTER => self.roster_grid.step(Direction::Left),
            AppPage::SUBMENU => self
                .tabs
                .get_selected()
                .expect("No tab selected!")
                .on_left(),
            AppPage::TOGGLE => self
                .tabs
                .get_selected()
                .expect("No tab selected!")
                .submenus
                .get_selected()
                .expect("No submenu selected!")
                .on_left(),
            AppPage::SLIDER => self
                .tabs
                .get_selected()
                .expect("No tab selected!")
                .submenus
                .get_selected()
                .expect("No submenu selected!")
                .on_left(),
            AppPage::CONFIRMATION => self.confirmation_state = self.confirmation_state.switch(),
            AppPage::CLOSE => {}
        }
    }
    fn on_right(&mut self) {
        match self.page {
            AppPage::ROSTER => self.roster_grid.step(Direction::Right),
            AppPage::SUBMENU => self
                .tabs
                .get_selected()
                .expect("No tab selected!")
                .on_right(),
            AppPage::TOGGLE => self
                .tabs
                .get_selected()
                .expect("No tab selected!")
                .submenus
                .get_selected()
                .expect("No submenu selected!")
                .on_right(),
            AppPage::SLIDER => self
                .tabs
                .get_selected()
                .expect("No tab selected!")
                .submenus
                .get_selected()
                .expect("No submenu selected!")
                .on_right(),
            AppPage::CONFIRMATION => self.confirmation_state = self.confirmation_state.switch(),
            AppPage::CLOSE => {}
        }
    }
    fn on_start(&mut self) {
        // Close menu
        self.page = AppPage::CLOSE;
    }
    fn on_l(&mut self) {}
    fn on_r(&mut self) {
        if self.page == AppPage::ROSTER {
            self.edit_roster(true);
            return;
        }
        // Reset settings to default
        // See App::on_a() for the logic
        self.confirmation_return_page = self.page;
        self.page = AppPage::CONFIRMATION;
    }
    fn on_zl(&mut self) {
        match self.page {
            AppPage::ROSTER => {
                let role = self.roster_grid.role();
                let mut values = self.roster_values(role);
                if !values.is_empty() {
                    values.fill(0);
                    values[0] = 1;
                    self.set_roster_values(role, values);
                    self.save_settings();
                }
            }
            AppPage::SUBMENU => {
                self.tabs.previous();
            }
            _ => {}
        }
    }
    fn on_zr(&mut self) {
        match self.page {
            AppPage::SUBMENU => self.tabs.next(),
            _ => {}
        }
    }
}
