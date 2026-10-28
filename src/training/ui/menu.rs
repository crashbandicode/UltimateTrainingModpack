use std::collections::HashMap;

use skyline::nn::ui2d::*;
use smash::ui2d::{SmashPane, SmashTextBox};
use training_mod_tui::{
    App, AppPage, ConfirmationState, SliderState, SubMenuType, NX_SUBMENU_COLUMNS, NX_SUBMENU_ROWS,
};

use crate::common::menu::{
    MENU_CLOSE_FRAME_COUNTER, MENU_CLOSE_WAIT_FRAMES, MENU_RECEIVED_INPUT, P1_CONTROLLER_STYLE,
    QUICK_MENU_ACTIVE, QUICK_MENU_APP,
};
use crate::input::*;
use crate::training::frame_counter;
use crate::training::ui::PaneExt;
use training_mod_consts::TOGGLE_MAX;
use training_mod_sync::*;

use super::fade_out;
use super::set_icon_text;

const BG_LEFT_ON_WHITE_COLOR: ResColor = ResColor {
    r: 0,
    g: 28,
    b: 118,
    a: 255,
};

const BG_LEFT_ON_BLACK_COLOR: ResColor = ResColor {
    r: 0,
    g: 22,
    b: 112,
    a: 0,
};

const BG_LEFT_OFF_WHITE_COLOR: ResColor = ResColor {
    r: 8,
    g: 13,
    b: 17,
    a: 255,
};

const BG_LEFT_OFF_BLACK_COLOR: ResColor = ResColor {
    r: 5,
    g: 10,
    b: 14,
    a: 0,
};

const BG_LEFT_SELECTED_BLACK_COLOR: ResColor = ResColor {
    r: 240,
    g: 154,
    b: 7,
    a: 0,
};

const BG_LEFT_SELECTED_WHITE_COLOR: ResColor = ResColor {
    r: 255,
    g: 166,
    b: 7,
    a: 255,
};

pub static VANILLA_MENU_ACTIVE: RwLock<bool> = RwLock::new(false);

const ITEM_FLASH_FRAMES: u8 = 24;
static ITEM_FLASH: RwLock<Option<(usize, u8)>> = RwLock::new(None);
const BUTTON_ROWS: [&[u8]; NX_SUBMENU_ROWS] = [
    b"TrModMenuButtonRow0\0", b"TrModMenuButtonRow1\0", b"TrModMenuButtonRow2\0",
    b"TrModMenuButtonRow3\0", b"TrModMenuButtonRow4\0", b"TrModMenuButtonRow5\0",
    b"TrModMenuButtonRow6\0", b"TrModMenuButtonRow7\0",
];
const BUTTON_COLUMNS: [&[u8]; NX_SUBMENU_COLUMNS] = [b"Button0\0", b"Button1\0", b"Button2\0", b"Button3\0"];

// SmashPane's &str wrapper concatenates a NUL on the heap. The short animation
// uses the same native lookup with static NUL-terminated byte strings instead.
unsafe fn find_flash_pane(root: &Pane, name: &'static [u8]) -> *mut Pane {
    smash::ui2d::find_pane_by_name_recursive(root, name.as_ptr().cast())
}

/// Input acknowledgement only: native grant completion keeps its own notification.
pub fn flash_character_item(index: usize) {
    assign(&ITEM_FLASH, Some((index, ITEM_FLASH_FRAMES)));
}

/// Clear every stacked picture, including aliases, item images and old numbers.
/// Called only on input-driven renders, never on idle/animation frames.
unsafe fn clear_button_icons(button: &Pane) {
    let icon = button.find_pane_by_name_recursive_expect("Icon");
    icon.set_visible(false);
    let sentinel = std::ptr::addr_of!(icon.children_list);
    let mut node = (*sentinel).next;
    while !node.is_null() && !std::ptr::eq(node, sentinel) {
        let child = node.cast::<u8>().sub(std::mem::offset_of!(Pane, link)).cast::<Pane>();
        (*child).set_visible(false);
        node = (*node).next;
    }
}

