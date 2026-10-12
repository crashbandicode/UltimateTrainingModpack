//! Compact native crop timeline, borrowing only the embedded menu resources.
//!
//! Call `draw` every foreground visual frame (not only after menu input), and
//! `restore` before ordinary menu drawing, including frames without new input.
//! The adapter owns input, capture selection, native playback, naming and its clock.
use std::sync::Mutex;

use skyline::nn::ui2d::{Material, MaterialFlags, Pane, PaneFlag, TextBoxFlag};
use smash::ui2d::{SmashPane, SmashTextBox};
use training_mod_sync::read;
use training_mod_tui::{
    clip_panel::{ClipView, Handle},
    NX_SUBMENU_COLUMNS, NX_SUBMENU_ROWS,
};

use crate::common::menu::P1_CONTROLLER_STYLE;
use crate::input::ControllerStyle;

// The embedded Training layout is 1920 x 1080, with its origin at screen center.
// This strip occupies y=-270..-540, leaving the stage above it unobstructed.
const LEFT: f32 = -820.0;
const WIDTH: f32 = 1640.0;
const TRACK_Y: f32 = -371.0;
const BACKDROP: [u8; 4] = [8, 14, 23, 255];
const LABEL_FONT_HEIGHT: f32 = 44.0;
const LABEL_BOX_HEIGHT: f32 = 45.0;
const WHITE: [u8; 4] = [240, 246, 250, 255];
const GOLD: [u8; 4] = [255, 183, 55, 255];
const CYAN: [u8; 4] = [77, 219, 255, 255];
const MUTED: [u8; 4] = [168, 189, 204, 255];

// Only scalar values and names survive a draw. In particular, no Pane, TextBox,
// Material, text-buffer pointer or linked-list node is retained across scenes.
static SNAPSHOT: Mutex<Option<Snapshot>> = Mutex::new(None);

struct Snapshot {
    root: usize,
    panes: Vec<SavedPane>,
}

struct SavedPane {
    path: Vec<String>,
    geometry: Geometry,
    style: Style,
}

struct Geometry {
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 2],
    size: [f32; 2],
    visible: bool,
    alpha: u8,
    global_alpha: u8,
    base_position: u8,
}

enum Style {
    Pane,
    Picture {
        colors: Option<[[f32; 4]; 2]>,
        vertices: [[u8; 4]; 4],
    },
    Text {
        font: [f32; 2],
        colors: [[u8; 4]; 2],
        material: Option<[[f32; 4]; 2]>,
        bits: u16,
        position: u8,
        text: String,
    },
}

#[derive(Clone, Copy)]
enum Kind {
    Pane,
    Picture,
    Text,
}

unsafe fn resolve<'a>(root: &'a Pane, path: &[String]) -> Option<&'a mut Pane> {
    let (first, rest) = path.split_first()?;
    let mut pane = root.find_pane_by_name_recursive(first)?;
    for name in rest {
        pane = pane.find_pane_by_name_recursive(name)?;
    }
    Some(pane)
}

unsafe fn material_colors(material: &Material) -> [[f32; 4]; 2] {
    let mut result = [[0.0; 4]; 2];
    for (index, flag) in IntoIterator::into_iter([
        MaterialFlags::BlackColorFloat,
        MaterialFlags::WhiteColorFloat,
    ])
    .enumerate()
    {
        if material.m_flag & (1 << flag as u8) != 0 {
            let values = *material.m_colors.p_float_color.add(index);
            for (channel, value) in result[index].iter_mut().enumerate() {
                *value = *values.add(channel);
            }
        } else {
            result[index] = material.m_colors.byte_color[index].map(f32::from);
        }
    }
    result
}

