//! Private startup observation. Never changes a native argument, result or page.
//! Pre-call work uses fixed storage; filesystem IPC runs only after the SDK's
//! virtual RO method has consumed its reply. Source images are never read then.
//! Each NRR slot keeps a second fixed row: the pre-registration hash list is
//! copied there and written only after the native reply.
use std::cell::UnsafeCell;
use std::convert::TryInto;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

const RECORD: usize = 4096;
const RECORDS: usize = 64;
const ITEM_SIZE: u64 = 0xcbb000;
const NRR_LIMIT: u64 = 0x10000;
const HASH_MAX: u32 = 112;
const LIST_AT: usize = 160;
// 0 complete, 1 overflow (only the first 112 of a readable list), 2 truncated, 3 malformed.
const LIST_COMPLETE: u32 = 0;
const LIST_OVERFLOW: u32 = 1;
const LIST_TRUNCATED: u32 = 2;
const LIST_MALFORMED: u32 = 3;
const PATH: &str = "sd:/ultimate/TrainingModpack/console-load.bin";
const PREVIOUS: &str = "sd:/ultimate/TrainingModpack/console-load.previous.bin";
static FILE: AtomicU64 = AtomicU64::new(0);
static MAIN: AtomicU64 = AtomicU64::new(0);
static SDK: AtomicU64 = AtomicU64::new(0);
static NEXT: AtomicU32 = AtomicU32::new(1);
static DROPPED: AtomicU32 = AtomicU32::new(0);
static WRITING: AtomicBool = AtomicBool::new(false);
static ITEM_ATTEMPTS: AtomicU32 = AtomicU32::new(0);
static GENERATION: AtomicU32 = AtomicU32::new(1);

struct Pending { key: AtomicU64, row: UnsafeCell<[u8; RECORD]> }
struct NrrPending { key: AtomicU64, row: UnsafeCell<[u8; RECORD]>, hashes: UnsafeCell<[u8; RECORD]> }
// key=0 is free, key=1 exclusively owned by a callback, otherwise it is a
// published native output pointer. Release/acquire transfers the byte buffer.
unsafe impl Sync for Pending {}
unsafe impl Sync for NrrPending {}
impl Pending {
    const fn new() -> Self { Self { key: AtomicU64::new(0), row: UnsafeCell::new([0; RECORD]) } }
}
impl NrrPending {
    const fn new() -> Self {
        Self { key: AtomicU64::new(0), row: UnsafeCell::new([0; RECORD]), hashes: UnsafeCell::new([0; RECORD]) }
    }
}
static NRR: [NrrPending; 4] = [const { NrrPending::new() }; 4];
static ITEM: Pending = Pending::new();

#[repr(C)]
#[derive(Default)]
struct MemoryInfo { address: u64, size: u64, kind: u32, attributes: u32,
    permission: u32, ipc: u32, device: u32, padding: u32 }
