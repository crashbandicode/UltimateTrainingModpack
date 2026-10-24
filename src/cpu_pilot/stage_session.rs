//! Bound the extra native analysis work; successful ownership lasts to scene exit.
//! The adapter supplies identity/native-AI checks before calling this state machine.

pub(super) const LIMIT: u16 = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Pending,
    Ready,
    Failed,
    Cancelled,
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Pass,
    Initialize,
    Isolate,
}

#[derive(Clone, Copy)]
pub(super) struct Session {
    pub status: Status,
    pub frames: u16,
}

impl Session {
    pub(super) fn start(intent: bool, human_only: bool, phase: i32, d0: u8) -> Option<Self> {
        (intent && human_only && phase == 1 && d0 & 1 == 0).then_some(Self {
            status: Status::Pending,
            frames: 0,
        })
    }

    pub(super) fn before(&mut self, intent: bool, paused: bool) -> Action {
        match self.status {
            // Phase 2 keeps running natively even when the request/menu ends.
            Status::Ready => Action::Isolate,
            Status::Pending if !intent => {
                self.status = Status::Cancelled;
                Action::Pass
            }
            Status::Pending if !paused => {
                self.frames += 1;
                Action::Initialize
            }
            _ => Action::Pass,
        }
    }

    pub(super) fn after(&mut self, phase: i32) {
        if self.status == Status::Pending {
            if phase == 2 {
                self.status = Status::Ready;
            } else if self.frames == LIMIT {
                self.status = Status::Failed;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_pair_alone_or_existing_native_ai_never_activates() {
        assert!(Session::start(false, true, 1, 0).is_none());
        assert!(Session::start(true, false, 1, 0).is_none());
        assert!(Session::start(true, true, 1, 1).is_none());
        assert!(Session::start(true, true, 2, 0).is_none());
        assert!(Session::start(true, true, 0, 0).is_none());
    }

    #[test]
    fn exactly_120_unpaused_attempts_then_no_automatic_retry() {
        let mut session = Session::start(true, true, 1, 0).unwrap();
        for _ in 0..180 {
            assert_eq!(session.before(true, true), Action::Pass);
        }
        assert_eq!(session.frames, 0);
        for attempt in 1..=120 {
            assert_eq!(session.before(true, false), Action::Initialize);
            session.after(1);
            assert_eq!(session.frames, attempt);
        }
        assert_eq!(session.status, Status::Failed);
        assert_eq!(session.before(true, false), Action::Pass);
        assert_eq!(session.frames, 120);
    }

    #[test]
    fn success_on_final_attempt_keeps_isolation_after_owner_and_during_menu() {
        let mut session = Session::start(true, true, 1, 0).unwrap();
        for _ in 0..119 {
            session.before(true, false);
            session.after(1);
        }
        assert_eq!(session.before(true, false), Action::Initialize);
        session.after(2);
        assert_eq!(session.status, Status::Ready);
        assert_eq!(session.before(false, true), Action::Isolate);
        // A later phase regression is not permission for another unbounded raise.
        session.after(1);
        assert_eq!(session.before(false, false), Action::Isolate);
        assert_eq!(session.frames, 120);
    }

    #[test]
    fn cancel_before_completion_never_raises_again() {
        let mut session = Session::start(true, true, 1, 0).unwrap();
        assert_eq!(session.before(false, false), Action::Pass);
        assert_eq!(session.status, Status::Cancelled);
        assert_eq!(session.before(true, false), Action::Pass);
    }
}
