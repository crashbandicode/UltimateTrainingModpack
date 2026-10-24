//! Opt-in direct item creation/identity experiment. L3+X requests the next case.
//! No item/accessor pointer survives a native callback; no save files are written.
//! gyro-variation-probe narrows the sequence to eight Gyro variants with a
//! native R.O.B. present, for separate real save/restore acceptance.
use std::convert::TryFrom;
use std::sync::Mutex;

use smash::app::{self, lua_bind::*, sv_battle_object};
use smash::lib::lua_const::*;

use crate::common::{self, consts::FighterId, input::MappedInputs, menu, try_get_module_accessor};
use crate::training::character_specific::items::TURNIP_CHOSEN;

extern "C" {
    #[link_name = "\u{1}_ZN3app11peachdaikon32PEACH_PEACHDAIKON_DAIKON_1_POWEREv"]
    fn peach_smile_power() -> f32;
    #[link_name = "\u{1}_ZN3app11peachdaikon32PEACH_PEACHDAIKON_DAIKON_6_POWEREv"]
    fn peach_winky_power() -> f32;
    #[link_name = "\u{1}_ZN3app11peachdaikon32PEACH_PEACHDAIKON_DAIKON_7_POWEREv"]
    fn peach_dot_eyes_power() -> f32;
    #[link_name = "\u{1}_ZN3app11peachdaikon32PEACH_PEACHDAIKON_DAIKON_8_POWEREv"]
    fn peach_stitch_power() -> f32;
    #[link_name = "\u{1}_ZN3app11daisydaikon32DAISY_DAISYDAIKON_DAIKON_8_POWEREv"]
    fn daisy_stitch_power() -> f32;
}

#[derive(Default)]
struct Probe {
    held: bool,
    next: usize,
    pending: Option<(u32, usize)>,
    observation: Option<(u32, usize, u8)>,
}
static STATE: Mutex<Probe> = Mutex::new(Probe {
    held: false,
    next: 0,
    pending: None,
    observation: None,
});

const CASE_COUNT: usize = if cfg!(feature = "gyro-variation-probe") { 8 } else { 10 };

/// The separate empty-entry experiment must not overlap a queued item probe.
#[cfg(feature = "source-capacity-probe")]
pub fn capacity_probe_idle() -> bool {
    let state = STATE.lock().unwrap();
    state.pending.is_none() && state.observation.is_none()
}

fn case(index: usize) -> (&'static str, i32, i32) {
    if cfg!(feature = "gyro-variation-probe") {
        const NAMES: [&str; 8] = ["Gyro 0", "Gyro 1", "Gyro 2", "Gyro 3",
            "Gyro 4", "Gyro 5", "Gyro 6", "Gyro 7"];
        let variation = index % 8;
        return (NAMES[variation], *ITEM_KIND_ROBOTGYRO, variation as i32);
    }
    match index % 10 {
        0 => ("Bob-omb", *ITEM_KIND_BOMBHEI, *ITEM_VARIATION_BOMBHEI_NORMAL),
        1 => ("Peach Smile", *ITEM_KIND_PEACHDAIKON, *ITEM_VARIATION_PEACHDAIKON_1),
        2 => ("Peach Stitch", *ITEM_KIND_PEACHDAIKON, *ITEM_VARIATION_PEACHDAIKON_8),
        3 => ("Daisy Stitch", *ITEM_KIND_DAISYDAIKON, *ITEM_VARIATION_DAISYDAIKON_8),
        4 => ("Banana", *ITEM_KIND_BANANA, 0),
        5 => ("Remote Bomb", *ITEM_KIND_LINKBOMB, 0),
        6 => ("Gyro", *ITEM_KIND_ROBOTGYRO, 0),
        7 => ("Peach Winky", *ITEM_KIND_PEACHDAIKON, *ITEM_VARIATION_PEACHDAIKON_6),
        8 => ("Peach Dot-Eyes", *ITEM_KIND_PEACHDAIKON, *ITEM_VARIATION_PEACHDAIKON_7),
        _ => ("Remote Bomb (Link article)", *ITEM_KIND_LINKBOMB, 0),
    }
}

unsafe fn permitted() -> bool {
    common::is_native_training_mode()
        && common::is_ready_go()
        && !menu::captures_input()
        && !crate::character_roster::busy()
        && !crate::training::save_states::is_loading()
        && !crate::training::recording_or_playback()
        && !crate::replay_preview::occupied()
        && (!cfg!(feature = "gyro-variation-probe")
            || IntoIterator::into_iter([FighterId::Player, FighterId::CPU])
                .filter_map(try_get_module_accessor)
                .any(|body| app::utility::get_kind(&mut *body) == *FIGHTER_KIND_ROBOT))
}

