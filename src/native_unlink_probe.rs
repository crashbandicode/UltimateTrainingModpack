//! Opt-in observer for the native old-body unlink crash. Its only synchronous
//! I/O is one bounded snapshot. The separate native-unlink-guard feature adds
//! an experimental missing-parent store guard; the corrupting writer is unknown.
use crate::native_unlink_core::{Correlator, NodeHistory, Observation, ParentEvent, ParentHistory};
use skyline::hooks::InlineCtx;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const PATH: &str = "sd:/ultimate/TrainingModpack/native-unlink.bin";
const NATIVE_PATH: &[u8] = b"sd:/ultimate/TrainingModpack/native-unlink.bin\0";
const SESSION_BYTES: u64 = 4096;
const CAP: u64 = 8 * 1024 * 1024;
const MAGIC: u64 = u64::from_le_bytes(*b"UNLINK01");
static READY: AtomicBool = AtomicBool::new(false);
static ARMED: AtomicBool = AtomicBool::new(false);
static FIXTURE: AtomicBool = AtomicBool::new(false);
static HEADER_FIXTURE: AtomicBool = AtomicBool::new(false);
static CLAIMED: AtomicBool = AtomicBool::new(false);
static LOCK: AtomicBool = AtomicBool::new(false);
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
static BUSY: AtomicU64 = AtomicU64::new(0);
static MATCHED: AtomicU64 = AtomicU64::new(0);
static FILE: AtomicU64 = AtomicU64::new(0);
static OFFSET: AtomicU64 = AtomicU64::new(0);
static MAIN: AtomicU64 = AtomicU64::new(0);
static HISTORY_LOCK: AtomicBool = AtomicBool::new(false);
static HISTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static HISTORY_BUSY: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "native-unlink-guard")]
static GUARD_EVENTS: AtomicU64 = AtomicU64::new(0);
struct History(UnsafeCell<ParentHistory<4096>>);
// Fixed 384 KiB. Constructor/parent-store callbacks never wait or perform I/O.
unsafe impl Sync for History {}
static HISTORY: History = History(UnsafeCell::new(ParentHistory::new()));
struct Table(UnsafeCell<Correlator>);
// Access is exclusively protected by LOCK; callbacks never wait for it.
unsafe impl Sync for Table {}
static TABLE: Table = Table(UnsafeCell::new(Correlator::new()));
#[repr(align(4096))]
struct Output(UnsafeCell<[u64; 512]>);
// Init uses this before installing hooks. Thereafter only CLAIMED's winner can
// access it. No file write or 4 KiB buffer lives under the correlator lock.
unsafe impl Sync for Output {}
static OUTPUT: Output = Output(UnsafeCell::new([0; 512]));

#[inline]
fn clock() -> (u64, u64) {
    let tick: u64;
    let thread: u64;
    unsafe {
        std::arch::asm!("mrs {tick}, cntpct_el0", "mrs {thread}, tpidrro_el0",
            tick=out(reg) tick, thread=out(reg) thread, options(nomem, nostack, preserves_flags));
    }
    (tick, thread)
}

pub fn arm(entry: usize, owner: u32, from: u32, to: u32) {
    if READY.load(Ordering::Acquire) {
        crate::practice_trace::checkpoint(1207, [entry as u64, owner as u64, from as u64, to as u64, 0]);
        ARMED.store(true, Ordering::Release);
    }
}

fn emit(code: u64, observation: &Observation, pair: u64, site: u64) {
    // Sequence and native TLS are in both halves: queue admission/loss is not
    // assumed atomic. Pair is the entry sequence, or zero for missing evidence.
    crate::practice_trace::event(code, [observation.sequence, observation.thread, observation.stack, observation.node, pair]);
    crate::practice_trace::event(code + 1, [observation.sequence, observation.links[0], observation.links[1], observation.links[2], observation.links[3]]);
    crate::practice_trace::event(code + 2, [observation.sequence, observation.links[4], observation.links[5], site, 0]);
}

