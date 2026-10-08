//! Read-only scene/resource observation. Selected exclusively by `lobby-probe`.
//! This build never installs the normal modpack's hooks, patches, UI or services.

use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;
use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use skyline::nn::ro::LookupSymbol;
use skyline::nn::ui2d::{Layout, Pane};
use smash::app::{lua_bind::*, utility, BattleObjectModuleAccessor};
use smash::lib::lua_const::*;
use smash::ui2d::SmashPane;

use crate::common::offsets::{OFFSET_DRAW, OFFSET_LAYOUT_ARC_MALLOC, OFFSET_STALE};

mod subfighters;

// Owned 13.0.5 main, build ID 21450c647b8c5940ed5b68218ea128fd3c5d597d.
// Verify the exported predicate's location AND instructions before reading globals.
const TRAINING_PREDICATE: usize = 0x1671650;
const PREDICATE_CODE: [u8; 32] = [
    0xfd, 0x7b, 0xbf, 0xa9, 0xfd, 0x03, 0x00, 0x91, 0x08, 0xe5, 0x01, 0xf0, 0x08, 0x61, 0x13, 0x91,
    0x08, 0xfd, 0xdf, 0x08, 0x48, 0x01, 0x00, 0x36, 0xa8, 0xe2, 0x01, 0x90, 0x08, 0xc1, 0x46, 0x39,
];
static BASE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Context {
    initialized: bool,
    melee: u8,
    mode: u32,
}

#[derive(Default)]
struct Observations {
    context: Option<Context>,
    epoch: u64,
    layouts: Vec<String>,
    fighters: Vec<u32>,
    pause_seen: bool,
    archives_seen: u8,
    pane_states: Vec<(String, u8)>,
    subfighters: subfighters::Observer,
}

static OBSERVATIONS: Mutex<Observations> = Mutex::new(Observations {
    context: None,
    epoch: 0,
    layouts: Vec::new(),
    fighters: Vec::new(),
    pause_seen: false,
    archives_seen: 0,
    pane_states: Vec::new(),
    subfighters: subfighters::Observer::new(),
});

unsafe fn context() -> Context {
    let base = BASE.load(Ordering::Relaxed);
    Context {
        initialized: std::ptr::read_volatile((base + 0x53144d8) as *const u8) & 1 != 0,
        melee: std::ptr::read_volatile((base + 0x52c51b0) as *const u8),
        mode: std::ptr::read_volatile((base + 0x53050f0) as *const u32),
    }
}

impl Observations {
    fn update(&mut self, current: Context) {
        if self.context == Some(current) {
            return;
        }
        self.epoch += 1;
        self.context = Some(current);
        self.layouts.clear();
        self.fighters.clear();
        self.pause_seen = false;
        self.archives_seen = 0;
        self.pane_states.clear();
        self.subfighters.reset_epoch();
        println!(
            "[LobbyProbe] context epoch={} initialized={} melee=0x{:02x} mode=0x{:08x} training={}",
            self.epoch,
            current.initialized,
            current.melee,
            current.mode,
            current.initialized && matches!(current.melee, 0x0c | 0x20)
        );
    }
}

// Bounded, read-only visibility observations for native dialog arbitration.
unsafe fn observe_panes(
    pane: &Pane,
    layout: &str,
    observations: &mut Observations,
    budget: &mut usize,
    depth: usize,
) {
    if *budget == 0 || depth > 12 {
        return;
    }
    *budget -= 1;
    let key = format!("{layout}/{}@{pane:p}", pane.get_name());
    let state =
        (pane.flags & 1) | (u8::from(pane.alpha > 0) << 1) | (u8::from(pane.global_alpha > 0) << 2);
    let previous = observations
        .pane_states
        .iter_mut()
        .find(|(name, _)| name == &key);
    let changed = if let Some((_, old)) = previous {
        let changed = *old != state;
        *old = state;
        changed
    } else if observations.pane_states.len() < 256 {
        observations.pane_states.push((key.clone(), state));
        true
    } else {
        false
    };
    if changed {
        println!(
            "[LobbyProbe] pane_state {} state={} alpha={} global={} pos=({}, {})",
            key, state, pane.alpha, pane.global_alpha, pane.pos_x, pane.pos_y
        );
    }
    let sentinel = &pane.children_list as *const _ as *mut skyline::nn::ui2d::PaneNode;
    let mut node = pane.children_list.next;
    while !node.is_null() && node != sentinel && *budget > 0 {
        let child = &*((node as *const u8).sub(std::mem::offset_of!(Pane, link)) as *const Pane);
        observe_panes(child, layout, observations, budget, depth + 1);
        node = (*node).next;
    }
}

#[skyline::hook(offset = *OFFSET_DRAW)]
unsafe fn draw(layout: *mut Layout, draw_info: u64, cmd_buffer: u64) {
    if !layout.is_null() && !(*layout).layout_name.is_null() {
        let name = skyline::from_c_str((*layout).layout_name);
        let mut observations = OBSERVATIONS.lock();
        observations.update(context());
        if context().melee == 0x0d
            && (name.contains("message") || name.contains("popup") || name.contains("dialog"))
            && !(*layout).root_pane.is_null()
        {
            observe_panes(&*(*layout).root_pane, &name, &mut observations, &mut 128, 0);
        }
        if observations.layouts.len() < 256 && !observations.layouts.contains(&name) {
            println!(
                "[LobbyProbe] layout epoch={} name={} root={}",
                observations.epoch,
                name,
                !(*layout).root_pane.is_null()
            );
            if matches!(name.as_str(), "info_training" | "info_waiting")
                && !(*layout).root_pane.is_null()
            {
                let root = &*(*layout).root_pane;
                for pane in [
                    "TrModInputLog",
                    "TrModMenu",
                    "TrModDisp0",
                    "L_staying_help",
                    "status_R",
                ] {
                    println!(
                        "[LobbyProbe] pane epoch={} name={} present={}",
                        observations.epoch,
                        pane,
                        root.find_pane_by_name_recursive(pane).is_some()
                    );
                }
            }
            observations.layouts.push(name);
        }
    }
    original!()(layout, draw_info, cmd_buffer);
}

