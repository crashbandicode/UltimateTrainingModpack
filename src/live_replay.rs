//! Optional 13.0.5 native-record capture and committed replay-save association.
//! Native pointers are read only on their owning callbacks. Stored addresses are
//! equality tokens, never dereferenced later; only owned values reach the writer.
//!
//! Revision 28: the hooks are resident whenever the INI says on or a Deluxe
//! host registered the capture row. Each accepted native record is owned here
//! (`RecordOwnership`) whether or not it is captured; the packed `CaptureToggle`
//! latches the arm state at segment-0 start and is released only by an event
//! matching that owner. A preference edited mid-record applies at that release.

use std::collections::VecDeque;
use std::sync::{atomic::{AtomicUsize, Ordering}, Arc, Mutex};

use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use smash::{app::{self, lua_bind::WorkModule, Fighter}, lib::lua_const::*};
use training_mod_macros::{CaptureToggle, LiveCapturePool, LiveFrame, LiveSession, NativeRecordKey,
    RecordOwner, RecordOwnership, MAX_RETAINED_CAPTURES};

static BASE: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "named-input-trace")]
static CLOCK_TRACE_LEFT: AtomicUsize = AtomicUsize::new(0);
static CAPTURE: Mutex<Capture> = Mutex::new(Capture {
    native_state: 0, epoch: 0, clock: None, pool: LiveCapturePool::new(), owner: RecordOwnership::new(),
    pending: [None; 8], saves: VecDeque::new(),
});

struct Capture {
    native_state: usize,
    epoch: u64,
    clock: Option<u32>,
    pool: LiveCapturePool,
    /// Accepted native record, armed or not, independent of the pool (R1).
    owner: RecordOwnership,
    pending: [Option<Pending>; 8],
    saves: VecDeque<BoundSave>,
}

fn toggle() -> &'static CaptureToggle { crate::replay_store::toggle() }

fn on_off(on: bool) -> &'static str { if on { "on" } else { "off" } }

#[derive(Clone, Copy)]
struct Pending {
    key: NativeRecordKey,
    tick: u32,
    body: u32,
    sample: LiveFrame,
}

struct BoundSave {
    actor: usize,
    payload: usize,
    index: u64,
    queued: bool,
    session: Arc<LiveSession>,
    miis: Option<training_mod_consts::mii::replay::ReplayMiis>,
}

#[derive(Clone, Copy)]
struct Native {
    state: usize,
    index: u32,
    tick: u32,
    recording: bool,
}

#[cfg(feature = "named-input-trace")]
fn trace_clock(capture: &Capture, phase: &str, entry: i32) {
    if let Ok(left) = CLOCK_TRACE_LEFT.fetch_update(Ordering::Relaxed, Ordering::Relaxed,
        |left| left.checked_sub(1)) {
        let pending = capture.pending.iter().enumerate().fold(0u16, |mask, (entry, value)|
            mask | (u16::from(value.is_some()) << entry));
        println!("[LiveCaptureClock] seq={} phase={} entry={} epoch={} counter={:?} pending={:#x} key={:?}",
            10_000 - left, phase, entry, capture.epoch, capture.clock, pending, capture.pool.active_key());
    }
}

unsafe fn read<T: Copy>(at: usize) -> T { std::ptr::read_volatile(at as *const T) }

/// Read a state borrowed by an executing native method, never a remembered pointer
/// or a global manager reached from an unrelated fighter/save callback.
unsafe fn native(state: usize) -> Option<Native> {
    let base = BASE.load(Ordering::Acquire);
    if base == 0 { return None; }
    if state == 0 || read::<usize>(state) != base + 0x509ea18
        || read::<u8>(state + 0x20) == 0 { return None; }
    let pool: usize = read(state + 0x18);
    if pool == 0 { return None; }
    let count: u64 = read(pool);
    if count == 0 || count > 0x600000 / 24 { return None; }
    Some(Native { state, index: (count - 1) as u32, tick: read(state + 0x200078),
        recording: read::<u32>(state + 0x24) == 2 })
}

