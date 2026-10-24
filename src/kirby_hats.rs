//! Shortcut-driven copy abilities. No game pointer is retained between callbacks.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

use smash::app::{self, lua_bind::*};
use smash::lib::lua_const::*;
use training_mod_consts::MENU;
use training_mod_sync::read;

use crate::common::consts::FighterId;
use crate::common::input::{Buttons, MappedInputs};
use crate::common::{self, button_config, menu, try_get_module_accessor};
use crate::kirby_hat_cycle::{HatContinuation, HatCycle, HatSnapshot, ShortcutLatch};
use crate::training;
use crate::character_roster::sources::{self, Lease};
use crate::source_lifetime::Holder;

#[derive(Clone, Copy)]
struct Pending {
    body_id: u32,
    kind: u32,
    age: u16,
    started: bool,
    cleared: bool,
    // Bookkeeping only; native copy state is revalidated on every retry.
    leases_released: bool,
    restoring: bool,
    lease: Option<Lease>,
    #[cfg(feature = "source-capacity-probe")]
    prepare_seen: bool,
}

#[cfg(feature = "source-capacity-probe")]
fn observe_pending(role: usize, pending: Pending, stage: &str, reason: &str) {
    crate::logging::diagnostic(format_args!(
        "[KirbyHatPending] role={} body=0x{:x} kind=0x{:x} stage={} reason={} age={} started={} cleared={} restoring={} prepare_seen={}",
        role, pending.body_id, pending.kind, stage, reason, pending.age,
        pending.started, pending.cleared, pending.restoring, pending.prepare_seen));
}

struct State {
    cycle: HatCycle,
    pending: Option<Pending>,
    continuation: HatContinuation,
}

static STATES: [Mutex<State>; 2] = [
    Mutex::new(State {
        cycle: HatCycle::new(),
        pending: None,
        continuation: HatContinuation::new(),
    }),
    Mutex::new(State {
        cycle: HatCycle::new(),
        pending: None,
        continuation: HatContinuation::new(),
    }),
];
static SHORTCUT: Mutex<ShortcutLatch> = Mutex::new(ShortcutLatch::new());
static PENDING_ROLES: AtomicU8 = AtomicU8::new(0);

#[cfg(feature = "source-capacity-probe")]
pub(crate) fn capacity_probe_idle() -> bool {
    STATES.iter().all(|state| state.lock().unwrap().pending.is_none())
        && PENDING_ROLES.load(Ordering::Acquire) == 0
}

fn clear_pending(role: usize, state: &mut State) {
    if let Some(pending) = state.pending {
        if let Some(lease) = pending.lease {
            if pending.started { sources::retain_hat(lease, pending.body_id); }
            else { sources::release(lease); }
        }
    }
    sources::cancel(role);
    state.pending = None;
    PENDING_ROLES.fetch_and(!(1 << role), Ordering::Release);
}

fn notice(role: usize, text: String) {
    training::ui::notifications::notification(
        if role == 0 {
            "Kirby Hats"
        } else {
            "CPU Kirby Hats"
        }
        .into(),
        text,
        120,
    );
}

unsafe fn permitted() -> bool {
    common::is_training_mode()
        && common::is_ready_go()
        && !crate::character_roster::busy()
        && !menu::captures_input()
        && !training::lobby::native_ui_busy()
        && !button_config::has_pending_request()
        && !training::save_states::is_loading()
        && !training::recording_or_playback()
        && !crate::replay_preview::occupied()
}

unsafe fn role_body(role: usize) -> Option<*mut app::BattleObjectModuleAccessor> {
    let body = try_get_module_accessor(if role == 0 {
        FighterId::Player
    } else {
        FighterId::CPU
    })?;
    (app::utility::get_kind(&mut *body) == *FIGHTER_KIND_KIRBY).then_some(body)
}

unsafe fn equipped(body: *mut app::BattleObjectModuleAccessor) -> Option<u32> {
    WorkModule::is_flag(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_FLAG_COPY)
        .then(|| WorkModule::get_int(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_INT_COPY_CHARA) as u32)
}