fn put_observation(words: &mut [u64], record: &Observation) {
    words[..5].copy_from_slice(&[record.sequence, record.tick, record.thread, record.stack, record.node]);
    words[5..11].copy_from_slice(&record.links);
}

fn put_history(words: &mut [u64], record: &NodeHistory) {
    words[..4].copy_from_slice(&[record.node, record.owner, record.birth, record.birth_tick]);
    let old = &record.previous;
    words[4..8].copy_from_slice(&[old.tick, old.thread, old.site, old.parent]);
    let last = &record.latest;
    words[8..12].copy_from_slice(&[last.tick, last.thread, last.site, last.parent]);
}

#[inline(never)]
unsafe fn parent_written(node: u64, owner: u64, site: u64, birth: bool) {
    if !READY.load(Ordering::Acquire) || node < 0x10000 || node & 7 != 0 { return; }
    let (tick, thread) = clock();
    let sequence = HISTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    // Store sites are AFTER the native parent/owner store. The separately
    // identified walker-entry site is a read observation, not a write witness.
    // Read only the live node's parent, never follow it.
    let event = ParentEvent { tick, thread, site,
        parent: std::ptr::read_volatile(node as *const u64) };
    if HISTORY_LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
        let table = &mut *HISTORY.0.get();
        if matches!(site, 0x38958a4 | 0x3895148) {
            table.record_read(node, event);
        } else {
            table.record(node, owner, if birth { sequence } else { 0 }, event);
        }
        HISTORY_LOCK.store(false, Ordering::Release);
    } else {
        HISTORY_BUSY.fetch_add(1, Ordering::Relaxed);
    }
}

unsafe extern "C" fn born(ctx: &InlineCtx) {
    parent_written(ctx.registers[19].x(), ctx.registers[8].x(), 0x3896bbc, true);
}
unsafe extern "C" fn cleared(ctx: &InlineCtx) {
    parent_written(ctx.registers[19].x(), 0, 0x3895904, false);
}
unsafe extern "C" fn sister_cleared(ctx: &InlineCtx) {
    parent_written(ctx.registers[19].x(), 0, 0x3895874, false);
}
unsafe extern "C" fn header_cleared(ctx: &InlineCtx) {
    parent_written(ctx.registers[21].x(), 0, 0x38951fc, false);
}
unsafe extern "C" fn vector_cleared(ctx: &InlineCtx) {
    parent_written(ctx.registers[21].x(), 0, 0x38952cc, false);
}

