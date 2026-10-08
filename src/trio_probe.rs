//! Configurable practice roster and guarded native switch requests.
//! Parent-selected owned-13.0.5 native pipeline; all mutation hooks fail closed.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use skyline::nn::ro::LookupSymbol;
use smash::app::{self, lua_bind::*, utility, BattleObjectModuleAccessor};
use smash::lib::lua_const::*;

use crate::common::game_mode;
use crate::common::input::{Buttons, MappedInputs};
use training_mod_consts::{OnOff, TrioFighter, MENU};
use training_mod_sync::read as read_setting;

macro_rules! trace {
    ($($arg:tt)*) => { crate::logging::diagnostic(format_args!($($arg)*)) };
}

// Menu choices belong to the next construction. Never retag loaded bodies or
// their controller tables when the user edits the menu in an existing scene.
#[derive(Clone, Copy, Debug)]
struct EntryRoster {
    entry: usize,
    kinds: [u32; 3],
    pending: Option<u8>,
}

static LOADED_ROSTER: Mutex<Option<EntryRoster>> = Mutex::new(None);

fn configured_roster() -> Option<[u32; 3]> {
    let menu = read_setting(&MENU);
    let kinds = [
        menu.trio_fighter_1.kind(),
        menu.trio_fighter_2.kind(),
        menu.trio_fighter_3.kind(),
    ];
    (kinds.iter().all(|kind| TrioFighter::supported_kind(*kind))
        && kinds[0] != kinds[1]
        && kinds[0] != kinds[2]
        && kinds[1] != kinds[2])
        .then_some(kinds)
}

pub fn settings_changed() {
    let menu = read_setting(&MENU);
    crate::logging::diagnostic(format_args!(
        "[TrioPractice] settings enabled={:?} requested={:x?} loaded={:?}; roster edits apply on fresh practice entry",
        menu.trio_practice,
        configured_roster(),
        *LOADED_ROSTER.lock().unwrap()
    ));
    if menu.trio_practice == OnOff::ON && configured_roster().is_none() {
        crate::training::ui::notifications::notification(
            "Trio Practice".to_string(),
            "Choose three different characters".to_string(),
            240,
        );
    }
}
static BASE: AtomicUsize = AtomicUsize::new(0);
static DESCRIPTOR_LOGS: AtomicUsize = AtomicUsize::new(0);
static FLAGS_LOGS: AtomicUsize = AtomicUsize::new(0);
static RESOURCE_LOGS: AtomicUsize = AtomicUsize::new(0);
static CALL_LOGS: AtomicUsize = AtomicUsize::new(0);
static PREPARE_LOGS: AtomicUsize = AtomicUsize::new(0);
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Scope {
    #[default]
    None,
    Training,
    Waiting,
}

#[derive(Clone, Copy, Debug, Default)]
struct Construction {
    scope: Scope,
    epoch: usize,
    resource: usize,
    queued: bool,
    kinds: [u32; 3],
}

static CONSTRUCTION: Mutex<Construction> = Mutex::new(Construction {
    scope: Scope::None,
    epoch: 0,
    resource: 0,
    queued: false,
    kinds: [0; 3],
});
static CONSTRUCTION_EPOCH: AtomicUsize = AtomicUsize::new(0);
static MANAGER_SYMBOL: AtomicUsize = AtomicUsize::new(0);
static SWITCH_LOGS: AtomicUsize = AtomicUsize::new(0);
static COMPLETION_LOGS: AtomicUsize = AtomicUsize::new(0);
static BOUNDARY_LOGS: AtomicUsize = AtomicUsize::new(0);
static GENERATION: AtomicUsize = AtomicUsize::new(0);
static HOTKEY_OWNS_TAUNT: AtomicBool = AtomicBool::new(false);
static HUD_REFRESH_LOGS: AtomicUsize = AtomicUsize::new(0);

