//! One outstanding native Switch software-keyboard request.
//!
//! Input hooks only queue requests/interruptions. [`service`] runs at the outer
//! native UI boundary without holding mod UI/game locks across SDK calls.
//! Waiting naming uses the shared inline Calc transport; ordinary Training keeps
//! ShowKeyboard. Neither transport creates another application-message consumer.
//!
//! `request` fails while an applet is running or a completion has not been
//! polled, so a second applet is never opened and a completion is never
//! overwritten. An inline interruption snapshots values then finalizes on its
//! owning UI thread. An in-flight synchronous call cannot be interrupted, so its
//! slot remains occupied until completion. Queued requests cancel before opening.

pub(crate) mod inline_keyboard;

use std::ffi::CString;
use std::mem::{self, MaybeUninit};

use parking_lot::Mutex;
use skyline::nn::swkbd::{
    get_required_work_buffer_size, make_preset_default, set_header_text, set_initial_text_utf8,
    ShowKeyboardArg, ShowKeyboardString,
};

/// Unicode scalar limit used by `MacroRecording` name validation.
pub const MAX_NAME_CHARS: usize = training_mod_macros::MAX_NAME_CHARS;

/// `Failed` value meaning `ShowKeyboard` was not called.
///
/// This is not an `nn::Result`. Allocation, size-query, encoding and inline
/// protocol failures use it so a local error cannot be mistaken for success (`0`).
pub const PREPARATION_FAILURE: u32 = 0xFFFF_FFFF;

const OUTSTANDING_REQUEST: &str = "keyboard request already outstanding";
/// `SwkbdConfigCommon::header_text` holds 64 UTF-16 units plus a NUL.
const MAX_HEADER_UTF16_UNITS: usize = 64;
/// Pinned binding output buffer: 1002 UTF-16 units, 0x7D4 bytes.
const OUTPUT_UTF16_UNITS: usize = 1002;
const MAX_NAME_LENGTH: u32 = {
    // Binding comment: the keyboard maximum is 500 characters.
    assert!(MAX_NAME_CHARS <= 500);
    MAX_NAME_CHARS as u32
};
const _: () = assert!(OUTPUT_UTF16_UNITS * 2 == 0x7D4);
/// Little-endian `u32` max length. The binding marks offset 0x3ac as that field.
const MAX_LENGTH_OFFSET: usize = 0x3ac;
/// `nn::Result` success. Eden models the same zero value as `ResultSuccess`.
const SDK_SUCCESS: u32 = 0;
/// Module SWKBD (159) description 1. The pinned binding treats only this as cancel.
const SDK_CANCEL: u32 = 0x29f;
/// Page alignment. The binding's `[u8; 0xd000]` array does not request it.
const WORK_BUFFER_ALIGN: usize = 0x1000;
const _: () = assert!(WORK_BUFFER_ALIGN.is_power_of_two());
const _: () = assert!(WORK_BUFFER_ALIGN >= 8);
/// Reject a size query that is empty or far above the binding's old 0xd000 buffer.
const MAX_WORK_BUFFER_BYTES: usize = 0x10_0000;

/// Outcome of one native keyboard request.
///
/// For synchronous requests, `Confirmed` is only raw result `0`, `Cancelled`
/// only `0x29f`, and every other SDK code is `Failed`. Inline requests use explicit
/// Enter/Cancel callbacks; local setup/protocol errors use PREPARATION_FAILURE.
#[derive(Debug, PartialEq, Eq)]
pub enum KeyboardResult {
    Confirmed(String),
    Cancelled,
    Failed(u32),
    /// The inline owner is fully closed. The last value-only snapshot is retained
    /// until poll consumes this result; this is never a confirmation.
    Interrupted,
}

/// Queue one keyboard request. `Ok` means the queue accepted it, not that the
/// applet has closed. `id` is returned unchanged from [`poll`].
pub fn request(id: u64, title: &str, initial: &str) -> Result<(), String> {
    validate_title(title)?;
    validate_initial(initial)?;
    let job = Job {
        id,
        title: title.to_string(),
        initial: initial.to_string(),
        inline: None,
    };
    let mut guard = STATE.lock();
    enqueue(&mut guard, job)
}

/// Waiting-room names use UTF-16 cursor coordinates and the macro scalar limit.
pub fn request_inline(id: u64, initial: &str, cursor: usize) -> Result<(), String> {
    let text = inline_keyboard::Text::macro_name(initial, cursor)
        .ok_or_else(|| "Invalid unfinished keyboard text/caret".to_string())?;
    let job = Job { id, title: String::new(), initial: initial.to_owned(), inline: Some(text) };
    enqueue(&mut STATE.lock(), job)
}

