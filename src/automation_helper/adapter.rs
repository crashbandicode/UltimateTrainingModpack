//! Connects the test-helper request, results flow and popup tracker.
//! Native codes are not `control::Decision`: rematch 1 is No and 2 is Yes.
//! Save confirm 2 is the consumer's proceed code. This module does not write
//! popup fields or call the game.

use super::control::{Decision, Progress, Request};
use super::flow::{Effect, Failure, Flow, Screen};
use super::popup::{Event, Kind, Popup, Snapshot, Source};
use super::pulse::{Action, Command, MASK_A, MASK_Y};

pub const CALLER_REMATCH: u64 = 0x147fb6c;
pub const CALLER_SAVE: u64 = 0x1d5b670;
const CALLER_SAVE_SUCCESS: u64 = 0x1d5a870;
pub const MSG_REMATCH: u64 = 0x58ffff1cb2b11288;
pub const MSG_SAVE: u64 = 0x58ffff145ea6f7be;
const MSG_SAVE_SUCCESS: u64 = 0x58ffff1e1eed5764;
pub const REMATCH_INPUT_TICKS: u32 = 36;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note {
    pub event: &'static str,
    pub value: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Outcome {
    pub notes: [Note; 6],
    pub note_count: u8,
    pub command: Command,
    pub publish: Option<Progress>,
}

#[derive(Clone, Copy, Debug)]
pub struct OpenView {
    pub owner: u64,
    pub caller: u64,
    pub message: u64,
    pub word_160: u32,
    pub word_164: u32,
    pub word_168: u32,
    pub word_16c: u32,
    pub word_170: u32,
    pub data_ok: bool,
    pub save_status: Option<i32>,
}

#[derive(Clone, Copy, Debug)]
pub struct CardsView {
    pub phase: i32,
    pub pending: i32,
    pub save_offered: u8,
    // R+0x928 chooses the native input route; both routes can open Save.
    pub input_route: u32,
    pub finished: u8,
    pub save_status: i32,
    pub modal_mask: u32,
    pub popup_idle: bool,
    pub fields_ok: bool,
}

#[derive(Clone, Copy)]
pub struct PopupTick {
    pub owner: u64,
    pub state: Option<Snapshot>,
    pub save_status: Option<i32>,
}

#[derive(Clone, Copy)]
pub enum Observation {
    Begin { seq: u64, save_status: Option<i32> },
    End,
    Open(OpenView),
    Popup(PopupTick),
    Cards(CardsView),
    // Tests can drive a transition alone. Native result and popup hooks carry it.
    #[cfg(test)]
    SaveStatus(i32),
}

pub struct Adapter {
    epoch: Option<u64>,
    next_epoch: u64,
    seq: Option<u64>,
    scene_ended: bool,
    flow: Option<Flow>,
    generation: Option<u64>,
    terminal: bool,
    popup: Popup,
    open_owner: Option<u64>,
    open_ticks: u32,
    prompt: Option<Snapshot>,
    rematch_ready: bool,
    save_ready: bool,
    dismiss_owner: Option<u64>,
    dismiss_ready: bool,
    save_status: Option<i32>,
    saw_1: bool,
    saw_2: bool,
    saw_3: bool,
    save_done: bool,
    complete_edge: bool,
    fail_edge: bool,
    refused_default: bool,
    refused_logged: u32,
    y_armed: bool,
    confirm_armed: bool,
    rematch_armed: bool,
    advance_waiting: bool,
    advance_armed: bool,
    pending_choice: Option<Decision>,
    accept: Option<Decision>,
    saving_edge: bool,
    cards: Option<CardsView>,
    serial: u32,
    command: Command,
    notes: [Note; 6],
    note_count: u8,
    publish: Option<Progress>,
}

impl Adapter {
    pub const fn new() -> Self {
        Self {
            epoch: None,
            next_epoch: 0,
            seq: None,
            scene_ended: false,
            flow: None,
            generation: None,
            terminal: false,
            popup: Popup::new(),
            open_owner: None,
            open_ticks: 0,
            prompt: None,
            rematch_ready: false,
            save_ready: false,
            dismiss_owner: None,
            dismiss_ready: false,
            save_status: None,
            saw_1: false,
            saw_2: false,
            saw_3: false,
            save_done: false,
            complete_edge: false,
            fail_edge: false,
            refused_default: false,
            refused_logged: 0xffff_ffff,
            y_armed: false,
            confirm_armed: false,
            rematch_armed: false,
            advance_waiting: false,
            advance_armed: false,
            pending_choice: None,
            accept: None,
            saving_edge: false,
            cards: None,
            serial: 0,
            command: Command::idle(),
            notes: [Note { event: "", value: 0 }; 6],
            note_count: 0,
            publish: None,
        }
    }

    pub fn handle(&mut self, now_ms: u64, request: Option<Request>, obs: Observation) -> Outcome {
        self.note_count = 0;
        self.publish = None;
        self.observe_native(obs);
        self.settle(now_ms, request);
        self.outcome()
    }

    fn observe_native(&mut self, obs: Observation) {
        match obs {
            Observation::Begin { seq, save_status } => self.begin_results(seq, save_status),
            Observation::End => {
                if self.epoch.is_some() {
                    self.finish_epoch();
                }
            }
            Observation::Open(view) => self.on_open(view),
            Observation::Popup(tick) => self.on_popup(tick),
            Observation::Cards(view) => {
                if view.fields_ok {
                    self.note_status(view.save_status);
                    self.cards = Some(view);
                }
            }
            #[cfg(test)]
            Observation::SaveStatus(status) => self.note_status(status),
        }
    }

    fn begin_results(&mut self, seq: u64, save_status: Option<i32>) {
        if self.epoch.is_some() && self.seq == Some(seq) {
            if let Some(status) = save_status {
                self.note_status(status);
            }
            return;
        }
        if self.epoch.is_some() {
            self.finish_epoch();
        }
        self.next_epoch = self.next_epoch.wrapping_add(1);
        if self.next_epoch == 0 {
            self.next_epoch = 1;
        }
        let epoch = self.next_epoch;
        self.epoch = Some(epoch);
        self.seq = Some(seq);
        self.popup.begin(epoch);
        self.reset_prompt();
        self.reset_save(save_status);
        self.note("epoch_begin", epoch);
    }

    fn finish_epoch(&mut self) {
        self.popup.end();
        self.epoch = None;
        self.seq = None;
        self.reset_prompt();
        self.scene_ended = true;
        self.disarm();
        self.note("epoch_end", 0);
    }

    fn reset_prompt(&mut self) {
        self.open_owner = None;
        self.open_ticks = 0;
        self.prompt = None;
        self.rematch_ready = false;
        self.save_ready = false;
        self.dismiss_owner = None;
        self.dismiss_ready = false;
        self.pending_choice = None;
        self.accept = None;
    }

    fn reset_save(&mut self, status: Option<i32>) {
        self.save_status = status;
        self.saw_1 = false;
        self.saw_2 = false;
        self.saw_3 = false;
        self.save_done = false;
        self.complete_edge = false;
        self.fail_edge = false;
        self.refused_default = false;
        self.refused_logged = 0xffff_ffff;
        self.saving_edge = false;
        self.cards = None;
        self.y_armed = false;
        self.confirm_armed = false;
        self.advance_waiting = false;
        self.advance_armed = false;
    }

    fn on_open(&mut self, view: OpenView) {
        let Some(epoch) = self.epoch else {
            self.note("open_without_epoch", view.caller);
            return;
        };
        if view.owner == 0 {
            self.note("open_rejected", 0);
            return;
        }
        if let Some(status) = view.save_status {
            self.note_status(status);
        }
        // Any new prompt ends the previous prompt's delayed confirmation.
        // confirm_armed remains the authorization for the save-status chain.
        if self.command.action == Action::SaveYes {
            self.disarm();
        }
        self.open_owner = Some(view.owner);
        self.open_ticks = 0;
        self.prompt = None;
        self.rematch_ready = false;
        self.save_ready = false;
        self.pending_choice = None;
        self.dismiss_owner = None;
        self.dismiss_ready = false;
        let owner = view.owner as usize;
        if !view.data_ok {
            self.popup.opened(epoch, owner, None);
            self.note("open_data_unreadable", view.caller);
            return;
        }
        if view.caller == CALLER_REMATCH && view.message == MSG_REMATCH {
            let shape = view.word_164 == 0 && view.word_16c == 0 && view.word_170 == 2;
            if shape {
                self.popup.opened(epoch, owner, Some(Kind::Rematch));
                self.note("rematch_open", view.message);
                if view.word_168 != 0 {
                    self.note("rematch_word_168", u64::from(view.word_168));
                }
                return;
            }
            self.popup.opened(epoch, owner, None);
            self.note(
                "rematch_shape_unsupported",
                u64::from(view.word_164) | (u64::from(view.word_170) << 32),
            );
            return;
        }
        if view.caller == CALLER_SAVE && view.message == MSG_SAVE {
            self.note("save_open_default", u64::from(view.word_16c));
            self.note("save_open_focus", u64::from(view.word_160));
            if view.word_16c == 0 && view.word_160 == 1 && view.word_170 == 2 {
                self.popup.opened(epoch, owner, Some(Kind::SaveConfirmation));
                return;
            }
            self.popup.opened(epoch, owner, None);
            self.refused_default = true;
            self.refused_logged = view.word_16c;
            self.note("save_default_unsupported", u64::from(view.word_16c));
            return;
        }
        self.popup.opened(epoch, owner, None);
        // The native success path opens its popup before storing status 3.
        // Retain this exact popup while still saving; pick() waits for status 3
        // and a ready popup before issuing its dismissal press.
        let success_opening = self.save_status == Some(2)
            && view.caller == CALLER_SAVE_SUCCESS
            && view.message == MSG_SAVE_SUCCESS;
        if self.saw_2 && (matches!(self.save_status, Some(3) | Some(4)) || success_opening) {
            self.dismiss_owner = Some(view.owner);
            self.dismiss_ready = false;
            self.note("result_popup_open", self.save_status.unwrap_or(-1) as u32 as u64);
        } else {
            self.note("popup_unrelated", view.caller);
        }
    }

    fn on_popup(&mut self, tick: PopupTick) {
        if self.open_owner == Some(tick.owner) {
            if let Some(status) = tick.save_status {
                self.note_status(status);
            }
            self.open_ticks = self.open_ticks.saturating_add(1);
            self.prompt = tick.state;
            if tick.state.is_some_and(prompt_closed) && self.accept.is_none() {
                self.pending_choice = None;
                if self.rematch_armed {
                    self.rematch_armed = false;
                    self.disarm();
                    self.note("rematch_prompt_lost", 0);
                }
                if self.command.action == Action::SaveYes {
                    self.disarm();
                    self.note("save_prompt_lost", 0);
                }
            }
        }
        if self.dismiss_owner == Some(tick.owner) {
            if let Some(state) = tick.state {
                if dismiss_ready(&state) {
                    self.dismiss_ready = true;
                }
            }
        }
        let (epoch, owner) = match (self.epoch, self.open_owner) {
            (Some(epoch), Some(owner)) if owner == tick.owner => (epoch, owner),
            _ => return,
        };
        let Some(state) = tick.state else {
            return;
        };
        let Some(event) = self.popup.observe(epoch, owner as usize, state) else {
            return;
        };
        match event {
            Event::Ready(Kind::Rematch) => {
                self.rematch_ready = true;
                self.note("rematch_ready", u64::from(self.open_ticks));
            }
            Event::Ready(Kind::SaveConfirmation) => {
                if self.refused_default {
                    self.note("save_default_unsupported", u64::from(self.refused_logged));
                } else {
                    self.save_ready = true;
                    self.note("save_ready", u64::from(self.open_ticks));
                }
            }
            Event::Decided { kind, result, source } => self.on_decided(kind, result, source),
        }
    }

    fn on_decided(&mut self, kind: Kind, result: u32, source: Source) {
        self.pending_choice = None;
        self.rematch_ready = false;
        self.save_ready = false;
        self.note("decision", decision_token(kind, result, source));
        let rematch_was_armed = self.rematch_armed;
        if kind == Kind::SaveConfirmation && self.command.action == Action::SaveYes {
            self.disarm();
        }
        if kind == Kind::Rematch && rematch_was_armed {
            // Every decision ends this prompt's input ownership, including a
            // forced close during the stick/neutral portion of the No script.
            self.rematch_armed = false;
            self.disarm();
        }
        if source != Source::Normal {
            self.note("decision_ignored", u64::from(result));
            return;
        }
        match kind {
            Kind::Rematch => match rematch_decision(result) {
                Some(choice) if rematch_was_armed => self.accept = Some(choice),
                Some(_) => self.note("decision_before_pulse", u64::from(result)),
                None => self.note("decision_unmapped", u64::from(result)),
            },
            Kind::SaveConfirmation => {
                if result == 2 {
                    self.note("save_confirm_yes", 2);
                } else {
                    self.note("save_confirm_not_yes", u64::from(result));
                }
            }
        }
    }

    fn note_status(&mut self, status: i32) {
        if self.save_status == Some(status) {
            return;
        }
        let prev = self.save_status;
        self.save_status = Some(status);
        if self.refused_default && matches!(status, 2 | 3 | 5) {
            self.fail_edge = true;
            self.note("save_untrusted", status as u32 as u64);
            return;
        }
        match (prev, status) {
            (_, 1) if self.y_armed => self.saw_1 = true,
            (Some(1), 2) if self.confirm_armed && self.saw_1 => {
                self.saw_2 = true;
                self.saving_edge = true;
            }
            (Some(1), 2) => {
                self.fail_edge = true;
                self.note("save_not_ours", 2);
            }
            (Some(1), 0) if self.saw_1 => {
                self.fail_edge = true;
                self.note("save_declined", 0);
            }
            (Some(2), 3) if self.saw_2 => self.saw_3 = true,
            (Some(2), 4) | (Some(3), 4) => {
                self.fail_edge = true;
                self.note("save_failed_status", 4);
            }
            (Some(3), 5) if self.saw_3 && !self.save_done => {
                self.save_done = true;
                self.complete_edge = true;
                self.note("save_completed", 5);
            }
            _ => {}
        }
    }

    fn settle(&mut self, now_ms: u64, request: Option<Request>) {
        let Some(request) = request else {
            if self.command.generation != 0 || self.command.action != Action::Idle {
                let generation = self.generation.unwrap_or(0);
                self.disarm();
                self.note("cancelled", generation);
            }
            return;
        };
        self.bind(now_ms, request);
        if self.scene_ended {
            self.scene_ended = false;
            if !self.terminal {
                if let Some(flow) = self.flow.as_mut() {
                    let effect = flow.observe(now_ms, None, Screen::Waiting);
                    self.consume(effect, request);
                }
            }
        }
        if self.terminal {
            return;
        }
        let Some(epoch) = self.epoch else {
            if let Some(flow) = self.flow.as_mut() {
                let effect = flow.observe(now_ms, None, Screen::Waiting);
                self.consume(effect, request);
            }
            return;
        };
        let screen = self.pick(request);
        let effect = self.flow.as_mut().unwrap().observe(now_ms, Some(epoch), screen);
        if effect != Effect::None {
            self.clear_edge(screen);
        }
        self.consume(effect, request);
        if !self.terminal {
            self.try_choice();
            self.try_advance();
            if self.saving_edge && self.publish.is_none() {
                self.saving_edge = false;
                self.publish = Some(Progress::Saving);
            }
        }
    }

    fn bind(&mut self, now_ms: u64, request: Request) {
        if self.generation == Some(request.generation) && self.flow.is_some() {
            return;
        }
        self.generation = Some(request.generation);
        self.flow = Some(Flow::new(request, now_ms));
        self.terminal = false;
        self.y_armed = false;
        self.confirm_armed = false;
        self.rematch_armed = false;
        self.advance_waiting = false;
        self.advance_armed = false;
        self.pending_choice = None;
        self.accept = None;
        self.complete_edge = false;
        self.fail_edge = false;
        self.disarm();
    }

    fn pick(&self, request: Request) -> Screen {
        if let Some(decision) = self.accept {
            return Screen::Accepted(decision);
        }
        if self.fail_edge {
            return Screen::SaveFailed;
        }
        // Status 5 can arrive before the success popup is dismissed. Close that
        // popup first; SavedAndClosed is only meaningful from the flow's Close stage.
        if self.dismiss_ready && self.saw_3 && matches!(self.save_status, Some(3) | Some(5)) {
            return Screen::SaveSuccess;
        }
        if self.complete_edge {
            return Screen::SavedAndClosed;
        }
        if self.save_ready && self.y_armed && !self.refused_default {
            return Screen::SaveConfirm;
        }
        if self.rematch_ready {
            return Screen::Rematch;
        }
        if self.cards_ready_for(request) {
            return Screen::Cards;
        }
        if self.saw_2 && self.save_status == Some(2) {
            return Screen::Saving;
        }
        Screen::Waiting
    }

    fn cards_ready_for(&self, request: Request) -> bool {
        let Some(cards) = self.cards else {
            return false;
        };
        if !cards.fields_ok || cards.phase != 2 || cards.finished != 0 || !cards.popup_idle {
            return false;
        }
        if cards.modal_mask > 1 {
            return false;
        }
        if request.save_replay {
            cards.pending == 2
                && cards.save_offered == 1
                && self.save_status == Some(0)
                && !self.refused_default
        } else {
            self.save_status == Some(0)
        }
    }

    fn clear_edge(&mut self, screen: Screen) {
        match screen {
            Screen::SaveFailed => self.fail_edge = false,
            Screen::SavedAndClosed => self.complete_edge = false,
            Screen::Accepted(_) => self.accept = None,
            _ => {}
        }
    }

    fn consume(&mut self, effect: Effect, request: Request) {
        match effect {
            Effect::None => {}
            Effect::AdvanceIntro => self.note("advance_intro_unsupported", 0),
            Effect::RequestSave => {
                if self.refused_default {
                    self.note("save_default_unsupported", u64::from(self.refused_logged));
                } else {
                    if let Some(cards) = self.cards {
                        self.note("cards_input_route", u64::from(cards.input_route));
                    }
                    self.arm(Action::ButtonY);
                    self.y_armed = true;
                    self.note("pulse_armed", u64::from(MASK_Y));
                }
            }
            Effect::ConfirmSave => {
                self.save_ready = false;
                self.arm(Action::SaveYes);
                self.confirm_armed = true;
                self.note("pulse_armed", u64::from(MASK_A));
            }
            Effect::CloseSave => {
                // Dismissal is once per success popup. Further ticks, including
                // reuse of its singleton for Rematch, cannot re-arm this latch.
                self.dismiss_owner = None;
                self.dismiss_ready = false;
                self.arm(Action::ButtonA);
                self.note("pulse_armed", u64::from(MASK_A));
            }
            Effect::AdvanceResults => {
                if request.save_replay {
                    self.advance_waiting = true;
                } else {
                    self.arm(Action::ButtonA);
                    self.advance_armed = true;
                    self.note("pulse_armed", u64::from(MASK_A));
                }
            }
            Effect::Choose(choice) => {
                self.rematch_ready = false;
                self.pending_choice = Some(choice);
                self.note("choice_latched", decision_code(choice));
            }
            Effect::Complete => {
                self.terminal = true;
                self.disarm();
                self.publish = Some(Progress::Done);
                self.note("completed", request.generation);
            }
            Effect::Failed(why) => {
                self.terminal = true;
                self.disarm();
                self.publish = Some(Progress::Failed);
                self.note(failure_name(why), 0);
            }
        }
    }

    fn try_choice(&mut self) {
        if self.rematch_armed || self.pending_choice.is_none() {
            return;
        }
        if self.open_ticks < REMATCH_INPUT_TICKS || !self.prompt.is_some_and(prompt_accepts_input) {
            return;
        }
        let choice = self.pending_choice.unwrap();
        match choice {
            Decision::No => {
                self.arm(Action::RematchNo);
                self.note("pulse_armed", u64::from(super::pulse::MASK_L_LEFT));
            }
            Decision::Yes => {
                self.arm(Action::RematchYes);
                self.note("yes2_unqualified", 2);
                self.note("pulse_armed", u64::from(MASK_A));
            }
            Decision::Manual => return,
        }
        self.rematch_armed = true;
        if self.publish.is_none() {
            self.publish = Some(Progress::Rematch);
        }
    }

    fn try_advance(&mut self) {
        if !self.advance_waiting || self.advance_armed || self.save_status != Some(5) {
            return;
        }
        let Some(cards) = self.cards else {
            return;
        };
        if cards.phase != 2
            || cards.finished != 0
            || cards.save_offered != 0
            || cards.modal_mask > 1
            || !cards.popup_idle
            || !cards.fields_ok
        {
            return;
        }
        self.arm(Action::ButtonA);
        self.advance_armed = true;
        self.advance_waiting = false;
        self.note("pulse_armed", u64::from(MASK_A));
    }

    fn arm(&mut self, action: Action) {
        self.serial = self.serial.wrapping_add(1).max(1);
        self.command = Command {
            generation: self.generation.unwrap_or(0),
            epoch: self.epoch.unwrap_or(0),
            serial: self.serial,
            action,
        };
    }

    fn disarm(&mut self) {
        if self.command.action == Action::Idle && self.command.generation == 0 {
            return;
        }
        self.serial = self.serial.wrapping_add(1).max(1);
        self.command = Command {
            generation: 0,
            epoch: 0,
            serial: self.serial,
            action: Action::Idle,
        };
    }

    fn note(&mut self, event: &'static str, value: u64) {
        if usize::from(self.note_count) >= self.notes.len() {
            return;
        }
        self.notes[usize::from(self.note_count)] = Note { event, value };
        self.note_count += 1;
    }

    fn outcome(&self) -> Outcome {
        Outcome {
            notes: self.notes,
            note_count: self.note_count,
            command: self.command,
            publish: self.publish,
        }
    }
}

/// Entry-hook admission. The native open bails without writing when a popup
/// is already shown or an open is already pending. Those reads are the values
/// at the hook, before the open body runs.
pub fn open_gate_admits(phase: i32, opening: u8) -> bool {
    phase == 0 && opening == 0
}

fn prompt_accepts_input(state: Snapshot) -> bool {
    state.phase == 3 && state.pending == 3 && !state.decided && !state.opening
}

fn prompt_closed(state: Snapshot) -> bool {
    state.phase == 0 && state.pending == 0 && !state.opening
}

pub fn rematch_decision(result: u32) -> Option<Decision> {
    match result {
        1 => Some(Decision::No),
        2 => Some(Decision::Yes),
        _ => None,
    }
}

fn decision_code(choice: Decision) -> u64 {
    match choice {
        Decision::Manual => 0,
        Decision::Yes => 1,
        Decision::No => 2,
    }
}

fn decision_token(kind: Kind, result: u32, source: Source) -> u64 {
    let kind: u64 = match kind {
        Kind::SaveConfirmation => 1,
        Kind::Rematch => 2,
    };
    let source: u64 = match source {
        Source::Normal => 1,
        Source::Forced => 2,
        Source::Late => 3,
    };
    kind << 16 | source << 8 | u64::from(result)
}

fn failure_name(why: Failure) -> &'static str {
    match why {
        Failure::TimedOut => "failed_timeout",
        Failure::SceneChanged => "failed_scene",
        Failure::SaveFailed => "failed_save",
        Failure::SaveSkipped => "failed_save_skipped",
        Failure::WrongDecision => "failed_wrong_decision",
    }
}

