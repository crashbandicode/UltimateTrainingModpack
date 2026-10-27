//! Private 13.0.5 bridge candidate. Not wired into a product build yet.
//!
//! Native state is rebound from callback arguments. Retained addresses are identity
//! tokens only; scene, fighter, widget and pool pointers are never restoration data.
use std::{convert::TryInto, mem::transmute};

pub(super) const MODE: usize = 0x53050f0;
pub(super) const DRAFT: usize = 0x5307b78;

// Required fields belong to the freshly rebound scene callback. Writes below
// are native transactions; a late optional-field refusal could leave one half
// published. Keep their qualification and state checks ahead of the mutation.
pub(super) use crate::native_fields::{
    object as aligned, read_unchecked as read, write_unchecked as write,
};

/// Native FixedString<64>, copied synchronously by the queue. Its hash uses
/// multiply-137 then xor, as shown at 37262f4; this is not standard FNV-1a.
#[repr(C)]
struct Key {
    hash: u32,
    len: u32,
    bytes: [u8; 64],
}

impl Key {
    fn new(name: &[u8]) -> Option<Self> {
        if name.len() >= 64 || name.contains(&0) {
            return None;
        }
        let mut key = Self { hash: 0x811c9dc5, len: name.len() as u32, bytes: [0; 64] };
        key.bytes[..name.len()].copy_from_slice(name);
        for byte in name {
            key.hash = key.hash.wrapping_mul(137) ^ u32::from(*byte);
        }
        Some(key)
    }

    fn empty() -> Self {
        // Native requesters explicitly zero both header words when there is no
        // current child. Hashing an empty string would not produce this sentinel.
        Self { hash: 0, len: 0, bytes: [0; 64] }
    }
}

/// Used only for the current callback's duration. Do not store this value.
pub(super) struct Training {
    pub sequence: usize,
    context: usize,
}

impl Training {
    pub unsafe fn bind(base: usize, context: usize) -> Option<Self> {
        Self::qualify(base, context, false)
    }

    /// The age-zero state-5 call that follows a state-stack transition or the
    /// product reenter. Identity only: no editor logic may run on that call.
    #[cfg(feature = "training-stage-rotation-alpha")]
    pub unsafe fn entering(base: usize, context: usize) -> bool {
        Self::qualify(base, context, true).is_some()
    }

    unsafe fn qualify(base: usize, context: usize, age_zero: bool) -> Option<Self> {
        if !aligned(context) {
            return None;
        }
        let sequence: usize = read(context, 0);
        let scene = sequence.checked_sub(0x50)?;
        if !aligned(scene) || read::<usize>(context, 8) != context
            || read::<usize>(scene, 0xe8) != context
            || read::<usize>(scene, 0) != base + 0x50bad50
            || read::<i32>(context, 0xf0) != 5
            || (read::<u32>(context, 0xf4) == 0) != age_zero
        {
            return None;
        }
        let depth: i32 = read(context, 0xec);
        if !(0..19).contains(&depth)
            || read::<i32>(context, 0xa0 + depth as usize * 4) != 5
        {
            return None;
        }
        Some(Self { sequence, context })
    }

    pub unsafe fn drained(&self) -> bool {
        drained(self.sequence)
    }

    pub unsafe fn reenter(&self) -> bool {
        if !self.drained() { return false; }
        // The native dispatcher consumes this byte and resets the state age.
        // Its complete state-5 entry then queues IngameCall and initializes the
        // Training presentation/speed services, instead of a partial replay.
        write(self.context, 0xfc, 1u8);
        true
    }
}

pub(super) unsafe fn drained(sequence: usize) -> bool {
    let manager: usize = read(sequence, 0x28);
    let queue: usize = read(sequence, 0x20);
    aligned(manager) && aligned(queue)
        && read::<usize>(manager, 0x60) == 0
        && read::<usize>(manager, 0x30) == read::<usize>(manager, 0x38)
        && read::<usize>(queue, 0x10) == 0
}

