//! Default-off native Training saved-original same-class menu/cycle adapter. No diagnostic shots or chords.
//! Included in character_roster to share its current service callback and INSTALL ownership.
use super::*;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use training_mod_consts::mii::{
    inventory::{MiiClass, Snapshot},
    lifetime::{Configuration, Operation, Phase, Ticket},
    role_adapter::SavedRole,
    selection::{self, Action, Active, Origin, PlayerLive, PlayerOrigin, Role as MiiRole},
};
use training_mod_tui::mii_menu::{Context as MenuContext, MenuCapture, PendingSelection};
#[path = "mii_saved_native.rs"]
mod native;

static ENABLED: AtomicBool = AtomicBool::new(false);
static WORK: AtomicBool = AtomicBool::new(false);
static MENU_EPOCH: AtomicU64 = AtomicU64::new(1);
static OWNER_EPOCH: [AtomicU64; 2] = [AtomicU64::new(1), AtomicU64::new(1)];
pub(super) fn current_generation(owner: u32) -> u64 {
    OWNER_EPOCH[owner as usize].load(Ordering::Acquire)
}
struct Owner {
    entry: usize,
    state: SavedRole,
    cycled: bool,
}
#[derive(Clone, Copy)]
struct Track {
    entry: usize,
    epoch: u64,
    ticket: Ticket,
    token: InstallToken,
    after: Configuration,
    ticks: u32,
    saw_reload: bool,
    departed: bool,
}
#[derive(Clone, Copy)]
enum Intent {
    Select(Action),
    Cycle,
}
struct Alpha {
    epoch: [u64; 2],
    owner: [Option<Owner>; 2],
    track: [Option<Track>; 2],
    intent: [Option<Intent>; 2],
    catalog: Option<Arc<Snapshot>>,
    menu: MenuCapture,
}
static ALPHA: Mutex<Alpha> = Mutex::new(Alpha {
    epoch: [1; 2],
    owner: [None, None],
    track: [None, None],
    intent: [None, None],
    catalog: None,
    menu: MenuCapture::closed(),
});
fn role(i: usize) -> MiiRole {
    if i == 0 {
        MiiRole::Player
    } else {
        MiiRole::Cpu
    }
}
/// Enabled role adapters are the sole owners, including capture refusals. Feature-disabled
/// builds retain the existing fixture path; they never compete for an enabled role.
fn unified(i:usize)->bool {
    #[cfg(feature="mii-ordinary-player-alpha")]
    if i==0 {return super::mii_ordinary_player::enabled();}
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    if i==1 {return super::mii_ordinary_cpu::enabled();}
    let _=i;false
}
fn notice(reason: &str) {
    trace!("[MiiSavedAlpha] {reason}");
}

pub(super) fn init(base: usize) {
    // Generated from the same pristine row/applier/INSTALL receipts as fullconfig. No new hook.
    let valid = super::mii_saved_fingerprints::valid(base);
    ENABLED.store(valid, Ordering::Release);
    notice(if valid {
        "enabled; native Training saved-original same-class alpha only"
    } else {
        "disabled: native fingerprint mismatch"
    });
}

/// Cancel the previous opening before App reset. No catalog/native work here.
pub(crate) fn menu_open() -> u64 {
    let opening = MENU_EPOCH.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    super::mii_auto_init::cancel();
    let mut state = ALPHA.lock().unwrap();
    state.menu.cancel();
    refresh_work(&state);
    opening
}

/// Queue the catalog read after App reset. No file IO and no writer wait on this thread.
pub(crate) fn menu_ready(opening: u64) {
    if !ENABLED.load(Ordering::Acquire) || mii_backend::current().is_none() {
        return;
    }
    super::mii_catalog_request::request(training_mod_consts::mii::catalog_request::Kind::Menu {
        opening,
        owners: [current_generation(0), current_generation(1)],
    });
}

