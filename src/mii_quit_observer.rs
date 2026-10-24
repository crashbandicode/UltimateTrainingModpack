//! One normal Training Quit event. Default-off and read-only; separate from the eight-event
//! Mii Test observer. Native hooks copy owned scalars only. One existing config-writer job
//! publishes after the native UI state callback. No native activation, getters or field writes.
use serde::{Deserialize, Serialize};
use serde_json::json;
use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::Mutex;
use training_mod_consts::mii::inventory;

const ARM: &str = "sd:/ultimate/TrainingModpack/mii_quit_observer.arm";
const ROOT: &str = "sd:/ultimate/TrainingModpack/mii_quit_ack";
const LIMIT: usize = 8192;
// Pristine 13.0.5 ranges; generated from the bound ELF, before installing any of these hooks.
const CODE: &[(usize, usize, u64)] = &[
    (0x376ded0, 0x198, 0x6c067722257c0af4),
    (0x23ebbf0, 0x90, 0x94048fba53dc4b60),
    (0x23ecdc0, 0x100, 0xcf1c1ffc043bbfd3),

    (0x13e8a60, 0x2d4, 0x2ac43d18f789c8b4),
    (0x13e5110, 0x10c, 0xc9989a5a8f2b5dfd),
    (0x13e6010, 0x1d0, 0x99090198d2a89497),
    (0x13e6cb0, 0xe0, 0x1384682e6c1048dd),
    (0x13e6fe0, 0x44, 0x3d2d22e1af105bd4),
    (0x376bce0, 0x1030, 0x4548a2590a24cc44),
    (0x376d5d0, 0x16c, 0x39f7905211b5b964),
    (0x376d860, 0x8, 0xf19620b16e1d776a),
    (0x1bb7bd0, 0x34, 0xe9a86bd48aedc0f6),
];
static BASE: AtomicUsize = AtomicUsize::new(0);
static ARMED: AtomicBool = AtomicBool::new(false);
static CLOSED: AtomicBool = AtomicBool::new(false);
static DRAIN_PENDING: AtomicBool = AtomicBool::new(false);
// Numeric filters only; cached identities are never dereferenced.
static STATE_OBJECT: AtomicUsize = AtomicUsize::new(0);
static SELECTOR: AtomicUsize = AtomicUsize::new(0);
static FINISH_BUTTON: AtomicUsize = AtomicUsize::new(0);
static FAULT: AtomicU8 = AtomicU8::new(0);
static READERS: AtomicUsize = AtomicUsize::new(0);
static STATE: Mutex<Trace> = Mutex::new(Trace::empty());

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arm {
    schema: u32,
    run: String,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize)]
struct Button {
    pointer: usize,
    vtable: usize,
    listener: usize,
    index: i32,
    predicate_function: usize,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize)]
