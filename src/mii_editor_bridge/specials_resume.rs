//! Values and bounded reconstruction of the native Special Moves interaction.
use super::lua_vm::{Error, Specials, Value, Vm};
use std::convert::TryFrom;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Subwindow { pub command: u8, pub cursor: u8 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub pending: [u8; 4],
    pub focus: i32,
    pub subwindow: Option<Subwindow>,
}

struct Observation {
    state: i32,
    pending: [u8; 4],
    focus: i32,
    command: i32,
    cursor: i32,
}

fn focus_valid(value: i32) -> bool { matches!(value, 0..=3 | 99 | 1000) }

impl Observation {
    unsafe fn values(vm: &mut Vm, objects: &Specials) -> Result<[Value; 8], Error> {
        vm.call::<8>(include_bytes!("lua_helpers/capture.luac"),
            &[objects.sf, objects.pending, objects.main, objects.sub])
    }

    unsafe fn read(vm: &mut Vm, objects: &Specials) -> Result<Self, Error> {
        Self::parse(Self::values(vm, objects)?)
    }

    fn parse(values: [Value; 8]) -> Result<Self, Error> {
        let mut ids = [0; 8];
        for (id, value) in ids.iter_mut().zip(values) { *id = value.int().ok_or(Error::Value)?; }
        if !(1..=8).contains(&ids[0]) || !ids[1..5].iter().all(|id| (0..3).contains(id))
            || !focus_valid(ids[5]) { return Err(Error::Value); }
        Ok(Self { state: ids[0], pending: [ids[1] as u8, ids[2] as u8, ids[3] as u8, ids[4] as u8],
            focus: ids[5], command: ids[6], cursor: ids[7] })
    }

    fn snapshot(&self) -> Result<Option<Snapshot>, Error> {
        // State 1 has already completed the native confirmation. Let the native
        // handler propagate it to DRAFT and capture the editor's main screen.
        if self.state == 1 { return Ok(None); }
        let command = if matches!(self.state, 5..=8) {
            Some(u8::try_from(self.command).ok().filter(|id| *id < 4).ok_or(Error::Value)?)
        } else { None };
        let subwindow = match self.state {
            5 | 6 => {
                let command = command.unwrap();
                Some(Subwindow { command, cursor: self.pending[command as usize] })
            }
            8 => Some(Subwindow { command: command.unwrap(),
                cursor: u8::try_from(self.cursor).ok().filter(|id| *id < 3).ok_or(Error::Value)? }),
            _ => None,
        };
        // Preserve the interaction on either side of an in-flight animation.
        // Never synthesize the final set_skill_id calls on interruption.
        let focus = match self.state {
            2 => 0,
            3 => 99,
            7 => i32::from(command.unwrap()),
            // Native MAIN_UPDATE redirects the footer to its OK button.
            _ if self.focus == 1000 => 99,
            _ => self.focus,
        };
        Ok(Some(Snapshot { pending: self.pending, focus, subwindow }))
    }
}

pub(super) unsafe fn capture(base: usize, component: usize) -> Result<Option<Snapshot>, Error> {
    let mut vm = Vm::bind(base, component)?;
    let objects = vm.specials()?;
    Observation::read(&mut vm, &objects)?.snapshot()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase { Main, Subwindow, Verify }

pub(super) struct Restore { wanted: Snapshot, phase: Phase, frames: u16 }

impl Restore {
    pub(super) fn new(wanted: Snapshot) -> Self {
        Self { wanted, phase: Phase::Main, frames: 0 }
    }

    /// Runs before one native component update. Returns true only after values,
    /// focus and (when requested) the open subwindow were read back from Lua.
    /// The caller drops this bounded state on failure or interruption.
    pub(super) unsafe fn step(&mut self, base: usize, component: usize) -> Result<bool, Error> {
        self.frames = self.frames.saturating_add(1);
        if self.frames > 120 { return Err(Error::Binding); }
        let mut vm = Vm::bind(base, component)?;
        let objects = vm.specials()?;
        let values = Observation::values(&mut vm, &objects)?;
        // The list's entrance initializes its selected row. Wait for the
        // native write state before validating that selection during startup.
        if self.phase == Phase::Main && values[0].int() != Some(4) { return Ok(false); }
        let seen = Observation::parse(values)?;
        match self.phase {
            Phase::Main if seen.state == 4 => {
                let p = self.wanted.pending.map(|id| Value::integer(i32::from(id)));
                // This updates the script's provisional table and labels only.
                // The native component and DRAFT keep their confirmed values.
                vm.call::<0>(include_bytes!("lua_helpers/pending.luac"),
                    &[objects.pending, objects.main, p[0], p[1], p[2], p[3]])?;
                vm.call::<0>(include_bytes!("lua_helpers/focus.luac"),
                    &[objects.main, Value::integer(self.wanted.focus)])?;
                if let Some(sub) = self.wanted.subwindow {
                    vm.call::<0>(include_bytes!("lua_helpers/open_sub.luac"),
                        &[objects.main, Value::integer(i32::from(sub.command))])?;
                    self.phase = Phase::Subwindow;
                } else { self.phase = Phase::Verify; }
            }
            Phase::Subwindow if seen.state == 8 => {
                let sub = self.wanted.subwindow.unwrap();
                if seen.command != i32::from(sub.command) { return Err(Error::Value); }
                vm.call::<0>(include_bytes!("lua_helpers/sub_cursor.luac"),
                    &[objects.sub, Value::integer(i32::from(sub.cursor))])?;
                self.phase = Phase::Verify;
            }
            Phase::Verify => {
                let Some(snapshot) = seen.snapshot()? else { return Err(Error::Value); };
                let expected_state = if self.wanted.subwindow.is_some() { 8 } else { 4 };
                if seen.state == expected_state && snapshot == self.wanted { return Ok(true); }
                return Err(Error::Value);
            }
            _ => {}
        }
        Ok(false)
    }
}