// Extracted from owned ELF, not inferred from decompiled C. Inline fingerprints
// start at the preceding csel, then include the hook instruction and its neighbors.
const FINGERPRINTS: [(usize, [u32; 8]); 16] = [
    (
        0x1510d30,
        [
            0xf94007e8, 0xb9400d00, 0x51009008, 0x7100091f, 0x54000c48, 0xf94006a8, 0xf9400108,
            0xb94d6909,
        ],
    ),
    (
        0x156073c,
        [
            0xf94007e8, 0xb9400d00, 0x51009008, 0x7100091f, 0x54000c48, 0xf9400688, 0xf9400108,
            0xb94d6909,
        ],
    ),
    (
        0x1790020,
        [
            0xa9ba6ffc, 0xa90167fa, 0xa9025ff8, 0xa90357f6, 0xa9044ff4, 0xa9057bfd, 0x910143fd,
            0xd11783ff,
        ],
    ),
    (
        0x678150,
        [
            0xd10243ff, 0xa9036ffc, 0xa90467fa, 0xa9055ff8, 0xa90657f6, 0xa9074ff4, 0xa9087bfd,
            0x910203fd,
        ],
    ),
    (
        0x653240,
        [
            0xd101c3ff, 0xf90013f9, 0xa9035ff8, 0xa90457f6, 0xa9054ff4, 0xa9067bfd, 0x910183fd,
            0x528b23c8,
        ],
    ),
    (
        0x652160,
        [
            0xd10103ff, 0xa90157f6, 0xa9024ff4, 0xa9037bfd, 0x9100c3fd, 0xf0026388, 0xf9407d15,
            0xaa0003f3,
        ],
    ),
    (
        0x6581f0,
        [
            0xd101c3ff, 0xfd0013e8, 0xf90017f9, 0xa9035ff8, 0xa90457f6, 0xa9054ff4, 0xa9067bfd,
            0x910183fd,
        ],
    ),
    (
        0x17e4f50,
        [
            0xd103c3ff, 0xa9096ffc, 0xa90a67fa, 0xa90b5ff8, 0xa90c57f6, 0xa90d4ff4, 0xa90e7bfd,
            0x910383fd,
        ],
    ),
    (
        0x678aa0,
        [
            0xf9400be1, 0xaa1603e0, 0x97ff6f72, 0xb940a288, 0x11000508, 0xb900a288, 0xa9487bfd,
            0xa9474ff4,
        ],
    ),
    (
        0x654dc0,
        [
            0xd10103ff, 0xf9000bf5, 0xa9024ff4, 0xa9037bfd, 0x9100c3fd, 0xf9407c14, 0xaa0003f3,
            0x91002280,
        ],
    ),
    (
        0x17e5bc0,
        [
            0xd105c3ff, 0xf90093fc, 0xa9135ff8, 0xa91457f6, 0xa9154ff4, 0xa9167bfd, 0x910583fd,
            0xb9405814,
        ],
    ),
    (
        0x17e64d0,
        [
            0xa9ba6ffc, 0xa90167fa, 0xa9025ff8, 0xa90357f6, 0xa9044ff4, 0xa9057bfd, 0x910143fd,
            0xd10dc3ff,
        ],
    ),
    (
        0x655480,
        [
            0x9a970137, 0xb94002f4, 0x3100069f, 0x54000460, 0x321c03e0, 0x321903e1, 0x94cb637e,
            0xaa0003f5,
        ],
    ),
    (
        0x64fbc8,
        [
            0x9a8a0169, 0xb940012a, 0x6b08015f, 0x54000061, 0xaa1f03e8, 0x1400000a, 0xb940052a,
            0x6b08015f,
        ],
    ),
    (
        0x64f9e4,
        [
            0x9a950135, 0xb94002a8, 0x3100051f, 0x54000380, 0xf960c674, 0xb4000354, 0xf0026328,
            0xf947ed08,
        ],
    ),
    (
        0x6516e0,
        [
            0x9a940134, 0xb9400288, 0x3100051f, 0x540000e0, 0xf960c660, 0xb40000a0, 0xf9400008,
            0xf9400508,
        ],
    ),
];

pub fn native_hooks_ready() -> bool {
    BASE.load(Ordering::Acquire) != 0
}

fn enabled() -> bool {
    cfg!(feature = "trio-probe") // Normal builds use one on-demand stock body per entry.
}

fn ordinary_training() -> bool {
    game_mode::current().is_some_and(|m| m.initialized && m.melee == 0x20 && m.mode == 0x04040000)
}

fn practice_scene() -> bool {
    ordinary_training() || crate::training::lobby::is_waiting_scene()
}

fn resource_scope(scope: Scope) -> bool {
    match scope {
        Scope::Training => training_resources(),
        Scope::Waiting => crate::training::lobby::is_waiting_scene(),
        Scope::None => false,
    }
}

// This native constructor establishes the local practice roster before resource
// preparation. Its one-use token excludes later match-preview resource requests,
// which can still run while the departing scene reports melee0d.
#[skyline::hook(offset = 0x1790020, inline)]
unsafe fn waiting_construction(_ctx: &mut InlineCtx) {
    let roster = configured_roster();
    let eligible = roster.is_some()
        && enabled()
        && read_setting(&MENU).lobby_practice == OnOff::ON
        && game_mode::current().is_some_and(|m| {
            m.initialized && (m.mode == 0x02010000 || m.mode & 0x7fff0000 == 0x07010000)
        });
    let construction = Construction {
        scope: if eligible {
            Scope::Waiting
        } else {
            Scope::None
        },
        epoch: CONSTRUCTION_EPOCH.fetch_add(1, Ordering::Relaxed) + 1,
        kinds: roster.unwrap_or_default(),
        ..Construction::default()
    };
    *CONSTRUCTION.lock().unwrap() = construction;
    if bounded_log(&BOUNDARY_LOGS, 384) {
        trace!(
            "[TrioBoundary] practice_intent {:?} mode={:?}",
            construction,
            game_mode::current()
        );
    }
}

