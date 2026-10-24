//! Default-off original-call observation. Never calls the converter or writes its arguments,
//! native fields, pool records, saved files, INSTALL or ownership state. Two inline hooks retain
//! every native register, including volatile return registers; no guessed Rust return ABI.
use super::{ready_kind, ready_single_mii, BASE, INSTALL};
use crate::common::game_mode;
use skyline::hooks::InlineCtx;
use std::convert::TryInto;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use training_mod_consts::mii::native_values::Mirrors;

const ARM: &str = "sd:/ultimate/TrainingModpack/mii_origin_observer.arm";
const E_ROWS: usize = 0x52c5730;
const HOLDER: usize = 0x593b6a0;
const OWNER_TABLE: usize = 0x5324680;
const LOCK: usize = 0x39c19c0;
const UNLOCK: usize = 0x39c19d0;
const RECORDS: usize = 133;
const STRIDE: usize = 0x68;
const MAX_SERVICE_VISITS: u16 = 1200;
// Owned 13.0.5 ELF 0789f0a4ae9652f335eca5eabc7df3e58b0954012a0bebdea374f317706536c2.
// Verify BOTH hook sites in the complete pristine function before installing either hook.
const CODE: [(usize, usize, u64); 7] = [
    (0x17880c0, 0x1010, 0x8709456c299de3da),
    (0x178f5b0, 0x8c, 0x31aea948405579fe), // R producer / E+a7 -> w2 / output E
    (0x1787ed4, 0x14, 0x74f609a94ba9a735), // R+456 mode skip
    (0x16b6a10, 0x198, 0xaecb3e64a124d550), // native configuration member copy
    (LOCK, 0x10, 0x159d9a9fb7f5b78b),
    (UNLOCK, 0x10, 0x805b05452dae4eff),
    (0x67e898, 0x28, 0x822d66dfd21f69b7), // resource-owner table / O+68
];

static ARMED: AtomicBool = AtomicBool::new(false);
static SERVICE_PENDING: AtomicU8 = AtomicU8::new(0xff);
static STATE: Mutex<State> = Mutex::new(State::new());

