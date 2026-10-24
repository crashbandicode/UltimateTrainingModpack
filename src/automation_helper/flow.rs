//! Once-per-results operation policy. Native code supplies observed states and
//! a results-lifetime generation; pointers or sent inputs are not completion.
use super::control::{Decision, Request};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Waiting,
    #[cfg_attr(not(test), expect(dead_code, reason = "The native adapter starts at cards; intro readiness is not yet connected."))]
    IntroReady,
    Cards,
    SaveConfirm,
    Saving,
    SaveSuccess,
    SavedAndClosed,
    SaveFailed,
    Rematch,
    Accepted(Decision),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    TimedOut,
    SceneChanged,
    SaveFailed,
    SaveSkipped,
    WrongDecision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    AdvanceIntro,
    RequestSave,
    ConfirmSave,
    CloseSave,
    AdvanceResults,
    Choose(Decision),
    Complete,
    Failed(Failure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Intro,
    Cards,
    Confirm,
    Saving,
    Close,
    Rematch,
    Acknowledge,
    End,
}

pub struct Flow {
    request: Request,
    epoch: Option<u64>,
    deadline_ms: u64,
    stage: Stage,
}

impl Flow {
    pub fn new(request: Request, now_ms: u64) -> Self {
        Self {
            request,
            epoch: None,
            deadline_ms: now_ms.saturating_add(600_000),
            stage: Stage::Intro,
        }
    }

    fn fail(&mut self, why: Failure) -> Effect {
        self.stage = Stage::End;
        Effect::Failed(why)
    }

    fn saved(&mut self) -> Effect {
        if self.request.rematch == Decision::Manual {
            self.stage = Stage::End;
            Effect::Complete
        } else {
            self.stage = Stage::Rematch;
            Effect::AdvanceResults
        }
    }