impl Snapshot {
    unsafe fn remember(&mut self, root: &Pane, path: &[String], kind: Kind) {
        if self.panes.iter().any(|saved| saved.path == path) {
            return;
        }
        let Some(pane) = resolve(root, path) else {
            return;
        };
        let geometry = Geometry {
            position: [pane.pos_x, pane.pos_y, pane.pos_z],
            rotation: [pane.rot_x, pane.rot_y, pane.rot_z],
            scale: [pane.scale_x, pane.scale_y],
            size: [pane.size_x, pane.size_y],
            visible: pane.flags & (1 << PaneFlag::Visible as u8) != 0,
            alpha: pane.alpha,
            global_alpha: pane.global_alpha,
            base_position: pane.base_position,
        };
        let style = match kind {
            Kind::Pane => Style::Pane,
            Kind::Picture => {
                let picture = pane.as_picture();
                Style::Picture {
                    colors: picture.material.as_ref().map(|m| material_colors(m)),
                    vertices: picture.vertex_colors,
                }
            }
            Kind::Text => {
                let text = pane.as_textbox();
                let len = usize::from(text.text_len.min(text.text_buf_len));
                let content = if text.text_buf.is_null() {
                    String::new()
                } else if text.is_utf8 {
                    String::from_utf8_lossy(std::slice::from_raw_parts(
                        text.text_buf.cast::<u8>(),
                        len,
                    ))
                    .into_owned()
                } else {
                    String::from_utf16_lossy(std::slice::from_raw_parts(
                        text.text_buf.cast::<u16>(),
                        len,
                    ))
                };
                Style::Text {
                    font: [text.font_size_x, text.font_size_y],
                    colors: text.text_colors,
                    material: text.p_material.as_ref().map(|m| material_colors(m)),
                    bits: text.bits,
                    position: text.text_position,
                    text: content,
                }
            }
        };
        self.panes.push(SavedPane {
            path: path.to_vec(),
            geometry,
            style,
        });
    }

    unsafe fn visible(&mut self, root: &Pane, path: &[String], visible: bool) {
        self.remember(root, path, Kind::Pane);
        if let Some(pane) = resolve(root, path) {
            pane.set_visible(visible);
        }
    }

    unsafe fn place(&mut self, root: &Pane, path: &[String], x: f32, y: f32) {
        self.remember(root, path, Kind::Pane);
        if let Some(pane) = resolve(root, path) {
            let [width, height] = [pane.size_x, pane.size_y];
            geometry(pane, x, y, width, height);
        }
    }

    unsafe fn picture(&mut self, root: &Pane, index: usize, rect: [f32; 4], color: [u8; 4]) {
        let mut path = button_path(index);
        path.extend(["Title".into(), "TitleBg".into()]);
        self.picture_at(root, &path, rect, color);
    }

    unsafe fn picture_at(&mut self, root: &Pane, path: &[String], rect: [f32; 4], color: [u8; 4]) {
        self.remember(root, path, Kind::Picture);
        if let Some(pane) = resolve(root, path) {
            geometry(pane, rect[0], rect[1], rect[2], rect[3]);
            let picture = pane.as_picture();
            picture.vertex_colors = [[255; 4]; 4];
            if let Some(material) = picture.material.as_mut() {
                let [r, g, b, a] = color.map(f32::from);
                // Equal endpoints turn the existing textured label into a solid
                // colored strip, independent of its black/white texture shading.
                material.set_black_color(r, g, b, a);
                material.set_white_color(r, g, b, a);
            }
        }
    }

