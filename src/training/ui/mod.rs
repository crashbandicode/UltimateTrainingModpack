#![allow(clippy::mut_from_ref)]
use sarc::SarcFile;
use skyline::nn::ui2d::*;
use smash::ui2d::{SmashPane, SmashTextBox};
use training_mod_consts::{OnOff, MENU};

use crate::common::menu::QUICK_MENU_ACTIVE;
use crate::common::offsets::OFFSET_LAYOUT_ARC_MALLOC;
use crate::common::{is_native_training_mode, is_ready_go, is_training_mode};
#[cfg(feature = "layout_arc_from_file")]
use crate::consts::LAYOUT_ARC_PATH;
use crate::training::{frame_counter, lobby};
use training_mod_sync::*;

mod anchors;
mod damage;
mod clip_editor;
mod display;
mod input_log;
mod roster_grid;
mod layout_registry;
pub mod menu;
pub(crate) mod native_menu;
pub mod notifications;
mod waiting_archive;

// Revision 33: the two embedded archives stay DEFLATE-compressed in the image (about 195 KB together)
// and are inflated per use. The game's hook point allocates a buffer of the size we report and copies
// from the pointer we report, so an inflated archive is only read during that copy. It is parked in
// `ARCHIVES` and released by `release_stale_archives` (heartbeat thread) after a grace period, after
// being overwritten so that any later reader fails loudly instead of silently depending on it. Before
// this revision the inflated Training archive (4,213,268 B) stayed resident for the whole session and
// the waiting-room archive added a second copy.
static TRAINING_ARCHIVE_DEFLATED: &[u8] =
    include_flate::codegen::deflate_file!("src/static/layout.arc");
static WAITING_UI_PATCH_DEFLATED: &[u8] =
    include_flate::codegen::deflate_file!("src/static/waiting_ui.patch");

struct InflatedArchive {
    label: &'static str,
    handed_out_ms: u64,
    bytes: Vec<u8>,
}

static ARCHIVES: std::sync::Mutex<Vec<InflatedArchive>> = std::sync::Mutex::new(Vec::new());
const ARCHIVE_GRACE_MS: u64 = 5_000;

fn inflate_training_archive() -> Vec<u8> {
    include_flate::decode(TRAINING_ARCHIVE_DEFLATED)
}

/// Park an inflated archive for the game to copy from; the pointer stays valid until the release.
fn hand_out(label: &'static str, bytes: Vec<u8>) -> (*const u8, usize) {
    let (pointer, len) = (bytes.as_ptr(), bytes.len());
    crate::logging::diagnostic(format_args!("[LayoutArchive] handed out {label} bytes={len}"));
    ARCHIVES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(InflatedArchive {
            label,
            handed_out_ms: crate::hang_trace::now_ms(),
            bytes,
        });
    (pointer, len)
}

/// Release archives handed out more than the grace period ago. Runs on the heartbeat thread and
/// never blocks a game thread: it only tries the lock, and the hook holds it for one push.
pub fn release_stale_archives(now_ms: u64) {
    let Ok(mut archives) = ARCHIVES.try_lock() else {
        return;
    };
    archives.retain_mut(|archive| {
        let age = now_ms.saturating_sub(archive.handed_out_ms);
        if age < ARCHIVE_GRACE_MS {
            return true;
        }
        archive.bytes.fill(0xA5);
        crate::logging::diagnostic(format_args!(
            "[LayoutArchive] released {} bytes={} after {}ms",
            archive.label,
            archive.bytes.len(),
            age
        ));
        false
    });
}

pub unsafe fn set_icon_text(pane: &mut TextBox, icons: &[u16]) {
    pane.set_text_string("");

    let it = pane.text_buf as *mut u16;
    pane.text_len = icons.len() as u16;
    for (idx, icon) in icons.iter().enumerate() {
        *(it.add(idx)) = *icon;
    }

    // Add nullptr at end to be sure
    *(it.add(icons.len())) = 0x0;
}

pub fn fade_out(pane: &mut Pane, current_frame: u32, total_frames: u32) {
    if current_frame >= total_frames {
        pane.alpha = 255;
        pane.global_alpha = 255;
    } else if current_frame > 0 {
        // Logarithmic fade out
        let alpha = ((255.0 / (total_frames as f32 + 1.0).log10())
            * (current_frame as f32 + 1.0).log10()) as u8;
        pane.alpha = alpha;
        pane.global_alpha = alpha;

        // Linear fade out
        // let alpha = ((current_frame as f32 / 100.0) * 255.0) as u8;
        // pane.alpha = alpha;
        // pane.global_alpha = alpha;
    } else {
        pane.alpha = 0;
        pane.global_alpha = 0;
    }
}

