//! Capture selection while a waiting-lobby CPU finishes its current native action.
//! No fighter pointers or named-macro state belong to this value-only browser.
use crate::macro_panel::{Effect as MenuEffect, LibraryRow, MacroUi, Pad, View};

const READY_TICKS: u32 = 180;

enum Selection {
    Browsing,
    Settling { id: String, ticks: u32 },
    Opening,
    Closed,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Effect { None, Open(String), Close, TimedOut, Notify(String) }

pub struct ClipBrowser { ui: MacroUi, selection: Selection }

impl ClipBrowser {
    pub fn new(rows: Vec<LibraryRow>, skipped: usize) -> Self {
        let mut ui = MacroUi::new();
        ui.open_library(rows, skipped);
        Self { ui, selection: Selection::Browsing }
    }

    pub fn pending(&self) -> bool {
        matches!(self.selection, Selection::Settling { .. } | Selection::Opening)
    }

    pub fn opening(&self) -> bool { matches!(self.selection, Selection::Opening) }

    /// Only a nonclean CPU may advance. Hold it again as soon as it is clean,
    /// including between the readiness tick and the native adapter's publication.
    pub fn release_cpu(&self, clean: bool) -> bool {
        matches!(self.selection, Selection::Settling { .. }) && !clean
    }

    pub fn view(&self) -> Option<View> {
        let mut view = match self.selection {
            Selection::Browsing => self.ui.view()?,
            Selection::Settling { .. } | Selection::Opening => View {
                title: String::new(), selected: 0,
                labels: vec![if self.opening() { "Opening capture..." }
                    else { "Waiting for CPU idle or fall..." }.into()],
                footer: "B: cancel".into(),
            },
            Selection::Closed => return None,
        };
        view.title = "Replay Captures".into();
        Some(view)
    }

    pub fn on_pad(&mut self, pad: Pad, waiting: bool) -> Effect {
        if matches!(pad, Pad::Back) {
            self.selection = Selection::Closed;
            return Effect::Close;
        }
        // Repeated Confirm and navigation cannot replace an admitted ID or renew
        // its deadline. Training continues to use immediate, validated opening.
        if !matches!(self.selection, Selection::Browsing) { return Effect::None; }
        match self.ui.on_pad(pad) {
            MenuEffect::Choose(id) if waiting => {
                self.selection = Selection::Settling { id, ticks: 0 };
                Effect::None
            }
            MenuEffect::Choose(id) => {
                self.selection = Selection::Opening;
                Effect::Open(id)
            }
            MenuEffect::Notify(error) => Effect::Notify(error),
            _ => Effect::None,
        }
    }

    /// Once per active practice layout, not per fighter or input poll. Departure
    /// wins even when CPU readiness arrives in the same tick. No request resumes
    /// itself after Cancel, timeout or an interruption.
    pub fn tick(&mut self, eligible: bool, clean: bool) -> Effect {
        // Opening is already in the native adapter (including synchronous
        // Training). Its revision/context fence, not this waiter, publishes it.
        if !matches!(self.selection, Selection::Settling { .. }) { return Effect::None; }
        if !eligible {
            self.selection = Selection::Closed;
            return Effect::Close;
        }
        let Selection::Settling { id, ticks } = &mut self.selection else { return Effect::None; };
        if clean {
            let id = std::mem::take(id);
            self.selection = Selection::Opening;
            return Effect::Open(id);
        }
        *ticks += 1;
        if *ticks >= READY_TICKS {
            self.selection = Selection::Closed;
            Effect::TimedOut
        } else { Effect::None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browser() -> ClipBrowser {
        ClipBrowser::new(vec![
            LibraryRow { id: "saved:500".into(), label: "Replay500f".into() },
            LibraryRow { id: "saved:other".into(), label: "Other capture".into() },
        ], 0)
    }

    #[test]
    fn active_waiting_cpu_can_finish_before_the_exact_capture_opens() {
        let mut browser = browser();
        assert_eq!(browser.on_pad(Pad::Confirm, true), Effect::None);
        assert!(browser.pending());
        // Counterexample: Mario is attacking, rather than an idle Training CPU.
        // The browser must release its panel stop and wait, not reopen/refreeze.
        assert!(browser.release_cpu(false));
        for _ in 0..45 {
            assert_eq!(browser.tick(true, false), Effect::None);
            assert_eq!(browser.on_pad(Pad::Confirm, true), Effect::None);
            assert_eq!(browser.on_pad(Pad::Right, true), Effect::None);
        }
        // WAIT or either admitted fall status is sampled by the native adapter.
        assert!(!browser.release_cpu(true));
        assert_eq!(browser.tick(true, true), Effect::Open("saved:500".into()));
        assert!(browser.opening());
        assert!(!browser.release_cpu(false));
        assert_eq!(browser.tick(true, true), Effect::None); // One load only.
    }

    #[test]
    fn idle_training_stays_immediate_and_waiting_can_choose_another_capture() {
        let mut training = browser();
        assert_eq!(training.on_pad(Pad::Confirm, false), Effect::Open("saved:500".into()));
        assert!(!training.release_cpu(false));
        // The waiting-only layout clock must not cancel a synchronous Training load.
        assert_eq!(training.tick(false, true), Effect::None);
        assert!(training.opening());

        let mut waiting = browser();
        assert_eq!(waiting.on_pad(Pad::Right, true), Effect::None);
        assert_eq!(waiting.on_pad(Pad::Confirm, true), Effect::None);
        assert_eq!(waiting.tick(true, true), Effect::Open("saved:other".into()));
    }

    #[test]
    fn explicit_cancel_before_or_after_readiness_cannot_open_on_return() {
        for ready in [false, true] {
            let mut browser = browser();
            browser.on_pad(Pad::Confirm, true);
            browser.tick(true, ready);
            assert_eq!(browser.on_pad(Pad::Back, true), Effect::Close);
            assert!(!browser.pending());
            assert!(!browser.opening()); // Native publication rejects this state.
            assert!(!browser.release_cpu(false));
            assert!(browser.view().is_none());
            assert_eq!(browser.tick(true, true), Effect::None);
        }
    }

    #[test]
    fn departure_or_native_ui_preempts_same_tick_readiness() {
        let mut browser = browser();
        browser.on_pad(Pad::Confirm, true);
        assert_eq!(browser.tick(false, true), Effect::Close);
        assert!(!browser.pending());
        assert!(!browser.release_cpu(false));
        assert_eq!(browser.tick(true, true), Effect::None);
    }

    #[test]
    fn repeated_confirm_cannot_extend_the_180_tick_deadline() {
        let mut browser = browser();
        browser.on_pad(Pad::Confirm, true);
        for _ in 0..179 {
            assert_eq!(browser.tick(true, false), Effect::None);
            assert_eq!(browser.on_pad(Pad::Confirm, true), Effect::None);
        }
        assert_eq!(browser.tick(true, false), Effect::TimedOut);
        assert!(!browser.pending());
        assert!(!browser.release_cpu(false));
        assert_eq!(browser.tick(true, true), Effect::None);
    }
}