fn dismiss_ready(state: &Snapshot) -> bool {
    state.phase == 3 && state.pending == 3 && state.age >= 1 && !state.decided && !state.opening
}

#[cfg(test)]
mod tests {
    use super::super::control::Controls;
    use super::super::pulse::{Action as PulseAction, Record, Scheduler, MASK_Y};
    use super::*;

    fn request(save: bool, rematch: Decision) -> Request {
        Request { generation: 0x100, save_replay: save, rematch }
    }

    fn notes(out: &Outcome) -> Vec<&'static str> {
        out.notes[..usize::from(out.note_count)].iter().map(|n| n.event).collect()
    }

    fn ready(kind_phase: u32) -> Snapshot {
        Snapshot {
            phase: kind_phase,
            pending: kind_phase,
            previous: 7,
            age: 1,
            result: 0,
            decided: false,
            opening: false,
        }
    }

    fn cards_s0() -> CardsView {
        CardsView {
            phase: 2,
            pending: 2,
            save_offered: 1,
            input_route: 0,
            finished: 0,
            save_status: 0,
            modal_mask: 1,
            popup_idle: true,
            fields_ok: true,
        }
    }

    fn open_save(default: u32) -> OpenView {
        OpenView {
            owner: 10,
            caller: CALLER_SAVE,
            message: MSG_SAVE,
            word_160: 1,
            word_164: 0,
            word_168: 0,
            word_16c: default,
            word_170: 2,
            data_ok: true, save_status: None,
        }
    }

    fn open_rematch() -> OpenView {
        OpenView {
            owner: 10,
            caller: CALLER_REMATCH,
            message: MSG_REMATCH,
            word_160: 2,
            word_164: 0,
            word_168: 0,
            word_16c: 0,
            word_170: 2,
            data_ok: true, save_status: None,
        }
    }

    fn blank() -> Record {
        Record { held: 0, pressed: 0, released: 0, pad: 0, left: [0.0; 4], right: [0.0; 4] }
    }

    #[test]
    fn one_save_command_pulses_y_once_and_cancel_releases_it() {
        let mut adapter = Adapter::new();
        let req = request(true, Decision::Manual);
        adapter.handle(1, Some(req), Observation::Begin { seq: 1, save_status: Some(0) });
        let armed = adapter.handle(2, Some(req), Observation::Cards(cards_s0()));
        assert_eq!(armed.command.action, PulseAction::ButtonY);
        let mut completed = Scheduler::new();
        let mut edges = 0;
        for _ in 0..8 {
            let mut record = blank();
            completed.step(&mut record, Some(armed.command));
            if record.pressed == MASK_Y {
                edges += 1;
            }
        }
        assert_eq!(edges, 1);
        let mut sched = Scheduler::new();
        let mut record = blank();
        sched.step(&mut record, Some(armed.command));
        assert_eq!(record.pressed, MASK_Y);
        record = blank();
        sched.step(&mut record, Some(armed.command));
        assert_eq!(record.held, MASK_Y);
        let cancelled = adapter.handle(3, None, Observation::SaveStatus(0));
        assert_eq!(cancelled.command.action, PulseAction::Idle);
        record = blank();
        sched.step(&mut record, Some(cancelled.command));
        assert_eq!(record.released, MASK_Y);
        assert_eq!(record.held, 0);
        record = blank();
        sched.step(&mut record, Some(cancelled.command));
        assert_eq!(record.released, 0);
    }

    // The captured Quickplay cards in results-native48-runtime01/cards-gate04
    // use route 1 at R+0x928. Native routes both zero and nonzero values to
    // 1d5b590; the nonzero route is not an outstanding save request.
    #[test]
    fn quickplay_port_routed_cards_can_request_save() {
        let mut adapter = Adapter::new();
        let req = request(true, Decision::Yes);
        adapter.handle(1, Some(req), Observation::Begin { seq: 1, save_status: Some(0) });
        let cards = CardsView { input_route: 1, ..cards_s0() };
        let armed = adapter.handle(2, Some(req), Observation::Cards(cards));
        assert_eq!(armed.command.action, Action::ButtonY);
    }

    // Actual Save prompt in results-native49-runtime01/save-focus02 starts on
    // No. Right, neutral, A selects native code 2 and creates a replay file.
    #[test]
    fn save_confirmation_selects_yes_before_pressing_a() {
        let mut adapter = Adapter::new();
        let req = request(true, Decision::Yes);
        adapter.handle(1, Some(req), Observation::Begin { seq: 1, save_status: Some(0) });
        adapter.handle(2, Some(req), Observation::Cards(cards_s0()));
        adapter.handle(3, Some(req), Observation::Open(open_save(0)));
        adapter.handle(4, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready(1)) }));
        let confirm = adapter.handle(5, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready(3)) }));
        let mut scheduler = Scheduler::new();
        let mut yes_selected = false;
        let mut a_edges = 0;
        for _ in 0..50 {
            let mut record = blank();
            scheduler.step(&mut record, Some(confirm.command));
            if record.left[0] > 0.5 { yes_selected = true; }
            if record.pressed & MASK_A != 0 {
                assert!(yes_selected, "A confirmed the captured default No choice");
                a_edges += 1;
            }
        }
        assert_eq!(a_edges, 1);
    }

    #[test]
    fn forced_save_close_cancels_confirmation_input() {
        let mut adapter = Adapter::new();
        let req = request(true, Decision::Yes);
        adapter.handle(1, Some(req), Observation::Begin { seq: 1, save_status: Some(0) });
        adapter.handle(2, Some(req), Observation::Cards(cards_s0()));
        adapter.handle(3, Some(req), Observation::Open(open_save(0)));
        adapter.handle(4, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready(1)) }));
        adapter.handle(5, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready(3)) }));
        let forced = Snapshot { phase: 5, pending: 6, result: 1, ..ready(3) };
        let out = adapter.handle(6, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(forced) }));
        assert_eq!(out.command.action, Action::Idle, "save input survived a forced prompt close");
    }

    #[test]
    fn open_gate_requires_an_idle_popup() {
        assert!(open_gate_admits(0, 0));
        assert!(!open_gate_admits(3, 0));
        assert!(!open_gate_admits(0, 1));
        assert!(!open_gate_admits(-1, 0));
    }

    #[test]
    fn native_codes_map_explicitly_and_do_not_follow_decision_discriminants() {
        assert_eq!(rematch_decision(1), Some(Decision::No));
        assert_eq!(rematch_decision(2), Some(Decision::Yes));
        assert_eq!(rematch_decision(0), None);
        assert_eq!(rematch_decision(3), None);
        assert_ne!(Decision::Yes as u8, 2);
        assert_ne!(Decision::No as u8, 1);
    }

    #[test]
    fn save_chain_completes_once_and_a_sticky_five_does_not() {
        let mut a = Adapter::new();
        let req = request(true, Decision::No);
        let begin = a.handle(1, Some(req), Observation::Begin { seq: 50, save_status: Some(0) });
        assert!(notes(&begin).contains(&"epoch_begin"));
        let armed = a.handle(2, Some(req), Observation::Cards(cards_s0()));
        assert_eq!(armed.command.action, Action::ButtonY);
        assert!(notes(&armed).contains(&"pulse_armed"));
        a.handle(3, Some(req), Observation::Open(open_save(0)));
        let mut state = ready(1);
        a.handle(4, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(state) }));
        state = ready(3);
        let confirm = a.handle(5, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(state) }));
        assert_eq!(confirm.command.action, Action::SaveYes);
        for status in [1, 2, 3] {
            a.handle(6, Some(req), Observation::SaveStatus(status));
        }
        let dismiss = OpenView {
            owner: 10,
            caller: 0x11,
            message: 0x22,
            word_160: 0,
            word_164: 0,
            word_168: 0,
            word_16c: 0,
            word_170: 0,
            data_ok: true, save_status: None,
        };
        a.handle(7, Some(req), Observation::Open(dismiss));
        let mut tick = ready(1);
        a.handle(8, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(tick) }));
        tick = ready(3);
        let success = a.handle(9, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(tick) }));
        assert_eq!(success.command.action, Action::ButtonA);
        let done = a.handle(10, Some(req), Observation::SaveStatus(5));
        assert!(notes(&done).contains(&"save_completed"));
        assert_ne!(done.publish, Some(Progress::Done));
        let again = a.handle(11, Some(req), Observation::SaveStatus(5));
        assert!(!notes(&again).contains(&"save_completed"));
        assert_ne!(again.publish, Some(Progress::Done));

        let mut sticky = Adapter::new();
        sticky.handle(1, Some(req), Observation::Begin { seq: 9, save_status: Some(5) });
        let held = sticky.handle(2, Some(req), Observation::SaveStatus(5));
        assert!(!notes(&held).contains(&"save_completed"));
        assert_ne!(held.publish, Some(Progress::Done));
    }

    #[test]
    fn success_popup_opens_before_native_save_status_becomes_three() {
        let mut a = Adapter::new();
        let req = request(true, Decision::Manual);
        a.handle(1, Some(req), Observation::Begin { seq: 71, save_status: Some(0) });
        a.handle(2, Some(req), Observation::Cards(cards_s0()));
        a.handle(3, Some(req), Observation::Open(open_save(0)));
        a.handle(4, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready(1)) }));
        a.handle(5, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready(3)) }));
        a.handle(6, Some(req), Observation::SaveStatus(1));
        a.handle(7, Some(req), Observation::SaveStatus(2));
        // Native 0x1d5a86c opens this popup BEFORE 0x1d5a870 stores status 3.
        let opened = a.handle(8, Some(req), Observation::Open(OpenView {
            owner: 10, caller: 0x1d5a870, message: 0x58ffff1e1eed5764,
            word_160: 0, word_164: 0, word_168: 0, word_16c: 0,
            word_170: 0, data_ok: true, save_status: None,
        }));
        assert_eq!(opened.command.action, Action::Idle);
        let saving = a.handle(9, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready(3)) }));
        assert_eq!(saving.command.action, Action::Idle);
        let success = a.handle(10, Some(req), Observation::SaveStatus(3));
        assert_eq!(success.command.action, Action::ButtonA);
        assert_ne!(success.publish, Some(Progress::Done));
        let done = a.handle(11, Some(req), Observation::SaveStatus(5));
        assert_eq!(done.publish, Some(Progress::Done));
    }

    // The result-card callback does not run during native save phases 3–6.
    // Reproduce the captured flow using only popup-hook status observations.
    #[test]
    fn popup_hooks_track_the_save_without_result_tick_samples() {
        let mut a = Adapter::new();
        let req = request(true, Decision::Manual);
        a.handle(1, Some(req), Observation::Begin { seq: 72, save_status: Some(0) });
        a.handle(2, Some(req), Observation::Cards(cards_s0()));
        a.handle(3, Some(req), Observation::Open(OpenView {
            save_status: Some(0), ..open_save(0)
        }));
        a.handle(4, Some(req), Observation::Popup(PopupTick {
            owner: 10, state: Some(ready(1)), save_status: Some(1),
        }));
        let confirm = a.handle(5, Some(req), Observation::Popup(PopupTick {
            owner: 10, state: Some(ready(3)), save_status: Some(1),
        }));
        assert_eq!(confirm.command.action, Action::SaveYes);
        a.handle(6, Some(req), Observation::Popup(PopupTick {
            owner: 10, state: Some(Snapshot { pending: 6, decided: true, result: 2, ..ready(3) }),
            save_status: Some(1),
        }));
        let opened = a.handle(7, Some(req), Observation::Open(OpenView {
            caller: CALLER_SAVE_SUCCESS, message: MSG_SAVE_SUCCESS,
            save_status: Some(2), ..open_save(0)
        }));
        assert!(notes(&opened).contains(&"result_popup_open"));
        assert_eq!(opened.command.action, Action::Idle);
        let success = a.handle(8, Some(req), Observation::Popup(PopupTick {
            owner: 10, state: Some(ready(3)), save_status: Some(3),
        }));
        assert_eq!(success.command.action, Action::ButtonA);
        assert_ne!(success.publish, Some(Progress::Done));
        let done = a.handle(9, Some(req), Observation::Cards(CardsView {
            save_status: 5, ..cards_s0()
        }));
        assert!(notes(&done).contains(&"save_completed"));
        assert_eq!(done.publish, Some(Progress::Done));
    }

    // Actual combined flow: saving succeeds, then the popup singleton is reused
    // for Rematch. A lingering success observation must not hide that prompt.
    #[test]
    fn completed_save_does_not_mask_the_next_rematch_prompt() {
        let mut a = Adapter::new();
        let req = request(true, Decision::Yes);
        a.handle(1, Some(req), Observation::Begin { seq: 73, save_status: Some(0) });
        a.handle(2, Some(req), Observation::Cards(cards_s0()));
        a.handle(3, Some(req), Observation::Open(open_save(0)));
        for phase in [1, 3] {
            a.handle(4, Some(req), Observation::Popup(PopupTick {
                owner: 10, state: Some(ready(phase)), save_status: Some(1),
            }));
        }
        a.handle(5, Some(req), Observation::Open(OpenView {
            caller: CALLER_SAVE_SUCCESS, message: MSG_SAVE_SUCCESS,
            save_status: Some(2), ..open_save(0)
        }));
        let dismissed = a.handle(6, Some(req), Observation::Popup(PopupTick {
            owner: 10, state: Some(ready(3)), save_status: Some(3),
        }));
        assert_eq!(dismissed.command.action, Action::ButtonA);
        let done = a.handle(7, Some(req), Observation::Cards(CardsView {
            save_status: 5, save_offered: 0, ..cards_s0()
        }));
        assert!(notes(&done).contains(&"save_completed"));
        assert_ne!(done.publish, Some(Progress::Done));
        a.handle(8, Some(req), Observation::Popup(PopupTick {
            owner: 10, state: Some(ready(3)), save_status: Some(5),
        }));
        a.handle(9, Some(req), Observation::Open(OpenView {
            save_status: Some(5), ..open_rematch()
        }));
        a.handle(10, Some(req), Observation::Popup(PopupTick {
            owner: 10, state: Some(ready(1)), save_status: Some(5),
        }));
        for tick in 11..48 {
            a.handle(tick, Some(req), Observation::Popup(PopupTick {
                owner: 10, state: Some(ready(3)), save_status: Some(5),
            }));
        }
        let choice = a.handle(48, Some(req), Observation::Popup(PopupTick {
            owner: 10, state: Some(ready(3)), save_status: Some(5),
        }));
        assert_eq!(choice.command.action, Action::RematchYes);
        let accepted = a.handle(49, Some(req), Observation::Popup(PopupTick {
            owner: 10,
            state: Some(Snapshot { pending: 6, decided: true, result: 2, ..ready(3) }),
            save_status: Some(5),
        }));
        assert_eq!(accepted.publish, Some(Progress::Done));
        assert_eq!(accepted.command.action, Action::Idle);
    }

    #[test]
    fn status_five_still_dismisses_the_success_popup_once() {
        let mut a = Adapter::new();
        let req = request(true, Decision::Manual);
        a.handle(1, Some(req), Observation::Begin { seq: 70, save_status: Some(0) });
        a.handle(2, Some(req), Observation::Cards(cards_s0()));
        a.handle(3, Some(req), Observation::Open(open_save(0)));
        a.handle(4, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready(1)) }));
        let confirm = a.handle(5, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready(3)) }));
        for status in [1, 2, 3] {
            a.handle(6, Some(req), Observation::SaveStatus(status));
        }
        let dismiss = OpenView {
            owner: 10,
            caller: 0x11,
            message: 0x22,
            word_160: 0,
            word_164: 0,
            word_168: 0,
            word_16c: 0,
            word_170: 0,
            data_ok: true, save_status: None,
        };
        a.handle(7, Some(req), Observation::Open(dismiss));
        a.handle(8, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready(1)) }));
        let jumped = a.handle(9, Some(req), Observation::SaveStatus(5));
        assert!(notes(&jumped).contains(&"save_completed"));
        assert_eq!(jumped.command.action, Action::Idle);
        assert_ne!(jumped.command.serial, confirm.command.serial);
        assert_ne!(jumped.publish, Some(Progress::Done));
        let success = a.handle(
            10,
            Some(req),
            Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready(3)) }),
        );
        assert_eq!(success.command.action, Action::ButtonA);
        assert_ne!(success.command.serial, jumped.command.serial);
        assert_ne!(success.publish, Some(Progress::Done));
        let finished = a.handle(11, Some(req), Observation::SaveStatus(5));
        assert_eq!(finished.publish, Some(Progress::Done));
        assert!(!notes(&finished).contains(&"save_completed"));
    }

    #[test]
    fn nonzero_save_default_refuses_and_does_not_complete() {
        let mut a = Adapter::new();
        let req = request(true, Decision::Yes);
        a.handle(1, Some(req), Observation::Begin { seq: 4, save_status: Some(0) });
        a.handle(2, Some(req), Observation::Cards(cards_s0()));
        let refused = a.handle(3, Some(req), Observation::Open(open_save(4)));
        assert!(notes(&refused).contains(&"save_default_unsupported"));
        assert_ne!(refused.command.action, Action::ButtonA);
        let failed = a.handle(4, Some(req), Observation::SaveStatus(2));
        assert_eq!(failed.publish, Some(Progress::Failed));
        assert!(notes(&failed).iter().any(|e| *e == "save_untrusted" || *e == "failed_save"));
        let later = a.handle(5, Some(req), Observation::SaveStatus(5));
        assert_ne!(later.publish, Some(Progress::Done));
    }

    fn ready_rematch(a: &mut Adapter, req: Request) -> Snapshot {
        a.handle(1, Some(req), Observation::Begin { seq: 8, save_status: Some(0) });
        let mut nosave = cards_s0();
        nosave.save_offered = 0;
        a.handle(2, Some(req), Observation::Cards(nosave));
        a.handle(3, Some(req), Observation::Open(open_rematch()));
        a.handle(4, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready(1)) }));
        let state = ready(3);
        let early = a.handle(5, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(state) }));
        assert!(notes(&early).contains(&"choice_latched"));
        assert_ne!(early.command.action, Action::RematchNo);
        assert_ne!(early.command.action, Action::RematchYes);
        state
    }

    #[test]
    fn rematch_yes_and_no_wait_thirty_six_ticks_and_ignore_forced_or_early() {
        for (choice, action, result) in [
            (Decision::No, Action::RematchNo, 1u32),
            (Decision::Yes, Action::RematchYes, 2u32),
        ] {
            let req = request(false, choice);
            let mut a = Adapter::new();
            let state = ready_rematch(&mut a, req);
            a.open_ticks = 35;
            let armed = a.handle(6, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(state) }));
            assert_eq!(armed.command.action, action);
            assert_eq!(armed.publish, Some(Progress::Rematch));
            if choice == Decision::Yes {
                assert!(notes(&armed).contains(&"yes2_unqualified"));
            }
            let answer = Snapshot { pending: 6, decided: true, result, ..state };
            let answered = a.handle(
                9,
                Some(req),
                Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(answer) }),
            );
            assert_eq!(answered.publish, Some(Progress::Done));

            let mut forced_adapter = Adapter::new();
            let state = ready_rematch(&mut forced_adapter, req);
            let forced = Snapshot { phase: 5, pending: 6, result, ..state };
            let ignored = forced_adapter.handle(
                8,
                Some(req),
                Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(forced) }),
            );
            assert_ne!(ignored.publish, Some(Progress::Done));
            assert!(notes(&ignored).contains(&"decision_ignored"));
        }
    }

    #[test]
    fn zero_and_three_are_not_accepted_and_a_foreign_owner_does_not_drop_ready() {
        let mut a = Adapter::new();
        let req = request(false, Decision::No);
        a.handle(1, Some(req), Observation::Begin { seq: 3, save_status: Some(0) });
        let mut nosave = cards_s0();
        nosave.save_offered = 0;
        a.handle(2, Some(req), Observation::Cards(nosave));
        a.handle(3, Some(req), Observation::Open(open_rematch()));
        a.handle(4, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready(1)) }));
        let ready3 = ready(3);
        a.handle(5, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready3) }));
        a.open_ticks = 35;
        let foreign = a.handle(6, Some(req), Observation::Popup(PopupTick { owner: 20, save_status: None, state: Some(ready(0)) }));
        assert_eq!(a.open_ticks, 35);
        assert_ne!(foreign.command.action, Action::RematchNo);
        let still = a.handle(7, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready3) }));
        assert_eq!(a.open_ticks, 36);
        assert_eq!(still.command.action, Action::RematchNo);
        let zero = Snapshot { pending: 6, decided: true, result: 0, ..ready3 };
        let ignored = a.handle(8, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(zero) }));
        assert!(notes(&ignored).contains(&"decision_unmapped"));
        assert_ne!(ignored.publish, Some(Progress::Done));
        let mut three_adapter = Adapter::new();
        let ready3 = ready_rematch(&mut three_adapter, req);
        three_adapter.open_ticks = 35;
        three_adapter.handle(6, Some(req), Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(ready3) }));
        let three = Snapshot { pending: 6, decided: true, result: 3, ..ready3 };
        let ignored = three_adapter.handle(
            8,
            Some(req),
            Observation::Popup(PopupTick { owner: 10, save_status: None, state: Some(three) }),
        );
        assert!(notes(&ignored).contains(&"decision_unmapped"));
        assert_ne!(ignored.publish, Some(Progress::Done));
    }

    #[test]
    fn cancel_and_epoch_end_do_not_publish_done() {
        let controls = Controls::new();
        controls.enable();
        assert!(controls.edit(0, 1));
        assert!(controls.edit(1, 2));
        assert!(controls.edit(2, 1));
        let req = controls.pending().unwrap();
        let mut a = Adapter::new();
        a.handle(1, Some(req), Observation::Begin { seq: 1, save_status: Some(0) });
        let armed = a.handle(2, Some(req), Observation::Cards(cards_s0()));
        assert_eq!(armed.command.action, Action::ButtonY);
        assert!(controls.edit(2, 0));
        let cancelled = a.handle(3, None, Observation::SaveStatus(0));
        assert_eq!(cancelled.command.action, Action::Idle);
        assert!(notes(&cancelled).contains(&"cancelled"));
        assert_ne!(cancelled.publish, Some(Progress::Done));
        let ended = a.handle(4, None, Observation::End);
        assert!(notes(&ended).contains(&"epoch_end"));
        assert_ne!(ended.publish, Some(Progress::Done));
    }

    #[test]
    fn second_results_lifetime_fails_the_bound_request() {
        let mut a = Adapter::new();
        let req = request(false, Decision::Manual);
        a.handle(1, Some(req), Observation::Begin { seq: 1, save_status: Some(0) });
        a.handle(2, Some(req), Observation::SaveStatus(0));
        let changed = a.handle(3, Some(req), Observation::Begin { seq: 2, save_status: Some(0) });
        assert_eq!(changed.publish, Some(Progress::Failed));
        assert!(notes(&changed).contains(&"failed_scene"));
    }
}