struct Owner {
    object: usize,
    state_object: usize,
    resource: usize,
    manager: usize,
    selector: usize,
    button: Button,
}
#[derive(Clone, Copy, Default, Serialize)]
struct Ui {
    owner: Owner,
    thread: usize,
    callback_lr: usize,
    state: i32,
    previous: i32,
    age: u32,
    stack_index: i32,
    stack_state: i32,
    selected: i32,
    pending: i32,
    result: i32,
    exit_requested: u8,
    override_byte: u8,
}
#[derive(Clone, Copy, Default, Serialize)]
struct Update {
    selector: usize,
    // Opaque native x1 at function entry; never dereferenced, never fabricated on replay.
    native_argument: usize,
    sp: usize,
    caller_lr: usize,
    thread: usize,
}
#[derive(Clone, Copy, Default, Serialize)]
struct Decision {
    update: Update,
    return_thread: usize,
    button: Button,
    selected: i32,
    pending: i32,
    result: i32,
}
#[derive(Clone, Copy, Default, Serialize)]
struct Predicate {
    selector: usize,
    button: Button,
    caller_lr: usize,
    thread: usize,
    selected: i32,
    pending: i32,
    native_return: u32,
}
#[derive(Clone, Copy, Default, Serialize)]
struct Exit {
    state_object: usize,
    thread: usize,
    state: i32,
    age: u32,
    exit_requested: u8,
}
/// Numeric identities only. The filesystem thread must not dereference them.
#[derive(Clone, Copy, Serialize)]
pub(crate) struct Trace {
    run: [u8; 32],
    bound: Option<Ui>,
    predicates: [Option<Predicate>; 2],
    predicate_count: usize,
    decision: Option<Decision>,
    consumed: Option<Ui>,
    state20_seen: bool,
    exit: Option<Exit>,
    fault: u8,
    direct: Option<Action>,
    registered: Option<Action>,
    registered_count: u32,
    store_site: Option<usize>,
    drain: Option<Drain>,
    drain_attempted: bool,
    #[serde(skip)]
    update: Option<Update>,
}
impl Trace {
    const fn empty() -> Self {
        Self {
            run: [0; 32], bound: None, predicates: [None; 2], predicate_count: 0,
            decision: None, consumed: None, state20_seen: false, exit: None, fault: 0,
            update: None, direct: None, registered: None,
            registered_count: 0, store_site: None, drain: None, drain_attempted: false,
        }
    }
}
// Additional owned observations; no pointer below is dereferenced by the publisher.
#[derive(Clone, Copy, Default, Serialize)]
struct Action {
    site: usize,
    selector: usize,
    context: usize,
    input: usize,
    thread: usize,
    outer_sp: usize,
    caller_lr: usize,
    registration: usize,
    button: usize,
    action_index: u32,
    mapping: [u32; 2],
    generated: [u32; 3],
    enabled: u8,
    result_before: i32,
}
#[derive(Clone, Copy, Default, Serialize)]
struct Drain {
    context: usize,
    sequence: usize,
    scene: usize,
    parent_sequence: usize,
    thread: usize,
    callback_lr: usize,
    sp: usize,
    state: i32,
    age: u32,
    state_depth: i32,
    stacked_state: i32,
    child_manager: usize,
    child: usize,
    child_stack: [usize; 2],
    request_queue: usize,
    queued_requests: usize,
    mode_byte: u8,
    reason_before_postgame: u8,
}

unsafe fn action(f: usize, context: usize, index: u32, site: usize, frame: (usize, usize),
    registration: usize, b: usize) -> Result<Action, u8> {
    if !aligned(context) || index >= 51 { return Err(6); }
    let u: usize = rd(context, 8);
    if !aligned(u) { return Err(6); }
    let off = index as usize * 4;
    Ok(Action { site, selector: f, context, input: u,
        thread: skyline::nn::os::GetCurrentThread() as usize, outer_sp: frame.0, caller_lr: frame.1,
        registration, button: b, action_index: index,
        mapping: [rd(u, off), rd(u, 0xcc + off)],
        generated: [rd(u, 0x198 + off), rd(u, 0x264 + off), rd(u, 0x330 + off)],
        enabled: rd(u, 0x4aa), result_before: rd(f, 0x25c) })
}

// Safe span AFTER native enabled/action4 checks. The direct store's own short block
// cannot carry a20-byte hook. This branch, prior result=-1 and the paired normal
// return result0 support an inference only; indirect writers are not excluded.
#[skyline::hook(offset = 0x376c32c, inline)]
unsafe fn direct_action(ctx: &mut InlineCtx) {
    let f = ctx.registers[19].x() as usize;
    if !active() || f == 0 || f != SELECTOR.load(Ordering::Acquire) { return; }
    let Some(_reader) = ReadGuard::enter() else { return; };
    let sp = ctx.sp.x() as usize;
    if sp == 0 || !sp.is_multiple_of(16) { fault(6); return; }
    let observed = action(f, ctx.registers[20].x() as usize, 4, 0x376c32c,
        (sp, rd(sp, 0x98)), 0, 0);
    capture(|s| {
        if s.decision.is_none() { s.direct = Some(observed?); }
        Ok(())
    });
}

// Positive registered-action epilogue, before any callee-saved restoration.
// w19 is the selected button index; x24 is its actual registration row.
#[skyline::hook(offset = 0x376e044, inline)]
unsafe fn registered_action(ctx: &mut InlineCtx) {
    if ctx.registers[19].w() != 0 { return; }
    let f = (ctx.registers[23].x() as usize).saturating_sub(0x1b8);
    if !active() || f == 0 || f != SELECTOR.load(Ordering::Acquire) { return; }
    let Some(_reader) = ReadGuard::enter() else { return; };
    let sp = ctx.sp.x() as usize;
    let row = ctx.registers[24].x() as usize;
    if sp == 0 || !sp.is_multiple_of(16) || !aligned(row) { fault(6); return; }
    let lr: usize = rd(sp, 0x38);
    let base = BASE.load(Ordering::Acquire);
    if ![0x376c540, 0x376c768, 0x376c784].iter().any(|v| lr == base + v) { fault(6); return; }
    let observed = action(f, ctx.registers[20].x() as usize, rd(row, 0x20), 0x376e044,
        (sp + 0x40, lr), row, ctx.registers[21].x() as usize);
    capture(|s| {
        if s.decision.is_some() { return Ok(()); }
        s.registered = Some(observed?);
        s.registered_count = s.registered_count.saturating_add(1);
        Ok(())
    });
}

