use std::collections::VecDeque;
use std::sync::Mutex;

use skyline::nn::ui2d::*;
use smash::ui2d::SmashTextBox;
use training_mod_consts::{InputDisplay, MENU};

use crate::common::consts::status_display_name;
use crate::menu::QUICK_MENU_ACTIVE;
use crate::training::input_log::{
    DirectionStrength, InputLog, DRAW_LOG_BASE_IDX, NUM_LOGS, P1_INPUT_LOGS, WHITE, YELLOW,
};
use crate::training::ui::menu::VANILLA_MENU_ACTIVE;
use crate::training::ui::{fade_out, PaneExt};
use training_mod_sync::*;

const ROW_NAMES: [&str; NUM_LOGS] = [
    "TrModInputLog0",
    "TrModInputLog1",
    "TrModInputLog2",
    "TrModInputLog3",
    "TrModInputLog4",
    "TrModInputLog5",
    "TrModInputLog6",
    "TrModInputLog7",
    "TrModInputLog8",
    "TrModInputLog9",
    "TrModInputLog10",
    "TrModInputLog11",
    "TrModInputLog12",
    "TrModInputLog13",
    "TrModInputLog14",
];
const SLOT_NAMES: [&str; 5] = ["Input0", "Input1", "Input2", "Input3", "Input4"];
const AVAILABLE_ICONS: [&str; 37] = [
    "a",
    "b",
    "x",
    "y",
    "lb",
    "rb",
    "zl",
    "zr",
    "up",
    "down",
    "left",
    "right",
    "up_left",
    "up_right",
    "down_left",
    "down_right",
    "gcc_l",
    "gcc_r",
    "gcc_z",
    "plus",
    "minus",
    "l_stick",
    "r_stick",
    "gcc_c_stick",
    "dpad_up",
    "dpad_down",
    "dpad_left",
    "dpad_right",
    "dpad_left_right",
    "up_strong",
    "down_strong",
    "left_strong",
    "right_strong",
    "up_left_strong",
    "down_left_strong",
    "up_right_strong",
    "down_right_strong",
];

