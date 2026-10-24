//! Shared read-only native entry7 contract and native creation boundary.
//! Saved addresses are identities, never authority to dereference old objects.
use super::{aligned, native_source, read, setting, BASE};
use crate::common;
use skyline::hooks::InlineCtx;
use super::native_service::ServiceEntryContext;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

const SLOT: usize = 7;

#[cfg(feature = "managed-grouped-source-alpha")]
#[path = "native_source_actors.rs"]
mod canonical;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Entry {
    pub(super) identity: usize,
    pub(super) count: u32,
    pub(super) kinds: [u32; 3],
    pub(super) installed: u64,
    pub(super) phase: [u8; 3],
    pub(super) bodies: [(usize, u32); 3],
}

/// Two witnessed layouts. The legacy snapshot wrapper always selects Training.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Layout {
    Training,
    #[cfg(any(feature = "source-ldn-capacity-empty-probe", feature = "managed-ldn-third-source-alpha"))]
    LdnWaiting,
}

impl Layout {
    pub(super) fn owners(self) -> [u32; 4] {
        match self {
            Self::Training => [0, 1, 2, 3],
            #[cfg(any(feature = "source-ldn-capacity-empty-probe", feature = "managed-ldn-third-source-alpha"))]
            Self::LdnWaiting => [0, 4, 5, 6],
        }
    }
    #[cfg(feature = "source-capacity-probe")]
    pub(super) fn callback(self, owner: u32) -> u8 {
        match self {
            Self::Training => u8::from(owner == 0),
            #[cfg(any(feature = "source-ldn-capacity-empty-probe", feature = "managed-ldn-third-source-alpha"))]
            Self::LdnWaiting => match owner { 5 => 1, 6 => 2, _ => 0 },
        }
    }
    #[cfg(feature = "source-capacity-probe")]
    pub(super) fn callbacks_complete(self, seen: u8) -> bool {
        match self {
            Self::Training => seen == 1,
            #[cfg(any(feature = "source-ldn-capacity-empty-probe", feature = "managed-ldn-third-source-alpha"))]
            Self::LdnWaiting => seen == 3,
        }
    }
}

#[cfg(any(feature = "source-ldn-capacity-empty-probe", feature = "managed-ldn-third-source-alpha"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LdnFields {
    pub(super) selection: [(u8, u8, u8); 8],
    pub(super) resources: Option<[(u32, u32); 4]>,
    pub(super) input: Option<Inputs>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub(super) layout: Layout,
    pub(super) manager: usize,
    pub(super) internal: usize,
    pub(super) context: usize,
    pub(super) service: usize,
    pub(super) service_inner: usize,
    pub(super) listeners: u64,
    pub(super) count: u32,
    pub(super) quantity: Option<u32>,
    #[cfg(any(feature = "source-ldn-capacity-empty-probe", feature = "managed-ldn-third-source-alpha"))]
    pub(super) ldn: Option<LdnFields>,
    pub(super) rules: [u8; 2],
    pub(super) ratio_bits: u32,
    pub(super) primary: [usize; 8],
    pub(super) backup: [usize; 8],
    pub(super) resources: [usize; 8],
    pub(super) aliases: [usize; 8],
    pub(super) selection: [u8; 8],
    pub(super) original: [Entry; 4],
}

/// The dispatcher owns this list node and callback object right now. Verify its
/// preserved registers and exact non-virtual callback tuple before using globals.
pub(super) unsafe fn dispatch_identity(
    ctx: &ServiceEntryContext,
    entry: *const u8,
) -> Option<(usize, usize)> {
    let [dispatcher_inner, dispatcher_sentinel, dispatcher_node, caller_lr] = ctx.dispatcher_tuple();
    let base = BASE.load(Ordering::Acquire);
    if caller_lr != base + 0x1755e80 {
        return None;
    }
    let service = read::<*const u8>(base as *const u8, 0x52c50f8);
    if !aligned(service) {
        return None;
    }
    let inner = read::<*const u8>(service, 0);
    if !aligned(inner)
        || dispatcher_inner != inner as usize
        || dispatcher_sentinel != inner as usize + 0x938
    {
        return None;
    }
    let node = dispatcher_node as *const u8;
    if !aligned(node) || node == inner.add(0x938) {
        return None;
    }
    let callback = read::<*const u8>(node, 0x10);
    if !aligned(callback)
        || read::<usize>(callback, 0) != base + 0x4f81468
        || read::<*const u8>(callback, 0x10) != entry
        || read::<usize>(callback, 0x18) != base + 0x650010
        || read::<usize>(callback, 0x20) != 0
    {
        return None;
    }
    Some((service as usize, inner as usize))
}

pub(super) unsafe fn entry_observation(entry: *const u8, owner: u32) -> Option<Entry> {
    if !aligned(entry) || read::<u32>(entry, 0xe0) != owner {
        return None;
    }
    let installed = read::<u64>(entry, 0x4150);
    if installed > 3 {
        return None;
    }
    let mut bodies = [(0, 0); 3];
    for (index, result) in bodies.iter_mut().enumerate().take(installed as usize) {
        let body = read::<*const u8>(entry, 0x4160 + index * 8);
        if !aligned(body)
            || read::<usize>(body, 0) != BASE.load(Ordering::Acquire) + 0x4f80210
            || read::<u64>(body, 0x10) != u64::from(owner)
        {
            return None;
        }
        *result = (body as usize, read(body, 8));
    }
    Some(Entry {
        identity: entry as usize,
        count: read(entry, 0x14),
        kinds: std::array::from_fn(|i| read(entry, 0x18 + i * 4)),
        installed,
        phase: [
            read(entry, 0x5920),
            read(entry, 0x591c),
            read(entry, 0x5935),
        ],
        bodies,
    })
}

/// Reacquire every pointer from current native roots, only at a verified service
/// callback. No pointer in a previously saved Snapshot is dereferenced here.
#[cfg(any(feature = "source-capacity-link-probe", feature = "managed-third-source-alpha"))]
pub(super) unsafe fn snapshot(ctx: &ServiceEntryContext, entry: *const u8, owner: u32) -> Option<Snapshot> {
    snapshot_for(Layout::Training, ctx, entry, owner)
}

pub(super) unsafe fn snapshot_for(
    layout: Layout, ctx: &ServiceEntryContext, entry: *const u8, owner: u32,
) -> Option<Snapshot> {
    let (service, service_inner) = dispatch_identity(ctx, entry)?;
    let base = BASE.load(Ordering::Acquire);
    let symbol = setting(&common::FIGHTER_MANAGER_ADDR);
    // The creation helper uses this exact singleton; reject an alternate binding.
    if symbol != base + 0x52b94f8 {
        return None;
    }
    let manager = read::<*const u8>(symbol as *const u8, 0);
    if !aligned(manager) {
        return None;
    }
    let internal = read::<*const u8>(manager, 0);
    let context = read::<*const u8>(base as *const u8, 0x5324680);
    if !aligned(internal) || !aligned(context) {
        return None;
    }
    let primary: [usize; 8] = std::array::from_fn(|i| read(internal, 0x20 + i * 8));
    if owner >= 8 || primary[owner as usize] != entry as usize {
        return None;
    }
    let owners = layout.owners();
    let original = [
        entry_observation(primary[owners[0] as usize] as *const u8, owners[0])?,
        entry_observation(primary[owners[1] as usize] as *const u8, owners[1])?,
        entry_observation(primary[owners[2] as usize] as *const u8, owners[2])?,
        entry_observation(primary[owners[3] as usize] as *const u8, owners[3])?,
    ];
    let current = Snapshot {
        layout,
        manager: manager as usize,
        internal: internal as usize,
        context: context as usize,
        service,
        service_inner,
        listeners: read(service_inner as *const u8, 0x930),
        count: read(internal, 0xa0),
        quantity: match layout {
            Layout::Training => Some(native_source::training_cpu_quantity()?),
            #[cfg(any(feature = "source-ldn-capacity-empty-probe", feature = "managed-ldn-third-source-alpha"))]
            Layout::LdnWaiting => None,
        },
        #[cfg(any(feature = "source-ldn-capacity-empty-probe", feature = "managed-ldn-third-source-alpha"))]
        ldn: None,
        // Preserve observed rule bytes and ratio bits without reinterpreting them.
        rules: [read(internal, 0xea), read(internal, 0xeb)],
        ratio_bits: read(internal, 0xf0),
        primary,
        backup: std::array::from_fn(|i| read(internal, 0x60 + i * 8)),
        resources: std::array::from_fn(|i| read(context, i * 8)),
        aliases: std::array::from_fn(|i| read(context, 0xe8 + i * 8)),
        selection: std::array::from_fn(|i| read((base + 0x52c5730 + i * 0x260) as *const u8, 0xa)),
        original,
    };
    #[cfg(any(feature = "source-ldn-capacity-empty-probe", feature = "managed-ldn-third-source-alpha"))]
    if layout == Layout::LdnWaiting {
        // A failed supplemental read remains visible. It must not discard the
        // current native owner needed to bind its eventual retirement.
        return Some(Snapshot {
            ldn: Some(LdnFields {
                selection: std::array::from_fn(|i| {
                    let row = (base + 0x52c5730 + i * 0x260) as *const u8;
                    (read(row, 0xa), read(row, 0x79), read(row, 0xa7))
                }),
                resources: original_resources(current),
                input: inputs().ok(),
            }),
            ..current
        });
    }
    Some(current)
}

