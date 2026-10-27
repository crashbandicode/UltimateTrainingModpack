use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;
use std::collections::HashMap;

use crate::roster_grid::{Direction, Outcome, Role, RosterGrid, RosterPurpose};
use crate::stage_forms::{Forms, HelpControls, Selection};
use crate::{InputControl, StatefulList, SubMenu, SubMenuType, Tab};

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
    /// Persisted independently per role/class in the existing menu envelope.
    pub mii_preferences: training_mod_mii::selection::Preferences,
    /// Direct mode is dormant unless the native alpha caller enables it.
    pub stage_preferences: crate::stage_menu::Preferences,
    stage_direct_enabled: bool,
    /// Training's action view is independent of the saved lobby entry mode.
    stage_training_view: Option<crate::stage_menu::Mode>,
    stage_catalog: Vec<u32>,
    stage_forms: Vec<[Option<u32>; 3]>,
    /// Per-opening Direct drafts never borrow or overwrite the rotation forms.
    stage_direct_forms: Forms,
    pending_stage: Option<Selection>,
    mii_catalog: Option<crate::mii_menu::CatalogMenu>,
    pending_mii: Option<crate::mii_menu::PendingSelection>,
    #[cfg(feature = "mii-editor-alpha")]
    pending_mii_editor: Option<crate::mii_menu::EditorRequest>,
    pub confirmation_state: ConfirmationState,
    pub confirmation_return_page: AppPage,
    /// Settings with no separate launch control, still serialized and reset normally.
    pub hidden_submenus: Vec<SubMenu<'a>>,
    pub roster_grid: RosterGrid,
    pub roster_kinds: Vec<u32>,
    /// Which pair the shared portrait grid edits. Not serialized.
    pub roster_purpose: RosterPurpose,
    /// One-shot input acknowledgement; consumed only after its brief portrait flash.
    /// Not serialized and never interpreted as an ordered-roster edit.
    pending_manual: Option<(Role, u32, u8)>,
    /// Native CSS kind (whole transforming group), supplied by the runtime.
    pub original_player_kind: Option<u32>,
    /// Original CPU kind for the Kirby hat highlight. Not serialized.
    original_cpu_kind: Option<u32>,
}