fn emit(args: std::fmt::Arguments<'_>) {
    crate::logging::diagnostic(args);
}
fn aligned(p: usize) -> bool {
    p != 0 && p.is_multiple_of(8)
}
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
unsafe fn rd<T: Copy>(p: usize, offset: usize) -> T {
    std::ptr::read_volatile((p + offset) as *const T)
}
fn mode() -> Option<game_mode::Mode> {
    game_mode::current().filter(|m| m.initialized && m.melee == 0x20 && m.mode == 0x04040000)
}
fn eligible() -> bool {
    ARMED.load(Ordering::Acquire)
        && crate::character_lifecycle::native_hooks_ready()
        && mode().is_some()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Input {
    kind_tag: u64,
    charinfo_hash: u64,
    outfit: u64,
    hat: u64,
    colour: u8,
    voice: u8,
    specials: [u8; 4],
    skip: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    kind: i8,
    owner: u32,
    selector: u8,
    tokens: [u16; 5],
    specials: [u8; 4],
    charinfo_hash: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pool {
    holder: usize,
    g: usize,
    begin: usize,
    end: usize,
    capacity: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Snapshot {
    mode: game_mode::Mode,
    input: Input,
    row: Row,
    pool: Option<Pool>,
    record_hash: Option<u64>,
    resources: Option<[usize; 2]>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ticket {
    epoch: usize,
    destruction: usize,
    thread: usize,
    sp: usize,
    owner: u32,
    index: u32,
    r: usize,
    e: usize,
    caller: usize,
    native_mii: bool,
}
#[derive(Clone, Copy)]
struct Pending {
    ticket: Ticket,
    before: Result<Snapshot, &'static str>,
    poisoned: bool,
}
struct State {
    epoch: usize,
    destruction: usize,
    seen: u8,
    paired: u8,
    serviced: u8,
    visits: [u16; 8],
    post: [Option<Row>; 8],
    pending: Option<Pending>,
    anomalous: bool,
    filtered: [u16; 2],
}
impl State {
    const fn new() -> Self {
        Self {
            epoch: 0,
            destruction: 0,
            seen: 0,
            paired: 0,
            serviced: 0,
            visits: [0; 8],
            post: [None; 8],
            pending: None,
            anomalous: false,
            filtered: [0; 2],
        }
    }
}

unsafe fn input(r: usize) -> Input {
    let raw: [u8; 0x58] = rd(r, 0xe0);
    Input {
        kind_tag: rd(r, 0x88),
        charinfo_hash: fnv(&raw),
        outfit: rd(r, 0xc8),
        hat: rd(r, 0xd0),
        colour: rd(r, 0xd8),
        voice: rd(r, 0xd9),
        specials: rd(r, 0xda),
        skip: rd(r, 0x456),
    }
}
unsafe fn row(e: usize) -> Row {
    let raw: [u8; 0x58] = rd(e, 0x150);
    Row {
        kind: rd(e, 8),
        owner: rd(e, 0x14),
        selector: rd(e, 0xa7),
        tokens: rd(e, 0x1a8),
        specials: rd(e, 0x1b2),
        charinfo_hash: fnv(&raw),
    }
}
unsafe fn manager(base: usize) -> Result<(usize, usize), &'static str> {
    let holder: usize = rd(base, HOLDER);
    if !aligned(holder) {
        return Err("holder_null_or_alignment");
    }
    let g: usize = rd(holder, 0);
    if !aligned(g) {
        return Err("G_null_or_alignment");
    }
    Ok((holder, g))
}
struct PoolLock {
    g: usize,
    unlock: unsafe extern "C" fn(usize),
}
impl Drop for PoolLock {
    fn drop(&mut self) {
        unsafe {
            (self.unlock)(self.g);
        }
    }
}

/// r/e are freshly supplied by the current inline context, never from a saved observation.
/// Only one 0x68-byte owned record copy under G: no hashing/logging/allocation/native lookup.
unsafe fn capture(
    base: usize,
    r: usize,
    e: usize,
    owner: u32,
    index: u32,
    native_mii: bool,
) -> Result<Snapshot, &'static str> {
    if owner >= 8 || e != base + E_ROWS + owner as usize * 0x260 || !aligned(r) {
        return Err("current_argument_identity");
    }
    if INSTALL.active() {
        return Err("plugin_INSTALL_active");
    }
    let current_mode = mode().ok_or("native_mode_unqualified")?;
    let before_input = input(r);
    let before_row = row(e);
    // Native classifier bit 0 controls 1788a94; R456 controls 1788d18. Only this
    // branch itself later takes G's lock. Ordinary and skipped rows never touch G here.
    if !native_mii || before_input.skip != 0 {
        if input(r) != before_input || row(e) != before_row || mode() != Some(current_mode) {
            return Err("arguments_changed_during_copy");
        }
        return Ok(Snapshot {
            mode: current_mode,
            input: before_input,
            row: before_row,
            pool: None,
            record_hash: None,
            resources: None,
        });
    }
    let identity = manager(base)?;
    let mut raw = [0u8; STRIDE];
    let pool;
    {
        let lock: unsafe extern "C" fn(usize) = std::mem::transmute(base + LOCK);
        let unlock: unsafe extern "C" fn(usize) = std::mem::transmute(base + UNLOCK);
        lock(identity.1);
        let _guard = PoolLock {
            g: identity.1,
            unlock,
        };
        if manager(base)? != identity {
            return Err("G_changed_during_lock");
        }
        let begin: usize = rd(identity.1, 0x24d0);
        let end: usize = rd(identity.1, 0x24d8);
        let capacity: usize = rd(identity.1, 0x24e0);
        if !aligned(begin)
            || end.checked_sub(begin) != Some(RECORDS * STRIDE)
            || capacity < end
            || capacity
                .checked_sub(begin)
                .is_none_or(|n| !n.is_multiple_of(STRIDE))
            || index as usize >= RECORDS
        {
            return Err("unknown_record_or_vector");
        }
        let at = begin
            .checked_add(index as usize * STRIDE)
            .ok_or("record_overflow")?;
        for (i, byte) in raw.iter_mut().enumerate() {
            *byte = rd(at, i);
        }
        pool = Pool {
            holder: identity.0,
            g: identity.1,
            begin,
            end,
            capacity,
        };
    }
    if manager(base)? != identity
        || input(r) != before_input
        || row(e) != before_row
        || mode() != Some(current_mode)
    {
        return Err("identity_or_arguments_changed_during_copy");
    }
    Ok(Snapshot {
        mode: current_mode,
        input: before_input,
        row: before_row,
        pool: Some(pool),
        record_hash: Some(fnv(&raw[..0x58])),
        resources: Some([
            usize::from_le_bytes(raw[0x58..0x60].try_into().unwrap()),
            usize::from_le_bytes(raw[0x60..0x68].try_into().unwrap()),
        ]),
    })
}

// After the native classifier, before ANY E write. This is a mid-function boundary,
// not an invented Rust return ABI. x20/x19/x23 retain original R/E/owner; SP+8 retains
// supplied index, SP+c holds the classifier bit, SP+168 is the ACTUAL caller LR
// saved by the prologue. This first E store does not consume prior condition flags.
#[skyline::hook(offset = 0x1788160, inline)]
unsafe fn before(ctx: &mut InlineCtx) {
    if !ARMED.load(Ordering::Acquire) {
        return;
    }
    let owner = ctx.registers[23].w();
    let hooks_ready = crate::character_lifecycle::native_hooks_ready();
    let current_mode = game_mode::current();
    let training =
        current_mode.is_some_and(|m| m.initialized && m.melee == 0x20 && m.mode == 0x04040000);
    if !hooks_ready || !training {
        // Scalars only: no stack, R/E, holder, pool or owner pointer dereferences.
        // One receipt per reason/owner (plus one unknown-owner bucket) per generation.
        let reason = if !hooks_ready { 0 } else { 1 };
        let (epoch, report) = {
            let mut s = STATE.lock().unwrap();
            let bit = 1u16 << owner.min(8);
            let report = s.filtered[reason] & bit == 0;
            s.filtered[reason] |= bit;
            (s.epoch, report)
        };
        if report {
            emit(format_args!("[MiiOrigin] filtered epoch={epoch} owner={owner} hooks_ready={hooks_ready} mode={current_mode:x?} reason={} r={:#x} e={:#x} sp={:#x} resolved_kind={:#x}",
            if reason == 0 { "hooks_not_ready" } else { "mode_unqualified" },
            ctx.registers[20].x(), ctx.registers[19].x(), ctx.sp.x(), ctx.registers[25].w()));
        }
        return;
    }
    let stack = ctx.sp.x() as usize;
    if stack == 0 || !stack.is_multiple_of(16) {
        return;
    }
    let index: u32 = rd(stack, 8);
    let caller: usize = rd(stack, 0x168);
    let native_mii = rd::<u32>(stack, 0xc) & 1 != 0;
    let thread = skyline::nn::os::GetCurrentThread() as usize;
    let ticket = {
        let mut s = STATE.lock().unwrap();
        if owner >= 8 {
            let report = !s.anomalous;
            s.anomalous = true;
            drop(s);
            if report {
                emit(format_args!(
                    "[MiiOrigin] refused reason=owner_out_of_range owner={owner}"
                ));
            }
            return;
        }
        let bit = 1u8 << owner;
        if let Some(p) = s.pending.as_mut() {
            p.poisoned = true;
            let report = s.seen & bit == 0;
            s.seen |= bit;
            drop(s);
            if report {
                emit(format_args!(
                    "[MiiOrigin] refused reason=reentrant_or_concurrent owner={owner}"
                ));
            }
            return;
        }
        if s.seen & bit != 0 {
            return;
        }
        s.seen |= bit;
        let ticket = Ticket {
            epoch: s.epoch,
            destruction: s.destruction,
            thread,
            sp: stack,
            owner,
            index,
            r: ctx.registers[20].x() as usize,
            e: ctx.registers[19].x() as usize,
            caller,
            native_mii,
        };
        s.pending = Some(Pending {
            ticket,
            before: Err("capture_incomplete"),
            poisoned: false,
        });
        ticket
    };
    let base = BASE.load(Ordering::Acquire);
    let result = if ticket.thread == 0 || ticket.sp == 0 || !ticket.sp.is_multiple_of(16) {
        Err("entry_stack_or_thread_invalid")
    } else {
        capture(
            base,
            ticket.r,
            ticket.e,
            owner,
            ticket.index,
            ticket.native_mii,
        )
    };
    let current = {
        let mut s = STATE.lock().unwrap();
        if let Some(p) = s.pending.as_mut().filter(|p| p.ticket == ticket) {
            p.before = result;
            true
        } else {
            false
        }
    };
    emit(format_args!("[MiiOrigin] before epoch={} owner={owner} ticket={ticket:x?} caller_main={:?} current={current} snapshot={result:x?}",
        ticket.epoch, ticket.caller.checked_sub(base)));
}

// Unique normal epilogue, BEFORE restoring x19/x20/x30 and adding 0x170 to native SP.
// The original prologue saved w2 at SP+8 and retains R=x20/E=x19 for the entire function.
#[skyline::hook(offset = 0x1788e58, inline)]
unsafe fn after(ctx: &mut InlineCtx) {
    if !ARMED.load(Ordering::Acquire) {
        return;
    }
    let Some(p) = STATE.lock().unwrap().pending else {
        return;
    };
    let thread = skyline::nn::os::GetCurrentThread() as usize;
    let stack = ctx.sp.x() as usize;
    if thread != p.ticket.thread || stack != p.ticket.sp {
        let report = {
            let mut s = STATE.lock().unwrap();
            if let Some(p) = s.pending.as_mut() {
                p.poisoned = true;
            }
            let report = !s.anomalous;
            s.anomalous = true;
            report
        };
        if report {
            emit(format_args!("[MiiOrigin] refused reason=unmatched_epilogue epoch={} owner={} thread={thread:#x} sp={stack:#x}", p.ticket.epoch, p.ticket.owner));
        }
        return;
    }
    let base = BASE.load(Ordering::Acquire);
    // Fresh native context and native stack, compared to numeric identities only.
    let r = ctx.registers[20].x() as usize;
    let e = ctx.registers[19].x() as usize;
    let index: u32 = rd(stack, 8);
    let return_lr: usize = rd(stack, 0x168);
    let result = if p.poisoned {
        Err("reentrant_or_unmatched")
    } else if p.before.is_err() {
        Err("before_refused")
    } else if r != p.ticket.r
        || e != p.ticket.e
        || index != p.ticket.index
        || return_lr != p.ticket.caller
    {
        Err("epilogue_arguments_changed")
    } else if p.before.is_ok_and(|b| input(r) != b.input) {
        Err("epilogue_input_changed")
    } else if !eligible() {
        Err("scene_or_hooks_changed")
    } else {
        capture(base, r, e, p.ticket.owner, index, p.ticket.native_mii)
    };
    let pair_valid = match (p.before, result) {
        (Ok(a), Ok(b)) => {
            a.mode == b.mode
                && a.pool == b.pool
                && a.input == b.input
                && b.row.owner == p.ticket.owner
                && p.ticket.native_mii == (0x48..=0x4a).contains(&b.row.kind)
        }
        _ => false,
    };
    let current = {
        let mut s = STATE.lock().unwrap();
        let current = s.epoch == p.ticket.epoch
            && s.destruction == p.ticket.destruction
            && s.pending
                .is_some_and(|now| now.ticket == p.ticket && !now.poisoned);
        if s.pending.is_some_and(|now| now.ticket == p.ticket) {
            s.pending = None;
        }
        if current && pair_valid {
            s.paired |= 1u8 << p.ticket.owner;
            s.post[p.ticket.owner as usize] = result.ok().map(|b| b.row);
        }
        current
    };
    let copy_matches = result.ok().and_then(|b| {
        ((0x48..=0x4a).contains(&b.row.kind) && b.input.skip == 0).then_some(
            b.row.tokens[0] == index as u16
                && b.row.charinfo_hash == b.input.charinfo_hash
                && b.record_hash == Some(b.input.charinfo_hash),
        )
    });
    emit(format_args!("[MiiOrigin] after epoch={} owner={} current={current} pair_valid={} return_lr={return_lr:#x} copy_matches={copy_matches:?} snapshot={result:x?}",
        p.ticket.epoch, p.ticket.owner, current && pair_valid));
}

/// At most one current READY association per owner/generation. No pool scan here. The atomic
/// mask makes later idle callbacks return immediately; missing converter events remain explicit.
pub(super) unsafe fn service(entry: *const u8, owner: u32) {
    if owner >= 8 || SERVICE_PENDING.load(Ordering::Acquire) & (1u8 << owner) == 0 || !eligible() {
        return;
    }
    let (epoch, destruction, timed_out) = {
        let mut s = STATE.lock().unwrap();
        s.visits[owner as usize] += 1;
        (
            s.epoch,
            s.destruction,
            s.visits[owner as usize] > MAX_SERVICE_VISITS,
        )
    };
    if timed_out {
        {
            let s = STATE.lock().unwrap();
            if s.epoch != epoch || s.destruction != destruction {
                return;
            }
            SERVICE_PENDING.fetch_and(!(1u8 << owner), Ordering::AcqRel);
        }
        emit(format_args!("[MiiOrigin] service_refused epoch={epoch} owner={owner} reason=READY_observation_budget"));
        return;
    }
    if INSTALL.active() {
        return;
    }
    let kind = rd::<u32>(entry as usize, 0x18);
    let ready = if (0x48..=0x4a).contains(&kind) {
        ready_single_mii(entry, owner)
    } else {
        ready_kind(entry, owner)
    };
    if ready != Some(kind) {
        return;
    }
    let base = BASE.load(Ordering::Acquire);
    let e = base + E_ROWS + owner as usize * 0x260;
    let actual = row(e);
    let fe_index: u16 = rd(entry as usize, 0x46);
    let d: usize = rd(entry as usize, 0xf8);
    let table: usize = rd(base, OWNER_TABLE);
    let object: usize = if aligned(table) {
        rd(table, 0xe8 + owner as usize * 8)
    } else {
        0
    };
    let owner_index = aligned(object).then(|| rd::<u16>(object, 0x68));
    let mirrors = if (0x48..=0x4a).contains(&kind) && aligned(d) && aligned(object) {
        Some(Mirrors::decode(
            rd(e, 0x1a8),
            rd(entry as usize, 0x46),
            rd(d, 0x72),
            rd(d, 0x390),
            rd(object, 0x68),
        ))
    } else {
        None
    };
    let current = row(e) == actual
        && rd::<u32>(entry as usize, 0xe0) == owner
        && rd::<u32>(entry as usize, 0x18) == kind
        && rd::<usize>(entry as usize, 0xf8) == d
        && rd::<usize>(base, OWNER_TABLE) == table
        && (!aligned(table) || rd::<usize>(table, 0xe8 + owner as usize * 8) == object)
        && mode().is_some();
    let (seen, paired, same_row) = {
        let mut s = STATE.lock().unwrap();
        if s.epoch != epoch || s.destruction != destruction {
            return;
        }
        s.serviced |= 1u8 << owner;
        SERVICE_PENDING.fetch_and(!(1u8 << owner), Ordering::AcqRel);
        (
            s.seen & (1u8 << owner) != 0,
            s.paired & (1u8 << owner) != 0,
            s.post[owner as usize].map(|r| r == actual),
        )
    };
    emit(format_args!("[MiiOrigin] service epoch={epoch} destruction={destruction} owner={owner} current={current} converter_seen={seen} converter_paired={paired} same_row={same_row:?} fe={:#x} kind={kind:#x} fe_index={fe_index} d={d:#x} row={actual:x?} owner_table={table:#x} object={object:#x} owner_index={owner_index:?} mirrors={mirrors:x?}", entry as usize));
}

fn invalidate(reason: &str, owner: Option<u32>) {
    if !ARMED.load(Ordering::Acquire) {
        return;
    }
    let (old, epoch, destruction, seen, paired, serviced, pending, filtered) = {
        let mut s = STATE.lock().unwrap();
        if owner.is_some() {
            s.destruction += 1;
        }
        let old = s.epoch;
        let receipt = (
            s.seen,
            s.paired,
            s.serviced,
            s.pending.map(|p| p.ticket.owner),
            s.filtered,
        );
        s.pending = None;
        if owner.is_none() || owner == Some(0) {
            let destruction = s.destruction;
            *s = State::new();
            s.epoch = old + 1;
            s.destruction = destruction;
            SERVICE_PENDING.store(0xff, Ordering::Release);
        }
        (
            old,
            s.epoch,
            s.destruction,
            receipt.0,
            receipt.1,
            receipt.2,
            receipt.3,
            receipt.4,
        )
    };
    emit(format_args!("[MiiOrigin] invalidate reason={reason} owner={owner:?} old_epoch={old} epoch={epoch} destruction={destruction} seen={seen:#x} paired={paired:#x} serviced={serviced:#x} incomplete_owner={pending:?} filtered_masks={filtered:x?}"));
}
pub(super) fn scene_changed() {
    invalidate("scene", None);
}
pub(super) fn on_destroy(owner: u32) {
    invalidate("native_entry_destroy", Some(owner));
}

pub(super) fn init(base: usize) {
    if !std::path::Path::new(ARM).exists() {
        emit(format_args!("[MiiOrigin] armed=false"));
        return;
    }
    unsafe {
        for (offset, len, expected) in CODE {
            if fnv(std::slice::from_raw_parts(
                (base + offset) as *const u8,
                len,
            )) != expected
            {
                emit(format_args!(
                    "[MiiOrigin] armed=false fingerprint_mismatch={offset:#x}"
                ));
                return;
            }
        }
        skyline::install_hooks!(before, after);
    }
    ARMED.store(true, Ordering::Release);
    emit(format_args!("[MiiOrigin] armed=true readonly=true before_configuration=1788160 epilogue=1788e58 frame=170 max_pairs_per_owner_epoch=1"));
}
