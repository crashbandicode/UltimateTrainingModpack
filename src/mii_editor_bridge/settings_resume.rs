//! Native Settings selectors, captured and rebuilt at their suspended callback.
//! The pane coordinate mirrored into the editor context is not a button id.
use super::{current_thread, lua_vm::{Error, Value, Vm}, native, Phase, BASE,
    EDIT_CONTEXT, EDIT_OWNED, STATE};
use std::sync::{atomic::Ordering, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub row: i32,
    // 0: settings list; 1: Random; 2: Test; 3: native Save footer.
    pub area: i32,
}

impl Snapshot {
    pub(super) const INITIAL: Self = Self { row: 0, area: 0 };

    fn parse(row: Value, area: Value) -> Result<Self, Error> {
        let row = row.int().filter(|row| (0..=5).contains(row)).ok_or(Error::Value)?;
        let area = area.int().filter(|area| (0..=3).contains(area)).ok_or(Error::Value)?;
        Ok(Self { row, area })
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
    let [done, row, area] = vm.call::<3>(include_bytes!("lua_helpers/settings_capture.luac"), &[settings])?;
    match done.int() {
        Some(1) => Ok(None),
        Some(0) => Snapshot::parse(row, area).map(Some),
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
                vm.call::<0>(include_bytes!("lua_helpers/settings_focus.luac"),
                    &[settings, Value::integer(wanted.row), Value::integer(wanted.area)])?;
                self.operation = Operation::Restore { wanted, applied: true };
                Ok(None)
            }
        }
    }
}

#[skyline::hook(offset = 0x19ca590)]
unsafe fn settings_frame(component: usize) {
    let context = EDIT_CONTEXT.load(Ordering::Acquire);
    // The ordinary editor pays no Lua work when no capture/restore is pending.
    let pending = JOB.lock().unwrap().as_ref().is_some_and(|job|
        job.context == context && job.result.is_none());
    let owned = pending && EDIT_OWNED.load(Ordering::Acquire) && context != 0
        && STATE.lock().unwrap().as_ref().is_some_and(|session|
            session.phase == Phase::Editing && session.thread == current_thread());
    let bound = owned && {
        let holder: usize = native::read(context, 0x178);
        native::aligned(holder) && native::read::<usize>(holder, 0x80) == component
    };
    // Age 0 starts the coroutine; wait until its default selection has run.
    if bound && native::read::<u32>(component, 0xe4) > 1 {
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
