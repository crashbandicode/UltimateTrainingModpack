//! Entry-time stage choice. Native preload and scene setup consume one selection.

#[derive(Default)]
pub struct StageCycle {
    last: Option<u32>,
    pending: Option<(u32, bool)>,
}

impl StageCycle {
    pub const fn new() -> Self {
        Self {
            last: None,
            pending: None,
        }
    }

    /// Repeated preload requests cannot advance the cycle. Menu edits apply to
    /// the next entry whose resources have not yet been requested.
    pub fn prepare(&mut self, stages: &[u32]) -> Option<u32> {
        if self.pending.is_none() && !stages.is_empty() {
            let next = self
                .last
                .and_then(|last| stages.iter().position(|stage| *stage == last))
                .map_or(0, |index| (index + 1) % stages.len());
            self.pending = Some((stages[next], true));
        }
        self.pending.map(|(stage, _)| stage)
    }

    #[cfg(feature = "practice-stage-request-alpha")]
    pub fn prepare_direct(&mut self, stage: u32) -> Option<u32> {
        self.pending.get_or_insert((stage, false));
        self.pending.map(|(stage, _)| stage)
    }

    /// Only native waiting-room setup consumes the prepared stage. A normal
    /// match, menu draw, or fighter reload must never call this operation.
    pub fn enter(&mut self) -> Option<u32> {
        let (stage, advance) = self.pending.take()?;
        if advance { self.last = Some(stage); }
        Some(stage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preload_retries_and_repeated_setup_do_not_skip_entries() {
        let mut cycle = StageCycle::new();
        for expected in [0, 3, 0, 3] {
            assert_eq!(cycle.prepare(&[0, 3]), Some(expected));
            assert_eq!(cycle.prepare(&[0, 3]), Some(expected));
            assert_eq!(cycle.enter(), Some(expected));
            assert_eq!(cycle.enter(), None);
        }
    }

    #[test]
    fn menu_edits_preserve_the_stage_already_preloaded() {
        let mut cycle = StageCycle::new();
        assert_eq!(cycle.prepare(&[0, 3]), Some(0));
        assert_eq!(cycle.prepare(&[3]), Some(0));
        assert_eq!(cycle.prepare(&[]), Some(0));
        assert_eq!(cycle.enter(), Some(0));
        assert_eq!(cycle.prepare(&[3]), Some(3));
        assert_eq!(cycle.enter(), Some(3));
        assert_eq!(cycle.prepare(&[]), None);
        assert_eq!(cycle.enter(), None);
    }

    #[test]
    fn reordered_list_continues_after_last_stage_and_removed_stage_restarts() {
        let mut cycle = StageCycle::new();
        cycle.prepare(&[0, 3, 12]);
        assert_eq!(cycle.enter(), Some(0));
        assert_eq!(cycle.prepare(&[12, 0, 3]), Some(3));
        cycle.enter();
        assert_eq!(cycle.prepare(&[12, 0]), Some(12));
    }

    #[cfg(feature = "practice-stage-request-alpha")]
    #[test]
    fn direct_entries_preserve_rotation_cursor_and_the_owned_preload() {
        let mut cycle = StageCycle::new();
        assert_eq!(cycle.prepare(&[0, 3, 12]), Some(0));
        // Switching mode cannot replace an already native-preloaded request.
        assert_eq!(cycle.prepare_direct(44), Some(0));
        assert_eq!(cycle.enter(), Some(0));
        assert_eq!(cycle.prepare_direct(44), Some(44));
        assert_eq!(cycle.prepare_direct(3), Some(44));
        assert_eq!(cycle.enter(), Some(44));
        assert_eq!(cycle.prepare_direct(44), Some(44));
        assert_eq!(cycle.enter(), Some(44));
        // Fixed entries never advance the independent rotation cursor.
        assert_eq!(cycle.prepare(&[0, 3, 12]), Some(3));
        assert_eq!(cycle.enter(), Some(3));
    }

}
