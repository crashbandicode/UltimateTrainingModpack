//! Allocation-free source lifetime bookkeeping. The caller serializes access and
//! verifies native ownership/readiness; this module never selects native slots.
//! Keep one book alive across scene changes. Tokens belong to that book, must not
//! cross book instances, and must be retained by native callbacks before loading.
//! Cancelling UI/queue intent only discards its epoch/token, never ends a load.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceKey {
    pub kind: u32,
    pub costume: u32,
}

/// Numeric native identity, not a pointer or proof of slot ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeEntry(pub u64);

/// Caller-defined category and identity (copy body, live item, etc.).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Holder {
    pub category: u32,
    pub id: u64,
}

/// Stamp pending intent and check it before performing any consumer operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneEpoch(u64);

/// Retain this full identity for destruction, including at a reused native entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceToken {
    slot: usize,
    entry: NativeEntry,
    generation: u64,
    epoch: SceneEpoch,
}

impl SourceToken {
    pub fn slot(self) -> usize { self.slot }
    pub fn entry(self) -> NativeEntry { self.entry }
    pub fn epoch(self) -> SceneEpoch { self.epoch }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadToken(SourceToken);

impl LoadToken {
    pub fn source(self) -> SourceToken { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadyToken(SourceToken);

impl ReadyToken {
    pub fn source(self) -> SourceToken { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaseToken {
    index: usize,
    serial: u64,
    source: SourceToken,
    holder: Holder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase { Empty, Loading, Ready, Failed, Departed }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidSlot,
    Occupied,
    EntryAlreadyRegistered,
    Pinned,
    StaleToken,
    NotReady,
    Departed,
    LeaseCapacity,
    CounterExhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Slot {
    generation: u64,
    source: Option<SourceToken>,
    key: Option<SourceKey>,
    phase: Phase,
}

impl Slot {
    const EMPTY: Self = Self {
        generation: 0, source: None, key: None, phase: Phase::Empty,
    };
}

/// Fixed bookkeeping capacity, not permission to use any native entry number.
pub struct SourceBook<const SOURCES: usize, const LEASES: usize> {
    slots: [Slot; SOURCES],
    leases: [Option<LeaseToken>; LEASES],
    epoch: SceneEpoch,
    lease_serial: u64,
}

impl<const SOURCES: usize, const LEASES: usize> SourceBook<SOURCES, LEASES> {
    pub const fn new() -> Self {
        Self {
            slots: [Slot::EMPTY; SOURCES], leases: [None; LEASES],
            epoch: SceneEpoch(0), lease_serial: 0,
        }
    }

    pub fn epoch(&self) -> SceneEpoch { self.epoch }

    pub fn is_current(&self, epoch: SceneEpoch) -> bool { self.epoch == epoch }

    pub fn phase(&self, slot: usize) -> Result<Phase, Error> {
        Ok(self.slot(slot)?.phase)
    }

    pub fn key(&self, slot: usize) -> Result<Option<SourceKey>, Error> {
        Ok(self.slot(slot)?.key)
    }

    /// Lookup only; loading/departed entries cannot supply ready tokens.
    pub fn ready(&self, slot: usize) -> Result<ReadyToken, Error> {
        let state = self.slot(slot)?;
        if state.phase != Phase::Ready { return Err(Error::NotReady); }
        Ok(ReadyToken(state.source.ok_or(Error::StaleToken)?))
    }

    /// Read-only advisory; begin_install rechecks before changing anything.
    pub fn can_replace(&self, slot: usize) -> Result<bool, Error> {
        let state = self.slot(slot)?;
        Ok(match state.phase {
            Phase::Empty => true,
            Phase::Ready | Phase::Failed => !self.pinned(slot),
            Phase::Loading | Phase::Departed => false,
        })
    }

    /// Register a freshly verified, already-ready source in an empty book slot.
    /// Caller must verify the entry/key in this epoch; never imports old tokens.
    pub fn register_ready(
        &mut self, slot: usize, epoch: SceneEpoch, entry: NativeEntry, key: SourceKey,
    ) -> Result<ReadyToken, Error> {
        if self.slot(slot)?.phase != Phase::Empty { return Err(Error::Occupied); }
        self.admit(slot, epoch, entry, key, Phase::Ready).map(ReadyToken)
    }

    /// Record an actual new native load on the caller's verified slot. Call under
    /// the same serialization as native initiation; retain its token for callbacks.
    /// Loading and departed slots stay occupied regardless of consumer count.
    pub fn begin_install(
        &mut self, slot: usize, epoch: SceneEpoch, entry: NativeEntry, key: SourceKey,
    ) -> Result<LoadToken, Error> {
        match self.slot(slot)?.phase {
            Phase::Loading | Phase::Departed => return Err(Error::Occupied),
            _ => {}
        }
        if self.pinned(slot) { return Err(Error::Pinned); }
        if self.slot(slot)?.source.is_some_and(|source| source.entry != entry) {
            return Err(Error::Occupied);
        }
        self.admit(slot, epoch, entry, key, Phase::Loading).map(LoadToken)
    }

    /// Only the exact current load can publish readiness. A late departed load
    /// stays occupied; use native_destroyed after verified native cleanup.
    pub fn complete(&mut self, load: LoadToken) -> Result<ReadyToken, Error> {
        let state = self.exact(load.0)?;
        if state.phase == Phase::Departed { return Err(Error::Departed); }
        if !self.is_current(load.0.epoch) { return Err(Error::StaleToken); }
        if state.phase != Phase::Loading { return Err(Error::NotReady); }
        self.slots[load.0.slot].phase = Phase::Ready;
        Ok(ReadyToken(load.0))
    }

    /// Native completion is proven, but usable resources were not obtained.
    /// Keep the native lifetime occupied and non-ready; permit an in-place retry.
    /// Never call this merely because an in-flight load exceeded a timeout.
    pub fn fail(&mut self, load: LoadToken) -> Result<(), Error> {
        let state = self.exact(load.0)?;
        if state.phase == Phase::Departed { return Err(Error::Departed); }
        if !self.is_current(load.0.epoch) { return Err(Error::StaleToken); }
        if state.phase != Phase::Loading { return Err(Error::NotReady); }
        self.slots[load.0.slot].phase = Phase::Failed;
        Ok(())
    }

    /// Every acquire owns a distinct lease, even when holder values are equal.
    /// Keep it until the actual consumer ends, not merely until the UI finishes.
    pub fn acquire(&mut self, ready: ReadyToken, holder: Holder) -> Result<LeaseToken, Error> {
        self.validate_ready(ready)?;
        let index = self.leases.iter().position(Option::is_none).ok_or(Error::LeaseCapacity)?;
        let serial = self.lease_serial.checked_add(1).ok_or(Error::CounterExhausted)?;
        let token = LeaseToken { index, serial, source: ready.0, holder };
        self.leases[index] = Some(token);
        self.lease_serial = serial;
        Ok(token)
    }

    pub fn validate_lease(&self, lease: LeaseToken, holder: Holder) -> bool {
        lease.holder == holder
            && self.leases.get(lease.index) == Some(&Some(lease))
            && self.validate_ready(ReadyToken(lease.source)).is_ok()
    }

    /// Idempotent: false means absent/stale and leaves every other lease intact.
    pub fn release(&mut self, lease: LeaseToken) -> bool {
        if !self.validate_lease(lease, lease.holder) { return false; }
        self.leases[lease.index] = None;
        true
    }

    /// Invalidates intent and leases immediately, but retains native occupancy.
    /// No native operation is performed. Counter exhaustion is mutation-free.
    pub fn depart_scene(&mut self) -> Result<SceneEpoch, Error> {
        let next = self.epoch.0.checked_add(1).ok_or(Error::CounterExhausted)?;
        self.epoch = SceneEpoch(next);
        self.leases = [None; LEASES];
        for state in &mut self.slots {
            if state.source.is_some() { state.phase = Phase::Departed; }
        }
        Ok(self.epoch)
    }

    /// Acknowledge actual native destruction/cleanup using the identity retained
    /// when that native lifetime began. Address-only destruction is insufficient.
    pub fn native_destroyed(&mut self, source: SourceToken) -> Result<usize, Error> {
        self.exact(source)?;
        let mut released = 0;
        for lease in &mut self.leases {
            if lease.map(|token| token.source == source).unwrap_or(false) {
                *lease = None;
                released += 1;
            }
        }
        let generation = self.slots[source.slot].generation;
        self.slots[source.slot] = Slot { generation, ..Slot::EMPTY };
        Ok(released)
    }

    fn slot(&self, slot: usize) -> Result<&Slot, Error> {
        self.slots.get(slot).ok_or(Error::InvalidSlot)
    }

    fn exact(&self, source: SourceToken) -> Result<&Slot, Error> {
        let state = self.slot(source.slot)?;
        if state.source != Some(source) { return Err(Error::StaleToken); }
        Ok(state)
    }

    fn validate_ready(&self, ready: ReadyToken) -> Result<(), Error> {
        let state = self.exact(ready.0)?;
        if !self.is_current(ready.0.epoch) { return Err(Error::StaleToken); }
        if state.phase != Phase::Ready { return Err(Error::NotReady); }
        Ok(())
    }

    fn pinned(&self, slot: usize) -> bool {
        self.leases.iter().flatten().any(|lease| lease.source.slot == slot)
    }

    fn admit(
        &mut self, slot: usize, epoch: SceneEpoch, entry: NativeEntry,
        key: SourceKey, phase: Phase,
    ) -> Result<SourceToken, Error> {
        if !self.is_current(epoch) { return Err(Error::StaleToken); }
        if self.slots.iter().enumerate().any(|(index, state)| {
            index != slot && state.source.map(|source| source.entry == entry).unwrap_or(false)
        }) { return Err(Error::EntryAlreadyRegistered); }
        let generation = self.slot(slot)?.generation.checked_add(1).ok_or(Error::CounterExhausted)?;
        let source = SourceToken { slot, entry, generation, epoch };
        self.slots[slot] = Slot { generation, source: Some(source), key: Some(key), phase };
        Ok(source)
    }
}

impl<const SOURCES: usize, const LEASES: usize> Default for SourceBook<SOURCES, LEASES> {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: SourceKey = SourceKey { kind: 7, costume: 2 };
    const B: SourceKey = SourceKey { kind: 9, costume: 0 };
    const COPY_P1: Holder = Holder { category: 1, id: 100 };
    const COPY_CPU: Holder = Holder { category: 1, id: 200 };
    const ITEM: Holder = Holder { category: 2, id: 300 };

    #[test]
    fn normal_load_ready_lease_release_reuse() {
        let mut book = SourceBook::<2, 4>::new();
        let load = book.begin_install(0, book.epoch(), NativeEntry(17), A).unwrap();
        assert_eq!(book.phase(0), Ok(Phase::Loading));
        assert_eq!(book.ready(0), Err(Error::NotReady));
        assert_eq!(book.can_replace(0), Ok(false));
        let ready = book.complete(load).unwrap();
        let lease = book.acquire(ready, COPY_P1).unwrap();
        assert!(book.validate_lease(lease, COPY_P1));
        assert!(book.release(lease));
        assert_eq!(book.can_replace(0), Ok(true));
        let next = book.begin_install(0, book.epoch(), NativeEntry(17), B).unwrap();
        assert_ne!(next.source(), ready.source());
        assert_eq!(book.acquire(ready, COPY_CPU), Err(Error::StaleToken));
        assert!(book.complete(next).is_ok());
        assert_eq!(book.key(0), Ok(Some(B)));
    }

    #[test]
    fn simultaneous_hat_and_item_pin_source_independently() {
        let mut book = SourceBook::<1, 4>::new();
        let ready = book.register_ready(0, book.epoch(), NativeEntry(17), A).unwrap();
        let hat = book.acquire(ready, COPY_P1).unwrap();
        let item = book.acquire(ready, ITEM).unwrap();
        assert!(!book.validate_lease(hat, ITEM));
        assert_eq!(book.begin_install(0, book.epoch(), NativeEntry(18), B), Err(Error::Pinned));
        assert!(book.release(hat));
        assert!(book.validate_lease(item, ITEM));
        assert_eq!(book.can_replace(0), Ok(false));
        assert_eq!(book.begin_install(0, book.epoch(), NativeEntry(17), B), Err(Error::Pinned));
        assert!(book.release(item));
        assert_eq!(book.can_replace(0), Ok(true));
    }

    #[test]
    fn full_source_and_lease_capacity_preserve_existing_dependencies() {
        let mut book = SourceBook::<2, 2>::new();
        let first = book.register_ready(0, book.epoch(), NativeEntry(17), A).unwrap();
        let second = book.register_ready(1, book.epoch(), NativeEntry(18), B).unwrap();
        let hat = book.acquire(first, COPY_P1).unwrap();
        let item = book.acquire(second, ITEM).unwrap();
        assert_eq!(book.acquire(first, COPY_CPU), Err(Error::LeaseCapacity));
        assert_eq!(book.begin_install(2, book.epoch(), NativeEntry(19), A), Err(Error::InvalidSlot));
        assert_eq!(book.begin_install(0, book.epoch(), NativeEntry(19), A), Err(Error::Pinned));
        assert_eq!(book.begin_install(1, book.epoch(), NativeEntry(19), A), Err(Error::Pinned));
        assert!(book.validate_lease(hat, COPY_P1));
        assert!(book.validate_lease(item, ITEM));
        assert_eq!(book.ready(0), Ok(first));
        assert_eq!(book.ready(1), Ok(second));
    }

    #[test]
    fn late_completion_after_same_address_reuse_cannot_commit() {
        let mut book = SourceBook::<1, 1>::new();
        let old = book.begin_install(0, book.epoch(), NativeEntry(17), A).unwrap();
        book.native_destroyed(old.source()).unwrap();
        let new = book.begin_install(0, book.epoch(), NativeEntry(17), A).unwrap();
        assert_ne!(old, new);
        assert_eq!(book.complete(old), Err(Error::StaleToken));
        assert_eq!(book.native_destroyed(old.source()), Err(Error::StaleToken));
        assert_eq!(book.phase(0), Ok(Phase::Loading));
        let ready = book.complete(new).unwrap();
        assert_eq!(book.complete(new), Err(Error::NotReady));
        assert_eq!(book.ready(0), Ok(ready));
    }

    #[test]
    fn stale_and_double_release_cannot_release_reused_lease_cell() {
        let mut book = SourceBook::<1, 2>::new();
        let ready = book.register_ready(0, book.epoch(), NativeEntry(17), A).unwrap();
        let old = book.acquire(ready, COPY_P1).unwrap();
        assert!(book.release(old));
        assert!(!book.release(old));
        let new = book.acquire(ready, COPY_P1).unwrap();
        let other = book.acquire(ready, COPY_P1).unwrap();
        assert_ne!(old, new);
        assert_ne!(new, other);
        assert!(!book.release(old));
        assert!(book.validate_lease(new, COPY_P1));
        book.native_destroyed(ready.source()).unwrap();
        assert!(!book.validate_lease(new, COPY_P1));
        assert!(!book.release(other));
        let next = book.register_ready(0, book.epoch(), NativeEntry(18), A).unwrap();
        let fresh = book.acquire(next, COPY_CPU).unwrap();
        assert!(!book.release(new));
        assert!(book.validate_lease(fresh, COPY_CPU));
    }

    #[test]
    fn departure_during_load_retains_occupancy_until_exact_cleanup() {
        let mut book = SourceBook::<2, 2>::new();
        let epoch = book.epoch();
        let load = book.begin_install(0, epoch, NativeEntry(17), A).unwrap();
        let ready = book.register_ready(1, epoch, NativeEntry(18), B).unwrap();
        let lease = book.acquire(ready, ITEM).unwrap();
        book.depart_scene().unwrap();
        assert!(!book.is_current(epoch));
        assert!(!book.validate_lease(lease, ITEM));
        assert!(!book.release(lease));
        for slot in 0..2 {
            assert_eq!(book.phase(slot), Ok(Phase::Departed));
            assert_eq!(book.can_replace(slot), Ok(false));
            assert_eq!(book.begin_install(slot, book.epoch(), NativeEntry(19), A), Err(Error::Occupied));
        }
        assert_eq!(book.complete(load), Err(Error::Departed));
        assert_eq!(book.phase(0), Ok(Phase::Departed));
        book.native_destroyed(load.source()).unwrap();
        assert_eq!(book.phase(0), Ok(Phase::Empty));
        assert_eq!(book.phase(1), Ok(Phase::Departed));
        assert_eq!(book.begin_install(0, epoch, NativeEntry(17), A), Err(Error::StaleToken));
        assert!(book.begin_install(0, book.epoch(), NativeEntry(17), A).is_ok());
    }

    #[test]
    fn two_scene_reconstruction_cycles_require_fresh_tokens() {
        let mut book = SourceBook::<1, 2>::new();
        let mut ready = book.register_ready(0, book.epoch(), NativeEntry(17), A).unwrap();
        for _ in 0..2 {
            let hat = book.acquire(ready, COPY_P1).unwrap();
            let item = book.acquire(ready, ITEM).unwrap();
            let epoch = book.epoch();
            book.depart_scene().unwrap();
            assert!(!book.is_current(epoch));
            assert_eq!(book.acquire(ready, COPY_CPU), Err(Error::StaleToken));
            book.native_destroyed(ready.source()).unwrap();
            let load = book.begin_install(0, book.epoch(), NativeEntry(17), A).unwrap();
            let next = book.complete(load).unwrap();
            assert_ne!(ready, next);
            assert!(!book.release(hat));
            assert!(!book.validate_lease(item, ITEM));
            ready = next;
        }
        assert!(book.acquire(ready, COPY_CPU).is_ok());
    }

    #[test]
    fn caller_cancellation_does_not_finish_native_install() {
        let mut book = SourceBook::<1, 1>::new();
        let load = book.begin_install(0, book.epoch(), NativeEntry(17), A).unwrap();
        // UI drops its intent; native callback still owns the load token.
        assert_eq!(book.begin_install(0, book.epoch(), NativeEntry(17), B), Err(Error::Occupied));
        assert_eq!(book.phase(0), Ok(Phase::Loading));
        let ready = book.complete(load).unwrap();
        assert_eq!(book.can_replace(0), Ok(true));
        assert!(book.acquire(ready, ITEM).is_ok());
    }

    #[test]
    fn duplicate_native_identity_and_stale_destruction_are_rejected() {
        let mut book = SourceBook::<2, 1>::new();
        let old = book.register_ready(0, book.epoch(), NativeEntry(17), A).unwrap();
        assert_eq!(book.register_ready(1, book.epoch(), NativeEntry(17), B), Err(Error::EntryAlreadyRegistered));
        assert_eq!(book.phase(1), Ok(Phase::Empty));
        assert_eq!(book.begin_install(0, book.epoch(), NativeEntry(18), B), Err(Error::Occupied));
        assert_eq!(book.ready(0), Ok(old));
        let next = book.begin_install(0, book.epoch(), NativeEntry(17), B).unwrap();
        assert_eq!(book.native_destroyed(old.source()), Err(Error::StaleToken));
        assert_eq!(book.phase(0), Ok(Phase::Loading));
        assert!(book.complete(next).is_ok());
    }

    #[test]
    fn counter_exhaustion_and_zero_capacity_fail_without_mutation() {
        let mut empty = SourceBook::<0, 0>::new();
        assert_eq!(empty.begin_install(0, empty.epoch(), NativeEntry(17), A), Err(Error::InvalidSlot));
        let mut book = SourceBook::<1, 1>::new();
        book.slots[0].generation = u64::MAX;
        assert_eq!(book.begin_install(0, book.epoch(), NativeEntry(17), A), Err(Error::CounterExhausted));
        assert_eq!(book.phase(0), Ok(Phase::Empty));
        book.slots[0].generation = 0;
        let ready = book.register_ready(0, book.epoch(), NativeEntry(17), A).unwrap();
        book.lease_serial = u64::MAX;
        assert_eq!(book.acquire(ready, ITEM), Err(Error::CounterExhausted));
        book.lease_serial = 0;
        book.epoch = SceneEpoch(u64::MAX);
        let before = book.slots[0];
        assert_eq!(book.depart_scene(), Err(Error::CounterExhausted));
        assert_eq!(book.slots[0], before);
    }

    #[test]
    fn completed_unusable_load_can_retry_only_on_same_native_entry() {
        let mut book = SourceBook::<1, 1>::new();
        let load = book.begin_install(0, book.epoch(), NativeEntry(17), A).unwrap();
        book.fail(load).unwrap();
        assert_eq!(book.phase(0), Ok(Phase::Failed));
        assert_eq!(book.ready(0), Err(Error::NotReady));
        assert_eq!(book.complete(load), Err(Error::NotReady));
        assert_eq!(book.can_replace(0), Ok(true));
        assert_eq!(book.begin_install(0, book.epoch(), NativeEntry(18), B), Err(Error::Occupied));
        let retry = book.begin_install(0, book.epoch(), NativeEntry(17), B).unwrap();
        assert_eq!(book.fail(load), Err(Error::StaleToken));
        let ready = book.complete(retry).unwrap();
        book.acquire(ready, ITEM).unwrap();
        assert_eq!(book.native_destroyed(ready.source()), Ok(1));
    }
}
