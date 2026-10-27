//! Reload a practice player or CPU through the game's asynchronous entry/resource loader.
//! Requests contain values and entry identities, never retained game pointers.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;

use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use training_mod_consts::{OnOff, TrainingModpackMenu, TrioFighter, MENU};
use training_mod_sync::{lock_write, read as setting};

use crate::character_memory::WaitingSession;
use crate::common::{
    button_config, game_mode,
    input::{Buttons, MappedInputs},
    menu,
};
use crate::training::{self, lobby};
use training_mod_consts::character_forms::{
    configured_forms, cycle_position, next_cycle, next_from_original, EntryState, Forms,
};

#[path = "native_service.rs"]
mod native_service;
use native_service::ServiceEntryContext;

#[path = "native_install.rs"]
mod native_install;
use native_install::{InstallGate, InstallToken};

#[path = "native_fighter_install.rs"]
mod native_fighter_install;

#[path = "native_hud_refresh.rs"]
mod native_hud_refresh;

#[path = "native_source.rs"]
mod native_source;
#[cfg(any(
    feature = "source-capacity-probe",
    feature = "managed-third-source-alpha"
))]
#[path = "native_source_capacity.rs"]
mod native_source_capacity;
#[path = "character_sources.rs"]
pub mod sources;

#[cfg(feature = "source-lifecycle-probe")]
#[path = "source_lifecycle_probe.rs"]
pub mod source_lifecycle_probe;

#[cfg(feature = "source-capacity-probe")]
#[path = "source_capacity_probe.rs"]
pub mod source_capacity_probe;

#[cfg(feature = "source-ldn-capacity-probe")]
#[path = "source_ldn_capacity_probe.rs"]
mod source_ldn_capacity_probe;

#[cfg(feature = "source-owner-probe")]
#[path = "source_owner_probe.rs"]
pub mod source_owner_probe;

macro_rules! trace {
    ($($arg:tt)*) => { crate::logging::diagnostic(format_args!($($arg)*)) };
}

#[cfg(feature = "mii-pool-observer")]
#[path = "mii_pool_observer.rs"]
pub mod mii_pool_observer;
#[cfg(feature = "mii-origin-observer")]
#[path = "mii_origin_observer.rs"]
mod mii_origin_observer;

#[cfg(feature="mii-saved-alpha")]
#[path="mii_saved_alpha.rs"]
pub(crate) mod mii_saved_alpha;
#[cfg(feature="mii-saved-alpha")]
#[path="mii_catalog_request.rs"]
pub(crate) mod mii_catalog_request;
#[cfg(feature="mii-saved-alpha")]
#[path="mii_backend.rs"]
mod mii_backend;
#[cfg(feature="mii-saved-alpha")]
#[path="mii_saved_fingerprints.rs"]
mod mii_saved_fingerprints;

#[cfg(feature="mii-ordinary-player-alpha")]
#[path="mii_ordinary_player.rs"]
mod mii_ordinary_player;
#[cfg(feature="mii-ordinary-cpu-alpha")]
#[path="mii_ordinary_cpu.rs"]
mod mii_ordinary_cpu;
#[cfg(feature="mii-ordinary-cpu-alpha")]
#[path="mii_selector_native.rs"]
mod mii_selector_native;
#[cfg(feature="mii-ldn-qualification-probe")]
#[path="mii_ldn_qualification.rs"]
mod mii_ldn_qualification;
#[cfg(feature="mii-managed-helper-alpha")]
#[path="mii_managed_helper.rs"]
mod mii_managed_helper;
#[cfg(feature="mii-ordinary-cpu-alpha")]
#[path="mii_auto_init.rs"]
mod mii_auto_init;
#[cfg(feature="mii-ordinary-cpu-alpha")]
#[path="mii_replay.rs"]
pub(crate) mod mii_replay;
#[cfg(feature="mii-hud-portrait-alpha")]
#[path="mii_hud_portrait_policy.rs"]
pub(crate) mod mii_hud_portrait_policy;
#[cfg(feature="mii-hud-portrait-alpha")]
#[path="mii_hud_portrait.rs"]
mod mii_hud_portrait;

static BASE: AtomicUsize = AtomicUsize::new(0);
static INSTALL: InstallGate = InstallGate::new();
static OWNED_BUTTONS: AtomicU32 = AtomicU32::new(0);
// The first ready body of this native entry supplies its character-select kind.
// Reloads retain the entry, so a practice replacement must not overwrite this.
static ORIGINAL_KINDS: [AtomicU32; 2] = [AtomicU32::new(u32::MAX), AtomicU32::new(u32::MAX)];
static SOLO_ACTIVE: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static STATES: [Mutex<State>; 2] = [Mutex::new(State::empty()), Mutex::new(State::empty())];
static WAITING_MEMORY: Mutex<WaitingSession<WaitingValue>> = Mutex::new(WaitingSession::empty());
type WaitingValue=training_mod_consts::mii::remembered::Value<TrioFighter,Forms>;