pub(super) fn accept_catalog(catalog: Arc<Snapshot>, opening: u64, owners: [u64; 2]) -> bool {
    if !crate::common::menu::quick_menu_active()
        || menu_epoch() != opening
        || current_generation(0) != owners[0]
        || current_generation(1) != owners[1]
    {
        return false;
    }
    let mut state = ALPHA.lock().unwrap();
    // Closing preserves the selection epoch. Recheck under the same lock as
    // menu_close so a completion that waited behind close cannot rearm capture.
    if !crate::common::menu::quick_menu_active()
        || menu_epoch() != opening
        || state.epoch != owners
    {
        return false;
    }
    #[cfg(feature="mii-ordinary-player-alpha")]
    super::mii_ordinary_player::catalog_ready(Arc::clone(&catalog), state.epoch[0], opening);
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    super::mii_ordinary_cpu::catalog_ready(Arc::clone(&catalog), state.epoch[1], opening);
    state.catalog = Some(catalog);
    state.menu.begin();
    refresh_work(&state);
    true
}
pub(crate) fn menu_epoch() -> u64 {
    MENU_EPOCH.load(Ordering::Acquire)
}

/// Called after releasing the App guard. P1 Origin produces no intent in the shared UI policy.
pub(crate) fn menu_close(pending: Option<PendingSelection>) {
    // Leave the opening epoch unchanged so a selection taken for this open still applies.
    super::mii_catalog_request::close_opening(menu_epoch());
    // Match the ordinary loader's request gate; menu capture itself is expected here.
    let another_action = lobby::native_ui_busy() || button_config::has_pending_request();
    let mut state = ALPHA.lock().unwrap();
    let validated = [state.menu.validated(MiiRole::Player), state.menu.validated(MiiRole::Cpu)];
    state.menu.cancel();
    if another_action {
        refresh_work(&state);
        drop(state);
        notice("menu-close request rejected: another operation owns practice");
        return;
    }
    refresh_work(&state);
    drop(state); // The ordinary router must not format notices while holding ALPHA.
    if let Some(pending) = pending.filter(|p| p.context.epoch == menu_epoch()) {
        for action in pending.actions {
            let i = usize::from(matches!(action, Action::ReloadCpu(_)));
            #[cfg(feature="mii-ordinary-player-alpha")]
            if i == 0 && super::mii_ordinary_player::menu_close(pending.context, action) {
                continue;
            }
            #[cfg(feature="mii-ordinary-cpu-alpha")]
            if i == 1 && super::mii_ordinary_cpu::menu_close(pending.context,action) {continue;}
            #[cfg(feature="mii-ordinary-cpu-alpha")]
            if super::mii_ordinary_cpu::writers_reserved(i,i as u16) {
                notice("saved Mii request refused: ordinary CPU retains its destination");continue;
            }
            if unified(i) || !validated[i] {
                continue;
            }
            let mut state = ALPHA.lock().unwrap();
            if pending.context.epoch != menu_epoch() {
                break;
            }
            if state.owner[i]
                .as_ref()
                .is_some_and(|o| o.state.phase() == Phase::Ready && (i == 1 || o.cycled))
            {
                state.intent[i] = Some(Intent::Select(action));
            }
            refresh_work(&state);
        }
    }
}