fn training_resources() -> bool {
    // Owned runtime: Training CSS has raw04040000 but melee0 until loading starts.
    game_mode::current()
        .is_some_and(|m| m.initialized && matches!(m.melee, 0 | 0x20) && m.mode == 0x04040000)
}

fn bounded_log(counter: &AtomicUsize, limit: usize) -> bool {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            (n < limit).then_some(n + 1)
        })
        .is_ok()
}

fn aligned(pointer: *const u8) -> bool {
    !pointer.is_null() && (pointer as usize).is_multiple_of(8)
}

unsafe fn read<T: Copy>(pointer: *const u8, offset: usize) -> T {
    std::ptr::read_volatile(pointer.add(offset) as *const T)
}

unsafe fn write<T>(pointer: *mut u8, offset: usize, value: T) {
    std::ptr::write(pointer.add(offset) as *mut T, value);
}

unsafe fn trio_entry(entry: *const u8) -> bool {
    let Some(roster) = *LOADED_ROSTER.lock().unwrap() else {
        return false;
    };
    roster.entry == entry as usize
        && aligned(entry)
        && read::<u32>(entry, 0xe0) == 0
        && read::<u32>(entry, 0x14) == 3
        && read::<u32>(entry, 0x28) == 1
        && [
            read::<u32>(entry, 0x18),
            read(entry, 0x1c),
            read(entry, 0x20),
        ] == roster.kinds
}

unsafe fn entry_kinds(entry: *const u8) -> [u32; 3] {
    [read(entry, 0x18), read(entry, 0x1c), read(entry, 0x20)]
}

unsafe fn descriptor(raw: *const u8) -> (u32, [u32; 3], u32, u8) {
    (
        read(raw, 0x14),
        [read(raw, 0x18), read(raw, 0x1c), read(raw, 0x20)],
        read(raw, 0x28),
        read(raw, 0x43),
    )
}

#[skyline::hook(offset = 0x678150, inline)]
unsafe fn allocation_trace(ctx: &mut InlineCtx) {
    let desc = ctx.registers[2].x() as *const u8;
    if aligned(desc) && bounded_log(&BOUNDARY_LOGS, 384) {
        trace!(
            "[TrioBoundary] allocation owner={} caller=main+0x{:x} descriptor={:x?} mode={:?}",
            ctx.registers[1].x(),
            ctx.registers[30]
                .x()
                .saturating_sub(BASE.load(Ordering::Acquire) as u64),
            descriptor(desc),
            game_mode::current()
        );
    }
}

#[skyline::hook(offset = 0x653240, inline)]
unsafe fn reload_trace(ctx: &mut InlineCtx) {
    let entry = ctx.registers[0].x() as *const u8;
    let desc = ctx.registers[1].x() as *const u8;
    if aligned(entry) && aligned(desc) && bounded_log(&BOUNDARY_LOGS, 384) {
        trace!("[TrioBoundary] reload entry=0x{:x} owner={} caller=main+0x{:x} guard={} incoming={:x?} active={:x?} pending={:x?} loader={} outer={} mode={:?}", entry as usize, read::<u32>(entry, 0xe0), ctx.registers[30].x().saturating_sub(BASE.load(Ordering::Acquire) as u64), read::<u8>(entry, 0x5935), descriptor(desc), descriptor(entry), descriptor(entry.add(0x70)), read::<u8>(entry, 0x591c), read::<u8>(entry, 0x5920), game_mode::current());
    }
}

#[skyline::hook(offset = 0x652160, inline)]
unsafe fn destruction_trace(ctx: &mut InlineCtx) {
    let entry = ctx.registers[0].x() as *const u8;
    crate::character_roster::on_destroy(entry);
    if aligned(entry) && bounded_log(&BOUNDARY_LOGS, 384) {
        trace!("[TrioBoundary] destroy entry=0x{:x} owner={} active={:x?} pending={:x?} installed={} mode={:?}", entry as usize, read::<u32>(entry, 0xe0), descriptor(entry), descriptor(entry.add(0x70)), read::<u64>(entry, 0x4150), game_mode::current());
    }
}

