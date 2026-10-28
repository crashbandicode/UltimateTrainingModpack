//! Shared, demand-driven native source ownership for copy abilities and items.
//! Native entries are borrowed only at their own loader callback. Requests,
//! leases and epochs contain values; every game accessor is resolved afresh.
#[cfg(feature = "managed-third-source-alpha")]
use super::native_source_capacity::managed as capacity;
use super::{aligned, finish_install, native_source, read, InstallToken};
use super::{ready_forms, INSTALL};
#[cfg(feature = "mii-managed-source-coexist-alpha")]
use super::mii_ordinary_player::native as coexist;
use crate::source_lifetime::{
    Holder, LeaseToken, LoadToken, NativeEntry, SceneEpoch, SourceBook,
    SourceToken,
};
use crate::source_lifetime::{Phase, SourceKey};
#[cfg(feature = "managed-third-source-alpha")]
use skyline::hooks::InlineCtx;
use super::native_service::ServiceEntryContext;
use smash::app::{self, lua_bind::*, sv_battle_object};
use smash::lib::lua_const::*;
use std::convert::TryFrom;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

// P1 hat, CPU hat, P1 item, then (portrait feature) P1 portrait, CPU portrait.
const CHANNELS: usize = if cfg!(feature = "mii-hud-portrait-alpha") { 5 } else { 3 };
/// Portrait wants carry the loaded configuration and avoid the other owner's bust row.
#[cfg(feature = "mii-hud-portrait-alpha")]
pub const PORTRAIT_CHANNELS: [usize; 2] = super::mii_hud_portrait_policy::SOURCE_CHANNELS;
const LEASES: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Want {
    epoch: SceneEpoch,
    kind: u32,
    configuration:Option<training_mod_consts::mii::lifetime::Configuration>,
    /// Portrait only. `None` keeps hat/item selection identical to the feature-off path.
    #[cfg(feature = "mii-hud-portrait-alpha")]
    avoid: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Source {
    token: SourceToken,
    load: LoadToken,
    install: Option<InstallToken>,
    kind: u32,
    ticks: u16,
    configuration:Option<training_mod_consts::mii::lifetime::Configuration>,
    #[cfg(feature = "mii-managed-source-coexist-alpha")]
    original: Option<coexist::helpers::OriginalRow>,
}

#[cfg(feature = "managed-third-source-alpha")]
#[derive(Clone, Copy)]
struct FirstLoad {
    wanted: Want,
    reservation: capacity::Reservation,
    previous: Option<Source>,
    before: capacity::Preflight,
    ticks: u16,
}

/// Event-only evidence; none of these fields participates in admission or release.
#[cfg(feature = "managed-third-source-alpha")]
#[derive(Clone, Copy, PartialEq, Eq)]
struct CleanupObservation {
    retiring: Option<Source>,
    #[cfg(feature = "managed-grouped-source-alpha")]
    current_actors: Option<capacity::Actors>,
    #[cfg(feature = "managed-grouped-source-alpha")]
    retiring_actors: Option<capacity::Actors>,
    #[cfg(feature = "managed-grouped-source-alpha")]
    masks: [(u8, u8); 2], // current/retiring: seen, valid; diagnostic only.
    #[cfg(feature = "managed-grouped-source-alpha")]
    cleaned: [[Option<capacity::Actor>; 3]; 2],
    last_actor: Option<(Option<SourceToken>, usize)>,
    ready_actors: u32,
    actor_events: u32,
    valid_actors: u32,
    ready_cleanups: u32,
    destroy_seen: bool,
    listeners_before: Option<u64>,
    listener_removed: Option<bool>,
}

#[cfg(feature = "managed-third-source-alpha")]
impl CleanupObservation {
    const fn new() -> Self {
        Self {
            retiring: None,
            #[cfg(feature = "managed-grouped-source-alpha")]
            current_actors: None,
            #[cfg(feature = "managed-grouped-source-alpha")]
            retiring_actors: None,
            #[cfg(feature = "managed-grouped-source-alpha")]
            masks: [(0, 0); 2],
            #[cfg(feature = "managed-grouped-source-alpha")]
            cleaned: [[None; 3]; 2],
            last_actor: None,
            ready_actors: 0,
            actor_events: 0,
            valid_actors: 0,
            ready_cleanups: 0,
            destroy_seen: false,
            listeners_before: None,
            listener_removed: None,
        }
    }
}

#[cfg(all(feature = "managed-third-source-alpha", feature = "source-capacity-probe"))]
#[derive(Clone, Copy, PartialEq, Eq)]
struct AdmissionSeen {
    wanted: Want,
    previous: SourceToken,
    reservation: Option<usize>,
    conditions: u32,
}

#[cfg(feature = "managed-third-source-alpha")]
struct Capacity {
    attempted: Option<SceneEpoch>,
    stable: Option<(Want, capacity::Preflight, u16, u8)>,
    reservation: Option<(SceneEpoch, capacity::Reservation)>,
    load: Option<(SourceToken, capacity::Preflight)>,
    first_load: Option<FirstLoad>,
    observation: CleanupObservation,
    #[cfg(feature = "source-capacity-probe")]
    admission: Option<AdmissionSeen>,
}

#[derive(Clone, Copy)]
#[cfg_attr(feature = "source-lifecycle-probe", derive(Debug))]
enum Consumer {
    Reserved,
    Hat { body: u32, kind: u32, slot: u32 },
    Item { body: u32, kind: i32 },
    /// Held only while a portrait render is queued. Scene departure and `release` end it.
    #[cfg(feature = "mii-hud-portrait-alpha")]
    Portrait,
}

#[derive(Clone, Copy)]
#[cfg_attr(feature = "source-lifecycle-probe", derive(Debug))]
struct LiveLease {
    token: LeaseToken,
    source: SourceToken,
    consumer: Consumer,
    settle_polls: u8,
}

struct State {
    book: SourceBook<8, LEASES>,
    sources: [Option<Source>; 8],
    wants: [Option<Want>; CHANNELS],
    #[cfg(feature="mii-managed-helper-alpha")]
    refusals: [Option<(Want, &'static str)>; CHANNELS],
    leases: [Option<LiveLease>; LEASES],
    failed: u128,
    #[cfg(feature = "managed-third-source-alpha")]
    capacity: Capacity,
}

static STATE: Mutex<State> = Mutex::new(State {
    book: SourceBook::new(),
    sources: [None; 8],
    wants: [None; CHANNELS],
    #[cfg(feature="mii-managed-helper-alpha")]
    refusals: [None; CHANNELS],
    leases: [None; LEASES],
    failed: 0,
    #[cfg(feature = "managed-third-source-alpha")]
    capacity: Capacity {
        attempted: None,
        stable: None,
        reservation: None,
        load: None,
        first_load: None,
        observation: CleanupObservation::new(),
        #[cfg(feature = "source-capacity-probe")]
        admission: None,
    },
});
static HAS_CONSUMERS: AtomicBool = AtomicBool::new(false);

/// Strict fresh-scene predicate for the separate empty-entry experiment.
#[cfg(feature = "source-capacity-probe")]
pub(super) fn capacity_probe_idle() -> bool {
    let state = STATE.lock().unwrap();
    state.sources.iter().all(Option::is_none)
        && state.wants.iter().all(Option::is_none)
        && state.leases.iter().all(Option::is_none)
        && !HAS_CONSUMERS.load(Ordering::Acquire)
}

#[derive(Clone, Copy)]
pub struct Lease {
    token: LeaseToken,
    pub slot: u32,
    pub kind: u32,
}

/// Three qualified backends share one book. Training keeps parked CPUs 2/3 beside
/// visible 0/1. The waiting backends (exact local wireless and Quickplay, one shared
/// PracticeScene shape) keep witnessed entries 5/6 beside visible 0/4. Arenas and
/// every other slot stay out. Callback dispatch is the union, so a departing owned
/// actor is still serviced after its scene ends.
const CALLBACK_SLOTS: [u32; 4] = [2, 3, 5, 6];

use training_mod_consts::mii::backend::Layout as Backend;

fn callback_slot(slot: u32) -> bool {
    CALLBACK_SLOTS.contains(&slot) || (cfg!(feature = "managed-third-source-alpha") && slot == 7)
}

fn candidates(backend: Backend) -> [u32; 3] {
    let [first, second] = backend.helpers();
    #[cfg(feature = "managed-third-source-alpha")]
    let third = if third_source_backend(backend) { 7 } else { 8 };
    #[cfg(not(feature = "managed-third-source-alpha"))]
    let third = 8;
    [first, second, third]
}

/// Demand-created entry7 is qualified for Training, and for local wireless only
/// behind its own alpha. Quickplay waiting has no third source.
#[cfg(feature = "managed-third-source-alpha")]
fn third_source_backend(backend: Backend) -> bool {
    backend == Backend::Training
        || (backend == Backend::Ldn && cfg!(feature = "managed-ldn-third-source-alpha"))
}

#[cfg(feature = "managed-third-source-alpha")]
fn reservation(state: &State) -> Option<capacity::Reservation> {
    state
        .capacity
        .reservation
        .filter(|(epoch, value)| {
            state.book.is_current(*epoch) && !value.entry_destroyed && !value.owner_retired
        })
        .map(|(_, value)| value)
}

unsafe fn usable(entry: *const u8, slot: u32, kind: u32, backend: Backend) -> bool {
    #[cfg(feature = "managed-third-source-alpha")]
    if slot == 7 {
        let current = reservation(&STATE.lock().unwrap());
        return third_source_backend(backend)
            && current.is_some_and(|value| capacity::ready(value, entry, kind));
    }
    native_source::ready(entry, slot, kind).is_some()
        && (!backend.waiting() || waiting_layout(entry, slot, backend))
}

/// The witnessed waiting shape (manager 0/4/5/6, CPU selection rows) that local
/// wireless and Quickplay share. A count5 scene is admitted only through our exact
/// current epoch/reservation; an isolated diagnostic claimant never becomes a
/// managed helper backend.
unsafe fn waiting_layout(entry: *const u8, slot: u32, backend: Backend) -> bool {
    if native_source::exact_waiting_layout(entry, slot) { return true; }
    #[cfg(feature = "managed-ldn-third-source-alpha")]
    if backend == Backend::Ldn {
        let owner = reservation(&STATE.lock().unwrap());
        if owner.is_some_and(|owner| capacity::owned_ldn_helper(owner, entry, slot)) {
            return true;
        }
    }
    #[cfg(not(feature = "managed-ldn-third-source-alpha"))]
    let _ = backend;
    false
}

/// Initialized melee 0x0d with lobby practice already active, in one of the two
/// waiting families the shared PracticeScene setup builds: local wireless
/// (`0x02010000`) or Quickplay (`0x070101xx`). Arenas and every other family stay out.
fn waiting_backend() -> Option<Backend> {
    let mode = crate::common::game_mode::current()?;
    Backend::scene(mode.initialized, mode.melee, mode.mode, super::lobby::active())
        .filter(|backend| backend.waiting())
}

/// Scene qualification only. The waiting manager and selection table are checked
/// later, on the target's own loader callback, and only when a request needs
/// a new install.
unsafe fn qualified_backend() -> Result<Backend, &'static str> {
    if !cfg!(feature = "managed-sources") {
        return Err("Character resource loading is not available in this scene yet");
    }
    if crate::common::is_native_training_mode() {
        return if native_source::training_cpu_quantity() == Some(1) {
            Ok(Backend::Training)
        } else {
            Err("Character resources currently need one visible CPU")
        };
    }
    waiting_backend().ok_or("Character resource loading is not available in this scene yet")
}

/// Returns None while the native loader works. A queue entry does not pin an old
/// source; the returned lease does, even before the consumer has been created.
pub unsafe fn prepare(
    channel: usize,
    kind: u32,
    holder: Holder,
) -> Result<Option<Lease>, &'static str> {
    #[cfg(feature = "mii-hud-portrait-alpha")]
    if PORTRAIT_CHANNELS.contains(&channel) {
        return Err("Portrait channels require prepare_portrait");
    }
    prepare_with(channel, kind, holder, None, None)
}

/// Exclusive portrait lease. The configuration is the one the visible owner loaded.
/// Nothing here resolves a different saved identity to fit a helper.
#[cfg(feature = "mii-hud-portrait-alpha")]
pub unsafe fn prepare_portrait(
    channel: usize,
    kind: u32,
    configuration: training_mod_consts::mii::lifetime::Configuration,
    holder: Holder,
    avoid: Option<u32>,
) -> Result<Option<Lease>, &'static str> {
    if !PORTRAIT_CHANNELS.contains(&channel) {
        return Err("Invalid portrait channel");
    }
    if !(0x48..=0x4a).contains(&kind) || configuration.canonical().class as u32 != kind {
        return Err("Portrait requires the loaded Mii class and its exact configuration");
    }
    prepare_with(channel, kind, holder, Some(configuration), avoid)
}