#[skyline::hook(offset = *OFFSET_LAYOUT_ARC_MALLOC, inline)]
unsafe fn layout_archive(ctx: &mut InlineCtx) {
    let pointer = ctx.registers[21].x() as *const u8;
    let size = ctx.registers[1].x() as usize;
    if pointer.is_null() || !(4..=16 * 1024 * 1024).contains(&size) {
        return;
    }
    let bytes = std::slice::from_raw_parts(pointer, size);
    if &bytes[..4] != b"SARC" {
        return;
    }
    let (name, bit) = if memchr::memmem::find(bytes, b"blyt/info_training.bflyt\0").is_some() {
        ("info_training", 1)
    } else if memchr::memmem::find(bytes, b"blyt/info_waiting.bflyt\0").is_some() {
        ("info_waiting", 2)
    } else if memchr::memmem::find(bytes, b"blyt/info_message.bflyt\0").is_some() {
        ("info_message", 4)
    } else {
        return;
    };
    let mut observations = OBSERVATIONS.lock();
    observations.update(context());
    if observations.archives_seen & bit == 0 {
        observations.archives_seen |= bit;
        println!(
            "[LobbyProbe] archive epoch={} name={} size={} replaced=false",
            observations.epoch, name, size
        );
        // Owned-game diagnostic assets stay in this isolated profile, never in source control.
        let directory = "sd:/ultimate/lobby_probe";
        let result = std::fs::create_dir_all(directory)
            .and_then(|()| std::fs::write(format!("{directory}/{name}.arc"), bytes));
        println!(
            "[LobbyProbe] archive_dump name={} success={}",
            name,
            result.is_ok()
        );
    }
    // Registers and the decompressed archive are deliberately left untouched.
}

#[skyline::hook(replace = ControlModule::get_command_flag_cat)]
unsafe fn command(module_accessor: &mut BattleObjectModuleAccessor, category: i32) -> i32 {
    let result = original!()(module_accessor, category);
    if category == FIGHTER_PAD_COMMAND_CATEGORY1
        && utility::get_category(module_accessor) == *BATTLE_OBJECT_CATEGORY_FIGHTER
    {
        if cfg!(feature = "trio-probe") {
            crate::trio_probe::command(module_accessor);
        }
        let mut observations = OBSERVATIONS.lock();
        observations.update(context());
        let id = module_accessor.battle_object_id;
        let current = observations.context.unwrap();
        if current.initialized && current.melee == 0x20 && current.mode == 0x04040000 {
            let epoch = observations.epoch;
            observations.subfighters.observe(module_accessor, epoch);
        }
        if observations.fighters.len() < 32 && !observations.fighters.contains(&id) {
            println!(
                "[LobbyProbe] fighter epoch={} id={} entry={} kind={} status={}",
                observations.epoch,
                id,
                WorkModule::get_int(module_accessor, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID),
                utility::get_kind(module_accessor),
                StatusModule::status_kind(module_accessor)
            );
            observations.fighters.push(id);
        }
    }
    result
}

#[skyline::hook(offset = *OFFSET_STALE, inline)]
unsafe fn training_pause(ctx: &mut InlineCtx) {
    let mut observations = OBSERVATIONS.lock();
    observations.update(context());
    if !observations.pause_seen {
        observations.pause_seen = true;
        println!(
            "[LobbyProbe] pause_callback epoch={} nonnull={}",
            observations.epoch,
            ctx.registers[22].x() != 0
        );
    }
    // No dereference of Training's PauseMenu, and no register writes.
}

unsafe fn observe_dispatch(entry: *const u8, phase: &str, argument: u64) {
    let current = context();
    if current.initialized && current.melee == 0x20 && current.mode == 0x04040000 {
        let mut observations = OBSERVATIONS.lock();
        observations.update(current);
        let epoch = observations.epoch;
        observations
            .subfighters
            .dispatch(entry, epoch, phase, argument);
    }
}

#[skyline::hook(offset = 0x6556b0)]
unsafe fn entry_dispatch(entry: *mut u8, argument: u64) {
    // Owned instructions establish a two-argument void ABI. Never hold the
    // observation lock over native execution, which can reenter fighter hooks.
    observe_dispatch(entry, "before", argument);
    original!()(entry, argument);
    observe_dispatch(entry, "after", argument);
}

pub fn init() {
    unsafe {
        let base = getRegionAddress(Region::Text) as usize;
        let mut predicate = 0usize;
        LookupSymbol(
            &mut predicate,
            b"_ZN3app9smashball16is_training_modeEv\0".as_ptr(),
        );
        if predicate != base + TRAINING_PREDICATE
            || std::slice::from_raw_parts(predicate as *const u8, PREDICATE_CODE.len())
                != PREDICATE_CODE
        {
            println!("[LobbyProbe] Unsupported main: no hooks installed");
            return;
        }
        BASE.store(base, Ordering::Relaxed);
        subfighters::init();
        println!("[LobbyProbe] Read-only observation build for 13.0.5; normal modpack disabled");
        skyline::install_hooks!(draw, layout_archive, command, training_pause);
        if subfighters::dispatcher_supported(base) {
            skyline::install_hooks!(entry_dispatch);
        } else {
            println!("[SubfighterProbe] dispatcher fingerprint mismatch: hook omitted");
        }
    }
}
