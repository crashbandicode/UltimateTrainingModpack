//! Native Settings selectors, captured and rebuilt at their suspended callback.
//! The pane coordinate mirrored into the editor context is not a button id.
use super::{current_thread, lua_vm::{Error, Value, Vm}, native, Origin, Phase, BASE,
    EDIT_CONTEXT, EDIT_OWNED, STATE};
use std::sync::{atomic::Ordering, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub row: i32,
    // 0: settings list; 1: Random; 2: Test; 3: native Save footer.
    pub area: i32,
    // The unfinished ID_POPUP_MMIIF_EDIT_RANDOM prompt. Area 1 alone is the
    // Random highlight, which is not that confirmation.
    pub confirming: bool,
}

impl Snapshot {
    pub(super) const INITIAL: Self = Self { row: 0, area: 0, confirming: false };

    fn parse(row: Value, area: Value, confirming: Value) -> Result<Self, Error> {
        let row = row.int().filter(|row| (0..=5).contains(row)).ok_or(Error::Value)?;
        let area = area.int().filter(|area| (0..=3).contains(area)).ok_or(Error::Value)?;
        let confirming = match confirming.int() {
            Some(0) => false,
            Some(1) => true,
            _ => return Err(Error::Value),
        };
        if confirming && area != 1 { return Err(Error::Value); }
        Ok(Self { row, area, confirming })
    }
}

enum Operation { Capture, Restore { wanted: Snapshot, applied: bool } }

struct Job {
    context: usize,
    operation: Operation,
    frames: u16,
    result: Option<Result<Option<Snapshot>, Error>>,
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);

pub(super) fn clear() { *JOB.lock().unwrap() = None; }

/// Editor release: the next owned Settings component is a new entry.
pub(super) fn release() {
    clear();
    *POLICY.lock().unwrap() = None;
}

pub(super) fn capture(context: usize) -> Option<Result<Option<Snapshot>, Error>> {
    let mut stored = JOB.lock().unwrap();
    if stored.as_ref().is_none_or(|job| job.context != context
        || !matches!(job.operation, Operation::Capture)) {
        *stored = Some(Job { context, operation: Operation::Capture, frames: 0, result: None });
        return None;
    }
    let job = stored.as_mut().unwrap();
    job.frames = job.frames.saturating_add(1);
    if job.frames > 120 && job.result.is_none() { job.result = Some(Err(Error::Binding)); }
    let result = job.result.take();
    if result.is_some() { *stored = None; }
    result
}

pub(super) fn begin_restore(context: usize, wanted: Snapshot) {
    *JOB.lock().unwrap() = Some(Job { context,
        operation: Operation::Restore { wanted, applied: false }, frames: 0, result: None });
}

pub(super) fn restore_result(context: usize) -> Option<Result<Option<Snapshot>, Error>> {
    let mut stored = JOB.lock().unwrap();
    let result = stored.as_mut().filter(|job| job.context == context
        && matches!(job.operation, Operation::Restore { .. })).and_then(|job| job.result.take());
    if result.is_some() { *stored = None; }
    result
}

unsafe fn observe(vm: &mut Vm, settings: Value) -> Result<Option<Snapshot>, Error> {
    let [done, row, area, confirming] = vm.call::<4>(
        include_bytes!("lua_helpers/settings_capture.luac"), &[settings])?;
    match done.int() {
        Some(1) => Ok(None),
        Some(0) => Snapshot::parse(row, area, confirming).map(Some),
        _ => Err(Error::Value),
    }
}

impl Job {
    unsafe fn step(&mut self, component: usize) -> Result<Option<Snapshot>, Error> {
        let mut vm = Vm::bind(BASE.load(Ordering::Acquire), component)?;
        let settings = vm.settings()?;
        let seen = observe(&mut vm, settings)?;
        match self.operation {
            Operation::Capture => Ok(seen),
            Operation::Restore { wanted, applied: true } => {
                if seen == Some(wanted) { Ok(seen) } else { Err(Error::Value) }
            }
            Operation::Restore { wanted, applied: false } => {
                if seen.is_none() { return Err(Error::Value); }
                if seen == Some(wanted) {
                    self.operation = Operation::Restore { wanted, applied: true };
                    return Ok(seen);
                }
                // A busy manager may still be the previous prompt. Opening
                // again would stack ID_POPUP_MMIIF_EDIT_RANDOM. A highlight
                // is not busy, so that is the one open.
                let open = i32::from(wanted.confirming && seen.is_some_and(|seen| !seen.confirming));
                vm.call::<0>(include_bytes!("lua_helpers/settings_focus.luac"), &[
                    settings, Value::integer(wanted.row), Value::integer(wanted.area),
                    Value::integer(open),
                ])?;
                self.operation = Operation::Restore { wanted, applied: true };
                Ok(None)
            }
        }
    }
}