unsafe fn prepare_with(
    channel: usize,
    kind: u32,
    holder: Holder,
    exact: Option<training_mod_consts::mii::lifetime::Configuration>,
    avoid: Option<u32>,
) -> Result<Option<Lease>, &'static str> {
    #[cfg(not(feature = "mii-hud-portrait-alpha"))]
    let _ = (exact, avoid);
    if channel >= CHANNELS {
        return Err("Invalid source request");
    }
    let Some(kind_bit) = 1u128
        .checked_shl(kind)
        .filter(|_| (!(0x48..=0x4a).contains(&kind) || cfg!(feature="mii-managed-helper-alpha"))
            && training_mod_consts::TrioFighter::from_kind(kind).is_some())
    else {
        return Err("This character's resources are unsupported");
    };
    // Unqualified scenes leave requests, channels and counters untouched.
    let backend = qualified_backend()?;
    #[cfg(feature="mii-managed-helper-alpha")]
    {
        // A consumer polls prepare until success/error. Deliver its exact
        // request's refusal before it can resolve/requeue or read native rows.
        let state = STATE.lock().unwrap();
        if let Some(refusal) = state.refusals[channel].as_ref()
            .filter(|(want, _)| want.kind == kind && state.book.is_current(want.epoch)) {
            return Err(refusal.1);
        }
    }
    #[cfg(not(feature="mii-managed-helper-alpha"))]
    let configuration=None;
    #[cfg(feature="mii-managed-helper-alpha")]
    let configuration=if (0x48..=0x4a).contains(&kind) {
        if channel == 2 {return Err("Mii helpers are limited to copy hats");}
        let queued={
            #[cfg(feature = "mii-hud-portrait-alpha")]
            if let Some(c) = exact {
                if !super::mii_managed_helper::admission_ready(true) {
                    return Ok(None);
                }
                let hats = super::mii_ordinary_cpu::helper_origin_hats()
                    .ok_or("Captured helper hat tokens are unavailable")?;
                Some(training_mod_consts::mii::ordinary_cpu::helper_configuration(c, hats)?)
            } else {
                let state=STATE.lock().unwrap();
                state.wants[channel]
                    .filter(|w|w.kind==kind&&state.book.is_current(w.epoch)).and_then(|w|w.configuration)
            }
            #[cfg(not(feature = "mii-hud-portrait-alpha"))]
            {
                let state=STATE.lock().unwrap();
                state.wants[channel]
                    .filter(|w|w.kind==kind&&state.book.is_current(w.epoch)).and_then(|w|w.configuration)
            }
        };
        Some(match queued {
            Some(c) => c,
            None => {
                // Automatic lobby restoration can arrive before the new
                // lifetime's catalog and immutable role captures. This is the
                // same transient admission gate used by helper installation,
                // not a failed configuration. Keep the consumer pending until
                // capture finishes; resolve real configuration errors afterward.
                if !super::mii_managed_helper::admission_ready(true) {
                    return Ok(None);
                }
                super::mii_managed_helper::configuration(channel, kind)?
            }
        })
    } else {None};
    let wanted = {
        let mut state = STATE.lock().unwrap();
        if state.failed & kind_bit != 0 {
            state.wants[channel] = None;
            return Err("This character's resources failed to load in this practice session");
        }
        let wanted = Want {
            epoch: state.book.epoch(),
            kind,configuration,
            #[cfg(feature = "mii-hud-portrait-alpha")]
            avoid,
        };
        #[cfg(feature="mii-managed-helper-alpha")]
        { state.refusals[channel] = None; }
        state.wants[channel] = Some(wanted);
        wanted
    };
    for slot in candidates(backend).iter().copied().filter(|slot| *slot < 8 && slot_allowed(avoid, *slot)) {
        let helper = slot as usize;
        let ready = {
            let state = STATE.lock().unwrap();
            let Ok(ready) = state.book.ready(helper) else {
                continue;
            };
            if state.book.key(helper).ok().flatten().map(|key| key.kind) != Some(kind)
                || state.sources[helper].is_none_or(|s|s.configuration!=wanted.configuration) {
                continue;
            }
            #[cfg(feature = "mii-hud-portrait-alpha")]
            {
                // A portrait never shares any live lease. Hats and items skip a helper
                // only while its portrait render is still the consumer.
                let portrait = PORTRAIT_CHANNELS.contains(&channel);
                if state.leases.iter().flatten().any(|live| live.source == ready.source()
                    && (portrait || !super::mii_hud_portrait_policy::hat_may_share(
                        matches!(live.consumer, Consumer::Portrait))))
                {
                    continue;
                }
            }
            ready
        };
        // Native readiness can reenter hooks. Hold no bookkeeping mutex here.
        let Some(entry) = native_source::entry(slot) else {
            continue;
        };
        if ready.source().entry() != NativeEntry(entry as u64)
            || !usable(entry, slot, kind, backend)
        {
            continue;
        }
        #[cfg(feature="mii-managed-helper-alpha")]
        if let Some(c)=wanted.configuration {
            if !super::mii_managed_helper::ready(ready,c) {continue;}
        }
        let mut state = STATE.lock().unwrap();
        if state.wants[channel] != Some(wanted)
            || !state.book.is_current(wanted.epoch)
            || state.book.ready(helper) != Ok(ready)
        {
            continue;
        }
        let Some(index) = state.leases.iter().position(Option::is_none) else {
            state.wants[channel] = None;
            return Err("Too many live character items or abilities");
        };
        let token = match state.book.acquire(ready, holder) {
            Ok(token) => token,
            Err(_) => {
                state.wants[channel] = None;
                return Err("Character resource lease could not be acquired");
            }
        };
        state.leases[index] = Some(LiveLease {
            token,
            source: ready.source(),
            consumer: Consumer::Reserved,
            settle_polls: 0,
        });
        state.wants[channel] = None;
        HAS_CONSUMERS.store(true, Ordering::Release);
        return Ok(Some(Lease { token, slot, kind }));
    }
    Ok(None)
}

/// True only for an otherwise serviceable fourth distinct demand blocked by
/// actual leases on all three ready sources in the selected capacity backend. Loading, native busy,
/// failed/unknown ownership and mere queued wants retain the normal timeout.
pub fn waiting_for_capacity(channel: usize) -> bool {
    #[cfg(not(feature = "managed-third-source-alpha"))]
    {
        let _ = channel;
        false
    }
    #[cfg(feature = "managed-third-source-alpha")]
    unsafe {
        if channel >= CHANNELS || admission_blocked() { return false; }
        let Ok(backend) = qualified_backend() else { return false; };
        let slots = candidates(backend);
        if slots[2] != 7 { return false; }
        let (wanted, sources) = {
            let state = STATE.lock().unwrap();
            let Some(wanted) = state.wants[channel] else {
                return false;
            };
            if pending_request(&state) != Some(wanted)
                || !capacity::source_kind(wanted.kind)
                || state.failed & (1u128 << wanted.kind) != 0
                || reservation(&state).is_none()
            {
                return false;
            }
            let mut sources = [None; 3];
            for (i, slot) in slots.iter().copied().map(|slot| slot as usize).enumerate() {
                let Some(source) = state.sources[slot] else {
                    return false;
                };
                if source.install.is_some()
                    || !state.book.is_current(source.token.epoch())
                    || state.book.phase(slot) != Ok(Phase::Ready)
                    || state.book.can_replace(slot) != Ok(false)
                {
                    return false;
                }
                sources[i] = Some(source);
            }
            (wanted, sources)
        };
        for (slot, source) in slots.iter().copied().zip(sources) {
            let source = source.unwrap();
            let Some(entry) = native_source::entry(slot) else {
                return false;
            };
            if source.token.entry() != NativeEntry(entry as u64)
                || !usable(entry, slot, source.kind, backend)
            {
                return false;
            }
        }
        if admission_blocked() {
            return false;
        }
        let state = STATE.lock().unwrap();
        state.wants[channel] == Some(wanted)
            && state.book.is_current(wanted.epoch)
            && slots.iter().copied().map(|slot| slot as usize)
                .zip(sources)
                .all(|(slot, source)| {
                    state.sources[slot] == source
                        && state.book.phase(slot) == Ok(Phase::Ready)
                        && state.book.can_replace(slot) == Ok(false)
                })
    }
}