// Fixed 512-byte record. Last two words are FNV-1a checksum and its complement;
// decoder rejects torn writes. nn::fs WriteOption(1) completes a flush, unlike
// the ordinary recorder's asynchronous checkpoint request.
fn persist(reason: u64, entry: &Observation, current: &Observation, losses: [u64; 3], site: u64, owner: u64) {
    if CLAIMED.swap(true, Ordering::AcqRel) {
        return;
    }
    let words = unsafe { &mut (&mut *OUTPUT.0.get())[..64] };
    words.fill(0);
    words[..8].copy_from_slice(&[MAGIC, 3, 512, reason, MAIN.load(Ordering::Relaxed),
        MATCHED.load(Ordering::Relaxed), FIXTURE.load(Ordering::Relaxed) as u64, entry.sequence]);
    words[8..11].copy_from_slice(&losses);
    put_observation(&mut words[12..23], entry);
    put_observation(&mut words[24..35], current);
    words[35] = u64::from_le_bytes(*b"HISTORY1");
    words[48] = HISTORY_BUSY.load(Ordering::Relaxed);
    words[51] = HISTORY_SEQUENCE.load(Ordering::Relaxed);
    // Only the one-shot anomaly/fixture path retries. A preempted writer must
    // not block the faulting task indefinitely; retain explicit missing status.
    let mut history_acquired = false;
    for _ in 0..4096 {
        if HISTORY_LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            history_acquired = true;
            break;
        }
        std::hint::spin_loop();
    }
    if history_acquired {
        // The successful CAS above exclusively owns this UnsafeCell.
        let table = unsafe { &*HISTORY.0.get() };
        let record = table.get(current.node);
        put_history(&mut words[36..48], &record);
        words[49] = table.replaced;
        words[50] = table.missing_birth;
        words[52] = u64::from(record.node != 0);
        HISTORY_LOCK.store(false, Ordering::Release);
    } else {
        words[52] = 2; // Bounded retry exhausted: history remains unknown.
    }
    words[53] = site;
    words[54] = owner; // Header destructor's live x19; zero for recursive walker.
    let bytes = unsafe { std::slice::from_raw_parts(words.as_ptr().cast::<u8>(), 62 * 8) };
    let mut checksum = 0xcbf29ce484222325u64;
    for byte in bytes { checksum = (checksum ^ u64::from(*byte)).wrapping_mul(0x100000001b3); }
    words[62] = checksum;
    words[63] = !checksum;
    let handle = skyline::nn::fs::FileHandle { handle: FILE.load(Ordering::Relaxed) };
    let result = unsafe {
        skyline::nn::fs::WriteFile(handle, (OFFSET.load(Ordering::Relaxed) + 512) as i64,
            words.as_ptr().cast::<u8>(), 512, &skyline::nn::fs::WriteOption { flags: 1 })
    };
    // Zero means the write/flush RETURNED successfully. Missing event remains
    // unknown; the checksummed file is independent evidence of retained bytes.
    crate::practice_trace::checkpoint(1208, [reason, result as u64, entry.sequence, current.sequence, losses[0]]);
}

#[inline(never)]
unsafe fn observe(ctx: &InlineCtx, entering: bool, header: bool) {
    let node = ctx.registers[if header { 21 } else { 19 }].x();
    let site = match (header, entering) {
        (false, true) => 0x38958a4,
        (false, false) => 0x38958e4,
        (true, true) => 0x3895148,
        (true, false) => 0x38951c4,
    };
    if entering {
        // Retain valid earlier walker visits even before the first swap is armed.
        // Invalid entries are left for the anomaly snapshot, not inserted here.
        parent_written(node, 0, site, false);
    }
    if !ARMED.load(Ordering::Acquire) { return; }
    let (tick, thread) = clock();
    let mut current = Observation {
        sequence: SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1,
        tick, thread, stack: ctx.sp.x(), node, links: [0; 6],
    };
    // The exact native sites receive a live 0x60-byte node and access these
    // same intrusive fields. Read only that node, never follow its links.
    // A zero/misaligned node is recorded without a diagnostic dereference.
    if current.node >= 0x10000 && current.node & 7 == 0 {
        for (i, value) in current.links[..5].iter_mut().enumerate() {
            *value = std::ptr::read_volatile((current.node as *const u64).add(i));
        }
        // Backlink is diagnostic-only; omit it if it lies on another page.
        current.links[5] = if (current.node + 0x28) & 0xfff != 0 {
            std::ptr::read_volatile((current.node as *const u64).add(5))
        } else { u64::MAX };
    }
    let mut entry = Observation::EMPTY;
    let mut losses = [BUSY.load(Ordering::Relaxed), u64::MAX, u64::MAX];
    if LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
        let table = &mut *TABLE.0.get();
        if entering { table.enter(current); } else { entry = table.leave(&current); }
        losses[1] = table.overwritten;
        losses[2] = table.unmatched;
        LOCK.store(false, Ordering::Release);
    } else {
        losses[0] = BUSY.fetch_add(1, Ordering::Relaxed) + 1;
    }
    if entering { entry = current; }
    if !entering && entry.sequence != 0 { MATCHED.fetch_add(1, Ordering::Relaxed); }
    if current.sequence <= 8192 {
        emit(if entering { 1200 } else { 1203 }, &current, entry.sequence, site);
    }
    let owner = if header { ctx.registers[19].x() } else { 0 };
    if current.links[0] == 0 {
        let reason = match (header, entering) {
            (false, true) => 2,
            (false, false) => 3,
            (true, true) => 4,
            (true, false) => 5,
        };
        persist(reason, &entry, &current, losses, site, owner);
    } else if !entering && entry.sequence != 0 && FIXTURE.load(Ordering::Relaxed)
        && (!HEADER_FIXTURE.load(Ordering::Relaxed) || header)
    {
        // Explicit validator file only: proves a synchronous normal paired
        // snapshot without corrupting game memory or claiming a reproduced fault.
        persist(1, &entry, &current, losses, site, owner);
    }
}

