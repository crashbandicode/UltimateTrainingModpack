//! Development-only results adapter. Hooks are installed only after the 13.0.5
//! fingerprints match and `results_native.arm` names one port.
//!
//! Unqualified, and not accepted: NRO record injection, rematch Yes = 2, the
//! dialog port mode, and the Save-confirm input sequence. The Save opener must
//! have initial focus 1 (No), kind 2 and zero fallback result. This file never
//! writes popup result fields.

use std::convert::TryFrom;
use std::fs::File;
use std::io::Read;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

use serde::Deserialize;
use skyline::hooks::{getRegionAddress, InlineCtx, Region};

use super::adapter::{
    open_gate_admits, Adapter, CardsView, Observation, OpenView, Outcome, PopupTick,
};
use super::plugin::{register_controls, CONTROLS};
use super::popup::Snapshot;
use super::pulse::{inject_port, Command, Record, Sample, Scheduler, Slot, Action};

const ARM_PATH: &str = "sd:/ultimate/TrainingModpack/results_native.arm";
const TICK_CALLERS: [usize; 4] = [0x14abed0, 0x14ac080, 0x14d3ac0, 0x151f790];
// Entry and dependency windows from ADAPTER45 / INPUT45 / the observer binding.
const CODE: &[(usize, usize, u64)] = &[
    (0x32f1bb0, 0x14, 0x105b7a2fc4241c43),
    (0x32f3c60, 0x14, 0x93bbaa429890e028),
    (0x1d59540, 0x14, 0x37dac75338f625f1),
    // R+0x928 is a u32 input-route selector. Bind both save-input branches.
    (0x1d59da8, 0x114, 0xeb46d56189445d99),
    (0x3762ec0, 0x14, 0x6fe78c861c736c7),
    (0x150e5f0, 0x3e0, 0x774bf6a587bf3495),
    (0x150e9d0, 0xd0, 0x7900bafaf1df025f),
    (0x1d5b590, 0x98, 0xd7a63bcf755ccd55),
    (0x32f1900, 0x34, 0x42a100bdcad8e28e),
    (0x1d5a540, 0x90, 0x3cd5c89b211f54e0),
    // Save success opens its known popup before publishing status 3.
    (0x1d5a7e0, 0x13c, 0x70f47f6bc9a56d18),
    // Caller and message materialization. Compared at runtime; these windows
    // bind the bytes those constants come from.
    (0x147f960, 0x22c, 0x108f1484300f02f),
    (0x147faa0, 0x28, 0x2e3f4825d781a88e),
    (0x1d5b628, 0x68, 0x4b015ae25ac7fd21),
];

static BASE: AtomicUsize = AtomicUsize::new(0);
static PORT: AtomicU8 = AtomicU8::new(0xff);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static POISON: AtomicBool = AtomicBool::new(false);
static POLICY: AtomicUsize = AtomicUsize::new(0);
static LOGGED_CALLER: AtomicBool = AtomicBool::new(false);
static LOGGED_REENTRY: AtomicBool = AtomicBool::new(false);
static DEPTH: AtomicU8 = AtomicU8::new(0);
static SLOT: Slot = Slot::new();
static mut ADAPTER: Adapter = Adapter::new();
static mut SCHEDULER: Scheduler = Scheduler::new();
static mut LAST: Command = Command::idle();
// Builder has one caller. These are read and written only on that thread.
static mut UI_COMMAND: Command = Command::idle();
static mut UI_SEEN: bool = false;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArmFile {
    schema: u32,
    port: u8,
}

pub(crate) fn init() {
    let port = match read_arm() {
        Ok(port) => port,
        Err("absent") => {
            diagnostic("inert", 0);
            return;
        }
        Err(error) => {
            diagnostic(error, 0);
            return;
        }
    };
    let base = unsafe { getRegionAddress(Region::Text) as usize };
    if base == 0 || !fingerprints(base) {
        diagnostic("init_refused_fingerprint", 0);
        return;
    }
    BASE.store(base, Ordering::Release);
    PORT.store(port, Ordering::Release);
    skyline::install_hooks!(
        popup_open,
        popup_tick,
        result_cards,
        result_tick,
        result_exit,
        ui_records
    );
    ACTIVE.store(true, Ordering::Release);
    CONTROLS.enable();
    register_controls();
    diagnostic("armed", u64::from(port));
    diagnostic("nro_action_unqualified", 1);
    diagnostic("yes2_unqualified", 1);
    diagnostic("port_mode_unqualified", 1);
    diagnostic("save_default_unproven", 1);
}

