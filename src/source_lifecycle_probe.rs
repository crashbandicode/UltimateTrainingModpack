//! Development-only observations at native-owned callbacks. Never allocates,
//! activates, reloads or reserves a fighter; numeric identities are not authority.
use super::{aligned, game_mode, native_source, read, setting, BASE};
use smash::app::{self, lua_bind::*};
use smash::lib::lua_const::*;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

#[derive(Clone, Copy)]
struct Seen { identity: usize, calls: u32 }
static SEEN: Mutex<[Seen; 8]> = Mutex::new([Seen { identity: 0, calls: 0 }; 8]);
static BODY_CALLS: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
static COPY_EVENTS: AtomicU32 = AtomicU32::new(0);

// Numeric-only state; destruction generations distinguish allocator address reuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GroupKey {
    count: u32,
    kinds: [u32; 3],
    index: u8,
    bodies: [(usize, u32); 3],
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct GroupSeen {
    identity: usize,
    generation: u64,
    last: Option<GroupKey>,
    events: u8,
}
impl GroupSeen {
    const fn empty(generation: u64) -> Self {
        Self {
            identity: 0,
            generation,
            last: None,
            events: 0,
        }
    }
}
#[cfg(not(feature = "source-ldn-grouped-probe"))]
const GROUP_OWNERS: usize = 4;
#[cfg(feature = "source-ldn-grouped-probe")]
const GROUP_OWNERS: usize = 8;
static GROUP_SEEN: Mutex<[GroupSeen; GROUP_OWNERS]> =
    Mutex::new([GroupSeen::empty(0); GROUP_OWNERS]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupScope {
    TrainingVisible,
    TrainingParked,
    #[cfg(feature = "source-ldn-grouped-probe")]
    LdnVisible,
    #[cfg(feature = "source-ldn-grouped-probe")]
    LdnParked,
}
impl GroupScope {
    fn parked(self) -> bool {
        match self {
            Self::TrainingVisible => false,
            Self::TrainingParked => true,
            #[cfg(feature = "source-ldn-grouped-probe")]
            Self::LdnVisible => false,
            #[cfg(feature = "source-ldn-grouped-probe")]
            Self::LdnParked => true,
        }
    }
}

unsafe fn group_scope(owner: u32) -> Option<GroupScope> {
    if crate::common::is_native_training_mode() {
        return match owner {
            0 | 1 => Some(GroupScope::TrainingVisible),
            2 | 3 => Some(GroupScope::TrainingParked),
            _ => None,
        };
    }
    #[cfg(feature = "source-ldn-grouped-probe")]
    if matches!(game_mode::current(), Some(mode)
        if mode.initialized && mode.melee == 0x0d && mode.mode == 0x02010000)
        && super::lobby::active()
    {
        // Observation roles only. Current-entry authority and canonical body
        // readiness are checked below; this grants no reservation or admission.
        return match owner {
            0 | 4 => Some(GroupScope::LdnVisible),
            5..=7 => Some(GroupScope::LdnParked),
            _ => None,
        };
    }
    None
}

type BodyObservation = (usize, u32, u32, u64, u8);
unsafe fn body_observations(entry: *const u8, installed: u64, base: usize) -> [BodyObservation; 3] {
    let mut bodies = [(0, 0, 0, 0, 0); 3];
    for (index, output) in bodies
        .iter_mut()
        .enumerate()
        .take(installed.min(3) as usize)
    {
        let body = read::<*const u8>(entry, 0x4160 + index * 8);
        if aligned(body) && read::<usize>(body, 0) == base + 0x4f80210 {
            *output = (
                body as usize,
                read(body, 8),
                read(body, 0xc),
                read(body, 0x10),
                read(body, 0x3a),
            );
        }
    }
    bodies
}

// This small key is the only grouped read on unchanged callbacks. Full body,
// resource, typed and input payloads are collected only for a new ready event.
unsafe fn group_key(entry: *const u8, owner: u32) -> Option<GroupKey> {
    if native_source::entry(owner) != Some(entry) {
        return None;
    }
    let count = read::<u32>(entry, 0x14);
    if !(2..=3).contains(&count) || read::<u64>(entry, 0x4150) != u64::from(count) {
        return None;
    }
    let kinds: [u32; 3] = std::array::from_fn(|i| read::<u32>(entry, 0x18 + i * 4));
    training_mod_consts::TrioFighter::from_kind(kinds[0])?;
    let forms = super::Forms::starting(kinds[0]);
    if forms.count != count || forms.kinds != kinds {
        return None;
    }
    let mut bodies = [(0, 0); 3];
    for (index, output) in bodies.iter_mut().enumerate().take(count as usize) {
        let body = read::<*const u8>(entry, 0x4160 + index * 8);
        if !aligned(body) || read::<usize>(body, 0) != BASE.load(Ordering::Acquire) + 0x4f80210 {
            return None;
        }
        *output = (body as usize, read(body, 8));
    }
    Some(GroupKey {
        count,
        kinds,
        index: read(entry, 0x5918),
        bodies,
    })
}

// Outer None means not canonically body-ready. Inner None preserves a parked
// raw index outside the group; parked_ready does not impose an active index.
unsafe fn group_ready(
    entry: *const u8,
    owner: u32,
    key: GroupKey,
    scope: GroupScope,
) -> Option<Option<u32>> {
    if !scope.parked() {
        super::ready_forms(entry, owner).map(|(_, kind)| Some(kind))
    } else {
        native_source::parked_ready(entry, owner, key.kinds[0]).then(|| {
            key.kinds
                .get(usize::from(key.index))
                .copied()
                .filter(|_| u32::from(key.index) < key.count)
        })
    }
}

// Retain mismatches as data. Only the verified owner vtable permits field reads.
type ResourceObservation = (usize, Option<usize>, Option<(u32, u32)>);
unsafe fn group_resources(owner: u32, base: usize) -> (usize, [ResourceObservation; 2]) {
    let context = read::<*const u8>((base + 0x5324680) as *const u8, 0);
    let pointers = if aligned(context) {
        [
            read::<*const u8>(context, owner as usize * 8),
            read::<*const u8>(context, 0xe8 + owner as usize * 8),
        ]
    } else {
        [std::ptr::null(); 2]
    };
    (
        context as usize,
        pointers.map(|pointer| {
            let vtable = aligned(pointer).then(|| read::<usize>(pointer, 0));
            let fields = (vtable == Some(base + 0x509fb10))
                .then(|| (read::<u32>(pointer, 0x58), read::<u32>(pointer, 0x64)));
            (pointer as usize, vtable, fields)
        }),
    )
}

unsafe fn group_service(entry: *const u8, owner: u32, calls: u32) {
    if owner as usize >= GROUP_OWNERS {
        return;
    }
    let Some(scope) = group_scope(owner) else {
        return;
    };
    let before = {
        let mut seen = GROUP_SEEN.lock().unwrap();
        let current = &mut seen[owner as usize];
        if current.identity != entry as usize {
            *current = GroupSeen {
                identity: entry as usize,
                ..GroupSeen::empty(current.generation.wrapping_add(1))
            };
        }
        *current
    };
    if before.events >= 8 {
        return;
    }
    let Some(key) = group_key(entry, owner) else {
        return;
    };
    if before.last == Some(key) {
        return;
    }
    let Some(active_kind) = group_ready(entry, owner, key, scope) else {
        return;
    };
    let base = BASE.load(Ordering::Acquire);
    let actor_count = read::<u32>(entry, 0x28);
    let flags: [u8; 5] = [0x5918, 0x5919, 0x591a, 0x591f, 0x5934].map(|offset| read(entry, offset));
    let phase: [u8; 3] = [0x5920, 0x591c, 0x5935].map(|offset| read(entry, offset));
    let bodies = body_observations(entry, u64::from(key.count), base);
    let actors: [usize; 5] =
        [0x4178, 0x4180, 0x4188, 0x4190, 0x4198].map(|offset| read(entry, offset));
    let resources = group_resources(owner, base);
    let [root, alias] = resources.1;
    let resource_associated =
        root.0 != 0 && root.0 == alias.0 && root.2.is_some() && alias.2.is_some();
    // Visible bodies cannot satisfy the parked-only full-ready API. None means
    // not applicable; a parked resource mismatch is explicitly Some(false).
    let full_ready = scope.parked()
        .then(|| resource_associated && native_source::ready(entry, owner, key.kinds[0]).is_some());
    let registry = read::<*const u8>((base + 0x52bc3b0) as *const u8, 0);
    let mut typed = [None; 3];
    if aligned(registry) {
        for (index, output) in typed.iter_mut().enumerate().take(key.count as usize) {
            let record = registry.add(0x60 + key.kinds[index] as usize * 0x38);
            *output = Some((
                read::<u32>(record, 0),
                read::<usize>(record, 0x10),
                read::<usize>(record, 0x18),
            ));
        }
    }
    #[cfg(any(
        feature = "source-capacity-link-probe",
        feature = "managed-third-source-alpha"
    ))]
    let inputs = super::native_source_capacity::inputs();

    // Reacquire authority after native readiness queries, before publishing.
    // The saved resource/body/actor pointers above are never dereferenced here.
    if group_scope(owner) != Some(scope)
        || group_key(entry, owner) != Some(key)
        || group_ready(entry, owner, key, scope) != Some(active_kind)
        || read::<u32>(entry, 0x28) != actor_count
        || [0x5918, 0x5919, 0x591a, 0x591f, 0x5934].map(|offset| read::<u8>(entry, offset)) != flags
        || [0x5920, 0x591c, 0x5935].map(|offset| read::<u8>(entry, offset)) != phase
        || body_observations(entry, u64::from(key.count), base) != bodies
        || [0x4178, 0x4180, 0x4188, 0x4190, 0x4198].map(|offset| read::<usize>(entry, offset))
            != actors
        || group_resources(owner, base) != resources
        || read::<*const u8>((base + 0x52bc3b0) as *const u8, 0) != registry
    {
        return;
    }
    #[cfg(any(
        feature = "source-capacity-link-probe",
        feature = "managed-third-source-alpha"
    ))]
    if super::native_source_capacity::inputs() != inputs {
        return;
    }
    let event = {
        let mut seen = GROUP_SEEN.lock().unwrap();
        let current = &mut seen[owner as usize];
        if *current != before {
            return;
        }
        current.last = Some(key);
        current.events += 1;
        current.events
    };
    // The existing scheduled sample retains the full descriptor. This event
    // records only the group fields and the shared body payload, never another
    // 0x70 descriptor dump. Native reads and native calls are outside the mutex.
    crate::logging::diagnostic(format_args!(
        "[SourceLifecycle] grouped owner={} entry={:p} generation={} event={}/8 services={} body_frames={} body_ready=true active_kind={:x?} count={} kinds={:x?} actor_count={} form(index,previous,pending,flags,changing)={:x?} phase={:?} bodies={:x?} actors(4178,4180,4188,4190,4198)={:x?} context=0x{:x} resources(root,alias)(identity,vtable,kind_costume)={:x?} resource_associated={} full_ready={:?} registry={:p} typed(users,data10,data18)={:x?} scope={:?}",
        owner, entry, before.generation, event, calls,
        BODY_CALLS[owner as usize].load(Ordering::Relaxed), active_kind, key.count,
        key.kinds, actor_count, flags, phase, bodies, actors, resources.0,
        resources.1, resource_associated, full_ready, registry, typed, scope));
    #[cfg(any(
        feature = "source-capacity-link-probe",
        feature = "managed-third-source-alpha"
    ))]
    crate::logging::diagnostic(format_args!(
        "[SourceLifecycle] grouped_inputs owner={} entry={:p} generation={} event={} inputs={:x?}",
        owner, entry, before.generation, event, inputs
    ));
    #[cfg(not(any(feature = "source-capacity-link-probe", feature = "managed-third-source-alpha")))]
    crate::logging::diagnostic(format_args!(
        "[SourceLifecycle] grouped_inputs owner={} entry={:p} generation={} event={} unavailable=capacity_reader_feature_disabled",
        owner, entry, before.generation, event));
}