    unsafe fn label(
        &mut self,
        root: &Pane,
        index: usize,
        rect: [f32; 3],
        text: &str,
        color: [u8; 4],
    ) {
        let mut path = button_path(index);
        path.extend(["Title".into(), "TitleTxt".into()]);
        self.remember(root, &path, Kind::Text);
        // The donor font is intentionally condensed (resource size 20 x 36).
        // Equal X/Y sizes stretch glyphs horizontally and make them look flat.
        // Scale both dimensions from the saved font, never from last draw's size.
        let aspect = self
            .panes
            .iter()
            .find_map(|saved| match &saved.style {
                Style::Text { font, .. } if saved.path == path && font[1] > 0.0 => {
                    Some(font[0] / font[1])
                }
                _ => None,
            })
            .unwrap_or(1.0);
        if let Some(pane) = resolve(root, &path) {
            geometry(pane, rect[0], rect[1], rect[2], LABEL_BOX_HEIGHT);
            let textbox = pane.as_textbox();
            textbox.font_size_x = LABEL_FONT_HEIGHT * aspect;
            textbox.font_size_y = LABEL_FONT_HEIGHT;
            textbox.bits |= (1 << TextBoxFlag::IsPTDirty as u8)
                | (1 << TextBoxFlag::WidthLimitEnabled as u8);
            // Native encoding: horizontal left (1), vertical center (0).
            textbox.text_position = 3;
            textbox.text_shadow_enable(true);
            textbox.text_outline_enable(true);
            if !textbox.p_material.is_null() {
                textbox.set_default_material_colors();
            }
            let [r, g, b, a] = color;
            textbox.set_color(r, g, b, a);
            // Parent status can include long diagnostics. Keep one line bounded
            // and do not let embedded NUL truncate native text unexpectedly.
            let mut chars = text.chars().map(|c| if c.is_control() { ' ' } else { c });
            let mut bounded: String = chars.by_ref().take(108).collect();
            if chars.next().is_some() {
                bounded.push_str("...");
            }
            textbox.set_text_string(&bounded);
        }
    }
}

fn menu_path(name: &str) -> Vec<String> {
    vec!["TrModMenu".into(), name.into()]
}

fn row_path(row: usize) -> Vec<String> {
    vec![
        "TrModMenu".into(),
        "TrModSelectors".into(),
        format!("TrModMenuButtonRow{row}"),
    ]
}

fn button_path(index: usize) -> Vec<String> {
    let mut path = row_path(index / NX_SUBMENU_COLUMNS);
    path.push(format!("Button{}", index % NX_SUBMENU_COLUMNS));
    path
}

unsafe fn geometry(pane: &mut Pane, x: f32, y: f32, width: f32, height: f32) {
    pane.pos_x = x;
    pane.pos_y = y;
    pane.pos_z = 0.0;
    pane.rot_x = 0.0;
    pane.rot_y = 0.0;
    pane.rot_z = 0.0;
    pane.scale_x = 1.0;
    pane.scale_y = 1.0;
    pane.size_x = width;
    pane.size_y = height;
    // Center base position; x/y denotes the center of the rectangle/text box.
    pane.base_position = 0;
    pane.alpha = 255;
    pane.global_alpha = 255;
    // Like the native input-log adapter, invalidate the cached transform after
    // changing geometry. Local fields alone leave the old menu-grid matrices.
    pane.flags |= 1 << PaneFlag::IsGlobalMatrixDirty as u8;
    pane.set_visible(true);
}