#[skyline::hook(offset = 0x678aac, inline)]
unsafe fn install_descriptor(ctx: &mut InlineCtx) {
    // Fresh allocation has copied the active descriptor and assigned its owner.
    // x22 is callee-saved across654870; x0/x1 are not valid entry provenance here.
    let entry = ctx.registers[22].x() as *mut u8;
    // Consume even when the option/descriptor no longer qualifies. A queued
    // resource from an earlier allocation must never authorize a later one.
    let construction = if aligned(entry) && read::<u32>(entry, 0xe0) == 0 {
        // Prior entry teardown has finished before the next native allocation.
        // Keep its table identity until here, even when options/modes change.
        *LOADED_ROSTER.lock().unwrap() = None;
        std::mem::take(&mut *CONSTRUCTION.lock().unwrap())
    } else {
        Construction::default()
    };
    if aligned(entry) && bounded_log(&BOUNDARY_LOGS, 384) {
        trace!(
            "[TrioBoundary] created generation={} entry=0x{:x} owner={} descriptor={:x?} mode={:?}",
            GENERATION.fetch_add(1, Ordering::Relaxed),
            entry as usize,
            read::<u32>(entry, 0xe0),
            descriptor(entry),
            game_mode::current()
        );
    }
    if aligned(entry) && bounded_log(&CALL_LOGS, 32) {
        trace!("[TrioDry] initial_call owner={} installed={} outer={} loader={} count={} kinds={:x?} controllers={} preload_requested={} mode={:?}",
            read::<u32>(entry, 0xe0), read::<u64>(entry, 0x4150),
            read::<u8>(entry, 0x5920), read::<u8>(entry, 0x591c),
            read::<u32>(entry, 0x14),
            [read::<u32>(entry, 0x18), read::<u32>(entry, 0x1c), read::<u32>(entry, 0x20)],
            read::<u32>(entry, 0x28), construction.queued, game_mode::current());
    }
    if !enabled()
        || !practice_scene()
        || !resource_scope(construction.scope)
        || !construction.queued
        || !aligned(entry)
        || read::<u32>(entry, 0xe0) != 0
        || read::<u64>(entry, 0x4150) != 0
        || read::<u8>(entry, 0x5920) != 0
        || read::<u8>(entry, 0x591c) != 14
        || read::<u32>(entry, 0x14) != 1
        || read::<u32>(entry, 0x28) != 1
        || read::<u32>(entry, 0x18) != construction.kinds[0]
        || read::<u32>(entry, 0x1c) != u32::MAX
        || read::<u32>(entry, 0x20) != u32::MAX
    {
        return;
    }
    // Entry-owned initial descriptor, before caller654570 starts resource loading.
    // No caller stack mutation, retained pointer, or pending-descriptor overwrite.
    write(entry, 0x14, 3u32);
    for (index, kind) in construction.kinds.iter().enumerate() {
        write(entry, 0x18 + index * 4, *kind);
    }
    write(entry, 0x43, 0u8);
    write(entry, 0x5918, 0u8);
    *LOADED_ROSTER.lock().unwrap() = Some(EntryRoster {
        entry: entry as usize,
        kinds: construction.kinds,
        pending: None,
    });
    if bounded_log(&DESCRIPTOR_LOGS, 32) {
        trace!("[TrioDry] descriptor_proposal entry=0 kinds={:x?} configured=3 controllers=1 initial=0 construction={:?}", construction.kinds, construction);
    }
}

#[skyline::hook(offset = 0x17e4f50)]
unsafe fn prepare_resources(resource: *mut u8) {
    let trace = aligned(resource) && bounded_log(&BOUNDARY_LOGS, 384);
    if trace {
        trace!("[TrioBoundary] resource_before r=0x{:x} owner={} flags4c=0x{:x} kinds={:x?} snapshot0={:x?} mode={:?}", resource as usize, read::<u32>(resource, 0x54), read::<u32>(resource, 0x4c), [read::<u32>(resource, 0x58), read(resource, 0x5c), read(resource, 0x60)], descriptor(resource.add(0xc0)), game_mode::current());
    }
    if aligned(resource) && bounded_log(&PREPARE_LOGS, 48) {
        trace!(
            "[TrioDry] resource_prepare owner={} kinds={:x?} mode={:?}",
            read::<u32>(resource, 0x54),
            [
                read::<u32>(resource, 0x58),
                read::<u32>(resource, 0x5c),
                read::<u32>(resource, 0x60)
            ],
            game_mode::current()
        );
    }
    if aligned(resource) && read::<u32>(resource, 0x54) == 0 {
        let roster = configured_roster();
        let mut construction = CONSTRUCTION.lock().unwrap();
        if training_resources() {
            *construction = Construction {
                scope: if roster.is_some() {
                    Scope::Training
                } else {
                    Scope::None
                },
                epoch: CONSTRUCTION_EPOCH.fetch_add(1, Ordering::Relaxed) + 1,
                kinds: roster.unwrap_or_default(),
                ..Construction::default()
            };
        }
        if enabled()
            && resource_scope(construction.scope)
            && construction.resource == 0
            && read::<u32>(resource, 0x4c) & 1 == 0
            && [
                read::<u32>(resource, 0x58),
                read::<u32>(resource, 0x5c),
                read::<u32>(resource, 0x60),
            ] == [construction.kinds[0], u32::MAX, u32::MAX]
        {
            construction.resource = resource as usize;
            write(resource, 0x5c, construction.kinds[1]);
            write(resource, 0x60, construction.kinds[2]);
        }
    }
    original!()(resource);
    if trace {
        trace!("[TrioBoundary] resource_after r=0x{:x} owner={} flags4c=0x{:x} snapshot0={:x?} mode={:?}", resource as usize, read::<u32>(resource, 0x54), read::<u32>(resource, 0x4c), descriptor(resource.add(0xc0)), game_mode::current());
    }
}