/// Safe from retirement/input callbacks: only marks intent. Calc/Finalize always
/// run later at the existing UI boundary on the keyboard's owning thread.
pub fn interrupt(id: u64) { interrupt_shared(&mut STATE.lock(), id); }

/// Values only. Call after service and before consuming its completion.
pub fn snapshot(id: u64) -> Option<(String, usize)> {
    let state = STATE.lock();
    let (owner, text) = state.snapshot?;
    (owner == id).then(|| (String::from_utf16_lossy(text.units()), text.cursor()))
}

/// Take the stored completion, if the applet has already returned.
pub fn poll() -> Option<(u64, KeyboardResult)> {
    poll_shared(&mut STATE.lock())
}

/// `true` while a request is queued or either native transport owns the keyboard.
///
/// A stored completion is not busy. [`request`] still rejects it until
/// [`poll`] takes it.
pub fn busy() -> bool {
    is_busy(&STATE.lock()) || inline_keyboard::active()
}

static STATE: Mutex<Shared> = Mutex::new(Shared {
    phase: Phase::Idle,
    pending: None,
    owner: None,
    interrupted: false,
    snapshot: None,
});

struct Shared {
    phase: Phase,
    pending: Option<Job>,
    owner: Option<InlineOwner>,
    interrupted: bool,
    snapshot: Option<(u64, inline_keyboard::Text)>,
}

enum Phase {
    Idle,
    Running { id: u64 },
    Ready { id: u64, result: KeyboardResult },
}

struct Job {
    id: u64,
    title: String,
    initial: String,
    inline: Option<inline_keyboard::Text>,
}

struct InlineOwner { id: u64, keyboard: inline_keyboard::Keyboard }

fn validate_title(title: &str) -> Result<(), String> {
    if title.as_bytes().contains(&0) {
        return Err("keyboard text contains an interior NUL".to_string());
    }
    if title.encode_utf16().take(MAX_HEADER_UTF16_UNITS + 1).count() > MAX_HEADER_UTF16_UNITS {
        return Err(format!(
            "keyboard title exceeds {MAX_HEADER_UTF16_UNITS} UTF-16 code units"
        ));
    }
    Ok(())
}

fn validate_initial(initial: &str) -> Result<(), String> {
    if initial.as_bytes().contains(&0) {
        return Err("keyboard text contains an interior NUL".to_string());
    }
    if initial.chars().count() > MAX_NAME_CHARS {
        return Err(format!(
            "keyboard name exceeds {MAX_NAME_CHARS} characters"
        ));
    }
    Ok(())
}

fn enqueue(shared: &mut Shared, job: Job) -> Result<(), String> {
    if !matches!(shared.phase, Phase::Idle) {
        return Err(OUTSTANDING_REQUEST.to_string());
    }
    shared.phase = Phase::Running { id: job.id };
    shared.interrupted = false;
    shared.snapshot = job.inline.map(|text| (job.id, text));
    shared.pending = Some(job);
    Ok(())
}

fn interrupt_shared(shared: &mut Shared, id: u64) {
    if matches!(shared.phase, Phase::Running { id: current } | Phase::Ready { id: current, .. } if current == id) {
        shared.interrupted = true;
        // Interruption also wins over an Enter already returned by Calc but not
        // yet consumed. Explicit Cancel remains a cancellation, not a checkpoint.
        if let Phase::Ready { result, .. } = &mut shared.phase {
            if shared.snapshot.is_some() && !matches!(result, KeyboardResult::Cancelled) {
                *result = KeyboardResult::Interrupted;
            }
        }
    }
}

fn finish(shared: &mut Shared, id: u64, result: KeyboardResult) {
    match shared.phase {
        Phase::Running { id: running } if running == id => {
            let result = if shared.interrupted && !matches!(result, KeyboardResult::Cancelled) {
                if shared.snapshot.is_some() { KeyboardResult::Interrupted } else { KeyboardResult::Cancelled }
            } else { result };
            shared.phase = Phase::Ready { id, result };
        }
        _ => {}
    }
}

fn poll_shared(shared: &mut Shared) -> Option<(u64, KeyboardResult)> {
    match mem::replace(&mut shared.phase, Phase::Idle) {
        Phase::Ready { id, result } => {
            shared.snapshot = None;
            Some((id, result))
        }
        other => {
            shared.phase = other;
            None
        }
    }
}