/// Draw the full captured interval and exclusive [In, Out) selection. This
/// function never advances cursor, modifies the view or inserts a lead-in.
pub unsafe fn draw(root: &Pane, view: &ClipView, status: &str) {
    let mut guard = SNAPSHOT.lock().unwrap_or_else(|poison| poison.into_inner());
    let identity = root as *const Pane as usize;
    if guard.as_ref().is_some_and(|saved| saved.root != identity) {
        // A different scene owns different panes. Discard values from the old
        // layout without ever resolving or dereferencing its retired pointers.
        *guard = None;
    }
    if root.find_pane_by_name_recursive("TrModMenu").is_none() {
        return;
    }
    if guard.is_none() {
        *guard = Some(Snapshot {
            root: identity,
            panes: Vec::new(),
        });
    }
    let saved = guard.as_mut().expect("snapshot initialized");
    let gcc = read(&P1_CONTROLLER_STYLE) == ControllerStyle::GCController;
    let coarse = if gcc { "Z" } else { "ZL/ZR" };
    let chord = if gcc { "Z" } else { "ZL" };
    saved.place(root, &["TrModMenu".into()], 0.0, 0.0);
    for name in [
        "FooterBackground",
        "FooterTxt",
        "LeftTab",
        "CurrentTab",
        "RightTab",
        "SaveDefaults",
        "ResetDefaults",
        "ClearToggle",
        "TrModTitle",
        "TrModSlider",
    ] {
        saved.visible(root, &menu_path(name), false);
    }
    saved.place(root, &menu_path("TrModSelectors"), 0.0, 0.0);
    for row in 0..NX_SUBMENU_ROWS {
        let row_path = row_path(row);
        saved.visible(root, &row_path, row < 4);
        if row >= 4 {
            continue;
        }
        saved.place(root, &row_path, 0.0, 0.0);
        for col in 0..NX_SUBMENU_COLUMNS {
            let index = row * NX_SUBMENU_COLUMNS + col;
            let path = button_path(index);
            saved.place(root, &path, 0.0, 0.0);
            for name in ["Background", "Value", "Icon"] {
                let mut child = path.clone();
                child.push(name.into());
                saved.visible(root, &child, false);
            }
            let mut title = path;
            title.push("Title".into());
            saved.place(root, &title, 0.0, 0.0);
            // Snapshot each typed pane BEFORE hiding it so its original style
            // and visibility are both retained, even on subsequent draw calls.
            for (name, kind) in [("TitleBg", Kind::Picture), ("TitleTxt", Kind::Text)] {
                let mut child = title.clone();
                child.push(name.into());
                saved.remember(root, &child, kind);
                saved.visible(root, &child, false);
            }
        }
    }

    // Native pane-tree order, not the order of these calls, determines layering.
    // FooterBackground is AFTER TrModSelectors in info_training.bflyt and covered
    // the timeline labels. The first button's TitleBg precedes every used label.
    saved.picture(
        root,
        0,
        [0.0, -405.0, 1920.0, 270.0],
        BACKDROP,
    );

    let total = view.total.max(1);
    let start = view.start.min(total);
    let end = view.end.clamp(start, total);
    let x = |frame: u32| LEFT + WIDTH * frame as f32 / total as f32;
    let in_x = x(start);
    let out_x = x(end);
    saved.picture(root, 1, [0.0, TRACK_Y, WIDTH, 24.0], [35, 48, 59, 255]);
    saved.picture(
        root,
        2,
        [(in_x + out_x) * 0.5, TRACK_Y, (out_x - in_x).max(1.0), 24.0],
        [38, 124, 161, 245],
    );
    // Handles sit on opposite sides of their exact boundary: even a one-frame
    // selection retains two independent grips rather than merging the markers.
    saved.picture(
        root,
        3,
        [in_x - 6.0, TRACK_Y, 12.0, 46.0],
        if view.handle == Handle::In {
            GOLD
        } else {
            CYAN
        },
    );
    saved.picture(
        root,
        4,
        [out_x + 6.0, TRACK_Y, 12.0, 46.0],
        if view.handle == Handle::Out {
            GOLD
        } else {
            CYAN
        },
    );
    // Cursor denotes an actual sample (not the exclusive Out boundary). Its
    // taller white line is rendered after both grips and on its own pane.
    saved.picture(
        root,
        10,
        [x(view.cursor.min(total - 1)), TRACK_Y + 3.0, 3.0, 54.0],
        WHITE,
    );

    let duration = view.end.saturating_sub(view.start) as f64 / 60.0;
    let focus = match view.handle {
        Handle::Playhead => "Playhead",
        Handle::In => "In",
        Handle::Out => "Out",
    };
    saved.label(
        root,
        5,
        [-240.0, -292.5, 1160.0],
        &format!(
            "Crop | Frame {}/{} | {:.2}s | Editing {}",
            view.cursor,
            view.total.saturating_sub(1),
            duration,
            focus
        ),
        WHITE,
    );
    let capture: String = view.source_name.chars().take(18).collect();
    saved.label(
        root,
        6,
        [600.0, -292.5, 480.0],
        &format!("P{} | F{} | {}", view.entry + 1, view.fighter_kind, capture),
        MUTED,
    );
    saved.label(
        root,
        7,
        [-260.0, -329.0, 1120.0],
        &format!(
            "L In {} | R Out {} excl. | {}f | Full 0..{}",
            view.start,
            view.end,
            view.end.saturating_sub(view.start),
            view.total
        ),
        CYAN,
    );
    let clean_label = if view.clean_start {
        "Clean start: valid".into()
    } else {
        format!("Needs {chord}+Y lead-in")
    };
    saved.label(
        root,
        8,
        [620.0, -329.0, 440.0],
        &clean_label,
        if view.clean_start { MUTED } else { GOLD },
    );
    let playback = if view.playing {
        "Stop [Playing]"
    } else {
        "Play selection"
    };
    saved.label(
        root,
        9,
        [-410.0, -414.0, 820.0],
        &format!("X: {playback} | Y: Restart"),
        WHITE,
    );
    saved.label(
        root,
        11,
        [410.0, -414.0, 820.0],
        "Up/Down: prev/next fighter | A: Name/save | B: Close",
        WHITE,
    );
    saved.label(
        root,
        13,
        [0.0, -449.0, WIDTH],
        &format!(
            "L/R: select In/Out | Left/Right: edit boundary | Hold {coarse}: 60-frame edits"
        ),
        MUTED,
    );
    saved.label(
        root,
        14,
        [0.0, -484.0, WIDTH],
        &format!(
            "{chord}+X: capture{} | {chord}+Y: clean-start lead-in{}{}",
            if view.previous_available {
                ""
            } else {
                " (no prev.)"
            },
            if view.quantized {
                " | Quantized"
            } else {
                ""
            },
            if view.truncated {
                " | Truncated"
            } else {
                ""
            }
        ),
        MUTED,
    );
    saved.label(root, 15, [0.0, -519.0, WIDTH], status, GOLD);
}

