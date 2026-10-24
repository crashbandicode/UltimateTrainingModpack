//! Item selection runs on P1's native callback. Persist values, not game pointers.
use std::convert::TryFrom;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use smash::app::{self, lua_bind::*, sv_battle_object};
use smash::lib::lua_const::*;
use training_mod_consts::character_items::{valid_variation, HeldItemSnapshot, ITEM_NAMES};
use training_mod_sync::{assign, read};

use crate::common::{self, consts::FighterId, try_get_module_accessor};
use crate::training::{self, character_specific::items::{ALL_CHAR_ITEMS, TURNIP_CHOSEN}};
use crate::character_roster::sources;
use crate::source_lifetime::Holder;

const LEGACY_INDICES: [usize; 38] = [
    4, 15, 5, 6, 7, 8, 9, 10, 0, 1, 12, 13, 14, 16,
    17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
    31, 32, 2, 3, 11, 40, 41, 42, 43, 44,
];

#[derive(Clone, Copy)]
struct Spec { choice: usize, kind: i32, variation: i32, source: i32 }
#[derive(Clone, Copy)]
struct Pending {
    body: u32, choice: usize, saved: Option<HeldItemSnapshot>, age: u16,
    // Alpha acceptance evidence only; no per-frame formatting or allocation.
    #[cfg(feature = "managed-third-source-alpha")]
    capacity_polls: u16,
}
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static HAS_PENDING: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "turnip-save-probe")]
pub(crate) fn turnip_probe_idle() -> bool {
    !HAS_PENDING.load(Ordering::Acquire) && PENDING.lock().unwrap().is_none()
}

#[cfg(feature = "source-capacity-probe")]
pub(crate) fn capacity_probe_idle() -> bool {
    PENDING.lock().unwrap().is_none() && !HAS_PENDING.load(Ordering::Acquire)
}

macro_rules! power_functions {
    ($char:ident, $caps:ident) => { paste::paste! {
        extern "C" {
            #[link_name = "\u{1}_ZN3app11" $char "daikon32" $caps "_" $caps "DAIKON_DAIKON_1_POWEREv"]
            fn [<$char _smile>]() -> f32;
            #[link_name = "\u{1}_ZN3app11" $char "daikon32" $caps "_" $caps "DAIKON_DAIKON_2_POWEREv"]
            fn [<$char _native_2>]() -> f32;
            #[link_name = "\u{1}_ZN3app11" $char "daikon32" $caps "_" $caps "DAIKON_DAIKON_3_POWEREv"]
            fn [<$char _native_3>]() -> f32;
            #[link_name = "\u{1}_ZN3app11" $char "daikon32" $caps "_" $caps "DAIKON_DAIKON_4_POWEREv"]
            fn [<$char _native_4>]() -> f32;
            #[link_name = "\u{1}_ZN3app11" $char "daikon32" $caps "_" $caps "DAIKON_DAIKON_5_POWEREv"]
            fn [<$char _native_5>]() -> f32;
            #[link_name = "\u{1}_ZN3app11" $char "daikon32" $caps "_" $caps "DAIKON_DAIKON_6_POWEREv"]
            fn [<$char _winky>]() -> f32;
            #[link_name = "\u{1}_ZN3app11" $char "daikon32" $caps "_" $caps "DAIKON_DAIKON_7_POWEREv"]
            fn [<$char _dot>]() -> f32;
            #[link_name = "\u{1}_ZN3app11" $char "daikon32" $caps "_" $caps "DAIKON_DAIKON_8_POWEREv"]
            fn [<$char _stitch>]() -> f32;
        }
    }};
}
power_functions!(peach, PEACH);
power_functions!(daisy, DAISY);

fn spec(choice: usize) -> Option<Spec> {
    let item = &ALL_CHAR_ITEMS[*LEGACY_INDICES.get(choice)?];
    let kind = match choice {
        2..=5 => *ITEM_KIND_DAISYDAIKON,
        8 => *ITEM_KIND_BANANA,
        22..=25 => *ITEM_KIND_PEACHDAIKON,
        _ => **item.item_kind.as_ref()?,
    };
    Some(Spec { choice, kind, source: *item.fighter_kind,
        variation: item.variation.as_ref().map(|v| **v).unwrap_or(0) })
}

fn notice(text: impl Into<String>) {
    training::ui::notifications::clear_notification("Character Item");
    training::ui::notifications::notification("Character Item".into(), text.into(), 180);
}

