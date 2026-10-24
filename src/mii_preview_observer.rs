//! One natural native editor Test -> return -> Cancel trace. Default-off; no native writes,
//! calls to scene/reload/converter APIs, retained pointer dereferences, or input interception.
//! Seven capture hooks only copy bounded current arguments. A separate post-state-call site
//! hands owned data to the EXISTING config writer once. No observation hook performs I/O.
use serde::{Deserialize, Serialize};
use serde_json::json;
use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::Mutex;
use training_mod_consts::mii::{inventory, saved};

const ARM: &str = "sd:/ultimate/TrainingModpack/mii_preview_observer.arm";
const ROOT: &str = "sd:/ultimate/TrainingModpack/mii_preview_ack";
const SAVED_ID: u32 = 2622;
const DRAFT: usize = 0x5307b78;
const LIMIT: usize = 8192;
// Filled from the owned 13.0.5 ELF; verify every pristine range BEFORE installing any hook.
const CODE: &[(usize, usize, u64)] = &[
    (0x180fbbc, 0xc4, 0xe2b5b7a888540317),
    (0x17c81d0, 0x318, 0x1e7e61c8c942d0ef),
    (0x17bd3e0, 0x1b0, 0x7b503c214b5daaa0),
    (0x22f9700, 0x1cc, 0xda9f738f902177d9),
    (0x22fa6d0, 0x64, 0xf7c0fa7ba3907cc7),
    (0x22f9a90, 0xc0, 0xf03cfc8e7b20f299),
    (0x22fa0cc, 0x14, 0xaced7277cdd452e),
    (0x180e13c, 0x20, 0x18ae7662a8922d5c),
    (0x180a05c, 0x20, 0x8cbc74ccc7904e12),
    (0x180cb24, 0x20, 0x89f6dda87fd85944),
    (0x1809f1c, 0x20, 0x2a569a1c1dadd2fc),
    (0x180ea70, 0x20, 0x559dede15266d8cc),
];
static BASE: AtomicUsize = AtomicUsize::new(0);
static ARMED: AtomicBool = AtomicBool::new(false);
static ENQUEUED: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);
// Idle parent ticks must not contend with queued initialization. This one-shot flag
// is released only after the full capture succeeds or first-fault metadata is ready.
static TERMINAL_READY: AtomicBool = AtomicBool::new(false);
static FAULT: AtomicU8 = AtomicU8::new(0);
// The first fault claims FAULT before filling these scalars. Publication waits for READY;
// no waiting/spinning or diagnostic-state lock is needed on a contended capture path.
static FAULT_READY: AtomicBool = AtomicBool::new(false);
static FAULT_HOOK: AtomicUsize = AtomicUsize::new(0);
static FAULT_EXPECTED_THREAD: AtomicUsize = AtomicUsize::new(0);
static FAULT_OBSERVED_THREAD: AtomicUsize = AtomicUsize::new(0);
static STATE: Mutex<Trace> = Mutex::new(Trace::empty());

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arm {
    schema: u32,
    run: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct Configuration {
    outfit: u64,
    hat: u64,
    colour: u8,
    voice: u8,
    specials: [u8; 4],
    charinfo_hash: u64,
    chara: u64,
}

// Native 180fc74 -> 17c81d0 copies the selected saved row (including its ID at +0)
// into DRAFT. The unrelated +1d8 value is a database selector: -1 selects the
// native working record at database+8d8. Never substitute it for the saved ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct Draft {
    saved_id: u32,
    database_selector: u32,
    #[serde(flatten)]
    configuration: Configuration,
}
#[derive(Serialize)]
struct SavedFixture {
    saved_id: u32,
    #[serde(flatten)]
    configuration: Configuration,
}
impl Draft {
    fn matches_saved(&self, expected: &SavedFixture) -> bool {
        self.saved_id == expected.saved_id && self.configuration == expected.configuration
    }
}