unsafe fn query(address: u64, info: &mut MemoryInfo) -> u32 {
    let result: u64;
    core::arch::asm!("svc 0x6", inout("x0") info as *mut MemoryInfo as u64 => result,
        lateout("x1") _, in("x2") address, clobber_abi("C"), options(nostack));
    result as u32
}
unsafe fn readable(start: u64, size: u64) -> bool {
    let Some(end) = start.checked_add(size) else { return false; };
    if start == 0 || size == 0 { return false; }
    let mut cursor = start;
    for _ in 0..32 {
        let mut info = MemoryInfo::default();
        if query(cursor, &mut info) != 0 || info.address > cursor || info.permission & 1 == 0 { return false; }
        let Some(next) = info.address.checked_add(info.size).filter(|next| *next > cursor) else { return false; };
        if next >= end { return true; }
        cursor = next;
    }
    false
}
fn put32(row: &mut [u8], at: usize, value: u32) { row[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
fn put64(row: &mut [u8], at: usize, value: u64) { row[at..at + 8].copy_from_slice(&value.to_le_bytes()); }
fn get32(row: &[u8], at: usize) -> u32 { u32::from_le_bytes(row[at..at + 4].try_into().unwrap()) }
fn get64(row: &[u8], at: usize) -> u64 { u64::from_le_bytes(row[at..at + 8].try_into().unwrap()) }
fn tick() -> u64 {
    let value: u64;
    unsafe { core::arch::asm!("mrs {value}, cntpct_el0", value = out(reg) value, options(nomem, nostack)); }
    value
}
unsafe fn hash(address: u64, size: u64, out: &mut [u8]) {
    skyline::nn::crypto::GenerateSha256Hash(out.as_mut_ptr(), 32, address as *const u8, size);
}
fn begin(row: &mut [u8; RECORD], event: u32, address: u64, size: u64, output: u64) {
    row.fill(0);
    row[..8].copy_from_slice(b"CLOAD920");
    put32(row, 8, 1); put32(row, 12, event); put32(row, 20, u32::MAX);
    put64(row, 24, tick()); put64(row, 40, MAIN.load(Ordering::Relaxed));
    put64(row, 48, SDK.load(Ordering::Relaxed)); put64(row, 56, address);
    put64(row, 64, size); put64(row, 72, output);
}
unsafe fn flush(row: &mut [u8; RECORD]) -> bool {
    let slot = NEXT.fetch_add(1, Ordering::Relaxed);
    if slot >= RECORDS as u32 || WRITING.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        DROPPED.fetch_add(1, Ordering::Relaxed); return false;
    }
    put32(row, 16, slot); put64(row, 32, tick()); put32(row, 88, DROPPED.load(Ordering::Relaxed));
    let mut crc = !0u32;
    for byte in &row[..RECORD - 4] {
        crc ^= u32::from(*byte);
        for _ in 0..8 { crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1))); }
    }
    put32(row, RECORD - 4, !crc);
    let result = skyline::nn::fs::WriteFile(skyline::nn::fs::FileHandle { handle: FILE.load(Ordering::Acquire) },
        i64::from(slot) * RECORD as i64, row.as_ptr(), RECORD as u64, &skyline::nn::fs::WriteOption { flags: 1 });
    if result != 0 { DROPPED.fetch_add(1, Ordering::Relaxed); }
    WRITING.store(false, Ordering::Release);
    result == 0
}
unsafe fn mappings(row: &mut [u8; RECORD]) {
    let start = get64(row, 56);
    let Some(end) = start.checked_add(get64(row, 64)) else { put32(row, 2524, 1); return; };
    let mut cursor = start;
    let mut count = 0;
    while cursor < end && count < 24 {
        let mut info = MemoryInfo::default();
        let result = query(cursor, &mut info);
        let at = 2560 + count * 56;
        put64(row, at, cursor); put32(row, at + 8, result);
        put64(row, at + 16, info.address); put64(row, at + 24, info.size);
        for (n, value) in [info.kind, info.attributes, info.permission, info.ipc, info.device, info.padding].iter().enumerate() {
            put32(row, at + 32 + n * 4, *value);
        }
        count += 1;
        if result != 0 || info.address > cursor { break; }
        let Some(next) = info.address.checked_add(info.size).filter(|next| *next > cursor) else { break; };
        cursor = next;
    }
    put32(row, 2520, count as u32); put32(row, 2524, u32::from(cursor < end));
}