// One span-qualified registered store. The alternate short store at376c79c is
// not patched. This is a pre-store record; normal-return result proves completion.
#[skyline::hook(offset = 0x376c6f0, inline)]
unsafe fn registered_store(ctx: &mut InlineCtx) {
    let f = ctx.registers[19].x() as usize;
    if !active() || f == 0 || f != SELECTOR.load(Ordering::Acquire) || ctx.registers[8].w() != 0 { return; }
    let Some(_reader) = ReadGuard::enter() else { return; };
    let thread = skyline::nn::os::GetCurrentThread() as usize;
    let sp = ctx.sp.x() as usize;
    capture(|s| {
        if s.decision.is_some() { return Ok(()); }
        if s.update.is_none_or(|u| u.thread != thread || u.sp != sp + 0xa0) { return Err(6); }
        s.store_site = Some(0x376c6f0);
        Ok(())
    });
}

// Current Training state5 callback AFTER the native child/stack/queue checks and
// BEFORE postgame23eea60. x0=C, x20=S, x30=the actual registered dispatcher return.
#[skyline::hook(offset = 0x23ebc44, inline)]
unsafe fn training_drained(ctx: &mut InlineCtx) {
    if !DRAIN_PENDING.load(Ordering::Acquire) { return; }
    let base = BASE.load(Ordering::Acquire);
    let lr = ctx.registers[30].x() as usize;
    if lr != base + 0x23ecea8 { return; }
    if !DRAIN_PENDING.swap(false, Ordering::AcqRel) { return; }
    let current = (|| -> Result<Drain, u8> {
        let c = ctx.registers[0].x() as usize;
        let s = ctx.registers[20].x() as usize;
        let scene = s.checked_sub(0x50).ok_or(7u8)?;
        if !aligned(c) || !aligned(s) || !aligned(scene) || rd::<usize>(c, 0) != s
            || rd::<usize>(c, 8) != c || rd::<usize>(scene, 0xe8) != c
            || rd::<usize>(scene, 0) != base + 0x50bad50 { return Err(7); }
        let state: i32 = rd(c, 0xf0); let age: u32 = rd(c, 0xf4); let depth: i32 = rd(c, 0xec);
        if state != 5 || age == 0 || !(0..19).contains(&depth) { return Err(7); }
        let stacked: i32 = rd(c, 0xa0 + depth as usize * 4);
        let manager: usize = rd(s, 0x28); let queue: usize = rd(s, 0x20);
        if stacked != 5 || !aligned(manager) || !aligned(queue) { return Err(7); }
        let child: usize = rd(manager, 0x60); let begin: usize = rd(manager, 0x30);
        let end: usize = rd(manager, 0x38); let queued: usize = rd(queue, 0x10);
        if child != 0 || begin != end || queued != 0 { return Err(7); }
        Ok(Drain { context: c, sequence: s, scene, parent_sequence: rd(s, 0x18),
            thread: skyline::nn::os::GetCurrentThread() as usize, callback_lr: lr,
            sp: ctx.sp.x() as usize, state, age, state_depth: depth, stacked_state: stacked,
            child_manager: manager, child, child_stack: [begin, end], request_queue: queue,
            queued_requests: queued, mode_byte: rd(base, 0x52c51b0),
            reason_before_postgame: rd(base, 0x52e8b42) })
    })();
    let trace = {
        let Ok(mut s) = STATE.try_lock() else { fault(1); return; };
        s.drain_attempted = true;
        match current {
            Ok(v) => s.drain = Some(v),
            _ => fault(7),
        }
        s.fault = FAULT.load(Ordering::Acquire);
        *s
    };
    crate::common::filesystem::write_mii_quit(Box::new(trace));
}

