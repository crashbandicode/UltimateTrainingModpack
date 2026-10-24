//! Private continuous recorder. Disabled builds compile event/checkpoint/scope away.
//! Exclude this module and practice_trace_core from generated source instrumentation.
//! Event IDs and persistence/loss guarantees: change-trace98/RECORDER_FORMAT.md.
#[cfg(not(feature = "console-practice-trace"))]
#[inline(always)]
pub fn event(_code: u64, _args: [u64; 5]) {}
#[cfg(not(feature = "console-practice-trace"))]
#[inline(always)]
pub fn checkpoint(_code: u64, _args: [u64; 5]) {}
#[cfg(not(feature = "console-practice-trace"))]
pub fn init() {}

#[cfg(feature = "console-practice-trace")]
pub use enabled::{checkpoint, event, init};

/// Private trace UI bridge, intentionally outside generated instrumentation.
/// 0=status, 1=Start, 2=Stop, 3=persisted bytes, 4=cap, 5=drops, 6=error.
#[cfg(feature = "console-practice-trace")]
#[no_mangle]
pub extern "C" fn training_modpack_trace_control_v1(command: u32) -> u64 {
    enabled::control(command)
}

// The frozen source overlay supplies callers; the ordinary source can have none.
#[allow(dead_code)]
#[must_use = "bind the scope guard to retain it until function exit"]
pub struct Scope {
    #[cfg(feature = "console-practice-trace")]
    id: u64,
}
#[allow(dead_code)]
#[inline(always)]
pub fn scope(id: u64) -> Scope {
    #[cfg(feature = "console-swap-trace")]
    let stack: u64;
    #[cfg(feature = "console-swap-trace")]
    unsafe {
        std::arch::asm!("mov {}, sp", out(reg) stack, options(nomem, nostack, preserves_flags));
    }
    #[cfg(not(feature = "console-swap-trace"))]
    let stack = 0;
    event(110, [id, stack, 0, 0, 0]);
    Scope {
        #[cfg(feature = "console-practice-trace")]
        id,
    }
}
impl Drop for Scope {
    #[inline(always)]
    fn drop(&mut self) {
        #[cfg(feature = "console-practice-trace")]
        event(111, [self.id, 0, 0, 0, 0]);
    }
}

#[cfg(feature = "console-practice-trace")]
mod enabled {
    use crate::practice_trace_core::{
        self as core, Error, Metadata, Progress, Queue, Recorder, Stats, Storage,
    };
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::UnsafeCell;
    use std::convert::TryFrom;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    #[cfg(not(feature = "console-swap-trace"))]
    const PATH: &str = "sd:/ultimate/TrainingModpack/practice-trace-controlled.bin";
    #[cfg(not(feature = "console-swap-trace"))]
    const NATIVE_PATH: &[u8] = b"sd:/ultimate/TrainingModpack/practice-trace-controlled.bin\0";
    #[cfg(feature = "console-swap-trace")]
    const PATH: &str = "sd:/ultimate/TrainingModpack/practice-trace-swap.bin";
    #[cfg(feature = "console-swap-trace")]
    const NATIVE_PATH: &[u8] = b"sd:/ultimate/TrainingModpack/practice-trace-swap.bin\0";
    const STACK_BYTES: usize = 128 * 1024;
    static STARTED: AtomicBool = AtomicBool::new(false);
    // Capture even the generated main-entry scope and bootstrap Rust allocations
    // before init. They stay in the same bounded FIFO until init starts the writer.
    static URGENT: AtomicBool = AtomicBool::new(false);
    static MAIN: AtomicU64 = AtomicU64::new(0);
    static WRITER_THREAD: AtomicU64 = AtomicU64::new(0);
    static LIVE: AtomicU64 = AtomicU64::new(0);
    static PEAK: AtomicU64 = AtomicU64::new(0);
    static QUEUE: Queue = Queue::new();
    struct Output(UnsafeCell<[u8; core::CHUNK_BYTES]>);
    // Exactly one spawned worker accesses OUTPUT, guaranteed by STARTED.
    unsafe impl Sync for Output {}
    static OUTPUT: Output = Output(UnsafeCell::new([0; core::CHUNK_BYTES]));

    pub fn control(command: u32) -> u64 {
        QUEUE.command(command)
    }