pub(super) fn creation_delta(before: Snapshot, after: Snapshot) -> bool {
    let mut expected = before;
    expected.primary[SLOT] = after.primary[SLOT];
    expected.resources[SLOT] = after.resources[SLOT];
    expected.aliases[SLOT] = after.aliases[SLOT];
    expected.count += 1;
    let Some(listeners) = expected.listeners.checked_add(1) else {
        return false;
    };
    expected.listeners = listeners;
    expected == after
        && after.primary[SLOT] != 0
        && !before.primary.contains(&after.primary[SLOT])
        && !before.resources.contains(&after.resources[SLOT])
        && !before.aliases.contains(&after.resources[SLOT])
}

#[cfg(any(
    feature = "source-capacity-link-probe",
    feature = "managed-third-source-alpha"
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct InputSlot {
    pub(super) pointer: usize,
    pub(super) state: u8,
    pub(super) key: i32,
    pub(super) owner: i32,
    pub(super) alias: i32,
}

#[cfg(any(
    feature = "source-capacity-link-probe",
    feature = "managed-third-source-alpha"
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Inputs {
    pub(super) service: usize,
    pub(super) data: usize,
    pub(super) records: usize,
    pub(super) mapping: usize,
    pub(super) map7: i32,
    pub(super) controller: usize,
    pub(super) controller_property: usize,
    pub(super) controller_flag: u8,
    pub(super) css_role: u8,
    pub(super) cpu_flag: u8,
    pub(super) slots: [InputSlot; 16],
}

#[cfg(any(
    feature = "source-capacity-link-probe",
    feature = "managed-third-source-alpha"
))]
impl Inputs {
    // Native313e90 supplies the occupancy/alias predicates. Also reject an
    // active control bound to either record through its owner at+160, and
    // require null pointers/unassigned keys for the two records it clears.
    // No meaning is invented for the other state-byte values.
    pub(super) fn unclaimed(self) -> bool {
        [7, 15]
            .iter()
            .all(|i| self.slots[*i].pointer == 0 && self.slots[*i].key == -1)
            && !self.slots.iter().enumerate().any(|(i, slot)| {
                slot.state == 1
                    && slot.key != -1
                    && slot.pointer != 0
                    && (matches!(i, 7 | 15)
                        || matches!(slot.owner, 7 | 15)
                        || matches!(slot.alias, 7 | 15))
            })
    }
}

#[cfg(any(
    feature = "source-capacity-link-probe",
    feature = "managed-third-source-alpha"
))]
pub(super) unsafe fn inputs() -> Result<Inputs, (&'static str, usize, usize)> {
    let base = BASE.load(Ordering::Acquire);
    let service = read::<*const u8>(base as *const u8, 0x52b6fd8);
    if !aligned(service) {
        return Err((
            "current native input service unavailable",
            service as usize,
            0,
        ));
    }
    let active = read::<u8>(service, 8);
    if active != 1 {
        return Err((
            "current native input service is not active",
            service as usize,
            usize::from(active),
        ));
    }
    let data = read::<*const u8>(service, 0);
    if !aligned(data) {
        return Err((
            "current native input data identity unavailable",
            service as usize,
            data as usize,
        ));
    }
    let records = read::<*const u8>(data, 0x90);
    let mapping = read::<*const u8>(base as *const u8, 0x52c50f0);
    if !aligned(records) || !aligned(mapping) {
        return Err((
            "current native input records or owner-map identity unavailable",
            records as usize,
            mapping as usize,
        ));
    }
    let mut slots = [InputSlot {
        pointer: 0,
        state: 0,
        key: -1,
        owner: -1,
        alias: -1,
    }; 16];
    for (index, slot) in slots.iter_mut().enumerate() {
        slot.pointer = read(data, index * 8);
        slot.state = read(data, 0x80 + index);
        slot.key = read(records, index * 0x280 + 0x28);
        // Native cleanup dereferences only these current active records.
        if slot.state == 1 && slot.key != -1 && slot.pointer != 0 {
            let control = slot.pointer as *const u8;
            if !aligned(control) {
                return Err((
                    "active input control pointer is unaligned",
                    index,
                    slot.pointer,
                ));
            }
            slot.owner = read(control, 0x160);
            slot.alias = read(control, 0xc50);
        }
    }
    let map7 = read::<i32>(mapping, 0x298 + SLOT * 4);
    let controller = if (map7 as u32) <= 9 {
        read::<*const u8>((base + 0x5339860) as *const u8, map7 as usize * 8)
    } else {
        (base + 0x5339830) as *const u8
    };
    // The native constructor permits a null table member and skips its
    // property read; out-of-range mappings use the native fallback object.
    let (controller_property, controller_flag) = if controller.is_null() {
        (0, 0)
    } else {
        if !aligned(controller) {
            return Err((
                "current mapped controller is unaligned",
                map7 as u32 as usize,
                controller as usize,
            ));
        }
        let property = read::<*const u8>(controller, 0x18);
        if !aligned(property) {
            return Err((
                "current mapped controller property is unavailable",
                controller as usize,
                property as usize,
            ));
        }
        (property as usize, read::<u8>(property, 8))
    };
    Ok(Inputs {
        service: service as usize,
        data: data as usize,
        records: records as usize,
        mapping: mapping as usize,
        map7,
        controller: controller as usize,
        controller_property,
        controller_flag,
        css_role: read((base + 0x52c5730 + SLOT * 0x260) as *const u8, 0x1ba),
        cpu_flag: read(data, 0xd0),
        slots,
    })
}

#[cfg(any(
    feature = "source-capacity-link-probe",
    feature = "managed-third-source-alpha"
))]
pub(super) unsafe fn original_resources(current: Snapshot) -> Option<[(u32, u32); 4]> {
    let mut fields = [(0, 0); 4];
    for (index, field) in fields.iter_mut().enumerate() {
        let owner = current.resources[current.layout.owners()[index] as usize] as *const u8;
        if !aligned(owner) || read::<usize>(owner, 0) != BASE.load(Ordering::Acquire) + 0x509fb10 {
            return None;
        }
        *field = (read(owner, 0x58), read(owner, 0x64));
    }
    Some(fields)
}

#[cfg(any(
    feature = "source-capacity-link-probe",
    feature = "managed-third-source-alpha"
))]
#[cfg(all(not(feature = "source-capacity-group-probe"), any(
    feature = "source-capacity-link-probe", not(feature = "managed-grouped-source-alpha")
)))]
pub(super) unsafe fn ordinary_actor(entry: *const u8, kind: u32) -> Option<(usize, usize)> {
    let actor = read::<*const u8>(entry, 0x4178);
    if !aligned(actor)
        || read::<usize>(actor, 0) != BASE.load(Ordering::Acquire) + 0x4f85d30
        || read::<u32>(actor, 8) != SLOT as u32
        || read::<u32>(actor, 0x6c) != SLOT as u32
        || read::<u8>(actor, 0x68) != 0
        || read::<u8>(actor, 0x69) != 1
        || read::<u8>(actor, 0x70) != 0
        || read::<u32>(actor, 0x74) != kind
    {
        return None;
    }
    let control = read::<*const u8>(actor, 0x58);
    let information = read::<*const u8>(entry, 0xf8);
    if !aligned(control)
        || !aligned(information)
        || read::<i32>(control, 0x160) != SLOT as i32
        || read::<u8>(information, 0x92) != 0
    {
        return None;
    }
    Some((actor as usize, read(control, 0x168)))
}