#[skyline::hook(offset = 0x654dc0)]
unsafe fn configure_flags(entry: *mut u8) {
    original!()(entry);
    if aligned(entry) && bounded_log(&BOUNDARY_LOGS, 384) {
        trace!("[TrioBoundary] configured entry=0x{:x} owner={} descriptor={:x?} pending={:x?} mode={:?}", entry as usize, read::<u32>(entry, 0xe0), descriptor(entry), descriptor(entry.add(0x70)), game_mode::current());
    }
    if trio_entry(entry) {
        let old: u8 = read(entry, 0x591f);
        write(entry, 0x591f, old | 0x04);
        if bounded_log(&FLAGS_LOGS, 32) {
            trace!(
                "[TrioDry] configured_flags entry=0 old=0x{:02x} new=0x{:02x}",
                old,
                old | 0x04
            );
        }
    }
}

unsafe fn table_override(ctx: &mut InlineCtx, target: usize) {
    // No mode gate: teardown must free the same kind-tagged contexts after mode exit.
    if trio_entry(ctx.registers[19].x() as *const u8) {
        // The native descriptor owns this contiguous three-kind table. It stays
        // stable through selection and teardown, unlike mutable menu settings.
        ctx.registers[target].set_x(ctx.registers[19].x() + 0x18);
    }
}

#[skyline::hook(offset = 0x655484, inline)]
unsafe fn create_table(ctx: &mut InlineCtx) {
    table_override(ctx, 23);
}
#[skyline::hook(offset = 0x64fbcc, inline)]
unsafe fn select_table(ctx: &mut InlineCtx) {
    table_override(ctx, 9);
}
#[skyline::hook(offset = 0x64f9e8, inline)]
unsafe fn load_ai_table(ctx: &mut InlineCtx) {
    table_override(ctx, 21);
}
#[skyline::hook(offset = 0x6516e4, inline)]
unsafe fn free_table(ctx: &mut InlineCtx) {
    table_override(ctx, 20);
}

#[skyline::hook(offset = 0x17e5bc0)]
unsafe fn request_resources(resource: *mut u8) {
    original!()(resource);
    let construction = *CONSTRUCTION.lock().unwrap();
    if !resource_scope(construction.scope)
        || construction.resource != resource as usize
        || construction.queued
        || !aligned(resource)
        || read::<u32>(resource, 0x54) != 0
        || [
            read::<u32>(resource, 0x58),
            read(resource, 0x5c),
            read(resource, 0x60),
        ] != construction.kinds
    {
        return;
    }
    let base = BASE.load(Ordering::Acquire);
    let request: unsafe extern "C" fn(*mut u8, u32) = std::mem::transmute(base + 0x17e64d0);
    for kind in &construction.kinds[1..] {
        if bounded_log(&RESOURCE_LOGS, 64) {
            trace!("[TrioDry] resource_extra kind=0x{:x}", kind);
        }
        request(resource, *kind);
    }
    // This records queued requests, not completion. Native loading owns its wait.
    let mut current = CONSTRUCTION.lock().unwrap();
    if current.epoch == construction.epoch && current.resource == resource as usize {
        current.queued = true;
    }
}