unsafe fn permitted() -> bool {
    common::is_training_mode() && common::is_ready_go()
        && !crate::character_roster::busy() && !training::lobby::native_ui_busy()
        && !training::save_states::is_loading() && !training::recording_or_playback()
        && !crate::replay_preview::occupied()
}

/// Invoked for every A press, including selecting the already-checked item.
pub unsafe fn request(choice: usize) {
    if spec(choice).is_none() || !permitted() {
        cancel_pending();
        notice("Wait for practice to be ready");
        return;
    }
    // A new menu choice supersedes the old resource request too. Otherwise a
    // loader callback after menu close can start a no-longer-wanted source
    // before this callback submits the latest selection.
    sources::cancel(2);
    if let Some(body) = try_get_module_accessor(FighterId::Player) {
        let mut pending = PENDING.lock().unwrap();
        *pending = Some(Pending { body: (*body).battle_object_id, choice, saved: None, age: 0,
            #[cfg(feature = "managed-third-source-alpha")]
            capacity_polls: 0,
        });
        HAS_PENDING.store(true, Ordering::Release);
    } else {
        cancel_pending();
        notice("Wait for the player to finish loading");
    }
}

pub fn cancel_pending() {
    let mut pending = PENDING.lock().unwrap();
    *pending = None;
    HAS_PENDING.store(false, Ordering::Release);
    sources::cancel(2);
}
pub fn scene_changed() { cancel_pending(); }

pub unsafe fn on_frame(body: *mut app::BattleObjectModuleAccessor) {
    if !HAS_PENDING.load(Ordering::Acquire) { return; }
    if !common::is_training_mode() { cancel_pending(); return; }
    if try_get_module_accessor(FighterId::Player) != Some(body) { return; }
    let pending = {
        let mut pending = PENDING.lock().unwrap();
        HAS_PENDING.store(false, Ordering::Release);
        pending.take()
    };
    let Some(mut pending) = pending else { return; };
    if pending.body == (*body).battle_object_id && pending.saved.is_some()
        && training::save_states::is_loading() {
        requeue(pending);
        return;
    }
    if pending.body != (*body).battle_object_id || !permitted() {
        sources::cancel(2);
        notice("Item request cancelled; select it again when ready");
        return;
    }
    // A short-fuse item must not expire while the picker still consumes the
    // player's throw/save inputs. Keep the latest selection queued through the
    // menu's close guard, then grant it on the first usable gameplay callback.
    if crate::common::menu::captures_input() {
        requeue(pending);
        return;
    }
    let mut item_spec = spec(pending.choice).expect("validated request");
    if let Some(HeldItemSnapshot::Item { variation, .. }) = pending.saved {
        item_spec.variation = variation;
    }
    let lease = if cfg!(feature = "managed-sources") && !common_item(item_spec) {
        match sources::prepare(2, item_spec.source as u32,
            Holder { category: 2, id: u64::from(pending.body) }) {
            Ok(Some(lease)) => Some(lease),
            Ok(None) => {
                // Live hats and thrown items may pin every source for longer
                // than a native load takes. Keep their resources intact and
                // pause only the loading budget while waiting for capacity.
                let capacity_wait = sources::waiting_for_capacity(2);
                #[cfg(feature = "managed-third-source-alpha")]
                if capacity_wait {
                    pending.capacity_polls = pending.capacity_polls.saturating_add(1);
                    if matches!(pending.capacity_polls, 1 | 901) {
                        crate::logging::diagnostic(format_args!(
                            "[CharacterItem] capacity wait choice={} polls={} loading_age={}",
                            pending.choice, pending.capacity_polls, pending.age));
                    }
                }
                if !capacity_wait {
                    pending.age = pending.age.saturating_add(1);
                }
                if pending.age < 900 { requeue(pending); }
                else { sources::cancel(2); notice("Character item resources could not be loaded"); }
                return;
            }
            Err(reason) => { sources::cancel(2); notice(reason); return; }
        }
    } else { sources::cancel(2); None };
    match grant(body, item_spec, lease.is_some()) {
        Ok(item) => {
            if let Some(lease) = lease {
                sources::retain_item(lease, (*item).battle_object_id, item_spec.kind);
            }
            if pending.saved.is_some_and(|saved| !restore_timers(item, saved)) {
                notice("Item restored; its native timer limits changed");
            } else if pending.saved.is_some() && matches!(pending.choice, 2 | 22)
                && (1..=4).contains(&item_spec.variation) {
                // These exact native faces have no dedicated menu label.
                notice("Turnip restored");
            } else {
                notice(ITEM_NAMES[pending.choice]);
            }
        }
        Err(reason) => {
            if let Some(lease) = lease { sources::release(lease); }
            notice(reason);
        }
    }
}