/// Loaded configuration, or an explicit original restoration. Absence is `NotMii`.
#[cfg(feature="mii-hud-portrait-alpha")]
#[derive(Clone, Copy, Debug)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum PortraitIntent {
    Exact(training_mod_consts::mii::lifetime::Configuration),
    Original { original_kind: u32 },
    NotMii,
}
#[cfg(feature="mii-saved-alpha")]
fn remember_owned(role:Role,generation:u64,kind:u32,forms:Forms,
    configuration:Option<training_mod_consts::mii::lifetime::Configuration>,cursor:Option<usize>,original:bool) {
    let mut memory=WAITING_MEMORY.lock().unwrap();
    if !mii_backend::current().is_some_and(|l|l.waiting())
        || mii_saved_alpha::current_generation(role as u32)!=generation {return;}
    if let Some(fighter)=TrioFighter::from_kind(kind) {
        memory.commit(role as usize,
            WaitingValue {fighter,forms,cursor,configuration,owned:true,original});
        drop(memory);
        crate::kirby_hats::fighter_committed(role as usize, kind);
    }
}
fn restore_owned(role:Role,value:WaitingValue)->bool {
    // Evaluate the role adapter even when the remembered value is already owned.
    // Missing qualification never falls through to a kind-only Mii load.
    match role {
        Role::Player => {
            #[cfg(feature="mii-ordinary-player-alpha")]
            { value.owned | mii_ordinary_player::request_restore(value) }
            #[cfg(not(feature="mii-ordinary-player-alpha"))]
            { value.owned }
        }
        Role::Cpu => {
            #[cfg(feature="mii-ordinary-cpu-alpha")]
            { value.owned | mii_ordinary_cpu::request_restore(value) }
            #[cfg(not(feature="mii-ordinary-cpu-alpha"))]
            { value.owned }
        }
    }
}
#[cfg(feature="mii-saved-alpha")]
fn observe_mii_origin(role:Role,generation:u64,kind:u32,forms:Forms,
    configuration:Option<training_mod_consts::mii::lifetime::Configuration>) {
    let (Some(configuration),Some(fighter))=(configuration,TrioFighter::from_kind(kind)) else{return;};
    let mut memory=WAITING_MEMORY.lock().unwrap();
    if !mii_backend::current().is_some_and(|l|l.waiting())
        || mii_saved_alpha::current_generation(role as u32)!=generation {return;}
    // observe preserves a same-lobby remembered choice; a fresh room clears both roles.
    let fresh = memory.observe(role as usize,WaitingValue {fighter,forms,cursor:None,
        configuration:Some(configuration),owned:true,original:true});
    drop(memory);
    if fresh { crate::kirby_hats::fresh_lobby(); }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Role {
    Player,
    Cpu,
}

impl Role {
    fn selected(self, m: &TrainingModpackMenu) -> TrioFighter {
        match self {
            Self::Player => m.player_character,
            Self::Cpu => m.cpu_character,
        }
    }
    fn enabled(self) -> bool {
        self == Self::Cpu || setting(&MENU).trio_practice == OnOff::ON
    }
    fn sync_selection(self, fighter: TrioFighter) {
        let mut m = lock_write(&MENU);
        match self {
            Self::Player => m.player_character = fighter,
            Self::Cpu => m.cpu_character = fighter,
        }
    }
}

fn state_for(role: Role) -> &'static Mutex<State> {
    &STATES[role as usize]
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Scope {
    mode: u32,
    owner: u32,
}

impl Scope {
    fn waiting(self) -> bool {
        self.mode != 0x04040000
    }
}

#[derive(Clone, Copy, Debug)]
enum Selection {
    Direct(TrioFighter),
    Cycle,
    RefreshPartners,
}

#[derive(Clone, Copy, Debug)]
struct Request {
    scope: Scope,
    epoch: usize,
    entry: usize,
    selection: Selection,
    age: usize,
    restoring: bool,
}

#[derive(Clone, Copy, Debug)]
struct Active {
    install: InstallToken,
    scope: Scope,
    epoch: usize,
    revision: usize,
    entry: usize,
    target: TrioFighter,
    forms: Forms,
    slot: Option<usize>,
    ticks: usize,
    phase: u32,
    departed: bool,
}

struct State {
    epoch: usize,
    revision: usize,
    pending: Option<Request>,
    active: Option<Active>,
    cursor: Option<usize>,
    loaded: Option<(usize, u32, u32)>,
    observed: Option<(usize, u32)>,
}

impl State {
    const fn empty() -> Self {
        Self {
            epoch: 0,
            revision: 0,
            pending: None,
            active: None,
            cursor: None,
            loaded: None,
            observed: None,
        }
    }
}

fn scope(role: Role) -> Option<Scope> {
    let mode = game_mode::current()?;
    if !mode.initialized {
        return None;
    }
    if mode.melee == 0x20 && mode.mode == 0x04040000 {
        Some(Scope {
            mode: mode.mode,
            owner: role as u32,
        })
    } else if cfg!(feature = "waiting-roster-reload") && lobby::active() {
        Some(Scope {
            mode: mode.mode,
            owner: if role == Role::Player { 0 } else { 4 },
        })
    } else {
        None
    }
}

pub fn busy() -> bool {
    INSTALL.suspends_practice()
}

/// Hat continuation waits for the remembered practice fighter, including owned
/// Mii transitions, instead of acting on a briefly ready CSS body after return.
pub fn waiting_choice_settled(role: usize) -> bool {
    if busy() { return false; }
    if STATES[role].lock().unwrap().observed.is_none_or(|(_, kind)| kind != 6) {
        return false;
    }
    // The other role's automatic replacement would cancel a newly queued hat.
    // Wait for both, without nesting roster/memory locks into hat acquisition.
    if STATES.iter().any(|state| {
        let state = state.lock().unwrap();
        state.pending.is_some() || state.active.is_some()
    }) { return false; }
    let memory = WAITING_MEMORY.lock().unwrap();
    !memory.restoration_pending([Role::Player.enabled(), Role::Cpu.enabled()])
}

/// Drafts resume after both returning entries have been observed and remembered
/// choices reconciled. Resuming on the first ready CSS body would let the later
/// automatic roster install close an unfinished panel as ordinary swap cleanup.
pub fn waiting_drafts_ready() -> bool {
    if busy() { return false; }
    if !cfg!(feature = "waiting-roster-reload") { return true; }
    if STATES.iter().any(|state| {
        let state = state.lock().unwrap();
        state.observed.is_none() || state.pending.is_some() || state.active.is_some()
    }) { return false; }
    !WAITING_MEMORY.lock().unwrap()
        .restoration_pending([Role::Player.enabled(), Role::Cpu.enabled()])
}

fn finish_install(token: InstallToken, reason: &str) -> bool {
    let finished = INSTALL.finish(token);
    #[cfg(feature = "source-ldn-capacity-link-probe")]
    if reason == "capacity Link native destruction" {
        trace!("[NativeInstall] capacity Link destructor token={:?} finished={} gate_active_after={}",
            token, finished, INSTALL.active());
    }
    #[cfg(feature = "managed-third-source-alpha")]
    if reason == "managed source destroyed" {
        trace!(
            "[NativeInstall] managed destructor token={:?} finished={} gate_active_after={}",
            token,
            finished,
            INSTALL.active()
        );
    }
    if !finished {
        trace!(
            "[NativeInstall] ignored stale completion token={:?} reason={}",
            token,
            reason
        );
    }
    finished
}

fn original_kind(role: Role) -> Option<u32> {
    let kind = ORIGINAL_KINDS[role as usize].load(Ordering::Acquire);
    (kind != u32::MAX).then_some(kind)
}

/// Original native P1 selection, not the current practice replacement.
pub fn original_player_kind() -> Option<u32> {
    scope(Role::Player).and_then(|_| original_kind(Role::Player))
}

/// Native CPU choice for the hat picker; practice swaps do not replace it.
pub fn original_cpu_kind() -> Option<u32> {
    scope(Role::Cpu).and_then(|_| original_kind(Role::Cpu))
}

/// Published only from a ready native descriptor, not from an unapplied preference.
pub fn solo_popo_active(cpu: bool) -> bool {
    let role = if cpu { Role::Cpu } else { Role::Player };
    scope(role).is_some() && SOLO_ACTIVE[role as usize].load(Ordering::Acquire)
}

/// Forms for a fighter that is not at a known numbered slot: slot 0 or a direct pick.
#[cfg(feature = "mii-ordinary-cpu-alpha")]
fn requested_forms(m: &TrainingModpackMenu, role: Role, kind: u32) -> Forms {
    requested_forms_at(m, role, kind, None)
}

/// `slot` is the numbered cycle position holding the fighter. The same Ice Climbers kind
/// takes the original preference at Player slot 0 and the cycle's at a numbered slot.
fn requested_forms_at(m: &TrainingModpackMenu, role: Role, kind: u32, slot: Option<usize>) -> Forms {
    configured_forms(m, role == Role::Cpu, original_kind(role), kind, slot.is_some())
}

fn publish_partners(role: Role, forms: Forms) {
    SOLO_ACTIVE[role as usize].store(forms == Forms::solo_popo(), Ordering::Release);
}

/// Ordinary slot 0 for this role. A CPU Mii class kind is not a saved identity;
/// that return belongs to the saved-Mii cycle, not a default Swordfighter load.
fn cycle_original(role: Role) -> Option<u32> {
    let kind = original_kind(role)?;
    if role == Role::Cpu && (0x48..=0x4a).contains(&kind) {
        return None;
    }
    Some(kind)
}

fn selection_label(role: Role, fighter: TrioFighter, slot: Option<usize>) -> String {
    match slot {
        Some(index) => format!("{}. {}", index + 1, fighter.name()),
        None if cycle_original(role) == Some(fighter.kind()) => {
            format!("0. {}", fighter.name())
        }
        None => fighter.name().into(),
    }
}

fn roster(m: &TrainingModpackMenu, role: Role) -> Vec<TrioFighter> {
    let excluded = match role {
        Role::Player => training_mod_tui::roster_grid::cycle_exclusion(
            original_kind(role),
            m.solo_popo_original == OnOff::ON,
            m.solo_popo_player_cycle == OnOff::ON,
        ),
        Role::Cpu => {
            let solo = m.solo_popo_cpu == OnOff::ON;
            training_mod_tui::roster_grid::cycle_exclusion(cycle_original(role), solo, solo)
        }
    };
    let fighters = match role {
        Role::Player => m.player_roster.fighters_excluding(excluded),
        Role::Cpu => m.cpu_roster.fighters_excluding(excluded),
    };
    fighters.into_iter().filter(|f| !(0x48..=0x4a).contains(&f.kind())).collect()
}

/// Numbered slot of `kind`'s group in `role`'s current cycle.
fn locate(m: &TrainingModpackMenu, role: Role, kind: u32) -> Option<usize> {
    let kinds: Vec<_> = roster(m, role).iter().map(|fighter| fighter.kind()).collect();
    cycle_position(&kinds, kind)
}

fn notify(role: Role, message: String) {
    let title = if role == Role::Player {
        "Player Character"
    } else {
        "CPU Character"
    };
    training::ui::notifications::notification(title.into(), message, 180);
}

/// Called after committing menu values. Editing an unrelated option must not
/// replace a fighter, and navigating/highlighting a fighter is not a request.
pub fn settings_changed(previous: &TrainingModpackMenu,
    manual: Option<(training_mod_tui::roster_grid::Role, u32)>) {
    let current = setting(&MENU);
    for role in [Role::Player, Role::Cpu] {
        if roster(previous, role) != roster(&current, role) {
            let mut state = state_for(role).lock().unwrap();
            state.revision = state.revision.wrapping_add(1);
            // A numbered fighter keeps its slot intent through an edit (Ice Climbers can be
            // both slot 0 and numbered). A removed entry keeps a stale hint: the
            // next cycle validates it by kind, while Solo preference still follows
            // the numbered origin until an explicit return to slot 0.
            let live = state.observed.map(|(_, kind)| kind);
            state.cursor = state.cursor.map(|old|
                live.and_then(|kind| locate(&current, role, kind)).unwrap_or(old));
            trace!(
                "[CharacterRoster] settings role={:?} size={} kinds={:x?}",
                role,
                roster(&current, role).len(),
                roster(&current, role)
                    .iter()
                    .map(|f| f.kind())
                    .collect::<Vec<_>>()
            );
        }
        let player_just_enabled = role == Role::Player
            && previous.trio_practice != OnOff::ON
            && current.trio_practice == OnOff::ON;
        // A portrait press is an action even when its one-hot setting is unchanged
        // (e.g. retrying a previously refused request). Dispatch it exactly once.
        let explicit = manual.is_some_and(|(picked_role, kind)|
            (picked_role == training_mod_tui::roster_grid::Role::Player) == (role == Role::Player)
                && kind == role.selected(&current).kind());
        if explicit || role.selected(previous) != role.selected(&current) || player_just_enabled {
            queue(role, Selection::Direct(role.selected(&current)));
        }
    }
}

fn queue(role: Role, selection: Selection) {
    queue_request(role, selection, false);
}

fn queue_request(role: Role, selection: Selection, restoring: bool) {
    if !role.enabled() && !matches!(selection, Selection::RefreshPartners) {
        return;
    }
    if BASE.load(Ordering::Acquire) == 0 || !crate::character_lifecycle::native_hooks_ready() {
        notify(role, "Unavailable on this game version".into());
        return;
    }
    let Some(scope) = scope(role) else {
        return;
    };
    if busy()
        || training::recording_or_playback()
        || unsafe { training::save_states::is_loading() }
        || lobby::native_ui_busy()
        || button_config::has_pending_request()
    {
        if !restoring && !matches!(selection, Selection::RefreshPartners) {
            trace!("[CharacterRoster] request rejected: another operation owns practice");
            notify(role, "Finish the current practice action first".into());
        }
        return;
    }
    #[cfg(any(feature="mii-ordinary-player-alpha",feature="mii-ordinary-cpu-alpha"))]
    let user_request=!restoring && !matches!(selection,Selection::RefreshPartners);
    #[cfg(feature="mii-ordinary-player-alpha")]
    if mii_ordinary_player::intercept(role,selection,user_request)==Some(true){return;}
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    if mii_ordinary_cpu::intercept(role,selection,user_request)==Some(true){return;}
    #[cfg(feature="mii-saved-alpha")]
    if mii_saved_alpha::intercept(role,selection){return;}
    let mut state = state_for(role).lock().unwrap();
    // The callback's restore peek may race a manual request. The role lock also
    // protects manual enqueue/cancellation: never overwrite newer explicit intent.
    if restoring
        && (state.pending.is_some()
            || state.active.is_some()
            || !matches!(selection, Selection::Direct(target)
                if WAITING_MEMORY.lock().unwrap().restore_target(role as usize).is_some_and(|v|!v.owned && v.fighter==target)))
    {
        return;
    }
    let Some((entry, _)) = state.observed else {
        drop(state);
        trace!("[CharacterRoster] request rejected: no current practice entry");
        if !matches!(selection, Selection::RefreshPartners) {
            notify(
                role,
                "Wait for the practice character to finish loading".into(),
            );
        }
        return;
    };
    state.pending = Some(Request {
        scope,
        epoch: state.epoch,
        entry,
        selection,
        age: 0,
        restoring,
    });
    if scope.waiting() && !restoring && !matches!(selection, Selection::RefreshPartners) {
        WAITING_MEMORY.lock().unwrap().manual_request(role as usize);
    }
    trace!(
        "[CharacterRoster] queued owner={} entry=0x{:x} epoch={} selection={:?}",
        scope.owner,
        entry,
        state.epoch,
        selection
    );
}

/// True when a CPU kind request can be queued without the rejection notice.
pub fn cpu_reload_ready() -> bool {
    if busy()
        || training::recording_or_playback()
        || unsafe { training::save_states::is_loading() }
        || lobby::native_ui_busy()
        || button_config::has_pending_request()
        || menu::captures_input()
    {
        return false;
    }
    let state = state_for(Role::Cpu).lock().unwrap();
    state.pending.is_none() && state.observed.is_some() && scope(Role::Cpu).is_some()
}

/// Ask the existing loader to change the CPU to `kind`. Values only.
/// A kind outside the roster notifies once and does not queue.
pub fn request_cpu_kind(kind: u32) -> bool {
    let Some(fighter) = TrioFighter::from_kind(kind) else {
        notify(Role::Cpu, "Named macro fighter is not available".into());
        return false;
    };
    if state_for(Role::Cpu).lock().unwrap().pending.is_some() {
        return false;
    }
    queue(Role::Cpu, Selection::Direct(fighter));
    state_for(Role::Cpu).lock().unwrap().pending.is_some_and(
        |request| matches!(request.selection, Selection::Direct(target) if target == fighter),
    )
}

/// Runs before the delay queue. Consume only our request, giving conflicting
/// save/record/menu requests precedence. Preserve suppression through a held
/// chord so it cannot turn into a delayed taunt or a player trio request.
pub fn handle_input(out: &mut MappedInputs) {
    let player = button_config::consume_player_cycle_request();
    let cpu = button_config::consume_cpu_cycle_request();
    if (player || cpu) && !button_config::has_pending_request() && !menu::captures_input() {
        OWNED_BUTTONS.store(out.buttons.bits(), Ordering::Relaxed);
        if player && cpu {
            notify(
                Role::Player,
                "Use different mappings for player and CPU".into(),
            );
        } else {
            queue(
                if player { Role::Player } else { Role::Cpu },
                Selection::Cycle,
            );
        }
    }
    suppress_owned_buttons(out);
}

/// Keep a consumed chord suppressed through the native body-loading interval,
/// when the rest of the practice hooks intentionally return early.
pub fn suppress_owned_buttons(out: &mut MappedInputs) {
    let owned = OWNED_BUTTONS.load(Ordering::Relaxed) & out.buttons.bits();
    OWNED_BUTTONS.store(owned, Ordering::Relaxed);
    out.buttons.remove(Buttons::from_bits_truncate(owned));
}

/// Cancel plugin intent immediately. An already installed native reload belongs
/// to the native scene teardown; never clear its flags or force its state ahead.
pub fn scene_changed(mode: game_mode::Mode) {
    #[cfg(feature="mii-saved-alpha")]
    mii_saved_alpha::scene_changed();
    #[cfg(feature = "mii-origin-observer")]
    mii_origin_observer::scene_changed();
    #[cfg(feature = "mii-pool-observer")]
    mii_pool_observer::scene_changed();
    #[cfg(feature = "source-ldn-capacity-probe")]
    source_ldn_capacity_probe::scene_changed();
    #[cfg(feature="mii-ldn-qualification-probe")]
    mii_ldn_qualification::scene_changed();
    #[cfg(feature = "source-capacity-probe")]
    source_capacity_probe::scene_changed();
    sources::scene_changed();
    #[cfg(feature="mii-hud-portrait-alpha")]
    mii_hud_portrait::scene_changed();
    #[cfg(feature = "source-owner-probe")]
    source_owner_probe::scene_changed();
    crate::character_items::scene_changed();
    #[cfg(feature = "mii-identity-probe")]
    crate::mii_identity_probe::forget(std::ptr::null());
    #[cfg(feature = "mii-identical-reload-probe")]
    mii_reload_probe::scene_changed();
    let same_lobby = WAITING_MEMORY
        .lock()
        .unwrap()
        .scene_changed(mode.initialized, mode.melee, mode.mode);
    crate::kirby_hats::scene_changed(same_lobby);
    training::named_macros::waiting_scene_changed(same_lobby);
    OWNED_BUTTONS.store(0, Ordering::Relaxed);
    for role in [Role::Player, Role::Cpu] {
        ORIGINAL_KINDS[role as usize].store(u32::MAX, Ordering::Release);
        SOLO_ACTIVE[role as usize].store(false, Ordering::Release);
        let mut state = state_for(role).lock().unwrap();
        state.epoch = state.epoch.wrapping_add(1);
        state.pending = None;
        state.cursor = None;
        state.loaded = None;
        state.observed = None;
        if let Some(active) = state.active.as_mut() {
            active.departed = true;
            trace!("[CharacterRoster] scene departure during reload owner={} target=0x{:x} phase=0x{:x}; native teardown owns cancellation",
                active.scope.owner, active.target.kind(), active.phase);
        }
    }
}

#[inline]
fn aligned(ptr: *const u8) -> bool {
    crate::native_fields::object(ptr as usize)
}

// Required entry fields are borrowed only inside the qualified roster callback
// or install transaction. This helper neither owns nor extends their lifetime.
#[inline]
unsafe fn read<T: Copy>(entry: *const u8, offset: usize) -> T {
    crate::native_fields::read_unchecked(entry as usize, offset)
}

unsafe fn active_body(entry: *const u8, owner: u32, kind: u32) -> bool {
    ready_kind(entry, owner) == Some(kind)
}

unsafe fn ready_kind(entry: *const u8, owner: u32) -> Option<u32> {
    ready_forms(entry, owner).map(|(_, kind)| kind)
}

unsafe fn ready_forms(entry: *const u8, owner: u32) -> Option<(Forms, u32)> {
    if !aligned(entry) || read::<u32>(entry, 0xe0) != owner {
        return None;
    }
    let state = EntryState {
        count: read(entry, 0x14),
        kinds: [read(entry, 0x18), read(entry, 0x1c), read(entry, 0x20)],
        actor_count: read(entry, 0x28),
        installed: read(entry, 0x4150),
        index: read(entry, 0x5918),
        previous_or_next: read(entry, 0x5919),
        pending: read(entry, 0x591a),
        flags: read(entry, 0x591f),
        outer: read(entry, 0x5920),
        loader: read(entry, 0x591c),
        changing: read(entry, 0x5934),
        guard: read(entry, 0x5935),
    };
    TrioFighter::from_kind(state.kinds[0])?;
    let (forms, live_kind) = state.ready()?;
    for index in 0..forms.count as usize {
        let body = read::<*const u8>(entry, 0x4160 + index * 8);
        if !aligned(body)
            || read::<usize>(body, 0) != BASE.load(Ordering::Acquire) + 0x4f80210
            || read::<u32>(body, 0xc) != forms.kinds[index]
            || read::<u64>(body, 0x10) != u64::from(owner)
            || read::<u8>(body, 0x3a) != forms.body_state(index, state.index)
            || !aligned(read::<*const u8>(body, 0x20))
        {
            return None;
        }
    }
    Some((forms, live_kind))
}

/// `ready_forms` without the roster catalog admission: single-kind Mii entries only.
#[cfg(any(
    feature = "mii-identical-reload-probe",
    feature = "mii-pool-observer",
    feature = "mii-origin-observer",
    feature = "mii-saved-alpha"
))]
unsafe fn ready_single_mii(entry: *const u8, owner: u32) -> Option<u32> {
    if !aligned(entry) || read::<u32>(entry, 0xe0) != owner {
        return None;
    }
    let state = EntryState {
        count: read(entry, 0x14),
        kinds: [read(entry, 0x18), read(entry, 0x1c), read(entry, 0x20)],
        actor_count: read(entry, 0x28),
        installed: read(entry, 0x4150),
        index: read(entry, 0x5918),
        previous_or_next: read(entry, 0x5919),
        pending: read(entry, 0x591a),
        flags: read(entry, 0x591f),
        outer: read(entry, 0x5920),
        loader: read(entry, 0x591c),
        changing: read(entry, 0x5934),
        guard: read(entry, 0x5935),
    };
    if !(0x48..=0x4a).contains(&state.kinds[0]) {
        return None;
    }
    let (forms, live_kind) = state.ready()?;
    if forms.count != 1 {
        return None;
    }
    let body = read::<*const u8>(entry, 0x4160);
    if !aligned(body)
        || read::<usize>(body, 0) != BASE.load(Ordering::Acquire) + 0x4f80210
        || read::<u32>(body, 0xc) != live_kind
        || read::<u64>(body, 0x10) != u64::from(owner)
        || read::<u8>(body, 0x3a) != forms.body_state(0, state.index)
        || !aligned(read::<*const u8>(body, 0x20))
    {
        return None;
    }
    Some(live_kind)
}

