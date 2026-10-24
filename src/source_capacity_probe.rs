//! One-shot empty entry7 experiment: Training, or separately opted-in LDN waiting.
//! Numeric snapshots are comparison evidence, never authority to dereference later.
//! This does not admit entry7 to managed sources or establish scene reservation for
//! production use. The operator explicitly requests one experiment with R3+X.
use super::native_source_capacity::{creation_delta, entry_observation, snapshot_for, Entry, Layout, Snapshot};
#[cfg(feature = "source-capacity-link-probe")]
use super::native_source_capacity::snapshot;
use super::{aligned, game_mode, native_source, read, ready_forms, setting, BASE, INSTALL};
use crate::common::{self, input::MappedInputs, menu};
use skyline::hooks::InlineCtx;
use super::native_service::ServiceEntryContext;
use smash::app::{self, lua_bind::*};
use smash::lib::lua_const::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

#[cfg(all(feature = "source-capacity-group-probe", any(
    feature = "source-ldn-capacity-link-probe", feature = "managed-third-source-alpha"
)))]
compile_error!("isolated grouped probe cannot share a build with ordinary P2 or managed third-source admission");

#[cfg(all(feature = "source-capacity-aegis-probe", feature = "source-capacity-trainer-probe"))]
compile_error!("select one isolated canonical group target per build");

const SLOT: usize = 7;
const STABLE_CALLBACKS: u16 = 30;
const QUIET_CALLBACKS: u16 = 300;
static ENABLED: AtomicBool = AtomicBool::new(false);
static BODY_CALLS: AtomicU32 = AtomicU32::new(0);

macro_rules! trace {
    ($($arg:tt)*) => { crate::logging::diagnostic(format_args!($($arg)*)) };
}

#[derive(Clone, Copy)]
struct Life {
    before: Snapshot,
    after: Option<Snapshot>,
    entry: usize,
    resource: usize,
    calls: u16,
    failed: bool,
    departed: bool,
    entry_destroyed: bool,
    destroy_listeners: Option<u64>,
    listener_removed: bool,
    owner_retired: bool,
    #[cfg(feature = "source-ldn-capacity-empty-probe")]
    claim_released: bool,
    #[cfg(feature = "source-ldn-capacity-empty-probe")]
    reconstruction_stable: Option<(Snapshot, u16, u8)>,
    #[cfg(feature = "source-ldn-capacity-empty-probe")]
    reconstruction_wait_logged: bool,
    reconstruction_calls: u16,
    reconstruction_done: bool,
    #[cfg(feature = "source-capacity-link-probe")]
    link: link_probe::Load,
}

struct Probe {
    held: bool,
    queued: bool,
    attempted: bool,
    epoch: u32,
    layout: Layout,
    stable: Option<(Snapshot, u16, u8)>,
    life: Option<Life>,
}
static STATE: Mutex<Probe> = Mutex::new(Probe {
    held: false,
    queued: false,
    attempted: false,
    epoch: 0,
    layout: Layout::Training,
    stable: None,
    life: None,
});

/// Called only after all existing subsystem locks have been released. Each query
/// takes and releases its own lock; no plugin mutex spans any native call.
fn plugin_idle() -> bool {
    if super::STATES.iter().any(|state| {
        let state = state.lock().unwrap();
        state.pending.is_some() || state.active.is_some() || state.loaded.is_some()
    }) { return false; }
    plugin_consumers_idle()
}

fn plugin_consumers_idle() -> bool {
    if !super::sources::capacity_probe_idle()
        || !crate::kirby_hats::capacity_probe_idle()
        || !crate::character_items::capacity_probe_idle()
    {
        return false;
    }
    #[cfg(feature = "item-state-probe")]
    if !crate::item_state_probe::capacity_probe_idle() {
        return false;
    }
    // The older independent CPU2 experiment would compete for the same scene.
    !cfg!(feature = "source-owner-probe") || cfg!(feature = "managed-sources")
}

/// Only read-only LDN return observations may accept a completed replacement.
/// Creation, Link installation/quiet checks, and Training keep plugin_idle().
#[cfg(feature = "source-ldn-capacity-empty-probe")]
unsafe fn reconstruction_idle() -> bool {
    !INSTALL.active()
        && super::source_ldn_capacity_probe::roster_settled()
        && plugin_consumers_idle()
}

unsafe fn scene_permitted() -> bool {
    ENABLED.load(Ordering::Acquire)
        && crate::character_lifecycle::native_hooks_ready()
        && common::is_native_training_mode()
        && matches!(game_mode::current(), Some(mode)
            if mode.initialized && mode.melee == 0x20 && mode.mode == 0x04040000)
        && common::is_ready_go()
        && !menu::captures_input()
        && !crate::training::save_states::is_loading()
        && !crate::replay_capture::loading()
        && !crate::replay_preview::occupied()
}

// Training Link retains the predicate above; a separate LDN opt-in uses its exact waiting gate.
unsafe fn permitted_layout() -> Option<Layout> {
    if scene_permitted() { return Some(Layout::Training); }
    #[cfg(feature = "source-ldn-capacity-empty-probe")]
    if scene_permitted_for(Layout::LdnWaiting) { return Some(Layout::LdnWaiting); }
    None
}

unsafe fn scene_permitted_for(layout: Layout) -> bool {
    match layout {
        Layout::Training => scene_permitted(),
        #[cfg(all(feature = "managed-ldn-third-source-alpha",
                not(feature = "source-ldn-capacity-empty-probe")))]
        Layout::LdnWaiting => false,
        #[cfg(feature = "source-ldn-capacity-empty-probe")]
        Layout::LdnWaiting => ENABLED.load(Ordering::Acquire)
            && crate::character_lifecycle::native_hooks_ready()
            && matches!(game_mode::current(), Some(mode)
                if mode.initialized && mode.melee == 0x0d && mode.mode == 0x02010000)
            && common::is_ready_go() && crate::training::lobby::active()
            && !crate::training::lobby::native_ui_busy() && !menu::captures_input()
            && !crate::training::save_states::is_loading()
            && !crate::training::recording_or_playback()
            && !crate::replay_capture::loading() && !crate::replay_preview::occupied()
            && !common::button_config::has_pending_request(),
    }
}

pub unsafe fn handle_input(out: &mut MappedInputs, left: bool, right: bool, x: bool) {
    let chord = right && x && !left;
    let layout = permitted_layout();
    let mut state = STATE.lock().unwrap();
    let edge = chord && !state.held;
    state.held = chord;
    if !chord { return; }
    let Some(layout) = layout else { return; };
    *out = MappedInputs::empty();
    if !edge {
        return;
    }
    if state.attempted || state.queued {
        trace!("[SourceCapacity] decline shortcut: already queued/attempted; restart required after a call");
        return;
    }
    state.queued = true;
    state.layout = layout;
    state.stable = None;
    trace!("[SourceCapacity] queued empty owner=7 layout={:?}; requires 30 stable native callbacks; separate_link_opt_in={} peer_not_inferred=true", state.layout, cfg!(feature = "source-capacity-link-probe") && (state.layout == Layout::Training || cfg!(feature = "source-ldn-capacity-link-probe")));
}

pub fn scene_changed() {
    let mut state = STATE.lock().unwrap();
    state.epoch = state.epoch.wrapping_add(1);
    if state.queued {
        trace!("[SourceCapacity] declined queued intent: scene changed");
    }
    state.queued = false;
    state.stable = None;
    if let Some(life) = state.life.as_mut() {
        life.departed = true;
    }
    // Never rearm attempted, clear native fields, or drop lifetime evidence here.
}

fn decline(reason: &str) {
    let mut state = STATE.lock().unwrap();
    state.queued = false;
    state.stable = None;
    trace!("[SourceCapacity] declined before native call: {}", reason);
}

unsafe fn baseline_for(current: &Snapshot) -> bool {
    match current.layout {
        Layout::Training => baseline(current),
        #[cfg(all(feature = "managed-ldn-third-source-alpha",
                not(feature = "source-ldn-capacity-empty-probe")))]
        Layout::LdnWaiting => false,
        #[cfg(feature = "source-ldn-capacity-empty-probe")]
        Layout::LdnWaiting => ldn_baseline(current),
    }
}

#[cfg(feature = "source-ldn-capacity-empty-probe")]
unsafe fn ldn_baseline(current: &Snapshot) -> bool {
    // Use the existing exact count4 admission. It remains unchanged and is never
    // called as a count5 post-create admission rule.
    if current.quantity.is_some() || current.count != 4
        || !native_source::exact_ldn_waiting_layout(current.primary[5] as *const u8, 5)
        || [1usize, 2, 3, 7].iter().any(|i| current.resources[*i] != 0 || current.aliases[*i] != 0)
    { return false; }
    let Some(fields) = current.ldn else { return false; };
    let Some(input) = fields.input else { return false; };
    let Some(resources) = fields.resources else { return false; };
    if fields.selection != [(0,0,0), (2,0x64,1), (2,0x64,2), (2,0x64,3),
        (1,1,4), (1,1,5), (1,1,6), (2,0x64,7)]
        || !input.unclaimed() || input.map7 != -1
        || input.controller != BASE.load(Ordering::Acquire) + 0x5339830
        || input.controller_flag != 0 || input.css_role != 0 || input.cpu_flag != 1
        || [7,15].iter().any(|i| input.slots[*i].state != 2)
    { return false; }
    for (index, owner) in current.layout.owners().iter().copied().enumerate() {
        let observed = current.original[index];
        let entry = current.primary[owner as usize] as *const u8;
        let kind = observed.kinds[0];
        let forms = super::Forms::starting(kind);
        if training_mod_consts::TrioFighter::from_kind(kind).is_none()
            || forms.count != 1 || forms.actor_count != 1
            || observed.count != 1 || observed.installed != 1
            || observed.kinds != [kind, u32::MAX, u32::MAX]
            || native_source::entry(owner) != Some(entry)
            || current.resources[owner as usize] != current.aliases[owner as usize]
            || resources[index].0 != kind
        { return false; }
        let ready = if index < 2 {
            ready_forms(entry, owner).is_some_and(|(forms, active)|
                forms.count == 1 && forms.actor_count == 1 && active == kind)
        } else {
            observed.phase == [0,14,0] && native_source::ready(entry, owner, kind).is_some()
        };
        if !ready { return false; }
    }
    let symbol = setting(&common::ITEM_MANAGER_ADDR);
    if symbol == 0 { return false; }
    let manager = read::<*mut app::ItemManager>(symbol as *const u8, 0);
    aligned(manager.cast::<u8>()) && ItemManager::get_num_of_active_item_all(manager) == 0
}

