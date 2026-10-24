//! Default-off read-only, three-shot pool/owner observer at the native READY service seam.
//! No hooks here, no native write/reload/token, and no pool reads from H1. Native pointers are
//! obtained anew within capture(); only owned scalars/hashes leave that call. The two lock calls
//! mutate the native mutex as required, but no pool/entry/shared-pointer/save field is written.
use super::*;

#[path = "mii_pool_observer_policy.rs"]
mod policy;
use policy::{Sequence, CHUNKS, CHUNK_ROWS, RECORDS, STRIDE};

const ARM_FILE: &str = "sd:/ultimate/TrainingModpack/mii_pool_observer.arm";
const POOL_HOLDER: usize = 0x593b6a0;
const OWNER_TABLE: usize = 0x5324680;
const LOCK: usize = 0x39c19c0;
const UNLOCK: usize = 0x39c19d0;
const CHARINFO_LEN: usize = 0x58;
const MAX_VISITS: u16 = 600;

// Owned main-13.0.5.elf SHA256 0789f0a4ae9652f335eca5eabc7df3e58b0954012a0bebdea374f317706536c2.
// Revalidate native layout users and the exact import thunks before enabling. The existing
// roster independently fingerprints its service/lifetime hook sites.
const CODE: [(usize, usize, u64); 7] = [
    (LOCK, 0x10, 0x159d9a9fb7f5b78b),
    (UNLOCK, 0x10, 0x805b05452dae4eff),
    (0x14ebbac, 0xb8, 0x57344b1f5a39e282), // native G lock/copy/unlock
    (0x67e898, 0x28, 0x822d66dfd21f69b7), // O table, u16 O+68
    (0x18130a4, 0x10, 0xd1fbfed5b1fbffd8), // P u16
    (0x1813180, 0x30, 0xa09c0097d46390cb), // P+C u32 writer
    (0x65011c, 0x40, 0x260512d9e392da15), // FE+46, share byte
];

static ARMED: AtomicBool = AtomicBool::new(false);
static HELD: AtomicBool = AtomicBool::new(false);
static EPOCH: AtomicUsize = AtomicUsize::new(0);
static DESTRUCTIONS: AtomicUsize = AtomicUsize::new(0);
static SEQUENCE: Mutex<Sequence> = Mutex::new(Sequence::new());

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100000001b3))
}

fn is_aligned(p: usize) -> bool { p != 0 && p.is_multiple_of(8) }