fn is_busy(shared: &Shared) -> bool {
    matches!(shared.phase, Phase::Running { .. })
}

/// Queue cancellation is consumed before any SDK admission. Retain the initial
/// value snapshot for an interrupted inline request that never opened.
fn take_pending(shared: &mut Shared) -> Option<Job> {
    let job = shared.pending.take()?;
    if shared.interrupted {
        let result = if job.inline.is_some() { KeyboardResult::Interrupted } else { KeyboardResult::Cancelled };
        finish(shared, job.id, result);
        None
    } else { Some(job) }
}

/// Invoke only from the outer UI frame hook. Recheck the caller's waiting scope
/// before opening and after Calc, so interruption wins over a same-frame Enter.
/// No state lock spans any SDK call or the caller's admission predicate.
pub fn service(allow_waiting: fn() -> bool) {
    let (owner, pending) = {
        let mut state = STATE.lock();
        if state.owner.as_ref().is_some_and(|o| !o.keyboard.on_owner_thread()) { return; }
        (state.owner.take(), take_pending(&mut state))
    };
    if let Some(owner) = owner {
        service_inline(owner, allow_waiting);
        return;
    }
    let Some(job) = pending else { return };
    if let Some(text) = job.inline {
        if !allow_waiting() || STATE.lock().interrupted {
            finish(&mut STATE.lock(), job.id, KeyboardResult::Interrupted);
            return;
        }
        // A Mii owner may still be finalizing. Keep the request queued, with its
        // cancellation flag intact, rather than opening a competing keyboard.
        if inline_keyboard::active() { STATE.lock().pending = Some(job); return; }
        let keyboard = unsafe { inline_keyboard::Keyboard::open(text, inline_keyboard::current_thread()) };
        match keyboard {
            Some(keyboard) => service_inline(InlineOwner { id: job.id, keyboard }, allow_waiting),
            None => finish(&mut STATE.lock(), job.id, KeyboardResult::Failed(PREPARATION_FAILURE)),
        }
        return;
    }
    if STATE.lock().interrupted {
        finish(&mut STATE.lock(), job.id, KeyboardResult::Cancelled);
        return;
    }
    let Some(_lease) = inline_keyboard::Lease::acquire() else {
        STATE.lock().pending = Some(job);
        return;
    };
    crate::logging::diagnostic(format_args!("[NativeKeyboard] begin id={}", job.id));
    let result = show_native(&job.title, &job.initial);
    // Do not put user-entered names in diagnostic logs.
    let code = match &result {
        KeyboardResult::Confirmed(_) => SDK_SUCCESS,
        KeyboardResult::Cancelled => SDK_CANCEL,
        KeyboardResult::Failed(code) => *code,
        KeyboardResult::Interrupted => PREPARATION_FAILURE,
    };
    crate::logging::diagnostic(format_args!("[NativeKeyboard] returned id={} result={:#x}", job.id, code));
    finish(&mut STATE.lock(), job.id, result);
}

fn inline_result(poll: &inline_keyboard::Poll, interrupted: bool) -> Option<KeyboardResult> {
    use inline_keyboard::Decision;
    if matches!(poll.decision, Some(Decision::Cancel)) { return Some(KeyboardResult::Cancelled); }
    if interrupted { return Some(KeyboardResult::Interrupted); }
    if poll.invalid || poll.finished { return Some(KeyboardResult::Failed(PREPARATION_FAILURE)); }
    match poll.decision {
        Some(Decision::Enter(text)) => Some(KeyboardResult::Confirmed(String::from_utf16_lossy(text.units()))),
        _ => None,
    }
}

fn service_inline(mut owner: InlineOwner, allow_waiting: fn() -> bool) {
    // Even a departing owner gets one final Calc to collect already-entered
    // text/caret. Its decision cannot commit when the scene has been interrupted.
    let poll = unsafe { owner.keyboard.poll() };
    let permitted = allow_waiting();
    let result = {
        let mut state = STATE.lock();
        if !permitted { state.interrupted = true; }
        match poll {
            Some(poll) => {
                state.snapshot = Some((owner.id, poll.text));
                inline_result(&poll, state.interrupted)
            }
            None => Some(KeyboardResult::Failed(PREPARATION_FAILURE)),
        }
    };
    if let Some(result) = result {
        if unsafe { owner.keyboard.close() } {
            // Finalize/Destruct and lease release have completed before readiness
            // can observe this completion or queue a reconstructed keyboard.
            finish(&mut STATE.lock(), owner.id, result);
            return;
        }
    }
    STATE.lock().owner = Some(owner);
}