fn group_destroy(entry: *const u8, owner: u32) {
    if owner as usize >= GROUP_OWNERS {
        return;
    }
    let mut seen = GROUP_SEEN.lock().unwrap();
    let current = &mut seen[owner as usize];
    if current.identity == entry as usize {
        *current = GroupSeen::empty(current.generation.wrapping_add(1));
    }
}


// These pointers are read only during the native routine that owns them. The
// capped records distinguish registered keys from groups that still own objects.
#[skyline::hook(offset = 0xba4090, inline)]
unsafe fn copy_change(ctx: &mut skyline::hooks::InlineCtx) {
    if COPY_EVENTS.fetch_add(1, Ordering::Relaxed) >= 256 { return; }
    let copy = ctx.registers[0].x() as *const u8;
    if !aligned(copy) { return; }
    let container = read::<*const u8>(copy, 0x258);
    if !aligned(container) { return; }
    let count = read::<i32>(container, 0x10);
    let entries = read::<*const u8>(container, 0x18);
    let mut groups = [(-1i32, 0i32, 0i64); 32];
    let sampled = if aligned(entries) { count.clamp(0, 32) as usize } else { 0 };
    for (index, output) in groups.iter_mut().enumerate().take(sampled) {
        let entry = entries.add(index * 0x10);
        output.0 = read(entry, 0);
        let group = read::<*const u8>(entry, 8);
        if !aligned(group) { continue; }
        output.1 = read(group, 0x10);
        let records = read::<*const u8>(group, 0x18);
        if !aligned(records) { continue; }
        for sub in 0..output.1.clamp(0, 32) as usize {
            output.2 += i64::from(read::<i32>(records, sub * 0x348 + 0x18));
        }
    }
    crate::logging::diagnostic(format_args!(
        "[CopyGroups] change context={:p} new={} old={} count={} groups(kind,records,objects)={:?}",
        copy, ctx.registers[1].w() as i32, read::<i32>(copy, 0x17398),
        count, &groups[..sampled]));
}

