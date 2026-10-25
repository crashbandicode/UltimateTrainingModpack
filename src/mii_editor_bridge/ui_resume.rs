//! One unfinished interaction in one outer Quickplay lifetime. Checkpoints
//! contain owned values only; native UI adapters rebuild objects after return.
use super::{cosmetic_resume, draft::Draft, inline_keyboard::Text, return_gate::ReturnGate,
    settings_resume, specials_resume, value, STATE};
use std::sync::{Arc, Mutex};
use training_mod_consts::mii::inventory::Snapshot as Catalog;
use training_mod_tui::resume::{ResumeReadiness, ResumeSlot, ResumeTicket};

#[derive(Clone, Copy, Debug)]
pub(super) enum Screen {
    Settings(settings_resume::Snapshot),
    Name(Text),
    Headgear(cosmetic_resume::GearSelection),
    Outfit(cosmetic_resume::GearSelection),
    Color(cosmetic_resume::ColorSelection),
    Specials(specials_resume::Snapshot),
    Voice(cosmetic_resume::VoiceSelection),
    Test,
}

#[derive(Clone)]
pub(super) struct Checkpoint {
    pub seed: Draft,
    pub catalog: Arc<Catalog>,
    pub screen: Screen,
    pub focus: i32,
}

struct Continuation {
    slot: ResumeSlot<Checkpoint>,
    generation: Option<u64>,
    gate: ReturnGate,
    released: bool,
}

static CONTINUATION: Mutex<Continuation> = Mutex::new(Continuation {
    slot: ResumeSlot::new(), generation: None, gate: ReturnGate::new(), released: false,
});

pub(super) fn pending() -> bool { CONTINUATION.lock().unwrap().slot.ticket().is_some() }

/// A resumed creation intent must acquire its complete value before editor
/// entry. Refine that exact restoring ticket as well as Session.seed so a second
/// interruption cannot regenerate a different default. Pending UI is untouched.
pub(super) fn materialized(
    ticket: ResumeTicket, class: super::inventory::MiiClass, row: value::Row,
) -> Result<(), &'static str> {
    let mut state = CONTINUATION.lock().unwrap();
    let checkpoint = state.slot.restoring_snapshot_mut(ticket).ok_or("default_resume_ticket_mismatch")?;
    checkpoint.seed.materialize(class, row)
}

/// Re-interruption during reconstruction keeps the original checkpoint. Once
/// reconstruction was acknowledged, a later interruption captures fresh edits.
pub(super) fn suspend(
    generation: u64, checkpoint: Checkpoint, restoring: Option<ResumeTicket>,
) -> Result<ResumeTicket, &'static str> {
    let mut state = CONTINUATION.lock().unwrap();
    let ticket = if let Some(ticket) = restoring {
        if state.generation != Some(generation) || !state.slot.interrupt_restore(ticket) {
            return Err("resume_ticket_mismatch");
        }
        ticket
    } else {
        state.slot.suspend(generation, checkpoint).map_err(|_| "resume_slot_occupied")?
    };
    state.generation = Some(generation);
    state.gate.arm(generation);
    state.released = false;
    Ok(ticket)
}

pub(super) fn previous_ui_released(generation: u64) {
    let mut state = CONTINUATION.lock().unwrap();
    if state.generation == Some(generation) { state.released = true; }
}

pub(super) fn observe_session(generation: u64, current: i32, previous: i32, changed: bool) {
    let waiting = {
        let state = CONTINUATION.lock().unwrap();
        if state.generation != Some(generation) { return; }
        state.slot.ticket().is_some()
    };
    if waiting && changed {
        if let Some(s) = STATE.lock().unwrap().as_mut() {
            s.event("resume_outer_transition", [generation, current as u64, previous as u64, 0]);
        }
    }
}

pub(super) fn observe_practice(generation: u64, match_received: bool, practice_ready: bool) {
    let mut state = CONTINUATION.lock().unwrap();
    state.gate.observe(generation, match_received, practice_ready);
}

pub(super) fn begin(
    generation: u64, practice_ready: bool, another_ui_active: bool,
) -> Option<(ResumeTicket, Checkpoint)> {
    let mut state = CONTINUATION.lock().unwrap();
    if state.generation != Some(generation) { return None; }
    let ready = ResumeReadiness {
        session: generation,
        previous_ui_released: state.released,
        interruption_resolved: state.gate.ready(generation),
        practice_ready,
        another_ui_active,
    };
    let ticket = state.slot.ticket()?;
    let checkpoint = state.slot.begin_restore(ticket, ready)?.clone();
    state.gate.attempted(generation);
    Some((ticket, checkpoint))
}

pub(super) fn restored(ticket: ResumeTicket) -> bool {
    let mut state = CONTINUATION.lock().unwrap();
    if !state.slot.restored(ticket) { return false; }
    if let Some(generation) = state.generation.take() { state.gate.end(generation); }
    state.released = false;
    true
}

pub(super) fn restore_failed(ticket: ResumeTicket) -> bool {
    CONTINUATION.lock().unwrap().slot.restore_failed(ticket)
}

pub(super) fn request_refused(ticket: ResumeTicket) {
    let mut state = CONTINUATION.lock().unwrap();
    if state.slot.restore_failed(ticket) {
        if let Some(generation) = state.generation { state.gate.request_refused(generation); }
    }
}

pub(super) fn readiness_bits(generation: u64) -> u64 {
    let state = CONTINUATION.lock().unwrap();
    u64::from(state.generation == Some(generation))
        | (u64::from(state.released) << 1)
        | (u64::from(state.gate.ready(generation)) << 2)
        | (u64::from(state.slot.ticket().is_some()) << 3)
}

/// Explicit editor cancellation or saving ends this unfinished interaction.
pub(super) fn discard() {
    let mut state = CONTINUATION.lock().unwrap();
    if let Some(ticket) = state.slot.ticket() { state.slot.discard(ticket); }
    if let Some(generation) = state.generation.take() { state.gate.end(generation); }
    state.released = false;
}

pub(super) fn end_session(generation: u64) -> bool {
    let mut state = CONTINUATION.lock().unwrap();
    if state.generation == Some(generation) {
        if let Some(ticket) = state.slot.ticket() { state.slot.discard(ticket); }
        state.gate.end(generation);
        state.generation = None;
        state.released = false;
        return true;
    }
    false
}