unsafe fn borrowed_state(interface: usize) -> Option<usize> {
    let base = BASE.load(Ordering::Acquire);
    if base == 0 || interface == 0 || read::<usize>(interface) != base + 0x509e8f8 { return None; }
    let state = read::<usize>(interface + 8);
    (state != 0 && read::<usize>(state) == base + 0x509ea18).then_some(state)
}

impl Capture {
    /// Every owner release ends the toggle's latched record. A request latched
    /// during the record is reported here, on the game thread, never from the
    /// draw-thread callback (operator decision Q2).
    fn release(&self, released: Option<RecordOwner>, reason: &str) {
        let Some(owner) = released else { return };
        if let Some(ended) = toggle().end_record().filter(|ended| ended.pending_applied()) {
            println!("[ReplayCapture] toggle applied capture={} source=pending index={} epoch={} reason={}",
                on_off(ended.desired), owner.index, owner.epoch, reason);
        }
    }

    fn reset_pool(&mut self, state: usize) {
        self.epoch += 1;
        self.clock = None;
        self.native_state = state;
        self.pool.clear();
        self.pending = [None; 8];
        // A save already copied by the native actor owns its immutable snapshot.
        // Clearing/reusing native records must not invalidate that pending save.
        let released = self.owner.release_all();
        self.release(released, "reset");
    }

    fn invalidate(&mut self, native: Native) {
        if self.native_state != native.state { return; }
        self.epoch += 1;
        self.clock = None;
        self.pending = [None; 8];
        self.pool.invalidate_record(native.index);
        let released = self.owner.release_record(native.state, native.index);
        self.release(released, "replace");
    }

    fn stop(&mut self, state: usize, partial: bool) {
        if self.native_state != state { return; }
        #[cfg(feature = "named-input-trace")]
        println!("[LiveCaptureClock] stop epoch={} counter={:?} partial={} pending={}",
            self.epoch, self.clock, partial, self.pending.iter().filter(|item| item.is_some()).count());
        self.epoch += 1;
        self.clock = None;
        if partial || self.pending.iter().any(Option::is_some) {
            self.pool.finish_partial();
        } else {
            self.pool.finish_record();
        }
        self.pending = [None; 8];
        let released = self.owner.release_state(state);
        self.release(released, if partial { "reset_recording" } else { "stop" });
    }

    fn forget_actor(&mut self, actor: usize) {
        self.saves.retain(|save| save.actor != actor);
    }
}

#[skyline::hook(offset = 0x17587a0)]
unsafe fn construct_records(interface: usize) {
    CAPTURE.lock().unwrap().reset_pool(0);
    original!()(interface);
}

#[skyline::hook(offset = 0x1759110)]
unsafe fn clear_records(interface: usize) {
    CAPTURE.lock().unwrap().reset_pool(0);
    original!()(interface);
}

unsafe fn invalidate_owner(manager: usize) {
    if let Some(state) = borrowed_state(manager + 0x588) {
        let mut capture = CAPTURE.lock().unwrap();
        if capture.native_state == state { capture.reset_pool(0); }
    }
}

// Both manager destructors release the embedded interface's state before returning.
#[skyline::hook(offset = 0x1754550)]
unsafe fn destroy_records(manager: usize) {
    invalidate_owner(manager);
    original!()(manager);
}

#[skyline::hook(offset = 0x17545f0)]
unsafe fn delete_records(manager: usize) {
    invalidate_owner(manager);
    original!()(manager);
}

// First state store on the validated segment-zero path. Non-mutating rejected
// attempts never reach this point. x0 is still the live native state argument.
#[skyline::hook(offset = 0x175ba4c, inline)]
unsafe fn replace_record(ctx: &mut InlineCtx) {
    if let Some(native) = native(ctx.registers[0].x() as usize) {
        CAPTURE.lock().unwrap().invalidate(native);
    }
}