#[skyline::hook(offset = 0xba5670, inline)]
unsafe fn copy_object_destroy(ctx: &mut skyline::hooks::InlineCtx) {
    if COPY_EVENTS.fetch_add(1, Ordering::Relaxed) >= 256 { return; }
    crate::logging::diagnostic(format_args!(
        "[CopyGroups] destroy context=0x{:x} kind={} record={} object=0x{:x}",
        ctx.registers[19].x(), ctx.registers[20].w() as i32,
        ctx.registers[24].w(), ctx.registers[0].x()));
}

pub unsafe fn install(base: usize) {
    if std::slice::from_raw_parts((base + 0xba4090) as *const u32, 4)
        != [0xd107c3ff, 0x6d1823e9, 0xa9196ffc, 0xa91a67fa]
        || std::ptr::read_volatile((base + 0xba5670) as *const u32) != 0xf9400008
    {
        crate::logging::diagnostic(format_args!("[CopyGroups] fingerprint mismatch; probe disabled"));
        return;
    }
    skyline::install_hooks!(copy_change, copy_object_destroy);
}

pub unsafe fn body_frame(body: *mut app::BattleObjectModuleAccessor) {
    let owner = WorkModule::get_int(body, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID);
    #[cfg(feature = "source-capacity-link-probe")]
    super::source_capacity_probe::body_identity(body, owner);
    if (0..8).contains(&owner) {
        BODY_CALLS[owner as usize].fetch_add(1, Ordering::Relaxed);
        #[cfg(feature = "source-capacity-probe")]
        super::source_capacity_probe::body_frame(owner as u32);
    }
}