pub fn cancel(channel: usize) {
    let mut state = STATE.lock().unwrap();
    #[cfg(all(feature = "managed-third-source-alpha", feature = "source-capacity-probe"))]
    if state.wants.get(channel).copied().flatten().is_some_and(|wanted|
        state.capacity.admission.is_some_and(|seen| seen.wanted == wanted))
    {
        state.capacity.admission = None;
    }
    if let Some(want) = state.wants.get_mut(channel) {
        *want = None;
    }
    #[cfg(feature="mii-managed-helper-alpha")]
    if let Some(refusal) = state.refusals.get_mut(channel) {
        *refusal = None;
    }
    #[cfg(feature = "managed-third-source-alpha")]
    {
        state.capacity.stable = None;
        state.capacity.first_load = None;
    }
}

pub fn release(lease: Lease) {
    let mut state = STATE.lock().unwrap();
    #[cfg(feature = "source-lifecycle-probe")]
    let observed = state.leases.iter().flatten().find(|live| live.token == lease.token).copied();
    let book_released = state.book.release(lease.token);
    for live in &mut state.leases {
        if live.is_some_and(|live| live.token == lease.token) { *live = None; }
    }
    HAS_CONSUMERS.store(state.leases.iter().any(Option::is_some), Ordering::Release);
    drop(state);
    #[cfg(feature = "source-lifecycle-probe")]
    if let Some(live) = observed {
        observe_item_lease("release", live, "explicit", Some(book_released));
    }
    #[cfg(not(feature = "source-lifecycle-probe"))]
    let _ = book_released;
}

fn retain(lease: Lease, consumer: Consumer) {
    let mut state = STATE.lock().unwrap();
    #[cfg(feature = "source-lifecycle-probe")]
    let mut observed = None;
    if let Some(live) = state.leases.iter_mut().flatten().find(|live| live.token == lease.token) {
        live.consumer = consumer;
        #[cfg(feature = "source-lifecycle-probe")]
        { observed = Some(*live); }
    }
    drop(state);
    #[cfg(feature = "source-lifecycle-probe")]
    if let Some(live) = observed {
        observe_item_lease("retain", live, "stored_consumer", None);
    }
}

// Records bookkeeping transitions only. No additional native object reads.
#[cfg(feature = "source-lifecycle-probe")]
fn observe_item_lease(event: &str, live: LiveLease, reason: &str, book_released: Option<bool>) {
    if let Consumer::Item { body, kind } = live.consumer {
        crate::logging::diagnostic(format_args!(
            "[SourceItemLease] event={} source={:?} lease={:?} object=0x{:x} item_kind=0x{:x} reason={} book_released={:?}",
            event, live.source, live.token, body, kind, reason, book_released));
    }
}

#[cfg(feature = "source-lifecycle-probe")]
const OBSERVED_LEASES: usize = 4;

// Bounded copies of the LiveLease cache mirror, not native-liveness samples
// or a copy of SourceBook's internal lease table. Count every omitted record.
#[cfg(feature = "source-lifecycle-probe")]
struct LeaseObservations {
    records: [Option<(LiveLease, Option<bool>)>; OBSERVED_LEASES],
    total: usize,
}

#[cfg(feature = "source-lifecycle-probe")]
impl LeaseObservations {
    fn new() -> Self { Self { records: [None; OBSERVED_LEASES], total: 0 } }

    fn push(&mut self, live: LiveLease, book_released: Option<bool>) {
        if let Some(record) = self.records.get_mut(self.total) {
            *record = Some((live, book_released));
        }
        self.total += 1;
    }

    fn recorded(&self) -> usize { self.total.min(OBSERVED_LEASES) }
    fn omitted(&self) -> usize { self.total.saturating_sub(OBSERVED_LEASES) }
}

#[cfg(feature = "source-lifecycle-probe")]
fn observe_released_items(records: &LeaseObservations, reason: &str, source_filter: Option<SourceToken>) {
    if records.total == 0 { return; }
    crate::logging::diagnostic(format_args!(
        "[SourceItemLease] release_batch reason={} source_filter={:?} total={} recorded={} omitted={} cache_mirror=true",
        reason, source_filter, records.total, records.recorded(), records.omitted()));
    for (live, book_released) in records.records.iter().flatten().copied() {
        observe_item_lease("release", live, reason, book_released);
    }
}

pub fn retain_hat(lease: Lease, body: u32) {
    retain(lease, Consumer::Hat { body, kind: lease.kind, slot: lease.slot });
}

pub fn retain_item(lease: Lease, body: u32, kind: i32) {
    retain(lease, Consumer::Item { body, kind });
}

/// The portrait lease stays live until the adapter releases it or the scene departs.
#[cfg(feature = "mii-hud-portrait-alpha")]
pub fn retain_portrait(lease: Lease) {
    retain(lease, Consumer::Portrait);
}

#[cfg(feature = "mii-hud-portrait-alpha")]
pub fn portrait_lease_valid(lease: Lease, holder: Holder) -> bool {
    let state = STATE.lock().unwrap();
    state.book.validate_lease(lease.token, holder)
        && state.leases.iter().flatten().any(|live| live.token == lease.token)
}

#[cfg(feature = "mii-hud-portrait-alpha")]
pub fn portrait_ready(lease: Lease, holder: Holder) -> Option<crate::source_lifetime::ReadyToken> {
    let state = STATE.lock().unwrap();
    if !state.book.validate_lease(lease.token, holder) {
        return None;
    }
    state.book.ready(lease.slot as usize).ok()
}

/// Native copy changes retain earlier kinds' object groups. Copy loss destroys
/// their objects and clears the groups; unused native entries have key -1.
/// Read only from a freshly resolved Kirby, and retain on an unknown layout.
/// The bounds cover the roster and observed native records without allowing an
/// unexpected count to turn a frame callback into an unbounded memory walk.
unsafe fn retained_copy_kinds(body: *mut app::BattleObjectModuleAccessor) -> Option<u128> {
    let context = WorkModule::get_int64(body, 0x10000106) as *const u8;
    if !aligned(context) { return None; }
    let container = read::<*const u8>(context, 0x258);
    if !aligned(container) { return None; }
    let count = usize::try_from(read::<i32>(container, 0x10)).ok()?;
    if count > 128 { return None; }
    let entries = read::<*const u8>(container, 0x18);
    if count != 0 && !aligned(entries) { return None; }
    let mut kinds = 0u128;
    for index in 0..count {
        let entry = entries.add(index * 0x10);
        let key = read::<i32>(entry, 0);
        let group = read::<*const u8>(entry, 8);
        if key == -1 && group.is_null() { continue; }
        let bit = if key == -1 { 0 } else {
            1u128.checked_shl(u32::try_from(key).ok()?)?
        };
        if !aligned(group) { return None; }
        let count = usize::try_from(read::<i32>(group, 0x10)).ok()?;
        if count > 32 { return None; }
        let records = read::<*const u8>(group, 0x18);
        if count != 0 && !aligned(records) { return None; }
        for record in 0..count {
            let objects = read::<i64>(records, record * 0x348 + 0x18);
            if objects < 0 { return None; }
            if key == -1 && objects != 0 { return None; }
            if objects != 0 { kinds |= bit; break; }
        }
    }
    Some(kinds)
}

/// Called after Kirby's original frame callback, while every old source is still
/// leased. Use native full copy loss (the silent KO variant), including its
/// kind-specific cleanup, instead of clearing fields or just changing the hat.
/// This candidate requires the source-reuse/KO runtime regression before release.
pub unsafe fn clear_hat_for_switch(body: *mut app::BattleObjectModuleAccessor)
    -> Result<(), &'static str>
{
    if !WorkModule::is_flag(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_FLAG_COPY) {
        return if retained_copy_kinds(body) == Some(0) { Ok(()) }
            else { Err("Kirby's previous copy is still being cleared") };
    }
    let Some(before) = retained_copy_kinds(body) else {
        return Err("Kirby's previous copy resources are not ready");
    };
    let object = crate::common::try_get_battle_object((*body).battle_object_id)
        .filter(|object| object.module_accessor == body)
        .ok_or("Kirby is still changing characters")?;
    let base = super::BASE.load(Ordering::Acquire);
    if base == 0 || std::slice::from_raw_parts((base + 0xb96770) as *const u32, 4)
        != [0xd10243ff, 0xa9036ffc, 0xa90467fa, 0xa9055ff8]
    { return Err("Native Kirby copy cleanup is unavailable for this game build"); }
    let clear: unsafe extern "C" fn(*mut app::Fighter, i32) =
        std::mem::transmute(base + 0xb96770);
    // The P1 sweep can run later in this same frame. Pin through that sweep and
    // the next one if this request is cancelled before its next Kirby callback.
    {
        let mut state = STATE.lock().unwrap();
        for live in state.leases.iter_mut().flatten() {
            if matches!(live.consumer, Consumer::Hat { body: id, .. }
                if id == (*body).battle_object_id) { live.settle_polls = 2; }
        }
    }
    clear(object as *const app::BattleObject as *mut app::Fighter, 0);
    let after = retained_copy_kinds(body);
    crate::logging::diagnostic(format_args!(
        "[KirbyHats] native cleanup body=0x{:x} groups_before=0x{:x} groups_after={:?}",
        (*body).battle_object_id, before, after));
    if WorkModule::is_flag(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_FLAG_COPY)
        || after != Some(0)
    { return Err("Kirby's previous copy is still being cleared"); }
    Ok(())
}

/// Recheck on a later Kirby callback before preparing a replacement. Native
/// cleanup has returned and had a frame to settle; no new copy owns these leases.
/// `leases_released` belongs to the same unstarted pending request/body.
/// A waiting request has not acquired its replacement hat lease.
pub unsafe fn finish_hat_clear(body: *mut app::BattleObjectModuleAccessor, leases_released: bool)
    -> Result<(), &'static str>
{
    if WorkModule::is_flag(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_FLAG_COPY)
        || retained_copy_kinds(body) != Some(0)
    { return Err("Kirby's previous copy is still being cleared"); }
    // Native copy state must still be validated before skipping completed work.
    if leases_released { return Ok(()); }
    let mut state = STATE.lock().unwrap();
    let mut released = 0;
    for index in 0..LEASES {
        let Some(live) = state.leases[index] else { continue; };
        if matches!(live.consumer, Consumer::Hat { body: id, .. }
            if id == (*body).battle_object_id)
        {
            state.book.release(live.token);
            state.leases[index] = None;
            released += 1;
        }
    }
    HAS_CONSUMERS.store(state.leases.iter().any(Option::is_some), Ordering::Release);
    if released != 0 {
        crate::logging::diagnostic(format_args!(
            "[KirbyHats] cleanup settled body=0x{:x} released={}",
            (*body).battle_object_id, released));
    }
    Ok(())
}

// At most one group walk per P1/CPU Kirby in this callback, even when several
// old hats retain leases. No pointer or observation survives the callback.
type HatObservation = Option<(u32, Option<u128>)>;