unsafe extern "C" fn enter(ctx: &InlineCtx) { observe(ctx, true, false); }
unsafe extern "C" fn leave(ctx: &InlineCtx) {
    observe(ctx, false, false);
    #[cfg(feature = "native-unlink-guard")]
    guarded_store(ctx, 0x38958e4, false, false);
}
unsafe extern "C" fn header_enter(ctx: &InlineCtx) { observe(ctx, true, true); }
unsafe extern "C" fn header_leave(ctx: &InlineCtx) { observe(ctx, false, true); }

#[cfg(feature = "native-unlink-guard")]
unsafe fn guarded_store(ctx: &InlineCtx, site: u64, add_sixteen: bool, header_first: bool) {
    let node = ctx.registers[if matches!(site, 0x38951dc | 0x38951f4 | 0x38952a8 | 0x38952c4) { 21 } else { 19 }].x();
    let address = ctx.registers[9].x().wrapping_add(if add_sixteen { 16 } else { 0 });
    let value = ctx.registers[8].x();
    let action = crate::native_unlink_guard::store(address, value, node,
        if header_first { ctx.registers[19].x() } else { 0 }, header_first);
    if action != 0 {
        // Diagnostic accounting only; no formatting or file I/O in the guard.
        // Existing anomaly capture remains responsible for the one durable packet.
        let count = GUARD_EVENTS.fetch_add(1, Ordering::Relaxed) + 1;
        if count <= 64 {
            crate::practice_trace::checkpoint(1209, [site, node, address, action, count]);
        }
    }
}

#[cfg(feature = "native-unlink-guard")]
unsafe extern "C" fn recursive_last_guard(ctx: &InlineCtx) { guarded_store(ctx, 0x38958fc, true, false); }
#[cfg(feature = "native-unlink-guard")]
unsafe extern "C" fn sister_first_guard(ctx: &InlineCtx) { guarded_store(ctx, 0x3895854, false, false); }
#[cfg(feature = "native-unlink-guard")]
unsafe extern "C" fn sister_last_guard(ctx: &InlineCtx) { guarded_store(ctx, 0x389586c, true, false); }
#[cfg(feature = "native-unlink-guard")]
unsafe extern "C" fn header_first_guard(ctx: &InlineCtx) { guarded_store(ctx, 0x38951dc, false, true); }
#[cfg(feature = "native-unlink-guard")]
unsafe extern "C" fn header_last_guard(ctx: &InlineCtx) { guarded_store(ctx, 0x38951f4, true, false); }
#[cfg(feature = "native-unlink-guard")]
unsafe extern "C" fn vector_first_guard(ctx: &InlineCtx) { guarded_store(ctx, 0x38952a8, false, false); }
#[cfg(feature = "native-unlink-guard")]
unsafe extern "C" fn vector_last_guard(ctx: &InlineCtx) { guarded_store(ctx, 0x38952c4, false, false); }

#[cfg(feature = "native-unlink-guard")]
const GUARDED_STORES: [(usize, [u32; 3]); 8] = [
    (0x38958e4, [0x91002129, 0xf9000128, 0xa941a668]),
    (0x38958fc, [0xf9400269, 0xf9000928, 0xf900027f]),
    (0x3895854, [0x91002129, 0xf9000128, 0xa941a668]),
    (0x389586c, [0xf9400269, 0xf9000928, 0xf900027f]),
    (0x38951dc, [0x91002129, 0xf9000128, 0xa941a6a8]),
    (0x38951f4, [0xf94002a9, 0xf9000928, 0xf90002bf]),
    (0x38952a8, [0x91002129, 0xf9000128, 0xa941a6a8]),
    (0x38952c4, [0x91004129, 0xf9000128, 0xf90002bf]),
];