/// Called only for P1, before the ordinary input-delay queue.
pub unsafe fn handle_input(out: &mut MappedInputs, left_click: bool, right_click: bool) {
    let special = out
        .buttons
        .intersects(Buttons::SPECIAL | Buttons::SPECIAL_RAW | Buttons::SPECIAL_RAW2);
    let neutral = out.lstick_x.unsigned_abs() < 20 && out.lstick_y.unsigned_abs() < 20;
    let mut shortcut = SHORTCUT.lock().unwrap();
    let mut bodies = [None; 2];
    if shortcut.would_request(special, neutral, left_click, right_click) && permitted() {
        let role = usize::from(right_click);
        bodies[role] = role_body(role);
    }
    let (request, suppress) = shortcut.sample(
        special,
        neutral,
        left_click,
        right_click,
        bodies.map(|body| body.is_some()),
    );
    drop(shortcut);
    if suppress {
        out.buttons
            .remove(Buttons::SPECIAL | Buttons::SPECIAL_RAW | Buttons::SPECIAL_RAW2);
    }
    let Some(role) = request else {
        return;
    };
    let body = bodies[role].expect("eligible Kirby body");
    #[cfg(feature = "source-owner-probe")]
    if !cfg!(feature = "managed-sources")
        && crate::character_roster::source_owner_probe::request_replacement(body) {
        return;
    }
    let settings = read(&MENU);
    let choices = settings.ordered_kirby_hat_kinds(if role == 0 {
        training_mod_tui::roster_grid::Role::Player
    } else {
        training_mod_tui::roster_grid::Role::Cpu
    });
    let mut state = STATES[role].lock().unwrap();
    if state.pending.is_some() {
        return;
    }
    let Some(kind) = state.cycle.target(&choices, equipped(body)) else {
        drop(state);
        notice(role, "Select a Kirby hat first".into());
        return;
    };
    state.pending = Some(Pending {
        body_id: (*body).battle_object_id,
        kind,
        age: 0,
        started: false,
        cleared: false,
        leases_released: false,
        restoring: false,
        lease: None,
        #[cfg(feature = "source-capacity-probe")]
        prepare_seen: false,
    });
    PENDING_ROLES.fetch_or(1 << role, Ordering::Release);
    #[cfg(feature = "source-capacity-probe")]
    {
        let pending = state.pending.expect("created pending request");
        drop(state);
        observe_pending(role, pending, "created", "shortcut");
    }
}