/// Existing readiness readers only; no native calls or Rust mutexes (also used under G lock).
#[cfg(feature = "mii-cpu-adapter-probe")]
pub(crate) unsafe fn mii_cpu_probe_visible_ready(entry: *const u8, owner: u32) -> bool {
    mii_saved_cpu_visible_ready(entry, owner, 0x49)
}

#[cfg(any(feature = "mii-cpu-adapter-probe", feature = "mii-saved-alpha"))]
pub(crate) unsafe fn mii_saved_cpu_visible_ready(entry: *const u8, owner: u32, kind: u32) -> bool {
    if !(0x48..=0x4a).contains(&kind) { return false; }
    match owner {
        0 => ready_forms(entry, owner).is_some_and(|(_, kind)| kind == 9),
        1 => ready_single_mii(entry, owner) == Some(kind),
        _ => false,
    }
}

/// Called before G lock; the current probe may already own INSTALL, never a roster replacement.
#[cfg(any(feature = "mii-cpu-adapter-probe",feature="mii-saved-alpha"))]
pub(crate) unsafe fn mii_cpu_probe_roster_idle() -> bool {
    native_source::training_cpu_quantity() == Some(1)
        && STATES.iter().all(|state| {
            let state = state.lock().unwrap();
            state.active.is_none() && state.pending.is_none() && state.loaded.is_none()
        })
}

// This current native callback owns entry lifetime. Cached numeric identities
// below are compared only; no saved fighter/entry/module pointer is dereferenced.
// Pre-service dispatch only. Returning here never suppresses the native call.
unsafe fn service(ctx: &ServiceEntryContext, entry: *mut u8) {
    crate::hang_trace::ROSTER_SERVICE.hit();
    if !aligned(entry) {
        return;
    }
    let owner = read::<u32>(entry, 0xe0);
    #[cfg(feature = "mii-pool-observer")]
    mii_pool_observer::service(entry, owner);
    #[cfg(feature = "mii-origin-observer")]
    mii_origin_observer::service(entry, owner);
    #[cfg(feature = "source-ldn-capacity-probe")]
    source_ldn_capacity_probe::service(ctx, entry, owner);
    #[cfg(feature="mii-ldn-qualification-probe")]
    mii_ldn_qualification::service(entry, owner);
    #[cfg(feature = "source-lifecycle-probe")]
    source_lifecycle_probe::service(entry, owner);
    #[cfg(feature = "source-capacity-probe")]
    if source_capacity_probe::service(ctx, entry, owner) {
        return;
    }
    sources::service(ctx, entry, owner);
    #[cfg(feature="mii-hud-portrait-alpha")]
    mii_hud_portrait::service(entry, owner);
    #[cfg(feature = "source-owner-probe")]
    if owner == 0 {
        source_owner_probe::observe_rules();
    }
    #[cfg(feature = "source-owner-probe")]
    if owner == 2 && !cfg!(feature = "managed-sources") {
        source_owner_probe::service(entry);
        return;
    }
    service_body(entry,owner,ctx.caller_lr());
}

