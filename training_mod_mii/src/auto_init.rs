//! One automatic catalog read per visible-entry lifetime. Values only; no IO or native access.
use crate::inventory::MiiClass;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub generations: [u64; 2],
    pub opening: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Waiting,
    Reading,
    Finished,
}

pub struct Initialization {
    token: Option<Token>,
    ready: u8,
    phase: Phase,
}

impl Initialization {
    pub const fn empty() -> Self {
        Self {
            token: None,
            ready: 0,
            phase: Phase::Waiting,
        }
    }

    pub fn pending(&self) -> bool {
        self.phase != Phase::Finished
    }

    /// `eligible` includes both known originals, the current role's native readiness,
    /// and the existing practice exclusions. A callback supplies only its own role.
    pub fn observe(&mut self, token: Token, role: u32, eligible: bool) -> Option<Token> {
        if role > 1 || self.phase != Phase::Waiting {
            return None;
        }
        if self.token.is_some_and(|previous| previous != token) {
            return None;
        }
        self.token = Some(token);
        let bit = 1 << role;
        if eligible {
            self.ready |= bit;
        } else {
            self.ready &= !bit;
        }
        if self.ready != 3 {
            return None;
        }
        self.phase = Phase::Reading;
        Some(token)
    }

    /// Consume this read exactly once, including incomplete or stale results.
    /// An old completion cannot consume a new lifetime's pending read.
    pub fn finish(&mut self, token: Token, current: Token, complete: bool) -> bool {
        if self.phase != Phase::Reading || self.token != Some(token) {
            return false;
        }
        self.phase = Phase::Finished;
        token == current && complete
    }

    /// An explicit menu refresh supersedes automatic work for this lifetime.
    pub fn cancel(&mut self) {
        self.phase = Phase::Finished;
    }

    /// Scene change and visible-entry destruction both start a fresh lifetime.
    pub fn reset(&mut self) {
        *self = Self::empty();
    }
}

/// A mixed Player roster waits only while its native qualification can still finish.
/// Explicit Mii/original-Mii protection is independent of this temporary wait.
pub fn player_qualifying(
    native_training: bool,
    cpu_quantity: Option<u32>,
    automatic: bool,
    capture: bool,
) -> bool {
    native_training && cpu_quantity == Some(1) && (automatic || capture)
}

/// Until capture succeeds, consume Mii-related input without creating a request.
/// Plain ordinary-only routing remains available while the catalog is loading.
pub fn requires_owner(original: Option<u32>, roster_has_mii: bool, direct: Option<u32>) -> bool {
    roster_has_mii
        || original.and_then(MiiClass::from_kind).is_some()
        || direct.and_then(MiiClass::from_kind).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: Token = Token {
        generations: [7, 11],
        opening: 13,
    };

    #[test]
    fn one_read_and_one_arm_per_lifetime_even_after_read_failure() {
        for complete in [true, false] {
            let mut init = Initialization::empty();
            assert_eq!(init.observe(FIRST, 0, true), None);
            assert_eq!(init.observe(FIRST, 0, true), None);
            assert_eq!(init.observe(FIRST, 1, true), Some(FIRST));
            assert_eq!(init.observe(FIRST, 1, true), None);
            assert_eq!(init.finish(FIRST, FIRST, complete), complete);
            assert!(!init.finish(FIRST, FIRST, true));
            assert!(!init.pending());
            assert_eq!(init.observe(FIRST, 0, true), None);
            assert_eq!(init.observe(FIRST, 1, true), None);
        }
    }

    #[test]
    fn scene_destructor_and_menu_changes_reject_stale_completion() {
        let mut init = Initialization::empty();
        init.observe(FIRST, 0, true);
        init.observe(FIRST, 1, true);
        init.cancel(); // A real menu opening owns its independent refresh.
        assert!(!init.finish(FIRST, FIRST, true));
        assert!(!init.pending());
        for next in [
            Token {
                generations: [8, 12],
                opening: 14,
            }, // Scene boundary.
            Token {
                generations: [7, 12],
                opening: 14,
            }, // CPU entry destruction.
        ] {
            init.reset();
            assert_eq!(init.observe(next, 0, true), None);
            assert_eq!(init.observe(next, 1, true), Some(next));
            assert!(!init.finish(FIRST, next, true));
            assert!(init.pending());
            assert!(init.finish(next, next, true));
        }
        init.reset();
        init.observe(FIRST, 0, true);
        init.observe(FIRST, 1, true);
        assert!(!init.finish(
            FIRST,
            Token {
                opening: 14,
                ..FIRST
            },
            true
        ));
    }

    #[test]
    fn readiness_precedes_arming_and_mii_input_never_falls_through_unqualified() {
        let mut init = Initialization::empty();
        assert_eq!(init.observe(FIRST, 0, false), None); // Original/readiness unavailable.
        assert_eq!(init.observe(FIRST, 1, true), None);
        assert_eq!(init.observe(FIRST, 0, false), None); // INSTALL or another action owns practice.
        assert_eq!(init.observe(FIRST, 4, true), None); // Not a visible Training role.
        assert!(!init.finish(FIRST, FIRST, true));
        assert!(requires_owner(Some(0x49), false, None));
        assert!(requires_owner(Some(9), true, None));
        assert!(requires_owner(Some(9), false, Some(0x48)));
        assert!(!requires_owner(Some(9), false, Some(0x0f)));
        assert!(!requires_owner(None, false, None));
        let mixed_player = |native, quantity, automatic, capture| {
            requires_owner(
                Some(9),
                player_qualifying(native, quantity, automatic, capture),
                None,
            )
        };
        assert!(mixed_player(true, Some(1), init.pending(), false));
        assert!(!mixed_player(false, Some(1), init.pending(), false)); // Waiting scope.
        assert!(!mixed_player(true, Some(2), init.pending(), false)); // Unsupported quantity.
        assert!(!mixed_player(true, None, init.pending(), true)); // Unknown native quantity.
        assert_eq!(init.observe(FIRST, 0, true), Some(FIRST));
        assert!(init.finish(FIRST, FIRST, true));
        assert!(mixed_player(true, Some(1), init.pending(), true)); // Catalog done; role capture armed.
        assert!(!mixed_player(true, Some(1), init.pending(), false)); // Refused/completed capture.
        init.reset();
        init.observe(FIRST, 0, true);
        init.observe(FIRST, 1, true);
        assert!(!init.finish(FIRST, FIRST, false)); // Incomplete catalog consumes the attempt.
        assert!(!mixed_player(true, Some(1), init.pending(), false));
        assert!(requires_owner(Some(0x49), false, None)); // Immutable Mii origin remains protected.
        assert!(requires_owner(Some(9), false, Some(0x48))); // Direct Mii remains refused.
    }
}
