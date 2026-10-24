//! Value-only waiting-practice choices, independent of native entry lifetimes.
//! The caller owns lobby boundaries and validates a loaded fighter before commit.

#[derive(Clone, Copy, Debug)]
pub struct WaitingChoice<T> {
    remembered: Option<T>,
    restoring: bool,
}

impl<T: Copy> WaitingChoice<T> {
    pub const fn empty() -> Self {
        Self {
            remembered: None,
            restoring: false,
        }
    }

    /// A fresh lobby has no practice override from its predecessor.
    pub fn clear(&mut self) {
        *self = Self::empty();
    }

    /// Native entries may disappear for a match; only the fighter value survives.
    pub fn scene_changed(&mut self) {
        self.restoring = self.remembered.is_some();
    }

    /// The first ready native body supplies the default for a new lobby.
    /// Observing the CSS body on return must not replace a remembered choice.
    pub fn observe(&mut self, fighter: T) {
        if self.remembered.is_none() {
            self.commit(fighter);
        }
    }

    /// A queued or interrupted replacement is not a completed choice.
    pub fn commit(&mut self, fighter: T) {
        self.remembered = Some(fighter);
        self.restoring = false;
    }

    /// A new explicit request supersedes automatic restoration. Until it actually
    /// completes, a later scene transition still remembers the previous body.
    pub fn manual_request(&mut self) {
        self.restoring = false;
    }

    /// Peeking does not consume restoration while a menu/native action is busy.
    pub fn restore_target(&self) -> Option<T> {
        self.remembered.filter(|_| self.restoring)
    }
}

/// A waiting player's entry can be destroyed before either a match or a room exit.
/// Defer that distinction: a match changes the scene; recreating an LDN room can
/// create new entries with the exact same scene tuple (13.0.5 runtime trace).
pub struct WaitingSession<T> {
    choices: [WaitingChoice<T>; 2],
    scene: Option<(bool, u8, u32)>,
    player_departed: bool,
}

impl<T: Copy> WaitingSession<T> {
    pub const fn empty() -> Self {
        Self {
            choices: [WaitingChoice::empty(); 2],
            scene: None,
            player_departed: false,
        }
    }

    /// Returns the shared continuation decision for other value-only practice state.
    pub fn scene_changed(&mut self, initialized: bool, melee: u8, mode: u32) -> bool {
        let same_lobby = initialized
            && matches!(melee, 0x01 | 0x0d)
            && (mode == 0x02010000 || mode & 0x7fff0000 == 0x07010000)
            && self
                .scene
                .is_some_and(|(old_initialized, old_melee, old_mode)| {
                    old_initialized && old_mode == mode && matches!(old_melee, 0x01 | 0x0d)
                });
        for choice in &mut self.choices {
            if same_lobby {
                choice.scene_changed();
            } else {
                choice.clear();
            }
        }
        self.scene = Some((initialized, melee, mode));
        self.player_departed = false;
        same_lobby
    }

    /// Caller proves destruction of its current waiting P1 entry, not a body reload,
    /// preview, CPU partner or actual-match entry. Never retain the entry pointer.
    pub fn player_departed(&mut self) {
        self.player_departed = true;
    }

    /// Called for a newly observed, ready waiting body. Returns whether this is a
    /// fresh entry lifetime without a match excursion, which forgets BOTH roles.
    pub fn observe(&mut self, role: usize, fighter: T) -> bool {
        let fresh = self.player_departed;
        if fresh {
            for choice in &mut self.choices {
                choice.clear();
            }
            self.player_departed = false;
        }
        self.choices[role].observe(fighter);
        fresh
    }

    pub fn commit(&mut self, role: usize, fighter: T) {
        self.choices[role].commit(fighter);
    }

    pub fn manual_request(&mut self, role: usize) {
        self.choices[role].manual_request();
    }

    pub fn restore_target(&self, role: usize) -> Option<T> {
        self.choices[role].restore_target()
    }