pub unsafe fn handle_input(out: &mut MappedInputs, click: bool, x: bool) {
    if cfg!(feature = "turnip-save-probe") {
        #[cfg(feature = "turnip-save-probe")]
        turnip_save::handle_input(out, click, x);
        return;
    }
    let held = click && x;
    let mut state = STATE.lock().unwrap();
    let edge = held && !state.held;
    state.held = held;
    if held && permitted() {
        *out = MappedInputs::empty();
        if edge && state.pending.is_none() {
            if let Some(body) = try_get_module_accessor(FighterId::Player) {
                state.pending = Some(((*body).battle_object_id, state.next));
                state.next = (state.next + 1) % CASE_COUNT;
            }
        }
    }
}

unsafe fn observe(body: *mut app::BattleObjectModuleAccessor, index: usize, phase: &str) {
    let (name, wanted_kind, wanted_variation) = case(index);
    let held = ItemModule::is_have_item(body, 0);
    let kind = ItemModule::get_have_item_kind(body, 0);
    let id = ItemModule::get_have_item_id(body, 0);
    let mut actual_variation = None;
    let mut turnip_power = None;
    if held {
        if let Ok(id) = u32::try_from(id) {
            if sv_battle_object::is_active(id) {
                let item = sv_battle_object::module_accessor(id);
                if !item.is_null() && app::utility::get_category(&mut *item) == *BATTLE_OBJECT_CATEGORY_ITEM {
                    actual_variation = Some(WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_VARIATION));
                    let values: Vec<i32> = (0..8).map(|offset| WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_VALUE_1 + offset)).collect();
                    crate::logging::diagnostic(format_args!(
                        "[ItemStateProbe] detail phase={phase} case={index} status={} values={values:?} life={}/{} bomb={}/{}",
                        StatusModule::status_kind(item),
                        WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_LIFE_TIME),
                        WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_LIFE_TIME_MAX),
                        WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_BOMB_COUNTER),
                        WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_BOMB_COUNTER_MAX)
                    ));
                    if kind == *ITEM_KIND_PEACHDAIKON || kind == *ITEM_KIND_DAISYDAIKON {
                        turnip_power = Some(WorkModule::get_int(item, *ITEM_PEACHDAIKON_INSTANCE_WORK_INT_ATTACK_POWER));
                    }
                }
            }
        }
    }
    crate::logging::diagnostic(format_args!(
        "[ItemStateProbe] phase={phase} case={index} name={name} fighter={} wanted={wanted_kind}:{wanted_variation} held={held} kind={kind} id=0x{id:x} variation={actual_variation:?} turnip_power={turnip_power:?}",
        app::utility::get_kind(&mut *body)
    ));
}

unsafe fn link_article(body: *mut app::BattleObjectModuleAccessor) -> u64 {
    let source = [Some(body), try_get_module_accessor(FighterId::CPU)]
        .iter().copied().flatten()
        .find(|&candidate| app::utility::get_kind(&mut *candidate) == *FIGHTER_KIND_LINK);
    let Some(source) = source else {
        crate::logging::diagnostic(format_args!("[ItemStateProbe] article requires loaded Link"));
        return 0;
    };
    if ItemModule::is_have_item(source, 0) {
        crate::logging::diagnostic(format_args!("[ItemStateProbe] source already holding item; leave it intact"));
        return 0;
    }
    ArticleModule::generate_article_have_item(source, *FIGHTER_LINK_GENERATE_ARTICLE_LINKBOMB,
        *FIGHTER_HAVE_ITEM_WORK_MAIN, smash::phx::Hash40::new("invalid"));
    observe(source, 9, "article-source");
    if !ItemModule::is_have_item(source, 0) || ItemModule::get_have_item_kind(source, 0) != *ITEM_KIND_LINKBOMB { return 0; }
    if source == body { return 1; }
    let Ok(id) = u32::try_from(ItemModule::get_have_item_id(source, 0)) else { return 0; };
    if !sv_battle_object::is_active(id) { return 0; }
    let address = training_mod_sync::read(&common::ITEM_MANAGER_ADDR);
    if address == 0 { return 0; }
    let manager = *(address as *const *mut app::ItemManager);
    if manager.is_null() { return 0; }
    ItemModule::drop_item(source, 0.0, 0.0, 0);
    if !sv_battle_object::is_active(id) { return 0; }
    let accessor = sv_battle_object::module_accessor(id);
    if accessor.is_null() || app::utility::get_category(&mut *accessor) != *BATTLE_OBJECT_CATEGORY_ITEM
        || app::utility::get_kind(&mut *accessor) != *ITEM_KIND_LINKBOMB { return 0; }
    let item = ItemManager::find_active_item_from_id(manager, id) as *mut app::Item;
    if item.is_null() { return 0; }
    ItemModule::have_item_instance(body, item, 0, false, false, false, false);
    crate::logging::diagnostic(format_args!("[ItemStateProbe] article transfer exact_id=0x{id:x} received=0x{:x}", ItemModule::get_have_item_id(body, 0)));
    u64::from(ItemModule::is_have_item(body, 0) && ItemModule::get_have_item_id(body, 0) == u64::from(id))
}

