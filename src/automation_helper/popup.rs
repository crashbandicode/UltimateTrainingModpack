//! Track a qualified native popup within one results lifetime. The native
//! adapter identifies the opener; a retained result at an idle popup is not a
//! new answer. Addresses here are identity values, never dereferenced pointers.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    SaveConfirmation,
    Rematch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Ready(Kind),
    Decided { kind: Kind, result: u32, source: Source },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Normal,
    Forced,
    Late,
}

/// Read together by the native adapter on the game thread. `pending` is the
/// checked top of the native phase stack, not a guessed next phase.
#[derive(Clone, Copy)]
pub struct Snapshot {
    pub phase: u32,
    pub pending: u32,
    pub previous: u32,
    pub age: u32,
    pub result: u32,
    pub decided: bool,
    pub opening: bool,
}

#[derive(Clone, Copy)]
struct Open {
    owner: usize,
    kind: Kind,
    saw_reset: bool,
    ready: bool,
}

pub struct Popup {
    epoch: Option<u64>,
    open: Option<Open>,
}

impl Popup {
    pub const fn new() -> Self {
        Self {
            epoch: None,
            open: None,
        }
    }

    /// Called at the results lifetime boundary, including an address reused by
    /// the next match. The epoch comes from native construction, not a pointer.
    pub fn begin(&mut self, epoch: u64) {
        self.epoch = Some(epoch);
        self.open = None;
    }

    pub fn end(&mut self) {
        self.epoch = None;
        self.open = None;
    }

    /// Every open on the primary native popup manager must be reported,
    /// including unrecognized messages. The adapter filters other managers.
    /// `None` invalidates the previous popup rather than inheriting its kind.
    /// Qualification includes the native caller, message and observed zero
    /// default result, not just timing. A nonzero default is unsupported.
    pub fn opened(&mut self, epoch: u64, owner: usize, kind: Option<Kind>) {
        self.open = if self.epoch == Some(epoch) && owner != 0 {
            kind.map(|kind| Open {
                owner,
                kind,
                saw_reset: false,
                ready: false,
            })
        } else {
            None
        };
    }