    #[inline]
    fn clock() -> (u64, u64) {
        let tick: u64;
        let thread: u64;
        unsafe {
            std::arch::asm!("mrs {tick}, cntpct_el0", "mrs {thread}, tpidrro_el0",
                tick=out(reg) tick, thread=out(reg) thread, options(nomem, nostack));
        }
        (tick, thread)
    }

    #[inline]
    pub fn event(code: u64, args: [u64; 5]) {
        // Keep bulk source/statement/allocation traffic out of this diagnostic's
        // FIFO. Allocation live/peak counters still run; metadata records them.
        // The separate file preserves the previous broad capture for comparison.
        #[cfg(feature = "console-swap-trace")]
        if !(matches!(code, 1 | 2 | 20 | 21 | 30..=33 | 220..=223 | 900..=909 | 999 | 1200..=1209)
            || matches!(code, 110 | 111) && (0x1080000..=0x10800ff).contains(&args[0]))
        {
            return;
        }
        if let Some(ticket) = QUEUE.admission_ticket() {
            let (tick, thread) = clock();
            // The recorder's IO/logging/thread teardown cannot trace itself.
            if thread != WRITER_THREAD.load(Ordering::Relaxed) {
                QUEUE.record_ticket(ticket, tick, thread, code, args);
            }
        }
    }

    #[inline]
    fn allocation_event(code: u64, args: [u64; 5]) {
        event(code, args);
    }

    /// A request, never synchronous IO. Fatal checkpoints can be lost if the
    /// process aborts before the writer gets scheduled; the decoder says so.
    pub fn checkpoint(code: u64, args: [u64; 5]) {
        event(code, args);
        URGENT.store(true, Ordering::Release);
    }

    // libnx GetInfo IDs 6/7, current process. Not heap free-list statistics.
    unsafe fn process_memory(id: u64) -> (u32, u64) {
        let result: u64;
        let value: u64;
        std::arch::asm!("svc 0x29", inout("x0") 0u64 => result,
            inout("x1") id => value, in("x2") 0xffff8001u64, in("x3") 0u64,
            clobber_abi("C"), options(nostack));
        (result as u32, value)
    }