/// The exact table layout consumed by register3723a90/unregister3723e70.
/// Both operations consume the stack table synchronously and copy native keys.
pub(super) unsafe fn register_editor(base: usize, current: &Training, add: bool) {
    register_editor_sequence(base, current.sequence, add);
}

pub(super) unsafe fn register_editor_sequence(base: usize, sequence: usize, add: bool) {
    let table = [[base + 0x4463822, base + 0x50b59a8], [0, 0]];
    let entry = if add { 0x3723a90 } else { 0x3723e70 };
    let call: unsafe extern "C" fn(usize, *const [usize; 2]) = transmute(base + entry);
    call(sequence, table.as_ptr());
}

/// Current Training sequence must be drained; no invented completion code or
/// jump into the middle of a native state callback. Keys are owned local values.
pub(super) unsafe fn enqueue(base: usize, current: &Training, child: &[u8]) -> bool {
    enqueue_sequence(base, current.sequence, child)
}

pub(super) unsafe fn enqueue_sequence(base: usize, sequence: usize, child: &[u8]) -> bool {
    if !drained(sequence) {
        return false;
    }
    let Some(key) = Key::new(child) else { return false };
    // S+38 is the current child, not the parent. Native drained requesters pass
    // the zero key (23ebdf0), and the registered sequence retains its own owner.
    if read::<usize>(sequence, 0x38) != 0 { return false; }
    let parent = Key::empty();
    let queue: usize = read(sequence, 0x20);
    let call: unsafe extern "C" fn(usize, *const Key, *const Key, usize) =
        transmute(base + 0x37266d0);
    call(queue, &key, &parent, 0);
    // Same publication performed by both native CSS-success and restart paths.
    write(sequence, 0x30, 1u32);
    read::<usize>(queue, 0x10) == 1
}

/// Current UI callback supplies its own object. The native cleanup routine closes
/// its presentation and schedules state22; the next native frame produces Quit5.
/// This is deliberately new command behavior, not a claim of controller replay.
pub(super) unsafe fn request_quit(base: usize, object: usize) -> bool {
    if !aligned(object) || read::<usize>(object, 0) != base + 0x5065b68
        || read::<u8>(base, 0x52c51b0) != 0x20
        || read::<u32>(base, MODE) & 0x7fff0000 != 0x04040000
    {
        return false;
    }
    let context: usize = read(object, 0x50);
    if !aligned(context) || read::<usize>(context, 0) != object
        || read::<i32>(context, 0x200) != 2
        || read::<u32>(context, 0x204) == 0
        || read::<u8>(context, 0x22a) != 0
        || read::<u8>(context, 0x22d) != 0
    {
        return false;
    }
    let depth: i32 = read(context, 0x1fc);
    let resources: usize = read(context, 8);
    if !(0..19).contains(&depth)
        || read::<i32>(context, 0x1b0 + depth as usize * 4) != 2
        || !aligned(resources) || !aligned(read(resources, 0x80))
    {
        return false;
    }
    let cleanup: unsafe extern "C" fn(usize) = transmute(base + 0x13e6cb0);
    cleanup(context);
    read::<i32>(context, 0x1fc) == 0 && read::<i32>(context, 0x1b0) == 22
}

unsafe fn catalog_data(base: usize) -> Option<usize> {
    let outer: usize = read(base, 0x5314510);
    if !aligned(outer) { return None; }
    let data: usize = read(outer, 0);
    if !aligned(data) { return None; }
    let owner: usize = read(data, 0x88);
    if !aligned(owner) { return None; }
    let catalog: usize = read(owner, 0);
    aligned(catalog).then_some(catalog)
}

pub(super) unsafe fn catalog_clean(base: usize) -> bool {
    catalog_data(base).is_some_and(|catalog| read::<u8>(catalog, 0xfc) == 0)
}

pub(super) unsafe fn save_mode(base: usize) -> Option<u16> {
    let outer: usize = read(base, 0x5314510);
    if !aligned(outer) { return None; }
    let data: usize = read(outer, 0);
    aligned(data).then(|| u16::from(read::<u8>(data, 0))
        | (u16::from(read::<u8>(data, 1)) << 8))
}

