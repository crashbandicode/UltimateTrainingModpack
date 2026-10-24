//! Fixed-size diagnostic correlation. No native pointers are dereferenced here.
//! Kept independent of Skyline so the fault-capture contract can be tested.

pub const SLOTS: usize = 512;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Observation {
    pub sequence: u64,
    pub tick: u64,
    pub thread: u64,
    pub stack: u64,
    pub node: u64,
    pub links: [u64; 6],
}

impl Observation {
    pub const EMPTY: Self = Self {
        sequence: 0, tick: 0, thread: 0, stack: 0, node: 0, links: [0; 6],
    };

    fn same_call(&self, other: &Self) -> bool {
        self.sequence != 0 && self.thread == other.thread
            && self.stack == other.stack && self.node == other.node
    }
}

pub struct Correlator {
    entries: [Observation; SLOTS],
    pub overwritten: u64,
    pub unmatched: u64,
}

impl Correlator {
    pub const fn new() -> Self {
        Self { entries: [Observation::EMPTY; SLOTS], overwritten: 0, unmatched: 0 }
    }

    fn index(record: &Observation) -> usize {
        ((record.stack >> 5) ^ (record.thread >> 12)) as usize & (SLOTS - 1)
    }

    pub fn enter(&mut self, record: Observation) {
        let slot = &mut self.entries[Self::index(&record)];
        if slot.sequence != 0 {
            self.overwritten += 1;
        }
        *slot = record;
    }

    pub fn leave(&mut self, record: &Observation) -> Observation {
        let slot = &mut self.entries[Self::index(record)];
        if slot.same_call(record) {
            let result = *slot;
            *slot = Observation::EMPTY;
            result
        } else {
            self.unmatched += 1;
            Observation::EMPTY
        }
    }
}

impl Default for Correlator {
    fn default() -> Self { Self::new() }
}

/// A native post-store or explicitly identified read site observed this parent.
/// This is a bounded history, not a claim that all possible writers are hooked.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct ParentEvent {
    pub tick: u64,
    pub thread: u64,
    pub site: u64,
    pub parent: u64,
}
impl ParentEvent {
    pub const EMPTY: Self = Self { tick: 0, thread: 0, site: 0, parent: 0 };
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct NodeHistory {
    pub node: u64,
    pub owner: u64,
    pub birth: u64,
    pub birth_tick: u64,
    pub previous: ParentEvent,
    pub latest: ParentEvent,
}
impl NodeHistory {
    pub const EMPTY: Self = Self { node: 0, owner: 0, birth: 0, birth_tick: 0,
        previous: ParentEvent::EMPTY, latest: ParentEvent::EMPTY };
}

pub struct ParentHistory<const N: usize> {
    entries: [NodeHistory; N],
    pub replaced: u64,
    pub missing_birth: u64,
}
impl<const N: usize> ParentHistory<N> {
    pub const fn new() -> Self {
        Self { entries: [NodeHistory::EMPTY; N], replaced: 0, missing_birth: 0 }
    }
    fn base(node: u64) -> usize { ((node >> 4) ^ (node >> 20)) as usize % N }
    fn find(&self, node: u64) -> Option<usize> {
        (0..N.min(16)).map(|i| (Self::base(node) + i) % N)
            .find(|&i| self.entries[i].node == node)
    }
    pub fn get(&self, node: u64) -> NodeHistory {
        self.find(node).map_or(NodeHistory::EMPTY, |i| self.entries[i])
    }
    /// Preserve an earlier detach when the faulting walker observes zero.
    pub fn record_read(&mut self, node: u64, event: ParentEvent) {
        if event.parent != 0 { self.record(node, 0, 0, event); }
    }
    /// birth != 0 denotes the constructor point, resetting even same-owner
    /// address reuse. owner == 0 on clear sites means retain the recorded owner.
    pub fn record(&mut self, node: u64, owner: u64, birth: u64, event: ParentEvent) {
        if node == 0 { return; }
        let existing = self.find(node);
        let index = existing.unwrap_or_else(|| {
            (0..N.min(16)).map(|i| (Self::base(node) + i) % N)
                .find(|&i| self.entries[i].node == 0).unwrap_or_else(|| {
                    self.replaced += 1;
                    Self::base(node)
                })
        });
        let slot = &mut self.entries[index];
        let owner_changed = owner != 0 && slot.owner != 0 && owner != slot.owner;
        if existing.is_none() || birth != 0 || owner_changed {
            if birth == 0 { self.missing_birth += 1; }
            *slot = NodeHistory { node, owner, birth,
                birth_tick: if birth != 0 { event.tick } else { 0 }, ..NodeHistory::EMPTY };
        } else if owner != 0 {
            slot.owner = owner;
        }
        slot.previous = slot.latest;
        slot.latest = event;
    }
}
impl<const N: usize> Default for ParentHistory<N> {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(sequence: u64, thread: u64, stack: u64, node: u64, parent: u64) -> Observation {
        Observation { sequence, thread, stack, node, links: [parent, 0, 0, 0, 0, 0], ..Observation::EMPTY }
    }

