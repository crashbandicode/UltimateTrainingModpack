//! Entry-time stage choice. Native preload and scene setup consume one selection.

/// The two qualified waiting families. This is scene admission, not permission
/// to overwrite a native resource slot; the live stage policy checks ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WaitingStageFamily {
    LocalWireless,
    Quickplay,
}

pub(super) fn family(mode: u32) -> Option<WaitingStageFamily> {
    if mode == 0x02010000 {
        Some(WaitingStageFamily::LocalWireless)
    } else if mode & 0x7fffff00 == 0x07010100 {
        Some(WaitingStageFamily::Quickplay)
    } else {
        None
    }
}

pub(super) fn waiting(mode: u32, melee: u8) -> bool {
    melee == 0x0d && family(mode).is_some()
}

/// Return addresses from fingerprinted native slot-2 PracticeScene preloads
/// (including Quickplay's CSS and return drivers). Match/Morph requests must not
/// reserve or advance the entry rotation.
pub(super) fn entry_preload(mode: u32, caller: usize) -> bool {
    matches!((family(mode), caller),
        (Some(WaitingStageFamily::LocalWireless), 0x2335cf0)
            | (Some(WaitingStageFamily::Quickplay), 0x23684b0 | 0x2369208 | 0x236ca48 | 0x237cce0))
}

/// A retained Quickplay waiting owner can recreate PracticeScene without the
/// CSS driver's preload. Its constructor runs before the generic loader, so
/// an empty practice resource slot can still take an entry-time selection.
pub(super) fn unprepared_quickplay_entry(mode: u32, melee: u8, native_stage: u32, empty: bool) -> bool {
    family(mode) == Some(WaitingStageFamily::Quickplay)
        && waiting(mode, melee) && native_stage == 311 && empty
}

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

    /// A later native preload supersedes an abandoned request. Forget only that
    /// reservation; no waiting entry happened, so the rotation must not advance.
    pub fn cancel_preload(&mut self) { self.pending = None; }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualified_preload_callers_do_not_admit_match_or_morph_requests() {
        assert!(entry_preload(0x02010000, 0x2335cf0));
        assert!(entry_preload(0x07010100, 0x237cce0));
        assert!(entry_preload(0x87010101, 0x237cce0));
        for caller in [0x23684b0, 0x2369208, 0x236ca48] {
            assert!(entry_preload(0x07010101, caller));
            assert!(!entry_preload(0x02010000, caller));
        }
        assert!(!entry_preload(0x07010100, 0x2335cf0));
        assert!(!entry_preload(0x02010000, 0x237cce0));
        for caller in [0x26074f4, 0x14f57c4, 0x23684ac, 0] {
            assert!(!entry_preload(0x07010100, caller));
        }
        // Same outer Quickplay family can own actual gameplay. Melee decides
        // whether the live stage controller is local waiting practice.
        assert!(waiting(0x07010100, 0x0d));
        assert!(!waiting(0x07010100, 1));
        assert!(!waiting(0x07010200, 0x0d));
        assert!(!waiting(0x04040000, 0x0d));
    }

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

    #[test]
    fn native_preload_after_cancelled_search_does_not_consume_a_stale_stage() {
        let mut cycle = StageCycle::new();
        cycle.prepare(&[0, 3]);
        assert_eq!(cycle.enter(), Some(0));
        assert_eq!(cycle.prepare(&[0, 3]), Some(3));
        // Search is abandoned; the next qualified preload uses native stage311
        // because practice/rotation was disabled. Its entry has no override.
        cycle.cancel_preload();
        assert_eq!(cycle.enter(), None);
        assert_eq!(cycle.prepare(&[0, 3]), Some(3));
        assert_eq!(cycle.enter(), Some(3));
    }

    #[test]
    fn retained_waiting_entry_requires_native_stage_and_an_unowned_resource_slot() {
        assert!(unprepared_quickplay_entry(0x07010101, 0x0d, 311, true));
        assert!(unprepared_quickplay_entry(0x87010101, 0x0d, 311, true));
        assert!(!unprepared_quickplay_entry(0x07010101, 0x0e, 311, true));
        assert!(!unprepared_quickplay_entry(0x02010000, 0x0d, 311, true));
        assert!(!unprepared_quickplay_entry(0x07010101, 0x0d, 0, true));
        assert!(!unprepared_quickplay_entry(0x07010101, 0x0d, 311, false));
        let mut cycle = StageCycle::new();
        cycle.prepare(&[0, 3]);
        assert_eq!(cycle.enter(), Some(0));
        // A match return has no driver preload; preparing at the constructor
        // continues the same rotation rather than resetting it or skipping.
        assert_eq!(cycle.prepare(&[0, 3]), Some(3));
        assert_eq!(cycle.enter(), Some(3));
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