unsafe fn verify_creation(base: usize) -> Result<(), usize> {
    // FNV-1a fingerprints of exact game13.0.5 instruction ranges. Full helper
    // identity is checked without embedding a native function body in source.
    for (offset, len, expected) in [
        (0x1652f40, 20, 0xc885cab694e69bc8u64),
        (0x4eb6a0, 224, 0xf0083382faa7d480),
        (0x1755e58, 52, 0x6b1cbd1a14a8a68c),
        (0x651640, 28, 0xf1df3a8ba145274c),
        (0x652214, 16, 0xd5869a938fb63fa8),
        (0x17e44b0, 48, 0x513f7e02e0de5031),
        (0x17eb000, 32, 0xb91b0e94d893f84f),
    ] {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in std::slice::from_raw_parts((base + offset) as *const u8, len) {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
        if hash != expected {
            return Err(offset);
        }
    }
    let mut symbol = 0usize;
    skyline::nn::ro::LookupSymbol(
        &mut symbol,
        b"_ZN3app16kiiladarzmanager20entry_summon_fighterEi\0".as_ptr(),
    );
    if symbol != base + 0x1652f40 {
        return Err(0x1652f40);
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) enum Claimant {
    #[cfg(feature = "source-capacity-probe")]
    Probe = 1,
    #[cfg(feature = "managed-third-source-alpha")]
    Managed = 2,
}
static CLAIM: AtomicU8 = AtomicU8::new(0);
static CLAIM_OWNER: AtomicUsize = AtomicUsize::new(0);
static CREATION_VERIFIED: AtomicBool = AtomicBool::new(false);
#[cfg(any(
    feature = "source-capacity-link-probe",
    feature = "managed-third-source-alpha"
))]
static LOAD_VERIFIED: AtomicBool = AtomicBool::new(false);

#[cfg(any(
    feature = "source-capacity-link-probe",
    feature = "managed-third-source-alpha"
))]
pub(super) fn load_supported() -> bool {
    LOAD_VERIFIED.load(Ordering::Acquire)
}

/// All clients verify the pristine ranges before either shared hook is installed.
#[cfg(any(
    feature = "source-capacity-link-probe",
    feature = "managed-third-source-alpha"
))]
unsafe fn verify_load(base: usize) -> bool {
    for (offset, len, expected) in [
        (0x653240, 592, 0xee56df013a817f73u64),
        (0x654dc0, 704, 0x549eb59c76d87db2),
        (0x655320, 912, 0xe0a543092d07ab2a),
        (0x6b6f10, 2572, 0xdfd785a8a55d9fcf),
        (0x6b7920, 132, 0xcb03f13e76de2a80),
        (0x313e90, 796, 0x8b2b8d1c33b31bc6),
        (0x17e4700, 80, 0x5d4213a39662f638),
        #[cfg(feature = "managed-third-source-alpha")]
        (0x150fc20, 1316, 0xb240697c885cdbeb),
        #[cfg(feature = "managed-third-source-alpha")]
        (0x15095cc, 36, 0x40be917c207acaf7),
        #[cfg(feature = "managed-third-source-alpha")]
        (0x65bd80, 132, 0x505c394d7cdcaa02),
        #[cfg(any(feature = "source-capacity-aegis-probe", feature = "source-capacity-trainer-probe", feature = "managed-grouped-source-alpha"))]
        (0x453c158, 24, 0x74162f9ac96d0337),
        #[cfg(any(feature = "source-capacity-group-probe", feature = "managed-grouped-source-alpha"))]
        (0x651660, 260, 0x871c1dd21365e5e3),
        #[cfg(any(feature = "source-capacity-group-probe", feature = "managed-grouped-source-alpha"))]
        (0x6b7aa0, 52, 0x7009ea488edbd02a),
    ] {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in std::slice::from_raw_parts((base + offset) as *const u8, len) {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
        if hash != expected {
            crate::logging::diagnostic(format_args!(
                "[NativeSourceCapacity] load contract mismatch +0x{:x} len={} expected=0x{:016x} actual=0x{:016x}; load clients disabled",
                offset, len, expected, hash));
            return false;
        }
    }
    #[cfg(any(feature = "source-capacity-group-probe", feature = "managed-grouped-source-alpha"))]
    if read::<usize>((base + 0x4f85d30) as *const u8, 0) != base + 0x6b7920
        || read::<usize>((base + 0x4f85d30) as *const u8, 8) != base + 0x6b7aa0
    {
        crate::logging::diagnostic(format_args!(
            "[NativeSourceCapacity] grouped actor vtable mismatch; load disabled"));
        return false;
    }
    true
}

#[skyline::hook(offset = 0x652214, inline)]
unsafe fn listener_removed(ctx: &InlineCtx) {
    #[cfg(feature = "source-capacity-probe")]
    super::source_capacity_probe::listener_removed(ctx);
    #[cfg(feature = "managed-third-source-alpha")]
    super::sources::capacity_listener_removed(ctx);
}

#[cfg(any(
    feature = "source-capacity-link-probe",
    feature = "managed-third-source-alpha"
))]
#[skyline::hook(offset = 0x6b79a0, inline)]
unsafe fn actor_cleanup(ctx: &InlineCtx) {
    #[cfg(feature = "source-capacity-link-probe")]
    super::source_capacity_probe::actor_cleanup(ctx);
    #[cfg(feature = "managed-third-source-alpha")]
    super::sources::capacity_actor_cleanup(ctx);
}

#[cfg(feature = "source-capacity-probe")]
pub(super) fn creation_supported() -> bool {
    CREATION_VERIFIED.load(Ordering::Acquire)
}

/// The caller also owns INSTALL. Retain this claim until actual owner cleanup;
/// failed post-call qualification deliberately cannot rearm another creator.
pub(super) unsafe fn create(claimant: Claimant) -> bool {
    if !CREATION_VERIFIED.load(Ordering::Acquire) {
        return false;
    }
    if CLAIM
        .compare_exchange(0, claimant as u8, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }
    let create: unsafe extern "C" fn(i32) =
        std::mem::transmute(BASE.load(Ordering::Acquire) + 0x1652f40);
    create(SLOT as i32);
    true
}

#[cfg(feature = "managed-third-source-alpha")]
pub(super) fn claimed() -> bool {
    CLAIM.load(Ordering::Acquire) != 0
}

pub(super) fn bind_owner(claimant: Claimant, owner: usize) {
    if owner != 0 && CLAIM.load(Ordering::Acquire) == claimant as u8 {
        CLAIM_OWNER.store(owner, Ordering::Release);
    }
}

#[skyline::hook(offset = 0x17e44c4, inline)]
unsafe fn owner_cleanup_complete(ctx: &InlineCtx) {
    #[cfg(feature = "source-capacity-probe")]
    super::source_capacity_probe::owner_cleanup_complete(ctx);
    let owner = ctx.registers[19].x() as usize;
    #[cfg(feature = "managed-third-source-alpha")]
    let managed_owner = super::sources::capacity_owner_retired(owner);
    let released = owner != 0
        && CLAIM_OWNER
            .compare_exchange(owner, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
    if released {
        CLAIM.store(0, Ordering::Release);
    }
    #[cfg(feature = "source-ldn-capacity-empty-probe")]
    super::source_capacity_probe::claim_retired(
        owner, released, CLAIM.load(Ordering::Acquire), CLAIM_OWNER.load(Ordering::Acquire),
    );
    #[cfg(feature = "managed-third-source-alpha")]
    if managed_owner {
        crate::logging::diagnostic(format_args!(
            "[ManagedSourceCleanup] claim owner=0x{:x} released={} claim={} bound_owner=0x{:x}",
            owner,
            released,
            CLAIM.load(Ordering::Acquire),
            CLAIM_OWNER.load(Ordering::Acquire)
        ));
    }
}

pub(super) unsafe fn init(base: usize) {
    if let Err(offset) = verify_creation(base) {
        crate::logging::diagnostic(format_args!(
            "[NativeSourceCapacity] creation contract mismatch +0x{:x}; disabled",
            offset
        ));
        return;
    }
    #[cfg(any(
        feature = "source-capacity-link-probe",
        feature = "managed-third-source-alpha"
    ))]
    let load_verified = verify_load(base);
    // Both original ranges have now been checked, before any overlapping patch.
    skyline::install_hooks!(owner_cleanup_complete, listener_removed);
    #[cfg(any(
        feature = "source-capacity-link-probe",
        feature = "managed-third-source-alpha"
    ))]
    if load_verified {
        skyline::install_hooks!(actor_cleanup);
        LOAD_VERIFIED.store(true, Ordering::Release);
    }
    CREATION_VERIFIED.store(true, Ordering::Release);
    #[cfg(feature = "managed-third-source-alpha")]
    managed::init();
}