/// A captured saved-original same-class origin cannot leave for an unqualified ordinary→Mii return.
/// Its Player cycle is exactly slot0 → selected distinct saved Mii → slot0.
pub(super) fn intercept(roster_role: Role, selection: Selection) -> bool {
    if !ENABLED.load(Ordering::Acquire) {
        // The feature adds saved-Mii roster values, but a failed native contract must never
        // forward that value (or strand a native saved origin) through the ordinary loader.
        return original_kind(roster_role).is_some_and(|k| (0x48..=0x4a).contains(&k))
            || matches!(selection, Selection::Direct(f) if (0x48..=0x4a).contains(&f.kind()));
    }
    let i = usize::from(roster_role == Role::Cpu);
    if unified(i) {
        return original_kind(roster_role).is_some_and(|k|MiiClass::from_kind(k).is_some())
            || matches!(selection,Selection::Direct(f) if MiiClass::from_kind(f.kind()).is_some());
    }
    let mut state = ALPHA.lock().unwrap();
    let Some(owner) = state.owner[i].as_ref() else {
        let protected = state.owner.iter().any(Option::is_some)
            || original_kind(roster_role).is_some_and(|k| (0x48..=0x4a).contains(&k))
            || matches!(selection,Selection::Direct(f) if (0x48..=0x4a).contains(&f.kind()));
        drop(state);
        if protected {
            notice("saved Mii cycle unavailable until the current menu catalog/owner is qualified; other Mii transitions remain unsupported");
        }
        return protected;
    };
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    if super::mii_ordinary_cpu::writers_reserved(i,i as u16) {
        drop(state);notice("saved Mii cycle refused: ordinary CPU retains its destination");return true;
    }
    if i == 0
        && matches!(selection, Selection::Cycle)
        && owner.state.phase() == Phase::Ready
        && (owner.cycled
            || setting(&MENU)
                .player_roster
                .fighters()
                .iter()
                .any(|f| f.kind() == owner.state.original().canonical().class as u32))
    {
        state.intent[i] = Some(Intent::Cycle);
        WORK.store(true, Ordering::Release);
    } else {
        drop(state);
        notify(
            roster_role,
            "Saved Mii alpha supports the original/alternate round trip only".into(),
        );
        notice(
            "outbound character change refused: alpha preserves the saved-original same-class origin round trip",
        );
    }
    true
}

fn refresh_work(s: &Alpha) {
    WORK.store(
        s.menu.waiting()
            || s.intent.iter().any(Option::is_some)
            || s.track.iter().any(Option::is_some),
        Ordering::Release,
    );
}

fn context(s: &Alpha) -> MenuContext {
    let original = s.owner[0].as_ref().map_or(PlayerOrigin::NotMii, |o| {
        PlayerOrigin::Mii(Origin(o.state.original().canonical()))
    });
    let player = s.owner[0].as_ref().map_or(PlayerLive::NotMii, |o| {
        if o.cycled {
            let canonical = o.state.expected().canonical();
            PlayerLive::Cycled { class: canonical.class, canonical }
        } else {
            PlayerLive::Origin
        }
    });
    let cpu = s.owner[1].as_ref().map(|o| {
        let canonical = o.state.expected().canonical();
        (canonical.class, canonical)
    });
    MenuContext {
        epoch: menu_epoch(),
        original,
        active: Active { player, cpu },
    }
}

unsafe fn permitted(i:usize) -> bool {
    let _=i;
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    if super::mii_ordinary_cpu::writers_reserved(i,i as u16) {return false;}
    crate::common::is_native_training_mode()
        && crate::common::is_ready_go()
        && !menu::captures_input()
        && !lobby::native_ui_busy()
        && !button_config::has_pending_request()
        && !INSTALL.active()
        && !training::save_states::is_loading()
        && !training::recording_or_playback()
        && !crate::replay_preview::occupied()
}

pub(super) unsafe fn service(entry: *mut u8, owner: u32) -> bool {
    // Already-owned Training fallback work still receives its departure callback.
    let i=match owner {0=>0,1|4=>1,_=>return false};
    if !ENABLED.load(Ordering::Acquire) || !WORK.load(Ordering::Acquire) {
        return false;
    }
    service_work(entry, owner, i)
}

// Keep native snapshot temporaries off ordinary fighter callbacks. Checking WORK
// inside this larger body would still reserve its frame before the early return.
// Settlement, menu capture, and submission each return a by-value native snapshot.
// Those three stay separate so a reload tick does not reserve their frames.
#[inline(never)]
unsafe fn service_work(entry: *mut u8, owner: u32, i: usize) -> bool {
    #[cfg(feature = "console-swap-trace")]
    let _swap_stack = crate::practice_trace::scope(0x1080030);
    let track = ALPHA.lock().unwrap().track[i];
    if let Some(mut track) = track {
        if track.entry != entry as usize {
            return false;
        }
        track.ticks = track.ticks.saturating_add(1);
        track.saw_reload |= read::<u8>(entry, 0x5920) == 3;
        if track.saw_reload
            && ready_single_mii(entry, owner) == Some(track.after.canonical().class as u32)
            && read::<u8>(entry, 0x5918) == 0
        {
            return settle_saved_track(entry, owner, i, track);
        }
        if track.ticks == 600 {
            notice("native reload readiness pending; INSTALL retained until witnessed readiness or exact destructor");
        }
        let mut s = ALPHA.lock().unwrap();
        if let Some(current) = s.track[i].as_mut().filter(|t| t.token == track.token) {
            current.ticks = track.ticks;
            current.saw_reload = track.saw_reload;
        }
        return true;
    }
    continue_saved_service(entry, owner, i)
}