fn aligned(p: usize) -> bool { p != 0 && p.is_multiple_of(8) }
unsafe fn rd<T: Copy>(p: usize, off: usize) -> T {
    std::ptr::read_volatile((p + off) as *const T)
}
fn fault(code: u8) { let _ = FAULT.compare_exchange(0, code, Ordering::AcqRel, Ordering::Acquire); }
fn active() -> bool { ARMED.load(Ordering::Acquire) && !CLOSED.load(Ordering::Acquire) }
// Publisher closes admission, then defers if a prior capture is still in flight. No waiting.
struct ReadGuard;
impl ReadGuard {
    fn enter() -> Option<Self> {
        if !active() { return None; }
        READERS.fetch_add(1, Ordering::AcqRel);
        let guard = Self;
        if active() { Some(guard) } else { None }
    }
}
impl Drop for ReadGuard {
    fn drop(&mut self) { READERS.fetch_sub(1, Ordering::Release); }
}
fn capture(f: impl FnOnce(&mut Trace) -> Result<(), u8>) {
    if !active() { return; }
    let Ok(mut s) = STATE.try_lock() else { fault(1); return; };
    if !active() { return; }
    if let Err(code) = f(&mut s) { fault(code); }
}

unsafe fn button(p: usize) -> Result<Button, u8> {
    if !aligned(p) { return Err(2); }
    let vt: usize = rd(p, 0);
    if !aligned(vt) { return Err(2); }
    Ok(Button { pointer: p, vtable: vt, listener: rd(p, 0x150), index: rd(p, 0x1a0),
        predicate_function: rd(vt, 0x230) })
}
// Native map F+1b8: node key+20, value+28. Only index0, at most16 current native nodes.
// Failure is incomplete diagnostic evidence; no fallback to a cached button or native getter.
unsafe fn finish_button(f: usize) -> Result<Button, u8> {
    let mut n: usize = rd(f, 0x1b8);
    for _ in 0..16 {
        if !aligned(n) { return Err(2); }
        let key: i32 = rd(n, 0x20);
        if key == 0 { return button(rd(n, 0x28)); }
        n = rd(n, if key > 0 { 0 } else { 8 });
    }
    Err(2)
}
// T is the current x0 at a registered native callback entry from the bound dispatcher.
// No value retained in Trace is used as a dereference source.
unsafe fn ui(t: usize, thread: usize, callback_lr: usize) -> Result<Ui, u8> {
    let base = BASE.load(Ordering::Acquire);
    if !aligned(t) || rd::<u8>(base, 0x52c51b0) != 0x20 { return Err(2); }
    let object: usize = rd(t, 0);
    let resource: usize = rd(t, 8);
    if !aligned(object) || !aligned(resource) || rd::<usize>(object, 0) != base + 0x5065b68
        || rd::<usize>(object, 0x50) != t { return Err(2); }
    let manager: usize = rd(resource, 0x80);
    if !aligned(manager) { return Err(2); }
    let selector: usize = rd(manager, 0x7b8);
    if !aligned(selector) || rd::<usize>(selector, 0) != base + 0x523d7f8
        || rd::<usize>(selector, 0x20) != base + 0x523d8b0 { return Err(2); }
    let button = finish_button(selector)?;
    if button.index != 0 || button.listener != selector + 0x20 { return Err(2); }
    let index: i32 = rd(t, 0x1fc);
    if !(0..19).contains(&index) { return Err(2); }
    Ok(Ui { owner: Owner { object, state_object: t, resource, manager, selector, button },
        thread, callback_lr, state: rd(t, 0x200), previous: rd(t, 0x208), age: rd(t, 0x204),
        stack_index: index, stack_state: rd(t, 0x1b0 + index as usize * 4),
        selected: rd(selector, 0x250), pending: rd(selector, 0x258), result: rd(selector, 0x25c),
        exit_requested: rd(t, 0x22d), override_byte: rd(t, 0x22a) })
}

