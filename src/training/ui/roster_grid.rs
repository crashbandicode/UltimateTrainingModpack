//! Native portrait grid. Pane addresses are indexed only for the current draw.
use skyline::nn::ui2d::{Pane, PaneFlag};
use smash::ui2d::{SmashPane, SmashTextBox};
use training_mod_tui::{
    roster_grid::{self, CellContent, Role, RosterPurpose, CELLS},
    App,
};

const PLAYER: [u8; 4] = [255, 83, 92, 255];
const CPU: [u8; 4] = [75, 164, 255, 255];
const ORIGINAL_CPU: [u8; 4] = [72, 214, 112, 255];

/// Called on input-driven page changes; the usual renderer restores row state.
pub unsafe fn ordinary_controls(root: &Pane, visible: bool) {
    for name in [
        "TrModSelectors",
        "TrModTitle",
        "FooterBackground",
        "FooterTxt",
        "LeftTab",
        "CurrentTab",
        "RightTab",
        "SaveDefaults",
        "ResetDefaults",
    ] {
        if let Some(pane) = root.find_pane_by_name_recursive(name) {
            pane.set_visible(visible);
        }
    }
    if !visible {
        if let Some(pane) = root.find_pane_by_name_recursive("ClearToggle") {
            pane.set_visible(false);
        }
    }
}

pub unsafe fn visibility(root: &Pane, visible: bool) {
    if let Some(pane) = root.find_pane_by_name_recursive("roster_grid") {
        pane.set_visible(visible);
    }
}

struct Panes {
    icons: [*mut Pane; 0x5f],
    player: [*mut Pane; CELLS],
    cpu: [*mut Pane; CELLS],
    all_player: [*mut Pane; CELLS],
    all_cpu: [*mut Pane; CELLS],
    cursor: *mut Pane,
    title: *mut Pane,
    name: *mut Pane,
    help: *mut Pane,
    role: *mut Pane,
    solo_player: *mut Pane,
    solo_cpu: *mut Pane,
}

impl Panes {
    unsafe fn index(parent: &Pane) -> Self {
        let mut panes = Self {
            icons: [std::ptr::null_mut(); 0x5f],
            player: [std::ptr::null_mut(); CELLS],
            cpu: [std::ptr::null_mut(); CELLS],
            all_player: [std::ptr::null_mut(); CELLS],
            all_cpu: [std::ptr::null_mut(); CELLS],
            cursor: std::ptr::null_mut(),
            title: std::ptr::null_mut(),
            name: std::ptr::null_mut(),
            help: std::ptr::null_mut(),
            role: std::ptr::null_mut(),
            solo_player: std::ptr::null_mut(),
            solo_cpu: std::ptr::null_mut(),
        };
        let sentinel = std::ptr::addr_of!(parent.children_list);
        let mut node = (*sentinel).next;
        let mut visited = 0;
        while !node.is_null() && !std::ptr::eq(node, sentinel) && visited < 512 {
            visited += 1;
            let child = node
                .cast::<u8>()
                .sub(std::mem::offset_of!(Pane, link))
                .cast::<Pane>();
            let raw = &(*child).name;
            let end = raw.iter().position(|c| *c == 0).unwrap_or(raw.len());
            if let Ok(name) = std::str::from_utf8(&raw[..end]) {
                if let Some(index) = name
                    .strip_prefix("roster_icon_")
                    .and_then(|n| usize::from_str_radix(n, 16).ok())
                {
                    if let Some(slot) = panes.icons.get_mut(index) {
                        *slot = child;
                    }
                } else if let Some(index) = name
                    .strip_prefix("roster_p_")
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    if let Some(slot) = panes.player.get_mut(index) {
                        *slot = child;
                    }
                } else if let Some(index) = name
                    .strip_prefix("roster_c_")
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    if let Some(slot) = panes.cpu.get_mut(index) {
                        *slot = child;
                    }
                } else if let Some(index) = name
                    .strip_prefix("roster_all_p_")
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    if let Some(slot) = panes.all_player.get_mut(index) {
                        *slot = child;
                    }
                } else if let Some(index) = name
                    .strip_prefix("roster_all_c_")
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    if let Some(slot) = panes.all_cpu.get_mut(index) {
                        *slot = child;
                    }
                } else {
                    match name {
                        "roster_cursor" => panes.cursor = child,
                        "roster_title" => panes.title = child,
                        "roster_name" => panes.name = child,
                        "roster_help" => panes.help = child,
                        "roster_role" => panes.role = child,
                        "roster_solo_p" => panes.solo_player = child,
                        "roster_solo_c" => panes.solo_cpu = child,
                        _ => {}
                    }
                }
            }
            node = (*node).next;
        }
        panes
    }
}