    struct NativeFile(skyline::nn::fs::FileHandle);
    impl Drop for NativeFile {
        fn drop(&mut self) {
            unsafe { skyline::nn::fs::CloseFile(self.0) }
        }
    }
    fn checked(result: u32) -> Result<(), u64> {
        if result == 0 {
            Ok(())
        } else {
            Err(u64::from(result))
        }
    }
    impl Storage for NativeFile {
        fn length(&mut self) -> Result<u64, u64> {
            let mut size = 0i64;
            checked(unsafe { skyline::nn::fs::GetFileSize(&mut size, self.0) })?;
            u64::try_from(size).map_err(|_| 0x8000_0000_0000_0001)
        }
        fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), u64> {
            let mut actual = 0u64;
            checked(unsafe {
                skyline::nn::fs::ReadFile1(
                    &mut actual,
                    self.0,
                    offset as i64,
                    bytes.as_mut_ptr(),
                    bytes.len() as u64,
                )
            })?;
            if actual == bytes.len() as u64 {
                Ok(())
            } else {
                Err(0x8000_0000_0000_0002)
            }
        }
        fn grow(&mut self, size: u64) -> Result<(), u64> {
            if size > core::DISK_CAP {
                return Err(0x8000_0000_0000_0003);
            }
            checked(unsafe { skyline::nn::fs::SetFileSize(self.0, size as i64) })
        }
        fn write_flush(&mut self, offset: u64, bytes: &[u8]) -> Result<(), u64> {
            checked(unsafe {
                skyline::nn::fs::WriteFile(
                    self.0,
                    offset as i64,
                    bytes.as_ptr(),
                    bytes.len() as u64,
                    &skyline::nn::fs::WriteOption { flags: 1 },
                )
            })
        }
    }

    // Only the worker can wait; producers never do. After close this lock also
    // observes any last producer which had already entered its critical section.
    fn stats() -> Stats {
        loop {
            if let Some(stats) = QUEUE.stats() {
                return stats;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn metadata() -> Metadata {
        let (total_result, total) = unsafe { process_memory(6) };
        let (used_result, used) = unsafe { process_memory(7) };
        let tick_hz: u64;
        unsafe {
            std::arch::asm!("mrs {hz}, cntfrq_el0", hz=out(reg) tick_hz, options(nomem, nostack));
        }
        Metadata {
            tick: clock().0,
            main: MAIN.load(Ordering::Relaxed),
            live: LIVE.load(Ordering::Relaxed),
            peak: PEAK.load(Ordering::Relaxed),
            memory_total: total,
            memory_used: used,
            memory_results: u64::from(total_result) | (u64::from(used_result) << 32),
            tick_hz,
            ..Metadata::default()
        }
    }

    fn writer() {
        WRITER_THREAD.store(clock().1, Ordering::Relaxed);
        let Some(build) =
            core::parse_build_id(option_env!("PRACTICE_TRACE_BUILD_ID").unwrap_or(""))
        else {
            QUEUE.fail(Error::Io(0x8000_0000_0000_0020));
            crate::logging::diagnostic(format_args!(
                "[PracticeTrace] disabled: require PRACTICE_TRACE_BUILD_ID=64-hex frozen source/map SHA256"
            ));
            return;
        };
        // All filesystem/open/growth/flush work is confined to this thread. No
        // truncation, backup copy or legacy capture rotation, including on errors.
        let opened = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(PATH);
        match opened {
            Ok(file) => drop(file),
            Err(error) => {
                QUEUE.fail(Error::Io(
                    error
                        .raw_os_error()
                        .map(|code| code as u32 as u64)
                        .unwrap_or(0x8000_0000_0000_0021),
                ));
                crate::logging::diagnostic(format_args!("[PracticeTrace] open failed: {error}"));
                return;
            }
        }
        let mut handle = skyline::nn::fs::FileHandle { handle: 0 };
        let result = unsafe { skyline::nn::fs::OpenFile(&mut handle, NATIVE_PATH.as_ptr(), 3) };
        if result != 0 || handle.handle == 0 {
            QUEUE.fail(Error::Io(if result != 0 {
                u64::from(result)
            } else {
                0x8000_0000_0000_0022
            }));
            crate::logging::diagnostic(format_args!(
                "[PracticeTrace] SDK open failed: {result:#x}"
            ));
            return;
        }
        let mut file = NativeFile(handle);
        // SAFETY: STARTED permits one worker; neither producer nor checkpoint
        // accesses this buffer. It never lives on the 128 KiB worker stack.
        let bytes = unsafe { &mut *OUTPUT.0.get() };
        let mut recorder = match Recorder::open(&mut file, bytes, build, &QUEUE, metadata) {
            Ok(recorder) => recorder,
            Err(error) => {
                crate::logging::diagnostic(format_args!(
                    "[PracticeTrace] append refused: {error:?}; existing capture retained"
                ));
                return;
            }
        };
        let stream = &recorder.stream;
        crate::logging::diagnostic(format_args!(
            "[PracticeTrace] continuous v2 session={} next_slot={} recovered_tail={}+{} max_bytes={} queue={} stack={} build={}",
            stream.session, stream.next_slot, stream.recovery_offset, stream.recovery_bytes,
            core::DISK_CAP, core::QUEUE_CAPACITY, STACK_BYTES,
            option_env!("PRACTICE_TRACE_BUILD_ID").unwrap_or(""),
        ));
        let mut last_flush = Instant::now();
        loop {
            // Consume before draining so a later checkpoint keeps its request.
            let urgent = URGENT.swap(false, Ordering::AcqRel);
            let deadline = last_flush.elapsed() >= Duration::from_millis(core::FLUSH_MS);
            match recorder.step(&mut file, bytes, &QUEUE, urgent, deadline, metadata) {
                Ok(Progress::Finished) => {
                    crate::logging::diagnostic(format_args!(
                        "[PracticeTrace] capped at {} bytes; queued events abandoned={}; recording stopped",
                        core::DISK_CAP, stats().queued(),
                    ));
                    return;
                }
                Ok(Progress::Flushed) => last_flush = Instant::now(),
                Ok(Progress::Idle) => std::thread::sleep(Duration::from_millis(2)),
                Err(error) => {
                    let abandoned = stats().queued() + recorder.stream.failed_records;
                    crate::logging::diagnostic(format_args!(
                        "[PracticeTrace] stopped write_error={error:?} failure_marker={:?} failures={} abandoned={abandoned}; no bytes overwritten",
                        recorder.failure_marker, recorder.stream.write_failures,
                    ));
                    return;
                }
            }
        }
    }

    struct TraceAllocator;
    #[global_allocator]
    static ALLOCATOR: TraceAllocator = TraceAllocator;
    fn allocated(size: usize) {
        let live = LIVE
            .fetch_add(size as u64, Ordering::Relaxed)
            .saturating_add(size as u64);
        PEAK.fetch_max(live, Ordering::Relaxed);
    }
    // Same System backend. Only this NRO's Rust allocator, never game/other-plugin
    // heaps. Events have opaque pointer values; callbacks never dereference them.
    unsafe impl GlobalAlloc for TraceAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let result = System.alloc(layout);
            if !result.is_null() {
                allocated(layout.size());
            }
            allocation_event(
                100,
                [
                    result as u64,
                    layout.size() as u64,
                    layout.align() as u64,
                    0,
                    0,
                ],
            );
            result
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let result = System.alloc_zeroed(layout);
            if !result.is_null() {
                allocated(layout.size());
            }
            allocation_event(
                101,
                [
                    result as u64,
                    layout.size() as u64,
                    layout.align() as u64,
                    0,
                    0,
                ],
            );
            result
        }
        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            allocation_event(
                102,
                [
                    pointer as u64,
                    layout.size() as u64,
                    layout.align() as u64,
                    0,
                    0,
                ],
            );
            System.dealloc(pointer, layout);
            LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
        }
        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            allocation_event(
                103,
                [
                    pointer as u64,
                    layout.size() as u64,
                    layout.align() as u64,
                    size as u64,
                    0,
                ],
            );
            let result = System.realloc(pointer, layout, size);
            if !result.is_null() {
                LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
                allocated(size);
            }
            allocation_event(
                104,
                [
                    pointer as u64,
                    result as u64,
                    layout.size() as u64,
                    size as u64,
                    0,
                ],
            );
            result
        }
    }

    // Existing pinned observation sites only. No speculative skips and no extra
    // pointer reads: native code still executes (and may fault) unchanged.
    unsafe extern "C" fn list_step(ctx: &skyline::hooks::InlineCtx) {
        let node = ctx.registers[20].x();
        let args = [
            node,
            ctx.registers[21].x(),
            ctx.registers[22].x(),
            ctx.registers[28].x(),
            ctx.registers[30].x(),
        ];
        event(300, args);
        if node == 0 {
            checkpoint(399, args);
        }
    }
    unsafe extern "C" fn list_callback_enter(ctx: &skyline::hooks::InlineCtx) {
        event(
            302,
            [
                ctx.registers[20].x(),
                ctx.registers[0].x(),
                ctx.registers[8].x(),
                ctx.registers[21].x(),
                ctx.registers[22].x(),
            ],
        );
    }

    pub fn init() {
        if STARTED.swap(true, Ordering::AcqRel) {
            return;
        }
        let main =
            unsafe { skyline::hooks::getRegionAddress(skyline::hooks::Region::Text) as usize };
        MAIN.store(main as u64, Ordering::Relaxed);
        event(1, [0; 5]);
        if let Err(error) = std::thread::Builder::new()
            .stack_size(STACK_BYTES)
            .spawn(writer)
        {
            QUEUE.fail(Error::Io(
                error
                    .raw_os_error()
                    .map(|code| code as u32 as u64)
                    .unwrap_or(0x8000_0000_0000_0023),
            ));
            crate::logging::diagnostic(format_args!(
                "[PracticeTrace] writer spawn failed: {error}"
            ));
            return;
        }
        unsafe {
            for (offset, words) in [
                (0x23ef1d0, [0xd63f0100, 0xf9400293, 0x37000320, 0x14000032]),
                (0x23ef1e0, [0xf9401280, 0xb40002a0, 0xf9400008, 0xf9401108]),
            ] {
                if std::slice::from_raw_parts((main + offset) as *const u32, words.len()) != words {
                    event(3, [offset as u64, 0, 0, 0, 0]);
                    return;
                }
            }
            skyline::hooks::A64InlineHook(
                (main + 0x23ef1d0) as *const _,
                list_callback_enter as *const _,
            );
            skyline::hooks::A64InlineHook((main + 0x23ef1e0) as *const _, list_step as *const _);
        }
        checkpoint(2, [0; 5]);
    }
}