#[skyline::hook(offset = 0x175b980)]
unsafe fn start_recording(state: usize, segment: u32) -> u32 {
    let result = original!()(state, segment);
    if result == 0 { return result; }
    let Some(native) = native(state).filter(|native| native.recording) else {
        return result;
    };
    let mut capture = CAPTURE.lock().unwrap();
    if capture.native_state != state { capture.reset_pool(state); }
    capture.epoch += 1;
    capture.clock = None;
    capture.pending = [None; 8];
    let epoch = capture.epoch;
    if segment as u8 == 0 {
        // Ownership is tracked for armed and unarmed records alike (R1). A record
        // still owned here never saw its end; report it before latching the new one.
        let displaced = capture.owner.begin(RecordOwner { state, index: native.index, epoch });
        capture.release(displaced, "restart");
        if toggle().begin_record() {
            let key = capture.pool.start_record(native.index);
            capture.clock = Some(native.tick);
            #[cfg(feature = "named-input-trace")]
            CLOCK_TRACE_LEFT.store(10_000, Ordering::Relaxed);
            println!("[ReplayCapture] record_start generation={} index={} tick={}",
                key.generation, key.index, native.tick);
            println!("[ReplayCapture] record_armed generation={} index={} capture=on",
                key.generation, key.index);
        } else {
            // Slot invalidation continues while off: an unarmed record must not
            // inherit an older armed key, so no later save can bind one (Q1/R1).
            capture.pool.invalidate_record(native.index);
            println!("[ReplayCapture] record_armed generation=0 index={} capture=off", native.index);
        }
    } else {
        // The native segment clock restarts. Do not concatenate separate timelines;
        // the owned record continues under its new native index.
        capture.pool.finish_partial();
        capture.owner.continue_segment(state, native.index, epoch);
    }
    result
}

#[skyline::hook(offset = 0x17594b0)]
unsafe fn reset_current_record(interface: usize, flags: u32) {
    if let Some(native) = borrowed_state(interface).and_then(|state| native(state)) {
        CAPTURE.lock().unwrap().invalidate(native);
    }
    original!()(interface, flags);
}

#[skyline::hook(offset = 0x175ac8c, inline)]
unsafe fn import_record(ctx: &mut InlineCtx) {
    // Validation has passed; native now clears state then appends to this slot.
    // x21 is the live state. Failed, non-mutating validation retains its capture.
    if let Some(native) = native(ctx.registers[21].x() as usize) {
        CAPTURE.lock().unwrap().invalidate(native);
    }
}

// These instructions clear native recording state. x8 is the live state being
// written; compare it only as a token. Inline observers do not change registers.
#[skyline::hook(offset = 0x14f37ec, inline)]
unsafe fn stop_recording(ctx: &mut InlineCtx) {
    CAPTURE.lock().unwrap().stop(ctx.registers[8].x() as usize, false);
}

#[skyline::hook(offset = 0x1503af8, inline)]
unsafe fn stop_recording_alternate(ctx: &mut InlineCtx) {
    CAPTURE.lock().unwrap().stop(ctx.registers[8].x() as usize, false);
}

#[skyline::hook(offset = 0x134be18, inline)]
unsafe fn reset_recording(ctx: &mut InlineCtx) {
    CAPTURE.lock().unwrap().stop(ctx.registers[8].x() as usize, true);
}

// Native's successful FRAM publication appends the complete frame buffer to the
// current record, then resets its cursor here. It does not clear state+0x24.
// Failed/duplicate publication paths bypass this instruction. x22 is the state
// borrowed by this executing method; no remembered native pointer is traversed.
#[skyline::hook(offset = 0x175b1bc, inline)]
unsafe fn publish_record_frames(ctx: &mut InlineCtx) {
    let Some(native) = native(ctx.registers[22].x() as usize) else { return };
    let mut capture = CAPTURE.lock().unwrap();
    if capture.native_state != native.state || !native.recording { return; }
    // Armed or not, only the owned record's own publication ends it (R1); a pool
    // key is absent for unarmed records and for armed ones already copied.
    if !capture.owner.owns_record(native.state, native.index) { return; }
    #[cfg(feature = "named-input-trace")]
    println!("[LiveCaptureClock] fram_publish generation={} index={} native_counter={}",
        capture.pool.active_key().map_or(0, |key| key.generation), native.index, native.tick);
    // Existing sealed tracks and an in-flight fighter pair still remain partial.
    // This only finalizes ownership; disk publication still requires native save success.
    capture.stop(native.state, false);
}