pub fn init() {
    if !game_mode::init() {
        trace!("[TrioDry] unsupported main mode fingerprint: no mutation hooks");
        return;
    }
    unsafe {
        let base = getRegionAddress(Region::Text) as usize;
        for (offset, expected) in &FINGERPRINTS {
            if std::slice::from_raw_parts((base + offset) as *const u32, expected.len()) != expected
            {
                trace!(
                    "[TrioDry] fingerprint mismatch main+0x{:x}: no mutation hooks",
                    offset
                );
                return;
            }
        }
        if read::<u32>((base + 0x17e5be8) as *const u8, 0) != 0x9400023a {
            trace!("[TrioDry] request call target mismatch: no mutation hooks");
            return;
        }
        BASE.store(base, Ordering::Release);
        let mut manager_symbol = 0;
        LookupSymbol(
            &mut manager_symbol,
            b"_ZN3lib9SingletonIN3app14FighterManagerEE9instance_E\0".as_ptr(),
        );
        MANAGER_SYMBOL.store(manager_symbol, Ordering::Release);
        if cfg!(feature = "trio-probe") {
            skyline::install_hooks!(
                install_descriptor,
                configure_flags,
                create_table,
                select_table,
                load_ai_table,
                free_table,
                prepare_resources,
                request_resources,
                commit_switch,
                allocation_trace,
                reload_trace,
                destruction_trace,
                waiting_construction,
                refresh_entry_portrait,
                refresh_ordinal_portrait
            );
        } else {
            // Normal builds retain lifecycle observation and the presentation-only
            // reset fix. No descriptor expansion, extra bodies or trio mutation hooks.
            skyline::install_hooks!(
                allocation_trace,
                reload_trace,
                destruction_trace,
                refresh_entry_portrait,
                refresh_ordinal_portrait
            );
        }
        trace!("[CharacterLifecycle] main21450c647b8c5940; fingerprints passed; exclusive_trio_probe={}", cfg!(feature = "trio-probe"));
    }
}

pub fn clear_input_state() {
    HOTKEY_OWNS_TAUNT.store(false, Ordering::Relaxed);
}

// Native reset rebuilds the HUD from the original selection, then repairs
// Pokémon Trainer's active portrait in these two presentation-only helpers.
// Their kind-range check excludes our arbitrary trio. Admit only the verified
// current trio body into that existing refresh path, retaining w0 (real kind).
// Native138c9b0/138a3a0 still construct the portrait descriptor and the original
// helper still owns the HUD cache/dirty flag. No selection, stock, or body write.
unsafe fn allow_trio_portrait(ctx: &mut InlineCtx, entry_id: u64, source: &str) {
    if entry_id > 1 || !ordinary_training() {
        return;
    }
    let symbol = MANAGER_SYMBOL.load(Ordering::Acquire);
    if symbol == 0 {
        return;
    }
    let manager = std::ptr::read_volatile(symbol as *const *mut app::FighterManager);
    if manager.is_null() {
        return;
    }
    let entry = FighterManager::get_fighter_entry(manager, app::FighterEntryID(entry_id as i32))
        as *const u8;
    crate::character_roster::allow_portrait(ctx, entry_id, entry);
    if !cfg!(feature = "trio-probe") || entry_id == 1 {
        return;
    }
    if !trio_entry(entry) || read::<u64>(entry, 0x4150) != 3 {
        return;
    }
    let active: u8 = read(entry, 0x5918);
    let kinds = entry_kinds(entry);
    if active >= 3 || ctx.registers[0].x() as u32 != kinds[active as usize] {
        return;
    }
    let body: *const u8 = read(entry, 0x4160 + active as usize * 8);
    if !aligned(body)
        || read::<usize>(body, 0) != BASE.load(Ordering::Acquire) + 0x4f80210
        || read::<u32>(body, 0xc) != kinds[active as usize]
        || read::<u64>(body, 0x10) != 0
    {
        return;
    }
    // Only the upcoming CMP's range-test temporary changes. The real kind,
    // native entry ID, costume and portrait arguments remain intact.
    ctx.registers[8].set_x(0);
    if bounded_log(&HUD_REFRESH_LOGS, 64) {
        trace!(
            "[TrioHud] native_refresh source={} index={} kind=0x{:x}",
            source,
            active,
            kinds[active as usize]
        );
    }
}

#[skyline::hook(offset = 0x1510d3c, inline)]
unsafe fn refresh_entry_portrait(ctx: &mut InlineCtx) {
    allow_trio_portrait(ctx, ctx.registers[20].x(), "entry");
}

#[skyline::hook(offset = 0x1560748, inline)]
unsafe fn refresh_ordinal_portrait(ctx: &mut InlineCtx) {
    allow_trio_portrait(ctx, ctx.registers[21].x(), "ordinal");
}

/// Reserve a conflicting chord before it enters the controller-delay queue.
/// Suppress the entire held taunt, otherwise the next held frame would generate
/// a delayed rising edge after the normal hotkey request had been consumed.
pub fn reserve_hotkey_inputs(out: &mut MappedInputs) {
    if !enabled() || !practice_scene() || !out.buttons.contains(Buttons::APPEAL_SR) {
        clear_input_state();
        return;
    }
    if crate::common::button_config::has_pending_request() {
        HOTKEY_OWNS_TAUNT.store(true, Ordering::Relaxed);
    }
    if HOTKEY_OWNS_TAUNT.load(Ordering::Relaxed) {
        out.buttons.remove(Buttons::APPEAL_SR);
    }
}