unsafe fn service_body(entry:*mut u8,owner:u32,_caller_lr:usize) {
    #[cfg(feature = "console-swap-trace")]
    let _swap_stack = crate::practice_trace::scope(0x1080000);
    if !matches!(owner, 0 | 1 | 4) {
        return;
    }
    let role = if owner == 0 { Role::Player } else { Role::Cpu };
    #[cfg(feature="mii-saved-alpha")]
    mii_catalog_request::service();
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    mii_auto_init::service(entry,owner);
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    mii_replay::service(entry,owner);
    #[cfg(feature="mii-ordinary-player-alpha")]
    if mii_ordinary_player::service(entry,owner){return;}
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    if mii_ordinary_cpu::service(entry,owner){return;}
    #[cfg(feature="mii-saved-alpha")]
    if mii_saved_alpha::service(entry,owner){return;}
    #[cfg(feature = "mii-identity-probe")]
    crate::mii_identity_probe::observe_entry(entry, owner);
    #[cfg(feature = "mii-saved-file-probe")]
    crate::mii_saved_file_probe::on_service(entry, owner);
    #[cfg(feature = "mii-identical-reload-probe")]
    if mii_reload_probe::service(entry, owner, role, _caller_lr) {
        return;
    }
    let active = state_for(role).lock().unwrap().active;
    if let Some(mut active) = active {
        if active.entry != entry as usize || active.scope.owner != owner {
            return;
        }
        active.ticks += 1;
        let phase = u32::from(read::<u8>(entry, 0x5920))
            | (u32::from(read::<u8>(entry, 0x591c)) << 8)
            | (u32::from(read::<u8>(entry, 0x5935)) << 16);
        if phase != active.phase {
            active.phase = phase;
            trace!(
                "[CharacterRoster] phase owner={} target=0x{:x} outer={} loader={} guard={} tick={}",
                owner,
                active.target.kind(),
                phase & 255,
                (phase >> 8) & 255,
                phase >> 16,
                active.ticks
            );
        }
        if !active.departed
            && scope(role) == Some(active.scope)
            && ready_forms(entry, owner) == Some((active.forms, active.target.kind()))
            && read::<u32>(entry, 0x18) == active.target.kind()
            && read::<u8>(entry, 0x5918) == 0
        {
            let mut state = state_for(role).lock().unwrap();
            if state.epoch != active.epoch
                || state.active.map(|current| current.install) != Some(active.install)
            {
                return;
            }
            state.active = None;
            state.loaded = Some((entry as usize, owner, active.target.kind()));
            state.observed = Some((entry as usize, active.target.kind()));
            state.cursor = active.slot.map(|old| {
                if state.revision == active.revision { old }
                else { locate(&setting(&MENU), role, active.target.kind()).unwrap_or(old) }
            });
            if active.scope.waiting() {
                WAITING_MEMORY
                    .lock()
                    .unwrap()
                    .commit(role as usize, WaitingValue::ordinary(active.target,active.forms,active.slot));
            }
            drop(state);
            if active.scope.waiting() {
                crate::kirby_hats::fighter_committed(role as usize, active.target.kind());
            }
            publish_partners(role, active.forms);
            role.sync_selection(active.target);
            #[cfg(feature="mii-hud-portrait-alpha")]
            mii_hud_portrait::before_refresh(entry, owner, active.target.kind());
            refresh_hud(entry, owner, active.target.kind());
            #[cfg(feature = "mii-ordinary-player-alpha")]
            if owner == 0 {
                mii_ordinary_player::complete_generic(entry, active.install, active.target.kind());
            }
            #[cfg(feature = "mii-ordinary-cpu-alpha")]
            if role == Role::Cpu {
                mii_ordinary_cpu::complete_generic(entry, active.install, active.target.kind(), active.slot);
            }
            finish_install(active.install, "roster ready");
            trace!("[CharacterRoster] complete owner={} target={} kind=0x{:x} slot={:?} ticks={} body_id=0x{:x}",
                owner, active.target.name(), active.target.kind(), active.slot.map(|i| i+1),
                active.ticks, read::<u32>(read::<*const u8>(entry, 0x4160), 8));
            notify(role, selection_label(role, active.target, active.slot));
        } else {
            // Departure cancels plugin intent, but the native entry may finish
            // before it is destroyed. Release only serialization; never publish
            // its old selection, HUD or readiness into the new scene.
            if active.departed
                && ready_forms(entry, owner) == Some((active.forms, active.target.kind()))
                && read::<u8>(entry, 0x5918) == 0
            {
                let mut state = state_for(role).lock().unwrap();
                if state.active.map(|current| current.install) == Some(active.install) {
                    state.active = None;
                    drop(state);
                    finish_install(active.install, "departed roster completed");
                    trace!("[CharacterRoster] departed install completed owner={}; selection remains cancelled", owner);
                }
                return;
            }
            if active.ticks == 600 {
                trace!("[CharacterRoster] reload stalled owner={} phase=0x{:x} departed={}; no forced native advance", owner, phase, active.departed);
            }
            let mut state = state_for(role).lock().unwrap();
            if let Some(current) = state.active.as_mut() {
                if current.entry == active.entry && current.epoch == active.epoch {
                    // Preserve a scene-change cancellation from another callback.
                    current.ticks = active.ticks;
                    current.phase = active.phase;
                }
            }
        }
        return;
    }
    if INSTALL.active() {
        return;
    } // Serialize player and CPU resource mutations.
    let request = state_for(role).lock().unwrap().pending;
    let Some(mut request) = request else {
        // Display the live fighter in the picker, including the initial CSS choice.
        // This permits choosing the same menu fighter again after a cycle.
        if let Some(current_scope) =
            scope(role).filter(|s| s.owner == owner && !menu::captures_input())
        {
            let Some((forms, kind)) = ready_forms(entry, owner) else {
                return;
            };
            if let Some(fighter) = TrioFighter::from_kind(kind) {
                let mut state = state_for(role).lock().unwrap();
                // A new same-owner preview can coexist with a departing entry
                // before the mode changes. Keep the identity acquired for this
                // scene until its destructor or scene invalidation releases it.
                if state
                    .observed
                    .is_some_and(|(identity, _)| identity != entry as usize)
                {
                    return;
                }
                let newly_observed = state.observed != Some((entry as usize, kind));
                if newly_observed {
                    let _ = ORIGINAL_KINDS[role as usize].compare_exchange(
                        u32::MAX,
                        kind,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    );
                    state.observed = Some((entry as usize, kind));
                    if state.loaded.is_some_and(|(identity, loaded_owner, _)| {
                        identity == entry as usize && loaded_owner == owner
                    }) {
                        state.loaded = Some((entry as usize, owner, kind));
                    }
                    trace!(
                        "[CharacterRoster] observed owner={} entry=0x{:x} kind=0x{:x} epoch={}",
                        owner,
                        entry as usize,
                        kind,
                        state.epoch
                    );
                }
                let slot = state.cursor;
                drop(state);
                publish_partners(role, forms);
                if newly_observed {
                    // Exact native origin capture supplies a saved Mii default. A kind-only
                    // observation must not suppress its configuration on a later return.
                    #[cfg(feature="mii-saved-alpha")]
                    let exact_origin_pending=mii_backend::current().is_some_and(|l|l.waiting())
                        && (0x48..=0x4a).contains(&kind)
                        && ((role==Role::Player && cfg!(feature="mii-ordinary-player-alpha"))
                            || (role==Role::Cpu && cfg!(feature="mii-ordinary-cpu-alpha")));
                    #[cfg(not(feature="mii-saved-alpha"))]
                    let exact_origin_pending=false;
                    let fresh_lobby = current_scope.waiting()
                        && !exact_origin_pending
                        && WAITING_MEMORY
                            .lock()
                            .unwrap()
                            .observe(role as usize, WaitingValue::ordinary(fighter,forms,None));
                    if fresh_lobby {
                        trace!("[CharacterRoster] fresh waiting entry lifetime; cleared both practice choices");
                        crate::kirby_hats::fresh_lobby();
                        training::named_macros::waiting_scene_changed(false);
                    }
                    role.sync_selection(fighter);
                }
                // A ready entry can precede the scene's first gameplay frame.
                // Reloading it during stage initialization races the native
                // fighter/script setup. Reconcile persisted Solo only once the
                // match is active; menu changes use the same native boundary.
                if kind == 0x4b
                    && crate::common::is_ready_go()
                    && forms != requested_forms_at(&setting(&MENU), role, kind, slot)
                {
                    queue(role, Selection::RefreshPartners);
                    return;
                }
                if current_scope.waiting() && role.enabled() {
                    let target = WAITING_MEMORY.lock().unwrap().restore_target(role as usize);
                    if let Some(target) = target {
                        if restore_owned(role,target) {return;}
                        if target.fighter.kind() == kind && target.forms==forms {
                            WAITING_MEMORY.lock().unwrap().commit(role as usize, target);
                            crate::kirby_hats::fighter_committed(role as usize, kind);
                            state_for(role).lock().unwrap().cursor=target.cursor;
                            role.sync_selection(target.fighter);
                        } else {
                            queue_request(role, Selection::Direct(target.fighter), true);
                        }
                    }
                }
            }
        }
        return;
    };
    if request.scope.owner != owner || request.entry != entry as usize {
        return;
    }
    let mut state = state_for(role).lock().unwrap();
    if request.epoch != state.epoch
        || scope(role) != Some(request.scope)
        || (!role.enabled() && !matches!(request.selection, Selection::RefreshPartners))
    {
        state.pending = None;
        trace!("[CharacterRoster] dropped stale request at native boundary");
        return;
    }
    request.age += 1;
    // A committed menu change waits for the menu's normal input release grace.
    if menu::captures_input() && request.age < 180 {
        state.pending = Some(request);
        return;
    }
    state.pending = None;
    let current = ready_kind(entry, owner);
    if menu::captures_input()
        || lobby::native_ui_busy()
        || button_config::has_pending_request()
        || training::save_states::is_loading()
        || training::recording_or_playback()
        || current.is_none()
    {
        drop(state);
        if !request.restoring {
            trace!(
                "[CharacterRoster] rejected at native boundary: fighter not idle or another action owns practice"
            );
            notify(role, "Character swap unavailable during this action".into());
        }
        return;
    }
    let current = current.unwrap();
    let m = setting(&MENU);
    let (target, slot) = match request.selection {
        Selection::Direct(target) => (target, None),
        Selection::RefreshPartners => (TrioFighter::from_kind(current).unwrap(), state.cursor),
        Selection::Cycle => {
            let choices = roster(&m, role);
            let kinds: Vec<_> = choices.iter().map(|fighter| fighter.kind()).collect();
            let original = cycle_original(role);
            // Slot 0 is told apart from a numbered slot of the same group (Ice Climbers whose
            // Solo preference differs) by the cursor, which a by-group search cannot see.
            let at_original = state.cursor.is_none()
                && original.is_some_and(|kind| cycle_position(&[kind], current).is_some());
            let step = if at_original {
                next_from_original(&kinds, original)
            } else {
                next_cycle(current, state.cursor, &kinds, original)
            };
            let Some((kind, slot)) = step else {
                drop(state);
                notify(role, "Select a character for this cycle".into());
                return;
            };
            let Some(target) = TrioFighter::from_kind(kind) else {
                return;
            };
            (target, slot)
        }
    };
    let remembered=if request.restoring {WAITING_MEMORY.lock().unwrap().restore_target(role as usize)}else{None};
    let slot=remembered.map_or(slot,|v|v.cursor);
    let forms = remembered.map_or_else(||requested_forms_at(&m, role, target.kind(), slot),|v|v.forms);
    if ready_forms(entry, owner) == Some((forms, target.kind())) {
        state.cursor = slot;
        if request.scope.waiting() {
            WAITING_MEMORY.lock().unwrap().commit(role as usize, WaitingValue::ordinary(target,forms,slot));
        }
        drop(state);
        if request.scope.waiting() {
            crate::kirby_hats::fighter_committed(role as usize, target.kind());
        }
        role.sync_selection(target);
        notify(role, selection_label(role, target, slot));
        trace!(
            "[CharacterRoster] already active owner={} kind=0x{:x} slot={:?}",
            owner,
            current,
            slot.map(|i| i + 1)
        );
        return;
    }
    let Some(install_token) = INSTALL.try_begin(true) else {
        state.pending = Some(request);
        return;
    };
    let active = Active {
        install: install_token,
        scope: request.scope,
        epoch: state.epoch,
        revision: state.revision,
        entry: entry as usize,
        target,
        forms,
        slot,
        ticks: 0,
        phase: u32::MAX,
        departed: false,
    };
    state.loaded = None;
    state.active = Some(active);
    drop(state);
    // 653240 synchronously copies this descriptor. Preserve all native role,
    // controller/team/costume options; replace the complete canonical group.
    let mut descriptor = native_fighter_install::Descriptor::copy_from(entry);
    descriptor.request_group(native_fighter_install::Group {
        count: forms.count,
        kinds: forms.kinds,
        actor_count: forms.actor_count,
    });
    training::clear_session_state();
    trace!("[CharacterRoster] install owner={} entry=0x{:x} from=0x{:x} to=0x{:x} slot={:?} epoch={} mode=0x{:x}",
        owner, entry as usize, current, target.kind(), slot.map(|i| i+1), active.epoch, active.scope.mode);
    #[cfg(feature = "console-swap-trace")]
    crate::practice_trace::checkpoint(901, [entry as u64, u64::from(owner), u64::from(current), u64::from(target.kind()), active.epoch as u64]);
    #[cfg(feature = "mii-ordinary-player-alpha")]
    if owner == 0 {
        mii_ordinary_player::begin_generic(entry, install_token, descriptor.bytes());
    }
    #[cfg(feature = "mii-ordinary-cpu-alpha")]
    if role == Role::Cpu {
        mii_ordinary_cpu::begin_generic(entry, install_token, descriptor.bytes());
    }
    #[cfg(feature = "console-swap-trace")]
    crate::practice_trace::checkpoint(902, [entry as u64, u64::from(owner), u64::from(current), u64::from(target.kind()), active.epoch as u64]);
    let base = BASE.load(Ordering::Acquire);
    #[cfg(feature = "native-unlink-probe")]
    crate::native_unlink_probe::arm(entry as usize, owner, current, target.kind());
    native_fighter_install::install(base, entry, &descriptor, native_fighter_install::Flags::Reload);
    #[cfg(feature = "console-swap-trace")]
    crate::practice_trace::checkpoint(903, [entry as u64, u64::from(owner), u64::from(current), u64::from(target.kind()), active.epoch as u64]);
}