unsafe fn label(pane: *mut Pane, text: &str, color: [u8; 4]) {
    if let Some(pane) = pane.as_mut() {
        let textbox = pane.as_textbox();
        textbox.set_text_string(text);
        textbox.set_color(color[0], color[1], color[2], color[3]);
    }
}

fn cycle_summary(values: &[u8]) -> String {
    if values.first() == Some(&1) {
        "All".into()
    } else {
        format!(
            "{} selected",
            values.iter().skip(1).filter(|rank| **rank != 0).count()
        )
    }
}

pub unsafe fn draw(root: &Pane, app: &App<'_>, gcc: bool) {
    let Some(panel) = root.find_pane_by_name_recursive("roster_grid") else {
        return;
    };
    let panes = Panes::index(panel);
    let manual = app.roster_purpose == RosterPurpose::ManualSwap;
    let selected = if manual {
        [app.manual_selected_kind(Role::Player), app.manual_selected_kind(Role::Cpu)]
    } else { [None; 2] };
    let player = app.roster_values(Role::Player);
    let cpu = app.roster_values(Role::Cpu);
    let player_badges = app.roster_badges_from_values(Role::Player, &player);
    let cpu_badges = app.roster_badges_from_values(Role::Cpu, &cpu);
    for pane in panes.icons.iter().copied().filter(|pane| !pane.is_null()) {
        (*pane).set_visible(false);
    }
    for cell in 0..CELLS {
        let info = app.roster_grid.info(cell).expect("valid roster cell");
        let (kind, supported) = match info.content {
            CellContent::Fighter(kind) => (Some(kind), true),
            CellContent::Unsupported => (
                match info.label {
                    "Mii Brawler" => Some(0x48),
                    "Mii Swordfighter" => Some(0x49),
                    "Mii Gunner" => Some(0x4a),
                    _ => None,
                },
                false,
            ),
            CellContent::All => (if manual { None } else { Some(0x5e) }, !manual),
        };
        if let Some(kind) = kind {
            if let Some(pane) = panes
                .icons
                .get(kind as usize)
                .and_then(|pane| pane.as_mut())
            {
                pane.set_visible(true);
                let excluded = app.roster_excluded(app.roster_grid.role(), kind);
                let disabled = !supported || excluded || app.hat_cell_disabled(kind);
                let highlighted = app.highlights_original_cpu(kind);
                pane.as_picture().vertex_colors = if highlighted {
                    [ORIGINAL_CPU; 4]
                } else if disabled {
                    [[110, 110, 110, 150]; 4]
                } else {
                    [[255; 4]; 4]
                };
            }
        }
        for (role, values, pane, color) in [
            (Role::Player, &player, panes.player[cell], PLAYER),
            (Role::Cpu, &cpu, panes.cpu[cell], CPU),
        ] {
            let stripe = if role == Role::Player {
                panes.all_player[cell]
            } else {
                panes.all_cpu[cell]
            };
            if let Some(stripe) = stripe.as_mut() {
                stripe.set_visible(
                    !manual && supported
                        && values.first() == Some(&1)
                        && kind.is_some_and(|kind| {
                            !app.roster_excluded(role, kind) && !app.hat_cell_disabled(kind)
                        }),
                );
            }
            let badge = if manual {
                if kind.is_some_and(|kind| selected[role as usize].is_some_and(|selected|
                    roster_grid::group_kind(selected) == roster_grid::group_kind(kind))) {
                    if role == Role::Player { "P".into() } else { "C".into() }
                } else { String::new() }
            } else if info.content == CellContent::All {
                if values.first() == Some(&1) {
                    "All".into()
                } else {
                    String::new()
                }
            } else {
                let badges = if role == Role::Player {
                    &player_badges
                } else {
                    &cpu_badges
                };
                kind.filter(|_| supported)
                    .and_then(|kind| roster_grid::catalog_index(&app.roster_kinds, kind))
                    .and_then(|i| badges[i])
                    .map(|n| n.to_string())
                    .unwrap_or_default()
            };
            label(pane, &badge, color);
        }
    }
    let hats = app.roster_purpose == RosterPurpose::KirbyHats;
    label(
        panes.solo_player,
        if !hats && !manual && app.roster_solo_enabled(Role::Player) {
            "S"
        } else {
            ""
        },
        PLAYER,
    );
    label(
        panes.solo_cpu,
        if !hats && !manual && app.roster_solo_enabled(Role::Cpu) {
            "S"
        } else {
            ""
        },
        CPU,
    );
    let info = app.roster_grid.cursor_info();
    let editing = app.roster_grid.role();
    let (role, color) = if editing == Role::Player {
        ("Player", PLAYER)
    } else {
        ("CPU", CPU)
    };
    if let Some(cursor) = panes.cursor.as_mut() {
        cursor.pos_x = (info.column as f32 - 6.0) * 132.0;
        cursor.pos_y = 345.0 - info.row as f32 * 105.0;
        cursor.flags |= 1 << PaneFlag::IsGlobalMatrixDirty as u8;
        let picture = cursor.as_picture();
        picture.vertex_colors = [[255; 4]; 4];
        if let Some(material) = picture.material.as_mut() {
            let [r, g, b, _] = color.map(f32::from);
            // Keep same-role order badges legible over the active cell.
            material.set_white_color(r, g, b, 80.0);
            material.set_black_color(r, g, b, 80.0);
        }
    }
    let title = if manual {
        "Change Character"
    } else if hats {
        "Kirby Hats"
    } else {
        "Character Cycles"
    };
    label(
        panes.title,
        &if manual { format!("{title}  -  {role}") } else { format!("{title}  -  Editing {role}") },
        color,
    );
    let suffix = if info.kind().is_some_and(|kind| app.hat_cell_disabled(kind)
        || (manual && app.roster_excluded(editing, kind)))
        || (hats && info.content == CellContent::Unsupported)
    {
        " - unavailable"
    } else if info.content == CellContent::Unsupported {
        " - unavailable for swapping"
    } else {
        ""
    };
    label(
        panes.name,
        &if manual { format!("{}{}", info.label, suffix) } else { format!(
            "{}{}   |   Player: {}   CPU: {}",
            info.label,
            suffix,
            cycle_summary(&player),
            cycle_summary(&cpu)
        ) },
        [255; 4],
    );
    label(panes.role, "", color);
    label(
        panes.help,
        if manual && editing == Role::Player && !app.player_swapping_enabled() {
            "Enable Player Swapping in Characters first   B Back"
        } else if manual && gcc {
            "A Apply now   X Form   R Player/CPU   B Back   Mii identity: Character Extras"
        } else if manual {
            "A Apply now   X Form   ZR Player/CPU   B Back   Mii identity: Character Extras"
        } else if hats && gcc {
            "A Add   Z Remove   X Form   R Player/CPU   L All   B Back"
        } else if hats {
            "A Add   R Remove   X Form   ZR Player/CPU   ZL All   B Back"
        } else if gcc {
            "A Add   Z Remove   X Form   Y Solo Popo   R Player/CPU   L All   B Back"
        } else {
            "A Add   R Remove   X Form   Y Solo Popo   ZR Player/CPU   ZL All   B Back"
        },
        [235, 239, 245, 255],
    );
}

/// Short input acknowledgement on the existing portrait, without cached panes or
/// rebuilding the grid on idle frames. Native completion uses its own notice.
pub unsafe fn draw_manual_flash(root: &Pane, app: &mut App<'_>) {
    let Some((_, kind, remaining)) = app.manual_selection_flash() else { return; };
    if app.page != training_mod_tui::AppPage::ROSTER { return; }
    if let Some(pane) = root.find_pane_by_name_recursive(&format!("roster_icon_{kind:02X}")) {
        let elapsed = roster_grid::MANUAL_FLASH_FRAMES - remaining;
        let tint = |phase: u8| {
            let strength = if remaining == 1 { 0 } else { 6 - ((elapsed + phase) % 12).abs_diff(6) };
            [255, 255 - strength * 5, 255 - strength * 24, 255]
        };
        pane.as_picture().vertex_colors = [tint(0), tint(3), tint(3), tint(6)];
    }
    app.advance_manual_selection();
}
