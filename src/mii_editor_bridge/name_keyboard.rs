//! Asynchronous Name adapter. The confirmed name remains in the native DRAFT.
//! Only an explicit Enter passes an owned string through the original commit.
use super::{current_thread, inline_keyboard::{Decision, Keyboard, Text}, native,
    Phase, BASE, EDIT_CONTEXT, EDIT_OWNED, STATE};
use std::sync::{atomic::{AtomicUsize, Ordering}, Mutex};

struct Owner {
    context: usize,
    thread: usize,
    keyboard: Option<Keyboard>,
    text: Text,
    decision: Option<Decision>,
    resumed: bool,
    shown: bool,
    failed: bool,
}

// The context is a comparison token; each native callback rebinds its objects.
// No SDK call runs with either lock held.
static OWNER: Mutex<Option<Owner>> = Mutex::new(None);
static PREPARED: Mutex<Option<(usize, Text)>> = Mutex::new(None);
static SUSPENDED: AtomicUsize = AtomicUsize::new(0);

fn note(name: &'static str, values: [u64; 4]) {
    if let Some(s) = STATE.lock().unwrap().as_mut() { s.event(name, values); }
}

fn ours(context: usize) -> bool {
    EDIT_OWNED.load(Ordering::Acquire) && context != 0
        && context == EDIT_CONTEXT.load(Ordering::Acquire)
}

fn on_editor_thread() -> bool {
    STATE.lock().unwrap().as_ref().is_some_and(|s|
        s.phase == Phase::Editing && s.thread == current_thread())
}

struct Binding {
    context: usize,
    text_object: usize,
    config: usize,
    age: u32,
}

impl Binding {
    unsafe fn bind(context: usize) -> Option<Self> {
        if !ours(context) || !on_editor_thread() || !native::aligned(context) { return None; }
        let level: i32 = native::read(context, 0x144);
        if !(0..19).contains(&level) || native::read::<i32>(context, 0x148) != 4
            || native::read::<i32>(context, 0xf8 + level as usize * 4) != 4 { return None; }
        let text_object: usize = native::read(context, 0x170);
        if !native::aligned(text_object) || native::read::<u32>(text_object, 0xc) != 2
            || native::read::<u8>(text_object, 0x12) != 0 { return None; }
        let config: usize = native::read(text_object, 0);
        if !native::aligned(config) { return None; }
        Some(Self { context, text_object, config, age: native::read(context, 0x14c) })
    }

    unsafe fn manager_idle(&self) -> bool {
        let base = BASE.load(Ordering::Acquire);
        let outer: usize = native::read(base, 0x532f850);
        if !native::aligned(outer) { return false; }
        let manager: usize = native::read(outer, 8);
        native::aligned(manager) && native::read::<u8>(manager, 0xd8) == 0
    }

    unsafe fn initial(&self) -> Option<Text> {
        let draft = BASE.load(Ordering::Acquire) + native::DRAFT;
        let len = native::read::<u32>(draft, 0x114) as usize;
        let units: [u16; 10] = native::read(draft, 0x118);
        Text::new(units.get(..len)?, len)
    }

    unsafe fn finish(&self, decision: Decision, original: unsafe extern "C" fn(usize) -> usize) -> usize {
        // The text object and its config remain owned by the editor throughout
        // this synchronous call. Never point them at storage beyond that call.
        let confirmed: u8 = native::read(self.text_object, 0x11);
        let pointer: usize = native::read(self.config, 0x500);
        let size: usize = native::read(self.config, 0x508);
        let text = match decision { Decision::Enter(text) => Some(text), Decision::Cancel => None };
        if let Some(text) = &text {
            native::write(self.config, 0x500, text.terminated().as_ptr() as usize);
            native::write(self.config, 0x508, std::mem::size_of_val(text.terminated()));
        }
        native::write(self.text_object, 0x11, u8::from(text.is_some()));
        let result = original(self.context);
        native::write(self.config, 0x500, pointer);
        native::write(self.config, 0x508, size);
        native::write(self.text_object, 0x11, confirmed);
        result
    }
}

/// Called with the fresh editor context before native entry into state 4.
pub(super) fn prepare_resume(context: usize, text: Text) -> bool {
    if !ours(context) || !on_editor_thread() || OWNER.lock().unwrap().is_some() { return false; }
    let mut pending = PREPARED.lock().unwrap();
    if pending.is_some() { return false; }
    *pending = Some((context, text));
    SUSPENDED.store(0, Ordering::Release);
    true
}

pub(super) fn restored(context: usize) -> bool {
    OWNER.lock().unwrap().as_ref().is_some_and(|owner| owner.context == context
        && owner.resumed && owner.shown && !owner.failed)
}

