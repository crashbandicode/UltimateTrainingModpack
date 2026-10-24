//! Development-only results observation. No input, native calls, or native writes.
//! Live pointers are read only at audited native game-thread boundaries. The
//! existing filesystem worker receives owned values, never pointers to follow.
use super::recording::{Recording, Stop};
use serde::{Deserialize, Serialize};
use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

const ARM: &str = "sd:/ultimate/TrainingModpack/results_observer.arm";
const ROOT: &str = "sd:/ultimate/TrainingModpack/results_observer";
const MAX_TASKS: usize = 64;
const MAX_SNAPSHOTS: usize = 256;
const MAX_TICKS: u32 = 18_000;
// Owned 13.0.5 ELF, main offsets. Verify before either patch is installed.
const CODE: &[(usize, usize, u64)] = &[
    (0x150e5f0, 0x3e0, 0x774bf6a587bf3495),
    (0x150e9d0, 0xd0, 0x7900bafaf1df025f),
    (0x145bec0, 0x38, 0xf97cfe53817ace0e),
    (0x1d5b628, 8, 0x9dedac6c153850db),
    (0x146cd4c, 0xc, 0x6e2239d140769ff9),
    (0x143fc10, 8, 0xb2ab3c8d2d151570),
    (0x145e850, 0xc, 0x2e8d48e4010e9ec9),
];
const TICK_CALLERS: [usize; 4] = [0x14abed0, 0x14ac080, 0x14d3ac0, 0x151f790];
static BASE: AtomicUsize = AtomicUsize::new(0);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static CONTENDED: AtomicBool = AtomicBool::new(false);
static TRACE: Mutex<Option<Box<Trace>>> = Mutex::new(None);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arm {
    schema: u32,
    run: String,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize)]
struct Task {
    key: u64,
    address: usize,
    vtable: usize,
    flags: [u8; 3],
    field_c: i32,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize)]