    /// `epoch` spans the results and its rematch prompt; the adapter increments
    /// it on a new results sequence, even if its native address was reused.
    /// The adapter must recheck Controls::owns immediately before each effect.
    pub fn observe(&mut self, now_ms: u64, epoch: Option<u64>, screen: Screen) -> Effect {
        if self.stage == Stage::End {
            return Effect::None;
        }
        if now_ms >= self.deadline_ms {
            return self.fail(Failure::TimedOut);
        }
        if let Some(bound) = self.epoch {
            if epoch != Some(bound) {
                return self.fail(Failure::SceneChanged);
            }
        } else if let Some(epoch) = epoch {
            self.epoch = Some(epoch);
            // This is an active native flow now, not the wait for a match.
            self.deadline_ms = now_ms.saturating_add(120_000);
        } else {
            return Effect::None;
        }
        if screen == Screen::SaveFailed {
            return self.fail(Failure::SaveFailed);
        }
        match (self.stage, screen) {
            (Stage::Intro, Screen::IntroReady) => {
                if !self.request.save_replay && self.request.rematch == Decision::Manual {
                    self.stage = Stage::End;
                    Effect::Complete
                } else {
                    self.stage = Stage::Cards;
                    Effect::AdvanceIntro
                }
            }
            (Stage::Intro | Stage::Cards, Screen::Cards) if self.request.save_replay => {
                self.stage = Stage::Confirm;
                Effect::RequestSave
            }
            (Stage::Intro | Stage::Cards, Screen::Cards) => self.saved(),
            (Stage::Confirm, Screen::SaveConfirm) => {
                self.stage = Stage::Saving;
                Effect::ConfirmSave
            }
            (Stage::Saving, Screen::SaveSuccess) => {
                self.stage = Stage::Close;
                Effect::CloseSave
            }
            (Stage::Close, Screen::SavedAndClosed) => self.saved(),
            (Stage::Intro | Stage::Cards | Stage::Rematch, Screen::Rematch)
                if !self.request.save_replay || self.stage == Stage::Rematch =>
            {
                // Native results can advance on their own. A no-save request
                // need not observe the cards, but a requested save must finish.
                if self.request.rematch == Decision::Manual {
                    self.stage = Stage::End;
                    Effect::Complete
                } else {
                    self.stage = Stage::Acknowledge;
                    Effect::Choose(self.request.rematch)
                }
            }
            (Stage::Acknowledge, Screen::Accepted(choice)) => {
                if choice != self.request.rematch {
                    return self.fail(Failure::WrongDecision);
                }
                self.stage = Stage::End;
                Effect::Complete
            }
            (
                Stage::Intro | Stage::Cards | Stage::Confirm | Stage::Saving | Stage::Close,
                Screen::Rematch,
            ) if self.request.save_replay => self.fail(Failure::SaveSkipped),
            _ => Effect::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(save_replay: bool, rematch: Decision) -> Request {
        Request {
            generation: 256,
            save_replay,
            rematch,
        }
    }
    #[test]
    fn every_action_is_once_and_save_must_finish_before_rematch() {
        let mut f = Flow::new(request(true, Decision::Yes), 0);
        let steps = [
            (Screen::IntroReady, Effect::AdvanceIntro),
            (Screen::Cards, Effect::RequestSave),
            (Screen::SaveConfirm, Effect::ConfirmSave),
            (Screen::Saving, Effect::None),
            (Screen::SaveSuccess, Effect::CloseSave),
            (Screen::SavedAndClosed, Effect::AdvanceResults),
            (Screen::Rematch, Effect::Choose(Decision::Yes)),
            (Screen::Accepted(Decision::Yes), Effect::Complete),
        ];
        for (screen, expected) in steps {
            assert_eq!(f.observe(1, Some(42), screen), expected);
            assert_eq!(f.observe(1, Some(42), screen), Effect::None);
        }
    }
    #[test]
    fn save_failure_or_skipped_save_never_accepts_rematch() {
        for (screen, why) in [
            (Screen::SaveFailed, Failure::SaveFailed),
            (Screen::Rematch, Failure::SaveSkipped),
        ] {
            let mut f = Flow::new(request(true, Decision::Yes), 0);
            assert_eq!(f.observe(1, Some(1), Screen::Cards), Effect::RequestSave);
            assert_eq!(f.observe(2, Some(1), screen), Effect::Failed(why));
            assert_eq!(f.observe(3, Some(1), Screen::Rematch), Effect::None);
        }
    }
    #[test]
    fn no_save_requires_the_requested_decision_and_manual_stops_at_cards() {
        let mut f = Flow::new(request(false, Decision::No), 0);
        assert_eq!(f.observe(1, Some(1), Screen::Cards), Effect::AdvanceResults);
        assert_eq!(
            f.observe(2, Some(1), Screen::Rematch),
            Effect::Choose(Decision::No)
        );
        assert_eq!(
            f.observe(3, Some(1), Screen::Accepted(Decision::Yes)),
            Effect::Failed(Failure::WrongDecision)
        );
        let mut f = Flow::new(request(false, Decision::Manual), 0);
        assert_eq!(f.observe(1, Some(2), Screen::Cards), Effect::Complete);
        assert_eq!(f.observe(2, Some(2), Screen::Rematch), Effect::None);
    }
    #[test]
    fn auto_advanced_results_still_apply_the_requested_rematch_once() {
        for choice in [Decision::Yes, Decision::No, Decision::Manual] {
            for saw_intro in [false, true] {
                let mut f = Flow::new(request(false, choice), 0);
                if saw_intro {
                    assert_eq!(
                        f.observe(1, Some(7), Screen::IntroReady),
                        if choice == Decision::Manual {
                            Effect::Complete
                        } else {
                            Effect::AdvanceIntro
                        }
                    );
                }
                let expected = match (choice, saw_intro) {
                    (Decision::Manual, true) => Effect::None,
                    (Decision::Manual, false) => Effect::Complete,
                    _ => Effect::Choose(choice),
                };
                assert_eq!(f.observe(2, Some(7), Screen::Rematch), expected);
                assert_eq!(f.observe(3, Some(7), Screen::Rematch), Effect::None);
                if choice != Decision::Manual {
                    assert_eq!(
                        f.observe(4, Some(7), Screen::Accepted(choice)),
                        Effect::Complete
                    );
                }
            }
        }
        // The same auto-advance must not bypass a requested replay save.
        for saw_intro in [false, true] {
            let mut f = Flow::new(request(true, Decision::Yes), 0);
            if saw_intro {
                assert_eq!(
                    f.observe(1, Some(7), Screen::IntroReady),
                    Effect::AdvanceIntro
                );
            }
            assert_eq!(
                f.observe(2, Some(7), Screen::Rematch),
                Effect::Failed(Failure::SaveSkipped)
            );
            assert_eq!(f.observe(3, Some(7), Screen::Rematch), Effect::None);
        }
    }
    #[test]
    fn changed_results_or_timeout_stops_without_an_action() {
        let mut f = Flow::new(request(true, Decision::Yes), 0);
        assert_eq!(f.observe(1, Some(1), Screen::Cards), Effect::RequestSave);
        assert_eq!(
            f.observe(2, Some(2), Screen::SaveConfirm),
            Effect::Failed(Failure::SceneChanged)
        );
        let mut f = Flow::new(request(true, Decision::Yes), 0);
        assert_eq!(
            f.observe(600_000, None, Screen::Waiting),
            Effect::Failed(Failure::TimedOut)
        );
    }
}