#[derive(Clone, Copy, Default, Serialize)]
struct Event {
    event: &'static str,
    thread: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scene: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    old_context: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sequence: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    manager: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    keys: Option<[usize; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameter: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameter_vtable: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    control: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    saved_caller_lr: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stack_index: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    draft: Option<Draft>,
}

#[derive(Clone, Copy, Serialize)]
struct Rejection {
    hook: usize,
    expected_thread: usize,
    observed_thread: usize,
}
/// Numeric identities only. The writer must never interpret any field as pointer authority.
/// Boxed only at the one post-native enqueue, so ordinary filesystem jobs remain small.
#[derive(Clone, Copy)]
pub(crate) struct Trace {
    run: [u8; 32],
    events: [Option<Event>; 8],
    len: usize,
    seen: u8,
    thread: usize,
    initialization_thread: usize,
    sequence: usize,
    context: usize,
    parent_context: usize,
    fault: u8,
    first_rejection: Option<Rejection>,
    publish_thread: usize,
    publish_context: usize,
    publish_sequence: usize,
}
impl Trace {
    const fn empty() -> Self {
        Self {
            run: [0; 32],
            events: [None; 8],
            len: 0,
            seen: 0,
            thread: 0,
            initialization_thread: 0,
            sequence: 0,
            context: 0,
            parent_context: 0,
            fault: 0,
            first_rejection: None,
            publish_thread: 0,
            publish_context: 0,
            publish_sequence: 0,
        }
    }
    fn push(&mut self, bit: u8, event: Event) -> Result<(), u8> {
        if self.seen & bit != 0 || self.len == self.events.len() {
            return Err(4);
        }
        self.events[self.len] = Some(event);
        self.len += 1;
        self.seen |= bit;
        Ok(())
    }
    fn editor(&self, context: usize) -> Result<(), u8> {
        if context == self.context && aligned(context) {
            Ok(())
        } else {
            Err(3)
        }
    }
}
fn aligned(p: usize) -> bool {
    p != 0 && p.is_multiple_of(8)
}
unsafe fn rd<T: Copy>(p: usize, off: usize) -> T {
    std::ptr::read_volatile((p + off) as *const T)
}
fn fault(reason: u8, hook: usize, expected_thread: usize, observed_thread: usize) {
    if FAULT
        .compare_exchange(0, reason, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        FAULT_HOOK.store(hook, Ordering::Relaxed);
        FAULT_EXPECTED_THREAD.store(expected_thread, Ordering::Relaxed);
        FAULT_OBSERVED_THREAD.store(observed_thread, Ordering::Relaxed);
        FAULT_READY.store(true, Ordering::Release);
        TERMINAL_READY.store(true, Ordering::Release);
    }
}
#[derive(Clone, Copy)]
enum CaptureThread {
    Parent,
    // Native 3725ca0 queues 3725fa0 -> 37261e0 -> scene init; it is not an inline
    // continuation of the requesting parent. All other callback thread guards remain.
    Initialization,
}
struct Capture;
impl Drop for Capture {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}
// No blocking wait, formatting, logging, allocation, filesystem or native call in the closure.
// GetCurrentThread executes before the diagnostic guard, following existing observer convention.
unsafe fn capture(
    hook: usize,
    phase: CaptureThread,
    f: impl FnOnce(&mut Trace, usize) -> Result<(), u8>,
) {
    if !ARMED.load(Ordering::Acquire)
        || ENQUEUED.load(Ordering::Acquire)
        || FAULT.load(Ordering::Acquire) != 0
    {
        return;
    }
    let thread = skyline::nn::os::GetCurrentThread() as usize;
    if BUSY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        fault(2, hook, 0, thread);
        return;
    }
    let _capture = Capture;
    if ENQUEUED.load(Ordering::Acquire) {
        return;
    }
    let Ok(mut s) = STATE.try_lock() else {
        fault(2, hook, 0, thread);
        return;
    };
    let expected_thread = match phase {
        CaptureThread::Parent => s.thread,
        CaptureThread::Initialization => s.initialization_thread,
    };
    if thread == 0 || (expected_thread != 0 && thread != expected_thread) {
        fault(1, hook, expected_thread, thread);
        return;
    }
    if let Err(reason) = f(&mut s, thread) {
        fault(reason, hook, expected_thread, thread);
    } else if s.seen == 255 {
        // All callback checks completed while STATE/BUSY still protect the capture.
        TERMINAL_READY.store(true, Ordering::Release);
    }
}