fn show_native(title: &str, initial: &str) -> KeyboardResult {
    let Ok(header) = utf16_z(title) else {
        return KeyboardResult::Failed(PREPARATION_FAILURE);
    };
    let Ok(initial) = CString::new(initial) else {
        return KeyboardResult::Failed(PREPARATION_FAILURE);
    };
    // False: no custom dictionary, matching the null dictionary fields below.
    let size = unsafe { get_required_work_buffer_size(false) };
    if size == 0 || size > MAX_WORK_BUFFER_BYTES {
        return KeyboardResult::Failed(PREPARATION_FAILURE);
    }

    // All-zero is a valid argument: Drop frees `work_buffer` only when it is
    // non-null, and the other pointers start null.
    let mut arg: ShowKeyboardArg = unsafe { MaybeUninit::zeroed().assume_init() };
    match allocate_work_buffer(size) {
        Ok(buffer) => {
            arg.work_buffer = buffer;
            arg.work_buffer_size = size;
        }
        Err(()) => return KeyboardResult::Failed(PREPARATION_FAILURE),
    }

    unsafe {
        make_preset_default(&mut arg.keyboard_config);
        // Keep `header` and `initial` alive until ShowKeyboard returns. The
        // setters take raw pointers and the binding does not say they copy.
        set_header_text(std::ptr::addr_of!(arg.keyboard_config), header.as_ptr());
        set_initial_text_utf8(&mut arg, initial.as_ptr().cast());
    }
    // Preset and the text setters run first so a default max cannot remain in
    // the high bytes of the u32 at 0x3ac.
    write_max_length(&mut arg.keyboard_config);

    let mut output = vec![0u16; OUTPUT_UTF16_UNITS];
    // Give the SDK a plain descriptor, never a mutable Rust Box/Vec header.
    // The pinned wrapper passes this same pointer + UTF-16-unit-count layout.
    let mut descriptor = ShowKeyboardString {
        string_buffer: output.as_mut_ptr().cast(), buffer_len: OUTPUT_UTF16_UNITS,
    };
    let code = unsafe { show_keyboard(&mut descriptor, &arg) };
    if descriptor.string_buffer != output.as_ptr().cast() {
        return KeyboardResult::Failed(PREPARATION_FAILURE);
    }
    classify(code, &output)
}

fn utf16_z(text: &str) -> Result<Vec<u16>, ()> {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    if units.len() > MAX_HEADER_UTF16_UNITS {
        return Err(());
    }
    units.push(0);
    Ok(units)
}

fn write_max_length(config: &mut [u8; 0x4D0]) {
    let start = MAX_LENGTH_OFFSET;
    config[start..start + 4].copy_from_slice(&MAX_NAME_LENGTH.to_le_bytes());
}

fn allocate_work_buffer(size: usize) -> Result<*mut skyline::libc::c_void, ()> {
    let mut raw = std::ptr::null_mut();
    // `ShowKeyboardArg`'s Drop frees this pointer with `libc::free`.
    // `posix_memalign` is the matching allocator in libc-nnsdk and is what
    // skyline uses for over-aligned layouts. Alignment 0x1000 is stricter
    // than the binding's align-1 array; the SDK does not state the requirement.
    let rc = unsafe { skyline::libc::posix_memalign(&mut raw, WORK_BUFFER_ALIGN, size) };
    if rc != 0 || raw.is_null() {
        return Err(());
    }
    unsafe { std::ptr::write_bytes(raw as *mut u8, 0, size) };
    Ok(raw)
}

fn classify(code: u32, output: &[u16]) -> KeyboardResult {
    match code {
        SDK_SUCCESS => KeyboardResult::Confirmed(decode_output(output)),
        SDK_CANCEL => KeyboardResult::Cancelled,
        other => KeyboardResult::Failed(other),
    }
}

fn decode_output(output: &[u16]) -> String {
    let end = output.iter().position(|unit| *unit == 0).unwrap_or(output.len());
    String::from_utf16_lossy(&output[..end])
}

extern "C" {
    #[link_name = "_ZN2nn5swkbd12ShowKeyboardEPNS0_6StringERKNS0_15ShowKeyboardArgE"]
    fn show_keyboard(output: *mut ShowKeyboardString, arg: *const ShowKeyboardArg) -> u32;
}

