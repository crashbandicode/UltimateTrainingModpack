//! Pure bounds/sequence policy. No native pointers or game writes.
pub const RECORDS: usize = 133;
pub const STRIDE: usize = 0x68;
pub const CHUNK_ROWS: usize = 8;
pub const CHUNKS: usize = RECORDS.div_ceil(CHUNK_ROWS);
pub const MAX_SNAPSHOTS: u8 = 3;

pub fn vector_capacity(begin: usize, end: usize, capacity: usize) -> Option<usize> {
    if begin == 0 || !begin.is_multiple_of(8) || end < begin || capacity < end {
        return None;
    }
    let used = end.checked_sub(begin)?;
    let allocated = capacity.checked_sub(begin)?;
    (used == RECORDS * STRIDE && allocated.is_multiple_of(STRIDE))
        .then_some(allocated / STRIDE)
}

/// A request is consumed once, even if the snapshot fails. A refusal is terminal.
/// Stage 2's reset is attributed by the independent native control trace, not this policy.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sequence {
    pub completed: u8,
    pub pending: bool,
    pub ended: bool,
    pub first_epoch: Option<usize>,
    pub request_epoch: usize,
    pub visits: u16,
}

impl Sequence {
    pub const fn new() -> Self {
        Self { completed: 0, pending: false, ended: false, first_epoch: None,
            request_epoch: 0, visits: 0 }
    }

    pub fn request(&mut self, epoch: usize) -> Result<u8, &'static str> {
        if self.ended || self.pending || self.completed >= MAX_SNAPSHOTS {
            return Err("terminal/pending/three-snapshot limit");
        }
        if (self.completed == 1 && self.first_epoch != Some(epoch))
            || (self.completed == 2 && self.first_epoch == Some(epoch))
        {
            self.ended = true;
            return Err("expected same epoch for post-reset, new epoch for reentry");
        }
        self.pending = true;
        self.request_epoch = epoch;
        self.visits = 0;
        Ok(self.completed + 1)
    }

    pub fn finish(&mut self, epoch: usize, success: bool) -> bool {
        if !self.pending || self.request_epoch != epoch || !success {
            self.ended = true;
            self.pending = false;
            return false;
        }
        self.pending = false;
        self.first_epoch.get_or_insert(epoch);
        self.completed += 1;
        true
    }

    pub fn cancel_pending(&mut self) -> bool {
        if self.pending {
            self.pending = false;
            self.ended = true;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_boundaries_need_same_then_new_epoch_and_never_fourth() {
        let mut s = Sequence::new();
        assert_eq!(s.request(10), Ok(1));
        assert!(s.finish(10, true));
        assert_eq!(s.request(10), Ok(2));
        assert!(s.finish(10, true));
        assert_eq!(s.request(14), Ok(3));
        assert!(s.finish(14, true));
        assert!(s.request(14).is_err());
        assert_eq!(s.completed, 3);
    }

    #[test]
    fn failed_or_departed_request_never_retries_in_later_scene() {
        for departure in [false, true] {
            let mut s = Sequence::new();
            assert_eq!(s.request(2), Ok(1));
            if departure { assert!(s.cancel_pending()); }
            else { assert!(!s.finish(2, false)); }
            assert!(s.request(3).is_err());
            assert_eq!(s.completed, 0);
        }
    }

    #[test]
    fn duplicate_chord_cannot_create_second_pending_snapshot() {
        let mut s = Sequence::new();
        assert_eq!(s.request(1), Ok(1));
        assert!(s.request(1).is_err());
        assert!(s.finish(1, true));
        assert_eq!(s.completed, 1);
    }

    #[test]
    fn premature_reentry_or_missing_reentry_refuses() {
        let mut s = Sequence::new();
        s.request(1).unwrap(); s.finish(1, true);
        assert!(s.request(2).is_err());
        let mut s = Sequence::new();
        s.request(1).unwrap(); s.finish(1, true);
        s.request(1).unwrap(); s.finish(1, true);
        assert!(s.request(1).is_err());
    }

    #[test]
    fn count_stride_and_capacity_are_checked_before_any_copy() {
        let b = 0x1000;
        let e = b + 133 * 104;
        assert_eq!(vector_capacity(b, e, e), Some(133));
        assert_eq!(vector_capacity(b, e, e + 104), Some(134));
        for (start, end, cap) in [(0,e,e),(b+1,e,e),(b,e-104,e),
            (b,e+104,e+104),(b,e,e-1),(b,e,e+1),(usize::MAX-7,0,0)] {
            assert_eq!(vector_capacity(start,end,cap), None);
        }
    }
}