unsafe fn consumer_alive(consumer: Consumer, hats: &mut [HatObservation; 2]) -> bool {
    let (id, category) = match consumer {
        Consumer::Reserved => return true,
        // Released by the portrait adapter after the queue drains, or by scene departure.
        #[cfg(feature = "mii-hud-portrait-alpha")]
        Consumer::Portrait => return true,
        Consumer::Hat { body, .. } => (body, *BATTLE_OBJECT_CATEGORY_FIGHTER),
        Consumer::Item { body, .. } => (body, *BATTLE_OBJECT_CATEGORY_ITEM),
    };
    if !sv_battle_object::is_active(id) { return false; }
    let body = sv_battle_object::module_accessor(id);
    if body.is_null() || app::utility::get_category(&mut *body) != category { return false; }
    match consumer {
        Consumer::Reserved => true,
        #[cfg(feature = "mii-hud-portrait-alpha")]
        Consumer::Portrait => true,
        Consumer::Item { kind, .. } => app::utility::get_kind(&mut *body) == kind,
        Consumer::Hat { kind, slot, .. } => {
            if app::utility::get_kind(&mut *body) != *FIGHTER_KIND_KIRBY { return false; }
            // Keep both old and new sources across native copy reconstruction.
            if WorkModule::is_flag(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_FLAG_COPY_ON_START)
                || (WorkModule::is_flag(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_FLAG_COPY)
                    && WorkModule::get_int(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_INT_COPY_CHARA) as u32 == kind
                    && WorkModule::get_int(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_INT_COPY_CHARA_SLOT_NO) as u32 == slot)
            { return true; }
            let observed = if let Some((_, kinds)) = hats.iter().flatten().find(|(seen, _)| *seen == id) {
                *kinds
            } else if let Some(observation) = hats.iter_mut().find(|seen| seen.is_none()) {
                let kinds = retained_copy_kinds(body);
                *observation = Some((id, kinds));
                kinds
            } else {
                None // Another live body is departing; wait for native destruction.
            };
            observed.is_none_or(|kinds| kinds & (1u128 << kind) != 0)
        }
    }
}

/// Once on the current P1 frame, not once per helper or per fighter. Thrown
/// items retain their lease until their actual native item object ends.
pub unsafe fn on_frame(body: *mut app::BattleObjectModuleAccessor) {
    if !HAS_CONSUMERS.load(Ordering::Acquire) { return; }
    if crate::common::try_get_module_accessor(crate::common::consts::FighterId::Player)
        != Some(body) { return; }
    let mut state = STATE.lock().unwrap();
    let mut hats = [None; 2];
    #[cfg(feature = "source-lifecycle-probe")]
    let mut released_items = None;
    for index in 0..LEASES {
        let Some(mut live) = state.leases[index] else { continue; };
        if live.settle_polls != 0 {
            live.settle_polls -= 1;
            state.leases[index] = Some(live);
            continue;
        }
        if !consumer_alive(live.consumer, &mut hats) {
            let book_released = state.book.release(live.token);
            #[cfg(feature = "source-lifecycle-probe")]
            if matches!(live.consumer, Consumer::Item { .. }) {
                released_items.get_or_insert_with(LeaseObservations::new)
                    .push(live, Some(book_released));
            }
            #[cfg(not(feature = "source-lifecycle-probe"))]
            let _ = book_released;
            state.leases[index] = None;
        }
    }
    HAS_CONSUMERS.store(state.leases.iter().any(Option::is_some), Ordering::Release);
    drop(state);
    #[cfg(feature = "source-lifecycle-probe")]
    if let Some(released) = released_items {
        observe_released_items(&released, "existing_consumer_alive_false", None);
    }
}

pub fn scene_changed() {
    #[cfg(feature="mii-managed-helper-alpha")]
    super::mii_managed_helper::depart();
    let mut state = STATE.lock().unwrap();
    #[cfg(feature = "mii-managed-source-coexist-alpha")]
    let mut registry = coexist::registry();
    #[cfg(feature = "source-lifecycle-probe")]
    let released = {
        let mut records = LeaseObservations::new();
        for live in state.leases.iter().flatten().copied() {
            if matches!(live.consumer, Consumer::Item { .. }) { records.push(live, None); }
        }
        records
    };
    state.wants = [None; CHANNELS];
    #[cfg(feature="mii-managed-helper-alpha")]
    { state.refusals = [None; CHANNELS]; }
    state.leases = [None; LEASES];
    state.failed = 0;
    #[cfg(feature = "managed-third-source-alpha")]
    {
        state.capacity.stable = None;
        state.capacity.attempted = None;
        state.capacity.first_load = None;
        // Keep the old reservation until both actual native boundaries arrive.
    }
    state
        .book
        .depart_scene()
        .expect("native scene epoch exhausted");
    #[cfg(feature = "mii-managed-source-coexist-alpha")]
    {
        for slot in registry.helpers.layout.helpers().map(|id|id as usize) {
            let helper = state.sources[slot].map_or(coexist::helpers::Helper::Unmanaged,
                |source| coexist::helpers::Helper::Unavailable { token: source.token, phase: Phase::Departed });
            registry.source(state.book.epoch(), slot, helper);
        }
        drop(registry);
    }
    HAS_CONSUMERS.store(false, Ordering::Release);
    // In-flight native loads stay owned until their callback completes or the
    // real native destructor runs. No old intent is published after departure.
    drop(state);
    #[cfg(feature = "source-lifecycle-probe")]
    observe_released_items(&released, "scene_departure_book_invalidated", None);
}

/// Preserve the old probe's Training owner2 automation suppression independently
/// of its diagnostics. This is not resource admission or native source ownership.
#[cfg(any(feature = "source-owner-probe", feature = "managed-sources"))]
pub unsafe fn legacy_training_owner_reserved(body: &mut app::BattleObjectModuleAccessor) -> bool {
    crate::common::is_native_training_mode()
        && app::utility::get_category(body) == *BATTLE_OBJECT_CATEGORY_FIGHTER
        && WorkModule::get_int(body, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID) == 2
}

/// Skip plugin automations for an owned helper body. Native simulation stays
/// untouched. Owned 2/3/5/6 stay reserved with no scene gate, including while
/// they depart. Unowned entries keep their original callbacks.
pub unsafe fn reserved(body: &mut app::BattleObjectModuleAccessor) -> bool {
    if !cfg!(feature = "managed-sources") { return false; }
    let slot = WorkModule::get_int(body, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID);
    let Ok(slot) = u32::try_from(slot) else { return false; };
    if !callback_slot(slot) { return false; }
    let source = STATE.lock().unwrap().sources[slot as usize];
    source.is_some_and(|source| native_source::entry(slot)
        .is_some_and(|entry| source.token.entry() == NativeEntry(entry as u64)))
}