pub fn init() {
    if READY.load(Ordering::Acquire) { return; }
    let main = unsafe { skyline::hooks::getRegionAddress(skyline::hooks::Region::Text) as usize };
    // x16/x17 and NZCV are dead at every accepted site. Skyline does not
    // preserve them. Require the surrounding store/load windows as well as
    // each displaced word, because the post-store meaning depends on them.
    for (offset, words) in [
        (0x38958a4, [0xaa0003f3u32, 0xf8408e80, 0xb4000040]),
        (0x38958e4, [0x91002129, 0xf9000128, 0xa941a668]),
        (0x3895148, [0x92800014, 0xaa1403e9, 0xaa1503ea]),
        (0x38951c4, [0x940001b4, 0xa941a2a9, 0xb4000069]),
        (0x3896bbc, [0xf9001668, 0x39410268, 0xb900327c]),
        (0x38952cc, [0xf90002bf, 0xa9017edf, 0xa9007edf]),
        (0x3895904, [0xf900027f, 0xa9017e9f, 0xa9007e9f]),
        (0x3895874, [0xf900027f, 0xa9017e9f, 0xa9007e9f]),
        (0x38951fc, [0xf90002bf, 0xa9017edf, 0xa9007edf]),
    ] {
        for (index, word) in words.iter().enumerate() {
            if unsafe { std::ptr::read_volatile((main + offset - 4 + index * 4) as *const u32) } != *word {
                crate::logging::diagnostic(format_args!("[NativeUnlink] refused window at {offset:#x}"));
                return;
            }
        }
    }
    #[cfg(feature = "native-unlink-guard")]
    for (offset, words) in GUARDED_STORES {
        for (index, word) in words.iter().enumerate() {
            if unsafe { std::ptr::read_volatile((main + offset - 4 + index * 4) as *const u32) } != *word {
                crate::logging::diagnostic(format_args!("[NativeUnlink] guard refused window at {offset:#x}"));
                return;
            }
        }
    }
    let prepared = (|| -> Result<(skyline::nn::fs::FileHandle, u64), u64> {
        let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(PATH)
            .map_err(|_| 0x100000001u64)?;
        drop(file);
        let mut handle = skyline::nn::fs::FileHandle { handle: 0 };
        let result = unsafe { skyline::nn::fs::OpenFile(&mut handle, NATIVE_PATH.as_ptr(), 3) };
        if result != 0 { return Err(result as u64); }
        let mut length = 0i64;
        let result = unsafe { skyline::nn::fs::GetFileSize(&mut length, handle) };
        if result != 0 || length < 0 || !(length as u64).is_multiple_of(SESSION_BYTES) || length as u64 + SESSION_BYTES > CAP {
            unsafe { skyline::nn::fs::CloseFile(handle); }
            return Err(0x100000002);
        }
        let result = unsafe { skyline::nn::fs::SetFileSize(handle, length + SESSION_BYTES as i64) };
        if result != 0 {
            unsafe { skyline::nn::fs::CloseFile(handle); }
            return Err(result as u64);
        }
        unsafe { skyline::nn::fs::CloseFile(handle); }
        let result = unsafe { skyline::nn::fs::OpenFile(&mut handle, NATIVE_PATH.as_ptr(), 3) };
        if result != 0 { return Err(result as u64); }
        // Preinitialize the complete session before hooks can observe anything.
        // Retain a source identity independently of the asynchronous trace.
        let words = unsafe { &mut *OUTPUT.0.get() };
        words.fill(0);
        words[..5].copy_from_slice(&[MAGIC, 3, SESSION_BYTES, main as u64, clock().0]);
        if let Some(build) = crate::practice_trace_core::parse_build_id(option_env!("PRACTICE_TRACE_BUILD_ID").unwrap_or("")) {
            let bytes = unsafe { std::slice::from_raw_parts_mut(words.as_mut_ptr().add(8).cast::<u8>(), 32) };
            bytes.copy_from_slice(&build);
        } else {
            unsafe { skyline::nn::fs::CloseFile(handle); }
            return Err(0x100000003);
        }
        let result = unsafe { skyline::nn::fs::WriteFile(handle, length, words.as_ptr().cast::<u8>(), SESSION_BYTES,
            &skyline::nn::fs::WriteOption { flags: 1 }) };
        if result != 0 {
            unsafe { skyline::nn::fs::CloseFile(handle); }
            return Err(result as u64);
        }
        Ok((handle, length as u64))
    })();
    let (handle, offset) = match prepared {
        Ok(value) => value,
        Err(error) => {
            crate::logging::diagnostic(format_args!("[NativeUnlink] open refused {error:#x}"));
            return;
        }
    };
    FILE.store(handle.handle, Ordering::Relaxed);
    OFFSET.store(offset, Ordering::Relaxed);
    MAIN.store(main as u64, Ordering::Relaxed);
    FIXTURE.store(std::path::Path::new("sd:/ultimate/TrainingModpack/native-unlink-validate.flag").exists(), Ordering::Relaxed);
    HEADER_FIXTURE.store(std::fs::read("sd:/ultimate/TrainingModpack/native-unlink-validate.flag")
        .is_ok_and(|bytes| bytes.starts_with(b"header")), Ordering::Relaxed);
    // This diagnostic retains one process-owned handle until process shutdown.
    // No callback accesses a closed handle; normal OS process teardown closes it.
    unsafe {
        #[cfg(feature = "native-unlink-guard")]
        for (offset, _) in GUARDED_STORES {
            // The callback executes the displaced STR conditionally. NOP is
            // copied into Skyline's trampoline so the STR never executes twice.
            // Init is quiescent; failure must not leave partially patched code running.
            if skyline::patching::Patch::in_text(offset).nop().is_err() {
                std::process::abort();
            }
        }
        for (offset, callback) in [
            (0x38958a4, enter as unsafe extern "C" fn(&InlineCtx)), (0x38958e4, leave),
            (0x3895148, header_enter), (0x38951c4, header_leave),
            (0x3896bbc, born), (0x38952cc, vector_cleared), (0x3895904, cleared),
            (0x3895874, sister_cleared), (0x38951fc, header_cleared),
        ] {
            skyline::hooks::A64InlineHook((main + offset) as *const _, callback as *const _);
            if std::ptr::read_volatile((main + offset) as *const u32) & 0xfc000000 != 0x14000000 {
                std::process::abort(); // Long patch can overwrite internal branch targets.
            }
        }
        #[cfg(feature = "native-unlink-guard")]
        for (offset, callback) in [
            (0x38958fc, recursive_last_guard as unsafe extern "C" fn(&InlineCtx)),
            (0x3895854, sister_first_guard), (0x389586c, sister_last_guard),
            (0x38951dc, header_first_guard), (0x38951f4, header_last_guard),
            (0x38952a8, vector_first_guard), (0x38952c4, vector_last_guard),
        ] {
            skyline::hooks::A64InlineHook((main + offset) as *const _, callback as *const _);
            if std::ptr::read_volatile((main + offset) as *const u32) & 0xfc000000 != 0x14000000 {
                std::process::abort();
            }
        }
    }
    READY.store(true, Ordering::Release);
    crate::logging::diagnostic(format_args!("[NativeUnlink] ready offset={offset} fixture={}", FIXTURE.load(Ordering::Relaxed)));
    #[cfg(feature = "native-unlink-guard")]
    crate::logging::diagnostic(format_args!("[NativeUnlink] experimental missing-parent guard enabled; writer unknown"));
}