pub(super) struct SaveProgress {
    current: i32,
    desired: i32,
    dirty: u8,
    flags: u16,
    busy: u64,
}

impl SaveProgress {
    pub(super) fn idle(&self) -> bool {
        self.current == 0 && self.desired == 0 && self.dirty == 0
            && self.flags == 0 && self.busy == 0
    }
    pub(super) fn values(&self) -> [u64; 4] {
        [self.current as u64, self.desired as u64,
            u64::from(self.dirty) | (u64::from(self.flags) << 8), self.busy]
    }
}

/// Native scheduling clears dirty before the file jobs finish. The current
/// and requested writer states plus its atomic busy gate distinguish the two.
pub(super) unsafe fn save_progress(base: usize) -> Option<SaveProgress> {
    let catalog = catalog_data(base)?;
    let writer: usize = read(catalog, 0x60);
    let (current, desired) = if writer == 0 {
        // Native state 12 treats an absent writer as not busy.
        (0, 0)
    } else {
        if !aligned(writer) { return None; }
        let level: i32 = read(writer, 0x444);
        if !(0..19).contains(&level) { return None; }
        (read(writer, 0x448), read(writer, 0x3f8 + level as usize * 4))
    };
    let outer: usize = read(base, 0x593b218);
    if !aligned(outer) { return None; }
    let service: usize = read(outer, 0);
    if !aligned(service) { return None; }
    // Mirrors the native LDAR X8 at 180cc8c (64-bit, acquire).
    let busy = crate::native_fields::load_acquire_u64(service + 0x50);
    Some(SaveProgress {
        current,
        desired,
        dirty: read(catalog, 0xfc),
        flags: u16::from(read::<u8>(base, DRAFT + 0x1bc))
            | (u16::from(read::<u8>(base, DRAFT + 0x1bd)) << 8),
        busy,
    })
}

pub(super) unsafe fn preview_state(base: usize) -> Option<(u32, u8)> {
    let outer: usize = read(base, 0x532f8b0);
    if !aligned(outer) { return None; }
    let service: usize = read(outer, 8);
    aligned(service).then(|| (read(service, 0x138), read(service, 0x2b8)))
}

/// Our direct state3 -> editor route skips SelectData's first visible frame.
/// Reuse its two guarded presentation requests (180fea4..180fee4), after the
/// editor's own resources have reached its main state. Native services consume
/// their own queues; do not write opacity, draw flags or transition internals.
/// Returns the pre-request native flags for the private runtime trace only.
pub(super) unsafe fn reveal_editor(base: usize) -> Option<[u64; 4]> {
    if read::<u32>(base, MODE) != 0x04050000
        || read::<u8>(base, DRAFT + 0x1c4) != 0 { return None; }
    let menu_outer: usize = read(base, 0x532f7f0);
    let transition_outer: usize = read(base, 0x532f7c8);
    if !aligned(menu_outer) || !aligned(transition_outer) { return None; }
    let menu: usize = read(menu_outer, 8);
    let transition: usize = read(transition_outer, 8);
    if !aligned(menu) || !aligned(transition) { return None; }
    let menu_child: usize = read(menu, 8);
    if !aligned(menu_child) || !aligned(read(menu_child, 0x80)) { return None; }
    let shown: u8 = read(menu, 0x35);
    let covered: u8 = read(transition, 0x69);
    let flags = [u64::from(shown), u64::from(covered),
        u64::from(read::<u8>(transition, 0x6a)), u64::from(read::<u8>(transition, 0x6b))];
    if shown == 0 {
        let show: unsafe extern "C" fn(usize, u32) = transmute(base + 0x32eafc0);
        show(menu, 1);
    }
    if covered != 0 {
        let reveal: unsafe extern "C" fn(usize) = transmute(base + 0x32e5a30);
        reveal(transition);
    }
    Some(flags)
}