#[skyline::hook(offset = 0x175a230)]
unsafe fn serialize_frame(interface: usize) -> u32 {
    let state = borrowed_state(interface);
    let epoch = {
        let mut capture = CAPTURE.lock().unwrap();
        if state == Some(capture.native_state) {
            #[cfg(feature = "named-input-trace")]
            trace_clock(&capture, "serializer_enter", -1);
            capture.clock = None;
            Some(capture.epoch)
        } else { None }
    };
    let result = original!()(interface);
    if let (Some(state), Some(epoch)) = (state, epoch) {
        let observed = native(state);
        let mut capture = CAPTURE.lock().unwrap();
        if capture.epoch == epoch && capture.native_state == state {
            match observed {
                Some(native) if native.recording && capture.owner.owns_record(state, native.index) => {
                    // The clock advances only while the pool still samples this record;
                    // an unarmed owner, or an armed one already copied, just stays owned.
                    if capture.pool.active_key().is_some_and(|key| key.index == native.index) {
                        capture.clock = Some(native.tick);
                    }
                }
                _ => {
                    // The serializer clears state2 on native buffer exhaustion. A record
                    // this adapter never saw start is ended the same way (R1).
                    capture.pool.finish_partial();
                    capture.pending = [None; 8];
                    let released = capture.owner.release_state(state);
                    capture.release(released, "exhausted");
                }
            }
            #[cfg(feature = "named-input-trace")]
            trace_clock(&capture, "serializer_leave", -1);
        }
    }
    result
}

#[skyline::hook(offset = 0x143cd40)]
unsafe fn copy_record(actor: usize) {
    let epoch = {
        let mut capture = CAPTURE.lock().unwrap();
        capture.forget_actor(actor);
        capture.epoch
    };
    original!()(actor);
    // Native code may skip invalid records, so bind the index AFTER its copy.
    let index: u64 = read(actor + 0x190);
    let total: u64 = read(actor + 0x198);
    let payload: usize = read(actor + 0x110);
    let size: u32 = read(actor + 0x118);
    if index >= total || index > u32::MAX as u64 || payload == 0 || size < 944 + 7088
        || read::<u32>(payload + 944 + 4) != 0x4b4d4643 { return; }
    // Copy only the already-owned immutable header; row IDs are resolved later in Training.
    let header = std::slice::from_raw_parts((payload + 944) as *const u8,
        training_mod_consts::mii::replay::HEADER_LEN);
    let miis = training_mod_consts::mii::replay::ReplayMiis::parse_inner(header).map_err(|error|
        println!("[ReplayCapture] Mii metadata unavailable: {error}")).ok();
    let mut capture = CAPTURE.lock().unwrap();
    // Reject any lifecycle transition during native's copy. Never look up a new
    // incarnation and attach it to payload copied before the transition.
    if capture.epoch != epoch { return; }
    let Some(key) = capture.pool.key_for_index(index as u32) else { return };
    // Copying a still-active record only captures a prefix. Never label that
    // prefix complete merely because no explicit stop callback was observed.
    if capture.pool.active_key() == Some(key) { capture.pool.finish_partial(); }
    let Some(session) = capture.pool.snapshot(key) else { return };
    while capture.saves.len() >= MAX_RETAINED_CAPTURES { capture.saves.pop_front(); }
    capture.saves.push_back(BoundSave { actor, payload, index, queued: false, session, miis });
    println!("[ReplayCapture] save_bound generation={} index={}", key.generation, key.index);
}

// Factory publication: x0 is the newly initialized actor, before it is stored
// through the caller's holder. This fences allocator reuse without guessing an ABI.
#[skyline::hook(offset = 0x143e984, inline)]
unsafe fn create_save_actor(ctx: &mut InlineCtx) {
    CAPTURE.lock().unwrap().forget_actor(ctx.registers[0].x() as usize);
}

