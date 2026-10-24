//! Stock stage thumbnails, updated only when the menu receives input.
use skyline::nn::ui2d::{Pane, PaneFlag};
use smash::ui2d::{SmashPane, SmashTextBox};
use training_mod_consts::lobby_stages::STAGES;
use training_mod_tui::{stage_menu::{PAGE_COLUMNS as COLUMNS, PAGE_SIZE}, App};

const ICON_SLOTS: usize = {
    let mut slots = 0;
    let mut index = 0;
    while index < STAGES.len() {
        let end = STAGES[index].0 as usize + 1;
        if end > slots {
            slots = end;
        }
        index += 1;
    }
    slots
};

struct Panes {
    icons: [*mut Pane; ICON_SLOTS],
    tiles: [*mut Pane; PAGE_SIZE],
    labels: [*mut Pane; PAGE_SIZE],
    ranks: [*mut Pane; PAGE_SIZE],
    all_icon: *mut Pane,
    cursor: *mut Pane,
    title: *mut Pane,
    name: *mut Pane,
    help: *mut Pane,
}

impl Panes {
    unsafe fn index(panel: &Pane) -> Self {
        let mut panes = Self {
            icons: [std::ptr::null_mut(); ICON_SLOTS],
            tiles: [std::ptr::null_mut(); PAGE_SIZE],
            labels: [std::ptr::null_mut(); PAGE_SIZE],
            ranks: [std::ptr::null_mut(); PAGE_SIZE],
            all_icon: std::ptr::null_mut(),
            cursor: std::ptr::null_mut(),
            title: std::ptr::null_mut(),
            name: std::ptr::null_mut(),
            help: std::ptr::null_mut(),
        };
        // The generated stage panel has unique, direct children, like the roster panel.
        // Index only this draw: layout allocations can change between scenes.
        let sentinel = std::ptr::addr_of!(panel.children_list);
        let mut node = (*sentinel).next;
        let mut visited = 0;
        while !node.is_null() && !std::ptr::eq(node, sentinel) && visited < 512 {
            visited += 1;
            let child = node
                .cast::<u8>()
                .sub(std::mem::offset_of!(Pane, link))
                .cast::<Pane>();
            let bytes = &(*child).name;
            let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
            if let Ok(name) = std::str::from_utf8(&bytes[..end]) {
                if name.starts_with("stage_icon_") || name == "stage_all_icon" {
                    (*child).set_visible(false);
                }
                if let Some(index) = name
                    .strip_prefix("stage_icon_")
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    if let Some(slot) = panes.icons.get_mut(index) {
                        *slot = child;
                    }
                } else if let Some(index) = name
                    .strip_prefix("stage_tile_")
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    if let Some(slot) = panes.tiles.get_mut(index) {
                        *slot = child;
                    }
                } else if let Some(index) = name
                    .strip_prefix("stage_label_")
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    if let Some(slot) = panes.labels.get_mut(index) {
                        *slot = child;
                    }
                } else if let Some(index) = name
                    .strip_prefix("stage_rank_")
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    if let Some(slot) = panes.ranks.get_mut(index) {
                        *slot = child;
                    }
                } else {
                    match name {
                        "stage_all_icon" => panes.all_icon = child,
                        "stage_cursor" => panes.cursor = child,
                        "stage_title" => panes.title = child,
                        "stage_name" => panes.name = child,
                        "stage_help" => panes.help = child,
                        _ => {}
                    }
                }
            }
            node = (*node).next;
        }
        panes
    }
}

pub unsafe fn visibility(root: &Pane, visible: bool) {
    if let Some(panel) = root.find_pane_by_name_recursive("stage_grid") {
        panel.set_visible(visible);
    }
}

unsafe fn label(pane: *mut Pane, text: &str, color: [u8; 4]) {
    if let Some(pane) = pane.as_mut() {
        let text_box = pane.as_textbox();
        text_box.set_text_string(text);
        text_box.set_color(color[0], color[1], color[2], color[3]);
    }
}

unsafe fn position(pane: &mut Pane, cell: usize, y_offset: f32) {
    pane.pos_x = (cell % COLUMNS) as f32 * 460.0 - 690.0;
    pane.pos_y = 300.0 - (cell / COLUMNS) as f32 * 200.0 + y_offset;
    pane.flags |= 1 << PaneFlag::IsGlobalMatrixDirty as u8;
}

/// Returns false for an older layout so the ordinary text selector remains usable.
pub unsafe fn draw(root: &Pane, app: &mut App<'_>) -> bool {
    let Some(panel) = root.find_pane_by_name_recursive("stage_grid") else {
        return false;
    };
    let direct = app.stage_is_direct();
    let title = app.stage_title();
    let direct_stage = app.stage_direct_marker();
    let help = app.stage_help();
    let submenu = app.selected_submenu();
    let selected = submenu.toggles.state.selected_row().unwrap_or(0) * COLUMNS
        + submenu.toggles.state.selected_col().unwrap_or(0);
    let first = selected / PAGE_SIZE * PAGE_SIZE;
    let all = submenu.toggles.get(0, 0).is_some_and(|t| t.value != 0);

    let panes = Panes::index(panel);

    for cell in 0..PAGE_SIZE {
        let index = first + cell;
        let toggle = submenu.toggles.get(index / COLUMNS, index % COLUMNS);
        if let Some(tile) = panes.tiles[cell].as_mut() {
            tile.set_visible(toggle.is_some());
        }
        let rank = toggle.map_or(0, |t| {
            if all && index > 0 {
                index as u8
            } else {
                t.value
            }
        });
        label(
            panes.labels[cell],
            toggle.map_or("", |t| t.title),
            if index == selected {
                [255; 4]
            } else {
                [205, 218, 230, 255]
            },
        );
        label(
            panes.ranks[cell],
            &if direct {
                if index.checked_sub(1).and_then(|i| STAGES.get(i)).is_some_and(|(id, _)| Some(*id) == direct_stage) {
                    "Selected".into()
                } else { String::new() }
            } else if rank == 0 {
                String::new()
            } else if index == 0 {
                "All".into()
            } else {
                rank.to_string()
            },
            [90, 180, 255, 255],
        );
        let icon = if index == 0 {
            panes.all_icon
        } else {
            // Waiting Room 311 and Training 308 remain readable text tiles when
            // the layout has no portrait pane for that native stage ID.
            STAGES
                .get(index - 1)
                .and_then(|(id, _)| panes.icons.get(*id as usize))
                .copied()
                .unwrap_or(std::ptr::null_mut())
        };
        if let Some(icon) = icon.as_mut() {
            position(icon, cell, 15.0);
            icon.set_visible(toggle.is_some());
        }
    }
    if let Some(cursor) = panes.cursor.as_mut() {
        position(cursor, selected % PAGE_SIZE, 0.0);
    }
    label(panes.title, title, [255; 4]);
    label(
        panes.name,
        submenu
            .toggles
            .get(selected / COLUMNS, selected % COLUMNS)
            .map_or("", |t| t.title),
        [255; 4],
    );
    label(
        panes.help,
        &format!(
            "{}   Page {}/{}",
            if direct && selected == 0 { "Choose one named stage   X Lobby Stage Rotation   L/R Page   B Back" } else { help },
            first / PAGE_SIZE + 1,
            (STAGES.len() + 1).div_ceil(PAGE_SIZE)
        ),
        [235, 239, 245, 255],
    );
    true
}
