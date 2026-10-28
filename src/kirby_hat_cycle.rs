//! Value-only Kirby shortcut state. Native loading commits only after acceptance.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HatCycle {
    last: Option<u32>,
}

impl HatCycle {
    pub const fn new() -> Self {
        Self { last: None }
    }

    pub fn target(&self, choices: &[u32], equipped: Option<u32>) -> Option<u32> {
        let first = *choices.first()?;
        let Some(index) = self
            .last
            .and_then(|last| choices.iter().position(|k| *k == last))
        else {
            return Some(first);
        };
        if equipped == self.last {
            Some(choices[(index + 1) % choices.len()])
        } else {
            // Losing the hat, including a KO, restores it without advancing.
            self.last
        }
    }

    pub fn commit(&mut self, kind: u32) {
        self.last = Some(kind);
    }
}

/// An equipped hat is distinct from the shortcut cursor: losing a hat must stay
/// lost, while the next explicit shortcut can still reapply the last selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HatSnapshot {
    pub cycle: HatCycle,
    pub equipped: Option<u32>,
}

pub struct HatContinuation {
    choice: crate::character_memory::WaitingChoice<HatSnapshot>,
}

impl Default for HatContinuation {
    fn default() -> Self { Self::new() }
}

impl HatContinuation {
    pub const fn new() -> Self {
        Self { choice: crate::character_memory::WaitingChoice::empty() }
    }

    pub fn scene_changed(&mut self, same_lobby: bool) {
        if same_lobby { self.choice.scene_changed(); } else { self.clear(); }
    }

    pub fn clear(&mut self) { self.choice.clear(); }

    pub fn pending(&self) -> bool { self.choice.restore_target().is_some() }

    /// Called only on a stable live Kirby, never on a transitional CSS body.
    pub fn observe(&mut self, snapshot: HatSnapshot) {
        if !self.pending() { self.choice.commit(snapshot); }
    }

    /// One attempt per return, consumed only after the new body and UI are ready.
    pub fn take_restore(&mut self) -> Option<HatSnapshot> {
        let snapshot = self.choice.restore_target()?;
        self.choice.manual_request();
        Some(snapshot)
    }
}

/// A claimed shortcut owns Special until its release. A held chord never repeats.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShortcutLatch {
    held: bool,
    owned_special: bool,
}

impl ShortcutLatch {
    pub const fn new() -> Self {
        Self {
            held: false,
            owned_special: false,
        }
    }

    /// Lets the input hook avoid game-object lookups on ordinary input frames.
    pub fn would_request(&self, special: bool, neutral: bool, left: bool, right: bool) -> bool {
        special && neutral && (left != right) && !self.held
    }

    /// Returns (new role request, suppress Special). Role 0 is P1, 1 is CPU.
    pub fn sample(
        &mut self,
        special: bool,
        neutral: bool,
        left_click: bool,
        right_click: bool,
        available: [bool; 2],
    ) -> (Option<usize>, bool) {
        if !special {
            self.owned_special = false;
        }
        let chord = special && (left_click || right_click);
        let edge = chord && !self.held;
        self.held = chord;
        let role = match (left_click, right_click) {
            (true, false) => Some(0),
            (false, true) => Some(1),
            _ => None,
        };
        let request = role.filter(|role| edge && neutral && available[*role]);
        if request.is_some() {
            self.owned_special = true;
        }
        (request, self.owned_special)
    }
}