#[skyline::hook(offset = 0x143e9a0)]
unsafe fn destroy_save_actor(holder: usize) {
    // Native clears *holder and frees the actor, path strings and copied payload.
    CAPTURE.lock().unwrap().forget_actor(read::<usize>(holder));
    original!()(holder);
}

#[skyline::hook(offset = 0x143ea40)]
unsafe fn update_save_actor(actor: usize) -> u32 {
    let result = original!()(actor);
    // Native returns actor+0x108: zero while running, nonzero at terminal outcome.
    if result != 0 { CAPTURE.lock().unwrap().forget_actor(actor); }
    result
}

/// Native libc++ string layout is proven at this actor's path producer/consumer.
/// Copy only the bounded replay basename; a title is deliberately not an identity.
unsafe fn replay_id(actor: usize) -> Option<String> {
    let at = actor + 0x170;
    let tag: u8 = read(at);
    let (ptr, len) = if tag & 1 == 0 { (at + 1, (tag >> 1) as usize) }
        else { (read::<usize>(at + 16), read::<usize>(at + 8)) };
    if ptr == 0 || len == 0 || len > 128 || (tag & 1 == 0 && len > 22) { return None; }
    let path = std::str::from_utf8(std::slice::from_raw_parts(ptr as *const u8, len)).ok()?;
    let id = path.strip_prefix("save:/save_data/replay/")?;
    if id.is_empty() || id.len() > 96 || id.chars().any(|c| c.is_control() || c == '/' || c == '\\') {
        return None;
    }
    Some(id.to_owned())
}

unsafe fn replay_title(payload: usize) -> Option<String> {
    // 40 UTF16 units plus NUL, copied in the native 944-byte metadata prefix.
    let units = std::slice::from_raw_parts((payload + 0x0c) as *const u16, 41);
    let end = units.iter().position(|unit| *unit == 0)?;
    let title = String::from_utf16(&units[..end]).ok()?;
    (!title.is_empty() && !title.chars().any(char::is_control)).then_some(title)
}

/// 13.0.5 replay-save entry. The first four words are the prologue Skyline
/// overwrites when any plugin replace-hooks this function.
const SAVE_ENTRY: usize = 0x143e2a0;
const SAVE_ENTRY_WORDS: [u32; 8] = [
    0xa9be4ffc, 0xa9017bfd, 0x910043fd, 0xd10b03ff,
    0xb940fc08, 0xaa0003f3, 0x34000448, 0xb9411e68,
];
/// Fifth instruction, `ldr w8, [x0, #0xfc]`. It is the first instruction that
/// still runs in the original function after a 16-byte Skyline entry hook, and
/// x0 is still the save actor because the prologue does not write it.
const SAVE_BODY: usize = 0x143e2b0;
const SAVE_BODY_WORDS: [u32; 4] = [
    0xb940fc08, 0xaa0003f3, 0x34000448, 0xb9411e68,
];

const _: () = assert!(SAVE_BODY == SAVE_ENTRY + 0x10);
const _: () = assert!(
    SAVE_ENTRY_WORDS[4] == SAVE_BODY_WORDS[0]
        && SAVE_ENTRY_WORDS[5] == SAVE_BODY_WORDS[1]
        && SAVE_ENTRY_WORDS[6] == SAVE_BODY_WORDS[2]
        && SAVE_ENTRY_WORDS[7] == SAVE_BODY_WORDS[3]
);

