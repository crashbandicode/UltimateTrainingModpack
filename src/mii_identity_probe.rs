//! Development-only Mii identity observer (feature `mii-identity-probe`). Read-only: never
//! writes game memory or files. Sites and registers: artifacts/enhancements/mii-fable/probe-next/PROPOSAL.md
//! section 3. Logs hashes of CharInfo records, never raw appearance bytes. Line-capped.
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use skyline::hooks::{getRegionAddress, InlineCtx, Region};

const LINE_CAP: usize = 120;
static LINES: AtomicUsize = AtomicUsize::new(0);
static LAST_TUPLE: [AtomicU64; 8] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];
/// FighterEntry -> saved-entry pairs captured at the native setup site (H4); at most eight.
static PAIRS: Mutex<[(usize, usize); 8]> = Mutex::new([(0, 0); 8]);

fn log(args: std::fmt::Arguments<'_>) {
    if LINES.fetch_add(1, Ordering::Relaxed) < LINE_CAP {
        crate::logging::diagnostic(args);
    }
}

unsafe fn rd<T: Copy>(base: usize, offset: usize) -> T {
    std::ptr::read_volatile((base + offset) as *const T)
}

fn aligned(p: usize) -> bool {
    p != 0 && p.is_multiple_of(8)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100000001b3))
}

fn is_mii_kind(kind: u32) -> bool {
    (0x48..=0x4a).contains(&kind)
}

fn pair_for(fe: usize) -> Option<usize> {
    PAIRS.lock().unwrap().iter().find(|(f, _)| *f == fe).map(|(_, e)| *e)
}

pub use crate::mii_identity_binding::bind_saved_entry;

/// H4: `main+0x178a970`, x19 = saved entry E, x8 = FighterEntry (setup writes D+0x390.. from E+0x1b2..).
#[skyline::hook(offset = 0x178a970, inline)]
unsafe fn setup_e_to_d(ctx: &mut InlineCtx) {
    let e = ctx.registers[19].x() as usize;
    let fe = ctx.registers[8].x() as usize;
    if !aligned(e) || !aligned(fe) {
        return;
    }
    {
        let mut pairs = PAIRS.lock().unwrap();
        if let Some(slot) = pairs.iter_mut().find(|(f, _)| *f == fe || *f == 0) {
            *slot = (fe, e);
        }
    }
    let tokens: [u16; 5] = [rd(e, 0x1a8), rd(e, 0x1aa), rd(e, 0x1ac), rd(e, 0x1ae), rd(e, 0x1b0)];
    let specials: [u8; 4] = [rd(e, 0x1b2), rd(e, 0x1b3), rd(e, 0x1b4), rd(e, 0x1b5)];
    log(format_args!(
        "[MiiProbe] setup fe={fe:#x} e={e:#x} kind={} id={} colour={} tokens={tokens:04x?} specials={specials:?}",
        rd::<i8>(e, 8), rd::<u32>(e, 0x14), rd::<u8>(e, 0x19)
    ));
}

/// H1: `main+0x60ab74`, x24 = FighterEntry, x8 = D, x10 = record begin, x11 = record end, w28 = kind.
#[skyline::hook(offset = 0x60ab74, inline)]
unsafe fn fighter_init_index(ctx: &mut InlineCtx) {
    let fe = ctx.registers[24].x() as usize;
    let d = ctx.registers[8].x() as usize;
    let begin = ctx.registers[10].x() as usize;
    let end = ctx.registers[11].x() as usize;
    let kind = ctx.registers[28].w();
    if !aligned(d) || !aligned(begin) || end < begin {
        return;
    }
    let count = (end - begin) / 0x68;
    let idx = usize::from(rd::<u16>(d, 0x72));
    let hash = (idx < count.min(16)).then(|| {
        let rec = std::slice::from_raw_parts((begin + idx * 0x68) as *const u8, 0x58);
        fnv1a64(rec)
    });
    let specials: [u32; 4] = [rd(d, 0x390), rd(d, 0x394), rd(d, 0x398), rd(d, 0x39c)];
    log(format_args!(
        "[MiiProbe] init fe={fe:#x} kind={kind:#x} idx={idx} count={count} record_fnv={hash:016x?} specials={specials:?} colour={} d94={}",
        rd::<u8>(d, 0x51), rd::<u8>(d, 0x94)
    ));
}

/// H2: `main+0x60ace8`, w1 = N, w27 = S, w26 = Hi, w25 = Lw, x19 = Fighter object.
#[skyline::hook(offset = 0x60ace8, inline)]
unsafe fn specials_applied(ctx: &mut InlineCtx) {
    let fighter = ctx.registers[19].x() as usize;
    if !aligned(fighter) {
        return;
    }
    log(format_args!(
        "[MiiProbe] set_int fighter_kind={:#x} entry_id={} N={} S={} Hi={} Lw={}",
        rd::<u32>(fighter, 0xc), rd::<u32>(fighter, 0x10),
        ctx.registers[1].w(), ctx.registers[27].w(), ctx.registers[26].w(), ctx.registers[25].w()
    ));
}

/// H3a: `main+0x651420`, x19 = FighterEntry, x8 = D.
#[skyline::hook(offset = 0x651420, inline)]
unsafe fn rebuild_gate(ctx: &mut InlineCtx) {
    let fe = ctx.registers[19].x() as usize;
    let d = ctx.registers[8].x() as usize;
    if !aligned(fe) || !aligned(d) {
        return;
    }
    log(format_args!(
        "[MiiProbe] rebuild_gate fe={fe:#x} d94={} state={} substate={}",
        rd::<u8>(d, 0x94), rd::<u8>(fe, 0x5920), rd::<u8>(fe, 0x591c)
    ));
}