struct RegistryGuard { sdk: usize }
impl Drop for RegistryGuard {
    fn drop(&mut self) {
        // The pinned register/unregister functions use this exact MutexType
        // and UnlockMutex. This guard exists only after TryLockMutex succeeds.
        unsafe {
            let unlock: unsafe extern "C" fn(*mut skyline::nn::os::MutexType) = core::mem::transmute(self.sdk + 0x330b20);
            unlock((self.sdk + 0xc09b40) as *mut _);
        }
    }
}
// The pinned SDK maintains RegistrationInfo nodes at info+0x10, protected by
// its MutexType at SDK+c09b40. Use its verified nonblocking TryLockMutex:
// QueryMemory alone would not prevent a concurrent unregister/free. Do no
// filesystem IPC while holding this lock; skip a contended snapshot explicitly.
unsafe fn registrations(row: &mut [u8; RECORD]) {
    let sdk = SDK.load(Ordering::Relaxed) as usize;
    let try_lock: unsafe extern "C" fn(*mut skyline::nn::os::MutexType) -> bool = core::mem::transmute(sdk + 0x330a90);
    if !try_lock((sdk + 0xc09b40) as *mut _) { put32(row, 2484, 7); return; }
    let _guard = RegistryGuard { sdk };
    let head = sdk as u64 + 0xcccca0;
    if !readable(head, 16) { put32(row, 2484, 1); return; }
    let first = (head as *const u64).read_unaligned();
    // The sentinel has its own RegistrationInfo at head-0x10. SDK unregister
    // treats it specially, so it may itself carry the game's authorization NRR.
    let mut node = head;
    let mut count = 0;
    let mut stop = 0;
    while count < 16 {
        let Some(info) = node.checked_sub(16) else { stop = 2; break; };
        if !readable(info, 32) { stop = 3; break; }
        let nrr = ((info + 8) as *const u64).read_unaligned();
        let state = (info as *const u32).read_unaligned();
        let next = (node as *const u64).read_unaligned();
        let at = 320 + count * 128;
        put64(row, at, info); put64(row, at + 8, nrr); put32(row, at + 16, state);
        put32(row, at + 36, u32::MAX); put64(row, at + 72, node); put64(row, at + 80, next);
        let mut flags = if node == head { 17 } else { 1 };
        if state == 1 && readable(nrr, 0x350) {
            flags |= 2;
            let size = ((nrr + 0x338) as *const u32).read_unaligned();
            let offset = ((nrr + 0x340) as *const u32).read_unaligned();
            let hashes = ((nrr + 0x344) as *const u32).read_unaligned();
            put32(row, at + 24, size); put32(row, at + 28, offset); put32(row, at + 32, hashes);
            if (nrr as *const u32).read_unaligned() == 0x3052524e && (0x350..=NRR_LIMIT).contains(&u64::from(size))
                && offset >= 0x350 && u64::from(offset) + u64::from(hashes) * 32 <= u64::from(size)
                && readable(nrr, u64::from(size)) {
                flags |= 4;
                hash(nrr, u64::from(size), &mut row[at + 40..at + 72]);
                for index in 0..hashes {
                    let candidate = core::slice::from_raw_parts((nrr + u64::from(offset) + u64::from(index) * 32) as *const u8, 32);
                    if candidate == &row[128..160] { put32(row, at + 36, index); flags |= 8; break; }
                }
            }
        }
        put32(row, at + 20, flags);
        count += 1;
        if next == head { node = head; break; }
        if next == node { stop = 4; break; }
        node = next;
    }
    if node != head && stop == 0 { stop = 5; }
    let after = (head as *const u64).read_unaligned();
    if after != first { stop = 6; }
    put32(row, 2480, count as u32); put32(row, 2484, stop);
    put64(row, 2488, first); put64(row, 2496, after);
}

