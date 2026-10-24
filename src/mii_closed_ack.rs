//! CPU-probe-only closed acknowledgements. No native reads, worker, or input retries.
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;

const ROOT: &str = "sd:/ultimate/TrainingModpack/mii_cpu_ack";
const LIMIT: usize = 8192;
const MAX_EVENTS: usize = 12;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arm { schema: u32, run: String }
#[derive(Clone, Copy)]
struct Session { run: [u8; 32] }
static SESSION: Mutex<Option<Session>> = Mutex::new(None);
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
static WRITING: AtomicBool = AtomicBool::new(false);
static FAILED: AtomicBool = AtomicBool::new(false);
static TERMINAL: AtomicU32 = AtomicU32::new(0);
static READY_ENTRY: AtomicUsize = AtomicUsize::new(0);
// These hold owned evidence only. Every guard is dropped before file I/O.
static COMMITTED: Mutex<Option<Value>> = Mutex::new(None);
static OBSERVATION: Mutex<Option<Value>> = Mutex::new(None);
static REFUSAL: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn hex(value: usize) -> String { format!("0x{value:x}") }
pub(crate) fn hash(value: u64) -> String { format!("0x{value:016x}") }
fn invalid(message: &'static str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }
struct Writer;
impl Drop for Writer {
    fn drop(&mut self) { WRITING.store(false, Ordering::Release); }
}

pub(crate) fn init() -> bool {
    let result = (|| -> io::Result<()> {
        let mut bytes = Vec::new();
        File::open(super::ARM_FILE)?.take(129).read_to_end(&mut bytes)?;
        if bytes.len() > 128 { return Err(invalid("arm exceeds 128 bytes")); }
        let arm: Arm = serde_json::from_slice(&bytes).map_err(|_| invalid("invalid arm JSON"))?;
        if arm.schema != 1 || arm.run.len() != 32
            || !arm.run.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        { return Err(invalid("invalid schema/run nonce")); }
        let mut run = [0u8; 32];
        run.copy_from_slice(arm.run.as_bytes());
        fs::create_dir_all(ROOT)?;
        // Exclusive namespace: never reuse or clear an earlier run's evidence.
        fs::create_dir(format!("{ROOT}/{}", arm.run))?;
        *SESSION.lock().unwrap() = Some(Session { run });
        publish("armed", 0, json!({"target_owner":1,"original_saved_id":2622,
            "alternate_saved_id":13216,"max_shots":2,"fingerprints_matched":true}))
    })();
    if let Err(error) = result {
        FAILED.store(true, Ordering::Release);
        super::log_always(format_args!("[MiiClosedAck] init refused: {error}"));
        false
    } else { true }
}

/// No caller may hold G or a Rust state lock. A failed write never retries or overwrites.
fn publish(event: &str, phase: u8, evidence: Value) -> io::Result<()> {
    let result = (|| -> io::Result<()> {
        if FAILED.load(Ordering::Acquire) { return Err(invalid("ack channel failed")); }
        if WRITING.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return Err(invalid("concurrent acknowledgement"));
        }
        let _writer = Writer;
        let session = (*SESSION.lock().unwrap()).ok_or_else(|| invalid("ack session absent"))?;
        let run = std::str::from_utf8(&session.run).map_err(|_| invalid("run encoding"))?;
        let sequence = SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1;
        if sequence > MAX_EVENTS || phase > 2 { return Err(invalid("ack event budget/phase")); }
        let document = json!({"schema":1,"run":run,"sequence":sequence,
            "event":event,"phase":phase,"owner":1,"evidence":evidence});
        let bytes = serde_json::to_vec(&document).map_err(|_| invalid("ack serialization"))?;
        if bytes.len() > LIMIT { return Err(invalid("ack exceeds 8192 bytes")); }
        let stem = format!("{ROOT}/{run}/{sequence:02}-{event}-{phase}");
        let part = format!("{stem}.part");
        let ready = format!("{stem}.ack");
        match fs::metadata(&ready) {
            Ok(_) => return Err(invalid("ack already exists")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {},
            Err(e) => return Err(e),
        }
        {
            // Both flags are required by the pinned Skyline std implementation.
            let mut file = OpenOptions::new().write(true).create(true).create_new(true)
                .truncate(false).open(&part)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file); // publication below cannot precede guest File close
        }
        fs::rename(part, ready)
    })();
    if let Err(error) = &result {
        FAILED.store(true, Ordering::Release);
        super::log_always(format_args!("[MiiClosedAck] publication failed event={event} phase={phase}: {error}"));
    }
    result
}