    /// Disabled roster roles do not service their remembered target. Consumers
    /// such as hats must still work on those roles' current native fighters.
    pub fn restoration_pending(&self, enabled: [bool; 2]) -> bool {
        enabled.iter().copied().enumerate()
            .any(|(role, enabled)| enabled && self.restore_target(role).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::{WaitingChoice, WaitingSession};

    #[test]
    fn disabled_player_restore_does_not_block_cpu_hat_continuation() {
        let mut session = WaitingSession::empty();
        session.scene_changed(true, 0x0d, 0x02010000);
        session.observe(0, 10);
        session.observe(1, 6);
        session.scene_changed(true, 0x01, 0x02010000);
        session.scene_changed(true, 0x0d, 0x02010000);
        assert!(session.restoration_pending([false, true]));
        session.commit(1, 6);
        assert!(!session.restoration_pending([false, true]));
        assert!(session.restoration_pending([true, true]));
        session.commit(0, 10);
        assert!(!session.restoration_pending([true, true]));
    }

    #[test]
    fn entry_destruction_then_match_preserves_both_roles() {
        let mut session = WaitingSession::empty();
        session.scene_changed(true, 0x0d, 0x02010000);
        session.observe(0, 10);
        session.observe(1, 20);
        session.commit(0, 30);
        session.commit(1, 40);
        session.player_departed(); // Destruction precedes the mode change.
        session.scene_changed(true, 0x01, 0x02010000);
        session.scene_changed(true, 0x0d, 0x02010000);
        assert!(!session.observe(1, 20)); // Either entry may be observed first.
        assert!(!session.observe(0, 10));
        assert_eq!(session.restore_target(0), Some(30));
        assert_eq!(session.restore_target(1), Some(40));
    }

    #[test]
    fn recreated_ldn_room_with_identical_tuple_forgets_both_roles() {
        let mut session = WaitingSession::empty();
        session.scene_changed(true, 0x0d, 0x02010000);
        session.observe(0, 10);
        session.observe(1, 20);
        session.commit(0, 30);
        session.commit(1, 40);
        session.player_departed();
        // Observed runtime: close room -> rooms list -> new CSS -> waiting,
        // all without any new scene tuple or epoch increment.
        assert!(session.observe(1, 50));
        assert!(!session.observe(0, 60));
        assert_eq!(session.restore_target(0), None);
        assert_eq!(session.restore_target(1), None);
        session.scene_changed(true, 0x01, 0x02010000);
        assert_eq!(session.restore_target(0), Some(60));
        assert_eq!(session.restore_target(1), Some(50));
    }

    #[test]
    fn ordinary_training_and_unknown_scenes_do_not_preserve_lobby_choices() {
        for (initialized, melee, mode) in [
            (true, 0x20, 0x04040000),
            (true, 0x00, 0x02010000),
            (false, 0x0d, 0x02010000),
            (true, 0x0d, 0x07010000),
        ] {
            let mut session = WaitingSession::empty();
            session.scene_changed(true, 0x0d, 0x02010000);
            session.commit(0, 10);
            session.scene_changed(initialized, melee, mode);
            session.scene_changed(true, 0x0d, 0x02010000);
            session.observe(0, 20);
            session.scene_changed(true, 0x01, 0x02010000);
            assert_eq!(session.restore_target(0), Some(20));
        }
    }

    #[test]
    fn match_and_css_return_preserve_both_practice_choices() {
        let mut player = WaitingChoice::empty();
        let mut cpu = WaitingChoice::empty();
        player.observe("CSS G&W");
        cpu.observe("initial CPU");
        player.commit("Palutena");
        cpu.commit("Pikachu");
        for _ in 0..3 {
            player.scene_changed();
            cpu.scene_changed();
        }
        player.observe("new match CSS Link");
        cpu.observe("fresh native CPU");
        assert_eq!(player.restore_target(), Some("Palutena"));
        assert_eq!(cpu.restore_target(), Some("Pikachu"));
        player.commit("Palutena");
        assert_eq!(player.restore_target(), None);
        assert_eq!(cpu.restore_target(), Some("Pikachu"));
    }

    #[test]
    fn fresh_lobby_discards_prior_choices_and_uses_its_native_defaults() {
        let mut memory = WaitingChoice::empty();
        memory.commit(10);
        memory.scene_changed();
        memory.clear();
        memory.scene_changed();
        memory.observe(20);
        assert_eq!(memory.restore_target(), None);
        memory.scene_changed();
        assert_eq!(memory.restore_target(), Some(20));
    }

    #[test]
    fn interrupted_manual_override_retains_last_completed_body() {
        let mut memory = WaitingChoice::empty();
        memory.commit(10);
        memory.scene_changed();
        memory.manual_request();
        assert_eq!(memory.restore_target(), None);
        // No commit: native teardown interrupted the replacement.
        memory.scene_changed();
        memory.observe(99);
        assert_eq!(memory.restore_target(), Some(10));
        memory.manual_request();
        memory.commit(20);
        memory.scene_changed();
        assert_eq!(memory.restore_target(), Some(20));
    }

    #[test]
    fn disabled_or_busy_restoration_remains_available_until_completion() {
        let mut memory = WaitingChoice::empty();
        memory.commit(10);
        memory.scene_changed();
        for _ in 0..600 {
            assert_eq!(memory.restore_target(), Some(10));
        }
        memory.commit(10);
        assert_eq!(memory.restore_target(), None);
    }
}
