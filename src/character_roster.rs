//! Reload a practice player or CPU through the game's asynchronous entry/resource loader.
//! Requests contain values and entry identities, never retained game pointers.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;

use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use training_mod_consts::{normalize_rosters, OnOff, TrainingModpackMenu, TrioFighter, MENU};
use training_mod_sync::{lock_write, read as setting};

use crate::common::{
    button_config, game_mode,
    input::{Buttons, MappedInputs},
    menu,
};
use crate::training::{self, lobby};

macro_rules! trace {
    ($($arg:tt)*) => { crate::logging::diagnostic(format_args!($($arg)*)) };
}

static BASE: AtomicUsize = AtomicUsize::new(0);
static BUSY: AtomicBool = AtomicBool::new(false);
static OWNED_BUTTONS: AtomicU32 = AtomicU32::new(0);
static STATES: [Mutex<State>; 2] = [Mutex::new(State::empty()), Mutex::new(State::empty())];

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

#[derive(Clone, Copy, Debug)]
enum Selection {
    Direct(TrioFighter),
    Cycle,
}

#[derive(Clone, Copy, Debug)]
struct Request {
    scope: Scope,
    epoch: usize,
    entry: usize,
    selection: Selection,
    age: usize,
}

#[derive(Clone, Copy, Debug)]
struct Active {
    scope: Scope,
    epoch: usize,
    revision: usize,
    entry: usize,
    target: TrioFighter,
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
    BUSY.load(Ordering::Acquire)
}