/// Called before native entry destruction, while that argument is still owned.
pub unsafe fn on_destroy(entry: *const u8) {
    if !aligned(entry) {
        return;
    }
    let owner = read::<u32>(entry, 0xe0);
    training::practice::entry_departed(entry as usize);
    #[cfg(feature="mii-saved-alpha")]
    mii_saved_alpha::on_destroy(entry,owner);
    #[cfg(feature="mii-ordinary-player-alpha")]
    mii_ordinary_player::on_destroy(entry);
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    mii_ordinary_cpu::on_destroy(entry);
    #[cfg(feature = "mii-pool-observer")]
    mii_pool_observer::on_destroy(owner);
    #[cfg(feature = "mii-origin-observer")]
    mii_origin_observer::on_destroy(owner);
    #[cfg(feature = "source-ldn-capacity-probe")]
    source_ldn_capacity_probe::on_destroy(entry, owner);
    #[cfg(feature="mii-ldn-qualification-probe")]
    mii_ldn_qualification::on_destroy(entry, owner);
    #[cfg(feature = "source-capacity-probe")]
    source_capacity_probe::on_destroy(entry, owner);
    #[cfg(feature = "source-lifecycle-probe")]
    source_lifecycle_probe::destroy(entry, owner);
    sources::on_destroy(entry, owner);
    #[cfg(feature="mii-hud-portrait-alpha")]
    mii_hud_portrait::on_destroy(entry, owner);
    #[cfg(feature = "source-owner-probe")]
    if owner == 2 && !cfg!(feature = "managed-sources") {
        source_owner_probe::destroy(entry);
        return;
    }
    if !matches!(owner, 0 | 1 | 4) {
        return;
    }
    let role = if owner == 0 { Role::Player } else { Role::Cpu };
    #[cfg(feature = "mii-identity-probe")]
    crate::mii_identity_probe::forget(entry);
    #[cfg(feature = "mii-identical-reload-probe")]
    mii_reload_probe::on_destroy(entry);
    let mut state = state_for(role).lock().unwrap();
    let practice_departure = role == Role::Player
        && (state
            .observed
            .is_some_and(|(identity, _)| identity == entry as usize)
            || state
                .active
                .is_some_and(|active| active.entry == entry as usize));
    if role == Role::Player
        && lobby::is_waiting_scene()
        && (state
            .observed
            .is_some_and(|(identity, _)| identity == entry as usize)
            || state.active.is_some_and(|a| {
                a.entry == entry as usize
                    && a.epoch == state.epoch
                    && !a.departed
                    && a.scope.waiting()
            }))
    {
        WAITING_MEMORY.lock().unwrap().player_departed();
    }
    if state.pending.is_some_and(|r| r.entry == entry as usize) {
        trace!(
            "[CharacterRoster] canceled queued request on entry destruction owner={} entry=0x{:x}",
            owner,
            entry as usize
        );
        state.pending = None;
    }
    if state
        .loaded
        .is_some_and(|(identity, _, _)| identity == entry as usize)
    {
        state.loaded = None;
        state.cursor = None;
    }
    if state
        .observed
        .is_some_and(|(identity, _)| identity == entry as usize)
    {
        trace!(
            "[CharacterRoster] invalidated observed entry before destruction owner={} entry=0x{:x}",
            owner,
            entry as usize
        );
        state.observed = None;
        // Mii adapters publish a cursor without setting the ordinary loader's
        // `loaded` receipt. Retire it with the observed entry as well: Training
        // -> CSS -> Training can keep the same mode tuple, and a new CSS fighter
        // must start at slot 0. Lobby continuation lives in WAITING_MEMORY.
        state.cursor = None;
        ORIGINAL_KINDS[role as usize].store(u32::MAX, Ordering::Release);
        SOLO_ACTIVE[role as usize].store(false, Ordering::Release);
    }
    if state.active.is_some_and(|a| a.entry == entry as usize) {
        trace!("[CharacterRoster] entry_destroy owner={} active={:?}; references invalidated before native destruction", owner, state.active);
        let active = state.active.take().unwrap();
        state.cursor = None;
        finish_install(active.install, "roster destroyed");
    }
    drop(state);
    if practice_departure {
        // Native Training -> CSS -> Training retains the same mode tuple. The
        // actual P1 entry destructor is also a scene boundary for plugin intent.
        #[cfg(feature = "source-capacity-probe")]
        source_capacity_probe::scene_changed();
        sources::scene_changed();
        #[cfg(feature="mii-hud-portrait-alpha")]
        mii_hud_portrait::scene_changed();
        crate::kirby_hats::practice_departed(lobby::is_waiting_scene());
        crate::character_items::scene_changed();
        trace!("[CharacterRoster] practice consumers invalidated on P1 entry destruction");
    }
}

/// Called only after a Mii adapter accepts its exact completion, while its
/// current callback and INSTALL token are still owned. Publish presentation only;
/// original CSS identity and saved configuration/preferences remain untouched.
#[cfg(feature = "mii-saved-alpha")]
unsafe fn publish_mii_presentation(
    entry: *const u8,
    owner: u32,
    generation: u64,
    kind: u32,
    #[cfg(feature="mii-hud-portrait-alpha")] portrait: PortraitIntent,
) {
    let Some(logical)=mii_backend::current().and_then(|l|l.logical(owner)) else{return;};
    let role = if owner == 0 { Role::Player } else { Role::Cpu };
    let Some(fighter) = TrioFighter::from_kind(kind) else { return; };
    if scope(role).is_none_or(|s|s.owner!=owner)
        || mii_saved_alpha::current_generation(logical as u32) != generation
        || !active_body(entry, owner, kind)
        || read::<u32>(entry, 0x18) != kind
        || read::<u8>(entry, 0x5918) != 0
    {
        return;
    }
    let mut state = state_for(role).lock().unwrap();
    if mii_saved_alpha::current_generation(logical as u32) != generation
        || state.active.is_some()
        || state.pending.is_some()
        || state.observed.is_some_and(|(identity, _)| identity != entry as usize)
        || state.loaded.is_some_and(|(identity, loaded_owner, _)| {
            identity != entry as usize || loaded_owner != owner
        })
    {
        return;
    }
    state.observed = Some((entry as usize, kind));
    drop(state);
    role.sync_selection(fighter);
    #[cfg(feature="mii-hud-portrait-alpha")]
    if mii_hud_portrait::should_defer_hud(owner, kind, portrait) {
        // The exact texture does not exist yet. Keep the previous portrait and let the
        // service callback refresh and bind after the render proof. A rejected hold
        // still paints the new kind, so the old portrait cannot stick.
        let accepted = mii_hud_portrait::request(entry, owner, generation, kind, portrait, true);
        if policy_rejected_hud(accepted) {
            mii_hud_portrait::before_refresh(entry, owner, kind);
            refresh_hud(entry, owner, kind);
        }
    } else {
        mii_hud_portrait::before_refresh(entry, owner, kind);
        refresh_hud(entry, owner, kind);
        mii_hud_portrait::request(entry, owner, generation, kind, portrait, false);
    }
    #[cfg(not(feature="mii-hud-portrait-alpha"))]
    refresh_hud(entry, owner, kind);
}

/// The publish site is the engine entry-service callback, after
/// `mii_hud_portrait::service` has returned, so this call does not hold that
/// function's `ServiceCallback`. Rejection still falls back here.
#[cfg(feature="mii-hud-portrait-alpha")]
fn policy_rejected_hud(accepted: bool) -> bool {
    mii_hud_portrait_policy::rejected_deferral(true, accepted, true, true)
        == mii_hud_portrait_policy::HudStep::RefreshFallback
}

/// Native reset already owns the HUD lookup. Admit our fresh, active fighter body
/// into its existing presentation helper; never retain a HUD pane reference.
pub unsafe fn allow_portrait(ctx: &mut InlineCtx, entry_id: u64, entry: *const u8) {
    if busy() || entry_id > 1 {
        return;
    }
    let role = if entry_id == 0 {
        Role::Player
    } else {
        Role::Cpu
    };
    if scope(role).is_none_or(|s| s.mode != 0x04040000) {
        return;
    }
    let loaded = state_for(role).lock().unwrap().loaded;
    if let Some((identity, owner, kind)) = loaded {
        if identity == entry as usize
            && ctx.registers[0].x() as u32 == kind
            && active_body(entry, owner, kind)
        {
            ctx.registers[8].set_x(0);
        }
    }
}

#[repr(C)]
struct PortraitEvent {
    vtable: usize,
    event_type: u32,
    entry: u32,
    active_index: u32,
    kind: u32,
}

/// Full reload emits body-ID event1e but not the portrait event1d. Find only
/// the registered HUD listener now and invoke its native functor. The native
/// dispatch depth/deferred-removal protocol protects iterator lifetime: removal
/// is postponed while depth != 0, and the flush runs when this call returns the
/// depth to zero. The functor itself runs on this caller. Publication and the
/// later portrait service are both the 650010 entry callback, so a deferred
/// refresh uses that same callback and does not hop threads.
unsafe fn refresh_hud(entry: *const u8, owner: u32, kind: u32) {
    let role = if owner == 0 { Role::Player } else { Role::Cpu };
    let current_scope = scope(role);
    let waiting_cpu = owner == 4 && current_scope.is_some_and(|s| s.owner == owner);
    let ordinary = owner <= 1 && current_scope.is_some_and(|s| s.mode == 0x04040000);
    if !ordinary && !waiting_cpu {
        trace!(
            "[CharacterRoster] HUD skipped: owner={} scope={:?} game={:?}",
            owner,
            scope(role),
            game_mode::current()
        );
        return;
    }
    let base = BASE.load(Ordering::Acquire);
    let symbol = setting(&crate::common::FIGHTER_MANAGER_ADDR);
    let hud_singleton = read::<usize>(base as *const u8, 0x52c3420);
    if symbol == 0 || hud_singleton == 0 {
        trace!(
            "[CharacterRoster] HUD skipped: manager_symbol=0x{:x} hud_singleton=0x{:x}",
            symbol,
            hud_singleton
        );
        return;
    }
    let fighter_manager = std::ptr::read_volatile(symbol as *const *mut u8);
    if !aligned(fighter_manager) {
        trace!(
            "[CharacterRoster] HUD skipped: fighter_manager=0x{:x}",
            fighter_manager as usize
        );
        return;
    }
    // The exported singleton is the FighterManager, whose first field points
    // to the event dispatcher. Native65617c/656194 perform these two loads.
    // The dispatch count/lists/depth belong to that second object.
    let manager = read::<*mut u8>(fighter_manager, 0);
    if !aligned(manager) {
        trace!(
            "[CharacterRoster] HUD skipped: manager=0x{:x}",
            manager as usize
        );
        return;
    }
    let list_count = read::<u32>(manager, 0);
    if list_count <= 0x1d {
        trace!(
            "[CharacterRoster] HUD skipped: manager=0x{:x} list_count={}",
            manager as usize,
            list_count
        );
        return;
    }
    let lists = read::<*mut u8>(manager, 8);
    if !aligned(lists) {
        trace!(
            "[CharacterRoster] HUD skipped: lists=0x{:x} list_count={}",
            lists as usize,
            list_count
        );
        return;
    }
    let sentinel = lists.add(0x2c0);
    let event = PortraitEvent {
        vtable: base + 0x4f82ef8,
        event_type: 0x1d,
        entry: read(entry, 0x41e4),
        active_index: 0,
        kind,
    };
    if event.entry != owner {
        trace!(
            "[CharacterRoster] HUD skipped: event_entry={} owner={}",
            event.entry,
            owner
        );
        return;
    }
    let depth = read::<u32>(manager, 0x18);
    if depth == u32::MAX {
        trace!("[CharacterRoster] HUD skipped: invalid dispatch depth");
        return;
    }
    crate::native_fields::write_unchecked(manager as usize, 0x18, depth + 1);
    let mut node = read::<*const u8>(sentinel, 0);
    let mut delivered = 0;
    let mut visited = 0;
    while node != sentinel && aligned(node) && visited < 64 {
        visited += 1;
        let listener = read::<*mut u8>(node, 0x10);
        if aligned(listener) {
            trace!(
                "[CharacterRoster] HUD listener owner={} vtable=0x{:x} member=0x{:x}",
                owner,
                read::<usize>(listener, 0).wrapping_sub(base),
                read::<usize>(listener, 0x18).wrapping_sub(base)
            );
        }
        if aligned(listener)
            && read::<usize>(listener, 0) == base + 0x5063320
            && read::<usize>(listener, 0x18) == base + 0x1377b40
            && read::<usize>(listener, 0x20) == 0
            && aligned(read::<*const u8>(listener, 0x10))
            && read::<usize>((base + 0x5063320) as *const u8, 0x18) == base + 0x1378ad0
        {
            if waiting_cpu {
                refresh_waiting_icon(listener, owner, kind, base);
            } else {
                let invoke: unsafe extern "C" fn(*mut u8, *const PortraitEvent) =
                    std::mem::transmute(base + 0x1378ad0);
                let hud = pointer_chain(base as *const u8, &[0x52c3420, 8, 0])
                    .map_or(0, |root| root as usize);
                native_hud_refresh::without_roster_animation(hud, owner, || {
                    invoke(listener, &event);
                });
            }
            delivered += 1;
        }
        node = read::<*const u8>(node, 0);
    }
    let remaining = read::<u32>(manager, 0x18) - 1;
    crate::native_fields::write_unchecked(manager as usize, 0x18, remaining);
    if remaining == 0 && read::<u32>(manager, 0x1c) != 0 {
        let flush: unsafe extern "C" fn(*mut u8) = std::mem::transmute(base + 0x37ae770);
        flush(manager);
    }
    trace!(
        "[CharacterRoster] HUD refresh owner={} kind=0x{:x} delivered={} listeners={}",
        owner,
        kind,
        delivered,
        visited
    );
}