// Three registered entries replace the unsafe adjacent pre-BLR hook. Incoming x30 proves
// the exact dispatcher call; all seven hook sites have separately bound 20-byte spans.
unsafe fn before_state(ctx: &InlineCtx, expected_state: i32) {
    let Some(_reader) = ReadGuard::enter() else { return; };
    let base = BASE.load(Ordering::Acquire);
    let callback_lr = ctx.registers[30].x() as usize;
    if callback_lr != base + 0x13e8d08 { return; }
    let t = ctx.registers[0].x() as usize;
    let bound_t = STATE_OBJECT.load(Ordering::Acquire);
    if bound_t != 0 && t != bound_t { return; }
    if rd::<u8>(base, 0x52c51b0) != 0x20 { return; }
    if !aligned(t) { if bound_t != 0 { fault(2); } return; }
    let state: i32 = rd(t, 0x200);
    if state != expected_state { if bound_t != 0 { fault(2); } return; }
    let thread = skyline::nn::os::GetCurrentThread() as usize;
    // State20 may have already closed UI resources. Compare the CURRENT native T
    // argument numerically; never traverse old selector/button/resource identities here.
    if state == 20 {
        if bound_t == 0 { return; }
        capture(|s| {
            let Some(first) = s.bound else { return Ok(()); };
            if first.owner.state_object != t { return Err(2); }
            if first.thread != thread { return Err(3); }
            if s.consumed.is_some() { s.state20_seen = true; }
            Ok(())
        });
        return;
    }
    // Construction/other unqualified UI before the first full binding is quiet, not a fault.
    let current = match ui(t, thread, callback_lr) {
        Ok(v) => v, Err(e) => { if bound_t != 0 { fault(e); } return; }
    };
    if thread == 0 { if bound_t != 0 { fault(3); } return; }
    capture(|s| {
        if s.bound.is_none() {
            s.bound = Some(current);
            FINISH_BUTTON.store(current.owner.button.pointer, Ordering::Release);
            STATE_OBJECT.store(t, Ordering::Release);
            SELECTOR.store(current.owner.selector, Ordering::Release);
        }
        let first = s.bound.ok_or(2u8)?;
        if first.owner != current.owner { return Err(2); }
        if first.thread != thread || thread == 0 { return Err(3); }
        if current.result == 0 && s.consumed.is_none() {
            s.consumed = Some(current);
        }
        Ok(())
    });
}

#[skyline::hook(offset = 0x13e5110, inline)]
unsafe fn state5_entry(ctx: &mut InlineCtx) { before_state(ctx, 5); }
#[skyline::hook(offset = 0x13e6010, inline)]
unsafe fn state8_entry(ctx: &mut InlineCtx) { before_state(ctx, 8); }
#[skyline::hook(offset = 0x13e6cb0, inline)]
unsafe fn state20_entry(ctx: &mut InlineCtx) { before_state(ctx, 20); }

// Entry captures only current arguments; x20 is later reused as a button in this function.
#[skyline::hook(offset = 0x376bce0, inline)]
unsafe fn update_entry(ctx: &mut InlineCtx) {
    let f = ctx.registers[0].x() as usize;
    if !active() || f == 0 || f != SELECTOR.load(Ordering::Acquire) { return; }
    let Some(_reader) = ReadGuard::enter() else { return; };
    let context = ctx.registers[1].x() as usize;
    let item = Update { selector: f, native_argument: context,
        sp: ctx.sp.x() as usize, caller_lr: ctx.registers[30].x() as usize,
        thread: skyline::nn::os::GetCurrentThread() as usize };
    capture(|s| {
        if s.decision.is_some() { return Ok(()); }
        if s.update.is_some() || !item.sp.is_multiple_of(16) || item.sp == 0 { return Err(4); }
        s.direct = None; s.registered = None; s.registered_count = 0; s.store_site = None;
        s.update = Some(item);
        Ok(())
    });
}

// Common normal epilogue, before restoring x19. Native frame is 0xa0 bytes; x19 remains F.
#[skyline::hook(offset = 0x376cc3c, inline)]
unsafe fn update_return(ctx: &mut InlineCtx) {
    let f = ctx.registers[19].x() as usize;
    if !active() || f == 0 || f != SELECTOR.load(Ordering::Acquire) { return; }
    let Some(_reader) = ReadGuard::enter() else { return; };
    let thread = skyline::nn::os::GetCurrentThread() as usize;
    let sp = ctx.sp.x() as usize;
    let result: i32 = rd(f, 0x25c);
    let selected = rd(f, 0x250);
    let pending = rd(f, 0x258);
    let b = if result == 0 { Some(finish_button(f)) } else { None };
    capture(|s| {
        if s.decision.is_some() { return Ok(()); }
        let Some(entry) = s.update.take() else { return Err(4); };
        if entry.sp != sp + 0xa0 || entry.selector != f || entry.thread != thread { return Err(4); }
        if let Some(b) = b {
            s.decision = Some(Decision { update: entry, return_thread: thread, button: b?,
                selected, pending, result });
        }
        Ok(())
    });
}