// Current native editor callback only. This is the wide editor draft, NOT a G-pool record.
// Two owned copies detect a changed snapshot; they do not assert an unseen global native lock.
unsafe fn draft() -> Result<Draft, u8> {
    unsafe fn once(p: usize) -> Draft {
        let charinfo: [u8; 0x58] = rd(p, 0x148);
        Draft {
            saved_id: rd(p, 0),
            database_selector: rd(p, 0x1d8),
            configuration: Configuration {
                outfit: rd(p, 0x130),
                hat: rd(p, 0x138),
                colour: rd(p, 0x140),
                voice: rd(p, 0x141),
                specials: rd(p, 0x142),
                charinfo_hash: inventory::fnv1a64(&charinfo),
                chara: rd(p, 0x1a0),
            },
        }
    }
    let p = BASE.load(Ordering::Acquire) + DRAFT;
    let a = once(p);
    if a != once(p) {
        return Err(5);
    }
    Ok(a)
}

#[skyline::hook(offset = 0x22f98b0, inline)]
unsafe fn request(ctx: &mut InlineCtx) {
    capture(0x22f98b0, CaptureThread::Parent, |s, thread| {
        if s.seen != 0 {
            return Err(4);
        }
        let sequence = ctx.registers[19].x() as usize;
        let manager = ctx.registers[0].x() as usize;
        let parameter = ctx.registers[3].x() as usize;
        let sp = ctx.sp.x() as usize;
        if !aligned(sequence)
            || !aligned(manager)
            || !aligned(parameter)
            || sp == 0
            || !sp.is_multiple_of(16)
        {
            return Err(3);
        }
        s.thread = thread;
        s.sequence = sequence;
        s.push(
            1,
            Event {
                event: "request",
                thread,
                sequence: Some(sequence),
                manager: Some(manager),
                keys: Some([ctx.registers[1].x() as usize, ctx.registers[2].x() as usize]),
                parameter: Some(parameter),
                parameter_vtable: Some(rd(parameter, 0)),
                control: Some(rd(parameter, 8)),
                saved_caller_lr: Some(rd(sp, 0xb8)),
                ..Event::default()
            },
        )
    });
}

#[skyline::hook(offset = 0x180e13c, inline)]
unsafe fn initialized(ctx: &mut InlineCtx) {
    capture(0x180e13c, CaptureThread::Initialization, |s, thread| {
        if s.seen == 0 {
            return Ok(());
        }
        if s.seen != 1 {
            return Err(4);
        }
        let context = ctx.registers[19].x() as usize;
        let scene = ctx.registers[20].x() as usize;
        let old = ctx.registers[21].x() as usize;
        if !aligned(context) || !aligned(scene) || rd::<usize>(scene, 0x58) != context {
            return Err(3);
        }
        s.context = context;
        s.initialization_thread = thread;
        s.push(
            2,
            Event {
                event: "initialized",
                thread,
                context: Some(context),
                scene: Some(scene),
                old_context: Some(old),
                control: Some(rd(context, 0x160)),
                state: Some(rd(context, 0x148)),
                ..Event::default()
            },
        )?;
        if old != 0 {
            return Err(3);
        }
        Ok(())
    });
}