fn roster(m: &TrainingModpackMenu, role: Role) -> Vec<TrioFighter> {
    let mut normalized = *m;
    normalize_rosters(&mut normalized);
    match role {
        Role::Player => normalized.player_roster,
        Role::Cpu => normalized.cpu_roster,
    }
    .expect("Roster normalization always produces a supported nonempty roster")
    .fighters()
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
pub fn settings_changed(previous: &TrainingModpackMenu) {
    let current = setting(&MENU);
    for role in [Role::Player, Role::Cpu] {
        if roster(previous, role) != roster(&current, role) {
            let mut state = state_for(role).lock().unwrap();
            state.revision = state.revision.wrapping_add(1);
            state.cursor = None;
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
        if role.selected(previous) != role.selected(&current) {
            queue(role, Selection::Direct(role.selected(&current)));
        }
    }
}

fn queue(role: Role, selection: Selection) {
    if !role.enabled() {
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
        trace!("[CharacterRoster] request rejected: another operation owns practice");
        notify(role, "Finish the current practice action first".into());
        return;
    }
    let mut state = state_for(role).lock().unwrap();
    let Some((entry, _)) = state.observed else {
        drop(state);
        trace!("[CharacterRoster] request rejected: no current practice entry");
        notify(
            role,
            "Wait for the practice character to finish loading".into(),
        );
        return;
    };
    state.pending = Some(Request {
        scope,
        epoch: state.epoch,
        entry,
        selection,
        age: 0,
    });
    trace!(
        "[CharacterRoster] queued owner={} entry=0x{:x} epoch={} selection={:?}",
        scope.owner,
        entry,
        state.epoch,
        selection
    );
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
pub fn scene_changed() {
    OWNED_BUTTONS.store(0, Ordering::Relaxed);
    for role in [Role::Player, Role::Cpu] {
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

fn aligned(ptr: *const u8) -> bool {
    !ptr.is_null() && (ptr as usize).is_multiple_of(8)
}

unsafe fn read<T: Copy>(entry: *const u8, offset: usize) -> T {
    std::ptr::read_volatile(entry.add(offset).cast::<T>())
}

unsafe fn active_body(entry: *const u8, owner: u32, kind: u32) -> bool {
    if !aligned(entry)
        || read::<u32>(entry, 0xe0) != owner
        || read::<u32>(entry, 0x14) != 1
        || read::<u32>(entry, 0x28) != 1
        || read::<u32>(entry, 0x18) != kind
        || read::<u32>(entry, 0x1c) != u32::MAX
        || read::<u32>(entry, 0x20) != u32::MAX
        || read::<u64>(entry, 0x4150) != 1
        || read::<u8>(entry, 0x5918) != 0
        || read::<u8>(entry, 0x5920) != 6
        || read::<u8>(entry, 0x591c) != 14
        || read::<u8>(entry, 0x5935) != 0
        || read::<u8>(entry, 0x5934) != 0
    {
        return false;
    }
    let body = read::<*const u8>(entry, 0x4160);
    aligned(body)
        && read::<usize>(body, 0) == BASE.load(Ordering::Acquire) + 0x4f80210
        && read::<u32>(body, 0xc) == kind
        && read::<u64>(body, 0x10) == u64::from(owner)
        && read::<u8>(body, 0x3a) == 4
        && aligned(read::<*const u8>(body, 0x20))
}

// This current native callback owns entry lifetime. Cached numeric identities
// below are compared only; no saved fighter/entry/module pointer is dereferenced.
#[skyline::hook(offset = 0x650010, inline)]
unsafe fn service(ctx: &mut InlineCtx) {
    let entry = ctx.registers[0].x() as *mut u8;
    if !aligned(entry) {
        return;
    }
    let owner = read::<u32>(entry, 0xe0);
    if !matches!(owner, 0 | 1 | 4) {
        return;
    }
    let role = if owner == 0 { Role::Player } else { Role::Cpu };
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
            && active_body(entry, owner, active.target.kind())
        {
            let mut state = state_for(role).lock().unwrap();
            if state.epoch != active.epoch {
                return;
            }
            state.active = None;
            state.loaded = Some((entry as usize, owner, active.target.kind()));
            state.observed = Some((entry as usize, active.target.kind()));
            if state.revision == active.revision {
                state.cursor = active.slot;
            }
            drop(state);
            role.sync_selection(active.target);
            refresh_hud(entry, owner, active.target.kind());
            BUSY.store(false, Ordering::Release);
            trace!("[CharacterRoster] complete owner={} target={} kind=0x{:x} slot={:?} ticks={} body_id=0x{:x}",
                owner, active.target.name(), active.target.kind(), active.slot.map(|i| i+1),
                active.ticks, read::<u32>(read::<*const u8>(entry, 0x4160), 8));
            notify(
                role,
                match active.slot {
                    Some(index) => format!("{}. {}", index + 1, active.target.name()),
                    None => active.target.name().into(),
                },
            );
        } else {
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
    if busy() {
        return;
    } // Serialize player and CPU resource mutations.
    let request = state_for(role).lock().unwrap().pending;
    let Some(mut request) = request else {
        // Display the live fighter in the picker, including the initial CSS choice.
        // This permits choosing the same menu fighter again after a cycle.
        if scope(role).is_some_and(|s| s.owner == owner) && !menu::captures_input() {
            let kind = read::<u32>(entry, 0x18);
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
                if state.observed != Some((entry as usize, kind)) && active_body(entry, owner, kind)
                {
                    state.observed = Some((entry as usize, kind));
                    trace!(
                        "[CharacterRoster] observed owner={} entry=0x{:x} kind=0x{:x} epoch={}",
                        owner,
                        entry as usize,
                        kind,
                        state.epoch
                    );
                    drop(state);
                    role.sync_selection(fighter);
                }
            }
        }
        return;
    };
    if request.scope.owner != owner || request.entry != entry as usize {
        return;
    }
    let mut state = state_for(role).lock().unwrap();
    if request.epoch != state.epoch || scope(role) != Some(request.scope) || !role.enabled() {
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
    let current = read::<u32>(entry, 0x18);
    if menu::captures_input()
        || lobby::native_ui_busy()
        || button_config::has_pending_request()
        || training::save_states::is_loading()
        || training::recording_or_playback()
        || !active_body(entry, owner, current)
        || read::<u8>(entry, 0x591a) != 0xff
        || read::<u8>(entry, 0x591f) != 0
    {
        drop(state);
        trace!(
            "[CharacterRoster] rejected at native boundary: fighter not idle or another action owns practice"
        );
        notify(role, "Character swap unavailable during this action".into());
        return;
    }
    let m = setting(&MENU);
    let choices = roster(&m, role);
    let count = choices.len();
    let (target, slot) = match request.selection {
        Selection::Direct(target) => (target, None),
        Selection::Cycle => {
            let next = state
                .cursor
                .filter(|i| *i < count && choices[*i].kind() == current)
                .or_else(|| choices[..count].iter().position(|f| f.kind() == current))
                .map_or(0, |i| (i + 1) % count);
            (choices[next], Some(next))
        }
    };
    if target.kind() == current {
        state.cursor = slot;
        drop(state);
        notify(
            role,
            format!(
                "{}{}",
                slot.map_or(String::new(), |i| format!("{}. ", i + 1)),
                target.name()
            ),
        );
        trace!(
            "[CharacterRoster] already active owner={} kind=0x{:x} slot={:?}",
            owner,
            current,
            slot.map(|i| i + 1)
        );
        return;
    }
    let active = Active {
        scope: request.scope,
        epoch: state.epoch,
        revision: state.revision,
        entry: entry as usize,
        target,
        slot,
        ticks: 0,
        phase: u32::MAX,
        departed: false,
    };
    if BUSY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        state.pending = Some(request);
        return;
    }
    state.loaded = None;
    state.active = Some(active);
    drop(state);
    // 653240 synchronously copies this descriptor. Preserve all native role,
    // controller/team/costume options; change only the supported single kind.
    let mut descriptor = [0u64; 14];
    std::ptr::copy_nonoverlapping(entry, descriptor.as_mut_ptr().cast::<u8>(), 0x70);
    std::ptr::write(
        descriptor.as_mut_ptr().cast::<u8>().add(0x18).cast::<u32>(),
        target.kind(),
    );
    training::clear_session_state();
    trace!("[CharacterRoster] install owner={} entry=0x{:x} from=0x{:x} to=0x{:x} slot={:?} epoch={} mode=0x{:x}",
        owner, entry as usize, current, target.kind(), slot.map(|i| i+1), active.epoch, active.scope.mode);
    let install: unsafe extern "C" fn(*mut u8, *const u64, *const u8) =
        std::mem::transmute(BASE.load(Ordering::Acquire) + 0x653240);
    install(entry, descriptor.as_ptr(), [1u8, 0, 0].as_ptr());
}

/// Called before native entry destruction, while that argument is still owned.
pub unsafe fn on_destroy(entry: *const u8) {
    if !aligned(entry) {
        return;
    }
    let owner = read::<u32>(entry, 0xe0);
    if !matches!(owner, 0 | 1 | 4) {
        return;
    }
    let role = if owner == 0 { Role::Player } else { Role::Cpu };
    let mut state = state_for(role).lock().unwrap();
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
    }
    if state.active.is_some_and(|a| a.entry == entry as usize) {
        trace!("[CharacterRoster] entry_destroy owner={} active={:?}; references invalidated before native destruction", owner, state.active);
        state.active = None;
        state.cursor = None;
        BUSY.store(false, Ordering::Release);
    }
}

/// Native reset already owns the HUD lookup. Admit our fresh, single fighter body
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
/// dispatch depth/deferred-removal protocol protects iterator lifetime.
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
    std::ptr::write_volatile(manager.add(0x18).cast::<u32>(), depth + 1);
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
                invoke(listener, &event);
            }
            delivered += 1;
        }
        node = read::<*const u8>(node, 0);
    }
    let remaining = read::<u32>(manager, 0x18) - 1;
    std::ptr::write_volatile(manager.add(0x18).cast::<u32>(), remaining);
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

unsafe fn waiting_icon(listener: *const u8, owner: u32, kind: u32, base: usize) -> bool {
    if owner != 4 {
        return false;
    }
    let hud = read::<*const u8>(listener, 0x10);
    let inner = read::<*const u8>(hud, 0x20);
    if !aligned(inner) || read::<usize>(inner, 0) != base + 0x5062f60 {
        trace!(
            "[CharacterRoster] waiting HUD rejected: inner=0x{:x}",
            inner as usize
        );
        return false;
    }
    let records = read::<*const u8>(inner, 0x18);
    if records as usize != base + 0x52c5730 {
        trace!(
            "[CharacterRoster] waiting HUD rejected: records=0x{:x}",
            records as usize
        );
        return false;
    }
    let slot = read::<i32>(records, owner as usize * 0x260 + 0xc);
    trace!(
        "[CharacterRoster] waiting HUD owner={} presentation_slot={}",
        owner,
        slot
    );
    if !(0..8).contains(&slot) {
        return false;
    }
    let Some(root) = pointer_chain(base as *const u8, &[0x52c3420, 8, 0]) else {
        return false;
    };
    let context = read::<*const u8>(root, 0xb98);
    if !aligned(context) {
        return false;
    }
    // Match the game's generation-checked waiting UI handle (136aef8).
    let handle = [
        base + 0x5063bc0,
        context as usize,
        read::<u32>(root, 0xba0) as usize,
    ];
    let valid: unsafe extern "C" fn(*const usize) -> bool = std::mem::transmute(base + 0x138b6c0);
    if !valid(handle.as_ptr()) {
        return false;
    }
    let Some(view) = pointer_chain(context, &[0]) else {
        return false;
    };
    let online = read::<u8>(view, 0xd3a) != 0;
    let Some(controller) = pointer_chain(view, &[if online { 0xa50 } else { 0x420 }]) else {
        return false;
    };
    let Some(control_state) = pointer_chain(controller, &[0]) else {
        return false;
    };
    if read::<u8>(control_state, 0x14) == 0 {
        return false;
    }
    // Resolve presentation ID -> visible partner index exactly as the native
    // damage setters (13cecb0 / 13d0f50); never assume CPU4 is pane zero.
    let (entries, indices) = if online {
        let Some(collection) = pointer_chain(controller, &[0x2f0]) else {
            return false;
        };
        if read::<u8>(collection, 0) == 0 {
            return false;
        }
        let begin = read::<*const i32>(collection, 0x18);
        let end = read::<usize>(collection, 0x20);
        let length = end.wrapping_sub(begin as usize);
        if !aligned(begin.cast()) || length > 12 || !length.is_multiple_of(4) {
            return false;
        }
        (
            read::<*const u8>(collection, 8),
            std::slice::from_raw_parts(begin, length / 4).to_vec(),
        )
    } else {
        (
            read::<*const u8>(controller, 0x148),
            (0..3)
                .map(|i| read::<i32>(controller, 0x150 + i * 4))
                .collect(),
        )
    };
    if !aligned(entries) {
        return false;
    }
    let Some(index) = indices.iter().position(|&i| {
        (0..8).contains(&i) && read::<i32>(entries, i as usize * 0x1d8 + 0x50) == slot
    }) else {
        return false;
    };
    let Some(ui) = pointer_chain(controller, &[if online { 8 } else { 0x130 }, 0x80]) else {
        return false;
    };
    let Some(partners) = pointer_chain(ui, &[if online { 0x20 } else { 0x220 }]) else {
        return false;
    };
    let Some(icon) = pointer_chain(partners, &[0x98 + index * 0x58]) else {
        return false;
    };
    let Some(cache) = pointer_chain(ui, &[0, 0x78]) else {
        return false;
    };
    let kind_hash: unsafe extern "C" fn(u32) -> u64 = std::mem::transmute(base + 0x138c9b0);
    let texture: unsafe extern "C" fn(*const u8, u64, u32, u32) -> *const u8 =
        std::mem::transmute(base + 0x323f330);
    let assign: unsafe extern "C" fn(*const u8, *const u8) = std::mem::transmute(base + 0x3778b80);
    let image = texture(cache, kind_hash(kind), 0, 0);
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
    let Some(loader) = pointer_chain(context, &[0x50, 0, 8, 0, 0xcd0, 0x28]) else {
        trace!("[CharacterRoster] waiting icon preload skipped: no native loader");
        return;
    };
    let base = BASE.load(Ordering::Acquire);
    let kind_hash: unsafe extern "C" fn(u32) -> u64 = std::mem::transmute(base + 0x138c9b0);
    let icon_hash: unsafe extern "C" fn(u64, u32, u32) -> u64 =
        std::mem::transmute(base + 0x3238a70);
    let request: unsafe extern "C" fn(*const u8, *const u64, i64, u32) =
        std::mem::transmute(base + 0x323c4e0);
    let mut count = 0;
    for fighter in TrioFighter::available() {
        let hash = icon_hash(kind_hash(fighter.kind()), 0, 0);
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

unsafe fn pointer_chain(mut object: *const u8, offsets: &[usize]) -> Option<*const u8> {
    for &offset in offsets {
        if !aligned(object) {
            return None;
        }
        object = read(object, offset);
    }
    aligned(object).then_some(object)
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
        skyline::install_hooks!(service);
        if cfg!(feature = "waiting-roster-reload") {
            skyline::install_hooks!(load_local_waiting_icons, load_online_waiting_icons);
        }
        trace!("[CharacterRoster] native async loader installed; ordinary P1/CPU1; waiting_enabled={}; one existing entry per role; no manual loader pumping", cfg!(feature = "waiting-roster-reload"));
    }
}