// Immediately AFTER the existing button virtual+230 BLR376d6d8; w0 is its actual return.
// x20=F/x19=B are callee-saved here. Original caller LR is saved at current SP+28.
#[skyline::hook(offset = 0x376d6dc, inline)]
unsafe fn predicate_return(ctx: &mut InlineCtx) {
    let f = ctx.registers[20].x() as usize;
    if !active() || f == 0 || f != SELECTOR.load(Ordering::Acquire) { return; }
    let p = ctx.registers[19].x() as usize;
    // This listener serves other buttons too. Ignore them before dereferencing their fields.
    if p == 0 || p != FINISH_BUTTON.load(Ordering::Acquire) { return; }
    let Some(_reader) = ReadGuard::enter() else { return; };
    let sp = ctx.sp.x() as usize;
    if sp == 0 || !sp.is_multiple_of(16) { fault(2); return; }
    let b = match button(p) {
        Ok(v) => v, Err(e) => { fault(e); return; }
    };
    let event = Predicate { selector: f, button: b, caller_lr: rd(sp, 0x28),
        thread: skyline::nn::os::GetCurrentThread() as usize, selected: rd(f, 0x250),
        pending: rd(f, 0x258), native_return: ctx.registers[0].w() };
    capture(|s| {
        let first = s.bound.ok_or(2u8)?;
        if b != first.owner.button { return Err(2); }
        if s.predicate_count == s.predicates.len() { return Err(5); }
        s.predicates[s.predicate_count] = Some(event);
        s.predicate_count += 1;
        Ok(())
    });
}

// After the real native UI callback, before its next T+204 access. No native lock is acquired.
// This is the only enqueue seam; no capture hook performs filesystem operations or formatting.
#[skyline::hook(offset = 0x13e8d08, inline)]
unsafe fn after_state(ctx: &mut InlineCtx) {
    if !active() { return; }
    let t = ctx.registers[20].x() as usize;
    let bound_t = STATE_OBJECT.load(Ordering::Acquire);
    if bound_t == 0 || t != bound_t { return; }
    let thread = skyline::nn::os::GetCurrentThread() as usize;
    let trace = {
        let Ok(mut s) = STATE.try_lock() else { fault(1); return; };
        if CLOSED.load(Ordering::Acquire) { return; }
        if !aligned(t) || s.bound.is_none_or(|b| b.owner.state_object != t) {
            fault(2);
        } else if s.bound.is_none_or(|b| b.thread != thread) {
            fault(3);
        } else if s.state20_seen && s.consumed.is_some() {
            // No hook at internal branch13e6fe0. Read only CURRENT live T at the common
            // dispatcher return. This observes state22/exit byte, not callback invocation.
            let state: i32 = rd(t, 0x200);
            let exit_requested: u8 = rd(t, 0x22d);
            if state == 22 && exit_requested == 1 {
                s.exit = Some(Exit { state_object: t, thread, state,
                    age: rd(t, 0x204), exit_requested });
            }
        }
        if s.exit.is_none() && FAULT.load(Ordering::Acquire) == 0 { return; }
        CLOSED.store(true, Ordering::Release);
        if READERS.load(Ordering::Acquire) != 0 {
            CLOSED.store(false, Ordering::Release);
            return;
        }
        SELECTOR.store(0, Ordering::Release);
        FINISH_BUTTON.store(0, Ordering::Release);
        STATE_OBJECT.store(0, Ordering::Release);
        if s.exit.is_some() && FAULT.load(Ordering::Acquire) == 0 { DRAIN_PENDING.store(true, Ordering::Release); }
        s.fault = FAULT.load(Ordering::Acquire);
        *s
    };
    crate::common::filesystem::write_mii_quit(Box::new(trace));
}

