//! Value-only policy shared by the native practice consumers and host probes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context { Training, Waiting }

pub fn context(native_training: bool, initialized: bool, melee: u8, mode: u32,
    waiting_active: bool) -> Option<Context> {
    if native_training { return Some(Context::Training); }
    (waiting_active && initialized && melee == 0x0d
        && (mode == 0x02010000 || mode & 0x7fffff00 == 0x07010100))
        .then_some(Context::Waiting)
}

impl Context {
    pub fn cpu_entry(self) -> i32 { if self == Self::Waiting { 4 } else { 1 } }
    pub fn role(self, entry: i32) -> Option<usize> {
        if entry == 0 { Some(0) } else if entry == self.cpu_entry() { Some(1) } else { None }
    }
}

/// Entry addresses are equality tokens only, never dereferenced after capture.
/// Death/rebirth can replace a body inside the same entry. Destruction retires
/// this binding before native free, even if the next entry reuses its address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entries { pub context: Context, pub player: usize, pub cpu: usize }
impl Entries {
    pub fn owns(self, entry: usize) -> bool { entry == self.player || entry == self.cpu }
}

pub struct EntryLease(Option<Entries>);
impl EntryLease {
    pub const fn new() -> Self { Self(None) }
    pub fn active(&self) -> bool { self.0.is_some() }
    pub fn acquire(&mut self, current: Entries) -> bool {
        if self.replaced(Some(current)) { return false; }
        self.0 = Some(current);
        true
    }
    pub fn replaced(&self, current: Option<Entries>) -> bool {
        self.0.is_some_and(|old| Some(old) != current)
    }
    pub fn owns(&self, entry: usize) -> bool { self.0.is_some_and(|old| old.owns(entry)) }
    pub fn retire(&mut self) -> bool { self.0.take().is_some() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn waiting_roles_are_not_dense_training_slots() {
        for mode in [0x02010000, 0x07010100, 0x87010101] {
            let c = context(false, true, 0x0d, mode, true).unwrap();
            assert_eq!([c.role(0), c.role(1), c.role(4), c.role(7)],
                [Some(0), None, Some(1), None]);
        }
        let c = context(true, true, 0x20, 0x04040000, false).unwrap();
        assert_eq!([c.role(0), c.role(1), c.role(4)], [Some(0), Some(1), None]);
    }
    #[test]
    fn match_css_results_arena_and_unready_waiting_cannot_acquire() {
        for (initialized, melee, mode, active) in [
            (true, 1, 0x02010000, true), (true, 1, 0x07010100, true),
            (true, 0x0e, 0x07010101, true),
            (true, 0x0d, 0x07010200, true), (true, 0x20, 0x04040000, false),
            (true, 0x0d, 0x02010000, false), (false, 0x0d, 0x02010000, true),
        ] { assert_eq!(context(false, initialized, melee, mode, active), None); }
    }
    #[test]
    fn replacement_and_mode_change_invalidate_both_roles() {
        let owned = Entries { context: Context::Waiting, player: 0x1000, cpu: 0x9000 };
        let mut lease = EntryLease::new();
        assert!(lease.acquire(owned));
        assert!(lease.owns(0x9000));
        assert!(!lease.owns(0x5000));
        // A new body inside the same entry does not change entry ownership.
        assert!(!lease.replaced(Some(owned)));
        assert!(lease.replaced(None));
        assert!(!lease.acquire(Entries { cpu: 0xa000, ..owned }));
        assert!(lease.replaced(Some(Entries { context: Context::Training, ..owned })));
        // Destruction retires the old operation even when an allocator reuses
        // both addresses. Return may acquire a NEW operation, never resume it.
        assert!(lease.retire());
        assert!(!lease.active());
        assert!(!lease.owns(0x9000));
        assert!(lease.acquire(owned));
    }
}