/// Service from Kirby's native frame callback while that body is owned by the game.
pub unsafe fn on_frame(body: *mut app::BattleObjectModuleAccessor) {
    let waiting = training::lobby::active() && training::lobby::is_waiting_scene();
    if (PENDING_ROLES.load(Ordering::Acquire) == 0 && !waiting)
        || body.is_null() || !common::is_training_mode() {
        return;
    }
    let role = if role_body(0) == Some(body) {
        0
    } else if role_body(1) == Some(body) {
        1
    } else {
        return;
    };
    // Resolve roster ownership before the hat mutex: roster completion can clear
    // this role's memory. Never restore onto the temporary character-select body.
    let settled = waiting && common::is_ready_go()
        && crate::character_roster::waiting_choice_settled(role);
    let mut state = STATES[role].lock().unwrap();
    if state.pending.is_none() && settled {
        if state.continuation.pending() {
            if !permitted() { return; }
            let snapshot = state.continuation.take_restore().expect("pending continuation");
            state.cycle = snapshot.cycle;
            if let Some(kind) = snapshot.equipped {
                state.pending = Some(Pending {
                    body_id: (*body).battle_object_id, kind,
                    age: 0, started: false, cleared: false, leases_released: false,
                    restoring: false, lease: None,
                    #[cfg(feature = "source-capacity-probe")]
                    prepare_seen: false,
                });
                PENDING_ROLES.fetch_or(1 << role, Ordering::Release);
                crate::logging::diagnostic(format_args!(
                    "[KirbyHats] lobby continuation role={} kind=0x{:x} body=0x{:x}",
                    role, kind, (*body).battle_object_id));
            }
        } else if !WorkModule::is_flag(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_FLAG_COPY_ON_START) {
            let snapshot = HatSnapshot { cycle: state.cycle, equipped: equipped(body) };
            state.continuation.observe(snapshot);
        }
    }
    let Some(mut pending) = state.pending else {
        return;
    };
    if pending.restoring && pending.body_id == (*body).battle_object_id
        && training::save_states::is_loading() { return; }
    let identity_changed = pending.body_id != (*body).battle_object_id;
    if identity_changed || !permitted() {
        clear_pending(role, &mut state);
        #[cfg(feature = "source-capacity-probe")]
        {
            drop(state);
            observe_pending(role, pending, "cancelled",
                if identity_changed { "body identity changed" } else { "permission gate" });
        }
        return;
    }
    pending.age = pending.age.saturating_add(1);
    if pending.started
        && equipped(body) == Some(pending.kind)
        && !WorkModule::is_flag(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_FLAG_COPY_ON_START)
    {
        state.cycle.commit(pending.kind);
        if settled {
            let snapshot = HatSnapshot { cycle: state.cycle, equipped: Some(pending.kind) };
            state.continuation.observe(snapshot);
        }
        if let Some(lease) = pending.lease.take() {
            sources::retain_hat(lease, pending.body_id);
        }
        state.pending = Some(pending);
        clear_pending(role, &mut state);
        drop(state);
        crate::logging::diagnostic(format_args!(
            "[KirbyHats] confirmed role={} kind=0x{:x} body=0x{:x}",
            role, pending.kind, pending.body_id
        ));
        let name = training_mod_consts::TrioFighter::from_kind(pending.kind)
            .map(|f| f.name())
            .unwrap_or("Kirby hat");
        notice(role, name.into());
        return;
    }
    if pending.age > 900 {
        clear_pending(role, &mut state);
        drop(state);
        #[cfg(feature = "source-capacity-probe")]
        {
            observe_pending(role, pending, "timeout", "age exceeded 900");
            crate::logging::diagnostic(format_args!(
                "[KirbyHatPending] timeout_native role={} body=0x{:x} status=0x{:x} changing={} stop={}",
                role, pending.body_id, StatusModule::status_kind(body),
                StatusModule::is_changing(body), StopModule::is_stop(body)));
        }
        notice(role, "Kirby hat could not be loaded".into());
        return;
    }
    if !pending.started
        && !StopModule::is_stop(body)
        && !StatusModule::is_changing(body)
        && matches!(StatusModule::status_kind(body), kind if kind == *FIGHTER_STATUS_KIND_WAIT || kind == *FIGHTER_STATUS_KIND_FALL)
    {
        if cfg!(feature = "managed-sources") && !pending.cleared {
            pending.cleared = true;
            state.pending = Some(pending);
            drop(state);
            // No state mutex is held across native cleanup or its listeners.
            if let Err(reason) = sources::clear_hat_for_switch(body) {
                clear_pending(role, &mut STATES[role].lock().unwrap());
                #[cfg(feature = "source-capacity-probe")]
                observe_pending(role, pending, "error", reason);
                notice(role, reason.into());
            }
            return;
        }
        let slot = if cfg!(feature = "managed-sources") {
            if let Err(reason) = sources::finish_hat_clear(body, pending.leases_released) {
                clear_pending(role, &mut state);
                drop(state);
                #[cfg(feature = "source-capacity-probe")]
                observe_pending(role, pending, "error", reason);
                notice(role, reason.into());
                return;
            }
            pending.leases_released = true;
            #[cfg(feature = "source-capacity-probe")]
            if !pending.prepare_seen {
                pending.prepare_seen = true;
                observe_pending(role, pending, "prepare", "first sources::prepare call");
            }
            match sources::prepare(role, pending.kind,
                Holder { category: 1, id: u64::from(pending.body_id) }) {
                Ok(Some(lease)) => {
                    pending.lease = Some(lease);
                    Some(lease.slot as i32)
                }
                Ok(None) => { state.pending = Some(pending); return; }
                Err(reason) => {
                    clear_pending(role, &mut state);
                    drop(state);
                    #[cfg(feature = "source-capacity-probe")]
                    observe_pending(role, pending, "error", reason);
                    notice(role, reason.into());
                    return;
                }
            }
        } else {
        let source = IntoIterator::into_iter([FighterId::Player, FighterId::CPU])
            .filter_map(try_get_module_accessor)
            .find(|source| app::utility::get_kind(&mut **source) as u32 == pending.kind);
        let slot = source.map(|source|
            WorkModule::get_int(source, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID));
        #[cfg(feature = "source-owner-probe")]
        let slot = slot.or_else(|| crate::character_roster::source_owner_probe::source_slot(pending.kind));
        slot
        };
        let Some(slot) = slot else {
            clear_pending(role, &mut state);
            drop(state);
            notice(role, "This hat's resources are not loaded".into());
            return;
        };
        pending.started = true;
        state.pending = Some(pending);
        drop(state);
        crate::logging::diagnostic(format_args!(
            "[KirbyHats] request role={} kind=0x{:x} source_slot={} body=0x{:x}",
            role, pending.kind, slot, pending.body_id
        ));
        training::charge::setup_copy_ability(body, slot, pending.kind as i32);
        return;
    }
    state.pending = Some(pending);
}