/// Waiting HUD construction at 14ede24 uses the same eight-record presentation
/// table as ordinary Training, with a distinct inner vtable. Check that fresh
/// registry-owned object before admitting CPU4; never cache a pane or HUD pointer.
unsafe fn refresh_waiting_icon(listener: *const u8, owner: u32, kind: u32, base: usize) {
    let refreshed = waiting_icon(listener, owner, kind, base);
    trace!(
        "[CharacterRoster] waiting icon owner={} kind=0x{:x} refreshed={}",
        owner,
        kind,
        refreshed
    );
}

/// CPU4's waiting-card icon pane, read fresh through the generation-checked waiting UI
/// handle for both the local and the online waiting controller. No texture assignment
/// and no heap use.
#[cfg(feature="mii-hud-portrait-alpha")]
pub(super) unsafe fn waiting_icon_pane(base: usize, owner: u32) -> Option<*const u8> {
    if owner != 4 {
        return None;
    }
    waiting_icon_source(base).map(|(_, pane)| pane)
}

/// Undo a custom waiting portrait with the same native kind assignment used by
/// ordinary swaps. Resolve the registered listener and generation-checked pane
/// now; never use a saved UI address during departure. Success retires its native
/// texture override, allowing the portrait batch to be destroyed afterward.
#[cfg(feature="mii-hud-portrait-alpha")]
pub(super) unsafe fn restore_waiting_kind_icon(base: usize, kind: u32) -> bool {
    waiting_icon_source(base).is_some_and(|(listener, _)| waiting_icon(listener, 4, kind, base))
}

#[cfg(feature="mii-hud-portrait-alpha")]
unsafe fn waiting_icon_source(base: usize) -> Option<(*const u8, *const u8)> {
    let symbol = setting(&crate::common::FIGHTER_MANAGER_ADDR);
    if symbol == 0 || read::<usize>(base as *const u8, 0x52c3420) == 0 {
        return None;
    }
    let fighter_manager = read::<*mut u8>(symbol as *const u8, 0);
    if !aligned(fighter_manager) {
        return None;
    }
    let manager = read::<*mut u8>(fighter_manager, 0);
    if !aligned(manager) || read::<u32>(manager, 0) <= 0x1d {
        return None;
    }
    let lists = read::<*mut u8>(manager, 8);
    if !aligned(lists) {
        return None;
    }
    let sentinel = lists.add(0x2c0);
    let mut node = read::<*const u8>(sentinel, 0);
    let mut visited = 0;
    while node != sentinel && aligned(node) && visited < 64 {
        visited += 1;
        let listener = read::<*mut u8>(node, 0x10);
        if aligned(listener)
            && read::<usize>(listener, 0) == base + 0x5063320
            && read::<usize>(listener, 0x18) == base + 0x1377b40
            && read::<usize>(listener, 0x20) == 0
            && aligned(read::<*const u8>(listener, 0x10))
            && read::<usize>((base + 0x5063320) as *const u8, 0x18) == base + 0x1378ad0
        {
            if let Some(pane) = waiting_card_pane(listener, base) {
                return Some((listener, pane));
            }
        }
        node = read::<*const u8>(node, 0);
    }
    None
}

#[cfg(feature="mii-hud-portrait-alpha")]
unsafe fn waiting_card_pane(listener: *const u8, base: usize) -> Option<*const u8> {
    waiting_partner_icon(listener, 4, base, false).map(|(icon, _)| icon)
}

/// Resolve CPU `owner`'s partner icon pane and the waiting UI root that owns its
/// texture cache, through the game's generation-checked waiting UI handle (136aef8).
/// The local (`view+0xd3a == 0`) and online waiting controllers differ only in their
/// offsets; presentation ID -> visible partner index is resolved exactly as the native
/// damage setters do (13cecb0 / 13d0f50), never assuming CPU4 is pane zero. `log`
/// traces rejections for the swap path; the per-tick portrait path stays silent.
/// Reads only; no heap use; no address survives this call.
unsafe fn waiting_partner_icon(
    listener: *const u8, owner: u32, base: usize, log: bool,
) -> Option<(*const u8, *const u8)> {
    if owner != 4 {
        return None;
    }
    let hud = read::<*const u8>(listener, 0x10);
    let inner = read::<*const u8>(hud, 0x20);
    if !aligned(inner) || read::<usize>(inner, 0) != base + 0x5062f60 {
        if log {
            trace!("[CharacterRoster] waiting HUD rejected: inner=0x{:x}", inner as usize);
        }
        return None;
    }
    let records = read::<*const u8>(inner, 0x18);
    if records as usize != base + 0x52c5730 {
        if log {
            trace!("[CharacterRoster] waiting HUD rejected: records=0x{:x}", records as usize);
        }
        return None;
    }
    let slot = read::<i32>(records, owner as usize * 0x260 + 0xc);
    if log {
        trace!("[CharacterRoster] waiting HUD owner={} presentation_slot={}", owner, slot);
    }
    if !(0..8).contains(&slot) {
        return None;
    }
    let root = pointer_chain(base as *const u8, &[0x52c3420, 8, 0])?;
    let context = read::<*const u8>(root, 0xb98);
    if !aligned(context) {
        return None;
    }
    let handle = [base + 0x5063bc0, context as usize, read::<u32>(root, 0xba0) as usize];
    let valid: unsafe extern "C" fn(*const usize) -> bool = std::mem::transmute(base + 0x138b6c0);
    if !valid(handle.as_ptr()) {
        return None;
    }
    let view = pointer_chain(context, &[0])?;
    let online = read::<u8>(view, 0xd3a) != 0;
    let controller = pointer_chain(view, &[if online { 0xa50 } else { 0x420 }])?;
    let control_state = pointer_chain(controller, &[0])?;
    if read::<u8>(control_state, 0x14) == 0 {
        return None;
    }
    let mut indices = [i32::MIN; 3];
    let entries = if online {
        let collection = pointer_chain(controller, &[0x2f0])?;
        if read::<u8>(collection, 0) == 0 {
            return None;
        }
        let begin = read::<*const u8>(collection, 0x18);
        let end = read::<usize>(collection, 0x20);
        let length = end.wrapping_sub(begin as usize);
        if !aligned(begin) || length > 12 || !length.is_multiple_of(4) {
            return None;
        }
        for (i, value) in indices.iter_mut().enumerate().take(length / 4) {
            *value = read::<i32>(begin, i * 4);
        }
        read::<*const u8>(collection, 8)
    } else {
        for (i, value) in indices.iter_mut().enumerate() {
            *value = read::<i32>(controller, 0x150 + i * 4);
        }
        read::<*const u8>(controller, 0x148)
    };
    if !aligned(entries) {
        return None;
    }
    let index = indices.iter().position(|&i| {
        (0..8).contains(&i) && read::<i32>(entries, i as usize * 0x1d8 + 0x50) == slot
    })?;
    let ui = pointer_chain(controller, &[if online { 8 } else { 0x130 }, 0x80])?;
    let partners = pointer_chain(ui, &[if online { 0x20 } else { 0x220 }])?;
    let icon = pointer_chain(partners, &[0x98 + index * 0x58])?;
    Some((icon, ui))
}

unsafe fn waiting_icon(listener: *const u8, owner: u32, kind: u32, base: usize) -> bool {
    let Some((icon, ui)) = waiting_partner_icon(listener, owner, base, true) else {
        return false;
    };
    let Some(cache) = pointer_chain(ui, &[0, 0x78]) else {
        return false;
    };
    let texture: unsafe extern "C" fn(*const u8, u64, u32, u32) -> *const u8 =
        std::mem::transmute(base + 0x323f330);
    let assign: unsafe extern "C" fn(*const u8, *const u8) = std::mem::transmute(base + 0x3778b80);
    let image = texture(cache, fighter_kind_hash(base, kind), 0, 0);
    let Some(image_data) = pointer_chain(image, &[8]) else {
        return false;
    };
    if read::<usize>(image_data, 0) == 0 {
        trace!(
            "[CharacterRoster] waiting icon texture missing kind=0x{:x}",
            kind
        );
        return false;
    }
    // Only the texture binding changes; partner descriptor, damage and CSS
    // stay native. Native setter retains/releases the texture reference.
    assign(icon, image);
    true
}

// These are the local/online waiting HUD's resource-registration callbacks,
// before its native load barrier and pane construction. Append small stock icons
// to that same owned loader so swaps need no asynchronous UI work or saved panes.
#[skyline::hook(offset = 0x13cdc00)]
unsafe fn load_local_waiting_icons(context: *const u8) {
    original!()(context);
    preload_waiting_icons(context);
}

#[skyline::hook(offset = 0x13d0360)]
unsafe fn load_online_waiting_icons(context: *const u8) {
    original!()(context);
    preload_waiting_icons(context);
}

unsafe fn preload_waiting_icons(context: *const u8) {
    // Settings are loaded before waiting entry. The practice menu cannot open
    // while Lobby Practice is off; enabling it takes effect on the next entry.
    // Check the persisted setting here, not scene/UI readiness: this callback
    // runs before the waiting HUD's native loading barrier and pane creation.
    if setting(&MENU).lobby_practice != OnOff::ON {
        trace!("[CharacterRoster] waiting icon preload skipped: Lobby Practice off");
        return;
    }
    let Some(loader) = pointer_chain(context, &[0x50, 0, 8, 0, 0xcd0, 0x28]) else {
        trace!("[CharacterRoster] waiting icon preload skipped: no native loader");
        return;
    };
    let base = BASE.load(Ordering::Acquire);
    let icon_hash: unsafe extern "C" fn(u64, u32, u32) -> u64 =
        std::mem::transmute(base + 0x3238a70);
    let request: unsafe extern "C" fn(*const u8, *const u64, i64, u32) =
        std::mem::transmute(base + 0x323c4e0);
    let mut count = 0;
    for fighter in TrioFighter::available() {
        let hash = icon_hash(fighter_kind_hash(base, fighter.kind()), 0, 0);
        if hash & 0xffffffffff != 0 {
            request(
                loader,
                &hash,
                i64::from(read::<i32>(loader, 0x70)),
                read(loader, 0x68),
            );
            count += 1;
        }
    }
    trace!("[CharacterRoster] waiting icon preload requested={} loader=0x{:x}; native load barrier retained", count, loader as usize);
}

// Both icon paths use this roster module's published, verified text base.
// The conversion borrows no native object and retains no address.
#[inline]
unsafe fn fighter_kind_hash(base: usize, kind: u32) -> u64 {
    let convert: unsafe extern "C" fn(u32) -> u64 = std::mem::transmute(base + 0x138c9b0);
    convert(kind)
}

#[inline]
unsafe fn pointer_chain(object: *const u8, offsets: &[usize]) -> Option<*const u8> {
    crate::native_fields::chain(object as usize, offsets).map(|address| address as *const u8)
}