unsafe fn baseline(current: &Snapshot) -> bool {
    if current.count != 4
        || current.quantity != Some(1)
        || current.primary[4..].iter().any(|p| *p != 0)
        || current.backup.iter().any(|p| *p != 0)
        || current.resources[SLOT] != 0
        || current.aliases[SLOT] != 0
        || current.selection[SLOT] != 2
    {
        return false;
    }
    for owner in 0..2 {
        // Fresh current pointers, not retained baseline pointers; callers invoke
        // this immediately after snapshot() in the same serialized callback.
        let entry = current.primary[owner] as *const u8;
        let Some((forms, kind)) = ready_forms(entry, owner as u32) else {
            return false;
        };
        if forms.count != 1
            || kind == *FIGHTER_KIND_KIRBY as u32
            || kind != super::ORIGINAL_KINDS[owner].load(Ordering::Acquire)
        {
            return false;
        }
    }
    // Fresh native Training keeps one inactive original CPU body in each
    // parked helper. Installed0 is a transient loading state, not this baseline.
    let cpu_kind = super::ORIGINAL_KINDS[1].load(Ordering::Acquire);
    for owner in 2..4 {
        let helper = &current.original[owner];
        let entry = current.primary[owner] as *const u8;
        let resource = current.resources[owner] as *const u8;
        if helper.count != 1
            || helper.kinds != [cpu_kind, u32::MAX, u32::MAX]
            || helper.installed != 1
            || helper.phase != [0, 14, 0]
            || native_source::entry(owner as u32) != Some(entry)
            || !native_source::parked_ready(entry, owner as u32, cpu_kind)
            || !aligned(resource)
            || current.aliases[owner] != resource as usize
            || read::<usize>(resource, 0) != BASE.load(Ordering::Acquire) + 0x509fb10
            || read::<u32>(resource, 0x58) != cpu_kind
        {
            return false;
        }
    }
    let symbol = setting(&common::ITEM_MANAGER_ADDR);
    if symbol == 0 {
        return false;
    }
    let manager = read::<*mut app::ItemManager>(symbol as *const u8, 0);
    aligned(manager.cast::<u8>()) && ItemManager::get_num_of_active_item_all(manager) == 0
}

unsafe fn empty_candidate(current: &Snapshot) -> Option<Entry> {
    let entry = entry_observation(current.primary[SLOT] as *const u8, SLOT as u32)?;
    let resource = current.resources[SLOT] as *const u8;
    #[cfg(feature = "source-ldn-capacity-empty-probe")]
    if current.layout == Layout::LdnWaiting
        && read::<u32>(current.primary[SLOT] as *const u8, 0x28) != 1
    { return None; }
    (entry.count == 1
        && entry.kinds == [u32::MAX; 3]
        && entry.installed == 0
        && entry.phase == [0, 14, 0]
        && aligned(resource)
        && current.resources[SLOT] == current.aliases[SLOT]
        && read::<usize>(resource, 0) == BASE.load(Ordering::Acquire) + 0x509fb10
        && read::<u32>(resource, 0x58) == u32::MAX
        && BODY_CALLS.load(Ordering::Acquire) == 0)
        .then_some(entry)
}

unsafe fn observe_life(ctx: &ServiceEntryContext, entry: *const u8, owner: u32, life: Life) {
    if owner == SLOT as u32
        && entry as usize == life.entry
        && !life.entry_destroyed
        && !life.departed
        && life.calls < QUIET_CALLBACKS
    {
        if !scene_permitted_for(life.before.layout) {
            return;
        }
        let current = snapshot_for(life.before.layout, ctx, entry, owner);
        let valid = !INSTALL.active()
            && plugin_idle()
            && current.is_some_and(|current| {
                life.after == Some(current) && empty_candidate(&current).is_some()
            });
        let mut state = STATE.lock().unwrap();
        let Some(saved) = state.life.as_mut().filter(|saved| saved.entry == life.entry
            && !saved.departed && !saved.entry_destroyed && saved.calls == life.calls) else {
            return;
        };
        saved.calls += 1;
        saved.failed |= !valid;
        if matches!(saved.calls, 1 | 30 | QUIET_CALLBACKS) || (!valid && !life.failed) {
            trace!(
                "[SourceCapacity] quiet calls={} valid={} failed={} body_frames={} current={:x?}",
                saved.calls,
                valid,
                saved.failed,
                BODY_CALLS.load(Ordering::Acquire),
                current
            );
        }
    } else if life.before.layout.callback(owner) != 0 && life.departed
        && !life.reconstruction_done && scene_permitted_for(life.before.layout) {
        #[cfg(feature = "source-ldn-capacity-empty-probe")]
        if life.before.layout == Layout::LdnWaiting && life.reconstruction_calls == 0
            && !reconstruction_idle()
        {
            let mut state = STATE.lock().unwrap();
            if let Some(saved) = state.life.as_mut().filter(|saved| saved.entry == life.entry
                && saved.departed && !saved.reconstruction_done && saved.reconstruction_calls == 0)
            {
                if !saved.reconstruction_wait_logged {
                    saved.reconstruction_wait_logged = true;
                    trace!("[SourceCapacity] LDN reconstruction waiting for settled roster/idle plugin; observation budget not started; no acceptance or native cancellation");
                }
            }
            return;
        }
        let current = snapshot_for(life.before.layout, ctx, entry, owner);
        let candidate_absent = current.is_some_and(|current| {
            current.count == 4
                && current.primary[SLOT] == 0
                && current.backup[SLOT] == 0
                && current.resources[SLOT] == 0
                && current.aliases[SLOT] == 0
        });
        let restored = current.is_some_and(|current| baseline_for(&current));
        #[cfg(feature = "source-ldn-capacity-empty-probe")]
        let restored = if life.before.layout == Layout::LdnWaiting {
            // Every read is from this callback. Stable saved values are identities only.
            let qualified = restored && reconstruction_idle()
                && snapshot_for(life.before.layout, ctx, entry, owner) == current
                && scene_permitted_for(life.before.layout);
            let mut state = STATE.lock().unwrap();
            let Some(saved) = state.life.as_mut().filter(|s| s.entry == life.entry
                && s.departed && !s.reconstruction_done) else { return; };
            saved.reconstruction_stable = current.filter(|_| qualified).map(|now| {
                let (count, callbacks) = match saved.reconstruction_stable {
                    Some((old, count, seen)) if old == now => (count + 1, seen),
                    _ => (1, 0),
                };
                (now, count, callbacks | now.layout.callback(owner))
            });
            saved.reconstruction_stable.is_some_and(|(_, count, seen)|
                count >= STABLE_CALLBACKS && life.before.layout.callbacks_complete(seen))
        } else { restored };
        #[cfg(feature = "source-capacity-link-probe")]
        let link_restored = !link_probe::requested(life.before.layout)
            || (restored && link_probe::reconstruction(life));
        let mut state = STATE.lock().unwrap();
        let Some(saved) = state.life.as_mut().filter(|saved| saved.entry == life.entry
            && saved.departed && !saved.reconstruction_done) else {
            return;
        };
        saved.reconstruction_calls += 1;
        #[cfg(feature = "source-ldn-capacity-empty-probe")]
        if saved.before.layout == Layout::LdnWaiting && saved.reconstruction_calls == 1 {
            trace!("[SourceCapacity] LDN reconstruction observation started; limit300 includes subsequent busy/invalid callbacks");
        }
        if restored || saved.reconstruction_calls >= QUIET_CALLBACKS {
            saved.reconstruction_done = true;
            #[cfg(feature = "source-ldn-capacity-empty-probe")]
            if saved.before.layout == Layout::LdnWaiting { saved.failed |= !saved.claim_released; }
            saved.failed |= !restored
                || !saved.entry_destroyed
                || !saved.listener_removed
                || !saved.owner_retired
                || saved.calls != QUIET_CALLBACKS
                || BODY_CALLS.load(Ordering::Acquire) != 0;
            #[cfg(feature = "source-capacity-link-probe")]
            {
                saved.failed |= !link_restored;
            }
            #[cfg(feature = "source-ldn-capacity-empty-probe")]
            if saved.before.layout == Layout::LdnWaiting {
                trace!("[SourceCapacity] LDN reconstruction stable={:?} claim_released={} input_unclaimed={} failed={}; input/service identities are fresh, not dereferenced old addresses",
                    saved.reconstruction_stable.map(|(_, count, seen)| (count, seen)),
                    saved.claim_released,
                    current.and_then(|s| s.ldn).and_then(|f| f.input).is_some_and(|input| input.unclaimed()),
                    saved.failed);
            }
            if !saved.entry_destroyed {
                trace!("[SourceCapacity] UNRESOLVED: entry7 destruction not observed before reentry; no recovery writes");
            }
            trace!("[SourceCapacity] reconstruction baseline={} candidate_absent={} entry_destroyed={} listener_removed={} owner_cleanup_complete={} quiet_calls={} failed={} body_frames={} current={:x?}; presentation and resource-cost acceptance remain external",
                restored, candidate_absent, saved.entry_destroyed,
                saved.listener_removed, saved.owner_retired, saved.calls, saved.failed,
                BODY_CALLS.load(Ordering::Acquire), current);
        }
    }
}