/// At most one tile is updated for 24 draw callbacks, with no stored pane pointer,
/// heap allocation, text rebuild, or resource request. Page changes cancel it.
unsafe fn draw_item_flash(root: &Pane) {
    let Some((index, remaining)) = read(&ITEM_FLASH) else { return; };
    let mut app = lock_write(&QUICK_MENU_APP);
    if !read(&QUICK_MENU_ACTIVE) || app.page != AppPage::TOGGLE
        || app.selected_submenu().id != "character_item_grant"
    {
        assign(&ITEM_FLASH, None);
        return;
    }
    let submenu = app.selected_submenu();
    let selected_row = submenu.toggles.state.selected_row().unwrap_or(0);
    let selected_col = submenu.toggles.state.selected_col().unwrap_or(0);
    let first_row = selected_row / NX_SUBMENU_ROWS * NX_SUBMENU_ROWS;
    let row = index / submenu.toggles.cols;
    let col = index % submenu.toggles.cols;
    if row < first_row || row >= first_row + NX_SUBMENU_ROWS {
        assign(&ITEM_FLASH, None);
        return;
    }
    let Some(button) = find_flash_pane(root, BUTTON_ROWS[row - first_row]).as_ref()
        .and_then(|row| find_flash_pane(row, BUTTON_COLUMNS[col]).as_ref()) else {
        assign(&ITEM_FLASH, None);
        return;
    };
    let selected = row == selected_row && col == selected_col;
    let base = if selected { BG_LEFT_ON_WHITE_COLOR } else { BG_LEFT_OFF_WHITE_COLOR };
    let elapsed = ITEM_FLASH_FRAMES - remaining;
    let strength = if remaining == 1 { 0 } else { 12 - (elapsed % 12).abs_diff(6) * 2 };
    let blend = |a: u8, b: u8| ((u16::from(a) * u16::from(12 - strength)
        + u16::from(b) * u16::from(strength)) / 12) as u8;
    let Some(background) = find_flash_pane(button, b"TitleBg\0").as_mut() else {
        assign(&ITEM_FLASH, None);
        return;
    };
    let material = &mut *background.as_picture().material;
    material.set_white_res_color(ResColor {
        r: blend(base.r, 255), g: blend(base.g, 220), b: blend(base.b, 90), a: 255,
    });
    assign(&ITEM_FLASH, (remaining > 1).then_some((index, remaining.saturating_sub(1))));
}

static GCC_BUTTON_MAPPING: LazyLock<HashMap<&'static str, u16>> = LazyLock::new(|| {
    HashMap::from([
        ("L", 0xE204),
        ("R", 0xE205),
        ("X", 0xE206),
        ("Y", 0xE207),
        ("Z", 0xE208),
    ])
});
static PROCON_BUTTON_MAPPING: LazyLock<HashMap<&'static str, u16>> = LazyLock::new(|| {
    HashMap::from([
        ("L", 0xE0E4),
        ("R", 0xE0E5),
        ("X", 0xE0E2),
        ("Y", 0xE0E3),
        ("ZL", 0xE0E6),
        ("ZR", 0xE0E7),
    ])
});

unsafe fn render_named_panel(root_pane: &Pane, view: &training_mod_tui::macro_panel::View) {
    root_pane
        .find_pane_by_name_recursive_expect("CurrentTab")
        .find_pane_by_name_recursive_expect("set_txt_help")
        .as_textbox()
        .set_text_string(&view.title);
    for name in [
        "LeftTab",
        "RightTab",
        "SaveDefaults",
        "ResetDefaults",
        "ClearToggle",
    ] {
        if let Some(help) = root_pane
            .find_pane_by_name_recursive(name)
            .and_then(|pane| pane.find_pane_by_name_recursive("set_txt_help"))
        {
            help.as_textbox().set_text_string("");
        }
    }
    root_pane
        .find_pane_by_name_recursive_expect("FooterTxt")
        .as_textbox()
        .set_text_string(&view.footer);
    for row in 0..NX_SUBMENU_ROWS {
        let menu_button_row = root_pane
            .find_pane_by_name_recursive_expect(format!("TrModMenuButtonRow{row}").as_str());
        menu_button_row.set_visible(true);
        for col in 0..NX_SUBMENU_COLUMNS {
            let index = row * NX_SUBMENU_COLUMNS + col;
            let menu_button =
                menu_button_row.find_pane_by_name_recursive_expect(format!("Button{col}").as_str());
            let label = view.labels.get(index).map(String::as_str).unwrap_or("");
            if label.is_empty() {
                menu_button.set_visible(false);
                continue;
            }
            menu_button.set_visible(true);
            if let Some(icon) = menu_button.find_pane_by_name_recursive("Icon") {
                icon.set_visible(false);
            }
            if let Some(check) = menu_button.find_pane_by_name_recursive("check") {
                check.set_visible(false);
            }
            for value in 1..=TOGGLE_MAX {
                if let Some(pane) =
                    menu_button.find_pane_by_name_recursive(format!("{value}").as_str())
                {
                    pane.set_visible(false);
                } else {
                    break;
                }
            }
            let title_text = menu_button
                .find_pane_by_name_recursive_expect("TitleTxt")
                .as_textbox();
            let title_bg = menu_button
                .find_pane_by_name_recursive_expect("TitleBg")
                .as_picture();
            let title_bg_material = &mut *title_bg.material;
            title_text.set_text_string(label);
            if index == view.selected {
                title_bg_material.set_white_res_color(BG_LEFT_ON_WHITE_COLOR);
                title_bg_material.set_black_res_color(BG_LEFT_ON_BLACK_COLOR);
                title_text.text_shadow_enable(true);
                title_text.text_outline_enable(true);
                title_text.set_color(255, 255, 255, 255);
            } else {
                title_bg_material.set_white_res_color(BG_LEFT_OFF_WHITE_COLOR);
                title_bg_material.set_black_res_color(BG_LEFT_OFF_BLACK_COLOR);
                title_text.text_shadow_enable(false);
                title_text.text_outline_enable(false);
                title_text.set_color(178, 199, 211, 255);
            }
        }
    }
}