/// The modpack's UI work for one layout, run by `anchors` immediately before Smash calculates
/// that layout. Each layout is calculated once per frame in the UI manager's pass and once when
/// it is registered. Revision 29 has no `Layout::draw` hook: this is the former `handle_draw`
/// body at the same position (after the layout's animate, before its calculate) and with the
/// same cadence, minus the call to the original.
pub unsafe fn tick(layout: *mut Layout) {
    lobby::update();
    crate::training::named_macros::service_keyboard();
    let layout_name = skyline::from_c_str((*layout).layout_name);
    let root_pane = &mut *(*layout).root_pane;
    if layout_name == "info_training" || !is_native_training_mode() {
        crate::replay_capture::service_reload();
    }
    let waiting_host = layout_name == "info_waiting" && lobby::requested();
    if waiting_host {
        lobby::set_ui_ready(
            root_pane
                .find_pane_by_name_recursive("TrModPort")
                .is_some_and(|host| {
                    ["TrModMenu", "TrModInputLog", "TrModDisp0", "TrModSlider"]
                        .iter()
                        .all(|name| host.find_pane_by_name_recursive(name).is_some())
                }),
        );
        lobby::update();
    }
    if layout_name == "info_waiting" && !lobby::active() {
        if let Some(host) = root_pane.find_pane_by_name_recursive("TrModPort") {
            host.set_visible(false);
        }
    } else if waiting_host {
        if let Some(host) = root_pane.find_pane_by_name_recursive("TrModPort") {
            host.set_visible(true);
        }
    }

    // Set HUD to invisible if HUD is toggled off
    if (is_training_mode()
        || (is_native_training_mode() && crate::replay_capture::loading()))
        && is_ready_go()
        && [
            "info_playercursor",
            "info_playercursor_item",
            "info_melee",
            "info_radar_a",
            "info_radar_b",
        ]
        .contains(&layout_name.as_str())
    {
        // InfluencedAlpha means "Should my children panes' alpha be influenced by mine, as the parent?"
        root_pane.flags |= 1 << PaneFlag::InfluencedAlpha as u8;
        root_pane.set_visible(read(&MENU).hud == OnOff::ON
            && !read(&QUICK_MENU_ACTIVE) && !crate::replay_capture::panel_visible()
            && !crate::replay_capture::loading());
    }

    damage::draw(root_pane, &layout_name);

    if (layout_name == "info_training" && is_native_training_mode())
        || (waiting_host && lobby::active())
    {
        let custom_root: &Pane = if waiting_host {
            root_pane.find_pane_by_name_recursive_expect("TrModPort")
        } else {
            &*root_pane
        };
        frame_counter::tick_real();
        input_log::draw(custom_root);
        display::draw(custom_root);
        menu::draw(custom_root, root_pane);
    }

    // Formerly right after info_training's calculate. Only that game calculate separated the
    // two points, and it reads no modpack state, so the held range-edit repeat keeps its cadence.
    if layout_name == "info_training" && is_native_training_mode() {
        crate::replay_capture::after_draw();
    }
}

// Allocate a static amount of memory that Smash isn't allowed to deallocate,
// in order for us to be able to swap the 'layout.arc' with the current
// version of the file in between loads of training mode.
#[cfg(feature = "layout_arc_from_file")]
const LAYOUT_ARC_SIZE: usize = 5 * 1024 * 1024;
#[cfg(feature = "layout_arc_from_file")]
static mut LAYOUT_ARC: &mut [u8; LAYOUT_ARC_SIZE] = &mut [0u8; LAYOUT_ARC_SIZE];