/// H3b: `main+0x651444`, w1 = id, w2 = kind, x4 = u16* idx, x19 = FighterEntry.
#[skyline::hook(offset = 0x651444, inline)]
unsafe fn rebuild_call(ctx: &mut InlineCtx) {
    let idx_ptr = ctx.registers[4].x() as *const u16;
    let idx = if idx_ptr.is_null() { u16::MAX } else { idx_ptr.read_volatile() };
    log(format_args!(
        "[MiiProbe] rebuild_call fe={:#x} id={} kind={:#x} idx={idx}",
        ctx.registers[19].x(), ctx.registers[1].w(), ctx.registers[2].w()
    ));
}

/// Called from `character_roster::service` with the live entry (owner already filtered).
/// Logs only when the observed tuple changes; the record hash is taken only at H1.
pub unsafe fn observe_entry(entry: *const u8, owner: u32) {
    let fe = entry as usize;
    if !aligned(fe) {
        return;
    }
    let kind: u32 = rd(fe, 0x18);
    if !is_mii_kind(kind) {
        return;
    }
    let d: usize = rd(fe, 0xf8);
    if !aligned(d) {
        return;
    }
    let tokens: [u16; 5] = [rd(fe, 0x46), rd(fe, 0x48), rd(fe, 0x4a), rd(fe, 0x4c), rd(fe, 0x4e)];
    let d_tokens: [u16; 4] = [rd(d, 0x72), rd(d, 0x74), rd(d, 0x76), rd(d, 0x78)];
    let specials: [u32; 4] = [rd(d, 0x390), rd(d, 0x394), rd(d, 0x398), rd(d, 0x39c)];
    let d94: u8 = rd(d, 0x94);
    let colour: u8 = rd(d, 0x51);
    let state: [u8; 2] = [rd(fe, 0x5920), rd(fe, 0x591c)];
    // Fixed 42-byte observation: avoid a heap allocation on every service tick.
    let mut tuple = [0u8; 42];
    let mut used = 0;
    let mut append = |bytes: &[u8]| {
        tuple[used..used + bytes.len()].copy_from_slice(bytes);
        used += bytes.len();
    };
    append(&kind.to_le_bytes());
    for value in tokens { append(&value.to_le_bytes()); }
    for value in d_tokens { append(&value.to_le_bytes()); }
    for value in specials { append(&value.to_le_bytes()); }
    append(&[d94, colour, state[0], state[1]]);
    let h = fnv1a64(&tuple);
    let slot = &LAST_TUPLE[(owner as usize).min(7)];
    if slot.swap(h, Ordering::Relaxed) == h {
        return;
    }
    // H4 never fires in Training (plan2-mii-runtime-01); bind through the game's entry table instead.
    let bound = bind_saved_entry(entry);
    let e_fields = pair_for(fe).or(bound.map(|b| b.e)).map(|e| {
        let t: [u16; 5] = [rd(e, 0x1a8), rd(e, 0x1aa), rd(e, 0x1ac), rd(e, 0x1ae), rd(e, 0x1b0)];
        let s: [u8; 4] = [rd(e, 0x1b2), rd(e, 0x1b3), rd(e, 0x1b4), rd(e, 0x1b5)];
        (t, s, rd::<u8>(e, 0x19))
    });
    log(format_args!(
        "[MiiProbe] entry owner={owner} fe={fe:#x} kind={kind:#x} fe_tokens={tokens:04x?} d_tokens={d_tokens:04x?} specials={specials:?} colour={colour} d94={d94} state={state:?} bound={:?} e={e_fields:?}",
        bound.map(|b| (b.id, b.e))
    ));
}

/// Called from `on_destroy` (that entry) and `scene_changed` (null = all).
pub fn forget(entry: *const u8) {
    let fe = entry as usize;
    for slot in PAIRS.lock().unwrap().iter_mut() {
        if fe == 0 || slot.0 == fe {
            *slot = (0, 0);
        }
    }
    for t in &LAST_TUPLE {
        t.store(0, Ordering::Relaxed);
    }
}

pub fn init() {
    unsafe {
        let base = getRegionAddress(Region::Text) as usize;
        let guards: [(usize, [u32; 4]); 5] = [
            (0x60ab74, [0x7940e508, 0xcb0a016b, 0x9343fd6b, 0x9b197d6b]),
            (0x60ace8, [0xf9400308, 0xf9405108, 0xaa1803e0, 0x2a1703e2]),
            (0x651420, [0x39425108, 0x350001a8, 0x90026348, 0xf9427d08]),
            (0x651444, [0x9400b4e3, 0xb9400320, 0xb9400281, 0x9100e3e2]),
            (0x178a970, [0x3946ca69, 0xf9407d0a, 0xb9039149, 0x3946ce69]),
        ];
        for (offset, words) in guards {
            let actual = std::slice::from_raw_parts((base + offset) as *const u32, 4);
            if actual != words {
                crate::logging::diagnostic(format_args!(
                    "[MiiProbe] disabled: words at {offset:#x} = {actual:08x?}, expected {words:08x?}"
                ));
                return;
            }
        }
        skyline::install_hooks!(setup_e_to_d, fighter_init_index, specials_applied, rebuild_gate, rebuild_call);
        crate::logging::diagnostic(format_args!("[MiiProbe] observer installed (five guarded sites)"));
    }
}