/// Returns true only when an armed attempt consumed this callback. Other plugin
/// work resumes normally on the next callback; existing source admission is intact.
pub unsafe fn service(ctx: &ServiceEntryContext, entry: *const u8, owner: u32) -> bool {
    if !ENABLED.load(Ordering::Acquire) {
        return false;
    }
    let (queued, epoch, layout, life) = {
        let state = STATE.lock().unwrap();
        (state.queued, state.epoch, state.layout, state.life)
    };
    if let Some(life) = life {
        #[cfg(feature = "source-capacity-link-probe")]
        if link_probe::requested(life.before.layout) && link_probe::service(ctx, entry, owner, life) {
            return true;
        }
        observe_life(ctx, entry, owner, life);
    }
    if !queued || layout.callback(owner) == 0 {
        return false;
    }
    // Observe idle before taking the common mutation gate, then revalidate under it.
    if !scene_permitted_for(layout) || INSTALL.active() || !plugin_idle() {
        decline("scene/UI, native hooks, active install or queued consumer work");
        return false;
    }
    let Some(before) = snapshot_for(layout, ctx, entry, owner) else {
        decline("current dispatcher, manager, resource context or entry identity unproven");
        return false;
    };
    if !baseline_for(&before) {
        trace!("[SourceCapacity] rejected baseline={:x?}", before);
        decline("exact selected-layout baseline unavailable; Training predicate unchanged; LDN requires ordinary 0/4/5/6, count4, default role and unclaimed inputs");
        return false;
    }
    #[cfg(feature = "source-ldn-capacity-empty-probe")]
    if layout == Layout::LdnWaiting && (snapshot_for(layout, ctx, entry, owner) != Some(before)
        || !scene_permitted_for(layout) || !plugin_idle()) {
        decline("LDN identity or idle state changed during native readiness reads");
        return false;
    }
    {
        let mut state = STATE.lock().unwrap();
        if !state.queued || state.epoch != epoch {
            return false;
        }
        let (count, callbacks) = match state.stable {
            None => (1, layout.callback(owner)),
            Some((old, count, seen)) if old == before => (count + 1, seen | layout.callback(owner)),
            Some(_) => {
                state.queued = false;
                state.stable = None;
                trace!("[SourceCapacity] declined: original baseline changed during observation");
                return false;
            }
        };
        state.stable = Some((before, count, callbacks));
        if count == 1 {
            trace!("[SourceCapacity] baseline start={:x?}", before);
        }
        if count < STABLE_CALLBACKS { return false; }
        if !layout.callbacks_complete(callbacks) {
            state.queued = false;
            state.stable = None;
            trace!("[SourceCapacity] declined: required helper callbacks absent in 30 stable samples");
            return false;
        }
        trace!("[SourceCapacity] preflight layout={:?} stable={} callbacks={}", layout, count, callbacks);
    }
    let Some(token) = INSTALL.try_begin(false) else {
        decline("mutation serialization unavailable");
        return false;
    };
    let revalidated = scene_permitted_for(layout)
        && plugin_idle()
        && snapshot_for(layout, ctx, entry, owner)
            .is_some_and(|current| current == before && baseline_for(&current))
        && snapshot_for(layout, ctx, entry, owner) == Some(before);
    let consumed = {
        let mut state = STATE.lock().unwrap();
        if !revalidated || !state.queued || state.epoch != epoch || state.attempted {
            false
        } else {
            // Consume intent before invoking native code. No native pointer is
            // cached for a later invocation, and no mutex spans the call.
            state.queued = false;
            state.stable = None;
            state.attempted = true;
            true
        }
    };
    if !consumed {
        super::finish_install(token, "capacity preflight declined");
        decline("pre-call revalidation changed");
        return false;
    }
    BODY_CALLS.store(0, Ordering::Release);
    trace!(
        "[SourceCapacity] invoking exact creation export owner=7 epoch={} before={:x?}",
        epoch,
        before
    );
    // The exact export takes signed int in w0, with no receiver. Its return type
    // is not recovered; this projection deliberately discards the return value.
    if !super::native_source_capacity::create(super::native_source_capacity::Claimant::Probe) {
        super::finish_install(token, "capacity creation claimed by another owner");
        decline("entry7 already claimed by another native source owner");
        return false;
    }
    let after = snapshot_for(layout, ctx, entry, owner);
    if let Some(after) = after {
        super::native_source_capacity::bind_owner(
            super::native_source_capacity::Claimant::Probe,
            after.resources[SLOT],
        );
    }
    let valid = after
        .is_some_and(|after| creation_delta(before, after) && empty_candidate(&after).is_some());
    {
        let mut state = STATE.lock().unwrap();
        let departed = state.epoch != epoch;
        state.life = Some(Life {
            before,
            after,
            entry: after.map_or(0, |s| s.primary[SLOT]),
            resource: after.map_or(0, |s| s.resources[SLOT]),
            calls: 0,
            failed: !valid || departed,
            departed,
            entry_destroyed: false,
            destroy_listeners: None,
            listener_removed: false,
            owner_retired: false,
            #[cfg(feature = "source-ldn-capacity-empty-probe")]
            claim_released: false,
            #[cfg(feature = "source-ldn-capacity-empty-probe")]
            reconstruction_stable: None,
            #[cfg(feature = "source-ldn-capacity-empty-probe")]
            reconstruction_wait_logged: false,
            reconstruction_calls: 0,
            reconstruction_done: false,
            #[cfg(feature = "source-capacity-link-probe")]
            link: link_probe::Load::new(),
        });
    }
    trace!("[SourceCapacity] creation returned valid={} after={:x?}; no recovery writes or manual removal", valid, after);
    super::finish_install(token, "empty capacity creation returned");
    true
}

pub fn body_frame(owner: u32) {
    if owner == SLOT as u32 {
        let count = BODY_CALLS.fetch_add(1, Ordering::AcqRel);
        if count == 0 {
            let mut state = STATE.lock().unwrap();
            if state.attempted {
                if let Some(life) = state.life.as_mut() {
                    life.failed = true;
                }
                trace!("[SourceCapacity] FAIL: owner7 fighter body callback observed");
            }
        }
    }
}

/// Also correlate the actual current native module: a wrong WorkModule entry ID
/// must not make a loaded owner7 body invisible to the development observation.
#[cfg(feature = "source-capacity-link-probe")]
pub unsafe fn body_identity(body: *mut app::BattleObjectModuleAccessor, owner: i32) {
    if owner == SLOT as i32 {
        return;
    } // Counted by the existing owner observation.
    let life = STATE.lock().unwrap().life;
    let Some(life) = life.filter(|life| !life.entry_destroyed) else {
        return;
    };
    let Some(entry) = native_source::entry(SLOT as u32) else {
        return;
    };
    if entry as usize != life.entry {
        return;
    }
    let installed = read::<u64>(entry, 0x4150);
    if installed > 3 {
        return;
    }
    for index in 0..installed as usize {
        let fighter = read::<*const u8>(entry, 0x4160 + index * 8);
        if aligned(fighter)
            && read::<usize>(fighter, 0) == BASE.load(Ordering::Acquire) + 0x4f80210
            && read::<u64>(fighter, 0x10) == SLOT as u64
            && read::<usize>(fighter, 0x20) == body as usize
        {
            body_frame(SLOT as u32);
            break;
        }
    }
}