/// Value-only preview request skipped with SelectData/SelectMii/SelectType.
/// Mirrors their first-fighter path (18152a8..18153d4), using the native
/// configuration initializer and synchronously copied owned row. No fighter,
/// service or configuration pointer survives this scene-thread call.
pub(super) unsafe fn request_editor_preview(base: usize) -> Option<[u64; 4]> {
    if read::<u32>(base, MODE) != 0x04050000
        || read::<u8>(base, DRAFT + 0x1c4) != 0 { return None; }
    let outer: usize = read(base, 0x532f8b0);
    let preview_outer: usize = read(base, 0x532f7a0);
    let fighter_outer: usize = read(base, 0x52b94f8);
    if !aligned(outer) || !aligned(preview_outer) || !aligned(fighter_outer)
        || !aligned(read(preview_outer, 0)) { return None; }
    let service: usize = read(outer, 8);
    let fighters: usize = read(fighter_outer, 0);
    if !aligned(service) || !aligned(fighters)
        || read::<u32>(service, 0x138) != 3
        || read::<u8>(service, 0x2b8) != 2
        || read::<u8>(service, 0x328) == 0 { return None; }
    let level: i32 = read(service, 0x134);
    if !(0..17).contains(&level) { return None; }
    let row = snapshot_draft(base);
    if super::value::parse(&row).is_err() { return None; }
    let existing: usize = read(fighters, 0x20);
    if existing != 0 {
        if !aligned(existing) { return None; }
        // Same already-created preview-group branch as native SelectType.
        let update: unsafe extern "C" fn(usize, i32, u64, *const u8, u32, u32, u32) =
            transmute(base + 0x3305ee0);
        update(service, 0, u64::from_le_bytes(row[0x1a0..0x1a8].try_into().unwrap()),
            row[0x110..].as_ptr(), 1, 0, 0);
        return Some([1, level as u64, read::<i32>(service, 0x134) as u64,
            read::<u32>(service, 0x138) as u64]);
    }
    #[repr(C, align(8))]
    struct PreviewConfiguration([u8; 0xb8]);
    let mut config = PreviewConfiguration([0; 0xb8]);
    let initialize: unsafe extern "C" fn(*mut u8) = transmute(base + 0x3302110);
    initialize(config.0.as_mut_ptr());
    // Native Mii preview overrides the generic defaults identically on all
    // three selection screens. Controller -1 uses native fallback 8.
    config.0[0..8].copy_from_slice(&row[0x1a0..0x1a8]);
    config.0[8..16].copy_from_slice(&0x10000000000u64.to_le_bytes());
    let controller: i32 = read(base, 0x5307b68);
    config.0[0x10..0x14].copy_from_slice(&if controller == -1 { 8i32 } else { controller }.to_le_bytes());
    config.0[0x43] = 1;
    config.0[0x46] = 0;
    config.0[0x48..0xa0].copy_from_slice(&row[0x148..0x1a0]);
    config.0[0xa0..0xb0].copy_from_slice(&row[0x130..0x140]);
    config.0[0xb0..0xb6].copy_from_slice(&row[0x140..0x146]);
    let request: unsafe extern "C" fn(usize, i32, *const u8) = transmute(base + 0x3305a30);
    request(service, 0, config.0.as_ptr());
    Some([0, level as u64, read::<i32>(service, 0x134) as u64,
        read::<u32>(service, 0x138) as u64])
}

/// Native by-value converters, audited in editor-contract01. A new draft cannot
/// inherit a native saved-row ID; explicit Save uses native new-slot allocation.
pub(super) unsafe fn seed_draft(base: usize, row: &super::value::Row) {
    let copy: unsafe extern "C" fn(usize, *const u8) = transmute(base + 0x17bd3e0);
    copy(base + DRAFT, row.as_ptr());
}

pub(super) unsafe fn snapshot_draft(base: usize) -> super::value::Row {
    let mut row = [0; training_mod_consts::mii::saved::HEADER_LEN];
    let copy: unsafe extern "C" fn(*mut u8, usize) = transmute(base + 0x17bd170);
    copy(row.as_mut_ptr(), base + DRAFT);
    row
}