fn invalid(text: &'static str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, text) }
fn publish(run: &[u8; 32], name: &str, value: &serde_json::Value) -> io::Result<()> {
    let run = std::str::from_utf8(run).map_err(|_| invalid("run encoding"))?;
    let bytes = serde_json::to_vec(value).map_err(|_| invalid("serialization"))?;
    if bytes.len() > LIMIT { return Err(invalid("document exceeds8192")); }
    let stem = format!("{ROOT}/{run}/{name}");
    let ready = format!("{stem}.ack");
    match fs::metadata(&ready) {
        Ok(_) => return Err(invalid("ack already exists")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {},
        Err(e) => return Err(e),
    }
    let part = format!("{stem}.part");
    let mut file = OpenOptions::new().write(true).create(true).create_new(true).truncate(false).open(&part)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(part, ready)
}

pub(crate) fn write_trace(s: &Trace) {
    // The deployed inline engine clobbers live x16/x17 at the removed input-row site.
    // Action snapshots remain observations; complete never certifies input routing.
    // A missing deferred callback is not fabricated or required for the direct-input route.
    let complete = s.fault == 0 && s.state20_seen && s.bound.is_some()
        && s.decision.is_some_and(|d| d.result == 0 && s.bound.is_some_and(|b|
            b.owner.selector == d.update.selector && b.owner.button == d.button
            && b.thread == d.update.thread && b.thread == d.return_thread))
        && s.consumed.is_some_and(|u| u.result == 0)
        && s.exit.is_some_and(|u| u.state == 22 && u.exit_requested == 1);
    let document = json!({"schema":1,"run":std::str::from_utf8(&s.run).unwrap_or(""),
        "event":if s.drain_attempted { "quit_training_drain" } else { "quit_trace" },
        "complete":if s.drain_attempted { complete && s.drain.is_some() } else { complete },
        "publication_provenance":serde_json::Value::Null,
        "input_routing_provenance_available":false,
        "input_routing_unavailable_reason":"unsafe_input_row_hook_removed",
        "input_routing_semantics_unqualified":true,"trace":s,
        "fault_names":["none","contended","current_binding","ui_thread_changed",
            "update_pair","predicate_limit","input_provenance","training_drain_binding"],
        "interpretation":{"deferred_predicate_observed":s.predicate_count != 0,
            "predicate_meaning_unqualified":true,"semantic_activation_abi_qualified":false,
            "direct_store_indirect_writers_excluded":false,
            "native_parent_drained_not_claimed":s.drain.is_none(),"state22_callback_invocation_not_claimed":true},
        "limits":{"input_rows_max":0,"positive_action_records_max":2,"documents_max":3,"no_native_writes":true,
            "publication_site":if s.drain_attempted { 0x23ebc44usize } else { 0x13e8d08usize }}});
    let name = if s.drain_attempted { "02-drain" } else { "01-trace" };
    if let Err(e) = publish(&s.run, name, &document) {
        crate::logging::diagnostic(format_args!("[MiiQuit] publication_failed: {e}"));
    }
}

pub(crate) fn init() {
    let result = (|| -> io::Result<()> {
        let mut bytes = Vec::new();
        File::open(ARM)?.take(129).read_to_end(&mut bytes)?;
        if bytes.len() > 128 { return Err(invalid("arm exceeds128")); }
        let arm: Arm = serde_json::from_slice(&bytes).map_err(|_| invalid("arm JSON"))?;
        if arm.schema != 1 || arm.run.len() != 32 || !arm.run.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) { return Err(invalid("schema/run")); }
        let base = unsafe { getRegionAddress(Region::Text) as usize };
        for &(offset, len, expected) in CODE {
            let got = unsafe { inventory::fnv1a64(std::slice::from_raw_parts((base + offset) as *const u8, len)) };
            if got != expected { return Err(invalid("pristine native fingerprint")); }
        }
        let mut run = [0; 32]; run.copy_from_slice(arm.run.as_bytes());
        fs::create_dir_all(ROOT)?;
        fs::create_dir(format!("{ROOT}/{}", arm.run))?;
        publish(&run, "00-armed", &json!({"schema":1,"run":arm.run,"event":"armed",
            "fingerprints_matched":true,"no_native_writes":true,"documents_max":3,
            "input_rows_max":0,"input_routing_provenance_available":false,
            "input_routing_unavailable_reason":"unsafe_input_row_hook_removed",
            "positive_action_records_max":2,"separate_from_mii_test_eight_events":true}))?;
        crate::common::filesystem::init();
        STATE.lock().unwrap().run = run;
        BASE.store(base, Ordering::Release);
        skyline::install_hooks!(state5_entry, state8_entry, state20_entry,
            update_entry, update_return, predicate_return, after_state,
            direct_action, registered_action, registered_store, training_drained);
        ARMED.store(true, Ordering::Release);
        Ok(())
    })();
    if let Err(e) = result {
        if e.kind() != io::ErrorKind::NotFound {
            crate::logging::diagnostic(format_args!("[MiiQuit] init_refused: {e}"));
        }
    }
}