// Called on the actual live fighter's category1 input callback, outside observer locks.
// This only queues the native request. FighterManager owns snapshot/deactivation.
pub unsafe fn command(accessor: &mut BattleObjectModuleAccessor) {
    observe_activation(accessor);
    if BASE.load(Ordering::Acquire) == 0
        || !enabled()
        || !practice_scene()
        || (crate::training::lobby::is_waiting_scene() && !crate::training::lobby::active())
        || crate::common::menu::captures_input()
        || crate::common::button_config::has_pending_request()
        || crate::training::save_states::is_loading()
        || crate::training::recording_owns_player_input()
        || crate::training::lobby::native_ui_busy()
        || WorkModule::get_int(accessor, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID) != 0
        || !ControlModule::check_button_on(accessor, *CONTROL_PAD_BUTTON_GUARD)
        || !ControlModule::check_button_trigger(accessor, *CONTROL_PAD_BUTTON_APPEAL_S_R)
    {
        return;
    }
    let status = StatusModule::status_kind(accessor);
    if ![
        *FIGHTER_STATUS_KIND_WAIT,
        *FIGHTER_STATUS_KIND_GUARD_ON,
        *FIGHTER_STATUS_KIND_GUARD,
    ]
    .contains(&status)
    {
        return;
    }
    let symbol = MANAGER_SYMBOL.load(Ordering::Acquire);
    if symbol == 0 {
        return;
    }
    let manager = std::ptr::read_volatile(symbol as *const *mut app::FighterManager);
    if manager.is_null() {
        return;
    }
    let entry = FighterManager::get_fighter_entry(manager, app::FighterEntryID(0)) as *mut u8;
    if !trio_entry(entry)
        || read::<u64>(entry, 0x4150) != 3
        || read::<u8>(entry, 0x5920) != 6
        || read::<u8>(entry, 0x591c) != 14
        || read::<u8>(entry, 0x591f) != 0x04
        || read::<u8>(entry, 0x591a) != 0xff
        || read::<u8>(entry, 0x5934) != 0
    {
        return;
    }
    let active: u8 = read(entry, 0x5918);
    let kinds = entry_kinds(entry);
    if active >= 3 || utility::get_kind(accessor) != kinds[active as usize] as i32 {
        return;
    }
    let body: *const u8 = read(entry, 0x4160 + active as usize * 8);
    if !aligned(body)
        || read::<usize>(body, 0) != BASE.load(Ordering::Acquire) + 0x4f80210
        || read::<u8>(body, 0x3a) != 4
        || read::<u32>(body, 8) != accessor.battle_object_id
        || !std::ptr::eq(
            read::<*mut BattleObjectModuleAccessor>(body, 0x20),
            accessor,
        )
    {
        return;
    }
    let next = (active + 1) % 3;
    let target: *const u8 = read(entry, 0x4160 + next as usize * 8);
    if !aligned(target)
        || read::<usize>(target, 0) != BASE.load(Ordering::Acquire) + 0x4f80210
        || read::<u32>(target, 0xc) != kinds[next as usize]
        || read::<u64>(target, 0x10) != 0
        || read::<u8>(target, 0x3a) != 3
    {
        return;
    }
    if !valid_bodies(entry, Some(active))
        || !(0..3).all(|i| {
            let body: *const u8 = read(entry, 0x4160 + i * 8);
            let specializer: *const u8 = read(body, 0xf700);
            if specializer.is_null() {
                return true;
            }
            if !aligned(specializer) {
                return false;
            }
            let vtable: usize = read(specializer, 0);
            let base = BASE.load(Ordering::Acquire);
            (base + 0x4800000..base + 0x5300000).contains(&vtable)
                && read::<usize>(vtable as *const u8, 0x2f0) == base + 0x68db80
        })
    {
        trace!("[TrioPractice] switch rejected: unsupported body/restore handler, entry=0x{:x} kinds={:x?}", entry as usize, kinds);
        return;
    }
    if let Some(roster) = LOADED_ROSTER.lock().unwrap().as_mut() {
        roster.pending = Some(next);
    }
    if bounded_log(&SWITCH_LOGS, 64) {
        trace!(
            "[TrioDry] native_request from={} to={} kinds={:x?} outgoing_status={}",
            active,
            next,
            kinds,
            status
        );
    }
    write(entry, 0x5932, 0u8);
    write(entry, 0x591a, next);
}

unsafe fn valid_bodies(entry: *const u8, active: Option<u8>) -> bool {
    let base = BASE.load(Ordering::Acquire);
    let kinds = entry_kinds(entry);
    (0..3).all(|i| {
        let body: *const u8 = read(entry, 0x4160 + i * 8);
        aligned(body)
            && read::<usize>(body, 0) == base + 0x4f80210
            && read::<u32>(body, 0xc) == kinds[i]
            && read::<u64>(body, 0x10) == 0
            && read::<u8>(body, 0x3a) == if active == Some(i as u8) { 4 } else { 3 }
    })
}