struct Popup {
    address: usize,
    phase: i32,
    result: i32,
    forced: i32,
    selected: i32,
    flag: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
struct Snapshot {
    sequence: usize,
    phase: i32,
    previous: i32,
    stack_top: i32,
    stack: [i32; 19],
    mirror: u8,
    mode: u8,
    owner_code: Option<u8>,
    save_status: Option<i32>,
    popup: Option<Popup>,
    queue: Option<[u8; 2]>,
    online: Option<[u8; 2]>,
    task_count: usize,
    #[serde(serialize_with = "serialize_tasks")]
    tasks: [Task; MAX_TASKS],
}

fn serialize_tasks<S: serde::Serializer>(
    tasks: &[Task; MAX_TASKS],
    s: S,
) -> Result<S::Ok, S::Error> {
    // Zero entries are unused. A valid native task has a nonzero address.
    tasks[..tasks
        .iter()
        .position(|t| t.address == 0)
        .unwrap_or(MAX_TASKS)]
        .serialize(s)
}

pub(crate) struct Trace {
    run: String,
    thread: usize,
    sequence: usize,
    owner: usize,
    caller: usize,
    recording: Recording<Snapshot>,
}

unsafe fn rd<T: Copy>(base: usize, offset: usize) -> T {
    std::ptr::read_volatile((base + offset) as *const T)
}

fn aligned(value: usize) -> bool {
    value != 0 && value.is_multiple_of(8)
}

unsafe fn pointer(base: usize, offset: usize) -> Result<Option<usize>, Stop> {
    let p: usize = rd(base, offset);
    if p == 0 {
        Ok(None)
    } else if aligned(p) {
        Ok(Some(p))
    } else {
        Err(Stop::InvalidBinding)
    }
}

unsafe fn singleton(base: usize, offset: usize) -> Result<Option<usize>, Stop> {
    match pointer(base, offset)? {
        Some(p) => pointer(p, 8),
        None => Ok(None),
    }
}

unsafe fn snapshot(base: usize) -> Result<(Snapshot, u32), Stop> {
    let seq = singleton(base, 0x52c35d0)?.ok_or(Stop::InvalidBinding)?;
    let top: i32 = rd(seq, 0x33c);
    let phase: i32 = rd(seq, 0x340);
    if !(0..19).contains(&top) || !(-1..42).contains(&phase) {
        return Err(Stop::InvalidBinding);
    }
    let mut value = Snapshot {
        sequence: seq,
        phase,
        previous: rd(seq, 0x348),
        stack_top: top,
        stack: rd(seq, 0x2f0),
        mirror: rd(seq, 0x350),
        mode: rd(base, 0x52cc5d8),
        owner_code: None,
        save_status: None,
        popup: None,
        queue: None,
        online: None,
        task_count: 0,
        tasks: [Task::default(); MAX_TASKS],
    };
    let size: usize = rd(seq, 0x18);
    if size > MAX_TASKS {
        return Err(Stop::InvalidBinding);
    }
    let mut node = pointer(seq, 0x10)?;
    while let Some(n) = node {
        if value.task_count == size {
            return Err(Stop::InvalidBinding);
        }
        let t = pointer(n, 0x18)?.ok_or(Stop::InvalidBinding)?;
        // Repeated tasks expose a cycle or inconsistent map; never walk indefinitely.
        if value.tasks[..value.task_count]
            .iter()
            .any(|v| v.address == t)
        {
            return Err(Stop::InvalidBinding);
        }
        value.tasks[value.task_count] = Task {
            key: rd::<u64>(n, 0x10) & 0xff_ffff_ffff,
            address: t,
            vtable: rd(t, 0),
            flags: rd(t, 8),
            field_c: rd(t, 0xc),
        };
        value.task_count += 1;
        node = pointer(n, 0)?;
    }
    if value.task_count != size {
        return Err(Stop::InvalidBinding);
    }
    if let Some(p) = singleton(base, 0x532f820)? {
        value.popup = Some(Popup {
            address: p,
            phase: rd(p, 0xf8),
            result: rd(p, 0x118),
            forced: rd(p, 0x11c),
            selected: rd(p, 0x120),
            flag: rd(p, 0x142),
        });
    }
    if let Some(p) = singleton(base, 0x532f7c8)? {
        value.queue = Some(rd(p, 0x6a));
    }
    if let Some(p) = singleton(base, 0x52c35c8)? {
        value.save_status = Some(rd(p, 0x10));
    }
    // The mode7 branch at145bee8 uses the outer singleton's +0 object, then
    // that object's +0x50 member (not the usual singleton +8 inner state).
    if value.mode == 7 {
        if let Some(outer) = pointer(base, 0x5329f98)? {
            if let Some(object) = pointer(outer, 0)? {
                if let Some(online) = pointer(object, 0x50)? {
                    value.online = Some(rd(online, 0xc));
                }
            }
        }
    }
    Ok((value, rd(seq, 0x344)))
}

fn close(state: &mut Option<Box<Trace>>, reason: Stop) {
    ACTIVE.store(false, Ordering::Release);
    if let Some(mut trace) = state.take() {
        trace.recording.finish(reason);
        crate::common::filesystem::write_results_observer(trace);
    }
}

unsafe fn sample(ctx: Option<&InlineCtx>) {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let Ok(mut slot) = TRACE.try_lock() else {
        CONTENDED.store(true, Ordering::Release);
        return;
    };
    if CONTENDED.load(Ordering::Acquire) {
        close(&mut slot, Stop::Contention);
        return;
    }
    let Some(trace) = slot.as_mut() else {
        return;
    };
    let thread = skyline::nn::os::GetCurrentThread() as usize;
    if thread == 0 || (trace.thread != 0 && trace.thread != thread) {
        close(&mut slot, Stop::BindingChanged);
        return;
    }
    let base = BASE.load(Ordering::Acquire);
    // Teardown permits an already-cleared holder. Do not reinterpret that
    // native exit as an invalid live tick or dereference the cached sequence.
    if ctx.is_none() {
        match singleton(base, 0x52c35d0) {
            Ok(None) => return,
            Ok(Some(_)) => {}
            Err(why) => {
                close(&mut slot, why);
                return;
            }
        }
    }
    let current_owner = if let Some(ctx) = ctx {
        let owner = ctx.registers[0].x() as usize;
        let caller = (ctx.registers[30].x() as usize).checked_sub(base);
        if !aligned(owner) || caller.is_none_or(|c| !TICK_CALLERS.contains(&c)) {
            close(&mut slot, Stop::InvalidBinding);
            return;
        }
        let caller = caller.unwrap_or(0);
        if trace.owner != 0 && (trace.owner != owner || trace.caller != caller) {
            close(&mut slot, Stop::BindingChanged);
            return;
        }
        trace.owner = owner;
        trace.caller = caller;
        Some(owner)
    } else {
        None
    };
    let (mut observed, age) = match snapshot(base) {
        Ok(v) => v,
        Err(why) => {
            close(&mut slot, why);
            return;
        }
    };
    // Only this hook's current argument is dereferenced; the retained numeric
    // owner is an identity check and is never a source of native reads.
    observed.owner_code = current_owner.map(|owner| rd(owner, 0x58));
    if trace.sequence != 0 && trace.sequence != observed.sequence {
        close(&mut slot, Stop::BindingChanged);
        return;
    }
    trace.thread = thread;
    trace.sequence = observed.sequence;
    if !trace.recording.observe(observed, age) {
        let why = trace.recording.stop().unwrap_or(Stop::InvalidBinding);
        close(&mut slot, why);
    }
}

unsafe fn finish() {
    // Capture the final live state before the native teardown frees the map.
    sample(None);
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let Ok(mut slot) = TRACE.try_lock() else {
        CONTENDED.store(true, Ordering::Release);
        return;
    };
    let Some(trace) = slot.as_ref() else {
        return;
    };
    let thread = skyline::nn::os::GetCurrentThread() as usize;
    let reason = if CONTENDED.load(Ordering::Acquire) {
        Stop::Contention
    } else if trace.thread == 0 || trace.thread != thread {
        Stop::BindingChanged
    } else {
        Stop::NativeExit
    };
    close(&mut slot, reason);
}

fn publication_step<T>(step: &str, result: io::Result<T>) -> io::Result<T> {
    result.map_err(|e| io::Error::new(e.kind(), format!("{step}: {e}")))
}

fn publish_json(run: &str, name: &str, value: &impl Serialize) -> io::Result<()> {
    let path = format!("{ROOT}/{run}/{name}.json");
    let part = format!("{path}.part");
    let file = publication_step(
        "create partial",
        // Pinned Skyline std checks create_new only for existing paths;
        // creating a missing file also requires create(true).
        OpenOptions::new()
            .write(true)
            .create(true)
            .create_new(true)
            .truncate(false)
            .open(&part),
    )?;
    let mut writer = BufWriter::new(file);
    publication_step(
        "serialize",
        serde_json::to_writer(&mut writer, value).map_err(Into::into),
    )?;
    publication_step("flush", writer.flush())?;
    publication_step("sync", writer.get_ref().sync_all())?;
    drop(writer);
    publication_step("rename", fs::rename(part, path))
}

pub(crate) fn write_trace(trace: &Trace) {
    #[derive(Serialize)]
    struct Document<'a> {
        schema: u32,
        run: &'a str,
        stop: Option<Stop>,
        ticks: u32,
        thread: usize,
        owner: usize,
        caller: usize,
        frames: &'a [super::recording::Frame<Snapshot>],
        no_native_writes: bool,
        rematch_encoding_qualified: bool,
    }
    let result = publish_json(
        &trace.run,
        "01-results",
        &Document {
            schema: 1,
            run: &trace.run,
            stop: trace.recording.stop(),
            ticks: trace.recording.ticks(),
            thread: trace.thread,
            owner: trace.owner,
            caller: trace.caller,
            frames: trace.recording.frames(),
            no_native_writes: true,
            rematch_encoding_qualified: false,
        },
    );
    let outcome = match result {
        Ok(()) => "published".to_owned(),
        Err(e) => format!("publish_failed: {e}"),
    };
    crate::logging::diagnostic(format_args!(
        "[ResultsObserver] {outcome}; stop={:?} ticks={} snapshots={}",
        trace.recording.stop(),
        trace.recording.ticks(),
        trace.recording.frames().len(),
    ));
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn prepare() -> io::Result<()> {
    let mut bytes = Vec::new();
    File::open(ARM)?.take(129).read_to_end(&mut bytes)?;
    if bytes.len() > 128 {
        return Err(invalid("arm exceeds128"));
    }
    let arm: Arm = serde_json::from_slice(&bytes)?;
    if arm.schema != 1
        || arm.run.len() != 32
        || !arm
            .run
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid("schema/run"));
    }
    let base = unsafe { getRegionAddress(Region::Text) as usize };
    for &(offset, size, expected) in CODE {
        let data = unsafe { std::slice::from_raw_parts((base + offset) as *const u8, size) };
        if training_mod_consts::mii::inventory::fnv1a64(data) != expected {
            return Err(invalid("pristine native fingerprint"));
        }
    }
    fs::create_dir_all(ROOT)?;
    fs::create_dir(format!("{ROOT}/{}", arm.run))?;
    // Exercise the exact native publication path before installing hooks or
    // spending a match on a capture that cannot be written.
    publish_json(
        &arm.run,
        "00-ready",
        &serde_json::json!({
            "schema": 1, "run": arm.run, "writer_ready": true,
        }),
    )?;
    *TRACE.lock().map_err(|_| invalid("trace lock"))? = Some(Box::new(Trace {
        run: arm.run,
        thread: 0,
        sequence: 0,
        owner: 0,
        caller: 0,
        recording: Recording::new(MAX_SNAPSHOTS, MAX_TICKS),
    }));
    crate::common::filesystem::init();
    BASE.store(base, Ordering::Release);
    Ok(())
}

// Inline entry avoids calling or replacing the native ABI. Both first20-byte
// spans are straight-line prologues; there is no live internal x16/x17 operand.
#[skyline::hook(offset = 0x150e5f0, inline)]
unsafe fn result_tick(ctx: &mut InlineCtx) {
    sample(Some(ctx));
}

#[skyline::hook(offset = 0x150e9d0, inline)]
unsafe fn result_exit(_ctx: &mut InlineCtx) {
    finish();
}

pub(crate) fn init() {
    match prepare() {
        Ok(()) => {
            skyline::install_hooks!(result_tick, result_exit);
            ACTIVE.store(true, Ordering::Release);
            crate::logging::diagnostic(format_args!(
                "[ResultsObserver] armed; readonly, one results lifetime, bounded capture"
            ));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => crate::logging::diagnostic(format_args!("[ResultsObserver] init_refused: {e}")),
    }
}
