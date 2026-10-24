//! Native entry lifecycle observations and ordinary Training HUD reset repair.
//! Only hooks used by the shared single-body character loader are installed.

use crate::common::game_mode;
use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use skyline::nn::ro::LookupSymbol;
use smash::app::{self, lua_bind::*};
use std::sync::atomic::{AtomicUsize, Ordering};

macro_rules! trace {
    ($($arg:tt)*) => { crate::logging::diagnostic(format_args!($($arg)*)) };
}

static BASE: AtomicUsize = AtomicUsize::new(0);
static MANAGER_SYMBOL: AtomicUsize = AtomicUsize::new(0);
static BOUNDARY_LOGS: AtomicUsize = AtomicUsize::new(0);

// Owned 13.0.5 instructions at the five installed hook sites.
const FINGERPRINTS: [(usize, [u32; 8]); 5] = [
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
];

pub fn native_hooks_ready() -> bool {
    BASE.load(Ordering::Acquire) != 0
}

fn ordinary_training() -> bool {
    game_mode::current().is_some_and(|m| m.initialized && m.melee == 0x20 && m.mode == 0x04040000)
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
    crate::practice_trace::checkpoint(30, [ctx.registers[0].x(), ctx.registers[1].x(), ctx.registers[2].x(), ctx.registers[30].x(), 0]);
    crate::hang_trace::ALLOCATION.hit();
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
    crate::practice_trace::checkpoint(31, [ctx.registers[0].x(), ctx.registers[1].x(), ctx.registers[2].x(), ctx.registers[30].x(), 0]);
    crate::hang_trace::RELOAD.hit();
    let entry = ctx.registers[0].x() as *const u8;
    let desc = ctx.registers[1].x() as *const u8;
    if aligned(entry) && aligned(desc) && bounded_log(&BOUNDARY_LOGS, 384) {
        trace!("[TrioBoundary] reload entry=0x{:x} owner={} caller=main+0x{:x} guard={} incoming={:x?} active={:x?} pending={:x?} loader={} outer={} mode={:?}", entry as usize, read::<u32>(entry, 0xe0), ctx.registers[30].x().saturating_sub(BASE.load(Ordering::Acquire) as u64), read::<u8>(entry, 0x5935), descriptor(desc), descriptor(entry), descriptor(entry.add(0x70)), read::<u8>(entry, 0x591c), read::<u8>(entry, 0x5920), game_mode::current());
    }
}

#[skyline::hook(offset = 0x652160, inline)]
unsafe fn destruction_trace(ctx: &mut InlineCtx) {
    crate::practice_trace::event(32, [ctx.registers[0].x(), ctx.registers[30].x(), 0, 0, 0]);
    crate::hang_trace::DESTROY.hit();
    let entry = ctx.registers[0].x() as *const u8;
    crate::character_roster::on_destroy(entry);
    crate::practice_trace::checkpoint(33, [entry as u64, 0, 0, 0, 0]);
    if aligned(entry) && bounded_log(&BOUNDARY_LOGS, 384) {
        trace!("[TrioBoundary] destroy entry=0x{:x} owner={} active={:x?} pending={:x?} installed={} mode={:?}", entry as usize, read::<u32>(entry, 0xe0), descriptor(entry), descriptor(entry.add(0x70)), read::<u64>(entry, 0x4150), game_mode::current());
    }
}

pub fn init() {
    if !game_mode::init() {
        trace!("[CharacterLifecycle] unsupported main mode fingerprint: no mutation hooks");
        return;
    }
    unsafe {
        let base = getRegionAddress(Region::Text) as usize;
        for (offset, expected) in &FINGERPRINTS {
            if std::slice::from_raw_parts((base + offset) as *const u32, expected.len()) != expected
            {
                trace!(
                    "[CharacterLifecycle] fingerprint mismatch main+0x{:x}: no mutation hooks",
                    offset
                );
                return;
            }
        }
        BASE.store(base, Ordering::Release);
        let mut manager_symbol = 0;
        LookupSymbol(
            &mut manager_symbol,
            b"_ZN3lib9SingletonIN3app14FighterManagerEE9instance_E\0".as_ptr(),
        );
        MANAGER_SYMBOL.store(manager_symbol, Ordering::Release);
        skyline::install_hooks!(
            allocation_trace,
            reload_trace,
            destruction_trace,
            refresh_entry_portrait,
            refresh_ordinal_portrait
        );
        trace!("[CharacterLifecycle] main21450c647b8c5940; fingerprints passed");
    }
}

// Native reset reconstructs the original CSS portrait. Reacquire the current
// entry and admit its verified replacement to the existing native refresh path.
// The character loader owns the identity/body checks; no game pointer is cached.
unsafe fn allow_portrait(ctx: &mut InlineCtx, entry_id: u64) {
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
}

#[skyline::hook(offset = 0x1510d3c, inline)]
unsafe fn refresh_entry_portrait(ctx: &mut InlineCtx) {
    allow_portrait(ctx, ctx.registers[20].x());
}

#[skyline::hook(offset = 0x1560748, inline)]
unsafe fn refresh_ordinal_portrait(ctx: &mut InlineCtx) {
    allow_portrait(ctx, ctx.registers[21].x());
}