unsafe fn manager_snapshot() {
    let symbol = setting(&crate::common::FIGHTER_MANAGER_ADDR);
    if symbol == 0 { return; }
    let manager = std::ptr::read_volatile(symbol as *const *const u8);
    if !aligned(manager) { return; }
    // get_fighter_entry_impl at main+2140f90 loads internal from public+0.
    let internal = read::<*const u8>(manager, 0);
    if !aligned(internal) { return; }
    let primary: [usize; 8] = std::array::from_fn(|i| read(internal, 0x20 + i * 8));
    let backup: [usize; 8] = std::array::from_fn(|i| read(internal, 0x60 + i * 8));
    crate::logging::diagnostic(format_args!(
        "[SourceLifecycle] manager={:p} count={} primary={:x?} backup={:x?} mode={:?} training_quantity={:?}",
        internal, read::<u32>(internal, 0xa0), primary, backup,
        game_mode::current(), native_source::training_cpu_quantity()));
}

/// Read-only preflight for the exact waiting layout seen with connected LDN.
/// Selection fields are observed values, not guessed player/controller roles.
/// This probe does not grant source ownership or enable the LDN backend.
unsafe fn ldn_preflight(entry: *const u8, owner: u32, kind: u32) {
    let Some(mode) = game_mode::current() else { return; };
    if !mode.initialized || mode.melee != 0x0d || mode.mode != 0x02010000
        || !matches!(owner, 5 | 6)
    { return; }
    let base = BASE.load(Ordering::Acquire);
    let records: [(u8, u8, u8, u32); 8] = std::array::from_fn(|slot| {
        let record = (base + 0x52c5730 + slot * 0x260) as *const u8;
        (read(record, 0xa), read(record, 0x79), read(record, 0xa7), read(record, 0))
    });
    let visible_ready: [bool; 2] = [0, 4].map(|slot|
        native_source::entry(slot).and_then(|current| super::ready_forms(current, slot)).is_some());
    let current_identity = native_source::entry(owner) == Some(entry);
    let parked = native_source::parked_ready(entry, owner, kind);
    // Unlike the older archive pointer sample, this invokes native archive-ready
    // and checks parsed parameters and optional copy-file directory readiness.
    let ready = native_source::ready(entry, owner, kind);
    crate::logging::diagnostic(format_args!(
        "[SourceLifecycle] ldn_preflight owner={} current_identity={} lobby_active={} ready_go={} native_ui_busy={} visible_ready={:?} parked={} full_ready={:?} selection(type,marker,owner_byte,opaque_word)={:x?}",
        owner, current_identity, super::lobby::active(), crate::common::is_ready_go(),
        super::lobby::native_ui_busy(), visible_ready, parked, ready, records));
    manager_snapshot();
}