// Menu and intent temporaries stay off the reload-tick path above.
#[inline(never)]
unsafe fn continue_saved_service(entry: *mut u8, owner: u32, i: usize) -> bool {
    if mii_backend::current().and_then(|l|l.logical(owner))!=Some(i) {return false;}
    // At most one catalog/current capture per menu-open and ready owner. No idle-frame census.
    let capture = {
        let s = ALPHA.lock().unwrap();
        if s.menu.needs(role(i)) {
            s.catalog.as_ref().map(|c| {
                (
                    s.epoch[i],
                    menu_epoch(),
                    Arc::clone(c),
                    s.owner[i].as_ref().map(|o| o.state.expected()),
                )
            })
        } else {
            None
        }
    };
    if let Some((epoch, capture_menu_epoch, catalog, expected)) = capture {
        if let Some(stop) = finish_saved_capture(entry, owner, i, epoch, capture_menu_epoch, catalog, expected) {
            return stop;
        }
    }
    if unified(i) || !permitted(i)
        || native_source::training_cpu_quantity() != Some(1)
        || STATES.iter().any(|state| {
            let s = state.lock().unwrap();
            s.active.is_some() || s.pending.is_some()
        })
    {
        return false;
    }
    let request = {
        let s = ALPHA.lock().unwrap();
        s.intent[i].and_then(|intent| {
            let o = s.owner[i].as_ref()?;
            (o.entry == entry as usize && o.state.phase() == Phase::Ready).then(|| {
                (
                    s.epoch[i],
                    intent,
                    o.state.expected(),
                    o.cycled,
                    o.state.original(),
                    s.catalog.as_ref().map(Arc::clone),
                )
            })
        })
    };
    let Some((epoch, intent, expected, cycled, original, catalog)) = request else {
        return false;
    };
    if ready_single_mii(entry, owner) != Some(expected.canonical().class as u32) {
        return false;
    }
    let Some(catalog) = catalog else {
        return false;
    };
    submit_saved_intent(entry, owner, i, (epoch, intent, expected, cycled, original, catalog))
}

// By-value native::current lives here. The per-tick reload path returns before this call.
#[inline(never)]
unsafe fn settle_saved_track(entry: *mut u8, owner: u32, i: usize, track: Track) -> bool {
    // Native readiness ends serialization, separately from accepting our target bytes.
    // A departed entry still owns this current callback; do not publish its old state.
    let fresh = if track.departed {
        None
    } else {
        Some(native::current(entry, role(i), track.epoch, track.after))
    };
    let mut s = ALPHA.lock().unwrap();
    let Some(current) = s.track[i].filter(|t| {
        t.entry == track.entry && t.epoch == track.epoch && t.token == track.token
    }) else {
        return true;
    };
    // Scene departure advances the owner epoch but retains this exact old track.
    // Any other generation mismatch has no authority to settle it.
    if !current.departed && s.epoch[i] != track.epoch {
        return true;
    }
    let done = if let Some(o) = s.owner[i].as_mut() {
        if current.departed {
            let _ = o.state.settle_terminal(track.ticket);
            false
        } else {
            let operation = match fresh {
                Some(Ok(now)) => o.state.complete(track.ticket, now).ok(),
                _ => None,
            };
            if let Some(operation) = operation {
                o.cycled = operation == Operation::Apply;
                true
            } else {
                // A failed current read is terminal too. Keep original/expected and
                // intercept protection; settling a receipt never returns it to Ready.
                let _ = o.state.refuse(track.ticket);
                let _ = o.state.settle_terminal(track.ticket);
                false
            }
        }
    } else {
        false
    };
    s.track[i] = None;
    s.intent[i] = None;
    refresh_work(&s);
    let cycled = s.owner[i].as_ref().is_some_and(|o| o.cycled);
    drop(s);
    let reason = if current.departed {
        "departed saved Mii native reload ready"
    } else if done {
        "saved Mii alpha complete"
    } else {
        "saved Mii native ready; exact completion refused"
    };
    if done {
        #[cfg(feature="mii-hud-portrait-alpha")]
        publish_mii_presentation(entry, owner, track.epoch, track.after.canonical().class as u32,
            super::PortraitIntent::Exact(track.after));
        #[cfg(not(feature="mii-hud-portrait-alpha"))]
        publish_mii_presentation(entry, owner, track.epoch, track.after.canonical().class as u32);
    }
    let released = finish_install(track.token, reason);
    if done {
        if i == 0 {
            state_for(Role::Player).lock().unwrap().cursor = cycled.then_some(0);
        }
        notify(
            if i == 0 { Role::Player } else { Role::Cpu },
            if cycled {
                "Saved Mii configuration applied"
            } else {
                "Original saved Mii restored"
            }
            .into(),
        );
    }
    trace!("[MiiSavedAlpha] native_ready owner={owner} epoch={} ticks={} accepted={done} departed={} serialization_released={released} saved_files_written=false",track.epoch,track.ticks,current.departed);
    true
}