unsafe fn observe_activation(accessor: &BattleObjectModuleAccessor) {
    let Some(roster) = *LOADED_ROSTER.lock().unwrap() else {
        return;
    };
    let Some(target) = roster.pending else {
        return;
    };
    if !practice_scene() {
        return;
    }
    let symbol = MANAGER_SYMBOL.load(Ordering::Acquire);
    if symbol == 0 {
        return;
    }
    let manager = std::ptr::read_volatile(symbol as *const *mut app::FighterManager);
    if manager.is_null() {
        return;
    }
    let entry = FighterManager::get_fighter_entry(manager, app::FighterEntryID(0)) as *const u8;
    if entry as usize != roster.entry
        || !trio_entry(entry)
        || read::<u64>(entry, 0x4150) != 3
        || read::<u8>(entry, 0x5920) != 6
        || read::<u8>(entry, 0x591c) != 14
        || read::<u8>(entry, 0x5918) != target
        || !valid_bodies(entry, Some(target))
    {
        return;
    }
    let body: *const u8 = read(entry, 0x4160 + target as usize * 8);
    if read::<u32>(body, 8) != accessor.battle_object_id {
        return;
    }
    if let Some(current) = LOADED_ROSTER.lock().unwrap().as_mut() {
        current.pending = None;
    }
    crate::logging::diagnostic(format_args!(
        "[TrioPractice] activation_complete entry=0x{:x} index={} kind=0x{:x} loader=14 outer=6",
        roster.entry, target, roster.kinds[target as usize]
    ));
}

#[skyline::hook(offset = 0x6581f0)]
unsafe fn commit_switch(entry: *mut u8) {
    let transition = if practice_scene()
        && trio_entry(entry)
        && read::<u64>(entry, 0x4150) == 3
        && read::<u8>(entry, 0x5920) == 6
        && read::<u8>(entry, 0x591c) == 14
        && read::<u8>(entry, 0x591f) == 4
        && read::<u8>(entry, 0x5932) == 0
        && read::<u8>(entry, 0x5934) == 0
    {
        let from: u8 = read(entry, 0x5918);
        let to: u8 = read(entry, 0x591a);
        (from < 3 && to < 3 && from != to && valid_bodies(entry, Some(from))).then_some((from, to))
    } else {
        None
    };
    // Exact owned ABI: one entry pointer, void. Original snapshots state and
    // synchronously deactivates the outgoing body before publishing loader12.
    original!()(entry);
    let Some((from, to)) = transition else {
        return;
    };
    if !practice_scene()
        || !trio_entry(entry)
        || read::<u64>(entry, 0x4150) != 3
        || read::<u8>(entry, 0x5920) != 3
        || read::<u8>(entry, 0x591c) != 12
        || read::<u8>(entry, 0x591f) != 4
        || read::<u8>(entry, 0x5910) != from
        || read::<u8>(entry, 0x5918) != to
        || read::<u8>(entry, 0x5919) != to
        || read::<u8>(entry, 0x591a) != 0xff
        || read::<u8>(entry, 0x591b) != 7
        || read::<u8>(entry, 0x5934) != 1
        || read::<u8>(entry, 0x41e8) != 0
        || !valid_bodies(entry, None)
    {
        trace!("[TrioDry] completion rejected: native postcondition mismatch");
        return;
    }
    let context: *mut u8 = read(entry, 0xf8);
    if !aligned(context) {
        trace!("[TrioDry] completion rejected: missing activation context");
        return;
    }
    // PT34cc7f4 and Aegis33f4920 complete their visual switch callback by
    // publishing change7/loader13 and a state-transfer mode. Ordinary fighters
    // have no such animation callback. Their native snapshot remains current
    // here, immediately after deactivation; no elapsed-time or5933 bypass.
    // 649980 already captured the final position/facing before deactivation.
    // 656ee0 activates from context+9f0/+9e0, so mirror the native completion
    // callback's copy instead of reusing the initial spawn position.
    write(context, 0x9f0, read::<[u32; 4]>(entry, 0x41f0));
    write(context, 0x9e0, read::<u32>(entry, 0x4200));
    // 64aea0 passes mode5 to specializer+2f0. The request gate verifies all
    // loaded specializers use the owned RET stub (or the native null path).
    write(entry, 0x41a0, 5u32);
    write(entry, 0x591c, 13u8);
    if bounded_log(&COMPLETION_LOGS, 64) {
        trace!("[TrioDry] animation_complete from={} to={} snapshot_xyz={:?} snapshot_lr={} restore_mode=5 loader=13 wait5933={}", from, to,
            [read::<f32>(entry, 0x41f0), read(entry, 0x41f4), read(entry, 0x41f8)],
            read::<f32>(entry, 0x4200), read::<u8>(entry, 0x5933));
    }
}