pub unsafe fn on_frame(body: *mut app::BattleObjectModuleAccessor) {
    if cfg!(feature = "turnip-save-probe") {
        #[cfg(feature = "turnip-save-probe")]
        turnip_save::on_frame(body);
        return;
    }
    if try_get_module_accessor(FighterId::Player) != Some(body) { return; }
    let allowed = permitted();
    let observer_idle = {
        let state = STATE.lock().unwrap();
        state.pending.is_none() && state.observation.is_none()
    };
    // The diagnostic's native reads run outside the item-probe bookkeeping lock.
    if allowed && observer_idle {
        crate::turnip_face_caller_probe::observe_keyed_state(body);
    } else {
        crate::turnip_face_caller_probe::reset_keyed_state_hold();
    }
    let mut state = STATE.lock().unwrap();
    if !allowed {
        state.pending = None;
        state.observation = None;
        return;
    }
    if let Some((id, index)) = state.pending.take() {
        if id != (*body).battle_object_id { return; }
        let (name, kind, variation) = case(index);
        observe(body, index, "before");
        if ItemModule::is_have_item(body, 0) { ItemModule::remove_item(body, 0); }
        let forced_face = kind == *ITEM_KIND_PEACHDAIKON || kind == *ITEM_KIND_DAISYDAIKON;
        let previous_face = training_mod_sync::read(&TURNIP_CHOSEN);
        if forced_face {
            training_mod_sync::assign(&TURNIP_CHOSEN, Some((variation + 1) as u32));
        }
        let result = if !cfg!(feature = "gyro-variation-probe") && index == 9 { link_article(body) } else {
            ItemModule::have_item(body, app::ItemKind(kind), variation, 0, false, false)
        };
        if forced_face {
            if ItemModule::is_have_item(body, 0) {
                observe(body, index, "before-special-init");
                let action = if kind == *ITEM_KIND_DAISYDAIKON {
                    *ITEM_DAISYDAIKON_ACTION_SPECIAL_INIT
                } else {
                    *ITEM_PEACHDAIKON_ACTION_SPECIAL_INIT
                };
                // Native03: zero initializes the face but produces zero damage.
                // Natural Peach has power 5 with generic variation -1. Test the
                // native per-face power getter as the action's float argument.
                let power = match index {
                    1 => peach_smile_power(),
                    2 => peach_stitch_power(),
                    3 => daisy_stitch_power(),
                    7 => peach_winky_power(),
                    8 => peach_dot_eyes_power(),
                    _ => 0.0,
                };
                crate::logging::diagnostic(format_args!("[ItemStateProbe] special_init power={power}"));
                ItemModule::set_have_item_action(body, action, power, 0);
                observe(body, index, "after-special-init");
            }
            training_mod_sync::assign(&TURNIP_CHOSEN, previous_face);
        }
        crate::logging::diagnostic(format_args!("[ItemStateProbe] have_item returned=0x{result:x}"));
        observe(body, index, "immediate");
        if !cfg!(feature = "gyro-variation-probe") && index == 5 && !ItemModule::is_have_item(body, 0) {
            let attached = ItemModule::attach_item(body, app::ItemKind(kind), 0, false);
            crate::logging::diagnostic(format_args!("[ItemStateProbe] attach_item returned=0x{attached:x}"));
            observe(body, index, "attached");
        }
        state.observation = Some((id, index, 30));
        crate::training::ui::notifications::notification("Item Probe".into(), name.into(), 120);
    } else if let Some((id, index, frames)) = state.observation {
        if id != (*body).battle_object_id { state.observation = None; return; }
        if frames == 0 {
            observe(body, index, "settled");
            state.observation = None;
        } else {
            state.observation = Some((id, index, frames - 1));
        }
    }
}