pub(crate) fn inputs_enabled() -> bool {
    !FAILED.load(Ordering::Acquire) && TERMINAL.load(Ordering::Acquire) == 0
        && READY_ENTRY.load(Ordering::Acquire) != 0
}
pub(crate) fn observed_entry() -> usize { READY_ENTRY.load(Ordering::Acquire) }
pub(crate) fn ordinary_ready(entry: usize, evidence: Value) {
    if publish("ordinary_ready", 0, evidence).is_ok() {
        READY_ENTRY.store(entry, Ordering::Release);
    }
}
pub(crate) fn prepared(phase: u8, evidence: Value) -> Result<(), String> {
    if !inputs_enabled() { return Err("closed acknowledgement not ready".into()); }
    // No native writes have occurred yet. Returning this error may refuse the shot.
    publish("prepared", phase, evidence).map_err(|e| e.to_string())
}
pub(crate) fn committed(evidence: Value) {
    // After commit: owned carry only, never file I/O or an error that could bypass INSTALL.
    *COMMITTED.lock().unwrap() = Some(evidence);
}
pub(crate) fn observation(evidence: Value) { *OBSERVATION.lock().unwrap() = Some(evidence); }
pub(crate) fn extend_observation(evidence: Value, valid: bool) {
    let mut stored = OBSERVATION.lock().unwrap();
    if let Some(value) = stored.as_mut() {
        let previous = value["valid"].as_bool() == Some(true);
        value["valid"] = json!(previous && valid);
        value["fullconfig"] = evidence;
    }
}
pub(crate) fn set_refusal(reason: String) { *REFUSAL.lock().unwrap() = Some(reason); }
pub(crate) fn take_refusal() -> String {
    REFUSAL.lock().unwrap().take().unwrap_or_else(|| "fullconfig refused without detail".into())
}

// Fixed terminal categories prevent repeat service/scene callbacks from producing spam.
pub(crate) const REFUSED: u32 = 1;
pub(crate) const STALLED: u32 = 2;
pub(crate) const DEPARTED: u32 = 4;
pub(crate) const DESTROYED: u32 = 8;
pub(crate) fn terminal(category: u32, phase: u8, reason: &str, evidence: Value) {
    if TERMINAL.fetch_or(category, Ordering::AcqRel) & category != 0 { return; }
    let committed = COMMITTED.lock().unwrap().clone();
    let _ = publish("terminal", phase, json!({"reason":reason,"detail":evidence,"committed":committed}));
}

/// Called only after the actual token finish and TRACK clear; no native work remains here.
pub(crate) fn complete(phase: u8, native: Value, finished: bool, active_after: bool) {
    let committed = COMMITTED.lock().unwrap().take();
    let observation = OBSERVATION.lock().unwrap().take();
    let valid = matches!(phase, 1 | 2) && finished && !active_after
        && TERMINAL.load(Ordering::Acquire) == 0 && native["owner"] == 1
        && native["saw_reload"] == true && native["departed"] == false
        && committed.as_ref().is_some_and(|c| c["phase"] == phase)
        && observation.as_ref().is_some_and(|o| o["valid"] == true && o["fullconfig"]["phase"] == phase);
    let evidence = json!({"native":native,"committed":committed,"observation":observation,
        "install_finished":finished,"install_active_after":active_after,"track_cleared":true});
    if valid { let _ = publish("complete", phase, evidence); }
    else { terminal(REFUSED, phase, "completion_checks_failed", evidence); }
}