pub unsafe fn service(entry: *const u8, owner: u32) {
    if owner >= 8 { return; }
    let calls = {
        let mut seen = SEEN.lock().unwrap();
        let current = &mut seen[owner as usize];
        if current.identity != entry as usize {
            *current = Seen { identity: entry as usize, calls: 0 };
            BODY_CALLS[owner as usize].store(0, Ordering::Relaxed);
        }
        current.calls = current.calls.saturating_add(1);
        current.calls
    };
    group_service(entry, owner, calls);
    // Three samples per native lifetime, plus the synchronous destruction event.
    if !matches!(calls, 1 | 300 | 1200) { return; }
    let base = BASE.load(Ordering::Acquire);
    let kind = read::<u32>(entry, 0x18);
    let descriptor: [u32; 28] = std::array::from_fn(|i| read(entry, i * 4));
    let installed = read::<u64>(entry, 0x4150);
    let bodies = body_observations(entry, installed, base);
    let context = std::ptr::read_volatile((base + 0x5324680) as *const *const u8);
    let archive = if aligned(context) { read::<*const u8>(context, 0xe8 + owner as usize * 8) }
        else { std::ptr::null() };
    let archive_info = aligned(archive).then(|| (archive as usize,
        read::<u32>(archive, 0x58), read::<u32>(archive, 0x64)));
    let registry = std::ptr::read_volatile((base + 0x52bc3b0) as *const *const u8);
    let typed = if aligned(registry) && training_mod_consts::TrioFighter::from_kind(kind).is_some() {
        let record = registry.add(0x60 + kind as usize * 0x38);
        Some((read::<u32>(record, 0), read::<usize>(record, 0x10), read::<usize>(record, 0x18)))
    } else { None };
    crate::logging::diagnostic(format_args!(
        "[SourceLifecycle] sample owner={} entry={:p} services={} body_frames={} outer={} loader={} guard={} installed={} bodies={:x?} archive={:x?} typed={:x?} descriptor={:x?}",
        owner, entry, calls, BODY_CALLS[owner as usize].load(Ordering::Relaxed),
        read::<u8>(entry, 0x5920), read::<u8>(entry, 0x591c), read::<u8>(entry, 0x5935),
        installed, bodies, archive_info, typed, descriptor));
    if owner == 0 { manager_snapshot(); }
    ldn_preflight(entry, owner, kind);
}

pub unsafe fn destroy(entry: *const u8, owner: u32) {
    if owner >= 8 { return; }
    group_destroy(entry, owner);
    let mut seen = SEEN.lock().unwrap();
    let current = &mut seen[owner as usize];
    crate::logging::diagnostic(format_args!(
        "[SourceLifecycle] destroy owner={} entry={:p} services={} body_frames={} identity_match={} mode={:?}",
        owner, entry, current.calls, BODY_CALLS[owner as usize].load(Ordering::Relaxed),
        current.identity == entry as usize, game_mode::current()));
    if current.identity == entry as usize {
        *current = Seen { identity: 0, calls: 0 };
        BODY_CALLS[owner as usize].store(0, Ordering::Relaxed);
    }
}
