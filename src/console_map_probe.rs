//! Private, one-shot failure diagnostic. Never changes a native result or context.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const PATH: &[u8] = b"sd:/ultimate/TrainingModpack/console-map.bin\0";
const RECORD: usize = 4096;
static FILE: AtomicU64 = AtomicU64::new(0);
static MAIN: AtomicU64 = AtomicU64::new(0);
static SDK: AtomicU64 = AtomicU64::new(0);
static INSTALLED: AtomicU64 = AtomicU64::new(0);
static FAILED: AtomicBool = AtomicBool::new(false);
static VALIDATE: AtomicBool = AtomicBool::new(false);
static VALIDATED: AtomicBool = AtomicBool::new(false);

#[repr(C)]
#[derive(Default)]
struct MemoryInfo {
    address: u64, size: u64, kind: u32, attributes: u32,
    permission: u32, ipc: u32, device: u32, padding: u32,
}

unsafe fn query(address: u64, info: &mut MemoryInfo) -> (u32, u32) {
    let result: u64;
    let page: u64;
    core::arch::asm!("svc 0x6", inout("x0") info as *mut MemoryInfo as u64 => result,
        lateout("x1") page, in("x2") address, clobber_abi("C"), options(nostack));
    (result as u32, page as u32)
}

fn put32(row: &mut [u8; RECORD], at: usize, value: u32) {
    row[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(row: &mut [u8; RECORD], at: usize, value: u64) {
    row[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
unsafe fn mapping(row: &mut [u8; RECORD], index: usize, address: u64) -> Option<u64> {
    let mut info = MemoryInfo::default();
    let (result, page) = query(address, &mut info);
    let at = 384 + index * 56;
    put64(row, at, address); put32(row, at + 8, result); put32(row, at + 12, page);
    put64(row, at + 16, info.address); put64(row, at + 24, info.size);
    for (n, value) in [info.kind, info.attributes, info.permission, info.ipc, info.device, info.padding].iter().enumerate() {
        put32(row, at + 32 + n * 4, *value);
    }
    if result != 0 || info.address > address { return None; }
    info.address.checked_add(info.size).filter(|end| *end > address)
}

fn finish(row: &mut [u8; RECORD]) {
    let mut crc = !0u32;
    for byte in &row[..RECORD - 4] {
        crc ^= u32::from(*byte);
        for _ in 0..8 { crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1))); }
    }
    put32(row, RECORD - 4, !crc);
}
unsafe fn write_record(row: &mut [u8; RECORD], slot: i64) -> u32 {
    finish(row);
    // Explicit offsets: no shared seek cursor, std formatting, resize or Rust
    // allocator. The preopened handle remains valid until process termination.
    skyline::nn::fs::WriteFile(
        skyline::nn::fs::FileHandle { handle: FILE.load(Ordering::Acquire) },
        slot * RECORD as i64, row.as_ptr(), RECORD as u64,
        &skyline::nn::fs::WriteOption { flags: 1 },
    )
}
fn header(row: &mut [u8; RECORD], kind: u32, api: u32, result: u32) {
    row[..8].copy_from_slice(b"CMAP8500");
    put32(row, 8, 1); put32(row, 12, kind); put32(row, 16, api); put32(row, 20, result);
    let tick: u64;
    unsafe { core::arch::asm!("mrs {tick}, cntpct_el0", tick = out(reg) tick, options(nomem, nostack)); }
    put64(row, 24, tick); put64(row, 32, MAIN.load(Ordering::Relaxed));
    put64(row, 40, SDK.load(Ordering::Relaxed)); put64(row, 120, INSTALLED.load(Ordering::Relaxed));
}

unsafe fn capture(ctx: &skyline::hooks::InlineCtx, api: u32) {
    let result = ctx.registers[0].w();
    let (kind, slot) = if result != 0 {
        if FAILED.swap(true, Ordering::Relaxed) { return; }
        (1, 2)
    } else {
        if !VALIDATE.load(Ordering::Relaxed) || VALIDATED.swap(true, Ordering::Relaxed) { return; }
        (2, 1)
    };
    let mut row = [0; RECORD];
    header(&mut row, kind, api, result);
    let start = ctx.registers[20].x();
    let size = ctx.registers[if api == 1 { 22 } else { 21 }].x();
    let pc = if api == 1 { SDK.load(Ordering::Relaxed) + 0x3398b0 } else { MAIN.load(Ordering::Relaxed) + 0x37cae24 };
    put64(&mut row, 48, pc); put64(&mut row, 56, start); put64(&mut row, 64, size);
    if api == 1 {
        put64(&mut row, 72, 8); put64(&mut row, 80, ctx.registers[21].x());
        put64(&mut row, 88, ctx.registers[19].x());
    }
    put64(&mut row, 104, ctx.sp.x());
    for (n, register) in ctx.registers.iter().enumerate() { put64(&mut row, 128 + n * 8, register.x()); }
    // The saved caller is optional; verify the native frame is readable first.
    if let Some(address) = ctx.registers[29].x().checked_add(8) {
        let mut info = MemoryInfo::default();
        if query(address, &mut info).0 == 0 && info.permission & 1 != 0 && info.address <= address
            && address.checked_add(8).zip(info.address.checked_add(info.size)).is_some_and(|(end, limit)| end <= limit) {
            put64(&mut row, 96, (address as *const u64).read_unaligned());
        }
    }
    let mut count = 0;
    let mut stop = 2; // range overflow
    if let Some(end) = start.checked_add(size) {
        let mut cursor = start;
        stop = 0; // covered
        while cursor < end && count < 22 {
            let next = mapping(&mut row, count, cursor); count += 1;
            match next { Some(next) => cursor = next, None => { stop = 3; break; } }
        }
        if stop == 0 && cursor < end { stop = 1; } // bounded/truncated
        mapping(&mut row, 23, end);
    }
    if start > 0 { mapping(&mut row, 22, start - 1); }
    put32(&mut row, 112, count as u32); put32(&mut row, 116, stop);
    // nn::fs may itself fail: the native abort remains authoritative, and a
    // missing/invalid-CRC record is never interpreted as a successful capture.
    let _ = write_record(&mut row, slot);
}

unsafe extern "C" fn attribute_result(ctx: &skyline::hooks::InlineCtx) { capture(ctx, 1); }
unsafe extern "C" fn free_pages_result(ctx: &skyline::hooks::InlineCtx) { capture(ctx, 2); }

unsafe fn matches(address: usize, expected: &[u32]) -> bool {
    expected.iter().enumerate().all(|(n, word)| ((address + n * 4) as *const u32).read_volatile() == *word)
}

pub fn init() {
    unsafe {
        let main = skyline::hooks::getRegionAddress(skyline::hooks::Region::Text) as usize;
        let mut entry = 0usize;
        skyline::nn::ro::LookupSymbol(&mut entry, b"_ZN2nn2os6detail22SetMemoryAttributeImplEmmNS0_15MemoryAttributeE\0".as_ptr());
        // Exact instruction pins derived from the matching SDK dump. Do not
        // chain unknown hooks or infer a module base from an unchecked symbol.
        if entry == 0 || !matches(entry, &ATTRIBUTE_WORDS) || entry < 0x339830 {
            crate::logging::diagnostic(format_args!("[ConsoleMap] SDK fingerprint mismatch; probe not installed")); return;
        }
        let sdk = entry - 0x339830;
        if !matches(sdk + 0x3897fc, &[0xd4000061, 0xd65f03c0])
            || !matches(main + 0x37cae24, &[0x35000640, 0xf0010b88, 0x91114108, 0xc85ffd09, 0xcb150129]) {
            crate::logging::diagnostic(format_args!("[ConsoleMap] syscall/free-pages fingerprint mismatch; probe not installed")); return;
        }
        let path = "sd:/ultimate/TrainingModpack/console-map.bin";
        let Ok(file) = std::fs::OpenOptions::new().create(true).truncate(true).write(true).open(path) else { return; };
        if file.set_len((RECORD * 3) as u64).is_err() { return; }
        drop(file);
        let mut handle = skyline::nn::fs::FileHandle { handle: 0 };
        if skyline::nn::fs::OpenFile(&mut handle, PATH.as_ptr(), 2) != 0 || handle.handle == 0 { return; }
        FILE.store(handle.handle, Ordering::Release); MAIN.store(main as u64, Ordering::Relaxed); SDK.store(sdk as u64, Ordering::Relaxed);
        VALIDATE.store(std::path::Path::new("sd:/ultimate/TrainingModpack/console-map.validate").exists(), Ordering::Relaxed);
        // Switch file extension does not guarantee zero-filled unwritten bytes.
        // Initialize all slots before any callback can observe this handle.
        let mut row = [0; RECORD];
        for slot in 0..3 {
            if skyline::nn::fs::WriteFile(handle, slot * RECORD as i64, row.as_ptr(),
                RECORD as u64, &skyline::nn::fs::WriteOption { flags: 1 }) != 0 {
                skyline::nn::fs::CloseFile(handle); FILE.store(0, Ordering::Release); return;
            }
        }
        header(&mut row, 0, 0, 0);
        if write_record(&mut row, 0) != 0 { skyline::nn::fs::CloseFile(handle); FILE.store(0, Ordering::Release); return; }
        skyline::hooks::A64InlineHook((sdk + 0x3398b0) as *const _, attribute_result as *const () as *const _);
        INSTALLED.fetch_or(1, Ordering::Relaxed);
        skyline::hooks::A64InlineHook((main + 0x37cae24) as *const _, free_pages_result as *const () as *const _);
        INSTALLED.fetch_or(2, Ordering::Relaxed);
        header(&mut row, 0, 0, 0); let _ = write_record(&mut row, 0);
        crate::logging::diagnostic(format_args!("[ConsoleMap] installed; one failure, native result unchanged; validation={}", VALIDATE.load(Ordering::Relaxed)));
        crate::console_load_probe::init(main, sdk);
    }
}

// Complete matching SDK function, including both native abort branches.
const ATTRIBUTE_WORDS: [u32; 56] = [
    0xd101c3ff, 0xa90457f6, 0xa9054ff4, 0xa9067bfd, 0x910183fd, 0x2a0203f5,
    0xaa0103f3, 0xaa0003f4, 0x34000082, 0x710006bf, 0x54000501, 0x321d03f5,
    0xb4000313, 0x910063e0, 0x910043e1, 0xaa1403e2, 0x94013fe9, 0xb9000be0,
    0x35000300, 0xa941a7e8, 0xcb140108, 0x8b090108, 0xeb13011f, 0x9a888276,
    0xb9402fe8, 0x6b15011f, 0x540000e0, 0x321d03e2, 0xaa1403e0, 0xaa1603e1,
    0x2a1503e3, 0x94013fd4, 0x35000120, 0x8b1402d4, 0xeb160273, 0x54fffd41,
    0xa9467bfd, 0xa9454ff4, 0xa94457f6, 0x9101c3ff, 0xd65f03c0, 0xb9000be0,
    0xb00035a0, 0x9134b800, 0x910023e4, 0xaa0003e1, 0xaa0003e2, 0x2a1f03e3,
    0xaa0003e5, 0x9407a437, 0xb00035a0, 0x9134b800, 0xaa0003e1, 0x2a1f03e2,
    0x9407a806, 0x00000000,
];
