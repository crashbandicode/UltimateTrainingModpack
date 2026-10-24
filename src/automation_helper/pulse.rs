//! Bounded per-port UI pulse. INPUT45 record layout and step rules.
//! Durations are UI steps, not held native buttons. A step is one builder call.
//! 100 ms is one press edge plus five holds. 180 ms of stick is eleven holds,
//! from that same "about 100 ms" ratio (ceil(180/100*6)). That ratio is not a
//! measured UI period.

use std::sync::atomic::{fence, AtomicU32, AtomicU64, Ordering};

pub const MASK_A: u32 = 1 << 5;
pub const MASK_Y: u32 = 1 << 7;
pub const MASK_L_LEFT: u32 = 1 << 23;
pub const MASK_L_RIGHT: u32 = 1 << 21;
pub const BUTTON_HELD_STEPS: u8 = 6;
pub const STICK_HELD_STEPS: u8 = 11;
pub const NEUTRAL_BEFORE_PRESS: u8 = 10;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Record {
    pub held: u32,
    pub pressed: u32,
    pub released: u32,
    pub pad: u32,
    pub left: [f32; 4],
    pub right: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<Record>() == 0x30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Idle = 0,
    ButtonA = 1,
    ButtonY = 2,
    RematchNo = 3,
    RematchYes = 4,
    SaveYes = 5,
}

