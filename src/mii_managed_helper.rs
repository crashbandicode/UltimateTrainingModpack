//! Helper2/3 adapter around shared selector transactions. SourceBook owns all leases/lifetimes.
use super::mii_ordinary_player::native::{self as provenance, check, helpers, Scalars};
use super::mii_selector_native::{self as native, PublicationTarget, Snapshot};
use super::{mii_saved_alpha, native_fighter_install, native_source, InstallToken, BASE, INSTALL};
use crate::source_lifetime::{LoadToken, SourceToken};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use training_mod_consts::mii::{
    inventory::MiiClass,
    lifetime::Configuration,
    ordinary_cpu as policy,
    ordinary_player::{Gate, Phase, Ticket},
    selection::{self, PlayerOrigin},
};
struct State {
    track: Option<Track>,
    next: u64,
    capture_wait_noted: bool,
}
static STATE: Mutex<State> = Mutex::new(State {
    track: None,
    next: 1,
    capture_wait_noted: false,
});
#[derive(Clone)]
struct Track {
    ticket: Ticket,
    gate: Gate,
    source: SourceToken,
    install: InstallToken,
    data: Arc<Prepared>,
}
pub(super) struct Prepared {
    before: Snapshot,
    descriptor: [u8; 0x70],
    configuration: Configuration,
    player_epoch: u64,
    hat_change: bool,
}
pub(super) fn origins_captured() -> bool {
    super::mii_ordinary_player::helper_origin_captured()
        && super::mii_ordinary_cpu::helper_origin_captured()
}
/// Ordinary helpers only need CPU capture when its original Mii copies must
/// survive. Actual Mii transactions still require both qualified role captures.
pub(super) fn admission_ready(mii_request: bool) -> bool {
    let original_cpu = super::original_kind(super::Role::Cpu);
    if policy::helper_captures_ready(mii_request, original_cpu, false, false) {
        return true;
    }
    let player = mii_request && super::mii_ordinary_player::helper_origin_captured();
    let cpu = super::mii_ordinary_cpu::helper_origin_captured();
    let ready = policy::helper_captures_ready(mii_request, original_cpu, player, cpu);
    let mut s = STATE.lock().unwrap();
    let report = !ready && !s.capture_wait_noted;
    s.capture_wait_noted = !ready;
    drop(s);
    if report {
        crate::logging::diagnostic(format_args!(
            "[MiiHelper] waiting for original capture mii_request={} original_cpu={:?} player={} cpu={}",
            mii_request, original_cpu, player, cpu));
    }
    ready
}
/// Once per queued request; no preference/cursor write and no native/file scan.
pub(super) fn configuration(role: usize, kind: u32) -> Result<Configuration, &'static str> {
    let class = MiiClass::from_kind(kind).ok_or("Mii source class absent")?;
    check(
        role < 2 && origins_captured(),
        "Wait for both original characters to be captured",
    )?;
    let catalog =
        super::mii_ordinary_cpu::replay_catalog().ok_or("Mii source catalog is not qualified")?;
    let role = if role == 0 {
        selection::Role::Player
    } else {
        selection::Role::Cpu
    };
    let choice = selection::resolve(
        &catalog,
        &super::menu::mii_preferences(),
        role,
        class,
        PlayerOrigin::NotMii,
    )
    .ok_or("No saved Mii configuration is available for this hat")?;
    let c = catalog
        .configuration(choice)
        .ok_or("Saved Mii source configuration is unavailable")?;
    let hats = super::mii_ordinary_cpu::helper_origin_hats()
        .ok_or("Captured helper hat tokens are unavailable")?;
    policy::helper_configuration(c, hats)
}
/// Captured origin hats are the before-value. Shared reload policy admits an
/// equal hat on every layout and a changed hat on Training. The live row,
/// fingerprints, and lease are still rechecked by prepare().
pub(super) fn slot_eligible(slot: u32, c: Configuration) -> bool {
    let Some(layout) = super::mii_backend::current() else { return false };
    let Some(hats) = super::mii_ordinary_cpu::helper_origin_hats() else { return false };
    policy::helper_slot_admission(layout, slot as usize, hats, c.tokens().hat)
}
pub(super) unsafe fn prepare(
    entry: usize,
    slot: u32,
    c: Configuration,
) -> Result<Prepared, &'static str> {
    check(
        super::mii_backend::current().is_some_and(|l|l.helper(slot as usize).is_some()) && origins_captured(),
        "helper original capture/slot unavailable",
    )?;
    check(
        STATE.lock().unwrap().track.is_none(),
        "helper transaction still retained",
    )?;
    native::target_rows(c)?;
    let row = BASE.load(Ordering::Acquire) + 0x52c5730 + slot as usize * 0x260;
    check(
        provenance::span(row, 0x260, false),
        "helper original row span",
    )?;
    let before = native::snapshot(
        entry,
        mii_saved_alpha::current_generation(1),
        crate::mii_native_access::rd::<i8>(row, 8) as u32,
        slot as usize,
        None,
    )?;
    if !policy::supported_kind(c.canonical().class as u32) {
        return Err("Mii class not qualified");
    }
    // The helper's own physical slot. Training is 2/3; waiting is 5/6.
    let hat_change = policy::headgear_reload(
        before.layout, slot as usize, before.scalars.e.hat, c.tokens().hat)?;
    if hat_change && !provenance::cpu_headgear_fingerprints() {
        return Err("helper headgear fingerprint mismatch");
    }
    let descriptor = policy::staged_descriptor_in(before.layout,slot as usize, before.descriptor, Some(c))
        .ok_or("helper selector unavailable")?;
    Ok(Prepared {
        before,
        descriptor,
        configuration: c,
        player_epoch: mii_saved_alpha::current_generation(0),
        hat_change,
    })
}
/// Source STATE held, no G/registry lock. Failure retains the load and never calls native INSTALL.
pub(super) fn bind(
    mut data: Prepared,
    load: LoadToken,
    install: InstallToken,
) -> Result<(), &'static str> {
    let source = load.source();
    let mut s = STATE.lock().unwrap();
    check(
        s.track.is_none()
            && INSTALL.owns(install)
            && source.slot() == data.before.binding.owner as usize
            && source.entry().0 == data.before.binding.entry as u64,
        "helper transaction binding changed",
    )?;
    check(
        data.before.binding.epoch == mii_saved_alpha::current_generation(1)
            && data.player_epoch == mii_saved_alpha::current_generation(0),
        "helper original generation changed",
    )?;
    let view = provenance::helper_snapshot_for(Some(source))?;
    for i in 0..2 {
        if Some(i) != view.ordinal(source.slot()) {
            check(
                view.slots[i] == data.before.helpers.slots[i]
                    && view.miis[i] == data.before.helpers.miis[i],
                "foreign source projection changed",
            )?;
        }
    }
    data.before.helpers = view;
    let ticket = Ticket {
        epoch: data.before.binding.epoch,
        entry: data.before.binding.entry,
        sequence: s.next,
    };
    let next = s.next.checked_add(1).ok_or("helper ticket exhausted")?;
    let gate = Gate::new(ticket).ok_or("helper ticket invalid")?;
    s.track = Some(Track {
        ticket,
        gate,
        source,
        install,
        data: Arc::new(data),
    });
    s.next = next;
    Ok(())
}
fn current(t: &Track) -> bool {
    INSTALL.owns(t.install)
        && t.ticket.epoch == mii_saved_alpha::current_generation(1)
        && t.data.player_epoch == mii_saved_alpha::current_generation(0)
}
fn target(t: &Track) -> PublicationTarget<'_> {
    PublicationTarget {
        before: &t.data.before,
        descriptor: &t.data.descriptor,
        payload: Some(t.data.configuration),
        scalars: Scalars::saved(t.data.configuration),
        source: Some(t.source),
        hat_change: t.data.hat_change,
        // Saved helper payload always targets the slot record. This publisher
        // does not restore; the pre-request mirror is not the write address.
        original_index: t.data.before.index,
    }
}
pub(super) unsafe fn install(source: SourceToken) -> bool {
    let descriptor = {
        let s = STATE.lock().unwrap();
        let Some(t) = s
            .track
            .as_ref()
            .filter(|t| t.source == source && current(t))
        else {
            return false;
        };
        t.data.descriptor
    };
    let staged = native_fighter_install::Descriptor::from_bytes(&descriptor);
    native_fighter_install::install(
        BASE.load(Ordering::Acquire),
        source.entry().0 as *mut u8,
        &staged,
        native_fighter_install::Flags::Parked,
    );
    true
}
pub(super) fn tracked(entry: usize) -> bool {
    STATE
        .lock()
        .unwrap()
        .track
        .as_ref()
        .is_some_and(|t| t.ticket.entry == entry)
}
pub(super) unsafe fn enter_native(entry: *mut u8) -> bool {
    let mut s = STATE.lock().unwrap();
    let Some(t) = s
        .track
        .as_mut()
        .filter(|t| t.ticket.entry == entry as usize)
    else {
        return true;
    };
    if !current(t) {
        t.gate.refuse();
        return false;
    }
    t.gate.enter(
        t.ticket,
        crate::mii_native_access::rd(entry as usize, 0x591c),
    )
}
pub(super) unsafe fn leave_native(entry: *mut u8) {
    let mut s = STATE.lock().unwrap();
    if let Some(t) = s
        .track
        .as_mut()
        .filter(|t| t.ticket.entry == entry as usize)
    {
        t.gate.leave(
            t.ticket,
            crate::mii_native_access::rd(entry as usize, 0x591c),
        );
    }
}
pub(super) fn refuse(entry: usize, reason: &str) {
    let mut s = STATE.lock().unwrap();
    if let Some(t) = s.track.as_mut().filter(|t| t.ticket.entry == entry) {
        t.gate.refuse();
        drop(s);
        crate::logging::diagnostic(format_args!(
            "[MiiHelper] refused entry={:#x} reason={}",
            entry, reason
        ));
    }
}
pub(super) unsafe fn prequeue(entry: usize, selected: usize) -> bool {
    let track = {
        let s = STATE.lock().unwrap();
        let Some(t) = s.track.as_ref().filter(|t| t.ticket.entry == entry) else {
            return false;
        };
        t.clone()
    };
    let mut evidence = None;
    let result = (|| {
        let publication = native::publication(target(&track), selected, &mut evidence)?;
        let mut s = STATE.lock().unwrap();
        let t = s.track.as_mut().ok_or("helper ticket retired")?;
        check(
            t.source == track.source
                && t.ticket == track.ticket
                && current(t)
                && t.gate.may_publish(t.ticket, 6),
            "helper final ticket changed",
        )?;
        let registry = provenance::final_registry_for(
            publication.observed.before.helpers,
            Some(t.install),
            Some(t.source),
        )?;
        publication.write();
        let published = t.gate.publish(t.ticket, 6);
        debug_assert!(published);
        drop(registry);
        drop(s);
        drop(publication);
        Ok::<_, &'static str>(())
    })();
    if let Some(e) = evidence {
        crate::logging::diagnostic(format_args!("[MiiHelper] prequeue source={:?} selected={:#x} init={} header={:x?} index={} tokens={:x?} task={:#x} outstanding={:?} Dindex={} rebuild={} before={:?}",
        track.source,e.selected,e.initialized,e.header,e.o_index,e.o_tokens,e.task,e.outstanding,e.d_index,e.rebuild,e.before));
    }
    match result {
        Ok(()) => true,
        Err(reason) => {
            refuse(entry, reason);
            false
        }
    }
}
/// Terminal transactions skip further SourceBook readiness/G reads; INSTALL
/// remains owned until the actual destructor. Missing tracks cover a refused bind.
pub(super) fn retained_terminal(source: SourceToken) -> bool {
    let s = STATE.lock().unwrap();
    s.track
        .as_ref()
        .filter(|t| t.source == source)
        .is_none_or(|t| matches!(t.gate.phase(), Phase::Refused | Phase::Departed))
}
pub(super) fn refuse_completion(source: SourceToken, reason: &str) {
    let mut s = STATE.lock().unwrap();
    let Some(t) = s.track.as_mut().filter(|t| t.source == source) else {
        return;
    };
    if matches!(t.gate.phase(), Phase::Refused | Phase::Departed) {
        return;
    }
    t.gate.refuse();
    drop(s);
    crate::logging::diagnostic(format_args!(
        "[MiiHelper] completion refused; INSTALL retained source={:?} reason={}",
        source, reason
    ));
}
/// INSTALL stays held until SourceBook's Ready commit, so no writer can intervene.
pub(super) unsafe fn completed(
    source: SourceToken,
    ticks: u16,
) -> Result<Option<helpers::MiiProof>, &'static str> {
    let track = {
        let s = STATE.lock().unwrap();
        let t = s.track.as_ref().ok_or("helper ticket absent")?;
        check(
            t.source == source && current(t) && t.gate.can_complete(t.ticket),
            "helper not published/current",
        )?;
        t.clone()
    };
    if !native_source::parked_ready(
        source.entry().0 as *const u8,
        source.slot() as u32,
        track.data.configuration.canonical().class as u32,
    ) {
        return Ok(None);
    }
    native::target_rows(track.data.configuration)?;
    let completion = match native::completed(target(&track)) {
        // Only this native job-counter lag is asynchronous. Returning None
        // follows service_owned's normal tick commit, including tick 899.
        Err("ready task outstanding") if ticks < 900 => return Ok(None),
        result => result?,
    };
    let s = STATE.lock().unwrap();
    let t = s.track.as_ref().ok_or("helper completion retired")?;
    check(
        t.source == source
            && t.ticket == track.ticket
            && current(t)
            && t.gate.can_complete(t.ticket),
        "helper completion token changed",
    )?;
    let registry =
        provenance::final_registry_for(completion.now.helpers, Some(t.install), Some(source))?;
    let proof = helpers::MiiProof {
        binding: completion.now.binding,
        configuration: track.data.configuration,
        descriptor: completion.now.descriptor,
        record: completion.now.record,
    };
    drop(registry);
    drop(s);
    drop(completion);
    Ok(Some(proof))
}
pub(super) fn release(source: SourceToken) {
    let mut s = STATE.lock().unwrap();
    if s.track.as_ref().is_some_and(|t| t.source == source) {
        s.track = None;
    }
}
pub(super) fn depart() {
    let mut s = STATE.lock().unwrap();
    s.capture_wait_noted = false;
    if let Some(t) = s.track.as_mut() {
        t.gate.depart();
    }
}

