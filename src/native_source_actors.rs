//! Canonical parked entry7 actor/control contract plus guarded visible form
//! input comparison. Current native members authorize reads; retained sets are
//! numeric comparison evidence only. This module never mutates native state.
use super::{aligned, inputs, native_source, read, Inputs, Snapshot, BASE, SLOT};
use skyline::hooks::InlineCtx;
use std::sync::atomic::Ordering;

#[path = "source_visible_forms.rs"]
mod visible;
pub(crate) use visible::Bank as VisibleBank;

pub(crate) fn supported(kind: u32) -> bool {
    if training_mod_consts::TrioFighter::from_kind(kind).is_none() {
        return false;
    }
    let forms = super::super::Forms::starting(kind);
    (forms.count == 1 && forms.actor_count == 1 && forms.flags == 0)
        || (forms.count == 2 && forms.actor_count == 2 && forms.flags == 2)
        || ((2..=3).contains(&forms.count) && forms.actor_count == 1 && forms.flags & 4 != 0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Actor {
    pub(crate) actor: usize,
    control: usize,
    partner: usize,
    owner: u32,
    cleanup_owner: u32,
    pub(crate) kind: u32,
    flags: [u8; 3],
    control_actor: usize,
    input_index: i32,
    binding: usize,
    alias_index: i32,
    partner_control: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Actors {
    pointers: [usize; 5],
    pub(crate) actors: [Option<Actor>; 3],
}

#[derive(Clone, Copy)]
struct ActorPlan {
    first: usize,
    count: usize,
    kinds: [u32; 3],
    partners: bool,
}

unsafe fn actor_plan(kind: u32) -> Option<ActorPlan> {
    let forms = super::super::Forms::starting(kind);
    if !supported(kind) {
        return None;
    }
    if forms.count == 1 {
        return Some(ActorPlan {
            first: 0,
            count: 1,
            kinds: forms.kinds,
            partners: false,
        });
    }
    if forms.flags & 4 == 0 {
        return (forms.count == 2 && forms.actor_count == 2).then_some(ActorPlan {
            first: 0,
            count: 2,
            kinds: forms.kinds,
            partners: true,
        });
    }
    if forms.actor_count != 1 || !(2..=3).contains(&forms.count) {
        return None;
    }
    // Same table selection as native655450/6516b0. The two immutable
    // tables are fingerprinted before hooks; descriptor order may differ.
    let offset = if kind & !1 == 0x5a {
        0x453c158
    } else {
        0x453c164
    };
    let table = (BASE.load(Ordering::Acquire) + offset) as *const u8;
    let kinds = [0usize, 1, 2].map(|index| read::<u32>(table, index * 4));
    let count = forms.count as usize;
    if kinds[..count].iter().any(|kind| !forms.same_group(*kind))
        || kinds[..count]
            .iter()
            .enumerate()
            .any(|(index, kind)| kinds[..index].contains(kind))
        || kinds[count..].iter().any(|kind| *kind != u32::MAX)
    {
        return None;
    }
    Some(ActorPlan {
        first: 2,
        count,
        kinds,
        partners: false,
    })
}

unsafe fn actor_record(actor: *const u8) -> Option<Actor> {
    if !aligned(actor) || read::<usize>(actor, 0) != BASE.load(Ordering::Acquire) + 0x4f85d30 {
        return None;
    }
    let control = read::<*const u8>(actor, 0x58);
    if !aligned(control) {
        return None;
    }
    Some(Actor {
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

unsafe fn group_descriptor(entry: *const u8, kind: u32) -> bool {
    let forms = super::super::Forms::starting(kind);
    actor_plan(kind).is_some()
        && read::<u32>(entry, 0x14) == forms.count
        && read::<u32>(entry, 0x28) == forms.actor_count
        && [0usize, 1, 2].map(|i| read::<u32>(entry, 0x18 + i * 4)) == forms.kinds
        && read::<u8>(entry, 0x591f) == forms.flags
}

unsafe fn group_actor_observation(entry: *const u8, kind: u32) -> Option<Actors> {
    if native_source::entry(SLOT as u32) != Some(entry) || !group_descriptor(entry, kind) {
        return None;
    }
    let plan = actor_plan(kind)?;
    let pointers: [usize; 5] =
        [0x4178, 0x4180, 0x4188, 0x4190, 0x4198].map(|offset| read(entry, offset));
    let actors = std::array::from_fn(|i| {
        if i < plan.count {
            actor_record(pointers[plan.first + i] as *const u8)
        } else {
            None
        }
    });
    if native_source::entry(SLOT as u32) != Some(entry)
        || !group_descriptor(entry, kind)
        || [0x4178, 0x4180, 0x4188, 0x4190, 0x4198].map(|offset| read::<usize>(entry, offset))
            != pointers
    {
        return None;
    }
    Some(Actors { pointers, actors })
}

fn actor_fields_valid(actor: Actor, index: usize, input: Inputs, plan: ActorPlan) -> bool {
    let route = SLOT + if plan.partners { index * 8 } else { 0 };
    actor.owner == SLOT as u32
        && actor.cleanup_owner == SLOT as u32
        && actor.kind == plan.kinds[index]
        && actor.flags == [0, 1, 0]
        && actor.control_actor == actor.actor
        && actor.input_index == route as i32
        && actor.alias_index == route as i32
        && actor.binding == input.records + route * 0x280
}

// Native658af0 runs 655320 and then 64f820. For an alternate bank, 64f820
// tail-calls 64fb70, which aliases the current form's member at +4178 and
// calls 313b00(service, route7, entry+34, null, -1). That stores only route
// 7's state byte; each member destructor's 313e90(7) restores state 2.
unsafe fn activated_member(entry: *const u8, kind: u32, plan: ActorPlan) -> Option<usize> {
    let form = read::<u8>(entry, 0x5918) as usize;
    if plan.first != 2
        || form >= super::super::Forms::starting(kind).count as usize
        || !group_descriptor(entry, kind)
    {
        return None;
    }
    let kind = read::<u32>(entry, 0x18 + form * 4);
    let index = plan.kinds[..plan.count]
        .iter()
        .position(|member| *member == kind)?;
    let member = read::<usize>(entry, 0x4178 + (plan.first + index) * 8);
    (member != 0).then_some(member)
}

pub(crate) unsafe fn ready(entry: *const u8, kind: u32) -> Option<Actors> {
    let input = inputs().ok()?;
    let observed = group_actor_observation(entry, kind)?;
    let plan = actor_plan(kind)?;
    let information = read::<*const u8>(entry, 0xf8);
    for (index, record) in observed.actors.iter().copied().enumerate().take(plan.count) {
        let record = record?;
        if !actor_fields_valid(record, index, input, plan)
            || observed.actors[..index].iter().flatten().any(|previous| {
                previous.actor == record.actor || previous.control == record.control
            })
        {
            return None;
        }
        if plan.partners {
            let first = observed.actors[0]?;
            let second = observed.actors[1]?;
            if first.partner != 0
                || second.partner != first.actor
                || first.partner_control != second.control
                || second.partner_control != 0
            {
                return None;
            }
        } else if record.partner != 0 || record.partner_control != 0 {
            return None;
        }
    }
    // Construction leaves the non-owning slots zero; an alternate bank's 64fb70
    // then aliases exactly the current form's member at +4178. Never follow it.
    let alias = if plan.first == 2 {
        activated_member(entry, kind, plan)?
    } else {
        0
    };
    let non_owning = [alias, 0, 0, 0, 0];
    if observed
        .pointers
        .iter()
        .zip(non_owning)
        .enumerate()
        .any(|(index, (pointer, expected))| {
            !(plan.first..plan.first + plan.count).contains(&index) && *pointer != expected
        })
        || !aligned(information)
        || read::<u8>(information, 0x92) != 0
        || input.cpu_flag != 1
        || !input.unclaimed()
        || read::<u8>(entry, 0x34) != 0
        || input.slots[7].state != if plan.first == 2 { 0 } else { 2 }
        || input.slots[15].state != 2
        || inputs() != Ok(input)
    {
        return None;
    }
    Some(observed)
}

impl Actors {
    pub(crate) fn contains(self, actor: usize) -> bool {
        self.actors
            .iter()
            .flatten()
            .any(|record| record.actor == actor)
    }
    pub(crate) fn count(self) -> u32 {
        self.actors.iter().flatten().count() as u32
    }
    pub(crate) fn unretired(self, actor: usize, seen: u8) -> bool {
        self.actors.iter().enumerate().any(|(index, record)| {
            seen & (1 << index) == 0 && record.is_some_and(|record| record.actor == actor)
        })
    }
}

/// Before a new install, the old complete bank's route is native-active (0),
/// otherwise native-unclaimed (2). Release313e90 resets it to2; 64fb70/313b00
/// sets only state7 to0 for the new alternate bank. This is the strict base
/// expectation; loaded_visible_input additionally qualifies visible form routes.
pub(crate) fn loaded_input(before: Inputs, previous: Option<u32>, kind: u32) -> Option<Inputs> {
    if !supported(kind) || !preflight_input(before, previous) {
        return None;
    }
    let mut after = before;
    after.slots[7].state = if super::super::Forms::starting(kind).flags & 4 != 0 {
        0
    } else {
        2
    };
    Some(after)
}

// Current native members authorize every pointer read. A saved bank is used
// only as a numeric identity comparison after reacquiring the same live FE.
unsafe fn visible_bank(scene: Snapshot, input: Inputs, index: usize) -> Option<VisibleBank> {
    if index >= 2 { return None; }
    let owner = scene.layout.owners()[index];
    let entry = native_source::entry(owner)?;
    let original = scene.original[index];
    if entry as usize != scene.primary[owner as usize]
        || super::entry_observation(entry, owner) != Some(original)
    { return None; }
    let (forms, active_kind) = super::super::ready_forms(entry, owner)?;
    if !matches!(forms.kinds[0], 0x24..=0x26 | 0x5a | 0x5b)
        || forms.actor_count != 1 || forms.flags & 4 == 0
    { return None; }
    let plan = actor_plan(forms.kinds[0])?;
    if plan.first != 2 { return None; }
    let resource = native_source::resource_owner(owner)?;
    if resource as usize != scene.resources[owner as usize]
        || resource as usize != scene.aliases[owner as usize]
        || read::<usize>(resource, 0) != BASE.load(Ordering::Acquire) + 0x509fb10
    { return None; }
    let pointers: [usize; 5] =
        [0x4178, 0x4180, 0x4188, 0x4190, 0x4198].map(|offset| read(entry, offset));
    let mut members = [None; 3];
    let mut active = None;
    let mut active_actor = None;
    let mut route = None;
    for (index, member) in members.iter_mut().enumerate().take(plan.count) {
        let actor = actor_record(pointers[plan.first + index] as *const u8)?;
        if actor.owner != owner || actor.cleanup_owner != owner
            || actor.kind != plan.kinds[index] || actor.control_actor != actor.actor
            || actor.partner != 0 || actor.partner_control != 0
        { return None; }
        *member = Some(visible::Member {
            actor: actor.actor, control: actor.control, kind: actor.kind,
        });
        if actor.kind == active_kind {
            if pointers[0] != actor.actor { return None; }
            active = Some(index);
            active_actor = Some(actor);
            route = Some((actor.input_index, actor.binding, actor.alias_index));
        }
    }
    if pointers[1] != 0 || (plan.count == 2 && pointers[4] != 0) { return None; }
    let bank = VisibleBank {
        entry: entry as usize, owner,
        resource: (resource as usize, read(resource, 0x58), read(resource, 0x64)),
        kinds: forms.kinds, bodies: original.bodies, members,
        active: active?, record: input.records + owner as usize * 0x280, route: route?,
    };
    // Recheck native membership and active control after the bounded reads.
    if native_source::entry(owner) != Some(entry)
        || super::entry_observation(entry, owner) != Some(original)
        || super::super::ready_forms(entry, owner) != Some((forms, active_kind))
        || native_source::resource_owner(owner) != Some(resource)
    { return None; }
    let current_pointers =
        [0x4178, 0x4180, 0x4188, 0x4190, 0x4198].map(|offset| read::<usize>(entry, offset));
    if current_pointers != pointers
        || actor_record(current_pointers[plan.first + bank.active] as *const u8) != active_actor
        || (read(resource, 0x58), read(resource, 0x64)) != (bank.resource.1, bank.resource.2)
        || inputs() != Ok(input)
    { return None; }
    // Even unchanged preflight input must bind the active native member.
    let slot = input.slots[owner as usize];
    let row = (slot.pointer, slot.state, slot.key, slot.owner, slot.alias);
    (visible::expected_slot(row, bank, bank) == Some(row)).then_some(bank)
}

pub(crate) unsafe fn visible_preflight(scene: Snapshot, input: Inputs) -> [Option<VisibleBank>; 2] {
    let mut banks = [None; 2];
    for (index, bank) in banks.iter_mut().enumerate() {
        if matches!(scene.original[index].kinds[0], 0x24..=0x26 | 0x5a | 0x5b) {
            *bank = visible_bank(scene, input, index);
        }
    }
    banks
}

// Extra evidence for initial native reservation only. No saved pointer is
// dereferenced: every member comes from the currently resolved original entry.
#[cfg(feature = "managed-visible-grouped-source-alpha")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CreationVisible {
    descriptor: [u32; 28],
    resource: (usize, u32, u32),
    forms: super::super::Forms,
    active_kind: u32,
    form: [u8; 5],
    bodies: [(usize, u32, u32, u64, u8, usize); 3],
    actors: Actors,
    alternate: Option<VisibleBank>,
}

#[cfg(feature = "managed-visible-grouped-source-alpha")]
unsafe fn creation_visible_one(scene: Snapshot, input: Inputs, index: usize) -> Option<CreationVisible> {
    let owner = scene.layout.owners()[index];
    let entry = native_source::entry(owner)?;
    let original = scene.original[index];
    if entry as usize != scene.primary[owner as usize]
        || super::entry_observation(entry, owner) != Some(original)
    { return None; }
    let (forms, active_kind) = super::super::ready_forms(entry, owner)?;
    let resource = native_source::resource_owner(owner)?;
    if resource as usize != scene.resources[owner as usize]
        || resource as usize != scene.aliases[owner as usize]
        || read::<usize>(resource, 0) != BASE.load(Ordering::Acquire) + 0x509fb10
        || read::<u32>(resource, 0x58) != forms.kinds[0]
    { return None; }
    let resource_fields = (resource as usize, read(resource, 0x58), read(resource, 0x64));
    let alternate = if forms.flags & 4 != 0 {
        Some(visible_bank(scene, input, index)?)
    } else { None };
    let plan = if alternate.is_some() {
        actor_plan(forms.kinds[0])?
    } else if (forms.count == 2 && forms.actor_count == 2 && forms.flags == 2)
        || (forms.count == 1 && forms.actor_count == 1 && forms.flags == 0)
    {
        // Includes native Solo Popo's current canonical one-body descriptor;
        // it does not change Forms::starting or any parked source target.
        ActorPlan { first: 0, count: forms.count as usize, kinds: forms.kinds,
            partners: forms.count == 2 }
    } else { return None; };
    let descriptor = std::array::from_fn(|i| read(entry, i * 4));
    let form = [0x5918, 0x5919, 0x591a, 0x591f, 0x5934].map(|offset| read(entry, offset));
    let pointers: [usize; 5] = [0x4178, 0x4180, 0x4188, 0x4190, 0x4198]
        .map(|offset| read(entry, offset));
    let mut actors = Actors { pointers, actors: [None; 3] };
    for i in 0..plan.count {
        let actor = actor_record(pointers[plan.first + i] as *const u8)?;
        if actor.owner != owner || actor.cleanup_owner != owner
            || actor.kind != plan.kinds[i] || actor.control_actor != actor.actor
            || actors.actors[..i].iter().flatten().any(|old|
                old.actor == actor.actor || old.control == actor.control)
        { return None; }
        if alternate.is_some() {
            // Every inactive bank member must also retain its own visible
            // owner's route. An active-row match alone cannot reserve7 safely.
            if actor.partner != 0 || actor.partner_control != 0
                || actor.input_index != owner as i32 || actor.alias_index != owner as i32
                || actor.binding != input.records + owner as usize * 0x280
            { return None; }
        } else {
            let route = owner as usize + i * 8;
            let expected = if owner == 0 && i == 0 { (0, -1, -1) }
                else { (1, route as i32, route as i32) };
            let slot = input.slots[route];
            if actor.input_index != route as i32 || actor.alias_index != route as i32
                || actor.binding != input.records + route * 0x280
                || (slot.pointer, slot.key) != (actor.control, actor.kind as i32)
                || (slot.state, slot.owner, slot.alias) != expected
            { return None; }
        }
        actors.actors[i] = Some(actor);
    }
    if alternate.is_none() {
        if pointers[plan.count..].iter().any(|pointer| *pointer != 0) { return None; }
        let first = actors.actors[0]?;
        if plan.partners {
            let second = actors.actors[1]?;
            if first.partner != 0 || second.partner != first.actor
                || first.partner_control != second.control || second.partner_control != 0
            { return None; }
        } else if first.partner != 0 || first.partner_control != 0 { return None; }
    }
    let mut bodies = [(0, 0, 0, 0, 0, 0); 3];
    for (i, output) in bodies.iter_mut().enumerate().take(forms.count as usize) {
        // ready_forms just validated each current body/vtable/owner/kind/state.
        let body = read::<*const u8>(entry, 0x4160 + i * 8);
        if !aligned(body) || read::<usize>(body, 0) != BASE.load(Ordering::Acquire) + 0x4f80210 {
            return None;
        }
        *output = (body as usize, read(body, 8), read(body, 0xc), read(body, 0x10),
            read(body, 0x3a), read(body, 0x20));
    }
    // Reacquire roots and recheck membership before rereading any member.
    if native_source::entry(owner) != Some(entry)
        || super::entry_observation(entry, owner) != Some(original)
        || super::super::ready_forms(entry, owner) != Some((forms, active_kind))
        || native_source::resource_owner(owner) != Some(resource)
        || (read(resource, 0x58), read(resource, 0x64)) != (resource_fields.1, resource_fields.2)
        || std::array::from_fn::<_, 28, _>(|i| read::<u32>(entry, i * 4)) != descriptor
        || [0x5918, 0x5919, 0x591a, 0x591f, 0x5934].map(|offset| read::<u8>(entry, offset)) != form
        || [0x4178, 0x4180, 0x4188, 0x4190, 0x4198].map(|offset| read::<usize>(entry, offset)) != pointers
        || inputs() != Ok(input)
    { return None; }
    for i in 0..plan.count {
        if actor_record(pointers[plan.first + i] as *const u8) != actors.actors[i] { return None; }
    }
    for (i, expected) in bodies.iter().enumerate().take(forms.count as usize) {
        let body = read::<*const u8>(entry, 0x4160 + i * 8);
        if !aligned(body) || body as usize != expected.0
            || read::<usize>(body, 0) != BASE.load(Ordering::Acquire) + 0x4f80210
            || (body as usize, read(body, 8), read(body, 0xc), read(body, 0x10),
                read(body, 0x3a), read(body, 0x20)) != *expected
        { return None; }
    }
    Some(CreationVisible { descriptor, resource: resource_fields, forms, active_kind,
        form, bodies, actors, alternate })
}

#[cfg(feature = "managed-visible-grouped-source-alpha")]
pub(crate) unsafe fn creation_visible(scene: Snapshot, input: Inputs) -> Option<[Option<CreationVisible>; 2]> {
    let mut originals = [None; 2];
    for (index, output) in originals.iter_mut().enumerate() {
        if !native_source::ordinary_kind(scene.original[index].kinds[0]) {
            *output = Some(creation_visible_one(scene, input, index)?);
        }
    }
    Some(originals)
}

pub(crate) unsafe fn loaded_visible_input(
    scene: Snapshot, before: Inputs, previous: Option<u32>, kind: u32,
    banks: [Option<VisibleBank>; 2], observed: Inputs,
) -> Option<Inputs> {
    let mut expected = loaded_input(before, previous, kind)?;
    if expected == observed { return Some(expected); }
    let mut current = [None; 2];
    let owners = [scene.layout.owners()[0], scene.layout.owners()[1]];
    for (index, bank) in banks.iter().copied().enumerate() {
        let owner = owners[index] as usize;
        if expected.slots[owner] == observed.slots[owner] { continue; }
        if bank.is_some() { current[index] = visible_bank(scene, observed, index); }
    }
    let rows = |input: Inputs| input.slots.map(|s| (s.pointer, s.state, s.key, s.owner, s.alias));
    let merged = visible::merge_slots(rows(expected), rows(observed), owners, banks, current)?;
    for (slot, row) in expected.slots.iter_mut().zip(merged) {
        // Only the independently authorized pointer/key may differ.
        slot.pointer = row.0;
        slot.key = row.2;
    }
    (observed == expected && inputs() == Ok(observed)).then_some(expected)
}

// Event-only attribution, including raw current control addresses when the
// qualification reader refuses their semantics/alignment. Never follow a saved
// actor or an unvalidated control; actor_record remains the dereference gate.
#[cfg(feature = "source-capacity-probe")]
unsafe fn visible_event_members(
    scene: Snapshot, index: usize,
) -> Option<(u32, Actors, [Option<usize>; 3])> {
    let owner = scene.layout.owners()[index];
    let entry = native_source::entry(owner)?;
    if entry as usize != scene.primary[owner as usize]
        || super::entry_observation(entry, owner) != Some(scene.original[index])
    { return None; }
    let (forms, active_kind) = super::super::ready_forms(entry, owner)?;
    let plan = actor_plan(forms.kinds[0])?;
    if plan.first != 2 { return None; }
    let pointers: [usize; 5] =
        [0x4178, 0x4180, 0x4188, 0x4190, 0x4198].map(|offset| read(entry, offset));
    let mut controls = [None; 3];
    let actors = std::array::from_fn(|index| {
        if index >= plan.count { return None; }
        let actor = pointers[plan.first + index] as *const u8;
        if aligned(actor) && read::<usize>(actor, 0) == BASE.load(Ordering::Acquire) + 0x4f85d30 {
            controls[index] = Some(read(actor, 0x58));
        }
        actor_record(actor)
    });
    if native_source::entry(owner) != Some(entry)
        || super::entry_observation(entry, owner) != Some(scene.original[index])
        || super::super::ready_forms(entry, owner) != Some((forms, active_kind))
        || [0x4178, 0x4180, 0x4188, 0x4190, 0x4198].map(|offset| read::<usize>(entry, offset)) != pointers
    { return None; }
    Some((active_kind, Actors { pointers, actors }, controls))
}

#[cfg(feature = "source-capacity-probe")]
pub(crate) unsafe fn observe_visible(
    source: crate::source_lifetime::SourceToken,
    scene: Snapshot, input: Option<Inputs>, captured: [Option<VisibleBank>; 2], event: &str,
) {
    for (index, bank) in captured.iter().copied().enumerate() {
        let owner = scene.layout.owners()[index];
        let applicable = matches!(scene.original[index].kinds[0], 0x24..=0x26 | 0x5a | 0x5b);
        let current = if applicable { input.and_then(|input| visible_bank(scene, input, index)) } else { None };
        let members = if applicable { visible_event_members(scene, index) } else { None };
        crate::logging::diagnostic(format_args!(
            "[SourceOverlap] visible_event source={:?} event={} owner={} applicable={} captured_qualified={} event_qualified={} event_only_resample=true input_row={:x?} captured_bank={:x?} event_bank={:x?} event_members(active_kind,actors,raw_controls)={:x?}",
            source, event, owner, applicable, bank.is_some(), current.is_some(),
            input.map(|input| input.slots[owner as usize]), bank, current, members));
    }
}

pub(crate) fn preflight_input(before: Inputs, previous: Option<u32>) -> bool {
    if previous.is_some_and(|kind| !supported(kind)) {
        return false;
    }
    let alternate = |kind| super::super::Forms::starting(kind).flags & 4 != 0;
    let expected_state = if previous.is_some_and(alternate) {
        0
    } else {
        2
    };
    let row = before.slots[7];
    before.unclaimed()
        && before.cpu_flag == 1
        && (row.pointer, row.state, row.key, row.owner, row.alias)
            == (0, expected_state, -1, -1, -1)
        && before.slots[15].state == 2
}

pub(crate) fn comparison_view(
    current: Snapshot,
    before: Inputs,
    expected: Option<Inputs>,
) -> Snapshot {
    #[cfg(any(
        feature = "source-ldn-capacity-empty-probe",
        feature = "managed-ldn-third-source-alpha"
    ))]
    {
        let mut current = current;
        if let Some(fields) = current.ldn.as_mut().filter(|fields| {
            expected.is_some_and(|expected| fields.input == Some(expected))
        }) {
            fields.input = Some(before);
        }
        current
    }
    #[cfg(not(any(
        feature = "source-ldn-capacity-empty-probe",
        feature = "managed-ldn-third-source-alpha"
    )))]
    {
        let _ = (before, expected);
        current
    }
}