#[skyline::hook(offset = 0x180a05c, inline)]
unsafe fn menu_result(ctx: &mut InlineCtx) {
    capture(0x180a05c, CaptureThread::Parent, |s, thread| {
        if s.seen == 0 {
            return Ok(());
        }
        let context = ctx.registers[19].x() as usize;
        s.editor(context)?;
        let result = ctx.registers[8].w() as i32;
        if result >= 0 && result != 6 {
            return Ok(());
        }
        let bit = if result == 6 && s.seen == 3 {
            4
        } else if result == -2 && s.seen == 15 {
            16
        } else {
            return Err(4);
        };
        s.push(
            bit,
            Event {
                event: "menu_result",
                thread,
                context: Some(context),
                result: Some(result),
                state: Some(rd(context, 0x148)),
                frame: Some(rd(context, 0x14c)),
                draft: Some(draft()?),
                ..Event::default()
            },
        )
    });
}

#[skyline::hook(offset = 0x180cb24, inline)]
unsafe fn test_return(ctx: &mut InlineCtx) {
    capture(0x180cb24, CaptureThread::Parent, |s, thread| {
        if s.seen == 0 {
            return Ok(());
        }
        if s.seen != 7 {
            return Err(4);
        }
        let context = ctx.registers[19].x() as usize;
        s.editor(context)?;
        let state = rd(context, 0x148);
        s.push(
            8,
            Event {
                event: "test_return_cleanup_reached",
                thread,
                context: Some(context),
                state: Some(state),
                stack_index: Some(rd(context, 0x144)),
                frame: Some(rd(context, 0x14c)),
                draft: Some(draft()?),
                ..Event::default()
            },
        )?;
        if state != 10 {
            return Err(4);
        }
        Ok(())
    });
}

#[skyline::hook(offset = 0x1809f1c, inline)]
unsafe fn completion(ctx: &mut InlineCtx) {
    capture(0x1809f1c, CaptureThread::Parent, |s, thread| {
        if s.seen == 0 {
            return Ok(());
        }
        if s.seen & 31 != 31 {
            return Err(4);
        }
        let context = ctx.registers[20].x() as usize;
        s.editor(context)?;
        let sp = ctx.sp.x() as usize;
        if sp == 0 || !sp.is_multiple_of(16) {
            return Err(3);
        }
        let result = ctx.registers[1].w() as i32;
        s.push(
            32,
            Event {
                event: "completion_requested",
                thread,
                context: Some(context),
                result: Some(result),
                manager: Some(ctx.registers[0].x() as usize),
                saved_caller_lr: Some(rd(sp, 0x18)),
                ..Event::default()
            },
        )?;
        if result != 2 {
            return Err(4);
        }
        Ok(())
    });
}

#[skyline::hook(offset = 0x180ea70, inline)]
unsafe fn destructor_returned(ctx: &mut InlineCtx) {
    capture(0x180ea70, CaptureThread::Parent, |s, thread| {
        if s.seen == 0 {
            return Ok(());
        }
        if s.seen & 31 != 31 {
            return Err(4);
        }
        // Current register identity only: destructor has returned; never read this context.
        let context = ctx.registers[19].x() as usize;
        s.editor(context)?;
        s.push(
            64,
            Event {
                event: "context_destructor_returned",
                thread,
                context: Some(context),
                ..Event::default()
            },
        )
    });
}

#[skyline::hook(offset = 0x22f9740, inline)]
unsafe fn parent_resumed(ctx: &mut InlineCtx) {
    capture(0x22f9740, CaptureThread::Parent, |s, thread| {
        if s.seen == 0 {
            return Ok(());
        }
        if s.seen & 31 != 31 {
            return Err(4);
        }
        let context = ctx.registers[0].x() as usize;
        if !aligned(context) {
            return Err(3);
        }
        let sequence = rd(context, 0);
        if sequence != s.sequence {
            return Err(3);
        }
        s.parent_context = context;
        s.push(
            128,
            Event {
                event: "parent_resumed",
                thread,
                context: Some(context),
                sequence: Some(sequence),
                ..Event::default()
            },
        )
    });
}