/// Recheck exact committed payload/resource identity before SourceBook grants a lease.
pub(super) unsafe fn ready(ready: crate::source_lifetime::ReadyToken, c: Configuration) -> bool {
    let source = ready.source();
    let Ok(view) = provenance::helper_snapshot() else {
        return false;
    };
    let Some(ordinal)=view.ordinal(source.slot()) else{return false;};
    let Some(helpers::Helper::Ready {
        ready: current,
        proof,
        ..
    }) = view.slots.get(ordinal).copied()
    else {
        return false;
    };
    let Some(mii) = view.miis[ordinal] else {
        return false;
    };
    if current != ready || mii.configuration != c {
        return false;
    }
    let Ok(m) = native::map(source.entry().0 as usize, source.slot()) else {
        return false;
    };
    let Ok(b) = native::pool(source.entry().0 as usize, mii.binding.epoch, source.slot()) else {
        return false;
    };
    let g = crate::mii_native_access::PoolLock::acquire(BASE.load(Ordering::Acquire), b.manager);
    let result = native::current_locked(m, b, proof.original.kind as u32, None, view, None)
        .is_ok_and(|now| {
            now.binding == mii.binding
                && now.record == mii.record
                && now.descriptor == mii.descriptor
                && now.scalars == Scalars::saved(c)
                && now.owners[source.slot()] == Some(proof.owner)
                && provenance::helper_kind(
                    view,
                    source.slot(),
                    proof.owner,
                    c.canonical().class as u32,
                )
        });
    drop(g);
    result
}