unsafe fn render_submenu_page(app: &mut App, root_pane: &Pane) {
    let tab = app.selected_tab();
    for row in 0..NX_SUBMENU_ROWS {
        let menu_button_row = root_pane
            .find_pane_by_name_recursive_expect(format!("TrModMenuButtonRow{row}").as_str());
        menu_button_row.set_visible(true);
        for col in 0..NX_SUBMENU_COLUMNS {
            if let Some(submenu) = tab.submenus.get(row, col) {
                // Find all the panes we need to modify
                let menu_button = menu_button_row
                    .find_pane_by_name_recursive_expect(format!("Button{col}").as_str());
                let title_text = menu_button
                    .find_pane_by_name_recursive_expect("TitleTxt")
                    .as_textbox();
                let title_bg = menu_button
                    .find_pane_by_name_recursive_expect("TitleBg")
                    .as_picture();
                let title_bg_material = &mut *title_bg.material;
                let is_selected = row == tab.submenus.state.selected_row().unwrap()
                    && col == tab.submenus.state.selected_col().unwrap();

                // Set Pane Visibility
                title_text.set_text_string(submenu.title);

                // In the actual 'layout.arc' file, every icon image is stacked
                // into a single container pane, with each image directly on top of another.
                // Hide all icon images, and strategically mark the icon that
                // corresponds with a particular button to be visible.

                // The original archive's fixed-width native pane name is truncated.
                let icon_name = match submenu.id {
                    "save_damage_limits_player" => "save_damage_limits_playe",
                    // Reuse the native CPU portrait chip without another texture.
                    "fighter_prompt" => "save_damage_cpu",
                    other => other,
                };
                if let Some(pane) = menu_button.find_pane_by_name_recursive(icon_name) {
                    pane.set_visible(true);
                }

                menu_button
                    .find_pane_by_name_recursive_expect("check")
                    .set_visible(false);

                for value in 1..=TOGGLE_MAX {
                    if let Some(pane) =
                        menu_button.find_pane_by_name_recursive(format!("{}", value).as_str())
                    {
                        pane.set_visible(false);
                    } else {
                        break;
                    }
                }

                if is_selected {
                    // Help text
                    root_pane
                        .find_pane_by_name_recursive_expect("FooterTxt")
                        .as_textbox()
                        .set_text_string(submenu.help_text);

                    title_bg_material.set_white_res_color(BG_LEFT_ON_WHITE_COLOR);
                    title_bg_material.set_black_res_color(BG_LEFT_ON_BLACK_COLOR);
                    title_text.text_shadow_enable(true);
                    title_text.text_outline_enable(true);
                    title_text.set_color(255, 255, 255, 255);
                } else {
                    title_bg_material.set_white_res_color(BG_LEFT_OFF_WHITE_COLOR);
                    title_bg_material.set_black_res_color(BG_LEFT_OFF_BLACK_COLOR);
                    title_text.text_shadow_enable(false);
                    title_text.text_outline_enable(false);
                    title_text.set_color(178, 199, 211, 255);
                }
                menu_button.set_visible(true);
                menu_button
                    .find_pane_by_name_recursive_expect("Icon")
                    .set_visible(true);
            }
        }
    }
}