/// The release loop clears a member field only after this destructor returns.
/// Plan comes from its source generation, not a newly published target kind.
pub(crate) unsafe fn cleanup(
    ctx: &InlineCtx,
    entry: *const u8,
    kind: u32,
    expected: Option<Actors>,
    first: Option<Actor>,
) -> Option<(usize, Option<Actor>, bool)> {
    let plan = actor_plan(kind)?;
    let actor = ctx.registers[20].x() as usize;
    let offsets = [0x4178, 0x4180, 0x4188, 0x4190, 0x4198];
    let index = (0..plan.count)
        .find(|index| actor != 0 && read::<usize>(entry, offsets[plan.first + index]) == actor)?;
    let record = actor_record(actor as *const u8);
    let input = inputs();
    let identity = record.is_some_and(|record| {
        input.is_ok_and(|input| actor_fields_valid(record, index, input, plan))
            && expected.is_none_or(|set| set.actors[index] == Some(record))
            && if plan.partners {
                if index == 0 {
                    record.partner == 0
                } else {
                    expected
                        .and_then(|set| set.actors[0])
                        .or(first)
                        .is_some_and(|first| {
                            record.partner == first.actor && first.partner_control == record.control
                        })
                }
            } else {
                record.partner == 0 && record.partner_control == 0
            }
    });
    if native_source::entry(7) != Some(entry)
        || read::<usize>(entry, offsets[plan.first + index]) != actor
    {
        return None;
    }
    Some((index, record, identity))
}