pub fn cancel_pending() {
    for (role, state) in STATES.iter().enumerate() {
        let mut state = state.lock().unwrap();
        #[cfg(feature = "source-capacity-probe")]
        let pending = state.pending;
        clear_pending(role, &mut state);
        drop(state);
        #[cfg(feature = "source-capacity-probe")]
        if let Some(pending) = pending {
            observe_pending(role, pending, "cancelled", "cancel_pending");
        }
    }
}

fn invalidate_runtime(same_lobby: Option<bool>) {
    for (role, state) in STATES.iter().enumerate() {
        let mut state = state.lock().unwrap();
        #[cfg(feature = "source-capacity-probe")]
        let pending = state.pending;
        state.pending = None;
        if let Some(same_lobby) = same_lobby {
            state.cycle = HatCycle::new();
            state.continuation.scene_changed(same_lobby);
        }
        drop(state);
        #[cfg(feature = "source-capacity-probe")]
        if let Some(pending) = pending {
            observe_pending(role, pending, "cancelled", "scene invalidated");
        }
        #[cfg(not(feature = "source-capacity-probe"))]
        let _ = role;
    }
    *SHORTCUT.lock().unwrap() = ShortcutLatch::default();
    PENDING_ROLES.store(0, Ordering::Release);
}

/// Uses the roster's already classified scene boundary. Runtime leases/pointers
/// are always discarded, even when value-only hat intent survives a match.
pub fn scene_changed(same_lobby: bool) {
    invalidate_runtime(Some(same_lobby));
}

/// Entry destruction precedes knowing whether this is a match or a room exit.
pub fn practice_departed(waiting: bool) {
    invalidate_runtime(if waiting { None } else { Some(false) });
}

pub fn fresh_lobby() { invalidate_runtime(Some(false)); }

/// An accepted swap away from Kirby must not restore an obsolete hat later.
pub fn fighter_committed(role: usize, kind: u32) {
    if kind == 6 { return; }
    let mut state = STATES[role].lock().unwrap();
    state.continuation.clear();
    state.cycle = HatCycle::new();
}

/// Restore uses the same asynchronous source acquisition as the shortcut. The
/// native save loader may finish first; an opponent slot is never hard-coded.
pub unsafe fn restore(body: *mut app::BattleObjectModuleAccessor, kind: u32) {
    let role = if role_body(0) == Some(body) { 0 }
        else if role_body(1) == Some(body) { 1 } else { return; };
    let mut state = STATES[role].lock().unwrap();
    #[cfg(feature = "source-capacity-probe")]
    let superseded = state.pending;
    clear_pending(role, &mut state);
    state.pending = Some(Pending { body_id: (*body).battle_object_id, kind,
        age: 0, started: false, cleared: false, leases_released: false,
        restoring: true, lease: None,
        #[cfg(feature = "source-capacity-probe")]
        prepare_seen: false,
    });
    PENDING_ROLES.fetch_or(1 << role, Ordering::Release);
    #[cfg(feature = "source-capacity-probe")]
    {
        let pending = state.pending.expect("created restore request");
        drop(state);
        if let Some(old) = superseded {
            observe_pending(role, old, "cancelled", "superseded by restore");
        }
        observe_pending(role, pending, "created", "save restore");
    }
}
