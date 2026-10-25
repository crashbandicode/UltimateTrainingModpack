//! One saved-Mii catalog read at a time. Values only: no files, threads, or native access.
//!
//! A menu open queues at most one read. A newer open, close, or retired generation does not
//! apply the older result and does not enqueue a second read beside it. The caller retries a
//! full writer queue across later polls, at most [`MAX_SENDS`] attempts, and never blocks.

use crate::auto_init::Token;

/// Attempts for one demand, including the first. Matches a full replay-writer queue draining
/// across later polls rather than a same-call spin.
pub const MAX_SENDS: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Live {
    pub menu_active: bool,
    pub opening: u64,
    pub owners: [u64; 2],
    pub auto: Option<Token>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Training or a qualified waiting backend (local wireless, Quickplay). The caller
    /// may arm fighter capture after this result.
    Menu { opening: u64, owners: [u64; 2] },
    /// A waiting room without a qualified fighter backend. The caller mounts the
    /// editor-only catalog and does not arm a fighter role. One opening mounts one
    /// catalog: a `Menu` demand for the same opening supersedes this kind, because the
    /// fighter menu already offers Add New.
    Editor { opening: u64 },
    /// Lifetime read before any menu open.
    Auto(Token),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Submit(Kind),
    Wait,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Menu { opening: u64, owners: [u64; 2] },
    Editor { opening: u64 },
    Auto(Token),
    Discard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    pub verdict: Verdict,
    pub step: Step,
    pub finished: Option<Kind>,
    /// A demand that will not receive a catalog, besides `finished` when that was discarded.
    pub dropped: Option<Kind>,
}