unsafe fn rd<T: Copy>(p: usize, offset: usize) -> T {
    std::ptr::read_volatile((p + offset) as *const T)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Globals { p: u16, p_plus_c: u32, share: u8 }

unsafe fn globals(base: usize) -> Globals {
    Globals { p: rd(base, 0x5307d28), p_plus_c: rd(base, 0x5307d2c), share: rd(base, 0x52c5290) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Owner {
    fe: usize,
    kind: u32,
    fe_index: u16,
    object: usize,
    object_index: u16,
    ready: bool,
    bound: bool,
    body: usize,
    body_id: u32,
    tokens: [u16; 5],
    specials: [u8; 4],
}

impl Owner {
    const EMPTY: Self = Self { fe: 0, kind: u32::MAX, fe_index: u16::MAX, object: 0,
        object_index: u16::MAX, ready: false, bound: false, body: 0, body_id: 0,
        tokens: [0; 5], specials: [0; 4] };
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Owners { manager: usize, table: usize, rows: [Owner; 8] }

/// Reacquire every FE and O for this call; never dereference a previous observation's identity.
/// Native fighter manager lookup is the existing roster/bind_saved_entry lifetime seam.
unsafe fn owners() -> Result<Owners, &'static str> {
    let address = training_mod_sync::read(&crate::common::FIGHTER_MANAGER_ADDR)
        as *mut *mut smash::app::FighterManager;
    if address.is_null() || (*address).is_null() { return Err("fighter manager unavailable"); }
    let manager = *address;
    let base = BASE.load(Ordering::Acquire);
    let table: usize = rd(base, OWNER_TABLE);
    if !is_aligned(table) { return Err("owner table null/misaligned"); }
    let mut rows = [Owner::EMPTY; 8];
    for (id, row) in rows.iter_mut().enumerate() {
        let fe = smash::app::lua_bind::FighterManager::get_fighter_entry(
            manager, smash::app::FighterEntryID(id as i32)) as usize;
        let object: usize = rd(table, 0xe8 + id * 8);
        if (fe != 0 && !is_aligned(fe)) || (object != 0 && !is_aligned(object)) {
            return Err("FE/O null distinction or alignment invalid");
        }
        row.object = object;
        if object != 0 { row.object_index = rd(object, 0x68); }
        if fe == 0 { continue; }
        if rd::<u32>(fe, 0xe0) != id as u32 { return Err("FE owner mismatch"); }
        row.fe = fe;
        row.kind = rd(fe, 0x18);
        row.fe_index = rd(fe, 0x46);
        if !(0x48..=0x4a).contains(&row.kind) { continue; }
        row.ready = ready_single_mii(fe as *const u8, id as u32) == Some(row.kind);
        if let Some(b) = crate::mii_identity_binding::bind_saved_entry(fe as *const u8) {
            row.bound = b.id == id as u32 && b.kind == row.kind;
            row.tokens = [rd(b.e, 0x1a8), rd(b.e, 0x1aa), rd(b.e, 0x1ac), rd(b.e, 0x1ae), rd(b.e, 0x1b0)];
            row.specials = [rd(b.e, 0x1b2), rd(b.e, 0x1b3), rd(b.e, 0x1b4), rd(b.e, 0x1b5)];
        }
        if row.ready {
            row.body = rd(fe, 0x4160);
            row.body_id = rd(row.body, 8);
        }
    }
    Ok(Owners { manager: manager as usize, table, rows })
}

fn fixture_ready(map: &Owners) -> bool {
    // Native Training roles, not waiting-room owner 4. The saved identities themselves are
    // independently bound by the two saved-file CharInfo hashes and native CSS selection.
    [0usize, 1].iter().all(|&id| {
        let r = map.rows[id];
        r.kind == 0x49 && r.ready && r.bound && r.object != 0
            && r.fe_index == r.object_index && usize::from(r.fe_index) < RECORDS
    })
}

struct NativePoolLock { g: usize, unlock: unsafe extern "C" fn(usize) }
impl Drop for NativePoolLock {
    fn drop(&mut self) { unsafe { (self.unlock)(self.g); } }
}

#[derive(Clone, Copy)]
struct Record { hash: u64, data: bool, control: bool }
struct Snapshot {
    records: [Record; RECORDS], owners: Owners, globals: Globals,
    g: usize, begin: usize, capacity: usize, epoch: usize, destruction: usize,
}

unsafe fn manager_identity(base: usize) -> Result<(usize, usize), &'static str> {
    let holder: usize = rd(base, POOL_HOLDER);
    if !is_aligned(holder) { return Err("pool holder null/misaligned"); }
    let g: usize = rd(holder, 0);
    if !is_aligned(g) { return Err("pool manager null/misaligned"); }
    Ok((holder, g))
}

/// One contiguous 13,832-byte owned copy, with no heap allocation, formatting, logging, manager
/// callbacks or hashing while the native pool lock is held. Raw CharInfo never leaves this call.
unsafe fn capture(entry: *const u8, epoch: usize) -> Result<Option<Snapshot>, &'static str> {
    let destruction = DESTRUCTIONS.load(Ordering::Acquire);
    let before = owners()?;
    if before.rows[0].fe != entry as usize || !fixture_ready(&before) { return Ok(None); }
    let base = BASE.load(Ordering::Acquire);
    let global_before = globals(base);
    let identity = manager_identity(base)?;
    let mut raw = [[0u8; STRIDE]; RECORDS];
    let begin;
    let capacity;
    {
        let lock: unsafe extern "C" fn(usize) = std::mem::transmute(base + LOCK);
        let unlock: unsafe extern "C" fn(usize) = std::mem::transmute(base + UNLOCK);
        lock(identity.1);
        let _guard = NativePoolLock { g: identity.1, unlock };
        if manager_identity(base)? != identity { return Err("pool rebound during lock acquisition"); }
        begin = rd::<usize>(identity.1, 0x24d0);
        let end: usize = rd(identity.1, 0x24d8);
        let cap: usize = rd(identity.1, 0x24e0);
        capacity = policy::vector_capacity(begin, end, cap).ok_or("pool count/stride/capacity differs from 133")?;
        if globals(base) != global_before { return Err("P/C/share changed before locked copy"); }
        // u8 read_volatile avoids creating a Rust slice/reference to concurrently native-owned data.
        for (index, record) in raw.iter_mut().enumerate() {
            for (offset, byte) in record.iter_mut().enumerate() {
                *byte = rd::<u8>(begin + index * STRIDE, offset);
            }
        }
    } // unlock before all hashing, live rebinding and output
    let after = owners()?;
    if before != after || !fixture_ready(&after) || manager_identity(base)? != identity
        || globals(base) != global_before || EPOCH.load(Ordering::Acquire) != epoch
        || DESTRUCTIONS.load(Ordering::Acquire) != destruction
    { return Err("identity/epoch/global changed across snapshot"); }
    let records = std::array::from_fn(|i| Record {
        hash: fnv(&raw[i][..CHARINFO_LEN]),
        data: raw[i][0x58..0x60].iter().any(|b| *b != 0),
        control: raw[i][0x60..0x68].iter().any(|b| *b != 0),
    });
    Ok(Some(Snapshot { records, owners: after, globals: global_before, g: identity.1,
        begin, capacity, epoch, destruction }))
}

fn emit(id: u8, snap: Snapshot) {
    // A stream is accepted only with begin + chunks 0..16 + owners 0..7 + end, matching IDs and
    // aggregate digest. diagnostic() can drop the SD copy on try_lock; missing lines are NOT PASS.
    let mut digest_bytes = [0u8; RECORDS * 10];
    for (i, r) in snap.records.iter().enumerate() {
        digest_bytes[i * 10..i * 10 + 8].copy_from_slice(&r.hash.to_le_bytes());
        digest_bytes[i * 10 + 8] = u8::from(r.data);
        digest_bytes[i * 10 + 9] = u8::from(r.control);
    }
    let digest = fnv(&digest_bytes);
    trace!("[MiiPool] begin id={id} epoch={} destruction={} records={RECORDS} chunks={CHUNKS} owners=8 lines=27 g={:#x} begin={:#x} capacity={} p={} p_plus_c={} share={} digest={digest:016x}",
        snap.epoch, snap.destruction, snap.g, snap.begin, snap.capacity,
        snap.globals.p, snap.globals.p_plus_c, snap.globals.share);
    for (chunk, records) in snap.records.chunks(CHUNK_ROWS).enumerate() {
        use std::fmt::Write;
        let mut line = String::with_capacity(256);
        for (offset, r) in records.iter().enumerate() {
            // Never raw CharInfo/name bytes or native shared_ptr targets.
            let _ = write!(line, " {}:{:016x}:{}:{}", chunk * CHUNK_ROWS + offset,
                r.hash, u8::from(r.data), u8::from(r.control));
        }
        trace!("[MiiPool] chunk id={id} chunk={chunk} count={} rows={line}", records.len());
    }
    for (owner, r) in snap.owners.rows.iter().enumerate() {
        trace!("[MiiPool] owner id={id} owner={owner} fe={:#x} kind={:#x} fe_index={} o={:#x} o_index={} ready={} bound={} body={:#x} body_id={:#x} tokens={:?} specials={:?}",
            r.fe, r.kind, r.fe_index, r.object, r.object_index, r.ready, r.bound,
            r.body, r.body_id, r.tokens, r.specials);
    }
    trace!("[MiiPool] end id={id} epoch={} records={RECORDS} chunks={CHUNKS} owners=8 lines=27 digest={digest:016x} complete=true",
        snap.epoch);
}

unsafe fn permitted() -> bool {
    crate::common::is_native_training_mode() && crate::common::is_ready_go()
        && crate::character_lifecycle::native_hooks_ready() && BASE.load(Ordering::Acquire) != 0
        && !menu::captures_input() && !INSTALL.active() && !training::save_states::is_loading()
        && !training::recording_or_playback() && !crate::replay_preview::occupied()
        && [Role::Player, Role::Cpu].iter().all(|&role| {
            let state = state_for(role).lock().unwrap();
            state.active.is_none() && state.pending.is_none()
        })
}

/// Existing player-0 L3+X input seam; no game pointer retained and no native request issued.
pub unsafe fn handle_input(out: &mut MappedInputs, click: bool, x: bool) {
    let held = click && x;
    let was_held = HELD.swap(held, Ordering::Relaxed);
    if !ARMED.load(Ordering::Acquire) || !held || !permitted() { return; }
    *out = MappedInputs::empty();
    if was_held { return; }
    let epoch = EPOCH.load(Ordering::Acquire);
    match SEQUENCE.lock().unwrap().request(epoch) {
        Ok(id) => trace!("[MiiPool] request id={id} epoch={epoch} max_visits={MAX_VISITS}"),
        Err(reason) => trace!("[MiiPool] refused reason={reason}"),
    }
}

pub(super) unsafe fn service(entry: *const u8, owner: u32) {
    if owner != 0 || !ARMED.load(Ordering::Acquire) { return; }
    let (id, epoch) = {
        let mut s = SEQUENCE.lock().unwrap();
        if !s.pending || s.ended { return; }
        s.visits += 1;
        if s.visits > MAX_VISITS {
            s.cancel_pending();
            trace!("[MiiPool] refused reason=READY_timeout visits={MAX_VISITS}");
            return;
        }
        (s.completed + 1, s.request_epoch)
    };
    if !permitted() { return; }
    match capture(entry, epoch) {
        Ok(None) => {}, // bounded READY wait, never scans the pool while either role is unready
        Ok(Some(snapshot)) => {
            let accepted = SEQUENCE.lock().unwrap().finish(EPOCH.load(Ordering::Acquire), true);
            if accepted { emit(id, snapshot); }
            else { trace!("[MiiPool] refused id={id} reason=sequence_changed"); }
        },
        Err(reason) => {
            SEQUENCE.lock().unwrap().finish(epoch, false);
            trace!("[MiiPool] refused id={id} reason={reason}");
        },
    }
}

pub(super) fn scene_changed() {
    EPOCH.fetch_add(1, Ordering::AcqRel);
    if SEQUENCE.lock().unwrap().cancel_pending() {
        trace!("[MiiPool] refused reason=scene_departure_while_pending");
    }
}

pub(super) fn on_destroy(owner: u32) {
    DESTRUCTIONS.fetch_add(1, Ordering::AcqRel);
    // Native Training -> CSS can keep the mode tuple unchanged. Its actual P1
    // entry destruction ends this observer epoch even with roster cycling off.
    if owner == 0 {
        EPOCH.fetch_add(1, Ordering::AcqRel);
    }
    if SEQUENCE.lock().unwrap().cancel_pending() {
        trace!("[MiiPool] refused reason=entry_destruction_while_pending");
    }
}

pub fn init() {
    let arm = std::path::Path::new(ARM_FILE).exists();
    if !arm { trace!("[MiiPool] armed=false max_snapshots=3"); return; }
    unsafe {
        let base = getRegionAddress(Region::Text) as usize;
        for (offset, len, expected) in CODE {
            if fnv(std::slice::from_raw_parts((base + offset) as *const u8, len)) != expected {
                trace!("[MiiPool] armed=false fingerprint_mismatch={offset:#x}");
                return;
            }
        }
    }
    ARMED.store(true, Ordering::Release);
    trace!("[MiiPool] armed=true max_snapshots=3 records=133 control=L3+X readonly=true");
}