/// Read the save actor before the native function consumes status 1.
unsafe fn observe_save(actor: usize) -> Option<(String, String, Arc<LiveSession>, Option<training_mod_consts::mii::replay::ReplayMiis>)> {
    if read::<u32>(actor + 0xfc) == 0 {
        // Entry queues operation8 and resets its status; an older operation's
        // status1 must never be accepted without seeing this exact queue boundary.
        let mut capture = CAPTURE.lock().unwrap();
        if let Some(save) = capture.saves.iter_mut().find(|save| save.actor == actor) {
            save.queued = save.payload == read::<usize>(actor + 0x110)
                && save.index == read::<u64>(actor + 0x190);
        }
        return None;
    }
    let status: u32 = read(actor + 0x11c);
    if status == 0 { return None; }
    let saved = {
        let mut capture = CAPTURE.lock().unwrap();
        capture.saves.iter().position(|save| save.actor == actor)
            .and_then(|index| capture.saves.remove(index))
    };
    let saved = saved?;
    if status == 1 && saved.queued && saved.payload == read::<usize>(actor + 0x110)
        && saved.index == read::<u64>(actor + 0x190)
        && read::<u32>(actor + 0x118) >= 944 + 7088 {
        if let (Some(id), Some(title)) = (replay_id(actor), replay_title(saved.payload)) {
            let header=std::slice::from_raw_parts((saved.payload+944) as *const u8,
                training_mod_consts::mii::replay::HEADER_LEN);
            let current=training_mod_consts::mii::replay::ReplayMiis::parse_inner(header).ok();
            let miis=if saved.miis==current {saved.miis} else {
                println!("[ReplayCapture] committed Mii metadata changed; exact association unavailable");None
            };
            return Some((id, title, saved.session, miis));
        }
        println!("[ReplayCapture] committed identity/title unavailable");
    } else {
        println!("[ReplayCapture] save not committed or association changed status={}", status);
    }
    None
}

#[skyline::hook(offset = 0x143e2a0)]
unsafe fn write_record(actor: usize) {
    const _: () = assert!(SAVE_ENTRY == 0x143e2a0);
    let committed = observe_save(actor);
    original!()(actor);
    if let Some((id, title, session, miis)) = committed {
        crate::replay_store::persist_committed(id, title, session, miis);
    }
}

#[skyline::hook(offset = 0x143e2b0, inline)]
unsafe fn write_record_shared(ctx: &mut InlineCtx) {
    const _: () = assert!(SAVE_BODY == 0x143e2b0);
    if let Some((id, title, session, miis)) = observe_save(ctx.registers[0].x() as usize) {
        crate::replay_store::persist_committed(id, title, session, miis);
    }
}

fn live_scene() -> bool {
    crate::common::game_mode::current().is_some_and(|mode|
        mode.initialized && mode.melee != 0x0d && mode.mode != 0x05010100)
        && unsafe { !crate::common::is_native_training_mode() && Fighter::is_ready_go() }
}

pub unsafe fn before_frame(fighter: &mut Fighter) {
    if BASE.load(Ordering::Acquire) == 0 { return; }
    // Sampling early-out (R1): an idle or unarmed record takes no mutex here.
    if !toggle().is_armed() { return; }
    let Some(module) = fighter.battle_object.module_accessor.as_mut() else { return };
    if app::utility::get_kind(module) == *FIGHTER_KIND_NANA { return; }
    let entry = WorkModule::get_int(module, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID);
    if !(0..8).contains(&entry) { return; }
    let mut capture = CAPTURE.lock().unwrap();
    capture.pending[entry as usize] = None;
    if !live_scene() { return; }
    let Some(tick) = capture.clock else { return };
    let Some(key) = capture.pool.active_key() else { return };
    if capture.pool.wants_sample(key, entry, tick) {
        let Some(sample) = crate::replay_capture::live_state(module) else {
            capture.pool.seal_entry(key, entry);
            println!("[ReplayCapture] native state unavailable entry={}", entry);
            return;
        };
        capture.pending[entry as usize] = Some(Pending { key, tick,
            body: module.battle_object_id, sample });
        #[cfg(feature = "named-input-trace")]
        trace_clock(&capture, "fighter_before", entry);
    }
}