impl Effect {
    pub const fn quiet(step: Step) -> Self {
        Self {
            verdict: Verdict::Discard,
            step,
            finished: None,
            dropped: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Flight {
    kind: Kind,
    serial: u64,
    sends: u8,
    usable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Waiting(Flight),
    Flying(Flight),
}

pub struct Requests {
    phase: Phase,
    next: Option<Kind>,
    serial: u64,
    /// Set when a menu opening closes. This is not an epoch bump.
    closed_opening: Option<u64>,
    /// Remains pending through delivery, until capture is actually armed.
    menu_unarmed: Option<u64>,
    /// The opening whose fighter menu owns the catalog mount; editor-only demands for it
    /// are dropped. Cleared by that opening's close or by retirement.
    menu_opening: Option<u64>,
}

impl Default for Requests {
    fn default() -> Self {
        Self::new()
    }
}

impl Requests {
    pub const fn new() -> Self {
        Self {
            phase: Phase::Idle,
            next: None,
            serial: 0,
            closed_opening: None,
            menu_unarmed: None,
            menu_opening: None,
        }
    }

    pub fn pending(&self) -> bool {
        self.next.is_some() || !matches!(self.phase, Phase::Idle)
    }

    pub fn serial(&self) -> u64 {
        self.serial
    }

    pub fn needs_retry(&self) -> bool {
        matches!(self.phase, Phase::Waiting(_))
    }

    pub fn allows_editor(&self, opening: u64, live: &Live) -> bool {
        self.wanted(Kind::Editor { opening }, live)
    }

    pub fn request(&mut self, kind: Kind) -> Effect {
        // A closed opening stays closed. Menu epochs are not reused, so this does not block the next open.
        if opening_of(kind).is_some_and(|opening| self.closed_opening == Some(opening)) {
            return Effect {
                dropped: Some(kind),
                ..Effect::quiet(Step::Wait)
            };
        }
        if let Kind::Menu { opening, .. } = kind {
            self.menu_unarmed = Some(opening);
            self.menu_opening = Some(opening);
            // An editor-only read already in flight for this opening must not mount.
            if let Phase::Flying(flight) = &mut self.phase {
                if flight.kind == (Kind::Editor { opening }) {
                    flight.usable = false;
                }
            }
        }
        if let Kind::Editor { opening } = kind {
            if self.menu_opening == Some(opening) {
                return Effect {
                    dropped: Some(kind),
                    ..Effect::quiet(Step::Wait)
                };
            }
        }
        let mut dropped = None;
        let step = match self.phase {
            Phase::Idle => self.begin(kind),
            Phase::Waiting(flight) => {
                if flight.kind != kind {
                    dropped = Some(flight.kind);
                }
                self.phase = Phase::Waiting(Flight { kind, ..flight });
                Step::Wait
            }
            Phase::Flying(flight) if flight.usable && flight.kind == kind => Step::Wait,
            Phase::Flying(_) => {
                let previous = self.next.replace(kind);
                if previous.is_some_and(|previous| previous != kind) {
                    dropped = previous;
                }
                Step::Wait
            }
        };
        Effect {
            dropped,
            ..Effect::quiet(step)
        }
    }

    pub fn rejected_full(&mut self) -> Effect {
        let Phase::Flying(flight) = self.phase else {
            return Effect::quiet(Step::Wait);
        };
        if flight.sends >= MAX_SENDS {
            self.phase = Phase::Idle;
            let dropped = self.next.take();
            return Effect {
                verdict: Verdict::Discard,
                step: Step::Unavailable,
                finished: Some(flight.kind),
                dropped,
            };
        }
        self.phase = Phase::Waiting(Flight {
            sends: flight.sends + 1,
            ..flight
        });
        Effect::quiet(Step::Wait)
    }

    pub fn retry(&mut self, live: Live) -> Effect {
        let Phase::Waiting(flight) = self.phase else {
            return Effect::quiet(Step::Wait);
        };
        if !flight.usable || !self.wanted(flight.kind, &live) {
            self.phase = Phase::Idle;
            return Effect {
                verdict: Verdict::Discard,
                step: Step::Unavailable,
                finished: Some(flight.kind),
                dropped: None,
            };
        }
        let sends = flight.sends;
        let step = self.begin(flight.kind);
        if let Phase::Flying(current) = &mut self.phase {
            current.sends = sends;
        }
        Effect::quiet(step)
    }

    pub fn complete(&mut self, serial: u64, live: Live) -> Effect {
        let Phase::Flying(flight) = self.phase else {
            return Effect::quiet(Step::Wait);
        };
        if flight.serial != serial {
            return Effect::quiet(Step::Wait);
        }
        self.phase = Phase::Idle;
        let verdict = if flight.usable && self.wanted(flight.kind, &live) {
            match flight.kind {
                Kind::Menu { opening, owners } => Verdict::Menu { opening, owners },
                Kind::Editor { opening } => Verdict::Editor { opening },
                Kind::Auto(token) => Verdict::Auto(token),
            }
        } else {
            Verdict::Discard
        };
        let (step, dropped) = self.take_next(&live);
        Effect {
            verdict,
            step,
            finished: Some(flight.kind),
            dropped,
        }
    }

    pub fn failed(&mut self, live: Live) -> Effect {
        let Phase::Flying(flight) = self.phase else {
            return Effect::quiet(Step::Wait);
        };
        self.phase = Phase::Idle;
        let (step, dropped) = self.take_next(&live);
        Effect {
            verdict: Verdict::Discard,
            step: if matches!(step, Step::Wait) {
                Step::Unavailable
            } else {
                step
            },
            finished: Some(flight.kind),
            dropped,
        }
    }

    /// A delivered snapshot is not yet applied. Keep auto-init recoverable until
    /// capture acknowledges it, including a close between completion and apply.
    pub fn menu_armed(&mut self, opening: u64) {
        if self.menu_unarmed == Some(opening) {
            self.menu_unarmed = None;
        }
    }

    /// Record a close without changing the selection epoch. Return whether the
    /// cancelled menu read still owes this lifetime an automatic capture.
    pub fn close_menu(&mut self, opening: u64) -> bool {
        let restart_auto = self.menu_unarmed == Some(opening);
        if restart_auto {
            self.menu_unarmed = None;
        }
        if self.menu_opening == Some(opening) {
            self.menu_opening = None;
        }
        self.closed_opening = Some(opening);
        if self.next.and_then(opening_of) == Some(opening) {
            self.next = None;
        }
        if let Phase::Waiting(flight) = self.phase {
            if opening_of(flight.kind) == Some(opening) {
                self.phase = Phase::Idle;
            }
        }
        if let Phase::Flying(flight) = &mut self.phase {
            if opening_of(flight.kind) == Some(opening) {
                flight.usable = false;
            }
        }
        restart_auto
    }

    /// Scene change and visible-entry destruction. The in-flight reply is still consumed once.
    pub fn retire(&mut self) {
        self.next = None;
        self.menu_unarmed = None;
        self.menu_opening = None;
        if let Phase::Waiting(_) = self.phase {
            self.phase = Phase::Idle;
        }
        if let Phase::Flying(flight) = &mut self.phase {
            flight.usable = false;
        }
    }

    fn begin(&mut self, kind: Kind) -> Step {
        self.serial = self.serial.wrapping_add(1);
        self.phase = Phase::Flying(Flight {
            kind,
            serial: self.serial,
            sends: 1,
            usable: true,
        });
        Step::Submit(kind)
    }

    fn take_next(&mut self, live: &Live) -> (Step, Option<Kind>) {
        let Some(kind) = self.next.take() else {
            return (Step::Wait, None);
        };
        if self.wanted(kind, live) {
            (self.begin(kind), None)
        } else {
            (Step::Wait, Some(kind))
        }
    }

    fn wanted(&self, kind: Kind, live: &Live) -> bool {
        match kind {
            Kind::Menu { opening, owners } => {
                live.menu_active
                    && self.closed_opening != Some(opening)
                    && live.opening == opening
                    && live.owners == owners
            }
            Kind::Editor { opening } => {
                live.menu_active && self.closed_opening != Some(opening) && live.opening == opening
            }
            Kind::Auto(token) => live.auto == Some(token),
        }
    }
}

fn opening_of(kind: Kind) -> Option<u64> {
    match kind {
        Kind::Menu { opening, .. } | Kind::Editor { opening } => Some(opening),
        Kind::Auto(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTO: Token = Token {
        generations: [2, 4],
        opening: 8,
    };

    fn menu(opening: u64) -> Kind {
        Kind::Menu {
            opening,
            owners: [2, 4],
        }
    }

    fn live(opening: u64, active: bool) -> Live {
        Live {
            menu_active: active,
            opening,
            owners: [2, 4],
            auto: None,
        }
    }

    fn submit(effect: Effect) -> Kind {
        match effect.step {
            Step::Submit(kind) => kind,
            other => panic!("expected one submit, got {other:?}"),
        }
    }

    #[test]
    fn menu_catalog_becomes_ready_only_for_its_opening() {
        let mut requests = Requests::new();
        assert!(!requests.pending());
        let kind = submit(requests.request(menu(6)));
        assert_eq!(kind, menu(6));
        assert!(requests.pending());
        assert!(matches!(requests.request(menu(6)).step, Step::Wait));
        let effect = requests.complete(requests.serial(), live(6, true));
        assert_eq!(
            effect.verdict,
            Verdict::Menu {
                opening: 6,
                owners: [2, 4]
            }
        );
        assert!(matches!(effect.step, Step::Wait));
        assert!(!requests.pending());
    }

    #[test]
    fn closing_before_capture_is_armed_restores_auto_init_once() {
        for phase in ["flying", "waiting", "delivered", "queued"] {
            let mut requests = Requests::new();
            if phase == "queued" {
                submit(requests.request(Kind::Auto(AUTO)));
            }
            requests.request(menu(6));
            if phase == "waiting" {
                requests.rejected_full();
            } else if phase == "delivered" {
                assert!(matches!(
                    requests.complete(requests.serial(), live(6, true)).verdict,
                    Verdict::Menu { .. }
                ));
            }
            assert!(requests.close_menu(6), "{phase}");
            assert!(!requests.close_menu(6), "restart only once: {phase}");
        }

        let mut armed = Requests::new();
        submit(armed.request(menu(6)));
        armed.complete(armed.serial(), live(6, true));
        armed.menu_armed(6);
        assert!(!armed.close_menu(6));

        for kind in [Kind::Auto(AUTO), Kind::Editor { opening: 6 }] {
            let mut other = Requests::new();
            other.request(kind);
            assert!(!other.close_menu(6));
        }
        let mut retired = Requests::new();
        retired.request(menu(6));
        retired.retire();
        assert!(!retired.close_menu(6));

        let mut reopened = Requests::new();
        reopened.request(menu(6));
        reopened.request(menu(7));
        reopened.menu_armed(6); // Old delivery cannot consume a new opening's debt.
        assert!(reopened.close_menu(7));
    }

    #[test]
    fn editor_result_is_not_a_fighter_menu_result() {
        let mut requests = Requests::new();
        let kind = submit(requests.request(Kind::Editor { opening: 9 }));
        assert!(matches!(kind, Kind::Editor { opening: 9 }));
        let effect = requests.complete(requests.serial(), live(9, true));
        assert_eq!(effect.verdict, Verdict::Editor { opening: 9 });
        assert!(!matches!(
            effect.verdict,
            Verdict::Menu { .. } | Verdict::Auto(_)
        ));
    }

    #[test]
    fn fighter_menu_demand_supersedes_the_editor_catalog_for_one_opening() {
        // Quickplay order: the fighter menu asks first, the editor bridge second.
        let mut requests = Requests::new();
        submit(requests.request(menu(6)));
        let refused = requests.request(Kind::Editor { opening: 6 });
        assert!(matches!(refused.step, Step::Wait));
        assert_eq!(refused.dropped, Some(Kind::Editor { opening: 6 }));
        let effect = requests.complete(requests.serial(), live(6, true));
        assert_eq!(effect.verdict, Verdict::Menu { opening: 6, owners: [2, 4] });
        assert!(matches!(effect.step, Step::Wait)); // no second read was queued
        assert!(!requests.pending());
        let again = requests.request(Kind::Editor { opening: 6 });
        assert_eq!(again.dropped, Some(Kind::Editor { opening: 6 }));
        assert!(!requests.pending());
        // Another opening is not owned by this fighter menu.
        requests.close_menu(6);
        assert_eq!(submit(requests.request(Kind::Editor { opening: 7 })), Kind::Editor { opening: 7 });
        // Retirement releases the ownership; the flying read is still consumed once.
        let mut retired = Requests::new();
        submit(retired.request(menu(8)));
        retired.retire();
        assert_eq!(retired.request(Kind::Editor { opening: 8 }).dropped, None);

        // Reverse order: an editor read already flying for the opening may not mount.
        let mut reversed = Requests::new();
        submit(reversed.request(Kind::Editor { opening: 9 }));
        let serial = reversed.serial();
        assert!(matches!(reversed.request(menu(9)).step, Step::Wait));
        let effect = reversed.complete(serial, live(9, true));
        assert_eq!(effect.verdict, Verdict::Discard);
        assert_eq!(effect.finished, Some(Kind::Editor { opening: 9 }));
        assert_eq!(submit(Effect::quiet(effect.step)), menu(9));
        let effect = reversed.complete(reversed.serial(), live(9, true));
        assert_eq!(effect.verdict, Verdict::Menu { opening: 9, owners: [2, 4] });

        // A waiting editor read is replaced by the fighter menu demand.
        let mut waiting = Requests::new();
        submit(waiting.request(Kind::Editor { opening: 10 }));
        waiting.rejected_full();
        let replaced = waiting.request(menu(10));
        assert_eq!(replaced.dropped, Some(Kind::Editor { opening: 10 }));
        assert_eq!(submit(waiting.retry(live(10, true))), menu(10));
    }

    #[test]
    fn stale_generation_and_wrong_serial_do_not_apply() {
        let mut requests = Requests::new();
        submit(requests.request(menu(6)));
        let serial = requests.serial();
        let ignored = requests.complete(serial.wrapping_add(9), live(6, true));
        assert_eq!(ignored.verdict, Verdict::Discard);
        assert!(ignored.finished.is_none());
        assert!(requests.pending());
        let mut moved = live(6, true);
        moved.owners[1] = 5;
        let effect = requests.complete(serial, moved);
        assert_eq!(effect.verdict, Verdict::Discard);
        assert_eq!(effect.finished, Some(menu(6)));
        assert!(!requests.pending());
    }

    #[test]
    fn reopen_discards_the_old_read_and_submits_one_fresh_read() {
        let mut requests = Requests::new();
        submit(requests.request(menu(3)));
        let old = requests.serial();
        assert!(matches!(requests.request(menu(4)).step, Step::Wait));
        let effect = requests.complete(old, live(4, true));
        assert_eq!(effect.verdict, Verdict::Discard);
        assert_eq!(effect.finished, Some(menu(3)));
        assert_eq!(submit(Effect::quiet(effect.step)), menu(4));
        let effect = requests.complete(requests.serial(), live(4, true));
        assert_eq!(
            effect.verdict,
            Verdict::Menu {
                opening: 4,
                owners: [2, 4]
            }
        );
    }

    #[test]
    fn close_keeps_the_selection_epoch_and_drops_the_catalog() {
        let opening = 11u64;
        let pending_epoch = opening;
        let mut requests = Requests::new();
        submit(requests.request(menu(opening)));
        let serial = requests.serial();
        requests.close_menu(opening);
        assert_eq!(opening, 11);
        assert_eq!(pending_epoch, opening);
        let effect = requests.complete(serial, live(opening, true));
        assert_eq!(effect.verdict, Verdict::Discard);
        assert!(matches!(effect.step, Step::Wait));
        let mut waiting = Requests::new();
        submit(waiting.request(menu(opening)));
        assert!(matches!(waiting.rejected_full().step, Step::Wait));
        waiting.close_menu(opening);
        assert!(!waiting.pending());
        assert!(matches!(
            waiting.retry(live(opening, true)).step,
            Step::Wait
        ));
    }

    #[test]
    fn retired_scene_discards_the_flying_read_and_a_later_open_submits_once() {
        let mut requests = Requests::new();
        submit(requests.request(menu(5)));
        let serial = requests.serial();
        requests.retire();
        assert!(matches!(requests.request(menu(6)).step, Step::Wait));
        let effect = requests.complete(serial, live(6, true));
        assert_eq!(effect.verdict, Verdict::Discard);
        assert_eq!(submit(Effect::quiet(effect.step)), menu(6));

        let mut waiting = Requests::new();
        submit(waiting.request(menu(5)));
        waiting.rejected_full();
        waiting.retire();
        assert!(!waiting.pending());
        assert_eq!(submit(waiting.request(menu(7))), menu(7));
    }

    #[test]
    fn auto_catalog_arms_only_while_its_token_is_still_current() {
        let mut requests = Requests::new();
        assert!(matches!(
            submit(requests.request(Kind::Auto(AUTO))),
            Kind::Auto(AUTO)
        ));
        let mut current = live(AUTO.opening, false);
        current.auto = Some(AUTO);
        let effect = requests.complete(requests.serial(), current);
        assert_eq!(effect.verdict, Verdict::Auto(AUTO));

        let mut cancelled = Requests::new();
        submit(cancelled.request(Kind::Auto(AUTO)));
        let serial = cancelled.serial();
        let effect = cancelled.complete(serial, live(AUTO.opening, false));
        assert_eq!(effect.verdict, Verdict::Discard);
        assert_eq!(effect.finished, Some(Kind::Auto(AUTO)));
    }

    #[test]
    fn full_writer_queue_stops_after_four_sends_and_does_not_enqueue() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        for n in 0..4u8 {
            tx.try_send(n).unwrap();
        }
        let mut requests = Requests::new();
        let mut step = requests.request(menu(1)).step;
        let mut sends = 0usize;
        loop {
            match step {
                Step::Submit(_) => {
                    sends += 1;
                    assert!(
                        tx.try_send(9).is_err(),
                        "a full queue must not block or grow"
                    );
                    step = requests.rejected_full().step;
                }
                Step::Wait => step = requests.retry(live(1, true)).step,
                Step::Unavailable => break,
            }
        }
        assert_eq!(sends, MAX_SENDS as usize);
        assert!(!requests.pending());
        assert_eq!(rx.try_iter().count(), 4);
        assert_eq!(submit(requests.request(menu(2))), menu(2));
    }

    #[test]
    fn closed_opening_cannot_be_requested_again() {
        let mut requests = Requests::new();
        submit(requests.request(menu(4)));
        let serial = requests.serial();
        requests.close_menu(4);
        let refused = requests.request(menu(4));
        assert!(matches!(refused.step, Step::Wait));
        assert_eq!(refused.dropped, Some(menu(4)));
        let effect = requests.complete(serial, live(4, true));
        assert_eq!(effect.verdict, Verdict::Discard);
        assert!(matches!(effect.step, Step::Wait));
        assert_eq!(effect.dropped, None);
        assert!(!requests.pending());
    }

    #[test]
    fn waiting_read_is_replaced_by_one_later_demand() {
        let mut requests = Requests::new();
        submit(requests.request(Kind::Auto(AUTO)));
        assert!(matches!(requests.rejected_full().step, Step::Wait));
        let replaced = requests.request(menu(3));
        assert!(matches!(replaced.step, Step::Wait));
        assert_eq!(replaced.dropped, Some(Kind::Auto(AUTO)));
        assert_eq!(submit(requests.retry(live(3, true))), menu(3));
        let effect = requests.complete(requests.serial(), live(3, true));
        assert_eq!(
            effect.verdict,
            Verdict::Menu {
                opening: 3,
                owners: [2, 4]
            }
        );
    }
}