fn requeue(request: Pending) {
    let mut pending = PENDING.lock().unwrap();
    if pending.is_none() { *pending = Some(request); }
    HAS_PENDING.store(true, Ordering::Release);
}

fn common_item(spec: Spec) -> bool {
    [*ITEM_KIND_BOMBHEI, *ITEM_KIND_DOSEISAN, *ITEM_KIND_BANANA].contains(&spec.kind)
}

unsafe fn held(body: *mut app::BattleObjectModuleAccessor)
    -> Option<(u32, *mut app::BattleObjectModuleAccessor)>
{
    if !ItemModule::is_have_item(body, 0) { return None; }
    let id = u32::try_from(ItemModule::get_have_item_id(body, 0)).ok()?;
    if !sv_battle_object::is_active(id) { return None; }
    let item = sv_battle_object::module_accessor(id);
    if item.is_null() || app::utility::get_category(&mut *item) != *BATTLE_OBJECT_CATEGORY_ITEM {
        return None;
    }
    Some((id, item))
}

unsafe fn turnip_power(spec: Spec) -> f32 {
    match (spec.kind == *ITEM_KIND_DAISYDAIKON, spec.variation) {
        (true, 1) => daisy_native_2(), (true, 2) => daisy_native_3(),
        (true, 3) => daisy_native_4(), (true, 4) => daisy_native_5(),
        (true, 0) => daisy_smile(), (true, 5) => daisy_winky(),
        (true, 6) => daisy_dot(), (true, 7) => daisy_stitch(),
        (false, 1) => peach_native_2(), (false, 2) => peach_native_3(),
        (false, 3) => peach_native_4(), (false, 4) => peach_native_5(),
        (false, 0) => peach_smile(), (false, 5) => peach_winky(),
        (false, 6) => peach_dot(), (false, 7) => peach_stitch(),
        _ => 0.0,
    }
}

unsafe fn grant(body: *mut app::BattleObjectModuleAccessor, spec: Spec, source_loaded: bool)
    -> Result<*mut app::BattleObjectModuleAccessor, String>
{
    let turnip = spec.kind == *ITEM_KIND_PEACHDAIKON || spec.kind == *ITEM_KIND_DAISYDAIKON;
    // Fighter-specific PRC values have native unguarded pointer consumers.
    // Asset-only loading is insufficient. A native resource-owner alternative
    // is being investigated; until verified, require the actual source fighter.
    let source_present = IntoIterator::into_iter([FighterId::Player, FighterId::CPU])
        .filter_map(try_get_module_accessor)
        .any(|source| app::utility::get_kind(&mut *source) == spec.source);
    #[cfg(feature = "source-owner-probe")]
    let source_present = source_present
        || crate::character_roster::source_owner_probe::item_source(spec.source as u32);
    if !common_item(spec) && !source_present && !source_loaded {
        let name = ITEM_NAMES[spec.choice].split(':').next().unwrap_or("Source fighter");
        return Err(format!("Load {name} as Player or CPU first"));
    }
    let power = if turnip { turnip_power(spec) } else { 0.0 };
    if turnip && (!power.is_finite() || power <= 0.0) {
        return Err("The turnip's fighter data is not ready".into());
    }
    let previous = read(&TURNIP_CHOSEN);
    if turnip { assign(&TURNIP_CHOSEN, Some((spec.variation + 1) as u32)); }
    // Replace only P1's held item. No CPU article or unrelated global item is taken.
    if ItemModule::is_have_item(body, 0) { ItemModule::remove_item(body, 0); }
    ItemModule::have_item(body, app::ItemKind(spec.kind), spec.variation, 0, false, false);
    let result = held(body).filter(|(_, item)| app::utility::get_kind(&mut **item) == spec.kind);
    if turnip {
        if result.is_some() {
            let action = if spec.kind == *ITEM_KIND_DAISYDAIKON { *ITEM_DAISYDAIKON_ACTION_SPECIAL_INIT }
                else { *ITEM_PEACHDAIKON_ACTION_SPECIAL_INIT };
            ItemModule::set_have_item_action(body, action, power, 0);
        }
        assign(&TURNIP_CHOSEN, previous);
    }
    // SPECIAL_INIT is a native action boundary; resolve the live held object
    // again rather than publishing the accessor obtained before that action.
    let Some((id, item)) = held(body).filter(|(_, item)| app::utility::get_kind(&mut **item) == spec.kind)
        else { return Err("This item could not be created".into()); };
    crate::logging::diagnostic(format_args!(
        "[CharacterItem] granted choice={} kind={} variation={} id=0x{:x}",
        spec.choice, spec.kind, spec.variation, id));
    Ok(item)
}