impl<'a> Default for App<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> App<'a> {
    pub fn new() -> App<'a> {
        App {
            tabs: StatefulList::new(),
            page: AppPage::SUBMENU,
            serialized_settings: String::new(),
            serialized_default_settings: String::new(),
            mii_preferences: training_mod_mii::selection::Preferences::default(),
            stage_preferences: crate::stage_menu::Preferences::default(),
            stage_direct_enabled: false,
            stage_training_view: None,
            stage_catalog: Vec::new(),
            stage_forms: Vec::new(),
            stage_direct_forms: Forms::NORMAL,
            pending_stage: None,
            mii_catalog: None,
            pending_mii: None,
            #[cfg(feature = "mii-editor-alpha")]
            pending_mii_editor: None,
            confirmation_state: ConfirmationState::HoverNo,
            confirmation_return_page: AppPage::SUBMENU,
            hidden_submenus: Vec::new(),
            roster_grid: RosterGrid::new(Role::Player),
            roster_kinds: Vec::new(),
            roster_purpose: RosterPurpose::FighterCycles,
            pending_manual: None,
            original_player_kind: None,
            original_cpu_kind: None,
        }
    }

    /// The consts catalog owns native IDs; the UI never derives one from a label.
    pub fn configure_stage_catalog(&mut self, stages: Vec<u32>) {
        self.stage_catalog = stages;
    }

    pub fn configure_stage_forms(&mut self, forms: Vec<[Option<u32>; 3]>) {
        self.stage_forms = forms;
    }

    pub fn open_stage_menu(&mut self, enabled: bool, preferences: crate::stage_menu::Preferences) {
        self.open_stage_menu_in_context(enabled, preferences, false);
    }

    pub fn open_stage_menu_in_context(
        &mut self,
        enabled: bool,
        preferences: crate::stage_menu::Preferences,
        ordinary_training: bool,
    ) {
        self.stage_direct_enabled = enabled;
        self.stage_preferences = preferences;
        self.stage_training_view = (enabled && ordinary_training)
            .then_some(crate::stage_menu::Mode::Direct);
        self.stage_direct_forms = Forms::NORMAL;
        if !ordinary_training {
            if let Some(index) = self.stage_catalog.iter().position(|id| *id == preferences.direct_stage) {
                let _ = self.stage_direct_forms.set(index, preferences.direct_form);
            }
        }
        self.pending_stage = None;
        if let Some(submenu) = self.tabs.iter_mut().flat_map(|tab| tab.submenus.iter_mut())
            .flatten().find(|submenu| submenu.id == "lobby_stages") {
            submenu.title = if enabled { "Practice Stages" } else { "Lobby Stages" };
            submenu.help_text = if enabled && ordinary_training {
                "Change Stage Now applies only to this Training session.\nX configures Lobby Stage Rotation for the next lobby entry."
            } else if enabled {
                "Change Stage Now also saves the direct lobby stage.\nLobby Stage Rotation advances only on the next lobby entry."
            } else {
                "Select stages in order. Changes apply on the next lobby entry.\nThe match stage is unchanged."
            };
        }
    }

    pub fn stage_direct_enabled(&self) -> bool { self.stage_direct_enabled }

    pub fn stage_page_active(&self) -> bool {
        self.page == AppPage::TOGGLE && self.tabs.state.selected()
            .and_then(|i| self.tabs.items.get(i))
            .and_then(|tab| tab.submenus.get(tab.submenus.state.selected_row().unwrap_or(0),
                tab.submenus.state.selected_col().unwrap_or(0)))
            .is_some_and(|submenu| submenu.id == "lobby_stages")
    }

    pub fn stage_is_direct(&self) -> bool {
        self.stage_direct_enabled && self.stage_training_view.unwrap_or(self.stage_preferences.mode)
            == crate::stage_menu::Mode::Direct
    }

    /// A saved lobby choice is not evidence of the current Training stage.
    pub fn stage_direct_marker(&self) -> Option<u32> {
        (self.stage_is_direct() && self.stage_training_view.is_none())
            .then_some(self.stage_preferences.direct_stage)
    }

    pub fn stage_title(&self) -> &'static str {
        if self.stage_is_direct() { "Change Stage Now" }
        else { "Lobby Stage Rotation" }
    }

    pub fn stage_help(&self) -> &'static str {
        // Static strings preserve the native renderers' borrow contract while
        // showing the highlighted form on every event-driven redraw.
        let controls = if self.stage_is_direct() && self.stage_training_view.is_some() {
            HelpControls::TrainingApply
        } else if self.stage_is_direct() {
            HelpControls::LobbyApply
        } else if self.stage_direct_enabled {
            HelpControls::RotationWithDirect
        } else {
            HelpControls::RotationOnly
        };
        match self.stage_selection() {
            Some(selection) => {
                let native = self.stage_index()
                    .and_then(|index| self.stage_forms.get(index).copied())
                    .unwrap_or([None; 3]);
                selection.form.help_line(native, controls)
            }
            None => controls.bare(),
        }
    }

    fn stage_index(&self) -> Option<usize> {
        if !self.stage_page_active() { return None; }
        let tab = self.tabs.items.get(self.tabs.state.selected()?)?;
        let toggles = &tab.submenus.get(tab.submenus.state.selected_row()?,
            tab.submenus.state.selected_col()?)?.toggles;
        (toggles.state.selected_row()? * toggles.cols + toggles.state.selected_col()?).checked_sub(1)
    }

    /// Base identity plus the displayed form, validated against the supplied
    /// catalog. All, missing catalog data and unsupported pairs produce no intent.
    pub fn stage_selection(&self) -> Option<Selection> {
        let index = self.stage_index()?;
        let forms = if self.stage_is_direct() { &self.stage_direct_forms }
            else { &self.stage_preferences.rotation_forms };
        let form = forms.get(index)?;
        self.stage_forms.get(index)?[form as usize]?;
        Some(Selection { stage: *self.stage_catalog.get(index)?, form })
    }

    fn edit_stage_form(&mut self) {
        let Some(index) = self.stage_index() else { return; };
        let Some(selection) = self.stage_selection() else { return; };
        let next = selection.form.next(self.stage_forms[index]);
        let forms = if self.stage_is_direct() { &mut self.stage_direct_forms }
            else { &mut self.stage_preferences.rotation_forms };
        let _ = forms.set(index, next);
    }

    fn page_stage_menu(&mut self, next: bool) {
        let toggles = &mut self.selected_submenu().toggles;
        let selected = toggles.state.selected_row().unwrap_or(0) * toggles.cols
            + toggles.state.selected_col().unwrap_or(0);
        if let Some(index) = crate::stage_menu::page_cursor(selected, toggles.len(), next) {
            toggles.select(index / toggles.cols, index % toggles.cols);
        }
    }

    /// Close intents are consumed once; opening/cancelling never submits one.
    pub fn take_pending_stage(&mut self) -> Option<Selection> {
        if self.page != AppPage::CLOSE { return None; }
        self.pending_stage.take()
    }

    /// Dormant until a production caller supplies a fresh owned catalog and
    /// verified session context. Mount after spawn_menu sets AppPage::SUBMENU,
    /// before input; open_settings_for_player alone does not reset a closed page.
    /// This enables preference editing only, never native mutation or cycle admission.
    pub fn mount_mii_catalog(
        &mut self,
        snapshot: training_mod_mii::inventory::Snapshot,
        context: crate::mii_menu::Context,
    ) -> bool {
        if self.page != AppPage::SUBMENU || self.mii_catalog.is_some() {
            return false;
        }
        let Some(menu) = crate::mii_menu::CatalogMenu::new(snapshot, context, &self.mii_preferences) else {
            return false;
        };
        self.mount_mii_menu(menu)
    }

    #[cfg(feature = "mii-editor-alpha")]
    pub fn mount_mii_editor_catalog(
        &mut self,
        snapshot: training_mod_mii::inventory::Snapshot,
        epoch: u64,
    ) -> bool {
        let Some(menu) = crate::mii_menu::CatalogMenu::editor_only(snapshot, epoch, &self.mii_preferences) else {
            return false;
        };
        self.mount_mii_menu(menu)
    }

    fn mount_mii_menu(&mut self, menu: crate::mii_menu::CatalogMenu) -> bool {
        if self.page != AppPage::SUBMENU || self.mii_catalog.is_some() { return false; }
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == "character_extras") else {
            return false;
        };
        let mut items = tab.submenus.as_vec();
        let insertion = items.iter().position(|item| item.id == "solo_popo_original")
            .map_or(0, |position| position + 1);
        if items.len() + 3 > tab.submenus.full_len() { return false; }
        drop(items.splice(insertion..insertion, crate::mii_menu::launchers()));
        tab.submenus = crate::StatefulTable::with_items(tab.submenus.rows, tab.submenus.cols, items);
        self.mii_catalog = Some(menu);
        self.pending_mii = None;
        true
    }

    /// Publish into this opening only. The caller holds App, supplies the current
    /// opening epoch, and invalidates the event-driven renderer on success.
    /// No native work occurs here.
    pub fn publish_mii_catalog(
        &mut self,
        snapshot: training_mod_mii::inventory::Snapshot,
        context: crate::mii_menu::Context,
        current_epoch: u64,
        request_redraw: impl FnOnce(),
    ) -> bool {
        if context.epoch != current_epoch || !self.mount_mii_catalog(snapshot, context) {
            return false;
        }
        if let training_mod_mii::selection::PlayerOrigin::Mii(origin) = context.original {
            self.original_player_kind = Some(origin.0.class as u32);
        }
        request_redraw();
        true
    }

    /// Cancel on departure or reopening. Does not restore/write native configuration.
    #[cfg(feature = "mii-editor-alpha")]
    pub fn take_pending_mii_editor(&mut self) -> Option<crate::mii_menu::EditorRequest> {
        self.pending_mii_editor.take()
    }

    pub fn cancel_mii_catalog(&mut self) {
        #[cfg(feature = "mii-editor-alpha")]
        { self.pending_mii_editor = None; }
        self.pending_mii = None;
        if self.mii_catalog.take().is_some() {
            if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == "character_extras") {
                let items = tab.submenus.iter().filter(|item| crate::mii_menu::class_for_id(item.id).is_none())
                    .cloned().collect();
                tab.submenus = crate::StatefulTable::with_items(tab.submenus.rows, tab.submenus.cols, items);
            }
            self.page = AppPage::SUBMENU;
        }
    }

    fn mii_page_active(&self) -> bool {
        self.page == AppPage::TOGGLE && self.mii_catalog.is_some()
            && self.tabs.state.selected().and_then(|index| self.tabs.items.get(index))
                .and_then(|tab| tab.submenus.get(tab.submenus.state.selected_row().unwrap_or(0),
                    tab.submenus.state.selected_col().unwrap_or(0)))
                .is_some_and(|submenu| crate::mii_menu::class_for_id(submenu.id).is_some())
    }

    pub fn mii_view(&self) -> Option<crate::mii_menu::View> {
        self.mii_page_active().then(|| self.mii_catalog.as_ref().unwrap().view(&self.mii_preferences))
    }

    fn finish_mii_selection(&mut self) {
        if self.page == AppPage::CLOSE { return; }
        self.pending_mii = self.mii_catalog.as_ref().and_then(|menu| menu.close(&self.mii_preferences));
    }

    /// No production caller is wired in this candidate. Consuming this intent
    /// still requires a fresh native binding and all existing lifetime guards.
    pub fn take_pending_mii_selection(&mut self, current_epoch: u64) -> Option<crate::mii_menu::PendingSelection> {
        if self.page != AppPage::CLOSE { return None; }
        self.pending_mii.take().filter(|intent| intent.context.epoch == current_epoch)
    }

    pub fn current_settings_to_json(&self) -> String {
        serde_json::to_string(&self).expect("Could not serialize the menu to JSON!")
    }

    pub fn get_serialized_settings_with_defaults(&self) -> String {
        format!(
            "{{\"menu\":{}, \"defaults_menu\":{}, \"mii_preferences\":{}, \"stage_preferences\":{}}}",
            self.serialized_settings, self.serialized_default_settings,
            serde_json::to_string(&self.mii_preferences).expect("Serialize Mii preferences"),
            serde_json::to_string(&self.stage_preferences).expect("Serialize stage preferences")
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
            for submenu in tab.submenus.iter_mut().flatten() {
                if let Some(val) = all_settings.get(submenu.id) {
                    submenu.update_from_vec(val.clone());
                }
            }
        }
        for submenu in &mut self.hidden_submenus {
            if let Some(val) = all_settings.get(submenu.id) {
                submenu.update_from_vec(val.clone());
            }
        }
        self.exclude_original_player();
        self.save_settings();
    }

    /// Opening an editor is one transaction: restore against this scene's
    /// original, never the identity left by a cancelled previous-scene editor.
    pub fn open_settings_for_player(&mut self, json: &str, original: Option<u32>) {
        self.cancel_mii_catalog();
        self.pending_stage = None;
        self.pending_manual = None;
        self.original_player_kind = original.map(crate::roster_grid::group_kind);
        self.update_all_from_json(json);
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
                'submenus_scope: for submenu in tab.submenus.iter_mut().flatten() {
                    if submenu.id == submenu_id {
                        submenu.update_from_vec(val.clone());
                        break 'tabs_scope;
                    }
                }
            }
        }
        self.exclude_original_player();
        self.save_settings();
    }

    pub fn confirm(&mut self) -> bool {
        self.confirmation_state == ConfirmationState::HoverYes
    }

    fn list_id(purpose: RosterPurpose, role: Role) -> &'static str {
        match (purpose, role) {
            (RosterPurpose::FighterCycles, Role::Player) => "player_roster",
            (RosterPurpose::FighterCycles, Role::Cpu) => "cpu_roster",
            (RosterPurpose::KirbyHats, Role::Player) => "player_kirby_hats",
            (RosterPurpose::KirbyHats, Role::Cpu) => "cpu_kirby_hats",
            (RosterPurpose::ManualSwap, Role::Player) => "player_character",
            (RosterPurpose::ManualSwap, Role::Cpu) => "cpu_character",
        }
    }

    fn read_id(&self, id: &str) -> Vec<u8> {
        self.tabs
            .iter()
            .flat_map(|tab| tab.submenus.iter())
            .chain(self.hidden_submenus.iter())
            .find(|submenu| submenu.id == id)
            .map(|submenu| submenu.toggles.iter().map(|toggle| toggle.value).collect())
            .unwrap_or_default()
    }

    pub fn roster_values(&self, role: Role) -> Vec<u8> {
        self.read_id(Self::list_id(self.roster_purpose, role))
    }

    pub fn manual_selected_kind(&self, role: Role) -> Option<u32> {
        self.read_id(Self::list_id(RosterPurpose::ManualSwap, role))
            .iter().position(|value| *value != 0)
            .and_then(|index| self.roster_kinds.get(index).copied())
    }

    pub fn player_swapping_enabled(&self) -> bool {
        self.tabs.iter().flat_map(|tab| tab.submenus.iter())
            .find(|submenu| submenu.id == "trio_practice")
            .and_then(|submenu| submenu.toggles.iter().next())
            .is_some_and(|toggle| toggle.value != 0)
    }

    pub fn manual_selection_flash(&self) -> Option<(Role, u32, u8)> {
        self.pending_manual.filter(|(_, _, frames)| *frames != 0)
    }

    /// Native draw callback owns the bounded acknowledgement clock. Input stays
    /// captured until close; the existing native request guards still own loading.
    pub fn advance_manual_selection(&mut self) {
        if self.page != AppPage::ROSTER { return; }
        if let Some((_, _, frames)) = self.pending_manual.as_mut() {
            *frames = frames.saturating_sub(1);
            if *frames == 0 { self.page = AppPage::CLOSE; }
        }
    }

    pub fn take_pending_manual_selection(&mut self) -> Option<(Role, u32)> {
        if self.page != AppPage::CLOSE { return None; }
        self.pending_manual.take().map(|(role, kind, _)| (role, kind))
    }

    fn select_manual_character(&mut self) {
        let role = self.roster_grid.role();
        let Some(kind) = self.roster_grid.cursor_info().kind() else { return; };
        if self.roster_excluded(role, kind) { return; }
        let Some(index) = crate::roster_grid::catalog_index(&self.roster_kinds, kind) else { return; };
        let mut values = vec![0; self.roster_kinds.len()];
        values[index] = 1;
        self.write_id(Self::list_id(RosterPurpose::ManualSwap, role), values);
        // Keep the other role's ordinary menu-close behavior. This role's direct
        // action resolves its freshly saved preference itself, so do not also
        // submit a same-class reload against its previous active body.
        self.finish_mii_selection();
        if let Some(pending) = self.pending_mii.as_mut() {
            pending.actions.retain(|action| match action {
                training_mod_mii::selection::Action::ReloadPlayer(_) => role != Role::Player,
                training_mod_mii::selection::Action::ReloadCpu(_) => role != Role::Cpu,
            });
        }
        self.pending_manual = Some((role, kind, crate::roster_grid::MANUAL_FLASH_FRAMES));
        self.save_settings();
    }

    fn step_roster(&mut self, direction: Direction) {
        if self.roster_purpose == RosterPurpose::ManualSwap {
            self.roster_grid.step_manual(direction);
        } else {
            self.roster_grid.step(direction);
        }
    }

    pub fn set_original_player_kind(&mut self, kind: Option<u32>) {
        if self.original_player_kind == kind.map(crate::roster_grid::group_kind) {
            return;
        }
        self.original_player_kind = kind.map(crate::roster_grid::group_kind);
        self.exclude_original_player();
        self.save_settings();
    }

    /// Captured CPU identity for slot 0 and the hat-grid highlight. Does not write lists.
    pub fn set_original_cpu_kind(&mut self, kind: Option<u32>) {
        self.original_cpu_kind = kind.map(crate::roster_grid::group_kind);
    }

    pub fn highlights_original_cpu(&self, kind: u32) -> bool {
        self.roster_purpose == RosterPurpose::KirbyHats
            && self.original_cpu_kind == Some(crate::roster_grid::group_kind(kind))
    }

    /// Kirby cannot be a hat; Mii cells require the separately qualified helper feature.
    pub fn hat_cell_disabled(&self, kind: u32) -> bool {
        if self.roster_purpose!=RosterPurpose::KirbyHats {return false;}
        if crate::roster_grid::group_kind(kind)==0x06 {return true;}
        if !(0x48..=0x4a).contains(&kind) {return false;}
        #[cfg(feature="mii-managed-helper-alpha")]
        {!self.mii_catalog.as_ref().is_some_and(|m|m.alpha_helper_hat(kind))}
        #[cfg(not(feature="mii-managed-helper-alpha"))]
        true
    }

    #[cfg(feature="mii-ordinary-cpu-alpha")]
    pub fn qualify_ordinary_cpu_cycle(&mut self, qualified:bool) {
        if let Some(menu)=self.mii_catalog.as_mut() {menu.qualify_ordinary_cpu_cycle(qualified);}
    }
    /// Numbered CPU configuration for one class, using the mounted catalog.
    /// Manual swap keeps `alpha_cpu_cycle` and does not call this.
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    pub fn cpu_numbered_saved(&self, kind: u32) -> Option<training_mod_mii::selection::Choice> {
        self.mii_catalog.as_ref().and_then(|menu| menu.cpu_numbered_choice(kind, &self.mii_preferences))
    }
    pub fn roster_excluded(&self, role: Role, kind: u32) -> bool {
        if self.roster_purpose == RosterPurpose::ManualSwap {
            if role == Role::Player && !self.player_swapping_enabled() { return true; }
            if (0x48..=0x4a).contains(&kind) {
                #[cfg(feature="mii-ordinary-player-alpha")]
                if role == Role::Player {
                    return !self.mii_catalog.as_ref().is_some_and(|m| m.alpha_player_cycle(kind));
                }
                #[cfg(feature="mii-ordinary-cpu-alpha")]
                if role == Role::Cpu {
                    return !self.mii_catalog.as_ref().is_some_and(|m| m.alpha_cpu_cycle(kind));
                }
                return true;
            }
            return false;
        }
        #[cfg(feature="mii-managed-helper-alpha")]
        if self.roster_purpose==RosterPurpose::KirbyHats && (0x48..=0x4a).contains(&kind) {
            return self.hat_cell_disabled(kind);
        }
        // Slot 0 is only the captured original. An unadmitted Mii stays
        // unselectable, but that is not an original identity.
        if self.roster_slot_zero(role, kind) {
            return true;
        }
        if (0x48..=0x4a).contains(&kind) {
            if self.roster_purpose != RosterPurpose::FighterCycles || !self.mii_cycle_admitted(role, kind) {
                return true;
            }
            return false;
        }
        false
    }

    /// Grey badge 0: this role's captured character-select group, and only when
    /// adding it would repeat slot 0. A missing or sentinel kind matches nothing.
    pub fn roster_slot_zero(&self, role: Role, kind: u32) -> bool {
        if self.roster_purpose != RosterPurpose::FighterCycles {
            return false;
        }
        let Some(original) = self.captured_original(role) else {
            return false;
        };
        if crate::roster_grid::group_kind(original) != crate::roster_grid::group_kind(kind) {
            return false;
        }
        if (0x48..=0x4a).contains(&crate::roster_grid::group_kind(original)) {
            // Manual swap uses alpha_cpu_cycle and never reaches this FighterCycles
            // predicate. CPU slot 0 is the captured configuration with no numbered
            // alternative. Player admission is unchanged.
            #[cfg(feature = "mii-ordinary-cpu-alpha")]
            if role == Role::Cpu {
                return self.mii_catalog.as_ref().is_some_and(|menu|
                    menu.cpu_unnumbered_original(kind, &self.mii_preferences));
            }
            return !self.mii_cycle_admitted(role, kind);
        }
        self.role_cycle_exclusion(role) == Some(crate::roster_grid::group_kind(kind))
    }

    fn captured_original(&self, role: Role) -> Option<u32> {
        let kind = match role {
            Role::Player => self.original_player_kind?,
            Role::Cpu => self.original_cpu_kind?,
        };
        let group = crate::roster_grid::group_kind(kind);
        self.roster_kinds
            .iter()
            .any(|candidate| *candidate == kind || *candidate == group)
            .then_some(kind)
    }

    /// Admitted Mii class tile. False when the catalog or role qualification is
    /// missing; that absence is not a selected Swordfighter.
    fn mii_cycle_admitted(&self, role: Role, kind: u32) -> bool {
        if self.roster_purpose != RosterPurpose::FighterCycles || !(0x48..=0x4a).contains(&kind) {
            return false;
        }
        match role {
            Role::Cpu => {
                #[cfg(feature = "mii-ordinary-cpu-alpha")]
                {
                    self.mii_catalog.as_ref().is_some_and(|menu| menu.alpha_cpu_cycle(kind))
                }
                #[cfg(not(feature = "mii-ordinary-cpu-alpha"))]
                {
                    false
                }
            }
            Role::Player => {
                #[cfg(feature = "mii-saved-alpha")]
                {
                    self.mii_catalog.as_ref().is_some_and(|menu| menu.alpha_player_cycle(kind))
                }
                #[cfg(not(feature = "mii-saved-alpha"))]
                {
                    false
                }
            }
        }
    }

    /// Native group this role's numbered cycle skips. Player Ice Climbers is
    /// released when its original and cycle Solo settings differ. CPU has one
    /// Solo setting, so those two flags are the same value.
    fn role_cycle_exclusion(&self, role: Role) -> Option<u32> {
        let original = self.captured_original(role)?;
        let (original_solo, cycle_solo) = match role {
            Role::Player => (
                self.read_id("solo_popo_original").first().is_some_and(|on| *on != 0),
                self.roster_solo_enabled(Role::Player),
            ),
            Role::Cpu => {
                let solo = self.roster_solo_enabled(Role::Cpu);
                (solo, solo)
            }
        };
        crate::roster_grid::cycle_exclusion(Some(original), original_solo, cycle_solo)
    }

    fn player_cycle_exclusion(&self) -> Option<u32> {
        self.role_cycle_exclusion(Role::Player)
    }

    fn exclude_original_player(&mut self) {
        #[cfg(feature="mii-saved-alpha")]
        if self.original_player_kind.is_some_and(|k| (0x48..=0x4a).contains(&k)){return;}
        let Some(kind) = self.player_cycle_exclusion() else {
            return;
        };
        let id = Self::list_id(RosterPurpose::FighterCycles, Role::Player);
        let mut values = self.read_id(id);
        if values.first() != Some(&0) {
            return;
        }
        if let Some(index) =
            crate::roster_grid::selected_group_index(&self.roster_kinds, &values, kind)
        {
            let removed = values[index + 1];
            values[index + 1] = 0;
            for rank in values.iter_mut().skip(1).filter(|rank| **rank > removed) {
                *rank -= 1;
            }
            if values.iter().skip(1).all(|rank| *rank == 0) {
                values[0] = 1;
            }
            self.write_id(id, values);
        }
    }

    pub fn roster_solo_enabled(&self, role: Role) -> bool {
        let id = Self::solo_id(role);
        self.hidden_submenus
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.toggles.iter().next())
            .is_some_and(|t| t.value != 0)
    }

    fn solo_id(role: Role) -> &'static str {
        match role {
            Role::Player => "solo_popo_player_cycle",
            Role::Cpu => "solo_popo_cpu",
        }
    }

    pub fn toggle_roster_role(&mut self) {
        self.roster_grid.toggle_role();
        if self.roster_purpose == RosterPurpose::ManualSwap {
            if let Some(kind) = self.manual_selected_kind(self.roster_grid.role()) {
                self.roster_grid.focus_kind(kind);
            }
            return;
        }
        let values = self.roster_values(self.roster_grid.role());
        self.roster_grid.sync_forms(&self.roster_kinds, &values);
    }

    /// Display both roles' numbers even when a different group form is previewed.
    pub fn roster_badge(&self, role: Role, kind: u32) -> Option<u8> {
        crate::roster_grid::catalog_index(&self.roster_kinds, kind)
            .and_then(|i| self.roster_badges(role)[i])
    }

    /// Catalog-indexed badges, prepared once per draw for each independent role.
    pub fn roster_badges(&self, role: Role) -> Vec<Option<u8>> {
        self.roster_badges_from_values(role, &self.roster_values(role))
    }

    /// Reuse the role's values when the renderer already needs them for stripes and labels.
    pub fn roster_badges_from_values(&self, role: Role, values: &[u8]) -> Vec<Option<u8>> {
        if self.roster_purpose == RosterPurpose::ManualSwap {
            return vec![None; self.roster_kinds.len()];
        }
        let excluded = match self.roster_purpose {
            RosterPurpose::FighterCycles => self
                .role_cycle_exclusion(role)
                .filter(|kind| self.roster_slot_zero(role, *kind)),
            RosterPurpose::KirbyHats => Some(0x06),
            _ => None,
        };
        let all = values.first() == Some(&1);
        let order = if all {
            crate::roster_grid::css_order(&self.roster_kinds, values, excluded)
        } else if self.roster_purpose == RosterPurpose::FighterCycles {
            // Stored ranks retain choices across scenes. Display the numbered
            // order after this role's current original/unavailable cells are
            // excluded, just as the cycle does, without rewriting preferences.
            let mut ranked: Vec<_> = self.roster_kinds.iter().copied().enumerate()
                .filter_map(|(index, kind)| crate::roster_grid::rank(values, index)
                    .map(|rank| (rank, kind)))
                .collect();
            ranked.sort_unstable_by_key(|(rank, _)| *rank);
            ranked.into_iter().map(|(_, kind)| kind).collect()
        } else {
            Vec::new()
        };
        let order: Vec<_> = order.into_iter()
            .filter(|kind| !self.hat_cell_disabled(*kind) && !self.roster_excluded(role, *kind))
            .collect();
        self.roster_kinds
            .iter()
            .map(|kind| {
                if self.hat_cell_disabled(*kind) {
                    None
                } else if self.roster_slot_zero(role, *kind) {
                    Some(0)
                } else if (0x48..=0x4a).contains(kind)
                    && self.roster_purpose == RosterPurpose::FighterCycles
                    && !self.mii_cycle_admitted(role, *kind)
                {
                    None
                } else if all || self.roster_purpose == RosterPurpose::FighterCycles {
                    order
                        .iter()
                        .position(|k| {
                            crate::roster_grid::group_kind(*k)
                                == crate::roster_grid::group_kind(*kind)
                        })
                        .map(|i| i as u8 + 1)
                } else {
                    crate::roster_grid::group_rank(&self.roster_kinds, values, *kind)
                }
            })
            .collect()
    }

    fn write_id(&mut self, id: &str, values: Vec<u8>) {
        for tab in self.tabs.iter_mut() {
            for submenu in tab.submenus.iter_mut().flatten() {
                if submenu.id == id {
                    submenu.update_from_vec(values);
                    return;
                }
            }
        }
        if let Some(submenu) = self
            .hidden_submenus
            .iter_mut()
            .find(|submenu| submenu.id == id)
        {
            submenu.update_from_vec(values);
        }
    }

    fn set_roster_values(&mut self, role: Role, values: Vec<u8>) {
        self.write_id(Self::list_id(self.roster_purpose, role), values);
    }

    fn edit_roster(&mut self, remove: bool) {
        if self.roster_purpose == RosterPurpose::ManualSwap {
            if !remove { self.select_manual_character(); }
            return;
        }
        let role = self.roster_grid.role();
        if self.roster_grid.cursor_info().kind().is_some_and(|kind| {
            self.roster_excluded(role, kind) || self.hat_cell_disabled(kind)
        }) {
            return;
        }
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
        let len: usize =
            self.tabs.iter().flat_map(|tab| tab.submenus.iter())
                .filter(|submenu| crate::mii_menu::class_for_id(submenu.id).is_none()).count()
                + self.hidden_submenus.len();
        let mut map = serializer.serialize_map(Some(len))?;
        for tab in self.tabs.iter() {
            for submenu in tab.submenus.iter() {
                if crate::mii_menu::class_for_id(submenu.id).is_none() {
                    map.serialize_entry(submenu.id, submenu)?;
                }
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
        if self.pending_manual.is_some() { return; }
        if self.stage_page_active() && self.stage_is_direct() {
            // Cell zero means All in Rotation only; it never becomes a live request.
            if let Some(selection) = self.stage_selection() {
                if self.stage_training_view.is_none() {
                    self.stage_preferences.direct_stage = selection.stage;
                    self.stage_preferences.direct_form = selection.form;
                }
                self.pending_stage = Some(selection);
                // Save edited Mii preferences, but do not issue another native action.
                self.pending_mii = None;
                self.page = AppPage::CLOSE;
                self.save_settings();
            }
            return;
        }
        if self.mii_page_active() {
            #[cfg(feature = "mii-editor-alpha")]
            if let Some(seed) = self.mii_catalog.as_ref().unwrap().editor_seed(&self.mii_preferences) {
                self.pending_mii_editor = Some(seed);
                self.pending_mii = None;
                self.page = AppPage::CLOSE;
                self.save_settings();
                return;
            }
            self.mii_catalog.as_mut().unwrap().confirm(&mut self.mii_preferences);
            self.save_settings();
            return;
        }
        match self.page {
            AppPage::SUBMENU => {
                let id = self.selected_submenu().id;
                if id == "lobby_stages" && self.stage_training_view.is_some() {
                    self.stage_training_view = Some(crate::stage_menu::Mode::Direct);
                }
                if let Some(class) = crate::mii_menu::class_for_id(id) {
                    if let Some(menu) = self.mii_catalog.as_mut() {
                        menu.open_class(class);
                        self.page = AppPage::TOGGLE;
                    }
                    return;
                }
                if matches!(id, "player_character" | "cpu_character") && !self.roster_kinds.is_empty() {
                    let role = if id == "player_character" { Role::Player } else { Role::Cpu };
                    self.roster_purpose = RosterPurpose::ManualSwap;
                    self.roster_grid = RosterGrid::new(role);
                    if let Some(kind) = self.manual_selected_kind(role) {
                        self.roster_grid.focus_kind(kind);
                    }
                    self.page = AppPage::ROSTER;
                    return;
                }
                if (id == "player_roster" || id == "player_kirby_hats")
                    && !self.roster_kinds.is_empty()
                {
                    self.roster_purpose = if id == "player_kirby_hats" {
                        RosterPurpose::KirbyHats
                    } else {
                        RosterPurpose::FighterCycles
                    };
                    let values = self.roster_values(self.roster_grid.role());
                    self.roster_grid.sync_forms(&self.roster_kinds, &values);
                    self.page = AppPage::ROSTER;
                    return;
                }
                // Large single-choice pickers reopen on the configured option,
                // including choices beyond the first screen of the native UI.
                let submenu = self.selected_submenu();
                if submenu.id == "character_item_grant" {
                    // This is an action picker, not a held-item setting. Keep
                    // the legacy serialized array shape, but discard old checks.
                    for toggle in submenu.toggles.iter_mut().flatten() {
                        toggle.value = 0;
                    }
                }
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
            AppPage::TOGGLE => {
                let submenu = self.selected_submenu();
                if submenu.id == "character_item_grant" {
                    // Native input submits every A press independently. Never
                    // persist an activation as an item that remains equipped.
                    for toggle in submenu.toggles.iter_mut().flatten() {
                        toggle.value = 0;
                    }
                } else {
                    submenu.on_a();
                }
            }
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
        if self.pending_manual.is_some() { return; }
        if self.mii_page_active() {
            if !self.mii_catalog.as_mut().unwrap().back() { self.page = AppPage::SUBMENU; }
            self.save_settings();
            return;
        }
        match self.page {
            AppPage::SUBMENU => {
                // Exit the app
                self.finish_mii_selection();
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
        if self.pending_manual.is_some() { return; }
        if self.stage_page_active() && self.stage_direct_enabled {
            let mode = if self.stage_is_direct() {
                crate::stage_menu::Mode::Rotation
            } else {
                crate::stage_menu::Mode::Direct
            };
            if let Some(view) = self.stage_training_view.as_mut() {
                *view = mode;
            }
            // Explicitly selecting lobby rotation enables it. Training's manual
            // action view never pins a stage or changes the saved direct choice.
            if self.stage_training_view.is_none() || mode == crate::stage_menu::Mode::Rotation {
                self.stage_preferences.mode = mode;
            }
            return;
        }
        if self.mii_page_active() { return; }
        if self.page == AppPage::ROSTER {
            self.roster_grid.cycle_form();
        } else {
            self.save_default_settings();
        }
    }
    fn on_y(&mut self) {
        if self.pending_manual.is_some() { return; }
        if self.stage_page_active() {
            self.edit_stage_form();
            self.save_settings();
            return;
        }
        if self.mii_page_active() { return; }
        // Clear current toggle, for toggles w/ weighted selections
        match self.page {
            AppPage::TOGGLE => self.selected_submenu().on_y(),
            AppPage::ROSTER => {
                if self.roster_purpose != RosterPurpose::FighterCycles {
                    return;
                }
                if self.roster_grid.cursor_info().kind() == Some(0x4b) {
                    let id = Self::solo_id(self.roster_grid.role());
                    let enabled = self.roster_solo_enabled(self.roster_grid.role());
                    if let Some(submenu) = self.hidden_submenus.iter_mut().find(|s| s.id == id) {
                        submenu.update_from_vec(vec![u8::from(!enabled), u8::from(enabled)]);
                        self.save_settings();
                    }
                }
            }
            _ => {}
        }
    }
    fn on_up(&mut self) {
        if self.pending_manual.is_some() { return; }
        if self.mii_page_active() {
            self.mii_catalog.as_mut().unwrap().step(-(crate::NX_SUBMENU_COLUMNS as isize));
            return;
        }
        match self.page {
            AppPage::ROSTER => self.step_roster(Direction::Up),
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
        if self.pending_manual.is_some() { return; }
        if self.mii_page_active() {
            self.mii_catalog.as_mut().unwrap().step(crate::NX_SUBMENU_COLUMNS as isize);
            return;
        }
        match self.page {
            AppPage::ROSTER => self.step_roster(Direction::Down),
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
        if self.pending_manual.is_some() { return; }
        if self.mii_page_active() {
            self.mii_catalog.as_mut().unwrap().step(-1);
            return;
        }
        match self.page {
            AppPage::ROSTER => self.step_roster(Direction::Left),
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
        if self.pending_manual.is_some() { return; }
        if self.mii_page_active() {
            self.mii_catalog.as_mut().unwrap().step(1);
            return;
        }
        match self.page {
            AppPage::ROSTER => self.step_roster(Direction::Right),
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
        if self.pending_manual.is_some() { return; }
        // Close menu
        self.finish_mii_selection();
        self.page = AppPage::CLOSE;
    }
    fn on_l(&mut self) {
        if self.pending_manual.is_some() { return; }
        if self.stage_page_active() { self.page_stage_menu(false); }
    }
    fn on_r(&mut self) {
        if self.pending_manual.is_some() { return; }
        if self.stage_page_active() {
            self.page_stage_menu(true);
            return;
        }
        if self.mii_page_active() { return; }
        if self.page == AppPage::TOGGLE && self.selected_submenu().id == "character_item_grant" {
            return; // One-shot actions have no current setting to reset.
        }
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
        if self.pending_manual.is_some() { return; }
        match self.page {
            AppPage::ROSTER => {
                if self.roster_purpose == RosterPurpose::ManualSwap { return; }
                let role = self.roster_grid.role();
                let mut values = self.roster_values(role);
                if !values.is_empty() {
                    crate::roster_grid::select_all(&self.roster_kinds, &mut values);
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
        if self.pending_manual.is_some() { return; }
        match self.page {
            AppPage::SUBMENU => self.tabs.next(),
            AppPage::ROSTER => self.toggle_roster_role(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage_forms::Form;
    use crate::{InputControl, StatefulList, StatefulTable, SubMenu, SubMenuType, Tab, Toggle};

    fn stage_app(native: [Option<u32>; 3]) -> App<'static> {
        let submenu = SubMenu {
            title: "Practice Stages",
            id: "lobby_stages",
            help_text: "",
            submenu_type: SubMenuType::Ordered,
            toggles: StatefulTable::with_items(1, 2, vec![
                Toggle { title: "All (listed order)", value: 0, max: 1 },
                Toggle { title: "Stage", value: 0, max: 1 },
            ]),
            slider: None,
        };
        let mut app = App::new();
        app.tabs = StatefulList::with_items(vec![Tab {
            title: "Misc",
            id: "misc",
            submenus: StatefulTable::with_items(1, 1, vec![submenu]),
        }]);
        app.page = AppPage::TOGGLE;
        app.stage_direct_enabled = true;
        app.configure_stage_catalog(vec![native[0].unwrap()]);
        app.configure_stage_forms(vec![native]);
        app.tabs.items[0].submenus.select(0, 0);
        app
    }

    fn select_stage(app: &mut App<'_>) {
        app.selected_submenu().toggles.select(0, 1);
    }

    #[test]
    fn y_cycles_distinct_supported_forms_without_merging_direct_state() {
        let row = [Some(95), Some(96), Some(97)];
        let mut app = stage_app(row);
        app.selected_submenu().toggles.select(0, 0);
        let before = app.stage_preferences;
        app.on_y();
        assert_eq!(app.stage_preferences, before);
        assert!(app.take_pending_stage().is_none());
        let all_help = app.stage_help();
        assert!(all_help.contains("L/R Page"), "{all_help}");
        assert!(!all_help.contains("Final Destination"), "{all_help}");

        select_stage(&mut app);
        let expected = [Form::Normal, Form::Omega, Form::Battlefield, Form::Normal];
        for (presses, form) in expected.into_iter().enumerate() {
            assert_eq!(app.stage_selection(), Some(Selection { stage: 95, form }));
            let help = app.stage_help();
            assert!(help.starts_with(&format!("Form: {}", form.label())), "{help}");
            assert!(!help.contains("Omega /"), "{help}");
            assert!(!help.contains("Final Destination"), "{help}");
            if presses + 1 < expected.len() {
                app.on_y();
            }
        }
        assert_eq!(app.stage_preferences.direct_form, Form::Normal);
        assert_eq!(app.stage_preferences.rotation_forms.get(0), Some(Form::Normal));

        app.on_y();
        assert_eq!(app.stage_selection(), Some(Selection { stage: 95, form: Form::Omega }));
        assert_eq!(app.stage_preferences.rotation_forms.get(0), Some(Form::Omega));
        app.on_x();
        assert!(app.stage_is_direct());
        assert_eq!(app.stage_selection(), Some(Selection::normal(95)));
        assert_eq!(app.stage_preferences.rotation_forms.get(0), Some(Form::Omega));
        app.on_y();
        app.on_y();
        assert_eq!(app.stage_selection(), Some(Selection { stage: 95, form: Form::Battlefield }));
        assert_eq!(app.stage_preferences.direct_form, Form::Normal);
        assert_eq!(app.stage_preferences.rotation_forms.get(0), Some(Form::Omega));
        assert!(app.stage_help().starts_with("Form: Battlefield"));
        app.on_a();
        assert_eq!(app.take_pending_stage(), Some(Selection { stage: 95, form: Form::Battlefield }));
        assert_eq!(app.take_pending_stage(), None);
        assert_eq!(app.stage_preferences.direct_form, Form::Battlefield);
        assert_eq!(app.stage_preferences.rotation_forms.get(0), Some(Form::Omega));
    }

    #[test]
    fn y_skips_forms_the_recorded_row_does_not_have() {
        let mut battlefield = stage_app([Some(0), Some(1), None]);
        select_stage(&mut battlefield);
        battlefield.on_y();
        assert_eq!(battlefield.stage_selection(), Some(Selection { stage: 0, form: Form::Omega }));
        assert!(battlefield.stage_help().contains("Y Change Form"));
        battlefield.on_y();
        assert_eq!(battlefield.stage_selection(), Some(Selection::normal(0)));

        let mut final_destination = stage_app([Some(3), None, Some(4)]);
        select_stage(&mut final_destination);
        final_destination.on_y();
        assert_eq!(final_destination.stage_selection(),
            Some(Selection { stage: 3, form: Form::Battlefield }));
        assert!(final_destination.stage_help().starts_with("Form: Battlefield"));
        assert!(!final_destination.stage_help().contains("Omega"));

        let mut training = stage_app([Some(308), None, None]);
        select_stage(&mut training);
        training.on_y();
        assert_eq!(training.stage_selection(), Some(Selection::normal(308)));
        assert!(training.stage_help().starts_with("Form: Normal"));
        assert!(!training.stage_help().contains("Y Change Form"));
    }
}