// native::current / native::original and the owned catalog copy live here.
// Some(false) is the previous early return; None continues to a saved intent.
#[inline(never)]
unsafe fn finish_saved_capture(
    entry: *mut u8,
    owner: u32,
    i: usize,
    epoch: u64,
    capture_menu_epoch: u64,
    catalog: Arc<Snapshot>,
    expected: Option<Configuration>,
) -> Option<bool> {
    // The preceding ordinary service may still be waiting for native readiness.
    #[cfg(feature="mii-ordinary-player-alpha")]
    if i == 0 && super::mii_ordinary_player::menu_capture_pending(entry) {
        return Some(false);
    }
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    if i == 1 && super::mii_ordinary_cpu::menu_capture_pending(entry) {return Some(false);}
    if !mii_backend::ready()
        || INSTALL.active()
        || STATES.iter().any(|state| {
            let s = state.lock().unwrap();
            s.active.is_some() || s.pending.is_some()
        })
    {
        return Some(false);
    }
    if MiiClass::from_kind(read::<u32>(entry, 0x18)).is_some() && ready_single_mii(entry, owner).is_none() {
        return Some(false);
    }
    #[cfg(feature="mii-ordinary-player-alpha")]
    let ordinary_owned=super::mii_ordinary_player::owns(entry as usize);
    #[cfg(not(feature="mii-ordinary-player-alpha"))]
    let ordinary_owned=false;
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    let ordinary_owned=ordinary_owned || super::mii_ordinary_cpu::owns(entry as usize);
    let result = if !unified(i) && !ordinary_owned && MiiClass::from_kind(read::<u32>(entry, 0x18)).is_some() {
        match expected {
            Some(c) => native::current(entry, role(i), epoch, c),
            None => native::original(entry, role(i), epoch, &catalog),
        }
    } else {
        Err("current original is not saved Mii")
    };
    let mut s = ALPHA.lock().unwrap();
    if s.epoch[i] != epoch || menu_epoch() != capture_menu_epoch || !s.menu.needs(role(i)) {
        return Some(false);
    }
    let mut valid = false;
    if expected.is_none() {
        if let Ok(current) = result {
            if let Ok(state) = SavedRole::capture(role(i), current) {
                s.owner[i] = Some(Owner {
                    entry: entry as usize,
                    state,
                    cycled: false,
                });
                valid = true;
            }
        }
    } else if let (Some(o), Ok(current)) = (s.owner[i].as_ref(), result) {
        valid = o.entry == entry as usize && o.state.menu_current(current).is_ok();
    }
    if !valid {
        s.intent[i] = None;
    }
    s.menu.finish(role(i), valid);
    #[cfg(feature="mii-ordinary-player-alpha")]
    let ordinary = super::mii_ordinary_player::player_context();
    #[cfg(not(feature="mii-ordinary-player-alpha"))]
    let ordinary = None;
    let mut captured_context = context(&s);
    if unified(0) && ordinary.is_none()
        && original_kind(Role::Player).is_some_and(|k|MiiClass::from_kind(k).is_some()) {
        captured_context.original=PlayerOrigin::Unresolved;
    }
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    let ordinary_cpu=super::mii_ordinary_cpu::menu_qualified();
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    let cpu_context=ordinary_cpu.then(super::mii_ordinary_cpu::cpu_context);
    #[cfg(not(feature="mii-ordinary-cpu-alpha"))]
    let cpu_context=None;
    // publish_mii_catalog takes an owned Snapshot. ALPHA keeps the Arc, so this
    // one clone is the menu publish, not a per-tick copy.
    let publish = s.menu.take_owned_context(captured_context,ordinary,cpu_context)
        .map(|context|((*catalog).clone(),context));
    refresh_work(&s);
    drop(s);
    if let Some((catalog, context)) = publish {
        // No G/state lock remains; App contains owned values only.
        let mut app = training_mod_sync::lock_write(&menu::QUICK_MENU_APP);
        // Check under App: an older capture may have completed before cancellation.
        let mounted=app.publish_mii_catalog(catalog, context, menu_epoch(), || {
            training_mod_sync::assign(&menu::MENU_RECEIVED_INPUT, true);
        });
        #[cfg(feature="mii-ordinary-cpu-alpha")]
        if mounted {app.qualify_ordinary_cpu_cycle(ordinary_cpu);}
        #[cfg(not(feature="mii-ordinary-cpu-alpha"))]
        let _=mounted;
    }
    None
}