unsafe fn render_toggle_page(app: &mut App, root_pane: &Pane) {
    let stage_page = app.stage_page_active() && app.stage_direct_enabled();
    let stage_direct = stage_page && app.stage_is_direct();
    let direct_stage = app.stage_direct_marker();
    let stage_help = app.stage_help();
    let submenu = app.selected_submenu();
    let item_page = submenu.id == "character_item_grant";
    let selected_row = submenu.toggles.state.selected_row().unwrap_or(0);
    let first_row = selected_row / NX_SUBMENU_ROWS * NX_SUBMENU_ROWS;
    if submenu.toggles.rows > NX_SUBMENU_ROWS {
        root_pane
            .find_pane_by_name_recursive_expect("FooterTxt")
            .as_textbox()
            .set_text_string(&format!(
                "{} - page {}/{}; Up/Down moves between pages.\n{}",
                submenu.title,
                first_row / NX_SUBMENU_ROWS + 1,
                submenu.toggles.rows.div_ceil(NX_SUBMENU_ROWS),
                submenu.help_text.lines().next().unwrap_or("")
            ));
    }
    if stage_page {
        root_pane.find_pane_by_name_recursive_expect("FooterTxt").as_textbox()
            .set_text_string(stage_help);
    }
    // If the options can only be toggled on or off, then use the check icon
    // instead of the number icons
    let use_check_icon = stage_direct || submenu.toggles.get(0, 0).unwrap().max == 1;
    for row in 0..NX_SUBMENU_ROWS {
        let menu_button_row = root_pane
            .find_pane_by_name_recursive_expect(format!("TrModMenuButtonRow{row}").as_str());
        menu_button_row.set_visible(true);
        for col in 0..NX_SUBMENU_COLUMNS {
            if let Some(toggle) = submenu.toggles.get(first_row + row, col) {
                let menu_button = menu_button_row
                    .find_pane_by_name_recursive_expect(format!("Button{col}").as_str());
                menu_button.set_visible(true);
                let title_text = menu_button
                    .find_pane_by_name_recursive_expect("TitleTxt")
                    .as_textbox();
                let title_bg = menu_button
                    .find_pane_by_name_recursive_expect("TitleBg")
                    .as_picture();
                let title_bg_material = &mut *title_bg.material;
                let is_selected = first_row + row == selected_row
                    && col == submenu.toggles.state.selected_col().unwrap();

                if is_selected {
                    title_text.text_shadow_enable(true);
                    title_text.text_outline_enable(true);
                    title_text.set_color(255, 255, 255, 255);
                    title_bg_material.set_white_res_color(BG_LEFT_ON_WHITE_COLOR);
                    title_bg_material.set_black_res_color(BG_LEFT_ON_BLACK_COLOR);
                } else {
                    title_text.text_shadow_enable(false);
                    title_text.text_outline_enable(false);
                    title_text.set_color(178, 199, 211, 255);
                    title_bg_material.set_white_res_color(BG_LEFT_OFF_WHITE_COLOR);
                    title_bg_material.set_black_res_color(BG_LEFT_OFF_BLACK_COLOR);
                }

                let ordered = matches!(submenu.submenu_type, SubMenuType::Ordered);
                let index = (first_row + row) * submenu.toggles.cols + col;
                let all = ordered && submenu.toggles.get(0, 0).unwrap().value != 0;
                let displayed_value = if stage_direct {
                    u8::from(index.checked_sub(1).and_then(|i| training_mod_consts::lobby_stages::STAGES.get(i))
                        .is_some_and(|(id, _)| Some(*id) == direct_stage))
                } else if all && index > 0 {
                    index as u8
                } else {
                    toggle.value
                };
                if ordered && !stage_direct && displayed_value > 0 && index > 0 {
                    title_text.set_text_string(&format!("{}. {}", displayed_value, toggle.title));
                } else {
                    title_text.set_text_string(toggle.title);
                }

                if item_page {
                    if let Some(picture) = menu_button.find_pane_by_name_recursive(
                        &format!("item_icon_{index:02}")) {
                        picture.set_visible(true);
                    }
                    menu_button.find_pane_by_name_recursive_expect("Icon").set_visible(true);
                } else if use_check_icon {
                    menu_button
                        .find_pane_by_name_recursive_expect("check")
                        .set_visible(true);

                    menu_button
                        .find_pane_by_name_recursive_expect("Icon")
                        .set_visible(displayed_value > 0);
                } else {
                    menu_button
                        .find_pane_by_name_recursive_expect("check")
                        .set_visible(false);
                    menu_button
                        .find_pane_by_name_recursive_expect("Icon")
                        .set_visible(displayed_value > 0);

                    // Note there's no pane for 0
                    for value in 1..=toggle.max {
                        let err_msg = format!("Could not find pane with name {}", value);
                        menu_button
                            .find_pane_by_name_recursive(format!("{}", value).as_str())
                            .expect(&err_msg)
                            .set_visible(value == toggle.value);
                    }
                }
            }
        }
    }
}