#[cfg(test)]
mod tests {
    use super::{
        classify, decode_output, enqueue, finish, is_busy, poll_shared, validate_initial,
        validate_title, Job, KeyboardResult, Phase, Shared, MAX_HEADER_UTF16_UNITS,
        MAX_NAME_CHARS, OUTPUT_UTF16_UNITS, SDK_CANCEL, SDK_SUCCESS,
    };

    fn blank_output() -> Vec<u16> {
        vec![0u16; OUTPUT_UTF16_UNITS]
    }

    fn output_with(text: &str) -> Vec<u16> {
        let mut units = vec![0u16; OUTPUT_UTF16_UNITS];
        for (index, unit) in text.encode_utf16().enumerate() {
            units[index] = unit;
        }
        units
    }

    fn fresh() -> Shared {
        Shared {
            phase: Phase::Idle,
            pending: None,
            owner: None,
            interrupted: false,
            snapshot: None,
        }
    }

    fn job(id: u64) -> Job {
        Job {
            id,
            title: "Name".to_string(),
            initial: String::new(),
            inline: None,
        }
    }

    #[test]
    fn success_code_returns_decoded_text() {
        let output = output_with("é");
        assert_eq!(
            classify(SDK_SUCCESS, &output),
            KeyboardResult::Confirmed("é".to_string())
        );
    }

    #[test]
    fn cancel_code_is_not_confirmed_text() {
        let output = output_with("kept");
        assert_eq!(classify(SDK_CANCEL, &output), KeyboardResult::Cancelled);
    }

    #[test]
    fn every_other_code_is_a_failure() {
        let output = blank_output();
        assert_eq!(classify(1, &output), KeyboardResult::Failed(1));
        assert_eq!(classify(0x29e, &output), KeyboardResult::Failed(0x29e));
    }

    #[test]
    fn decode_stops_at_the_first_nul() {
        let mut units = vec![0u16; OUTPUT_UTF16_UNITS];
        units[0] = u16::from(b'a');
        units[1] = u16::from(b'b');
        units[3] = u16::from(b'c');
        assert_eq!(decode_output(&units), "ab");
    }

    #[test]
    fn name_limit_is_the_macro_scalar_count() {
        let ok = "x".repeat(MAX_NAME_CHARS);
        let too_long = "x".repeat(MAX_NAME_CHARS + 1);
        assert!(validate_initial(&ok).is_ok());
        assert!(validate_initial(&too_long).is_err());
        assert!(validate_initial("é".repeat(MAX_NAME_CHARS).as_str()).is_ok());
    }

    #[test]
    fn title_limit_counts_utf16_units() {
        assert!(validate_title(&"a".repeat(MAX_HEADER_UTF16_UNITS)).is_ok());
        assert!(validate_title(&"a".repeat(MAX_HEADER_UTF16_UNITS + 1)).is_err());
        // U+1F600 is one scalar and two UTF-16 units.
        let grinning = "\u{1F600}".repeat(MAX_HEADER_UTF16_UNITS / 2 + 1);
        assert!(validate_title(&grinning).is_err());
    }

    #[test]
    fn interior_nul_is_rejected() {
        assert!(validate_initial("a\0b").is_err());
        assert!(validate_title("a\0b").is_err());
    }

    #[test]
    fn a_running_request_rejects_another() {
        let mut shared = fresh();
        assert!(enqueue(&mut shared, job(1)).is_ok());
        assert!(is_busy(&shared));
        assert!(enqueue(&mut shared, job(2)).is_err());
        assert!(poll_shared(&mut shared).is_none());
    }

    #[test]
    fn an_unpolled_completion_rejects_another_request() {
        let mut shared = fresh();
        assert!(enqueue(&mut shared, job(7)).is_ok());
        finish(
            &mut shared,
            7,
            KeyboardResult::Confirmed("clip".to_string()),
        );
        assert!(!is_busy(&shared));
        assert!(enqueue(&mut shared, job(8)).is_err());
        assert_eq!(
            poll_shared(&mut shared),
            Some((7, KeyboardResult::Confirmed("clip".to_string())))
        );
        assert!(enqueue(&mut shared, job(8)).is_ok());
    }

    #[test]
    fn a_mismatched_completion_does_not_retire_the_running_request() {
        let mut shared = fresh();
        assert!(enqueue(&mut shared, job(4)).is_ok());
        finish(&mut shared, 5, KeyboardResult::Cancelled);
        assert!(is_busy(&shared));
        assert!(poll_shared(&mut shared).is_none());
        assert!(enqueue(&mut shared, job(6)).is_err());
    }