    #[test]
    fn recursive_children_do_not_replace_parent_entry() {
        let mut table = Correlator::new();
        let parent = record(1, 0x1000, 0x8000, 0x9000, 0xa000);
        let child = record(2, 0x1000, 0x7fe0, 0x9100, 0x9000);
        table.enter(parent);
        table.enter(child);
        assert_eq!(table.leave(&child), child);
        let detached = record(3, parent.thread, parent.stack, parent.node, 0);
        assert_eq!(table.leave(&detached), parent);
        assert_eq!(table.unmatched, 0);
    }

    #[test]
    fn collision_or_other_thread_is_missing_evidence_not_false_pair() {
        let mut table = Correlator::new();
        let first = record(1, 0x1000, 0x8000, 0x9000, 0xa000);
        let collision = record(2, 0x201000, 0x8000, 0x9000, 0xb000);
        table.enter(first);
        table.enter(collision);
        assert_eq!(table.overwritten, 1);
        assert_eq!(table.leave(&first), Observation::EMPTY);
        assert_eq!(table.leave(&collision), collision);
        assert_eq!(table.unmatched, 1);
    }

    #[test]
    fn reused_stack_and_node_require_new_entry_and_are_consumed_once() {
        let mut table = Correlator::new();
        let first = record(1, 0x1000, 0x8000, 0x9000, 0xa000);
        table.enter(first);
        assert_eq!(table.leave(&first), first);
        assert_eq!(table.leave(&first), Observation::EMPTY);
        let next = record(2, first.thread, first.stack, first.node, 0xb000);
        table.enter(next);
        assert_eq!(table.leave(&next), next);
    }

    fn write(site: u64, parent: u64, tick: u64) -> ParentEvent {
        ParentEvent { tick, thread: 7, site, parent }
    }
    #[test]
    fn parent_history_distinguishes_link_from_completed_clear() {
        let mut table = ParentHistory::<8>::new();
        table.record(0x1000, 0x2000, 1, write(10, 0, 1));
        table.record(0x1000, 0x2000, 0, write(20, 0x3000, 2));
        let linked = table.get(0x1000);
        assert_eq!((linked.birth, linked.latest.parent), (1, 0x3000));
        table.record(0x1000, 0, 0, write(30, 0, 3));
        let detached = table.get(0x1000);
        assert_eq!((detached.owner, detached.previous.parent, detached.latest.site), (0x2000, 0x3000, 30));
        assert_eq!(detached.latest.parent, 0);
    }
    #[test]
    fn constructor_resets_same_address_and_owner_reuse() {
        let mut table = ParentHistory::<8>::new();
        table.record(0x1000, 0x2000, 1, write(10, 0, 1));
        table.record(0x1000, 0, 0, write(30, 0, 2));
        table.record(0x1000, 0x2000, 9, write(10, 0, 3));
        let next = table.get(0x1000);
        assert_eq!((next.birth, next.birth_tick), (9, 3));
        assert_eq!(next.previous, ParentEvent::EMPTY);
    }
    #[test]
    fn bounded_history_eviction_does_not_return_another_node() {
        let mut table = ParentHistory::<2>::new();
        table.record(0x1000, 0x2000, 1, write(10, 0, 1));
        table.record(0x1020, 0x2000, 2, write(10, 0, 2));
        table.record(0x1040, 0x2000, 3, write(10, 0, 3));
        assert_eq!(table.replaced, 1);
        assert_eq!(table.get(0x1000), NodeHistory::EMPTY);
        assert_eq!(table.get(0x1020).node, 0x1020);
        assert_eq!(table.get(0x1040).node, 0x1040);
    }
    #[test]
    fn changed_owner_without_constructor_is_unknown_generation() {
        let mut table = ParentHistory::<8>::new();
        table.record(0x1000, 0x2000, 1, write(10, 0, 1));
        table.record(0x1000, 0x3000, 0, write(20, 0x4000, 2));
        let next = table.get(0x1000);
        assert_eq!((next.birth, next.owner, table.missing_birth), (0, 0x3000, 1));
        assert_eq!(next.previous, ParentEvent::EMPTY);
    }
    #[test]
    fn invalid_entry_read_preserves_prior_detach_evidence() {
        let mut table = ParentHistory::<8>::new();
        table.record(0x1000, 0x2000, 1, write(10, 0, 1));
        table.record_read(0x1000, write(40, 0x3000, 2));
        table.record(0x1000, 0, 0, write(30, 0, 3));
        let detached = table.get(0x1000);
        table.record_read(0x1000, write(40, 0, 4));
        assert_eq!(table.get(0x1000), detached);
        assert_eq!(detached.previous.parent, 0x3000);
        assert_eq!(detached.latest.site, 30);
    }
}