// By-value native::current, native::apply, and the 0x70 descriptor live here.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
unsafe fn submit_saved_intent(
    entry: *mut u8,
    owner: u32,
    i: usize,
    request: (u64, Intent, Configuration, bool, Configuration, Arc<Snapshot>),
) -> bool {
    let (epoch, intent, expected, cycled, original, catalog) = request;
    let current = match native::current(entry, role(i), epoch, expected) {
        Ok(v) => v,
        Err(reason) => {
            let mut state = ALPHA.lock().unwrap();
            state.intent[i] = None;
            refresh_work(&state);
            drop(state);
            notice(reason);
            return false;
        }
    };
    let choice = match intent {
        Intent::Select(a) => Some(a),
        Intent::Cycle if cycled => None,
        Intent::Cycle => {
            let canonical = original.canonical();
            selection::resolve(
                &catalog,
                &menu::mii_preferences(),
                MiiRole::Player,
                canonical.class,
                PlayerOrigin::Mii(Origin(canonical)),
            )
            .map(Action::ReloadPlayer)
        }
    };
    let target = choice.and_then(|a| {
        let c = match a {
            Action::ReloadPlayer(c) | Action::ReloadCpu(c) => c,
        };
        catalog.configuration(c)
            .map(|v| (a, v))
    });
    // Recheck pending native UI/buttons immediately before acquiring mutation ownership.
    if !permitted(i) {
        return false;
    }
    // No native mutation without actual shared INSTALL ownership.
    let Some(token) = INSTALL.try_begin(true) else {
        return false;
    };
    let submitted = {
        let mut s = ALPHA.lock().unwrap();
        if s.epoch[i] != epoch || s.intent[i].is_none() {
            None
        } else {
            s.intent[i] = None;
            let o = s.owner[i].as_mut().unwrap();
            let transaction = if matches!(intent, Intent::Cycle) && cycled {
                o.state.restore(current)
            } else if let Some((action, target)) = target {
                o.state.select(epoch, current, action, target)
            } else {
                Err(training_mod_consts::mii::role_adapter::Error::Scope)
            };
            let submitted = transaction.ok().flatten().and_then(|tx| {
                o.state
                    .submit(tx.ticket(), current)
                    .ok()
                    .map(|commit| (tx, commit))
            });
            refresh_work(&s);
            submitted
        }
    };
    let Some((tx, commit)) = submitted else {
        finish_install(token, "saved Mii alpha refused before write");
        return false;
    };
    {
        let mut s = ALPHA.lock().unwrap();
        s.track[i] = Some(Track {
            entry: entry as usize,
            epoch,
            ticket: tx.ticket(),
            token,
            after: tx.after(),
            ticks: 0,
            saw_reload: false,
            departed: false,
        });
        refresh_work(&s);
    }
    if let Err(reason) = native::apply(entry, role(i), epoch, commit) {
        let mut s = ALPHA.lock().unwrap();
        if let Some(o) = s.owner[i].as_mut() {
            let _ = o.state.abandon_unwritten(tx.ticket());
        }
        s.track[i] = None;
        refresh_work(&s);
        drop(s);
        finish_install(token, "saved Mii alpha pre-write refusal");
        notice(reason);
        return false;
    }
    let descriptor = native_fighter_install::Descriptor::copy_from(entry);
    trace!("[MiiSavedAlpha] install owner={owner} epoch={epoch} operation={:?} kind={:#x} saved_files_written=false",tx.operation(),tx.after().canonical().class as u32);
    training::clear_session_state();
    native_fighter_install::install(
        BASE.load(Ordering::Acquire),
        entry,
        &descriptor,
        native_fighter_install::Flags::Reload,
    );
    true
}