/// Finish a load this entry already owns. True means this callback must not
/// start another install. Pending Mii completion reaches the normal tick commit;
/// a SourceBook complete() failure keeps the previous tick copy.
unsafe fn service_owned(_ctx: &ServiceEntryContext, entry: *mut u8, slot: u32) -> bool {
    let index = slot as usize;
    let (original, departed) = {
        let state = STATE.lock().unwrap();
        let Some(source) = state.sources[index] else {
            return false;
        };
        if source.token.entry() != NativeEntry(entry as u64) {
            return true;
        }
        if source.install.is_none() {
            return state.book.phase(index) == Ok(Phase::Departed);
        }
        (source, !state.book.is_current(source.token.epoch()))
    };
    #[cfg(feature="mii-managed-helper-alpha")]
    if original.configuration.is_some()
        && super::mii_managed_helper::retained_terminal(original.token) {
        return true;
    }
    let mut source = original;
    source.ticks = source.ticks.saturating_add(1);
    let native_done = native_source::parked_ready(entry, slot, source.kind);
    #[cfg(feature = "managed-third-source-alpha")]
    let capacity_observation = if slot == 7 {
        let state = STATE.lock().unwrap();
        (
            reservation(&state),
            state
                .capacity
                .load
                .filter(|(token, _)| *token == source.token)
                .map(|(_, before)| before),
        )
    } else {
        (None, None)
    };
    #[cfg(feature = "managed-third-source-alpha")]
    let complete = if slot == 7 {
        // The diagnostic limit and scene departure never release a pending
        // entry7 load. Only full acceptance or its actual destructor does.
        !departed
            && native_done
            && capacity_observation
                .0
                .zip(capacity_observation.1)
                .is_some_and(|(owner, before)| {
                    capacity::loaded(_ctx, entry, owner, before, source.kind)
                })
    } else {
        native_done && (departed || native_source::ready(entry, slot, source.kind).is_some())
    };
    #[cfg(not(feature = "managed-third-source-alpha"))]
    let complete =
        native_done && (departed || native_source::ready(entry, slot, source.kind).is_some());
    #[cfg(feature = "managed-third-source-alpha")]
    let stalled = if slot == 7 && source.ticks == 900 {
        Some(capacity::stalled_checks(
            _ctx,
            entry,
            capacity_observation.0,
            capacity_observation.1,
            source.kind,
        ))
    } else {
        None
    };
    #[cfg(feature = "managed-grouped-source-alpha")]
    let ready_actors = (slot == 7 && complete).then(|| capacity::actors(entry, source.kind)).flatten();
    #[cfg(feature = "managed-grouped-source-alpha")]
    let complete = complete && (slot != 7 || ready_actors.is_some());
    #[cfg(feature="mii-managed-helper-alpha")]
    let mii_proof=if source.configuration.is_some() && complete && !departed {
        match super::mii_managed_helper::completed(source.token,source.ticks) {
            Ok(proof)=>proof,Err(reason)=>{
                super::mii_managed_helper::refuse_completion(source.token,reason);
                return true;
            }
        }
    } else {None};
    #[cfg(feature="mii-managed-helper-alpha")]
    let complete=complete && (source.configuration.is_none() || mii_proof.is_some());
    #[cfg(feature = "mii-managed-source-coexist-alpha")]
    let proof = if complete && !departed {
        coexist::helpers::observe(entry as usize, slot).filter(|proof| {
            #[cfg(feature="mii-managed-helper-alpha")]
            if mii_proof.is_some() {
                return source.original.is_some_and(|old|coexist::helpers::OriginalRow {
                    index:proof.original.index,..old}==proof.original);
            }
            Some(proof.original)==source.original
        })
    } else { None };
    #[cfg(feature="mii-managed-helper-alpha")]
    if mii_proof.is_some() && proof.is_none() {
        super::mii_managed_helper::refuse_completion(source.token,"managed original row/provenance changed at completion");
        return true;
    }
    #[cfg(feature="mii-managed-helper-alpha")]
    let complete=complete && (source.configuration.is_none() || proof.is_some());
    let mut state = STATE.lock().unwrap();
    if state.sources[index] != Some(original) {
        return true;
    }
    #[cfg(feature = "mii-managed-source-coexist-alpha")]
    if !source.install.is_some_and(|token| INSTALL.owns(token)) { return true; }
    #[cfg(feature = "mii-managed-source-coexist-alpha")]
    let mut registry = (complete || (native_done && source.ticks >= 900 && !departed))
        .then(coexist::registry);
    let departed = !state.book.is_current(source.token.epoch());
    let mut finished = None;
    let mut event = None;
    if complete && (!cfg!(feature = "managed-third-source-alpha") || slot != 7 || !departed)
        && (source.configuration.is_none() || !departed) {
        if !departed {
            if state.book.complete(source.load).is_err() {
                return true;
            }
            event = Some("ready");
        }
        finished = source.install.take();
    } else if native_done
        && source.configuration.is_none()
        && source.ticks >= 900
        && !departed
        && (!cfg!(feature = "managed-third-source-alpha") || slot != 7)
    {
        if state.book.fail(source.load).is_ok() {
            state.failed |= 1u128 << source.kind;
            finished = source.install.take();
            for want in &mut state.wants {
                if want.is_some_and(|want| want.kind == source.kind) {
                    *want = None;
                }
            }
            event = Some("native completion without usable resources");
        }
    } else if source.ticks == 900 {
        event = Some("stalled; native ownership retained");
    }
    #[cfg(feature="mii-managed-helper-alpha")]
    if complete && !departed && source.configuration.is_some() {
        // Only a completed selector transaction can advance this managed row's index anchor.
        // The visible roles' immutable saved-original snapshots remain untouched.
        source.original=proof.map(|p|p.original);
    }
    state.sources[index] = Some(source);
    #[cfg(feature = "mii-managed-source-coexist-alpha")]
    {
        if let Some(registry) = registry.as_mut() {
            let helper = match (state.book.ready(index), state.book.key(index), proof) {
                #[cfg(feature="mii-managed-helper-alpha")]
                (Ok(ready),Ok(Some(key)),Some(proof))
                    if !departed && ready.source()==source.token && proof.kind==key.kind
                        && proof.costume==key.costume && mii_proof.is_some() =>
                    coexist::helpers::Helper::Ready {ready,key,proof},
                (Ok(ready), Ok(Some(key)), Some(proof))
                    if !departed && ready.source() == source.token && proof.kind == key.kind
                        && proof.costume == key.costume && !(0x48..=0x4a).contains(&key.kind) =>
                    coexist::helpers::Helper::Ready { ready, key, proof },
                _ => coexist::helpers::Helper::Unavailable { token: source.token,
                    phase: state.book.phase(index).expect("registered source slot") },
            };
            registry.source(state.book.epoch(), index, helper);
            #[cfg(feature="mii-managed-helper-alpha")]
            if matches!(helper,coexist::helpers::Helper::Ready {..}) {
                if let Some(ordinal)=registry.helpers.ordinal(index) {registry.helpers.miis[ordinal]=mii_proof;}
            }
        }
        drop(registry);
    }
    #[cfg(feature="mii-managed-helper-alpha")]
    if finished.is_some() && source.configuration.is_some() {
        super::mii_managed_helper::release(source.token);
    }
    #[cfg(feature = "managed-third-source-alpha")]
    if slot == 7 && finished.is_some() {
        state.capacity.load = None;
        if event == Some("ready") {
            #[cfg(feature = "managed-grouped-source-alpha")]
            let count = ready_actors.map_or(0, |actors| actors.count());
            #[cfg(not(feature = "managed-grouped-source-alpha"))]
            let count = 1;
            state.capacity.observation.ready_actors =
                state.capacity.observation.ready_actors.saturating_add(count);
            #[cfg(feature = "managed-grouped-source-alpha")]
            { state.capacity.observation.current_actors = ready_actors; }
        }
    }
    drop(state);
    #[cfg(all(feature = "managed-third-source-alpha", feature = "source-capacity-probe"))]
    if slot == 7 && matches!(event, Some("ready" | "stalled; native ownership retained")) {
        capacity::observe_load_inputs(source.token, source.kind, event.unwrap(), capacity_observation.1);
    }
    #[cfg(feature = "managed-grouped-source-alpha")]
    if slot == 7 && event == Some("ready") {
        crate::logging::diagnostic(format_args!(
            "[ManagedSourceCleanup] canonical ready source={:?} kind=0x{:x} actors={:x?}",
            source.token, source.kind, ready_actors));
    }
    #[cfg(feature="mii-managed-helper-alpha")]
    if event==Some("ready") {
        if let Some(proof)=mii_proof {
            crate::logging::diagnostic(format_args!("[MiiHelper] exact parked completion source={:?} proof={:?}",source.token,proof));
        }
    }
    if let Some(token) = finished {
        finish_install(token, "managed source completion");
    }
    if let Some(event) = event {
        #[cfg(feature = "managed-third-source-alpha")]
        if let Some(checks) = stalled.filter(|_| event == "stalled; native ownership retained") {
            crate::logging::diagnostic(format_args!(
                "[CharacterSources] stalled slot=7 kind=0x{:x} ticks={} reservation={} preflight={} owned={} snapshot={:?} scene={:?} original_resources={:?} inputs={:?} ready={:?} native_done={} departed={}; native ownership retained",
                source.kind, source.ticks, checks.reservation, checks.preflight, checks.owned,
                checks.snapshot, checks.scene, checks.original_resources, checks.inputs,
                checks.ready, native_done, departed));
            return true;
        }
        crate::logging::diagnostic(format_args!(
            "[CharacterSources] {} slot={} kind=0x{:x} ticks={}",
            event, slot, source.kind, source.ticks
        ));
    }
    true
}

/// Only a proven configuration mismatch retires an unsubmitted exact intent.
/// Keep a per-channel receipt so its consumer observes Err instead of requeueing.
#[cfg(feature="mii-managed-helper-alpha")]
fn refuse_want(wanted: Want, slot: u32, reason: &'static str) {
    let mut state = STATE.lock().unwrap();
    let mut refused = false;
    for channel in 0..CHANNELS {
        if state.wants[channel] == Some(wanted) {
            state.wants[channel] = None;
            state.refusals[channel] = Some((wanted, reason));
            refused = true;
        }
    }
    drop(state);
    if refused {
        crate::logging::diagnostic(format_args!(
            "[MiiHelper] admission refused slot={} epoch={:?} kind={:#x} reason={}",
            slot, wanted.epoch, wanted.kind, reason));
    }
}

fn slot_allowed(avoid: Option<u32>, slot: u32) -> bool {
    #[cfg(feature = "mii-hud-portrait-alpha")]
    { super::mii_hud_portrait_policy::slot_may_install(avoid, slot) }
    #[cfg(not(feature = "mii-hud-portrait-alpha"))]
    { let _ = (avoid, slot); true }
}

#[cfg(not(feature = "mii-hud-portrait-alpha"))]
fn pending_request_for(state: &State, slot: Option<u32>, menu_open: bool) -> Option<Want> {
    let _ = slot;
    if menu_open { None } else { pending_request(state) }
}

#[cfg(not(feature = "mii-hud-portrait-alpha"))]
fn pending_request(state: &State) -> Option<Want> {
    state.wants.iter().flatten().copied().find(|want| {
        state.book.is_current(want.epoch) && !state.sources.iter().flatten().any(|source|
            source.kind == want.kind && source.configuration==want.configuration
                && state.book.is_current(source.token.epoch())
                && state.book.phase(source.token.slot()) != Ok(Phase::Failed))
    })
}

#[cfg(all(feature = "mii-hud-portrait-alpha", feature = "managed-third-source-alpha"))]
fn pending_request(state: &State) -> Option<Want> {
    pending_request_for(state, None, false)
}

/// The first unsatisfied want this slot may install. An avoided slot neither
/// satisfies the want nor installs it. `slot == None` is the previous global walk.
#[cfg(feature = "mii-hud-portrait-alpha")]
fn pending_request_for(state: &State, slot: Option<u32>, menu_open: bool) -> Option<Want> {
    // Exact portrait requests depend on the loaded fighter, not menu preferences.
    // Hats/items still wait for menu close, even when a hat carries a Mii payload.
    // Filter before finding a want so a pending hat cannot hide a portrait.
    state.wants.iter().enumerate().filter_map(|(channel, want)| {
        want.filter(|want| super::mii_hud_portrait_policy::source_channel_admitted(
            channel, want.configuration.is_some(), menu_open))
    }).find(|want| {
        slot.is_none_or(|slot| super::mii_hud_portrait_policy::slot_may_install(want.avoid, slot))
            && state.book.is_current(want.epoch) && !state.sources.iter().flatten().any(|source|
            source.kind == want.kind && source.configuration==want.configuration
                && super::mii_hud_portrait_policy::slot_may_install(want.avoid, source.token.slot() as u32)
                && state.book.is_current(source.token.epoch())
                && state.book.phase(source.token.slot()) != Ok(Phase::Failed))
    })
}

#[cfg(not(feature = "mii-hud-portrait-alpha"))]
fn ready_still_requested(state: &State, index: usize) -> bool {
    state.book.phase(index) == Ok(Phase::Ready) && state.sources[index].is_some_and(|source|
        state.wants.iter().flatten().any(|want| want.kind == source.kind && want.configuration==source.configuration))
}

#[cfg(feature = "mii-hud-portrait-alpha")]
fn ready_still_requested(state: &State, index: usize) -> bool {
    state.book.phase(index) == Ok(Phase::Ready) && state.sources[index].is_some_and(|source|
        state.wants.iter().flatten().any(|want|
            super::mii_hud_portrait_policy::slot_may_install(want.avoid, index as u32)
                && want.kind == source.kind && want.configuration==source.configuration))
}

/// Hard gates apply to every source. Only an exact portrait may pass the
/// separate mod-menu gate; it never relaxes native UI, save/preview ownership,
/// native readiness, or the single installer. Also used by the portrait wait
/// budget so deliberately deferred acquisition cannot expire during playback.
unsafe fn admission_blocker() -> Option<&'static str> {
    if INSTALL.active() { Some("another source install") }
    else if !crate::common::is_ready_go() { Some("ready-go") }
    else if super::lobby::native_ui_busy() { Some("native UI") }
    else if super::training::save_states::is_loading() { Some("save-state restore") }
    else if super::training::recording_or_playback() { Some("recording or playback") }
    else if crate::replay_preview::occupied() { Some("replay preview") }
    else { None }
}

#[cfg(feature = "mii-hud-portrait-alpha")]
pub unsafe fn portrait_admission_blocker() -> Option<&'static str> {
    admission_blocker()
}