#[cfg(feature = "managed-third-source-alpha")]
#[path = "source_descriptor.rs"]
mod source_descriptor;

#[cfg(feature = "managed-third-source-alpha")]
pub(super) mod managed {
    use super::*;
    static ENABLED: AtomicBool = AtomicBool::new(false);

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct Preflight {
        scene: Snapshot,
        input: Inputs,
        resources: [(u32, u32); 4],
        #[cfg(feature = "managed-grouped-source-alpha")]
        previous: Option<u32>,
        #[cfg(feature = "managed-grouped-source-alpha")]
        visible: [Option<canonical::VisibleBank>; 2],
        #[cfg(feature = "managed-visible-grouped-source-alpha")]
        initial_visible: [Option<canonical::CreationVisible>; 2],
        #[cfg(any(feature = "source-ldn-grouped-helper-capacity-probe", feature = "managed-grouped-source-alpha"))]
        parked_groups: [Option<ParkedGroup>; 2],
    }

    // Temporary experiment evidence, captured only by pending creation/load work.
    // Actor pointers are compared as numbers, never decoded as ordinary actors.
    #[cfg(any(feature = "source-ldn-grouped-helper-capacity-probe", feature = "managed-grouped-source-alpha"))]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct ParkedGroup {
        descriptor: [u32; 28],
        form: [u8; 5],
        bodies: [(usize, u32, u32, u64, u8, usize); 3],
        actors: [usize; 5],
    }

    #[cfg(any(feature = "source-ldn-grouped-helper-capacity-probe", feature = "managed-grouped-source-alpha"))]
    unsafe fn parked_groups(scene: Snapshot) -> Option<[Option<ParkedGroup>; 2]> {
        let mut groups = [None; 2];
        if !cfg!(feature = "managed-grouped-source-alpha") && matches!(scene.layout, Layout::Training) {
            return Some(groups);
        }
        for (index, output) in groups.iter_mut().enumerate() {
            let owner = scene.layout.owners()[index + 2];
            let observed = scene.original[index + 2];
            if observed.count == 1 {
                continue;
            }
            let entry = native_source::entry(owner)?;
            let kind = observed.kinds[0];
            let forms = super::super::Forms::starting(kind);
            if scene.primary[owner as usize] != entry as usize
                || !(2..=3).contains(&forms.count)
                || observed.count != forms.count
                || observed.kinds != forms.kinds
                || observed.installed != u64::from(forms.count)
                || read::<u8>(entry, 0x591f) != forms.flags
                || native_source::ready(entry, owner, kind).is_none()
            {
                return None;
            }
            let base = BASE.load(Ordering::Acquire);
            let mut bodies = [(0, 0, 0, 0, 0, 0); 3];
            for (index, body_fields) in bodies.iter_mut().enumerate().take(forms.count as usize) {
                let body = read::<*const u8>(entry, 0x4160 + index * 8);
                if !aligned(body) || read::<usize>(body, 0) != base + 0x4f80210 {
                    return None;
                }
                *body_fields = (
                    body as usize,
                    read(body, 8),
                    read(body, 0xc),
                    read(body, 0x10),
                    read(body, 0x3a),
                    read(body, 0x20),
                );
            }
            let group = ParkedGroup {
                descriptor: descriptor(entry),
                form: [0x5918, 0x5919, 0x591a, 0x591f, 0x5934].map(|offset| read(entry, offset)),
                bodies,
                actors: [0x4178, 0x4180, 0x4188, 0x4190, 0x4198].map(|offset| read(entry, offset)),
            };
            if native_source::entry(owner) != Some(entry)
                || entry_observation(entry, owner) != Some(observed)
            {
                return None;
            }
            *output = Some(group);
        }
        Some(groups)
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) struct Reservation {
        pub(crate) entry: usize,
        pub(crate) resource: usize,
        pub(crate) entry_destroyed: bool,
        pub(crate) owner_retired: bool,
        scene: Snapshot,
        input: Inputs,
        descriptor: [u32; 28],
    }

    #[derive(Clone, Copy, Debug)]
    pub(crate) struct ActorCleanup {
        pub(crate) actor: usize,
        #[cfg(feature = "managed-grouped-source-alpha")]
        pub(crate) index: usize,
        #[cfg(feature = "managed-grouped-source-alpha")]
        pub(crate) record: Option<canonical::Actor>,
        pub(crate) kind: u32,
        pub(crate) identity: bool,
        pub(crate) resource_associated: bool,
        pub(crate) returned: bool,
        pub(in crate::character_roster) input: Result<Inputs, (&'static str, usize, usize)>,
        pub(crate) valid: bool,
    }

    /// Called only by the native ordinary actor destructor, before entry removal.
    /// The current entry still owns +4178. No saved pointer is dereferenced.
    #[cfg(not(feature = "managed-grouped-source-alpha"))]
    pub(crate) unsafe fn actor_cleanup_observation(
        ctx: &InlineCtx,
        reservation: Reservation,
    ) -> Option<ActorCleanup> {
        if reservation.entry_destroyed || reservation.owner_retired {
            return None;
        }
        let current = native_source::entry(SLOT as u32)?;
        let actor = ctx.registers[20].x() as *const u8;
        if current as usize != reservation.entry
            || !aligned(actor)
            || read::<usize>(current, 0x4178) != actor as usize
        {
            return None;
        }
        // The current entry owns this actor. Report the independent resource
        // association even if native teardown has already changed its alias.
        let resource_associated = native_source::resource_owner(SLOT as u32).map(|p| p as usize)
            == Some(reservation.resource);
        let base = BASE.load(Ordering::Acquire);
        let kind = read::<u32>(actor, 0x74);
        let identity = read::<usize>(actor, 0) == base + 0x4f85d30
            && read::<u32>(actor, 8) == SLOT as u32
            && read::<u32>(actor, 0x6c) == SLOT as u32;
        let returned = ctx.registers[30].x() as usize == base + 0x6b79a0;
        let input = inputs();
        let valid = identity
            && resource_associated
            && returned
            && input.is_ok_and(|input| {
                input.unclaimed()
                    && input.service == reservation.input.service
                    && input.data == reservation.input.data
                    && input.records == reservation.input.records
            });
        Some(ActorCleanup {
            actor: actor as usize,
            kind,
            identity,
            resource_associated,
            returned,
            input,
            valid,
        })
    }

    #[cfg(feature = "managed-grouped-source-alpha")]
    pub(crate) unsafe fn actor_cleanup_observation(
        ctx: &InlineCtx, reservation: Reservation, kind: u32, expected: Option<Actors>, first: Option<Actor>,
    ) -> Option<ActorCleanup> {
        if reservation.entry_destroyed || reservation.owner_retired { return None; }
        let current = native_source::entry(7)?;
        if current as usize != reservation.entry { return None; }
        let (index, record, identity) = canonical::cleanup(ctx, current, kind, expected, first)?;
        let resource_associated = native_source::resource_owner(7).map(|p| p as usize)
            == Some(reservation.resource);
        let returned = ctx.registers[30].x() as usize == BASE.load(Ordering::Acquire) + 0x6b79a0;
        let input = inputs();
        let valid = identity && resource_associated && returned && input.is_ok_and(|input|
            input.unclaimed() && input.service == reservation.input.service
                && input.data == reservation.input.data && input.records == reservation.input.records
                && input.slots[7].state == 2 && input.slots[15].state == 2);
        Some(ActorCleanup { actor: ctx.registers[20].x() as usize, index, record,
            kind: record.map_or(u32::MAX, |actor| actor.kind), identity,
            resource_associated, returned, input, valid })
    }

    /// Current service root at the native entry destructor/removal boundary.
    pub(crate) unsafe fn listener_count(reservation: Reservation) -> Option<u64> {
        let service = read::<*const u8>(BASE.load(Ordering::Acquire) as *const u8, 0x52c50f8);
        if !aligned(service) || service as usize != reservation.scene.service {
            return None;
        }
        let inner = read::<*const u8>(service, 0);
        (aligned(inner) && inner as usize == reservation.scene.service_inner)
            .then(|| read::<u64>(inner, 0x930))
    }

    pub(crate) unsafe fn listener_after(ctx: &InlineCtx, reservation: Reservation) -> Option<u64> {
        if ctx.registers[19].x() as usize != reservation.entry
            || ctx.registers[21].x() as usize != reservation.scene.service
        {
            return None;
        }
        listener_count(reservation)
    }

    /// The opt-in path accepts catalog sources with a proven native actor plan.
    /// It does not change the selectable catalog or coalesce form resource keys.
    pub(crate) fn source_kind(kind: u32) -> bool {
        #[cfg(feature = "managed-grouped-source-alpha")]
        { canonical::supported(kind) }
        #[cfg(not(feature = "managed-grouped-source-alpha"))]
        { native_source::ordinary_kind(kind) }
    }

    #[cfg(feature = "managed-grouped-source-alpha")]
    pub(crate) use super::canonical::{Actor, Actors};

    #[cfg(feature = "managed-grouped-source-alpha")]
    pub(crate) unsafe fn actors(entry: *const u8, kind: u32) -> Option<Actors> {
        canonical::ready(entry, kind)
    }

    pub(crate) fn enabled() -> bool {
        ENABLED.load(Ordering::Acquire)
    }

    unsafe fn training() -> bool {
        enabled()
            && crate::character_lifecycle::native_hooks_ready()
            && common::is_native_training_mode()
            && matches!(common::game_mode::current(), Some(mode)
                if mode.initialized && mode.melee == 0x20 && mode.mode == 0x04040000)
    }

    unsafe fn layout_permitted(layout: Layout) -> bool {
        match layout {
            Layout::Training => training(),
            #[cfg(any(feature = "source-ldn-capacity-empty-probe", feature = "managed-ldn-third-source-alpha"))]
            Layout::LdnWaiting => cfg!(feature = "managed-ldn-third-source-alpha")
                && enabled() && crate::character_lifecycle::native_hooks_ready()
                && matches!(common::game_mode::current(), Some(mode)
                    if mode.initialized && mode.melee == 0x0d && mode.mode == 0x02010000)
                && super::super::lobby::active(),
        }
    }

    unsafe fn current_layout() -> Option<Layout> {
        if training() { return Some(Layout::Training); }
        #[cfg(feature = "managed-ldn-third-source-alpha")]
        if layout_permitted(Layout::LdnWaiting) { return Some(Layout::LdnWaiting); }
        None
    }

    unsafe fn current_snapshot(
        layout: Layout, ctx: &ServiceEntryContext, entry: *const u8, slot: u32,
    ) -> Option<Snapshot> {
        // Preserve the legacy Training wrapper, including its mandatory quantity.
        if layout == Layout::Training { snapshot(ctx, entry, slot) }
        else { snapshot_for(layout, ctx, entry, slot) }
    }

    /// The witnessed LDN defaults, with strict count4 admission still authoritative.
    /// Active consumers are allowed; their source leases prevent replacement.
    #[cfg(any(feature = "source-ldn-capacity-empty-probe", feature = "managed-ldn-third-source-alpha"))]
    unsafe fn ldn_creation_shape(scene: Snapshot) -> bool {
        if !cfg!(feature = "managed-ldn-third-source-alpha") || scene.quantity.is_some()
            || !native_source::exact_ldn_waiting_layout(scene.primary[5] as *const u8, 5)
            || [1usize, 2, 3, 7].iter().any(|i| scene.resources[*i] != 0 || scene.aliases[*i] != 0)
        { return false; }
        let Some(fields) = scene.ldn else { return false; };
        let Some(input) = fields.input else { return false; };
        fields.resources.is_some()
            && fields.selection == [(0,0,0), (2,0x64,1), (2,0x64,2), (2,0x64,3),
                (1,1,4), (1,1,5), (1,1,6), (2,0x64,7)]
            && input.unclaimed() && input.map7 == -1
            && input.controller == BASE.load(Ordering::Acquire) + 0x5339830
            && input.controller_flag == 0 && input.css_role == 0 && input.cpu_flag == 1
            && [7,15].iter().all(|i| input.slots[*i].state == 2)
    }

    unsafe fn descriptor(entry: *const u8) -> [u32; 28] {
        std::array::from_fn(|i| read(entry, i * 4))
    }

    /// Reset's native all-entry loop writes borrowed configuration references
    /// before its visible-quantity cutoff. Derive them from current original
    /// entries, never from retained pointers or by dereferencing either value.
    unsafe fn training_reset_references(
        ctx: &ServiceEntryContext,
        current: *const u8,
        reservation: Reservation,
    ) -> Option<[[u64; 2]; 4]> {
        if reservation.scene.layout != Layout::Training {
            return None;
        }
        let scene = current_snapshot(Layout::Training, ctx, current, 7)?;
        let mut references = [[0; 2]; 4];
        for (owner, references) in references.iter_mut().enumerate() {
            let original = native_source::entry(owner as u32)?;
            if original as usize != scene.primary[owner]
                || original as usize != reservation.scene.primary[owner]
            {
                return None;
            }
            *references = [read(original, 0x58), read(original, 0x60)];
        }
        // Revalidate roots and the callback after reads. Only numeric identities
        // are retained; neither borrowed configuration pointer is followed.
        (owned(reservation, current)
            && current_snapshot(Layout::Training, ctx, current, 7) == Some(scene))
            .then_some(references)
    }

    unsafe fn empty(entry: *const u8, resource: *const u8) -> bool {
        entry_observation(entry, SLOT as u32).is_some_and(|entry| {
            entry.count == 1
                && entry.kinds == [u32::MAX; 3]
                && entry.installed == 0
                && entry.phase == [0, 14, 0]
        }) && read::<u32>(entry, 0x28) == 1
            && aligned(resource)
            && read::<usize>(resource, 0) == BASE.load(Ordering::Acquire) + 0x509fb10
            && read::<u32>(resource, 0x58) == u32::MAX
    }

    /// Called only while an unsatisfied demand cannot replace either native
    /// helper. Initial grouped visible originals require the separate alpha
    /// feature and a current canonical actor/control snapshot preserved by creation.
    pub(crate) unsafe fn preflight(
        ctx: &ServiceEntryContext,
        entry: *const u8,
        slot: u32,
    ) -> Option<Preflight> {
        let layout = current_layout()?;
        let owners = layout.owners();
        if claimed() || !owners[2..].contains(&slot) { return None; }
        let scene = current_snapshot(layout, ctx, entry, slot)?;
        let shape = match layout {
            Layout::Training => scene.quantity == Some(1)
                && scene.primary[4..].iter().all(|p| *p == 0),
            #[cfg(any(feature = "source-ldn-capacity-empty-probe", feature = "managed-ldn-third-source-alpha"))]
            Layout::LdnWaiting => ldn_creation_shape(scene),
        };
        if scene.count != 4 || !shape
            || scene.backup.iter().any(|p| *p != 0)
            || scene.resources[SLOT] != 0
            || scene.aliases[SLOT] != 0
            || scene.selection[SLOT] != 2
        {
            return None;
        }
        for (index, owner) in owners.iter().copied().enumerate() {
            let current = scene.primary[owner as usize] as *const u8;
            let observed = scene.original[index];
            let ordinary = observed.count == 1
                && observed.installed == 1
                && native_source::ordinary_kind(observed.kinds[0]);
            #[cfg(any(feature = "source-ldn-grouped-helper-capacity-probe", feature = "managed-grouped-source-alpha"))]
            let grouped_helper =
                (cfg!(feature = "managed-grouped-source-alpha") || !matches!(layout, Layout::Training))
                && index >= 2 && (2..=3).contains(&observed.count);
            #[cfg(not(any(feature = "source-ldn-grouped-helper-capacity-probe", feature = "managed-grouped-source-alpha")))]
            let grouped_helper = false;
            let grouped_visible = cfg!(feature = "managed-visible-grouped-source-alpha")
                && index < 2
                && (2..=3).contains(&super::super::Forms::starting(observed.kinds[0]).count);
            if !(ordinary || grouped_helper || grouped_visible)
                || scene.aliases[owner as usize] != scene.resources[owner as usize]
            {
                return None;
            }
            if index < 2 {
                if !super::super::ready_forms(current, owner)
                    .is_some_and(|(forms, _)| grouped_visible
                        || (forms.count == 1 && forms.actor_count == 1))
                {
                    return None;
                }
            } else if !native_source::parked_ready(current, owner, observed.kinds[0]) {
                return None;
            }
            #[cfg(feature = "managed-ldn-third-source-alpha")]
            if layout == Layout::LdnWaiting && index >= 2
                && native_source::ready(current, owner, observed.kinds[0]).is_none()
            { return None; }
        }
        let resources = original_resources(scene)?;
        if resources
            .iter()
            .enumerate()
            .any(|(i, resource)| resource.0 != scene.original[i].kinds[0])
        {
            return None;
        }
        let input = inputs().ok()?;
        #[cfg(feature = "managed-grouped-source-alpha")]
        if !canonical::preflight_input(input, None) { return None; }
        input.unclaimed().then_some(Preflight {
            scene,
            input,
            resources,
            #[cfg(feature = "managed-grouped-source-alpha")]
            previous: None,
            #[cfg(feature = "managed-grouped-source-alpha")]
            visible: [None; 2], // Load-time switching authority is captured at each install.
            #[cfg(feature = "managed-visible-grouped-source-alpha")]
            initial_visible: canonical::creation_visible(scene, input)?,
            #[cfg(any(feature = "source-ldn-grouped-helper-capacity-probe", feature = "managed-grouped-source-alpha"))]
            parked_groups: parked_groups(scene)?,
        })
    }

    /// Synchronous creation only. Caller consumes its epoch intent and releases
    /// its mutex before this function, while retaining the shared INSTALL gate.
    pub(crate) unsafe fn create_reserved(
        ctx: &ServiceEntryContext,
        entry: *const u8,
        slot: u32,
        before: Preflight,
    ) -> Option<Reservation> {
        if preflight(ctx, entry, slot) != Some(before) || !create(Claimant::Managed) {
            return None;
        }
        let after = current_snapshot(before.scene.layout, ctx, entry, slot)?;
        // Bind native retirement even when a subsequent observation declines.
        bind_owner(Claimant::Managed, after.resources[SLOT]);
        if !creation_delta(before.scene, after)
            || after.aliases[SLOT] != after.resources[SLOT]
            || original_resources(after) != Some(before.resources)
            || inputs().ok() != Some(before.input)
            || !empty(
                after.primary[SLOT] as *const u8,
                after.resources[SLOT] as *const u8,
            )
        {
            return None;
        }
        #[cfg(any(feature = "source-ldn-grouped-helper-capacity-probe", feature = "managed-grouped-source-alpha"))]
        {
            let current_groups = parked_groups(after);
            let preserved = current_groups == Some(before.parked_groups);
            crate::logging::diagnostic(format_args!(
                "[GroupedHelperCapacity] creation layout={:?} entry=0x{:x} preserved={} before={:x?} after={:x?}",
                after.layout, after.primary[SLOT], preserved, before.parked_groups, current_groups));
            if !preserved {
                return None;
            }
        }
        #[cfg(feature = "managed-visible-grouped-source-alpha")]
        {
            // Native creation is synchronous: permit no original body, actor,
            // control, role, form or descriptor change across this call.
            let current = canonical::creation_visible(after, before.input);
            let preserved = current == Some(before.initial_visible);
            if before.initial_visible.iter().any(Option::is_some) {
                crate::logging::diagnostic(format_args!(
                    "[GroupedVisibleCapacity] creation layout={:?} entry=0x{:x} preserved={} before={:x?} after={:x?}",
                    after.layout, after.primary[SLOT], preserved, before.initial_visible, current));
            }
            if !preserved { return None; }
        }
        Some(Reservation {
            entry: after.primary[SLOT],
            resource: after.resources[SLOT],
            entry_destroyed: false,
            owner_retired: false,
            scene: after,
            input: before.input,
            descriptor: descriptor(after.primary[SLOT] as *const u8),
        })
    }

    fn same_input_role(current: Inputs, before: Inputs) -> bool {
        // Other actors may legitimately change their own key after a hotswap.
        // Each individual install additionally compares the complete input view.
        let mut expected = before;
        expected.slots = current.slots;
        current == expected && current.unclaimed()
    }

    /// Reacquire all roots; retained addresses are equality values only.
    pub(crate) unsafe fn owned(reservation: Reservation, current: *const u8) -> bool {
        owned_checked(reservation, current).is_ok()
    }

    unsafe fn owned_checked(reservation: Reservation, current: *const u8) -> Result<(), (&'static str, usize)> {
        if !layout_permitted(reservation.scene.layout)
            || reservation.entry_destroyed
            || reservation.owner_retired
            || current as usize != reservation.entry
            || native_source::entry(SLOT as u32) != Some(current)
        {
            return Err(("scene/lifetime/current-entry", 7));
        }
        let base = BASE.load(Ordering::Acquire);
        let manager = read::<*const u8>(base as *const u8, 0x52b94f8);
        let context = read::<*const u8>(base as *const u8, 0x5324680);
        let service = read::<*const u8>(base as *const u8, 0x52c50f8);
        if !aligned(manager)
            || manager as usize != reservation.scene.manager
            || !aligned(context)
            || context as usize != reservation.scene.context
            || !aligned(service)
            || service as usize != reservation.scene.service
        {
            return Err(("manager/context/service identity", 7));
        }
        let internal = read::<*const u8>(manager, 0);
        if !aligned(internal)
            || internal as usize != reservation.scene.internal
            || read::<usize>(service, 0) != reservation.scene.service_inner
            || read::<u32>(internal, 0xa0) != 5
            || (reservation.scene.layout == Layout::Training
                && native_source::training_cpu_quantity() != Some(1))
            || [read::<u8>(internal, 0xea), read(internal, 0xeb)] != reservation.scene.rules
            || read::<u32>(internal, 0xf0) != reservation.scene.ratio_bits
        {
            return Err(("internal/count/quantity/rules/ratio", 7));
        }
        for owner in 0..8 {
            if read::<usize>(internal, 0x20 + owner * 8) != reservation.scene.primary[owner]
                || read::<usize>(internal, 0x60 + owner * 8) != 0
                || read::<usize>(context, owner * 8) != reservation.scene.resources[owner]
                || read::<usize>(context, 0xe8 + owner * 8) != reservation.scene.aliases[owner]
                || read::<u8>((base + 0x52c5730 + owner * 0x260) as *const u8, 0xa)
                    != reservation.scene.selection[owner]
            {
                return Err(("original/owned roots, aliases or selection", owner));
            }
        }
        #[cfg(any(feature = "source-ldn-capacity-empty-probe", feature = "managed-ldn-third-source-alpha"))]
        if reservation.scene.layout == Layout::LdnWaiting {
            let Some(fields) = reservation.scene.ldn else { return Err(("LDN snapshot unavailable", 7)); };
            for owner in 0..8 {
                let row = (base + 0x52c5730 + owner * 0x260) as *const u8;
                if (read::<u8>(row, 0xa), read::<u8>(row, 0x79), read::<u8>(row, 0xa7))
                    != fields.selection[owner]
                { return Err(("LDN selection tuple", owner)); }
            }
        }
        let input = inputs().map_err(|_| ("input view unavailable", 7usize))?;
        if !same_input_role(input, reservation.input) {
            return Err(("input service/role identity", 7));
        }
        Ok(())
    }

    /// Only this current managed LDN reservation may admit helper5/6 at count5.
    /// The strict count4 predicate remains unchanged for every unowned scene.
    #[cfg(feature = "managed-ldn-third-source-alpha")]
    pub(crate) unsafe fn owned_ldn_helper(
        reservation: Reservation, current: *const u8, slot: u32,
    ) -> bool {
        if reservation.scene.layout != Layout::LdnWaiting || !matches!(slot, 5 | 6)
            || reservation.scene.primary[slot as usize] != current as usize
            || native_source::entry(slot) != Some(current)
        { return false; }
        let Some(candidate) = native_source::entry(SLOT as u32) else { return false; };
        owned(reservation, candidate)
    }

    /// The same ordered admission checks, with a value-only decline reason.
    /// The ordinary wrapper still returns Option; reasons never authorize work.
    #[derive(Clone, Copy, Debug)]
    pub(crate) enum LoadDecline {
        Owned,
        Descriptor,
        PreviousKind,
        Parked,
        Resource,
        Empty,
        Available,
        #[cfg(feature = "managed-grouped-source-alpha")]
        Canonical,
        Snapshot,
        Inputs,
        #[cfg(feature = "managed-grouped-source-alpha")]
        InputRole,
        OriginalResources,
        #[cfg(any(feature = "source-ldn-grouped-helper-capacity-probe", feature = "managed-grouped-source-alpha"))]
        ParkedGroups,
    }

    pub(crate) unsafe fn load_preflight(
        ctx: &ServiceEntryContext, entry: *const u8, reservation: Reservation, previous: Option<u32>,
    ) -> Option<Preflight> {
        load_preflight_checked(ctx, entry, reservation, previous).ok()
    }

    /// Event-only supplementary reads from this live callback, never saved pointers.
    #[cfg(feature = "source-capacity-probe")]
    pub(in crate::character_roster) type AdmissionDetail = (
        bool, bool, Option<(&'static str, usize)>, Option<[u32; 28]>, Option<bool>, Option<bool>,
        Result<Inputs, (&'static str, usize, usize)>,
    );

    #[cfg(feature = "source-capacity-probe")]
    pub(in crate::character_roster) unsafe fn admission_detail(
        ctx: &ServiceEntryContext, entry: *const u8, previous: u32, reservation: Option<Reservation>,
    ) -> AdmissionDetail {
        let current_entry = native_source::entry(7) == Some(entry);
        let dispatch = current_entry && dispatch_identity(ctx, entry).is_some();
        let parked = dispatch.then(|| native_source::parked_ready(entry, 7, previous));
        #[cfg(feature = "managed-grouped-source-alpha")]
        let canonical = (parked == Some(true)).then(|| canonical::ready(entry, previous).is_some());
        #[cfg(not(feature = "managed-grouped-source-alpha"))]
        let canonical = None;
        let owned_error = reservation.and_then(|owner| owned_checked(owner, entry).err());
        (current_entry, dispatch, owned_error, dispatch.then(|| descriptor(entry)),
            parked, canonical, inputs())
    }

    pub(crate) unsafe fn load_preflight_checked(
        ctx: &ServiceEntryContext,
        entry: *const u8,
        reservation: Reservation,
        previous: Option<u32>,
    ) -> Result<Preflight, LoadDecline> {
        if !owned(reservation, entry) {
            return Err(LoadDecline::Owned);
        }
        let expected_kind = previous.unwrap_or(u32::MAX);
        let mut expected = reservation.descriptor;
        expected[0x18 / 4] = expected_kind;
        #[cfg(feature = "managed-grouped-source-alpha")]
        if let Some(kind) = previous {
            let forms = super::super::Forms::starting(kind);
            expected[0x14 / 4] = forms.count;
            expected[0x18 / 4..0x24 / 4].copy_from_slice(&forms.kinds);
            expected[0x28 / 4] = forms.actor_count;
        }
        expected[0x40 / 4] &= 0x00ff_ffff; // Native install's descriptor byte43=0.
        let current_descriptor = descriptor(entry);
        if current_descriptor != expected {
            // Creation/first load and LDN keep the exact original comparison.
            // A Training replacement may retain only the pair written by native
            // reset to every current entry; every other descriptor word is exact.
            let references =
                previous.and_then(|_| training_reset_references(ctx, entry, reservation));
            if descriptor(entry) != current_descriptor
                || !source_descriptor::matches(
                    current_descriptor,
                    expected,
                    references,
                    (BASE.load(Ordering::Acquire) + 0x52c5730) as u64,
                )
            {
                return Err(LoadDecline::Descriptor);
            }
        }
        if let Some(kind) = previous {
            if !source_kind(kind) { return Err(LoadDecline::PreviousKind); }
            if !native_source::parked_ready(entry, 7, kind) {
                return Err(LoadDecline::Parked);
            }
        } else {
            let resource = native_source::resource_owner(7).ok_or(LoadDecline::Resource)?;
            let available: unsafe extern "C" fn(*const u8) -> bool =
                std::mem::transmute(BASE.load(Ordering::Acquire) + 0x17e4700);
            if !empty(entry, resource) { return Err(LoadDecline::Empty); }
            if !available(resource) { return Err(LoadDecline::Available); }
        }
        #[cfg(feature = "managed-grouped-source-alpha")]
        if previous.is_some_and(|kind| canonical::ready(entry, kind).is_none()) {
            return Err(LoadDecline::Canonical);
        }
        let scene = current_snapshot(reservation.scene.layout, ctx, entry, 7)
            .ok_or(LoadDecline::Snapshot)?;
        let input = inputs().map_err(|_| LoadDecline::Inputs)?;
        #[cfg(feature = "managed-grouped-source-alpha")]
        if !canonical::preflight_input(input, previous) { return Err(LoadDecline::InputRole); }
        #[cfg(feature = "managed-grouped-source-alpha")]
        let visible = canonical::visible_preflight(scene, input);
        Ok(Preflight {
            scene,
            input,
            resources: original_resources(scene).ok_or(LoadDecline::OriginalResources)?,
            #[cfg(feature = "managed-grouped-source-alpha")]
            previous,
            #[cfg(feature = "managed-grouped-source-alpha")]
            visible,
            #[cfg(feature = "managed-visible-grouped-source-alpha")]
            initial_visible: [None; 2], // Creation-only authority; never freezes later native switches.
            #[cfg(any(feature = "source-ldn-grouped-helper-capacity-probe", feature = "managed-grouped-source-alpha"))]
            parked_groups: parked_groups(scene).ok_or(LoadDecline::ParkedGroups)?,
        })
    }

    pub(crate) unsafe fn loaded(
        ctx: &ServiceEntryContext,
        entry: *const u8,
        reservation: Reservation,
        before: Preflight,
        kind: u32,
    ) -> bool {
        if !owned(reservation, entry) {
            return false;
        }
        let Some(current) = current_snapshot(reservation.scene.layout, ctx, entry, 7) else {
            return false;
        };
        #[cfg(any(feature = "source-ldn-grouped-helper-capacity-probe", feature = "managed-grouped-source-alpha"))]
        if parked_groups(current) != Some(before.parked_groups) {
            return false;
        }
        let expected_input = inputs().ok().and_then(|input| load_expected_input(before, kind, input));
        #[cfg(feature = "managed-grouped-source-alpha")]
        let current = canonical::comparison_view(current, before.input, expected_input);
        let mut expected = before.scene;
        expected.listeners = current.listeners; // Native actor/body registration.
        expected == current
            && original_resources(current) == Some(before.resources)
            && expected_input.is_some()
            && ready(reservation, entry, kind)
    }

    unsafe fn load_expected_input(before: Preflight, kind: u32, input: Inputs) -> Option<Inputs> {
        #[cfg(feature = "managed-grouped-source-alpha")]
        { canonical::loaded_visible_input(before.scene, before.input, before.previous, kind, before.visible, input) }
        #[cfg(not(feature = "managed-grouped-source-alpha"))]
        { let _ = kind; (input == before.input).then_some(before.input) }
    }

    // Diagnostic-only copies of the existing load contract. Neither result
    // participates in admission, completion or native ownership release.
    #[cfg(feature = "source-capacity-probe")]
    pub(crate) unsafe fn observe_install_inputs(
        source: crate::source_lifetime::SourceToken,
        previous: Option<(crate::source_lifetime::SourceToken, u32)>,
        kind: u32,
        before: Option<Preflight>,
    ) {
        crate::logging::diagnostic(format_args!(
            "[SourceOverlap] install_commit source={:?} previous(source,kind)={:?} kind=0x{:x} captured_preflight_input={:x?}",
            source, previous, kind, before.map(|before| before.input)));
        #[cfg(feature = "managed-grouped-source-alpha")]
        if let Some(before) = before {
            canonical::observe_visible(source, before.scene, inputs().ok(), before.visible, "commit");
        }
    }

    #[cfg(feature = "source-capacity-probe")]
    pub(crate) unsafe fn observe_load_inputs(
        source: crate::source_lifetime::SourceToken,
        kind: u32,
        event: &str,
        before: Option<Preflight>,
    ) {
        let Some(before) = before else {
            crate::logging::diagnostic(format_args!(
                "[SourceOverlap] load_event source={:?} kind=0x{:x} event={} preflight_unavailable=true",
                source, kind, event));
            return;
        };
        #[cfg(feature = "managed-grouped-source-alpha")]
        let strict_expected = canonical::loaded_input(before.input, before.previous, kind);
        #[cfg(not(feature = "managed-grouped-source-alpha"))]
        let strict_expected = Some(before.input);
        // One bounded read at this terminal event, after the caller released
        // STATE. This is not the earlier read that decided completion.
        let observed = inputs();
        let current = observed.as_ref().ok().copied();
        let qualified = current.and_then(|input| load_expected_input(before, kind, input));
        let expected = qualified.or(strict_expected);
        let mask = |left: Inputs, right: Inputs| {
            left.slots.iter().zip(right.slots).enumerate().fold(0u16, |mask, (index, (a, b))| {
                mask | if *a != b { 1u16 << index } else { 0 }
            })
        };
        let changed_from_preflight = current.map(|current| mask(before.input, current));
        let authorized_visible_mask = strict_expected.zip(qualified).map(|(strict, qualified)| mask(strict, qualified));
        let unexpected_slots = expected.zip(current).map(|(expected, current)| mask(expected, current));
        let non_slot_fields_equal = expected.zip(current).map(|(expected, mut current)| {
            current.slots = expected.slots;
            current == expected
        });
        crate::logging::diagnostic(format_args!(
            "[SourceOverlap] load_event source={:?} kind=0x{:x} event={} event_only_resample=true changed_from_preflight_mask={:x?} unexpected_slots_mask={:x?} non_slot_fields_equal={:?} expected_available={} authorized_visible_mask={:x?} current={:x?}",
            source, kind, event, changed_from_preflight, unexpected_slots, non_slot_fields_equal,
            expected.is_some(), authorized_visible_mask, observed));
        #[cfg(feature = "managed-grouped-source-alpha")]
        if event == "stalled; native ownership retained" {
            canonical::observe_visible(source, before.scene, current, before.visible, "stalled");
        }
    }

    // Fixed field bits documented by the diagnostic contract. This observes the
    // same normalized view as the decision; it never changes a comparison.
    #[cfg(feature = "source-capacity-probe")]
    fn snapshot_difference(expected: Snapshot, current: Snapshot) -> u32 {
        let fields = [
            expected.layout != current.layout,
            expected.manager != current.manager,
            expected.internal != current.internal,
            expected.context != current.context,
            expected.service != current.service,
            expected.service_inner != current.service_inner,
            expected.listeners != current.listeners,
            expected.count != current.count,
            expected.quantity != current.quantity,
            expected.rules != current.rules,
            expected.ratio_bits != current.ratio_bits,
            expected.primary != current.primary,
            expected.backup != current.backup,
            expected.resources != current.resources,
            expected.aliases != current.aliases,
            expected.selection != current.selection,
            expected.original != current.original,
        ];
        let mask = fields.iter().enumerate().fold(0u32, |mask, (index, differs)| {
            mask | if *differs { 1 << index } else { 0 }
        });
        #[cfg(any(feature = "source-ldn-capacity-empty-probe", feature = "managed-ldn-third-source-alpha"))]
        let mask = mask
            | (u32::from(expected.ldn.is_some() != current.ldn.is_some()) << 17)
            | (u32::from(expected.ldn.map(|f| f.selection) != current.ldn.map(|f| f.selection)) << 18)
            | (u32::from(expected.ldn.map(|f| f.resources) != current.ldn.map(|f| f.resources)) << 19)
            | (u32::from(expected.ldn.map(|f| f.input) != current.ldn.map(|f| f.input)) << 20);
        mask
    }

    pub(crate) unsafe fn ready(reservation: Reservation, entry: *const u8, kind: u32) -> bool {
        #[cfg(feature = "managed-grouped-source-alpha")]
        { owned(reservation, entry) && canonical::ready(entry, kind).is_some()
            && native_source::ready(entry, 7, kind).is_some() }
        #[cfg(not(feature = "managed-grouped-source-alpha"))]
        { ready_ordinary(reservation, entry, kind) }
    }

    #[cfg(not(feature = "managed-grouped-source-alpha"))]
    unsafe fn ready_ordinary(reservation: Reservation, entry: *const u8, kind: u32) -> bool {
        owned(reservation, entry)
            && native_source::ordinary_kind(kind)
            && ordinary_actor(entry, kind)
                .is_some_and(|(_, binding)| binding == reservation.input.records + SLOT * 0x280)
            && native_source::ready(entry, 7, kind).is_some()
    }

    /// One diagnostic at the stalled-load boundary. None means a prerequisite
    /// prevented a safe observation; it is not reported as a successful check.
    #[derive(Default)]
    pub(crate) struct LoadChecks {
        pub(crate) reservation: bool,
        pub(crate) preflight: bool,
        pub(crate) owned: bool,
        pub(crate) snapshot: Option<bool>,
        pub(crate) scene: Option<bool>,
        pub(crate) original_resources: Option<bool>,
        pub(crate) inputs: Option<bool>,
        pub(crate) ready: Option<bool>,
    }

    pub(crate) unsafe fn stalled_checks(
        ctx: &ServiceEntryContext,
        entry: *const u8,
        reservation: Option<Reservation>,
        before: Option<Preflight>,
        kind: u32,
    ) -> LoadChecks {
        let mut checks = LoadChecks {
            reservation: reservation.is_some(),
            preflight: before.is_some(),
            ..LoadChecks::default()
        };
        let Some((reservation, before)) = reservation.zip(before) else {
            return checks;
        };
        checks.owned = owned(reservation, entry);
        if !checks.owned {
            return checks;
        }
        let current = current_snapshot(reservation.scene.layout, ctx, entry, 7);
        checks.snapshot = Some(current.is_some());
        let expected_input = inputs().ok().and_then(|input| load_expected_input(before, kind, input));
        if let Some(current) = current {
            #[cfg(feature = "managed-grouped-source-alpha")]
            let current = canonical::comparison_view(current, before.input, expected_input);
            let mut expected = before.scene;
            expected.listeners = current.listeners;
            checks.scene = Some(expected == current);
            #[cfg(feature = "source-capacity-probe")]
            crate::logging::diagnostic(format_args!(
                "[SourceOverlap] stalled_scene entry={:p} kind=0x{:x} normalized_scene_diff_mask=0x{:x} input_qualified={} listeners_normalized=true",
                entry, kind, snapshot_difference(expected, current), expected_input.is_some()));
            checks.original_resources = Some(original_resources(current) == Some(before.resources));
            #[cfg(any(feature = "source-ldn-grouped-helper-capacity-probe", feature = "managed-grouped-source-alpha"))]
            if before.parked_groups.iter().any(Option::is_some) {
                let groups = parked_groups(current);
                crate::logging::diagnostic(format_args!(
                    "[GroupedHelperCapacity] stalled entry={:p} preserved={} before={:x?} current={:x?}",
                    entry, groups == Some(before.parked_groups), before.parked_groups, groups));
            }
        }
        checks.inputs = Some(expected_input.is_some());
        checks.ready = Some(ready(reservation, entry, kind));
        checks
    }

    pub(super) fn init() {
        if !load_supported() {
            crate::logging::diagnostic(format_args!(
                "[NativeSourceCapacity] managed load contract unavailable; disabled"
            ));
            return;
        }
        ENABLED.store(true, Ordering::Release);
        crate::logging::diagnostic(format_args!(
            "[NativeSourceCapacity] managed third-source alpha enabled; native Training; separate_ldn_opt_in={}",
            cfg!(feature = "managed-ldn-third-source-alpha")
        ));
    }
}