pub(super) fn scene_changed() {
    #[cfg(feature="mii-ordinary-player-alpha")]
    super::mii_ordinary_player::scene_changed();
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    super::mii_ordinary_cpu::scene_changed();
    MENU_EPOCH.fetch_add(1, Ordering::AcqRel);
    let mut s = ALPHA.lock().unwrap();
    s.menu.cancel();
    s.catalog = None;
    s.intent = [None, None];
    for (i, owner_epoch) in OWNER_EPOCH.iter().enumerate() {
        s.epoch[i] = owner_epoch
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        if let Some(o) = s.owner[i].as_mut() {
            o.state.depart();
        }
        if let Some(t) = s.track[i].as_mut() {
            t.departed = true;
        } else {
            s.owner[i] = None;
        }
    }
    refresh_work(&s);
    drop(s);
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    super::mii_auto_init::reset();
    super::mii_catalog_request::retire();
}
pub(super) fn on_destroy(entry: *const u8, owner: u32) {
    let i=match owner {0=>0,1|4=>1,_=>return};
    let exact={let state=state_for(if i==0 {Role::Player}else{Role::Cpu}).lock().unwrap();
        state.observed.is_some_and(|(at,_)|at==entry as usize)
            || state.active.is_some_and(|a|a.entry==entry as usize)};
    let legacy={let s=ALPHA.lock().unwrap();s.owner[i].as_ref().is_some_and(|o|o.entry==entry as usize)
        || s.track[i].is_some_and(|t|t.entry==entry as usize)};
    #[cfg(feature="mii-ordinary-player-alpha")]
    let exact=exact || super::mii_ordinary_player::owns(entry as usize) || super::mii_ordinary_player::tracked(entry as usize);
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    let exact=exact || super::mii_ordinary_cpu::owns(entry as usize) || super::mii_ordinary_cpu::tracked(entry as usize);
    if !exact && !legacy {return;}
    MENU_EPOCH.fetch_add(1, Ordering::AcqRel);
    let token = {
        let mut s = ALPHA.lock().unwrap();
        s.epoch[i] = OWNER_EPOCH[i]
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        s.intent[i] = None;
        s.menu.cancel();
        if s.owner[i]
            .as_ref()
            .is_some_and(|o| o.entry == entry as usize)
        {
            s.owner[i] = None;
        }
        let token = s.track[i]
            .filter(|t| t.entry == entry as usize)
            .map(|t| t.token);
        if token.is_some() {
            s.track[i] = None;
        }
        refresh_work(&s);
        token
    };
    if let Some(token) = token {
        finish_install(token, "saved Mii alpha exact native destructor");
    }
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    super::mii_auto_init::reset();
    super::mii_catalog_request::retire();
    trace!(
        "[MiiSavedAlpha] destroyed owner={owner} entry={:#x} cleanup_writes=false",
        entry as usize
    );
}