pub unsafe fn after_frame(fighter: &mut Fighter) {
    if BASE.load(Ordering::Acquire) == 0 { return; }
    // Every owner release clears `pending` under the mutex, so a record that ended
    // between the two callbacks leaves nothing behind for this early-out to miss.
    if !toggle().is_armed() { return; }
    let Some(module) = fighter.battle_object.module_accessor.as_mut() else { return };
    if app::utility::get_kind(module) == *FIGHTER_KIND_NANA { return; }
    let entry = WorkModule::get_int(module, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID);
    if !(0..8).contains(&entry) { return; }
    let mut capture = CAPTURE.lock().unwrap();
    let Some(mut pending) = capture.pending[entry as usize].take() else { return };
    #[cfg(feature = "named-input-trace")]
    trace_clock(&capture, "fighter_after", entry);
    let stable = capture.clock == Some(pending.tick) && capture.pool.active_key() == Some(pending.key)
        && pending.body == module.battle_object_id;
    if !stable || !live_scene() { capture.pool.seal_entry(pending.key, entry); return; }
    let Some(input) = crate::replay_capture::observed_input(module) else {
        capture.pool.seal_entry(pending.key, entry); return;
    };
    pending.sample.input = input;
    let accepted = capture.pool.push(pending.key, entry, pending.tick, pending.sample);
    #[cfg(feature = "named-input-trace")]
    if entry == 1 && pending.tick < 600 {
        println!("[LiveCapture] sample generation={} index={} tick={} accepted={} buttons={:#x} lx={} ly={}",
            pending.key.generation, pending.key.index, pending.tick, accepted,
            input.buttons, input.axes[0], input.axes[1]);
    }
    #[cfg(not(feature = "named-input-trace"))]
    let _ = accepted;
}