// UNAPPLIED append to src/item_state_probe.rs.
// Existing L3+X alternates seed / verify. Save/load remain root-operated.
#[cfg(all(feature = "turnip-save-probe", any(feature = "source-capacity-probe", feature = "gyro-variation-probe")))]
compile_error!("turnip-save-probe must run separately from source-capacity-probe and gyro-variation-probe");

#[cfg(feature = "turnip-save-probe")]
mod turnip_save {
    use super::*;
    use training_mod_consts::character_items::HeldItemSnapshot;

    const CASES: usize = 16;
    const SETTLE: u8 = 30;
    const MAX_REPORTS: u8 = 40; // 16 setup + 16 verify + 8 manual natural/pickup reads.
    const WAIT_LIMIT: u16 = 1800;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Phase { Seed, Verify, Manual }
    #[derive(Clone, Copy)]
    struct Request { owner: u32, case: usize, phase: Phase }
    #[derive(Clone, Copy)]
    struct Wait { request: Request, age: u16, held: Option<u32>, stable: u8 }
    struct State {
        chord: bool,
        case: usize,
        baseline: Option<HeldItemSnapshot>,
        queued: Option<Request>,
        waiting: Option<Wait>,
        reports: u8,
    }
    static STATE: Mutex<State> = Mutex::new(State {
        chord: false, case: 0, baseline: None, queued: None, waiting: None, reports: 0,
    });

    fn seed(case: usize) -> HeldItemSnapshot {
        let face = (case % 8) as i32;
        let base = if case < 8 { 22 } else { 2 };
        let choice = base + match face { 5 => 1, 6 => 2, 7 => 3, _ => 0 };
        // Sentinel timers are valid and never request a positive countdown.
        // If native maxima differ, production keeps its native timers. The
        // actual settled snapshot, not this seed, becomes the saved baseline.
        HeldItemSnapshot::Item { choice, variation: face, lifetime: -1,
            lifetime_max: -1, fuse: -1, fuse_max: -1 }
    }

    fn saved_identity(snapshot: HeldItemSnapshot) -> Option<(u8, i32)> {
        match snapshot {
            HeldItemSnapshot::Item { choice, variation, .. } if snapshot.valid() => Some((choice, variation)),
            _ => None,
        }
    }

    unsafe fn live(body: *mut app::BattleObjectModuleAccessor)
        -> Option<(u32, i32, *mut app::BattleObjectModuleAccessor)>
    {
        if !ItemModule::is_have_item(body, 0) { return None; }
        let id = u32::try_from(ItemModule::get_have_item_id(body, 0)).ok()?;
        if !sv_battle_object::is_active(id) { return None; }
        let item = sv_battle_object::module_accessor(id);
        if item.is_null() || (*item).battle_object_id != id
            || app::utility::get_category(&mut *item) != *BATTLE_OBJECT_CATEGORY_ITEM
        { return None; }
        let kind = app::utility::get_kind(&mut *item);
        if kind != *ITEM_KIND_PEACHDAIKON && kind != *ITEM_KIND_DAISYDAIKON { return None; }
        Some((id, kind, item))
    }

    pub(super) unsafe fn handle_input(out: &mut MappedInputs, click: bool, x: bool) {
        let chord = click && x;
        let edge = {
            let mut state = STATE.lock().unwrap();
            let edge = chord && !state.chord;
            state.chord = chord;
            edge
        };
        if !chord || !super::permitted() { return; }
        *out = MappedInputs::empty();
        if !edge || !crate::character_items::turnip_probe_idle() { return; }
        let Some(body) = try_get_module_accessor(FighterId::Player) else { return; };
        let owner = (*body).battle_object_id;
        let request = {
            let mut state = STATE.lock().unwrap();
            if state.queued.is_some() || state.waiting.is_some() || state.reports >= MAX_REPORTS { return; }
            let phase = if state.case >= CASES { Phase::Manual }
                else if state.baseline.is_some() { Phase::Verify } else { Phase::Seed };
            let request = Request { owner, case: state.case, phase };
            state.queued = Some(request);
            request
        };
        crate::logging::diagnostic(format_args!(
            "[TurnipSaveProbe] requested phase={:?} case={} owner=0x{owner:x}; Seed is setup only, Verify never calls restore",
            request.phase, request.case));
    }