// 22fa6d0 dispatches the current state through BLR22fa718. This site is AFTER that call;
// x20 is its current context (which native code next reads at +114), not a saved pointer.
// No native locks are acquired in this dispatcher. No guessed function return ABI/wrapper.
// This sole producer boxes/enqueues AFTER dropping the diagnostic guard; never performs I/O.
#[skyline::hook(offset = 0x22fa71c, inline)]
unsafe fn after_state(ctx: &mut InlineCtx) {
    if !ARMED.load(Ordering::Acquire)
        || ENQUEUED.load(Ordering::Acquire)
        || !TERMINAL_READY.load(Ordering::Acquire)
    {
        return;
    }
    let thread = skyline::nn::os::GetCurrentThread() as usize;
    if BUSY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        // No capture is lost: defer publication to an existing later parent tick.
        return;
    }
    let trace = {
        let _capture = Capture;
        let Ok(mut s) = STATE.try_lock() else {
            return;
        };
        if s.seen != 255 && FAULT.load(Ordering::Acquire) == 0 {
            return;
        }
        if FAULT.load(Ordering::Acquire) != 0 && !FAULT_READY.load(Ordering::Acquire) {
            return;
        }
        // Close capture BEFORE copying owned state. No later native event can be accepted.
        ENQUEUED.store(true, Ordering::Release);
        s.publish_thread = thread;
        if FAULT.load(Ordering::Acquire) == 0 {
            let context = ctx.registers[20].x() as usize;
            if thread != s.thread || !aligned(context) || context != s.parent_context {
                fault(3, 0x22fa71c, s.thread, thread);
            } else {
                s.publish_context = context;
                s.publish_sequence = rd(context, 0);
                if s.publish_sequence != s.sequence {
                    fault(3, 0x22fa71c, s.thread, thread);
                }
            }
        }
        s.fault = FAULT.load(Ordering::Acquire);
        if s.fault != 0 && !FAULT_READY.load(Ordering::Acquire) {
            // A contender has claimed a fault but has not published its scalars yet.
            // Nothing was queued; retry publication at the next existing parent callback.
            ENQUEUED.store(false, Ordering::Release);
            return;
        }
        if s.fault != 0 {
            s.first_rejection = Some(Rejection {
                hook: FAULT_HOOK.load(Ordering::Relaxed),
                expected_thread: FAULT_EXPECTED_THREAD.load(Ordering::Relaxed),
                observed_thread: FAULT_OBSERVED_THREAD.load(Ordering::Relaxed),
            });
        }
        *s
    };
    // At most one queued job in the process. Existing writer already forced in init().
    crate::common::filesystem::write_mii_preview(Box::new(trace));
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn publish(run: &[u8; 32], name: &str, value: &serde_json::Value) -> io::Result<()> {
    let run = std::str::from_utf8(run).map_err(|_| invalid("run encoding"))?;
    let bytes = serde_json::to_vec(value).map_err(|_| invalid("serialization"))?;
    if bytes.len() > LIMIT {
        return Err(invalid("document exceeds 8192 bytes"));
    }
    let stem = format!("{ROOT}/{run}/{name}");
    let ready = format!("{stem}.ack");
    match fs::metadata(&ready) {
        Ok(_) => return Err(invalid("ack already exists")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let part = format!("{stem}.part");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .create_new(true)
        .truncate(false)
        .open(&part)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(part, ready)
}

fn saved_draft() -> Result<SavedFixture, String> {
    let header = saved::read_stable_header(std::path::Path::new("save:/save_data/mii"), SAVED_ID)?;
    let raw = saved::parse_header(SAVED_ID, 0, &header)?;
    inventory::classify(&raw).map_err(|e| format!("invalid saved fixture: {e:?}"))?;
    Ok(SavedFixture {
        saved_id: raw.slot_id,
        configuration: Configuration {
            outfit: raw.tag_outfit.0,
            hat: raw.tag_hat.0,
            colour: raw.colour,
            voice: raw.voice,
            specials: raw.specials,
            charinfo_hash: inventory::fnv1a64(&raw.charinfo),
            chara: raw.tag_chara.0,
        },
    })
}

/// Existing config-writer thread only. Own data + bounded read-only saved header; no native reads,
/// scene calls or retained pointer dereferences. A failed publication is never retried/overwritten.
pub(crate) fn write_trace(trace: &Trace) {
    let saved = saved_draft();
    let matching_drafts = trace
        .events
        .iter()
        .flatten()
        .filter_map(|e| e.draft)
        .filter(|d| {
            saved
                .as_ref()
                .is_ok_and(|expected| d.matches_saved(expected))
        })
        .count();
    let complete = trace.fault == 0 && trace.seen == 255 && trace.len == 8 && matching_drafts == 3;
    let run = std::str::from_utf8(&trace.run).unwrap_or("");
    let document = json!({"schema":1,"run":run,"event":"trace","complete":complete,
        "native_events":&trace.events[..trace.len],"terminal":{
            "fault":trace.fault,"fault_names":["none","thread_changed","reentrant_or_contended",
                "current_identity","event_order_or_result","draft_changed"],
            "event_mask":trace.seen,"matching_drafts":matching_drafts,
            "parent_thread":trace.thread,"initialization_thread":trace.initialization_thread,
            "first_rejection":trace.first_rejection,
            "rejection_expected_thread_zero_means_unavailable_or_not_bound":true,
            "publication_site":0x22fa71cusize,"thread":trace.publish_thread,
            "current_parent_context":trace.publish_context,"current_sequence":trace.publish_sequence},
        "saved_fixture_at_publication":saved,
        "identity_policy":{"native_saved_id_offset":0,"database_selector_offset":0x1d8,
            "saved_id_and_all_configuration_fields_required":true,
            "database_selector_is_not_saved_id":true},
        "limits":{"records_including_armed_and_terminal":10,"no_native_writes":true,
            "destructor_return_is_not_free_complete":true,"no_integration_acceptance":true}});
    if let Err(error) = publish(&trace.run, "01-trace", &document) {
        crate::logging::diagnostic(format_args!("[MiiPreview] publication_failed: {error}"));
    }
}

pub(crate) fn init() {
    let result = (|| -> io::Result<()> {
        let mut bytes = Vec::new();
        File::open(ARM)?.take(129).read_to_end(&mut bytes)?;
        if bytes.len() > 128 {
            return Err(invalid("arm exceeds 128 bytes"));
        }
        let arm: Arm = serde_json::from_slice(&bytes).map_err(|_| invalid("invalid arm JSON"))?;
        if arm.schema != 1
            || arm.run.len() != 32
            || !arm
                .run
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid("invalid schema/run nonce"));
        }
        let base = unsafe { getRegionAddress(Region::Text) as usize };
        for &(offset, len, expected) in CODE {
            let actual = unsafe {
                inventory::fnv1a64(std::slice::from_raw_parts(
                    (base + offset) as *const u8,
                    len,
                ))
            };
            if actual != expected {
                return Err(io::Error::new(io::ErrorKind::InvalidData,
                    format!("pristine fingerprint mismatch at {offset:#x}: {actual:016x} != {expected:016x}")));
            }
        }
        let mut run = [0; 32];
        run.copy_from_slice(arm.run.as_bytes());
        fs::create_dir_all(ROOT)?;
        fs::create_dir(format!("{ROOT}/{}", arm.run))?;
        publish(
            &run,
            "00-armed",
            &json!({"schema":1,"run":arm.run,"event":"armed",
            "fingerprints_matched":true,"activation":"install_after_ack",
            "expected_saved_id":SAVED_ID,"max_native_events":8,"max_total_records":10}),
        )?;
        // Force the already-existing writer here, never lazily create it from a native callback.
        crate::common::filesystem::init();
        STATE.lock().unwrap().run = run;
        BASE.store(base, Ordering::Release);
        skyline::install_hooks!(
            request,
            initialized,
            menu_result,
            test_return,
            completion,
            destructor_returned,
            parent_resumed,
            after_state
        );
        ARMED.store(true, Ordering::Release);
        Ok(())
    })();
    if let Err(error) = result {
        if error.kind() != io::ErrorKind::NotFound {
            crate::logging::diagnostic(format_args!("[MiiPreview] init_refused: {error}"));
        }
    }
}