unsafe extern "C" fn item_pre(ctx: &skyline::hooks::InlineCtx) {
    let image = ctx.registers[21].x();
    let size = ctx.registers[24].x();
    // This is the observed item's exact declared size, before any RO IPC.
    if size != ITEM_SIZE || ITEM_ATTEMPTS.fetch_add(1, Ordering::Relaxed) >= 2 { return; }
    let key = ctx.registers[19].x();
    if key <= 1 || ITEM.key.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_err() {
        DROPPED.fetch_add(1, Ordering::Relaxed); return;
    }
    let row = &mut *ITEM.row.get();
    begin(row, 3, image, size, key);
    put64(row, 96, ctx.registers[20].x()); put64(row, 104, ctx.registers[25].x());
    if readable(image, 0x80) {
        core::ptr::copy_nonoverlapping(image as *const u8, row.as_mut_ptr().add(160), 0x80);
        put32(row, 80, 1);
        if readable(image, size) {
            hash(image, size, &mut row[128..160]); put32(row, 80, 3);
            registrations(row);
        }
    }
    ITEM.key.store(key, Ordering::Release);
}
unsafe extern "C" fn item_post(ctx: &skyline::hooks::InlineCtx) {
    let key = ctx.registers[19].x();
    if key <= 1 || ITEM.key.compare_exchange(key, 1, Ordering::Acquire, Ordering::Relaxed).is_err() { return; }
    let row = &mut *ITEM.row.get();
    put32(row, 20, ctx.registers[0].w());
    mappings(row); let _ = flush(row);
    ITEM.key.store(0, Ordering::Release);
}
// Copy the hash list while the pre-registration mapping is still readable.
// The bytes stay in this slot's second fixed row until nrr_post, which is
// after the native reply. No filesystem and no further NRR read happens there.
unsafe fn fill_hash_list(address: u64, size: u64, output: u64, generation: u32, into: &mut [u8; RECORD]) {
    into.fill(0);
    into[..8].copy_from_slice(b"CLOAD920");
    put32(into, 8, 2); put32(into, 12, 4); put32(into, 20, u32::MAX); put32(into, 100, u32::MAX);
    put64(into, 24, tick()); put64(into, 40, MAIN.load(Ordering::Relaxed));
    put64(into, 48, SDK.load(Ordering::Relaxed)); put64(into, 56, address);
    put64(into, 64, size); put64(into, 72, output); put32(into, 96, generation);
    if address == 0 || !readable(address, 0x350) { put32(into, 112, LIST_TRUNCATED); return; }
    let magic = (address as *const u32).read_unaligned();
    let hdr = ((address + 0x338) as *const u32).read_unaligned();
    let offset = ((address + 0x340) as *const u32).read_unaligned();
    let count = ((address + 0x344) as *const u32).read_unaligned();
    put32(into, 84, count); put32(into, 104, offset); put32(into, 108, hdr);
    let span = u64::from(count).checked_mul(32).and_then(|bytes| u64::from(offset).checked_add(bytes));
    let bounds = magic == 0x3052_524e && u64::from(hdr) == size && (0x350..=NRR_LIMIT).contains(&size)
        && offset >= 0x350 && span.is_some_and(|span| span <= size);
    if !bounds { put32(into, 112, LIST_MALFORMED); return; }
    let span = span.unwrap();
    let cap = count.min(HASH_MAX);
    if readable(address, span) {
        if cap > 0 {
            core::ptr::copy_nonoverlapping((address + u64::from(offset)) as *const u8,
                into.as_mut_ptr().add(LIST_AT), (cap as usize) * 32);
        }
        put32(into, 92, cap); put32(into, 80, u32::from(cap > 0));
        put32(into, 112, if count > HASH_MAX { LIST_OVERFLOW } else { LIST_COMPLETE });
        return;
    }
    let mut stored = 0u32;
    while stored < cap {
        let at = address + u64::from(offset) + u64::from(stored) * 32;
        if !readable(at, 32) { break; }
        core::ptr::copy_nonoverlapping(at as *const u8, into.as_mut_ptr().add(LIST_AT + (stored as usize) * 32), 32);
        stored += 1;
    }
    put32(into, 92, stored); put32(into, 80, u32::from(stored > 0)); put32(into, 112, LIST_TRUNCATED);
}
unsafe extern "C" fn nrr_pre(ctx: &skyline::hooks::InlineCtx) {
    let key = ctx.registers[19].x();
    if key <= 1 || NEXT.load(Ordering::Relaxed) >= RECORDS as u32 { return; }
    let Some(slot) = NRR.iter().find(|slot| slot.key.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok()) else {
        DROPPED.fetch_add(1, Ordering::Relaxed); return;
    };
    let row = &mut *slot.row.get();
    let hashes = &mut *slot.hashes.get();
    let address = ctx.registers[1].x();
    let size = ctx.registers[8].x();
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    begin(row, 1, address, size, key);
    fill_hash_list(address, size, key, generation, hashes);
    put32(row, 96, generation); put32(row, 100, get32(hashes, 84)); put32(row, 104, get32(hashes, 104));
    put32(row, 108, get32(hashes, 108)); put32(row, 112, get32(hashes, 112)); put32(row, 116, get32(hashes, 92));
    if readable(address, 0x350) {
        core::ptr::copy_nonoverlapping(address as *const u8, row.as_mut_ptr().add(160), 0x350);
        put32(row, 80, 1);
        if (0x350..=NRR_LIMIT).contains(&size) && readable(address, size) {
            hash(address, size, &mut row[128..160]); put32(row, 80, 3);
        }
    }
    slot.key.store(key, Ordering::Release);
}
unsafe extern "C" fn nrr_post(ctx: &skyline::hooks::InlineCtx) {
    let key = ctx.registers[19].x();
    if key <= 1 { return; }
    let Some(slot) = NRR.iter().find(|slot| slot.key.compare_exchange(key, 1, Ordering::Acquire, Ordering::Relaxed).is_ok()) else { return; };
    let result = ctx.registers[0].w();
    let (paired, ticks) = {
        let row = &mut *slot.row.get();
        put32(row, 20, result); mappings(row);
        let kept = flush(row);
        (if kept { get32(row, 16) } else { u32::MAX }, get64(row, 24))
    };
    let hashes = &mut *slot.hashes.get();
    put32(hashes, 20, result); put32(hashes, 100, paired); put64(hashes, 24, ticks);
    let _ = flush(hashes);
    slot.key.store(0, Ordering::Release);
}
unsafe extern "C" fn nrr_unregister(ctx: &skyline::hooks::InlineCtx) {
    if NEXT.load(Ordering::Relaxed) >= RECORDS as u32 { return; }
    let info = ctx.registers[19].x();
    let mut row = [0; RECORD];
    begin(&mut row, 2, 0, 0, info);
    put32(&mut row, 20, ctx.registers[0].w());
    if readable(info, 16) {
        put64(&mut row, 56, ((info + 8) as *const u64).read_unaligned());
        put32(&mut row, 96, (info as *const u32).read_unaligned());
        put32(&mut row, 80, 1);
    }
    let _ = flush(&mut row);
}