pub unsafe fn on_destroy(entry: *const u8, owner: u32) {
    #[cfg(feature = "source-ldn-capacity-empty-probe")]
    {
        let invalidates = {
            let state = STATE.lock().unwrap();
            let matches = |snapshot: Snapshot| snapshot.layout == Layout::LdnWaiting
                && snapshot.layout.owners().iter().copied().enumerate().any(|(i, slot)|
                    slot == owner && snapshot.original[i].identity == entry as usize);
            state.stable.is_some_and(|(snapshot, _, _)| matches(snapshot))
                || state.life.is_some_and(|life| !life.departed && matches(life.before))
        };
        if invalidates { scene_changed(); }
    }
    if owner != SLOT as u32 {
        return;
    }
    let observed = STATE.lock().unwrap().life;
    let Some(observed) = observed.filter(|life| entry as usize == life.entry && !life.entry_destroyed) else {
        return;
    };
    // This destructor owns entry. Its current global service is the same root
    // used immediately afterward by native listener removal.
    let service = read::<*const u8>(BASE.load(Ordering::Acquire) as *const u8, 0x52c50f8);
    let same_service = service as usize == observed.before.service && aligned(service);
    let inner = if same_service {
        read::<*const u8>(service, 0)
    } else {
        std::ptr::null()
    };
    let destroy_listeners = (inner as usize == observed.before.service_inner && aligned(inner))
        .then(|| read::<u64>(inner, 0x930));
    let installed = read::<u64>(entry, 0x4150);
    #[cfg(feature = "source-capacity-link-probe")]
    let link_destruction = link_probe::requested(observed.before.layout)
        .then(|| link_probe::destruction_observation(entry));
    let mut state = STATE.lock().unwrap();
    let Some(life) = state.life.as_mut().filter(|life| life.entry == observed.entry && !life.entry_destroyed) else {
        return;
    };
    life.destroy_listeners = destroy_listeners;
    life.entry_destroyed = true;
    life.failed |= life.destroy_listeners.is_none();
    #[cfg(feature = "source-capacity-link-probe")]
    let install = link_destruction.and_then(|observation| link_probe::destroying(life, observation));
    trace!("[SourceCapacity] native entry destruction entry={:p} original_service={} listeners={:?} body_frames={} installed={}",
        entry, life.destroy_listeners.is_some(), life.destroy_listeners,
        BODY_CALLS.load(Ordering::Acquire), installed);
    drop(state);
    #[cfg(feature = "source-capacity-link-probe")]
    if let Some(token) = install {
        super::finish_install(token, "capacity Link native destruction");
    }
}

// Exact boundary immediately after native removal returned, while entry and
// current service arguments remain owned by the native destructor.
pub(super) unsafe fn listener_removed(ctx: &InlineCtx) {
    let observed = STATE.lock().unwrap().life;
    let Some(observed) = observed.filter(|life| ctx.registers[19].x() as usize == life.entry
        && life.entry_destroyed) else { return; };
    let Some(before) = observed.destroy_listeners else { return; };
    let service = ctx.registers[21].x() as *const u8;
    let same = aligned(service) && service as usize == observed.before.service;
    let inner = if same {
        read::<*const u8>(service, 0)
    } else {
        std::ptr::null()
    };
    let count = (aligned(inner) && inner as usize == observed.before.service_inner)
        .then(|| read::<u64>(inner, 0x930));
    let mut state = STATE.lock().unwrap();
    let Some(life) = state.life.as_mut().filter(|life| life.entry == observed.entry
        && life.entry_destroyed && life.destroy_listeners == Some(before)) else { return; };
    // Consume once, after current reads and identity revalidation.
    life.destroy_listeners = None;
    life.listener_removed = count.is_some_and(|after| before.checked_sub(1) == Some(after));
    life.failed |= !life.listener_removed;
    trace!("[SourceCapacity] native listener removal entry=0x{:x} same_service={} before={:?} after={:?} removed={}",
        life.entry, same, before, count, life.listener_removed);
}

// Resource owner's deleting destructor, after its cleanup body returns and
// before the tail call to native free. Do not dereference retired subobjects.
pub(super) unsafe fn owner_cleanup_complete(ctx: &InlineCtx) {
    let mut state = STATE.lock().unwrap();
    let Some(life) = state.life.as_mut() else {
        return;
    };
    if life.resource == 0
        || ctx.registers[19].x() as usize != life.resource
        || life.owner_retired
        || life.reconstruction_done
    {
        return;
    }
    life.owner_retired = true;
    trace!("[SourceCapacity] native resource-owner cleanup complete owner=0x{:x} original_context=0x{:x} entry_destroyed={} departed={}; native free is next, no pointer dereference",
        life.resource, life.before.context, life.entry_destroyed, life.departed);
}

#[cfg(feature = "source-ldn-capacity-empty-probe")]
pub(super) fn claim_retired(owner: usize, released: bool, claim: u8, bound_owner: usize) {
    let mut state = STATE.lock().unwrap();
    let Some(life) = state.life.as_mut().filter(|life|
        (life.before.layout == Layout::LdnWaiting || cfg!(feature = "source-capacity-group-probe"))
        && life.resource != 0 && life.resource == owner && !life.reconstruction_done) else { return; };
    life.claim_released = released && claim == 0 && bound_owner == 0;
    life.failed |= !life.claim_released || !life.owner_retired
        || !life.entry_destroyed || !life.listener_removed;
    #[cfg(feature = "source-capacity-group-probe")]
    trace!("[SourceCapacityGroup] claim retirement layout={:?} owner=0x{:x} released={} claim={} bound_owner=0x{:x} actor_mask=0x{:x} entry_destroyed={} listener_removed={} owner_retired={} failed={}",
        life.before.layout, owner, released, claim, bound_owner, link_probe::cleanup_mask(life),
        life.entry_destroyed, life.listener_removed, life.owner_retired, life.failed);
    #[cfg(not(feature = "source-capacity-group-probe"))]
    trace!("[SourceCapacity] LDN claim retirement owner=0x{:x} released={} claim={} bound_owner=0x{:x} entry_destroyed={} listener_removed={} owner_retired={} failed={}",
        owner, released, claim, bound_owner, life.entry_destroyed, life.listener_removed,
        life.owner_retired, life.failed);
}

pub unsafe fn install(_base: usize) {
    if !super::native_source_capacity::creation_supported() {
        trace!("[SourceCapacity] shared creation contract unavailable; disabled");
        return;
    }
    #[cfg(feature = "source-capacity-link-probe")]
    link_probe::install(_base);
    ENABLED.store(true, Ordering::Release);
    #[cfg(feature = "source-ldn-capacity-empty-probe")]
    trace!("[SourceCapacity] separate LDN empty mutation opt-in enabled: R3+X, exact 0/4/5/6; separate_ldn_link_opt_in={} no source admission; connected-peer chronology remains external", cfg!(feature = "source-ldn-capacity-link-probe"));
    trace!("[SourceCapacity] development empty probe enabled; native Training R3+X; one creation per process; separate_link_opt_in={}", cfg!(feature = "source-capacity-link-probe"));
}

#[cfg(feature = "source-capacity-link-probe")]
pub(super) unsafe fn actor_cleanup(ctx: &InlineCtx) {
    link_probe::actor_cleanup(ctx);
}

/// This feature adds one isolated load, not a managed-source admission policy.
#[cfg(feature = "source-capacity-link-probe")]
mod link_probe {
    use super::super::native_source_capacity::{inputs, original_resources, Inputs};
    use super::super::{finish_install, InstallToken};
    use super::*;