unsafe fn render_slider_page(app: &mut App, root_pane: &Pane) {
    let submenu = app.selected_submenu();
    let slider = submenu.slider.unwrap();
    let selected_min = slider.lower;
    let selected_max = slider.upper;

    let slider_pane = root_pane.find_pane_by_name_recursive_expect("TrModSlider");
    slider_pane.set_visible(true);

    let _background = slider_pane
        .find_pane_by_name_recursive_expect("Background")
        .as_picture();
    let header = slider_pane
        .find_pane_by_name_recursive_expect("Header")
        .as_textbox();
    header.set_text_string(submenu.title);
    let min_button = slider_pane
        .find_pane_by_name_recursive_expect("MinButton")
        .as_picture();
    let max_button = slider_pane
        .find_pane_by_name_recursive_expect("MaxButton")
        .as_picture();
    let min_title_text = min_button
        .find_pane_by_name_recursive_expect("TitleTxt")
        .as_textbox();
    let min_title_bg = min_button
        .find_pane_by_name_recursive_expect("TitleBg")
        .as_picture();
    let min_value_text = min_button
        .find_pane_by_name_recursive_expect("ValueTxt")
        .as_textbox();
    let max_title_text = max_button
        .find_pane_by_name_recursive_expect("TitleTxt")
        .as_textbox();
    let max_title_bg = max_button
        .find_pane_by_name_recursive_expect("TitleBg")
        .as_picture();
    let max_value_text = max_button
        .find_pane_by_name_recursive_expect("ValueTxt")
        .as_textbox();

    min_title_text.set_text_string("Min");
    match slider.state {
        SliderState::LowerHover | SliderState::LowerSelected => {
            min_title_text.text_shadow_enable(true);
            min_title_text.text_outline_enable(true);
            min_title_text.set_color(255, 255, 255, 255);
        }
        _ => {
            min_title_text.text_shadow_enable(false);
            min_title_text.text_outline_enable(false);
            min_title_text.set_color(178, 199, 211, 255);
        }
    }

    max_title_text.set_text_string("Max");
    match slider.state {
        SliderState::UpperHover | SliderState::UpperSelected => {
            max_title_text.text_shadow_enable(true);
            max_title_text.text_outline_enable(true);
            max_title_text.set_color(255, 255, 255, 255);
        }
        _ => {
            max_title_text.text_shadow_enable(false);
            max_title_text.text_outline_enable(false);
            max_title_text.set_color(178, 199, 211, 255);
        }
    }

    min_value_text.set_text_string(&format!("{selected_min}"));
    max_value_text.set_text_string(&format!("{selected_max}"));

    let min_title_bg_material = &mut *min_title_bg.as_picture().material;
    let min_colors = match slider.state {
        SliderState::LowerHover => (BG_LEFT_ON_WHITE_COLOR, BG_LEFT_ON_BLACK_COLOR),
        SliderState::LowerSelected => (BG_LEFT_SELECTED_WHITE_COLOR, BG_LEFT_SELECTED_BLACK_COLOR),
        _ => (BG_LEFT_OFF_WHITE_COLOR, BG_LEFT_OFF_BLACK_COLOR),
    };

    min_title_bg_material.set_white_res_color(min_colors.0);
    min_title_bg_material.set_black_res_color(min_colors.1);

    let max_title_bg_material = &mut *max_title_bg.as_picture().material;
    let max_colors = match slider.state {
        SliderState::UpperHover => (BG_LEFT_ON_WHITE_COLOR, BG_LEFT_ON_BLACK_COLOR),
        SliderState::UpperSelected => (BG_LEFT_SELECTED_WHITE_COLOR, BG_LEFT_SELECTED_BLACK_COLOR),
        _ => (BG_LEFT_OFF_WHITE_COLOR, BG_LEFT_OFF_BLACK_COLOR),
    };

    max_title_bg_material.set_white_res_color(max_colors.0);
    max_title_bg_material.set_black_res_color(max_colors.1);

    min_value_text.set_visible(true);
    max_value_text.set_visible(true);

    // Hide the Icon pane for MinButton and MaxButton
    [min_button, max_button].iter().for_each(|button| {
        let icon = button.find_pane_by_name_recursive_expect("Icon");
        icon.set_visible(false);
    });
}

unsafe fn render_confirmation_page(app: &mut App, root_pane: &Pane) {
    let show_row = 3; // Row in the middle of the page
    let show_cols = [1, 2]; // Columns in the middle of the page
    let no_col = show_cols[0]; // Left
    let yes_col = show_cols[1]; // Right
    let help_text = match app.confirmation_return_page {
        AppPage::TOGGLE | AppPage::SLIDER => {
            "Are you sure you want to reset the current setting to the defaults?"
        }
        AppPage::SUBMENU => "Are you sure you want to reset ALL settings to the defaults?",
        _ => "", // Shouldn't ever get this case, but don't panic if we do
    };

    // Set help text
    root_pane
        .find_pane_by_name_recursive_expect("FooterTxt")
        .as_textbox()
        .set_text_string(help_text);

    // Show only the buttons that we care about
    for row in 0..NX_SUBMENU_ROWS {
        let should_show_row = row == show_row;
        let menu_button_row = root_pane
            .find_pane_by_name_recursive_expect(format!("TrModMenuButtonRow{row}").as_str());
        menu_button_row.set_visible(should_show_row);
        if should_show_row {
            for col in 0..NX_SUBMENU_COLUMNS {
                let should_show_col = show_cols.contains(&col);
                let menu_button = menu_button_row
                    .find_pane_by_name_recursive_expect(format!("Button{col}").as_str());
                menu_button.set_visible(should_show_col);
                if should_show_col {
                    let title_text = menu_button
                        .find_pane_by_name_recursive_expect("TitleTxt")
                        .as_textbox();
                    let title_bg = menu_button
                        .find_pane_by_name_recursive_expect("TitleBg")
                        .as_picture();
                    let title_bg_material = &mut *title_bg.material;

                    if col == no_col {
                        title_text.set_text_string("No");
                    } else if col == yes_col {
                        title_text.set_text_string("Yes");
                    }
                    let is_selected = (col == no_col
                        && app.confirmation_state == ConfirmationState::HoverNo)
                        || (col == yes_col
                            && app.confirmation_state == ConfirmationState::HoverYes);

                    if is_selected {
                        title_text.text_shadow_enable(true);
                        title_text.text_outline_enable(true);
                        title_text.set_color(255, 255, 255, 255);
                        title_bg_material.set_white_res_color(BG_LEFT_ON_WHITE_COLOR);
                        title_bg_material.set_black_res_color(BG_LEFT_ON_BLACK_COLOR);
                    } else {
                        title_text.text_shadow_enable(false);
                        title_text.text_outline_enable(false);
                        title_text.set_color(178, 199, 211, 255);
                        title_bg_material.set_white_res_color(BG_LEFT_OFF_WHITE_COLOR);
                        title_bg_material.set_black_res_color(BG_LEFT_OFF_BLACK_COLOR);
                    }

                    // Hide all submenu icons, since we're not on the submenu page
                    // TODO: Do we want to show the check on "Yes" and a red "X" on "No?"
                    for t in app.tabs.iter() {
                        for s in t.submenus.iter() {
                            let pane = menu_button.find_pane_by_name_recursive(s.id);
                            if let Some(p) = pane {
                                p.set_visible(false);
                            }
                        }
                    }
                }
            }
        }
    }
}

