//! Passive 13.0.5 stage-transition prerequisites when used alone.
//! The separate default-off request alpha reuses this verified update callback;
//! observing a stage never queues a request on its own.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use skyline::hooks::{getRegionAddress, InlineCtx, Region};

static VERIFIED: AtomicBool = AtomicBool::new(false);
static UPDATE_LINES: AtomicUsize = AtomicUsize::new(0);
static RESOURCE_LINES: AtomicUsize = AtomicUsize::new(0);
static LAST: Mutex<Option<Snapshot>> = Mutex::new(None);
const LIMIT: usize = 64;

// Exact fixed list initialized by native 0x15bf530 and tested by 0x15bebd0.
// Bit positions in ItemGate.blocked_kinds correspond to this order.
const BLOCKED_KINDS: [usize; 18] = [
    0xa6, 0xab, 0xd8, 0xb8, 0xca, 0x128, 0x148, 0x137, 0x102, 0x103, 0x106, 0x10d, 0xfd, 0xf2,
    0xe4, 0xe1, 0x153, 0x154,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Resource {
    ptr: usize,
    state: u32,
    kind: u32,
    hash: u64,
    flag_500: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ItemGate {
    flags: u32,
    dragoon_count: u64,
    blocked_kinds: u32,
    ready: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Snapshot {
    mode: u32,
    melee: u8,
    waiting_ui_ready: bool,
    native_ui_busy: bool,
    roster_busy: bool,
    core: usize,
    disabled: u8,
    stop_mask: u32,
    state: u32,
    active: i32,
    incoming: i32,
    outgoing: i32,
    stage_slots: [usize; 3],
    active_kind: Option<u32>,
    resources: [Option<Resource>; 4],
    native_next_slot: Option<u32>,
    item_gate: Option<ItemGate>,
    task_state: Option<u32>,
}

unsafe fn ptr(at: usize) -> Option<usize> {
    let value = std::ptr::read_volatile(at as *const usize);
    (value != 0 && value & 7 == 0).then_some(value)
}

unsafe fn word(at: usize) -> u32 {
    std::ptr::read_volatile(at as *const u32)
}

unsafe fn resource_slots(owner: usize) -> [Option<Resource>; 4] {
    std::array::from_fn(|slot| {
        let at = ptr(owner + slot * 8)?;
        Some(Resource {
            ptr: at,
            state: word(at),
            kind: word(at + 0x20),
            hash: std::ptr::read_volatile((at + 0x28) as *const u64),
            flag_500: std::ptr::read_volatile((at + 0x500) as *const u8),
        })
    })
}

// Read-only equivalent of the native gate. The original function allocates
// a popcount table and may initialize a list; this observer does neither.
unsafe fn item_gate(base: usize) -> Option<ItemGate> {
    let manager = ptr(base + 0x52c4070)?;
    let a = ptr(manager + 0xa0)?;
    let b = ptr(a)?;
    let input = ptr(b)?;
    let control = ptr(input)?;
    let counts = ptr(manager + 0x58)?;
    let flags = word(control + 0x2c);
    let dragoon_count = std::ptr::read_volatile((counts + 0x348) as *const u64);
    let mut blocked_kinds = 0;
    for (bit, kind) in BLOCKED_KINDS.iter().enumerate() {
        if std::ptr::read_volatile((counts + kind * 8) as *const u64) != 0 {
            blocked_kinds |= 1 << bit;
        }
    }
    Some(ItemGate {
        flags,
        dragoon_count,
        blocked_kinds,
        ready: u64::from((flags & 7).count_ones()).min(dragoon_count) <= 2
            && flags & 8 == 0
            && blocked_kinds == 0,
    })
}

fn log_budget(counter: &AtomicUsize) -> bool {
    let line = counter.fetch_add(1, Ordering::Relaxed);
    if line == LIMIT {
        crate::logging::diagnostic(format_args!(
            "[PracticeStageObserver] line limit reached for this stream"
        ));
    }
    line < LIMIT
}

#[skyline::hook(offset = 0x2607360, inline)]
unsafe fn stage_update(ctx: &mut InlineCtx) {
    let Some(mode) = crate::common::game_mode::current() else {
        return;
    };
    let in_scope = mode.initialized
        && (crate::common::is_native_training_mode()
            || crate::lobby_stage_probe::qualified_waiting(mode.mode, mode.melee));
    if !in_scope {
        #[cfg(feature = "practice-stage-request-alpha")]
        crate::practice_stage_request::depart();
        if let Ok(mut last) = LAST.try_lock() {
            *last = None;
        }
        return;
    }
    let wrapper = ctx.registers[0].x() as usize;
    if wrapper == 0 || wrapper & 7 != 0 {
        return;
    }
    let Some(core) = ptr(wrapper) else { return };
    let active = word(core + 0x17c) as i32;
    let stage_slots = std::array::from_fn(|slot| ptr(core + 0x110 + slot * 8).unwrap_or(0));
    // The budget is process-lifetime. Once its final notice has been emitted,
    // no consumer needs these diagnostic reads or LAST's comparison. Keep the
    // request service below running on every update, including after the budget.
    let snapshot = (UPDATE_LINES.load(Ordering::Relaxed) <= LIMIT).then(|| {
        let base = getRegionAddress(Region::Text) as usize;
        let resources = ptr(base + 0x532a9d0).map(|owner| resource_slots(owner));
        let active_kind = if (0..3).contains(&active) && stage_slots[active as usize] != 0 {
            Some(word(stage_slots[active as usize] + 8))
        } else {
            None
        };
        Snapshot {
            mode: mode.mode,
            melee: mode.melee,
            waiting_ui_ready: crate::training::lobby::active(),
            native_ui_busy: crate::training::lobby::native_ui_busy(),
            roster_busy: crate::character_roster::busy(),
            core,
            disabled: std::ptr::read_volatile((core + 0x178) as *const u8),
            stop_mask: word(core + 0x250) & word(core + 0x254),
            state: word(core + 0xbc),
            active,
            incoming: word(core + 0xb0) as i32,
            outgoing: word(core + 0xb4) as i32,
            stage_slots,
            active_kind,
            native_next_slot: resources
                .map(|slots| u32::from(slots[0].is_some_and(|r| (r.state | 2) == 3))),
            resources: resources.unwrap_or([None; 4]),
            item_gate: item_gate(base),
            task_state: ptr(base + 0x532f488)
                .and_then(|owner| ptr(owner))
                .map(|at| word(at)),
        }
    });
    #[cfg(feature = "practice-stage-request-alpha")]
    crate::practice_stage_request::update(wrapper, core, mode.mode, mode.melee,
        stage_slots.get(active as usize).copied().unwrap_or(0));
    let Some(snapshot) = snapshot else { return };
    if let Ok(mut last) = LAST.try_lock() {
        if last.as_ref() != Some(&snapshot) {
            *last = Some(snapshot);
            if log_budget(&UPDATE_LINES) {
                crate::logging::diagnostic(format_args!(
                    "[PracticeStageObserver] update {snapshot:?}"
                ));
            }
        }
    }
}

// Called from the existing preload hook, so two hooks never compete at one site.
// Include both waiting families' non-practice preloads in passive diagnostics:
// their slot ownership matters, but this observer never changes them.
pub unsafe fn observe_resource_request(ctx: &InlineCtx) {
    if !VERIFIED.load(Ordering::Acquire) {
        return;
    }
    let Some(mode) = crate::common::game_mode::current() else {
        return;
    };
    if !mode.initialized || !(crate::lobby_stage_cycle::family(mode.mode).is_some()
        || crate::common::is_native_training_mode()) {
        return;
    }
    let owner = ctx.registers[0].x() as usize;
    let config = ctx.registers[1].x() as usize;
    if owner == 0 || owner & 7 != 0 || config == 0 || config & 7 != 0 {
        return;
    }
    if log_budget(&RESOURCE_LINES) {
        let base = getRegionAddress(Region::Text) as usize;
        crate::logging::diagnostic(format_args!(
            "[PracticeStageObserver] resource_before mode={:?} caller=0x{:x} slot={} kind={} hash=0x{:x} resources={:?}",
            mode,
            (ctx.registers[30].x() as usize).wrapping_sub(base),
            ctx.registers[2].w(),
            word(config),
            std::ptr::read_volatile((config + 8) as *const u64),
            resource_slots(owner),
        ));
    }
}

pub fn init() {
    let base = unsafe { getRegionAddress(Region::Text) } as usize;
    // Fingerprint hook, field reads, ItemManager indirection, predicate, and fixed
    // item list. Offsets alone do not authorize reading another build's objects.
    let checks: &[(usize, &[u32])] = &[
        (
            0x2607360,
            &[
                0xd10583ff, 0xfd008be8, 0xa9125ffc, 0xa91357f6, 0xa9144ff4, 0xa9157bfd, 0x910543fd,
                0xf9400013,
            ],
        ),
        (
            0x260743c,
            &[0xb9425268, 0xb9425669, 0x6a08013f, 0x54000180, 0xf9410a74],
        ),
        (
            0x25f7860,
            &[
                0xd10103ff, 0xa90157f6, 0xa9024ff4, 0xa9037bfd, 0x9100c3fd, 0x7100085f, 0x54000448,
                0xaa0003f3,
            ],
        ),
        (
            0x14a52e8,
            &[
                0xf001f0e8, 0xf9403908, 0xf9405108, 0xf9400108, 0xf9400100, 0x94046635,
            ],
        ),
        (
            0x15bec5c,
            &[
                0xb9402e88, 0x92400908, 0xf8687815, 0xb4000040, 0x948dbfb5, 0xd001e834, 0xf9403a88,
                0xf9402d08, 0xf941a508, 0xeb0802bf, 0x9a953108, 0xf100091f, 0x54000268, 0xf9400268,
                0x3940b108, 0x37180208, 0x94000225, 0xb001e829, 0x913f6129, 0xa9402528, 0xeb09011f,
                0x54000100, 0xf9403a8a, 0xf9402d4a, 0xb880450b, 0xf86b794b, 0xb50000ab, 0xeb08013f,
                0x54ffff81, 0x320003e0, 0x14000002,
            ],
        ),
        (
            0x15bf5bc,
            &[
                0xd28014c9, 0xd2801b0a, 0x9001e821, 0x913f6021, 0x91012008, 0xf9000020, 0xf2c01569,
                0xf2c0170a, 0xf9000828, 0xa9002809, 0xd2801949, 0xd280290a, 0xf2c02509, 0xf2c026ea,
                0xa9012809, 0xd2802049, 0xd28020ca, 0xf2c02069, 0xf2c021aa, 0xa9022809, 0xd2801fa9,
                0xd2801c8a, 0xf2c01e49, 0xf2c01c2a, 0xa9032809, 0xd2802a69, 0xf2c02a89, 0xf9002009,
            ],
        ),
        (
            0x260235c,
            &[
                0xb9400008, 0x121f7908, 0x7100091f, 0x540013e0, 0xb9402015, 0xb9400028, 0xaa0103f3,
                0xaa0003f4, 0x6b0802bf, 0x54001321, 0xf9401688, 0xf9400669,
            ],
        ),
    ];
    let verified = checks.iter().all(|(offset, expected)| unsafe {
        std::slice::from_raw_parts((base + offset) as *const u32, expected.len()) == *expected
    });
    crate::logging::diagnostic(format_args!(
        "[PracticeStageObserver] verified={verified} passive=true"
    ));
    if verified {
        #[cfg(feature = "practice-stage-request-alpha")]
        crate::practice_stage_request::init(base);
        VERIFIED.store(true, Ordering::Release);
        skyline::install_hooks!(stage_update);
    }
}