/// Snapshot before native leaf completion. Even an Enter arriving in this poll
/// stays provisional when the incoming match has already won the frame.
pub(super) unsafe fn suspend(context: usize) -> Result<Text, &'static str> {
    if !ours(context) || !on_editor_thread() { return Err("keyboard_owner_mismatch"); }
    let mut owner = {
        let mut state = OWNER.lock().unwrap();
        if state.as_ref().is_none_or(|owner|
            owner.context != context || owner.thread != current_thread()) {
            return Err("keyboard_not_owned");
        }
        state.take().unwrap()
    };
    // An interruption may need another native frame before leaf completion.
    // Never reopen or confirm a second keyboard in that interval.
    SUSPENDED.store(context, Ordering::Release);
    if let Some(keyboard) = owner.keyboard.as_mut() {
        match keyboard.poll() {
            Some(poll) => { owner.text = poll.text; owner.failed |= poll.invalid || poll.finished; }
            None => owner.failed = true,
        }
        if !keyboard.close() { owner.failed = true; }
    }
    *PREPARED.lock().unwrap() = None;
    if owner.failed { Err("keyboard_capture_failed") } else { Ok(owner.text) }
}

/// Defensive cleanup while the current editor callback still owns the thread.
/// This must run before native editor destruction, never afterward.
pub(super) unsafe fn before_release(context: usize) {
    let mut owner = {
        let mut stored = OWNER.lock().unwrap();
        if stored.as_ref().is_some_and(|owner|
            owner.context != context || owner.thread != current_thread()) { return; }
        stored.take()
    };
    if let Some(owner) = owner.as_mut() {
        if let Some(keyboard) = owner.keyboard.as_mut() { keyboard.close(); }
    }
    *PREPARED.lock().unwrap() = None;
    SUSPENDED.store(0, Ordering::Release);
}

#[skyline::hook(offset = 0x180a570)]
unsafe fn name_frame(context: usize) -> usize {
    if !ours(context) { return original!()(context); }
    if SUSPENDED.load(Ordering::Acquire) == context { return 0; }
    let Some(binding) = Binding::bind(context) else { return 0; };
    // Never take ownership of the synchronous global manager or clear its flags.
    if !binding.manager_idle() { return 0; }
    let mut owner = OWNER.lock().unwrap().take();
    if owner.is_none() {
        let pending = {
            let mut prepared = PREPARED.lock().unwrap();
            if prepared.as_ref().is_some_and(|(target, _)| *target == context) {
                prepared.take().map(|(_, text)| text)
            } else { None }
        };
        let Some(text) = pending.or_else(|| binding.initial()) else { return 0; };
        let thread = current_thread();
        let keyboard = Keyboard::open(text, thread);
        let failed = keyboard.is_none();
        note("name_inline_opened", [u64::from(!failed), u64::from(pending.is_some()),
            text.units().len() as u64, text.cursor() as u64]);
        owner = Some(Owner { context, thread, keyboard, text,
            decision: failed.then_some(Decision::Cancel),
            resumed: pending.is_some(), shown: false, failed });
    }
    let mut owner = owner.unwrap();
    if owner.context != context || owner.thread != current_thread() {
        *OWNER.lock().unwrap() = Some(owner);
        return 0;
    }
    if let Some(keyboard) = owner.keyboard.as_mut() {
        if let Some(poll) = keyboard.poll() {
            owner.text = poll.text;
            owner.failed |= poll.invalid || poll.finished;
            // Calc returns the applet's reported state, not merely the fact
            // that Initialize was requested. State 3 is InitializedIsShown.
            if !owner.shown && poll.initialized && poll.state == 3 && !owner.failed {
                owner.shown = true;
                note("name_inline_shown", [u64::from(owner.resumed),
                    owner.text.units().len() as u64, owner.text.cursor() as u64, 0]);
            }
            if owner.decision.is_none() { owner.decision = poll.decision; }
        } else { owner.failed = true; }
        if owner.failed { owner.decision = Some(Decision::Cancel); }
    }
    if binding.age > 0 {
        if let Some(decision) = owner.decision {
            if let Some(keyboard) = owner.keyboard.as_mut() {
                if !keyboard.close() {
                    *OWNER.lock().unwrap() = Some(owner);
                    return 0;
                }
            }
            // Original age>0 performs native pop, normalization and preview
            // refresh. It is never called to accept a suspended edit.
            let result = binding.finish(decision, original!());
            note("name_inline_decided", [u64::from(matches!(decision, Decision::Enter(_))),
                u64::from(owner.failed), 0, 0]);
            return result;
        }
    }
    *OWNER.lock().unwrap() = Some(owner);
    0
}

pub(super) fn init() { skyline::install_hooks!(name_frame); }