    /// Training retains its existing opt-in. P1-only LDN cannot enter this path.
    pub(super) fn requested(layout: Layout) -> bool {
        match layout {
            Layout::Training => true,
            #[cfg(all(feature = "managed-ldn-third-source-alpha",
                not(feature = "source-ldn-capacity-empty-probe")))]
            Layout::LdnWaiting => false,
            #[cfg(feature = "source-ldn-capacity-empty-probe")]
            Layout::LdnWaiting => cfg!(any(feature = "source-ldn-capacity-link-probe", feature = "source-capacity-group-probe")),
        }
    }

    unsafe fn permitted(life: Life) -> bool {
        requested(life.before.layout) && scene_permitted_for(life.before.layout)
    }

    unsafe fn current_snapshot(ctx: &ServiceEntryContext, entry: *const u8, life: Life) -> Option<Snapshot> {
        match life.before.layout {
            // Keep the legacy Training wrapper and its mandatory quantity read.
            Layout::Training => snapshot(ctx, entry, SLOT as u32),
            #[cfg(all(feature = "managed-ldn-third-source-alpha",
                not(feature = "source-ldn-capacity-empty-probe")))]
            Layout::LdnWaiting => None,
            #[cfg(feature = "source-ldn-capacity-empty-probe")]
            Layout::LdnWaiting => snapshot_for(Layout::LdnWaiting, ctx, entry, SLOT as u32),
        }
    }

    unsafe fn actor(entry: *const u8) -> Option<(usize, usize)> {
        #[cfg(not(feature = "source-capacity-group-probe"))]
        { super::super::native_source_capacity::ordinary_actor(entry, TARGET) }
        #[cfg(feature = "source-capacity-group-probe")]
        { actor_set(entry).and_then(|set| set.actors[0]).map(|record| (record.actor, record.binding)) }
    }

    #[cfg(not(feature = "source-capacity-group-probe"))]
    const TARGET: u32 = 0x02;
    #[cfg(all(feature = "source-capacity-group-probe", not(any(
        feature = "source-capacity-aegis-probe", feature = "source-capacity-trainer-probe"
    ))))]
    const TARGET: u32 = 0x4b;
    #[cfg(feature = "source-capacity-aegis-probe")]
    const TARGET: u32 = 0x5b; // Non-first descriptor tests fixed native bank order.
    #[cfg(all(feature = "source-capacity-trainer-probe", not(feature = "source-capacity-aegis-probe")))]
    const TARGET: u32 = 0x25; // Same alternate-bank contract, three members.
    #[cfg(not(feature = "source-capacity-group-probe"))]
    const TARGET_LABEL: &str = "Link";
    #[cfg(feature = "source-capacity-group-probe")]
    const TARGET_LABEL: &str = "target";

    macro_rules! load_trace {
        ($message:literal $(, $argument:expr)* $(,)?) => {{
            #[cfg(feature = "source-capacity-group-probe")]
            trace!(concat!("[SourceCapacityGroup] ", $message) $(, $argument)*);
            #[cfg(not(feature = "source-capacity-group-probe"))]
            trace!(concat!("[SourceCapacityLink] ", $message) $(, $argument)*);
        }};
    }
    // Native651660 has two ownership classes: linked partners, or a fixed
    // alternate bank. Descriptor actor_count is not the bank allocation count.
    #[cfg(feature = "source-capacity-group-probe")]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct ActorRecord {
        actor: usize,
        control: usize,
        partner: usize,
        owner: u32,
        cleanup_owner: u32,
        kind: u32,
        flags: [u8; 3],
        control_actor: usize,
        input_index: i32,
        binding: usize,
        alias_index: i32,
        partner_control: usize,
    }

    #[cfg(feature = "source-capacity-group-probe")]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct ActorSet {
        pointers: [usize; 5],
        actors: [Option<ActorRecord>; 3],
    }

    #[cfg(feature = "source-capacity-group-probe")]
    #[derive(Clone, Copy)]
    struct ActorPlan {
        first: usize,
        count: usize,
        kinds: [u32; 3],
        partners: bool,
    }

    #[cfg(feature = "source-capacity-group-probe")]
    fn expected_actor_mask() -> u8 {
        (1u8 << super::super::Forms::starting(TARGET).count) - 1
    }

    #[cfg(feature = "source-capacity-group-probe")]
    unsafe fn actor_plan() -> Option<ActorPlan> {
        let forms = super::super::Forms::starting(TARGET);
        if forms.flags & 4 == 0 {
            return (forms.count == 2 && forms.actor_count == 2).then_some(ActorPlan {
                first: 0, count: 2, kinds: forms.kinds, partners: true,
            });
        }
        if forms.actor_count != 1 || !(2..=3).contains(&forms.count) { return None; }
        // Same table selection as native655450/6516b0. The two immutable
        // tables are fingerprinted before hooks; descriptor order may differ.
        let offset = if TARGET & !1 == 0x5a { 0x453c158 } else { 0x453c164 };
        let table = (BASE.load(Ordering::Acquire) + offset) as *const u8;
        let kinds = [0usize, 1, 2].map(|index| read::<u32>(table, index * 4));
        let count = forms.count as usize;
        if kinds[..count].iter().any(|kind| !forms.same_group(*kind))
            || kinds[..count].iter().enumerate().any(|(index, kind)| kinds[..index].contains(kind))
            || kinds[count..].iter().any(|kind| *kind != u32::MAX)
        { return None; }
        Some(ActorPlan { first: 2, count, kinds, partners: false })
    }

    #[cfg(feature = "source-capacity-group-probe")]
    pub(super) fn cleanup_mask(life: &Life) -> u8 {
        life.link.group_cleanup_valid
    }

    // Authority is a current native entry member or x20 at its verified native
    // destructor. Partner links are numbers only and are never followed.
    #[cfg(feature = "source-capacity-group-probe")]
    unsafe fn actor_record(actor: *const u8) -> Option<ActorRecord> {
        if !aligned(actor)
            || read::<usize>(actor, 0) != BASE.load(Ordering::Acquire) + 0x4f85d30
        {
            return None;
        }
        let control = read::<*const u8>(actor, 0x58);
        if !aligned(control) {
            return None;
        }
        Some(ActorRecord {
            actor: actor as usize,
            control: control as usize,
            partner: read(actor, 0x60),
            owner: read(actor, 8),
            cleanup_owner: read(actor, 0x6c),
            kind: read(actor, 0x74),
            flags: [read(actor, 0x68), read(actor, 0x69), read(actor, 0x70)],
            control_actor: read(control, 8),
            input_index: read(control, 0x160),
            binding: read(control, 0x168),
            alias_index: read(control, 0xc50),
            partner_control: read(control, 0x980),
        })
    }

    #[cfg(feature = "source-capacity-group-probe")]
    unsafe fn group_descriptor(entry: *const u8) -> bool {
        let forms = super::super::Forms::starting(TARGET);
        actor_plan().is_some()
            && read::<u32>(entry, 0x14) == forms.count
            && read::<u32>(entry, 0x28) == forms.actor_count
            && [0usize, 1, 2].map(|i| read::<u32>(entry, 0x18 + i * 4)) == forms.kinds
            && read::<u8>(entry, 0x591f) == forms.flags
    }

    #[cfg(feature = "source-capacity-group-probe")]
    unsafe fn group_actor_observation(entry: *const u8) -> Option<ActorSet> {
        if native_source::entry(SLOT as u32) != Some(entry) || !group_descriptor(entry) {
            return None;
        }
        let plan = actor_plan()?;
        let pointers: [usize; 5] =
            [0x4178, 0x4180, 0x4188, 0x4190, 0x4198].map(|offset| read(entry, offset));
        let actors = std::array::from_fn(|i| {
            if i < plan.count { actor_record(pointers[plan.first + i] as *const u8) }
            else { None }
        });
        if native_source::entry(SLOT as u32) != Some(entry)
            || !group_descriptor(entry)
            || [0x4178, 0x4180, 0x4188, 0x4190, 0x4198]
                .map(|offset| read::<usize>(entry, offset)) != pointers
        { return None; }
        Some(ActorSet { pointers, actors })
    }

    #[cfg(feature = "source-capacity-group-probe")]
    fn actor_fields_valid(actor: ActorRecord, index: usize, input: Inputs, plan: ActorPlan) -> bool {
        let route = SLOT + if plan.partners { index * 8 } else { 0 };
        actor.owner == SLOT as u32 && actor.cleanup_owner == SLOT as u32
            && actor.kind == plan.kinds[index] && actor.flags == [0, 1, 0]
            && actor.control_actor == actor.actor
            && actor.input_index == route as i32 && actor.alias_index == route as i32
            && actor.binding == input.records + route * 0x280
    }

    // Native658af0 runs 655320 and then 64f820. For an alternate bank, 64f820
    // tail-calls 64fb70, which aliases the current form's member at +4178 and
    // calls 313b00(service, route7, entry+34, null, -1). That stores only route
    // 7's state byte; each member destructor's 313e90(7) restores state 2.
    #[cfg(feature = "source-capacity-group-probe")]
    unsafe fn activated_member(entry: *const u8, plan: ActorPlan) -> Option<usize> {
        let form = read::<u8>(entry, 0x5918) as usize;
        if plan.partners || form > 2 || !group_descriptor(entry) {
            return None;
        }
        let kind = read::<u32>(entry, 0x18 + form * 4);
        let index = plan.kinds[..plan.count].iter().position(|member| *member == kind)?;
        let member = read::<usize>(entry, 0x4178 + (plan.first + index) * 8);
        (member != 0).then_some(member)
    }

    /// The preflight record after only 64fb70's native route-state store.
    #[cfg(feature = "source-capacity-group-probe")]
    unsafe fn activated_input(entry: *const u8, before: Inputs) -> Option<Inputs> {
        let member = activated_member(entry, actor_plan()?)?;
        let route = before.slots[SLOT];
        if read::<usize>(entry, 0x4178) != member
            || read::<usize>(entry, 0x4180) != 0
            || read::<u8>(entry, 0x34) != 0
            || (route.pointer, route.state, route.key, route.owner, route.alias) != (0, 2, -1, -1, -1)
        {
            return None;
        }
        let mut after = before;
        after.slots[SLOT].state = 0;
        Some(after)
    }

    /// Some(false): exact preflight input. Some(true): exact native activation delta.
    #[cfg(feature = "source-capacity-group-probe")]
    unsafe fn input_stage(entry: *const u8, before: Inputs, current: Inputs) -> Option<bool> {
        if current == before {
            return Some(false);
        }
        (activated_input(entry, before) == Some(current)).then_some(true)
    }

    /// Compare every other snapshot field with creation; no other delta is adopted.
    #[cfg(feature = "source-capacity-group-probe")]
    unsafe fn creation_view(mut current: Snapshot, life: Life, entry: *const u8) -> Snapshot {
        let Some(before) = life.link.input else {
            return current;
        };
        let after = activated_input(entry, before);
        if let Some(fields) = current.ldn.as_mut().filter(|fields| after.is_some() && fields.input == after) {
            fields.input = Some(before);
        }
        current
    }

    #[cfg(feature = "source-capacity-group-probe")]
    unsafe fn actor_set(entry: *const u8) -> Option<ActorSet> {
        let input = inputs().ok()?;
        let observed = group_actor_observation(entry)?;
        let plan = actor_plan()?;
        let information = read::<*const u8>(entry, 0xf8);
        for (index, record) in observed.actors.iter().copied().enumerate().take(plan.count) {
            let record = record?;
            if !actor_fields_valid(record, index, input, plan)
                || observed.actors[..index].iter().flatten().any(|previous|
                    previous.actor == record.actor || previous.control == record.control)
            { return None; }
            if plan.partners {
                let first = observed.actors[0]?;
                let second = observed.actors[1]?;
                if first.partner != 0 || second.partner != first.actor
                    || first.partner_control != second.control || second.partner_control != 0
                { return None; }
            } else if record.partner != 0 || record.partner_control != 0 { return None; }
        }
        // Construction leaves the non-owning slots zero; an alternate bank's 64fb70
        // then aliases exactly the current form's member at +4178. Never follow it.
        let alias = if plan.partners { 0 } else { activated_member(entry, plan)? };
        let non_owning = [alias, 0, 0, 0, 0];
        if observed.pointers.iter().zip(non_owning).enumerate().any(|(index, (pointer, expected))|
            !(plan.first..plan.first + plan.count).contains(&index) && *pointer != expected)
            || !aligned(information) || read::<u8>(information, 0x92) != 0
            || inputs() != Ok(input)
        { return None; }
        Some(observed)
    }

    #[cfg(feature = "source-capacity-group-probe")]
    unsafe fn group_actor_cleanup(ctx: &InlineCtx) {
        let actor = ctx.registers[20].x() as usize;
        let observed = STATE.lock().unwrap().life;
        let Some(life) = observed.filter(|life| requested(life.before.layout)
            && matches!(life.link.phase, Phase::Loading | Phase::Ready | Phase::Retired)
            && !life.entry_destroyed && !life.owner_retired && !life.reconstruction_done)
        else { return; };
        let Some(current) = native_source::entry(SLOT as u32) else { return; };
        if current as usize != life.entry || !group_descriptor(current) { return; }
        let Some(plan) = actor_plan() else { return; };
        let offsets = [0x4178, 0x4180, 0x4188, 0x4190, 0x4198];
        let pointers: [usize; 3] = std::array::from_fn(|index|
            if index < plan.count { read(current, offsets[plan.first + index]) } else { 0 });
        let Some(index) = pointers[..plan.count].iter().position(|pointer| *pointer == actor && actor != 0)
        else { return; };
        let bit = 1u8 << index;
        if life.link.group_cleanup_seen & bit != 0 { return; }
        let record = actor_record(actor as *const u8);
        let input = inputs();
        let returned = ctx.registers[30].x() as usize == BASE.load(Ordering::Acquire) + 0x6b79a0;
        let resource_associated = native_source::resource_owner(SLOT as u32)
            .map(|resource| resource as usize) == Some(life.resource);
        let expected = life.link.group.and_then(|set| set.actors[index]);
        let identity = record.is_some_and(|record| {
            input.is_ok_and(|input| actor_fields_valid(record, index, input, plan))
                && expected.is_none_or(|expected| expected == record)
                && if !plan.partners { record.partner == 0 && record.partner_control == 0 }
                   else if index == 0 { record.partner == 0 }
                   else { life.link.group_cleanup[0].is_some_and(|first|
                       record.partner == first.actor && first.partner_control == record.control) }
        });
        let valid = identity && returned && resource_associated
            && input.is_ok_and(|input| input.unclaimed() && life.link.input.is_some_and(|before|
                input.service == before.service && input.data == before.data && input.records == before.records));
        // Native651660 clears each pointer only after its destructor returns.
        // Reacquire current authority; never follow the saved first actor/partner.
        if native_source::entry(SLOT as u32) != Some(current)
            || read::<usize>(current, offsets[plan.first + index]) != actor
        { return; }
        let recorded = {
            let mut state = STATE.lock().unwrap();
            if let Some(saved) = state.life.as_mut().filter(|saved|
                saved.entry == life.entry && saved.resource == life.resource
                && saved.link.phase == life.link.phase && !saved.entry_destroyed
                && !saved.owner_retired && !saved.reconstruction_done
                && saved.link.group_cleanup_seen == life.link.group_cleanup_seen)
            {
                saved.link.group_cleanup[index] = record;
                saved.link.group_cleanup_seen |= bit;
                if valid { saved.link.group_cleanup_valid |= bit; }
                saved.link.actor_cleanup_seen = saved.link.group_cleanup_seen == expected_actor_mask();
                saved.link.actor_cleanup = saved.link.group_cleanup_valid == expected_actor_mask();
                saved.failed |= !valid;
                true
            } else { false }
        };
        if recorded {
            load_trace!("actor cleanup index={} actor=0x{:x} phase={:?} pending_install={:?} record={:x?} identity={} resource_associated={} native_return={} valid={} input={:x?}",
                index, actor, life.link.phase, life.link.install, record,
                identity, resource_associated, returned, valid, input);
        }
    }

    const LOAD_DIAGNOSTIC: u16 = 900;
    static ENABLED: AtomicBool = AtomicBool::new(false);

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Phase {
        Waiting,
        Loading,
        Ready,
        Declined,
        Retired,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Typed {
        registry: usize,
        users: u32,
        parameters: [usize; 2],
        #[cfg(feature = "source-capacity-group-probe")]
        members: [Option<TypedMember>; 3],
    }

    #[cfg(feature = "source-capacity-group-probe")]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct TypedMember {
        kind: u32,
        key: u32,
        users: u32,
        parameters: [usize; 2],
    }

    impl Typed {
        fn unused(self) -> bool {
            #[cfg(feature = "source-capacity-group-probe")]
            if self.members.iter().flatten().any(|member| member.users != 0) { return false; }
            self.users == 0
        }
    }

    #[derive(Clone, Copy)]
    pub(super) struct Load {
        phase: Phase,
        install: Option<InstallToken>,
        input: Option<Inputs>,
        typed: Option<Typed>,
        resources: Option<[(u32, u32); 4]>,
        descriptor: [u32; 28],
        stable: u16,
        ticks: u16,
        quiet: u16,
        last_phase: Option<[u8; 3]>,
        phase_logs: u8,
        actor: usize,
        actor_cleanup_seen: bool,
        actor_cleanup: bool,
        #[cfg(feature = "source-capacity-group-probe")]
        group: Option<ActorSet>,
        #[cfg(feature = "source-capacity-group-probe")]
        group_cleanup: [Option<ActorRecord>; 3],
        #[cfg(feature = "source-capacity-group-probe")]
        group_cleanup_valid: u8,
        #[cfg(feature = "source-capacity-group-probe")]
        group_cleanup_seen: u8,
        #[cfg(feature = "source-capacity-group-probe")]
        input_activated: bool,
    }

    impl Load {
        pub(super) const fn new() -> Self {
            Self {
                phase: Phase::Waiting,
                install: None,
                input: None,
                typed: None,
                resources: None,
                descriptor: [0; 28],
                stable: 0,
                ticks: 0,
                quiet: 0,
                last_phase: None,
                phase_logs: 0,
                actor: 0,
                actor_cleanup_seen: false,
                actor_cleanup: false,
                #[cfg(feature = "source-capacity-group-probe")]
                group: None,
                #[cfg(feature = "source-capacity-group-probe")]
                group_cleanup: [None; 3],
                #[cfg(feature = "source-capacity-group-probe")]
                group_cleanup_valid: 0,
                #[cfg(feature = "source-capacity-group-probe")]
                group_cleanup_seen: 0,
                #[cfg(feature = "source-capacity-group-probe")]
                input_activated: false,
            }
        }
    }

    /// Read only from the current native roots, at the verified entry callback
    /// or exact actor-destructor boundary. Saved numeric identities are not read.
    unsafe fn typed() -> Option<Typed> {
        let registry = read::<*const u8>(BASE.load(Ordering::Acquire) as *const u8, 0x52bc3b0);
        if !aligned(registry) {
            return None;
        }
        let record = registry.add(0x60 + TARGET as usize * 0x38);
        #[cfg(feature = "source-capacity-group-probe")]
        let members = {
            let forms = super::super::Forms::starting(TARGET);
            let mut members = [None; 3];
            for (index, member) in members.iter_mut().enumerate().take(forms.count as usize) {
                let kind = forms.kinds[index];
                let key = training_mod_consts::character_forms::typed_record_kind(TARGET, kind)?;
                let record = registry.add(0x60 + key as usize * 0x38);
                *member = Some(TypedMember { kind, key, users: read(record, 0),
                    parameters: [read(record, 0x10), read(record, 0x18)] });
            }
            members
        };
        Some(Typed {
            registry: registry as usize,
            users: read(record, 0),
            parameters: [read(record, 0x10), read(record, 0x18)],
            #[cfg(feature = "source-capacity-group-probe")]
            members,
        })
    }

    unsafe fn descriptor(entry: *const u8) -> [u32; 28] {
        std::array::from_fn(|i| read(entry, i * 4))
    }

    fn save(entry: usize, load: Load, failed: bool) {
        let mut state = STATE.lock().unwrap();
        if let Some(life) = state
            .life
            .as_mut()
            .filter(|life| life.entry == entry && !life.entry_destroyed)
        {
            life.link = load;
            life.failed |= failed;
        }
    }

    fn decline(life: Life, reason: &str) {
        let mut load = life.link;
        load.phase = Phase::Declined;
        save(life.entry, load, true);
        load_trace!(
            "declined before install: {}; no descriptor/input/CSS writes",
            reason
        );
    }

    unsafe fn scene_unchanged(life: Life, current: Snapshot, loading: bool) -> bool {
        let Some(mut expected) = life.after else {
            return false;
        };
        // Native body construction registers additional callbacks. Alias7 may
        // transiently clear during loading; owning roots must never migrate.
        expected.listeners = current.listeners;
        if loading {
            expected.aliases[SLOT] = current.aliases[SLOT];
        }
        expected == current
            && current.primary[SLOT] == life.entry
            && current.resources[SLOT] == life.resource
            && (loading || current.aliases[SLOT] == life.resource)
            && life.link.resources.is_some()
            && original_resources(current) == life.link.resources
    }

    unsafe fn ready(life: Life, current: Snapshot, entry: *const u8) -> bool {
        #[cfg(feature = "source-capacity-group-probe")]
        let current = creation_view(current, life, entry);
        if !scene_unchanged(life, current, false)
            || read::<usize>(current.resources[SLOT] as *const u8, 0)
                != BASE.load(Ordering::Acquire) + 0x509fb10
            || !native_source::parked_ready(entry, SLOT as u32, TARGET)
        {
            return false;
        }
        let Ok(input) = inputs() else {
            return false;
        };
        let Some((_, binding)) = actor(entry) else {
            return false;
        };
        #[cfg(feature = "source-capacity-group-probe")]
        if input.cpu_flag != 1 || actor_set(entry).is_none_or(|set|
            life.link.group.is_some_and(|before| before != set))
        {
            return false;
        }
        #[cfg(not(feature = "source-capacity-group-probe"))]
        let expected_input = life.link.input == Some(input);
        // An alternate bank is ready only after its exact native activation delta.
        #[cfg(feature = "source-capacity-group-probe")]
        let expected_input = actor_plan().is_some_and(|plan| {
            life.link.input.and_then(|before| input_stage(entry, before, input)) == Some(!plan.partners)
        });
        // Native inactive actors borrow their dedicated records without claiming them.
        input.unclaimed()
            && expected_input
            && binding == input.records + SLOT * 0x280
            && BODY_CALLS.load(Ordering::Acquire) == 0
            && native_source::ready(entry, SLOT as u32, TARGET).is_some()
    }

    pub(super) unsafe fn service(
        ctx: &ServiceEntryContext,
        entry: *const u8,
        owner: u32,
        life: Life,
    ) -> bool {
        if owner != SLOT as u32 || entry as usize != life.entry || life.entry_destroyed {
            return false;
        }
        match life.link.phase {
            Phase::Waiting => {
                if life.departed || life.calls != QUIET_CALLBACKS {
                    return false;
                }
                if life.failed || !ENABLED.load(Ordering::Acquire) {
                    decline(
                        life,
                        if cfg!(feature = "source-capacity-group-probe") {
                            "empty lifetime failed or load fingerprints unavailable"
                        } else {
                            "empty lifetime failed or Link fingerprints unavailable"
                        },
                    );
                    return true;
                }
                start(ctx, entry, life);
                true
            }
            Phase::Loading => {
                loading(ctx, entry, life);
                true
            }
            Phase::Ready => {
                #[cfg(any(feature = "source-ldn-capacity-link-probe", feature = "source-capacity-group-probe"))]
                if (cfg!(feature = "source-capacity-group-probe") || life.before.layout == Layout::LdnWaiting)
                    && life.link.quiet == QUIET_CALLBACKS {
                    return false; // Bounded LDN observation; no perpetual full snapshots.
                }
                if life.departed || !permitted(life) {
                    return false;
                }
                let current = current_snapshot(ctx, entry, life);
                let valid = !INSTALL.active()
                    && plugin_idle()
                    && current.is_some_and(|current| ready(life, current, entry));
                let mut load = life.link;
                load.quiet = load.quiet.saturating_add(1).min(QUIET_CALLBACKS);
                if matches!(load.quiet, 1 | 30)
                    || (load.quiet == QUIET_CALLBACKS && life.link.quiet != QUIET_CALLBACKS)
                    || (!valid && !life.failed)
                {
                    load_trace!("ready quiet={} valid={} body_frames={} typed={:x?} actor={:x?} input={:x?}",
                        load.quiet, valid, BODY_CALLS.load(Ordering::Acquire), typed(), actor(entry), inputs());
                }
                save(life.entry, load, !valid);
                true
            }
            Phase::Declined | Phase::Retired => false,
        }
    }

    unsafe fn start(ctx: &ServiceEntryContext, entry: *const u8, life: Life) {
        // No plugin mutex is held across these observations or a native call.
        if !permitted(life) || INSTALL.active() || !plugin_idle() {
            decline(life, "scene/UI or competing plugin work");
            return;
        }
        let Some(current) = current_snapshot(ctx, entry, life) else {
            decline(life, "current callback/native identities unavailable");
            return;
        };
        if life.after != Some(current)
            || empty_candidate(&current).is_none()
            || current.original.iter().any(|e| e.kinds.contains(&TARGET))
            || (cfg!(feature = "source-capacity-group-probe")
                && current.original.iter().any(|e| e.kinds.iter().any(|kind|
                    training_mod_consts::character_forms::is_source_member(TARGET, *kind))))
            || descriptor(entry)[0x34 / 4] != 0
        {
            decline(
                life,
                if cfg!(feature = "source-capacity-group-probe") {
                    "empty candidate changed, non-default role or target group already present"
                } else {
                    "empty candidate changed, non-default role or Link already present"
                },
            );
            return;
        }
        let input = match inputs() {
            Ok(input) => input,
            Err(observation) => {
                load_trace!(
                    "input preflight unavailable(reason,current values)={:x?}",
                    observation
                );
                decline(life, observation.0);
                return;
            }
        };
        let parameters = typed();
        let resources = original_resources(current);
        if !input.unclaimed()
            || (cfg!(feature = "source-capacity-group-probe") && input.cpu_flag != 1)
            || parameters.is_none_or(|typed| !typed.unused())
            || resources.is_none()
        {
            load_trace!(
                "rejected input={:x?} typed={:x?} original_resources={:x?}",
                input,
                parameters,
                resources
            );
            decline(life, if cfg!(feature = "source-capacity-group-probe") {
                "input7/15 is claimed, target typed users are not zero, or original resource identity is unavailable"
            } else {
                "input7/15 is claimed, Link typed users are not zero, or original resource identity is unavailable"
            });
            return;
        }
        let mut load = life.link;
        let desc = descriptor(entry);
        if load.stable != 0
            && (load.input != Some(input)
                || load.typed != parameters
                || load.descriptor != desc
                || load.resources != resources)
        {
            load_trace!(
                "changed preflight input={:x?} typed={:x?}",
                input,
                parameters
            );
            decline(
                life,
                "input/typed/descriptor identity changed during 30-callback preflight",
            );
            return;
        }
        load.input = Some(input);
        load.typed = parameters;
        load.resources = resources;
        load.descriptor = desc;
        load.stable += 1;
        if load.stable == 1 {
            load_trace!(
                "preflight input={:x?} typed={:x?} descriptor={:x?}",
                input,
                parameters,
                desc
            );
        }
        save(life.entry, load, false);
        if load.stable < STABLE_CALLBACKS {
            return;
        }
        let Some(token) = INSTALL.try_begin(false) else {
            decline(life, "shared mutation gate unavailable");
            return;
        };
        let available: unsafe extern "C" fn(*const u8) -> bool =
            std::mem::transmute(BASE.load(Ordering::Acquire) + 0x17e4700);
        let valid = permitted(life)
            && plugin_idle()
            && current_snapshot(ctx, entry, life)
                .is_some_and(|now| now == current && empty_candidate(&now).is_some())
            && inputs() == Ok(input)
            && typed() == parameters
            && original_resources(current) == resources
            && descriptor(entry) == desc
            && available(current.resources[SLOT] as *const u8);
        let consumed = {
            let mut state = STATE.lock().unwrap();
            match state.life.as_mut() {
                Some(saved)
                    if valid
                        && saved.entry == life.entry
                        && !saved.departed
                        && !saved.entry_destroyed
                        && !saved.failed
                        && saved.link.phase == Phase::Waiting =>
                {
                    load.phase = Phase::Loading;
                    load.install = Some(token);
                    saved.link = load; // Consume before calling native code.
                    true
                }
                _ => false,
            }
        };
        if !consumed {
            finish_install(token, "capacity Link preflight declined");
            decline(
                life,
                "final current identity/availability revalidation failed",
            );
            return;
        }
        load_trace!("invoking ONE {} install entry={:p} layout={:?} token={:?}; native descriptor preserved; flags000",
            TARGET_LABEL, entry, life.before.layout, token);
        native_source::install(entry.cast_mut(), TARGET);
        // The token remains owned until actual native completion or destruction.
    }

    unsafe fn loading(ctx: &ServiceEntryContext, entry: *const u8, life: Life) {
        let mut load = life.link;
        load.ticks = load.ticks.saturating_add(1);
        if super::super::native_source_capacity::dispatch_identity(ctx, entry).is_none()
            || native_source::entry(SLOT as u32) != Some(entry)
        {
            if load.ticks == 1 || load.ticks == LOAD_DIAGNOSTIC {
                load_trace!("pending callback identity unavailable ticks={}; install retained", load.ticks);
            }
            save(life.entry, load, true);
            return;
        }
        let phase = [
            read(entry, 0x5920),
            read(entry, 0x591c),
            read(entry, 0x5935),
        ];
        if load.last_phase != Some(phase) && load.phase_logs < 16 {
            load.last_phase = Some(phase);
            load.phase_logs += 1;
            load_trace!(
                "loading ticks={} native={:?} typed={:x?}",
                load.ticks,
                phase,
                typed()
            );
        }
        let done = native_source::parked_ready(entry, SLOT as u32, TARGET);
        let current = current_snapshot(ctx, entry, life);
        let mut observed = life;
        observed.link = load;
        #[cfg(feature = "source-capacity-group-probe")]
        let group = done.then(|| actor_set(entry)).flatten();
        #[cfg(feature = "source-capacity-group-probe")]
        { observed.link.group = group; }
        let usable = done
            && !life.departed
            && permitted(life)
            && plugin_idle()
            && current.is_some_and(|current| ready(observed, current, entry));
        #[cfg(feature = "source-capacity-group-probe")]
        let usable = usable && group.is_some();
        let input = (!life.departed).then(|| inputs());
        #[cfg(not(feature = "source-capacity-group-probe"))]
        let (input_valid, compared) = (input.is_some_and(|input| input.ok() == life.link.input), current);
        #[cfg(feature = "source-capacity-group-probe")]
        let (input_valid, compared) = {
            let stage = input
                .and_then(Result::ok)
                .zip(life.link.input)
                .and_then(|(input, before)| input_stage(entry, before, input));
            // After activation, the preflight record again means a member released route7.
            let valid = stage.is_some_and(|activated| activated || !load.input_activated);
            if stage == Some(true) && !load.input_activated {
                load.input_activated = true;
                load_trace!("native alternate-bank activation ticks={} alias=0x{:x}: route7 state 2->0 only (64fb70/313b00, null control)",
                    load.ticks, read::<usize>(entry, 0x4178));
            }
            (valid, current.map(|current| creation_view(current, life, entry)))
        };
        let failed = !life.departed
            && (!compared.is_some_and(|current| scene_unchanged(life, current, true)) || !input_valid);
        if failed && !life.failed {
            load_trace!("loading lifetime invariant changed; native install retained; current={:x?} input={:x?}", current, input);
        }
        let mut finish = None;
        if usable && !failed {
            finish = load.install.take();
            load.phase = Phase::Ready;
            #[cfg(not(feature = "source-capacity-group-probe"))]
            { load.actor = actor(entry).map_or(0, |(actor, _)| actor); }
            #[cfg(feature = "source-capacity-group-probe")]
            {
                load.group = group;
                load.actor = group.and_then(|set| set.actors[0]).map_or(0, |record| record.actor);
                load_trace!("canonical ready target=0x{:x} actors={:x?}", TARGET, load.group);
            }
            load_trace!("native completed ticks={} usable={} departed={} actor=0x{:x} typed={:x?} input={:x?} descriptor={:x?}",
                load.ticks, usable, life.departed, load.actor, typed(), inputs(), descriptor(entry));
        } else if load.ticks == LOAD_DIAGNOSTIC {
            #[cfg(feature = "source-capacity-group-probe")]
            load_trace!("group stalled target=0x{:x} actors={:x?} input={:x?}; no timeout release",
                TARGET, group_actor_observation(entry), inputs());
            load_trace!("observation limit900 native_parked={} departed={}; full acceptance unproven, install retained until full readiness or actual destruction; no timeout unlock/removal/retry", done, life.departed);
        }
        save(life.entry, load, failed);
        if let Some(token) = finish {
            finish_install(token, "capacity Link native completion");
        }
    }

    pub(super) struct Destruction {
        actor: Option<usize>,
        typed: Option<Typed>,
    }

    /// Synchronous native entry destruction owns entry, even after departure.
    /// Collect current native facts before taking the observer mutex.
    pub(super) unsafe fn destruction_observation(entry: *const u8) -> Destruction {
        let actor = read::<*const u8>(entry, 0x4178);
        Destruction {
            actor: (aligned(actor)
                && read::<usize>(actor, 0) == BASE.load(Ordering::Acquire) + 0x4f85d30
                && read::<u32>(actor, 0x6c) == SLOT as u32).then_some(actor as usize),
            typed: typed(),
        }
    }

    pub(super) fn destroying(life: &mut Life, observation: Destruction) -> Option<InstallToken> {
        if let Some(actor) = observation.actor { life.link.actor = actor; }
        load_trace!("entry destruction phase={:?} ticks={} ready_quiet={} actor=0x{:x} typed={:x?} pending_install={:?}; native cleanup owns all objects",
            life.link.phase, life.link.ticks, life.link.quiet, life.link.actor, observation.typed,
            life.link.install);
        #[cfg(feature = "source-capacity-group-probe")]
        load_trace!("group entry destruction actors={:x?} cleanup={:x?} valid_mask=0x{:x}; pending token released only by actual entry destructor",
            life.link.group, life.link.group_cleanup, life.link.group_cleanup_valid);
        life.link.phase = Phase::Retired;
        life.link.install.take()
    }

    #[cfg(not(feature = "source-capacity-group-probe"))]
    fn pending_ldn(life: &Life) -> bool {
        #[cfg(feature = "source-ldn-capacity-link-probe")]
        { life.before.layout == Layout::LdnWaiting && life.link.phase == Phase::Loading
            && life.link.install.is_some() }
        #[cfg(not(feature = "source-ldn-capacity-link-probe"))]
        { let _ = life; false }
    }

    pub(super) unsafe fn actor_cleanup(ctx: &InlineCtx) {
        #[cfg(feature = "source-capacity-group-probe")]
        { group_actor_cleanup(ctx); }
        #[cfg(not(feature = "source-capacity-group-probe"))]
        ordinary_actor_cleanup(ctx);
    }

    #[cfg(not(feature = "source-capacity-group-probe"))]
    unsafe fn ordinary_actor_cleanup(ctx: &InlineCtx) {
        let actor = ctx.registers[20].x() as usize;
        let observed = STATE.lock().unwrap().life;
        let Some(life) = observed.filter(|life| {
            ((matches!(life.link.phase, Phase::Ready | Phase::Retired)
                && life.link.actor != 0 && life.link.actor == actor)
                || (pending_ldn(life) && (life.link.actor == 0 || life.link.actor == actor)))
                && !life.link.actor_cleanup_seen
                && !life.owner_retired
                && !life.reconstruction_done
        }) else {
            return;
        };
        let base = BASE.load(Ordering::Acquire);
        // Native manager cleanup releases the actor before entry destruction.
        // x20 is owned by this actor destructor; retained numeric identities
        // alone never authorize reading an old entry or actor allocation.
        let current_actor = actor as *const u8;
        let actor_identity = aligned(current_actor)
            && read::<usize>(current_actor, 0) == base + 0x4f85d30
            && read::<u32>(current_actor, 8) == SLOT as u32
            && read::<u32>(current_actor, 0x6c) == SLOT as u32
            && read::<u32>(current_actor, 0x74) == TARGET;
        // Before the entry destructor, native651660 still holds the actor in
        // current entry+4178. Reacquire that entry; never dereference life.entry.
        // After entry destruction starts, its manager slot may already be null.
        let entry_associated = life.entry_destroyed
            || native_source::entry(SLOT as u32).is_some_and(|current| {
                current as usize == life.entry && read::<usize>(current, 0x4178) == actor
            });
        // A pending actor may not yet have been recorded by full readiness.
        // Its association must come from the live native entry at this boundary.
        if pending_ldn(&life) && !entry_associated { return; }
        let input = inputs();
        // This instruction is also reachable when cleanup was skipped. The
        // preserved LR distinguishes an actual313e90 return from those branches.
        let returned = ctx.registers[30].x() as usize == base + 0x6b79a0;
        let valid = actor_identity
            && entry_associated
            && returned
            && input.is_ok_and(|input| {
                input.unclaimed()
                    && life.link.input.is_some_and(|before| {
                        input.service == before.service
                            && input.data == before.data
                            && input.records == before.records
                    })
            });
        let mut state = STATE.lock().unwrap();
        if let Some(saved) = state.life.as_mut().filter(|saved| {
            saved.entry == life.entry
                && (saved.link.actor == actor
                    || (pending_ldn(saved) && saved.link.actor == 0))
                && !saved.link.actor_cleanup_seen
                && !saved.owner_retired
                && !saved.reconstruction_done
        }) {
            saved.link.actor = actor; // Numeric identity from this exact native callback.
            saved.link.actor_cleanup_seen = true;
            saved.link.actor_cleanup = valid;
            saved.failed |= !valid;
            load_trace!("native actor input cleanup actor=0x{:x} entry_destroyed={} actor_identity={} entry_associated={} native_return={} valid={} phase={:?} pending_install={:?} current={:x?}",
                actor, life.entry_destroyed, actor_identity, entry_associated, returned, valid,
                life.link.phase, life.link.install, input);
        }
    }

    pub(super) unsafe fn reconstruction(life: Life) -> bool {
        let input = inputs();
        let parameters = typed();
        #[cfg(feature = "source-capacity-group-probe")]
        load_trace!("group reconstruction actors={:x?} cleanup={:x?} valid_mask=0x{:x}",
            life.link.group, life.link.group_cleanup, life.link.group_cleanup_valid);
        let valid = life.link.quiet == QUIET_CALLBACKS
            && life.link.actor_cleanup_seen
            && life.link.actor_cleanup
            && life.link.install.is_none()
            && input.is_ok_and(Inputs::unclaimed)
            && parameters.is_some_and(Typed::unused);
        #[cfg(feature = "source-capacity-group-probe")]
        let valid = valid && life.claim_released;
        load_trace!("reconstruction valid={} ready_quiet={} actor_cleanup={} pending_install={} typed={:x?} input={:x?}",
            valid, life.link.quiet, life.link.actor_cleanup, life.link.install.is_some(), parameters, input);
        valid
    }

    pub(super) unsafe fn install(_base: usize) {
        if !super::super::native_source_capacity::load_supported() {
            load_trace!("shared load contract unavailable; {} load disabled", TARGET_LABEL);
            return;
        }
        ENABLED.store(true, Ordering::Release);
        #[cfg(feature = "source-capacity-group-probe")]
        load_trace!("isolated canonical target=0x{:x}; actor_mask=0x{:x}, input7/15, no SourceBook/consumer admission; empty300 + preflight30 + ready300", TARGET, expected_actor_mask());
        #[cfg(feature = "source-ldn-capacity-link-probe")]
        load_trace!("separate LDN P2 opt-in enabled: ONE ordinary Link, no SourceBook/consumer admission; full-ready acceptance still requires ready300");
        load_trace!("explicit opt-in enabled: empty300 + input preflight30 + ONE {} load + ready300; no managed admission", TARGET_LABEL);
    }
}