pub unsafe fn draw(root_pane: &Pane, native_root: &Pane) {
    // Determine if we're in the menu by seeing if the "help" footer has
    // begun moving upward. It starts at -80 and moves to 0 over 10 frames
    // in info_training_in_menu.bflan
    let waiting = crate::training::lobby::is_waiting_scene();
    let vanilla_active = if waiting {
        let popup_busy = crate::common::game_mode::popup_busy().unwrap_or(true);
        popup_busy
            || native_root
                .find_pane_by_name_recursive("option_menu_setting")
                .is_none_or(|pane| pane.pos_x != -540.0)
    } else {
        native_root
            .find_pane_by_name_recursive_expect("L_staying_help")
            .pos_y
            != -80.0
    };
    if waiting {
        crate::training::lobby::set_native_ui_busy(vanilla_active);
    }
    assign(&VANILLA_MENU_ACTIVE, vanilla_active);

    let clip_foreground = crate::training::named_macros::clip_foreground() && !vanilla_active;
    if !clip_foreground {
        super::clip_editor::restore(root_pane);
    }
    let panel_open =
        crate::training::named_macros::panel_visible() || crate::replay_capture::loading();
    let roster_open = !panel_open
        && read(&QUICK_MENU_ACTIVE)
        && lock_read(&QUICK_MENU_APP).page == AppPage::ROSTER;
    let stage_open = !panel_open && read(&QUICK_MENU_ACTIVE) && {
        let mut app = lock_write(&QUICK_MENU_APP);
        app.page == AppPage::TOGGLE && app.selected_submenu().id == "lobby_stages"
    };
    // Visibility is lifecycle state, not an input-driven menu update. In
    // particular, neither the clip branch nor a quiet loading frame may skip it.
    if let Some(status_r_pane) = native_root.find_pane_by_name_recursive("status_R") {
        status_r_pane.set_visible(!read(&QUICK_MENU_ACTIVE) && !panel_open);
    }
    let overall_parent_pane = root_pane.find_pane_by_name_recursive_expect("TrModMenu");
    overall_parent_pane
        .set_visible((read(&QUICK_MENU_ACTIVE) || panel_open) && !read(&VANILLA_MENU_ACTIVE));
    if read(&QUICK_MENU_ACTIVE) || panel_open {
        // The parent hides the grid during gameplay; no extra grid traversal then.
        super::roster_grid::visibility(overall_parent_pane, roster_open);
        super::stage_grid::visibility(overall_parent_pane, stage_open);
    }
    let menu_close_wait_frame = frame_counter::get_frame_count(*MENU_CLOSE_FRAME_COUNTER);
    if panel_open {
        overall_parent_pane.alpha = 255;
        overall_parent_pane.global_alpha = 255;
    } else {
        fade_out(
            overall_parent_pane,
            MENU_CLOSE_WAIT_FRAMES.saturating_sub(menu_close_wait_frame),
            MENU_CLOSE_WAIT_FRAMES,
        );
    }

    if clip_foreground {
        assign(&ITEM_FLASH, None);
        if let Some(view) = crate::replay_capture::clip_view() {
            super::clip_editor::draw(root_pane, &view, crate::replay_preview::status());
        }
        return;
    }
    // Only submit updates if we have received input
    let received_input = read(&MENU_RECEIVED_INPUT);
    if !received_input {
        if read(&VANILLA_MENU_ACTIVE) || panel_open {
            assign(&ITEM_FLASH, None);
        } else {
            draw_item_flash(root_pane);
            if roster_open {
                super::roster_grid::draw_manual_flash(root_pane, &mut lock_write(&QUICK_MENU_APP));
            }
        }
        return;
    } else {
        assign(&MENU_RECEIVED_INPUT, false);
    }

    if let Some(quit_button) = root_pane.find_pane_by_name_recursive("TrModTitle") {
        for quit_txt_s in &["set_txt_00", "set_txt_01"] {
            if let Some(quit_txt) = quit_button.find_pane_by_name_recursive(quit_txt_s) {
                quit_txt.as_textbox().set_text_string(
                    if crate::training::lobby::is_waiting_scene() {
                        "Lobby Practice"
                    } else {
                        "Modpack Menu"
                    },
                );
            }
        }
    }

    // Make all invisible first
    for row_idx in 0..NX_SUBMENU_ROWS {
        for col_idx in 0..NX_SUBMENU_COLUMNS {
            let menu_button_row = root_pane.find_pane_by_name_recursive_expect(
                format!("TrModMenuButtonRow{row_idx}").as_str(),
            );
            menu_button_row.set_visible(false);

            let menu_button = menu_button_row
                .find_pane_by_name_recursive_expect(format!("Button{col_idx}").as_str());
            menu_button.set_visible(false);
            clear_button_icons(menu_button);

            menu_button
                .find_pane_by_name_recursive_expect("ValueTxt")
                .set_visible(false);
        }
    }

    root_pane
        .find_pane_by_name_recursive_expect("TrModSlider")
        .set_visible(false);

    // Update menu display
    // Grabbing lock as read-only, essentially
    let mut app = lock_write(&QUICK_MENU_APP);
    let roster_page = app.page == AppPage::ROSTER;
    app.set_original_player_kind(crate::character_roster::original_player_kind());
    super::roster_grid::ordinary_controls(root_pane, !roster_page);
    if roster_page {
        super::roster_grid::draw(
            root_pane,
            &app,
            read(&P1_CONTROLLER_STYLE) == ControllerStyle::GCController,
        );
        super::roster_grid::draw_manual_flash(root_pane, &mut app);
        return;
    }
    if let Some(view) = app.mii_view() {
        render_named_panel(root_pane, &view.panel);
        // Reuse the ordinary event-driven grid; disabled saved-original rows
        // remain visible, but both UI input and selection policy refuse them.
        for (index, disabled) in view.disabled.iter().enumerate() {
            if *disabled {
                let row = root_pane.find_pane_by_name_recursive_expect(
                    format!("TrModMenuButtonRow{}", index / NX_SUBMENU_COLUMNS).as_str());
                let button = row.find_pane_by_name_recursive_expect(
                    format!("Button{}", index % NX_SUBMENU_COLUMNS).as_str());
                button.find_pane_by_name_recursive_expect("TitleTxt").as_textbox()
                    .set_color(100, 100, 100, 255);
            }
        }
        return;
    }
    if stage_open && super::stage_grid::draw(root_pane, &mut app) {
        super::roster_grid::ordinary_controls(root_pane, false);
        return;
    }
    // We don't really need to change anything, but get_before_selected requires &mut self

    let tab_titles = [
        app.tabs
            .get_before_selected()
            .expect("No tab selected!")
            .title,
        app.tabs.get_selected().expect("No tab selected!").title,
        app.tabs
            .get_after_selected()
            .expect("No tab selected!")
            .title,
    ];

    let is_gcc = read(&P1_CONTROLLER_STYLE) == ControllerStyle::GCController;
    let button_mapping = if is_gcc {
        &(*GCC_BUTTON_MAPPING)
    } else {
        &(*PROCON_BUTTON_MAPPING)
    };

    let (x_key, y_key, l_key, r_key, zl_key, zr_key, z_key) = (
        button_mapping.get("X"),
        button_mapping.get("Y"),
        button_mapping.get("L"),
        button_mapping.get("R"),
        button_mapping.get("ZL"),
        button_mapping.get("ZR"),
        button_mapping.get("Z"),
    );

    let (left_tab_key, right_tab_key, save_defaults_key, reset_key, clear_toggle_key) = if is_gcc {
        (l_key, r_key, x_key, z_key, y_key)
    } else {
        (zl_key, zr_key, x_key, r_key, y_key)
    };

    [
        (left_tab_key, "LeftTab"),
        (None, "CurrentTab"),
        (right_tab_key, "RightTab"),
    ]
    .iter()
    .enumerate()
    .for_each(|(idx, (key, name))| {
        let key_help_pane = root_pane.find_pane_by_name_recursive_expect(name);

        let icon_pane = key_help_pane
            .find_pane_by_name_recursive_expect("set_txt_icon")
            .as_textbox();
        let help_pane = key_help_pane
            .find_pane_by_name_recursive_expect("set_txt_help")
            .as_textbox();
        icon_pane.set_text_string("");

        // Left/Right tabs have keys
        if let Some(key) = key {
            set_icon_text(icon_pane, &[**key]);
        }

        if *name == "CurrentTab" {
            icon_pane.set_text_string("");
            // Center tab should be highlighted
            help_pane.set_default_material_colors();
            help_pane.set_color(255, 255, 0, 255);
        }
        help_pane.set_text_string(tab_titles[idx]);
    });

    // Save Defaults Keyhelp
    let name = "SaveDefaults";
    let key = save_defaults_key;
    let title = if app.stage_page_active() && app.stage_direct_enabled() {
        if app.stage_is_direct() { "Lobby Rotation" } else { "Change Stage Now" }
    } else { "Save Defaults" };
    let key_help_pane = root_pane.find_pane_by_name_recursive_expect(name);
    let icon_pane = key_help_pane
        .find_pane_by_name_recursive_expect("set_txt_icon")
        .as_textbox();
    set_icon_text(icon_pane, &[*key.unwrap()]);
    key_help_pane
        .find_pane_by_name_recursive_expect("set_txt_help")
        .as_textbox()
        .set_text_string(title);

    // Reset Keyhelp
    let name = "ResetDefaults";
    let key = reset_key;
    let item_page = app.page == AppPage::TOGGLE && app.selected_submenu().id == "character_item_grant";
    let title = match app.page {
        AppPage::TOGGLE if item_page => "",
        AppPage::TOGGLE if app.stage_page_active() && app.stage_is_direct() => "",
        AppPage::SUBMENU => "Reset All",
        AppPage::SLIDER => "Reset Current",
        AppPage::TOGGLE => "Reset Current",
        AppPage::CONFIRMATION => "",
        AppPage::CLOSE => "",
        AppPage::ROSTER => "",
    };
    root_pane.find_pane_by_name_recursive_expect(name).set_visible(!title.is_empty());
    if !title.is_empty() {
        let key_help_pane = root_pane.find_pane_by_name_recursive_expect(name);
        let icon_pane = key_help_pane
            .find_pane_by_name_recursive_expect("set_txt_icon")
            .as_textbox();
        set_icon_text(icon_pane, &[*key.unwrap()]);
        key_help_pane
            .find_pane_by_name_recursive_expect("set_txt_help")
            .as_textbox()
            .set_text_string(title);
    }

    // Clear Toggle Keyhelp
    let name = "ClearToggle";
    let key_help_pane = root_pane.find_pane_by_name_recursive_expect(name);
    let icon_pane = key_help_pane.find_pane_by_name_recursive_expect("set_txt_icon");
    if !(app.stage_page_active() && app.stage_is_direct()) && app.should_show_clear_keyhelp() {
        // This is only displayed when you're in a multiple selection toggle menu w/ toggle.max > 1
        let key = clear_toggle_key;
        let title = "Clear Toggle";
        set_icon_text(icon_pane.as_textbox(), &[*key.unwrap()]);
        key_help_pane
            .find_pane_by_name_recursive_expect("set_txt_help")
            .as_textbox()
            .set_text_string(title);
        icon_pane.set_visible(true);
        key_help_pane.set_visible(true);
    } else {
        icon_pane.set_visible(false);
        key_help_pane.set_visible(false);
    }

    match app.page {
        AppPage::SUBMENU => render_submenu_page(&mut app, root_pane),
        AppPage::SLIDER => render_slider_page(&mut app, root_pane),
        AppPage::TOGGLE => render_toggle_page(&mut app, root_pane),
        AppPage::CONFIRMATION => render_confirmation_page(&mut app, root_pane),
        AppPage::CLOSE => {}
        AppPage::ROSTER => {}
    }
    if let Some(view) = crate::training::named_macros::panel_view() {
        render_named_panel(root_pane, &view);
    }

    // Preferences can still be edited for the next ordinary Training session.
    // Explain the current capability instead of silently promising a lobby action.
    if crate::training::lobby::is_waiting_scene()
        && !crate::training::named_macros::panel_visible()
        && !matches!(app.page, AppPage::CONFIRMATION | AppPage::CLOSE)
    {
        let tab = app.selected_tab().id;
        let submenu = app.selected_submenu().id;
        if (!crate::training::practice::scene_enabled() && matches!(tab, "input" | "save_state"))
            || (!cfg!(feature = "waiting-roster-reload")
                && matches!(tab, "player_characters" | "cpu_characters"))
            || (!crate::training::practice::scene_enabled() && matches!(
                submenu,
                "save_state_save"
                    | "save_state_load"
                    | "input_record"
                    | "input_playback"
                    | "stage_hazards"
                    | "lra_reset"
                    | "hitbox_vis"
                    | "tech_hide"
            ))
        {
            root_pane
                .find_pane_by_name_recursive_expect("FooterTxt")
                .as_textbox()
                .set_text_string(
                    "Available in Training mode only.\nChanges here are saved for Training mode.",
                );
        }
    }
    drop(app);
    if read(&VANILLA_MENU_ACTIVE) || panel_open {
        assign(&ITEM_FLASH, None);
    } else {
        draw_item_flash(root_pane);
    }
}