pub(super) unsafe fn init(main: usize, sdk: usize) {
    // The retained plugin set already detours LoadModuleInternal's first four
    // instructions. Run58 observed this exact 16-byte ldr/br/literal shape;
    // every other word below matched. We patch only internal pre/post sites,
    // never replace or invoke the entry detour. Require the original entry or
    // this bounded executable-target shape, then verify the entire remaining
    // body (including its native argument setup and stack accesses).
    let entry = sdk + 0x347460;
    let words = core::slice::from_raw_parts(entry as *const u32, 4);
    let entry_target = if words == [0xd101c3ff, 0xf9000bfb, 0xa90267fa, 0xa9035ff8] {
        0
    } else if words[0] == 0x58000051 && words[1] == 0xd61f0220 {
        let target = ((entry + 8) as *const u64).read_unaligned();
        let mut info = MemoryInfo::default();
        if target == 0 || target & 3 != 0 || query(target, &mut info) != 0
            || info.permission & 5 != 5 || info.address > target
            || target.checked_add(4).zip(info.address.checked_add(info.size))
                .is_none_or(|(end, limit)| end > limit) {
            crate::logging::diagnostic(format_args!("[ConsoleLoad] invalid entry detour target; skipped")); return;
        }
        target
    } else {
        crate::logging::diagnostic(format_args!("[ConsoleLoad] unknown entry instructions; skipped")); return;
    };
    for (offset, words) in PINS {
        for (n, expected) in words.iter().enumerate() {
            let at = sdk + offset + n * 4;
            let actual = (at as *const u32).read_volatile();
            if actual != *expected {
                crate::logging::diagnostic(format_args!(
                    "[ConsoleLoad] SDK fingerprint mismatch offset={:#x} expected={:#x} actual={:#x}; skipped",
                    at - sdk, expected, actual));
                return;
            }
        }
    }
    // Retain exactly one prior launch, including a successful boot immediately
    // preceding a crash. Never destroy the current capture if rotation fails.
    if std::path::Path::new(PATH).exists() {
        if std::path::Path::new(PREVIOUS).exists() && std::fs::remove_file(PREVIOUS).is_err() { return; }
        if std::fs::rename(PATH, PREVIOUS).is_err() { return; }
    }
    let Ok(file) = std::fs::OpenOptions::new().create(true).truncate(true).write(true).open(PATH) else { return; };
    if file.set_len((RECORD * RECORDS) as u64).is_err() { return; }
    drop(file);
    let mut handle = skyline::nn::fs::FileHandle { handle: 0 };
    if skyline::nn::fs::OpenFile(&mut handle, b"sd:/ultimate/TrainingModpack/console-load.bin\0".as_ptr(), 2) != 0 || handle.handle == 0 { return; }
    let mut row = [0; RECORD];
    for slot in 0..RECORDS {
        if skyline::nn::fs::WriteFile(handle, (slot * RECORD) as i64, row.as_ptr(), RECORD as u64,
            &skyline::nn::fs::WriteOption { flags: 0 }) != 0 {
            skyline::nn::fs::CloseFile(handle); return;
        }
    }
    FILE.store(handle.handle, Ordering::Release); MAIN.store(main as u64, Ordering::Relaxed); SDK.store(sdk as u64, Ordering::Relaxed);
    begin(&mut row, 0, 0, 0, 0); put64(&mut row, 96, entry_target);
    if !flush(&mut row) { skyline::nn::fs::CloseFile(handle); FILE.store(0, Ordering::Release); return; }
    for (offset, callback) in [(0x3474d4, item_pre as *const ()), (0x3474f0, item_post as *const ()),
        (0x347250, nrr_pre as *const ()), (0x347264, nrr_post as *const ()), (0x3473e8, nrr_unregister as *const ())] {
        skyline::hooks::A64InlineHook((sdk + offset) as *const _, callback as *const _);
    }
    begin(&mut row, 0, 0, 0, 0); put32(&mut row, 80, 1); put64(&mut row, 96, entry_target); let _ = flush(&mut row);
    crate::logging::diagnostic(format_args!("[ConsoleLoad] installed; bounded item/NRR observation, native results unchanged"));
}