#[cfg(feature = "managed-third-source-alpha")]
unsafe fn admission_blocked() -> bool {
    admission_blocker().is_some() || super::menu::captures_input()
}

unsafe fn visible_ready(backend: Backend) -> bool {
    let [left, right] = backend.visible();
    let Some(first) = native_source::entry(left) else { return false; };
    let Some(second) = native_source::entry(right) else { return false; };
    if backend.waiting() && first == second { return false; }
    ready_forms(first, left).is_some() && ready_forms(second, right).is_some()
}

/// Training's first install still requires outer 0, guard 0, this entry, and
/// visible 0/1. Its later installs require parked_ready of the managed kind.
/// A waiting first install also requires loader 14 and parked_ready of descriptor
/// +0x18. Every new waiting install requires distinct ready 0/4 and the witnessed
/// manager shape before any selection-table read.
unsafe fn native_admission(
    entry: *const u8, slot: u32, backend: Backend, managed: Option<u32>,
) -> bool {
    if let Some(kind) = managed {
        if !native_source::parked_ready(entry, slot, kind) { return false; }
        if backend.waiting() && !visible_ready(backend) { return false; }
    } else {
        if read::<u8>(entry, 0x5920) != 0 || read::<u8>(entry, 0x5935) != 0 { return false; }
        if backend.waiting() && read::<u8>(entry, 0x591c) != 14 { return false; }
        if native_source::entry(slot) != Some(entry) { return false; }
        if !visible_ready(backend) { return false; }
        if backend.waiting()
            && !native_source::parked_ready(entry, slot, read::<u32>(entry, 0x18))
        { return false; }
    }
    !backend.waiting() || waiting_layout(entry, slot, backend)
}

#[cfg(feature = "managed-third-source-alpha")]
fn helpers_unavailable(state: &State, backend: Backend) -> bool {
    backend.helpers().iter().copied().all(|slot| {
        let index = slot as usize;
        ready_still_requested(state, index) || state.book.can_replace(index) != Ok(true)
    })
}

/// Creation has its own one-shot epoch intent. A failed post-call qualification
/// leaves the shared native claim held until observed retirement, never guessed
/// free. Only a fresh native scene may try again.
#[cfg(feature = "managed-third-source-alpha")]
unsafe fn create_capacity(ctx: &ServiceEntryContext, entry: *const u8, slot: u32, wanted: Want) {
    let Ok(backend) = qualified_backend() else { return; };
    let helpers = backend.helpers();
    if !helpers.contains(&slot) || !candidates(backend).contains(&7)
        || !capacity::source_kind(wanted.kind)
        || admission_blocked()
    {
        return;
    }
    {
        let state = STATE.lock().unwrap();
        if state.capacity.reservation.is_some()
            || state.capacity.attempted == Some(wanted.epoch)
            || pending_request(&state) != Some(wanted)
            || !helpers_unavailable(&state, backend)
        {
            return;
        }
    }
    let Some(before) = capacity::preflight(ctx, entry, slot) else {
        STATE.lock().unwrap().capacity.stable = None;
        return;
    };
    {
        let mut state = STATE.lock().unwrap();
        if pending_request(&state) != Some(wanted)
            || !helpers_unavailable(&state, backend)
            || state.capacity.reservation.is_some()
            || state.capacity.attempted == Some(wanted.epoch)
        {
            state.capacity.stable = None;
            return;
        }
        let (ticks, seen) = state.capacity.stable
            .filter(|(old, observed, _, _)| *old == wanted && *observed == before)
            .map_or((1, 0), |(_, _, ticks, seen)| (ticks.saturating_add(1), seen));
        let seen = seen | u8::from(slot == helpers[0]) | (u8::from(slot == helpers[1]) << 1);
        state.capacity.stable = Some((wanted, before, ticks, seen));
        if ticks < 30 || (backend.waiting() && seen != 3) {
            return;
        }
    }
    let Some(install) = INSTALL.try_begin(false) else {
        return;
    };
    {
        let mut state = STATE.lock().unwrap();
        if pending_request(&state) != Some(wanted)
            || !helpers_unavailable(&state, backend)
            || state.capacity.reservation.is_some()
            || state.capacity.attempted == Some(wanted.epoch)
        {
            drop(state);
            finish_install(install, "capacity intent changed");
            return;
        }
        state.capacity.attempted = Some(wanted.epoch);
        state.capacity.stable = None;
    }
    // Native helper and all its possible reentrant callbacks run without STATE.
    let created = capacity::create_reserved(ctx, entry, slot, before);
    if let Some(value) = created {
        let mut state = STATE.lock().unwrap();
        state.capacity.reservation = Some((wanted.epoch, value));
        state.capacity.first_load = None;
        state.capacity.observation = CleanupObservation::new();
    }
    finish_install(install, "native capacity creation returned");
    crate::logging::diagnostic(format_args!(
        "[CharacterSources] capacity creation epoch={:?} backend={:?} qualified={} entry=0x{:x} owner=0x{:x}",
        wanted.epoch, backend,
        created.is_some(),
        created.map_or(0, |r| r.entry),
        created.map_or(0, |r| r.resource)
    ));
}

/// Once per pending request/source/condition, only for replacement of a live7.
/// All supplementary native reads and formatting occur outside STATE.
#[cfg(all(feature = "managed-third-source-alpha", feature = "source-capacity-probe"))]
unsafe fn admission_declined(
    ctx: &ServiceEntryContext, entry: *const u8, wanted: Want, previous: Option<Source>,
    condition: u8, reason: &str,
) {
    let Some(previous) = previous else { return; };
    let bit = 1u32 << condition;
    let (seen, owner, phase, replaceable, requested) = {
        let state = STATE.lock().unwrap();
        if state.sources[7] != Some(previous) || pending_request(&state) != Some(wanted) {
            return;
        }
        let owner = reservation(&state);
        let seen = AdmissionSeen {
            wanted, previous: previous.token, reservation: owner.map(|value| value.entry),
            conditions: 0,
        };
        if state.capacity.admission.is_some_and(|old|
            AdmissionSeen { conditions: 0, ..old } == seen && old.conditions & bit != 0)
        { return; }
        (seen, owner, state.book.phase(7), state.book.can_replace(7),
            ready_still_requested(&state, 7))
    };
    let detail = capacity::admission_detail(ctx, entry, previous.kind, owner);
    let mut state = STATE.lock().unwrap();
    if state.sources[7] != Some(previous) || pending_request(&state) != Some(wanted)
        || reservation(&state) != owner
        || state.book.phase(7) != phase || state.book.can_replace(7) != replaceable
        || ready_still_requested(&state, 7) != requested
    { return; }
    let old = state.capacity.admission.filter(|old|
        AdmissionSeen { conditions: 0, ..*old } == seen).map_or(0, |old| old.conditions);
    if old & bit != 0 { return; }
    state.capacity.admission = Some(AdmissionSeen { conditions: old | bit, ..seen });
    drop(state);
    crate::logging::diagnostic(format_args!(
        "[ManagedSourceAdmission] declined want_epoch={:?} want_kind=0x{:x} previous={:?} previous_kind=0x{:x} pending_install={} condition={} reason={} phase={:?} can_replace={:?} still_requested={} reservation_entry={:x?} detail(current_entry,dispatch,owned_error,descriptor,parked,canonical,inputs)={:x?}",
        wanted.epoch, wanted.kind, previous.token, previous.kind, previous.install.is_some(), condition, reason,
        phase, replaceable, requested, seen.reservation, detail));
}

