//! Bounded change capture. Allocation happens once, before native hooks start.
//! Tick/age are metadata: a ticking clock alone must not fill the change buffer.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "automation-results-observer", derive(serde::Serialize))]
pub enum Stop {
    NativeExit,
    SnapshotLimit,
    TickLimit,
    BindingChanged,
    Contention,
    InvalidBinding,
}

#[cfg_attr(feature = "automation-results-observer", derive(serde::Serialize))]
pub struct Frame<T> {
    pub tick: u32,
    pub native_age: u32,
    pub last_tick: u32,
    pub last_native_age: u32,
    pub value: T,
}

pub struct Recording<T> {
    frames: Vec<Frame<T>>,
    limit: usize,
    ticks: u32,
    tick_limit: u32,
    stop: Option<Stop>,
}

impl<T: PartialEq> Recording<T> {
    pub fn new(limit: usize, tick_limit: u32) -> Self {
        assert!(limit > 0 && tick_limit > 0);
        Self {
            frames: Vec::with_capacity(limit),
            limit,
            ticks: 0,
            tick_limit,
            stop: None,
        }
    }

    /// No growth, blocking, or allocation. False means the caller must close capture.
    pub fn observe(&mut self, value: T, native_age: u32) -> bool {
        if self.stop.is_some() {
            return false;
        }
        self.ticks += 1;
        if self.frames.last().is_none_or(|last| last.value != value) {
            if self.frames.len() == self.limit {
                self.finish(Stop::SnapshotLimit);
                return false;
            }
            self.frames.push(Frame {
                tick: self.ticks,
                native_age,
                last_tick: self.ticks,
                last_native_age: native_age,
                value,
            });
        } else if let Some(last) = self.frames.last_mut() {
            last.last_tick = self.ticks;
            last.last_native_age = native_age;
        }
        if self.ticks >= self.tick_limit {
            self.finish(Stop::TickLimit);
            return false;
        }
        true
    }

    pub fn finish(&mut self, reason: Stop) {
        if self.stop.is_none() {
            self.stop = Some(reason);
        }
    }

    pub fn frames(&self) -> &[Frame<T>] {
        &self.frames
    }
    pub fn ticks(&self) -> u32 {
        self.ticks
    }
    pub fn stop(&self) -> Option<Stop> {
        self.stop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clocks_do_not_consume_slots_and_change_has_the_original_tick() {
        let mut r = Recording::new(2, 20);
        let capacity = r.frames.capacity();
        for age in 0..10 {
            assert!(r.observe(7u32, age));
        }
        assert!(r.observe(8, 0));
        assert_eq!(r.frames().len(), 2);
        assert_eq!(r.frames()[1].tick, 11);
        assert_eq!(r.frames()[1].native_age, 0);
        assert_eq!(r.frames()[0].last_tick, 10);
        assert_eq!(r.frames()[0].last_native_age, 9);
        assert_eq!(r.frames.capacity(), capacity);
        r.finish(Stop::NativeExit);
        assert_eq!(r.stop(), Some(Stop::NativeExit));
    }

    #[test]
    fn overflow_is_explicit_and_cannot_be_relabelled_as_a_complete_capture() {
        let mut r = Recording::new(1, 20);
        assert!(r.observe(1u32, 0));
        assert!(!r.observe(2, 1));
        r.finish(Stop::NativeExit);
        assert_eq!(r.stop(), Some(Stop::SnapshotLimit));
        assert_eq!(r.frames().len(), 1);
        assert!(!r.observe(3, 2));
        assert_eq!(r.ticks(), 2);
    }

    #[test]
    fn unchanging_capture_ends_at_tick_budget() {
        let mut r = Recording::new(3, 2);
        assert!(r.observe(1u32, 0));
        assert!(!r.observe(1, 1));
        assert_eq!(r.stop(), Some(Stop::TickLimit));
        assert_eq!(r.frames().len(), 1);
        for reason in [Stop::BindingChanged, Stop::Contention, Stop::InvalidBinding] {
            let mut r = Recording::<u8>::new(1, 1);
            r.finish(reason);
            assert!(!r.observe(0, 0));
            assert_eq!(r.stop(), Some(reason));
        }
    }
}
