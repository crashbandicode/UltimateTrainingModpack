//! Bounded, read-only entry7 baseline evidence in exact LDN waiting practice.
//! A waiting layout does not establish a connected peer or authorize creation.
use super::native_source_capacity::{self as shared, Entry, Inputs};
use super::{aligned, game_mode, lobby, native_source, read, setting, Forms, BASE, INSTALL};
use crate::common;
use super::native_service::ServiceEntryContext;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use training_mod_consts::TrioFighter;

const OWNERS: [u32; 4] = [0, 4, 5, 6];
const SAMPLE_LIMIT: u16 = 120;
const POLL_LIMIT: u16 = 900;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Roots {
    manager: usize,
    internal: usize,
    context: usize,
    service: usize,
    service_inner: usize,
    listeners: u64,
    count: u32,
    primary: [usize; 8],
    backup: [usize; 8],
    resources: [usize; 8],
    aliases: [usize; 8],
    rules: [u8; 2],
    ratio_bits: u32,
    selection: [(u8, u8, u8); 8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sample {
    roots: Roots,
    originals: [Entry; 4],
    resource_kind_costume: [(u32, u32); 4],
    input: Inputs,
}

struct State {
    generation: u64,
    entries: [usize; 4],
    samples: u16,
    started: bool,
    polls: u16,
    refused: u32,
    first: bool,
    callbacks: u8,
    logged_callbacks: u8,
    stable: Option<(Sample, u16)>,
    done: bool,
}

impl State {
    const fn new(generation: u64) -> Self {
        Self {
            generation,
            entries: [0; 4],
            samples: 0,
            started: false,
            polls: 0,
            refused: 0,
            first: false,
            callbacks: 0,
            logged_callbacks: 0,
            stable: None,
            done: false,
        }
    }
    fn reset(&mut self) {
        *self = Self::new(self.generation.wrapping_add(1));
    }
}
static STATE: Mutex<State> = Mutex::new(State::new(0));

type Refusal = (u8, &'static str, usize, usize);

fn waiting() -> bool {
    matches!(game_mode::current(), Some(mode)
        if mode.initialized && mode.melee == 0x0d && mode.mode == 0x02010000)
}

/// Read-only observation eligibility, not permission to create/install a source.
/// loaded records a completed roster replacement. Resolve its current native
/// identity/readiness without a state mutex, then revalidate that exact state.
pub(super) unsafe fn roster_settled() -> bool {
    if !waiting() { return false; }
    for (owner, state) in [0u32, 4].iter().copied().zip(super::STATES.iter()) {
        let before = {
            let state = state.lock().unwrap();
            if state.pending.is_some() || state.active.is_some() { return false; }
            (state.epoch, state.revision, state.loaded, state.observed)
        };
        if let Some((identity, loaded_owner, kind)) = before.2 {
            if loaded_owner != owner || before.3 != Some((identity, kind)) { return false; }
            let Some(entry) = native_source::entry(owner) else { return false; };
            if entry as usize != identity
                || super::ready_forms(entry, owner).is_none_or(|(_, active)| active != kind)
            { return false; }
        }
        let state = state.lock().unwrap();
        if state.pending.is_some() || state.active.is_some()
            || (state.epoch, state.revision, state.loaded, state.observed) != before
        { return false; }
    }
    true
}

/// Existing private state is queried with no observer/native lock held.
unsafe fn idle() -> Result<(), Refusal> {
    if INSTALL.active() {
        return Err((14, "native install active", 0, 0));
    }
    if !common::is_ready_go() || !lobby::active() {
        return Err((2, "native waiting practice not ready/active", 0, 0));
    }
    if lobby::native_ui_busy() || super::menu::captures_input() {
        return Err((15, "native or plugin UI busy", 0, 0));
    }
    if super::training::save_states::is_loading()
        || super::training::recording_or_playback()
        || crate::replay_capture::loading()
        || crate::replay_preview::occupied()
        || super::button_config::has_pending_request()
    {
        return Err((16, "reset/replay/controller work active", 0, 0));
    }
    if !roster_settled() {
        return Err((17, "roster work or completed replacement identity not ready", 0, 0));
    }
    if !super::sources::capacity_probe_idle() {
        return Err((18, "managed sources/wants/leases present", 0, 0));
    }
    if !crate::kirby_hats::capacity_probe_idle() || !crate::character_items::capacity_probe_idle() {
        return Err((19, "hat/item work pending", 0, 0));
    }
    if cfg!(feature = "source-owner-probe") && !cfg!(feature = "managed-sources") {
        return Err((20, "independent source-owner experiment enabled", 0, 0));
    }
    if !item_probe_idle() {
        return Err((21, "item experiment active", 0, 0));
    }
    Ok(())
}

fn item_probe_idle() -> bool {
    #[cfg(feature = "item-state-probe")]
    {
        crate::item_state_probe::capacity_probe_idle()
    }
    #[cfg(not(feature = "item-state-probe"))]
    {
        true
    }
}

fn refuse(generation: u64, (code, reason, first, second): Refusal) -> bool {
    let mut state = STATE.lock().unwrap();
    if state.generation != generation {
        return false;
    }
    state.stable = None;
    state.callbacks = 0;
    let bit = 1u32 << code;
    let log = state.refused & bit == 0;
    state.refused |= bit;
    drop(state);
    if log {
        crate::logging::diagnostic(format_args!(
            "[LdnCapacityBaseline] refusal generation={} code={} reason={} detail=0x{:x}/0x{:x}; no native mutation",
            generation, code, reason, first, second));
    }
    log
}

/// These are the already established common-root offsets. The strict Training
/// Snapshot is deliberately not called or generalized: its originals/quantity differ.
unsafe fn roots(ctx: &ServiceEntryContext, entry: *const u8, owner: u32) -> Result<Roots, Refusal> {
    let (service, service_inner) = shared::dispatch_identity(ctx, entry).ok_or((
        3,
        "current dispatcher tuple unavailable",
        owner as usize,
        entry as usize,
    ))?;
    if !native_source::exact_waiting_layout(entry, owner) {
        return Err((
            4,
            "exact 0/4/5/6 waiting layout unavailable",
            owner as usize,
            entry as usize,
        ));
    }
    let base = BASE.load(Ordering::Acquire);
    let symbol = setting(&common::FIGHTER_MANAGER_ADDR);
    if symbol != base + 0x52b94f8 {
        return Err((
            5,
            "fighter manager singleton differs",
            symbol,
            base + 0x52b94f8,
        ));
    }
    let manager = read::<*const u8>(symbol as *const u8, 0);
    if !aligned(manager) {
        return Err((5, "manager unavailable", manager as usize, 0));
    }
    let internal = read::<*const u8>(manager, 0);
    let context = read::<*const u8>(base as *const u8, 0x5324680);
    if !aligned(internal) || !aligned(context) {
        return Err((
            5,
            "manager/context unavailable",
            internal as usize,
            context as usize,
        ));
    }
    Ok(Roots {
        manager: manager as usize,
        internal: internal as usize,
        context: context as usize,
        service,
        service_inner,
        listeners: read(service_inner as *const u8, 0x930),
        count: read(internal, 0xa0),
        primary: std::array::from_fn(|i| read(internal, 0x20 + i * 8)),
        backup: std::array::from_fn(|i| read(internal, 0x60 + i * 8)),
        resources: std::array::from_fn(|i| read(context, i * 8)),
        aliases: std::array::from_fn(|i| read(context, 0xe8 + i * 8)),
        rules: [read(internal, 0xea), read(internal, 0xeb)],
        ratio_bits: read(internal, 0xf0),
        selection: std::array::from_fn(|i| {
            let row = (base + 0x52c5730 + i * 0x260) as *const u8;
            (read(row, 0xa), read(row, 0x79), read(row, 0xa7))
        }),
    })
}

unsafe fn observe(ctx: &ServiceEntryContext, entry: *const u8, owner: u32) -> Result<Sample, Refusal> {
    let roots = roots(ctx, entry, owner)?;
    if roots.resources[7] != 0 || roots.aliases[7] != 0 {
        return Err((
            6,
            "candidate7 resource owner/alias is occupied",
            roots.resources[7],
            roots.aliases[7],
        ));
    }
    let mut originals = [None; 4];
    let mut fields = [(0, 0); 4];
    for (index, slot) in OWNERS.iter().copied().enumerate() {
        let current = roots.primary[slot as usize] as *const u8;
        let observed = shared::entry_observation(current, slot).ok_or((
            7,
            "original entry observation unavailable",
            slot as usize,
            current as usize,
        ))?;
        let kind = observed.kinds[0];
        if TrioFighter::from_kind(kind).is_none() {
            return Err((
                7,
                "original kind is not in the catalog",
                slot as usize,
                kind as usize,
            ));
        }
        let forms = Forms::starting(kind);
        if forms.count != 1
            || forms.actor_count != 1
            || observed.count != 1
            || observed.installed != 1
        {
            return Err((
                7,
                "original is not an installed ordinary single form",
                slot as usize,
                kind as usize,
            ));
        }
        let ready = if index < 2 {
            super::ready_forms(current, slot)
                .is_some_and(|(forms, _)| forms.count == 1 && forms.actor_count == 1)
        } else {
            native_source::ready(current, slot, kind).is_some()
        };
        if !ready {
            return Err((
                8,
                "original full readiness unavailable",
                slot as usize,
                kind as usize,
            ));
        }
        let resource = roots.resources[slot as usize] as *const u8;
        if !aligned(resource)
            || roots.aliases[slot as usize] != resource as usize
            || read::<usize>(resource, 0) != BASE.load(Ordering::Acquire) + 0x509fb10
        {
            return Err((
                9,
                "original owner/alias identity unavailable",
                slot as usize,
                resource as usize,
            ));
        }
        fields[index] = (read(resource, 0x58), read(resource, 0x64));
        if fields[index].0 != kind {
            return Err((
                9,
                "original resource kind differs",
                slot as usize,
                fields[index].0 as usize,
            ));
        }
        originals[index] = Some(observed);
    }
    let input = shared::inputs().map_err(|(reason, a, b)| (10, reason, a, b))?;
    Ok(Sample {
        roots,
        originals: originals.map(|value| value.unwrap()),
        resource_kind_costume: fields,
        input,
    })
}

pub fn scene_changed() {
    STATE.lock().unwrap().reset();
}

/// Numeric identities only: destruction can precede a mode tuple change.
pub fn on_destroy(entry: *const u8, owner: u32) {
    let Some(index) = OWNERS.iter().position(|slot| *slot == owner) else {
        return;
    };
    let mut state = STATE.lock().unwrap();
    if state.entries[index] == entry as usize {
        state.reset();
    }
}

/// Runs before existing mutation services, from the already installed callback.
pub unsafe fn service(ctx: &ServiceEntryContext, entry: *const u8, owner: u32) {
    if !waiting() {
        return;
    }
    let Some(index) = OWNERS.iter().position(|slot| *slot == owner) else {
        return;
    };
    let generation = {
        let mut state = STATE.lock().unwrap();
        if state.entries[index] != 0 && state.entries[index] != entry as usize {
            state.reset();
        }
        state.entries[index] = entry as usize;
        if index < 2 || state.done {
            return;
        }
        // Setup before the first idle callback does not spend the budget.
        // Once started, later busy callbacks still advance its finite lifetime.
        if state.started {
            state.polls += 1;
        }
        if state.samples >= SAMPLE_LIMIT || state.polls > POLL_LIMIT {
            state.done = true;
            let generation = state.generation;
            let detail = (state.polls as usize, state.samples as usize);
            drop(state);
            refuse(
                generation,
                (
                    13,
                    "bounded observation budget exhausted",
                    detail.0,
                    detail.1,
                ),
            );
            return;
        }
        state.generation
    };
    if !shared::creation_supported()
        || !shared::load_supported()
        || !crate::character_lifecycle::native_hooks_ready()
    {
        refuse(
            generation,
            (1, "verified native contracts unavailable", 0, 0),
        );
        return;
    }
    if let Err(reason) = idle() {
        refuse(generation, reason);
        return;
    }
    {
        let mut state = STATE.lock().unwrap();
        if state.generation != generation {
            return;
        }
        if !state.started {
            state.started = true;
            state.polls = 1;
        }
        state.samples += 1;
    }
    let sample = match observe(ctx, entry, owner) {
        Ok(sample) => sample,
        Err(reason) => {
            refuse(generation, reason);
            return;
        }
    };
    // Reacquire current roots after readiness/native getter calls. Never read
    // addresses retained in the prior callback's Sample.
    if !waiting() || idle().is_err() || roots(ctx, entry, owner).ok() != Some(sample.roots) {
        refuse(
            generation,
            (
                12,
                "identity/idle changed during observation",
                owner as usize,
                entry as usize,
            ),
        );
        return;
    }
    {
        let mut state = STATE.lock().unwrap();
        if state.generation != generation || state.entries[index] != entry as usize || state.done {
            return;
        }
        // Bind every fresh original identity, even if its own callback has not
        // reached us yet. Any exact original destructor invalidates this run.
        state.entries = sample.originals.map(|original| original.identity);
    }
    if !sample.input.unclaimed() {
        if refuse(
            generation,
            (
                11,
                "candidate input7/15 or active alias occupied",
                sample.input.slots[7].pointer,
                sample.input.slots[15].pointer,
            ),
        ) {
            crate::logging::diagnostic(format_args!(
                "[LdnCapacityBaseline] occupied input generation={} input={:x?}",
                generation, sample.input
            ));
        }
        return;
    }
    let mut state = STATE.lock().unwrap();
    if state.generation != generation || state.entries[index] != entry as usize || state.done {
        return;
    }
    let same = state.stable.is_some_and(|(before, _)| before == sample);
    let count = if same { state.stable.unwrap().1 + 1 } else { 1 };
    if !same {
        state.callbacks = 0;
    }
    let bit = 1 << (index - 2);
    let new_callback = state.logged_callbacks & bit == 0;
    state.logged_callbacks |= bit;
    state.callbacks |= bit;
    state.stable = Some((sample, count));
    let first = !state.first;
    state.first = true;
    let complete = count >= 30 && state.callbacks == 3;
    state.done = complete;
    let callbacks = state.callbacks;
    drop(state);
    if new_callback {
        crate::logging::diagnostic(format_args!(
            "[LdnCapacityBaseline] dispatcher generation={} owner={} entry={:p} registers(x19,x20,x21,lr)={:x?}",
            generation, owner, entry,
            ctx.dispatcher_tuple()));
    }
    if first || complete {
        crate::logging::diagnostic(format_args!(
            "[LdnCapacityBaseline] qualified generation={} stable={} callbacks={} sample={:x?}; mode=0x0d/0x02010000 lobby_active=true observation_only=true quantity_not_applicable=true peer_not_inferred=true creation_not_authorized=true",
            generation, count, callbacks, sample));
    }
}