pub fn init() {
    #[cfg(feature = "deluxe-sdk")]
    let registered = crate::capture_plugin::host_registered();
    #[cfg(not(feature = "deluxe-sdk"))]
    let registered = false;
    let seed = crate::replay_store::seed_enabled();
    // Revision 27 semantics without a registered host: hooks only when the INI
    // says on. With a registered host the guarded adapter is resident from
    // startup even when off, because a later ON choice cannot install hooks.
    if !seed && !registered { return; }
    if !crate::common::game_mode::init() { return; }
    // Owned main13.0.5 SHA0789f0a4...536c2. Guard every optional replacement before
    // installing any, including constructor/reset, record-start, copy and success.
    const GUARDS: &[(usize, [u32; 8])] = &[
        (0x1754550, [0xf81e0ff3, 0xa9017bfd, 0x910043fd, 0xd001ca48,
            0x9116a108, 0xaa0003f3, 0xf9000008, 0x9116c000]),
        (0x17545f0, [0xf81e0ff3, 0xa9017bfd, 0x910043fd, 0xd001ca48,
            0x9116a108, 0xaa0003f3, 0xf9000008, 0x9116c000]),
        (0x17587a0, [0xd10143ff, 0xf9000bf7, 0xa90257f6, 0xa9034ff4,
            0xa9047bfd, 0x910103fd, 0x52804215, 0x72a00415]),
        (0x1759110, [0xf81d0ff5, 0xa9014ff4, 0xa9027bfd, 0x910083fd,
            0xf9400409, 0xf9400d33, 0x320b07e8, 0x52800709]),
        (0x175b980, [0xd10083ff, 0xa9017bfd, 0x910043fd, 0xf9400c0b,
            0xf9400568, 0xb4000268, 0x52800708, 0x72a00c08]),
        (0x175ba4c, [0x382e681f, 0x52800fce, 0x72a0040e, 0xaa1f03ec,
            0x2a1f03ed, 0x782e680f, 0x14000012, 0x52836e11]),
        (0x17594b0, [0xf81c0ff7, 0xa90157f6, 0xa9024ff4, 0xa9037bfd,
            0x9100c3fd, 0xf9400416, 0xf9400ec8, 0xf9400109]),
        (0x175a230, [0xa9bd57f6, 0xa9014ff4, 0xa9027bfd, 0x910083fd,
            0xf9400414, 0xb9402688, 0x7100091f, 0x54000921]),
        (0x175ac8c, [0xb90026bf, 0x97fff070, 0xf9400ea8, 0x52800709,
            0x72a00c09, 0x38696909, 0x34000109, 0x320003e0]),
        (0x14f37ec, [0xb900251f, 0xf942c928, 0x39408109, 0x34000149,
            0xb9402509, 0x7100053f, 0x540000e1, 0xb900251f]),
        (0x1503af8, [0xb900251f, 0xf942c928, 0x39408109, 0x34000149,
            0xb9402509, 0x7100053f, 0x540000e1, 0xb900251f]),
        (0x134be18, [0xb900251f, 0xf829691f, 0x52801409, 0x72a00409,
            0x8b090100, 0x2a1f03e1, 0x9499d11c, 0x91169668]),
        (0x175b1bc, [0xf90002b3, 0x17ffffa7, 0x94899d23, 0x00000000,
            0x00000000, 0xf81c0ff7, 0xa90157f6, 0xa9024ff4]),
        (0x143cd40, [0xd101c3ff, 0xa90267fa, 0xa9035ff8, 0xa90457f6,
            0xa9054ff4, 0xa9067bfd, 0x910183fd, 0xa9592001]),
        (0x143e984, [0xf9000260, 0xa9427bfd, 0xa9414ff4, 0x9100c3ff,
            0xd65f03c0, 0x00000000, 0x00000000, 0xf81e0ff3]),
        (0x143e9a0, [0xf81e0ff3, 0xa9017bfd, 0x910043fd, 0xf9400013,
            0xf900001f, 0xb4000273, 0x3945c268, 0x36000088]),
        (0x143ea40, [0xf81e0ff3, 0xa9017bfd, 0x910043fd, 0xb980f408,
            0x71004d1f, 0x54000742, 0x8b080808, 0xb940a908]),
        // Pinned packed-vector velocity export used by live_state's local binding.
        (0x2043a30, [0xf9403400, 0xf9400008, 0xf9407102, 0xd61f0040,
            0xf9403400, 0xf9400008, 0xf9407502, 0xd61f0040]),
    ];
    let save_site = unsafe {
        let base = getRegionAddress(Region::Text) as usize;
        for (offset, code) in GUARDS {
            if std::slice::from_raw_parts((base + offset) as *const u32, code.len()) != code {
                crate::logging::diagnostic(format_args!(
                    "[ReplayCapture] live adapter disabled: native guard {:#x}", offset));
                return;
            }
        }
        let entry_open = std::slice::from_raw_parts(
            (base + SAVE_ENTRY) as *const u32, SAVE_ENTRY_WORDS.len()) == SAVE_ENTRY_WORDS;
        let body_open = std::slice::from_raw_parts(
            (base + SAVE_BODY) as *const u32, SAVE_BODY_WORDS.len()) == SAVE_BODY_WORDS;
        // The replay-to-MP4 plugin replace-hooks this same entry. Its branch covers
        // the first four instructions only; the body words still identify 13.0.5.
        // Installing our own entry hook would replace theirs, so observe from the
        // first instruction their call to the original still executes.
        let save_site = if entry_open { "entry" } else if body_open { "shared" } else {
            crate::logging::diagnostic(format_args!(
                "[ReplayCapture] live adapter disabled: native guard {:#x}", SAVE_ENTRY));
            return;
        };
        if let Err(error) = crate::replay_store::start_writer() {
            crate::logging::diagnostic(format_args!(
                "[ReplayCapture] live adapter disabled: writer {}", error));
            return;
        }
        BASE.store(base, Ordering::Release);
        skyline::install_hooks!(construct_records, clear_records, destroy_records, delete_records,
            reset_current_record, import_record, replace_record, start_recording, serialize_frame,
            stop_recording, stop_recording_alternate, reset_recording, publish_record_frames,
            copy_record, create_save_actor, destroy_save_actor, update_save_actor);
        if save_site == "entry" {
            skyline::install_hooks!(write_record);
        } else {
            skyline::install_hooks!(write_record_shared);
        }
        save_site
    };
    // Ready only after guards, writer and hooks all succeeded (R2); until then the
    // Deluxe row answers UNAVAILABLE and the host writes nothing.
    toggle().set_ready();
    crate::logging::diagnostic(format_args!(
        "[ReplayCapture] guarded live adapter ready capture={} host_registered={} save={}",
        on_off(seed), registered, save_site));
}