/// Restore the borrowed panes before an ordinary menu draw. A root mismatch
/// drops the stale snapshot; no attempt is made to write into a retired scene.
pub unsafe fn restore(root: &Pane) {
    let mut guard = SNAPSHOT.lock().unwrap_or_else(|poison| poison.into_inner());
    let Some(saved) = guard.take() else { return };
    if saved.root != root as *const Pane as usize {
        return;
    }
    for entry in saved.panes {
        let Some(pane) = resolve(root, &entry.path) else {
            continue;
        };
        let g = entry.geometry;
        [pane.pos_x, pane.pos_y, pane.pos_z] = g.position;
        [pane.rot_x, pane.rot_y, pane.rot_z] = g.rotation;
        [pane.scale_x, pane.scale_y] = g.scale;
        [pane.size_x, pane.size_y] = g.size;
        pane.alpha = g.alpha;
        pane.global_alpha = g.global_alpha;
        pane.base_position = g.base_position;
        pane.flags |= 1 << PaneFlag::IsGlobalMatrixDirty as u8;
        pane.set_visible(g.visible);
        match entry.style {
            Style::Pane => {}
            Style::Picture { colors, vertices } => {
                let picture = pane.as_picture();
                picture.vertex_colors = vertices;
                if let (Some(colors), Some(material)) = (colors, picture.material.as_mut()) {
                    let [r, g, b, a] = colors[0];
                    material.set_black_color(r, g, b, a);
                    let [r, g, b, a] = colors[1];
                    material.set_white_color(r, g, b, a);
                }
            }
            Style::Text {
                font,
                colors,
                material,
                bits,
                position,
                text,
            } => {
                let textbox = pane.as_textbox();
                textbox.set_text_string(&text);
                [textbox.font_size_x, textbox.font_size_y] = font;
                textbox.text_colors = colors;
                if let (Some(colors), Some(target)) = (material, textbox.p_material.as_mut()) {
                    let [r, g, b, a] = colors[0];
                    target.set_black_color(r, g, b, a);
                    let [r, g, b, a] = colors[1];
                    target.set_white_color(r, g, b, a);
                }
                textbox.text_position = position;
                // Restored font/color/text require native glyph recomputation.
                textbox.bits = bits | (1 << TextBoxFlag::IsPTDirty as u8);
            }
        }
    }
}

/// Layout reconstruction starts a new native generation, even at a reused address.
pub fn discard() {
    *SNAPSHOT.lock().unwrap_or_else(|poison| poison.into_inner()) = None;
}