impl Action {
    pub const fn code(self) -> u32 {
        self as u32
    }
    pub const fn from_code(code: u32) -> Self {
        match code {
            1 => Self::ButtonA,
            2 => Self::ButtonY,
            3 => Self::RematchNo,
            4 => Self::RematchYes,
            5 => Self::SaveYes,
            _ => Self::Idle,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command {
    pub generation: u64,
    pub epoch: u64,
    pub serial: u32,
    pub action: Action,
}

impl Command {
    pub const fn idle() -> Self {
        Self { generation: 0, epoch: 0, serial: 0, action: Action::Idle }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepKind {
    Edge { mask: u32, stick: bool },
    Hold { mask: u32, stick: bool },
    Release { mask: u32 },
    Neutral,
}

pub struct Scheduler {
    serial: u32,
    script: [StepKind; 28],
    len: u8,
    cursor: u8,
    pending: Option<Command>,
    forced_release: Option<u32>,
    held_button: Option<u32>,
    neutral: u8,
    had_press: bool,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            serial: 0,
            script: [StepKind::Neutral; 28],
            len: 0,
            cursor: 0,
            pending: None,
            forced_release: None,
            held_button: None,
            neutral: NEUTRAL_BEFORE_PRESS,
            had_press: false,
        }
    }

    pub fn step(&mut self, record: &mut Record, command: Option<Command>) {
        self.retarget(command);
        if let Some(mask) = self.forced_release.take() {
            record.released |= mask;
            self.held_button = None;
            self.neutral = 0;
            self.had_press = true;
            return;
        }
        if self.cursor >= self.len {
            self.neutral = self.neutral.saturating_add(1).min(NEUTRAL_BEFORE_PRESS);
            self.start_pending();
            return;
        }
        let kind = self.script[usize::from(self.cursor)];
        self.cursor = self.cursor.saturating_add(1);
        match kind {
            StepKind::Edge { mask, stick } => self.press(record, mask, stick, true),
            StepKind::Hold { mask, stick } => self.press(record, mask, stick, false),
            StepKind::Release { mask } => {
                record.released |= mask;
                self.held_button = None;
                self.neutral = 0;
                self.had_press = true;
            }
            StepKind::Neutral => {
                self.held_button = None;
                self.neutral = self.neutral.saturating_add(1).min(NEUTRAL_BEFORE_PRESS);
            }
        }
    }

    fn press(&mut self, record: &mut Record, mask: u32, stick: bool, edge: bool) {
        record.held |= mask;
        if edge {
            record.pressed |= mask;
            self.had_press = true;
            self.neutral = 0;
        }
        if stick {
            record.left[0] = if mask == MASK_L_RIGHT { 1.0 } else { -1.0 };
            record.left[1] = 0.0;
            record.left[3] = 0.0;
            self.held_button = None;
        } else {
            self.held_button = Some(mask);
        }
    }

    fn retarget(&mut self, command: Option<Command>) {
        let Some(cmd) = command else {
            self.pending = None;
            if self.cursor < self.len || self.forced_release.is_some() || self.held_button.is_some() {
                self.cancel_script();
            }
            return;
        };
        if cmd.serial == self.serial {
            return;
        }
        if cmd.action == Action::Idle {
            self.pending = None;
            self.serial = cmd.serial;
            self.cancel_script();
            return;
        }
        if self.cursor < self.len || self.forced_release.is_some() || self.held_button.is_some() {
            self.serial = cmd.serial;
            self.pending = Some(cmd);
            self.cancel_script();
        } else if self.had_press && self.neutral < NEUTRAL_BEFORE_PRESS {
            self.serial = cmd.serial;
            self.pending = Some(cmd);
        } else {
            self.pending = None;
            self.start(cmd);
        }
    }

    fn cancel_script(&mut self) {
        self.cursor = 0;
        self.len = 0;
        if let Some(mask) = self.held_button.take() {
            self.forced_release = Some(mask);
        }
    }

    fn start_pending(&mut self) {
        if self.forced_release.is_some() {
            return;
        }
        let ready = !self.had_press || self.neutral >= NEUTRAL_BEFORE_PRESS;
        if ready {
            if let Some(cmd) = self.pending.take() {
                self.start(cmd);
            }
        }
    }

    fn start(&mut self, cmd: Command) {
        self.serial = cmd.serial;
        self.cursor = 0;
        self.len = write_script(cmd.action, &mut self.script);
    }
}

fn write_script(action: Action, out: &mut [StepKind; 28]) -> u8 {
    match action {
        Action::Idle => 0,
        Action::ButtonA | Action::RematchYes => button(MASK_A, out),
        Action::ButtonY => button(MASK_Y, out),
        Action::RematchNo => select_then_confirm(MASK_L_LEFT, out),
        Action::SaveYes => select_then_confirm(MASK_L_RIGHT, out),
    }
}

fn button(mask: u32, out: &mut [StepKind; 28]) -> u8 {
    out[0] = StepKind::Edge { mask, stick: false };
    let mut index = 1u8;
    while index < BUTTON_HELD_STEPS {
        out[usize::from(index)] = StepKind::Hold { mask, stick: false };
        index += 1;
    }
    out[usize::from(index)] = StepKind::Release { mask };
    index + 1
}

fn select_then_confirm(direction: u32, out: &mut [StepKind; 28]) -> u8 {
    out[0] = StepKind::Edge { mask: direction, stick: true };
    let mut index = 1u8;
    while index < STICK_HELD_STEPS {
        out[usize::from(index)] = StepKind::Hold { mask: direction, stick: true };
        index += 1;
    }
    let mut gap = 0u8;
    while gap < NEUTRAL_BEFORE_PRESS {
        out[usize::from(index)] = StepKind::Neutral;
        index += 1;
        gap += 1;
    }
    out[usize::from(index)] = StepKind::Edge { mask: MASK_A, stick: false };
    index += 1;
    let mut held = 1u8;
    while held < BUTTON_HELD_STEPS {
        out[usize::from(index)] = StepKind::Hold { mask: MASK_A, stick: false };
        index += 1;
        held += 1;
    }
    out[usize::from(index)] = StepKind::Release { mask: MASK_A };
    index + 1
}

/// One stable publish. A busy writer preserves the reader's last command; it does not
/// cancel the pulse. An unknown action code loads as idle.
pub struct Slot {
    seq: AtomicU64,
    generation: AtomicU64,
    epoch: AtomicU64,
    serial: AtomicU32,
    action: AtomicU32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sample {
    Stable(Command),
    Busy,
}

impl Slot {
    pub const fn new() -> Self {
        Self {
            seq: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            epoch: AtomicU64::new(0),
            serial: AtomicU32::new(0),
            action: AtomicU32::new(0),
        }
    }

    pub fn publish(&self, command: Command) {
        self.seq.fetch_add(1, Ordering::Relaxed);
        fence(Ordering::Release);
        self.generation.store(command.generation, Ordering::Relaxed);
        self.epoch.store(command.epoch, Ordering::Relaxed);
        self.serial.store(command.serial, Ordering::Relaxed);
        self.action.store(command.action.code(), Ordering::Relaxed);
        self.seq.fetch_add(1, Ordering::Release);
    }

    pub fn load(&self) -> Sample {
        let mut spins = 0u8;
        loop {
            let start = self.seq.load(Ordering::Acquire);
            if start & 1 != 0 {
                spins += 1;
                if spins == 3 {
                    return Sample::Busy;
                }
                continue;
            }
            let command = Command {
                generation: self.generation.load(Ordering::Relaxed),
                epoch: self.epoch.load(Ordering::Relaxed),
                serial: self.serial.load(Ordering::Relaxed),
                action: Action::from_code(self.action.load(Ordering::Relaxed)),
            };
            fence(Ordering::Acquire);
            let end = self.seq.load(Ordering::Relaxed);
            if start == end && start & 1 == 0 {
                return Sample::Stable(command);
            }
            spins += 1;
            if spins == 3 {
                return Sample::Busy;
            }
        }
    }
}

pub fn inject_port(
    records: &mut [Record; 10],
    port: u8,
    command: Option<Command>,
    sched: &mut Scheduler,
) -> bool {
    let index = usize::from(port);
    if index >= records.len() {
        return false;
    }
    sched.step(&mut records[index], command);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh(trigger: f32) -> Record {
        Record {
            held: 0,
            pressed: 0,
            released: 0,
            pad: 0,
            left: [0.25, -0.5, trigger, 0.75],
            right: [0.1, 0.2, 0.3, 0.4],
        }
    }

    fn run(action: Action, steps: usize) -> Vec<Record> {
        let mut sched = Scheduler::new();
        let cmd = Command { generation: 0x100, epoch: 1, serial: 1, action };
        let mut out = Vec::new();
        for _ in 0..steps {
            let mut record = fresh(0.4);
            sched.step(&mut record, Some(cmd));
            out.push(record);
        }
        out
    }

    #[test]
    fn record_is_one_port_slot_and_only_that_port_changes() {
        let mut records = [fresh(0.4); 10];
        for record in &mut records {
            record.held = 0x11;
        }
        let mut sched = Scheduler::new();
        let cmd = Command { generation: 1, epoch: 1, serial: 1, action: Action::ButtonY };
        assert!(inject_port(&mut records, 3, Some(cmd), &mut sched));
        assert_eq!(records[3].held, 0x11 | MASK_Y);
        assert_eq!(records[3].pressed, MASK_Y);
        assert_eq!(records[3].right, [0.1, 0.2, 0.3, 0.4]);
        for (index, record) in records.iter().enumerate() {
            if index != 3 {
                assert_eq!(record.held, 0x11);
                assert_eq!(record.pressed, 0);
            }
        }
        let mut untouched = [fresh(0.4); 10];
        assert!(!inject_port(&mut untouched, 10, Some(cmd), &mut sched));
        assert_eq!(untouched[0].held, 0);
    }

    #[test]
    fn button_is_one_edge_five_holds_then_one_release_and_does_not_repeat() {
        let trace = run(Action::ButtonA, 9);
        assert_eq!(trace[0].held, MASK_A);
        assert_eq!(trace[0].pressed, MASK_A);
        assert_eq!(trace[0].released, 0);
        for (step, record) in trace.iter().enumerate().take(6).skip(1) {
            assert_eq!(record.held, MASK_A, "hold {step}");
            assert_eq!(record.pressed, 0);
        }
        assert_eq!(trace[6].held, 0);
        assert_eq!(trace[6].released, MASK_A);
        assert_eq!(trace[6].left[2], 0.4);
        assert_eq!(trace[7].held, 0);
        assert_eq!(trace[7].released, 0);
        assert_eq!(trace[8].pressed, 0);
        let yes = run(Action::RematchYes, 7);
        assert_eq!(yes[0].pressed, MASK_A);
        assert_eq!(yes[0].left[0], 0.25);
        assert_ne!(yes[0].held, MASK_L_LEFT);
    }

    #[test]
    fn rematch_no_holds_stick_then_ten_neutrals_then_one_a() {
        let trace = run(Action::RematchNo, 30);
        for (step, record) in trace.iter().enumerate().take(11) {
            assert_eq!(record.held, MASK_L_LEFT, "stick {step}");
            assert_eq!(record.left[0], -1.0);
            assert_eq!(record.left[1], 0.0);
            assert_eq!(record.left[2], 0.4);
            assert_eq!(record.left[3], 0.0);
            assert_eq!(record.right[0], 0.1);
        }
        assert_eq!(trace[0].pressed, MASK_L_LEFT);
        assert_eq!(trace[1].pressed, 0);
        for (step, record) in trace.iter().enumerate().take(21).skip(11) {
            assert_eq!(record.held, 0, "gap {step}");
            assert_eq!(record.pressed, 0);
            assert_eq!(record.released, 0);
            assert_eq!(record.left[0], 0.25);
        }
        assert_eq!(trace[21].pressed, MASK_A);
        assert_eq!(trace[21].held, MASK_A);
        for record in trace.iter().take(27).skip(22) {
            assert_eq!(record.held, MASK_A);
            assert_eq!(record.pressed, 0);
        }
        assert_eq!(trace[27].released, MASK_A);
        assert_eq!(trace[27].held, 0);
        assert_eq!(trace[28].held, 0);
        assert_eq!(trace[29].pressed, 0);
    }

    #[test]
    fn cancel_releases_a_button_once_and_drops_the_rest_of_no() {
        let mut sched = Scheduler::new();
        let cmd = Command { generation: 1, epoch: 1, serial: 1, action: Action::ButtonY };
        let mut record = fresh(0.4);
        sched.step(&mut record, Some(cmd));
        record = fresh(0.4);
        sched.step(&mut record, Some(cmd));
        record = fresh(0.4);
        sched.step(&mut record, None);
        assert_eq!(record.released, MASK_Y);
        assert_eq!(record.held, 0);
        record = fresh(0.4);
        sched.step(&mut record, None);
        assert_eq!(record.held, 0);
        assert_eq!(record.released, 0);

        let mut sched = Scheduler::new();
        let no = Command { generation: 1, epoch: 2, serial: 3, action: Action::RematchNo };
        for _ in 0..4 {
            let mut record = fresh(0.4);
            sched.step(&mut record, Some(no));
        }
        let idle = Command { generation: 0, epoch: 0, serial: 4, action: Action::Idle };
        let mut record = fresh(0.4);
        sched.step(&mut record, Some(idle));
        assert_eq!(record.held, 0);
        assert_eq!(record.pressed, 0);
        assert_eq!(record.left[0], 0.25);
        for _ in 0..8 {
            record = fresh(0.4);
            sched.step(&mut record, Some(idle));
            assert_eq!(record.pressed, 0);
            assert_eq!(record.held & MASK_A, 0);
            assert_eq!(record.left[0], 0.25);
        }
    }

    #[test]
    fn a_second_press_waits_ten_neutral_steps() {
        let mut sched = Scheduler::new();
        let y = Command { generation: 1, epoch: 1, serial: 1, action: Action::ButtonY };
        for _ in 0..7 {
            let mut record = fresh(0.4);
            sched.step(&mut record, Some(y));
        }
        let a = Command { generation: 1, epoch: 1, serial: 2, action: Action::ButtonA };
        for step in 0..10 {
            let mut record = fresh(0.4);
            sched.step(&mut record, Some(a));
            assert_eq!(record.pressed, 0, "neutral {step}");
            assert_eq!(record.held, 0, "neutral {step}");
        }
        let mut record = fresh(0.4);
        sched.step(&mut record, Some(a));
        assert_eq!(record.pressed, MASK_A);
    }

    #[test]
    fn slot_round_trip_keeps_generation_and_unknown_action_is_idle() {
        let slot = Slot::new();
        let cmd = Command { generation: 0x300, epoch: 9, serial: 4, action: Action::RematchNo };
        slot.publish(cmd);
        assert_eq!(slot.load(), Sample::Stable(cmd));
        assert_eq!(Action::from_code(99), Action::Idle);
    }
}