pub fn init() {
    unsafe {
        let base = getRegionAddress(Region::Text) as usize;
        for (offset, words) in [
            (
                0x650010,
                [0xd103c3ffu32, 0xa9096ffc, 0xa90a67fa, 0xa90b5ff8],
            ),
            (
                0x653240,
                [0xd101c3ffu32, 0xf90013f9, 0xa9035ff8, 0xa90457f6],
            ),
            (
                0x652160,
                [0xd10103ffu32, 0xa90157f6, 0xa9024ff4, 0xa9037bfd],
            ),
            (
                0x1377b40,
                [0xf9401000u32, 0xb40000e0, 0xf9400009, 0x29418828],
            ),
            (
                0x1378ad0,
                [0xa9410808u32, 0xf9401009, 0x8b890500, 0x36000069],
            ),
            (
                0x13639d0,
                [0xd10203ffu32, 0xf9001bf9, 0xa9045ff8, 0xa90557f6],
            ),
            (
                0x37ae770,
                [0xb9401c08u32, 0x340007e8, 0xa9bb67fa, 0xa9015ff8],
            ),
            (
                0x138b6c0,
                [0xf9400408u32, 0xb4000168u32, 0xf9400108u32, 0x3974e909u32],
            ),
            (
                0x138c9b0,
                [0xf81d0ff5u32, 0xa9014ff4u32, 0xa9027bfdu32, 0x910083fdu32],
            ),
            (
                0x323f330,
                [0xd100c3ffu32, 0xf9000bf3u32, 0xa9027bfdu32, 0x910083fdu32],
            ),
            (
                0x13cdc00,
                [0xf81e0ff3u32, 0xa9017bfdu32, 0x910043fdu32, 0xf9402808u32],
            ),
            (
                0x13d0360,
                [0xf81e0ff3u32, 0xa9017bfdu32, 0x910043fdu32, 0xf9402808u32],
            ),
            (
                0x3238a70,
                [0xd106c3ffu32, 0xa91667fcu32, 0xa9175ff8u32, 0xa91857f6u32],
            ),
            (
                0x323c4e0,
                [0xd10243ffu32, 0xf9001bfbu32, 0xa90467fau32, 0xa9055ff8u32],
            ),
            (
                0x3778b80,
                [0xd100c3ffu32, 0xa9014ff4u32, 0xa9027bfdu32, 0x910083fdu32],
            ),
        ] {
            if std::slice::from_raw_parts((base + offset) as *const u32, 4) != words {
                trace!(
                    "[CharacterRoster] fingerprint mismatch +0x{:x}; disabled",
                    offset
                );
                return;
            }
        }
        BASE.store(base, Ordering::Release);
    #[cfg(feature="mii-saved-alpha")]
    mii_saved_alpha::init(base);
        #[cfg(feature = "source-lifecycle-probe")]
        source_lifecycle_probe::install(base);
        #[cfg(any(
            feature = "source-capacity-probe",
            feature = "managed-third-source-alpha"
        ))]
        native_source_capacity::init(base);
        #[cfg(feature = "source-capacity-probe")]
        source_capacity_probe::install(base);
        #[cfg(feature="mii-ordinary-player-alpha")]
        if !mii_ordinary_player::init(base) {return;}
        #[cfg(feature="mii-ldn-qualification-probe")]
        mii_ldn_qualification::init(base);
        #[cfg(feature="mii-ordinary-cpu-alpha")]
        mii_ordinary_cpu::init();
        native_service::install(base);
        #[cfg(feature="mii-hud-portrait-alpha")]
        mii_hud_portrait::init(base);
        #[cfg(feature = "mii-origin-observer")]
        mii_origin_observer::init(base);
        if cfg!(feature = "waiting-roster-reload") {
            skyline::install_hooks!(load_local_waiting_icons, load_online_waiting_icons);
        }
        trace!("[CharacterRoster] native async loader installed; ordinary P1/CPU1; waiting_enabled={}; one existing entry per role; no manual loader pumping", cfg!(feature = "waiting-roster-reload"));
    }
}

/// Development-only one-shot identical-descriptor reload of P1's native Mii in Training.
/// It observes whether a same-kind state-3 cycle re-creates the body and re-runs the native
/// Mii resource rebuild. No descriptor byte, record, saved entry or information block is
/// written, except that `mii-special-change-probe` writes the neutral special of the bound saved
/// row and information block, and `mii-fullconfig-probe` writes the full alternate configuration
/// (`crate::mii_fullconfig_probe`, shot 1) and restores it (shot 2) on the same live entry.
/// The install token is held until native completion or entry destruction.
#[cfg(feature = "mii-identical-reload-probe")]
pub(crate) mod mii_reload_probe {
    use super::*;
    #[cfg(feature = "mii-cpu-adapter-probe")]
    use crate::mii_fullconfig_probe::{ack, ack_phase};

    const ARM_FILE: &str = "sd:/ultimate/TrainingModpack/mii_probe.arm";
    /// Second arm file required by the special-change variant; both must exist.
    const CHANGE_ARM_FILE: &str = "sd:/ultimate/TrainingModpack/mii_special_probe.arm";
    const CHANGE: bool = cfg!(feature = "mii-special-change-probe");
    /// Full-configuration variant: both shots go through `crate::mii_fullconfig_probe::step`.
    const FULL: bool = cfg!(feature = "mii-fullconfig-probe");
    const TARGET_OWNER: u32 = if cfg!(feature = "mii-cpu-adapter-probe") { 1 } else { 0 };
    /// Identical reload: one shot. Special change: shot 1 changes, shot 2 restores.
    const MAX_SHOTS: usize = if CHANGE || FULL { 2 } else { 1 };

    #[derive(Clone, Copy, Debug)]
    struct Track {
        token: InstallToken,
        entry: usize,
        kind: u32,
        ticks: usize,
        phase: u32,
        saw_reload: bool,
        departed: bool,
    }

    static ARMED: AtomicBool = AtomicBool::new(false);
    static FIRED: AtomicBool = AtomicBool::new(false);
    static SHOTS: AtomicUsize = AtomicUsize::new(0);

    /// Shot-1 record. `fe` is the live entry's identity only: it is compared with callback
    /// arguments and never dereferenced. `original` held equal `E+0x1b2..` and `D+0x390..` values.
    #[derive(Clone, Copy, Debug)]
    struct ChangeState {
        fe: usize,
        id: u32,
        e: usize,
        original: [u8; 4],
        written: u8,
        changed: bool,
    }

    impl ChangeState {
        /// Saved-row specials expected while shot 1 is in effect.
        fn changed_tuple(&self) -> [u8; 4] {
            [self.written, self.original[1], self.original[2], self.original[3]]
        }
    }
    static ORIGINAL: Mutex<Option<ChangeState>> = Mutex::new(None);
    static REQUESTED: AtomicBool = AtomicBool::new(false);
    static HELD: AtomicBool = AtomicBool::new(false);
    static TRACK: Mutex<Option<Track>> = Mutex::new(None);

    pub fn init() {
        let armed = std::path::Path::new(ARM_FILE).exists()
            && (!CHANGE || std::path::Path::new(CHANGE_ARM_FILE).exists());
        // Third arm file and the fingerprint-gated hook installation of the full-configuration probe.
        #[cfg(feature = "mii-fullconfig-probe")]
        let armed = armed && crate::mii_fullconfig_probe::armed();
        #[cfg(feature = "mii-cpu-adapter-probe")]
        let armed = armed && ack::init();
        ARMED.store(armed, Ordering::Relaxed);
        #[cfg(feature = "mii-cpu-adapter-probe")]
        trace!("[MiiCpuAdapter] armed={} target_owner=1 native_training_only=true ordinary_observation_once=true old_p1_mutation_disabled=true", armed);
        trace!("[MiiReloadProbe] armed={} arm_file={} change={} change_arm_file={} full={} max_shots={}",
            armed, ARM_FILE, CHANGE, CHANGE_ARM_FILE, FULL, MAX_SHOTS);
    }

    unsafe fn permitted() -> bool {
        crate::common::is_native_training_mode()
            && crate::common::is_ready_go()
            && !menu::captures_input()
            && !INSTALL.active()
            && !training::save_states::is_loading()
            && !training::recording_or_playback()
            && !crate::replay_preview::occupied()
    }

    /// L3+X edge while permitted requests the one shot; the chord is consumed like the
    /// item-state probe does. Both features cannot be enabled together (lib.rs guard).
    pub unsafe fn handle_input(out: &mut MappedInputs, click: bool, x: bool) {
        let held = click && x;
        let was_held = HELD.swap(held, Ordering::Relaxed);
        if !held || !permitted() {
            return;
        }
        *out = MappedInputs::empty();
        #[cfg(feature = "mii-cpu-adapter-probe")]
        if !ack::inputs_enabled() { return; }
        if held && !was_held && ARMED.load(Ordering::Relaxed) && !FIRED.load(Ordering::Relaxed)
            && SHOTS.load(Ordering::Relaxed) < MAX_SHOTS
        {
            REQUESTED.store(true, Ordering::Relaxed);
            trace!("[MiiReloadProbe] requested; waiting for a ready native Mii entry for owner {}", TARGET_OWNER);
        }
    }

    unsafe fn info_block(entry: *const u8) -> (u16, u8, u16) {
        let d = read::<*const u8>(entry, 0xf8);
        if aligned(d) {
            (read::<u16>(d, 0x72), read::<u8>(d, 0x94), read::<u16>(entry, 0x46))
        } else {
            (u16::MAX, u8::MAX, read::<u16>(entry, 0x46))
        }
    }