    pub(super) unsafe fn on_frame(body: *mut app::BattleObjectModuleAccessor) {
        if try_get_module_accessor(FighterId::Player) != Some(body) { return; }
        let owner = (*body).battle_object_id;
        if !super::permitted() {
            let mut state = STATE.lock().unwrap();
            // Production setup can asynchronously load its native source.
            // Pause that setup observation until gameplay is permitted again.
            // Verify/Manual must be rearmed after menu/load/roster activity.
            if state.queued.is_some_and(|request| request.phase != Phase::Seed) {
                state.queued = None;
            }
            if let Some(wait) = &mut state.waiting {
                if wait.request.phase == Phase::Seed {
                    wait.stable = 0;
                    wait.held = None;
                } else {
                    state.waiting = None;
                }
            }
            return;
        }
        let idle = crate::character_items::turnip_probe_idle();
        let request = {
            let mut state = STATE.lock().unwrap();
            if state.reports >= MAX_REPORTS { return; }
            if idle { state.queued.take() } else { None }
        };
        if let Some(request) = request {
            if request.owner != owner { return; }
            if request.phase == Phase::Seed {
                let expected = seed(request.case);
                crate::logging::diagnostic(format_args!(
                    "[TurnipSaveProbe] setup-submit case={} seed={expected:?}; production restore call, not real save/load acceptance",
                    request.case));
                // The only mutation in this probe: production owns resources,
                // constructor, probability override, power and timer behavior.
                crate::character_items::restore(body, expected);
            }
            STATE.lock().unwrap().waiting = Some(Wait { request, age: 0, held: None, stable: 0 });
            return;
        }
        let Some(mut wait) = STATE.lock().unwrap().waiting.take() else { return; };
        if wait.request.owner != owner { return; }
        wait.age = wait.age.saturating_add(1);
        if wait.age >= WAIT_LIMIT {
            crate::logging::diagnostic(format_args!(
                "[TurnipSaveProbe] timeout phase={:?} case={} age={}; no acceptance",
                wait.request.phase, wait.request.case, wait.age));
            return;
        }
        if !idle {
            wait.stable = 0;
            wait.held = None;
            STATE.lock().unwrap().waiting = Some(wait);
            return;
        }
        let identity = live(body);
        let id = identity.map(|(id, _, _)| id);
        if id != wait.held {
            wait.held = id;
            wait.stable = 0;
        }
        wait.stable = wait.stable.saturating_add(1);
        if wait.stable < SETTLE {
            STATE.lock().unwrap().waiting = Some(wait);
            return;
        }
        // Costly guarded reads happen exactly once for this explicit request.
        let actual = crate::character_items::snapshot(body);
        let keyed = identity.and_then(|(id, _, item)| crate::turnip_identity::held_face(body, id, item));
        let same = live(body) == identity;
        let expected = {
            let state = STATE.lock().unwrap();
            if wait.request.phase == Phase::Manual { None }
            else if wait.request.phase == Phase::Seed { Some(seed(wait.request.case)) }
            else { state.baseline }
        };
        let wanted_kind = if wait.request.case < 8 { *ITEM_KIND_PEACHDAIKON } else { *ITEM_KIND_DAISYDAIKON };
        let actual_identity = saved_identity(actual);
        let keyed_matches_snapshot = actual_identity.is_some_and(|(_, face)| keyed == Some(face));
        let identity_matches = same && keyed_matches_snapshot && match expected {
            Some(expected) => saved_identity(expected) == actual_identity
                && identity.is_some_and(|(_, kind, _)| kind == wanted_kind),
            None => identity.is_some(),
        };
        let full_snapshot_equal = expected == Some(actual);
        let logged_identity = identity.map(|(id, kind, _)| (id, kind));
        crate::logging::diagnostic(format_args!(
            "[TurnipSaveProbe] settled phase={:?} case={} expected={expected:?} actual={actual:?} held={logged_identity:x?} keyed={keyed:?} same={same} identity_matches={identity_matches} full_snapshot_equal={full_snapshot_equal}; real save/load provenance requires root receipt",
            wait.request.phase, wait.request.case));
        let mut state = STATE.lock().unwrap();
        state.reports = state.reports.saturating_add(1);
        if identity_matches {
            if wait.request.phase == Phase::Seed {
                state.baseline = Some(actual);
            } else if wait.request.phase == Phase::Verify {
                state.baseline = None;
                state.case += 1;
            }
        }
    }
}