type Icon = (&'static str, ResColor);

#[derive(Clone, Copy)]
struct Head {
    log: InputLog,
    mode: InputDisplay,
    status: bool,
    icons: [Option<Icon>; 5],
}

#[derive(Clone, Copy, Default)]
struct Row {
    position: Option<usize>,
    ttl: u32,
    head: Option<Head>,
}

// Cache only values we wrote, never game-owned pane/material/text pointers. Registration
// invalidates even if the allocator reuses the same root address after a scene reload.
struct RenderCache {
    root: usize,
    rows: [Row; NUM_LOGS],
    refresh_all: bool,
}
static CACHE: Mutex<Option<RenderCache>> = Mutex::new(None);

#[derive(Clone, Copy)]
struct DrawSettings {
    mode: InputDisplay,
    status: bool,
    refresh_all: bool,
}

pub fn invalidate() {
    *CACHE.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

fn get_input_icons(log: &InputLog) -> VecDeque<Icon> {
    let mut icons = log.button_icons();

    let (rstick_strength, rstick_angle) = log.binned_rstick();
    let rstick_icon = if rstick_strength != DirectionStrength::None {
        match rstick_angle as u32 {
            0 => "right",
            45 => "up_right",
            90 => "up",
            135 => "up_left",
            180 => "left",
            225 => "down_left",
            270 => "down",
            315 => "down_right",
            _ => "?",
        }
    } else {
        ""
    };

    if !rstick_icon.is_empty() {
        icons.push_front((rstick_icon, YELLOW));
    }

    let (lstick_strength, lstick_angle) = log.binned_lstick();
    let lstick_icon = if lstick_strength != DirectionStrength::None {
        match lstick_angle as u32 {
            0 => "right",
            45 => "up_right",
            90 => "up",
            135 => "up_left",
            180 => "left",
            225 => "down_left",
            270 => "down",
            315 => "down_right",
            _ => "?",
        }
    } else {
        ""
    };

    if !lstick_icon.is_empty() {
        icons.push_front((lstick_icon, WHITE));
    }

    icons
}

unsafe fn draw_log(
    logs_pane: &Pane,
    log_idx: usize,
    draw_log_idx: usize,
    log: &InputLog,
    settings: DrawSettings,
    row: &mut Row,
) {
    let DrawSettings {
        mode,
        status,
        refresh_all,
    } = settings;
    const FADE_FRAMES: u32 = 200;
    let position_changed = row.position != Some(log_idx);
    let fade_changed =
        row.position.is_none() || row.ttl.min(FADE_FRAMES) != log.ttl.min(FADE_FRAMES);
    let previous = row.head;
    let update_content = log_idx == 0 || (refresh_all && log.ttl > 0);
    let icons_changed = update_content
        && previous.is_none_or(|head| head.mode != mode || head.log.is_different(log));
    let frames_changed =
        update_content && previous.is_none_or(|head| head.log.frames != log.frames);
    let status_changed = update_content
        && previous.is_none_or(|head| {
            head.status != status
                || (status
                    && (head.log.status != log.status || head.log.fighter_kind != log.fighter_kind))
        });
    if !position_changed && !fade_changed && !icons_changed && !frames_changed && !status_changed {
        return;
    }
    let log_pane = logs_pane.find_pane_by_name_recursive_expect(ROW_NAMES[draw_log_idx]);

    // Handle visibility and alpha
    if row.position.is_none() {
        log_pane.set_visible(true);
    }
    if fade_changed {
        fade_out(log_pane, log.ttl, FADE_FRAMES);
    }
    row.ttl = log.ttl;

    // Handle positioning
    let new_pos_y = -52.5 * log_idx as f32;
    if new_pos_y != log_pane.pos_y {
        log_pane.pos_y = new_pos_y;
        log_pane.flags |= 1 << PaneFlag::IsGlobalMatrixDirty as u8;
    }
    row.position = Some(log_idx);

    // Historical rows stay unchanged unless hidden frames or a new layout require a refresh.
    if !update_content {
        return;
    }

    let mut icons = previous.map_or([None; 5], |head| head.icons);
    if icons_changed {
        let mut next = [None; 5];
        for (index, icon) in get_input_icons(log).into_iter().take(5).enumerate() {
            if AVAILABLE_ICONS.contains(&icon.0) {
                next[index] = Some(icon);
            }
        }
        for (index, slot_name) in SLOT_NAMES.iter().enumerate() {
            if previous.is_some() && same_icon(icons[index], next[index]) {
                continue;
            }
            let slot = log_pane.find_pane_by_name_recursive_expect(slot_name);
            if previous.is_none() {
                // Layout resources can start with any icon visible. Clear once per row.
                for name in AVAILABLE_ICONS {
                    slot.find_pane_by_name_recursive_expect(name)
                        .set_visible(false);
                }
            } else if let Some((name, _)) = icons[index] {
                slot.find_pane_by_name_recursive_expect(name)
                    .set_visible(false);
            }
            if let Some((name, color)) = next[index] {
                let pane = slot.find_pane_by_name_recursive_expect(name).as_picture();
                pane.set_visible(true);
                (*pane.material).set_black_res_color(color);
            }
        }
        icons = next;
    }
    if frames_changed {
        log_pane
            .find_pane_by_name_recursive_expect("Frame")
            .as_textbox()
            .set_text_string(&log.frames.to_string());
    }
    if status_changed {
        let text = if status {
            status_display_name(log.fighter_kind, log.status)
        } else {
            String::new()
        };
        log_pane
            .find_pane_by_name_recursive_expect("Status")
            .as_textbox()
            .set_text_string(&text);
    }
    row.head = Some(Head {
        log: *log,
        mode,
        status,
        icons,
    });
}

fn same_icon(left: Option<Icon>, right: Option<Icon>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some((a, x)), Some((b, y))) => a == b && [x.r, x.g, x.b, x.a] == [y.r, y.g, y.b, y.a],
        _ => false,
    }
}

pub unsafe fn draw(root_pane: &Pane) {
    let logs_pane = root_pane.find_pane_by_name_recursive_expect("TrModInputLog");
    let (mode, status) = {
        let menu = lock_read(&MENU);
        (menu.input_display, menu.input_display_status.as_bool())
    };
    let visible =
        !read(&QUICK_MENU_ACTIVE) && !read(&VANILLA_MENU_ACTIVE) && mode != InputDisplay::NONE;
    logs_pane.set_visible(visible);
    if !visible {
        if let Some(cache) = CACHE.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            cache.refresh_all = true;
        }
        return;
    }

    let logs = read(&(*P1_INPUT_LOGS));
    let base = read(&DRAW_LOG_BASE_IDX);
    let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    let root = root_pane as *const Pane as usize;
    if cache.as_ref().is_none_or(|cache| cache.root != root) {
        *cache = Some(RenderCache {
            root,
            rows: [Row::default(); NUM_LOGS],
            refresh_all: true,
        });
    }
    let cache = cache.as_mut().unwrap();
    let settings = DrawSettings {
        mode,
        status,
        refresh_all: cache.refresh_all,
    };

    for (log_idx, log) in logs.iter().enumerate() {
        let draw_log_idx = (log_idx + (NUM_LOGS - base)) % NUM_LOGS;
        draw_log(
            logs_pane,
            log_idx,
            draw_log_idx,
            log,
            settings,
            &mut cache.rows[draw_log_idx],
        );
    }
    cache.refresh_all = false;
}