pub unsafe fn snapshot(body: *mut app::BattleObjectModuleAccessor) -> HeldItemSnapshot {
    let Some((id, item)) = held(body) else {
        return if ItemModule::is_have_item(body, 0) { HeldItemSnapshot::Unsupported } else { HeldItemSnapshot::Empty };
    };
    let kind = app::utility::get_kind(&mut *item);
    // Turnips persist their actual native face, independently of the generic
    // item variation (which is -1 for naturally pulled turnips).
    let variation = if kind == *ITEM_KIND_PEACHDAIKON || kind == *ITEM_KIND_DAISYDAIKON {
        let Some(face) = crate::turnip_identity::held_face(body, id, item) else {
            return HeldItemSnapshot::Unsupported;
        };
        face
    } else { WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_VARIATION) };
    let choice = (0..ITEM_NAMES.len()).find(|&choice| {
        spec(choice).is_some_and(|spec| spec.kind == kind && valid_variation(choice, variation))
    });
    let Some(choice) = choice else { return HeldItemSnapshot::Unsupported; };
    let saved = HeldItemSnapshot::Item { choice: choice as u8, variation,
        lifetime: WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_LIFE_TIME),
        lifetime_max: WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_LIFE_TIME_MAX),
        fuse: WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_BOMB_COUNTER),
        fuse_max: WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_BOMB_COUNTER_MAX) };
    if held(body) == Some((id, item)) && saved.valid() { saved } else { HeldItemSnapshot::Unsupported }
}

pub unsafe fn restore(body: *mut app::BattleObjectModuleAccessor, saved: HeldItemSnapshot) {
    if !saved.valid() { notice("Saved item data is invalid"); return; }
    match saved {
        HeldItemSnapshot::Empty => {
            if ItemModule::is_have_item(body, 0) { ItemModule::remove_item(body, 0); }
        }
        HeldItemSnapshot::Unsupported => notice("This saved item's exact variant is unsupported"),
        HeldItemSnapshot::Item { choice, variation, lifetime, lifetime_max, fuse, fuse_max } => {
            if cfg!(feature = "managed-sources") {
                cancel_pending();
                requeue(Pending { body: (*body).battle_object_id, choice: usize::from(choice),
                    saved: Some(saved), age: 0,
                    #[cfg(feature = "managed-third-source-alpha")]
                    capacity_polls: 0,
                });
                return;
            }
            let mut spec = spec(usize::from(choice)).expect("validated saved choice");
            spec.variation = variation;
            match grant(body, spec, false) {
                Ok(item) => {
                    // A different resource version must not receive incompatible timers.
                    if WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_LIFE_TIME_MAX) == lifetime_max
                        && WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_BOMB_COUNTER_MAX) == fuse_max
                    {
                        WorkModule::set_int(item, lifetime, *ITEM_INSTANCE_WORK_INT_LIFE_TIME);
                        WorkModule::set_int(item, fuse, *ITEM_INSTANCE_WORK_INT_BOMB_COUNTER);
                        crate::logging::diagnostic(format_args!("[CharacterItem] restored choice={choice} variation={variation} life={lifetime} fuse={fuse}"));
                    } else { notice("Item restored; its native timer limits changed"); }
                }
                Err(reason) => notice(reason),
            }
        }
    }
}

unsafe fn restore_timers(item: *mut app::BattleObjectModuleAccessor, saved: HeldItemSnapshot) -> bool {
    let HeldItemSnapshot::Item { choice, variation, lifetime, lifetime_max, fuse, fuse_max } = saved
        else { return false; };
    if WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_LIFE_TIME_MAX) == lifetime_max
        && WorkModule::get_int(item, *ITEM_INSTANCE_WORK_INT_BOMB_COUNTER_MAX) == fuse_max
    {
        WorkModule::set_int(item, lifetime, *ITEM_INSTANCE_WORK_INT_LIFE_TIME);
        WorkModule::set_int(item, fuse, *ITEM_INSTANCE_WORK_INT_BOMB_COUNTER);
        crate::logging::diagnostic(format_args!(
            "[CharacterItem] restored choice={choice} variation={variation} life={lifetime} fuse={fuse}"));
        true
    } else { false }
}