/// Current CharaMake callback's owner; never retain this value across callbacks.
pub(super) struct Editor {
    pub sequence: usize,
    pub context: usize,
    pub state: i32,
    pub desired: i32,
    slot: usize,
}

impl Editor {
    pub unsafe fn bind(base: usize, context: usize, owner: usize) -> Option<Self> {
        if !aligned(context) { return None; }
        let sequence: usize = read(context, 0);
        let scene = sequence.checked_sub(0x50)?;
        if !aligned(scene) || read::<usize>(scene, 0) != base + 0x50b6bd8
            || read::<usize>(scene, 0xe8) != context
            || read::<usize>(sequence, 0x18) != owner
            || read::<u8>(context, 0x124) != 0 || read::<u8>(context, 0x125) != 0
        { return None; }
        let level: i32 = read(context, 0x10c);
        if !(0..19).contains(&level) { return None; }
        let slot = context + 0xc0 + level as usize * 4;
        Some(Self { sequence, context, state: read(context, 0x110), desired: read(slot, 0), slot })
    }

    pub unsafe fn enter_edit(&self, base: usize, row: &super::value::Row) -> bool {
        if self.state != 3 || self.desired != 4 || !drained(self.sequence) { return false; }
        seed_draft(base, row);
        write(self.slot, 0, 7i32);
        true
    }

    pub unsafe fn leave_edit(&self) -> bool {
        if self.state != 7 || self.desired != 4 || !drained(self.sequence) { return false; }
        self.cancel_parent();
        true
    }

    pub unsafe fn cancel_parent(&self) {
        // Same result/request pair as native Back (22f9238). No SelectData or
        // saved-Mii operation is needed when the owned seed cannot be entered.
        write(self.context, 0x120, 2i32);
        write(self.slot, 0, 1i32);
    }
}

/// Diagnostics of input configuration, not a backup or restoration mechanism.
/// Native IngameCall must reconstruct its battle services from these input rows.
pub(super) unsafe fn practice_inputs(base: usize) -> [u64; 3] {
    [(0x5308770, 8 * 0x1c8), (0x53095b0, 8 * 0x240), (0x530d568, 0x267)]
        .map(|(offset, len)| training_mod_consts::mii::inventory::fnv1a64(
            std::slice::from_raw_parts((base + offset) as *const u8, len)))
}

// Private qualification diagnostics only. These bytes are never restored or
// interpreted as native objects. Capture at four transition milestones, not per
// frame, to distinguish editor writes from online configuration updates.
pub(super) struct InputSnapshot([Vec<u8>; 3]);

#[derive(Clone, serde::Serialize)]
pub(super) struct InputChange {
    pub region: usize,
    pub expected_hash: u64,
    pub observed_hash: u64,
    pub changed_bytes: usize,
    pub first_changes: Vec<(usize, u8, u8)>,
}

pub(super) unsafe fn input_snapshot(base: usize) -> InputSnapshot {
    InputSnapshot([(0x5308770, 8 * 0x1c8), (0x53095b0, 8 * 0x240), (0x530d568, 0x267)]
        .map(|(offset, len)| std::slice::from_raw_parts((base + offset) as *const u8, len).to_vec()))
}

impl InputSnapshot {
    pub unsafe fn compare(&self, base: usize) -> [InputChange; 3] {
        let observed = input_snapshot(base);
        std::array::from_fn(|region| {
            let expected = &self.0[region];
            let actual = &observed.0[region];
            let differences = expected.iter().zip(actual).enumerate()
                .filter(|(_, (a, b))| a != b);
            InputChange {
                region,
                expected_hash: training_mod_consts::mii::inventory::fnv1a64(expected),
                observed_hash: training_mod_consts::mii::inventory::fnv1a64(actual),
                changed_bytes: differences.clone().count(),
                first_changes: differences.take(64).map(|(i, (&a, &b))| (i, a, b)).collect(),
            }
        })
    }
}