/// We are editing the info_training/layout.arc and replacing the original file with our
/// modified version from `LAYOUT_ARC_PATH`
///
/// When we edit the layout we are doing two things.
///
/// 1. Creating a new BFLYT inside the layout.arc for whatever component we are making. For example,
/// the slider menu button.
///
/// 2. Adding a Parts pane to the info_training.bflyt with the "Part Name" matching the name of
/// our new BFLYT without the file extension (mimicking how it's done with native Parts panes)
///
/// # Warnings
/// When creating a BFLYT from an existing one we need to edit the names of the panes so that
/// the default animations no longer modify them.  Otherwise the game will override properties,
/// i.e. material colours, and we won't be able to control them properly.
///
/// Once we have the file edited and saved to the correct location we can access the pane
/// from the layout as we normally would in `tick`
/// `(root_pane.find_pane_by_name_recursive("name_of_parts_pane")`.
///
/// # Usage
/// Now say I want to edit background colour of the button's label.
/// I would have to grab the parts pane and find the pane I want to modify on it, then I'd be able
/// to make the modifications as I normally would.
///
/// ```rust
/// let slider_button = root_pane.find_pane_by_name_recursive("name_of_parts_pane");
/// let label_bg = slider_button.find_pane_by_name_recursive("name_of_picture_pane");
///
/// let label_material = &mut *label_bg.as_picture().material;
///
/// label_material.set_white_res_color(LABEL_WHITE_SELECTED_COLOR);
/// label_material.set_black_res_color(LABEL_BLACK_SELECTED_COLOR);
/// ```
#[skyline::hook(offset = *OFFSET_LAYOUT_ARC_MALLOC, inline)]
unsafe fn handle_layout_arc_malloc(ctx: &mut skyline::hooks::InlineCtx) {
    let _probe = crate::hang_trace::LAYOUT_ARC.enter(ctx.registers[1].x());
    if !is_native_training_mode() && !lobby::requested() {
        return;
    }

    let decompressed_file = ctx.registers[21].x() as *const u8;
    let decompressed_size = ctx.registers[1].x() as usize;

    if decompressed_file.is_null() || !(4..=16 * 1024 * 1024).contains(&decompressed_size) {
        return;
    }

    if lobby::requested() {
        let bytes = std::slice::from_raw_parts(decompressed_file, decompressed_size);
        if bytes.starts_with(b"SARC")
            && memchr::memmem::find(bytes, b"blyt/info_waiting.bflyt\0").is_some()
        {
            // Also covers reconstruction without a change to the mode tuple.
            lobby::set_ui_ready(false);
            lobby::update();
            if let Some((pointer, len)) = waiting_archive(bytes) {
                ctx.registers[21].set_x(pointer as u64);
                for register in [1, 23, 24] {
                    ctx.registers[register].set_x(len as u64);
                }
                crate::logging::diagnostic(format_args!(
                    "[LobbyPractice] injected embedded waiting UI bytes={len}"
                ));
            }
        }
        return;
    }

    let layout_arc = SarcFile::read(std::slice::from_raw_parts(
        decompressed_file,
        decompressed_size,
    ))
    .expect("Could not read layout.arc SarcFile!");
    let training_layout = layout_arc.files.iter().find(|f| {
        f.name.is_some() && f.name.as_ref().unwrap() == &String::from("blyt/info_training.bflyt")
    });
    if training_layout.is_none() {
        return;
    }
    clip_editor::discard();

    let inject_arc;
    let inject_arc_size: u64;

    #[cfg(feature = "layout_arc_from_file")]
    #[allow(static_mut_refs)]
    {
        let inject_arc_from_file =
            std::fs::read(LAYOUT_ARC_PATH).expect("Could not read layout.arc from file!");
        inject_arc_size = inject_arc_from_file.len() as u64;

        // Copy read file to global
        inject_arc_from_file
            .iter()
            .enumerate()
            .for_each(|(idx, byte)| LAYOUT_ARC[idx] = *byte);
        inject_arc = LAYOUT_ARC.as_ptr();
    }

    #[cfg(not(feature = "layout_arc_from_file"))]
    {
        let (pointer, len) = hand_out("info_training", inflate_training_archive());
        inject_arc = pointer;
        inject_arc_size = len as u64;
    }

    // Decompressed file pointer
    ctx.registers[21].set_x(inject_arc as u64);

    // Decompressed size is in each of these registers
    ctx.registers[1].set_x(inject_arc_size);
    ctx.registers[23].set_x(inject_arc_size);
    ctx.registers[24].set_x(inject_arc_size);
}

fn waiting_archive(stock: &[u8]) -> Option<(*const u8, usize)> {
    // Built per waiting-room load from version-checked input: the Training archive and the patch
    // are inflated only for the build and dropped here; the result is parked until the game has
    // copied it (see `hand_out`). Never cache a pointer into the game's input.
    let training = inflate_training_archive();
    let patch = include_flate::decode(WAITING_UI_PATCH_DEFLATED);
    match waiting_archive::build(stock, &training, &patch) {
        Ok(bytes) => {
            crate::logging::diagnostic(format_args!(
                "[LobbyPractice] embedded waiting UI ready bytes={}",
                bytes.len()
            ));
            Some(hand_out("info_waiting", bytes))
        }
        Err(error) => {
            crate::logging::diagnostic(format_args!(
                "[LobbyPractice] embedded waiting UI disabled: {error}; stock_bytes={}",
                stock.len()
            ));
            None
        }
    }
}

pub trait PaneExt {
    unsafe fn find_pane_by_name_recursive_expect(&self, name: &str) -> &mut Pane;
}

impl PaneExt for Pane {
    unsafe fn find_pane_by_name_recursive_expect(&self, name: &str) -> &mut Pane {
        self.find_pane_by_name_recursive(name)
            .unwrap_or_else(|| panic!("Could not find pane {}", name))
    }
}

pub fn init() {
    native_menu::init();
    // Revision 29: verify and install the UI manager anchors (panics on any site mismatch).
    anchors::install();
    skyline::install_hooks!(handle_layout_arc_malloc);
}