/// Retained disabler. Training and Quickplay keep the native cosmetic rows.
/// Local Wireless Add New and Test do not enable those rows. Native
/// `initialize` rebuilds both selectors on every Settings entry (component
/// age 0), so a caller still runs once per entry, never per frame.
pub(super) fn suppress_native_cosmetics(origin: Origin) -> bool {
    match origin {
        Origin::Training | Origin::Quickplay => false,
        Origin::LocalWireless => true,
    }
}
struct Policy { context: usize, component: usize, last_age: u32, attempts: u16, settled: bool }

static POLICY: Mutex<Option<Policy>> = Mutex::new(None);

impl Policy {
    const ATTEMPTS: u16 = 120;

    /// Whether this callback should run the helper for the bound owned component.
    fn due(slot: &mut Option<Self>, context: usize, component: usize, age: u32) -> bool {
        // A new component, or an age that went backwards, is a new Settings entry.
        if !slot.as_ref().is_some_and(|entry| entry.context == context
            && entry.component == component && age >= entry.last_age) {
            *slot = None;
        }
        let entry = slot.get_or_insert_with(|| Self {
            context, component, last_age: 0, attempts: 0, settled: false,
        });
        entry.last_age = age;
        age >= 1 && !entry.settled && entry.attempts < Self::ATTEMPTS
    }

    /// The trace event for a finished helper run, once it is settled.
    fn record(&mut self, result: &Result<i32, Error>) -> Option<(&'static str, [u64; 4])> {
        self.attempts = self.attempts.saturating_add(1);
        let (age, attempts) = (u64::from(self.last_age), u64::from(self.attempts));
        let failed = |code: u64| Some(("waiting_cosmetics_policy_failed", [code, age, attempts, 0]));
        match result {
            // 1: gate installed and rows disabled; 0: gate present, rows disabled again.
            Ok(applied @ (0 | 1)) => {
                self.settled = true;
                Some(("waiting_cosmetics_disabled", [u64::from(*applied == 0), age, attempts, 0]))
            }
            // Selectors not built yet, or the player not bound yet: next callback.
            Ok(2) | Err(Error::Binding | Error::Stack) if self.attempts < Self::ATTEMPTS => None,
            Ok(2) => { self.settled = true; failed(0) }
            Err(error) => { self.settled = true; failed(error.code() as u64) }
            Ok(_) => { self.settled = true; failed(Error::Value.code() as u64) }
        }
    }
}

unsafe fn install(component: usize) -> Result<i32, Error> {
    // initialize has built the selectors before main's first resume at age 1;
    // the helper reports 2 until both exist and is retried on the next callback.
    let mut vm = Vm::bind_from(BASE.load(Ordering::Acquire), component, 1)?;
    let settings = vm.settings()?;
    let [applied] = vm.call::<1>(include_bytes!("lua_helpers/settings_policy.luac"), &[settings])?;
    applied.int().ok_or(Error::Value)
}

unsafe fn apply_policy(context: usize, component: usize) {
    let age: u32 = native::read(component, 0xe4);
    if !Policy::due(&mut POLICY.lock().unwrap(), context, component, age) { return; }
    // Protected Lua runs without a Rust mutex held.
    let result = install(component);
    let event = POLICY.lock().unwrap().as_mut()
        .filter(|policy| policy.context == context && policy.component == component)
        .and_then(|policy| policy.record(&result));
    if let Some((name, values)) = event {
        if let Some(session) = STATE.lock().unwrap().as_mut() { session.event(name, values); }
        if name == "waiting_cosmetics_policy_failed" {
            // Fail open: the native rows stay as they are. Quickplay resume reads
            // the child page itself, so a helper failure must not invent a cursor.
            crate::logging::diagnostic(format_args!("[MiiEditor] waiting cosmetics policy failed: {result:?}"));
        }
    }
}

#[skyline::hook(offset = 0x19ca590)]
unsafe fn settings_frame(component: usize) {
    let context = EDIT_CONTEXT.load(Ordering::Acquire);
    // The ordinary editor pays no Lua work: an unowned editor never reaches the
    // session lock, and an owned Training editor only compares its origin.
    let pending = JOB.lock().unwrap().as_ref().is_some_and(|job|
        job.context == context && job.result.is_none());
    let origin = if EDIT_OWNED.load(Ordering::Acquire) && context != 0 {
        STATE.lock().unwrap().as_ref()
            .filter(|session| session.phase == Phase::Editing && session.thread == current_thread())
            .map(|session| session.origin)
    } else { None };
    let owned = origin.is_some();
    let suppress = origin.is_some_and(suppress_native_cosmetics);
    let bound = owned && (pending || suppress) && {
        let holder: usize = native::read(context, 0x178);
        native::aligned(holder) && native::read::<usize>(holder, 0x80) == component
    };
    if bound && suppress { apply_policy(context, component); }
    // Age 0 starts the coroutine; wait until its default selection has run.
    if bound && pending && native::read::<u32>(component, 0xe4) > 1 {
        let mut job = JOB.lock().unwrap().take().unwrap();
        let capture = matches!(job.operation, Operation::Capture);
        let result = job.step(component);
        let completed = matches!(result, Ok(None));
        if capture || !completed { job.result = Some(result); }
        *JOB.lock().unwrap() = Some(job);
        // Freeze a captured interaction for this tick. A completed native
        // action must propagate normally; never convert Cancel into Save.
        if capture && !completed { return; }
    }
    original!()(component);
}