fn diagnostic(event: &'static str, value: u64) {
    crate::logging::diagnostic(format_args!("[TestHelper] {event} value={value:#x}"));
}

fn read_arm() -> Result<u8, &'static str> {
    let file = match File::open(ARM_PATH) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err("absent"),
        Err(_) => return Err("init_refused_arm"),
    };
    let mut bytes = Vec::new();
    file.take(129).read_to_end(&mut bytes).map_err(|_| "init_refused_arm")?;
    if bytes.len() > 128 {
        return Err("init_refused_arm");
    }
    let arm: ArmFile = serde_json::from_slice(&bytes).map_err(|_| "init_refused_arm")?;
    if arm.schema != 1 || arm.port >= 10 {
        return Err("init_refused_arm");
    }
    Ok(arm.port)
}

fn fingerprints(base: usize) -> bool {
    CODE.iter().all(|&(offset, size, expected)| {
        let data = unsafe { std::slice::from_raw_parts((base + offset) as *const u8, size) };
        training_mod_consts::mii::inventory::fnv1a64(data) == expected
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

fn poison(reason: &'static str) {
    // The flag is the only cross-thread write. The UI thread releases a held
    // button when it sees the flag; this thread does not publish the slot.
    if !POISON.swap(true, Ordering::AcqRel) {
        diagnostic(reason, 0);
    }
}

fn enter_policy() -> bool {
    let thread = unsafe { skyline::nn::os::GetCurrentThread() as usize };
    if thread == 0 {
        poison("policy_thread_missing");
        return false;
    }
    match POLICY.compare_exchange(0, thread, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => true,
        Err(current) if current == thread => true,
        Err(_) => {
            poison("policy_thread_changed");
            false
        }
    }
}

fn drive(obs: Observation) {
    if !ACTIVE.load(Ordering::Acquire) || !enter_policy() || POISON.load(Ordering::Acquire) {
        return;
    }
    // The inline hooks return before the native body runs, so a body can enter
    // another hooked function before this call returns. A nested observation is
    // dropped; the outer call owns the adapter.
    if DEPTH.fetch_add(1, Ordering::AcqRel) != 0 {
        DEPTH.fetch_sub(1, Ordering::AcqRel);
        if !LOGGED_REENTRY.swap(true, Ordering::AcqRel) {
            diagnostic("policy_reentered", 0);
        }
        return;
    }
    let adapter = unsafe { &mut *ptr::addr_of_mut!(ADAPTER) };
    let pending = CONTROLS.pending();
    let outcome = adapter.handle(now_ms(), pending, obs);
    apply_outcome(pending.as_ref(), &outcome);
    DEPTH.fetch_sub(1, Ordering::Release);
}

fn apply_outcome(pending: Option<&super::control::Request>, outcome: &Outcome) {
    for note in &outcome.notes[..usize::from(outcome.note_count)] {
        diagnostic(note.event, note.value);
    }
    // Recheck after the policy work. Cancellation during this hook must not
    // publish a pulse; a terminal status is stored before the idle command.
    let mut command = outcome.command;
    if let Some(request) = pending {
        if !CONTROLS.owns(request.generation) {
            command = Command::idle();
        } else if let Some(progress) = outcome.publish {
            CONTROLS.publish(request.generation, progress);
            if matches!(
                progress,
                super::control::Progress::Done
                    | super::control::Progress::Failed
                    | super::control::Progress::Cancelled
            ) {
                command = Command::idle();
            }
        }
    }
    unsafe {
        if command != *ptr::addr_of!(LAST) {
            SLOT.publish(command);
            *ptr::addr_of_mut!(LAST) = command;
        }
    }
}

unsafe fn rd<T: Copy>(addr: usize) -> Option<T> {
    if addr == 0 || !addr.is_multiple_of(std::mem::align_of::<T>()) {
        return None;
    }
    Some(ptr::read_volatile(addr as *const T))
}

unsafe fn singleton(base: usize, offset: usize) -> Option<usize> {
    let holder = rd::<usize>(base.checked_add(offset)?)?;
    if holder == 0 || !holder.is_multiple_of(8) {
        return None;
    }
    let inner = rd::<usize>(holder + 8)?;
    if inner == 0 || !inner.is_multiple_of(8) {
        None
    } else {
        Some(inner)
    }
}

unsafe fn pending_at(obj: usize) -> Option<i32> {
    let index = rd::<i32>(obj + 0xf4)?;
    if !(0..19).contains(&index) {
        return None;
    }
    rd::<i32>(obj + 0xa8 + (index as usize) * 4)
}

unsafe fn popup_idle(base: usize) -> Option<bool> {
    let popup = singleton(base, 0x532f820)?;
    let phase = rd::<i32>(popup + 0xf8)?;
    let opening = rd::<u8>(popup + 0x142)?;
    Some(phase == 0 && opening == 0)
}

unsafe fn snapshot(popup: usize) -> Option<Snapshot> {
    let phase = rd::<i32>(popup + 0xf8)?;
    let pending = pending_at(popup)?;
    let previous = rd::<i32>(popup + 0x100)?;
    let age = rd::<i32>(popup + 0xfc)?;
    let result = rd::<i32>(popup + 0x118)?;
    let opening = rd::<u8>(popup + 0x142)?;
    let holder = rd::<usize>(popup + 0x110)?;
    if holder == 0 || !holder.is_multiple_of(8) {
        return None;
    }
    let dialog = rd::<usize>(holder + 0x80)?;
    if dialog == 0 || !dialog.is_multiple_of(8) {
        return None;
    }
    let decided = rd::<u8>(dialog + 0x14d)?;
    Some(Snapshot {
        phase: phase_code(phase),
        pending: phase_code(pending),
        previous: phase_code(previous),
        age: if age < 0 { 0 } else { age as u32 },
        result: if result < 0 { u32::MAX } else { result as u32 },
        decided: decided != 0,
        opening: opening != 0,
    })
}

fn phase_code(value: i32) -> u32 {
    if (0..=7).contains(&value) {
        value as u32
    } else {
        8
    }
}

unsafe fn open_view(base: usize, ctx: &InlineCtx) -> Option<OpenView> {
    let primary = singleton(base, 0x532f820)?;
    let owner = ctx.registers[0].x() as usize;
    if owner == 0 || owner != primary {
        return None;
    }
    // The body rejects an open when a popup is already shown or pending.
    // Drop that call before it can replace the prompt we already track.
    let phase = rd::<i32>(owner + 0xf8)?;
    let opening = rd::<u8>(owner + 0x142)?;
    if !open_gate_admits(phase, opening) {
        return None;
    }
    let caller = (ctx.registers[30].x() as usize).wrapping_sub(base) as u64;
    let message = ctx.registers[20].x();
    let data = ctx.registers[1].x() as usize;
    let mut view = OpenView {
        owner: owner as u64,
        caller,
        message,
        word_160: 0,
        word_164: 0,
        word_168: 0,
        word_16c: 0,
        word_170: 0,
        data_ok: false,
        save_status: singleton(base, 0x52c35c8).and_then(|p| rd::<i32>(p + 0x10)),
    };
    if data != 0 && data.is_multiple_of(4) {
        if let (Some(w160), Some(w164), Some(w168), Some(w16c), Some(w170)) = (
            rd::<u32>(data + 0x160),
            rd::<u32>(data + 0x164),
            rd::<u32>(data + 0x168),
            rd::<u32>(data + 0x16c),
            rd::<u32>(data + 0x170),
        ) {
            view.word_160 = w160;
            view.word_164 = w164;
            view.word_168 = w168;
            view.word_16c = w16c;
            view.word_170 = w170;
            view.data_ok = true;
        }
    }
    Some(view)
}

unsafe fn cards_view(base: usize, cards: usize) -> Option<CardsView> {
    if cards == 0 || !cards.is_multiple_of(8) {
        return None;
    }
    let phase = rd::<i32>(cards + 0xf8)?;
    let pending = pending_at(cards)?;
    let save_offered = rd::<u8>(cards + 0x930)?;
    let input_route = rd::<u32>(cards + 0x928)?;
    let finished = rd::<u8>(cards + 0x924)?;
    let save_status = rd::<i32>(singleton(base, 0x52c35c8)? + 0x10)?;
    // Save gate: ldr [[main+0x593b988]+8]; ldr w8, [x8].
    let modal_mask = rd::<u32>(singleton(base, 0x593b988)?)?;
    let popup_idle = popup_idle(base)?;
    Some(CardsView {
        phase,
        pending,
        save_offered,
        input_route,
        finished,
        save_status,
        modal_mask,
        popup_idle,
        fields_ok: true,
    })
}

#[skyline::hook(offset = 0x32f1bb0, inline)]
unsafe fn popup_open(ctx: &mut InlineCtx) {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let Some(view) = open_view(BASE.load(Ordering::Acquire), ctx) else {
        return;
    };
    drive(Observation::Open(view));
}

#[skyline::hook(offset = 0x32f3c60, inline)]
unsafe fn popup_tick(ctx: &mut InlineCtx) {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let base = BASE.load(Ordering::Acquire);
    let owner = ctx.registers[0].x() as usize;
    let Some(primary) = singleton(base, 0x532f820) else {
        return;
    };
    if owner == 0 || owner != primary {
        return;
    }
    let state = snapshot(owner);
    let save_status = singleton(base, 0x52c35c8).and_then(|p| rd::<i32>(p + 0x10));
    drive(Observation::Popup(PopupTick { owner: owner as u64, state, save_status }));
}

#[skyline::hook(offset = 0x1d59540, inline)]
unsafe fn result_cards(ctx: &mut InlineCtx) {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let base = BASE.load(Ordering::Acquire);
    let Some(view) = cards_view(base, ctx.registers[0].x() as usize) else {
        return;
    };
    drive(Observation::Cards(view));
}

#[skyline::hook(offset = 0x150e5f0, inline)]
unsafe fn result_tick(ctx: &mut InlineCtx) {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let base = BASE.load(Ordering::Acquire);
    let caller = (ctx.registers[30].x() as usize).wrapping_sub(base);
    if !TICK_CALLERS.contains(&caller) {
        if !LOGGED_CALLER.swap(true, Ordering::AcqRel) {
            diagnostic("unexpected_results_caller", caller as u64);
        }
        return;
    }
    let Some(seq) = singleton(base, 0x52c35d0) else {
        drive(Observation::End);
        return;
    };
    let status = singleton(base, 0x52c35c8).and_then(|obj| rd::<i32>(obj + 0x10));
    // Begin is idempotent for the same sequence address and applies status.
    // A reused address after End starts a new epoch inside the adapter.
    drive(Observation::Begin { seq: seq as u64, save_status: status });
}

#[skyline::hook(offset = 0x150e9d0, inline)]
unsafe fn result_exit(_ctx: &mut InlineCtx) {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    drive(Observation::End);
}

#[skyline::hook(offset = 0x3762ec0)]
unsafe fn ui_records(records: *mut [Record; 10]) {
    call_original!(records);
    if records.is_null() || !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let command = if POISON.load(Ordering::Acquire) {
        None
    } else {
        match SLOT.load() {
            Sample::Busy => held_ui_command(),
            Sample::Stable(cmd) => {
                *ptr::addr_of_mut!(UI_COMMAND) = cmd;
                *ptr::addr_of_mut!(UI_SEEN) = true;
                owned_pulse(cmd)
            }
        }
    };
    let sched = &mut *ptr::addr_of_mut!(SCHEDULER);
    let _ = inject_port(&mut *records, PORT.load(Ordering::Acquire), command, sched);
}

fn held_ui_command() -> Option<Command> {
    unsafe {
        if *ptr::addr_of!(UI_SEEN) {
            owned_pulse(*ptr::addr_of!(UI_COMMAND))
        } else {
            None
        }
    }
}

fn owned_pulse(cmd: Command) -> Option<Command> {
    if cmd.generation != 0 && cmd.action != Action::Idle && CONTROLS.owns(cmd.generation) {
        Some(cmd)
    } else {
        None
    }
}
