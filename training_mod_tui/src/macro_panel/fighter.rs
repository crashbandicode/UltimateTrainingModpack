//! Which fighter plays a recording's inputs: the one it was recorded with, or the
//! CPU already on stage.
//!
//! Recordings carry inputs, so any fighter can play them. When the recorded
//! fighter differs from the settled CPU the user chooses once per explicit
//! selection. This module is value-only: the runtime computes `matches` with its
//! exact readiness predicate (a same-class Mii with different exact
//! configuration is not a match) and owns every native request.

use super::{Pad, View, PAGE};

/// What a selection must do before it may play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// The recorded identity is already the settled CPU. Nothing to choose.
    Proceed,
    /// Play on the current CPU. No prompt, no CPU change.
    Keep,
    /// Ask once for this selection.
    Prompt,
}

/// `matches`: the recorded identity is already the settled CPU.
/// `switchable`: an exact recorded target exists to switch to; an older Mii file
/// without its exact Mii has none, so there is nothing to offer.
/// `prompt_enabled`: the persistent Fighter Prompt setting.
pub fn gate(prompt_enabled: bool, switchable: bool, matches: bool) -> Gate {
    if matches {
        Gate::Proceed
    } else if !prompt_enabled || !switchable {
        Gate::Keep
    } else {
        Gate::Prompt
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Safe default: inputs play on the current CPU, which is never reloaded.
    Keep,
    /// Load the recorded fighter through the existing exact-fighter route.
    Switch,
}

/// Which selection owns the prompt. The runtime routes the result by origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Named library load. The UI keeps the candidate and applies the choice.
    Named,
    /// Replay clip/track selection. The clip editor keeps the candidate.
    Clip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    None,
    Chose(Choice),
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    origin: Origin,
    revision: u64,
    recorded: String,
    current: String,
    cursor: usize,
}

const ROWS: [(&str, Choice); 2] = [
    ("Keep current fighter", Choice::Keep),
    ("Switch to recorded fighter", Choice::Switch),
];

impl Prompt {
    /// `revision` is the owning selection's revision. The caller drops a prompt
    /// whose revision is no longer current. The cursor starts on Keep.
    pub fn new(origin: Origin, revision: u64, recorded: String, current: String) -> Self {
        Self { origin, revision, recorded, current, cursor: 0 }
    }

    pub fn origin(&self) -> Origin {
        self.origin
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn choice(&self) -> Choice {
        ROWS[self.cursor].1
    }

    pub fn on_pad(&mut self, pad: Pad) -> Outcome {
        match pad {
            Pad::Left | Pad::Up => self.cursor = self.cursor.saturating_sub(1),
            Pad::Right | Pad::Down => self.cursor = (self.cursor + 1).min(ROWS.len() - 1),
            Pad::Confirm => return Outcome::Chose(self.choice()),
            Pad::Back => return Outcome::Cancelled,
            Pad::Extra => {}
        }
        Outcome::None
    }

    pub fn view(&self) -> View {
        let mut labels = vec![String::new(); PAGE];
        for (label, (text, _)) in labels.iter_mut().zip(ROWS) {
            *label = text.to_owned();
        }
        View {
            title: "Fighter Mismatch".to_owned(),
            footer: format!(
                "Recorded {} · current CPU {}\nInputs are the same either way. A chooses, B cancels.",
                self.recorded, self.current
            ),
            labels,
            selected: self.cursor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_asks_only_for_a_real_mismatch_that_can_be_switched() {
        // Same fighter: never a prompt, whatever the setting.
        assert_eq!(gate(true, true, true), Gate::Proceed);
        assert_eq!(gate(false, true, true), Gate::Proceed);
        // Mismatch with the setting on.
        assert_eq!(gate(true, true, false), Gate::Prompt);
        // Setting off keeps the current CPU without asking.
        assert_eq!(gate(false, true, false), Gate::Keep);
        // Nothing to switch to (older Mii file): keep, never prompt.
        assert_eq!(gate(true, false, false), Gate::Keep);
    }

    #[test]
    fn prompt_defaults_to_keep_and_cancel_is_distinct_from_both_choices() {
        let mut prompt = Prompt::new(Origin::Named, 7, "Kirby".into(), "Hero".into());
        assert_eq!(prompt.choice(), Choice::Keep);
        assert_eq!(prompt.view().selected, 0);
        assert_eq!(prompt.on_pad(Pad::Confirm), Outcome::Chose(Choice::Keep));
        assert_eq!(prompt.on_pad(Pad::Back), Outcome::Cancelled);
        assert_eq!(prompt.on_pad(Pad::Extra), Outcome::None);
        assert_eq!(prompt.on_pad(Pad::Down), Outcome::None);
        assert_eq!(prompt.choice(), Choice::Switch);
        assert_eq!(prompt.on_pad(Pad::Confirm), Outcome::Chose(Choice::Switch));
        // The cursor clamps; it never wraps past either row.
        prompt.on_pad(Pad::Right);
        assert_eq!(prompt.choice(), Choice::Switch);
        prompt.on_pad(Pad::Up);
        prompt.on_pad(Pad::Left);
        assert_eq!(prompt.choice(), Choice::Keep);
        assert_eq!((prompt.origin(), prompt.revision()), (Origin::Named, 7));
    }

    #[test]
    fn prompt_view_names_both_fighters_and_both_actions() {
        let view = Prompt::new(Origin::Clip, 1, "Kirby".into(), "Hero".into()).view();
        assert_eq!(view.labels.len(), PAGE);
        assert_eq!(view.labels[0], "Keep current fighter");
        assert_eq!(view.labels[1], "Switch to recorded fighter");
        assert!(view.labels[2..].iter().all(String::is_empty));
        assert!(view.footer.contains("Kirby") && view.footer.contains("Hero"));
        assert!(view.footer.contains("A chooses") && view.footer.contains("B cancels"));
    }
}