    /// Runs inside the native service callback with the live entry. Returns true while this
    /// probe owns the entry's reload so the roster path leaves that tick alone.
    pub(super) unsafe fn service(entry: *mut u8, owner: u32, role: Role, _service_entry_lr: usize) -> bool {
        #[cfg(feature = "mii-cpu-adapter-probe")]
        if owner == 0 && ARMED.load(Ordering::Relaxed)
            && crate::mii_fullconfig_probe::cpu_ordinary_pending() && permitted()
        {
            crate::mii_fullconfig_probe::observe_cpu_ordinary(entry);
        }
        let tracked = *TRACK.lock().unwrap();
        if let Some(mut track) = tracked {
            if track.entry != entry as usize {
                return false;
            }
            track.ticks += 1;
            let phase = u32::from(read::<u8>(entry, 0x5920))
                | (u32::from(read::<u8>(entry, 0x591c)) << 8)
                | (u32::from(read::<u8>(entry, 0x5935)) << 16);
            if phase & 255 == 3 {
                track.saw_reload = true;
            }
            if phase != track.phase {
                track.phase = phase;
                trace!(
                    "[MiiReloadProbe] phase owner={} outer={} loader={} guard={} tick={}",
                    owner, phase & 255, (phase >> 8) & 255, phase >> 16, track.ticks
                );
            }
            if track.saw_reload
                && ready_single_mii(entry, owner) == Some(track.kind)
                && read::<u8>(entry, 0x5918) == 0
            {
                let body = read::<*const u8>(entry, 0x4160);
                let body_id = read::<u32>(body, 8);
                let (d72, d94, fe46) = info_block(entry);
                trace!(
                    "[MiiReloadProbe] complete owner={} kind=0x{:x} ticks={} body_id=0x{:x} fe46=0x{:x} d72=0x{:x} d94={}",
                    owner, track.kind, track.ticks, body_id, fe46, d72, d94
                );
                #[cfg(feature = "mii-fullconfig-probe")]
                crate::mii_fullconfig_probe::on_complete(entry, _service_entry_lr);
                #[cfg(not(feature = "mii-cpu-adapter-probe"))]
                finish_install(track.token, "identical reload probe complete");
                #[cfg(feature = "mii-cpu-adapter-probe")]
                let finished = finish_install(track.token, "identical reload probe complete");
                *TRACK.lock().unwrap() = None;
                #[cfg(feature = "mii-cpu-adapter-probe")]
                ack::complete(ack_phase(), serde_json::json!({"owner":owner,"kind":track.kind,
                    "entry":ack::hex(entry as usize),"ticks":track.ticks,"saw_reload":track.saw_reload,
                    "departed":track.departed,"body_id":body_id,
                    "fe46":fe46,"d72":d72,"d94":d94}), finished, INSTALL.active());
                return true;
            }
            if track.ticks == 600 {
                #[cfg(feature = "mii-cpu-adapter-probe")]
                ack::terminal(ack::STALLED, ack_phase(), "native_reload_stalled",
                    serde_json::json!({"entry":ack::hex(entry as usize),"ticks":track.ticks,
                        "native_phase":phase,"departed":track.departed,"install_active":INSTALL.active()}));
                trace!(
                    "[MiiReloadProbe] stalled owner={} phase=0x{:x} departed={}; token retained until native completion or destruction",
                    owner, phase, track.departed
                );
            }
            // Preserve a departure recorded by the scene callback while this
            // service tick observed the native loader.
            if let Some(current) = TRACK.lock().unwrap().as_mut() {
                if current.entry == track.entry && current.token == track.token {
                    current.ticks = track.ticks;
                    current.phase = track.phase;
                    current.saw_reload = track.saw_reload;
                }
            }
            return true;
        }
        let target_role = if TARGET_OWNER == 0 { Role::Player } else { Role::Cpu };
        if !REQUESTED.load(Ordering::Relaxed) || owner != TARGET_OWNER || role != target_role {
            return false;
        }
        // Native Training only; the waiting room and LDN scopes are out of this probe's scope.
        if scope(role).is_none_or(|s| s.waiting()) || !permitted() {
            return false;
        }
        let Some(kind) = ready_single_mii(entry, owner) else {
            return false;
        };
        {
            let state = state_for(role).lock().unwrap();
            if state.active.is_some() || state.pending.is_some() {
                return false;
            }
        }
        let Some(token) = INSTALL.try_begin(true) else {
            return false;
        };
        REQUESTED.store(false, Ordering::Relaxed);
        if CHANGE {
            // Writes exactly u8 E+0x1b2 and u32 D+0x390 (shot 1: (n+1)%3; shot 2: originals) after the
            // table-based binding verified the row. Nothing else is written. Failure releases the
            // token immediately because no install was issued.
            if !change_step(entry) {
                finish_install(token, "special change probe refused (binding or shot state)");
                return false;
            }
        }
        // Full configuration: shot 0 applies the alternate saved configuration, shot 1 restores the
        // captured one. Both run before the descriptor copy below so the copy carries the written
        // tokens. A refusal writes nothing and releases the token (no install was issued).
        #[cfg(feature = "mii-fullconfig-probe")]
        if !crate::mii_fullconfig_probe::step(entry, SHOTS.load(Ordering::Relaxed), _service_entry_lr) {
            #[cfg(not(feature = "mii-cpu-adapter-probe"))]
            finish_install(token, "full configuration probe refused (see [MiiFullProbe] lines)");
            #[cfg(feature = "mii-cpu-adapter-probe")]
            {
                let finished = finish_install(token, "full configuration probe refused (see [MiiFullProbe] lines)");
                ack::terminal(ack::REFUSED, (SHOTS.load(Ordering::Relaxed) + 1) as u8, "step_refused", serde_json::json!({
                    "reason":ack::take_refusal(),"entry":ack::hex(entry as usize),"writes_committed":false,
                    "committed_phase":ack_phase(),
                    "install_finished":finished,"install_active_after":INSTALL.active()}));
            }
            return false;
        }
        let shots = SHOTS.fetch_add(1, Ordering::Relaxed) + 1;
        if shots >= MAX_SHOTS {
            FIRED.store(true, Ordering::Relaxed);
        }
        // Identical descriptor: the live 0x70-byte descriptor is copied without any change.
        let mut descriptor = [0u64; 14];
        std::ptr::copy_nonoverlapping(entry as *const u8, descriptor.as_mut_ptr().cast::<u8>(), 0x70);
        let (d72, d94, fe46) = info_block(entry);
        trace!(
            "[MiiReloadProbe] install owner={} entry=0x{:x} kind=0x{:x} count={} fe46=0x{:x} d72=0x{:x} d94={} body_id=0x{:x}",
            owner, entry as usize, kind, read::<u32>(entry, 0x14), fe46, d72, d94,
            read::<u32>(read::<*const u8>(entry, 0x4160), 8)
        );
        *TRACK.lock().unwrap() = Some(Track {
            token,
            entry: entry as usize,
            kind,
            ticks: 0,
            phase: u32::MAX,
            saw_reload: false,
            departed: false,
        });
        training::clear_session_state();
        let install: unsafe extern "C" fn(*mut u8, *const u64, *const u8) =
            std::mem::transmute(BASE.load(Ordering::Acquire) + 0x653240);
        install(entry, descriptor.as_ptr(), [1u8, 0, 0].as_ptr());
        true
    }

    unsafe fn row_specials(e: usize) -> [u8; 4] {
        let e = e as *const u8;
        [read(e, 0x1b2), read(e, 0x1b3), read(e, 0x1b4), read(e, 0x1b5)]
    }

    /// Shot 1 writes the neutral special (`E+0x1b2`, `D+0x390`) to `(n+1)%3` when all four bound
    /// specials are within 0..=2. Shot 2 restores both on the same live entry, only while the row
    /// still holds the shot-1 tuple. Both shots require a fresh table binding of the live callback
    /// argument; any mismatch writes nothing.
    unsafe fn change_step(entry: *mut u8) -> bool {
        let Some(bound) = crate::mii_identity_probe::bind_saved_entry(entry) else {
            trace!("[MiiSpecialProbe] refused: live entry could not be bound to a consistent saved row");
            return false;
        };
        let d = read::<*const u8>(entry, 0xf8);
        if !aligned(d) {
            return false;
        }
        let d = d as usize;
        let fe = entry as usize;
        // The binding guarantees `D+0x390..` equals these values.
        let current = row_specials(bound.e);
        let mut original = ORIGINAL.lock().unwrap();
        match (SHOTS.load(Ordering::Relaxed), *original) {
            (0, None) if current.iter().all(|&n| n <= 2) => {
                let written = (current[0] + 1) % 3;
                std::ptr::write_volatile((bound.e + 0x1b2) as *mut u8, written);
                std::ptr::write_volatile((d + 0x390) as *mut u32, u32::from(written));
                *original = Some(ChangeState { fe, id: bound.id, e: bound.e, original: current, written, changed: true });
                trace!("[MiiSpecialProbe] shot1 id={} fe=0x{:x} e=0x{:x} kind=0x{:x} neutral {} -> {} (E+0x1b2, D+0x390 written; identical reload follows)",
                    bound.id, fe, bound.e, bound.kind, current[0], written);
                true
            }
            (1, Some(o)) if o.changed && o.fe == fe && o.id == bound.id && o.e == bound.e
                && current == o.changed_tuple() =>
            {
                std::ptr::write_volatile((bound.e + 0x1b2) as *mut u8, o.original[0]);
                std::ptr::write_volatile((d + 0x390) as *mut u32, u32::from(o.original[0]));
                *original = Some(ChangeState { changed: false, ..o });
                trace!("[MiiSpecialProbe] shot2 id={} fe=0x{:x} restored neutral {} (E+0x1b2, D+0x390; identical reload follows)",
                    bound.id, fe, o.original[0]);
                true
            }
            (shot, state) => {
                trace!("[MiiSpecialProbe] refused: shot={} state={:?} bound=({}, 0x{:x}) fe=0x{:x} current={:?}",
                    shot, state, bound.id, bound.e, fe, current);
                false
            }
        }
    }

    /// The exact shot-1 entry is being destroyed: end the experiment and consume its record. A row
    /// still holding the shot-1 tuple is restored only through a fresh table binding of this live
    /// argument; otherwise nothing is written. `D` belongs to the dying entry and is never written.
    unsafe fn end_change_on_destroy(entry: *const u8) {
        let Some(o) = ORIGINAL.lock().unwrap().take_if(|o| o.fe == entry as usize) else {
            return;
        };
        FIRED.store(true, Ordering::Relaxed);
        if !o.changed {
            trace!("[MiiSpecialProbe] ended on entry destruction id={} fe=0x{:x}; row already restored", o.id, o.fe);
            return;
        }
        match crate::mii_identity_probe::bind_saved_entry(entry) {
            Some(b) if b.id == o.id && b.e == o.e && row_specials(b.e) == o.changed_tuple() => {
                std::ptr::write_volatile((o.e + 0x1b2) as *mut u8, o.original[0]);
                trace!("[MiiSpecialProbe] restored E+0x1b2={} on entry destruction id={} fe=0x{:x}",
                    o.original[0], o.id, o.fe);
            }
            bound => trace!(
                "[MiiSpecialProbe] restore refused on entry destruction id={} fe=0x{:x} bound={:?}: saved-row ownership not re-established; E+0x1b2 not written (expected {})",
                o.id, o.fe, bound.map(|b| (b.id, b.e)), o.written
            ),
        }
    }

    /// Native destruction ends the install like the roster path: the token is finished here,
    /// never on a tick count.
    pub unsafe fn on_destroy(entry: *const u8) {
        if CHANGE {
            end_change_on_destroy(entry);
        }
        #[cfg(feature = "mii-fullconfig-probe")]
        crate::mii_fullconfig_probe::on_destroy(entry);
        #[cfg(not(feature = "mii-cpu-adapter-probe"))]
        {
        let mut track = TRACK.lock().unwrap();
        if track.is_some_and(|t| t.entry == entry as usize) {
            let t = track.take().unwrap();
            finish_install(t.token, "identical reload probe entry destroyed");
            trace!(
                "[MiiReloadProbe] entry destroyed entry=0x{:x} ticks={} departed={} saw_reload={}",
                entry as usize, t.ticks, t.departed, t.saw_reload
            );
        }
        }
        #[cfg(feature = "mii-cpu-adapter-probe")]
        {
            let taken = {
                let mut track = TRACK.lock().unwrap();
                track.take_if(|t| t.entry == entry as usize)
            };
            if let Some(t) = taken {
                let finished = finish_install(t.token, "identical reload probe entry destroyed");
                trace!("[MiiReloadProbe] entry destroyed entry=0x{:x} ticks={} departed={} saw_reload={}",
                    entry as usize, t.ticks, t.departed, t.saw_reload);
                ack::terminal(ack::DESTROYED, ack_phase(), "entry_destroyed",
                    serde_json::json!({"entry":ack::hex(entry as usize),"ticks":t.ticks,
                        "departed":t.departed,"saw_reload":t.saw_reload,"install_finished":finished,
                        "install_active_after":INSTALL.active(),"track_cleared":true}));
            } else if ack::observed_entry() == entry as usize {
                ack::terminal(ack::DESTROYED, ack_phase(), "entry_destroyed_no_pending_install",
                    serde_json::json!({"entry":ack::hex(entry as usize),"install_active":INSTALL.active(),
                        "cleanup_writes":false}));
            }
        }
    }

    pub fn scene_changed() {
        REQUESTED.store(false, Ordering::Relaxed);
        // No live entry here: end the experiment, but never write the static row from this callback.
        if CHANGE {
            if let Some(o) = *ORIGINAL.lock().unwrap() {
                FIRED.store(true, Ordering::Relaxed);
                trace!("[MiiSpecialProbe] ended on scene departure id={} changed={}; restoration left to entry 0x{:x} destruction",
                    o.id, o.changed, o.fe);
            }
        }
        #[cfg(feature = "mii-fullconfig-probe")]
        crate::mii_fullconfig_probe::scene_changed();
        if let Some(t) = TRACK.lock().unwrap().as_mut() {
            t.departed = true;
            trace!("[MiiReloadProbe] scene departure during reload; native teardown owns cancellation");
        }
        #[cfg(feature = "mii-cpu-adapter-probe")]
        if ack::observed_entry() != 0 {
            let tracked = *TRACK.lock().unwrap();
            ack::terminal(ack::DEPARTED, ack_phase(), "scene_departure", serde_json::json!({
                "observed_entry":ack::hex(ack::observed_entry()),
                "pending_entry":tracked.map(|t| ack::hex(t.entry)),
                "install_active":INSTALL.active(),"cleanup_writes":false,
                "entry_destroyed":false}));
        }
    }
}