/// Only called with the current native loader callback's argument.
/// Already-owned loads run before any scene, mode or ownership gate.
pub unsafe fn service(ctx: &ServiceEntryContext, entry: *mut u8, slot: u32) {
    if !cfg!(feature = "managed-sources") || !callback_slot(slot) {
        return;
    }
    let index = slot as usize;
    // Every entry7 callback starts with no saved stability. Restore it only
    // after a valid first-load observation; any early decline resets the run.
    #[cfg(feature = "managed-third-source-alpha")]
    let prior_first = if slot == 7 {
        STATE.lock().unwrap().capacity.first_load.take()
    } else {
        None
    };
    if service_owned(ctx, entry, slot) {
        #[cfg(all(feature = "managed-third-source-alpha", feature = "source-capacity-probe"))]
        if slot == 7 {
            let pending = {
                let state = STATE.lock().unwrap();
                pending_request(&state).map(|wanted| (wanted, state.sources[7]))
            };
            if let Some((wanted, previous)) = pending {
                admission_declined(ctx, entry, wanted, previous, 27,
                    "existing source owns a load or is departed");
            }
        }
        return;
    }
    let initial = {
        let state = STATE.lock().unwrap();
        // Idle: no native snapshots, log formatting, or allocation.
        // Per-slot: a want this helper may take is not hidden by an earlier avoided want.
        let Some(wanted) = pending_request_for(&state, Some(slot), false) else {
            return;
        };
        (
            wanted,
            !ready_still_requested(&state, index) && state.book.can_replace(index) == Ok(true),
            state.sources[index],
        )
    };
    // Check the menu only after finding work: idle loader callbacks retain the
    // original allocation-free fast path. Re-select under the same channel rule
    // used at commit, without holding the source mutex while reading menu state.
    let menu_open = super::menu::captures_input();
    let (wanted, replaceable, previous) = if menu_open {
        let state = STATE.lock().unwrap();
        let Some(wanted) = pending_request_for(&state, Some(slot), true) else { return; };
        (wanted, !ready_still_requested(&state, index) && state.book.can_replace(index) == Ok(true),
            state.sources[index])
    } else { initial };
    #[cfg(all(feature = "managed-third-source-alpha", feature = "source-capacity-probe"))]
    macro_rules! declined {
        ($code:expr, $reason:expr) => {
            if slot == 7 { admission_declined(ctx, entry, wanted, previous, $code, $reason); }
        };
    }
    if !replaceable {
        #[cfg(all(feature = "managed-third-source-alpha", feature = "source-capacity-probe"))]
        declined!(0, "book not replaceable or current source still requested");
        #[cfg(feature = "managed-third-source-alpha")]
        create_capacity(ctx, entry, slot, wanted);
        return;
    }
    if admission_blocker().is_some() {
        #[cfg(all(feature = "managed-third-source-alpha", feature = "source-capacity-probe"))]
        declined!(1, "install/ready-go/native-ui/save-state/playback gate");
        return;
    }
    let Ok(backend) = qualified_backend() else {
        #[cfg(all(feature = "managed-third-source-alpha", feature = "source-capacity-probe"))]
        declined!(2, "scene backend not qualified");
        return;
    };
    #[cfg(feature="mii-managed-helper-alpha")]
    if (backend==Backend::Training || wanted.configuration.is_some())
        && !super::mii_managed_helper::admission_ready(wanted.configuration.is_some()) {return;}
    if !candidates(backend).contains(&slot) {
        #[cfg(all(feature = "managed-third-source-alpha", feature = "source-capacity-probe"))]
        declined!(3, "slot not a candidate for this backend");
        return;
    }
    #[cfg(feature = "managed-third-source-alpha")]
    let (capacity_load, first_load_callbacks) = if slot == 7 {
        // Only this module's current reservation can admit 7. An isolated
        // R3+X experiment's entry never becomes a managed source.
        if !capacity::source_kind(wanted.kind) {
            #[cfg(feature = "source-capacity-probe")]
            declined!(4, "requested source kind not supported");
            return;
        }
        let owner = { reservation(&STATE.lock().unwrap()) };
        let Some(owner) = owner else {
            #[cfg(feature = "source-capacity-probe")]
            declined!(5, "no current owned reservation");
            return;
        };
        #[cfg(feature = "source-capacity-probe")]
        let before = match capacity::load_preflight_checked(
            ctx, entry, owner, previous.map(|source| source.kind))
        {
            Ok(before) => before,
            Err(reason) => {
                declined!(8 + reason as u8, match reason {
                    capacity::LoadDecline::Owned => "reservation roots or input role identity",
                    capacity::LoadDecline::Descriptor => "previous descriptor",
                    capacity::LoadDecline::PreviousKind => "previous source kind",
                    capacity::LoadDecline::Parked => "previous native parked readiness",
                    capacity::LoadDecline::Resource => "empty resource owner unavailable",
                    capacity::LoadDecline::Empty => "candidate not empty",
                    capacity::LoadDecline::Available => "empty resource owner not available",
                    #[cfg(feature = "managed-grouped-source-alpha")]
                    capacity::LoadDecline::Canonical => "previous canonical actors/input readiness",
                    capacity::LoadDecline::Snapshot => "current dispatcher/scene snapshot",
                    capacity::LoadDecline::Inputs => "current input view unavailable",
                    #[cfg(feature = "managed-grouped-source-alpha")]
                    capacity::LoadDecline::InputRole => "previous input role",
                    capacity::LoadDecline::OriginalResources => "original resource owners unavailable",
                    #[cfg(any(feature = "source-ldn-grouped-helper-capacity-probe", feature = "managed-grouped-source-alpha"))]
                    capacity::LoadDecline::ParkedGroups => "existing canonical grouped helper readiness",
                });
                return;
            }
        };
        #[cfg(not(feature = "source-capacity-probe"))]
        let Some(before) =
            capacity::load_preflight(ctx, entry, owner, previous.map(|source| source.kind))
        else {
            return;
        };
        let ticks = if previous.is_none() {
            let ticks = prior_first
                .filter(|old| {
                    old.wanted == wanted
                        && old.reservation == owner
                        && old.previous == previous
                        && old.before == before
                })
                .map_or(1, |old| old.ticks.saturating_add(1));
            let mut state = STATE.lock().unwrap();
            if pending_request_for(&state, Some(slot), menu_open) != Some(wanted)
                || state.sources[index] != previous
                || reservation(&state) != Some(owner)
            {
                return;
            }
            state.capacity.first_load = Some(FirstLoad {
                wanted,
                reservation: owner,
                previous,
                before,
                ticks,
            });
            if ticks < 30 {
                return;
            }
            ticks
        } else {
            0
        };
        (Some(before), ticks)
    } else {
        (None, 0)
    };
    if slot != 7 && !native_admission(entry, slot, backend, previous.map(|source| source.kind)) {
        return;
    }
    let Some(costume) = native_source::inherited_costume(slot) else {
        #[cfg(all(feature = "managed-third-source-alpha", feature = "source-capacity-probe"))]
        declined!(24, "inherited resource costume unavailable");
        return;
    };
    #[cfg(feature="mii-managed-helper-alpha")]
    if wanted.configuration.is_some_and(|c| !super::mii_managed_helper::slot_eligible(slot,c)) {
        return;
    }
    let Some(install) = INSTALL.try_begin(false) else {
        #[cfg(all(feature = "managed-third-source-alpha", feature = "source-capacity-probe"))]
        declined!(25, "shared install gate unavailable");
        return;
    };
    #[cfg(feature = "managed-third-source-alpha")]
    if slot == 7 {
        let owner = reservation(&STATE.lock().unwrap());
        if owner.and_then(|owner| {
            capacity::load_preflight(ctx, entry, owner, previous.map(|source| source.kind))
        }) != capacity_load
            || capacity_load.is_none()
        {
            STATE.lock().unwrap().capacity.first_load = None;
            finish_install(install, "capacity load preflight changed");
            #[cfg(feature = "source-capacity-probe")]
            declined!(26, "second preflight differs from first");
            return;
        }
    }
    #[cfg(feature="mii-managed-helper-alpha")]
    let mii_prepared=match wanted.configuration {
        Some(c)=>match super::mii_managed_helper::prepare(entry as usize,slot,c) {
            Ok(p)=>Some(p),Err(reason)=>{
                // A resolved row/hash mismatch belongs to this exact payload.
                // Table availability, live hat, snapshot and provenance errors
                // remain pending; a different helper callback may be eligible.
                if reason == "saved appearance hint/hash mismatch" {
                    refuse_want(wanted,slot,reason);
                }
                finish_install(install,reason);
                return;
            }
        },None=>None,
    };
    #[cfg(feature = "mii-managed-source-coexist-alpha")]
    let original = match previous {
        // Missing first-install proof stays missing for this native lifetime.
        Some(source) => {
            if source.original.is_some_and(|original|
                coexist::helpers::observe(entry as usize, slot)
                    .is_none_or(|now| now.original != original))
            {
                finish_install(install, "managed original row changed");
                return;
            }
            source.original
        }
        None => coexist::helpers::original(entry as usize, slot),
    };
    let mut state = STATE.lock().unwrap();
    // Same per-slot selection as initiation. Slot 7 capacity probes stay on the global walk.
    if pending_request_for(&state, Some(slot), menu_open) != Some(wanted)
        || state.sources[index] != previous
        || ready_still_requested(&state, index)
        || state.book.can_replace(index) != Ok(true)
    {
        #[cfg(feature = "managed-third-source-alpha")]
        if slot == 7 {
            state.capacity.first_load = None;
        }
        drop(state);
        finish_install(install, "managed source intent changed");
        return;
    }
    #[cfg(feature = "mii-managed-source-coexist-alpha")]
    if !INSTALL.owns(install) {
        drop(state);
        finish_install(install, "managed exact install owner changed");
        return;
    }
    #[cfg(feature = "mii-managed-source-coexist-alpha")]
    let mut registry = coexist::registry();
    let load = match state.book.begin_install(
        index,
        wanted.epoch,
        NativeEntry(entry as u64),
        SourceKey {
            kind: wanted.kind,
            costume,
        },
    ) {
        Ok(load) => load,
        Err(_) => {
            #[cfg(feature = "managed-third-source-alpha")]
            if slot == 7 {
                state.capacity.first_load = None;
            }
            #[cfg(feature = "mii-managed-source-coexist-alpha")]
            drop(registry);
            drop(state);
            finish_install(install, "managed source admission rejected");
            return;
        }
    };
    state.sources[index] = Some(Source {
        token: load.source(),
        load,
        install: Some(install),
        kind: wanted.kind,
        ticks: 0,
        configuration:wanted.configuration,
        #[cfg(feature = "mii-managed-source-coexist-alpha")]
        original,
    });
    #[cfg(feature = "mii-managed-source-coexist-alpha")]
    {
        registry.source(state.book.epoch(), index,
            coexist::helpers::Helper::Unavailable { token: load.source(), phase: Phase::Loading });
        drop(registry);
    }
    #[cfg(feature="mii-managed-helper-alpha")]
    if let Some(prepared)=mii_prepared {
        if let Err(reason)=super::mii_managed_helper::bind(prepared,load,install) {
            drop(state);
            crate::logging::diagnostic(format_args!("[MiiHelper] bind refused; INSTALL retained source={:?} reason={}",load.source(),reason));
            return;
        }
    }
    #[cfg(feature = "managed-third-source-alpha")]
    if slot == 7 {
        state.capacity.load = capacity_load.map(|before| (load.source(), before));
        state.capacity.first_load = None;
        // A replacement publishes its new source before native release of the
        // old actor. Retain the old generation solely to attribute that event.
        state.capacity.observation.retiring = previous;
        #[cfg(feature = "managed-grouped-source-alpha")]
        {
            let observed = &mut state.capacity.observation;
            observed.retiring_actors = observed.current_actors.take();
            observed.masks = [(0, 0), observed.masks[0]];
            observed.cleaned = [[None; 3], observed.cleaned[0]];
        }
    }
    #[cfg(all(feature = "managed-third-source-alpha", feature = "source-capacity-probe"))]
    let cached_book = (slot == 7).then(|| {
        let mut records = LeaseObservations::new();
        for live in state.leases.iter().flatten().copied() { records.push(live, None); }
        (records, candidates(backend).map(|slot| (slot, state.book.phase(slot as usize),
            state.book.can_replace(slot as usize))))
    });
    drop(state);
    #[cfg(all(feature = "managed-third-source-alpha", feature = "source-capacity-probe"))]
    if let Some((records, slots)) = cached_book {
        capacity::observe_install_inputs(load.source(), previous.map(|source| (source.token, source.kind)),
            wanted.kind, capacity_load);
        crate::logging::diagnostic(format_args!(
            "[SourceOverlap] commit_cache_mirror source={:?} slots(phase,can_replace)={:?} total={} recorded={} omitted={} native_liveness_resampled=false",
            load.source(), slots, records.total, records.recorded(), records.omitted()));
        for (live, _) in records.records.iter().flatten() {
            crate::logging::diagnostic(format_args!(
                "[SourceOverlap] cached_lease_mirror_record commit_source={:?} retained={:?}", load.source(), live));
        }
    }
    #[cfg(feature = "managed-third-source-alpha")]
    if first_load_callbacks != 0 {
        crate::logging::diagnostic(format_args!(
            "[CharacterSources] first capacity load stable slot=7 callbacks={}",
            first_load_callbacks
        ));
    }
    crate::logging::diagnostic(format_args!(
        "[CharacterSources] install slot={} kind=0x{:x}",
        slot, wanted.kind
    ));
    #[cfg(feature="mii-managed-helper-alpha")]
    if wanted.configuration.is_some() {
        if !super::mii_managed_helper::install(load.source()) {
            crate::logging::diagnostic(format_args!("[MiiHelper] native submit refused; INSTALL retained source={:?}",load.source()));
        }
        return;
    }
    native_source::install(entry, wanted.kind);
}