    /// Only settled phase 3 accepts normal input. Phase 4 runs a timer.
    /// Latch the decision before closing animation: the results lifetime can
    /// end before a later idle observation. A forced or late result retains
    /// its provenance so the caller cannot claim its input caused the answer.
    /// Native result interpretation belongs to the version-checked adapter.
    pub fn observe(
        &mut self,
        epoch: u64,
        owner: usize,
        state: Snapshot,
    ) -> Option<Event> {
        let open = self.open.as_mut()?;
        if self.epoch != Some(epoch) {
            self.open = None;
            return None;
        }
        // The native update also runs for a second, unrelated manager.
        if open.owner != owner {
            return None;
        }
        if state.phase > 7 || state.pending > 7 || state.previous > 7 {
            self.open = None;
            return None;
        }
        // Admission requires an observed zero default. Seeing that active
        // reset prevents the old idle result from acknowledging a new request.
        if state.phase != 0 && state.result == 0 {
            open.saw_reset = true;
        }
        let source = match (state.phase, state.pending) {
            (3, 6) if state.decided => Some(Source::Normal),
            (5, 6) => Some(Source::Forced),
            (6 | 7, _) => Some(Source::Late),
            (0, 0) if state.previous == 7 => Some(Source::Late),
            _ => None,
        };
        if let Some(source) = source.filter(|_| open.saw_reset) {
            let event = Event::Decided { kind: open.kind, result: state.result, source };
            self.open = None;
            return Some(event);
        }
        if open.saw_reset && state.phase == 3 && state.pending == 3
            && state.age >= 1 && state.result == 0 && !state.decided && !state.opening
            && !open.ready {
            open.ready = true;
            return Some(Event::Ready(open.kind));
        }
        if state.phase == 0 && state.pending == 0 && !state.opening {
            self.open = None;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(phase: u32, result: u32) -> Snapshot {
        Snapshot { phase, pending: phase, previous: 7, age: 1, result,
            decided: false, opening: false }
    }

    #[test]
    fn retained_idle_result_is_not_a_new_decision() {
        // binding44/no02/popup-after-no.json: same popup owner, phase0,
        // previous7, result1 long after No returned to character select.
        let mut p = Popup::new();
        p.begin(1);
        p.opened(1, 0x11e21bee80, Some(Kind::Rematch));
        assert_eq!(p.observe(1, 0x11e21bee80, Snapshot { opening: false, ..state(0, 1) }), None);
        assert_eq!(p.observe(1, 0x11e21bee80, state(3, 0)), None);
    }

    #[test]
    fn open_pending_idle_ticks_preserve_the_fresh_prompt() {
        // Native open sets opening=1 first. The idle handler pushes phase1
        // on a later tick, so an entry hook can observe idle more than once.
        let mut p = Popup::new();
        p.begin(1);
        p.opened(1, 10, Some(Kind::Rematch));
        for _ in 0..2 {
            assert_eq!(p.observe(1, 10, Snapshot { opening: true, ..state(0, 0) }), None);
        }
        assert_eq!(p.observe(1, 10, state(1, 0)), None);
        assert_eq!(p.observe(1, 10, state(3, 0)), Some(Event::Ready(Kind::Rematch)));
    }

    #[test]
    fn second_manager_tick_does_not_discard_the_owned_prompt() {
        let mut p = Popup::new();
        p.begin(1);
        p.opened(1, 10, Some(Kind::Rematch));
        p.observe(1, 10, state(1, 0));
        assert_eq!(p.observe(1, 20, state(0, 1)), None);
        assert_eq!(p.observe(1, 10, state(3, 0)), Some(Event::Ready(Kind::Rematch)));
    }

    #[test]
    fn normal_answer_is_latched_before_results_teardown() {
        for result in [0, 1, 2, 3] {
            let mut p = Popup::new();
            p.begin(2);
            p.opened(2, 10, Some(Kind::Rematch));
            assert_eq!(p.observe(2, 10, state(1, 0)), None);
            assert_eq!(p.observe(2, 10, state(3, 0)), Some(Event::Ready(Kind::Rematch)));
            assert_eq!(p.observe(2, 10, state(3, 0)), None);
            let answer = Snapshot { pending: 6, decided: true, ..state(3, result) };
            assert_eq!(p.observe(2, 10, answer), Some(Event::Decided {
                kind: Kind::Rematch, result, source: Source::Normal,
            }));
            p.end(); // No idle-popup tick is required before epoch teardown.
            assert_eq!(p.observe(2, 10, state(0, result)), None);
        }
    }

    #[test]
    fn timer_and_unsettled_prompt_are_not_ready_for_input() {
        let mut p = Popup::new();
        p.begin(1);
        p.opened(1, 10, Some(Kind::Rematch));
        for s in [state(4, 0), Snapshot { age: 0, ..state(3, 0) },
            Snapshot { opening: true, ..state(3, 0) },
            Snapshot { decided: true, ..state(3, 0) },
            Snapshot { pending: 2, ..state(3, 0) }] {
            assert_eq!(p.observe(1, 10, s), None);
        }
        assert_eq!(p.observe(1, 10, state(3, 0)), Some(Event::Ready(Kind::Rematch)));
    }

    #[test]
    fn forced_and_late_answers_keep_their_provenance() {
        for (s, source) in [(Snapshot { pending: 6, ..state(5, 1) }, Source::Forced),
            (state(6, 2), Source::Late), (state(0, 1), Source::Late)] {
            let mut p = Popup::new();
            p.begin(1);
            p.opened(1, 10, Some(Kind::Rematch));
            p.observe(1, 10, state(1, 0));
            assert_eq!(p.observe(1, 10, s), Some(Event::Decided {
                kind: Kind::Rematch, result: s.result, source,
            }));
            assert_eq!(p.observe(1, 10, s), None);
        }
    }

    #[test]
    fn another_dialog_cannot_acknowledge_the_rematch() {
        let mut p = Popup::new();
        p.begin(1);
        p.opened(1, 10, Some(Kind::Rematch));
        p.observe(1, 10, state(3, 0));
        p.opened(1, 10, Some(Kind::SaveConfirmation));
        assert_eq!(p.observe(1, 10, state(3, 0)), Some(Event::Ready(Kind::SaveConfirmation)));
        assert_eq!(p.observe(1, 10, state(0, 2)), Some(Event::Decided {
            kind: Kind::SaveConfirmation, result: 2, source: Source::Late,
        }));
        p.opened(1, 10, Some(Kind::Rematch));
        p.observe(1, 10, state(3, 0));
        p.opened(1, 10, None);
        assert_eq!(p.observe(1, 10, state(0, 2)), None);
    }

    #[test]
    fn teardown_reused_address_or_wrong_epoch_discards_pending_answer() {
        // Each path represents a different invalidation boundary, not an
        // additional gameplay matrix.
        for boundary in 0..4 {
            let mut p = Popup::new();
            p.begin(1);
            p.opened(1, 10, Some(Kind::Rematch));
            p.observe(1, 10, state(3, 0));
            match boundary {
                0 => p.end(),
                1 => p.begin(2),
                2 => { p.observe(2, 10, state(3, 0)); }
                _ => { p.observe(1, 10, state(99, 0)); }
            }
            assert_eq!(p.observe(1, 10, state(0, 2)), None);
        }
    }
}
