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

/// The cursor follows a base-stage identity even when its selected form changes.
pub trait CycleStage: Copy {
    fn identity(self) -> u32;
}

impl CycleStage for u32 {
    fn identity(self) -> u32 { self }
}

impl CycleStage for training_mod_consts::lobby_stages::Selection {
    fn identity(self) -> u32 { self.stage }
}

pub struct StageCycle<S = u32> {
    last: Option<u32>,
    pending: Option<(S, bool)>,
}

impl<S: CycleStage> Default for StageCycle<S> {
    fn default() -> Self { Self::new() }
}

impl<S: CycleStage> StageCycle<S> {
    pub const fn new() -> Self {
        Self {
            last: None,
            pending: None,
        }
    }

    /// Repeated preload requests cannot advance the cycle. Menu edits apply to
    /// the next entry whose resources have not yet been requested.
    pub fn prepare(&mut self, stages: &[S]) -> Option<S> {
        if self.pending.is_none() && !stages.is_empty() {
            let next = self
                .last
                .and_then(|last| stages.iter().position(|stage| stage.identity() == last))
                .map_or(0, |index| (index + 1) % stages.len());
            self.pending = Some((stages[next], true));
        }
        self.pending.map(|(stage, _)| stage)
    }

    pub fn prepare_direct(&mut self, stage: S) -> Option<S> {
        self.pending.get_or_insert((stage, false));
        self.pending.map(|(stage, _)| stage)
    }

    /// The reserved selection, without consuming or advancing anything.
    pub fn pending(&self) -> Option<S> {
        self.pending.map(|(stage, _)| stage)
    }

    /// Only a qualified native practice-entry setup consumes the prepared stage.
    /// A normal match, menu draw, or fighter reload must never call this operation.
    pub fn enter(&mut self) -> Option<S> {
        let (stage, advance) = self.pending.take()?;
        if advance { self.last = Some(stage.identity()); }
        Some(stage)
    }

    /// A later native preload supersedes an abandoned request. Forget only that
    /// reservation; no waiting entry happened, so the rotation must not advance.
    pub fn cancel_preload(&mut self) { self.pending = None; }
}