/// Association uses the native destructor's current actor and current entry7,
/// including old actors released while a new managed install is pending.
#[cfg(feature = "managed-third-source-alpha")]
pub(super) unsafe fn capacity_actor_cleanup(ctx: &InlineCtx) {
    let (saved, current, observation) = {
        let state = STATE.lock().unwrap();
        let Some(saved) = state.capacity.reservation else {
            return;
        };
        (saved, state.sources[7], state.capacity.observation)
    };
    #[cfg(feature = "managed-grouped-source-alpha")]
    let association = {
        let actor = ctx.registers[20].x() as usize;
        if observation.retiring_actors.is_some_and(|set| set.unretired(actor, observation.masks[1].0)) {
            observation.retiring.map(|source| (source, observation.retiring_actors, 1))
        } else {
            current.filter(|_| observation.current_actors.is_none_or(|set| set.contains(actor)))
                .map(|source| (source, observation.current_actors, 0))
        }
    };
    #[cfg(feature = "managed-grouped-source-alpha")]
    let event = association.and_then(|(source, actors, group)|
        capacity::actor_cleanup_observation(ctx, saved.1, source.kind, actors, observation.cleaned[group][0]));
    #[cfg(not(feature = "managed-grouped-source-alpha"))]
    let event = capacity::actor_cleanup_observation(ctx, saved.1);
    let Some(event) = event else {
        return;
    };
    #[cfg(feature = "managed-grouped-source-alpha")]
    let source = association.map(|(source, _, _)| source);
    #[cfg(not(feature = "managed-grouped-source-alpha"))]
    let source = current
        .filter(|source| source.kind == event.kind)
        .or_else(|| {
            observation
                .retiring
                .filter(|source| source.kind == event.kind)
        });
    let source = source.filter(|source| {
            source.token.epoch() == saved.0
                && source.token.entry() == NativeEntry(saved.1.entry as u64)
        });
    let key = (source.map(|source| source.token), event.actor);
    #[cfg(feature = "managed-grouped-source-alpha")]
    let group = association.map(|(_, _, group)| group).unwrap_or(0);
    #[cfg(feature = "managed-grouped-source-alpha")]
    if observation.masks[group].0 & (1 << event.index) != 0 { return; }
    if observation.last_actor == Some(key) {
        return;
    }
    let valid = event.valid && source.is_some();
    let mut state = STATE.lock().unwrap();
    if state.capacity.reservation != Some(saved)
        || state.sources[7] != current
        || state.capacity.observation != observation
    {
        return;
    }
    let observed = &mut state.capacity.observation;
    observed.last_actor = Some(key);
    #[cfg(feature = "managed-grouped-source-alpha")]
    {
        observed.masks[group].0 |= 1 << event.index;
        observed.cleaned[group][event.index] = event.record;
        if valid { observed.masks[group].1 |= 1 << event.index; }
    }
    observed.actor_events = observed.actor_events.saturating_add(1);
    if valid {
        observed.valid_actors = observed.valid_actors.saturating_add(1);
        if source.is_some_and(|source| source.install.is_none()) {
            observed.ready_cleanups = observed.ready_cleanups.saturating_add(1);
        }
    }
    #[cfg(feature = "managed-grouped-source-alpha")]
    let masks = observed.masks;
    drop(state);
    #[cfg(feature = "managed-grouped-source-alpha")]
    crate::logging::diagnostic(format_args!(
        "[ManagedSourceCleanup] canonical cleanup source={:?} retiring={} index={} masks={:?} record={:x?}",
        key.0, group == 1, event.index, masks, event.record));
    crate::logging::diagnostic(format_args!(
        "[ManagedSourceCleanup] actor epoch={:?} entry=0x{:x} owner=0x{:x} actor=0x{:x} kind=0x{:x} source={:?} pending={:?} identity={} entry_associated=true resource_associated={} native_return={} valid={} input={:x?}",
        saved.0, saved.1.entry, saved.1.resource, event.actor, event.kind,
        key.0, source.map(|source| source.install.is_some()),
        event.identity, event.resource_associated, event.returned, valid, event.input));
}

/// The entry destructor owns entry now; read the current service outside STATE.
#[cfg(feature = "managed-third-source-alpha")]
unsafe fn capacity_destroying(entry: *const u8) {
    let saved = STATE.lock().unwrap().capacity.reservation;
    let Some(saved) = saved.filter(|(_, value)| {
        value.entry == entry as usize && !value.entry_destroyed && !value.owner_retired
    }) else {
        return;
    };
    let before = capacity::listener_count(saved.1);
    let mut state = STATE.lock().unwrap();
    if state.capacity.reservation == Some(saved) {
        state.capacity.observation.destroy_seen = true;
        state.capacity.observation.listeners_before = before;
    }
}

#[cfg(feature = "managed-third-source-alpha")]
pub(super) unsafe fn capacity_listener_removed(ctx: &InlineCtx) {
    let (saved, observation) = {
        let state = STATE.lock().unwrap();
        let Some(saved) = state.capacity.reservation else {
            return;
        };
        (saved, state.capacity.observation)
    };
    if ctx.registers[19].x() as usize != saved.1.entry
        || !saved.1.entry_destroyed
        || !observation.destroy_seen
        || observation.listener_removed.is_some()
    {
        return;
    }
    let after = capacity::listener_after(ctx, saved.1);
    let removed = observation
        .listeners_before
        .zip(after)
        .is_some_and(|(before, after)| before.checked_sub(1) == Some(after));
    let mut state = STATE.lock().unwrap();
    if state.capacity.reservation != Some(saved) || state.capacity.observation != observation {
        return;
    }
    state.capacity.observation.listener_removed = Some(removed);
    drop(state);
    crate::logging::diagnostic(format_args!(
        "[ManagedSourceCleanup] listener epoch={:?} entry=0x{:x} owner=0x{:x} before={:?} after={:?} removed={}",
        saved.0, saved.1.entry, saved.1.resource, observation.listeners_before, after, removed));
}

/// Numeric association at the actual owner cleanup boundary, after its native
/// destructor body. Never dereference a saved owner, including allocator reuse.
#[cfg(feature = "managed-third-source-alpha")]
pub(super) fn capacity_owner_retired(owner: usize) -> bool {
    let mut state = STATE.lock().unwrap();
    let Some((epoch, mut value)) = state.capacity.reservation else {
        return false;
    };
    if value.resource != owner || value.owner_retired {
        return false;
    }
    value.owner_retired = true;
    state.capacity.first_load = None;
    state.capacity.reservation = if value.entry_destroyed {
        None
    } else {
        Some((epoch, value))
    };
    let observed = state.capacity.observation;
    let cleared = state.capacity.reservation.is_none();
    drop(state);
    crate::logging::diagnostic(format_args!(
        "[CharacterSources] capacity owner retired owner=0x{:x} entry_destroyed={}",
        owner, value.entry_destroyed
    ));
    crate::logging::diagnostic(format_args!(
        "[ManagedSourceCleanup] retirement epoch={:?} entry=0x{:x} owner=0x{:x} entry_destroyed={} reservation_cleared={} ready_actors={} actor_events={} valid_actors={} ready_cleanups={} listener_removed={:?}",
        epoch, value.entry, owner, value.entry_destroyed, cleared, observed.ready_actors,
        observed.actor_events, observed.valid_actors, observed.ready_cleanups, observed.listener_removed));
    true
}

/// The synchronous native entry destructor owns this argument. Keep the full
/// admission token beside it until this callback, including across mode changes.
pub unsafe fn on_destroy(entry: *const u8, slot: u32) {
    if slot >= 8 {
        return;
    }
    #[cfg(feature = "managed-third-source-alpha")]
    if slot == 7 {
        capacity_destroying(entry);
    }
    let mut state = STATE.lock().unwrap();
    #[cfg(feature = "managed-third-source-alpha")]
    if slot == 7 {
        if let Some((epoch, mut value)) = state.capacity.reservation {
            if value.entry == entry as usize {
                state.capacity.first_load = None;
                value.entry_destroyed = true;
                state.capacity.reservation = if value.owner_retired {
                    None
                } else {
                    Some((epoch, value))
                };
            }
        }
    }
    let Some(source) = state.sources[slot as usize] else {
        return;
    };
    if source.token.entry() != NativeEntry(entry as u64) {
        return;
    }
    #[cfg(feature = "managed-third-source-alpha")]
    let destruction = (
        state.book.phase(slot as usize),
        !state.book.is_current(source.token.epoch()),
    );
    #[cfg(feature = "mii-managed-source-coexist-alpha")]
    let mut registry = coexist::registry();
    let released = match state.book.native_destroyed(source.token) {
        Ok(released) => released,
        Err(error) => {
            #[cfg(feature = "mii-managed-source-coexist-alpha")]
            drop(registry);
            drop(state);
            crate::logging::diagnostic(format_args!(
                "[CharacterSources] destructor identity rejected slot={} error={:?}",
                slot, error
            ));
            return;
        }
    };
    #[cfg(feature = "source-lifecycle-probe")]
    let released_items = {
        let mut records = LeaseObservations::new();
        for live in state.leases.iter().flatten().copied() {
            if live.source == source.token && matches!(live.consumer, Consumer::Item { .. }) {
                records.push(live, None);
            }
        }
        records
    };
    for live in &mut state.leases {
        if live.is_some_and(|live| live.source == source.token) {
            *live = None;
        }
    }
    HAS_CONSUMERS.store(state.leases.iter().any(Option::is_some), Ordering::Release);
    state.sources[slot as usize] = None;
    #[cfg(feature = "mii-managed-source-coexist-alpha")]
    {
        registry.source(state.book.epoch(), slot as usize, coexist::helpers::Helper::Unmanaged);
        drop(registry);
    }
    #[cfg(feature="mii-managed-helper-alpha")]
    super::mii_managed_helper::release(source.token);
    #[cfg(feature = "managed-third-source-alpha")]
    if slot == 7 {
        state.capacity.load = None;
    }
    drop(state);
    #[cfg(feature = "source-lifecycle-probe")]
    observe_released_items(&released_items, "native_source_destroyed_book_invalidated", Some(source.token));
    #[cfg(feature = "managed-third-source-alpha")]
    if slot == 7 {
        crate::logging::diagnostic(format_args!(
            "[ManagedSourceCleanup] entry destruction source={:?} phase={:?} ticks={} departed={} pending_install={:?} gate_active={}",
            source.token, destruction.0, source.ticks, destruction.1, source.install, INSTALL.active()));
    }
    if let Some(install) = source.install {
        finish_install(install, "managed source destroyed");
    }
    crate::logging::diagnostic(format_args!(
        "[CharacterSources] destroyed slot={} tracked_leases={}",
        slot, released
    ));
}