/// (left, right) clicks for [`ShortcutLatch::sample`] from the controller operating the
/// CPU under native Behavior=Control. Its CPU hat chord is P1's (Special plus right stick
/// click); Special plus left stick click, its own fighter, is an alias. Both clicks hold
/// the chord without a request, as for P1. It never requests P1's hat.
pub const fn operator_clicks(left_click: bool, right_click: bool) -> (bool, bool) {
    (left_click && right_click, left_click || right_click)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lobby_return_restores_equipped_hat_once_for_each_role() {
        let mut roles = [HatContinuation::new(), HatContinuation::new()];
        for (role, kind) in [0x48, 0x02].iter().copied().enumerate() {
            let mut cycle = HatCycle::new();
            cycle.commit(kind);
            roles[role].observe(HatSnapshot { cycle, equipped: Some(kind) });
            roles[role].scene_changed(true); // Match departure.
            roles[role].scene_changed(true); // Waiting practice returns.
            roles[role].observe(HatSnapshot::default()); // Transitional native body.
            assert_eq!(roles[role].take_restore().unwrap().equipped, Some(kind));
            assert!(roles[role].take_restore().is_none());
        }
    }

    #[test]
    fn lost_hat_stays_lost_but_shortcut_cursor_survives_return() {
        let mut memory = HatContinuation::new();
        let mut cycle = HatCycle::new();
        cycle.commit(0x48);
        memory.observe(HatSnapshot { cycle, equipped: Some(0x48) });
        memory.observe(HatSnapshot { cycle, equipped: None });
        assert!(!memory.pending()); // Ordinary loss never queues a restore.
        memory.scene_changed(true);
        let snapshot = memory.take_restore().unwrap();
        assert_eq!(snapshot.equipped, None);
        assert_eq!(snapshot.cycle.target(&[0x02, 0x48], None), Some(0x48));
    }

    #[test]
    fn fresh_session_or_non_kirby_choice_forgets_hat() {
        for fresh_scene in [false, true] {
            let mut memory = HatContinuation::new();
            memory.observe(HatSnapshot { cycle: HatCycle::new(), equipped: Some(0x48) });
            if fresh_scene { memory.scene_changed(false); } else { memory.clear(); }
            memory.scene_changed(true);
            assert!(memory.take_restore().is_none());
        }
    }

    #[test]
    fn starts_advances_restores_and_wraps() {
        let mut cycle = HatCycle::default();
        let choices = [0, 3, 0x53];
        assert_eq!(cycle.target(&choices, Some(3)), Some(0));
        cycle.commit(0);
        assert_eq!(cycle.target(&choices, Some(0)), Some(3));
        cycle.commit(3);
        assert_eq!(cycle.target(&choices, None), Some(3));
        cycle.commit(3);
        assert_eq!(cycle.target(&choices, Some(3)), Some(0x53));
        cycle.commit(0x53);
        assert_eq!(cycle.target(&choices, Some(0x53)), Some(0));
    }

    #[test]
    fn unaccepted_load_does_not_advance_and_roles_are_independent() {
        let mut roles = [HatCycle::default(); 2];
        roles[0].commit(0);
        assert_eq!(roles[0].target(&[0, 3], Some(0)), Some(3));
        // No native acceptance, therefore no commit.
        assert_eq!(roles[0].target(&[0, 3], Some(0)), Some(3));
        assert_eq!(roles[1].target(&[3, 0], None), Some(3));
        assert_eq!(roles[0].target(&[0, 3], None), Some(0));
    }

    #[test]
    fn edited_list_uses_current_order_without_equipping_anything() {
        let mut cycle = HatCycle::default();
        cycle.commit(3);
        assert_eq!(cycle.target(&[3, 0, 2], Some(3)), Some(0));
        assert_eq!(cycle.target(&[0, 2], Some(3)), Some(0));
        assert_eq!(cycle.target(&[], None), None);
        assert_eq!(cycle.target(&[3], None), Some(3));
        assert_eq!(cycle.target(&[3], Some(3)), Some(3));
    }

    #[test]
    fn held_shortcut_is_consumed_until_special_release() {
        let mut latch = ShortcutLatch::default();
        assert_eq!(
            latch.sample(true, true, true, false, [true; 2]),
            (Some(0), true)
        );
        for _ in 0..60 {
            assert_eq!(
                latch.sample(true, true, true, false, [true; 2]),
                (None, true)
            );
        }
        assert_eq!(
            latch.sample(true, true, false, false, [true; 2]),
            (None, true)
        );
        assert_eq!(
            latch.sample(false, true, false, false, [true; 2]),
            (None, false)
        );
        assert_eq!(
            latch.sample(true, true, false, true, [true; 2]),
            (Some(1), true)
        );
    }

    #[test]
    fn directional_unavailable_and_ambiguous_chords_do_not_request() {
        let mut latch = ShortcutLatch::default();
        assert_eq!(
            latch.sample(true, false, true, false, [true; 2]),
            (None, false)
        );
        assert_eq!(
            latch.sample(true, true, true, false, [true; 2]),
            (None, false)
        );
        assert_eq!(
            latch.sample(false, true, false, false, [true; 2]),
            (None, false)
        );
        assert_eq!(
            latch.sample(true, true, true, false, [false, true]),
            (None, false)
        );
        // Becoming available while held must not equip unexpectedly.
        assert_eq!(
            latch.sample(true, true, true, false, [true; 2]),
            (None, false)
        );
        latch.sample(false, true, false, false, [true; 2]);
        assert_eq!(
            latch.sample(true, true, true, true, [true; 2]),
            (None, false)
        );
    }

    #[test]
    fn cpu_operator_chords_cycle_only_the_cpu_hat_on_their_own_latch() {
        let sample = |latch: &mut ShortcutLatch, special, left_click, right_click, available| {
            let (left, right) = operator_clicks(left_click, right_click);
            latch.sample(special, true, left, right, available)
        };
        let release = |latch: &mut ShortcutLatch| sample(latch, false, false, false, [true; 2]);
        let (mut p1, mut operator) = (ShortcutLatch::new(), ShortcutLatch::new());
        // P1 holds its CPU chord (Special + R3); the operator's separate latch still takes
        // the same chord once, for the CPU.
        assert_eq!(p1.sample(true, true, false, true, [true; 2]), (Some(1), true));
        assert_eq!(sample(&mut operator, true, false, true, [true; 2]), (Some(1), true));
        assert_eq!(sample(&mut operator, true, false, true, [true; 2]), (None, true));
        release(&mut operator);
        // Special + L3 is an alias for the same CPU request.
        assert_eq!(sample(&mut operator, true, true, false, [true; 2]), (Some(1), true));
        release(&mut operator);
        // Never P1's hat, even when only P1 is an eligible Kirby.
        assert_eq!(sample(&mut operator, true, true, false, [true, false]), (None, false));
        assert_eq!(sample(&mut operator, true, false, true, [true, false]), (None, false));
        release(&mut operator);
        // Both clicks hold the chord without a request; releasing one is not a new edge.
        assert_eq!(operator_clicks(true, true), (true, true));
        assert_eq!(sample(&mut operator, true, true, true, [true; 2]), (None, false));
        assert_eq!(sample(&mut operator, true, false, true, [true; 2]), (None, false));
        release(&mut operator);
        assert_eq!(operator_clicks(false, false), (false, false));
        assert_eq!(sample(&mut operator, true, false, false, [true; 2]), (None, false));
    }
}