// Matching internal SDK functions; checked before any patch. The load entry's
// four overwritten words are separately validated above; no entry patch is made.
const PINS: &[(usize, &[u32])] = &[
    (0x347220, &[
        0xf81d0ff5, 0xa9014ff4, 0xa9027bfd, 0x910083fd, 0xb9400028, 0x528a49c9,
        0x72a60a49, 0x6b09011f, 0x54000241, 0xf9400049, 0xb9433828, 0xaa0003f3,
        0xaa0203e0, 0xf9400129, 0xaa0803e2, 0xaa0103f4, 0xd63f0120, 0x12002008,
        0x7100591f, 0x54000121, 0x53095408, 0x51000508, 0x710ffd1f, 0x540000a2,
        0x35000360, 0x14000005, 0x528182c0, 0x14000018, 0xb9000be0, 0x35000360,
        0x91004275, 0xa900d674, 0xd0004614, 0x912d0294, 0x320003e8, 0xaa1403e0,
        0xb9000268, 0xf9000e75, 0x94076b96, 0xb0004c29, 0x91328129, 0xf9400a68,
        0xf940012a, 0xf9000a6a, 0xf9000509, 0xb0004c29, 0xaa1403e0, 0xf9000555,
        0xf9065128, 0x94076b8f, 0xaa1f03e0, 0xa9427bfd, 0x92407c00, 0xa9414ff4,
        0xf84307f5, 0xd65f03c0, 0xf0003520, 0x9134b800, 0x910023e4, 0xaa0003e1,
        0xaa0003e2, 0x2a1f03e3, 0xaa0003e5, 0x94076dad,
    ]),
    (0x347370, &[
        0xd100c3ff, 0xf9000bf3, 0xa9027bfd, 0x910083fd, 0xaa0003f3, 0xd0004600,
        0x912d0000, 0x94076b61, 0xb0004c28, 0x91324108, 0xeb08027f, 0x54000120,
        0xa9412668, 0xf940012a, 0xf9000509, 0xf9400a68, 0xf9000128, 0x91004268,
        0xf9000548, 0xf9000a6a, 0xd0004600, 0x912d0000, 0x94076b56, 0xb0004c28,
        0xf9464d00, 0xf9400008, 0xf9400662, 0xf9401d08, 0xaa1f03e1, 0xd63f0100,
        0x12002008, 0x7100591f, 0x54000161, 0x53095408, 0x51000508, 0x710ffd1f,
        0x540000e2, 0xb9000be0, 0x34000100, 0xf0003520, 0x9134b800, 0x910023e4,
        0x1400000c, 0xb81f83a0, 0x350000e0, 0xb9000bff, 0xb900027f, 0xa9427bfd,
        0xf9400bf3, 0x9100c3ff, 0xd65f03c0, 0xf0003520, 0x9134b800, 0xd10023a4,
        0xaa0003e1, 0xaa0003e2, 0x2a1f03e3, 0xaa0003e5, 0x94076d5e, 0x00000000,
    ]),
    (0x347470, &[
        0xa90457f6, 0xa9054ff4,
        0xa9067bfd, 0x910183fd, 0xb9401028, 0x528a49c9, 0x72a609e9, 0x6b09011f,
        0x540004e1, 0xb9401838, 0xb940383a, 0xaa0203f4, 0xaa0003f3, 0x32010be0,
        0x321903e2, 0x2a0503f7, 0x2a0403f6, 0xaa0303f9, 0xaa0103f5, 0xf9000fff,
        0x94010935, 0xb0004c28, 0xf9464d00, 0xf9400008, 0xf9401108, 0x910063e1,
        0xaa1f03e2, 0xaa1503e3, 0xaa1803e4, 0xaa1403e5, 0xaa1903e6, 0xd63f0100,
        0xf9400fe1, 0xaa0003f9, 0x52800080, 0x72b00000, 0x321903e2, 0x94010924,
        0x12002328, 0x7100591f, 0x54000121, 0x53095728, 0x51000508, 0x710ffd1f,
        0x540000a2, 0x35000b99, 0x14000005, 0x528102d9, 0x14000059, 0xb9000bf9,
        0x35000cd9, 0xf9400ff9, 0xb100073f, 0x54000ba0, 0xb9400728, 0x8b08033b,
        0xb9400368, 0x5289e9a9, 0x72a60889, 0x6b09011f, 0x54000ac1, 0x29412768,
        0x8b080360, 0x8b090368, 0x8b18035a, 0xeb000102, 0xaa1b03e8, 0x540000a0,
        0x2a1f03e1, 0x94076c4f, 0xb9400728, 0x8b080328, 0xb9401b69, 0x8b090118,
        0x120002e4, 0xa9006318, 0xf9000278, 0xb9400728, 0xb9400769, 0x8b080328,
        0x8b090103, 0xaa1803e0, 0xaa1903e1, 0xaa1a03e2, 0x9407c168, 0xf0004780,
        0xf9468400, 0x94078fb5, 0xf0004797, 0xf94692f7, 0xf94002e8, 0xf9400309,
        0xf940010a, 0xf900030a, 0xf9000528, 0xf940010a, 0x530106c1, 0xaa1803e0,
        0xf9000558, 0xf9000109, 0x9407c15c, 0xaa1803e0, 0x9407c15e, 0xf0004799,
        0xf9469739, 0xf9400328, 0xf9400516, 0xeb16011f, 0x54000100, 0xaa1603e0,
        0xaa1803e1, 0x9407c159, 0xf94006d6, 0xf9400328, 0xeb16011f, 0x54ffff41,
        0xf94002e8, 0xf9400500, 0xaa0003f6, 0xf8408ec9, 0xeb09011f, 0x54000120,
        0xaa1803e1, 0x9407c14d, 0xf94002e8, 0xf94002c0, 0xaa0003f6, 0xf8408ec9,
        0xeb09011f, 0x54ffff21, 0xf0004780, 0xf9468400, 0x94078f8c, 0xaa1f03f9,
        0x320003e8, 0xb9000a68, 0xa9015275, 0x92407f20, 0xa9467bfd, 0xa9454ff4,
        0xf9400bfb, 0xa94457f6, 0xa9435ff8, 0xa94267fa, 0x9101c3ff, 0xd65f03c0,
        0xf0003520, 0x9134b800, 0xaa0003e1, 0xaa0003e2, 0x2a1f03e3, 0x94076cd5,
        0xf0003520, 0x9134b800, 0x910023e4, 0xaa0003e1, 0xaa0003e2, 0x2a1f03e3,
        0xaa0003e5, 0x94076cb9,
    ]),
    (0x330a90, &[
        0xa9be4ff4, 0xa9017bfd, 0x910043fd, 0xd53bd068, 0xf940fd14, 0x39400408,
        0xaa0003f3, 0x34000168, 0xf9400a68, 0xeb14011f, 0x54000160, 0x91006260,
        0x94081858, 0x37000100, 0x2a1f03e0, 0xa9417bfd, 0xa8c24ff4, 0xd65f03c0,
        0x91006260, 0x94081851, 0x36000120, 0xb9400a68, 0x11000508, 0x320003e0,
        0xf9000a74, 0xb9000a68, 0xa9417bfd, 0xa8c24ff4, 0xd65f03c0, 0x2a1f03e0,
        0xa9417bfd, 0xa8c24ff4, 0xd65f03c0,
    ]),
    (0x330b20, &[
        0xb9400808, 0x71000508, 0xb9000808, 0x54000040, 0xd65f03c0, 0xf900081f,
        0x91006000, 0x14081789,
    ]),
];