    #[test]
    fn a_second_completion_does_not_overwrite_the_stored_result() {
        let mut shared = fresh();
        assert!(enqueue(&mut shared, job(3)).is_ok());
        finish(&mut shared, 3, KeyboardResult::Confirmed("first".to_string()));
        finish(&mut shared, 3, KeyboardResult::Confirmed("second".to_string()));
        assert_eq!(
            poll_shared(&mut shared),
            Some((3, KeyboardResult::Confirmed("first".to_string())))
        );
    }

    fn inline_job(id: u64) -> Job {
        Job { inline: Some(super::inline_keyboard::Text::macro_name("unfinished 😀", 3).unwrap()), ..job(id) }
    }

    #[test]
    fn interrupted_queue_never_reaches_sdk_admission() {
        let mut shared = fresh();
        enqueue(&mut shared, inline_job(21)).unwrap();
        super::interrupt_shared(&mut shared, 21);
        assert!(super::take_pending(&mut shared).is_none());
        let (_, text) = shared.snapshot.unwrap();
        assert_eq!(String::from_utf16(text.units()).unwrap(), "unfinished 😀");
        assert_eq!(text.cursor(), 3);
        assert_eq!(poll_shared(&mut shared), Some((21, KeyboardResult::Interrupted)));
        assert!(shared.snapshot.is_none());

        enqueue(&mut shared, job(22)).unwrap();
        super::interrupt_shared(&mut shared, 22);
        assert!(super::take_pending(&mut shared).is_none());
        assert_eq!(poll_shared(&mut shared), Some((22, KeyboardResult::Cancelled)));
    }

    #[test]
    fn stale_cancel_cannot_touch_the_reconstructed_request() {
        let mut shared = fresh();
        enqueue(&mut shared, inline_job(30)).unwrap();
        super::interrupt_shared(&mut shared, 30);
        assert!(super::take_pending(&mut shared).is_none());
        assert_eq!(poll_shared(&mut shared), Some((30, KeyboardResult::Interrupted)));
        enqueue(&mut shared, inline_job(31)).unwrap();
        super::interrupt_shared(&mut shared, 30);
        assert_eq!(super::take_pending(&mut shared).unwrap().id, 31);
        assert!(!shared.interrupted);
        assert!(is_busy(&shared));
        assert!(enqueue(&mut shared, inline_job(32)).is_err());
    }

    #[test]
    fn interruption_overrides_unconsumed_enter_but_never_explicit_cancel() {
        let mut shared = fresh();
        enqueue(&mut shared, inline_job(40)).unwrap();
        assert_eq!(super::take_pending(&mut shared).unwrap().id, 40);
        finish(&mut shared, 40, KeyboardResult::Confirmed("unfinished 😀".into()));
        super::interrupt_shared(&mut shared, 40);
        assert_eq!(poll_shared(&mut shared), Some((40, KeyboardResult::Interrupted)));
        enqueue(&mut shared, inline_job(41)).unwrap();
        assert_eq!(super::take_pending(&mut shared).unwrap().id, 41);
        finish(&mut shared, 41, KeyboardResult::Cancelled);
        super::interrupt_shared(&mut shared, 41);
        assert_eq!(poll_shared(&mut shared), Some((41, KeyboardResult::Cancelled)));
    }

    #[test]
    fn calc_checkpoint_cannot_confirm_without_an_uninterrupted_enter() {
        use super::inline_keyboard::{Decision, Poll, Text};
        let text = Text::macro_name("edited 😀", 4).unwrap();
        let mut poll = Poll { text, decision: None, initialized: true,
            finished: false, invalid: false, state: 3 };
        assert_eq!(super::inline_result(&poll, false), None);
        poll.decision = Some(Decision::Enter(text));
        assert_eq!(super::inline_result(&poll, true), Some(KeyboardResult::Interrupted));
        assert_eq!(super::inline_result(&poll, false), Some(KeyboardResult::Confirmed("edited 😀".into())));
        poll.invalid = true;
        assert_eq!(super::inline_result(&poll, false), Some(KeyboardResult::Failed(super::PREPARATION_FAILURE)));
        poll.decision = Some(Decision::Cancel);
        assert_eq!(super::inline_result(&poll, true), Some(KeyboardResult::Cancelled));
    }
}