pub(super) fn init() { skyline::install_hooks!(settings_frame); }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_runs_once_per_settings_entry() {
        let mut slot = None;
        assert!(!Policy::due(&mut slot, 1, 2, 0), "age 0 has no selectors yet");
        assert!(Policy::due(&mut slot, 1, 2, 1));
        assert_eq!(slot.as_mut().unwrap().record(&Ok(1)),
            Some(("waiting_cosmetics_disabled", [0, 1, 1, 0])));
        assert!(!Policy::due(&mut slot, 1, 2, 2), "settled entries stay idle");
        // Returning to Settings resets the native component age: reapply once.
        assert!(!Policy::due(&mut slot, 1, 2, 0));
        assert!(Policy::due(&mut slot, 1, 2, 1));
        assert_eq!(slot.as_mut().unwrap().record(&Ok(0)),
            Some(("waiting_cosmetics_disabled", [1, 1, 1, 0])));
        assert!(!Policy::due(&mut slot, 1, 2, 2));
        assert!(Policy::due(&mut slot, 1, 3, 5), "another component is a new entry");
    }

    #[test]
    fn quickplay_keeps_native_cosmetic_rows() {
        assert!(!suppress_native_cosmetics(Origin::Quickplay));
        assert!(!suppress_native_cosmetics(Origin::Training));
        assert!(suppress_native_cosmetics(Origin::LocalWireless));
        let random = Snapshot::parse(Value::integer(2), Value::integer(1), Value::integer(0)).unwrap();
        assert_eq!(random, Snapshot { row: 2, area: 1, confirming: false });
        let voice_row = Snapshot::parse(Value::integer(5), Value::integer(0), Value::integer(0)).unwrap();
        assert_eq!(voice_row.row, 5);
    }

    #[test]
    fn random_confirmation_is_not_a_highlight() {
        let highlight = Snapshot::parse(Value::integer(2), Value::integer(1), Value::integer(0)).unwrap();
        let pending = Snapshot::parse(Value::integer(2), Value::integer(1), Value::integer(1)).unwrap();
        assert_eq!(highlight, Snapshot { row: 2, area: 1, confirming: false });
        assert_eq!(pending, Snapshot { row: 2, area: 1, confirming: true });
        assert_ne!(highlight, pending);
        assert!(Snapshot::parse(Value::integer(2), Value::integer(0), Value::integer(1)).is_err());
        assert!(Snapshot::parse(Value::integer(2), Value::integer(1), Value::integer(2)).is_err());
        let focus = include_str!("lua_helpers/settings_focus.lua");
        let capture = include_str!("lua_helpers/settings_capture.lua");
        assert!(focus.contains("open_database(\"ID_POPUP_MMIIF_EDIT_RANDOM\")"));
        assert!(!focus.contains("set_random"));
        assert!(!capture.contains("get_result"));
        assert!(!capture.contains("set_random"));
    }

    #[test]
    fn policy_retries_transient_results_and_stops_on_identity() {
        let mut slot = None;
        assert!(Policy::due(&mut slot, 1, 2, 1));
        assert_eq!(slot.as_mut().unwrap().record(&Ok(2)), None);
        assert!(Policy::due(&mut slot, 1, 2, 2));
        assert_eq!(slot.as_mut().unwrap().record(&Err(Error::Binding)), None);
        assert!(Policy::due(&mut slot, 1, 2, 3));
        assert_eq!(slot.as_mut().unwrap().record(&Err(Error::Identity)),
            Some(("waiting_cosmetics_policy_failed", [3, 3, 3, 0])));
        assert!(!Policy::due(&mut slot, 1, 2, 4));
    }

    #[test]
    fn policy_gives_up_after_bounded_attempts() {
        let mut slot = None;
        for age in 1..u32::from(Policy::ATTEMPTS) {
            assert!(Policy::due(&mut slot, 1, 2, age));
            assert_eq!(slot.as_mut().unwrap().record(&Ok(2)), None);
        }
        assert!(Policy::due(&mut slot, 1, 2, u32::from(Policy::ATTEMPTS)));
        assert_eq!(slot.as_mut().unwrap().record(&Ok(2)),
            Some(("waiting_cosmetics_policy_failed", [0, 120, 120, 0])));
        assert!(!Policy::due(&mut slot, 1, 2, 121));
    }
}
