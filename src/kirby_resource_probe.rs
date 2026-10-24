//! Opt-in observations of native calls; never requests or releases a resource.

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use skyline::hooks::{getRegionAddress, InlineCtx, Region};

static LINES: AtomicUsize = AtomicUsize::new(0);
static OWNER: AtomicUsize = AtomicUsize::new(0);
static READY: AtomicU8 = AtomicU8::new(2);

fn log(args: std::fmt::Arguments<'_>) {
    if LINES.fetch_add(1, Ordering::Relaxed) < 160 {
        crate::logging::diagnostic(args);
    }
}

unsafe fn thread() -> usize {
    skyline::nn::os::GetCurrentThread() as usize
}

#[skyline::hook(offset = 0x17effa0, inline)]
unsafe fn request(ctx: &mut InlineCtx) {
    let owner = ctx.registers[0].x() as usize;
    OWNER.store(owner, Ordering::Relaxed);
    READY.store(2, Ordering::Relaxed);
    log(format_args!(
        "[KirbyProbe] request owner={:#x} variant={} kind={:#x} thread={:#x}",
        owner,
        ctx.registers[1].w(),
        ctx.registers[2].w(),
        thread()
    ));
}

#[skyline::hook(offset = 0x17e2df0, inline)]
unsafe fn register(ctx: &mut InlineCtx) {
    let key = ctx.registers[1].w();
    if key & 0x1ffff == 0x1003f {
        let id = ctx.registers[2].x() as *const u32;
        if !id.is_null() {
            log(format_args!(
                "[KirbyProbe] register owner={:#x} key={:#x} directory={:#x} thread={:#x}",
                ctx.registers[0].x(),
                key,
                id.read(),
                thread()
            ));
        }
    }
}

#[skyline::hook(offset = 0x17e2fe0, inline)]
unsafe fn release(ctx: &mut InlineCtx) {
    let key = ctx.registers[1].w();
    if key & 0x1ffff == 0x1003f {
        log(format_args!(
            "[KirbyProbe] release owner={:#x} key={:#x} thread={:#x}",
            ctx.registers[0].x(),
            key,
            thread()
        ));
    }
}

#[skyline::hook(offset = 0x17e2880)]
unsafe fn ready(owner: *mut u8) -> bool {
    let result = original!()(owner);
    if owner as usize == OWNER.load(Ordering::Relaxed)
        && READY.swap(u8::from(result), Ordering::Relaxed) != u8::from(result)
    {
        log(format_args!(
            "[KirbyProbe] ready owner={:#x} result={} thread={:#x}",
            owner as usize,
            result,
            thread()
        ));
    }
    result
}

#[skyline::hook(offset = 0xba0e80, inline)]
unsafe fn setup(ctx: &mut InlineCtx) {
    log(format_args!(
        "[KirbyProbe] setup body={:#x} slot={} kind={:#x} flags={},{} thread={:#x}",
        ctx.registers[0].x(),
        ctx.registers[1].w(),
        ctx.registers[2].w(),
        ctx.registers[3].w(),
        ctx.registers[4].w(),
        thread()
    ));
}

#[skyline::hook(offset = 0xba4520, inline)]
unsafe fn cache(ctx: &mut InlineCtx) {
    // Native next instruction reads this same pair from the current cache entry.
    let entry = ctx.registers[8].x() as *const u8;
    if !entry.is_null() {
        log(format_args!(
        "[KirbyProbe] motion owner={:#x} kind={:#x} variant={} ptr={:#x} control={:#x} thread={:#x}",
            ctx.registers[24].x(), ctx.registers[20].w(), ctx.registers[21].w(),
            entry.add(0xc0).cast::<usize>().read(),
            entry.add(0xc8).cast::<usize>().read(), thread()
        ));
    }
}

#[skyline::hook(offset = 0x17f1a60, inline)]
unsafe fn prepare(ctx: &mut InlineCtx) {
    log(format_args!(
        "[KirbyProbe] prepare owner={:#x} config={:#x} kind={:#x} costume={} entry={} sub={} thread={:#x} lr={:#x}",
        ctx.registers[0].x(), ctx.registers[1].x(), ctx.registers[2].w(),
        ctx.registers[3].w(), ctx.registers[4].w(), ctx.registers[5].w(),
        thread(), ctx.registers[30].x()
    ));
}

#[skyline::hook(offset = 0x17f26c0, inline)]
unsafe fn unprepare(ctx: &mut InlineCtx) {
    log(format_args!(
        "[KirbyProbe] unprepare owner={:#x} kind={:#x} entry={} sub={} thread={:#x}",
        ctx.registers[0].x(),
        ctx.registers[1].w(),
        ctx.registers[2].w(),
        ctx.registers[3].w(),
        thread()
    ));
}

unsafe fn tuple(ctx: &InlineCtx, operation: &str) {
    let record = ctx.registers[1].x() as *const i32;
    if !record.is_null() {
        log(format_args!(
            "[KirbyProbe] tuple_{} owner={:#x} kind={:#x} costume={} context={:#x} thread={:#x}",
            operation,
            ctx.registers[0].x(),
            record.read(),
            record.add(1).read(),
            record.add(2).read(),
            thread()
        ));
    }
}

#[skyline::hook(offset = 0x17f0b90, inline)]
unsafe fn prepare_tuple(ctx: &mut InlineCtx) {
    tuple(ctx, "prepare");
}

#[skyline::hook(offset = 0x17f1360, inline)]
unsafe fn release_tuple(ctx: &mut InlineCtx) {
    tuple(ctx, "release");
}

pub fn init() {
    unsafe {
        // Resolve the existing lazy signature before our entry hook replaces it.
        // Otherwise the first shortcut would search already-patched instructions.
        if *crate::common::offsets::OFFSET_COPY_SETUP != 0xba0e80 {
            log(format_args!(
                "[KirbyProbe] refused: copy setup address mismatch"
            ));
            return;
        }
        let base = getRegionAddress(Region::Text) as usize;
        let expected = [
            (0x17effa0, 0xd10583ff),
            (0x17e2df0, 0xd10203ff),
            (0x17e2fe0, 0xa9bd57f6),
            (0x17e2880, 0xa9ba6ffc),
            (0xba0e80, 0xfc190fe8),
            (0xba4520, 0xa94c2109),
            (0x17f1a60, 0xd10243ff),
            (0x17f26c0, 0xd101c3ff),
            (0x17f0b90, 0xd10683ff),
            (0x17f1360, 0xd10203ff),
        ];
        if expected
            .iter()
            .any(|&(offset, word)| ((base + offset as usize) as *const u32).read() != word)
        {
            log(format_args!(
                "[KirbyProbe] refused: native instruction mismatch"
            ));
            return;
        }
        skyline::install_hooks!(
            request,
            register,
            release,
            ready,
            setup,
            cache,
            prepare,
            unprepare,
            prepare_tuple,
            release_tuple
        );
        log(format_args!(
            "[KirbyProbe] installed read-only native observations"
        ));
    }
}