impl StageCycle<training_mod_consts::lobby_stages::Selection> {
    /// Used by the native waiting adapter. Validate a new choice before reserving
    /// it; later menu edits cannot replace a choice whose preload is already owned.
    pub fn prepare_preferences(
        &mut self,
        enabled: bool,
        order: training_mod_consts::lobby_stages::LobbyStageOrder,
        prefs: &training_mod_tui::stage_menu::Preferences,
        direct_enabled: bool,
    ) -> Result<Option<training_mod_consts::lobby_stages::Selection>, &'static str> {
        if !enabled {
            // A new qualified native preload with practice disabled supersedes
            // an abandoned reservation. No entry occurred; do not advance.
            self.cancel_preload();
            return Ok(None);
        }
        if let Some((selection, _)) = self.pending { return Ok(Some(selection)); }
        training_mod_consts::lobby_stages::validate_preferences(prefs)?;
        if direct_enabled && prefs.mode == training_mod_tui::stage_menu::Mode::Direct {
            Ok(self.prepare_direct(training_mod_consts::lobby_stages::Selection {
                stage: prefs.direct_stage, form: prefs.direct_form,
            }))
        } else {
            Ok(self.prepare(&order.selections(&prefs.rotation_forms)?))
        }
    }

    /// Resolve before consuming/advancing. Both native resource and scene paths
    /// use the same catalog resolver, never the base identity as a native kind.
    pub fn enter_resolved(&mut self) -> Result<Option<(training_mod_consts::lobby_stages::Selection, u32)>, &'static str> {
        let Some((selection, _)) = self.pending else { return Ok(None); };
        let native = training_mod_consts::lobby_stages::native_stage(selection)?;
        let _ = self.enter();
        Ok(Some((selection, native)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use training_mod_consts::lobby_stages::{native_stage, Form, Selection};

    #[test]
    fn production_preference_admission_preserves_owned_form_and_rejects_new_invalid_choices() {
        use training_mod_consts::lobby_stages::{Forms, LobbyStageOrder, STAGES};
        use training_mod_tui::stage_menu::{Mode, Preferences};
        let order: LobbyStageOrder = serde_json::from_str("[0,0,1,2]").unwrap();
        let mut prefs = Preferences::default();
        let bf = STAGES.iter().position(|(stage, _)| *stage == 0).unwrap();
        let training = STAGES.iter().position(|(stage, _)| *stage == 308).unwrap();
        prefs.rotation_forms.set(bf, Form::Omega).unwrap();
        let mut cycle = StageCycle::<Selection>::new();
        let omega = Selection { stage: 0, form: Form::Omega };
        assert_eq!(cycle.prepare_preferences(true, order, &prefs, true), Ok(Some(omega)));
        // Invalid unranked metadata after resource preparation cannot discard it.
        prefs.rotation_forms.set(training, Form::Battlefield).unwrap();
        assert_eq!(cycle.prepare_preferences(true, order, &prefs, true), Ok(Some(omega)));
        assert_eq!(cycle.enter_resolved(), Ok(Some((omega, 1))));
        assert_eq!(cycle.enter_resolved(), Ok(None));
        assert!(cycle.prepare_preferences(true, order, &prefs, true).is_err());
        assert_eq!(cycle.enter_resolved(), Ok(None));
        prefs.rotation_forms = Forms::NORMAL;
        prefs.mode = Mode::Direct;
        prefs.direct_stage = 95;
        prefs.direct_form = Form::Battlefield;
        let direct = Selection { stage: 95, form: Form::Battlefield };
        assert_eq!(cycle.prepare_preferences(true, order, &prefs, true), Ok(Some(direct)));
        assert_eq!(cycle.enter_resolved(), Ok(Some((direct, 97))));
        // Feature-off Direct cannot pin the waiting entry or move its cursor.
        assert_eq!(cycle.prepare_preferences(true, order, &prefs, false), Ok(Some(Selection::normal(3))));
        assert_eq!(cycle.prepare_preferences(false, order, &prefs, false), Ok(None));
        assert_eq!(cycle.enter_resolved(), Ok(None));
        assert_eq!(cycle.prepare_preferences(true, order, &prefs, false), Ok(Some(Selection::normal(3))));
    }

    #[test]
    fn form_reservation_survives_preload_retries_and_return_without_moving_ranks() {
        let omega = Selection { stage: 95, form: Form::Omega };
        let fd = Selection::normal(3);
        let mut cycle = StageCycle::new();
        let order = [omega, fd];
        for expected in [omega, fd, omega] {
            assert_eq!(cycle.prepare(&order), Some(expected));
            assert_eq!(cycle.prepare(&order), Some(expected));
            // A menu form edit cannot mismatch an already requested resource.
            assert_eq!(cycle.prepare(&[Selection::normal(95)]), Some(expected));
            let loaded = cycle.enter().unwrap();
            assert_eq!(loaded, expected);
            assert_eq!(native_stage(loaded), native_stage(expected));
            assert_eq!(cycle.enter(), None);
        }
        // Editing the last stage's form does not turn it into a removed identity.
        assert_eq!(cycle.prepare(&[Selection::normal(95), fd]), Some(fd));
        cycle.cancel_preload();
        assert_eq!(cycle.enter(), None);
        assert_eq!(cycle.prepare(&order), Some(fd));
    }

    #[test]
    fn independent_form_cursors_do_not_advance_on_fixed_reentry() {
        let mut cycles = [StageCycle::new(), StageCycle::new(), StageCycle::new()];
        let omega = Selection { stage: 0, form: Form::Omega };
        let order = [Selection::normal(308), omega];
        for cycle in &mut cycles {
            assert_eq!(cycle.prepare(&order), Some(order[0]));
            let _ = cycle.enter();
        }
        for _ in 0..3 {
            assert_eq!(cycles[0].prepare_direct(omega), Some(omega));
            assert_eq!(cycles[0].enter(), Some(omega));
        }
        for cycle in &mut cycles {
            assert_eq!(cycle.prepare(&order), Some(omega));
            assert_eq!(cycle.enter(), Some(omega));
        }
    }

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
        let mut cycle = StageCycle::<u32>::new();
        for expected in [0, 3, 0, 3] {
            assert_eq!(cycle.prepare(&[0, 3]), Some(expected));
            assert_eq!(cycle.prepare(&[0, 3]), Some(expected));
            assert_eq!(cycle.enter(), Some(expected));
            assert_eq!(cycle.enter(), None);
        }
    }

    #[test]
    fn menu_edits_preserve_the_stage_already_preloaded() {
        let mut cycle = StageCycle::<u32>::new();
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
        let mut cycle = StageCycle::<u32>::new();
        cycle.prepare(&[0, 3, 12]);
        assert_eq!(cycle.enter(), Some(0));
        assert_eq!(cycle.prepare(&[12, 0, 3]), Some(3));
        cycle.enter();
        assert_eq!(cycle.prepare(&[12, 0]), Some(12));
    }

    #[test]
    fn native_preload_after_cancelled_search_does_not_consume_a_stale_stage() {
        let mut cycle = StageCycle::<u32>::new();
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
        let mut cycle = StageCycle::<u32>::new();
        cycle.prepare(&[0, 3]);
        assert_eq!(cycle.enter(), Some(0));
        // A match return has no driver preload; preparing at the constructor
        // continues the same rotation rather than resetting it or skipping.
        assert_eq!(cycle.prepare(&[0, 3]), Some(3));
        assert_eq!(cycle.enter(), Some(3));
    }

    #[test]
    fn direct_entries_preserve_rotation_cursor_and_the_owned_preload() {
        let mut cycle = StageCycle::<u32>::new();
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
